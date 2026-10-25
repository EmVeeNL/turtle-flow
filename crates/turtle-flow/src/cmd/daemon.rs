//! `turtle daemon | up | down | status`. One daemon per data directory (FR-131), enforced with
//! an exclusive lock on `daemon.lock` that the operating system drops if the process dies.

use std::fs::{File, OpenOptions, TryLockError};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use turtle_agents::{AgentRunner, ClaudeConfig, ClaudeRunner, FakeRunner};
use turtle_scheduler::{CrashPlan, Dispatcher, DispatcherConfig, FactoryExecutor};

use crate::paths::Home;
use crate::runtime::Runtime;

fn lock(home: &Home) -> Result<File> {
    home.ensure()?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(home.daemon_lock())
        .context("opening the daemon lock")?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => {
            bail!("a daemon is already running for {}", home.root.display())
        }
        Err(TryLockError::Error(e)) => Err(e.into()),
    }
}

/// True when some process holds the daemon lock.
fn running(home: &Home) -> bool {
    let Ok(file) = OpenOptions::new().read(true).open(home.daemon_lock()) else {
        return false;
    };
    matches!(file.try_lock_shared(), Err(TryLockError::WouldBlock))
}

/// Which agent drives the jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum AgentChoice {
    /// Claude Code CLI, headless and isolated.
    Claude,
    /// Scripted placeholder agent that writes dummy files; for trying the workflow.
    Demo,
}

/// Settings for the Claude Code agent.
#[derive(Debug, Clone, Default)]
pub struct ClaudeOptions {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub max_run_usd: Option<f64>,
}

/// Runs the dispatcher in the foreground until interrupted.
pub async fn run(home: Home, agent: Option<AgentChoice>, claude: ClaudeOptions) -> Result<()> {
    let Some(agent) = agent else {
        bail!(
            "choose an agent: `--agent claude` runs Claude Code (it must be installed and logged in), \
             `--agent demo` runs a scripted placeholder so you can watch the workflow.\n\
             Codex arrives later in milestone M3."
        );
    };
    let _lock = lock(&home)?;
    let rt = Runtime::open(home.clone())?;
    std::fs::write(home.daemon_pid(), std::process::id().to_string())?;
    let result = match agent {
        AgentChoice::Demo => serve(&rt, FakeRunner::new(), "demo agent").await,
        AgentChoice::Claude => {
            let mut config = ClaudeConfig {
                model: claude.model,
                effort: claude.effort,
                ..ClaudeConfig::default()
            };
            if let Some(usd) = claude.max_run_usd {
                config.max_budget_usd = Some(usd);
            }
            serve(&rt, ClaudeRunner::new(config), "Claude Code").await
        }
    };
    let _ = std::fs::remove_file(home.daemon_pid());
    result?;
    eprintln!("turtle daemon stopped.");
    Ok(())
}

async fn serve<R: AgentRunner>(rt: &Runtime, runner: R, label: &str) -> Result<()> {
    let executor = Arc::new(FactoryExecutor::new(
        rt.store.clone(),
        Arc::clone(&rt.blobs),
        Arc::new(runner),
        rt.approvals.clone(),
        Arc::clone(&rt.cfg),
    ));
    let config = DispatcherConfig {
        poll_interval: Duration::from_millis(500),
        ..DispatcherConfig::default()
    };
    let (dispatcher, report) = Dispatcher::start(
        rt.store.clone(),
        Arc::clone(&rt.ctx),
        executor,
        config,
        CrashPlan::never(),
    )
    .await?;
    eprintln!("turtle daemon running ({label}). Recovery: {report:?}");

    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = stop_tx.send(true);
    });
    dispatcher.run(stop_rx).await?;
    Ok(())
}

pub async fn up(home: &Home, agent: Option<AgentChoice>, claude: &ClaudeOptions) -> Result<()> {
    home.ensure()?;
    if running(home) {
        println!("Already running.");
        return Ok(());
    }
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.daemon_log())?;
    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("daemon");
    if let Some(agent) = agent {
        cmd.args([
            "--agent",
            if agent == AgentChoice::Demo {
                "demo"
            } else {
                "claude"
            },
        ]);
    }
    if let Some(model) = &claude.model {
        cmd.args(["--claude-model", model]);
    }
    if let Some(effort) = &claude.effort {
        cmd.args(["--claude-effort", effort]);
    }
    if let Some(usd) = claude.max_run_usd {
        cmd.args(["--max-run-usd", &usd.to_string()]);
    }
    let child = cmd
        .env("TURTLE_HOME", &home.root)
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()
        .context("starting the daemon")?;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if running(home) {
            println!(
                "Started (pid {}). Log: {}",
                child.id(),
                home.daemon_log().display()
            );
            return Ok(());
        }
    }
    bail!(
        "the daemon did not start; see {}",
        home.daemon_log().display()
    )
}

pub fn down(home: &Home) -> Result<()> {
    if !running(home) {
        println!("Not running.");
        return Ok(());
    }
    let pid = std::fs::read_to_string(home.daemon_pid()).context("no pid file")?;
    let pid = pid.trim();
    if pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
        bail!("the pid file is corrupt");
    }
    let status = std::process::Command::new("kill")
        .args(["-INT", pid])
        .status()?;
    if !status.success() {
        bail!("could not signal process {pid}");
    }
    println!("Stopping (pid {pid}).");
    Ok(())
}

pub fn status(home: &Home) {
    if running(home) {
        let pid = std::fs::read_to_string(home.daemon_pid()).unwrap_or_default();
        println!("Daemon running (pid {}).", pid.trim());
    } else {
        println!("Daemon not running. Start it with: turtle up --agent claude");
    }
}
