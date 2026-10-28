#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
output=${1:-docs/CLI-REFERENCE.md}
cargo build --quiet -p turtle-flow
binary=target/debug/turtle
{
  printf '# CLI reference\n\nGenerated from the current Clap command tree. Regenerate with `scripts/generate-cli-reference.sh`.\n'
  for command in \
    "" "doctor" "init" "completions" "providers" "providers list" "providers refresh" \
    "up" "down" "status" "dashboard" "daemon" \
    "project" "project add" "project delivery" "project static-gates" "project list" "project show" \
    "resource" "resource add" "search" \
    "job" "job create" "job list" "job show" "job events" "job watch" "job start" \
    "job pause" "job resume" "job resolve" "job sync" "job worktree" "job revert" "job cancel" \
    "budget" "budget show" "budget estimate" "budget set" \
    "runs" "runs list" "runs transcript" "approvals" "approve" "audit" "audit verify" \
    "export" "export job"; do
    if [ -z "$command" ]; then title=turtle; set --; else
      # Intentional splitting: every command entry above is a fixed list of safe words.
      set -- $command
      title="turtle $command"
    fi
    printf '\n## `%s`\n\n```text\n' "$title"
    "$binary" "$@" --help | sed -e 's/[[:space:]]*$//'
    printf '```\n'
  done
} > "$output"
