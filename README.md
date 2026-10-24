<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/turtle-flow-wordmark-on-dark.png">
    <img src="docs/assets/turtle-flow-wordmark-on-light.png" alt="Turtle Flow" width="520">
  </picture>
</p>

<p align="center"><b>Supervised AI software delivery, orchestrated locally in Rust.</b></p>

---

Turtle Flow is a local-first, CLI-first orchestrator for AI coding agents. It drives locally installed agents — initially **Claude Code CLI** and **OpenAI Codex CLI** — through a supervised, auditable workflow:

```text
specification → scope approval → planning → implementation (isolated Git worktree)
→ build & tests → AI review → verification → acceptance approval → delivery
```

Rust owns the workflow state, policies, budgets, approvals and Git operations. The AI agents act as workers: they produce artifacts, and they never decide what happens next.

- **Hash-bound approvals.** You approve an exact spec or diff revision, and the approval becomes invalid when that revision changes.
- **Token intelligence.** Every agent run is estimated, reserved against a budget, measured and fed back into estimation and routing. Unknown provider quota is shown as unknown.
- **Crash-safe.** Every side effect is a recorded, idempotent step, so Turtle Flow resumes correctly after crashes, rate limits or reboots.
- **Local-first.** A single binary with SQLite. No account and no cloud dependency beyond the AI providers you already use.

```bash
turtle up
turtle project add ~/dev/payments-api
turtle job create -p payments-api "Add per-client rate limiting" --start
turtle approvals
```

## Status

**Early development (milestones M0/M1).** Turtle Flow is not usable yet. What exists so far:

- the Cargo workspace (13 crates)
- core domain types
- a process runner that scrubs the environment, enforces timeouts and kills whole process groups
- a Git version check
- provider CLI discovery
- parsers for Claude Code and Codex output, tested against real recorded transcripts ([spike report](spikes/S-1-S-2-provider-cli-report.md))
- the workflow state machine (`decide()`, profiles, approval binding)
- the SQLite store: migrations, hash-chained event log, effect queue, job persistence
- Git operations: worktrees per job, factory commits with trailers, post-run policy check, conflict probes, squash delivery, recovery snapshots
- the effect dispatcher: leases and heartbeats, retry with backoff, startup crash recovery, and a crash-injection test harness
- `turtle doctor`

There is no daemon process, CLI job commands or agent execution yet.

### Development

```bash
cargo test --workspace                                  # all tests
cargo clippy --workspace --all-targets -- -D warnings   # lints (enforced in CI)
cargo run -p turtle-flow -- doctor                      # check Git and provider CLIs
cargo deny check                                        # licenses, advisories, sources
```

The toolchain is pinned in `rust-toolchain.toml`. Crates live in `crates/turtle-*` (see [docs/ARCHITECTURE.md §4.1](docs/ARCHITECTURE.md#41-crate-responsibilities-and-boundaries)). Provider CLI fixtures are in `spikes/fixtures/`.

### Documentation

The architecture and product documentation package is in [`docs/`](docs/README.md):

| Start here | |
|---|---|
| [docs/README.md](docs/README.md) | Index, glossary, key decisions |
| [docs/PRD.md](docs/PRD.md) | Product requirements |
| [docs/MVP.md](docs/MVP.md) | MVP scope, epics, milestones |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | System architecture |
| [docs/adr/](docs/adr/README.md) | Architecture Decision Records |
| [docs/ARCHITECTURE-REVIEW.md](docs/ARCHITECTURE-REVIEW.md) | Top risks and next steps |

## Naming

| Thing | Name |
|---|---|
| Product | Turtle Flow |
| CLI binary | `turtle` |
| Cargo package | `turtle-flow` (crates prefixed `turtle-`) |
| Data directory | `~/.turtle/` (`TURTLE_HOME`) |
| Per-repo config | `.turtle/project.toml` |
| Job branches | `turtle/<job-key>-<slug>` |

## License

To be decided ([OQ-02](docs/PRD.md#26-open-questions): open-core vs. source-available). Until a license is added, all rights are reserved.
