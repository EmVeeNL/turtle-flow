//! Runs `ClaudeRunner` against a stand-in `claude` script, to check what really crosses the
//! process boundary: the prompt on stdin, the arguments, the working directory and the
//! environment.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use turtle_agents::{AgentError, AgentRequest, AgentRunner, ClaudeConfig, ClaudeRunner};
use turtle_domain::{Role, RunId, UsageValue};

const STREAM: &str = r#"{"type":"system","subtype":"init","session_id":"s","claude_code_version":"9.9.9","model":"m","tools":["Read","Grep","Glob","StructuredOutput"],"mcp_servers":[]}
{"type":"result","subtype":"success","is_error":false,"num_turns":1,"total_cost_usd":0.01,"result":"{}","structured_output":{"summary":"found it","relevant_files":["src/lib.rs"]},"usage":{"input_tokens":120,"output_tokens":30,"cache_read_input_tokens":10,"cache_creation_input_tokens":5},"permission_denials":[]}
"#;

fn fake_claude(dir: &Path, body: &str) -> std::path::PathBuf {
    let path = dir.join("claude");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

fn request(workdir: Option<&Path>) -> AgentRequest {
    AgentRequest {
        run_id: RunId::new(),
        role: Role::Discovery,
        job_key: "API-1".into(),
        title: "Find the limiter".into(),
        description: "Where is rate limiting done?".into(),
        workdir: workdir.map(Path::to_path_buf),
        spec: None,
        plan: None,
        task: None,
        feedback: None,
        diff_summary: None,
        context: None,
    }
}

#[tokio::test]
async fn the_prompt_arguments_directory_and_environment_are_what_we_intend() {
    let dir = tempfile::tempdir().expect("tempdir");
    let work = tempfile::tempdir().expect("workdir");
    let seen = dir.path().join("seen");
    let stream = dir.path().join("stream.ndjson");
    std::fs::write(&stream, STREAM).expect("stream");
    let program = fake_claude(
        dir.path(),
        &format!(
            "cat > {seen}.stdin\nprintf '%s\\n' \"$@\" > {seen}.args\npwd > {seen}.cwd\nenv | sort > {seen}.env\ncat {stream}",
            seen = seen.display(),
            stream = stream.display()
        ),
    );

    // The test process carries both variables (cargo sets them); only one is allow-listed.
    let runner = ClaudeRunner::new(ClaudeConfig {
        program,
        inherit_env: vec!["CARGO_MANIFEST_DIR".into()],
        ..ClaudeConfig::default()
    });
    let out = runner
        .run(&request(Some(work.path())))
        .await
        .expect("run succeeds");

    assert_eq!(out.structured.expect("document")["summary"], "found it");
    assert!(
        matches!(out.usage, UsageValue::Reported { usage } if usage.input == 120 && usage.output == 30)
    );

    let stdin = std::fs::read_to_string(format!("{}.stdin", seen.display())).expect("stdin");
    assert!(stdin.contains("Where is rate limiting done?") && stdin.contains("API-1"));
    let args = std::fs::read_to_string(format!("{}.args", seen.display())).expect("args");
    assert!(
        args.contains("--restricted")
            && args.contains("--json-schema")
            && args.contains("--max-budget-usd")
    );
    let cwd = std::fs::read_to_string(format!("{}.cwd", seen.display())).expect("cwd");
    assert_eq!(
        Path::new(cwd.trim()).canonicalize().expect("cwd"),
        work.path().canonicalize().expect("work")
    );
    let env = std::fs::read_to_string(format!("{}.env", seen.display())).expect("env");
    assert!(
        env.contains("CARGO_MANIFEST_DIR="),
        "allow-listed variable is passed"
    );
    assert!(
        !env.contains("CARGO_PKG_NAME"),
        "everything else is scrubbed:\n{env}"
    );
}

#[tokio::test]
async fn a_failing_cli_surfaces_its_message_and_a_missing_cli_is_fatal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let work = tempfile::tempdir().expect("workdir");
    let program = fake_claude(
        dir.path(),
        r#"echo '{"type":"result","is_error":true,"result":"API Error: 529 overloaded"}'; exit 1"#,
    );
    let runner = ClaudeRunner::new(ClaudeConfig {
        program,
        ..ClaudeConfig::default()
    });
    match runner.run(&request(Some(work.path()))).await {
        Err(e) if e.retryable() => assert!(e.to_string().contains("529"), "{e}"),
        other => panic!("expected a retryable error, got {other:?}"),
    }

    let missing = ClaudeRunner::new(ClaudeConfig {
        program: dir.path().join("does-not-exist"),
        ..ClaudeConfig::default()
    });
    match missing.run(&request(Some(work.path()))).await {
        Err(AgentError::Fatal(m)) => assert!(m.contains("was not found"), "{m}"),
        other => panic!("expected a fatal error, got {other:?}"),
    }

    match runner.run(&request(None)).await {
        Err(AgentError::Fatal(m)) => assert!(m.contains("working directory")),
        other => panic!("expected a fatal error, got {other:?}"),
    }
}
