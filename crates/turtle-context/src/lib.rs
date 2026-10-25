//! Deterministic, redacted repository context packages (v1).
//!
//! Selection priority is: requested files, instruction files and build manifests, README and
//! contribution guides, then the repository map. Lower priority items are dropped first when
//! the character-based token estimate exceeds the budget. Only `git ls-files` entries are read.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const DEFAULT_TOKEN_BUDGET: usize = 12_000;
const MAX_FILE_BYTES: u64 = 64 * 1024;
const MAX_MAP_BYTES: usize = 24 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestItem {
    pub path: String,
    pub bytes: usize,
    pub estimated_tokens: usize,
    pub reason: String,
    pub redactions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExcludedItem {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextManifest {
    pub version: u32,
    pub estimated_tokens: usize,
    pub content_sha256: String,
    pub included: Vec<ManifestItem>,
    pub excluded: Vec<ExcludedItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPackage {
    pub text: String,
    pub manifest: ContextManifest,
}

#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("git ls-files failed: {0}")]
    Git(String),
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid glob {0:?}")]
    Glob(String),
}

pub type TokenEstimator = fn(&str) -> usize;

fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Replace with a provider tokenizer later; v1 deliberately uses the documented chars/4 proxy.
#[must_use]
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

fn patterns(globs: &[String]) -> Result<GlobSet, ContextError> {
    let mut builder = GlobSetBuilder::new();
    for glob in globs {
        builder.add(Glob::new(glob).map_err(|_| ContextError::Glob(glob.clone()))?);
    }
    builder
        .build()
        .map_err(|_| ContextError::Glob("set".into()))
}

fn secret_name(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    let in_secrets_dir = path
        .split('/')
        .any(|component| component.eq_ignore_ascii_case("secrets"));
    name.starts_with(".env")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.starts_with("id_")
        || name.contains("credential")
        || name == "secrets.toml"
        || name == ".npmrc"
        || name == ".pypirc"
        || name.ends_with(".p12")
        || name.ends_with(".pfx")
        || name.ends_with(".p8")
        || name.ends_with(".keystore")
        || name.ends_with(".tfstate")
        || in_secrets_dir
}

fn tracked_files(root: &Path) -> Result<Vec<String>, ContextError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .map_err(|e| ContextError::Git(e.to_string()))?;
    if !output.status.success() {
        return Err(ContextError::Git(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    let mut paths: Vec<_> = output
        .stdout
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect();
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn key_priority(path: &str) -> Option<(u8, &'static str)> {
    let basename = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    if basename == "agents.md" || basename == "claude.md" || basename == "gemini.md" {
        Some((1, "repository instructions"))
    } else if matches!(
        basename.as_str(),
        "cargo.toml"
            | "package.json"
            | "pyproject.toml"
            | "go.mod"
            | "makefile"
            | "justfile"
            | "cargo.lock"
    ) {
        Some((2, "build manifest"))
    } else if matches!(
        basename.as_str(),
        "readme" | "readme.md" | "readme.rst" | "contributing.md" | "contributing.rst"
    ) {
        Some((3, "project overview"))
    } else {
        None
    }
}

/// Build a deterministic package from tracked repository files, selected paths, and key docs.
pub fn build(
    root: &Path,
    affected_areas: &[String],
    relevant_files: &[String],
    token_budget: usize,
) -> Result<ContextPackage, ContextError> {
    build_with_estimator(
        root,
        affected_areas,
        relevant_files,
        token_budget,
        estimate_tokens,
    )
}

pub fn build_with_estimator(
    root: &Path,
    affected_areas: &[String],
    relevant_files: &[String],
    token_budget: usize,
    estimator: TokenEstimator,
) -> Result<ContextPackage, ContextError> {
    let files = tracked_files(root)?;
    let affected = patterns(affected_areas)?;
    let requested: BTreeSet<String> = relevant_files.iter().cloned().collect();
    let mut chosen: BTreeMap<String, (u8, String)> = BTreeMap::new();
    for path in &files {
        if secret_name(path) {
            continue;
        }
        if requested.contains(path) || affected.is_match(path) {
            chosen.insert(
                path.clone(),
                (
                    0,
                    if requested.contains(path) {
                        "discovery relevant file"
                    } else {
                        "specification affected area"
                    }
                    .into(),
                ),
            );
        }
        if let Some((priority, reason)) = key_priority(path) {
            chosen
                .entry(path.clone())
                .or_insert((priority, reason.into()));
        }
    }
    let mut excluded = Vec::new();
    for path in &files {
        if secret_name(path) {
            excluded.push(ExcludedItem {
                path: path.clone(),
                reason: "secret-like filename".into(),
            });
        } else if !chosen.contains_key(path) {
            excluded.push(ExcludedItem {
                path: path.clone(),
                reason: "not selected by v1 policy".into(),
            });
        }
    }

    let mut contents: Vec<(u8, String, String, String, usize)> = Vec::new();
    for (path, (priority, reason)) in chosen {
        let full = root.join(&path);
        let metadata = std::fs::symlink_metadata(&full).map_err(|source| ContextError::Read {
            path: full.clone(),
            source,
        })?;
        if !metadata.is_file() {
            excluded.push(ExcludedItem {
                path,
                reason: "not a regular file".into(),
            });
            continue;
        }
        if metadata.len() > MAX_FILE_BYTES {
            excluded.push(ExcludedItem {
                path,
                reason: "file exceeds 64 KiB limit".into(),
            });
            continue;
        }
        let bytes =
            std::fs::read(&full).map_err(|source| ContextError::Read { path: full, source })?;
        let Ok(raw) = String::from_utf8(bytes) else {
            excluded.push(ExcludedItem {
                path,
                reason: "not UTF-8 text".into(),
            });
            continue;
        };
        let (clean, redactions) = redact(&raw);
        contents.push((priority, path, reason, clean, redactions));
    }
    contents.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));

    let mut map_entries = BTreeMap::<String, (usize, u64)>::new();
    for path in &files {
        if secret_name(path) {
            continue;
        }
        let size = std::fs::symlink_metadata(root.join(path))
            .map(|m| m.len())
            .unwrap_or(0);
        let components: Vec<_> = path.split('/').collect();
        let key = if components.len() > 5 {
            format!("{}/…/", components[..5].join("/"))
        } else {
            path.clone()
        };
        let entry = map_entries.entry(key).or_default();
        entry.0 += 1;
        entry.1 += size;
    }
    let mut map = String::from("# Repository map (tracked files)\n");
    let mut map_truncated = false;
    for (path, (count, size)) in map_entries {
        let collapsed = count > 1 || path.ends_with("/…/");
        let depth = path
            .matches('/')
            .count()
            .saturating_sub(usize::from(collapsed));
        let line = if collapsed {
            let noun = if count == 1 { "file" } else { "files" };
            format!(
                "{}{} ({count} {noun}, {size} bytes)\n",
                "  ".repeat(depth.min(4)),
                path
            )
        } else {
            format!("{}{} ({size} bytes)\n", "  ".repeat(depth.min(4)), path)
        };
        if map.len() + line.len() > MAX_MAP_BYTES {
            map_truncated = true;
            break;
        }
        map.push_str(&line);
    }
    if map_truncated {
        map.push_str("… (map size limit reached)\n");
        excluded.push(ExcludedItem {
            path: "<repository-map-tail>".into(),
            reason: "repository map size limit".into(),
        });
    }

    let mut included = Vec::new();
    let mut sections = Vec::new();
    let mut used = 0usize;
    for (_, path, reason, content, redactions) in contents {
        let block = format!("# File: {path}\n{content}\n");
        let tokens = estimator(&block);
        let separator = if sections.is_empty() {
            0
        } else {
            estimator("\n")
        };
        if used.saturating_add(separator).saturating_add(tokens) > token_budget {
            excluded.push(ExcludedItem {
                path,
                reason: "token budget".into(),
            });
            continue;
        }
        used += separator + tokens;
        included.push(ManifestItem {
            path,
            bytes: block.len(),
            estimated_tokens: tokens,
            reason,
            redactions,
        });
        sections.push(block);
    }
    let map_tokens = estimator(&map);
    let separator = if sections.is_empty() {
        0
    } else {
        estimator("\n")
    };
    if used.saturating_add(separator).saturating_add(map_tokens) <= token_budget {
        included.push(ManifestItem {
            path: "<repository-map>".into(),
            bytes: map.len(),
            estimated_tokens: map_tokens,
            reason: "tracked file tree".into(),
            redactions: 0,
        });
        sections.push(map);
    } else {
        excluded.push(ExcludedItem {
            path: "<repository-map>".into(),
            reason: "token budget".into(),
        });
    }
    excluded.sort_by(|a, b| a.path.cmp(&b.path));
    let text = sections.join("\n");
    let digest = sha256_hex(&text);
    let estimated_tokens = estimator(&text);
    Ok(ContextPackage {
        text,
        manifest: ContextManifest {
            version: 1,
            estimated_tokens,
            content_sha256: digest,
            included,
            excluded,
        },
    })
}

/// Redact common credentials while avoiding bare look-alike identifiers in ordinary code.
#[must_use]
pub fn redact(input: &str) -> (String, usize) {
    let mut out = String::with_capacity(input.len());
    let mut count = 0;
    let mut lines = input.lines().peekable();
    while let Some(line) = lines.next() {
        if line.contains("-----BEGIN ") && line.contains("PRIVATE KEY-----") {
            out.push_str("[REDACTED:private_key]\n");
            count += 1;
            for next in lines.by_ref() {
                if next.contains("-----END ") && next.contains("PRIVATE KEY-----") {
                    break;
                }
            }
            continue;
        }
        let (redacted, n) = redact_line(line);
        count += n;
        out.push_str(&redacted);
        if lines.peek().is_some() {
            out.push('\n');
        }
    }
    (out, count)
}

fn redact_line(line: &str) -> (String, usize) {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut out = String::with_capacity(line.len());
    let mut count = 0;
    while i < bytes.len() {
        let tail = &line[i..];
        let assignment = [
            "api_key",
            "apikey",
            "secret_access_key",
            "client_secret",
            "access_token",
            "auth_token",
            "secret",
            "password",
            "passwd",
            "token",
        ]
        .iter()
        .find_map(|key| {
            assignment_value(tail, key, i == 0 || !bytes[i - 1].is_ascii_alphanumeric())
        });
        let jwt_len = token_len(tail, |s| s.split('.').count() == 3 && s.starts_with("eyJ"));
        let prefixed = [
            "AKIA",
            "ASIA",
            "AIza",
            "ya29.",
            "ghp_",
            "gho_",
            "github_pat_",
            "xoxb-",
            "xoxp-",
            "sk_live_",
            "rk_live_",
            "sk_test_",
        ]
        .iter()
        .find_map(|prefix| {
            tail.starts_with(prefix)
                .then(|| token_len(tail, |s| s.starts_with(prefix)))
        });
        if let Some((kind, start, end)) = assignment {
            out.push_str(&line[i..i + start]);
            out.push_str(&format!("[REDACTED:{kind}]"));
            i += end;
            count += 1;
        } else if jwt_len > 0 {
            out.push_str("[REDACTED:jwt]");
            i += jwt_len;
            count += 1;
        } else if let Some(len) = prefixed.filter(|n| *n >= 12) {
            let kind = if tail.starts_with("AKIA") || tail.starts_with("ASIA") {
                "aws_key"
            } else if tail.starts_with("AIza") || tail.starts_with("ya29.") {
                "google_token"
            } else if tail.starts_with("gh") {
                "github_token"
            } else if tail.starts_with("xox") {
                "slack_token"
            } else {
                "stripe_key"
            };
            out.push_str(&format!("[REDACTED:{kind}]"));
            i += len;
            count += 1;
        } else {
            let ch = tail.chars().next().expect("non-empty tail");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    (out, count)
}

// Return (assignment prefix length, full value end) when a credential assignment has a value.
fn assignment_value(
    tail: &str,
    key: &str,
    before_ok: bool,
) -> Option<(&'static str, usize, usize)> {
    let key_len = key.len();
    if !tail.get(..key_len)?.eq_ignore_ascii_case(key) {
        return None;
    }
    if !before_ok {
        return None;
    }
    let mut delimiter = key_len;
    let bytes = tail.as_bytes();
    if matches!(bytes.get(delimiter), Some(b'\'' | b'"')) {
        delimiter += 1;
    }
    while bytes.get(delimiter).is_some_and(u8::is_ascii_whitespace) {
        delimiter += 1;
    }
    if !matches!(bytes.get(delimiter), Some(b'=' | b':')) {
        return None;
    }
    let start = tail[delimiter + 1..]
        .find(|c: char| !c.is_whitespace())
        .map(|n| delimiter + 1 + n)?;
    if bytes[delimiter] == b':' && !matches!(bytes.get(start), Some(b'\'' | b'"')) {
        let value_len = tail[start..]
            .find(char::is_whitespace)
            .unwrap_or(tail.len() - start);
        if value_len < 12 {
            return None;
        }
    }
    let quote = tail.as_bytes()[start];
    let end = if quote == b'\'' || quote == b'"' {
        tail[start + 1..]
            .find(char::from(quote))
            .map(|n| start + n + 2)?
    } else {
        tail[start..]
            .find(|c: char| c.is_whitespace() || c == ';' || c == ',')
            .map_or(tail.len(), |n| start + n)
    };
    let kind = match key {
        "api_key" | "apikey" => "api_key",
        "password" | "passwd" => "password",
        "secret" | "secret_access_key" | "client_secret" => "secret",
        "access_token" | "auth_token" | "token" => "token",
        _ => "token",
    };
    Some((kind, start, end))
}

fn token_len(tail: &str, predicate: impl Fn(&str) -> bool) -> usize {
    let end = tail
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')))
        .unwrap_or(tail.len());
    if end >= 16 && predicate(&tail[..end]) {
        end
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        let git = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .output()
                .expect("git")
        };
        assert!(git(&["init", "-q"]).status.success());
        fs::write(dir.path().join("README.md"), "project docs\n").expect("write readme");
        fs::write(dir.path().join("src.rs"), "fn main() {}\n").expect("write source");
        assert!(git(&["add", "."]).status.success());
        dir
    }

    #[test]
    fn deterministic_package_and_snapshot_shape() {
        let dir = repo();
        let a = build(dir.path(), &[], &["src.rs".into()], 500).expect("package");
        let b = build(dir.path(), &[], &["src.rs".into()], 500).expect("package");
        assert_eq!(a, b);
        assert_eq!(
            a.text,
            "# File: src.rs\nfn main() {}\n\n# File: README.md\nproject docs\n\n# Repository map (tracked files)\nREADME.md (13 bytes)\nsrc.rs (13 bytes)\n"
        );
        assert_eq!(a.manifest.content_sha256, sha256_hex(&a.text));
    }

    #[test]
    fn filesystem_creation_order_does_not_change_package_or_hash() {
        let first = tempfile::tempdir().expect("temp dir");
        let second = tempfile::tempdir().expect("temp dir");
        for root in [first.path(), second.path()] {
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(root)
                    .args(["init", "-q"])
                    .output()
                    .expect("git init")
                    .status
                    .success()
            );
        }
        for (root, order) in [
            (first.path(), ["a.md", "z.rs"]),
            (second.path(), ["z.rs", "a.md"]),
        ] {
            for path in order {
                fs::write(
                    root.join(path),
                    if path.ends_with(".md") {
                        "docs\n"
                    } else {
                        "fn f() {}\n"
                    },
                )
                .expect("write fixture");
            }
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(root)
                    .args(["add", "."])
                    .output()
                    .expect("git add")
                    .status
                    .success()
            );
        }
        let a = build(first.path(), &[], &[], 500).expect("package A");
        let b = build(second.path(), &[], &[], 500).expect("package B");
        assert_eq!(a, b);
    }

    #[test]
    fn secret_file_omitted_and_tracked_file_secret_redacted() {
        let dir = repo();
        fs::write(dir.path().join(".env.local"), "PASSWORD=secret-env\n")
            .expect("write secret file");
        fs::write(dir.path().join("untracked.txt"), "untracked-secret-value\n")
            .expect("write untracked file");
        fs::write(
            dir.path().join("src.rs"),
            "const API_KEY: &str = \"ghp_123456789012345678901234567890\";\n",
        )
        .expect("write source");
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(["add", ".env.local", "src.rs"])
                .status()
                .expect("git add")
                .success()
        );
        let p = build(dir.path(), &[], &["src.rs".into()], 500).expect("package");
        assert!(!p.text.contains("ghp_123456789012345678901234567890"));
        assert!(!p.text.contains("secret-env"));
        assert!(!p.text.contains("untracked-secret-value"));
        assert!(
            !p.manifest
                .included
                .iter()
                .any(|item| item.path == "untracked.txt")
        );
        assert!(p.manifest.excluded.iter().any(|i| i.path == ".env.local"));
    }

    #[test]
    fn sixty_synthetic_credentials_do_not_survive() {
        let mut records = Vec::new();
        let mut secrets = Vec::new();
        for i in 0..60 {
            let aws = format!("AKIA{i:016}");
            let github = format!("ghp_synthetic{i:03}abcdefghijklmnop");
            let slack = format!("xoxb-synthetic{i:03}-abcdefghijklmnop");
            let stripe = format!("sk_live_synthetic{i:03}abcdefghijklmnop");
            let google = format!("AIzaSyntheticGoogle{i:03}abcdefghijklmnop");
            let aws_secret = format!("aws-synthetic-secret-{i:03}-abcdefghijklmnop");
            let password = format!("pass{i:03}wordabcdefghijkl");
            let jwt = format!(
                "eyJsynthetic{i:03}abcdefghijkl.eyJpayload{i:03}abcdefghijkl.signature{i:03}abcdefghijkl"
            );
            let generic = format!("synthetic-secret-{i:03}-abcdefghijklmnop");
            records.push(format!("api_key={generic}\n{{\"api_key\": \"{generic}\"}}\nAWS={aws}\nAWS_SECRET_ACCESS_KEY={aws_secret}\nGitHub={github}\nSlack={slack}\nStripe={stripe}\nGoogle={google}\npassword=\"{password}\"\nJWT={jwt}\n"));
            secrets.extend([
                aws, aws_secret, github, slack, stripe, google, password, jwt,
            ]);
            secrets.push(generic);
        }
        let all = records.join("");
        let (clean, replacements) = redact(&all);
        assert!(replacements >= 60 * 10);
        for secret in secrets {
            assert!(!clean.contains(&secret), "secret survived: {secret}");
        }
    }

    #[test]
    fn lookalike_identifiers_remain_untouched() {
        let input = "let api_key_count = 3;\nconst secret_name = \"placeholder\";\nlet token_bucket = 8;\nconst API_KEY: &str = \"placeholder\";";
        assert_eq!(redact(input), (input.into(), 0));
    }

    #[test]
    fn private_key_block_is_replaced_as_a_single_stable_marker() {
        let key = "-----BEGIN RSA PRIVATE KEY-----\nsynthetic-private-material\n-----END RSA PRIVATE KEY-----";
        assert_eq!(redact(key), ("[REDACTED:private_key]\n".into(), 1));
    }

    #[test]
    fn budget_records_dropped_items() {
        let dir = repo();
        let package = build(dir.path(), &[], &["src.rs".into()], 2).expect("package");
        assert!(
            package
                .manifest
                .excluded
                .iter()
                .any(|item| item.reason == "token budget")
        );
    }

    #[test]
    fn repository_map_collapses_deep_tracked_paths() {
        let dir = repo();
        let deep = dir.path().join("a/b/c/d/e/f.txt");
        fs::create_dir_all(deep.parent().expect("deep parent")).expect("deep dirs");
        fs::write(&deep, "deep\n").expect("deep file");
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(["add", "a/b/c/d/e/f.txt"])
                .output()
                .expect("git add")
                .status
                .success()
        );
        let package = build(dir.path(), &[], &[], 500).expect("package");
        assert!(package.text.contains("a/b/c/d/e/…/ (1 file, 5 bytes)"));
        assert!(!package.text.contains("a/b/c/d/e/f.txt (5 bytes)"));
    }
}
