//! Stable job exports assembled from stored, redacted records and content-addressed blobs.

use anyhow::Result;
use clap::ValueEnum;
use serde::Serialize;
use serde_json::{Value, json};
use turtle_domain::ArtifactKind;

use crate::{cmd::job::find, runtime::Runtime};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Format {
    Md,
    Json,
}

#[derive(Debug, Serialize)]
struct Export {
    job: Value,
    artifacts: Vec<Value>,
    approvals: Vec<Value>,
    runs: Vec<Value>,
    usage: Value,
    events: Vec<Value>,
    delivery: Option<Value>,
}

fn redact_value(value: &mut Value) -> usize {
    match value {
        Value::String(text) => {
            let (clean, count) = turtle_context::redact(text);
            *text = clean;
            count
        }
        Value::Array(values) => values.iter_mut().map(redact_value).sum(),
        Value::Object(values) => values.values_mut().map(redact_value).sum(),
        _ => 0,
    }
}

pub async fn run(rt: &Runtime, key: &str, format: Format) -> Result<()> {
    let job = find(rt, key).await?;
    let id = job.job.id;
    let (project, approvals, runs, usage, events, delivery, revisions) = rt
        .store
        .with(move |s| {
            let kinds = [
                ArtifactKind::Specification,
                ArtifactKind::Plan,
                ArtifactKind::GateResult,
                ArtifactKind::ReviewReport,
                ArtifactKind::VerificationReport,
            ];
            let mut revisions = Vec::new();
            for kind in kinds {
                revisions.extend(s.list_revisions(id, kind)?);
            }
            Ok::<_, turtle_store::StoreError>((
                s.project_of_job(id)?,
                s.list_approvals(id)?,
                s.runs_for_job(id)?,
                s.job_usage(id)?,
                s.list_events(Some(id))?,
                s.delivery(id)?,
                revisions,
            ))
        })
        .await?;
    let mut artifacts = Vec::with_capacity(revisions.len());
    for revision in revisions {
        let content = rt.blobs.get(&revision.sha256)?;
        let mut value = serde_json::from_slice(&content)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&content).into_owned()));
        let redactions = redact_value(&mut value);
        artifacts.push(json!({
            "kind": revision.kind, "revision": revision.rev, "sha256": revision.sha256,
            "created_at": revision.created_at, "content": value, "content_redacted": redactions > 0
        }));
    }
    artifacts.sort_by(|a, b| {
        (a["kind"].as_str(), a["revision"].as_u64())
            .cmp(&(b["kind"].as_str(), b["revision"].as_u64()))
    });
    let approvals = approvals.into_iter().map(|a| {
        let comment = a.comment.as_deref().map(|value| turtle_context::redact(value).0);
        json!({
            "id": a.id, "gate": a.gate, "status": a.status, "subject_hash": a.subject_hash,
            "binding": a.binding, "decided_by": a.decided_by, "decided_at": a.decided_at,
            "comment": comment, "invalidated_reason": a.invalidated_reason, "created_at": a.created_at
        })
    }).collect();
    let runs = runs.into_iter().map(|r| json!({
        "id": r.id, "role": r.role, "provider": r.provider, "model": r.model,
        "status": r.status, "started_at": r.started_at, "ended_at": r.ended_at,
        "duration_ms": r.duration_ms, "usage": {"input": r.input, "cache_read": r.cache_read,
            "cache_write": r.cache_write, "output": r.output,
            "total": r.input + r.cache_read + r.cache_write + r.output,
            "provenance": r.provenance}, "transcript_sha256": r.transcript_blob
    })).collect();
    let events = events
        .into_iter()
        .map(|e| {
            json!({
                "seq": e.seq, "at": e.occurred_at, "kind": e.kind,
                "actor": {"kind": e.actor_kind, "id": e.actor_id}, "hash": e.hash
            })
        })
        .collect();
    let delivery = delivery.map(|d| {
        json!({
            "mode": d.mode, "target_branch": d.target_branch, "result_commit": d.result_commit,
            "pr_url": d.pr_url, "status": d.status, "approval_id": d.approval_id
        })
    });
    let document = Export {
        job: json!({"key": job.key, "title": turtle_context::redact(&job.title).0,
            "description": turtle_context::redact(&job.description).0,
            "type": job.job_type, "priority": job.priority, "state": job.job.state,
            "project": project.slug, "base_commit": job.base_commit}),
        artifacts,
        approvals,
        runs,
        usage: json!({"runs": usage.runs, "tokens_reported": usage.tokens_reported,
            "tokens_estimated": usage.tokens_estimated, "runs_unavailable": usage.runs_unavailable}),
        events,
        delivery,
    };
    match format {
        Format::Json => println!("{}", serde_json::to_string_pretty(&document)?),
        Format::Md => print_markdown(&document)?,
    }
    Ok(())
}

fn print_markdown(doc: &Export) -> Result<()> {
    let job = &doc.job;
    println!(
        "# {} — {}\n",
        job["key"].as_str().unwrap_or("unknown"),
        job["title"].as_str().unwrap_or("")
    );
    println!(
        "- Project: {}\n- State: {}\n- Type: {}\n- Base commit: {}\n",
        job["project"].as_str().unwrap_or("unknown"),
        job["state"].as_str().unwrap_or("unknown"),
        job["type"].as_str().unwrap_or("unknown"),
        job["base_commit"].as_str().unwrap_or("unknown")
    );
    if let Some(desc) = job["description"].as_str().filter(|s| !s.is_empty()) {
        println!("{}\n", desc);
    }
    println!("## Artifacts\n");
    if doc.artifacts.is_empty() {
        println!("No stored artifact revisions.\n");
    }
    for artifact in &doc.artifacts {
        println!(
            "### {} revision {}\n\nStored SHA-256: `{}` (content redacted: {})\n\n```json\n{}\n```\n",
            artifact["kind"].as_str().unwrap_or("artifact"),
            artifact["revision"],
            artifact["sha256"].as_str().unwrap_or("unknown"),
            artifact["content_redacted"],
            serde_json::to_string_pretty(&artifact["content"])?
        );
    }
    println!("## Approvals\n");
    for approval in &doc.approvals {
        println!(
            "- {} {} — {} (subject `{}`)",
            approval["gate"], approval["status"], approval["id"], approval["subject_hash"]
        );
    }
    println!(
        "\n## Runs and usage\n\n| Run | Role | Provider / model | Status | Tokens | Provenance |\n|---|---|---|---|---:|---|"
    );
    for run in &doc.runs {
        println!(
            "| {} | {} | {} / {} | {} | {} | {} |",
            run["id"],
            run["role"],
            run["provider"],
            run["model"],
            run["status"],
            run["usage"]["total"],
            run["usage"]["provenance"]
        );
    }
    println!(
        "\nJob usage totals: reported {}, estimated {}, unavailable runs {}.\n",
        doc.usage["tokens_reported"], doc.usage["tokens_estimated"], doc.usage["runs_unavailable"]
    );
    println!("## Delivery\n");
    if let Some(delivery) = &doc.delivery {
        println!(
            "{} — commit {}, PR {}",
            delivery["mode"], delivery["result_commit"], delivery["pr_url"]
        );
    } else {
        println!("No delivery record.");
    }
    println!("\n## Audit events\n\n| Seq | Time | Event | Hash |\n|---:|---|---|---|");
    for event in &doc.events {
        println!(
            "| {} | {} | {} | `{}` |",
            event["seq"], event["at"], event["kind"], event["hash"]
        );
    }
    Ok(())
}
