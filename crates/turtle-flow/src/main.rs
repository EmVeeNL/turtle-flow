//! `turtle` — the Turtle Flow CLI. Only `doctor` exists so far; the daemon and job
//! commands follow the milestones in `docs/MVP.md` §6.

mod doctor;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

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
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Doctor { json } => doctor::run(json).await,
    }
}
