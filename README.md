# azure-pipelines-cli

A terminal (TUI) dashboard for monitoring Azure DevOps Pipelines in real-time.

Built with [ratatui](https://ratatui.rs/) and designed to run inside [Ghostty](https://ghostty.org/) or any modern terminal emulator.

## Features

- **Dashboard view** — Pipeline definitions grouped by folder with latest build status
- **Pipelines view** — Flat, searchable list of all pipeline definitions
- **Active Runs view** — All currently running builds across the fleet
- **Build History** — Drill into a pipeline's recent builds
- **Log Viewer** — Drill into a build to view live log output
- **Queue pipeline** — Trigger a new pipeline run directly from the TUI
- **Cancel build** — Stop a running build without leaving the terminal
- **Retry stage** — Re-run a failed stage without re-queuing the entire pipeline
- **Approve / Reject checks** — Approve or reject environment approval gates inline from the Log Viewer
- **Open in browser** — Jump to any pipeline or build in the Azure DevOps web UI
- **Auto-refresh** — Background polling with configurable interval (default 30s)
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

Create `~/.config/pipelines/config.toml`:

```toml
[azure_devops]
organization = "your-org"
project = "your-project"

[display]
refresh_interval_secs = 30
log_refresh_interval_secs = 5

# Optional: filter which pipelines are shown
# Note: folder filters apply to Dashboard and Pipelines views.
# Active Runs can only filter by definition ID (builds don't carry folder paths).
[filters]
folders = ["\\Infra", "\\Deploy"]  # only show these folder paths (prefix match)
definition_ids = [42, 99]          # only show these pipeline definition IDs

# Optional: control update checks
[update]
check_for_updates = true  # set to false to disable background update checks
```

## Usage

```bash
# Uses default config path
pipelines

# Override config path
pipelines --config /path/to/config.toml

# Update to the latest release
pipelines update
```

## Keybindings

| Key          | Action                                                |
|--------------|-------------------------------------------------------|
| ↑ / ↓        | Navigate list items                                   |
| ← / →        | Collapse / expand folder (Dashboard) or tree node (Log Viewer) |
| Home / End   | Jump to first / last item                             |
| Enter        | Drill into selected item / expand-collapse folder     |
| Esc          | Go back to previous view                              |
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
| x            | Dismiss notification                                  |
| f            | Follow latest active task (Log Viewer)                |
| PgUp / PgDn  | Scroll log content                                   |
| Mouse scroll | Scroll log content (Log Viewer)                       |
| ?            | Toggle help overlay                                   |
| Ctrl+C       | Quit                                                  |
| q            | Quit (root views) / Go back (child views)             |

## Authentication

Uses the Azure SDK `DeveloperToolsCredential`, which tries local developer credentials in this order:

1. Azure CLI (`az login`)
2. Azure Developer CLI (`azd auth login`)

For local development, ensure you're logged in with `az login` or `azd auth login`.

## Security

Please report vulnerabilities privately as described in [SECURITY.md](SECURITY.md).
