# Security model summary

Turtle Flow runs locally and stores workflow state in a private SQLite database and content-addressed blob directory. It binds approvals to exact artifact hashes and records changes in an append-only, hash-chained event log. `turtle audit verify` checks the chain. Stored agent transcripts and gate outputs are redacted and size-capped before persistence. Job exports retain artifact hashes, redact free-text job fields and approval comments, and list event hashes without copying event payloads or transcript bodies.

Agents work in separate Git worktrees. Turtle Flow owns Git mutations and commits, hardens Git subprocess settings, and checks repository state around agent runs. Worktree isolation reduces accidental interference; it is not an operating-system security boundary. Gate commands and repository build scripts execute code and should be treated as trusted project code. Do not run Turtle Flow against an untrusted repository with credentials available to its commands.

The HTTP API binds to loopback by default and requires a one-use login code. Phone pairing uses Tailscale Serve and a read-only session; the daemon remains loopback-bound. Keep Tailscale Serve approval and device access limited to your tailnet.

The threat model, trust boundaries, and detailed controls are in [SECURITY.md](SECURITY.md). Turtle Flow does not currently provide OS sandboxing, signing/notarization of release binaries, automatic updates, telemetry, or a guarantee that redaction catches every secret format. See [GIT-STRATEGY.md §9](GIT-STRATEGY.md#9-security-considerations) for Git-specific controls.
