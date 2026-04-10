pub mod active_runs;
pub mod builds;
pub mod dashboard;
pub mod header;
pub mod help;
pub mod helpers;
pub mod logs;
pub mod pipelines;
pub mod settings;
pub mod setup;
pub mod theme;

use ratatui::Frame;

use crate::app::App;

pub fn draw(f: &mut Frame, app: &mut App) {
    use ratatui::layout::{Constraint, Layout};

    let chunks = Layout::vertical([
        Constraint::Length(3), // header
        Constraint::Min(0),    // body
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

    if app.show_settings {
        if let Some(ref s) = app.settings {
            settings::draw(f, s);
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
        crate::app::View::Dashboard => {
            "↑↓ navigate  ←→ collapse/expand  Enter drill-in  Q queue  o open  1/2/3 tabs  r refresh  , settings  ? help  q quit"
        }
        crate::app::View::Pipelines => {
            "↑↓ navigate  →/Enter drill-in  Q queue  o open  / search  1/2/3 tabs  r refresh  , settings  ? help  q/Esc back"
        }
        crate::app::View::ActiveRuns => {
            "↑↓ navigate  Space select  c cancel  / filter  →/Enter view logs  o open  1/2/3 tabs  r refresh  , settings  ? help  q/Esc back"
        }
        crate::app::View::BuildHistory => {
            "↑↓ navigate  →/Enter view logs  ←/q/Esc back  Space select  d delete leases  c cancel  Q queue  o open  r refresh  ? help"
        }
        crate::app::View::LogViewer => {
            "↑↓ navigate  ←→ collapse/expand  Enter inspect  f follow  R retry  A approve  D reject  c cancel  o open  q/Esc back"
        }
    };

    let footer = Paragraph::new(Line::from(vec![Span::styled(hints, theme::MUTED)]));
    f.render_widget(footer, area);
}
