//! `turtle daemon | up | down | status`. One daemon per data directory (FR-131), enforced with
//! an exclusive lock on `daemon.lock` that the operating system drops if the process dies.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{IsTerminal, Read, Write};
use std::net::TcpStream;
use std::process::Command;
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

fn tailscale_binary() -> Command {
    Command::new("tailscale")
}

fn resolve_tailnet_host(explicit: Option<&str>) -> Result<Option<String>> {
    resolve_tailnet_host_with(explicit, std::ffi::OsStr::new("tailscale"))
}

fn resolve_tailnet_host_with(
    explicit: Option<&str>,
    program: &std::ffi::OsStr,
) -> Result<Option<String>> {
    if let Some(host) = explicit {
        return turtle_api::validate_tailnet_host(host)
            .map(Some)
            .map_err(anyhow::Error::msg);
    }
    let output = match Command::new(program).args(["status", "--json"]).output() {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("running tailscale status --json"),
    };
    if !output.status.success() {
        return Ok(None);
    }
    let Ok(status) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        return Ok(None);
    };
    let Some(dns_name) = status
        .pointer("/Self/DNSName")
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(None);
    };
    turtle_api::validate_tailnet_host(dns_name)
        .map(Some)
        .map_err(anyhow::Error::msg)
}

fn serve_command_args(port: u16) -> Vec<String> {
    vec![
        "serve".into(),
        "--bg".into(),
        "--https=443".into(),
        format!("http://127.0.0.1:{port}"),
    ]
}

fn configure_tailscale_serve(program: &std::ffi::OsStr, port: u16) -> Result<std::process::Output> {
    Command::new(program)
        .args(serve_command_args(port))
        .output()
        .context("configuring Tailscale Serve")
}

/// Daemon provider selection and per-provider settings.
#[derive(Debug, Clone, Default)]
pub struct DaemonOptions {
    pub no_api: bool,
    pub api_port: Option<u16>,
    pub tailnet_host: Option<String>,
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
        let tailnet_host = resolve_tailnet_host(options.tailnet_host.as_deref())?;
        let api = ApiServer::bind(
            services,
            &home.root.join("run"),
            dashboard_dir,
            options.api_port.unwrap_or(0),
            tailnet_host,
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
    if let Some(host) = &options.tailnet_host {
        cmd.args(["--tailnet-host", host]);
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

pub async fn dashboard(home: &Home, phone: bool) -> Result<()> {
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
    if phone {
        return dashboard_phone(port, &rt).await;
    }
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

async fn dashboard_phone(port: u16, rt: &Runtime) -> Result<()> {
    let host = resolve_tailnet_host(None)?.context(
        "Tailscale is not connected or its DNS name could not be detected; connect Tailscale and retry, or start the daemon with --tailnet-host <name>",
    )?;
    if !daemon_allows_tailnet(port, &host)? {
        bail!(
            "the running daemon does not allow {host}; stop it and restart with `turtle up --agent <agent> --tailnet-host {host}` (or connect Tailscale before restarting to enable auto-detection)"
        );
    }
    let mut tailscale = tailscale_binary();
    let help = tailscale
        .args(["serve", "--help"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
    if let Some(help) = &help
        && (!help.contains("--bg") || !help.contains("--https"))
    {
        bail!(
            "installed `tailscale serve --help` does not document the required --bg and --https flags"
        );
    }
    let status = tailscale_binary()
        .args(["serve", "status"])
        .output()
        .context("running `tailscale serve status`")?;
    if !status.status.success() {
        bail!(
            "`tailscale serve status` failed: {}",
            String::from_utf8_lossy(&status.stderr).trim()
        );
    }
    let mapping = format!("http://127.0.0.1:{port}");
    let status_text = format!(
        "{}{}",
        String::from_utf8_lossy(&status.stdout),
        String::from_utf8_lossy(&status.stderr)
    );
    let root_mapping = format!("|-- / proxy {mapping}");
    if !status_text.contains(&format!("https://{host}")) || !status_text.contains(&root_mapping) {
        eprintln!("No Tailscale Serve mapping to {mapping} was found.");
        eprint!("Add the private HTTPS mapping on this device? [y/N] ");
        std::io::stderr().flush()?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            bail!("Tailscale configuration was not changed");
        }
        let result = configure_tailscale_serve(std::ffi::OsStr::new("tailscale"), port)?;
        if !result.status.success() {
            bail!(
                "Tailscale Serve setup failed: {}",
                String::from_utf8_lossy(&result.stderr).trim()
            );
        }
        let configured = tailscale_binary()
            .args(["serve", "status"])
            .output()
            .context("verifying Tailscale Serve setup")?;
        let configured_text = format!(
            "{}{}",
            String::from_utf8_lossy(&configured.stdout),
            String::from_utf8_lossy(&configured.stderr)
        );
        if !configured.status.success()
            || !configured_text.contains(&format!("https://{host}"))
            || !configured_text.contains(&root_mapping)
        {
            bail!("Tailscale Serve did not report the expected HTTPS root mapping to {mapping}");
        }
    }
    let code = turtle_api::issue_read_only_login_code(&rt.store)
        .await
        .map_err(|e| anyhow::anyhow!("creating read-only dashboard login code: {e}"))?;
    let url = format!("https://{host}/login#code={code}");
    let qr = qrcode::QrCode::new(url.as_bytes()).context("generating phone QR code")?;
    println!(
        "{}",
        qr.render::<qrcode::render::unicode::Dense1x2>()
            .quiet_zone(true)
            .build()
    );
    println!("{url}");
    println!("Login code expires in 60 seconds. This phone session is read-only.");
    Ok(())
}

fn daemon_allows_tailnet(port: u16, host: &str) -> Result<bool> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .context("connecting to the loopback dashboard API")?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    write!(
        stream,
        "GET /api/v1/health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or("");
    let json: serde_json::Value =
        serde_json::from_str(body).context("daemon returned invalid API health JSON")?;
    Ok(json.get("tailnet_host").and_then(serde_json::Value::as_str) == Some(host))
}

#[cfg(test)]
mod phone_tests {
    use super::*;

    #[test]
    fn phone_setup_uses_only_private_serve_with_loopback_proxy() {
        let args = serve_command_args(4321);
        assert_eq!(
            args,
            ["serve", "--bg", "--https=443", "http://127.0.0.1:4321"]
        );
        assert!(!args.iter().any(|arg| arg.contains("funnel")));
    }

    #[cfg(unix)]
    #[test]
    fn serve_command_is_testable_with_a_stand_in_cli() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("temporary directory");
        let log = dir.path().join("args.log");
        let script = dir.path().join("tailscale");
        std::fs::write(
            &script,
            format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n", log.display()),
        )
        .expect("write stand-in");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))
            .expect("make executable");
        let result = configure_tailscale_serve(script.as_os_str(), 4321).expect("serve call");
        assert!(result.status.success());
        assert_eq!(
            std::fs::read_to_string(log).expect("arguments"),
            "serve\n--bg\n--https=443\nhttp://127.0.0.1:4321\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn tailnet_dns_detection_is_testable_with_a_stand_in_cli() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("temporary directory");
        let script = dir.path().join("tailscale");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf '%s' '{\"Self\":{\"DNSName\":\"node.team.ts.net.\"}}'\n",
        )
        .expect("write stand-in");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))
            .expect("make executable");
        assert_eq!(
            resolve_tailnet_host_with(None, script.as_os_str())
                .expect("DNS name")
                .as_deref(),
            Some("node.team.ts.net")
        );
    }
}
