# Beta Readiness Checklist

This checklist defines the release evidence required for Turtle Flow 0.3. It records *what* must be shown and how; it does not track progress. Live status of each item is tracked in the [Turtle Flow - Roadmap GitHub Project](https://github.com/users/EmVeeNL/projects/9), and each section below names its tracking issue. The milestone snapshot is in [MVP.md §6.1](MVP.md#61-progress-against-the-milestones-2026-10-26); the release plan is [ROADMAP.md](ROADMAP.md).

## Integration and CI

Integrated into `main`: the supervised workflow and beta foundations (PR #27, which brought PRs #22–#26 onto `main`), strict per-task build/test/lint, secret-scan and scope gates with bounded fixer retries and the plan-estimate threshold against the configured job token limit (PR #33), and PDF resource extraction, remote-base fetching, expanded security fixtures and the stalled streaming provider fixture (PRs #34–#37, re-landed on `main` by PR #40). PR merge polling (`turtle job sync --wait` with a bounded polling interval) and the PR #29 security regressions are also on `main`.

The integration checks are Rust fmt/clippy, Linux and macOS workspace tests, cargo-deny, dashboard checks, and Playwright E2E.

## M5 completeness and safety

- Complete the remaining security fixture matrix in [TESTING-STRATEGY.md §7](TESTING-STRATEGY.md#7-security-tests-tst-13). PR #29 added prompt-injection, unauthorized-commit/push, and context-budget regressions; PR #36 added categorized injection cases and unbounded-stream limits. Tracking: [#50](https://github.com/EmVeeNL/turtle-flow/issues/50).
- Close the remaining recovery gaps: power-loss simulation, or an explicitly approved exclusion (AC-03). PR #37 added the sleeping/streaming provider fixture. Tracking: [#49](https://github.com/EmVeeNL/turtle-flow/issues/49).

## Live end-to-end acceptance (AC-01)

The templates, benchmark prompts, hidden acceptance checks, preparation script, and provider preflight are in [the live E2E checklist](LIVE-E2E-CHECKLIST.md). Run at least one `standard` job with Claude Code and one with Codex on **each** repository (six jobs total), with a per-job token ceiling and only the two expected human approvals. Preserve for each run the commit, CLI/provider version, configuration, budget, job export, test result, and delivery outcome. Do not treat FakeAdapter CI or synthetic provider tests as evidence for this criterion.

- `fixtures/ref-rust-cli`: live standard jobs with Claude Code and Codex, and hidden acceptance tests. Tracking: [#44](https://github.com/EmVeeNL/turtle-flow/issues/44).
- `fixtures/ref-ts-api`: live standard jobs with Claude Code and Codex, and hidden acceptance tests. Tracking: [#45](https://github.com/EmVeeNL/turtle-flow/issues/45).
- `fixtures/ref-py-lib`: live standard jobs with Claude Code and Codex, and hidden acceptance tests. Tracking: [#46](https://github.com/EmVeeNL/turtle-flow/issues/46).
- For all six jobs, record real CLI versions and outcomes.

Live provider runs spend tokens and may modify only their disposable reference worktrees. Keep each run budget-capped and inspect the final diff before delivery.

## Performance acceptance (TST-19)

Tracking: [#47](https://github.com/EmVeeNL/turtle-flow/issues/47).

Run the benchmarks and record machine, OS, toolchain, DB fixture size, warm/cold state, and sample count. Compare with the targets in [TESTING-STRATEGY.md §11](TESTING-STRATEGY.md#11-performance-tests-tst-19):

- Daemon cold start with 1 GB DB: ≤ 1 s to API ready.
- CLI read commands: p95 ≤ 200 ms.
- Event to SSE client at 50 events/s: p95 ≤ 500 ms.
- Four fake runs streaming 200 lines/s each: no backpressure loss, CPU ≤ 1 core.
- Dashboard queries on 10k jobs / 1M events: p95 ≤ 200 ms.
- Context assembly for a 10k-file repo: ≤ 2 s.

If a benchmark is not yet implemented, track it as a release blocker instead of inferring performance from unit-test duration.

## Usability validation (AC-10)

Tracking: [#48](https://github.com/EmVeeNL/turtle-flow/issues/48).

Conduct a short moderated session with target developers who have not built the product. Ask each participant to install Turtle Flow, register a repository, create a job, understand and approve a bound scope/plan, inspect a run and diff, resolve an approval request, and find recovery/security guidance. Record task completion, time, errors, points of confusion, and participant feedback; do not collect source code or provider credentials. Fix blockers and rerun the affected tasks before beta publication.

- Recruit and run sessions with representative developers.
- Record findings and prioritized follow-up issues.
- Verify the critical install, approval, review, and recovery tasks after fixes.

## Release artifact verification

The current tag workflow builds archives for macOS arm64/x86_64 and Linux x86_64/aarch64 and publishes them as a GitHub draft release with SHA-256 sidecars. The checksum detects accidental corruption but is not an authenticity signature. The installer downloads the archive and sidecar from the same release endpoint, verifies the digest, and installs to `~/.local/bin` by default.

Before publishing a draft, maintainers can manually inspect and smoke-test an archive by running `scripts/check-release-archive.sh turtle-<target>.tar.gz` from the directory containing the archive and its `.sha256` sidecar. Pass the sidecar path as a second argument if it has a different name or location. The script checks that the archive contains only an executable root-level `turtle`, verifies the sidecar, and runs `turtle --version` when the archive target matches the host; cross-target archives receive structural and checksum checks only.

- On each supported platform, install a draft artifact into a temporary directory, verify its checksum, and run `turtle --version` and `turtle doctor --json`. Tracking: [#53](https://github.com/EmVeeNL/turtle-flow/issues/53).
- Verify upgrade/reinstall behavior, unsupported-platform errors, and that existing installs are not modified when download or checksum verification fails. Tracking: [#53](https://github.com/EmVeeNL/turtle-flow/issues/53).
- Review release notes, archive names, and sidecar naming before publishing the GitHub draft. Tracking: [#53](https://github.com/EmVeeNL/turtle-flow/issues/53).
- Sign/notarize macOS binaries and staple/validate the ticket using the approved Apple Developer identity. Keep signing secrets outside the repository and its logs. Tracking: [#51](https://github.com/EmVeeNL/turtle-flow/issues/51).
- Decide whether Linux artifact signing is required for the beta distribution channel; if required, sign and publish verifiable signatures. Tracking: [#52](https://github.com/EmVeeNL/turtle-flow/issues/52).

Do not call the artifacts signed or publish them as trusted signed binaries until signature verification is part of the release and install process. The installer currently verifies checksums only.
