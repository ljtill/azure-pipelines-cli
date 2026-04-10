use crossterm::event::{KeyCode, KeyEvent};

use super::Action;
use super::navigation;
use crate::app::{App, InputMode};

pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('/') => {
            tracing::debug!(view = ?app.view, "entering search mode");
            app.search.mode = InputMode::Search;
            Action::None
        }
        KeyCode::Right => handle_enter_pipelines(app),
        KeyCode::Enter => handle_enter_pipelines(app),
        KeyCode::Char('Q') => navigation::handle_queue_request(app),
        KeyCode::Char('o') => navigation::handle_open_in_browser(app),
        _ => Action::None,
    }
}

fn handle_enter_pipelines(app: &mut App) -> Action {
    if let Some(def) = app
        .pipelines
        .filtered
        .get(app.pipelines.nav.index())
        .cloned()
    {
        let def_id = def.id;
        app.navigate_to_build_history(def);
        Action::FetchBuildHistory(def_id)
    } else {
        Action::None
    }
}

#[cfg(test)]
mod tests {
    use crate::app::View;
    use crate::events::{Action, handle_key};
    use crate::test_helpers::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn right_arrow_on_pipelines_drills_into_build_history() {
        let mut app = make_app();
        app.view = View::Pipelines;
        assert!(!app.pipelines.filtered.is_empty());

        let action = handle_key(&mut app, key(KeyCode::Right));
        assert_eq!(app.view, View::BuildHistory);
        assert!(matches!(action, Action::FetchBuildHistory(_)));
    }

    #[test]
    fn left_arrow_on_pipelines_is_noop() {
        let mut app = make_app();
        app.view = View::Pipelines;

        let action = handle_key(&mut app, key(KeyCode::Left));
        assert_eq!(app.view, View::Pipelines);
        assert!(matches!(action, Action::None));
    }
}
