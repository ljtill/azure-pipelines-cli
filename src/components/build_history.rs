//! Build history view component displaying past build results.

use std::collections::HashSet;

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};

use super::Component;
use crate::client::models::{Build, PipelineDefinition};
use crate::render::helpers::{
    build_elapsed, effective_status_icon, effective_status_label, truncate,
};
use crate::render::theme;
use crate::state::nav::ListNav;
use crate::state::{App, View};

/// Renders builds for a selected pipeline definition.
#[derive(Debug, Default)]
pub struct BuildHistory {
    pub selected_definition: Option<PipelineDefinition>,
    pub builds: Vec<Build>,
    pub nav: ListNav,
    /// Holds build IDs selected for batch lease deletion.
    pub selected: HashSet<u32>,
    /// Stores the view to return to when pressing Esc/back from Build History.
    pub return_to: Option<View>,
    /// Indicates whether more builds may be available beyond what's loaded.
    pub has_more: bool,
    /// Indicates whether a "load more" request is currently in flight.
    pub loading_more: bool,
    /// Holds the ADO continuation token for fetching the next page.
    pub continuation_token: Option<String>,
    /// Stores a stashed nav index to restore after a refresh (e.g. post-lease-deletion).
    pub pending_nav_index: Option<usize>,
}

impl BuildHistory {
    /// Toggles selection state for the build at the current nav index.
    pub fn toggle_selected_at_cursor(&mut self) {
        if let Some(build) = self.builds.get(self.nav.index()) {
            let id = build.id;
            if !self.selected.remove(&id) {
                self.selected.insert(id);
            }
        }
    }

    pub fn draw_with_app(&self, f: &mut Frame, app: &App, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Length(2), // Pipeline name header.
            Constraint::Min(0),    // Builds list.
        ])
        .split(area);

        let def_name = self
            .selected_definition
            .as_ref()
            .map_or("Unknown", |d| d.name.as_str());

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

        let mut items: Vec<ListItem> = self
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
                let selected = self.selected.contains(&build.id);
                let check = if selected { "✓ " } else { "  " };

                let row_style = if i == self.nav.index() {
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
                    Span::styled(format!(" {icon} "), Style::new().fg(icon_color)),
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

        if self.loading_more {
            items.push(ListItem::new(Line::from(vec![Span::styled(
                "   ⟳ Loading more...",
                theme::MUTED,
            )])));
        } else if self.has_more {
            items.push(ListItem::new(Line::from(vec![Span::styled(
                "   ▾ ↓ for more",
                theme::MUTED,
            )])));
        }

        let sel_count = self.selected.len();
        let total = self.builds.len();
        let title = if sel_count > 0 {
            format!(" Builds ({total}) — {sel_count} selected ")
        } else {
            format!(" Builds ({total}) ")
        };
        let list = List::new(items).block(Block::new().title(title).title_style(theme::TITLE));

        let mut state = ListState::default();
        state.select(Some(self.nav.index()));
        f.render_stateful_widget(list, chunks[1], &mut state);
    }
}

impl Component for BuildHistory {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn footer_hints(&self) -> &'static str {
        "↑↓ navigate  →/Enter view logs  ←/q/Esc back  Space select  d delete leases  c cancel  Q queue  o open  r refresh  ? help"
    }
}
