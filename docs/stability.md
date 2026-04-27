# Stability

The following public surfaces are frozen for the 1.x line. New behavior may be added, but nothing below will be renamed, removed, or repurposed without a major version bump.

## Config schema

Keys under `[devops.connection]`, `[devops.filters]`, `[devops.update]`, `[devops.logging]`, `[devops.notifications]`, and `[devops.display]`. New keys may be added (always with defaults so existing configs keep working). An optional top-level `schema_version` field is accepted (default `1`).

Configs are forward-compatible within a major version: newer `devops` binaries will load older configs. Downgrading is **not** supported — a binary will refuse to load a config whose `schema_version` is higher than it understands, with a clear error message.

## Keybindings

Every binding in [keybindings.md](keybindings.md) keeps its current action for 1.x. New bindings may be added; existing keys will not be reassigned.

## File paths

Stable and safe to script against:

- `~/.config/devops/config.toml` — config.
- `~/.local/state/devops/` — persistent state.
- `~/.local/share/devops/versions/` — installed release binaries.
- `~/.local/bin/devops` — symlink to the active version.

## CLI surface

The `devops`, `devops version`, and `devops update` subcommands, along with the `--config` and `--api-version` flags, are stable. New flags and subcommands may be introduced.

This version targets Azure DevOps REST API v7.1. Pass `--api-version` or set `DEVOPS_API_VERSION=X.Y` to override.
