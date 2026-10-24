# ADR-009: Git Worktree Isolation

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

Implementation jobs must not disturb the user's working checkout (FR-102) and concurrent jobs must not collide (US-18). Git is the delivery mechanism and audit anchor (FR-100).

## Problem Statement

How should jobs be isolated at the source-control level, and who performs Git mutations?

## Decision

1. **One Git worktree and one branch per job** (`turtle/<job-key>-<slug>`), created from a recorded base commit under `~/.turtle/worktrees/<project>/<job-key>/`.
2. **Turtle Flow performs all Git mutations** (commits, rebases, merges, pushes, PRs). Agents may only edit files; post-run checks enforce that refs/HEAD/branch are unchanged.
3. **Git CLI** for all mutations with hardened configuration (hooks disabled, fsmonitor off, restricted protocols); per-repository serialized mutation worker.
4. **Delivery** via PR (`gh`) or local plumbing-based squash/ff/merge with compare-and-swap ref updates; never modify a branch checked out in the user's main worktree unless explicitly enabled and clean ([GIT-STRATEGY.md §5](../GIT-STRATEGY.md#5-delivery-policies)).
5. **Conflict detection** via non-mutating `git merge-tree --write-tree`.
6. **Worktree isolation is explicitly not a security boundary**; security isolation is ADR-012.

## Considered Alternatives

| Alternative | Assessment |
|---|---|
| Full clones per job | Strong separation, but disk/time heavy; object duplication; separate remotes config |
| Branch switching in the main checkout | Disrupts user; impossible to run jobs concurrently |
| Copy-on-write directory snapshots (APFS clones, overlayfs) | Fast, but platform-specific; Git state handling still needed |
| Containers with bind-mounted clones | Good for security tier later; not needed for change isolation |
| Agents commit themselves | Loses deterministic commit policy, trailers, and enables history manipulation |
| libgit2 (git2) for mutations | Worktree and merge-tree parity gaps; behavioral drift from user's Git |

## Rationale

Worktrees share the object database (cheap), are a native Git concept, and support concurrency. Turtle Flow-owned commits give consistent lineage and make policy checks simple.

## Consequences

- Fresh worktrees need dependency setup (`worktree_setup`); build caches per worktree.
- Shared refs/gc require a serialized mutation worker.
- Some Git features unsupported in MVP (submodule changes, shallow repos for local delivery).

## Risks

- Interrupted Git operations leave locks or half-done rebases.
- Users manually operate in Turtle Flow worktrees.

## Mitigations

- Recovery probes ([WORKFLOW.md §10.1](../WORKFLOW.md#101-step-boundaries-and-recovery-actions-crash-injection-catalogue)); never leave rebase in progress across effects.
- Manual commits are allowed when the job is `blocked` or paused and are recorded as `author_kind=user`; acceptance invalidated by any new commit.

## Related ADRs

[ADR-012](ADR-012-agent-execution-security.md), [ADR-017](ADR-017-recovery-idempotency-crash-safety.md), [ADR-006](ADR-006-supervised-autonomy-approvals.md), [ADR-011](ADR-011-job-scheduling-concurrency.md)
