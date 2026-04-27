//! Build history view component displaying past build results.

use std::collections::HashSet;

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};

use super::Component;
use crate::client::models::{Build, PipelineDefinition};
use crate::render::columns::{BuildRowOpts, build_row};
use crate::render::helpers::{
    build_elapsed, draw_state_message, draw_view_frame, effective_status_icon,
    effective_status_label, row_style, truncate,
};
use crate::render::table::{render_header, resolve_widths};
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
    /// Monotonic counter incremented on each fetch request to discard stale responses.
    pub generation: u64,
}

impl BuildHistory {
    /// Increments the generation counter and returns the new value.
    pub fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

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
        let def_name = self
            .selected_definition
            .as_ref()
            .map_or("Unknown", |d| d.name.as_str());
        let total = self.builds.len();
        let selected_count = self.selected.len();
        let subtitle = Line::from(vec![
            Span::styled(format!(" {def_name}"), theme::TEXT),
            Span::styled(format!("  ·  {total} builds"), theme::MUTED),
            Span::styled(
                format!("  ·  {selected_count} selected"),
                if selected_count > 0 {
                    theme::SUCCESS
                } else {
                    theme::MUTED
                },
            ),
        ]);
        let content_area = draw_view_frame(f, area, " Build History ", Some(subtitle));

        if self.builds.is_empty() {
            draw_state_message(f, content_area, " No builds found", theme::SUBTLE);
            return;
        }

        let schema = build_row(BuildRowOpts {
            select: true,
            name: false,
            retained: true,
        });
        let content_area = render_header(f, content_area, &schema.columns);
        let resolved = resolve_widths(&schema.columns, content_area.width);
        let widths: Vec<usize> = resolved.iter().map(|&w| w as usize).collect();

        let mut items: Vec<ListItem> = self
            .builds
            .iter()
            .enumerate()
            .map(|(i, build)| {
                let is_focused = i == self.nav.index();
                let awaiting = app.data.pending_approval_build_ids.contains(&build.id);
                let (icon, icon_color) =
                    effective_status_icon(build.status, build.result, awaiting);
                let label = effective_status_label(build.status, build.result, awaiting);
                let time_info = build_elapsed(build);
                let branch = build.branch_display();
                let retained = app.retention_leases.retained_run_ids.contains(&build.id);
                let selected = self.selected.contains(&build.id);
                let check = if selected { "✓ " } else { "  " };
                let primary_style = theme::TEXT;
                let secondary_style = theme::SUBTLE;

                ListItem::new(Line::from(vec![
                    Span::styled(
                        check,
                        if selected {
                            theme::SUCCESS
                        } else {
                            Style::new()
                        },
                    ),
                    Span::styled(format!(" {icon} "), theme::foreground(icon_color)),
                    Span::styled(
                        format!("{:<width$}", label, width = widths[2]),
                        theme::foreground(icon_color),
                    ),
                    Span::styled(
                        format!(
                            "#{:<width$}",
                            truncate(&build.build_number, widths[3] - 1),
                            width = widths[3] - 1
                        ),
                        primary_style,
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
                        secondary_style,
                    ),
                    Span::styled(
                        format!("{:>width$}", time_info, width = widths[7]),
                        secondary_style,
                    ),
                ]))
                .style(row_style(is_focused))
            })
            .collect();

        if self.loading_more {
            items.push(ListItem::new(Line::from(vec![Span::styled(
                "   ⟳ Loading more...",
                theme::SUBTLE,
            )])));
        } else if self.has_more {
            items.push(ListItem::new(Line::from(vec![Span::styled(
                "   ▾ ↓ for more",
                theme::SUBTLE,
            )])));
        }

        let list = List::new(items).scroll_padding(3);

        let mut state = ListState::default();
        state.select(Some(self.nav.index()));
        f.render_stateful_widget(list, content_area, &mut state);
    }
}

impl Component for BuildHistory {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn footer_hints(&self) -> &'static str {
        "↑↓ navigate  →/Enter view logs  ←/q/Esc back  Space select  d delete leases  c cancel  Q queue  o open  1–4 areas  r refresh  ? help"
    }
}
