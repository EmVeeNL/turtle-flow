//! `turtle`: the Turtle Flow CLI. Milestone M1 commands work on the local database directly;
//! the HTTP API and dashboard arrive in M4 (`docs/MVP.md` §6).

mod cmd;
mod doctor;
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
    /// Check the environment: Git, provider CLIs and the data directory.
    Doctor {
        /// Print machine-readable JSON instead of a table.
        #[arg(long)]
        json: bool,
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
    /// Run the daemon in the foreground (what `up` starts).
    Daemon {
        #[command(flatten)]
        agent: AgentArgs,
    },
    /// Register and inspect projects.
    #[command(subcommand)]
    Project(ProjectCommand),
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
    if let Command::Doctor { json } = command {
        return Ok(doctor::run(json).await);
    }
    if let Command::Providers(ProvidersCommand::List) = command {
        let registry = turtle_agents::CapabilityRegistry::discover().await;
        for provider in turtle_agents::ProviderId::ALL {
            let caps = registry.get(provider);
            let version = caps
                .info
                .version
                .as_ref()
                .map_or_else(|| "unknown".into(), ToString::to_string);
            println!(
                "{}: version={}, compatibility={:?}, login={:?}, schema={}, quota={}, sandbox={}, effort={}; {}",
                provider.as_str(),
                version,
                caps.info.compat,
                caps.auth,
                caps.native_json_schema,
                caps.quota_visibility,
                caps.sandbox_modes.join(","),
                caps.effort_levels.join(","),
                caps.info.detail
            );
        }
        return Ok(ExitCode::SUCCESS);
    }
    let home = Home::locate()?;
    let ok = ExitCode::SUCCESS;
    match command {
        Command::Doctor { .. } => unreachable!("handled above"),
        Command::Providers(_) => unreachable!("handled above"),
        Command::Up { agent } => cmd::daemon::up(&home, &agent.options()).await?,
        Command::Down => cmd::daemon::down(&home)?,
        Command::Status => cmd::daemon::status(&home),
        Command::Daemon { agent } => {
            let options = agent.options();
            cmd::daemon::run(home, options).await?;
        }
        Command::Project(c) => cmd::project::run(&Runtime::open(home)?, c).await?,
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
