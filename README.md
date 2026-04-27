<p align="center"><code>curl -fsSL https://raw.githubusercontent.com/ljtill/azure-devops-cli/main/install.sh | sh</code><br />or <code>irm https://raw.githubusercontent.com/ljtill/azure-devops-cli/main/install.ps1 | iex</code></p>
<p align="center"><strong>azure-devops-cli</strong> is a terminal dashboard for Azure DevOps that runs in any modern terminal.</p>

---

## Quickstart

### Installing and running

Install with the script for your platform:

```sh
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/ljtill/azure-devops-cli/main/install.sh | sh
```

```powershell
# Windows (PowerShell)
irm https://raw.githubusercontent.com/ljtill/azure-devops-cli/main/install.ps1 | iex
```

Then run `devops` to get started. On first launch, an interactive setup wizard creates `~/.config/devops/config.toml` with your Azure DevOps organization and project.

<details>
<summary>You can also pin a specific version, build from source, or download a release binary.</summary>

Pin a specific version:

```sh
VERSION=1.0.0 curl -fsSL https://raw.githubusercontent.com/ljtill/azure-devops-cli/main/install.sh | sh
```

Build from source (requires the Rust toolchain pinned in `Cargo.toml`):

```sh
cargo install --path .
```

Or download a binary directly from the [latest GitHub Release](https://github.com/ljtill/azure-devops-cli/releases/latest). Release archives are signed with [Sigstore](https://www.sigstore.dev/) — see [SECURITY.md](SECURITY.md) for verification steps.

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
