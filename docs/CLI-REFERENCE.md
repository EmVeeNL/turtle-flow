# CLI reference

Generated from the current Clap command tree. Regenerate with `scripts/generate-cli-reference.sh`.

## `turtle`

```text
Supervised AI software delivery, orchestrated locally.

Usage: turtle <COMMAND>

Commands:
  login        Link this CLI to your Turtle Flow account license
  doctor       Check the environment: Git, provider CLIs, capability snapshots and the data directory
  init         Set up Turtle Flow for the current repository (interactive terminal required)
  export       Export a job's audit package
  completions  Generate shell completion scripts
  providers    Inspect available provider CLIs and verified capabilities
  up           Start the background daemon that runs jobs
  down         Stop the background daemon
  status       Say whether the daemon is running
  dashboard    Open the local dashboard using a one-time interactive login code
  daemon       Run the daemon in the foreground (what `up` starts)
  project      Register and inspect projects
  resource     Attach versioned resources and search them with full-text retrieval
  search       Search project resources with SQLite FTS5
  job          Create and follow jobs
  budget       Token budgets
  runs         Inspect agent runs and their redacted transcripts
  approvals    List what is waiting for your decision
  approve      Decide an approval (interactive terminal only)
  audit        Verify the tamper-evident event log
  help         Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

## `turtle doctor`

```text
Check the environment: Git, provider CLIs, capability snapshots and the data directory

Usage: turtle doctor [OPTIONS]

Options:
      --json  Print machine-readable JSON instead of a table
  -h, --help  Print help
```

Each `capabilities:<provider>` check shows the provider's current capability snapshot and its
difference from the previous one: CLI version change and added, removed or changed capabilities.
After a provider CLI upgrade that is not recorded yet, the check warns and compares the live set
with the latest stored snapshot; `turtle providers refresh` or a daemon start records it. With
`--json`, these checks carry a `snapshot` object with `current`, `previous` and `diff`. Doctor
reads the database read-only and never creates it.

## `turtle init`

```text
Set up Turtle Flow for the current repository (interactive terminal required)

Usage: turtle init

Options:
  -h, --help  Print help
```

## `turtle completions`

```text
Generate shell completion scripts

Usage: turtle completions <SHELL>

Arguments:
  <SHELL>  [possible values: bash, elvish, fish, powershell, zsh]

Options:
  -h, --help  Print help
```

## `turtle providers`

```text
Inspect available provider CLIs and verified capabilities

Usage: turtle providers <COMMAND>

Commands:
  list     Show installation, login, compatibility and verified capabilities
  refresh  Re-discover every provider and record a capability snapshot when it changed
  help     Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

## `turtle providers list`

```text
Show installation, login, compatibility and verified capabilities

Usage: turtle providers list

Options:
  -h, --help  Print help
```

## `turtle providers refresh`

```text
Re-discover every provider and record a capability snapshot when it changed

Usage: turtle providers refresh

Options:
  -h, --help  Print help
```

## `turtle up`

```text
Start the background daemon that runs jobs

Usage: turtle up [OPTIONS]

Options:
      --no-api                         Disable the local HTTP API
      --api-port <API_PORT>            Fixed loopback API port (default: random available port)
      --tailnet-host <TAILNET_HOST>    Allow the API through Tailscale Serve at this MagicDNS name
      --agent <AGENT>                  Ordered provider list: claude, codex, or both (demo runs alone)
      --role-agent <ROLE_AGENT>        Per-role priority, e.g. reviewer=codex. Repeat for more roles
      --claude-model <CLAUDE_MODEL>    Claude model (default: the CLI's own default)
      --claude-effort <CLAUDE_EFFORT>  Claude effort: low, medium, high, xhigh or max
      --max-run-usd <MAX_RUN_USD>      Spend cap per Claude run in API-equivalent USD (default 3.00)
      --codex-model <CODEX_MODEL>      Codex model (default: the CLI's own default)
      --codex-effort <CODEX_EFFORT>    Codex reasoning effort: low, medium, high, xhigh or max
  -h, --help                           Print help
```

## `turtle down`

```text
Stop the background daemon

Usage: turtle down

Options:
  -h, --help  Print help
```

## `turtle status`

```text
Say whether the daemon is running

Usage: turtle status

Options:
  -h, --help  Print help
```

## `turtle dashboard`

```text
Open the local dashboard using a one-time interactive login code

Usage: turtle dashboard [OPTIONS]

Options:
      --phone  Print a read-only QR login link for a Tailscale device
  -h, --help   Print help
```

## `turtle daemon`

```text
Run the daemon in the foreground (what `up` starts)

Usage: turtle daemon [OPTIONS]

Options:
      --no-api                         Disable the local HTTP API
      --api-port <API_PORT>            Fixed loopback API port (default: random available port)
      --tailnet-host <TAILNET_HOST>    Allow the API through Tailscale Serve at this MagicDNS name
      --agent <AGENT>                  Ordered provider list: claude, codex, or both (demo runs alone)
      --role-agent <ROLE_AGENT>        Per-role priority, e.g. reviewer=codex. Repeat for more roles
      --claude-model <CLAUDE_MODEL>    Claude model (default: the CLI's own default)
      --claude-effort <CLAUDE_EFFORT>  Claude effort: low, medium, high, xhigh or max
      --max-run-usd <MAX_RUN_USD>      Spend cap per Claude run in API-equivalent USD (default 3.00)
      --codex-model <CODEX_MODEL>      Codex model (default: the CLI's own default)
      --codex-effort <CODEX_EFFORT>    Codex reasoning effort: low, medium, high, xhigh or max
  -h, --help                           Print help
```

## `turtle project`

```text
Register and inspect projects

Usage: turtle project <COMMAND>

Commands:
  add           Register an existing Git repository
  delivery      Change how future deliveries for this project are made
  static-gates  Configure deterministic gates for one project
  list          List registered projects
  show          Show one project
  help          Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

## `turtle project add`

```text
Register an existing Git repository

Usage: turtle project add [OPTIONS] <PATH>

Arguments:
  <PATH>

Options:
      --slug <SLUG>
          Short name used on the command line (default: derived from the directory)
      --name <NAME>
          Display name (default: the slug)
      --key <KEY>
          2-6 uppercase letters used in job keys such as `API-42` (default: derived from the slug)
      --profile <PROFILE>
          Default workflow profile for new jobs [default: standard] [possible values: fast, standard, strict]
      --branch <BRANCH>
          Branch that deliveries go to (default: the branch currently checked out)
      --build <BUILD>
          Gate command: build the project
      --test <TEST>
          Gate command: run the tests
      --lint <LINT>
          Gate command: lint
      --worktree-setup <WORKTREE_SETUP>
          Command run once after a job worktree is created (install dependencies)
      --delivery <DELIVERY>
          Delivery mode: local-squash (default) or pr [default: local-squash] [possible values: local-squash, pr]
  -h, --help
          Print help
```

## `turtle project delivery`

```text
Change how future deliveries for this project are made

Usage: turtle project delivery <SLUG> <MODE>

Arguments:
  <SLUG>
  <MODE>  [possible values: local-squash, pr]

Options:
  -h, --help  Print help
```

## `turtle project static-gates`

```text
Configure deterministic gates for one project

Usage: turtle project static-gates [OPTIONS] <SLUG>

Arguments:
  <SLUG>

Options:
      --secret-scan <SECRET_SCAN>                [possible values: true, false]
      --protected-paths <PROTECTED_PATHS>        [possible values: true, false]
      --max-diff-lines <MAX_DIFF_LINES>
      --disable-diff-size
      --scope-conformance <SCOPE_CONFORMANCE>    [possible values: true, false]
      --dependency-changes <DEPENDENCY_CHANGES>  [possible values: true, false]
  -h, --help                                     Print help
```

## `turtle project list`

```text
List registered projects

Usage: turtle project list

Options:
  -h, --help  Print help
```

## `turtle project show`

```text
Show one project

Usage: turtle project show <SLUG>

Arguments:
  <SLUG>

Options:
  -h, --help  Print help
```

## `turtle resource`

```text
Attach versioned resources and search them with full-text retrieval

Usage: turtle resource <COMMAND>

Commands:
  add   Add a local text/PDF file or take a one-time HTTP(S) URL snapshot
  help  Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

## `turtle resource add`

```text
Add a local text/PDF file or take a one-time HTTP(S) URL snapshot

Usage: turtle resource add [OPTIONS] <INPUT>

Arguments:
  <INPUT>

Options:
  -p, --project <PROJECT>
      --job <JOB>
      --trust <TRUST>      [default: untrusted] [possible values: trusted, internal, untrusted]
  -h, --help               Print help
```

Text resources are limited to 8 MiB. PDFs are parsed in a time-limited child process; extracted
text is limited to 4 MiB, redacted, and stored as text for search and context. Image-only PDFs
without extractable text are rejected. Encrypted, malformed, or over-limit PDFs are rejected.

## `turtle search`

```text
Search project resources with SQLite FTS5

Usage: turtle search --project <PROJECT> <QUERY>

Arguments:
  <QUERY>

Options:
  -p, --project <PROJECT>
  -h, --help               Print help
```

## `turtle job`

```text
Create and follow jobs

Usage: turtle job <COMMAND>

Commands:
  create          Create a job in the backlog
  list            List jobs, newest first
  show            Show one job, including the state of each plan task
  events          Print the job's event history
  watch           Follow a job until it finishes or needs you
  start           Start a job from the backlog
  pause           Pause a job
  resume
  approve-budget  Approve a higher hard token budget and continue a held job
  resolve         Continue a blocked conflict, authorize a fix after review findings, or continue after repairing a task mismatch
  sync            Check whether a pending pull request has merged and finish delivery
  worktree        Print the deterministic worktree path for a job
  revert          Create an approval-gated follow-up job for a delivered change's revert
  cancel
  help            Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

## `turtle job create`

```text
Create a job in the backlog

Usage: turtle job create [OPTIONS] <TITLE>

Arguments:
  <TITLE>  What to build, in one line

Options:
  -p, --project <PROJECT>          Project slug (optional when only one project is registered)
  -d, --description <DESCRIPTION>  Longer description; use --file to read it from a Markdown file instead
  -f, --file <FILE>
      --type <JOB_TYPE>            feature, bugfix, refactor, docs or chore [default: feature]
      --priority <PRIORITY>        low, normal, high or urgent [default: normal]
      --profile <PROFILE>          fast, standard or strict (default: the project's profile) [possible values: fast, standard, strict]
      --budget <BUDGET>            Token budget for this job, e.g. `400k`; the job waits for you when it is used up
      --start                      Start the job right away
  -h, --help                       Print help
```

## `turtle job list`

```text
List jobs, newest first

Usage: turtle job list [OPTIONS]

Options:
  -p, --project <PROJECT>
      --active             Only jobs that are not finished
  -h, --help               Print help
```

## `turtle job show`

```text
Show one job, including the state of each plan task

Usage: turtle job show [OPTIONS] <KEY>

Arguments:
  <KEY>

Options:
      --json  Print the job and its tasks as JSON
  -h, --help  Print help
```

Tasks come from the `tasks` table and belong to the newest plan revision: state, commit, agent
runs, fix iterations and the latest outcome of each per-task gate. A job blocked with
`task_state_mismatch` also lists the disagreements between the table and the branch's
`Turtle-Task` trailers (`task_mismatches` in JSON).

## `turtle job events`

```text
Print the job's event history

Usage: turtle job events [OPTIONS] <KEY>

Arguments:
  <KEY>

Options:
      --json
  -h, --help  Print help
```

## `turtle job watch`

```text
Follow a job until it finishes or needs you

Usage: turtle job watch <KEY>

Arguments:
  <KEY>

Options:
  -h, --help  Print help
```

## `turtle job start`

```text
Start a job from the backlog

Usage: turtle job start <KEY>

Arguments:
  <KEY>

Options:
  -h, --help  Print help
```

## `turtle job pause`

```text
Pause a job

Usage: turtle job pause [OPTIONS] <KEY>

Arguments:
  <KEY>

Options:
      --hard  Stop the current agent run too (not implemented yet: the run finishes first)
  -h, --help  Print help
```

## `turtle job resume`

```text
Usage: turtle job resume <KEY>

Arguments:
  <KEY>

Options:
  -h, --help  Print help
```

## `turtle job approve-budget`

```text
Approve a higher hard token budget and continue a held job

Usage: turtle job approve-budget --budget <BUDGET> <KEY>

Arguments:
  <KEY>

Options:
      --budget <BUDGET>  New total token limit, e.g. 1.5M
  -h, --help             Print help
```

## `turtle job resolve`

```text
Continue a blocked conflict, authorize a fix after review findings, or continue after repairing a task mismatch

Usage: turtle job resolve [OPTIONS] <KEY>

Arguments:
  <KEY>

Options:
      --target <TARGET>  For review findings or a task mismatch: implementation, planning or specification
  -h, --help             Print help
```

## `turtle job sync`

```text
Check whether a pending pull request has merged and finish delivery

Usage: turtle job sync [OPTIONS] <KEY>

Arguments:
  <KEY>

Options:
      --wait                 Keep checking until the pull request is merged
      --interval <INTERVAL>  Seconds between checks when --wait is set [default: 30]
  -h, --help                 Print help
```

## `turtle job worktree`

```text
Print the deterministic worktree path for a job

Usage: turtle job worktree <KEY>

Arguments:
  <KEY>

Options:
  -h, --help  Print help
```

## `turtle job revert`

```text
Create an approval-gated follow-up job for a delivered change's revert

Usage: turtle job revert <KEY>

Arguments:
  <KEY>

Options:
  -h, --help  Print help
```

## `turtle job cancel`

```text
Usage: turtle job cancel [OPTIONS] <KEY>

Arguments:
  <KEY>

Options:
      --delete-branch  Also remove the isolated worktree and delete the Turtle Flow job branch
  -h, --help           Print help
```

## `turtle budget`

```text
Token budgets

Usage: turtle budget <COMMAND>

Commands:
  show      Show budgets and what has been spent against them
  estimate  Estimate a planned job's token range and budget decision
  set       Set or change a token limit. Raising the limit of a held job resumes it
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

## `turtle budget show`

```text
Show budgets and what has been spent against them

Usage: turtle budget show

Options:
  -h, --help  Print help
```

## `turtle budget estimate`

```text
Estimate a planned job's token range and budget decision

Usage: turtle budget estimate <JOB_KEY>

Arguments:
  <JOB_KEY>

Options:
  -h, --help  Print help
```

## `turtle budget set`

```text
Set or change a token limit. Raising the limit of a held job resumes it

Usage: turtle budget set [OPTIONS] <TOKENS>

Arguments:
  <TOKENS>  Limit in tokens processed, e.g. `400k` or `20M`

Options:
      --job <JOB>          Budget for one job (its lifetime), e.g. `--job PAY-1`
  -p, --project <PROJECT>  Monthly budget for a project, by slug
      --global             Monthly budget for everything Turtle Flow does
      --soft               Keep going and only warn when the limit is passed (default: stop)
  -h, --help               Print help
```

## `turtle runs`

```text
Inspect agent runs and their redacted transcripts

Usage: turtle runs <COMMAND>

Commands:
  list        List runs for a job
  transcript  Print a run transcript. The stored stream is already redacted
  help        Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

## `turtle runs list`

```text
List runs for a job

Usage: turtle runs list --job <JOB>

Options:
      --job <JOB>
  -h, --help       Print help
```

## `turtle runs transcript`

```text
Print a run transcript. The stored stream is already redacted

Usage: turtle runs transcript [OPTIONS] <RUN_ID>

Arguments:
  <RUN_ID>

Options:
      --raw
  -h, --help  Print help
```

## `turtle approvals`

```text
List what is waiting for your decision

Usage: turtle approvals

Options:
  -h, --help  Print help
```

## `turtle approve`

```text
Decide an approval (interactive terminal only)

Usage: turtle approve [OPTIONS] <ID>

Arguments:
  <ID>

Options:
      --request-changes    Send the job back with feedback instead of approving
      --reject             Reject the job
  -c, --comment <COMMENT>  Required with --request-changes and --reject
  -h, --help               Print help
```

## `turtle audit`

```text
Verify the tamper-evident event log

Usage: turtle audit <COMMAND>

Commands:
  verify  Walk the hash chain; exits with status 2 if it is broken
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

## `turtle audit verify`

```text
Walk the hash chain; exits with status 2 if it is broken

Usage: turtle audit verify

Options:
  -h, --help  Print help
```

## `turtle export`

```text
Export a job's audit package

Usage: turtle export <COMMAND>

Commands:
  job   Export specification, plan, approvals, runs, reports and delivery
  help  Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

## `turtle export job`

```text
Export specification, plan, approvals, runs, reports and delivery

Usage: turtle export job [OPTIONS] <KEY>

Arguments:
  <KEY>

Options:
      --format <FORMAT>  [default: md] [possible values: md, json]
  -h, --help             Print help
```
