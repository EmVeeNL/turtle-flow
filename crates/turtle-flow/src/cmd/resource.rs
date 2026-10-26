//! Resource ingestion and full-text search commands.

use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use turtle_domain::JobKey;
use turtle_store::NewResource;

use crate::runtime::Runtime;

#[derive(Debug, Subcommand)]
pub enum ResourceCommand {
    /// Add a local file or take a one-time HTTP(S) URL snapshot.
    Add {
        input: String,
        #[arg(short, long)]
        project: Option<String>,
        #[arg(long)]
        job: Option<String>,
        #[arg(long, default_value = "untrusted", value_parser = ["trusted", "internal", "untrusted"])]
        trust: String,
    },
}

fn sanitize_text(bytes: &[u8]) -> Result<(Vec<u8>, String)> {
    let text = std::str::from_utf8(bytes).context("binary resource extraction is unsupported; refusing to store content that cannot be redacted")?;
    let clean = turtle_context::redact(text).0;
    Ok((clean.as_bytes().to_vec(), clean))
}

pub async fn add(
    rt: &Runtime,
    input: &str,
    project: Option<&str>,
    job: Option<&str>,
    trust: &str,
) -> Result<()> {
    let project = project.map(str::to_owned);
    let job = job.map(str::to_owned);
    let (project_id, job_id) = rt
        .store
        .with(move |store| {
            let job_record = job
                .map(|key| {
                    store.find_job(
                        &key.parse::<JobKey>()
                            .map_err(|e| turtle_store::StoreError::Corrupt(e.to_string()))?,
                    )
                })
                .transpose()?;
            let project_record = match (project, job_record.as_ref()) {
                (Some(slug), _) => store.project_by_slug(&slug)?,
                (None, Some(job)) => store.load_project(job.project_id)?,
                (None, None) => {
                    return Err(turtle_store::StoreError::NotFound(
                        "provide -p or --job".into(),
                    ));
                }
            };
            if let Some(job) = &job_record
                && job.project_id != project_record.id
            {
                return Err(turtle_store::StoreError::Corrupt(
                    "job is not in selected project".into(),
                ));
            }
            Ok::<_, turtle_store::StoreError>((project_record.id, job_record.map(|j| j.job.id)))
        })
        .await?;

    let is_url = input.starts_with("https://") || input.starts_with("http://");
    let raw = if is_url {
        let output = Command::new("curl")
            .args([
                "--fail",
                "--silent",
                "--show-error",
                "--location",
                "--max-time",
                "30",
                "--max-filesize",
                "8388608",
                "--proto",
                "=http,https",
                input,
            ])
            .output()
            .context("fetching URL snapshot with curl")?;
        if !output.status.success() {
            bail!(
                "URL snapshot failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        output.stdout
    } else {
        std::fs::read(input).with_context(|| format!("reading {input}"))?
    };
    if raw.len() > 8 * 1024 * 1024 {
        bail!("resource exceeds the 8 MiB ingestion limit");
    }
    if raw.starts_with(b"%PDF-") || input.to_ascii_lowercase().ends_with(".pdf") {
        bail!(
            "PDF extraction is unsupported; only UTF-8 Markdown, plain text and source files are supported"
        );
    }
    let (sanitized, extracted) = sanitize_text(&raw)?;
    let hash = rt
        .blobs
        .put(&sanitized)
        .context("storing redacted resource content")?;
    let title = if is_url {
        input
            .rsplit('/')
            .find(|part| !part.is_empty())
            .unwrap_or(input)
            .to_owned()
    } else {
        std::path::Path::new(input)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("resource")
            .to_owned()
    };
    let title = turtle_context::redact(&title).0;
    let source = turtle_context::redact(input).0;
    let record = NewResource {
        project_id,
        title,
        kind: if is_url {
            "url_snapshot"
        } else if input.ends_with(".md") || input.ends_with(".markdown") {
            "markdown"
        } else {
            "text"
        }
        .into(),
        trust: trust.to_owned(),
        source: Some(source),
        sha256: hash.to_string(),
        media_type: "text/plain".into(),
        size_bytes: sanitized.len() as u64,
        extracted_text: Some(extracted),
        job_id,
    };
    let saved = rt
        .store
        .with(move |store| store.add_resource(&record))
        .await?;
    println!(
        "{} v{} {} ({})",
        saved.title, saved.version, saved.trust, saved.sha256
    );
    Ok(())
}

pub async fn search(rt: &Runtime, slug: &str, query: &str) -> Result<()> {
    let slug = slug.to_owned();
    let query = query.to_owned();
    let project = rt
        .store
        .with(move |store| store.project_by_slug(&slug))
        .await?;
    let hits = rt
        .store
        .with(move |store| store.search_resources(project.id, None, &query, 10))
        .await?;
    for hit in hits {
        println!(
            "{} [{}] {}\n{}\n",
            hit.title, hit.trust, hit.resource_id, hit.text
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_ingest_redacts_before_hashable_bytes_are_returned() {
        let secret = b"api_key=ghp_synthetic_token_12345678901234567890";
        let (stored, indexed) = sanitize_text(secret).expect("text resource");
        assert!(!String::from_utf8_lossy(&stored).contains("ghp_synthetic_token"));
        assert!(indexed.contains("[REDACTED:api_key]"));
        assert!(sanitize_text(&[0xff]).is_err());
    }
}
