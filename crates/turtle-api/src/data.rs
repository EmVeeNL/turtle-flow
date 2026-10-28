//! Read projections for the dashboard. Values absent from the store remain null or unavailable.
use super::{ApiError, AppState, store_error};
use axum::{
    Json,
    extract::{Path, Query, State, rejection::QueryRejection},
    http::HeaderMap,
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{convert::Infallible, time::Duration};
use turtle_domain::{ApprovalId, ArtifactKind, JobKey};
use turtle_store::{
    ApprovalRecord, BudgetRecord, BudgetScope, DashboardEstimate, EventRow, JobRecord,
    ProjectRecord, ProjectStats, RevisionRecord, RunRecord, TaskRow,
};

fn budget(b: &BudgetRecord) -> Value {
    let scope = match b.scope {
        BudgetScope::Global => "global",
        BudgetScope::Project(_) => "project",
        BudgetScope::Job(_) => "job",
    };
    json!({"id":b.id.to_string(),"scope":scope,"settled":b.settled_tokens,"reserved":b.reserved_tokens,"limit":b.limit_tokens,"kind":if b.hard {"hard"} else {"soft"}})
}
fn project_value(
    p: &ProjectRecord,
    stats: &ProjectStats,
    jobs: &[JobRecord],
    approvals: &[ApprovalRecord],
    budgets: &[BudgetRecord],
) -> Value {
    let active = jobs
        .iter()
        .filter(|j| j.project_id == p.id && !j.job.state.is_terminal())
        .count();
    let pending = approvals
        .iter()
        .filter(|a| {
            a.status == turtle_domain::ApprovalStatus::Pending
                && jobs
                    .iter()
                    .any(|j| j.job.id == a.job_id && j.project_id == p.id)
        })
        .count();
    let b = budgets
        .iter()
        .find(|b| b.scope == BudgetScope::Project(p.id));
    json!({"id":p.id.to_string(),"slug":p.slug,"key_prefix":p.key_prefix,"name":p.name,"description":p.description,"repo_path":p.repo_path,"default_branch":p.default_branch,"default_profile":p.default_profile,"status":p.status,"active_jobs":active,"pending_approvals":pending,"gate_pass_rate":stats.gate_pass_rate,"last_delivery":stats.last_delivery,"health":"unknown","budget":b.map(budget)})
}
fn usage_for_job(u: &turtle_store::JobUsage) -> Value {
    if u.runs_unavailable > 0 || u.runs == 0 {
        json!({"total":null,"provenance":"unavailable"})
    } else if u.tokens_estimated > 0 {
        json!({"total":u.tokens_reported + u.tokens_estimated,"provenance":"estimated"})
    } else {
        json!({"total":u.tokens_reported,"provenance":"reported"})
    }
}
fn job_value(
    j: &JobRecord,
    u: &turtle_store::JobUsage,
    provider: &Option<String>,
    estimate: &Option<DashboardEstimate>,
    budgets: &[BudgetRecord],
) -> Value {
    let state = serde_json::to_value(j.job.state).unwrap_or(Value::Null);
    let hold = j.job.holds.first().map(|h| match h {
        turtle_workflow::Hold::Paused { .. } => "paused",
        turtle_workflow::Hold::AwaitingBudget => "awaiting_budget",
        turtle_workflow::Hold::RateLimited { .. } => "rate_limited",
        turtle_workflow::Hold::WaitingForDependencies => "waiting_for_dependencies",
    });
    let priority = serde_json::to_value(j.priority).unwrap_or(Value::Null);
    let job_type = serde_json::to_value(j.job_type).unwrap_or(Value::Null);
    let estimate = estimate
        .as_ref()
        .map(|e| json!({"p10":e.p10,"p50":e.p50,"p90":e.p90,"confidence":e.confidence}));
    let budget = budgets
        .iter()
        .find(|b| b.scope == BudgetScope::Job(j.job.id))
        .map(budget);
    json!({"id":j.job.id.to_string(),"key":j.key.to_string(),"project_id":j.project_id.to_string(),"title":j.title,"description":j.description,"type":job_type,"profile":j.job.profile.name,"state":state,"hold":hold,"holds":j.job.holds,"priority":priority,"stage_index":stage_index(j.job.state),"provider":provider,"usage":usage_for_job(u),"estimate":estimate,"budget":budget,"elapsed":null,"base_commit":j.base_commit,"head_commit":null,"version":j.job.version})
}
fn stage_index(s: turtle_domain::JobState) -> usize {
    use turtle_domain::JobState;
    match s {
        JobState::Backlog | JobState::Blocked | JobState::Cancelled | JobState::Failed => 0,
        JobState::Discovery | JobState::Specification => 1,
        JobState::ScopeApproval => 2,
        JobState::Planning | JobState::PlanApproval => 3,
        JobState::Implementation | JobState::BuildTest => 4,
        JobState::Review => 5,
        JobState::Verification => 6,
        JobState::Delivery | JobState::AwaitingRelease => 7,
        JobState::AcceptanceApproval | JobState::Done => 8,
    }
}
fn artifact_value(r: &RevisionRecord) -> Value {
    json!({"id":r.id,"job_id":r.job_id.to_string(),"kind":serde_json::to_value(r.kind).unwrap_or(Value::Null),"rev":r.rev,"sha256":r.sha256.to_hex(),"author":r.author_id.as_deref().unwrap_or(&r.author_kind),"created_at":r.created_at,"summary":null})
}
fn approval_value(
    a: &ApprovalRecord,
    revisions: &[RevisionRecord],
    blobs: &turtle_artifacts::BlobStore,
) -> Result<Value, ApiError> {
    let artifacts: Vec<_> = a.binding.artifacts.iter().map(|(kind, binding)| {
        let record = revisions.iter().find(|r| r.job_id == a.job_id && r.rev == binding.rev && r.sha256 == binding.sha256);
        let content = blobs.get(&binding.sha256).map_err(|_| ApiError::unavailable()).and_then(|bytes| String::from_utf8(bytes).map_err(|_| ApiError::unavailable()))?;
        Ok(json!({"kind":kind,"rev":binding.rev,"sha256":binding.sha256.to_hex(),"id":record.map(|r| r.id.as_str()),"content":content}))
    }).collect::<Result<_,ApiError>>()?;
    Ok(
        json!({"id":a.id.to_string(),"job_id":a.job_id.to_string(),"gate_kind":serde_json::to_value(a.gate).unwrap_or(Value::Null),"subject_hash":a.subject_hash.to_hex(),"status":serde_json::to_value(a.status).unwrap_or(Value::Null),"requested_at":a.created_at,"expires_at":null,"binding":{"artifacts":artifacts,"base_commit":a.binding.git.as_ref().map(|g| &g.base_commit),"head_commit":a.binding.git.as_ref().map(|g| &g.head_commit),"diff_sha256":a.binding.git.as_ref().map(|g| g.diff_sha256.to_hex()),"target_branch":a.binding.git.as_ref().map(|g| &g.target_branch),"budget_envelope":a.binding.budget_envelope.as_ref().map(|b| b.tokens_max),"budget_increase":a.binding.budget_increase,"profile_hash":a.binding.profile_hash.to_hex(),"policy_version":a.binding.policy_version,"canonical":a.binding},"flags":a.binding.acknowledged_flags,"invalidated_if":["bound content changes"]}),
    )
}
fn run_value(r: &RunRecord) -> Value {
    let total = r.input + r.cache_read + r.cache_write + r.output;
    let usable = r.provenance != "unavailable";
    json!({"id":r.id,"job_id":r.job_id,"role":r.role,"provider":r.provider,"model":r.model,"cli_version":r.cli_version,"routing_reason":r.routing_reason,"status":r.status,"duration":r.duration_ms.map(|ms| format!("{}s", ms / 1000)),"usage":{"total":if usable {Some(total)} else {None},"provenance":r.provenance,"input":if usable {Some(r.input)} else {None},"cached":if usable {Some(r.cache_read + r.cache_write)} else {None},"output":if usable {Some(r.output)} else {None}},"cost_usd":null})
}
fn event_value(e: &EventRow) -> Value {
    json!({"id":e.seq.to_string(),"seq":e.seq,"occurred_at":e.occurred_at,"type":e.kind,"actor_kind":e.actor_kind,"actor_id":e.actor_id,"job_id":e.job_id,"payload":{"summary":e.kind,"detail":e.payload.to_string()},"raw_payload":e.payload})
}
fn task_value(t: &TaskRow) -> Value {
    json!({"id":t.id,"job_id":t.job_id,"ordinal":t.ordinal,"title":t.title,"state":t.state,"commit_sha":t.commit_sha,"gate_summary":null})
}

pub async fn overview(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let (
        projects,
        jobs,
        approvals,
        runs,
        revisions,
        events,
        budgets,
        usages,
        limits,
        quotas,
        tasks,
        stats,
        job_facts,
    ) = state
        .services
        .store
        .with(|s| {
            let projects = s.list_projects()?;
            let jobs = s.list_jobs(None)?;
            let approvals = s.pending_approvals()?;
            let runs = s.recent_runs(50)?;
            let revisions = s.all_revisions()?;
            let events = s.recent_events(50)?;
            let budgets = s.list_budgets()?;
            let usages = jobs
                .iter()
                .map(|j| s.dashboard_job_usage(j.job.id))
                .collect::<turtle_store::Result<Vec<_>>>()?;
            let limits = s.current_limited_providers()?;
            let quotas = s.latest_quota_snapshots()?;
            let tasks = s.dashboard_tasks(None)?;
            let stats = projects
                .iter()
                .map(|p| s.dashboard_project_stats(p.id))
                .collect::<turtle_store::Result<Vec<_>>>()?;
            let job_facts = jobs
                .iter()
                .map(|j| {
                    Ok((
                        s.dashboard_latest_provider(j.job.id)?,
                        s.dashboard_estimate(j.job.id)?,
                    ))
                })
                .collect::<turtle_store::Result<Vec<_>>>()?;
            Ok::<_, turtle_store::StoreError>((
                projects, jobs, approvals, runs, revisions, events, budgets, usages, limits,
                quotas, tasks, stats, job_facts,
            ))
        })
        .await
        .map_err(store_error)?;
    let projects: Vec<_> = projects
        .iter()
        .zip(&stats)
        .map(|(p, stats)| project_value(p, stats, &jobs, &approvals, &budgets))
        .collect();
    let jobs: Vec<_> = jobs
        .iter()
        .zip(&usages)
        .zip(&job_facts)
        .map(|((j, u), (provider, estimate))| job_value(j, u, provider, estimate, &budgets))
        .collect();
    let approvals: Vec<_> = approvals
        .iter()
        .map(|a| approval_value(a, &revisions, &state.services.blobs))
        .collect::<Result<_, _>>()?;
    let runs: Vec<_> = runs.iter().map(run_value).collect();
    let artifacts: Vec<_> = revisions.iter().map(artifact_value).collect();
    let events: Vec<_> = events.iter().map(event_value).collect();
    let providers: Vec<_> = ["claude-code", "codex"].iter().map(|id| {
        let limit = limits.iter().find(|l| l.provider == *id);
        let windows: Vec<_> = quotas.iter().filter(|q| q.provider == *id).map(|q| json!({"window":q.window_name,"utilization":q.utilization,"status":q.status,"observed_at":q.observed_at})).collect();
        json!({"id":id,"name":id,"version":null,"model":null,"status":if limit.is_some() {"rate_limited"} else {"untested"},"running":null,"quota":null,"quota_windows":windows,"rate_limit_reset":limit.and_then(|l| l.reset_at.as_deref()),"capabilities":{},"price_per_million":null})
    }).collect();
    Ok(Json(
        json!({"projects":projects,"jobs":jobs,"approvals":approvals,"runs":runs,"artifacts":artifacts,"tasks":tasks.iter().map(task_value).collect::<Vec<_>>(),"resources":[],"events":events,"providers":providers,"budgets":budgets.iter().map(budget).collect::<Vec<_>>()}),
    ))
}

pub async fn project(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let (p, stats, jobs, approvals, budgets, usages, facts) = state
        .services
        .store
        .with(move |s| {
            let p = s.project_by_slug(&slug)?;
            let stats = s.dashboard_project_stats(p.id)?;
            let jobs = s.list_jobs(Some(p.id))?;
            let usages = jobs
                .iter()
                .map(|j| s.dashboard_job_usage(j.job.id))
                .collect::<turtle_store::Result<Vec<_>>>()?;
            let facts = jobs
                .iter()
                .map(|j| {
                    Ok((
                        s.dashboard_latest_provider(j.job.id)?,
                        s.dashboard_estimate(j.job.id)?,
                    ))
                })
                .collect::<turtle_store::Result<Vec<_>>>()?;
            Ok::<_, turtle_store::StoreError>((
                p,
                stats,
                jobs,
                s.pending_approvals()?,
                s.list_budgets()?,
                usages,
                facts,
            ))
        })
        .await
        .map_err(store_error)?;
    let mut value = project_value(&p, &stats, &jobs, &approvals, &budgets);
    value["jobs"] = json!(
        jobs.iter()
            .zip(&usages)
            .zip(&facts)
            .map(|((j, u), (provider, estimate))| job_value(j, u, provider, estimate, &budgets))
            .collect::<Vec<_>>()
    );
    Ok(Json(value))
}

pub async fn resources(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let records = state
        .services
        .store
        .with(move |store| {
            let project = store.project_by_slug(&slug)?;
            store.list_resources(project.id)
        })
        .await
        .map_err(store_error)?;
    Ok(Json(json!({"resources": records})))
}

#[derive(Deserialize)]
pub struct ResourceSearchQuery {
    project: String,
    query: String,
}

pub async fn resource_search(
    State(state): State<AppState>,
    query: Result<Query<ResourceSearchQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::bad("invalid query"))?;
    let hits = state
        .services
        .store
        .with(move |store| {
            let project = store.project_by_slug(&query.project)?;
            store.search_resources(project.id, None, &query.query, 20)
        })
        .await
        .map_err(store_error)?;
    Ok(Json(json!({"hits": hits})))
}
#[derive(Deserialize)]
pub struct JobQuery {
    include: Option<String>,
}
pub async fn job(
    State(state): State<AppState>,
    Path(key): Path<String>,
    query: Result<Query<JobQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::bad("invalid query"))?;
    let key: JobKey = key.parse().map_err(|_| ApiError::not_found())?;
    let (
        job,
        usage,
        approvals,
        runs,
        events,
        revisions,
        tasks,
        provider,
        estimate,
        budgets,
        gate_revision,
    ) = state
        .services
        .store
        .with(move |s| {
            let j = s.find_job(&key)?;
            let id = j.job.id;
            Ok::<_, turtle_store::StoreError>((
                j,
                s.dashboard_job_usage(id)?,
                s.list_approvals(id)?,
                s.runs_for_job(id)?,
                s.list_events(Some(id))?,
                s.all_revisions()?
                    .into_iter()
                    .filter(|r| r.job_id == id)
                    .collect::<Vec<_>>(),
                s.dashboard_tasks(Some(id))?,
                s.dashboard_latest_provider(id)?,
                s.dashboard_estimate(id)?,
                s.list_budgets()?,
                s.latest_revision(id, ArtifactKind::GateResult)?,
            ))
        })
        .await
        .map_err(store_error)?;
    let mut value = job_value(&job, &usage, &provider, &estimate, &budgets);
    value["gates"] = gate_revision
        .as_ref()
        .and_then(|revision| state.services.blobs.get(&revision.sha256).ok())
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .unwrap_or(Value::Null);
    if query.include.is_some() {
        value["approvals"] = json!(
            approvals
                .iter()
                .map(|a| approval_value(a, &revisions, &state.services.blobs))
                .collect::<Result<Vec<_>, _>>()?
        );
        value["runs"] = json!(runs.iter().map(run_value).collect::<Vec<_>>());
        value["events"] = json!(events.iter().map(event_value).collect::<Vec<_>>());
        value["artifacts"] = json!(revisions.iter().map(artifact_value).collect::<Vec<_>>());
        value["tasks"] = json!(tasks.iter().map(task_value).collect::<Vec<_>>());
    }
    Ok(Json(value))
}
pub async fn approval(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let id: ApprovalId = id.parse().map_err(|_| ApiError::not_found())?;
    let (a, revisions) = state
        .services
        .store
        .with(move |s| Ok::<_, turtle_store::StoreError>((s.approval(id)?, s.all_revisions()?)))
        .await
        .map_err(store_error)?;
    Ok(Json(approval_value(&a, &revisions, &state.services.blobs)?))
}
pub async fn run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let run = state
        .services
        .store
        .with(move |s| s.run_by_id(&id))
        .await
        .map_err(store_error)?;
    Ok(Json(run_value(&run)))
}
#[derive(Deserialize)]
pub struct Page {
    offset: Option<usize>,
    limit: Option<usize>,
}
pub async fn transcript(
    State(state): State<AppState>,
    Path(id): Path<String>,
    query: Result<Query<Page>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(page) = query.map_err(|_| ApiError::bad("invalid query"))?;
    let run = state
        .services
        .store
        .with(move |s| s.run_by_id(&id))
        .await
        .map_err(store_error)?;
    let Some(hash) = run.transcript_blob else {
        return Ok(Json(
            json!({"lines":[],"next_offset":null,"provenance":"unavailable"}),
        ));
    };
    let bytes = state
        .services
        .blobs
        .get(&hash)
        .map_err(|_| ApiError::unavailable())?;
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<_> = text.lines().collect();
    let offset = page.offset.unwrap_or(0).min(lines.len());
    let limit = page.limit.unwrap_or(100).min(500);
    let end = (offset + limit).min(lines.len());
    Ok(Json(
        json!({"lines":&lines[offset..end],"next_offset":if end<lines.len(){Some(end)}else{None},"provenance":"redacted"}),
    ))
}
pub async fn artifact(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let r = state
        .services
        .store
        .with(move |s| s.revision_by_id(&id))
        .await
        .map_err(store_error)?;
    let bytes = state
        .services
        .blobs
        .get(&r.sha256)
        .map_err(|_| ApiError::unavailable())?;
    let mut value = artifact_value(&r);
    value["content"] = json!(String::from_utf8(bytes).ok());
    Ok(Json(value))
}
pub async fn usage(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let jobs = state
        .services
        .store
        .with(|s| {
            let jobs = s.list_jobs(None)?;
            jobs.iter()
                .map(|j| Ok((j.key.to_string(), s.dashboard_job_usage(j.job.id)?)))
                .collect::<turtle_store::Result<Vec<_>>>()
        })
        .await
        .map_err(store_error)?;
    Ok(Json(
        json!({"jobs":jobs.iter().map(|(key,u)| json!({"job_key":key,"usage":usage_for_job(u)})).collect::<Vec<_>>()}),
    ))
}
#[derive(Deserialize)]
pub struct StreamQuery {
    project: Option<String>,
    job: Option<String>,
    #[serde(rename = "lastEventId")]
    last_event_id: Option<i64>,
}
pub async fn events_stream(
    State(state): State<AppState>,
    query: Result<Query<StreamQuery>, QueryRejection>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::bad("invalid query"))?;
    let project = if let Some(slug) = query.project {
        Some(
            state
                .services
                .store
                .with(move |s| s.project_by_slug(&slug))
                .await
                .map_err(store_error)?
                .id
                .to_string(),
        )
    } else {
        None
    };
    let job = if let Some(key) = query.job {
        let key: JobKey = key.parse().map_err(|_| ApiError::not_found())?;
        Some(
            state
                .services
                .store
                .with(move |s| s.find_job(&key))
                .await
                .map_err(store_error)?
                .job
                .id
                .to_string(),
        )
    } else {
        None
    };
    let header_cursor = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<i64>().ok());
    let mut seq = query.last_event_id.or(header_cursor).unwrap_or(0).max(0);
    let store = state.services.store;
    let initial = {
        let p = project.clone();
        let j = job.clone();
        store
            .with(move |s| s.events_after(seq, p.as_deref(), j.as_deref(), 128))
            .await
            .map_err(store_error)?
    };
    let mut stop = state.stop;
    let stream = async_stream::stream! {
        let mut initial=Some(initial);
        loop {
            let rows=if let Some(rows)=initial.take() { rows } else {
                let p=project.clone(); let j=job.clone();
                match store.with(move |s| s.events_after(seq,p.as_deref(),j.as_deref(),128)).await { Ok(rows)=>rows, Err(_)=>break }
            };
            for row in rows.iter() {
                seq=row.seq;
                yield Ok::<Event,Infallible>(Event::default().id(seq.to_string()).data(event_value(row).to_string()));
            }
            if rows.len()<128 {
                if let Some(rx)=stop.as_mut() {
                    tokio::select! { _ = tokio::time::sleep(Duration::from_millis(500)) => {}, _ = rx.changed() => { if *rx.borrow() { break; } } }
                } else { tokio::time::sleep(Duration::from_millis(500)).await; }
            }
        }
    };
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response())
}
