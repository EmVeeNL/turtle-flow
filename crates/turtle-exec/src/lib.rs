//! The only crate that spawns OS processes (architecture invariant I-2).
//!
//! Every child runs with a cleared environment plus an explicit allowlist, in its own
//! process group, with a wall-clock timeout and capped output. On timeout the whole
//! process group is killed (SIGTERM, then SIGKILL after a grace period).

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitOutcome {
    Exited(i32),
    Signaled,
    TimedOut,
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
}

/// Runs a command to completion (or timeout) and captures capped output.
pub async fn run(spec: CommandSpec) -> Result<Output, ExecError> {
    let program = spec.program.to_string_lossy().into_owned();
    let started = Instant::now();

    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .env_clear()
        .stdin(if spec.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true);
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
    let pgid = child.id().map(|id| Pid::from_raw(id as i32));

    if let (Some(input), Some(mut stdin)) = (spec.stdin.clone(), child.stdin.take()) {
        // Ignore broken pipes: the child may exit without reading stdin.
        let _ = stdin.write_all(&input).await;
        drop(stdin);
    }

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let cap = spec.max_output_bytes;
    let out_task = tokio::spawn(read_capped(stdout, cap));
    let err_task = tokio::spawn(read_capped(stderr, cap));

    let outcome = match tokio::time::timeout(spec.timeout, child.wait()).await {
        Ok(Ok(status)) => status
            .code()
            .map_or(ExitOutcome::Signaled, ExitOutcome::Exited),
        Ok(Err(source)) => return Err(ExecError::Io { program, source }),
        Err(_elapsed) => {
            terminate_group(pgid, &mut child).await;
            ExitOutcome::TimedOut
        }
    };

    let (stdout, out_trunc) = out_task.await.unwrap_or_default();
    let (stderr, err_trunc) = err_task.await.unwrap_or_default();
    Ok(Output {
        outcome,
        stdout,
        stderr,
        truncated: out_trunc || err_trunc,
        duration: started.elapsed(),
    })
}

async fn terminate_group(pgid: Option<Pid>, child: &mut tokio::process::Child) {
    if let Some(pgid) = pgid {
        let _ = killpg(pgid, Signal::SIGTERM);
        if tokio::time::timeout(KILL_GRACE, child.wait()).await.is_ok() {
            return;
        }
        let _ = killpg(pgid, Signal::SIGKILL);
    }
    let _ = child.kill().await;
}

async fn read_capped<R: AsyncRead + Unpin>(reader: Option<R>, cap: usize) -> (Vec<u8>, bool) {
    let Some(mut reader) = reader else {
        return (Vec::new(), false);
    };
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut truncated = false;
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let room = cap.saturating_sub(buf.len());
                if room < n {
                    truncated = true;
                }
                buf.extend_from_slice(&chunk[..n.min(room)]);
            }
        }
    }
    (buf, truncated)
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
}
