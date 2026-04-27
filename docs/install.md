# Installation

Release archives are listed in `SHA256SUMS`, and that manifest is signed with
[Sigstore](https://www.sigstore.dev/) (cosign, keyless via GitHub Actions OIDC).
The install scripts and `devops update` verify the signed manifest and the
archive SHA-256 before installing — see [SECURITY.md](../SECURITY.md) for the
expected signer identity and issuer.

## Recommended secure flow

For the highest assurance, especially on corporate machines:

1. Pick an explicit release tag from the GitHub Release page instead of relying
   on "latest" when you need reproducible installs.
2. Download the platform archive, `SHA256SUMS`, `SHA256SUMS.cosign.bundle`, and
   optionally `devops-sbom.cdx.json` from that release over HTTPS.
3. Verify `SHA256SUMS` with an approved `cosign` binary:

   ```sh
   cosign verify-blob \
     --bundle SHA256SUMS.cosign.bundle \
     --certificate-identity-regexp '^https://github\.com/ljtill/azure-devops-cli/\.github/workflows/ci\.release\.yml@refs/heads/main$' \
     --certificate-oidc-issuer 'https://token.actions.githubusercontent.com' \
     SHA256SUMS
   ```

4. Compare the archive and SBOM SHA-256 digests with the matching lines in the
   verified `SHA256SUMS` file before extracting or trusting them.
5. Extract only the expected single binary for your platform and install it into
   a directory you control.
6. Confirm the binary you run is the one you installed (`command -v devops` on
   macOS/Linux or `Get-Command devops` on Windows).

Do not assume a tool found through `PATH` is trustworthy. Prefer approved
absolute tool paths or managed images for `cosign`, hashing tools, and archive
extractors; do not install new tools or execute downloaded scripts unless your
organization's policy permits it.

## macOS / Linux

```sh
curl -fsSL https://raw.githubusercontent.com/ljtill/azure-devops-cli/main/install.sh | sh
```

Pin a specific version:

```sh
curl -fsSL https://raw.githubusercontent.com/ljtill/azure-devops-cli/main/install.sh | VERSION=0.1.0 sh
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

The convenience install scripts place the active binary directly in:

- `~/.local/bin/devops` on macOS/Linux.
- `%USERPROFILE%\.local\bin\devops.exe` on Windows.

The built-in updater keeps versioned binaries at:

- `~/.local/share/devops/versions/<version>/devops` — the installed release binary.
- `~/.local/bin/devops` — symlink to the active version on macOS/Linux.
- `%USERPROFILE%\.local\bin\devops.exe` — copied active binary on Windows.

Make sure the install directory is on your `PATH`, and verify it resolves before
any older or unexpected `devops` binary.

## Updating

```bash
devops update
```

Downloads the latest release, verifies the Sigstore bundle for `SHA256SUMS`,
checks the archive SHA-256 in process, installs it under
`~/.local/share/devops/versions/`, and updates the active binary. On macOS/Linux
the active entry is a symlink; on Windows it is copied into place.

## Uninstall

```sh
rm ~/.local/bin/devops
rm -rf ~/.local/share/devops
rm -rf ~/.config/devops          # config (optional)
rm -rf ~/.local/state/devops     # persistent state (optional)
```
