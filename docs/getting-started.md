# Getting started

A terminal (TUI) dashboard for Azure DevOps, built with [ratatui](https://ratatui.rs/) and designed to run inside any modern terminal emulator.

## First run

On first launch, an interactive setup wizard creates `~/.config/devops/config.toml` with your Azure DevOps organization and project. Boards uses that same project and resolves the default team/backlog at runtime, so there is no separate Boards section to configure. Do not store tokens, PATs, or other secrets in the config file; sign in with Azure CLI or Azure Developer CLI instead. All settings can be adjusted in-app with `,` (settings).

```bash
# Uses default config path
devops

# Override config path
devops --config /path/to/config.toml

# Override the Azure DevOps REST API version (default: 7.1)
devops --api-version 7.2-preview.3
DEVOPS_API_VERSION=7.2-preview.3 devops

# Print installed version
devops version

# Verify and update to the latest release
devops update
```

See [configuration.md](configuration.md) for the full config schema and [authentication.md](authentication.md) for sign-in details.

## Feature overview

### Navigation

- **Area-based navigation** — Move between Dashboard, Boards, Repos, and Pipelines from a consistent top bar; `Tab` / `Shift+Tab` cycles views within the current area.
- **Search / filter** — Incremental search in Boards, My Work Items, Pipelines, Active Runs, and Pull Requests views.

### Dashboard

Cross-service landing view with pinned pipelines, pinned work items, and personal pull requests. Pin items from any view with `p`.

### Boards

Read-only backlog tree plus personal "Assigned to me" and "Created by me" work item lists, with drill-in work item detail view.

### Repos / Pull Requests

"Created by me", "Assigned to me", and "All active" PR lists, with drill-in pull request detail view.

### Pipelines

- **Definitions view** — Flat, searchable list of all pipeline definitions.
- **Active Runs view** — All currently running builds across the fleet.
- **Build History** — Drill into a pipeline's recent builds; delete retention leases to allow pruning.
- **Log Viewer** — Drill into a build to view live log output with collapsible timeline tree.
- **Queue pipeline** — Trigger a new pipeline run directly from the TUI (`n`).
- **Cancel build** — Stop a running build (`c`); multi-select batch cancel in Active Runs.
- **Retry stage** — Re-run a failed stage without re-queuing the entire pipeline (`t`).
- **Approve / Reject checks** — Approve (`a`) or reject (`j`) environment approval gates inline from the Log Viewer.

### General

- **Open in browser** — Jump to pipelines, builds, pull requests, and work items in the Azure DevOps web UI (`o`).
- **Auto-refresh** — Background polling with configurable interval (default 15s).
- **Build state-change notifications** — Inline notifications when builds start, succeed, or fail (configurable).
- **In-app settings** — Edit configuration live, save, and reload without restarting (`,`).
- **First-run setup** — Interactive setup wizard when no config file exists.
- **Auto-update check** — Background check for new releases on GitHub; persistent notification when an update is available.
- **Self-update** — `devops update` verifies the Sigstore-signed checksum manifest, verifies the archive SHA-256, and updates the active binary.
- **Azure CLI auth** — Uses `DeveloperToolsCredential` (Azure CLI / Azure Developer CLI chain).

## Next steps

- [Keybindings](keybindings.md) — full key reference.
- [Installation](install.md) — secure install and update verification.
- [Configuration](configuration.md) — config file schema and display options.
- [Limitations](limitations.md) — known caps and how to override them.
- [Stability](stability.md) — 1.x compatibility guarantees.
