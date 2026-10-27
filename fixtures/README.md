# Live E2E fixtures

- `reference-sources/<repo>/` contains only the small, agent-visible baseline project.
- `benchmarks/<repo>/*.md` contains five bounded job prompts for each project.
- `acceptance/<repo>/` contains checks that remain in this checkout and are not copied into a project clone.
- `scripts/prepare-live-e2e.sh` copies each source into a fresh Git repository outside the checkout. Choose a different `TURTLE_REFERENCE_ROOT` for each provider so they start from identical baselines.
- `scripts/live-e2e-preflight.sh` checks that all three generated repositories are clean, the language toolchains are present, and both provider CLIs are authenticated.
- `scripts/check-live-e2e-acceptance.py <benchmark-id> <delivered-repo-path>` runs the selected hidden check after job delivery.

The suite is deliberately dependency-light: Rust uses only the standard library, TypeScript runs with Node's built-in type stripping, and Python uses the standard library. This keeps setup deterministic and avoids package downloads during live qualification.
