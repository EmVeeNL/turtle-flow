//! Black-box tests of the `turtle` binary against a throwaway data directory and repository.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use std::sync::Arc;
use tempfile::TempDir;
use turtle_artifacts::BlobStore;
use turtle_domain::{Actor, ArtifactKind, JobKey};
use turtle_store::{NewRevision, Store, SystemClock};

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
