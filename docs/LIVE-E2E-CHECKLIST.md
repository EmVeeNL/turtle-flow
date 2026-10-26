# Live E2E Reference-Repository Checklist

This checklist tracks pre-release criterion AC-01 / TST-16: run one `standard` job with Claude Code and one with Codex on each of the three reference repositories. A passing preflight is necessary but does not count as a live run.

## Before starting

1. Build or obtain a Turtle Flow binary from the exact commit being qualified.
2. Prepare disposable Git clones with `TURTLE_REFERENCE_ROOT=/tmp/turtle-flow-live-e2e-claude ./scripts/prepare-live-e2e.sh`. Create a second independent set for Codex using a different root. The checked-in templates are in `fixtures/reference-sources/`; benchmark prompts are in `fixtures/benchmarks/`; hidden acceptance checks are in `fixtures/acceptance/` and are never copied into the project clones.
3. Authenticate Claude Code and Codex in the same user session that will run Turtle Flow.
4. Run `TURTLE_REFERENCE_ROOT=/tmp/turtle-flow-live-e2e-claude ./scripts/live-e2e-preflight.sh`. It checks fixture Git repositories and provider authentication without printing account details or modifying projects.
5. Use a separate `TURTLE_HOME` for each provider. Register each fixture with deterministic gates. Example Rust setup: `turtle project add /tmp/turtle-flow-live-e2e-claude/ref-rust-cli --slug ref-rust-cli --profile standard --build 'cargo build --locked' --test 'cargo test --locked' --lint 'cargo clippy --all-targets --locked -- -D warnings'`. For TypeScript use `--build 'node --check test/smoke.mjs' --test 'npm test'`; for Python use `--build 'python3 -m compileall -q src' --test 'PYTHONPATH=src python3 -m unittest discover -s tests'`. Set a hard per-job token ceiling with `--budget <tokens>`. Claude runs can additionally use `turtle up --max-run-usd <amount>`; that currency cap applies to Claude, and Codex has no equivalent USD cap in the current CLI. Select exactly one provider in `turtle up --agent claude` or `turtle up --agent codex`; do not enable automatic failover during qualification.
6. Choose a benchmark prompt from `fixtures/benchmarks/<repo>/`. Create one standard job with that prompt using `turtle job create <title> --project <slug> --profile standard --budget <tokens> --file <prompt-path> --start`. Inspect and approve all required human gates. After delivery, run `scripts/check-live-e2e-acceptance.py <benchmark-id> <fixture-clone-path>` from the Turtle Flow checkout. Record the Turtle Flow commit, fixture commit, provider/model, token ceiling, job ID, approval IDs, run IDs, final delivery commit, gate output, and usage provenance. Do not record credentials or raw secrets.

## Run matrix

| Reference repository | Claude Code job | Codex job | Delivery and acceptance evidence |
|---|---|---|---|
| `ref-rust-cli` | ☐ | ☐ | ☐ |
| `ref-ts-api` | ☐ | ☐ | ☐ |
| `ref-py-lib` | ☐ | ☐ | ☐ |

For each cell, create a bounded benchmark job from the repository's benchmark suite, select `standard`, and run only the named provider. Inspect and approve the scope and acceptance bindings as a human. A pass requires successful delivery, deterministic gates passing, stored transcripts and usage provenance, and no hidden acceptance-test failure. A failed or cancelled attempt remains recorded and requires explicit explanation before a rerun is counted.

## After each run

- Verify `turtle audit verify` and export the job record.
- Confirm the delivery commit is present and the fixture's hidden acceptance tests pass against that commit.
- Record any retry, provider limitation, manual intervention, policy denial, or unexpected cost. Redact personal data from reports.
- Keep the fixture clean for the other provider; do not reuse a changed worktree as the second provider's starting point.

## Current environment check

As of 2026-10-26, the checkout includes all three small source templates, 15 benchmark prompts, and hidden acceptance checks kept outside the prepared clone directories. The templates pass their Rust, TypeScript, and Python smoke checks. The three provider-ready Git clones can be generated with `scripts/prepare-live-e2e.sh`; the preflight has passed against a disposable generated set with Claude Code and Codex authenticated. No live provider jobs have been started. Choose six prompts (one job per provider per repo), configure separate state directories and per-job token ceilings, then complete the matrix above. A Claude USD ceiling is optional; Codex currently has no equivalent currency cap. Human scope and acceptance approvals remain required.
