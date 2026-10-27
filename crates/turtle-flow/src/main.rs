//! `turtle`: the Turtle Flow CLI. CLI commands use the local database directly; the M4 daemon
//! also hosts the dashboard API (`docs/MVP.md` §6).

mod account_license;
mod cmd;
mod doctor;
mod export;
mod init;
mod paths;
mod runs;
mod runtime;

use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::cmd::budget::BudgetCommand;
use crate::cmd::daemon::DaemonOptions;
use crate::cmd::job::JobCommandLine;
use crate::cmd::project::ProjectCommand;
use crate::cmd::resource::ResourceCommand;
use crate::paths::Home;
use crate::runtime::Runtime;

#[derive(Debug, Parser)]
#[command(
    name = "turtle",
    version,
    about = "Supervised AI software delivery, orchestrated locally."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Link this CLI to your Turtle Flow account license.
    Login,
    /// Internal subprocess used to extract text from an attached PDF.
    #[command(name = "__extract-pdf", hide = true)]
    ExtractPdf,
    /// Check the environment: Git, provider CLIs and the data directory.
    Doctor {
        /// Print machine-readable JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Set up Turtle Flow for the current repository (interactive terminal required).
    Init,
    /// Export a job's audit package.
    Export {
        #[command(subcommand)]
        command: ExportCommand,
    },
    /// Generate shell completion scripts.
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Inspect available provider CLIs and verified capabilities.
    #[command(subcommand)]
    Providers(ProvidersCommand),
    /// Start the background daemon that runs jobs.
    Up {
        #[command(flatten)]
        agent: AgentArgs,
    },
    /// Stop the background daemon.
    Down,
    /// Say whether the daemon is running.
    Status,
    /// Open the local dashboard using a one-time interactive login code.
    Dashboard {
        /// Print a read-only QR login link for a Tailscale device.
        #[arg(long)]
        phone: bool,
    },
    /// Run the daemon in the foreground (what `up` starts).
    Daemon {
        #[command(flatten)]
        agent: AgentArgs,
    },
    /// Register and inspect projects.
    #[command(subcommand)]
    Project(ProjectCommand),
    /// Attach versioned resources and search them with full-text retrieval.
    #[command(subcommand)]
    Resource(ResourceCommand),
    /// Search project resources with SQLite FTS5.
    Search {
        #[arg(short, long)]
        project: String,
        query: String,
    },
    /// Create and follow jobs.
    #[command(subcommand)]
    Job(JobCommandLine),
    /// Token budgets.
    #[command(subcommand)]
    Budget(BudgetCommand),
    /// Inspect agent runs and their redacted transcripts.
    #[command(subcommand)]
    Runs(runs::RunsCommand),
    /// List what is waiting for your decision.
    Approvals,
    /// Decide an approval (interactive terminal only).
    Approve {
        id: String,
        /// Send the job back with feedback instead of approving.
        #[arg(long)]
        request_changes: bool,
        /// Reject the job.
        #[arg(long)]
        reject: bool,
        /// Required with --request-changes and --reject.
        #[arg(short, long)]
        comment: Option<String>,
    },
    /// Verify the tamper-evident event log.
    #[command(subcommand)]
    Audit(AuditCommand),
}

#[derive(Debug, clap::Args)]
struct AgentArgs {
    /// Disable the local HTTP API.
    #[arg(long)]
    no_api: bool,
    /// Fixed loopback API port (default: random available port).
    #[arg(long)]
    api_port: Option<u16>,
    /// Allow the API through Tailscale Serve at this MagicDNS name.
    #[arg(long)]
    tailnet_host: Option<String>,
    /// Ordered provider list: claude, codex, or both (demo runs alone).
    #[arg(long)]
    agent: Option<String>,
    /// Per-role priority, e.g. reviewer=codex. Repeat for more roles.
    #[arg(long = "role-agent")]
    role_agent: Vec<String>,
    /// Claude model (default: the CLI's own default).
    #[arg(long)]
    claude_model: Option<String>,
    /// Claude effort: low, medium, high, xhigh or max.
    #[arg(long)]
    claude_effort: Option<String>,
    /// Spend cap per Claude run in API-equivalent USD (default 3.00).
    #[arg(long)]
    max_run_usd: Option<f64>,
    /// Codex model (default: the CLI's own default).
    #[arg(long)]
    codex_model: Option<String>,
    /// Codex reasoning effort: low, medium, high, xhigh or max.
    #[arg(long)]
    codex_effort: Option<String>,
}

impl AgentArgs {
    fn options(&self) -> DaemonOptions {
        DaemonOptions {
            no_api: self.no_api,
            api_port: self.api_port,
            tailnet_host: self.tailnet_host.clone(),
            agent: self.agent.clone(),
            role_agent: self.role_agent.clone(),
            claude_model: self.claude_model.clone(),
            claude_effort: self.claude_effort.clone(),
            max_run_usd: self.max_run_usd,
            codex_model: self.codex_model.clone(),
            codex_effort: self.codex_effort.clone(),
        }
    }
}

#[derive(Debug, Subcommand)]
enum ProvidersCommand {
    /// Show installation, login, compatibility and verified capabilities.
    List,
}

#[derive(Debug, Subcommand)]
enum AuditCommand {
    /// Walk the hash chain; exits with status 2 if it is broken.
    Verify,
}

#[derive(Debug, Subcommand)]
enum ExportCommand {
    /// Export specification, plan, approvals, runs, reports and delivery.
    Job {
        key: String,
        #[arg(long, value_enum, default_value = "md")]
        format: export::Format,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(cli.command).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn dispatch(command: Command) -> Result<ExitCode> {
    if matches!(&command, Command::ExtractPdf) {
        return cmd::resource::extract_pdf_worker();
    }
    if let Command::Doctor { json } = command {
        return Ok(doctor::run(json).await);
    }
    if matches!(&command, Command::Init) {
        init::run().await?;
        return Ok(ExitCode::SUCCESS);
    }
    if let Command::Completions { shell } = &command {
        use clap::CommandFactory;
        let mut cli = Cli::command();
        clap_complete::generate(*shell, &mut cli, "turtle", &mut std::io::stdout());
        return Ok(ExitCode::SUCCESS);
    }
    let home = Home::locate()?;
    if !matches!(&command, Command::Login) {
        account_license::report_status(&home);
    }
    let ok = ExitCode::SUCCESS;
    match command {
        Command::Login => account_license::login(&home).await?,
        Command::ExtractPdf => unreachable!("handled above"),
        Command::Doctor { .. } => unreachable!("handled above"),
        Command::Init | Command::Completions { .. } => unreachable!("handled above"),
        Command::Export {
            command: ExportCommand::Job { key, format },
        } => {
            export::run(&Runtime::open(home)?, &key, format).await?;
        }
        Command::Providers(ProvidersCommand::List) => {
            providers_list(&home).await?;
        }
        Command::Up { agent } => cmd::daemon::up(&home, &agent.options()).await?,
        Command::Down => cmd::daemon::down(&home)?,
        Command::Status => cmd::daemon::status(&home).await?,
        Command::Dashboard { phone } => cmd::daemon::dashboard(&home, phone).await?,
        Command::Daemon { agent } => {
            let options = agent.options();
            cmd::daemon::run(home, options).await?;
        }
        Command::Project(c) => cmd::project::run(&Runtime::open(home)?, c).await?,
        Command::Resource(ResourceCommand::Add {
            input,
            project,
            job,
            trust,
        }) => {
            cmd::resource::add(
                &Runtime::open(home)?,
                &input,
                project.as_deref(),
                job.as_deref(),
                &trust,
            )
            .await?
        }
        Command::Search { project, query } => {
            cmd::resource::search(&Runtime::open(home)?, &project, &query).await?
        }
        Command::Job(c) => cmd::job::run(&Runtime::open(home)?, c).await?,
        Command::Budget(c) => cmd::budget::run(&Runtime::open(home)?, c).await?,
        Command::Runs(c) => runs::run(&Runtime::open(home)?, c).await?,
        Command::Approvals => cmd::approvals::list(&Runtime::open(home)?).await?,
        Command::Approve {
            id,
            request_changes,
            reject,
            comment,
        } => {
            cmd::approvals::decide(&Runtime::open(home)?, &id, request_changes, reject, comment)
                .await?;
        }
        Command::Audit(AuditCommand::Verify) => {
            return cmd::audit::verify(&Runtime::open(home)?).await;
        }
    }
    Ok(ok)
}

async fn providers_list(home: &Home) -> Result<ExitCode> {
    let registry = turtle_agents::CapabilityRegistry::discover().await;
    let runtime = Runtime::open(home.clone())?;
    let (limits, snapshots) = runtime
        .store
        .with(|store| {
            Ok::<_, turtle_store::StoreError>((
                store.current_limited_providers()?,
                store.latest_quota_snapshots()?,
            ))
        })
        .await?;
    for provider in turtle_agents::ProviderId::ALL {
        let caps = registry.get(provider);
        let version = caps
            .info
            .version
            .as_ref()
            .map_or_else(|| "unknown".into(), ToString::to_string);
        let limit_label = limits
            .iter()
            .find(|limit| limit.provider == provider.as_str())
            .map_or_else(
                || "not rate limited".to_owned(),
                |limit| match limit.reset_at.as_deref() {
                    Some(reset) => format!(
                        "rate limited until {} (provider hint)",
                        display_local_time(reset)
                    ),
                    None => "rate limited (reset unknown)".into(),
                },
            );
        println!(
            "{}: version={}, compatibility={:?}, login={:?}, schema={}, quota={}, sandbox={}, effort={}; {}; {}",
            provider.as_str(),
            version,
            caps.info.compat,
            caps.auth,
            caps.native_json_schema,
            caps.quota_visibility,
            caps.sandbox_modes.join(","),
            caps.effort_levels.join(","),
            limit_label,
            caps.info.detail
        );
        for quota in snapshots.iter().filter(|q| q.provider == provider.as_str()) {
            let age = snapshot_age(&quota.observed_at);
            println!(
                "  {}: {:.0}% of provider window ({} old, {})",
                quota.window_name,
                quota.utilization * 100.0,
                age,
                quota.status
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn display_local_time(timestamp: &str) -> String {
    let Ok(parsed) =
        time::OffsetDateTime::parse(timestamp, &time::format_description::well_known::Rfc3339)
    else {
        return "unknown time".into();
    };
    parsed
        .to_offset(turtle_agents::local_offset().unwrap_or(time::UtcOffset::UTC))
        .format(time::macros::format_description!("[hour]:[minute]"))
        .unwrap_or_else(|_| "unknown time".into())
}

fn snapshot_age(timestamp: &str) -> String {
    let Ok(observed) =
        time::OffsetDateTime::parse(timestamp, &time::format_description::well_known::Rfc3339)
    else {
        return "age unknown".into();
    };
    let seconds = (time::OffsetDateTime::now_utc() - observed)
        .whole_seconds()
        .max(0);
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3_600 {
        format!("{}m", seconds / 60)
    } else {
        format!("{}h", seconds / 3_600)
    }
}
