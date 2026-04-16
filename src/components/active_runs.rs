//! Active runs view component showing currently executing builds.

use std::collections::HashSet;

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};

use super::Component;
use crate::client::models::Build;
use crate::render::columns::{BuildRowOpts, build_row};
use crate::render::helpers::{
    build_elapsed, draw_state_message, draw_view_frame, effective_status_icon,
    effective_status_label, row_style, split_with_search_bar, truncate,
};
use crate::render::table::resolve_widths;
use crate::render::theme;
use crate::state::nav::ListNav;
use crate::state::{App, InputMode};

/// Renders currently running builds with multi-select support.
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

    /// Toggles selection state for the item at the current nav index.
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
        let sel_count = self.selected.len();
        let filtered = self.filtered.len();
        let total = app.data.active_builds.len();
        let mut subtitle_spans = crate::render::helpers::sub_view_tab_spans(app.service, app.view);
        if !subtitle_spans.is_empty() {
            subtitle_spans.push(Span::styled("  ·  ", theme::MUTED));
        }
        subtitle_spans.push(Span::styled(format!("{filtered} shown"), theme::TEXT));
        subtitle_spans.push(Span::styled(format!("  ·  {total} total"), theme::MUTED));
        subtitle_spans.push(Span::styled(
            format!("  ·  {sel_count} selected"),
            if sel_count > 0 {
                theme::SUCCESS
            } else {
                theme::MUTED
            },
        ));
        let subtitle = Line::from(subtitle_spans);
        let frame_area = draw_view_frame(f, area, " Active Runs ", Some(subtitle));
        let list_area = split_with_search_bar(
            f,
            frame_area,
            &app.search.query,
            app.search.mode,
            show_search,
        );

        if self.filtered.is_empty() {
            let hint = if show_search {
                " No active runs match the current search"
            } else {
                " No active runs found"
            };
            draw_state_message(f, list_area, hint, theme::MUTED);
            return;
        }

        let schema = build_row(BuildRowOpts {
            select: true,
            name: true,
            retained: true,
        });
        let resolved = resolve_widths(&schema.columns, list_area.width);
        let widths: Vec<usize> = resolved.iter().map(|&w| w as usize).collect();

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
                    Span::styled(format!(" {icon} "), Style::new().fg(icon_color)),
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
        let list = List::new(items);

        let mut state = ListState::default();
        state.select(Some(self.nav.index()));
        f.render_stateful_widget(list, list_area, &mut state);
    }
}

impl Component for ActiveRuns {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn footer_hints(&self) -> &'static str {
        "↑↓ navigate  Space select  c cancel  / filter  →/Enter view logs  o open  1–4 areas  r refresh  , settings  ? help  q/Esc back"
    }
}
