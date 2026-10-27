#!/bin/sh
set -eu

usage() {
  printf 'Usage: %s <turtle-target.tar.gz> [checksum-file]\n' "$0" >&2
  exit 2
}

[ "$#" -ge 1 ] && [ "$#" -le 2 ] || usage
archive=$1
[ -f "$archive" ] || { printf 'Archive not found: %s\n' "$archive" >&2; exit 1; }

archive_dir=$(CDPATH= cd "$(dirname "$archive")" && pwd -P)
archive_name=$(basename "$archive")
archive_path=$archive_dir/$archive_name
checksum=${2:-$archive_path.sha256}
[ -f "$checksum" ] || { printf 'Checksum sidecar not found: %s\n' "$checksum" >&2; exit 1; }
checksum_dir=$(CDPATH= cd "$(dirname "$checksum")" && pwd -P)
checksum_name=$(basename "$checksum")

case "$archive_name" in
  turtle-*.tar.gz) target=${archive_name#turtle-}; target=${target%.tar.gz} ;;
  *) printf 'Unexpected archive name: %s\n' "$archive_name" >&2; exit 1 ;;
esac
case "$target" in
  aarch64-apple-darwin|x86_64-apple-darwin|x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu) ;;
  *) printf 'Unsupported archive target in name: %s\n' "$target" >&2; exit 1 ;;
esac

entry_list=$(tar -tzf "$archive_path") || { printf 'Could not read archive: %s\n' "$archive_name" >&2; exit 1; }
[ "$entry_list" = turtle ] || {
  printf 'Archive must contain only the root-level turtle binary; found:\n%s\n' "$entry_list" >&2
  exit 1
}

line_count=$(awk 'END { print NR }' "$checksum")
[ "$line_count" -eq 1 ] || { printf 'Checksum sidecar must contain exactly one line.\n' >&2; exit 1; }
IFS=' ' read -r digest listed_name < "$checksum" || {
  printf 'Could not read checksum sidecar: %s\n' "$checksum" >&2
  exit 1
}
case "$digest" in
  *[!0123456789abcdefABCDEF]*|'') printf 'Invalid SHA-256 digest in %s\n' "$checksum" >&2; exit 1 ;;
esac
[ "${#digest}" -eq 64 ] || { printf 'Invalid SHA-256 digest length in %s\n' "$checksum" >&2; exit 1; }
[ "$listed_name" = "$archive_name" ] || {
  printf 'Checksum sidecar names %s, expected %s.\n' "$listed_name" "$archive_name" >&2
  exit 1
}

if command -v sha256sum >/dev/null 2>&1; then
  (CDPATH= cd -- "$checksum_dir" && sha256sum --check --status "$checksum_name") || {
    printf 'SHA-256 verification failed for %s\n' "$archive_name" >&2
    exit 1
  }
elif command -v shasum >/dev/null 2>&1; then
  (CDPATH= cd -- "$checksum_dir" && shasum -a 256 --check "$checksum_name" >/dev/null) || {
    printf 'SHA-256 verification failed for %s\n' "$archive_name" >&2
    exit 1
  }
else
  printf 'Need sha256sum or shasum to verify the checksum.\n' >&2
  exit 1
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
tar -xzf "$archive_path" -C "$tmp"
[ -x "$tmp/turtle" ] || { printf 'Packaged turtle file is not executable.\n' >&2; exit 1; }

host_target=
case "$(uname -s)/$(uname -m)" in
  Darwin/arm64|Darwin/aarch64) host_target=aarch64-apple-darwin ;;
  Darwin/x86_64|Darwin/amd64) host_target=x86_64-apple-darwin ;;
  Linux/x86_64|Linux/amd64) host_target=x86_64-unknown-linux-gnu ;;
  Linux/aarch64|Linux/arm64) host_target=aarch64-unknown-linux-gnu ;;
esac

if [ "$target" = "$host_target" ]; then
  "$tmp/turtle" --version
else
  printf 'Checksum and archive contents are valid; skipped --version for non-native target %s (host: %s).\n' \
    "$target" "${host_target:-unknown}"
fi
