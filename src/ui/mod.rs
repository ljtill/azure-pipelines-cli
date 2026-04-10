pub mod helpers;
pub mod setup;
pub mod theme;

use ratatui::Frame;

use crate::app::App;
use crate::components::Component;

pub fn draw(f: &mut Frame, app: &mut App) {
    use ratatui::layout::{Constraint, Layout};

    let chunks = Layout::vertical([
        Constraint::Length(3), // header
        Constraint::Min(0),    // body
        Constraint::Length(1), // footer
    ])
    .split(f.area());

    app.header.draw_with_app(f, app, chunks[0]);

    match app.view {
        crate::app::View::Dashboard => app.dashboard_component.draw_with_app(f, app, chunks[1]),
        crate::app::View::Pipelines => app.pipelines_component.draw_with_app(f, app, chunks[1]),
        crate::app::View::ActiveRuns => app.active_runs_component.draw_with_app(f, app, chunks[1]),
        crate::app::View::BuildHistory => {
            app.build_history_component.draw_with_app(f, app, chunks[1])
        }
        crate::app::View::LogViewer => {
            crate::components::log_viewer::draw_log_viewer(f, app, chunks[1])
        }
    }

    draw_footer(f, app, chunks[2]);

    if app.show_help {
        let _ = app.help.draw(f, f.area());
    }

    if app.show_settings {
        if let Some(ref s) = app.settings {
            app.settings_component.draw_with_state(f, s);
        }
    }
}

fn draw_footer(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    use ratatui::text::{Line, Span};
    use ratatui::widgets::Paragraph;

    // Show confirmation prompt if active
    if let Some(prompt) = &app.confirm_prompt {
        let footer = Paragraph::new(Line::from(vec![Span::styled(
            format!(" {}", prompt.message),
            theme::WARNING,
        )]));
        f.render_widget(footer, area);
        return;
    }

    let hints = match app.view {
        crate::app::View::Dashboard => app.dashboard_component.footer_hints(),
        crate::app::View::Pipelines => app.pipelines_component.footer_hints(),
        crate::app::View::ActiveRuns => app.active_runs_component.footer_hints(),
        crate::app::View::BuildHistory => app.build_history_component.footer_hints(),
        crate::app::View::LogViewer => app.log_viewer_component.footer_hints(),
    };

    let footer = Paragraph::new(Line::from(vec![Span::styled(hints, theme::MUTED)]));
    f.render_widget(footer, area);
}
