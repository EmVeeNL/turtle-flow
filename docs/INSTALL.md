# Install Turtle Flow

Turtle Flow currently ships unsigned macOS arm64/x86_64 and Linux x86_64/aarch64 archives. Windows and automatic updates are not supported.

## Install from a draft or published GitHub release

After a release is published, the checksum-verifying installer can be run with:

```sh
curl -fsSL https://raw.githubusercontent.com/EmVeeNL/turtle-flow/main/install.sh | sh
```

It downloads the archive and its SHA-256 file, verifies the archive, and installs `turtle` to `~/.local/bin`. To select a release or install directory:

```sh
TURTLE_VERSION=v0.3.0 TURTLE_INSTALL_DIR="$HOME/bin" sh install.sh
```

For a private repository or mirror, set `TURTLE_REPOSITORY=owner/repository`. Inspect the script before piping it to a shell. Releases are draft until reviewed and published.

## Manual install

Download the archive matching your operating system and CPU from the [GitHub releases](https://github.com/EmVeeNL/turtle-flow/releases), then verify it from the directory containing both files:

```sh
shasum -a 256 --check turtle-aarch64-apple-darwin.tar.gz.sha256
tar -xzf turtle-aarch64-apple-darwin.tar.gz
install -m 0755 turtle "$HOME/.local/bin/turtle"
```

Use the matching target name in the archive/checksum. Linux users can run `sha256sum --check` instead of `shasum`.

## Shell completions

Generate a completion file using Clap:

```sh
turtle completions zsh > "${ZDOTDIR:-$HOME}/.zfunc/_turtle"
turtle completions bash > "$HOME/.local/share/bash-completion/completions/turtle"
turtle completions fish > "$HOME/.config/fish/completions/turtle.fish"
```

Restart the shell or reload its completion configuration.

## Signing and notarization

Release artifacts are not code-signed or notarized because no signing credentials are configured. Before distributing a release, a maintainer must sign and notarize macOS binaries using the organization's approved Apple Developer identity, then staple and validate the ticket. Do not add credentials to this repository or its workflow. Linux signing is also a manual release step if required by the distribution channel.
