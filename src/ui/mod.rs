pub mod active_runs;
pub mod builds;
pub mod dashboard;
pub mod header;
pub mod help;
pub mod logs;
pub mod pipelines;

use ratatui::Frame;

use crate::app::App;

pub fn draw(f: &mut Frame, app: &App) {
    use ratatui::layout::{Constraint, Layout};

    let chunks = Layout::vertical([
        Constraint::Length(3), // header
        Constraint::Min(0),   // body
        Constraint::Length(1), // footer
    ])
    .split(f.area());

    header::draw(f, app, chunks[0]);

    match app.view {
        crate::app::View::Dashboard => dashboard::draw(f, app, chunks[1]),
        crate::app::View::Pipelines => pipelines::draw(f, app, chunks[1]),
        crate::app::View::ActiveRuns => active_runs::draw(f, app, chunks[1]),
        crate::app::View::BuildHistory => builds::draw(f, app, chunks[1]),
        crate::app::View::LogViewer => logs::draw(f, app, chunks[1]),
    }

    draw_footer(f, app, chunks[2]);

    if app.show_help {
        help::draw(f);
    }
}

fn draw_footer(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    use ratatui::style::{Color, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::Paragraph;

    let hints = match app.view {
        crate::app::View::Dashboard => {
            "↑↓ navigate  ←→ collapse/expand  Enter drill-in  1/2/3 tabs  r refresh  ? help  q quit"
        }
        crate::app::View::Pipelines => {
            "↑↓ navigate  Enter drill-in  / search  1/2/3 tabs  r refresh  ? help  q quit"
        }
        crate::app::View::ActiveRuns => {
            "↑↓ navigate  Enter view logs  1/2/3 tabs  r refresh  ? help  q quit"
        }
        crate::app::View::BuildHistory => {
            "↑↓ navigate  Enter view logs  Esc back  r refresh  ? help  q quit"
        }
        crate::app::View::LogViewer => {
            "↑↓ navigate  ←→ collapse/expand  Enter inspect  f follow  PgUp/PgDn scroll  Esc back  q quit"
        }
    };

    let footer = Paragraph::new(Line::from(vec![Span::styled(
        hints,
        Style::default().fg(Color::DarkGray),
    )]));
    f.render_widget(footer, area);
}
