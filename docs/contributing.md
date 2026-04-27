# Contributing

## Build, test, and lint

- Build: `cargo build`
- Run locally: `cargo run -- --config ~/.config/devops/config.toml`
- Install the binary from the repo root: `cargo install --path .`
- Test all: `cargo test`
- Run a single test: `cargo test <test_name_substring>`
- Format check: `cargo fmt --check`
- Clippy: `cargo clippy --all-targets -- -D warnings`

The current baseline is clean — `cargo build`, `cargo test`, `cargo fmt --check`, and `cargo clippy --all-targets -- -D warnings` all pass without warnings or formatting drift. Keep it that way.

Before committing, always run:

```sh
cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test
```

A git pre-commit hook (`.githooks/pre-commit`) enforces these same checks. One-time setup:

```sh
git config core.hooksPath .githooks
```

## High-level architecture

The codebase follows the [ratatui Component Architecture](https://ratatui.rs/concepts/application-patterns/component-architecture/) pattern.

### Data flow

1. **Input** — Terminal events are dispatched to per-view handlers in `events/`, which return an `Action` describing the intent.
2. **Action handling** — `state/actions/dispatch.rs` processes the action, spawning async API calls via helpers in `state/actions/spawn.rs`.
3. **Message handling** — Async results arrive as `AppMessage` variants (defined in `state/messages.rs`). `state/actions/messages.rs` applies them to `App` state.
4. **Rendering** — `render/mod.rs` delegates to each component's `draw_with_app()` method. Components never perform mutations or network calls.

### Module layout

| Module        | Purpose |
|---------------|---------|
| `client/`     | Azure DevOps REST layer — `auth.rs` (bearer tokens), `http/` (reqwest wrapper, per-domain submodules), `endpoints/` (URL builders), `models/` (ADO payload types). |
| `state/`      | Application state (`App` in `mod.rs`), per-view sub-states, `actions/` (dispatch → spawn → messages loop), `run.rs` (main event loop with `tokio::select!`). |
| `components/` | Self-contained UI components implementing the `Component` trait. Each view plus overlays (Header, Help, Settings) has its own module. |
| `events/`     | Keyboard/mouse event handling — split per-view with shared handlers in `events/common.rs`. Returns `Action` variants. |
| `render/`     | Shared rendering: `helpers.rs` (status icons, elapsed time, truncation), `theme.rs` (colors), `setup.rs` (first-run wizard), `columns.rs`/`table.rs` (column layout primitives). |
| `shared/`     | Cross-cutting infrastructure: `ListNav`, `RefreshState`, `Notifications`. |
| `config.rs`   | TOML configuration loading and validation. |
| `update.rs`   | Self-update mechanism (GitHub releases). |

### Adding a new view

1. Create `src/components/my_view.rs` implementing the `Component` trait.
2. Create `src/events/my_view.rs` with view-specific key handling.
3. Add the view state to `App` and a `View::MyView` variant in `state/mod.rs`.
4. Register rendering in `render/mod.rs` and event dispatch in `events/mod.rs`.

## Conventions

- **Network orchestration stays out of components.** Remote work follows `Action` → `spawn_*` helper in `state/actions/spawn.rs` → `AppMessage` → `handle_message` in `state/actions/messages.rs`. Components are render-only.
- **`log_generation` is a stale-response guard.** Timeline and log messages carry a generation counter; stale results from a previously selected build are silently dropped.
- **ADO status/result strings are case-insensitive.** Reuse `eq_ignore_ascii_case` and the shared render helpers.
- **Dashboard grouping uses raw ADO definition paths.** Root is stored as `\`; the user-facing `" / "` folder display is derived at render time.
- **Timeline flattening.** `LogViewer` flattens `Stage → Phase → Job → Task` into `Stage → Job row → Task`.
- **Collapse state is keyed by ADO record IDs**, not by visible row indices. Rebuild row lists after structural state changes.
- **Per-view sub-states.** `App` is decomposed into `data` (`CoreData`), `filters` (`FilterConfig`), `search` (`SearchState`), and view-specific component structs. Don't add fields to `App` that belong to a specific view.
- **List view column layout.** All list views source widths from shared schemas in `src/render/columns.rs` built on the `Column` / `ColumnWidth::{Fixed, Flex}` primitive in `src/render/table.rs`.
- **Test helpers.** `src/test_helpers.rs` provides factory functions used by both unit tests in `src/` and integration tests in `tests/`.

## Code comments

- Every `.rs` file starts with a `//!` module doc — a single sentence ending with a period.
- `///` doc comments start with a third-person present-tense verb ("Returns…", "Renders…") and end with a period.
- Inline `//` comments are sentence case, end with a period, and explain *why*, not *what*. Always include a space after `//`.
- Use the short divider form only: `// --- Section Name ---`. No full-width `// ----` or `// ====` dividers.
- No commented-out code; no empty doc comments.

## Reporting security issues

See [SECURITY.md](../SECURITY.md).
