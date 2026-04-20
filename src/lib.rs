//! # azure-devops-cli
//!
//! A terminal-based dashboard for monitoring and controlling Azure DevOps,
//! built with [ratatui](https://ratatui.rs) and crossterm.
//!
//! ## Architecture
//!
//! The codebase follows the [ratatui Component Architecture](https://ratatui.rs/concepts/application-patterns/component-architecture/)
//! pattern. Each view is a self-contained component in [`components`] that owns
//! its rendering logic and keybinding hints.
//!
//! ## Modules
//!
//! | Module         | Purpose |
//! |----------------|---------|
//! | [`client`]     | Azure DevOps REST layer — split by domain (`models/`, `client/`, `endpoints/`) with per-domain submodules for builds, definitions, approvals, and retention leases. |
//! | [`state`]      | Application state (`App`), per-view sub-states, and the async action/message loop (`actions/dispatch.rs`, `actions/messages.rs`, `actions/spawn.rs`). |
//! | [`components`] | Self-contained UI components following the ratatui Component trait pattern. Each view (Dashboard, Pipelines, Active Runs, Build History, Log Viewer) plus overlays (Header, Help, Settings) has its own module. |
//! | [`config`]     | TOML configuration loading and validation (`--config` flag or XDG default). |
//! | [`events`]     | Keyboard/mouse event handling — split per-view (`events/dashboard.rs`, etc.) with shared handlers in `events/common.rs`. |
//! | [`shared`]     | Cross-cutting infrastructure: `RefreshState`, `ListNav`, `Notifications`, `SecretString`. |
//! | [`render`]     | Shared rendering utilities: `helpers.rs` (status icons, elapsed time), `theme.rs` (color constants), `setup.rs` (first-run wizard). |
//! | [`update`]     | Self-update mechanism that pulls new releases from GitHub. |
//!
//! ## Data flow
//!
//! 1. **Input** — Terminal events are dispatched to per-view handlers in [`events`],
//!    which return an [`events::Action`] describing the intent.
//! 2. **Action handling** — [`state::actions::handle_action`] processes the action,
//!    spawning async API calls via helpers in `actions/spawn.rs`.
//! 3. **Message handling** — Async results arrive as `AppMessage` variants.
//!    [`state::actions::handle_message`] applies them to [`state::App`] state.
//! 4. **Rendering** — [`render::draw`] delegates to each component's `draw_with_app()`
//!    method. Components never perform mutations or network calls.
//!
//! ## Adding a new view
//!
//! 1. Create `src/components/my_view.rs` with a struct implementing rendering.
//! 2. Create `src/events/my_view.rs` with view-specific key handling.
//! 3. Add the view state to `App` and a `View::MyView` variant.
//! 4. Register rendering in `render/mod.rs` and event dispatch in `events/mod.rs`.

pub mod client;
pub mod components;
pub mod config;
pub mod events;
pub mod render;
pub mod shared;
pub mod state;
pub mod update;

/// Provides test factory functions for unit and integration tests.
///
/// Kept public for integration tests in `tests/`. Not intended for
/// downstream consumers — will be gated behind a feature flag if
/// published as a library crate.
pub mod test_helpers;
