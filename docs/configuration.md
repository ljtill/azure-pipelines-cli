# Configuration

## File location

Config is loaded from `--config <path>` when provided. Otherwise the resolution order is:

1. `$XDG_CONFIG_HOME/devops/config.toml`
2. `~/.config/devops/config.toml` (used on macOS too — there is no platform-specific override)

On first launch with no config file, an interactive setup wizard creates one for you.

## Required keys

```toml
[devops.connection]
organization = "your-org"
project = "your-project"
```

Boards uses the same project and resolves the default team and backlog at runtime — there is no separate `[devops.boards]` section.

Use the organization and project path segments from your Azure DevOps URL (`https://dev.azure.com/{organization}/{project}`), not the full URL. Both values are validated when the config loads and when the setup wizard writes a config:

- They must not be empty or whitespace-only.
- They must not contain control characters such as newlines or tabs.
- They must be 256 bytes or fewer.

Do not store tokens, PATs, service principal credentials, or other secrets in `config.toml`. The CLI does not read credentials from config; authentication uses Azure CLI or Azure Developer CLI sign-in state.

Optional network timeouts default to:

```toml
[devops.connection.timeouts]
request_timeout_secs = 30
connect_timeout_secs = 10
log_timeout_secs = 60
```

`request_timeout_secs` applies to regular Azure DevOps REST calls,
`connect_timeout_secs` caps TCP connection setup, and `log_timeout_secs` applies
to build log downloads. All timeout values must be between 1 and 600 seconds.

## Sections

The config file groups all app tables under a single top-level `[devops]` table. New keys are added with safe defaults so older configs keep loading on newer binaries.

- `[devops.connection]` — organization, project, and related ADO connection settings.
- `[devops.filters]` — view-level filters (e.g. authors, assignees) that persist across runs.
- `[devops.update]` — auto-update check cadence and behavior.
- `[devops.logging]` — log level and file output.
- `[devops.notifications]` — toggles for build state-change notifications.
- `[devops.display]` — refresh cadences and log-viewer caps (see below).

An optional top-level `schema_version` field is accepted (default `1`). See [stability.md](stability.md) for forward/backward compatibility rules.

## `[devops.update]`

```toml
[devops.update]
check_for_updates = true
```

`check_for_updates` controls the background GitHub Release check and update
notification only; it does not install anything. Running `devops update` always
uses the verified update flow described in [install.md](install.md).

Update checks use a 5-second request timeout and update downloads use a 60-second
request timeout by default. Set `DEVOPS_UPDATE_CHECK_TIMEOUT_SECS` or
`DEVOPS_UPDATE_DOWNLOAD_TIMEOUT_SECS` to override those values for constrained
networks; overrides must also be between 1 and 600 seconds.

## `[devops.display]`

```toml
[devops.display]
refresh_interval_secs = 15      # Data refresh cadence. Min 5.
log_refresh_interval_secs = 5   # Log refresh cadence. Min 1.
max_log_lines = 100000          # Ring-buffer cap for live log output. Min 1000.
```

`max_log_lines` bounds the memory used by the log viewer. When a task emits more lines than the cap, the oldest lines are dropped (FIFO) so the tail of the log — the part users actually want to see in follow mode — is always preserved. A subtle banner at the top of the log pane surfaces how many lines were dropped.

## In-app settings

Press `,` to open the settings overlay. Edits are applied live, saved to the active config file, and reloaded without restarting.
