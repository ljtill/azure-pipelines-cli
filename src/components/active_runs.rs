use std::collections::HashSet;

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState};

use super::Component;
use crate::client::models::Build;
use crate::render::helpers::{
    build_elapsed, effective_status_icon, effective_status_label, row_style, split_with_search_bar,
    truncate,
};
use crate::render::theme;
use crate::state::nav::ListNav;
use crate::state::{App, InputMode};

/// Active Runs component — renders currently running builds with multi-select.
#[derive(Debug, Default)]
pub struct ActiveRuns {
    pub filtered: Vec<Build>,
    pub nav: ListNav,
    pub selected: HashSet<u32>,
}

impl ActiveRuns {
    pub fn rebuild(
        &mut self,
        active_builds: &[Build],
        filter_definition_ids: &[u32],
        search_query: &str,
    ) {
        let base = active_builds.iter().filter(|b| {
            if !filter_definition_ids.is_empty()
                && !filter_definition_ids.contains(&b.definition.id)
            {
                return false;
            }
            true
        });

        if search_query.is_empty() {
            self.filtered = base.cloned().collect();
        } else {
            let q = search_query.to_lowercase();
            self.filtered = base
                .filter(|b| {
                    b.definition.name.to_lowercase().contains(&q)
                        || b.build_number.to_lowercase().contains(&q)
                        || b.branch_display().to_lowercase().contains(&q)
                })
                .cloned()
                .collect();
        }
        self.nav.set_len(self.filtered.len());
    }

    /// Toggle selection state for the item at the current nav index.
    pub fn toggle_selected_at_cursor(&mut self) {
        if let Some(build) = self.filtered.get(self.nav.index()) {
            let id = build.id;
            if !self.selected.remove(&id) {
                self.selected.insert(id);
            }
        }
    }

    pub fn draw_with_app(&self, f: &mut Frame, app: &App, area: Rect) {
        let show_search = app.view == crate::state::View::ActiveRuns
            && (app.search.mode == InputMode::Search || !app.search.query.is_empty());
        let list_area =
            split_with_search_bar(f, area, &app.search.query, app.search.mode, show_search);

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

        let items: Vec<ListItem> = self
            .filtered
            .iter()
            .enumerate()
            .map(|(i, build)| {
                let elapsed = build_elapsed(build);
                let selected = self.selected.contains(&build.id);
                let retained = app.retention_leases.retained_run_ids.contains(&build.id);
                let check = if selected { "✓ " } else { "  " };
                let awaiting = app.data.pending_approval_build_ids.contains(&build.id);
                let (icon, icon_color) =
                    effective_status_icon(build.status, build.result, awaiting);
                let label = effective_status_label(build.status, build.result, awaiting);

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
                .style(row_style(i == self.nav.index()))
            })
            .collect();

        let sel_count = self.selected.len();
        let filtered = self.filtered.len();
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
        state.select(Some(self.nav.index()));
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
