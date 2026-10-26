#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
reference_root="${TURTLE_REFERENCE_ROOT:-${TMPDIR:-/tmp}/turtle-flow-live-e2e}"
missing=0

check() {
  if "$@" >/dev/null 2>&1; then
    printf 'PASS %s\n' "$1"
  else
    printf 'FAIL %s\n' "$1"
    missing=1
  fi
}

check git --version
check cargo --version
check node --version
check npm --version
check python3 --version
if node -e 'const [major, minor] = process.versions.node.split(".").map(Number); process.exit(major > 22 || (major === 22 && minor >= 6) ? 0 : 1)' >/dev/null 2>&1; then
  printf 'PASS Node.js version (>=22.6)\n'
else
  printf 'FAIL Node.js version (requires >=22.6 for built-in TypeScript stripping)\n'
  missing=1
fi
if python3 -c 'import sys; sys.exit(0 if sys.version_info >= (3, 10) else 1)' >/dev/null 2>&1; then
  printf 'PASS Python version (>=3.10)\n'
else
  printf 'FAIL Python version (requires >=3.10)\n'
  missing=1
fi

for name in ref-rust-cli ref-ts-api ref-py-lib; do
  path="$reference_root/$name"
  if [[ ! -d "$path" ]]; then
    printf 'FAIL fixture %s (missing: %s)\n' "$name" "$path"
    missing=1
  elif git -C "$path" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    case "$name" in
      ref-rust-cli) marker="$path/Cargo.toml" ;;
      ref-ts-api) marker="$path/package.json" ;;
      ref-py-lib) marker="$path/pyproject.toml" ;;
    esac
    if [[ -f "$marker" ]]; then
      if [[ -z "$(git -C "$path" status --porcelain)" ]]; then
        printf 'PASS fixture %s\n' "$name"
      else
        printf 'FAIL fixture %s (working tree is not clean; prepare a fresh clone)\n' "$name"
        missing=1
      fi
    else
      printf 'FAIL fixture %s (missing project manifest)\n' "$name"
      missing=1
    fi
  else
    printf 'FAIL fixture %s (not a Git repository: %s)\n' "$name" "$path"
    missing=1
  fi
done

if command -v claude >/dev/null 2>&1; then
  if claude auth status --json 2>/dev/null | python3 -c 'import json,sys; sys.exit(0 if json.load(sys.stdin).get("loggedIn") is True else 1)' >/dev/null 2>&1; then
    printf 'PASS Claude Code authenticated\n'
  else
    printf 'FAIL Claude Code authentication (run claude auth status locally)\n'
    missing=1
  fi
else
  printf 'FAIL Claude Code CLI missing\n'
  missing=1
fi

if command -v codex >/dev/null 2>&1; then
  codex_status="$(codex login status 2>&1 || true)"
  if [[ "$codex_status" == *"Logged in"* ]]; then
    printf 'PASS Codex authenticated\n'
  else
    printf 'FAIL Codex authentication (run codex login status locally)\n'
    missing=1
  fi
else
  printf 'FAIL Codex CLI missing\n'
  missing=1
fi

if (( missing != 0 )); then
  printf '\nLive E2E preflight is incomplete; no repositories or jobs were modified.\n' >&2
  exit 1
fi

printf '\nPreflight passed. Review docs/LIVE-E2E-CHECKLIST.md before starting provider jobs.\n'
