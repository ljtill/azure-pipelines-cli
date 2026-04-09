use chrono::Utc;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::app::App;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    use ratatui::layout::{Constraint, Layout};

    let chunks = Layout::vertical([
        Constraint::Length(2), // pipeline name header
        Constraint::Min(0),   // builds list
    ])
    .split(area);

    // Pipeline name header
    let def_name = app
        .selected_definition
        .as_ref()
        .map(|d| d.name.as_str())
        .unwrap_or("Unknown");

    let header = Paragraph::new(Line::from(vec![
        Span::styled(" ← ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            def_name,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" — Build History", Style::default().fg(Color::DarkGray)),
    ]));
    f.render_widget(header, chunks[0]);

    let items: Vec<ListItem> = app
        .definition_builds
        .iter()
        .enumerate()
        .map(|(i, build)| {
            let (icon, icon_color) = match build.status.as_str() {
                "inProgress" | "InProgress" => ("⏳", Color::Yellow),
                _ => match build.result.as_deref() {
                    Some("succeeded") | Some("Succeeded") => ("✓", Color::Green),
                    Some("failed") | Some("Failed") => ("✗", Color::Red),
                    Some("partiallySucceeded") | Some("PartiallySucceeded") => {
                        ("◐", Color::Yellow)
                    }
                    Some("canceled") | Some("Canceled") => ("⊘", Color::DarkGray),
                    _ => ("○", Color::DarkGray),
                },
            };

            let time_info = if build.status == "inProgress" || build.status == "InProgress" {
                if let Some(start) = build.start_time {
                    let dur = Utc::now().signed_duration_since(start);
                    format!("running {}m", dur.num_minutes())
                } else {
                    "queued".to_string()
                }
            } else if let Some(finish) = build.finish_time {
                let ago = Utc::now().signed_duration_since(finish);
                if ago.num_hours() < 1 {
                    format!("{}m ago", ago.num_minutes())
                } else if ago.num_hours() < 24 {
                    format!("{}h ago", ago.num_hours())
                } else {
                    format!("{}d ago", ago.num_days())
                }
            } else {
                String::new()
            };

            ListItem::new(Line::from(vec![
                Span::styled(format!(" {} ", icon), Style::default().fg(icon_color)),
                Span::styled(
                    format!("#{:<14} ", build.build_number),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("{:<26} ", build.short_branch()),
                    Style::default().fg(Color::Blue),
                ),
                Span::styled(
                    format!("{:<20} ", build.requestor()),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(time_info, Style::default().fg(Color::DarkGray)),
            ]))
            .style(if i == app.builds_index {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            })
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::NONE)
            .title(format!(" Builds ({}) ", app.definition_builds.len()))
            .title_style(Style::default().fg(Color::Cyan)),
    );

    let mut state = ListState::default();
    state.select(Some(app.builds_index));
    f.render_stateful_widget(list, chunks[1], &mut state);
}
