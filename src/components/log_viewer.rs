use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Wrap};

use super::Component;
use crate::app::{App, TimelineRow};
use crate::ui::helpers::{checkpoint_status_icon, timeline_status_icon};
use crate::ui::theme;

/// Log Viewer component — renders timeline tree + log output for a selected build.
pub struct LogViewer;

/// Draw the log viewer. This is a free function rather than a method on `LogViewer`
/// because it needs `&mut App` (for `set_layout_areas` mouse hit-testing state)
/// while the component is itself a field of `App`.
pub fn draw_log_viewer(f: &mut Frame, app: &mut App, area: Rect) {
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

    let header = Paragraph::new(Line::from(vec![
        Span::styled(" ← ", theme::MUTED),
        Span::styled(&build_label, theme::BRAND),
        Span::styled(" — Log Viewer", theme::MUTED),
    ]));
    f.render_widget(header, chunks[0]);

    let body = Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(chunks[1]);

    // Store layout areas for mouse hit-testing.
    app.log_viewer.set_layout_areas(body[0], body[1]);

    draw_tree(f, app, body[0]);
    draw_log(f, app, body[1]);
}

impl Component for LogViewer {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn footer_hints(&self) -> &str {
        "↑↓ navigate  ←→ collapse/expand  Enter inspect  f follow  R retry  A approve  D reject  c cancel  o open  q/Esc back"
    }
}

fn draw_tree(f: &mut Frame, app: &App, area: Rect) {
    if app.log_viewer.timeline_rows().is_empty() {
        let loading = Paragraph::new(" Loading timeline...")
            .style(theme::MUTED)
            .block(
                Block::bordered()
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
                        Span::styled(format!("{} ", icon), Style::new().fg(icon_color)),
                        Span::styled(name.as_str(), theme::STAGE),
                    ]))
                    .style(if selected {
                        theme::SELECTED
                    } else {
                        Style::new()
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
                        Span::styled(format!("{} ", icon), Style::new().fg(icon_color)),
                        Span::styled(name.as_str(), theme::JOB),
                    ]))
                    .style(if selected {
                        theme::SELECTED
                    } else {
                        Style::new()
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
                        Span::styled(format!("{} ", icon), Style::new().fg(icon_color)),
                        Span::styled(name.as_str(), theme::JOB),
                        Span::styled(log_indicator, theme::MUTED),
                    ]))
                    .style(if selected {
                        theme::SELECTED
                    } else {
                        Style::new()
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
                        Span::styled(format!("{} ", icon), Style::new().fg(icon_color)),
                        Span::styled(name.as_str(), Style::new().fg(icon_color)),
                    ]))
                    .style(if selected {
                        theme::SELECTED
                    } else {
                        Style::new()
                    })
                }
            }
        })
        .collect();

    let list = List::new(items).block(
        Block::bordered()
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
            .block(Block::bordered().title(title));
        f.render_widget(hint, area);
    } else {
        let lines: Vec<Line> = app
            .log_viewer
            .log_content()
            .iter()
            .map(|l| Line::from(Span::raw(l.as_str())))
            .collect();

        let total_lines = lines.len() as u32;
        let visible_height = area.height.saturating_sub(2) as u32;
        let max_scroll = total_lines.saturating_sub(visible_height);

        let scroll_offset_u32 = if app.log_viewer.log_auto_scroll() {
            max_scroll
        } else {
            app.log_viewer.log_scroll_offset().min(max_scroll)
        };
        let scroll_offset = scroll_offset_u32.min(u16::MAX as u32) as u16;

        let title_style = if app.log_viewer.is_following() {
            theme::FOLLOW_TITLE
        } else {
            theme::TITLE
        };

        let log = Paragraph::new(Text::from(lines))
            .block(Block::bordered().title(title).title_style(title_style))
            .wrap(Wrap { trim: false })
            .scroll((scroll_offset, 0));
        f.render_widget(log, area);
    }
}
