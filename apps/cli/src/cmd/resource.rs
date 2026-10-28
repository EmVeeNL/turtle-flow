//! Resource ingestion and full-text search commands.

use std::io::{Read, Write};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use turtle_domain::JobKey;
use turtle_exec::{CommandSpec, ExitOutcome};
use turtle_store::NewResource;

use crate::runtime::Runtime;

const MAX_RESOURCE_BYTES: usize = 8 * 1024 * 1024;
const MAX_EXTRACTED_TEXT_BYTES: usize = 4 * 1024 * 1024;
const PDF_EXTRACTION_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Subcommand)]
pub enum ResourceCommand {
    /// Add a local text/PDF file or take a one-time HTTP(S) URL snapshot.
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

fn is_pdf(input: &str, bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF-") || input.to_ascii_lowercase().ends_with(".pdf")
}

fn read_limited(path: &str) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path).with_context(|| format!("reading {path}"))?;
    let mut bytes = Vec::new();
    file.take((MAX_RESOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .with_context(|| format!("reading {path}"))?;
    if bytes.len() > MAX_RESOURCE_BYTES {
        bail!("resource exceeds the 8 MiB ingestion limit");
    }
    Ok(bytes)
}

async fn extract_pdf(bytes: &[u8]) -> Result<Vec<u8>> {
    let executable =
        std::env::current_exe().context("locating turtle executable for PDF extraction")?;
    let output = turtle_exec::run(
        CommandSpec::new(executable.into_os_string())
            .arg("__extract-pdf")
            .stdin(bytes.to_vec())
            .timeout(PDF_EXTRACTION_TIMEOUT)
            .max_output_bytes(MAX_EXTRACTED_TEXT_BYTES + 1),
    )
    .await
    .context("starting isolated PDF text extraction")?;

    if matches!(output.outcome, ExitOutcome::TimedOut) {
        bail!("PDF extraction exceeded the 20 second time limit");
    }
    if output.truncated || output.stdout.len() > MAX_EXTRACTED_TEXT_BYTES {
        bail!("PDF extracted text exceeds the 4 MiB limit");
    }
    if !output.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.trim();
        bail!(
            "PDF extraction failed{}",
            if detail.is_empty() {
                String::new()
            } else {
                format!(": {detail}")
            }
        );
    }
    if output.stdout.iter().all(u8::is_ascii_whitespace) {
        bail!("PDF contains no extractable text");
    }
    Ok(output.stdout)
}

fn extract_pdf_text(bytes: &[u8]) -> Result<String> {
    let text = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(bytes))
        .map_err(|_| anyhow::anyhow!("PDF parser rejected malformed input"))?
        .map_err(|error| anyhow::anyhow!("PDF parser rejected input: {error}"))?;
    if text.len() > MAX_EXTRACTED_TEXT_BYTES {
        bail!("PDF extracted text exceeds the 4 MiB limit");
    }
    if text.trim().is_empty() {
        bail!("PDF contains no extractable text");
    }
    Ok(text)
}

/// Runs in a dedicated child process so parser hangs/panics cannot wedge the CLI.
pub fn extract_pdf_worker() -> Result<std::process::ExitCode> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take((MAX_RESOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .context("reading PDF input")?;
    if bytes.len() > MAX_RESOURCE_BYTES {
        bail!("PDF exceeds the 8 MiB input limit");
    }

    let text = extract_pdf_text(&bytes)?;
    std::io::stdout()
        .write_all(text.as_bytes())
        .context("writing extracted PDF text")?;
    Ok(std::process::ExitCode::SUCCESS)
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
        let output = turtle_exec::run(
            CommandSpec::new("curl")
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
                .timeout(Duration::from_secs(30))
                .max_output_bytes(MAX_RESOURCE_BYTES + 1),
        )
        .await
        .context("fetching URL snapshot with curl")?;
        if matches!(output.outcome, ExitOutcome::TimedOut) {
            bail!("URL snapshot timed out after 30 seconds");
        }
        if output.truncated || output.stdout.len() > MAX_RESOURCE_BYTES {
            bail!("resource exceeds the 8 MiB ingestion limit");
        }
        if !output.success() {
            bail!(
                "URL snapshot failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        output.stdout
    } else {
        read_limited(input)?
    };
    if raw.len() > MAX_RESOURCE_BYTES {
        bail!("resource exceeds the 8 MiB ingestion limit");
    }
    let pdf = is_pdf(input, &raw);
    let text_bytes = if pdf {
        extract_pdf(&raw).await?
    } else {
        raw.clone()
    };
    let (sanitized, extracted) = sanitize_text(&text_bytes)?;
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
        kind: if pdf {
            "pdf"
        } else if is_url {
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

    fn text_pdf(text: &str) -> Vec<u8> {
        let stream = format!("BT /F1 12 Tf 72 720 Td ({text}) Tj ET");
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_owned(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
            format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0usize];
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", offsets.len()).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                offsets.len()
            )
            .as_bytes(),
        );
        pdf
    }

    #[test]
    fn resource_ingest_redacts_before_hashable_bytes_are_returned() {
        let secret = b"api_key=ghp_synthetic_token_12345678901234567890";
        let (stored, indexed) = sanitize_text(secret).expect("text resource");
        assert!(!String::from_utf8_lossy(&stored).contains("ghp_synthetic_token"));
        assert!(indexed.contains("[REDACTED:api_key]"));
        assert!(sanitize_text(&[0xff]).is_err());
    }

    #[test]
    fn pdf_detection_uses_signature_or_case_insensitive_extension() {
        assert!(is_pdf("guide.PDF", b"not a pdf"));
        assert!(is_pdf("download", b"%PDF-1.7"));
        assert!(!is_pdf("guide.txt", b"plain text"));
    }

    #[test]
    fn malformed_pdf_is_rejected_by_the_worker_parser() {
        let bad = b"%PDF-1.7\nnot a valid document";
        assert!(extract_pdf_text(bad).is_err());
    }

    #[test]
    fn pdf_text_is_extracted_and_redacted_before_storage() {
        let pdf = text_pdf("api_key=ghp_synthetic_token_12345678901234567890");
        let text = extract_pdf_text(&pdf).expect("valid text PDF");
        let (stored, indexed) = sanitize_text(text.as_bytes()).expect("redacted text");
        assert!(!String::from_utf8_lossy(&stored).contains("ghp_synthetic_token"));
        assert!(indexed.contains("[REDACTED:api_key]"));
    }

    #[test]
    fn image_only_or_empty_text_pdf_is_rejected() {
        let pdf = text_pdf("");
        assert!(extract_pdf_text(&pdf).is_err());
    }

    #[test]
    fn extracted_text_limit_is_enforced() {
        let oversized = "x".repeat(MAX_EXTRACTED_TEXT_BYTES + 1);
        let pdf = text_pdf(&oversized);
        assert!(extract_pdf_text(&pdf).is_err());
    }
}
