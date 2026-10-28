# Troubleshooting

Run `turtle doctor` first. It checks Git, provider CLI compatibility/auth state, capability snapshots (what changed since the previous snapshot, for example after a provider CLI upgrade), and the Turtle Flow data directory. `turtle providers list` shows detected provider capabilities and current recorded limit observations.

| Symptom | What to check |
|---|---|
| `turtle init` refuses to run | Run it from an interactive terminal. For automation, use `turtle project add` and pass gate commands explicitly. |
| Git is too old or `merge-tree` is unavailable | Install Git 2.38 or newer, then rerun `turtle doctor`. |
| No usable provider | Install and log into Claude Code or Codex, then run `turtle providers list`. |
| Job waits at an approval | Run `turtle approvals`, inspect the current artifact/hash with `turtle job show <key>`, then approve or request changes. |
| Build/test command fails in a job | Run `turtle job worktree <key>`, inspect the recorded gate output with `turtle job show <key>`, and update the project's configured commands if the suggestion was wrong. |
| Job is blocked by a conflict | Incorporate the displayed target branch into the printed job worktree, commit the resolution there, and run `turtle job resolve <key>`. |
| Local delivery is pending integration | Use the exact `git merge --ff-only` command printed by Turtle Flow, then inspect `turtle job show <key>`. |
| Dashboard is unavailable | Check `turtle status`, then restart with `turtle up --agent <provider>`. The dashboard defaults to loopback; `turtle dashboard --phone` requires a connected Tailscale setup. |
| Data directory permissions fail | Keep `TURTLE_HOME` on a local filesystem and restrict it to the current user (`chmod 700 "$TURTLE_HOME"`). |
| Usage is unknown | Provider CLIs may not report usage. Turtle Flow records unavailable values as unknown and does not invent token totals. |

For a potentially damaged event history, stop the daemon and run `turtle audit verify`. Preserve the data directory before attempting manual recovery.
