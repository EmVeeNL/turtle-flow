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
turtle up --agent claude
turtle project add ~/dev/payments-api
turtle job create -p payments-api "Add per-client rate limiting" --start
turtle approvals
```

## Status

**Early development.** Milestone M1 (the walking skeleton) is complete and M2 (the first real agent) is in progress. Today you can register a Git repository, create a job and watch Claude Code take it from a specification through approved scope, plan, implementation in an isolated worktree, build and tests, AI review and verification to a delivered commit, with every step in a hash-chained audit log. It has been run for real only on small jobs.

| Milestone | State |
|---|---|
| M0 spikes | Claude Code headless contract verified; Codex error path only (usage-limited). S-3 to S-7 not started. |
| **M1 walking skeleton** | **Complete.** Full loop with real Git, gates, hash-bound approvals, crash recovery and the `turtle` CLI. |
| M2 first real agent | In progress: Claude Code adapter and token ledger with budgets work; the estimator and the context builder are next. |
| M3 and later | Not started: Codex, review routing, rate-limit handling, dashboard API, strict profile, releases. |

Details and the list of known simplifications: [docs/MVP.md §6.1](docs/MVP.md#61-progress-against-the-milestones-2026-10-25). The dashboard UI (`dashboard/`) is built against a mock API; no server serves it yet (M4).

What works:

- `turtle doctor`, `project add|list|show`, `job create|list|show|events|watch|start|pause|resume|cancel`, `approvals` and `approve` (interactive terminal only, you type the bound hash back), `budget show|set`, `audit verify`, `up|down|status`
- `turtle up --agent claude` runs real jobs through Claude Code (headless, isolated, with structured output and a per-run spend cap); `--agent demo` runs a scripted placeholder agent so you can watch the workflow without spending tokens
- a token ledger: every run is recorded with its usage as measured, estimated or unknown; budgets per job, project (monthly) and globally are reserved before each run and settled after it, a job that runs out of budget waits for you (`turtle budget set`), and `turtle audit verify` checks the ledger as well as the event chain
- the workflow state machine, SQLite store with hash-chained events, effect outbox with leases and crash recovery (crash-injection tested at every step), Git worktrees with factory-only commits and squash delivery, content-addressed artifacts and hash-bound approvals

What does not exist yet: token estimation and preflight (reservations use a fixed guess per role), currency budgets, Codex integration, rate-limit handling, the HTTP API (the CLI reads the database directly), `turtle init`, the dashboard server, licensing.

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
