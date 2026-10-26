# Beta Readiness Checklist

This checklist records the remaining release evidence for Turtle Flow 0.3. Feature status is summarized in [MVP.md §6.1](MVP.md#61-progress-against-the-milestones-2026-10-26); the release plan is [ROADMAP.md](ROADMAP.md).

## Integration and CI

- [x] Merge [PR #27](https://github.com/EmVeeNL/turtle-flow/pull/27), which brings stacked PRs #22–#26 onto `main`. The stack branches were merged into one another, so those merges alone did not advance `main` beyond PR #21.
- [x] Confirm integration checks pass: Rust fmt/clippy, Linux and macOS workspace tests, cargo-deny, dashboard checks, and Playwright E2E all passed on PR #27.

## M5 completeness and safety

- [ ] Implement per-task strict gates and the estimate-envelope plan approval threshold.
- [ ] Add PDF resource extraction with content limits, redaction, and negative/security tests.
- [ ] Add remote-base fetching with retry/recovery coverage.
- [ ] Complete the remaining security fixture matrix in [TESTING-STRATEGY.md §7](TESTING-STRATEGY.md#7-security-tests-tst-13). PR #29 added prompt-injection, unauthorized-commit/push, and context-budget regressions; broader injection and resource-limit coverage remains.
- [ ] Close the remaining recovery gaps: a sleeping/streaming provider subprocess fixture and power-loss simulation, or document explicitly approved exclusions.

PR merge polling (`turtle job sync --wait` with a bounded polling interval) and the focused PR #29 security regressions are integrated into `main`.

## Live end-to-end acceptance (AC-01)

The templates, benchmark prompts, hidden acceptance checks, preparation script, and provider preflight in [the live E2E checklist](LIVE-E2E-CHECKLIST.md) are integrated into `main` by PR #30. Run at least one `standard` job with Claude Code and one with Codex on **each** repository (six jobs total), with a per-job token ceiling and only the two expected human approvals. Preserve for each run the commit, CLI/provider version, configuration, budget, job export, test result, and delivery outcome. Do not treat FakeAdapter CI or synthetic provider tests as evidence for this criterion.

- [ ] `fixtures/ref-rust-cli`: live standard job and hidden acceptance tests.
- [ ] `fixtures/ref-ts-api`: live standard job and hidden acceptance tests.
- [ ] `fixtures/ref-py-lib`: live standard job and hidden acceptance tests.
- [ ] Exercise both Claude Code and Codex on all three repos; record real CLI versions and outcomes for all six jobs.

Live provider runs spend tokens and may modify only their disposable reference worktrees. Keep each run budget-capped and inspect the final diff before delivery.

## Performance acceptance (TST-19)

Run the benchmarks and record machine, OS, toolchain, DB fixture size, warm/cold state, and sample count. Compare with the targets in [TESTING-STRATEGY.md §11](TESTING-STRATEGY.md#11-performance-tests-tst-19):

- [ ] Daemon cold start with 1 GB DB: ≤ 1 s to API ready.
- [ ] CLI read commands: p95 ≤ 200 ms.
- [ ] Event to SSE client at 50 events/s: p95 ≤ 500 ms.
- [ ] Four fake runs streaming 200 lines/s each: no backpressure loss, CPU ≤ 1 core.
- [ ] Dashboard queries on 10k jobs / 1M events: p95 ≤ 200 ms.
- [ ] Context assembly for a 10k-file repo: ≤ 2 s.

If a benchmark is not yet implemented, track it as a release blocker instead of inferring performance from unit-test duration.

## Usability validation

Conduct a short moderated session with target developers who have not built the product. Ask each participant to install Turtle Flow, register a repository, create a job, understand and approve a bound scope/plan, inspect a run and diff, resolve an approval request, and find recovery/security guidance. Record task completion, time, errors, points of confusion, and participant feedback; do not collect source code or provider credentials. Fix blockers and rerun the affected tasks before beta publication.

- [ ] Recruit and run sessions with representative developers.
- [ ] Record findings and prioritized follow-up issues.
- [ ] Verify the critical install, approval, review, and recovery tasks after fixes.

## Release artifact verification

The current tag workflow builds archives for macOS arm64/x86_64 and Linux x86_64/aarch64 and publishes them as a GitHub draft release with SHA-256 sidecars. The checksum detects accidental corruption but is not an authenticity signature. The installer downloads the archive and sidecar from the same release endpoint, verifies the digest, and installs to `~/.local/bin` by default.

Before publishing a draft, maintainers can manually inspect and smoke-test an archive by running `scripts/check-release-archive.sh turtle-<target>.tar.gz` from the directory containing the archive and its `.sha256` sidecar. Pass the sidecar path as a second argument if it has a different name or location. The script checks that the archive contains only an executable root-level `turtle`, verifies the sidecar, and runs `turtle --version` when the archive target matches the host; cross-target archives receive structural and checksum checks only.

- [ ] On each supported platform, install a draft artifact into a temporary directory, verify its checksum, and run `turtle --version` and `turtle doctor --json`.
- [ ] Verify upgrade/reinstall behavior, unsupported-platform errors, and that existing installs are not modified when download or checksum verification fails.
- [ ] Review release notes, archive names, and sidecar naming before publishing the GitHub draft.
- [ ] Sign/notarize macOS binaries and staple/validate the ticket using the approved Apple Developer identity. Keep signing secrets outside the repository and its logs.
- [ ] Decide whether Linux artifact signing is required for the beta distribution channel; if required, sign and publish verifiable signatures.

Do not call the artifacts signed or publish them as trusted signed binaries until signature verification is part of the release and install process. The installer currently verifies checksums only.
