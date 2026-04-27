<h1 align="center">azure-devops-cli</h1>
<p align="center">A terminal dashboard for Azure DevOps that runs in any modern terminal.</p>

---

## Quickstart

### Installing and running

Install with the convenience script for your platform. The scripts verify the
Sigstore-signed checksum manifest and the archive SHA-256 before installing; if
your environment does not allow piping downloaded scripts to a shell, use the
[secure manual flow](./docs/install.md#recommended-secure-flow).

```sh
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/ljtill/azure-devops-cli/main/install.sh | sh
```

```powershell
# Windows (PowerShell)
irm https://raw.githubusercontent.com/ljtill/azure-devops-cli/main/install.ps1 | iex
```

Then run `devops` to get started. On first launch, an interactive setup wizard creates `~/.config/devops/config.toml` with your Azure DevOps organization and project. Do not store tokens, PATs, or other secrets in this file; authentication comes from Azure CLI or Azure Developer CLI credentials.

<details>
<summary>You can also pin a specific version, build from source, or download a release binary.</summary>

Pin a specific version:

```sh
curl -fsSL https://raw.githubusercontent.com/ljtill/azure-devops-cli/main/install.sh | VERSION=0.1.0 sh
```

Build from source (requires the Rust toolchain pinned in `Cargo.toml`):

```sh
cargo install --path .
```

Or download a binary directly from the [latest GitHub Release](https://github.com/ljtill/azure-devops-cli/releases/latest). Release archives are covered by a Sigstore-signed `SHA256SUMS` manifest — see [SECURITY.md](SECURITY.md) for verification steps.

</details>

### Signing in

`devops` uses the Azure SDK `DeveloperToolsCredential`. Sign in once with either Azure CLI or Azure Developer CLI and the TUI picks up your credentials automatically:

```sh
az login
# or
azd auth login
```

## Docs

- [**Getting started**](./docs/getting-started.md)
- [**Installation**](./docs/install.md)
- [**Configuration**](./docs/configuration.md)
- [**Keybindings**](./docs/keybindings.md)
- [**Authentication**](./docs/authentication.md)
- [**Stability**](./docs/stability.md)
- [**Limitations**](./docs/limitations.md)
- [**Contributing**](./docs/contributing.md)

Please report security issues privately as described in [SECURITY.md](SECURITY.md).
