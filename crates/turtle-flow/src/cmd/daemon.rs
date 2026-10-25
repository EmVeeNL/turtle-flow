//! `turtle daemon | up | down | status`. One daemon per data directory (FR-131), enforced with
//! an exclusive lock on `daemon.lock` that the operating system drops if the process dies.

use std::fs::{File, OpenOptions, TryLockError};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use turtle_agents::FakeRunner;
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

/// Runs the dispatcher in the foreground until interrupted.
pub async fn run(home: Home, demo_agent: bool) -> Result<()> {
    if !demo_agent {
        bail!(
            "no real agent adapter exists yet (Claude Code arrives in milestone M2).\n\
             Run `turtle daemon --demo-agent` to drive jobs with the scripted demo agent, which \
             writes placeholder files so you can watch the whole workflow."
        );
    }
    let _lock = lock(&home)?;
    let rt = Runtime::open(home.clone())?;
    std::fs::write(home.daemon_pid(), std::process::id().to_string())?;

    let executor = Arc::new(FactoryExecutor::new(
        rt.store.clone(),
        Arc::clone(&rt.blobs),
        Arc::new(FakeRunner::new()),
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
    eprintln!("turtle daemon running (demo agent). Recovery: {report:?}");

    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = stop_tx.send(true);
    });
    let result = dispatcher.run(stop_rx).await;
    let _ = std::fs::remove_file(home.daemon_pid());
    result?;
    eprintln!("turtle daemon stopped.");
    Ok(())
}

pub async fn up(home: &Home, demo_agent: bool) -> Result<()> {
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
    if demo_agent {
        cmd.arg("--demo-agent");
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
        println!("Daemon not running. Start it with: turtle up --demo-agent");
    }
}
