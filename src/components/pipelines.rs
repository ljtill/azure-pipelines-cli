use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState};

use super::Component;
use crate::api::models::PipelineDefinition;
use crate::app::nav::ListNav;
use crate::app::{App, InputMode};
use crate::ui::helpers::{row_style, split_with_search_bar, truncate};
use crate::ui::theme;

/// Pipelines flat-list component — renders all pipeline definitions with search.
#[derive(Debug, Default)]
pub struct Pipelines {
    pub filtered: Vec<PipelineDefinition>,
    pub nav: ListNav,
}

impl Pipelines {
    pub fn rebuild(
        &mut self,
        definitions: &[PipelineDefinition],
        filter_folders: &[String],
        filter_definition_ids: &[u32],
        search_query: &str,
    ) {
        let base = definitions.iter().filter(|d| {
            if !filter_definition_ids.is_empty() && !filter_definition_ids.contains(&d.id) {
                return false;
            }
            if !filter_folders.is_empty() && !filter_folders.iter().any(|f| d.path.starts_with(f)) {
                return false;
            }
            true
        });

        if search_query.is_empty() {
            self.filtered = base.cloned().collect();
        } else {
            let q = search_query.to_lowercase();
            self.filtered = base
                .filter(|d| {
                    d.name.to_lowercase().contains(&q) || d.path.to_lowercase().contains(&q)
                })
                .cloned()
                .collect();
        }
        self.filtered
            .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.nav.set_len(self.filtered.len());
    }

    /// Render the pipelines view using data from the App.
    pub fn draw_with_app(&self, f: &mut Frame, app: &App, area: Rect) {
        let show_search = app.search.mode == InputMode::Search || !app.search.query.is_empty();
        let list_area =
            split_with_search_bar(f, area, &app.search.query, app.search.mode, show_search);

        // Column layout: padding(1) | name(fill) | folder(fill)
        let rects = Layout::horizontal([
            Constraint::Length(1),
            Constraint::Fill(2),
            Constraint::Fill(3),
        ])
        .split(area);
        let mut widths: Vec<usize> = rects.iter().map(|r| r.width as usize).collect();
        widths[1] = widths[1].min(50);
        widths[2] = widths[2].min(80);

        let items: Vec<ListItem> = self
            .filtered
            .iter()
            .enumerate()
            .map(|(i, def)| {
                let folder = def.path.trim_start_matches('\\').replace('\\', " / ");

                ListItem::new(Line::from(vec![
                    Span::raw(" "),
                    Span::styled(
                        format!(
                            "{:<width$} ",
                            truncate(&def.name, widths[1]),
                            width = widths[1]
                        ),
                        theme::TEXT,
                    ),
                    Span::styled(truncate(&folder, widths[2]), theme::MUTED),
                ]))
                .style(row_style(i == self.nav.index()))
            })
            .collect();

        let title = format!(" All Pipelines ({}) ", self.filtered.len());
        let list = List::new(items).block(Block::new().title(title).title_style(theme::TITLE));

        let mut state = ListState::default();
        state.select(Some(self.nav.index()));
        f.render_stateful_widget(list, list_area, &mut state);
    }
}

impl Component for Pipelines {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        // Rendering requires App context — use draw_with_app() instead.
        Ok(())
    }

    fn footer_hints(&self) -> &str {
        "↑↓ navigate  →/Enter drill-in  Q queue  o open  / search  1/2/3 tabs  r refresh  , settings  ? help  q/Esc back"
    }
}
