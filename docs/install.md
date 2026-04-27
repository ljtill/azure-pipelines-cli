# Installation

Release archives are signed with [Sigstore](https://www.sigstore.dev/) (cosign, keyless via GitHub Actions OIDC). The install scripts and `devops update` verify the signature before installing — see [SECURITY.md](../SECURITY.md) for details.

## macOS / Linux

```sh
curl -fsSL https://raw.githubusercontent.com/ljtill/azure-devops-cli/main/install.sh | sh
```

Pin a specific version:

```sh
VERSION=0.1.0 curl -fsSL https://raw.githubusercontent.com/ljtill/azure-devops-cli/main/install.sh | sh
```

## Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/ljtill/azure-devops-cli/main/install.ps1 | iex
```

## From source

Requires a recent Rust toolchain (see `rust-version` in `Cargo.toml`).

```bash
cargo install --path .
```

## Install layout

The install scripts place files at:

- `~/.local/share/devops/versions/<version>/devops` — the installed release binary.
- `~/.local/bin/devops` — symlink to the active version.

Make sure `~/.local/bin` is on your `PATH`.

## Updating

```bash
devops update
```

Downloads the latest release, verifies its Sigstore signature, installs it under `~/.local/share/devops/versions/`, and updates the `~/.local/bin/devops` symlink.

## Uninstall

```sh
rm ~/.local/bin/devops
rm -rf ~/.local/share/devops
rm -rf ~/.config/devops          # config (optional)
rm -rf ~/.local/state/devops     # persistent state (optional)
```
