//! The only crate that spawns OS processes (architecture invariant I-2).
//!
//! Every child runs with a cleared environment plus an explicit allowlist, in its own
//! process group, with a wall-clock timeout and capped output. On timeout the whole
//! process group is killed (SIGTERM, then SIGKILL after a grace period).

use std::collections::HashMap;
use std::ffi::OsString;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[cfg(unix)]
use nix::sys::signal::{Signal, kill, killpg};
#[cfg(unix)]
use nix::unistd::Pid;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::watch;

static RUN_SIGNALS: OnceLock<Mutex<HashMap<String, watch::Sender<bool>>>> = OnceLock::new();
type SpawnHook = std::sync::Arc<
    dyn Fn(u32) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>> + Send + Sync,
>;
static SPAWN_HOOKS: OnceLock<Mutex<HashMap<String, SpawnHook>>> = OnceLock::new();

#[derive(Debug)]
pub struct SpawnHookGuard(String);

impl Drop for SpawnHookGuard {
    fn drop(&mut self) {
        SPAWN_HOOKS
            .get_or_init(Default::default)
            .lock()
            .expect("spawn hooks lock")
            .remove(&self.0);
    }
}

/// Register a callback that durably records the process group before its prompt is sent.
pub fn install_spawn_hook(key: String, hook: SpawnHook) -> SpawnHookGuard {
    SPAWN_HOOKS
        .get_or_init(Default::default)
        .lock()
        .expect("spawn hooks lock")
        .insert(key.clone(), hook);
    SpawnHookGuard(key)
}

fn signals() -> &'static Mutex<HashMap<String, watch::Sender<bool>>> {
    RUN_SIGNALS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Signal every process in a job's current and subsequently starting run.
pub fn cancel_job(key: &str) {
    let mut signals = signals().lock().expect("run signals lock");
    let sender = signals
        .entry(key.to_owned())
        .or_insert_with(|| watch::channel(false).0);
    sender.send_replace(true);
}

/// Allow a resumed job to start a new run. Existing receivers keep their cancellation signal.
pub fn resume_job(key: &str) {
    signals().lock().expect("run signals lock").remove(key);
}

/// Variables always passed through when present (SECURITY.md §6.2). Everything else is dropped.
pub const BASE_ENV_ALLOWLIST: &[&str] = &["PATH", "HOME", "USER", "LANG", "LC_ALL", "TMPDIR"];

/// Grace period between SIGTERM and SIGKILL.
pub const KILL_GRACE: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
pub struct CommandSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    /// Names of variables to inherit from the current environment (in addition to the base list).
    pub inherit_env: Vec<String>,
    /// Explicit variables to set.
    pub set_env: Vec<(String, String)>,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Duration,
    /// Per stream; output beyond this is discarded and `truncated` is set.
    pub max_output_bytes: usize,
    /// Job key used by the run supervisor for hard-pause cancellation.
    pub job_key: Option<String>,
    pub kill_grace: Duration,
    pub spool_stdout: Option<PathBuf>,
}

impl CommandSpec {
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            inherit_env: Vec::new(),
            set_env: Vec::new(),
            stdin: None,
            timeout: Duration::from_secs(60),
            max_output_bytes: 8 * 1024 * 1024,
            job_key: None,
            kill_grace: KILL_GRACE,
            spool_stdout: None,
        }
    }

    #[must_use]
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    #[must_use]
    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

    #[must_use]
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.set_env.push((key.into(), value.into()));
        self
    }

    #[must_use]
    pub fn stdin(mut self, input: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(input.into());
        self
    }

    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    #[must_use]
    pub const fn max_output_bytes(mut self, bytes: usize) -> Self {
        self.max_output_bytes = bytes;
        self
    }

    #[must_use]
    pub fn job_key(mut self, key: impl Into<String>) -> Self {
        self.job_key = Some(key.into());
        self
    }

    #[must_use]
    pub const fn kill_grace(mut self, grace: Duration) -> Self {
        self.kill_grace = grace;
        self
    }

    #[must_use]
    pub fn spool_stdout(mut self, path: impl Into<PathBuf>) -> Self {
        self.spool_stdout = Some(path.into());
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitOutcome {
    Exited(i32),
    Signaled,
    TimedOut,
    Cancelled,
}

#[derive(Clone, Debug)]
pub struct Output {
    pub outcome: ExitOutcome,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub truncated: bool,
    pub duration: Duration,
}

impl Output {
    #[must_use]
    pub const fn success(&self) -> bool {
        matches!(self.outcome, ExitOutcome::Exited(0))
    }

    #[must_use]
    pub fn stdout_lossy(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("program not found: {0}")]
    NotFound(String),
    #[error("failed to spawn {program}: {source}")]
    Spawn {
        program: String,
        source: std::io::Error,
    },
    #[error("io error while running {program}: {source}")]
    Io {
        program: String,
        source: std::io::Error,
    },
    #[error("failed to register process for {program}: {reason}")]
    Registration { program: String, reason: String },
}

#[cfg(unix)]
struct ProcessGuard(Option<Pid>);

#[cfg(unix)]
impl Drop for ProcessGuard {
    fn drop(&mut self) {
        if let Some(pgid) = self.0 {
            let _ = killpg(pgid, Signal::SIGKILL);
        }
    }
}

/// Runs a command to completion (or timeout) and captures capped output.
pub async fn run(spec: CommandSpec) -> Result<Output, ExecError> {
    let program = spec.program.to_string_lossy().into_owned();
    let started = Instant::now();

    let hook = spec.job_key.as_ref().and_then(|key| {
        SPAWN_HOOKS
            .get_or_init(Default::default)
            .lock()
            .expect("spawn hooks lock")
            .get(key)
            .cloned()
    });
    #[cfg(unix)]
    let supervised = hook.is_some();
    #[cfg(not(unix))]
    let supervised = false;
    let spool_file = spec
        .spool_stdout
        .as_ref()
        .map(|path| {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options.open(path)
        })
        .transpose()
        .map_err(|source| ExecError::Io {
            program: program.clone(),
            source,
        })?;

    #[cfg(unix)]
    let mut cmd = if supervised {
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "IFS= read -r _turtle_ready || exit 0; exec \"$@\"",
                "turtle-supervisor",
            ])
            .arg(&spec.program)
            .args(&spec.args);
        command
    } else {
        let mut command = Command::new(&spec.program);
        command.args(&spec.args);
        command
    };
    #[cfg(not(unix))]
    let mut cmd = Command::new(&spec.program);
    #[cfg(not(unix))]
    cmd.args(&spec.args);
    cmd.env_clear()
        .stdin(if supervised || spec.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    for name in BASE_ENV_ALLOWLIST
        .iter()
        .copied()
        .chain(spec.inherit_env.iter().map(String::as_str))
    {
        if let Some(value) = std::env::var_os(name) {
            cmd.env(name, value);
        }
    }
    for (k, v) in &spec.set_env {
        cmd.env(k, v);
    }
    if let Some(dir) = &spec.cwd {
        cmd.current_dir(dir);
    }

    let mut child = cmd.spawn().map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            ExecError::NotFound(program.clone())
        } else {
            ExecError::Spawn {
                program: program.clone(),
                source,
            }
        }
    })?;
    #[cfg(unix)]
    let pgid = child.id().map(|id| Pid::from_raw(id as i32));
    #[cfg(unix)]
    let guard = ProcessGuard(pgid);
    #[cfg(not(unix))]
    let pgid = child.id();
    let mut cancel = spec.job_key.as_ref().map(|key| {
        let mut signals = signals().lock().expect("run signals lock");
        signals
            .entry(key.clone())
            .or_insert_with(|| watch::channel(false).0)
            .subscribe()
    });
    if let Some(pid) = child.id()
        && let Some(hook) = hook
        && let Err(reason) = hook(pid).await
    {
        terminate_group(pgid, &mut child, Duration::ZERO).await;
        return Err(ExecError::Registration { program, reason });
    }

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let cap = spec.max_output_bytes;
    let out_task = tokio::spawn(read_capped(stdout, cap, spool_file));
    let err_task = tokio::spawn(read_capped(stderr, cap, None));
    tokio::task::yield_now().await;

    if let Some(mut stdin) = child.stdin.take() {
        // The supervised child waits for this line, so it cannot exec before PID registration.
        if supervised {
            let _ = stdin.write_all(b"ready\n").await;
        }
        if let Some(input) = &spec.stdin {
            let _ = stdin.write_all(input).await;
        }
        drop(stdin);
    }

    let outcome = tokio::select! {
        biased;
        () = async {
            if let Some(receiver) = &mut cancel {
                while !*receiver.borrow() && receiver.changed().await.is_ok() {}
            } else {
                std::future::pending::<()>().await;
            }
        } => {
            terminate_group(pgid, &mut child, spec.kill_grace).await;
            ExitOutcome::Cancelled
        }
        status = child.wait() => status
            .map_err(|source| ExecError::Io { program: program.clone(), source })?
            .code()
            .map_or(ExitOutcome::Signaled, ExitOutcome::Exited),
        () = tokio::time::sleep(spec.timeout) => {
            terminate_group(pgid, &mut child, spec.kill_grace).await;
            ExitOutcome::TimedOut
        }
    };

    #[cfg(unix)]
    if let Some(pgid) = pgid {
        let _ = killpg(pgid, Signal::SIGKILL);
    }

    let (stdout, out_trunc) = out_task
        .await
        .map_err(std::io::Error::other)
        .and_then(|result| result)
        .map_err(|source| ExecError::Io {
            program: program.clone(),
            source,
        })?;
    let (stderr, err_trunc) = err_task
        .await
        .map_err(std::io::Error::other)
        .and_then(|result| result)
        .map_err(|source| ExecError::Io {
            program: program.clone(),
            source,
        })?;
    #[cfg(unix)]
    drop(guard);
    Ok(Output {
        outcome,
        stdout,
        stderr,
        truncated: out_trunc || err_trunc,
        duration: started.elapsed(),
    })
}

#[cfg(unix)]
async fn terminate_group(pgid: Option<Pid>, child: &mut tokio::process::Child, grace: Duration) {
    if let Some(pgid) = pgid {
        let started = Instant::now();
        let _ = killpg(pgid, Signal::SIGTERM);
        // The leader may exit before a tool it spawned. Escalate for the whole group anyway.
        let _ = tokio::time::timeout(grace, child.wait()).await;
        if kill(Pid::from_raw(-pgid.as_raw()), None).is_ok() {
            tokio::time::sleep(grace.saturating_sub(started.elapsed())).await;
            let _ = killpg(pgid, Signal::SIGKILL);
        }
    }
    let _ = child.wait().await;
}

/// Stop a process group left by a daemon crash before any effects are re-dispatched.
#[cfg(unix)]
pub async fn terminate_orphan_group(pid: u32, grace: Duration) {
    let Ok(raw) = i32::try_from(pid) else { return };
    let pgid = Pid::from_raw(raw);
    if killpg(pgid, Signal::SIGTERM).is_err() {
        return;
    }
    let started = Instant::now();
    while started.elapsed() < grace {
        if kill(Pid::from_raw(-raw), None).is_err() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50).min(grace.saturating_sub(started.elapsed())))
            .await;
    }
    let _ = killpg(pgid, Signal::SIGKILL);
}

#[cfg(not(unix))]
async fn terminate_group(_pgid: Option<u32>, child: &mut tokio::process::Child, _grace: Duration) {
    let _ = child.kill().await;
}

async fn read_capped<R: AsyncRead + Unpin>(
    reader: Option<R>,
    cap: usize,
    spool: Option<std::fs::File>,
) -> std::io::Result<(Vec<u8>, bool)> {
    let Some(mut reader) = reader else {
        return Ok((Vec::new(), false));
    };
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut truncated = false;
    let mut file = spool.map(tokio::fs::File::from_std);
    loop {
        match reader.read(&mut chunk).await? {
            0 => break,
            n => {
                let room = cap.saturating_sub(buf.len());
                if room < n {
                    truncated = true;
                }
                buf.extend_from_slice(&chunk[..n.min(room)]);
                if let Some(file) = &mut file {
                    file.write_all(&chunk[..n.min(room)]).await?;
                    file.sync_data().await?;
                }
            }
        }
    }
    Ok((buf, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn captures_stdout_and_exit_code() {
        let out = run(CommandSpec::new("sh").args(["-c", "echo hi; exit 3"]))
            .await
            .expect("runs");
        assert_eq!(out.outcome, ExitOutcome::Exited(3));
        assert_eq!(out.stdout_lossy(), "hi\n");
        assert!(!out.truncated);
    }

    #[tokio::test]
    async fn environment_is_scrubbed() {
        // A variable that is not allowlisted must not reach the child.
        let out = run(CommandSpec::new("sh")
            .args([
                "-c",
                "echo \"${TURTLE_TEST_SECRET:-absent} ${TURTLE_SET:-unset}\"",
            ])
            .env("TURTLE_SET", "set"))
        .await
        .expect("runs");
        assert_eq!(out.stdout_lossy(), "absent set\n");
    }

    #[tokio::test]
    async fn passes_stdin() {
        let out = run(CommandSpec::new("cat").stdin(b"from stdin".to_vec()))
            .await
            .expect("runs");
        assert_eq!(out.stdout_lossy(), "from stdin");
    }

    #[tokio::test]
    async fn timeout_kills_process_group() {
        let started = Instant::now();
        let out = run(CommandSpec::new("sh")
            .args(["-c", "sleep 30 & sleep 30"])
            .timeout(Duration::from_millis(300)))
        .await
        .expect("runs");
        assert_eq!(out.outcome, ExitOutcome::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[tokio::test]
    async fn output_is_capped() {
        let out = run(CommandSpec::new("sh")
            .args(["-c", "yes | head -c 100000"])
            .max_output_bytes(1000))
        .await
        .expect("runs");
        assert_eq!(out.stdout.len(), 1000);
        assert!(out.truncated);
    }

    #[tokio::test]
    async fn unbounded_stream_is_capped_and_terminated_by_timeout() {
        let out = run(CommandSpec::new("sh")
            .args(["-c", "yes"])
            .timeout(Duration::from_millis(300))
            .max_output_bytes(1024))
        .await
        .expect("runs");
        assert_eq!(out.outcome, ExitOutcome::TimedOut);
        assert_eq!(out.stdout.len(), 1024);
        assert!(out.truncated);
    }

    #[tokio::test]
    async fn missing_program_is_not_found() {
        let err = run(CommandSpec::new("turtle-definitely-missing-binary"))
            .await
            .expect_err("missing");
        assert!(matches!(err, ExecError::NotFound(_)));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn hard_pause_kills_the_grandchild() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pid_file = dir.path().join("grandchild.pid");
        let heartbeat = dir.path().join("heartbeat");
        let key = format!("grandchild-{}", std::process::id());
        let command = format!(
            "(while :; do printf . >> '{}'; sleep 0.02; done) & echo $! > '{}'; wait",
            heartbeat.display(),
            pid_file.display()
        );
        let mut task = tokio::spawn(run(CommandSpec::new("sh")
            .args(["-c", &command])
            .job_key(key.clone())
            .kill_grace(Duration::from_millis(100))));
        for _ in 0..100 {
            if pid_file.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let _: i32 = std::fs::read_to_string(&pid_file)
            .expect("grandchild pid")
            .trim()
            .parse()
            .expect("pid");
        cancel_job(&key);
        let out = match tokio::time::timeout(Duration::from_secs(3), &mut task).await {
            Ok(joined) => joined.expect("joined").expect("run"),
            Err(_) => {
                task.abort();
                panic!("cancel exceeded bound");
            }
        };
        assert_eq!(out.outcome, ExitOutcome::Cancelled);
        tokio::time::sleep(Duration::from_millis(100)).await;
        let before = std::fs::metadata(&heartbeat).expect("heartbeat").len();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let after = std::fs::metadata(&heartbeat).expect("heartbeat").len();
        assert_eq!(before, after, "grandchild kept writing after cancellation");
        resume_job(&key);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn hard_pause_escalates_after_grace() {
        let key = format!("ignores-term-{}", std::process::id());
        let mut task = tokio::spawn(run(CommandSpec::new("sh")
            .args(["-c", "trap '' TERM; echo ready; while :; do :; done"])
            .job_key(key.clone())
            .kill_grace(Duration::from_millis(150))));
        tokio::time::sleep(Duration::from_millis(50)).await;
        let start = Instant::now();
        cancel_job(&key);
        let out = match tokio::time::timeout(Duration::from_secs(3), &mut task).await {
            Ok(joined) => joined.expect("joined").expect("run"),
            Err(_) => {
                task.abort();
                panic!("cancel exceeded bound");
            }
        };
        assert_eq!(out.outcome, ExitOutcome::Cancelled);
        assert!(start.elapsed() >= Duration::from_millis(150));
        assert!(start.elapsed() < Duration::from_secs(3));
        resume_job(&key);
    }
}
