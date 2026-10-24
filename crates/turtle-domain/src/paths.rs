//! Path patterns: a small glob matcher, protected paths and dependency manifests
//! (`docs/PRD.md` SEC-011, SEC-016).
//!
//! Patterns: `*` matches within one path segment, `**` matches any number of segments, `?` one
//! character, and a trailing `/` means "everything below this directory".

/// Paths whose change needs explicit acknowledgement at acceptance (SEC-011).
pub const DEFAULT_PROTECTED_PATHS: &[&str] = &[
    ".github/**",
    ".turtle/**",
    "Cargo.lock",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "Dockerfile",
    "**/Dockerfile",
    "build.rs",
    "**/build.rs",
];

/// File names that declare dependencies; a change is flagged for review (SEC-016).
pub const DEPENDENCY_MANIFESTS: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "pyproject.toml",
    "requirements.txt",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "Gemfile",
];

fn segment_matches(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    fn go(p: &[char], t: &[char]) -> bool {
        match p.split_first() {
            None => t.is_empty(),
            Some(('*', rest)) => (0..=t.len()).any(|i| go(rest, &t[i..])),
            Some(('?', rest)) => !t.is_empty() && go(rest, &t[1..]),
            Some((c, rest)) => t.first() == Some(c) && go(rest, &t[1..]),
        }
    }
    go(&p, &t)
}

fn segments_match(pat: &[&str], text: &[&str]) -> bool {
    match pat.split_first() {
        None => text.is_empty(),
        Some((&"**", rest)) => (0..=text.len()).any(|i| segments_match(rest, &text[i..])),
        Some((p, rest)) => {
            !text.is_empty() && segment_matches(p, text[0]) && segments_match(rest, &text[1..])
        }
    }
}

/// Whether `path` (relative, `/`-separated) matches `pattern`.
#[must_use]
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
    let owned;
    let pattern = if pattern.ends_with('/') {
        owned = format!("{pattern}**");
        owned.as_str()
    } else {
        pattern
    };
    let pat: Vec<&str> = pattern.split('/').collect();
    let txt: Vec<&str> = path.split('/').collect();
    segments_match(&pat, &txt)
}

#[must_use]
pub fn is_protected(path: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| glob_match(p, path))
}

#[must_use]
pub fn is_dependency_manifest(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    DEPENDENCY_MANIFESTS.contains(&name)
}

/// The flags a human must acknowledge for these changed paths, sorted and de-duplicated.
#[must_use]
pub fn flags_for_changes(changed: &[String], protected: &[String]) -> Vec<String> {
    let mut flags: Vec<String> = changed
        .iter()
        .flat_map(|p| {
            let mut v = Vec::new();
            if is_protected(p, protected) {
                v.push(format!("protected_path:{p}"));
            }
            if is_dependency_manifest(p) {
                v.push(format!("dependency_manifest:{p}"));
            }
            v
        })
        .collect();
    flags.sort();
    flags.dedup();
    flags
}

/// Whether a planned path pattern stays inside the specification's declared areas. An empty
/// list of areas means the specification declared no restriction.
#[must_use]
pub fn within_areas(expected: &str, areas: &[String]) -> bool {
    areas.is_empty()
        || areas.iter().any(|a| {
            glob_match(a, expected)
                // A pattern such as `src/http/*.rs` is inside `src/http/**` even though it is
                // not itself a path: compare against its directory prefix.
                || expected
                    .rsplit_once('/')
                    .is_some_and(|(dir, _)| glob_match(a, &format!("{dir}/x")))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_basics() {
        assert!(glob_match("src/**", "src/a/b.rs"));
        assert!(
            glob_match("src/**", "src"),
            "`**` also matches zero segments"
        );
        assert!(glob_match("**/build.rs", "build.rs"));
        assert!(glob_match("**/build.rs", "crates/x/build.rs"));
        assert!(glob_match("*.rs", "main.rs"));
        assert!(
            !glob_match("*.rs", "src/main.rs"),
            "`*` does not cross directories"
        );
        assert!(glob_match("src/*.rs", "src/lib.rs"));
        assert!(glob_match("a?c", "abc") && !glob_match("a?c", "ac"));
        assert!(
            glob_match("docs/", "docs/a/b.md"),
            "trailing slash means everything below"
        );
        assert!(!glob_match("docs/", "other/a.md"));
        assert!(glob_match("./src/**", "src/x"));
        assert!(!glob_match("Cargo.lock", "sub/Cargo.lock"));
    }

    #[test]
    fn protected_and_manifest_flags() {
        let protected: Vec<String> = DEFAULT_PROTECTED_PATHS
            .iter()
            .map(ToString::to_string)
            .collect();
        let changed: Vec<String> = [
            "src/main.rs",
            "Cargo.lock",
            "Cargo.toml",
            ".github/workflows/ci.yml",
            "web/package.json",
            "crates/a/build.rs",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        assert_eq!(
            flags_for_changes(&changed, &protected),
            [
                "dependency_manifest:Cargo.toml",
                "dependency_manifest:web/package.json",
                "protected_path:.github/workflows/ci.yml",
                "protected_path:Cargo.lock",
                "protected_path:crates/a/build.rs",
            ]
        );
        assert!(flags_for_changes(&["src/lib.rs".into()], &protected).is_empty());
    }

    #[test]
    fn planned_paths_must_stay_in_the_declared_areas() {
        let areas = vec!["src/http/**".to_owned()];
        assert!(within_areas("src/http/middleware/limit.rs", &areas));
        assert!(
            within_areas("src/http/*.rs", &areas),
            "pattern inside the area"
        );
        assert!(!within_areas("src/auth/login.rs", &areas));
        assert!(!within_areas("Cargo.toml", &areas));
        assert!(
            within_areas("anything/at/all", &[]),
            "no declared areas means no restriction"
        );
    }
}
