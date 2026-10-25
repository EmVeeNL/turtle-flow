//! `turtle audit verify`: walks the hash-chained event log (SEC-009).

use std::process::ExitCode;

use anyhow::Result;
use turtle_store::{ChainVerdict, LedgerVerdict};

use crate::runtime::Runtime;

pub async fn verify(rt: &Runtime) -> Result<ExitCode> {
    match rt.store.with(|s| s.verify_chain()).await? {
        ChainVerdict::Intact { events, head } => {
            println!("OK: {events} events, chain head {head}");
            match rt.store.with(|s| s.verify_ledger()).await? {
                LedgerVerdict::Consistent { budgets, entries } => {
                    println!("OK: ledger consistent ({budgets} budgets, {entries} entries)");
                    Ok(ExitCode::SUCCESS)
                }
                LedgerVerdict::Mismatch { budget, detail } => {
                    eprintln!("TAMPERED: budget {budget} does not match its ledger: {detail}");
                    Ok(ExitCode::from(2))
                }
            }
        }
        ChainVerdict::Broken { seq, reason } => {
            eprintln!("TAMPERED: the event log is broken at seq {seq}: {reason}");
            Ok(ExitCode::from(2))
        }
    }
}
