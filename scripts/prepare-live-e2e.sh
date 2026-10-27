#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_root="$repo_root/fixtures/reference-sources"
target_root="${TURTLE_REFERENCE_ROOT:-${TMPDIR:-/tmp}/turtle-flow-live-e2e}"

mkdir -p "$target_root"
for name in ref-rust-cli ref-ts-api ref-py-lib; do
  source="$source_root/$name"
  target="$target_root/$name"
  if [[ ! -d "$source" ]]; then
    printf 'Missing fixture source: %s\n' "$source" >&2
    exit 1
  fi
  if [[ -e "$target" ]]; then
    printf 'Refusing to overwrite existing fixture: %s\n' "$target" >&2
    exit 1
  fi
done

for name in ref-rust-cli ref-ts-api ref-py-lib; do
  target="$target_root/$name"
  cp -R "$source_root/$name" "$target"
  git -C "$target" init --quiet -b main
  git -C "$target" config user.name "Turtle Flow Fixture"
  git -C "$target" config user.email "fixture@example.invalid"
  git -C "$target" config core.autocrlf false
  git -C "$target" add -A
  GIT_AUTHOR_DATE="2026-01-01T00:00:00Z" \
    GIT_COMMITTER_DATE="2026-01-01T00:00:00Z" \
    git -C "$target" commit --quiet -m "fixture: initial benchmark baseline"
  printf '%s\n' "$target"
done

printf '\nFixture clones created. Benchmark prompts and acceptance checks remain in the Turtle Flow checkout.\n'
