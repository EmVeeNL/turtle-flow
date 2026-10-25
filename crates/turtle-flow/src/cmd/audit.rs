//! `turtle audit verify`: walks the hash-chained event log (SEC-009).

use std::process::ExitCode;

use anyhow::Result;
use turtle_store::ChainVerdict;

use crate::runtime::Runtime;

pub async fn verify(rt: &Runtime) -> Result<ExitCode> {
    match rt.store.with(|s| s.verify_chain()).await? {
        ChainVerdict::Intact { events, head } => {
            println!("OK: {events} events, chain head {head}");
            Ok(ExitCode::SUCCESS)
        }
        ChainVerdict::Broken { seq, reason } => {
            eprintln!("TAMPERED: the event log is broken at seq {seq}: {reason}");
            Ok(ExitCode::from(2))
        }
    }
}
