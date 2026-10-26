# Live E2E Reference-Repository Checklist

This checklist tracks pre-release criterion AC-01 / TST-16: run one `standard` job with Claude Code and one with Codex on each of the three reference repositories. A passing preflight is necessary but does not count as a live run.

## Before starting

1. Build or obtain a Turtle Flow binary from the exact commit being qualified.
2. Prepare disposable, clean Git clones for `ref-rust-cli`, `ref-ts-api`, and `ref-py-lib`. By default, place them under `fixtures/`; set `TURTLE_REFERENCE_ROOT` to use another directory. Keep generated acceptance tests and expected outputs outside the agent-visible working tree.
3. Authenticate Claude Code and Codex in the same user session that will run Turtle Flow.
4. Run `scripts/live-e2e-preflight.sh`. It checks CLI authentication without printing account details and does not modify projects.
5. Register each fixture with explicit, deterministic build/test/lint commands and the `standard` profile. Set a per-run budget cap appropriate for the provider account. Use separate jobs for each provider; do not let automatic failover turn one provider's result into another provider's result.
6. Record the Turtle Flow commit, fixture commit, provider/model, job ID, approval IDs, run IDs, final delivery commit, gate output, and usage provenance. Do not record credentials or raw secrets.

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

As of 2026-10-26, the repository checkout has no `fixtures/` directory, and no directories named `ref-rust-cli`, `ref-ts-api`, or `ref-py-lib` were found under `/Users/michael/Developer` or `/private/tmp`. Claude Code and Codex CLIs are installed and authenticated in the current user session. The six required live jobs were therefore not started. Put the disposable fixture repositories in place, rerun the preflight, then complete the matrix above.
