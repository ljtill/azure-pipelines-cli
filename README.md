# azure-pipelines-cli

A terminal (TUI) dashboard for monitoring Azure DevOps Pipelines in real-time.

Built with [ratatui](https://ratatui.rs/) and designed to run inside [Ghostty](https://ghostty.org/) or any modern terminal emulator.

## Features

- **Dashboard view** — Pipeline definitions grouped by folder with latest build status
- **Pipelines view** — Flat, searchable list of all pipeline definitions
- **Active Runs view** — All currently running builds across the fleet
- **Build History** — Drill into a pipeline's recent builds
- **Log Viewer** — Drill into a build to view live log output with collapsible timeline tree
- **Search / filter** — Incremental search in Pipelines and Active Runs views
- **Queue pipeline** — Trigger a new pipeline run directly from the TUI
- **Cancel build** — Stop a running build; multi-select batch cancel in Active Runs
- **Retry stage** — Re-run a failed stage without re-queuing the entire pipeline
- **Approve / Reject checks** — Approve or reject environment approval gates inline from the Log Viewer
- **Open in browser** — Jump to any pipeline or build in the Azure DevOps web UI
- **Auto-refresh** — Background polling with configurable interval (default 15s)
- **Build state-change notifications** — Inline notifications when builds start, succeed, or fail (configurable)
- **In-app settings** — Edit configuration live, save, and reload without restarting
- **First-run setup** — Interactive setup wizard when no config file exists
- **Auto-update check** — Background check for new releases on GitHub; persistent notification when an update is available
- **Self-update** — `pipelines update` downloads the latest release and updates the symlink
- **Azure CLI auth** — Uses `DeveloperToolsCredential` (Azure CLI / Azure Developer CLI chain)

## Installation

### macOS / Linux

```sh
curl -fsSL https://raw.githubusercontent.com/ljtill/azure-pipelines-cli/main/install.sh | sh
```

Pin a specific version:

```sh
VERSION=0.1.0 curl -fsSL https://raw.githubusercontent.com/ljtill/azure-pipelines-cli/main/install.sh | sh
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/ljtill/azure-pipelines-cli/main/install.ps1 | iex
```

### From source

```bash
cargo install --path .
```

## Configuration

On first launch, an interactive setup wizard creates `~/.config/pipelines/config.toml` with your Azure DevOps organization and project. All settings can be adjusted in-app with `,` (settings).

## Usage

```bash
# Uses default config path
pipelines

# Override config path
pipelines --config /path/to/config.toml

# Print installed version
pipelines version

# Update to the latest release
pipelines update
```

## Keybindings

| Key          | Action                                                |
|--------------|-------------------------------------------------------|
| ↑ / ↓        | Navigate list items                                   |
| → / Enter    | Drill into selected item / expand (tree views)        |
| ← / q / Esc  | Go back / collapse (tree views)                       |
| Home / End   | Jump to first / last item                             |
| 1 / 2 / 3    | Switch between Dashboard / Pipelines / Active Runs   |
| /            | Search / filter (Pipelines / Active Runs)             |
| Space        | Select / deselect build (Active Runs)                 |
| Q            | Queue pipeline run                                    |
| R            | Retry failed stage (Log Viewer)                       |
| A            | Approve check (Log Viewer, on checkpoint row)         |
| D            | Reject check (Log Viewer, on checkpoint row)          |
| c            | Cancel build (Active Runs / Build History / Log Viewer) |
| o            | Open in browser                                       |
| r            | Force data refresh                                    |
| f            | Follow latest active task (Log Viewer)                |
| x            | Dismiss notification                                  |
| ,            | Open settings                                         |
| PgUp / PgDn  | Scroll log content                                   |
| Mouse scroll | Scroll log content (Log Viewer)                       |
| ?            | Toggle help overlay                                   |
| Ctrl+C       | Quit immediately                                      |

## Authentication

Uses the Azure SDK `DeveloperToolsCredential`, which tries local developer credentials in this order:

1. Azure CLI (`az login`)
2. Azure Developer CLI (`azd auth login`)

For local development, ensure you're logged in with `az login` or `azd auth login`.

## Security

Please report vulnerabilities privately as described in [SECURITY.md](SECURITY.md).
