# Copilot instructions

## Build, test, and lint

- Build: `cargo build`
- Type/check all targets without building binaries: `cargo check --all-targets`
- Run locally: `cargo run -- --config ~/.config/devops/config.toml`
- Install the binary from the repo root: `cargo install --path .`
- Test all: `cargo test`
- Run a single test: `cargo test <test_name_substring>`
- Format check: `cargo fmt --all -- --check`
- Clippy: `cargo clippy --all-targets -- -D warnings`
- Security checks, when the tools are available: `cargo audit` and `cargo deny --all-features check`.
- Current baseline: `cargo build`, `cargo test`, `cargo fmt --all -- --check`, and `cargo clippy --all-targets -- -D warnings` all pass cleanly. Keep it that way — do not commit code that introduces new warnings or formatting drift.

Before committing, always run:

```sh
cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test
```

A git pre-commit hook (`.githooks/pre-commit`) enforces these same checks. One-time setup: `git config core.hooksPath .githooks`.

## Runtime and configuration

- Config is loaded from `--config <path>` when provided; otherwise `src/config.rs` prefers `$XDG_CONFIG_HOME/devops/config.toml` and falls back to `~/.config/devops/config.toml`, even on macOS.
- Required config keys are `[devops.connection].organization` and `[devops.connection].project`.
- All app config lives under `[devops]` (`connection`, `filters`, `update`, `logging`, `notifications`, `display`). Add new keys with serde defaults so older configs keep loading; reject only a `schema_version` newer than `CURRENT_SCHEMA_VERSION`.
- Display defaults live in `DisplayConfig`: 15 seconds for main data refresh, 5 seconds for log refresh, and a 100000-line log buffer cap.
- Authentication uses `DeveloperToolsCredential` (Azure CLI / Azure Developer CLI chain); local development depends on `az login` or `azd auth login`.
- The Azure DevOps REST API defaults to version `7.1`; override with `--api-version` or `DEVOPS_API_VERSION`. Keep endpoint builders wired through `Endpoints::api_version` so overrides reach every request.

## High-level architecture

The codebase is a Rust/ratatui terminal dashboard organized around top-level `Service` areas (Dashboard, Boards, Repos, Pipelines) and `View` variants. It follows the [ratatui Component Architecture](https://ratatui.rs/concepts/application-patterns/component-architecture/) pattern.

### Data flow

1. **Input** — Terminal events are dispatched to per-view handlers in `events/`, which return an `Action` describing the intent.
2. **Action handling** — `state/actions/dispatch.rs` processes the action, spawning async API calls via helpers in `state/actions/spawn.rs`.
3. **Message handling** — Async results arrive as `AppMessage` variants (defined in `state/messages.rs`). `state/actions/messages.rs` applies them to `App` state.
4. **Rendering** — `render/mod.rs` delegates to each component's `draw_with_app()` method. Components never perform mutations or network calls.

### Module layout

| Module | Purpose |
|---|---|
| `client/` | Azure DevOps REST layer — `auth.rs` (bearer tokens), `http/` (reqwest wrapper and per-domain submodules for builds, definitions, approvals, retention, pull requests, boards), `endpoints/` (URL builders), `models/` (ADO payload types), `wiql.rs` (safe WIQL escaping). |
| `state/` | Application state (`App` in `mod.rs`), per-view sub-states, `actions/` (dispatch → spawn → messages loop), `run.rs` (main event loop with `tokio::select!`). |
| `components/` | Self-contained UI components implementing the `Component` trait. Each view (Dashboard, Boards, work items, Pull Requests, Pipeline lists/history/logs) plus overlays (Header, Help, Settings) has its own module. |
| `events/` | Keyboard/mouse event handling — split per-view (`events/dashboard.rs`, etc.) with shared handlers in `events/common.rs`. Returns `Action` variants. |
| `render/` | Shared rendering: `helpers.rs` (status icons, elapsed time, truncation), `theme.rs` (color constants), `setup.rs` (first-run wizard), `columns.rs`/`table.rs` (canonical list column schemas). |
| `shared/` | Cross-cutting infrastructure: `ListNav`, `RefreshState`, `Notifications`, `LogBuffer`, and secret handling. |
| `config.rs` | TOML configuration loading and validation. |
| `update.rs` | Self-update mechanism (GitHub releases). |

### Adding a new view

1. Create `src/components/my_view.rs` implementing the `Component` trait.
2. Create `src/events/my_view.rs` with view-specific key handling.
3. Add the view state to `App` and a `View::MyView` variant in `state/mod.rs`; wire root views through `Service::root_views()` if it belongs in a top-level area.
4. Register rendering in `render/mod.rs` and event dispatch in `events/mod.rs`.

## Key conventions

- **Direct-to-`main` workflow.** Commit and push straight to `main` by default. Do not create worktrees, feature branches, or PR flows unless explicitly asked.
- **Network orchestration stays out of components.** New remote work follows `Action` → `spawn_*` helper in `state/actions/spawn.rs` → `AppMessage` → `handle_message` in `state/actions/messages.rs`. Components are render-only.
- **Generation counters are stale-response guards.** Build history, log/timeline, pull request, Boards, and My Work Items fetches carry generation values; stale async responses are dropped instead of overwriting newer state.
- **ADO status/result strings are case-insensitive.** Reuse `eq_ignore_ascii_case` and the shared render helpers rather than matching a single exact casing.
- **Endpoint and WIQL construction is centralized.** Put URLs in `src/client/endpoints/*` so path encoding and API-version overrides stay consistent. Use `wiql_escape` before interpolating user/config values into WIQL strings.
- **Dashboard grouping uses raw ADO definition paths.** Root is stored as `\`; the user-facing `" / "` folder display is derived at render time.
- **Dashboard identity filtering is strict.** Use `ExactUserIdentity` fields (`id`, `unique_name`, `descriptor`) for "mine" filters; do not fall back to display-name matching for verified Dashboard PR/work-item sections.
- **Boards tree rows are derived from hydrated parent fields.** Backlog data is seeded from Epics plus recursive hierarchy links, then `derive_child_ids_from_parents` normalizes children to avoid duplicate or wrongly attached rows.
- **Timeline flattening.** `LogViewer` flattens the ADO hierarchy from `Stage → Phase → Job → Task` into `Stage → Job row → Task`. "Job" rows may represent either ADO `Phase` or `Job` records.
- **Collapse state is keyed by stable IDs** (`collapsed_stages`, `collapsed_jobs`, Boards work item IDs), not by visible row indices. Rebuild row lists after structural state changes.
- **Log viewer modes.** Follow mode tracks the active task and auto-refreshes; inspect mode pins the selected task after Enter.
- **Per-view sub-states.** `App` is decomposed into `data` (`CoreData`), `filters` (`FilterConfig`), `search` (`SearchState`), and view-specific component structs (e.g., `dashboard: Dashboard`, `pipelines: Pipelines`). Rebuild methods live on the component and take `&CoreData`/`&FilterConfig` parameters — do not add new fields directly to `App` when they belong to a specific view.
- **Data refresh derives aggregate views.** `DataRefresh` derives active builds from recent builds, builds `latest_builds_by_def` from `definition.latest_build` overlaid with newer recent builds, and rebuilds Dashboard/Pipelines/Active Runs from that shared state.
- **Test helpers.** `src/test_helpers.rs` provides factory functions (`make_app`, `make_build`, `make_definition`, `make_config`, `make_simple_timeline`) used by both unit tests in `src/` and integration tests in `tests/`.
- **List view column layout.** All list views source their column widths from shared schemas in `src/render/columns.rs` (`build_row`, `pull_request_row`, `work_item_row`, `board_row`) built on the `Column` / `ColumnWidth::{Fixed, Flex}` primitive in `src/render/table.rs`. Views call `render_header(f, area, &schema.columns)` to draw a muted single-line header and receive the body rect, then reuse `resolve_widths` with the same schema so the header and rows line up exactly. New columns belong in the schema, not in per-view `Layout::horizontal` calls — `Flex { weight, min, max }` keeps extra horizontal space in the primary text columns instead of sprawling into the last column.

## Code comments

Every source file follows a uniform comment style. Preserve these conventions when adding or editing code.

### Module docs (`//!`)

Every `.rs` file starts with a `//!` module doc — a single sentence describing the module's purpose, ending with a period.

```rust
//! Shared rendering utilities for status icons, elapsed time, and text truncation.
```

### Doc comments (`///`)

- Start with a **third-person present tense verb**: "Returns…", "Renders…", "Handles…", "Represents…".
- End every doc comment (including the last line of multi-line docs) with a **period**.
- Use sentence case.
- Only document public items that benefit from it — do not add trivial comments to self-explanatory code.

```rust
/// Returns the compiled-in version string.
pub fn version() -> &'static str { ... }

/// Spawns an async task that fetches build definitions from the
/// Azure DevOps REST API and sends the result as an `AppMessage`.
pub fn spawn_fetch_definitions(...) { ... }
```

### Inline comments (`//`)

- Sentence case, ending with a **period**.
- Always include a space after `//`.
- Explain *why*, not *what* — prefer no comment over a comment that restates the code.

```rust
// Derive active builds from recent builds instead of a separate API call.
```

### Section dividers

Use the short form only:

```rust
// --- Section Name ---
```

Do **not** use full-width dividers (`// -------...`) or equals dividers (`// =======...`).

### Anti-patterns

- **No commented-out code.** Delete it; git keeps history.
- **No empty doc comments.** Remove standalone `///` lines that carry no text (paragraph-separator `///` lines within multi-line docs are fine).
