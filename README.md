# azure-pipelines-cli

A terminal (TUI) dashboard for monitoring Azure DevOps Pipelines in real-time.

Built with [ratatui](https://ratatui.rs/) and designed to run inside [Ghostty](https://ghostty.org/) or any modern terminal emulator.

## Features

- **Dashboard view** — Pipeline definitions grouped by folder with latest build status
- **Pipelines view** — Flat, searchable list of all pipeline definitions
- **Active Runs view** — All currently running builds across the fleet
- **Build History** — Drill into a pipeline's recent builds
- **Log Viewer** — Drill into a build to view live log output
- **Auto-refresh** — Background polling with configurable interval (default 30s)
- **Azure CLI auth** — Uses `DeveloperToolsCredential` (Azure CLI / Azure Developer CLI chain)

## Installation

```bash
cargo install --path .
```

## Configuration

Create `~/.config/azure-pipelines-cli/config.toml`:

```toml
[azure_devops]
organization = "your-org"
project = "your-project"

[display]
refresh_interval_secs = 30
log_refresh_interval_secs = 5

# Optional: filter which pipelines are shown
[filters]
folders = ["\\Infra", "\\Deploy"]  # only show these folder paths (prefix match)
definition_ids = [42, 99]          # only show these pipeline definition IDs
```

## Usage

```bash
# Uses default config path
azure-pipelines-cli

# Override config path
azure-pipelines-cli --config /path/to/config.toml
```

## Keybindings

| Key          | Action                                         |
|--------------|-------------------------------------------------|
| ↑ / ↓        | Navigate list items                             |
| Enter        | Drill into selected item / expand-collapse folder |
| Esc          | Go back to previous view                        |
| 1 / 2 / 3    | Switch between Dashboard / Pipelines / Active Runs |
| /            | Search / filter (Pipelines view)                |
| r            | Force data refresh                              |
| PgUp / PgDn  | Scroll log content                              |
| ?            | Toggle help overlay                             |
| q            | Quit                                            |

## Authentication

Uses the Azure SDK `DeveloperToolsCredential`, which tries local developer credentials in this order:

1. Azure CLI (`az login`)
2. Azure Developer CLI (`azd auth login`)

For local development, ensure you're logged in with `az login` or `azd auth login`.
