use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use super::helpers::{Column, build_elapsed, compute_columns, status_icon, status_label, truncate};
use super::theme;
use crate::app::{App, DashboardRow};

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    // Column layout for pipeline rows:
    // indent(4) | icon(3) | status(12) | name(flex) | build_number(16) | branch(flex) | requestor(flex) | elapsed(12)
    let col_spec = [
        Column::Fixed(4),  // indent
        Column::Fixed(3),  // icon
        Column::Fixed(12), // status label
        Column::Flex {
            weight: 3,
            min: 10,
            max: 40,
        }, // pipeline name
        Column::Fixed(16), // build number
        Column::Flex {
            weight: 2,
            min: 10,
            max: 30,
        }, // branch
        Column::Flex {
            weight: 2,
            min: 10,
            max: 30,
        }, // requestor
        Column::Fixed(16), // elapsed
    ];
    let widths = compute_columns(&col_spec, area.width as usize);

    let items: Vec<ListItem> = app
        .dashboard
        .rows
        .iter()
        .enumerate()
        .map(|(i, row)| match row {
            DashboardRow::FolderHeader { path, collapsed } => {
                let icon = if *collapsed { "▸" } else { "▾" };
                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {} ", icon), theme::ARROW),
                    Span::styled(path.to_string(), theme::FOLDER),
                ]))
                .style(if i == app.dashboard.nav.index() {
                    theme::SELECTED
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
                let label = match latest_build {
                    Some(b) => status_label(b.status, b.result),
                    None => "",
                };

                let build_spans = if let Some(b) = latest_build {
                    let branch = b.branch_display();
                    let elapsed = build_elapsed(b);
                    vec![
                        Span::styled(
                            format!(
                                "#{:<width$}",
                                truncate(&b.build_number, widths[4] - 1),
                                width = widths[4] - 1
                            ),
                            theme::MUTED,
                        ),
                        Span::styled(
                            format!(
                                "{:<width$} ",
                                truncate(&branch, widths[5].saturating_sub(1)),
                                width = widths[5].saturating_sub(1)
                            ),
                            theme::BRANCH,
                        ),
                        Span::styled(
                            format!(
                                "{:<width$} ",
                                truncate(b.requestor(), widths[6].saturating_sub(1)),
                                width = widths[6].saturating_sub(1)
                            ),
                            theme::MUTED,
                        ),
                        Span::styled(elapsed, theme::MUTED),
                    ]
                } else {
                    vec![Span::styled("no builds", theme::MUTED)]
                };

                let mut spans = vec![
                    Span::raw("    "),
                    Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
                    Span::styled(
                        format!("{:<width$}", label, width = widths[2]),
                        Style::default().fg(icon_color),
                    ),
                    Span::styled(
                        format!(
                            "{:<width$} ",
                            truncate(&definition.name, widths[3].saturating_sub(1)),
                            width = widths[3].saturating_sub(1)
                        ),
                        theme::TEXT,
                    ),
                ];
                spans.extend(build_spans);

                ListItem::new(Line::from(spans)).style(if i == app.dashboard.nav.index() {
                    theme::SELECTED
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
            .title_style(theme::TITLE),
    );

    let mut state = ListState::default();
    state.select(Some(app.dashboard.nav.index()));
    f.render_stateful_widget(list, area, &mut state);
}
