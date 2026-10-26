# Quickstart

## 1. Check the machine and register a project

Install Git (2.38 or later), Claude Code and/or Codex, then run:

```sh
turtle doctor
turtle providers list
cd /path/to/your/git/repository
turtle init
```

`turtle init` requires a terminal. It detects the current repository and suggests build, test and lint commands for Cargo, npm/pnpm, Python and Go projects. Review or edit every suggestion. Nothing is registered until you confirm. In scripts and other non-interactive sessions, use `turtle project add .` with explicit command flags instead.

## 2. Create and run a job

```sh
turtle job create "Add the feature I need" --start
turtle up --agent claude,codex
turtle job watch <key>
```

The key is printed by `job create`. Turtle Flow pauses at approval gates. Inspect each artifact and hash before approving with `turtle approvals` and `turtle approve <id>`. View the job using `turtle job show <key>`.

## 3. Open the dashboard

```sh
turtle dashboard
```

This command requires a terminal and prints a one-use local login URL. For a read-only phone session over your tailnet, start the daemon with Tailscale available and run:

```sh
turtle dashboard --phone
```

Approve the Tailscale Serve setup prompt if it appears, then scan the QR code from a device on the same tailnet. The dashboard session on the phone is read-only.

## 4. Export and inspect

```sh
turtle export job <key> --format md
turtle export job <key> --format json
turtle job worktree <key>
turtle audit verify
```

The export includes stored artifact content, approval bindings and hashes, audit events, run-level usage provenance and delivery information. Transcripts are represented by their content hash, not copied into the export. See the [CLI reference](CLI-REFERENCE.md) for all commands.
