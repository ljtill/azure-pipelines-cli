# azure-devops-cli

A terminal (TUI) dashboard for Azure DevOps.

Built with [ratatui](https://ratatui.rs/) and designed to run inside any modern terminal emulator.

## Features

- **Area-based navigation** — Move between Dashboard, Boards, Repos, and Pipelines from a consistent top bar
- **Dashboard** — Cross-service landing view with pinned pipelines and personal pull requests
- **Boards Backlog** — Read-only backlog tree with collapse/expand, search, refresh, and open-in-browser support
- **Pipelines Definitions view** — Flat, searchable list of all pipeline definitions
- **Active Runs view** — All currently running builds across the fleet
- **Build History** — Drill into a pipeline's recent builds
- **Log Viewer** — Drill into a build to view live log output with collapsible timeline tree
- **Search / filter** — Incremental search in Boards, Pipelines, Active Runs, and Pull Requests views
- **Queue pipeline** — Trigger a new pipeline run directly from the TUI
- **Cancel build** — Stop a running build; multi-select batch cancel in Active Runs
- **Retry stage** — Re-run a failed stage without re-queuing the entire pipeline
- **Approve / Reject checks** — Approve or reject environment approval gates inline from the Log Viewer
- **Open in browser** — Jump to pipelines, builds, pull requests, and work items in the Azure DevOps web UI
- **Auto-refresh** — Background polling with configurable interval (default 15s)
- **Build state-change notifications** — Inline notifications when builds start, succeed, or fail (configurable)
- **In-app settings** — Edit configuration live, save, and reload without restarting
- **First-run setup** — Interactive setup wizard when no config file exists
- **Auto-update check** — Background check for new releases on GitHub; persistent notification when an update is available
- **Self-update** — `devops update` downloads the latest release and updates the symlink
- **Azure CLI auth** — Uses `DeveloperToolsCredential` (Azure CLI / Azure Developer CLI chain)

## Installation

### macOS / Linux

```sh
curl -fsSL https://raw.githubusercontent.com/ljtill/azure-devops-cli/main/install.sh | sh
```

Pin a specific version:

```sh
VERSION=1.0.0 curl -fsSL https://raw.githubusercontent.com/ljtill/azure-devops-cli/main/install.sh | sh
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/ljtill/azure-devops-cli/main/install.ps1 | iex
```

### From source

```bash
cargo install --path .
```

## Configuration

On first launch, an interactive setup wizard creates `~/.config/devops/config.toml` with your Azure DevOps organization and project. Boards uses that same project and resolves the default team/backlog at runtime, so there is no separate Boards section to configure. All settings can be adjusted in-app with `,` (settings).

### Stability

The following public surfaces are frozen for the 1.x line. New behavior may be added, but nothing below will be renamed, removed, or repurposed without a major version bump.

- **Config schema** — Keys under `[azure_devops]`, `[filters]`, `[update]`, `[logging]`, `[notifications]`, and `[display]`. New keys may be added (always with defaults so existing configs keep working). An optional top-level `schema_version` field is accepted (default `1`); unknown values are warned about but do not prevent the config from loading.
- **Keybindings** — Every binding in the table below keeps its current action for 1.x. New bindings may be added; existing keys will not be reassigned.
- **File paths** — `~/.config/devops/config.toml` (config), `~/.local/state/devops/` (persistent state), `~/.local/share/devops/versions/` (installed release binaries), and `~/.local/bin/devops` (symlink to the active version) are stable and safe to script against.
- **CLI surface** — The `devops`, `devops version`, and `devops update` subcommands, along with the `--config` flag, are stable. New flags and subcommands may be introduced.

### Display options

The `[display]` section controls refresh and log-viewer behavior:

```toml
[display]
refresh_interval_secs = 15      # Data refresh cadence. Min 5.
log_refresh_interval_secs = 5   # Log refresh cadence. Min 1.
max_log_lines = 100000          # Ring-buffer cap for live log output. Min 1000.
```

`max_log_lines` bounds the memory used by the log viewer. When a task emits more lines than the cap, the oldest lines are dropped (FIFO) so the tail of the log — the part users actually want to see in follow mode — is always preserved. A subtle banner at the top of the log pane surfaces how many lines were dropped.

### Known limitations

- **Log buffer cap.** The log viewer keeps at most `max_log_lines` lines in memory (default 100000). For builds that produce more output than this, the oldest lines are truncated with a visible banner.
- **Pagination safety cap.** The Azure DevOps REST client refuses to follow more than 1000 continuation-token pages for a single list endpoint, as a defense against server-side loops. This limit is well above any realistic production workload.

## Usage

```bash
# Uses default config path
devops

# Override config path
devops --config /path/to/config.toml

# Print installed version
devops version

# Update to the latest release
devops update
```

## Keybindings

| Key          | Action                                                |
|--------------|-------------------------------------------------------|
| ↑ / ↓        | Navigate list items                                   |
| → / Enter    | Drill into selected item / expand (tree views)        |
| ← / q / Esc  | Go back / collapse (tree views)                       |
| Home / End   | Jump to first / last item                             |
| 1 / 2 / 3 / 4 | Switch between Dashboard / Boards / Repos / Pipelines areas |
| [/]          | Switch views within the current area                 |
| /            | Search / filter (Boards / Pipelines / Active Runs / Pull Requests) |
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
