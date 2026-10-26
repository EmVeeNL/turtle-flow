//! GitHub PR operations. Every call uses argv (no shell), and the body comes from the
//! specification revision bound into the approved acceptance decision.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use turtle_domain::Specification;
use turtle_exec::CommandSpec;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrInfo {
    pub url: String,
    pub state: String,
    pub merged_at: Option<String>,
    pub head_ref_oid: Option<String>,
    pub body: Option<String>,
}

impl PrInfo {
    pub fn merged(&self) -> bool {
        self.state == "MERGED" && self.merged_at.is_some()
    }
}

#[derive(Debug, Clone)]
pub struct PrClient {
    program: PathBuf,
}

impl PrClient {
    pub fn new(program: PathBuf) -> Self {
        Self { program }
    }

    async fn run(&self, dir: &Path, args: &[&str]) -> Result<String, String> {
        let mut spec = CommandSpec::new(self.program.as_os_str())
            .args(args.iter().copied())
            .cwd(dir)
            .max_output_bytes(64 * 1024);
        spec.inherit_env = vec!["GH_TOKEN".into(), "GITHUB_TOKEN".into(), "GH_HOST".into()];
        let out = turtle_exec::run(spec).await.map_err(|e| e.to_string())?;
        if !out.success() {
            let raw = String::from_utf8_lossy(&out.stderr);
            return Err(turtle_context::redact(&raw).0);
        }
        Ok(out.stdout_lossy().trim().to_owned())
    }

    pub async fn find(&self, dir: &Path, branch: &str) -> Result<Option<PrInfo>, String> {
        let raw = self
            .run(
                dir,
                &[
                    "pr",
                    "list",
                    "--head",
                    branch,
                    "--state",
                    "all",
                    "--json",
                    "url,state,mergedAt,headRefOid,body",
                ],
            )
            .await?;
        let rows: Vec<PrInfo> = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        Ok(rows.into_iter().next())
    }

    pub async fn create(
        &self,
        dir: &Path,
        base: &str,
        head: &str,
        title: &str,
        body: &str,
    ) -> Result<String, String> {
        let file = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
        std::fs::write(file.path(), body).map_err(|e| e.to_string())?;
        let path = file.path().to_string_lossy().into_owned();
        let raw = self
            .run(
                dir,
                &[
                    "pr",
                    "create",
                    "--base",
                    base,
                    "--head",
                    head,
                    "--title",
                    title,
                    "--body-file",
                    &path,
                ],
            )
            .await?;
        let url = raw.lines().last().unwrap_or("").trim();
        if !url.starts_with("https://") || url.chars().any(char::is_whitespace) {
            return Err("gh did not return a valid HTTPS PR URL".into());
        }
        Ok(url.to_owned())
    }

    pub async fn view(&self, dir: &Path, url: &str) -> Result<PrInfo, String> {
        let raw = self
            .run(
                dir,
                &["pr", "view", url, "--json", "url,state,mergedAt,headRefOid"],
            )
            .await?;
        serde_json::from_str(&raw).map_err(|e| e.to_string())
    }
}

pub fn approved_body(
    spec: &Specification,
    scope: &str,
    plan: Option<&str>,
    acceptance: &str,
) -> String {
    let mut body = format!(
        "## Summary\n\n{}\n\n## Acceptance criteria\n\n",
        spec.summary.trim()
    );
    for criterion in &spec.criteria {
        body.push_str(&format!("- {}: {}\n", criterion.id, criterion.statement));
    }
    body.push_str(&format!("\n## Approval hashes\n\n- Scope: `{scope}`\n"));
    if let Some(hash) = plan {
        body.push_str(&format!("- Plan: `{hash}`\n"));
    }
    body.push_str(&format!("- Acceptance: `{acceptance}`\n"));
    turtle_context::redact(&body).0
}
