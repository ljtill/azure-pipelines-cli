use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    use ratatui::layout::{Constraint, Layout};

    let build_label = app
        .selected_build
        .as_ref()
        .map(|b| format!("{} #{}", b.definition.name, b.build_number))
        .unwrap_or_else(|| "Build".to_string());

    let chunks = Layout::vertical([
        Constraint::Length(2),  // build info header
        Constraint::Length(12), // timeline entries
        Constraint::Min(0),    // log content
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

    // Timeline entries (steps with logs)
    if let Some(timeline) = &app.build_timeline {
        let log_records: Vec<_> = timeline
            .records
            .iter()
            .filter(|r| r.log.is_some())
            .collect();

        let items: Vec<ListItem> = log_records
            .iter()
            .enumerate()
            .map(|(i, record)| {
                let (icon, icon_color) = match record.result.as_deref() {
                    Some("succeeded") => ("✓", Color::Green),
                    Some("failed") => ("✗", Color::Red),
                    Some("skipped") => ("⊘", Color::DarkGray),
                    _ => match record.state.as_deref() {
                        Some("inProgress") => ("⏳", Color::Yellow),
                        Some("pending") => ("○", Color::DarkGray),
                        _ => ("○", Color::DarkGray),
                    },
                };

                let type_tag = match record.record_type.as_str() {
                    "Stage" => "[S]",
                    "Job" => "[J]",
                    "Task" => "[T]",
                    _ => "   ",
                };

                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {} ", icon), Style::default().fg(icon_color)),
                    Span::styled(
                        format!("{} ", type_tag),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(&record.name, Style::default().fg(Color::White)),
                ]))
                .style(if i == app.log_entries_index {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                })
            })
            .collect();

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Steps — Enter to view log ")
                .title_style(Style::default().fg(Color::Cyan)),
        );

        let mut state = ListState::default();
        state.select(Some(app.log_entries_index));
        f.render_stateful_widget(list, chunks[1], &mut state);
    } else {
        let loading = Paragraph::new(" Loading timeline...")
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title(" Steps "));
        f.render_widget(loading, chunks[1]);
    }

    // Log content
    if app.log_content.is_empty() {
        let hint = Paragraph::new(" Select a step and press Enter to view its log")
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Log Output "),
            );
        f.render_widget(hint, chunks[2]);
    } else {
        let lines: Vec<Line> = app
            .log_content
            .iter()
            .map(|l| Line::from(Span::raw(l.as_str())))
            .collect();

        let total_lines = lines.len() as u16;
        let visible_height = chunks[2].height.saturating_sub(2); // borders
        let max_scroll = total_lines.saturating_sub(visible_height);

        let scroll_offset = if app.log_auto_scroll {
            max_scroll
        } else {
            app.log_scroll_offset.min(max_scroll)
        };

        let log = Paragraph::new(Text::from(lines))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Log Output ")
                    .title_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: false })
            .scroll((scroll_offset, 0));
        f.render_widget(log, chunks[2]);
    }
}
