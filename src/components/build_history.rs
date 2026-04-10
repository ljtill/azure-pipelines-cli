use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};

use super::Component;
use crate::app::App;
use crate::ui::helpers::{build_elapsed, effective_status_icon, effective_status_label, truncate};
use crate::ui::theme;

/// Build History component — renders builds for a selected pipeline definition.
pub struct BuildHistory;

impl BuildHistory {
    pub fn draw_with_app(&self, f: &mut Frame, app: &App, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Length(2), // pipeline name header
            Constraint::Min(0),    // builds list
        ])
        .split(area);

        let def_name = app
            .build_history
            .selected_definition
            .as_ref()
            .map(|d| d.name.as_str())
            .unwrap_or("Unknown");

        let header = Paragraph::new(Line::from(vec![
            Span::styled(" ← ", theme::MUTED),
            Span::styled(def_name, theme::BRAND),
            Span::styled(" — Build History", theme::MUTED),
        ]));
        f.render_widget(header, chunks[0]);

        let rects = Layout::horizontal([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Length(12),
            Constraint::Length(18),
            Constraint::Length(2),
            Constraint::Fill(2),
            Constraint::Fill(2),
            Constraint::Length(15),
        ])
        .split(area);
        let mut widths: Vec<usize> = rects.iter().map(|r| r.width as usize).collect();
        widths[5] = widths[5].min(40);
        widths[6] = widths[6].min(35);

        let mut items: Vec<ListItem> = app
            .build_history
            .builds
            .iter()
            .enumerate()
            .map(|(i, build)| {
                let awaiting = app.data.pending_approval_build_ids.contains(&build.id);
                let (icon, icon_color) =
                    effective_status_icon(build.status, build.result, awaiting);
                let label = effective_status_label(build.status, build.result, awaiting);
                let time_info = build_elapsed(build);
                let branch = build.branch_display();
                let retained = app.retention_leases.retained_run_ids.contains(&build.id);
                let selected = app.build_history.selected.contains(&build.id);
                let check = if selected { "✓ " } else { "  " };

                let row_style = if i == app.build_history.nav.index() {
                    theme::SELECTED
                } else {
                    Style::new()
                };

                ListItem::new(Line::from(vec![
                    Span::styled(
                        check,
                        if selected {
                            theme::SUCCESS
                        } else {
                            Style::new()
                        },
                    ),
                    Span::styled(format!(" {} ", icon), Style::new().fg(icon_color)),
                    Span::styled(
                        format!("{:<width$}", label, width = widths[2]),
                        Style::new().fg(icon_color),
                    ),
                    Span::styled(
                        format!(
                            "#{:<width$}",
                            truncate(&build.build_number, widths[3] - 1),
                            width = widths[3] - 1
                        ),
                        theme::TEXT,
                    ),
                    Span::styled(if retained { "◈ " } else { "  " }, theme::WARNING),
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
                            truncate(build.requestor(), widths[6].saturating_sub(1)),
                            width = widths[6].saturating_sub(1)
                        ),
                        theme::MUTED,
                    ),
                    Span::styled(
                        format!("{:>width$}", time_info, width = widths[7]),
                        theme::MUTED,
                    ),
                ]))
                .style(row_style)
            })
            .collect();

        if app.build_history.loading_more {
            items.push(ListItem::new(Line::from(vec![Span::styled(
                "   ⟳ Loading more...",
                theme::MUTED,
            )])));
        } else if app.build_history.has_more {
            items.push(ListItem::new(Line::from(vec![Span::styled(
                "   ▾ ↓ for more",
                theme::MUTED,
            )])));
        }

        let sel_count = app.build_history.selected.len();
        let total = app.build_history.builds.len();
        let title = if sel_count > 0 {
            format!(" Builds ({}) — {} selected ", total, sel_count)
        } else {
            format!(" Builds ({}) ", total)
        };
        let list = List::new(items).block(Block::new().title(title).title_style(theme::TITLE));

        let mut state = ListState::default();
        state.select(Some(app.build_history.nav.index()));
        f.render_stateful_widget(list, chunks[1], &mut state);
    }
}

impl Component for BuildHistory {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn footer_hints(&self) -> &str {
        "↑↓ navigate  →/Enter view logs  ←/q/Esc back  Space select  d delete leases  c cancel  Q queue  o open  r refresh  ? help"
    }
}
