//! Exhaustive state × command table (TST-02). Any change to the state machine shows up as a
//! snapshot diff that must be reviewed. Update with `INSTA_UPDATE=always cargo test` and review
//! the diff in the pull request.

mod common;

use std::fmt::Write;

use common::{hostile, job_in, natural_actor, permissive, sample_commands};
use turtle_domain::JobState;
use turtle_workflow::{DecisionContext, Profile, decide};

fn render(cx: &DecisionContext) -> String {
    let mut out = String::new();
    for state in JobState::ALL {
        writeln!(out, "## {state:?}").expect("write to string");
        for cmd in sample_commands() {
            let job = job_in(state, Profile::standard());
            let label = serde_json::to_string(&cmd).expect("serialize command");
            let line = match decide(&job, &cmd, &natural_actor(&cmd), cx) {
                Ok(d) => {
                    let effects: Vec<String> = d
                        .effects
                        .iter()
                        .map(|e| serde_json::to_string(e).expect("serialize effect"))
                        .collect();
                    format!("ok -> {:?} [{}]", d.job.state, effects.join(", "))
                }
                Err(r) => format!("rejected: {}", r.guard),
            };
            writeln!(out, "{label} => {line}").expect("write to string");
        }
        out.push('\n');
    }
    out
}

#[test]
fn transition_table_permissive_context() {
    insta::assert_snapshot!(render(&permissive()));
}

#[test]
fn transition_table_hostile_context() {
    insta::assert_snapshot!(render(&hostile()));
}
