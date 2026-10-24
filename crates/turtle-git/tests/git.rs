//! Integration tests against real Git in temporary repositories.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tempfile::TempDir;
use turtle_git::commit::Trailers;
use turtle_git::delivery::{DeliveryOutcome, SquashRequest};
use turtle_git::merge::MergeProbe;
use turtle_git::recovery::Healed;
use turtle_git::worktree::{WorktreeOutcome, WorktreeSpec};
use turtle_git::{GitError, Identity, Repo};

struct Fixture {
    _dir: TempDir,
    root: PathBuf,
    repo: Repo,
}

/// A repository with one commit on `main`, hermetic with respect to the user's Git config.
async fn fixture() -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonical").join("repo");
    std::fs::create_dir_all(&root).expect("mkdir");
    let repo = Repo::new(&root)
        .with_env("GIT_CONFIG_GLOBAL", "/dev/null")
        .with_env("GIT_CONFIG_NOSYSTEM", "1")
        .with_identity(&Identity {
            name: "Turtle Test".into(),
            email: "test@turtle.invalid".into(),
        });
    repo.run(&["init", "-q", "-b", "main"]).await.expect("init");
    write(&root, "README.md", "# project\n");
    write(&root, "src/lib.rs", "pub fn one() -> u32 { 1 }\n");
    repo.run(&["add", "-A"]).await.expect("add");
    repo.run(&["commit", "-q", "-m", "init"])
        .await
        .expect("commit");
    Fixture {
        _dir: dir,
        root,
        repo,
    }
}

fn write(root: &Path, rel: &str, content: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
    std::fs::write(p, content).expect("write");
}

fn spec(f: &Fixture, key: &str, base: &str) -> WorktreeSpec {
    WorktreeSpec {
        root: f.root.parent().expect("parent").join("worktrees"),
        project_slug: "proj".into(),
        job_key: key.into(),
        branch: format!("turtle/{key}-work"),
        base_commit: base.into(),
    }
}

fn trailers(job: &str, run: &str) -> Trailers {
    Trailers {
        job: job.into(),
        run: Some(run.into()),
        provider: Some("fake".into()),
        ..Trailers::default()
    }
}

#[tokio::test]
async fn validates_project_repositories() {
    let f = fixture().await;
    f.repo.validate_project_repo().await.expect("valid");
    assert_eq!(f.repo.default_branch().await.expect("branch"), "main");

    let empty = tempfile::tempdir().expect("tempdir");
    let r = Repo::new(empty.path()).with_env("GIT_CONFIG_GLOBAL", "/dev/null");
    r.run(&["init", "-q"]).await.expect("init");
    let err = r.validate_project_repo().await.expect_err("no commits");
    assert!(err.to_string().contains("no commits"), "{err}");

    let bare = tempfile::tempdir().expect("tempdir");
    let r = Repo::new(bare.path());
    r.run(&["init", "-q", "--bare"]).await.expect("init bare");
    assert!(r.validate_project_repo().await.is_err());

    let not_repo = tempfile::tempdir().expect("tempdir");
    assert!(
        Repo::new(not_repo.path())
            .validate_project_repo()
            .await
            .is_err()
    );
}

#[tokio::test]
async fn worktree_creation_is_idempotent_and_leaves_main_checkout_alone() {
    let f = fixture().await;
    let base = f.repo.head().await.expect("head");
    let s = spec(&f, "API-1", &base);

    assert_eq!(
        f.repo.ensure_worktree(&s).await.expect("create"),
        WorktreeOutcome::Created
    );
    assert_eq!(
        f.repo.ensure_worktree(&s).await.expect("again"),
        WorktreeOutcome::AlreadyPresent
    );

    let wt = f.repo.at(s.path().expect("path"));
    assert_eq!(
        wt.current_branch().await.expect("branch").as_deref(),
        Some("turtle/API-1-work")
    );
    assert_eq!(wt.head().await.expect("head"), base);
    // The main checkout is untouched: still on main, still clean.
    assert_eq!(
        f.repo.current_branch().await.expect("branch").as_deref(),
        Some("main")
    );
    assert!(f.repo.is_clean().await.expect("clean"));
}

#[tokio::test]
async fn worktree_recovers_when_branch_exists_but_worktree_is_missing() {
    let f = fixture().await;
    let base = f.repo.head().await.expect("head");
    let s = spec(&f, "API-2", &base);
    // Crash between "branch created" and "worktree added" (recovery boundary 13).
    f.repo
        .run(&["branch", &s.branch, &base])
        .await
        .expect("branch");
    assert_eq!(
        f.repo.ensure_worktree(&s).await.expect("recover"),
        WorktreeOutcome::Created
    );

    // A branch that moved away from the base is never reused silently.
    let s2 = spec(&f, "API-3", &base);
    f.repo
        .run(&["branch", &s2.branch, &base])
        .await
        .expect("branch");
    write(&f.root, "x.txt", "x");
    f.repo.run(&["add", "-A"]).await.expect("add");
    f.repo
        .run(&["commit", "-q", "-m", "moves main"])
        .await
        .expect("commit");
    f.repo
        .run(&["branch", "-f", &s2.branch, "main"])
        .await
        .expect("move branch");
    let err = f.repo.ensure_worktree(&s2).await.expect_err("moved branch");
    assert!(matches!(err, GitError::Invalid(_)), "{err}");
}

#[tokio::test]
async fn worktree_spec_rejects_path_escape() {
    let f = fixture().await;
    let mut s = spec(&f, "API-1", "main");
    s.project_slug = "../evil".into();
    assert!(s.path().is_err());
    s.project_slug = "proj".into();
    s.job_key = "API-1/../../x".into();
    assert!(s.path().is_err());
}

#[tokio::test]
async fn commits_carry_trailers_and_can_be_found_by_run() {
    let f = fixture().await;
    let base = f.repo.head().await.expect("head");
    let s = spec(&f, "API-4", &base);
    f.repo.ensure_worktree(&s).await.expect("create");
    let wt = f.repo.at(s.path().expect("path"));

    assert_eq!(
        wt.commit_all("feat(API-4): nothing", "", &trailers("API-4", "RUN1"))
            .await
            .expect("noop"),
        None
    );

    write(wt.path(), "src/limit.rs", "pub fn limit() {}\n");
    let sha = wt
        .commit_all(
            "feat(API-4): add limiter",
            "Adds the limiter.",
            &trailers("API-4", "RUN1"),
        )
        .await
        .expect("commit")
        .expect("created");
    let body = wt.run(&["log", "-1", "--format=%B"]).await.expect("log");
    assert!(body.starts_with("feat(API-4): add limiter\n\nAdds the limiter.\n\nTurtle-Job: API-4\nTurtle-Run: RUN1\nTurtle-Provider: fake"), "{body}");
    assert_eq!(
        f.repo
            .find_commit_by_run(&s.branch, "RUN1")
            .await
            .expect("probe"),
        Some(sha)
    );
    assert_eq!(
        f.repo
            .find_commit_by_run(&s.branch, "NOPE")
            .await
            .expect("probe"),
        None
    );
}

#[tokio::test]
async fn commit_refuses_secrets_and_escaping_symlinks() {
    let f = fixture().await;
    let base = f.repo.head().await.expect("head");
    let s = spec(&f, "API-5", &base);
    f.repo.ensure_worktree(&s).await.expect("create");
    let wt = f.repo.at(s.path().expect("path"));

    write(wt.path(), ".env", "TOKEN=abc\n");
    let err = wt
        .commit_all("feat(API-5): x", "", &trailers("API-5", "R"))
        .await
        .expect_err("secret");
    assert!(matches!(err, GitError::CommitRefused(_)), "{err}");
    assert!(
        wt.run(&["diff", "--cached", "--name-only"])
            .await
            .expect("diff")
            .is_empty(),
        "index reset"
    );
    std::fs::remove_file(wt.path().join(".env")).expect("rm");

    std::os::unix::fs::symlink("/etc/passwd", wt.path().join("link")).expect("symlink");
    let err = wt
        .commit_all("feat(API-5): x", "", &trailers("API-5", "R"))
        .await
        .expect_err("symlink");
    assert!(err.to_string().contains("outside"), "{err}");
    std::fs::remove_file(wt.path().join("link")).expect("rm");

    std::os::unix::fs::symlink("src/lib.rs", wt.path().join("ok-link")).expect("symlink");
    wt.commit_all("feat(API-5): inside link", "", &trailers("API-5", "R"))
        .await
        .expect("inside symlink is fine");
}

#[tokio::test]
async fn repository_hooks_and_fsmonitor_never_run() {
    use std::os::unix::fs::PermissionsExt;
    let f = fixture().await;
    let marker = f.root.parent().expect("parent").join("PWNED");
    let script = |body: &str| format!("#!/bin/sh\ntouch '{}'\n{body}\n", marker.display());
    for hook in [
        "pre-commit",
        "post-commit",
        "post-checkout",
        "commit-msg",
        "reference-transaction",
    ] {
        let p = f.root.join(".git/hooks").join(hook);
        std::fs::write(&p, script("exit 0")).expect("hook");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    let fsmon = f.root.join("fsmon.sh");
    std::fs::write(&fsmon, script("echo")).expect("fsmon");
    std::fs::set_permissions(&fsmon, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    f.repo
        .run(&["config", "core.fsmonitor", &fsmon.to_string_lossy()])
        .await
        .expect("config");

    let base = f.repo.head().await.expect("head");
    let s = spec(&f, "API-6", &base);
    f.repo.ensure_worktree(&s).await.expect("create");
    let wt = f.repo.at(s.path().expect("path"));
    write(wt.path(), "a.txt", "a");
    wt.commit_all("feat(API-6): a", "", &trailers("API-6", "R"))
        .await
        .expect("commit");
    wt.status_paths().await.expect("status");
    f.repo
        .deliver_squash(&SquashRequest {
            target_branch: "main",
            job_branch: &s.branch,
            job_key: "API-6",
            subject: "feat(API-6): a",
            body: "",
            trailers: "Turtle-Job: API-6",
        })
        .await
        .expect("deliver");
    assert!(
        !marker.exists(),
        "a repository hook or fsmonitor script executed"
    );
}

#[tokio::test]
async fn policy_check_detects_commits_branch_switches_ref_changes_and_hook_edits() {
    let f = fixture().await;
    let base = f.repo.head().await.expect("head");
    let s = spec(&f, "API-7", &base);
    f.repo.ensure_worktree(&s).await.expect("create");
    let wt = f.repo.at(s.path().expect("path"));

    // Editing files is allowed.
    let before = wt.snapshot().await.expect("snapshot");
    write(wt.path(), "src/lib.rs", "pub fn one() -> u32 { 2 }\n");
    assert!(wt.check_policy(&before).await.expect("check").is_ok());

    // An agent that commits.
    wt.run(&["add", "-A"]).await.expect("add");
    wt.run(&["commit", "-q", "-m", "agent commit"])
        .await
        .expect("commit");
    let check = wt.check_policy(&before).await.expect("check");
    assert!(
        !check.head_unchanged && !check.refs_unchanged && check.branch_unchanged,
        "{check:?}"
    );
    assert!(!check.violations().is_empty());

    // An agent that creates a branch / tag, or switches branch.
    let before = wt.snapshot().await.expect("snapshot");
    wt.run(&["tag", "sneaky"]).await.expect("tag");
    assert!(
        !wt.check_policy(&before)
            .await
            .expect("check")
            .refs_unchanged
    );
    let before = wt.snapshot().await.expect("snapshot");
    wt.run(&["switch", "-q", "-c", "other"])
        .await
        .expect("switch");
    assert!(
        !wt.check_policy(&before)
            .await
            .expect("check")
            .branch_unchanged
    );
    wt.run(&["switch", "-q", &s.branch]).await.expect("back");

    // An agent that edits hooks or config in the shared git dir.
    let before = wt.snapshot().await.expect("snapshot");
    std::fs::write(f.root.join(".git/hooks/pre-push"), "#!/bin/sh\nexit 0\n").expect("hook");
    assert!(
        !wt.check_policy(&before)
            .await
            .expect("check")
            .git_dir_untouched
    );
    std::fs::remove_file(f.root.join(".git/hooks/pre-push")).expect("rm");
    let before = wt.snapshot().await.expect("snapshot");
    f.repo
        .run(&["config", "alias.x", "!echo pwned"])
        .await
        .expect("config");
    assert!(
        !wt.check_policy(&before)
            .await
            .expect("check")
            .git_dir_untouched
    );
}

#[tokio::test]
async fn diff_hash_is_stable_and_changes_with_content() {
    let f = fixture().await;
    let base = f.repo.head().await.expect("head");
    let s = spec(&f, "API-8", &base);
    f.repo.ensure_worktree(&s).await.expect("create");
    let wt = f.repo.at(s.path().expect("path"));
    write(
        wt.path(),
        "src/lib.rs",
        "pub fn one() -> u32 { 1 }\npub fn two() -> u32 { 2 }\n",
    );
    write(wt.path(), "new.txt", "new\n");
    std::fs::write(wt.path().join("blob.bin"), [0u8, 1, 2, 255]).expect("bin");
    wt.run(&["mv", "README.md", "README2.md"])
        .await
        .expect("mv");
    let head = wt
        .commit_all("feat(API-8): change", "", &trailers("API-8", "R"))
        .await
        .expect("c")
        .expect("sha");

    let h1 = wt.diff_sha256(&base, &head).await.expect("hash");
    assert_eq!(h1, wt.diff_sha256(&base, &head).await.expect("hash"));

    let files = wt.changed_files(&base, &head).await.expect("files");
    let by = |p: &str| {
        files
            .iter()
            .find(|f| f.path == p)
            .unwrap_or_else(|| panic!("{p} missing: {files:?}"))
    };
    assert_eq!(
        (by("src/lib.rs").status, by("src/lib.rs").added),
        ('M', Some(1))
    );
    assert_eq!(by("new.txt").status, 'A');
    assert_eq!(by("blob.bin").added, None, "binary has no line counts");
    assert_eq!(
        (
            by("README2.md").status,
            by("README2.md").old_path.as_deref()
        ),
        ('R', Some("README.md"))
    );

    write(wt.path(), "new.txt", "new!\n");
    let head2 = wt
        .commit_all("feat(API-8): more", "", &trailers("API-8", "R2"))
        .await
        .expect("c")
        .expect("sha");
    assert_ne!(h1, wt.diff_sha256(&base, &head2).await.expect("hash"));
}

#[tokio::test]
async fn merge_probe_reports_clean_and_conflicting_branches_without_side_effects() {
    let f = fixture().await;
    let base = f.repo.head().await.expect("head");
    let s = spec(&f, "API-9", &base);
    f.repo.ensure_worktree(&s).await.expect("create");
    let wt = f.repo.at(s.path().expect("path"));
    write(wt.path(), "src/lib.rs", "pub fn one() -> u32 { 100 }\n");
    wt.commit_all("feat(API-9): job edit", "", &trailers("API-9", "R"))
        .await
        .expect("c");

    let refs_before = f.repo.run(&["for-each-ref"]).await.expect("refs");
    assert!(matches!(
        f.repo.probe_merge("main", &s.branch).await.expect("probe"),
        MergeProbe::Clean { .. }
    ));

    // main changes the same line: conflict.
    write(&f.root, "src/lib.rs", "pub fn one() -> u32 { 7 }\n");
    f.repo
        .run(&["commit", "-q", "-am", "main edit"])
        .await
        .expect("commit");
    let MergeProbe::Conflicts { files } =
        f.repo.probe_merge("main", &s.branch).await.expect("probe")
    else {
        panic!("expected a conflict")
    };
    assert_eq!(files, ["src/lib.rs"]);

    assert_eq!(
        f.repo
            .run(&["for-each-ref"])
            .await
            .expect("refs")
            .lines()
            .count(),
        refs_before.lines().count()
    );
    assert!(
        f.repo.is_clean().await.expect("clean"),
        "probe must not touch the work tree"
    );
    let err = f
        .repo
        .deliver_squash(&SquashRequest {
            target_branch: "main",
            job_branch: &s.branch,
            job_key: "API-9",
            subject: "x",
            body: "",
            trailers: "Turtle-Job: API-9",
        })
        .await
        .expect_err("conflict blocks delivery");
    assert!(
        matches!(err, GitError::Conflicts(ref f) if f == &["src/lib.rs"]),
        "{err}"
    );
}

#[tokio::test]
async fn squash_delivery_to_a_branch_that_is_not_checked_out_moves_it_with_cas() {
    let f = fixture().await;
    let base = f.repo.head().await.expect("head");
    f.repo
        .run(&["branch", "release", &base])
        .await
        .expect("branch");
    let s = spec(&f, "API-10", &base);
    f.repo.ensure_worktree(&s).await.expect("create");
    let wt = f.repo.at(s.path().expect("path"));
    write(wt.path(), "a.txt", "a\n");
    wt.commit_all("feat(API-10): a", "", &trailers("API-10", "R1"))
        .await
        .expect("c");
    write(wt.path(), "b.txt", "b\n");
    wt.commit_all("feat(API-10): b", "", &trailers("API-10", "R2"))
        .await
        .expect("c");

    let req = SquashRequest {
        target_branch: "release",
        job_branch: &s.branch,
        job_key: "API-10",
        subject: "feat(API-10): add a and b",
        body: "Squashed.",
        trailers: "Turtle-Job: API-10",
    };
    let DeliveryOutcome::Delivered { commit } = f.repo.deliver_squash(&req).await.expect("deliver")
    else {
        panic!("release is not checked out, expected a direct delivery")
    };
    assert_eq!(
        f.repo.branch_tip("release").await.expect("tip"),
        Some(commit.clone())
    );
    let parents = f
        .repo
        .run(&["log", "-1", "--format=%P", &commit])
        .await
        .expect("parents");
    assert_eq!(
        parents, base,
        "squash commit has the old target as its only parent"
    );
    assert_eq!(
        f.repo
            .run(&["show", &format!("{commit}:a.txt")])
            .await
            .expect("show"),
        "a"
    );
    assert_eq!(
        f.repo
            .find_delivery_commit("release", "API-10")
            .await
            .expect("probe"),
        Some(commit)
    );
    assert_eq!(
        f.repo.current_branch().await.expect("branch").as_deref(),
        Some("main"),
        "main checkout untouched"
    );
    assert!(!f.root.join("a.txt").exists());
}

#[tokio::test]
async fn delivery_to_a_checked_out_branch_is_parked_not_applied() {
    let f = fixture().await;
    let base = f.repo.head().await.expect("head");
    let s = spec(&f, "API-11", &base);
    f.repo.ensure_worktree(&s).await.expect("create");
    let wt = f.repo.at(s.path().expect("path"));
    write(wt.path(), "a.txt", "a\n");
    wt.commit_all("feat(API-11): a", "", &trailers("API-11", "R"))
        .await
        .expect("c");

    let outcome = f
        .repo
        .deliver_squash(&SquashRequest {
            target_branch: "main",
            job_branch: &s.branch,
            job_key: "API-11",
            subject: "feat(API-11): a",
            body: "",
            trailers: "Turtle-Job: API-11",
        })
        .await
        .expect("deliver");
    let DeliveryOutcome::PendingIntegration { commit, reference } = outcome else {
        panic!("main is checked out, must not be moved")
    };
    assert_eq!(reference, "refs/turtle/delivery/API-11");
    assert_eq!(
        f.repo.head().await.expect("head"),
        base,
        "main did not move"
    );
    assert!(f.repo.is_clean().await.expect("clean"));
    assert_eq!(f.repo.rev(&reference).await.expect("ref"), commit);
    // The documented follow-up command works.
    f.repo
        .run(&["merge", "--ff-only", "-q", &reference])
        .await
        .expect("ff");
    assert!(f.root.join("a.txt").exists());
}

#[tokio::test]
async fn delivery_fails_safely_when_the_target_moves_concurrently() {
    let f = fixture().await;
    let base = f.repo.head().await.expect("head");
    f.repo
        .run(&["branch", "release", &base])
        .await
        .expect("branch");
    let s = spec(&f, "API-12", &base);
    f.repo.ensure_worktree(&s).await.expect("create");
    let wt = f.repo.at(s.path().expect("path"));
    write(wt.path(), "a.txt", "a\n");
    wt.commit_all("feat(API-12): a", "", &trailers("API-12", "R"))
        .await
        .expect("c");

    // Delivery was prepared and approved against `base`; then someone else advances `release`.
    let other = f
        .repo
        .run(&[
            "commit-tree",
            "-p",
            &base,
            "-m",
            "someone else",
            &format!("{base}^{{tree}}"),
        ])
        .await
        .expect("commit-tree");
    f.repo
        .run(&["update-ref", "refs/heads/release", &other, &base])
        .await
        .expect("move release");

    let req = SquashRequest {
        target_branch: "release",
        job_branch: &s.branch,
        job_key: "API-12",
        subject: "feat(API-12): a",
        body: "",
        trailers: "Turtle-Job: API-12",
    };
    let err = f
        .repo
        .deliver_squash_from(&req, &base)
        .await
        .expect_err("stale expectation");
    assert!(matches!(err, GitError::TargetMoved { .. }), "{err}");
    assert_eq!(
        f.repo.branch_tip("release").await.expect("tip"),
        Some(other.clone()),
        "their commit is intact"
    );

    // Re-preparing against the new tip succeeds.
    let ok = f
        .repo
        .deliver_squash_from(&req, &other)
        .await
        .expect("retry on fresh tip");
    assert!(matches!(ok, DeliveryOutcome::Delivered { .. }));
}

#[tokio::test]
async fn stash_to_recovery_ref_saves_untracked_work_and_cleans_the_worktree() {
    let f = fixture().await;
    let base = f.repo.head().await.expect("head");
    let s = spec(&f, "API-13", &base);
    f.repo.ensure_worktree(&s).await.expect("create");
    let wt = f.repo.at(s.path().expect("path"));

    assert_eq!(
        wt.stash_to_recovery_ref("RUN13").await.expect("clean"),
        None
    );

    write(wt.path(), "src/lib.rs", "pub fn one() -> u32 { 99 }\n");
    write(wt.path(), "brand_new.rs", "// untracked\n");
    let rec = wt
        .stash_to_recovery_ref("RUN13")
        .await
        .expect("stash")
        .expect("commit");

    assert!(wt.is_clean().await.expect("clean"), "worktree reset");
    assert_eq!(wt.head().await.expect("head"), base, "HEAD unchanged");
    assert_eq!(
        f.repo.rev("refs/turtle/recovery/RUN13").await.expect("ref"),
        rec
    );
    assert_eq!(
        f.repo
            .run(&["show", &format!("{rec}:brand_new.rs")])
            .await
            .expect("show"),
        "// untracked"
    );
    assert!(
        f.repo
            .run(&["show", &format!("{rec}:src/lib.rs")])
            .await
            .expect("show")
            .contains("99")
    );
    assert!(wt.stash_to_recovery_ref("bad/id").await.is_err());
}

#[tokio::test]
async fn heal_removes_stale_locks_only_when_safe_and_aborts_interrupted_operations() {
    let f = fixture().await;
    let base = f.repo.head().await.expect("head");
    let s = spec(&f, "API-14", &base);
    f.repo.ensure_worktree(&s).await.expect("create");
    let wt = f.repo.at(s.path().expect("path"));
    let lock = wt.git_dir().await.expect("gitdir").join("index.lock");

    // Fresh lock: left alone even without a git process.
    std::fs::write(&lock, "").expect("lock");
    assert_eq!(
        wt.heal_with(false).await.expect("heal"),
        [Healed::LeftIndexLock]
    );
    assert!(lock.exists());

    // Old lock but a git process is running: left alone.
    let old = SystemTime::now() - Duration::from_secs(600);
    std::fs::File::options()
        .write(true)
        .open(&lock)
        .expect("open")
        .set_modified(old)
        .expect("mtime");
    assert_eq!(
        wt.heal_with(true).await.expect("heal"),
        [Healed::LeftIndexLock]
    );
    assert!(lock.exists());

    // Old lock, nothing running: removed.
    assert_eq!(
        wt.heal_with(false).await.expect("heal"),
        [Healed::RemovedStaleIndexLock]
    );
    assert!(!lock.exists());
    assert!(wt.heal_with(false).await.expect("heal").is_empty());

    // An interrupted rebase is aborted.
    write(&f.root, "main.txt", "m\n");
    f.repo.run(&["add", "-A"]).await.expect("add");
    f.repo
        .run(&["commit", "-q", "-m", "main moves"])
        .await
        .expect("commit");
    write(wt.path(), "src/lib.rs", "pub fn one() -> u32 { 5 }\n");
    wt.commit_all("feat(API-14): edit", "", &trailers("API-14", "R"))
        .await
        .expect("c");
    write(&f.root, "src/lib.rs", "pub fn one() -> u32 { 6 }\n");
    f.repo
        .run(&["commit", "-q", "-am", "conflicting main edit"])
        .await
        .expect("commit");
    wt.run(&["rebase", "main"])
        .await
        .expect_err("rebase should stop on a conflict");
    assert_eq!(
        wt.heal_with(false).await.expect("heal"),
        [Healed::AbortedRebase]
    );
    assert!(wt.is_clean().await.expect("clean"));
}

#[tokio::test]
async fn removing_a_worktree_prunes_it_and_branch_deletion_is_prefix_guarded() {
    let f = fixture().await;
    let base = f.repo.head().await.expect("head");
    let s = spec(&f, "API-15", &base);
    f.repo.ensure_worktree(&s).await.expect("create");
    let path = s.path().expect("path");
    write(&path, "leftover.tmp", "x");
    f.repo.remove_worktree(&path, true).await.expect("remove");
    assert!(!path.exists());
    f.repo
        .remove_worktree(&path, true)
        .await
        .expect("removing twice is fine");

    f.repo
        .delete_branch(&s.branch, "turtle/", false)
        .await
        .expect("merged branch deleted");
    assert!(
        f.repo.delete_branch("main", "turtle/", true).await.is_err(),
        "non-factory branch is protected"
    );
    assert_eq!(f.repo.branch_tip("main").await.expect("tip"), Some(base));
}

#[tokio::test]
async fn other_jobs_activity_during_a_run_is_not_a_violation() {
    let f = fixture().await;
    let base = f.repo.head().await.expect("head");
    let a = spec(&f, "API-20", &base);
    f.repo.ensure_worktree(&a).await.expect("create a");
    let wt_a = f.repo.at(a.path().expect("path"));
    let before = wt_a.snapshot().await.expect("snapshot");

    // While job A's agent runs, job B is created and commits, and a delivery ref and a recovery
    // ref appear. None of that is A's agent misbehaving.
    let b = spec(&f, "API-21", &base);
    f.repo.ensure_worktree(&b).await.expect("create b");
    let wt_b = f.repo.at(b.path().expect("path"));
    write(wt_b.path(), "b.txt", "b\n");
    wt_b.commit_all("feat(API-21): b", "", &trailers("API-21", "RB"))
        .await
        .expect("commit b");
    f.repo
        .run(&["update-ref", "refs/turtle/delivery/API-9", &base])
        .await
        .expect("delivery ref");
    f.repo
        .run(&["update-ref", "refs/turtle/recovery/OTHER", &base])
        .await
        .expect("recovery ref");

    let check = wt_a.check_policy(&before).await.expect("check");
    assert!(check.is_ok(), "{:?}", check.violations());
}

#[tokio::test]
async fn quarantine_preserves_and_undoes_everything_a_misbehaving_agent_did() {
    let f = fixture().await;
    let base = f.repo.head().await.expect("head");
    let s = spec(&f, "API-22", &base);
    f.repo.ensure_worktree(&s).await.expect("create");
    let wt = f.repo.at(s.path().expect("path"));
    write(wt.path(), "legit.txt", "legit\n");
    wt.commit_all("feat(API-22): legit", "", &trailers("API-22", "R0"))
        .await
        .expect("c");
    let before = wt.snapshot().await.expect("snapshot");

    // The agent commits, tags, creates a branch, switches to it, and leaves an untracked file.
    write(wt.path(), "sneaky.txt", "sneaky\n");
    wt.run(&["add", "-A"]).await.expect("add");
    wt.run(&["commit", "-q", "-m", "agent commit"])
        .await
        .expect("commit");
    wt.run(&["tag", "agent-tag"]).await.expect("tag");
    wt.run(&["switch", "-q", "-c", "agent-branch"])
        .await
        .expect("switch");
    write(wt.path(), "leftover.txt", "untracked\n");
    let check = wt.check_policy(&before).await.expect("check");
    assert!(!check.is_ok());

    let saved = wt.quarantine("RUN22", &before).await.expect("quarantine");

    // Everything the agent did is preserved...
    assert_eq!(
        f.repo.rev("refs/turtle/recovery/RUN22").await.expect("ref"),
        saved
    );
    assert_eq!(
        f.repo
            .run(&["show", &format!("{saved}:leftover.txt")])
            .await
            .expect("show"),
        "untracked"
    );
    assert_eq!(
        f.repo
            .run(&["show", &format!("{saved}:sneaky.txt")])
            .await
            .expect("show"),
        "sneaky"
    );
    // ...and the worktree is back exactly as it was.
    assert_eq!(
        wt.current_branch().await.expect("branch").as_deref(),
        Some(s.branch.as_str())
    );
    assert_eq!(wt.head().await.expect("head"), before.head);
    assert!(wt.is_clean().await.expect("clean"));
    assert!(!wt.path().join("sneaky.txt").exists());
    assert!(
        f.repo
            .branch_tip("agent-branch")
            .await
            .expect("tip")
            .is_none(),
        "created branch removed"
    );
    assert!(
        f.repo
            .run(&["tag", "-l", "agent-tag"])
            .await
            .expect("tags")
            .is_empty(),
        "created tag removed"
    );
    assert!(wt.check_policy(&before).await.expect("check").is_ok());
}

#[tokio::test]
async fn quarantine_handles_a_commit_on_an_otherwise_clean_tree() {
    let f = fixture().await;
    let base = f.repo.head().await.expect("head");
    let s = spec(&f, "API-23", &base);
    f.repo.ensure_worktree(&s).await.expect("create");
    let wt = f.repo.at(s.path().expect("path"));
    let before = wt.snapshot().await.expect("snapshot");
    write(wt.path(), "x.txt", "x\n");
    wt.run(&["add", "-A"]).await.expect("add");
    wt.run(&["commit", "-q", "-m", "agent commit"])
        .await
        .expect("commit");
    let agent_head = wt.head().await.expect("head");

    let saved = wt.quarantine("RUN23", &before).await.expect("quarantine");
    assert_eq!(
        saved, agent_head,
        "a clean-tree commit is preserved as the recovery ref"
    );
    assert_eq!(wt.head().await.expect("head"), before.head);
}

#[tokio::test]
async fn tasks_are_found_by_trailer() {
    let f = fixture().await;
    let base = f.repo.head().await.expect("head");
    let s = spec(&f, "API-24", &base);
    f.repo.ensure_worktree(&s).await.expect("create");
    let wt = f.repo.at(s.path().expect("path"));
    write(wt.path(), "t1.txt", "1\n");
    let t = Trailers {
        job: "API-24".into(),
        task: Some("REV1/T1".into()),
        run: Some("R".into()),
        ..Trailers::default()
    };
    let sha = wt
        .commit_all("feat(API-24): t1", "", &t)
        .await
        .expect("c")
        .expect("sha");
    assert_eq!(
        f.repo
            .find_commit_by_task(&s.branch, "REV1/T1")
            .await
            .expect("probe"),
        Some(sha)
    );
    assert_eq!(
        f.repo
            .find_commit_by_task(&s.branch, "REV1/T2")
            .await
            .expect("probe"),
        None
    );
    assert_eq!(wt.tree_of("HEAD").await.expect("tree").len(), 40);
}
