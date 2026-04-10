use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState};

use super::Component;
use crate::app::{App, DashboardRow};
use crate::ui::helpers::{build_elapsed, effective_status_icon, effective_status_label, truncate};
use crate::ui::theme;

/// Dashboard component — renders pipelines grouped by folder with collapse/expand.
pub struct Dashboard;

impl Dashboard {
    pub fn draw_with_app(&self, f: &mut Frame, app: &App, area: Rect) {
        let rects = Layout::horizontal([
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Length(12),
            Constraint::Fill(2),
            Constraint::Length(18),
            Constraint::Fill(2),
            Constraint::Fill(2),
            Constraint::Length(15),
        ])
        .split(area);
        let mut widths: Vec<usize> = rects.iter().map(|r| r.width as usize).collect();
        widths[3] = widths[3].min(40);
        widths[5] = widths[5].min(35);
        widths[6] = widths[6].min(35);

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
                        Style::new()
                    })
                }
                DashboardRow::Pipeline {
                    definition,
                    latest_build,
                } => {
                    let row_style = if i == app.dashboard.nav.index() {
                        theme::SELECTED
                    } else {
                        Style::new()
                    };

                    let (icon, icon_color) = match latest_build {
                        Some(b) => {
                            let awaiting = app.data.pending_approval_build_ids.contains(&b.id);
                            effective_status_icon(b.status, b.result, awaiting)
                        }
                        None => ("○", Color::DarkGray),
                    };
                    let label = match latest_build {
                        Some(b) => {
                            let awaiting = app.data.pending_approval_build_ids.contains(&b.id);
                            effective_status_label(b.status, b.result, awaiting)
                        }
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
                            Span::styled(
                                format!("{:>width$}", elapsed, width = widths[7]),
                                theme::MUTED,
                            ),
                        ]
                    } else {
                        vec![Span::styled("no builds", theme::MUTED)]
                    };

                    let name_style = if latest_build.is_some() {
                        theme::TEXT
                    } else {
                        theme::MUTED
                    };

                    let mut spans = vec![
                        Span::raw("    "),
                        Span::styled(format!("{} ", icon), Style::new().fg(icon_color)),
                        Span::styled(
                            format!("{:<width$}", label, width = widths[2]),
                            Style::new().fg(icon_color),
                        ),
                        Span::styled(
                            format!(
                                "{:<width$} ",
                                truncate(&definition.name, widths[3].saturating_sub(1)),
                                width = widths[3].saturating_sub(1)
                            ),
                            name_style,
                        ),
                    ];

                    spans.extend(build_spans);

                    ListItem::new(Line::from(spans)).style(row_style)
                }
            })
            .collect();

        let list = List::new(items).block(
            Block::new()
                .title(" Dashboard — Pipelines by Folder ")
                .title_style(theme::TITLE),
        );

        let mut state = ListState::default();
        state.select(Some(app.dashboard.nav.index()));
        f.render_stateful_widget(list, area, &mut state);
    }
}

impl Component for Dashboard {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn footer_hints(&self) -> &str {
        "↑↓ navigate  ←→ collapse/expand  Enter drill-in  Q queue  o open  1/2/3 tabs  r refresh  , settings  ? help  q quit"
    }
}
