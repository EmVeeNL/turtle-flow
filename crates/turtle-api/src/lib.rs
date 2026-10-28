//! Loopback dashboard API. The daemon passes its existing services; this crate never opens SQLite.
mod data;

use axum::{
    Json, Router,
    extract::{Path as AxumPath, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use time::{Duration as TimeDuration, OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::{net::TcpListener, sync::watch};
use turtle_artifacts::BlobStore;
use turtle_domain::{Actor, ApprovalId, JobKey, JobType, Priority, Sha256 as SubjectHash};
use turtle_scheduler::{
    AddProject, ApprovalError, ApprovalService, Engine, ProjectError, ProjectService, RepoGitFacts,
    SharedStore, StoreContext, lift_budget_hold,
};
use turtle_store::{BudgetScope, Decision, NewJob, StoreError};
use turtle_workflow::{JobCommand, PauseMode, Profile, ProfileBase};

type Context = StoreContext<RepoGitFacts>;

#[derive(Debug, Clone)]
pub struct Services {
    pub store: SharedStore,
    pub engine: Engine<Context>,
    pub approvals: ApprovalService<Context, RepoGitFacts>,
    pub projects: ProjectService,
    pub blobs: Arc<BlobStore>,
}
#[derive(Debug)]
struct Session {
    digest: [u8; 32],
    expires: Instant,
    read_only: bool,
}
#[derive(Debug, Clone)]
struct AppState {
    services: Services,
    sessions: Arc<Mutex<Vec<Session>>>,
    command_lock: Arc<tokio::sync::Mutex<()>>,
    port: u16,
    tailnet_host: Option<String>,
    dashboard_dir: Option<PathBuf>,
    stop: Option<watch::Receiver<bool>>,
}
#[derive(Debug)]
pub struct ApiServer {
    listener: TcpListener,
    state: AppState,
    port_file: PathBuf,
}
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}
impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
    fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "record not found")
    }
    fn unavailable() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            "daemon cannot answer",
        )
    }
    fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", message)
    }
    fn bad(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", message)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"error":{"code":self.code,"message":self.message}})),
        )
            .into_response()
    }
}
fn store_error(e: StoreError) -> ApiError {
    match e {
        StoreError::NotFound(_) => ApiError::not_found(),
        StoreError::HashMismatch
        | StoreError::NotPending { .. }
        | StoreError::VersionConflict { .. }
        | StoreError::Forbidden(_) => ApiError::conflict(e.to_string()),
        StoreError::CommentRequired => ApiError::bad(e.to_string()),
        _ => ApiError::unavailable(),
    }
}
fn digest_hex(input: &str) -> String {
    Sha256::digest(input.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn random_hex(bytes: usize) -> Result<String, getrandom::Error> {
    let mut raw = vec![0u8; bytes];
    getrandom::fill(&mut raw)?;
    Ok(raw.iter().map(|b| format!("{b:02x}")).collect())
}
/// Called only by the interactive CLI. The database receives a hash, never the URL code.
pub async fn issue_login_code(
    store: &SharedStore,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    issue_login_code_with_scope(store, "full").await
}
pub async fn issue_read_only_login_code(
    store: &SharedStore,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    issue_login_code_with_scope(store, "read_only").await
}
async fn issue_login_code_with_scope(
    store: &SharedStore,
    scope: &'static str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let code = random_hex(16).map_err(|e| std::io::Error::other(e.to_string()))?;
    let hash = digest_hex(&code);
    let expires = (OffsetDateTime::now_utc() + TimeDuration::seconds(60)).format(&Rfc3339)?;
    store
        .with(move |s| s.create_dashboard_code(&hash, &expires, scope))
        .await?;
    Ok(code)
}
pub fn validate_tailnet_host(host: &str) -> Result<String, String> {
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.len() > 253
        || !host.ends_with(".ts.net")
        || host.bytes().any(|b| b.is_ascii_whitespace() || b == b':')
    {
        return Err(
            "tailnet host must be a lowercase DNS name ending in .ts.net, without a port".into(),
        );
    }
    if host != host.to_ascii_lowercase() {
        return Err("tailnet host must use lowercase DNS labels".into());
    }
    let labels: Vec<_> = host.split('.').collect();
    if labels.len() != 4
        || labels.iter().any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
    {
        return Err("tailnet host contains an invalid DNS label".into());
    }
    Ok(host.to_owned())
}
pub fn loopback_address(ip: IpAddr, port: u16) -> Result<SocketAddr, ApiError> {
    if !ip.is_loopback() {
        return Err(ApiError::bad("API binds loopback only"));
    }
    Ok(SocketAddr::new(ip, port))
}
impl ApiServer {
    pub async fn bind(
        services: Services,
        run_dir: &Path,
        dashboard_dir: Option<PathBuf>,
        port: u16,
        tailnet_host: Option<String>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let addr = loopback_address(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
            .map_err(|e| std::io::Error::other(e.message))?;
        let tailnet_host = tailnet_host
            .map(|host| validate_tailnet_host(&host).map_err(std::io::Error::other))
            .transpose()?;
        let listener = TcpListener::bind(addr).await?;
        let port = listener.local_addr()?.port();
        std::fs::create_dir_all(run_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(run_dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let port_file = run_dir.join("api.port");
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&port_file)?;
            write!(file, "{port}")?;
            std::fs::set_permissions(&port_file, std::fs::Permissions::from_mode(0o600))?;
        }
        #[cfg(not(unix))]
        std::fs::write(&port_file, port.to_string())?;
        Ok(Self {
            listener,
            state: AppState {
                services,
                sessions: Arc::new(Mutex::new(Vec::new())),
                command_lock: Arc::new(tokio::sync::Mutex::new(())),
                port,
                tailnet_host,
                dashboard_dir,
                stop: None,
            },
            port_file,
        })
    }
    #[must_use]
    pub fn port(&self) -> u16 {
        self.state.port
    }
    pub async fn run(self, mut stop: watch::Receiver<bool>) -> Result<(), std::io::Error> {
        let mut state = self.state;
        state.stop = Some(stop.clone());
        let app = router(state);
        let result = axum::serve(self.listener, app)
            .with_graceful_shutdown(async move {
                while !*stop.borrow() && stop.changed().await.is_ok() {}
            })
            .await;
        let _ = std::fs::remove_file(self.port_file);
        result
    }
}
fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/session", post(exchange_session))
        .route("/api/v1/me", get(me))
        .route("/api/v1/overview", get(data::overview))
        .route("/api/v1/projects", post(create_project))
        .route("/api/v1/projects/{slug}", get(data::project))
        .route("/api/v1/projects/{slug}/resources", get(data::resources))
        .route("/api/v1/resources/search", get(data::resource_search))
        .route("/api/v1/projects/{slug}/jobs", post(create_job))
        .route("/api/v1/budgets", post(set_budget))
        .route("/api/v1/jobs/{key}", get(data::job))
        .route("/api/v1/approvals/{id}", get(data::approval))
        .route("/api/v1/approvals/{id}/decisions", post(decide_approval))
        .route("/api/v1/jobs/{key}/commands", post(job_command))
        .route("/api/v1/runs/{id}", get(data::run))
        .route("/api/v1/runs/{id}/transcript", get(data::transcript))
        .route("/api/v1/artifacts/{id}", get(data::artifact))
        .route("/api/v1/analytics/usage", get(data::usage))
        .route("/api/v1/events/stream", get(data::events_stream))
        .route("/login", get(login_page))
        .route("/login.js", get(login_script))
        .method_not_allowed_fallback(|| async {
            ApiError::new(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "method not allowed",
            )
        })
        .fallback(static_file)
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}
async fn health(State(state): State<AppState>) -> Json<Value> {
    Json(json!({"status":"ok","tailnet_host":state.tailnet_host}))
}
async fn guard(
    State(state): State<AppState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let sensitive =
        request.uri().path().starts_with("/api/v1/") || request.uri().path() == "/login";
    let mut refresh_cookie = None;
    let headers = request.headers();
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let allowed = [
        format!("127.0.0.1:{}", state.port),
        format!("localhost:{}", state.port),
        format!("[::1]:{}", state.port),
    ];
    let tailnet = state.tailnet_host.as_deref() == Some(host.as_str());
    let mut response = if !allowed.iter().any(|a| a == &host) && !tailnet {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_host",
            "Host is not loopback API origin",
        )
        .into_response()
    } else if !matches!(*request.method(), Method::GET | Method::HEAD)
        && (headers.get(header::ORIGIN).and_then(|v| v.to_str().ok())
            != Some(format!("{}://{host}", if tailnet { "https" } else { "http" }).as_str())
            || headers
                .get("X-Turtle-Request")
                .and_then(|v| v.to_str().ok())
                != Some("1"))
    {
        ApiError::new(
            StatusCode::FORBIDDEN,
            "invalid_origin",
            "Origin and X-Turtle-Request are required",
        )
        .into_response()
    } else {
        let path = request.uri().path();
        let session = authorized(&state, headers);
        // Redeeming a one-time code is how a session is replaced, so a read-only session must
        // still be able to do it (a phone scanning a fresh QR code); it needs a valid code
        // either way, and a code can only come from the interactive CLI.
        if path != "/api/v1/session"
            && session.as_ref().is_some_and(|(_, read_only)| {
                *read_only && !matches!(*request.method(), Method::GET | Method::HEAD)
            })
        {
            ApiError::new(
                StatusCode::FORBIDDEN,
                "read_only_session",
                "this dashboard session is read-only",
            )
            .into_response()
        } else {
            let public = matches!(
                path,
                "/login" | "/login.js" | "/api/v1/session" | "/api/v1/health"
            );
            if public {
                next.run(request).await
            } else if let Some((token, _)) = session {
                refresh_cookie = Some(token);
                next.run(request).await
            } else {
                ApiError::new(
                    StatusCode::UNAUTHORIZED,
                    "unauthorized",
                    "dashboard login required",
                )
                .into_response()
            }
        }
    };
    let h = response.headers_mut();
    h.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    if let Some(token) = refresh_cookie
        && let Ok(value) = HeaderValue::from_str(&format!(
            "turtle_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200{}",
            if tailnet { "; Secure" } else { "" }
        ))
    {
        h.insert(header::SET_COOKIE, value);
    }
    if sensitive {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    if !h.contains_key("content-security-policy") {
        h.insert("content-security-policy", HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'"));
    }
    response
}
fn authorized(state: &AppState, headers: &HeaderMap) -> Option<(String, bool)> {
    let raw = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = raw
        .split(';')
        .find_map(|p| p.trim().strip_prefix("turtle_session="));
    let token = token?;
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
    let mut sessions = state
        .sessions
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let now = Instant::now();
    sessions.retain(|s| s.expires > now);
    let mut matched = false;
    let mut read_only = false;
    for session in sessions.iter_mut() {
        if bool::from(session.digest.ct_eq(&digest)) {
            session.expires = now + Duration::from_secs(12 * 3600);
            matched = true;
            read_only = session.read_only;
        }
    }
    matched.then(|| (token.to_owned(), read_only))
}
#[derive(Deserialize)]
struct SessionRequest {
    code: String,
}
async fn exchange_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<SessionRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(input) = payload.map_err(|_| ApiError::bad("invalid JSON body"))?;
    if input.code.len() != 32 || !input.code.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_code",
            "invalid login code",
        ));
    }
    let hash = digest_hex(&input.code);
    let scope = state
        .services
        .store
        .with(move |s| s.redeem_dashboard_code(&hash))
        .await
        .map_err(store_error)?;
    let Some(scope) = scope else {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_code",
            "expired or used login code",
        ));
    };
    let token = random_hex(32).map_err(|_| ApiError::unavailable())?;
    let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
    let mut sessions = state
        .sessions
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    sessions.retain(|s| s.expires > Instant::now());
    sessions.push(Session {
        digest,
        expires: Instant::now() + Duration::from_secs(12 * 3600),
        read_only: scope == "read_only",
    });
    if sessions.len() > 128 {
        sessions.remove(0);
    }
    let mut response = Json(json!({"ok":true})).into_response();
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "turtle_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200{}",
            if state.tailnet_host.as_deref() == Some(host) {
                "; Secure"
            } else {
                ""
            }
        ))
        .map_err(|_| ApiError::unavailable())?,
    );
    Ok(response)
}
async fn me(State(state): State<AppState>, headers: HeaderMap) -> Json<Value> {
    let read_only = authorized(&state, &headers).is_some_and(|(_, read_only)| read_only);
    Json(json!({"readonly":read_only}))
}
#[derive(Deserialize)]
struct CreateProjectRequest {
    path: String,
    slug: Option<String>,
    name: Option<String>,
    key: Option<String>,
    profile: Option<String>,
    branch: Option<String>,
}
async fn create_project(
    State(state): State<AppState>,
    payload: Result<Json<CreateProjectRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(input) = payload.map_err(|_| ApiError::bad("invalid JSON body"))?;
    let project = state
        .services
        .projects
        .add(AddProject {
            path: PathBuf::from(input.path),
            slug: input.slug,
            name: input.name,
            key: input.key,
            profile: input.profile.unwrap_or_else(|| "standard".into()),
            branch: input.branch,
        })
        .await
        .map_err(|error| match error {
            ProjectError::Invalid(message) => ApiError::bad(message),
            ProjectError::Store(error) => store_error(error),
        })?;
    Ok(Json(
        json!({"ok":true,"id":project.0.to_string(),"slug":project.1}),
    ))
}
#[derive(Deserialize)]
struct CreateJobRequest {
    title: String,
    #[serde(default)]
    description: String,
    #[serde(rename = "type", default = "default_job_type")]
    job_type: String,
    #[serde(default = "default_priority")]
    priority: String,
    profile: Option<String>,
    budget: Option<u64>,
    start: Option<bool>,
}
fn default_job_type() -> String {
    "feature".into()
}
fn default_priority() -> String {
    "normal".into()
}
async fn create_job(
    State(state): State<AppState>,
    AxumPath(slug): AxumPath<String>,
    payload: Result<Json<CreateJobRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(input) = payload.map_err(|_| ApiError::bad("invalid JSON body"))?;
    if input.title.trim().is_empty() {
        return Err(ApiError::bad("title must not be empty"));
    }
    let job_type: JobType = serde_json::from_value(json!(input.job_type))
        .map_err(|_| ApiError::bad("type must be feature, bugfix, refactor, docs, or chore"))?;
    let priority: Priority = serde_json::from_value(json!(input.priority))
        .map_err(|_| ApiError::bad("priority must be low, normal, high, or urgent"))?;
    if input.budget == Some(0) {
        return Err(ApiError::bad("a budget must be more than zero tokens"));
    }
    let start = input.start.unwrap_or(false);
    let project = state
        .services
        .store
        .with(move |s| s.project_by_slug(&slug))
        .await
        .map_err(store_error)?;
    let profile_name = input
        .profile
        .unwrap_or_else(|| project.default_profile.clone());
    let base = match profile_name.as_str() {
        "fast" => ProfileBase::Fast,
        "standard" => ProfileBase::Standard,
        "strict" => ProfileBase::Strict,
        _ => return Err(ApiError::bad("profile must be fast, standard, or strict")),
    };
    let new = NewJob {
        project_id: project.id,
        title: input.title,
        description: input.description,
        job_type,
        priority,
        profile: Profile::builtin(base),
    };
    let record = state
        .services
        .store
        .with(move |s| {
            s.transaction(|tx| {
                let job = tx.create_job(&new, &Actor::local_user())?;
                if let Some(budget) = input.budget {
                    tx.set_budget(
                        BudgetScope::Job(job.job.id),
                        budget,
                        true,
                        Some(job.job.id),
                        &Actor::local_user(),
                    )?;
                }
                Ok(job)
            })
        })
        .await
        .map_err(store_error)?;
    let mut version = record.job.version;
    if start {
        let started = state
            .services
            .engine
            .submit(record.job.id, JobCommand::Start, Actor::local_user())
            .await
            .map_err(|e| ApiError::conflict(e.to_string()))?;
        version = started.job.version;
    }
    Ok(Json(
        json!({"ok":true,"id":record.job.id.to_string(),"key":record.key.to_string(),"version":version,"started":start}),
    ))
}
#[derive(Deserialize)]
struct SetBudgetRequest {
    scope: String,
    target: Option<String>,
    limit: u64,
    soft: Option<bool>,
}
async fn set_budget(
    State(state): State<AppState>,
    payload: Result<Json<SetBudgetRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(input) = payload.map_err(|_| ApiError::bad("invalid JSON body"))?;
    if input.limit == 0 {
        return Err(ApiError::bad("a budget must be more than zero tokens"));
    }
    let (scope, job_id, jobs) = match (input.scope.as_str(), input.target) {
        ("job", Some(key)) => {
            let key: JobKey = key.parse().map_err(|_| ApiError::not_found())?;
            let j = state
                .services
                .store
                .with(move |s| s.find_job(&key))
                .await
                .map_err(store_error)?;
            (BudgetScope::Job(j.job.id), Some(j.job.id), vec![j.job.id])
        }
        ("project", Some(slug)) => {
            let p = state
                .services
                .store
                .with(move |s| s.project_by_slug(&slug))
                .await
                .map_err(store_error)?;
            let id = p.id;
            let jobs = state
                .services
                .store
                .with(move |s| s.list_jobs(Some(id)))
                .await
                .map_err(store_error)?
                .into_iter()
                .map(|j| j.job.id)
                .collect();
            (BudgetScope::Project(p.id), None, jobs)
        }
        ("global", None) => {
            let jobs = state
                .services
                .store
                .with(|s| s.list_jobs(None))
                .await
                .map_err(store_error)?
                .into_iter()
                .map(|j| j.job.id)
                .collect();
            (BudgetScope::Global, None, jobs)
        }
        _ => {
            return Err(ApiError::bad(
                "choose job with a key, project with a slug, or global scope",
            ));
        }
    };
    let hard = !input.soft.unwrap_or(false);
    let budget = state
        .services
        .store
        .with(move |s| {
            s.transaction(|tx| {
                tx.set_budget(scope, input.limit, hard, job_id, &Actor::local_user())
            })
        })
        .await
        .map_err(store_error)?;
    for id in jobs {
        lift_budget_hold(&state.services.engine, id)
            .await
            .map_err(|_| ApiError::unavailable())?;
    }
    let budget_scope = match budget.scope {
        BudgetScope::Global => "global",
        BudgetScope::Project(_) => "project",
        BudgetScope::Job(_) => "job",
    };
    Ok(Json(
        json!({"ok":true,"budget":{"id":budget.id.to_string(),"scope":budget_scope,"settled":budget.settled_tokens,"reserved":budget.reserved_tokens,"limit":budget.limit_tokens,"kind":if budget.hard{"hard"}else{"soft"}}}),
    ))
}
#[derive(Deserialize)]
struct DecisionRequest {
    decision: String,
    subject_hash: String,
    comment: Option<String>,
    #[serde(default)]
    acknowledged_flags: Vec<String>,
}
async fn decide_approval(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    payload: Result<Json<DecisionRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(input) = payload.map_err(|_| ApiError::bad("invalid JSON body"))?;
    let id: ApprovalId = id.parse().map_err(|_| ApiError::not_found())?;
    let hash: SubjectHash = input
        .subject_hash
        .parse()
        .map_err(|_| ApiError::bad("invalid subject_hash"))?;
    let decision = match input.decision.as_str() {
        "approve" => Decision::Approve,
        "changes" | "request_changes" => Decision::RequestChanges,
        "reject" => Decision::Reject,
        _ => return Err(ApiError::bad("invalid decision")),
    };
    match state
        .services
        .approvals
        .decide(
            id,
            decision,
            hash,
            Actor::local_user(),
            "dashboard",
            input.comment,
            input.acknowledged_flags,
        )
        .await
    {
        Ok(job) => Ok(Json(
            json!({"ok":true,"job_key":job.key.to_string(),"version":job.job.version}),
        )),
        Err(ApprovalError::Store(e)) => Err(store_error(e)),
        Err(ApprovalError::Stale) => Err(ApiError::conflict("approval binding is stale")),
        Err(ApprovalError::FlagsNotAcknowledged(flags)) => Err(ApiError::bad(format!(
            "acknowledge flags: {}",
            flags.join(", ")
        ))),
        Err(_) => Err(ApiError::unavailable()),
    }
}
#[derive(Deserialize)]
struct CommandRequest {
    command: String,
    expected_version: u64,
    idempotency_key: String,
}
async fn job_command(
    State(state): State<AppState>,
    AxumPath(key): AxumPath<String>,
    payload: Result<Json<CommandRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(input) = payload.map_err(|_| ApiError::bad("invalid JSON body"))?;
    if input.idempotency_key.len() > 128 || input.idempotency_key.is_empty() {
        return Err(ApiError::bad("invalid idempotency_key"));
    }
    let key: JobKey = key.parse().map_err(|_| ApiError::not_found())?;
    let _guard = state.command_lock.lock().await;
    let idem = input.idempotency_key.clone();
    let record = state
        .services
        .store
        .with(move |s| s.find_job(&key))
        .await
        .map_err(store_error)?;
    let job_id = record.job.id;
    let job_id_text = job_id.to_string();
    let replay = state
        .services
        .store
        .with(move |s| s.command_replay(&idem))
        .await
        .map_err(store_error)?;
    if let Some((saved_job, command, version)) = replay {
        if saved_job == job_id_text && command == input.command {
            return Ok(Json(json!({"ok":true,"version":version,"replayed":true})));
        }
        return Err(ApiError::conflict(
            "idempotency key was used for another command",
        ));
    }
    if record.job.version != input.expected_version {
        return Err(ApiError::conflict("job version changed"));
    }
    let command = match input.command.as_str() {
        "start" => JobCommand::Start,
        "pause" => JobCommand::Pause {
            mode: PauseMode::Soft,
        },
        "resume" => JobCommand::Resume,
        "cancel" => JobCommand::Cancel,
        _ => return Err(ApiError::bad("invalid command")),
    };
    let (after, replayed) = state
        .services
        .engine
        .submit_api_command(
            job_id,
            command,
            Actor::local_user(),
            input.expected_version,
            input.idempotency_key,
            input.command,
        )
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?;
    Ok(Json(
        json!({"ok":true,"version":after.job.version,"replayed":replayed}),
    ))
}
async fn login_page() -> Html<&'static str> {
    Html(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Turtle Flow login</title></head><body><main><h1>Turtle Flow</h1><p id=\"status\">Signing in…</p></main><script src=\"/login.js\" defer></script></body></html>",
    )
}
async fn login_script() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        "const code=new URLSearchParams(location.hash.slice(1)).get('code');history.replaceState(null,'','/login');if(!code){document.getElementById('status').textContent='Open this page with turtle dashboard.'}else{fetch('/api/v1/session',{method:'POST',headers:{'Content-Type':'application/json','X-Turtle-Request':'1'},body:JSON.stringify({code}),credentials:'same-origin'}).then(r=>{if(!r.ok)throw Error('Code expired or already used');location.replace('/')}).catch(e=>document.getElementById('status').textContent=e.message)}",
    )
}
async fn static_file(State(state): State<AppState>, uri: axum::http::Uri) -> Response {
    let path = uri.path();
    if path.contains("..") || path.starts_with("/api/") {
        return ApiError::not_found().into_response();
    }
    let relative = if path == "/" {
        "index.html".to_owned()
    } else if path.ends_with('/') {
        format!("{}index.html", path.trim_start_matches('/'))
    } else {
        let p = path.trim_start_matches('/');
        if p.rsplit('/').next().is_some_and(|n| n.contains('.')) {
            p.to_owned()
        } else {
            format!("{p}/index.html")
        }
    };
    if let Some(dir) = &state.dashboard_dir {
        let file = dir.join(&relative);
        if let Ok(bytes) = tokio::fs::read(&file).await {
            return asset_response(&relative, bytes);
        }
    }
    #[cfg(feature = "embedded-dashboard")]
    if let Some(file) = Embedded::get(&relative) {
        return asset_response(&relative, file.data.to_vec());
    }
    if path == "/" {
        Html("<!doctype html><title>Dashboard not built</title><p>Dashboard not built. Run <code>npm run build</code> in apps/dashboard/.</p>").into_response()
    } else {
        ApiError::not_found().into_response()
    }
}
fn asset_response(path: &str, bytes: Vec<u8>) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let csp = if path.ends_with(".html") {
        std::str::from_utf8(&bytes)
            .ok()
            .and_then(|html| {
                html.split_once("<meta http-equiv=\"content-security-policy\" content=\"")
            })
            .and_then(|(_, rest)| rest.split_once('"'))
            .map(|(policy, _)| format!("{policy}; frame-ancestors 'none'"))
    } else {
        None
    };
    let mut response = ([(header::CONTENT_TYPE, mime.as_ref().to_owned())], bytes).into_response();
    if let Some(policy) = csp.and_then(|s| HeaderValue::from_str(&s).ok()) {
        response
            .headers_mut()
            .insert("content-security-policy", policy);
    }
    response
}
#[cfg(feature = "embedded-dashboard")]
#[derive(rust_embed::RustEmbed)]
#[folder = "../../apps/dashboard/dist"]
struct Embedded;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use futures_util::StreamExt;
    use tower::ServiceExt;
    use turtle_scheduler::{ContextConfig, CrashPlan, ExecConfig};
    use turtle_store::{Store, SystemClock};

    fn state() -> (AppState, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temporary home");
        let store = SharedStore::new(Store::open_in_memory(Arc::new(SystemClock)).expect("store"));
        let blobs = Arc::new(BlobStore::open(dir.path().join("blobs")).expect("blobs"));
        let cfg = Arc::new(ExecConfig::new(dir.path().join("worktrees")));
        let git = Arc::new(RepoGitFacts::new(store.clone(), cfg));
        let ctx = Arc::new(StoreContext::new(
            store.clone(),
            Arc::clone(&blobs),
            Arc::clone(&git),
            ContextConfig::default(),
        ));
        let engine = Engine::new(store.clone(), ctx, CrashPlan::never());
        let approvals = ApprovalService::new(engine.clone(), git);
        (
            AppState {
                services: Services {
                    store: store.clone(),
                    engine,
                    approvals,
                    projects: ProjectService::new(store.clone()),
                    blobs,
                },
                sessions: Arc::new(Mutex::new(Vec::new())),
                command_lock: Arc::new(tokio::sync::Mutex::new(())),
                port: 9876,
                tailnet_host: None,
                dashboard_dir: None,
                stop: None,
            },
            dir,
        )
    }
    fn req(method: Method, path: &str, host: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, host)
            .body(Body::empty())
            .expect("request")
    }
    fn session_request(
        state: &AppState,
        method: Method,
        path: &str,
        body: Value,
        read_only: bool,
    ) -> Request<Body> {
        let token = "a".repeat(64);
        state.sessions.lock().expect("session lock").push(Session {
            digest: Sha256::digest(token.as_bytes()).into(),
            expires: Instant::now() + Duration::from_secs(60),
            read_only,
        });
        Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, format!("127.0.0.1:{}", state.port))
            .header(header::ORIGIN, format!("http://127.0.0.1:{}", state.port))
            .header("X-Turtle-Request", "1")
            .header(header::COOKIE, format!("turtle_session={token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .expect("authorized request")
    }

    #[tokio::test]
    async fn agent_approval_intent_has_no_decision_route() {
        let (state, _dir) = state();
        let response = router(state.clone())
            .oneshot(session_request(
                &state,
                Method::POST,
                "/api/v1/agents/approvals",
                json!({"actor":"agent_run", "decision":"approve", "subject_hash":"0".repeat(64)}),
                false,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    #[tokio::test]
    async fn rejects_non_loopback_bind_and_rebinding() {
        assert!(loopback_address("0.0.0.0".parse().expect("IP"), 0).is_err());
        assert!(loopback_address("192.0.2.1".parse().expect("IP"), 0).is_err());
        assert!(loopback_address("127.0.0.1".parse().expect("IP"), 0).is_ok());
        let (state, _dir) = state();
        let response = router(state)
            .oneshot(req(Method::GET, "/api/v1/health", "attacker.test"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response
                .headers()
                .get("x-content-type-options")
                .expect("header"),
            "nosniff"
        );
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none()
        );
    }
    #[tokio::test]
    async fn dashboard_write_routes_create_project_job_and_budget() {
        let (state, dir) = state();
        let repo = dir.path().join("example repo");
        std::fs::create_dir_all(&repo).expect("repo directory");
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .expect("git command")
        };
        assert!(git(&["init", "-b", "main"]).status.success());
        assert!(
            git(&["config", "user.email", "test@example.invalid"])
                .status
                .success()
        );
        assert!(git(&["config", "user.name", "API test"]).status.success());
        std::fs::write(repo.join("README.md"), "hello\n").expect("initial file");
        assert!(git(&["add", "README.md"]).status.success());
        assert!(git(&["commit", "-m", "initial"]).status.success());
        let app = router(state.clone());
        let response = app
            .clone()
            .oneshot(session_request(
                &state,
                Method::POST,
                "/api/v1/projects",
                json!({"path":repo}),
                false,
            ))
            .await
            .expect("project response");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("project body");
        let project: Value = serde_json::from_slice(&bytes).expect("project JSON");
        assert_eq!(project["slug"], "example-repo");
        let response=app.clone().oneshot(session_request(&state,Method::POST,"/api/v1/projects/example-repo/jobs",json!({"title":"Ship feature","description":"Detailed request","type":"bugfix","priority":"urgent","profile":"strict","budget":250000,"start":false}),false)).await.expect("job response");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("job body");
        let job: Value = serde_json::from_slice(&bytes).expect("job JSON");
        let key = job["key"].as_str().expect("job key");
        assert!(key.starts_with("EXA-"));
        assert_eq!(job["started"], false);
        let response = app
            .clone()
            .oneshot(session_request(
                &state,
                Method::POST,
                "/api/v1/budgets",
                json!({"scope":"job","target":key,"limit":500000,"soft":true}),
                false,
            ))
            .await
            .expect("budget response");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("budget body");
        let budget: Value = serde_json::from_slice(&bytes).expect("budget JSON");
        assert_eq!(budget["budget"]["limit"], 500000);
        assert_eq!(budget["budget"]["kind"], "soft");
    }
    #[tokio::test]
    async fn read_only_session_cannot_use_any_dashboard_write_route() {
        let (state, _dir) = state();
        for (path, body) in [
            ("/api/v1/projects", json!({"path":"/tmp"})),
            ("/api/v1/projects/demo/jobs", json!({"title":"blocked"})),
            ("/api/v1/budgets", json!({"scope":"global","limit":1000})),
        ] {
            let response = router(state.clone())
                .oneshot(session_request(&state, Method::POST, path, body, true))
                .await
                .expect("write response");
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("error body");
            let value: Value = serde_json::from_slice(&bytes).expect("error JSON");
            assert_eq!(value["error"]["code"], "read_only_session");
        }
    }
    #[test]
    fn tailnet_host_validation_is_strict() {
        assert_eq!(
            validate_tailnet_host("node-1.team.ts.net.").expect("valid name"),
            "node-1.team.ts.net"
        );
        for invalid in [
            "Node.team.ts.net",
            "node.team.ts.net:443",
            "*.team.ts.net",
            "node.team.ts.net ",
            "node..team.ts.net",
            "node.example.com",
        ] {
            assert!(
                validate_tailnet_host(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
    }
    #[tokio::test]
    async fn tailnet_host_is_exact_and_uses_https_origin_and_secure_cookie() {
        let (mut state, _dir) = state();
        state.tailnet_host = Some("node.team.ts.net".into());
        let health = router(state.clone())
            .oneshot(req(Method::GET, "/api/v1/health", "node.team.ts.net"))
            .await
            .expect("tailnet health");
        assert_eq!(health.status(), StatusCode::OK);
        for host in [
            "other.team.ts.net",
            "node.team.ts.net:443",
            "node.team.ts.net ",
        ] {
            let response = router(state.clone())
                .oneshot(req(Method::GET, "/api/v1/health", host))
                .await
                .expect("rejected host");
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{host:?}");
        }
        let bad_origin = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/session")
            .header(header::HOST, "node.team.ts.net")
            .header(header::ORIGIN, "http://node.team.ts.net")
            .header("X-Turtle-Request", "1")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                "{\"code\":\"00000000000000000000000000000000\"}",
            ))
            .expect("bad origin request");
        assert_eq!(
            router(state.clone())
                .oneshot(bad_origin)
                .await
                .expect("bad origin")
                .status(),
            StatusCode::FORBIDDEN
        );
        let code = issue_login_code(&state.services.store)
            .await
            .expect("login code");
        let valid_origin = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/session")
            .header(header::HOST, "node.team.ts.net")
            .header(header::ORIGIN, "https://node.team.ts.net")
            .header("X-Turtle-Request", "1")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"code":code}).to_string()))
            .expect("session request");
        let response = router(state)
            .oneshot(valid_origin)
            .await
            .expect("session response");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response
                .headers()
                .get(header::SET_COOKIE)
                .expect("cookie")
                .to_str()
                .expect("cookie text")
                .contains("; Secure")
        );
    }
    #[test]
    fn astro_csp_hashes_are_sent_in_the_http_header() {
        let html=b"<meta http-equiv=\"content-security-policy\" content=\"default-src 'self'; script-src 'self' 'sha256-example'\">";
        let response = asset_response("index.html", html.to_vec());
        let policy = response
            .headers()
            .get("content-security-policy")
            .expect("CSP")
            .to_str()
            .expect("header");
        assert!(policy.contains("'sha256-example'"));
        assert!(policy.contains("frame-ancestors 'none'"));
    }
    #[tokio::test]
    async fn post_requires_origin_and_custom_header_and_cookie() {
        let (state, _dir) = state();
        let response = router(state.clone())
            .oneshot(req(Method::GET, "/api/v1/overview", "127.0.0.1:9876"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = router(state.clone())
            .oneshot(req(Method::POST, "/api/v1/session", "127.0.0.1:9876"))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let foreign = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/session")
            .header(header::HOST, "127.0.0.1:9876")
            .header(header::ORIGIN, "http://evil.test")
            .header("X-Turtle-Request", "1")
            .body(Body::empty())
            .expect("request");
        assert_eq!(
            router(state.clone())
                .oneshot(foreign)
                .await
                .expect("response")
                .status(),
            StatusCode::FORBIDDEN
        );
        let missing = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/session")
            .header(header::HOST, "127.0.0.1:9876")
            .header(header::ORIGIN, "http://127.0.0.1:9876")
            .body(Body::empty())
            .expect("request");
        assert_eq!(
            router(state)
                .oneshot(missing)
                .await
                .expect("response")
                .status(),
            StatusCode::FORBIDDEN
        );
    }
    #[tokio::test]
    async fn code_is_hashed_once_and_cookie_is_private() {
        let (state, _dir) = state();
        let code = issue_login_code(&state.services.store).await.expect("code");
        let code_hash = state
            .services
            .store
            .with(|s| {
                s.raw_connection().query_row(
                    "SELECT code_hash FROM dashboard_login_codes",
                    [],
                    |r| r.get::<_, String>(0),
                )
            })
            .await
            .expect("stored hash");
        assert_ne!(code_hash, code);
        assert_eq!(code_hash, digest_hex(&code));
        let make = || {
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/session")
                .header(header::HOST, "127.0.0.1:9876")
                .header(header::ORIGIN, "http://127.0.0.1:9876")
                .header("X-Turtle-Request", "1")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"code":code}).to_string()))
                .expect("request")
        };
        let response = router(state.clone())
            .oneshot(make())
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .expect("cookie")
            .to_str()
            .expect("cookie text");
        assert!(
            cookie.contains("HttpOnly")
                && cookie.contains("SameSite=Strict")
                && cookie.contains("Path=/")
                && cookie.contains("Max-Age=43200")
        );
        assert!(
            !cookie.contains("Secure"),
            "plain loopback HTTP must receive the cookie"
        );
        let replay = router(state.clone())
            .oneshot(make())
            .await
            .expect("response");
        assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
        let session = cookie.split(';').next().expect("session");
        let private = Request::builder()
            .method(Method::GET)
            .uri("/api/v1/overview")
            .header(header::HOST, "127.0.0.1:9876")
            .header(header::COOKIE, session)
            .body(Body::empty())
            .expect("request");
        let renewed = router(state).oneshot(private).await.expect("response");
        assert_eq!(renewed.status(), StatusCode::OK);
        assert!(
            renewed
                .headers()
                .get(header::SET_COOKIE)
                .expect("sliding cookie")
                .to_str()
                .expect("cookie text")
                .contains("Max-Age=43200")
        );
    }
    #[tokio::test]
    async fn expired_code_cannot_be_redeemed() {
        let (state, _dir) = state();
        let hash = digest_hex("expired");
        let for_insert = hash.clone();
        state
            .services
            .store
            .with(move |s| s.create_dashboard_code(&for_insert, "2000-01-01T00:00:00Z", "full"))
            .await
            .expect("insert");
        assert!(
            state
                .services
                .store
                .with(move |s| s.redeem_dashboard_code(&hash))
                .await
                .expect("redeem")
                .is_none()
        );
    }
    #[tokio::test]
    async fn approval_hash_mismatch_keeps_gate_and_valid_decision_moves_it() {
        use turtle_domain::{ArtifactKind, GateKind, JobState, JobType, Priority};
        use turtle_store::{NewJob, NewProject, NewRevision};
        let (state, _dir) = state();
        let hash = state
            .services
            .blobs
            .put(b"{\"summary\":\"reviewed\"}")
            .expect("blob");
        let job = state
            .services
            .store
            .with(move |s| {
                s.transaction(|tx| {
                    let project = tx.insert_project(&NewProject {
                        slug: "sample".into(),
                        key_prefix: "API".into(),
                        name: "Sample".into(),
                        description: String::new(),
                        repo_path: "/tmp/sample".into(),
                        default_branch: "main".into(),
                        default_profile: "standard".into(),
                    })?;
                    let job = tx.create_job(
                        &NewJob {
                            project_id: project,
                            title: "Test".into(),
                            description: String::new(),
                            job_type: JobType::Feature,
                            priority: Priority::Normal,
                            profile: turtle_workflow::Profile::standard(),
                        },
                        &Actor::local_user(),
                    )?;
                    tx.add_revision(&NewRevision {
                        job_id: job.job.id,
                        kind: ArtifactKind::Specification,
                        sha256: hash,
                        size_bytes: 22,
                        media_type: "application/json".into(),
                        schema_id: None,
                        author: Actor::System,
                    })?;
                    Ok(job)
                })
            })
            .await
            .expect("job");
        let id = job.job.id;
        state
            .services
            .store
            .with(move |s| {
                s.raw_connection().execute(
                    "UPDATE jobs SET state='scope_approval' WHERE id=?1",
                    [id.to_string()],
                )
            })
            .await
            .expect("gate");
        let (approval, _) = state
            .services
            .approvals
            .request(id, GateKind::Scope)
            .await
            .expect("approval");
        let phone_code = issue_read_only_login_code(&state.services.store)
            .await
            .expect("phone code");
        let exchanged = router(state.clone())
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/session")
                    .header(header::HOST, "127.0.0.1:9876")
                    .header(header::ORIGIN, "http://127.0.0.1:9876")
                    .header("X-Turtle-Request", "1")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({"code":phone_code}).to_string()))
                    .expect("exchange request"),
            )
            .await
            .expect("exchange response");
        assert_eq!(exchanged.status(), StatusCode::OK);
        let phone_cookie = exchanged
            .headers()
            .get(header::SET_COOKIE)
            .expect("phone cookie")
            .to_str()
            .expect("cookie text")
            .to_owned();
        let phone_token = phone_cookie
            .strip_prefix("turtle_session=")
            .and_then(|v| v.split(';').next())
            .expect("session token")
            .to_owned();
        let me = router(state.clone())
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/me")
                    .header(header::HOST, "127.0.0.1:9876")
                    .header(header::COOKIE, format!("turtle_session={phone_token}"))
                    .body(Body::empty())
                    .expect("me request"),
            )
            .await
            .expect("me response");
        let me: Value = serde_json::from_slice(
            &axum::body::to_bytes(me.into_body(), usize::MAX)
                .await
                .expect("me body"),
        )
        .expect("me JSON");
        assert_eq!(me, json!({"readonly":true}));
        let readonly_decision = Request::builder().method(Method::POST).uri(format!("/api/v1/approvals/{}/decisions",approval.id))
            .header(header::HOST,"127.0.0.1:9876").header(header::ORIGIN,"http://127.0.0.1:9876")
            .header("X-Turtle-Request","1").header(header::COOKIE,format!("turtle_session={phone_token}"))
            .header(header::CONTENT_TYPE,"application/json")
            .body(Body::from(json!({"decision":"approve","subject_hash":approval.subject_hash.to_hex(),"comment":"","acknowledged_flags":[]}).to_string())).expect("decision request");
        let denied = router(state.clone())
            .oneshot(readonly_decision)
            .await
            .expect("denied decision");
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        let denied_body = axum::body::to_bytes(denied.into_body(), usize::MAX)
            .await
            .expect("denied body");
        assert_eq!(
            serde_json::from_slice::<Value>(&denied_body).expect("denied JSON")["error"]["code"],
            "read_only_session"
        );
        let job_command = Request::builder().method(Method::POST).uri(format!("/api/v1/jobs/{}/commands",job.key))
            .header(header::HOST,"127.0.0.1:9876").header(header::ORIGIN,"http://127.0.0.1:9876")
            .header("X-Turtle-Request","1").header(header::COOKIE,format!("turtle_session={phone_token}"))
            .header(header::CONTENT_TYPE,"application/json")
            .body(Body::from(json!({"command":"start","expected_version":job.job.version,"idempotency_key":"phone-start"}).to_string())).expect("job command");
        let denied = router(state.clone())
            .oneshot(job_command)
            .await
            .expect("denied job command");
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        let unchanged = state
            .services
            .store
            .with(move |s| {
                Ok::<_, StoreError>((
                    s.approval(approval.id)?.status,
                    s.load_job(id)?.job.state,
                    s.load_job(id)?.job.version,
                ))
            })
            .await
            .expect("read-only actions left state unchanged");
        assert_eq!(unchanged.0, turtle_domain::ApprovalStatus::Pending);
        assert_eq!(unchanged.1, JobState::ScopeApproval);
        assert_eq!(unchanged.2, job.job.version);
        let upgrade = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/session")
            .header(header::HOST, "127.0.0.1:9876")
            .header(header::ORIGIN, "http://127.0.0.1:9876")
            .header("X-Turtle-Request", "1")
            .header(header::COOKIE, format!("turtle_session={phone_token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({ "code": "00000000000000000000000000000000" }).to_string(),
            ))
            .expect("upgrade attempt");
        // A read-only cookie cannot talk its way into a full session: the exchange needs a valid
        // one-time code, and a full-scope code only comes from the interactive CLI.
        assert_eq!(
            router(state.clone())
                .oneshot(upgrade)
                .await
                .expect("upgrade response")
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let me = router(state.clone())
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/me")
                    .header(header::HOST, "127.0.0.1:9876")
                    .header(header::COOKIE, format!("turtle_session={phone_token}"))
                    .body(Body::empty())
                    .expect("scope check"),
            )
            .await
            .expect("scope response");
        let me: Value = serde_json::from_slice(
            &axum::body::to_bytes(me.into_body(), usize::MAX)
                .await
                .expect("scope body"),
        )
        .expect("scope JSON");
        assert_eq!(me, json!({"readonly":true}));
        let token = "a".repeat(64);
        let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        state.sessions.lock().expect("session lock").push(Session {
            digest,
            expires: Instant::now() + Duration::from_secs(60),
            read_only: false,
        });
        let make = |subject_hash: String| {
            Request::builder().method(Method::POST).uri(format!("/api/v1/approvals/{}/decisions",approval.id))
            .header(header::HOST,"127.0.0.1:9876").header(header::ORIGIN,"http://127.0.0.1:9876")
            .header("X-Turtle-Request","1").header(header::COOKIE,format!("turtle_session={token}"))
            .header(header::CONTENT_TYPE,"application/json")
            .body(Body::from(json!({"decision":"approve","subject_hash":subject_hash,"comment":"","acknowledged_flags":[]}).to_string())).expect("request")
        };
        let wrong = router(state.clone())
            .oneshot(make("0".repeat(64)))
            .await
            .expect("response");
        assert_eq!(wrong.status(), StatusCode::CONFLICT);
        let before = state
            .services
            .store
            .with(move |s| {
                Ok::<_, StoreError>((s.approval(approval.id)?.status, s.load_job(id)?.job.state))
            })
            .await
            .expect("unchanged");
        assert_eq!(before.0, turtle_domain::ApprovalStatus::Pending);
        assert_eq!(before.1, JobState::ScopeApproval);
        let (right, concurrent) = tokio::join!(
            router(state.clone()).oneshot(make(approval.subject_hash.to_hex())),
            router(state.clone()).oneshot(make(approval.subject_hash.to_hex()))
        );
        let statuses = [
            right.expect("response").status(),
            concurrent.expect("response").status(),
        ];
        assert_eq!(statuses.iter().filter(|s| **s == StatusCode::OK).count(), 1);
        assert_eq!(
            statuses
                .iter()
                .filter(|s| **s == StatusCode::CONFLICT)
                .count(),
            1
        );
        let after = state
            .services
            .store
            .with(move |s| {
                Ok::<_, StoreError>((s.approval(approval.id)?.status, s.load_job(id)?.job.state))
            })
            .await
            .expect("decided");
        assert_eq!(after.0, turtle_domain::ApprovalStatus::Approved);
        assert_ne!(after.1, JobState::ScopeApproval);
    }
    #[tokio::test]
    async fn sse_resumes_from_header_and_query_without_gaps() {
        let (state, _dir) = state();
        state
            .services
            .store
            .with(|s| {
                s.transaction(|tx| {
                    for n in 1..=3 {
                        tx.append_event(&turtle_store::NewEvent {
                            project_id: None,
                            job_id: None,
                            kind: format!("test.{n}"),
                            payload: json!({"n":n}),
                            actor: Actor::System,
                        })?;
                    }
                    Ok(())
                })
            })
            .await
            .expect("events");
        let token = "b".repeat(64);
        let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        state.sessions.lock().expect("session lock").push(Session {
            digest,
            expires: Instant::now() + Duration::from_secs(60),
            read_only: false,
        });
        for query_form in [false, true] {
            let path = if query_form {
                "/api/v1/events/stream?lastEventId=1"
            } else {
                "/api/v1/events/stream"
            };
            let mut builder = Request::builder()
                .method(Method::GET)
                .uri(path)
                .header(header::HOST, "127.0.0.1:9876")
                .header(header::COOKIE, format!("turtle_session={token}"));
            if !query_form {
                builder = builder.header("Last-Event-ID", "1");
            }
            let response = router(state.clone())
                .oneshot(builder.body(Body::empty()).expect("request"))
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::OK);
            let mut body = response.into_body().into_data_stream();
            let first = tokio::time::timeout(Duration::from_secs(2), body.next())
                .await
                .expect("first event")
                .expect("chunk")
                .expect("bytes");
            let second = tokio::time::timeout(Duration::from_secs(2), body.next())
                .await
                .expect("second event")
                .expect("chunk")
                .expect("bytes");
            assert!(
                String::from_utf8_lossy(&first).contains("id: 2"),
                "{first:?}"
            );
            assert!(
                String::from_utf8_lossy(&second).contains("id: 3"),
                "{second:?}"
            );
        }
    }
    #[tokio::test]
    async fn command_replay_does_not_apply_twice() {
        use turtle_domain::{JobType, Priority};
        use turtle_store::{NewJob, NewProject};
        let (state, _dir) = state();
        let job = state
            .services
            .store
            .with(|s| {
                s.transaction(|tx| {
                    let project = tx.insert_project(&NewProject {
                        slug: "sample".into(),
                        key_prefix: "API".into(),
                        name: "Sample".into(),
                        description: String::new(),
                        repo_path: "/tmp/sample".into(),
                        default_branch: "main".into(),
                        default_profile: "fast".into(),
                    })?;
                    tx.create_job(
                        &NewJob {
                            project_id: project,
                            title: "Test".into(),
                            description: String::new(),
                            job_type: JobType::Feature,
                            priority: Priority::Normal,
                            profile: turtle_workflow::Profile::fast(),
                        },
                        &Actor::local_user(),
                    )
                })
            })
            .await
            .expect("job");
        let token = "c".repeat(64);
        let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        state.sessions.lock().expect("session lock").push(Session {
            digest,
            expires: Instant::now() + Duration::from_secs(60),
            read_only: false,
        });
        let path = format!("/api/v1/jobs/{}/commands", job.key);
        let make = || {
            Request::builder()
                .method(Method::POST)
                .uri(&path)
                .header(header::HOST, "127.0.0.1:9876")
                .header(header::ORIGIN, "http://127.0.0.1:9876")
                .header("X-Turtle-Request", "1")
                .header(header::COOKIE, format!("turtle_session={token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"command":"start","expected_version":0,"idempotency_key":"same-key"})
                        .to_string(),
                ))
                .expect("request")
        };
        assert_eq!(
            router(state.clone())
                .oneshot(make())
                .await
                .expect("first")
                .status(),
            StatusCode::OK
        );
        let after_first = state
            .services
            .store
            .with(move |s| s.load_job(job.job.id))
            .await
            .expect("job");
        assert_eq!(
            router(state.clone())
                .oneshot(make())
                .await
                .expect("replay")
                .status(),
            StatusCode::OK
        );
        let after_replay = state
            .services
            .store
            .with(move |s| s.load_job(job.job.id))
            .await
            .expect("job");
        assert_eq!(after_replay.job.version, after_first.job.version);
    }

    #[tokio::test]
    async fn a_read_only_session_can_still_log_in_again_with_a_fresh_code() {
        let (state, _dir) = state();
        let exchange = |code: String, cookie: Option<String>| {
            let mut builder = Request::builder()
                .method(Method::POST)
                .uri("/api/v1/session")
                .header(header::HOST, "127.0.0.1:9876")
                .header(header::ORIGIN, "http://127.0.0.1:9876")
                .header("X-Turtle-Request", "1")
                .header(header::CONTENT_TYPE, "application/json");
            if let Some(cookie) = cookie {
                builder = builder.header(header::COOKIE, cookie);
            }
            builder
                .body(Body::from(json!({ "code": code }).to_string()))
                .expect("exchange request")
        };
        let first = issue_read_only_login_code(&state.services.store)
            .await
            .expect("first code");
        let response = router(state.clone())
            .oneshot(exchange(first, None))
            .await
            .expect("first exchange");
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .expect("session cookie")
            .to_owned();

        let second = issue_read_only_login_code(&state.services.store)
            .await
            .expect("second code");
        let again = router(state.clone())
            .oneshot(exchange(second, Some(cookie.clone())))
            .await
            .expect("second exchange");
        assert_eq!(again.status(), StatusCode::OK, "re-scanning must work");

        // Only the session exchange is exempt: other writes stay blocked for that cookie.
        let write = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/jobs/PAY-1/commands")
            .header(header::HOST, "127.0.0.1:9876")
            .header(header::ORIGIN, "http://127.0.0.1:9876")
            .header("X-Turtle-Request", "1")
            .header(header::COOKIE, cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .expect("write request");
        let blocked = router(state).oneshot(write).await.expect("blocked write");
        assert_eq!(blocked.status(), StatusCode::FORBIDDEN);
    }
}
