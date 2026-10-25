//! `turtle daemon | up | down | status`. One daemon per data directory (FR-131), enforced with
//! an exclusive lock on `daemon.lock` that the operating system drops if the process dies.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::IsTerminal;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use turtle_agents::{
    AgentRunner, CapabilityRegistry, ClaudeConfig, ClaudeRunner, CodexConfig, CodexRunner,
    FakeRunner, ProviderId, ProviderSlot, RoutingRunner,
};
use turtle_api::{ApiServer, Services};
use turtle_domain::Role;
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

/// Daemon provider selection and per-provider settings.
#[derive(Debug, Clone, Default)]
pub struct DaemonOptions {
    pub no_api: bool,
    pub api_port: Option<u16>,
    pub agent: Option<String>,
    pub role_agent: Vec<String>,
    pub claude_model: Option<String>,
    pub claude_effort: Option<String>,
    pub max_run_usd: Option<f64>,
    pub codex_model: Option<String>,
    pub codex_effort: Option<String>,
}

fn parse_role(name: &str) -> Result<Role> {
    match name {
        "discovery" => Ok(Role::Discovery),
        "spec_writer" => Ok(Role::SpecWriter),
        "planner" => Ok(Role::Planner),
        "implementer" => Ok(Role::Implementer),
        "fixer" => Ok(Role::Fixer),
        "reviewer" => Ok(Role::Reviewer),
        "verifier" => Ok(Role::Verifier),
        "summarizer" => Ok(Role::Summarizer),
        "release_notes" => Ok(Role::ReleaseNotes),
        _ => bail!("unknown role `{name}` in --role-agent"),
    }
}

fn parse_agents(options: &DaemonOptions) -> Result<Vec<&str>> {
    let Some(agent) = options.agent.as_deref() else {
        bail!(
            "choose an agent: `--agent claude`, `--agent codex`, `--agent claude,codex`, or `--agent demo`"
        );
    };
    let agents: Vec<_> = agent.split(',').collect();
    if agents.is_empty()
        || agents.len() > 2
        || agents
            .iter()
            .any(|p| !matches!(*p, "claude" | "codex" | "demo"))
    {
        bail!("--agent must be claude, codex, claude,codex, codex,claude, or demo");
    }
    if agents.contains(&"demo") && agents.len() != 1 || agents.len() == 2 && agents[0] == agents[1]
    {
        bail!("--agent needs distinct real providers, or demo alone");
    }
    if !options.role_agent.is_empty() && agents.len() != 2 {
        bail!("--role-agent requires two providers in --agent");
    }
    for assignment in &options.role_agent {
        let Some((role, provider)) = assignment.split_once('=') else {
            bail!("--role-agent needs role=provider")
        };
        parse_role(role)?;
        if !agents.contains(&provider) {
            bail!("provider `{provider}` in --role-agent is not in --agent")
        }
    }
    Ok(agents)
}

/// Runs the dispatcher in the foreground until interrupted.
pub async fn run(home: Home, options: DaemonOptions) -> Result<()> {
    let agents = parse_agents(&options)?;
    let mut claude_config = ClaudeConfig {
        model: options.claude_model.clone(),
        effort: options.claude_effort.clone(),
        ..ClaudeConfig::default()
    };
    if let Some(usd) = options.max_run_usd {
        claude_config.max_budget_usd = Some(usd);
    }
    let codex_config = CodexConfig {
        model: options.codex_model.clone(),
        effort: options.codex_effort.clone(),
        ..CodexConfig::default()
    };
    let registry = if agents[0] == "demo" {
        None
    } else {
        Some(CapabilityRegistry::discover().await)
    };
    let mut availability = std::collections::HashMap::new();
    for agent in &agents {
        let provider = match *agent {
            "claude" => ProviderId::ClaudeCode,
            "codex" => ProviderId::Codex,
            _ => continue,
        };
        let caps = registry
            .as_ref()
            .expect("real providers have registry")
            .get(provider);
        availability.insert(*agent, caps.exclusion_reason());
    }
    if agents
        .iter()
        .filter(|agent| **agent != "demo")
        .all(|agent| availability.get(agent).is_some_and(Option::is_some))
        && agents[0] != "demo"
    {
        bail!(
            "no configured provider is available: {}",
            agents
                .iter()
                .map(|agent| format!(
                    "{agent}: {}",
                    availability
                        .get(agent)
                        .and_then(Option::as_deref)
                        .unwrap_or("unknown")
                ))
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    if agents.len() == 1
        && let Some(Some(reason)) = availability.get(agents[0])
    {
        bail!("{} is unavailable: {reason}", agents[0]);
    }
    let _lock = lock(&home)?;
    let _ = std::fs::remove_file(home.root.join("run/api.port"));
    let rt = Runtime::open(home.clone())?;
    std::fs::write(home.daemon_pid(), std::process::id().to_string())?;
    let result = match agents.as_slice() {
        ["demo"] => serve(&rt, &home, &options, FakeRunner::new(), "demo agent").await,
        ["claude"] => {
            serve(
                &rt,
                &home,
                &options,
                ClaudeRunner::new(claude_config),
                "Claude Code",
            )
            .await
        }
        ["codex"] => {
            serve(
                &rt,
                &home,
                &options,
                CodexRunner::new(codex_config),
                "Codex",
            )
            .await
        }
        ["claude", "codex"] => {
            let mut router = RoutingRunner::new(
                ProviderSlot::from_capabilities(
                    ClaudeRunner::new(claude_config),
                    registry
                        .as_ref()
                        .expect("registry")
                        .get(ProviderId::ClaudeCode),
                ),
                ProviderSlot::from_capabilities(
                    CodexRunner::new(codex_config),
                    registry.as_ref().expect("registry").get(ProviderId::Codex),
                ),
            );
            for assignment in &options.role_agent {
                let (role, provider) = assignment.split_once('=').expect("validated assignment");
                router = router.with_role_default(
                    parse_role(role)?,
                    if provider == "claude" {
                        "claude-code"
                    } else {
                        "codex"
                    },
                );
            }
            serve(&rt, &home, &options, router, "Claude Code, Codex").await
        }
        ["codex", "claude"] => {
            let mut router = RoutingRunner::new(
                ProviderSlot::from_capabilities(
                    CodexRunner::new(codex_config),
                    registry.as_ref().expect("registry").get(ProviderId::Codex),
                ),
                ProviderSlot::from_capabilities(
                    ClaudeRunner::new(claude_config),
                    registry
                        .as_ref()
                        .expect("registry")
                        .get(ProviderId::ClaudeCode),
                ),
            );
            for assignment in &options.role_agent {
                let (role, provider) = assignment.split_once('=').expect("validated assignment");
                router = router.with_role_default(
                    parse_role(role)?,
                    if provider == "claude" {
                        "claude-code"
                    } else {
                        "codex"
                    },
                );
            }
            serve(&rt, &home, &options, router, "Codex, Claude Code").await
        }
        _ => unreachable!("parse_agents validated the list"),
    };
    let _ = std::fs::remove_file(home.daemon_pid());
    result?;
    eprintln!("turtle daemon stopped.");
    Ok(())
}

async fn serve<R: AgentRunner>(
    rt: &Runtime,
    home: &Home,
    options: &DaemonOptions,
    runner: R,
    label: &str,
) -> Result<()> {
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

    let api = if options.no_api {
        None
    } else {
        let source_dist =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dashboard/dist");
        let dashboard_dir = std::env::var_os("TURTLE_DASHBOARD_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| source_dist.exists().then_some(source_dist));
        let services = Services {
            store: rt.store.clone(),
            engine: rt.engine.clone(),
            approvals: rt.approvals.clone(),
            blobs: Arc::clone(&rt.blobs),
        };
        let api = ApiServer::bind(
            services,
            &home.root.join("run"),
            dashboard_dir,
            options.api_port.unwrap_or(0),
        )
        .await
        .map_err(|e| anyhow::anyhow!("starting local API: {e}"))?;
        eprintln!("turtle API listening at http://127.0.0.1:{}", api.port());
        Some(api)
    };

    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let signal_tx = stop_tx.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = signal_tx.send(true);
    });
    let api_task = api.map(|api| tokio::spawn(api.run(stop_rx.clone())));
    let result = dispatcher.run(stop_rx).await;
    let _ = stop_tx.send(true);
    if let Some(task) = api_task {
        task.await??;
    }
    result?;
    Ok(())
}

pub async fn up(home: &Home, options: &DaemonOptions) -> Result<()> {
    parse_agents(options)?;
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
    if options.no_api {
        cmd.arg("--no-api");
    }
    if let Some(port) = options.api_port {
        cmd.args(["--api-port", &port.to_string()]);
    }
    if let Some(agent) = &options.agent {
        cmd.args(["--agent", agent]);
    }
    for assignment in &options.role_agent {
        cmd.args(["--role-agent", assignment]);
    }
    if let Some(model) = &options.claude_model {
        cmd.args(["--claude-model", model]);
    }
    if let Some(effort) = &options.claude_effort {
        cmd.args(["--claude-effort", effort]);
    }
    if let Some(usd) = options.max_run_usd {
        cmd.args(["--max-run-usd", &usd.to_string()]);
    }
    if let Some(model) = &options.codex_model {
        cmd.args(["--codex-model", model]);
    }
    if let Some(effort) = &options.codex_effort {
        cmd.args(["--codex-effort", effort]);
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
        if running(home) && (options.no_api || home.root.join("run/api.port").exists()) {
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

pub async fn status(home: &Home) -> Result<()> {
    if running(home) {
        let pid = std::fs::read_to_string(home.daemon_pid()).unwrap_or_default();
        println!("Daemon running (pid {}).", pid.trim());
        if let Ok(port) = std::fs::read_to_string(home.root.join("run/api.port")) {
            println!("API: http://127.0.0.1:{}", port.trim());
        } else {
            println!("API: disabled or starting.");
        }
    } else {
        println!("Daemon not running. Start it with: turtle up --agent claude");
    }
    if home.db().exists() {
        let runtime = Runtime::open(home.clone())?;
        let limited = runtime
            .store
            .with(|store| store.current_limited_providers())
            .await?;
        if limited.is_empty() {
            println!("No providers are rate limited.");
        } else {
            for item in limited {
                println!(
                    "Provider {} is rate limited{}.",
                    item.provider,
                    item.reset_at
                        .as_deref()
                        .map_or_else(|| " (reset unknown)".into(), |v| format!(" until {}", v))
                );
            }
        }
    }
    Ok(())
}

pub async fn dashboard(home: &Home) -> Result<()> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        bail!("turtle dashboard requires an interactive terminal");
    }
    if !running(home) {
        bail!("the daemon is not running; start it with turtle up --agent demo");
    }
    let port = std::fs::read_to_string(home.root.join("run/api.port"))
        .context("the daemon API is not running; restart without --no-api")?;
    let port: u16 = port
        .trim()
        .parse()
        .context("the API port file is invalid")?;
    let rt = Runtime::open(home.clone())?;
    let code = turtle_api::issue_login_code(&rt.store)
        .await
        .map_err(|e| anyhow::anyhow!("creating dashboard login code: {e}"))?;
    let url = format!("http://127.0.0.1:{port}/login#code={code}");
    let launcher = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    match std::process::Command::new(launcher).arg(&url).status() {
        Ok(status) if status.success() => println!("Dashboard opened."),
        _ => println!("Open this URL within 60 seconds: {url}"),
    }
    Ok(())
}
