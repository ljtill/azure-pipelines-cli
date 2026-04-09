use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, TimelineRow};

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    use ratatui::layout::{Constraint, Layout};

    let build_label = app
        .selected_build
        .as_ref()
        .map(|b| format!("{} #{}", b.definition.name, b.build_number))
        .unwrap_or_else(|| "Build".to_string());

    let chunks = Layout::vertical([
        Constraint::Length(2), // build info header
        Constraint::Min(0),   // body (tree + log)
    ])
    .split(area);

    // Build info header
    let header = Paragraph::new(Line::from(vec![
        Span::styled(" ← ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            &build_label,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" — Log Viewer", Style::default().fg(Color::DarkGray)),
    ]));
    f.render_widget(header, chunks[0]);

    // Horizontal split: tree (left) + log (right)
    let body = Layout::horizontal([
        Constraint::Percentage(35), // tree panel
        Constraint::Percentage(65), // log panel
    ])
    .split(chunks[1]);

    draw_tree(f, app, body[0]);
    draw_log(f, app, body[1]);
}

fn status_icon(state: Option<&str>, result: Option<&str>) -> (&'static str, Color) {
    match result {
        Some("succeeded") => ("✓", Color::Green),
        Some("failed") => ("✗", Color::Red),
        Some("partiallySucceeded") => ("◐", Color::Yellow),
        Some("canceled") | Some("cancelled") => ("⊘", Color::DarkGray),
        Some("skipped") => ("⊘", Color::DarkGray),
        _ => match state {
            Some("inProgress") => ("⏳", Color::Yellow),
            Some("pending") => ("○", Color::DarkGray),
            Some("completed") => ("✓", Color::Green),
            _ => ("○", Color::DarkGray),
        },
    }
}

fn draw_tree(f: &mut Frame, app: &App, area: Rect) {
    if app.timeline_rows.is_empty() {
        let loading = Paragraph::new(" Loading timeline...")
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Pipeline Stages ")
                    .title_style(Style::default().fg(Color::Cyan)),
            );
        f.render_widget(loading, area);
        return;
    }

    let items: Vec<ListItem> = app
        .timeline_rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let selected = i == app.log_entries_index;
            match row {
                TimelineRow::Stage {
                    name,
                    state,
                    result,
                    collapsed,
                    ..
                } => {
                    let arrow = if *collapsed { "▸" } else { "▾" };
                    let (icon, icon_color) =
                        status_icon(state.as_deref(), result.as_deref());
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{} ", arrow),
                            Style::default().fg(Color::Yellow),
                        ),
                        Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
                        Span::styled(
                            name.as_str(),
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]))
                    .style(if selected {
                        Style::default().bg(Color::DarkGray)
                    } else {
                        Style::default()
                    })
                }
                TimelineRow::Job {
                    name,
                    state,
                    result,
                    collapsed,
                    ..
                } => {
                    let arrow = if *collapsed { "▸" } else { "▾" };
                    let (icon, icon_color) =
                        status_icon(state.as_deref(), result.as_deref());
                    ListItem::new(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(
                            format!("{} ", arrow),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
                        Span::styled(name.as_str(), Style::default().fg(Color::White)),
                    ]))
                    .style(if selected {
                        Style::default().bg(Color::DarkGray)
                    } else {
                        Style::default()
                    })
                }
                TimelineRow::Task {
                    name,
                    state,
                    result,
                    log_id,
                    ..
                } => {
                    let (icon, icon_color) =
                        status_icon(state.as_deref(), result.as_deref());
                    let log_indicator = if log_id.is_some() { "" } else { " ·" };
                    ListItem::new(Line::from(vec![
                        Span::raw("      "),
                        Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
                        Span::styled(name.as_str(), Style::default().fg(Color::White)),
                        Span::styled(log_indicator, Style::default().fg(Color::DarkGray)),
                    ]))
                    .style(if selected {
                        Style::default().bg(Color::DarkGray)
                    } else {
                        Style::default()
                    })
                }
            }
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Pipeline Stages ")
            .title_style(Style::default().fg(Color::Cyan)),
    );

    let mut state = ListState::default();
    state.select(Some(app.log_entries_index));
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_log(f: &mut Frame, app: &App, area: Rect) {
    let title = if app.follow_mode && !app.followed_task_name.is_empty() {
        format!(" Log Output — FOLLOW: {} ", app.followed_task_name)
    } else if !app.follow_mode {
        // Show the name of the pinned task
        if let Some(TimelineRow::Task { name, .. }) =
            app.timeline_rows.get(app.log_entries_index)
        {
            format!(" Log Output — {} ", name)
        } else {
            " Log Output ".to_string()
        }
    } else {
        " Log Output ".to_string()
    };

    if app.log_content.is_empty() {
        let hint = Paragraph::new(" Select a task and press Enter to view its log")
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title),
            );
        f.render_widget(hint, area);
    } else {
        let lines: Vec<Line> = app
            .log_content
            .iter()
            .map(|l| Line::from(Span::raw(l.as_str())))
            .collect();

        let total_lines = lines.len() as u16;
        let visible_height = area.height.saturating_sub(2);
        let max_scroll = total_lines.saturating_sub(visible_height);

        let scroll_offset = if app.log_auto_scroll {
            max_scroll
        } else {
            app.log_scroll_offset.min(max_scroll)
        };

        let title_style = if app.follow_mode {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::Cyan)
        };

        let log = Paragraph::new(Text::from(lines))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .title_style(title_style),
            )
            .wrap(Wrap { trim: false })
            .scroll((scroll_offset, 0));
        f.render_widget(log, area);
    }
}
