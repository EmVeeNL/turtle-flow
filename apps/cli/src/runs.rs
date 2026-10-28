use anyhow::{Context, Result, bail};
use clap::Subcommand;

use crate::{cmd::job, runtime::Runtime};

#[derive(Debug, Subcommand)]
pub enum RunsCommand {
    /// List runs for a job.
    List {
        #[arg(long)]
        job: String,
    },
    /// Print a run transcript. The stored stream is already redacted.
    Transcript {
        run_id: String,
        #[arg(long)]
        raw: bool,
    },
}

fn event_summary(value: &serde_json::Value) -> String {
    if let Some(text) = value.get("result").and_then(|v| v.as_str()) {
        return text.to_owned();
    }
    if let Some(text) = value.get("message").and_then(|v| v.as_str()) {
        return text.to_owned();
    }
    if let Some(content) = value
        .get("message")
        .and_then(|v| v.get("content"))
        .and_then(|v| v.as_array())
    {
        if let Some(text) = content
            .iter()
            .find_map(|item| item.get("text").and_then(|v| v.as_str()))
        {
            return text.to_owned();
        }
        if let Some(tool) = content
            .iter()
            .find(|item| item.get("type").and_then(|v| v.as_str()) == Some("tool_use"))
        {
            let name = tool.get("name").and_then(|v| v.as_str()).unwrap_or("tool");
            let input = tool
                .get("input")
                .map(serde_json::Value::to_string)
                .unwrap_or_default();
            return format!("{name} {input}");
        }
    }
    value
        .get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned()
}

pub async fn run(rt: &Runtime, command: RunsCommand) -> Result<()> {
    match command {
        RunsCommand::List { job: key } => {
            let job = job::find(rt, &key).await?;
            let id = job.job.id;
            let records = rt.store.with(move |s| s.runs_for_job(id)).await?;
            if records.is_empty() {
                println!("No runs for {key}.");
            }
            for r in records {
                let usage = if r.provenance == "unavailable" {
                    "unknown".to_owned()
                } else {
                    format!(
                        "{} tokens: {} input + {} cache read + {} cache write + {} output ({})",
                        r.input + r.cache_read + r.cache_write + r.output,
                        r.input,
                        r.cache_read,
                        r.cache_write,
                        r.output,
                        r.provenance
                    )
                };
                let duration = r.duration_ms.map_or_else(
                    || "unknown".to_owned(),
                    |ms| format!("{:.1}s", ms as f64 / 1000.0),
                );
                println!(
                    "{}  {:<14} {}/{}  {:<10} {:<38} {}",
                    r.id, r.role, r.provider, r.model, r.status, usage, duration
                );
            }
        }
        RunsCommand::Transcript { run_id, raw } => {
            let id = run_id.clone();
            let record = rt.store.with(move |s| s.run_by_id(&id)).await?;
            let Some(hash) = record.transcript_blob else {
                bail!("run {run_id} has no transcript");
            };
            let bytes = rt
                .blobs
                .get(&hash)
                .with_context(|| format!("reading transcript for run {run_id}"))?;
            let transcript = String::from_utf8(bytes).context("stored transcript is not UTF-8")?;
            if raw {
                print!("{transcript}");
            } else {
                for line in transcript.lines() {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                        let kind = value
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("event");
                        println!(
                            "{kind:<14} {}",
                            event_summary(&value).chars().take(300).collect::<String>()
                        );
                    } else {
                        println!("{}", line);
                    }
                }
            }
        }
    }
    Ok(())
}
