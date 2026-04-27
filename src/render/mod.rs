//! Top-level render dispatch for all application views.

pub mod columns;
pub mod helpers;
pub mod setup;
pub mod table;
pub mod theme;

use ratatui::Frame;

use crate::components::Component;
use crate::state::App;

/// Defines the minimum terminal width supported by the full dashboard UI.
pub const MIN_TERMINAL_WIDTH: u16 = 80;
/// Defines the minimum terminal height supported by the full dashboard UI.
pub const MIN_TERMINAL_HEIGHT: u16 = 20;

pub fn draw(f: &mut Frame, app: &mut App) {
    use ratatui::layout::{Constraint, Layout};

    let area = f.area();
    if is_terminal_too_small(area) {
        draw_terminal_too_small(f, area);
        return;
    }

    let chunks = Layout::vertical([
        Constraint::Length(3), // Header.
        Constraint::Min(0),    // Body.
        Constraint::Length(1), // Footer.
    ])
    .split(area);

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
        crate::state::View::WorkItemDetail => {
            app.work_item_detail.draw_with_app(f, app, chunks[1]);
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

pub(crate) fn is_terminal_too_small(area: ratatui::layout::Rect) -> bool {
    area.width < MIN_TERMINAL_WIDTH || area.height < MIN_TERMINAL_HEIGHT
}

pub(crate) fn draw_terminal_too_small(f: &mut Frame, area: ratatui::layout::Rect) {
    use ratatui::layout::Alignment;
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Paragraph, Wrap};

    let message = vec![
        Line::from(Span::styled("Terminal too small", theme::WARNING)),
        Line::from(""),
        Line::from(format!(
            "Resize to at least {MIN_TERMINAL_WIDTH}x{MIN_TERMINAL_HEIGHT}."
        )),
        Line::from(format!("Current size: {}x{}.", area.width, area.height)),
    ];
    let paragraph = Paragraph::new(message)
        .block(
            Block::bordered()
                .title(" devops ")
                .title_style(theme::BRAND),
        )
        .style(theme::TEXT)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(paragraph, area);
}

fn draw_footer(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    use ratatui::text::{Line, Span};
    use ratatui::widgets::Paragraph;

    // Show confirmation prompt if active.
    if let Some(prompt) = &app.confirm_prompt {
        let footer = Paragraph::new(Line::from(vec![
            Span::styled("confirm: ", theme::WARNING),
            Span::styled(&prompt.message, theme::TEXT),
        ]));
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
        crate::state::View::WorkItemDetail => app.work_item_detail.footer_hints(),
    };

    let footer = Paragraph::new(Line::from(vec![
        Span::styled("actions: ", theme::MUTED),
        Span::styled(hints, theme::SUBTLE),
    ]));
    f.render_widget(footer, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    fn buffer_to_string(buf: &Buffer) -> String {
        let mut out = String::new();
        for y in 0..buf.area.height {
            let mut line = String::new();
            for x in 0..buf.area.width {
                line.push_str(buf[(x, y)].symbol());
            }
            out.push_str(line.trim_end_matches(' '));
            out.push('\n');
        }
        out
    }

    #[test]
    fn draw_shows_terminal_size_fallback_when_too_small() {
        let mut terminal = Terminal::new(TestBackend::new(
            MIN_TERMINAL_WIDTH - 1,
            MIN_TERMINAL_HEIGHT,
        ))
        .unwrap();
        let mut app = crate::test_helpers::make_app();

        terminal.draw(|f| draw(f, &mut app)).unwrap();

        let rendered = buffer_to_string(terminal.backend().buffer());
        assert!(rendered.contains("Terminal too small"));
        assert!(rendered.contains("Resize to at least 80x20."));
        assert!(rendered.contains("Current size: 79x20."));
    }

    #[test]
    fn draw_uses_full_ui_at_minimum_size() {
        let mut terminal =
            Terminal::new(TestBackend::new(MIN_TERMINAL_WIDTH, MIN_TERMINAL_HEIGHT)).unwrap();
        let mut app = crate::test_helpers::make_app();

        terminal.draw(|f| draw(f, &mut app)).unwrap();

        let rendered = buffer_to_string(terminal.backend().buffer());
        assert!(!rendered.contains("Terminal too small"));
        assert!(rendered.contains("devops"));
    }
}
