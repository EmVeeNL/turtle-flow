//! `turtle approvals` and `turtle approve`. A decision is only accepted from an interactive
//! terminal: the approver sees what is bound, then types the first characters of its hash.

use std::io::{BufRead, IsTerminal, Write};

use anyhow::{Result, bail};
use turtle_domain::{Actor, ApprovalId};
use turtle_scheduler::ApprovalError;
use turtle_store::{ApprovalRecord, Decision, StoreError};

use crate::runtime::{Runtime, label};

/// Characters of the subject hash the approver types to confirm they saw it.
const CONFIRM_CHARS: usize = 8;

pub async fn list(rt: &Runtime) -> Result<()> {
    let pending = rt.store.with(|s| s.pending_approvals()).await?;
    if pending.is_empty() {
        println!("Nothing is waiting for you.");
        return Ok(());
    }
    for a in pending {
        let id = a.job_id;
        let job = rt.store.with(move |s| s.load_job(id)).await?;
        println!(
            "{}  {:<10} {:<11} {}  {}",
            a.id,
            job.key.to_string(),
            label(&a.gate),
            a.subject_hash.short(),
            job.title
        );
    }
    println!("\nDecide with: turtle approve <id>   (changes: --request-changes -c \"why\")");
    Ok(())
}

pub async fn decide(
    rt: &Runtime,
    id: &str,
    request_changes: bool,
    reject: bool,
    comment: Option<String>,
) -> Result<()> {
    let id: ApprovalId = id
        .parse()
        .map_err(|_| anyhow::anyhow!("`{id}` is not an approval id"))?;
    let decision = match (request_changes, reject) {
        (true, true) => bail!("choose one of --request-changes and --reject"),
        (true, false) => Decision::RequestChanges,
        (false, true) => Decision::Reject,
        (false, false) => Decision::Approve,
    };
    if decision != Decision::Approve && comment.as_deref().is_none_or(|c| c.trim().is_empty()) {
        bail!("a comment is required to request changes or reject: add -c \"why\"");
    }
    if !std::io::stdin().is_terminal() {
        bail!("approvals are only accepted from an interactive terminal; run this in your shell");
    }

    let approval = rt.store.with(move |s| s.approval(id)).await?;
    let job_id = approval.job_id;
    let job = rt.store.with(move |s| s.load_job(job_id)).await?;
    print_subject(rt, &job.key.to_string(), &job.title, &approval);

    let verb = match decision {
        Decision::Approve => "approve",
        Decision::RequestChanges => "request changes on",
        Decision::Reject => "reject",
    };
    let expected = approval.subject_hash.to_hex();
    let expected = &expected[..CONFIRM_CHARS];
    print!("To {verb} this, type the first {CONFIRM_CHARS} characters of the hash: ");
    std::io::stdout().flush()?;
    let mut typed = String::new();
    std::io::stdin().lock().read_line(&mut typed)?;
    if typed.trim() != expected {
        bail!("that is not the hash shown above; nothing was decided");
    }

    let flags = approval.binding.acknowledged_flags.clone();
    let after = rt
        .approvals
        .decide(
            id,
            decision,
            approval.subject_hash,
            Actor::local_user(),
            "cli_tty",
            comment,
            flags,
        )
        .await
        .map_err(explain)?;
    println!("Done. {} is now {}.", after.key, label(&after.job.state));
    Ok(())
}

fn explain(e: ApprovalError) -> anyhow::Error {
    match e {
        ApprovalError::Store(StoreError::HashMismatch) => {
            anyhow::anyhow!("the approval subject changed; nothing was approved")
        }
        other => other.into(),
    }
}

fn print_subject(rt: &Runtime, key: &str, title: &str, a: &ApprovalRecord) {
    println!("\n{key}  {title}");
    println!("Gate     {}", label(&a.gate));
    println!("Hash     {}", a.subject_hash.to_hex());
    for (kind, r) in &a.binding.artifacts {
        println!("  {kind} rev {} sha256 {}", r.rev, r.sha256.short());
        if let Ok(bytes) = rt.blobs.get(&r.sha256) {
            let text = String::from_utf8_lossy(&bytes);
            // Documents are canonical one-line JSON; show them the way a person reads them.
            let text = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| serde_json::to_string_pretty(&v).ok())
                .unwrap_or_else(|| text.into_owned());
            for line in text.lines().take(60) {
                println!("    | {line}");
            }
            if text.lines().count() > 60 {
                println!("    | ... (truncated; the full document is in the blob store)");
            }
        }
    }
    if let Some(g) = &a.binding.git {
        println!(
            "  git: {} -> {} on {} (diff {})",
            &g.base_commit[..g.base_commit.len().min(10)],
            &g.head_commit[..g.head_commit.len().min(10)],
            g.target_branch,
            g.diff_sha256.short()
        );
    }
    if !a.binding.acknowledged_flags.is_empty() {
        println!("Flags you acknowledge by approving:");
        for f in &a.binding.acknowledged_flags {
            println!("  ! {f}");
        }
    }
    println!();
}
