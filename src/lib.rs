//! # azure-pipelines-cli
//!
//! A terminal-based dashboard for monitoring and controlling Azure DevOps
//! Pipelines, built with [ratatui](https://ratatui.rs) and crossterm.
//!
//! ## Modules
//!
//! | Module    | Purpose |
//! |-----------|---------|
//! | [`api`]   | Azure DevOps REST client layer — auth, endpoint builders, HTTP client, and response models. |
//! | [`app`]   | Application state model (`App`) decomposed into per-view sub-states (`DashboardState`, `PipelinesState`, `ActiveRunsState`, `BuildHistoryState`, `LogViewerState`) plus shared `CoreData`, `FilterConfig`, and `SearchState`. The `Action` → spawn → `AppMessage` async loop lives here too. |
//! | [`config`]| TOML configuration loading and validation (`--config` flag or XDG default). |
//! | [`events`]| Keyboard/mouse event handling — translates terminal input into [`app::actions::Action`] variants. |
//! | [`ui`]    | Render-only TUI modules. Each view has a dedicated module; shared helpers and theming live in submodules. |
//! | [`update`]| Self-update mechanism that pulls new releases from GitHub. |
//!
//! ## Data flow
//!
//! The crate follows an async event loop:
//!
//! 1. **Input** — Terminal events are dispatched to [`events::handle_key`], which
//!    returns an [`app::actions::Action`] describing the intent.
//! 2. **Action handling** — [`app::actions::handle_action`] processes the action,
//!    spawning async API calls via the `spawn_api` helper when network work is
//!    needed.
//! 3. **Message handling** — Async results arrive as [`app::actions::AppMessage`]
//!    variants. [`app::actions::handle_message`] applies them to [`app::App`]
//!    state.
//! 4. **Rendering** — [`ui::draw`] reads `App` state and renders the current
//!    frame. UI modules never perform mutations or network calls.
//!
//! ## Key design decisions
//!
//! - **`log_generation`** — A monotonic counter that guards against stale async
//!   responses overwriting the log or timeline of a newly selected build.
//! - **Timeline flattening** — ADO's Stage → Phase → Job → Task hierarchy is
//!   collapsed to Stage → Job → Task for display. "Job" rows may represent
//!   either ADO Phase or Job records.
//! - **Follow vs inspect mode** — The log viewer defaults to *follow* mode
//!   (tracks the active task, auto-refreshes). Pressing Enter switches to
//!   *inspect* mode, pinning the selected task.

pub mod api;
pub mod app;
pub mod components;
pub mod config;
pub mod events;
pub mod shared;
pub mod ui;
pub mod update;

// TODO: gate behind a `test-helpers` feature to exclude from release builds
pub mod test_helpers;
