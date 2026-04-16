//! Top-level render dispatch for all application views.

pub mod helpers;
pub mod setup;
pub mod table;
pub mod theme;

use ratatui::Frame;

use crate::components::Component;
use crate::state::App;

pub fn draw(f: &mut Frame, app: &mut App) {
    use ratatui::layout::{Constraint, Layout};

    let chunks = Layout::vertical([
        Constraint::Length(3), // Header.
        Constraint::Min(0),    // Body.
        Constraint::Length(1), // Footer.
    ])
    .split(f.area());

    app.header.draw_with_app(f, app, chunks[0]);

    match app.view {
        crate::state::View::Dashboard => app.dashboard.draw_with_app(f, app, chunks[1]),
        crate::state::View::Pipelines => app.pipelines.draw_with_app(f, app, chunks[1]),
        crate::state::View::ActiveRuns => app.active_runs.draw_with_app(f, app, chunks[1]),
        crate::state::View::BuildHistory => app.build_history.draw_with_app(f, app, chunks[1]),
        crate::state::View::LogViewer => {
            crate::components::log_viewer::draw_log_viewer(f, app, chunks[1]);
        }
        crate::state::View::PullRequestsCreatedByMe
        | crate::state::View::PullRequestsAssignedToMe
        | crate::state::View::PullRequestsAllActive => {
            app.pull_requests.draw_with_app(f, app, chunks[1]);
        }
        crate::state::View::PullRequestDetail => {
            app.pull_request_detail.draw_with_app(f, app, chunks[1]);
        }
        crate::state::View::Boards => app.boards.draw_with_app(f, app, chunks[1]),
        crate::state::View::BoardsAssignedToMe | crate::state::View::BoardsCreatedByMe => {
            app.my_work_items.draw_with_app(f, app, chunks[1]);
        }
    }

    draw_footer(f, app, chunks[2]);

    if app.show_help {
        let _ = app.help.draw(f, f.area());
    }

    if app.show_settings
        && let Some(ref s) = app.settings
    {
        app.settings_component.draw_with_state(f, s);
    }
}

fn draw_footer(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    use ratatui::text::{Line, Span};
    use ratatui::widgets::Paragraph;

    // Show confirmation prompt if active.
    if let Some(prompt) = &app.confirm_prompt {
        let footer = Paragraph::new(Line::from(vec![Span::styled(
            format!(" {}", prompt.message),
            theme::WARNING,
        )]));
        f.render_widget(footer, area);
        return;
    }

    let hints = match app.view {
        crate::state::View::Dashboard => app.dashboard.footer_hints(),
        crate::state::View::Pipelines => app.pipelines.footer_hints(),
        crate::state::View::ActiveRuns => app.active_runs.footer_hints(),
        crate::state::View::BuildHistory => app.build_history.footer_hints(),
        crate::state::View::LogViewer => app.log_viewer.footer_hints(),
        crate::state::View::PullRequestsCreatedByMe
        | crate::state::View::PullRequestsAssignedToMe
        | crate::state::View::PullRequestsAllActive => app.pull_requests.footer_hints(),
        crate::state::View::PullRequestDetail => app.pull_request_detail.footer_hints(),
        crate::state::View::Boards => app.boards.footer_hints(),
        crate::state::View::BoardsAssignedToMe | crate::state::View::BoardsCreatedByMe => {
            app.my_work_items.footer_hints()
        }
    };

    let footer = Paragraph::new(Line::from(vec![Span::styled(hints, theme::MUTED)]));
    f.render_widget(footer, area);
}
