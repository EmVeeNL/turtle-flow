#!/bin/sh
set -eu

repo=${TURTLE_REPOSITORY:-EmVeeNL/turtle-flow}
version=${TURTLE_VERSION:-latest}
home=${HOME:?HOME must be set to choose an install directory}
prefix=${TURTLE_INSTALL_DIR:-"$home/.local/bin"}
os=$(uname -s)
arch=$(uname -m)
case "$os/$arch" in
  Darwin/arm64|Darwin/aarch64) target=aarch64-apple-darwin ;;
  Darwin/x86_64|Darwin/amd64) target=x86_64-apple-darwin ;;
  Linux/x86_64|Linux/amd64) target=x86_64-unknown-linux-gnu ;;
  Linux/aarch64|Linux/arm64) target=aarch64-unknown-linux-gnu ;;
  *) printf 'Unsupported platform: %s/%s\n' "$os" "$arch" >&2; exit 1 ;;
esac

if [ "$version" = latest ]; then
  release="https://github.com/$repo/releases/latest/download"
else
  release="https://github.com/$repo/releases/download/$version"
fi
archive="turtle-${target}.tar.gz"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
curl --fail --location --silent --show-error "$release/$archive" -o "$tmp/$archive"
curl --fail --location --silent --show-error "$release/$archive.sha256" -o "$tmp/$archive.sha256"
if command -v sha256sum >/dev/null 2>&1; then
  (cd "$tmp" && sha256sum --check "$archive.sha256")
else
  (cd "$tmp" && shasum -a 256 --check "$archive.sha256")
fi
mkdir -p "$prefix"
tar -xzf "$tmp/$archive" -C "$tmp"
install -m 0755 "$tmp/turtle" "$prefix/turtle"
printf 'Installed turtle to %s/turtle\n' "$prefix"
case ":${PATH}:" in *":$prefix:"*) ;; *) printf 'Add %s to PATH to run turtle.\n' "$prefix" ;; esac
