# Copilot instructions

## Build, test, and lint

- Build: `cargo build`
- Run locally: `cargo run -- --config ~/.config/pipelines/config.toml`
- Install the binary from the repo root: `cargo install --path .`
- Test all: `cargo test`
- Run a single test: `cargo test <test_name_substring>`
- Format check: `cargo fmt --check`
- Clippy: `cargo clippy --all-targets -- -D warnings`
- Current baseline: `cargo build`, `cargo test`, `cargo fmt --check`, and `cargo clippy --all-targets -- -D warnings` all pass cleanly. Keep it that way — do not commit code that introduces new warnings or formatting drift.

Before committing, always run:

```sh
cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test
```

A git pre-commit hook (`.githooks/pre-commit`) enforces these same checks. One-time setup: `git config core.hooksPath .githooks`.

## Runtime and configuration

- Config is loaded from `--config <path>` when provided; otherwise `src/config.rs` prefers `$XDG_CONFIG_HOME/pipelines/config.toml` and falls back to `~/.config/pipelines/config.toml`, even on macOS.
- Required config keys are `[azure_devops].organization` and `[azure_devops].project`.
- Display refresh defaults live in `DisplayConfig`: 15 seconds for main data refresh and 5 seconds for log refresh.
- Authentication uses `DeveloperToolsCredential` (Azure CLI / Azure Developer CLI chain); local development depends on `az login` or `azd auth login`.

## High-level architecture

The codebase follows the [ratatui Component Architecture](https://ratatui.rs/concepts/application-patterns/component-architecture/) pattern.

### Data flow

1. **Input** — Terminal events are dispatched to per-view handlers in `events/`, which return an `Action` describing the intent.
2. **Action handling** — `state/actions/dispatch.rs` processes the action, spawning async API calls via helpers in `state/actions/spawn.rs`.
3. **Message handling** — Async results arrive as `AppMessage` variants (defined in `state/messages.rs`). `state/actions/messages.rs` applies them to `App` state.
4. **Rendering** — `render/mod.rs` delegates to each component's `draw_with_app()` method. Components never perform mutations or network calls.

### Module layout

| Module | Purpose |
|---|---|
| `client/` | Azure DevOps REST layer — `auth.rs` (bearer tokens), `http/` (reqwest wrapper, per-domain submodules for builds, definitions, approvals, retention), `endpoints/` (URL builders), `models/` (ADO payload types). |
| `state/` | Application state (`App` in `mod.rs`), per-view sub-states, `actions/` (dispatch → spawn → messages loop), `run.rs` (main event loop with `tokio::select!`). |
| `components/` | Self-contained UI components implementing the `Component` trait. Each view (Dashboard, Pipelines, Active Runs, Build History, Log Viewer) plus overlays (Header, Help, Settings) has its own module. |
| `events/` | Keyboard/mouse event handling — split per-view (`events/dashboard.rs`, etc.) with shared handlers in `events/common.rs`. Returns `Action` variants. |
| `render/` | Shared rendering: `helpers.rs` (status icons, elapsed time, truncation), `theme.rs` (color constants), `setup.rs` (first-run wizard). |
| `shared/` | Cross-cutting infrastructure: `ListNav`, `RefreshState`, `Notifications`. |
| `config.rs` | TOML configuration loading and validation. |
| `update.rs` | Self-update mechanism (GitHub releases). |

### Adding a new view

1. Create `src/components/my_view.rs` implementing the `Component` trait.
2. Create `src/events/my_view.rs` with view-specific key handling.
3. Add the view state to `App` and a `View::MyView` variant in `state/mod.rs`.
4. Register rendering in `render/mod.rs` and event dispatch in `events/mod.rs`.

## Key conventions

- **Direct-to-`main` workflow.** Commit and push straight to `main` by default. Do not create worktrees, feature branches, or PR flows unless explicitly asked.
- **Network orchestration stays out of components.** New remote work follows `Action` → `spawn_*` helper in `state/actions/spawn.rs` → `AppMessage` → `handle_message` in `state/actions/messages.rs`. Components are render-only.
- **`log_generation` is a stale-response guard.** Timeline and log messages carry a generation counter; stale results from a previously selected build are silently dropped. Preserve this behavior when touching log/timeline code.
- **ADO status/result strings are case-insensitive.** Reuse `eq_ignore_ascii_case` and the shared render helpers rather than matching a single exact casing.
- **Dashboard grouping uses raw ADO definition paths.** Root is stored as `\`; the user-facing `" / "` folder display is derived at render time.
- **Timeline flattening.** `LogViewer` flattens the ADO hierarchy from `Stage → Phase → Job → Task` into `Stage → Job row → Task`. "Job" rows may represent either ADO `Phase` or `Job` records.
- **Collapse state is keyed by ADO record IDs** (`collapsed_stages`, `collapsed_jobs`), not by visible row indices. Rebuild the row list after structural state changes.
- **Log viewer modes.** Follow mode tracks the active task and auto-refreshes; inspect mode pins the selected task after Enter.
- **Per-view sub-states.** `App` is decomposed into `data` (`CoreData`), `filters` (`FilterConfig`), `search` (`SearchState`), and view-specific component structs (e.g., `dashboard: Dashboard`, `pipelines: Pipelines`). Rebuild methods live on the component and take `&CoreData`/`&FilterConfig` parameters — do not add new fields directly to `App` when they belong to a specific view.
- **Test helpers.** `src/test_helpers.rs` provides factory functions (`make_app`, `make_build`, `make_definition`, `make_config`, `make_simple_timeline`) used by both unit tests in `src/` and integration tests in `tests/`.
