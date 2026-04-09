use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

use super::helpers::{checkpoint_status_icon, timeline_status_icon};
use super::theme;
use crate::app::{App, TimelineRow};

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    use ratatui::layout::{Constraint, Layout};

    let build_label = app
        .log_viewer
        .selected_build()
        .map(|b| format!("{} #{}", b.definition.name, b.build_number))
        .unwrap_or_else(|| "Build".to_string());

    let chunks = Layout::vertical([
        Constraint::Length(2), // build info header
        Constraint::Min(0),    // body (tree + log)
    ])
    .split(area);

    // Build info header
    let header = Paragraph::new(Line::from(vec![
        Span::styled(" ← ", theme::MUTED),
        Span::styled(&build_label, theme::BRAND),
        Span::styled(" — Log Viewer", theme::MUTED),
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

fn draw_tree(f: &mut Frame, app: &App, area: Rect) {
    if app.log_viewer.timeline_rows().is_empty() {
        let loading = Paragraph::new(" Loading timeline...")
            .style(theme::MUTED)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Pipeline Stages ")
                    .title_style(theme::TITLE),
            );
        f.render_widget(loading, area);
        return;
    }

    let items: Vec<ListItem> = app
        .log_viewer
        .timeline_rows()
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let selected = i == app.log_viewer.nav().index();
            match row {
                TimelineRow::Stage {
                    name,
                    state,
                    result,
                    collapsed,
                    ..
                } => {
                    let arrow = if *collapsed { "▸" } else { "▾" };
                    let (icon, icon_color) = timeline_status_icon(*state, *result);
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{} ", arrow), theme::ARROW),
                        Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
                        Span::styled(name.as_str(), theme::STAGE),
                    ]))
                    .style(if selected {
                        theme::SELECTED
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
                    let (icon, icon_color) = timeline_status_icon(*state, *result);
                    ListItem::new(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(format!("{} ", arrow), theme::JOB_ARROW),
                        Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
                        Span::styled(name.as_str(), theme::JOB),
                    ]))
                    .style(if selected {
                        theme::SELECTED
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
                    let (icon, icon_color) = timeline_status_icon(*state, *result);
                    let log_indicator = if log_id.is_some() { "" } else { " ·" };
                    ListItem::new(Line::from(vec![
                        Span::raw("      "),
                        Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
                        Span::styled(name.as_str(), theme::JOB),
                        Span::styled(log_indicator, theme::MUTED),
                    ]))
                    .style(if selected {
                        theme::SELECTED
                    } else {
                        Style::default()
                    })
                }
                TimelineRow::Checkpoint {
                    name,
                    state,
                    result,
                    ..
                } => {
                    let (icon, icon_color) = checkpoint_status_icon(*state, *result);
                    ListItem::new(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
                        Span::styled(name.as_str(), Style::default().fg(icon_color)),
                    ]))
                    .style(if selected {
                        theme::SELECTED
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
            .title_style(theme::TITLE),
    );

    let mut state = ListState::default();
    state.select(Some(app.log_viewer.nav().index()));
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_log(f: &mut Frame, app: &App, area: Rect) {
    let title = if app.log_viewer.is_following() && !app.log_viewer.followed_task_name().is_empty()
    {
        format!(
            " Log Output — FOLLOW: {} ",
            app.log_viewer.followed_task_name()
        )
    } else if !app.log_viewer.is_following() {
        // Show the name of the pinned task
        if let Some(TimelineRow::Task { name, .. }) = app
            .log_viewer
            .timeline_rows()
            .get(app.log_viewer.nav().index())
        {
            format!(" Log Output — {} ", name)
        } else {
            " Log Output ".to_string()
        }
    } else {
        " Log Output ".to_string()
    };

    if app.log_viewer.log_content().is_empty() {
        let hint = Paragraph::new(" Select a task and press Enter to view its log")
            .style(theme::MUTED)
            .block(Block::default().borders(Borders::ALL).title(title));
        f.render_widget(hint, area);
    } else {
        let lines: Vec<Line> = app
            .log_viewer
            .log_content()
            .iter()
            .map(|l| Line::from(Span::raw(l.as_str())))
            .collect();

        let total_lines = lines.len() as u16;
        let visible_height = area.height.saturating_sub(2);
        let max_scroll = total_lines.saturating_sub(visible_height);

        let scroll_offset = if app.log_viewer.log_auto_scroll() {
            max_scroll
        } else {
            app.log_viewer.log_scroll_offset().min(max_scroll)
        };

        let title_style = if app.log_viewer.is_following() {
            theme::FOLLOW_TITLE
        } else {
            theme::TITLE
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
