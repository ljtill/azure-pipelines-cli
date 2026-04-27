# azure-devops-cli

A terminal (TUI) dashboard for Azure DevOps. Built with [ratatui](https://ratatui.rs/) and designed to run inside any modern terminal emulator.

## Quickstart

### macOS / Linux

```sh
curl -fsSL https://raw.githubusercontent.com/ljtill/azure-devops-cli/main/install.sh | sh
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/ljtill/azure-devops-cli/main/install.ps1 | iex
```

Then run:

```sh
devops
```

On first launch, an interactive setup wizard creates `~/.config/devops/config.toml` with your Azure DevOps organization and project.

## Docs

- [**Getting started**](docs/getting-started.md) — first run, feature overview, and basic usage.
- [**Installation**](docs/install.md) — all install methods, version pinning, building from source, uninstall.
- [**Configuration**](docs/configuration.md) — config file schema and display options.
- [**Keybindings**](docs/keybindings.md) — full key reference.
- [**Authentication**](docs/authentication.md) — `DeveloperToolsCredential` chain (`az login` / `azd auth login`).
- [**Stability**](docs/stability.md) — 1.x compatibility guarantees.
- [**Limitations**](docs/limitations.md) — log buffer cap, pagination cap, env overrides.
- [**Contributing**](docs/contributing.md) — build, test, lint, and architecture overview.

## Security

Release archives are signed with [Sigstore](https://www.sigstore.dev/) (cosign, keyless via GitHub Actions OIDC). Install scripts and `devops update` verify the signature before installing.

Please report vulnerabilities privately as described in [SECURITY.md](SECURITY.md).
