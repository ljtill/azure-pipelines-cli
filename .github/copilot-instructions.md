# Copilot instructions

## Build, test, and lint

- Build: `cargo build`
- Run locally: `cargo run -- --config ~/.config/pipelines/config.toml`
- Install the binary from the repo root: `cargo install --path .`
- Test all: `cargo test`
- Run a single test: `cargo test <test_name_substring>` (there are currently no committed Rust tests, so this is the form to use when tests are added)
- Format check: `cargo fmt --check`
- Clippy: `cargo clippy --all-targets -- -D warnings`
- Current baseline: `cargo build`, `cargo test`, `cargo fmt --check`, and `cargo clippy --all-targets -- -D warnings` all pass cleanly. Keep it that way — do not commit code that introduces new warnings or formatting drift.

## Pre-commit checks

A git pre-commit hook enforces the same checks that CI runs. One-time setup:

```sh
git config core.hooksPath .githooks
```

The hook runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` before every commit. If any check fails, the commit is blocked.

Before committing, always run:

```sh
cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test
```

## Runtime and configuration

- Config is loaded from `--config <path>` when provided; otherwise `src/config.rs` prefers `$XDG_CONFIG_HOME/pipelines/config.toml` and falls back to `~/.config/pipelines/config.toml`, even on macOS.
- Required config keys are `[azure_devops].organization` and `[azure_devops].project`.
- Display refresh defaults live in `DisplayConfig`: 30 seconds for main data refresh and 5 seconds for log refresh.
- Authentication is via `azure_identity::DefaultAzureCredential`; local development usually depends on `az login`.

## High-level architecture

- `src/main.rs` is the coordinator. It loads config, creates `AdoClient`, sets up the terminal/panic hook, and runs a `tokio::select!` loop that multiplexes terminal input with async refresh results.
- `src/events.rs` owns keybinding behavior. It mutates `App` for purely local UI changes and returns an `Action` only when async work is required.
- `src/app/` is the central state model, decomposed into per-view sub-states. `App` groups state into: `data` (`CoreData` — shared API data), `filters` (`FilterConfig`), `search` (`SearchState`), and view-specific sub-structs (`dashboard`, `pipelines`, `active_runs`, `build_history`, `log_viewer`). Each sub-state owns its `ListNav` and rebuild logic; rebuild methods take `&CoreData`/`&FilterConfig` parameters instead of reaching into `App` directly.
- `src/api/` is a thin Azure DevOps REST layer: `auth.rs` gets bearer tokens, `endpoints.rs` builds URLs, `client.rs` wraps `reqwest`, and `models.rs` mirrors the ADO payloads used by the UI.
- `src/ui/` is render-only. `ui/mod.rs` switches on `App.view`, screen modules draw from `App`, and `src/ui/helpers.rs` centralizes status icon, elapsed-time, and truncation helpers shared across views.
- The navigation flow is Dashboard/Pipelines/Active Runs -> Build History or Log Viewer -> task log. Background refreshes land back in the app through `AppMessage`.

## Key conventions

- This repository uses a direct-to-`main` workflow. Commit and push straight to `main` by default.
- Do not create worktrees, feature branches, or PR-only flows here unless explicitly asked.
- Keep network orchestration out of UI modules. New remote work should normally be modeled as `Action` -> `spawn_*` helper in `main.rs` -> `AppMessage` -> `handle_message`.
- Preserve `log_generation` behavior when touching log/timeline code. It is the stale-response guard that prevents old async log/timeline results from overwriting the newly selected build.
- Azure DevOps status/result strings are treated case-insensitively in current code. Reuse `eq_ignore_ascii_case` and the shared UI helpers rather than matching a single exact casing.
- Dashboard grouping uses the raw ADO definition path as state. Root is stored as `\`, and the user-facing `" / "` folder display is derived from that raw value instead of being stored directly.
- `LogViewerState::rebuild_timeline_rows` intentionally flattens the ADO hierarchy from `Stage -> Phase -> Job -> Task` into `Stage -> Job row -> Task`. "Job" rows in the UI may represent either ADO `Phase` or `Job` records.
- Timeline collapse state is keyed by ADO record IDs (`collapsed_stages`, `collapsed_jobs`), not by visible row indices. Rebuild the rows after structural state changes instead of mutating the rendered list directly.
- Log viewer behavior has two modes: follow mode tracks the active task and auto-refreshes that log, while inspect mode pins the currently selected task after Enter.
- `App::new` now builds the header label from config (`org_project_label`), so config loading and header rendering need to stay in sync.
- `filters` exists in `Config` but is not wired into API calls or rendering yet; treat it as reserved/incomplete, not as an active feature.
- `App` is decomposed into per-view sub-states: `data` (`CoreData`), `filters` (`FilterConfig`), `search` (`SearchState`), `dashboard` (`DashboardState`), `pipelines` (`PipelinesState`), `active_runs` (`ActiveRunsState`), `build_history` (`BuildHistoryState`), and `log_viewer` (`LogViewerState`). Rebuild methods live on the sub-state and take `&CoreData`/`&FilterConfig` parameters — do not add new fields directly to `App` when they belong to a specific view.
