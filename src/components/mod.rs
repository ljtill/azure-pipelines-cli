use anyhow::Result;
use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::Frame;
use ratatui::layout::Rect;

/// A self-contained UI component following the
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
    /// Handle a key event. Return an `Action` if async or cross-component work
    /// is needed; return `None` for purely local state changes.
    fn handle_key_event(&mut self, _key: KeyEvent) -> Result<Option<crate::events::Action>> {
        Ok(None)
    }

    /// Handle a mouse event.
    fn handle_mouse_event(&mut self, _mouse: MouseEvent) -> Result<Option<crate::events::Action>> {
        Ok(None)
    }

    /// Render this component into the given area. Must not mutate state.
    fn draw(&self, frame: &mut Frame, area: Rect) -> Result<()>;

    /// Return the footer hint text describing this component's keybindings.
    fn footer_hints(&self) -> &str {
        ""
    }
}
