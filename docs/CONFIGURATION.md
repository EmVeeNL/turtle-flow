# Configuration reference

## Data directory

`TURTLE_HOME` selects the local state directory. If unset, Turtle Flow uses `~/.turtle`. The directory contains the SQLite database, content-addressed blobs, job worktrees, daemon state and logs. Turtle Flow creates the directory with owner-only permissions when a stateful command is first run.

## Project settings

Project records and settings are stored in the local database; `.turtle/project.toml` is not currently read. Register or inspect a project with `turtle project add` and `turtle project show`. Add explicit gate commands with `--build`, `--test`, `--lint` and `--worktree-setup`. All command flags are shell command lines executed in the isolated job worktree.

Supported project settings include:

| Setting | Values / purpose |
|---|---|
| `commands.build`, `test`, `lint`, `format_check` | Optional gate command strings |
| `commands.worktree_setup` | Optional one-time setup command for each job worktree |
| `delivery` | `local_squash` (default) or `pr` |
| `target_branch` | Delivery target; defaults to the registered project branch |
| `branch_prefix` | Optional generated job branch prefix |
| `gate_timeout_secs` | Optional gate timeout |
| `protected_paths` | Optional replacement for default protected paths |
| `env_allowlist` | Names of extra environment variables passed to gates |
| `static_gates` | Secret scan, protected-path, diff-size, scope and dependency checks |

Commands: `turtle project delivery <slug> local-squash|pr` changes delivery mode. `turtle project static-gates --help` shows flags for deterministic diff checks.

## Daemon and agents

Set provider and daemon options on `turtle up` (or `turtle daemon`): `--agent`, `--role-agent`, `--claude-model`, `--claude-effort`, `--codex-model`, `--codex-effort`, `--max-run-usd`, `--api-port`, `--no-api`, and `--tailnet-host`. See generated [CLI help](CLI-REFERENCE.md) for supported values. Claude quota preference can be adjusted with `TURTLE_CLAUDE_QUOTA_THRESHOLD`, a fraction from 0 through 1; the default is 0.9.

The project settings data model is [settings.rs](../crates/turtle-domain/src/settings.rs). Settings are private local state and are not synchronized between machines.
