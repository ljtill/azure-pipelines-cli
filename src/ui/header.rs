use chrono::Utc;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};

use super::helpers::truncate;
use crate::app::{App, View};

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    use ratatui::layout::{Constraint, Layout};

    let chunks = Layout::vertical([
        Constraint::Length(1), // title + status
        Constraint::Length(2), // tabs
    ])
    .split(area);

    // Title bar with org/project and refresh status
    let refresh_text = if app.loading {
        " ⟳ refreshing...".to_string()
    } else if let Some(last) = app.last_refresh {
        let elapsed = Utc::now().signed_duration_since(last);
        if elapsed.num_seconds() < 60 {
            format!(" ⟳ {}s ago", elapsed.num_seconds())
        } else {
            format!(" ⟳ {}m ago", elapsed.num_minutes())
        }
    } else {
        " ⟳ --".to_string()
    };

    let error_span = if let Some(err) = &app.error_message {
        Span::styled(
            format!("  ⚠ {}", truncate(err, 60)),
            Style::default().fg(Color::Red),
        )
    } else {
        Span::raw("")
    };

    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            " azure-pipelines-cli",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ●  ", Style::default().fg(Color::DarkGray)),
        Span::styled(&app.org_project_label, Style::default().fg(Color::White)),
        Span::styled(refresh_text, Style::default().fg(Color::DarkGray)),
        error_span,
    ]));
    f.render_widget(title, chunks[0]);

    // Tab bar
    let tab_titles = vec!["[1] Dashboard", "[2] Pipelines", "[3] Active Runs"];
    let selected = match app.view {
        View::Dashboard => 0,
        View::Pipelines => 1,
        View::ActiveRuns => 2,
        View::BuildHistory | View::LogViewer => {
            // Highlight whichever tab we drilled in from
            match app.previous_view {
                Some(View::Pipelines) => 1,
                Some(View::ActiveRuns) => 2,
                _ => 0,
            }
        }
    };

    let tabs = Tabs::new(tab_titles)
        .block(Block::default().borders(Borders::BOTTOM))
        .select(selected)
        .style(Style::default().fg(Color::DarkGray))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(tabs, chunks[1]);
}
