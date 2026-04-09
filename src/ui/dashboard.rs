use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use super::helpers::{build_elapsed, status_icon, truncate};
use crate::app::{App, DashboardRow};

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .dashboard_rows
        .iter()
        .enumerate()
        .map(|(i, row)| match row {
            DashboardRow::FolderHeader { path, collapsed } => {
                let icon = if *collapsed { "▸" } else { "▾" };
                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {} ", icon), Style::default().fg(Color::Yellow)),
                    Span::styled(
                        path.to_string(),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]))
                .style(if i == app.dashboard_nav.index() {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                })
            }
            DashboardRow::Pipeline {
                definition,
                latest_build,
            } => {
                let (icon, icon_color) = match latest_build {
                    Some(b) => status_icon(b.status, b.result),
                    None => ("○", Color::DarkGray),
                };

                let build_info = if let Some(b) = latest_build {
                    let branch = b.short_branch();
                    let branch_display = truncate(&branch, 25);
                    let elapsed = build_elapsed(b);
                    format!(
                        "#{:<14} {:<26} {:<20} {}",
                        b.build_number,
                        branch_display,
                        b.requestor(),
                        elapsed
                    )
                } else {
                    "no builds".to_string()
                };

                ListItem::new(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
                    Span::styled(
                        format!("{:<40} ", definition.name),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(build_info, Style::default().fg(Color::DarkGray)),
                ]))
                .style(if i == app.dashboard_nav.index() {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                })
            }
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::NONE)
            .title(" Dashboard — Pipelines by Folder ")
            .title_style(Style::default().fg(Color::Cyan)),
    );

    let mut state = ListState::default();
    state.select(Some(app.dashboard_nav.index()));
    f.render_stateful_widget(list, area, &mut state);
}
