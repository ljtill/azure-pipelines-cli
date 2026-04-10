use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState};

use super::Component;
use crate::app::{App, InputMode};
use crate::ui::helpers::{
    build_elapsed, draw_search_bar, effective_status_icon, effective_status_label, truncate,
};
use crate::ui::theme;

/// Active Runs component — renders currently running builds with multi-select.
pub struct ActiveRuns;

impl ActiveRuns {
    pub fn draw_with_app(&self, f: &mut Frame, app: &App, area: Rect) {
        let show_search = app.view == crate::app::View::ActiveRuns
            && (app.search.mode == InputMode::Search || !app.search.query.is_empty());

        let chunks = if show_search {
            Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area)
        } else {
            Layout::vertical([Constraint::Min(0)]).split(area)
        };

        let list_area = if show_search { chunks[1] } else { chunks[0] };

        if show_search {
            draw_search_bar(f, chunks[0], &app.search.query, app.search.mode);
        }

        let rects = Layout::horizontal([
            Constraint::Length(2),
            Constraint::Length(4),
            Constraint::Length(12),
            Constraint::Fill(2),
            Constraint::Length(18),
            Constraint::Length(2),
            Constraint::Fill(2),
            Constraint::Fill(2),
            Constraint::Length(15),
        ])
        .split(area);
        let mut widths: Vec<usize> = rects.iter().map(|r| r.width as usize).collect();
        widths[3] = widths[3].min(40);
        widths[6] = widths[6].min(35);
        widths[7] = widths[7].min(35);

        let items: Vec<ListItem> = app
            .active_runs
            .filtered
            .iter()
            .enumerate()
            .map(|(i, build)| {
                let elapsed = build_elapsed(build);
                let selected = app.active_runs.selected.contains(&build.id);
                let retained = app.retention_leases.retained_run_ids.contains(&build.id);
                let check = if selected { "✓ " } else { "  " };
                let (icon, icon_color) = {
                    let awaiting = app.data.pending_approval_build_ids.contains(&build.id);
                    effective_status_icon(build.status, build.result, awaiting)
                };
                let label = {
                    let awaiting = app.data.pending_approval_build_ids.contains(&build.id);
                    effective_status_label(build.status, build.result, awaiting)
                };

                let row_style = if i == app.active_runs.nav.index() {
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
                            "{:<width$} ",
                            truncate(&build.definition.name, widths[3].saturating_sub(1)),
                            width = widths[3].saturating_sub(1)
                        ),
                        theme::TEXT,
                    ),
                    Span::styled(
                        format!(
                            "#{:<width$}",
                            truncate(&build.build_number, widths[4] - 1),
                            width = widths[4] - 1
                        ),
                        theme::MUTED,
                    ),
                    Span::styled(if retained { "◈ " } else { "  " }, theme::WARNING),
                    Span::styled(
                        format!(
                            "{:<width$} ",
                            truncate(&build.branch_display(), widths[6].saturating_sub(1)),
                            width = widths[6].saturating_sub(1)
                        ),
                        theme::BRANCH,
                    ),
                    Span::styled(
                        format!(
                            "{:<width$} ",
                            truncate(build.requestor(), widths[7].saturating_sub(1)),
                            width = widths[7].saturating_sub(1)
                        ),
                        theme::MUTED,
                    ),
                    Span::styled(
                        format!("{:>width$}", elapsed, width = widths[8]),
                        theme::WARNING,
                    ),
                ]))
                .style(row_style)
            })
            .collect();

        let sel_count = app.active_runs.selected.len();
        let filtered = app.active_runs.filtered.len();
        let total = app.data.active_builds.len();
        let title = if sel_count > 0 {
            format!(
                " Active Runs ({} / {}) — {} selected ",
                filtered, total, sel_count
            )
        } else if filtered != total {
            format!(" Active Runs ({} / {}) ", filtered, total)
        } else {
            format!(" Active Runs ({}) ", total)
        };
        let list = List::new(items).block(Block::new().title(title).title_style(theme::TITLE));

        let mut state = ListState::default();
        state.select(Some(app.active_runs.nav.index()));
        f.render_stateful_widget(list, list_area, &mut state);
    }
}

impl Component for ActiveRuns {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn footer_hints(&self) -> &str {
        "↑↓ navigate  Space select  c cancel  / filter  →/Enter view logs  o open  1/2/3 tabs  r refresh  , settings  ? help  q/Esc back"
    }
}
