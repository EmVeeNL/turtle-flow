//! Black-box tests of the `turtle` binary against a throwaway data directory and repository.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use std::sync::Arc;
use tempfile::TempDir;
use turtle_artifacts::BlobStore;
use turtle_domain::{
    Actor, ArtifactKind, GateKind, JobKey, Role, RunId, Sha256, TokenUsage, UsageValue,
};
use turtle_policy::ApprovalBinding;
use turtle_store::{NewEvent, NewRevision, NewRun, RunEnd, Store, SystemClock};

struct World {
    _dir: TempDir,
    home: PathBuf,
    repo: PathBuf,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonical");
        let repo = root.join("payments-api");
        std::fs::create_dir_all(&repo).expect("mkdir");
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("README.md"), "# payments\n").expect("write");
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "init"]);
        Self {
            home: root.join("home"),
            repo,
            _dir: dir,
        }
    }

    fn turtle(&self, args: &[&str]) -> Output {
        self.command(args).output().expect("run turtle")
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_turtle"));
        c.args(args)
            .env("TURTLE_HOME", &self.home)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null());
        c
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.turtle(args);
        assert!(
            out.status.success(),
            "turtle {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn fails(&self, args: &[&str]) -> String {
        let out = self.turtle(args);
        assert!(!out.status.success(), "turtle {args:?} should have failed");
        String::from_utf8_lossy(&out.stderr).into_owned()
    }

    fn add_project(&self) {
        self.ok(&[
            "project",
            "add",
            self.repo.to_str().expect("utf8"),
            "--build",
            "true",
            "--test",
            "true",
        ]);
    }
}

#[test]
fn dashboard_refuses_without_tty() {
    let world = World::new();
    let error = world.fails(&["dashboard"]);
    assert!(error.contains("interactive terminal"), "{error}");
}

#[test]
fn init_refuses_non_interactive_without_creating_turtle_home() {
    let w = World::new();
    let error = w.fails(&["init"]);
    assert!(
        error.contains("interactive") && error.contains("will not guess"),
        "{error}"
    );
    assert!(
        !w.home.exists(),
        "init must not create its data directory before confirmation"
    );
}

#[test]
fn job_export_is_repeatable_and_worktree_path_is_printed_without_creation() {
    let w = World::new();
    w.add_project();
    w.ok(&["job", "create", "Export this job"]);
    let mut store =
        Store::open(&w.home.join("turtle.db"), Arc::new(SystemClock)).expect("open store");
    let key: JobKey = "PAY-1".parse().expect("job key");
    let job = store.find_job(&key).expect("job record");
    let bytes = br#"{"summary":"approved scope","note":"password=hunter2"}"#;
    let blobs = BlobStore::open(w.home.join("blobs")).expect("blob store");
    let artifact_hash = blobs.put(bytes).expect("store artifact");
    let binding = ApprovalBinding::new(GateKind::Scope, job.job.id, Sha256::from_bytes([7; 32]), 1);
    let subject_hash = binding.subject_hash();
    store
        .transaction(|tx| {
            tx.add_revision(&NewRevision {
                job_id: job.job.id,
                kind: ArtifactKind::Specification,
                sha256: artifact_hash,
                size_bytes: bytes.len() as u64,
                media_type: "application/json".into(),
                schema_id: None,
                author: Actor::local_user(),
            })?;
            tx.request_approval(&binding)?;
            Ok(())
        })
        .expect("store export fixtures");
    drop(store);
    let first = w.ok(&["export", "job", "PAY-1", "--format", "json"]);
    let second = w.ok(&["export", "job", "PAY-1", "--format", "json"]);
    assert_eq!(first, second);
    let exported: serde_json::Value = serde_json::from_str(&first).expect("valid export JSON");
    assert_eq!(exported["job"]["key"], "PAY-1");
    assert_eq!(exported["job"]["title"], "Export this job");
    assert_eq!(
        exported["artifacts"][0]["content"]["summary"],
        "approved scope"
    );
    assert_eq!(
        exported["artifacts"][0]["content"]["note"],
        "password=[REDACTED:password]"
    );
    assert_eq!(exported["artifacts"][0]["content_redacted"], true);
    assert_eq!(
        exported["artifacts"][0]["sha256"],
        artifact_hash.to_string()
    );
    assert_eq!(
        exported["approvals"][0]["subject_hash"],
        subject_hash.to_string()
    );
    let path = w.ok(&["job", "worktree", "PAY-1"]);
    assert!(path.trim().ends_with("PAY-1"), "{path}");
    assert!(
        !Path::new(path.trim()).exists(),
        "printing the path must not create the worktree"
    );
}

#[test]
fn cancel_can_delete_a_job_branch() {
    let w = World::new();
    w.add_project();
    w.ok(&["job", "create", "Cancel this job"]);
    git(&w.repo, &["branch", "turtle/PAY-1-cancel-this-job"]);
    let output = w.ok(&["job", "cancel", "PAY-1", "--delete-branch"]);
    assert!(
        output.contains("Cancelled PAY-1") && output.contains("deletion are queued"),
        "{output}"
    );
    let _daemon = Daemon(
        w.command(&["daemon", "--agent", "demo"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn daemon"),
    );
    wait_for("job branch deletion", || {
        !Command::new("git")
            .arg("-C")
            .arg(&w.repo)
            .args([
                "show-ref",
                "--verify",
                "--quiet",
                "refs/heads/turtle/PAY-1-cancel-this-job",
            ])
            .status()
            .expect("check branch ref")
            .success()
    });
    assert!(w.ok(&["audit", "verify"]).starts_with("OK:"));
}

fn http(port: u16, method: &str, path: &str, headers: &str, body: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect API");
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("timeout");
    write!(stream,"{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n{body}",body.len()).expect("request");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("response");
    response
}

#[test]
fn daemon_serves_authenticated_api_and_static_dashboard() {
    use sha2::Digest;
    let w = World::new();
    w.add_project();
    w.ok(&["job", "create", "Add a greeting"]);
    let dist = w.home.parent().expect("parent").join("dashboard-dist");
    std::fs::create_dir_all(dist.join("jobs/view")).expect("static directories");
    std::fs::write(
        dist.join("index.html"),
        "<!doctype html><title>Test dashboard</title>",
    )
    .expect("index");
    std::fs::write(
        dist.join("jobs/view/index.html"),
        "<!doctype html><title>Job detail</title>",
    )
    .expect("detail");
    let _daemon = Daemon(
        w.command(&["daemon", "--agent", "demo"])
            .env("TURTLE_DASHBOARD_DIR", &dist)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("daemon"),
    );
    let port_file = w.home.join("run/api.port");
    wait_for("API port", || port_file.exists());
    let port: u16 = std::fs::read_to_string(&port_file)
        .expect("port")
        .parse()
        .expect("number");
    assert!(
        w.ok(&["status"])
            .contains(&format!("http://127.0.0.1:{port}"))
    );
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&port_file)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(http(port, "GET", "/api/v1/health", "", "").contains("200 OK"));
    assert!(http(port, "GET", "/api/v1/overview", "", "").contains("401 Unauthorized"));
    let code = "0123456789abcdef0123456789abcdef";
    let hash: String = sha2::Sha256::digest(code.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let mut store = Store::open(&w.home.join("turtle.db"), Arc::new(SystemClock)).expect("store");
    store
        .create_dashboard_code(&hash, "2099-01-01T00:00:00Z", "full")
        .expect("code");
    drop(store);
    let session = http(
        port,
        "POST",
        "/api/v1/session",
        &format!(
            "Origin: http://127.0.0.1:{port}\r\nX-Turtle-Request: 1\r\nContent-Type: application/json\r\n"
        ),
        &format!("{{\"code\":\"{code}\"}}"),
    );
    assert!(session.contains("200 OK"), "{session}");
    let cookie = session
        .lines()
        .find_map(|line| {
            line.strip_prefix("set-cookie: ")
                .or_else(|| line.strip_prefix("Set-Cookie: "))
        })
        .and_then(|line| line.split(';').next())
        .expect("session cookie");
    let auth = format!("Cookie: {cookie}\r\n");
    let overview = http(port, "GET", "/api/v1/overview", &auth, "");
    assert!(
        overview.contains("200 OK") && overview.contains("PAY-1"),
        "{overview}"
    );
    assert!(http(port, "GET", "/", &auth, "").contains("Test dashboard"));
    assert!(http(port, "GET", "/jobs/view?id=PAY-1", &auth, "").contains("Job detail"));
    assert!(http(port, "GET", "/does-not-exist", &auth, "").contains("404 Not Found"));
    w.ok(&["down"]);
    wait_for("API port cleanup", || !port_file.exists());
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t.invalid"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A daemon child process that is always killed, even when a test fails.
struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for(what: &str, mut check: impl FnMut() -> bool) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        if check() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn provider_commands_report_capabilities_and_validate_routing_flags() {
    let w = World::new();
    use std::os::unix::fs::PermissionsExt;
    let bin = w.home.parent().expect("parent").join("provider-bin");
    std::fs::create_dir_all(&bin).expect("bin");
    for (name, script) in [
        (
            "claude",
            "#!/bin/sh\nif [ \"$1\" = '--version' ]; then echo '2.1.310 (Claude Code)'; else echo '{\"loggedIn\":true}'; fi\n",
        ),
        (
            "codex",
            "#!/bin/sh\nif [ \"$1\" = '--version' ]; then echo 'codex-cli 0.163.0'; else echo 'Logged in using ChatGPT' >&2; fi\n",
        ),
    ] {
        let path = bin.join(name);
        std::fs::write(&path, script).expect("script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    let out = w
        .command(&["providers", "list"])
        .env("PATH", &bin)
        .output()
        .expect("providers");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let providers = String::from_utf8_lossy(&out.stdout);
    assert!(
        providers.contains("claude-code:") && providers.contains("codex:"),
        "{providers}"
    );
    assert!(
        providers.contains("login=LoggedIn")
            && providers.contains("schema=true")
            && providers.contains("quota="),
        "{providers}"
    );
    let conn = rusqlite::Connection::open(w.home.join("turtle.db")).expect("db");
    conn.execute(
        "INSERT INTO provider_limits_observed(id,provider,observed_at,raw_message) VALUES ('limit-1','codex','2026-10-25T00:00:00.000Z','fixture hint')",
        [],
    ).expect("limit observation");
    conn.execute(
        "INSERT INTO provider_quota_snapshots(id,provider,observed_at,window_name,utilization,status) VALUES ('quota-1','claude-code','2026-10-25T00:00:00.000Z','five_hour',0.93,'warning')",
        [],
    ).expect("quota snapshot");
    let refreshed = w
        .command(&["providers", "list"])
        .env("PATH", &bin)
        .output()
        .expect("providers list");
    let output = String::from_utf8_lossy(&refreshed.stdout);
    assert!(
        output.contains("codex: ") && output.contains("rate limited (reset unknown)"),
        "{output}"
    );
    assert!(
        output.contains("93% of provider window") && output.contains("five_hour"),
        "{output}"
    );
    assert!(
        w.fails(&["daemon", "--agent", "claude,bogus"])
            .contains("--agent must")
    );
    assert!(
        w.fails(&[
            "daemon",
            "--agent",
            "claude,codex",
            "--role-agent",
            "bogus=codex"
        ])
        .contains("unknown role")
    );
    assert!(
        w.fails(&[
            "daemon",
            "--agent",
            "demo",
            "--role-agent",
            "reviewer=codex"
        ])
        .contains("requires two providers")
    );
}

#[test]
fn project_add_accepts_a_repository_and_rejects_the_rest() {
    let w = World::new();
    assert!(
        w.fails(&["project", "add", "/definitely/not/here"])
            .contains("does not exist")
    );

    let plain = w.home.parent().expect("parent").join("plain");
    std::fs::create_dir_all(&plain).expect("mkdir");
    assert!(
        w.fails(&["project", "add", plain.to_str().expect("utf8")])
            .contains("not a Git repository")
    );

    let empty = w.home.parent().expect("parent").join("empty");
    std::fs::create_dir_all(&empty).expect("mkdir");
    git(&empty, &["init", "-q", "-b", "main"]);
    assert!(
        w.fails(&["project", "add", empty.to_str().expect("utf8")])
            .contains("no commits")
    );

    w.add_project();
    let list = w.ok(&["project", "list"]);
    assert!(
        list.contains("payments-api") && list.contains("PAY"),
        "{list}"
    );
    let show = w.ok(&["project", "show", "payments-api"]);
    assert!(show.contains("gate build"), "{show}");
    assert!(show.contains("delivery     local-squash"), "{show}");
    w.ok(&["project", "delivery", "payments-api", "pr"]);
    let show = w.ok(&["project", "show", "payments-api"]);
    assert!(show.contains("delivery     pr"), "{show}");

    let again = w.fails(&["project", "add", w.repo.to_str().expect("utf8")]);
    assert!(again.contains("already exists"), "{again}");
}

#[test]
fn jobs_are_created_listed_and_have_an_event_history() {
    let w = World::new();
    assert!(w.fails(&["job", "create", "x"]).contains("no projects yet"));
    w.add_project();

    let out = w.ok(&["job", "create", "Add a greeting", "--priority", "high"]);
    assert!(out.contains("Created PAY-1"), "{out}");
    assert!(w.ok(&["job", "list"]).contains("PAY-1"));
    let show = w.ok(&["job", "show", "pay-1"]);
    assert!(
        show.contains("state        backlog") && show.contains("standard"),
        "{show}"
    );
    let events = w.ok(&["job", "events", "PAY-1"]);
    assert!(events.contains("job.created"), "{events}");

    assert!(w.fails(&["job", "show", "PAY-9"]).contains("not found"));
    assert!(
        w.fails(&["job", "create", "x", "--type", "banana"])
            .contains("unknown job type")
    );
    assert!(w.ok(&["job", "list", "--active"]).contains("PAY-1"));
    w.ok(&["job", "cancel", "PAY-1"]);
    assert!(!w.ok(&["job", "list", "--active"]).contains("PAY-1"));
}

#[test]
fn a_planned_job_shows_an_honest_preflight_range() {
    let w = World::new();
    w.add_project();
    w.ok(&["job", "create", "Add a greeting"]);
    assert!(
        w.ok(&["budget", "estimate", "PAY-1"])
            .contains("estimate unknown")
    );
    let mut store = Store::open(&w.home.join("turtle.db"), Arc::new(SystemClock)).expect("store");
    let key: JobKey = "PAY-1".parse().expect("job key");
    let id = store.find_job(&key).expect("job").job.id;
    let blobs = BlobStore::open(w.home.join("blobs")).expect("blobs");
    let documents = [
        (
            ArtifactKind::Specification,
            serde_json::json!({
                "summary":"Add a greeting", "criteria":[{"id":"AC-1","statement":"greeting appears","verification":"test"}],
                "affected_areas":["src/**"], "risks":[]
            }),
        ),
        (
            ArtifactKind::Plan,
            serde_json::json!({"tasks":[{
                "id":"T1", "title":"Greeting", "instructions":"Add greeting", "expected_paths":["src/main.rs"],
                "criteria":["AC-1"]
            }]}),
        ),
    ];
    for (kind, value) in documents {
        let bytes = serde_json::to_vec(&value).expect("json");
        let hash = blobs.put(&bytes).expect("blob");
        store
            .transaction(|tx| {
                tx.add_revision(&NewRevision {
                    job_id: id,
                    kind,
                    sha256: hash,
                    size_bytes: bytes.len() as u64,
                    media_type: "application/json".into(),
                    schema_id: None,
                    author: Actor::Scheduler,
                })
            })
            .expect("revision");
    }
    drop(store);
    let show = w.ok(&["job", "show", "PAY-1"]);
    assert!(
        show.contains("estimate") && show.contains("unknown confidence"),
        "{show}"
    );
    let report = w.ok(&["budget", "estimate", "PAY-1"]);
    assert!(
        report.contains("estimate")
            && report.contains("confidence   unknown")
            && report.contains("envelope")
            && report.contains("planned runs only"),
        "{report}"
    );
}

#[test]
fn run_commands_list_usage_and_print_the_stored_redacted_transcript() {
    let w = World::new();
    w.add_project();
    w.ok(&["job", "create", "Add a greeting"]);
    let mut store = Store::open(&w.home.join("turtle.db"), Arc::new(SystemClock)).expect("store");
    let key: JobKey = "PAY-1".parse().expect("job key");
    let job = store.find_job(&key).expect("job");
    let run = RunId::new();
    let text = "{\"type\":\"result\",\"result\":\"done\"}\n";
    let blobs = BlobStore::open(w.home.join("blobs")).expect("blobs");
    let hash = blobs.put(text.as_bytes()).expect("transcript blob");
    store.transaction(|tx| {
        tx.begin_run(&NewRun { id: run, job_id: job.job.id, role: Role::Reviewer, stage: "review".into(), provider: "claude-code".into(), model: "sonnet".into(), adapter_version: "test".into(), context_manifest_rev: None })?;
        tx.finish_run(run, RunEnd::Succeeded, None, UsageValue::Reported { usage: TokenUsage { input: 12, output: 4, ..TokenUsage::default() } })?;
        tx.set_transcript_blob(run, hash, text.len() as u64)?;
        tx.append_event(&NewEvent { project_id: Some(job.project_id), job_id: Some(job.job.id), kind: "run.policy_denied".into(), payload: serde_json::json!({"run":run.to_string(),"denial":{"tool":"Bash","input":"git commit"}}), actor: Actor::AgentRun { id: run } })?;
        Ok(())
    }).expect("record run");
    drop(store);

    let listed = w.ok(&["runs", "list", "--job", "PAY-1"]);
    assert!(
        listed.contains("claude-code/sonnet") && listed.contains("16 tokens: 12 input"),
        "{listed}"
    );
    let transcript = w.ok(&["runs", "transcript", &run.to_string()]);
    assert!(
        transcript.contains("result") && transcript.contains("done"),
        "{transcript}"
    );
    let show = w.ok(&["job", "show", "PAY-1"]);
    assert!(
        show.contains("denied actions 1") && show.contains("Bash"),
        "{show}"
    );
    let raw = w.ok(&["runs", "transcript", &run.to_string(), "--raw"]);
    assert_eq!(raw, text);
}

#[test]
fn approving_needs_a_terminal_and_a_comment_for_changes() {
    let w = World::new();
    w.add_project();
    let stderr = w.fails(&["approve", "01M5S3KMMYB9H58821Z34WGDGM"]);
    assert!(stderr.contains("interactive terminal"), "{stderr}");
    let stderr = w.fails(&["approve", "01M5S3KMMYB9H58821Z34WGDGM", "--reject"]);
    assert!(stderr.contains("comment is required"), "{stderr}");
    assert!(w.ok(&["approvals"]).contains("Nothing is waiting"));
}

#[test]
fn audit_verify_passes_then_detects_tampering() {
    let w = World::new();
    w.add_project();
    w.ok(&["job", "create", "Add a greeting"]);
    assert!(w.ok(&["audit", "verify"]).starts_with("OK:"));

    let conn = rusqlite::Connection::open(w.home.join("turtle.db")).expect("open db");
    // An attacker with file access drops the append-only trigger first.
    conn.execute_batch(
        "DROP TRIGGER trg_events_no_update;
         UPDATE events SET payload = '{\"forged\":true}' WHERE type = 'job.created';",
    )
    .expect("tamper");
    drop(conn);

    let out = w.turtle(&["audit", "verify"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("TAMPERED"));
}

#[test]
fn the_daemon_runs_a_job_to_its_first_approval_and_is_exclusive() {
    let w = World::new();
    w.add_project();
    w.ok(&["job", "create", "Add a greeting", "--start"]);

    let refusal = w.fails(&["daemon"]);
    assert!(refusal.contains("choose an agent"), "{refusal}");

    let _daemon = Daemon(
        w.command(&["daemon", "--agent", "demo"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn daemon"),
    );
    wait_for("the daemon lock", || {
        w.ok(&["status"]).contains("running (pid")
    });
    assert!(
        w.fails(&["daemon", "--agent", "demo"])
            .contains("already running")
    );

    wait_for("the scope approval", || {
        w.ok(&["job", "show", "PAY-1"])
            .contains("state        scope_approval")
    });
    let waiting = w.ok(&["approvals"]);
    assert!(
        waiting.contains("PAY-1") && waiting.contains("scope"),
        "{waiting}"
    );
    assert!(w.ok(&["audit", "verify"]).starts_with("OK:"));
}

#[test]
fn a_job_that_runs_out_of_budget_waits_and_resumes_when_the_budget_is_raised() {
    let w = World::new();
    w.add_project();
    assert!(w.ok(&["budget", "show"]).contains("No budgets set"));
    assert!(
        w.fails(&["budget", "set", "lots", "--global"])
            .contains("not a token amount")
    );
    assert!(
        w.fails(&["budget", "set", "5k"])
            .contains("say which budget")
    );

    let out = w.ok(&[
        "job",
        "create",
        "Add a greeting",
        "--budget",
        "5k",
        "--start",
    ]);
    assert!(out.contains("Budget: 5000 tokens"), "{out}");

    let _daemon = Daemon(
        w.command(&["daemon", "--agent", "demo"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn daemon"),
    );
    wait_for("the job to wait for budget", || {
        w.ok(&["job", "show", "PAY-1"]).contains("out of budget")
    });
    let show = w.ok(&["budget", "show"]);
    assert!(
        show.contains("job PAY-1") && show.contains("Waiting for budget: PAY-1"),
        "{show}"
    );

    let raised = w.ok(&["budget", "set", "5M", "--job", "PAY-1"]);
    assert!(
        raised.contains("PAY-1 was waiting for budget and will continue"),
        "{raised}"
    );
    wait_for("the scope approval", || {
        w.ok(&["job", "show", "PAY-1"])
            .contains("state        scope_approval")
    });
    let show = w.ok(&["job", "show", "PAY-1"]);
    assert!(
        show.contains("tokens       0 measured, 1200 estimated over 1 runs"),
        "{show}"
    );
    let audit = w.ok(&["audit", "verify"]);
    assert!(audit.contains("ledger consistent"), "{audit}");
}
