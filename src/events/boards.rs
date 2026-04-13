//! Keyboard event handling for the Azure Boards placeholder view.

use crossterm::event::KeyEvent;

use super::Action;
use crate::state::App;

/// Handles keys specific to the Boards placeholder.
pub fn handle_key(_app: &mut App, _key: KeyEvent) -> Action {
    Action::None
}
