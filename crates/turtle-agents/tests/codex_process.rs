//! The Codex process boundary: flags, schema file lifetime, prompt, environment and usage.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use turtle_agents::{AgentRequest, AgentRunner, CodexConfig, CodexRunner};
use turtle_domain::{Role, RunId, UsageValue};

fn request(workdir: &Path) -> AgentRequest {
    AgentRequest {
        run_id: RunId::new(),
        role: Role::Discovery,
        job_key: "API-1".into(),
        control_key: None,
        title: "Find files".into(),
        description: "List the layout".into(),
        workdir: Some(workdir.to_path_buf()),
        spec: None,
        plan: None,
        task: None,
        feedback: None,
        diff_summary: None,
        context: None,
        spool_path: None,
    }
}

#[tokio::test]
async fn codex_receives_a_private_schema_and_scrubbed_environment() {
    let dir = tempfile::tempdir().expect("tempdir");
    let work = tempfile::tempdir().expect("workdir");
    let script = dir.path().join("codex");
    let seen = dir.path().join("seen");
    let fixture = format!(
        "{}/../../docs/spikes/fixtures/codex/02-readonly-structured.ndjson",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::write(
        &script,
        format!(
            r##"#!/bin/sh
cat > {seen}.stdin
printf '%s\n' "$@" > {seen}.args
pwd > {seen}.cwd
env | sort > {seen}.env
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--output-schema" ]; then
    shift
    echo "$1" > {seen}.schema_path
    cp "$1" {seen}.schema
  fi
  shift
done
cat {fixture}
"##,
            seen = seen.display()
        ),
    )
    .expect("script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let runner = CodexRunner::new(CodexConfig {
        program: script,
        model: Some("gpt-6-luna".into()),
        effort: Some("low".into()),
        inherit_env: vec!["CARGO_MANIFEST_DIR".into()],
        ..CodexConfig::default()
    });
    let out = runner.run(&request(work.path())).await.expect("run");
    assert!(
        matches!(out.usage, UsageValue::Reported { usage } if usage.input == 18_083 && usage.total() == 52_729)
    );
    let args = std::fs::read_to_string(format!("{}.args", seen.display())).expect("args");
    for flag in [
        "exec",
        "--json",
        "--ephemeral",
        "--ignore-user-config",
        "--output-schema",
        "read-only",
        "-C",
        "-",
    ] {
        assert!(args.lines().any(|a| a == flag), "missing {flag}: {args}");
    }
    assert!(args.contains("model_reasoning_effort=low"));
    let schema: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{}.schema", seen.display())).expect("schema"),
    )
    .expect("json");
    let properties = schema["properties"].as_object().expect("properties");
    let required = schema["required"].as_array().expect("required");
    assert_eq!(required.len(), properties.len());
    let schema_path =
        std::fs::read_to_string(format!("{}.schema_path", seen.display())).expect("path");
    assert!(
        !Path::new(schema_path.trim()).exists(),
        "schema scratch directory must be removed"
    );
    let stdin = std::fs::read_to_string(format!("{}.stdin", seen.display())).expect("stdin");
    assert!(stdin.contains("List the layout") && stdin.contains("API-1"));
    let cwd = std::fs::read_to_string(format!("{}.cwd", seen.display())).expect("cwd");
    assert_eq!(
        Path::new(cwd.trim()).canonicalize().expect("cwd"),
        work.path().canonicalize().expect("workdir")
    );
    let env = std::fs::read_to_string(format!("{}.env", seen.display())).expect("env");
    assert!(env.contains("CARGO_MANIFEST_DIR="));
    assert!(!env.contains("CARGO_PKG_NAME"), "leaked environment: {env}");
}
