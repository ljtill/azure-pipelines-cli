//! UI component trait and shared component infrastructure.

use anyhow::Result;
use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::Frame;
use ratatui::layout::Rect;

pub mod active_runs;
pub mod boards;
pub mod build_history;
pub mod dashboard;
pub mod header;
pub mod help;
pub mod log_viewer;
pub mod my_work_items;
pub mod pipelines;
pub mod pull_request_detail;
pub mod pull_requests;
pub mod settings;

/// Defines a self-contained UI component following the
/// [ratatui component architecture](https://ratatui.rs/concepts/application-patterns/component-architecture/).
///
/// Each component encapsulates its own state, keybindings, rendering, and action
/// handling. The `App` coordinator routes events to the active component and
/// dispatches actions through the component's `update()` method.
///
/// # Lifecycle
///
/// 1. **Construction** — Component is created with initial state.
/// 2. **Event handling** — `handle_key_event` / `handle_mouse_event` translate
///    user input into [`Action`](crate::events::Action) values.
/// 3. **Update** — `update(action)` processes actions (including async results)
///    and returns optional follow-up actions.
/// 4. **Rendering** — `draw(&self, frame, area)` renders the component. The
///    `&self` receiver enforces render-only access.
pub trait Component {
    /// Handles a key event. Returns an `Action` if async or cross-component work
    /// is needed; returns `None` for purely local state changes.
    fn handle_key_event(&mut self, _key: KeyEvent) -> Result<Option<crate::events::Action>> {
        Ok(None)
    }

    /// Handles a mouse event.
    fn handle_mouse_event(&mut self, _mouse: MouseEvent) -> Result<Option<crate::events::Action>> {
        Ok(None)
    }

    /// Renders this component into the given area. Must not mutate state.
    fn draw(&self, frame: &mut Frame, area: Rect) -> Result<()>;

    /// Returns the footer hint text describing this component's keybindings.
    fn footer_hints(&self) -> &'static str {
        ""
    }
}
