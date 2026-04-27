//! Personal work items list view (Assigned to me / Created by me).

use std::collections::BTreeMap;

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};

use super::Component;
use crate::client::models::{AssignedToField, WorkItem};
use crate::render::columns::work_item_row;
use crate::render::helpers::{
    draw_state_message, draw_view_frame, row_style, split_with_search_bar, sub_view_tab_spans,
};
use crate::render::table::{
    Align, DEFAULT_SCROLL_PADDING, format_cell, render_header, resolve_widths, visible_rows,
};
use crate::render::theme;
use crate::state::{App, InputMode, ListNav, View};

/// Flat work-item row data used for rendering and filtering.
#[derive(Debug, Clone)]
pub struct MyWorkItemRow {
    pub id: u32,
    pub title: String,
    pub work_item_type: String,
    pub state: String,
    pub assigned_to: Option<String>,
    pub iteration_path: Option<String>,
}

impl MyWorkItemRow {
    fn from_work_item(item: &WorkItem) -> Self {
        let assigned_to = item.fields.assigned_to.as_ref().map(|a| match a {
            AssignedToField::Identity(id) => id.display_name.clone(),
            AssignedToField::DisplayName(s) => s.clone(),
        });
        Self {
            id: item.id,
            title: item.fields.title.clone(),
            work_item_type: item.fields.work_item_type.clone(),
            state: item.fields.state.clone().unwrap_or_default(),
            assigned_to,
            iteration_path: item.fields.iteration_path.clone(),
        }
    }

    fn matches(&self, query: &str) -> bool {
        let q = query.to_lowercase();
        self.id.to_string().contains(&q)
            || self.title.to_lowercase().contains(&q)
            || self.work_item_type.to_lowercase().contains(&q)
            || self.state.to_lowercase().contains(&q)
            || self
                .assigned_to
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .contains(&q)
            || self
                .iteration_path
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .contains(&q)
    }
}

/// State for one of the personal Boards list sub-views.
#[derive(Debug, Default)]
pub struct MyWorkItemsList {
    all: BTreeMap<u32, MyWorkItemRow>,
    order: Vec<u32>,
    pub filtered: Vec<u32>,
    pub nav: ListNav,
    pub generation: u64,
}

impl MyWorkItemsList {
    /// Increments the generation counter and returns the new value.
    pub fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    /// Replaces the underlying data (preserving the WIQL-provided ordering)
    /// and rebuilds the filtered list using the given search query.
    pub fn set_data(&mut self, work_items: &[WorkItem], search_query: &str) {
        self.order.clear();
        self.all.clear();
        for row in work_items.iter().map(MyWorkItemRow::from_work_item) {
            if !self.all.contains_key(&row.id) {
                self.order.push(row.id);
            }
            self.all.insert(row.id, row);
        }
        self.rebuild(search_query);
    }

    /// Rebuilds the filtered list from `all` using the search query.
    pub fn rebuild(&mut self, search_query: &str) {
        if search_query.is_empty() {
            self.filtered = self.order.clone();
        } else {
            self.filtered = self
                .order
                .iter()
                .copied()
                .filter(|id| {
                    self.all
                        .get(id)
                        .is_some_and(|row| row.matches(search_query))
                })
                .collect();
        }
        self.nav.set_len(self.filtered.len());
    }

    /// Returns the row at the filtered row index.
    pub fn row_at(&self, index: usize) -> Option<&MyWorkItemRow> {
        self.filtered.get(index).and_then(|id| self.all.get(id))
    }
}

/// Stores state for both personal Boards sub-views (Assigned / Created).
#[derive(Debug, Default)]
pub struct MyWorkItems {
    pub assigned: MyWorkItemsList,
    pub created: MyWorkItemsList,
}

impl MyWorkItems {
    /// Returns a shared reference to the list backing the given view, if any.
    pub fn list_for(&self, view: View) -> Option<&MyWorkItemsList> {
        match view {
            View::BoardsAssignedToMe => Some(&self.assigned),
            View::BoardsCreatedByMe => Some(&self.created),
            _ => None,
        }
    }

    /// Returns a mutable reference to the list backing the given view, if any.
    pub fn list_for_mut(&mut self, view: View) -> Option<&mut MyWorkItemsList> {
        match view {
            View::BoardsAssignedToMe => Some(&mut self.assigned),
            View::BoardsCreatedByMe => Some(&mut self.created),
            _ => None,
        }
    }

    /// Renders the list for the currently active view.
    pub fn draw_with_app(&self, f: &mut Frame, app: &App, area: Rect) {
        let Some(list) = self.list_for(app.view) else {
            return;
        };

        let show_search = app.search.mode == InputMode::Search || !app.search.query.is_empty();
        let mut subtitle_spans = sub_view_tab_spans(app.service, app.view);
        subtitle_spans.push(Span::styled(
            format!("  ·  {} shown", list.filtered.len()),
            theme::MUTED,
        ));

        let title = match app.view {
            View::BoardsAssignedToMe => " Assigned to me ",
            View::BoardsCreatedByMe => " Created by me ",
            _ => " Work Items ",
        };
        let frame_area = draw_view_frame(f, area, title, Some(Line::from(subtitle_spans)));
        let list_area = split_with_search_bar(
            f,
            frame_area,
            &app.search.query,
            app.search.mode,
            show_search,
        );

        if list.filtered.is_empty() {
            let hint = if show_search {
                " No work items match the current search"
            } else {
                " No work items found"
            };
            draw_state_message(f, list_area, hint, theme::SUBTLE);
            return;
        }

        let schema = work_item_row();
        let list_area = render_header(f, list_area, &schema.columns);
        let widths: Vec<usize> = resolve_widths(&schema.columns, list_area.width)
            .iter()
            .map(|&w| w as usize)
            .collect();

        let window = visible_rows(
            list.filtered.len(),
            list.nav.index(),
            list_area.height,
            DEFAULT_SCROLL_PADDING,
        );
        let items: Vec<ListItem> = window
            .range()
            .filter_map(|i| {
                let row = list.row_at(i)?;
                let is_selected = window.selected == Some(i - window.start);
                let w_id = widths[schema.id];
                let w_type = widths[schema.work_item_type];
                let w_title = widths[schema.title];
                let w_state = widths[schema.state];
                let w_assigned = widths[schema.assigned];
                let w_iter = widths[schema.iteration];
                Some(
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format_cell(&format!("#{}", row.id), w_id, Align::Left),
                            id_style(),
                        ),
                        Span::styled(
                            format_cell(&row.work_item_type, w_type, Align::Left),
                            theme::work_item_type_style(&row.work_item_type),
                        ),
                        Span::styled(format_cell(&row.title, w_title, Align::Left), title_style()),
                        Span::styled(
                            format_cell(&row.state, w_state, Align::Left),
                            theme::work_item_state_style(&row.state),
                        ),
                        Span::styled(
                            format_cell(
                                row.assigned_to.as_deref().unwrap_or("—"),
                                w_assigned,
                                Align::Left,
                            ),
                            theme::SUBTLE,
                        ),
                        Span::styled(
                            format_cell(
                                row.iteration_path.as_deref().unwrap_or(""),
                                w_iter,
                                Align::Left,
                            ),
                            theme::SUBTLE,
                        ),
                    ]))
                    .style(row_style(is_selected)),
                )
            })
            .collect();

        let mut state = ListState::default();
        state.select(window.selected);
        f.render_stateful_widget(
            List::new(items).scroll_padding(DEFAULT_SCROLL_PADDING),
            list_area,
            &mut state,
        );
    }
}

fn id_style() -> Style {
    theme::MUTED
}

fn title_style() -> Style {
    theme::TEXT
}

impl Component for MyWorkItems {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn footer_hints(&self) -> &'static str {
        "Tab/Shift-Tab view  ↑↓ navigate  / search  o open  r refresh  1–4 areas  ? help"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::models::{AssignedToField, IdentityRef, WorkItem, WorkItemFields};

    fn wi(
        id: u32,
        title: &str,
        wtype: &str,
        state: &str,
        assigned: Option<&str>,
        iteration: Option<&str>,
    ) -> WorkItem {
        WorkItem {
            id,
            rev: None,
            fields: WorkItemFields {
                title: title.into(),
                work_item_type: wtype.into(),
                state: Some(state.into()),
                assigned_to: assigned.map(|s| {
                    AssignedToField::Identity(IdentityRef {
                        id: None,
                        display_name: s.into(),
                        unique_name: Some("user@example.com".into()),
                        descriptor: None,
                    })
                }),
                iteration_path: iteration.map(String::from),
                area_path: None,
                parent: None,
                board_column: None,
                stack_rank: None,
                ..Default::default()
            },
            relations: vec![],
            url: None,
        }
    }

    #[test]
    fn set_data_populates_filtered() {
        let mut list = MyWorkItemsList::default();
        list.set_data(
            &[
                wi(1, "A", "Bug", "Active", Some("Me"), None),
                wi(2, "B", "Task", "New", None, Some("Sprint 1")),
            ],
            "",
        );
        assert_eq!(list.filtered.len(), 2);
        assert_eq!(list.nav.len(), 2);
    }

    #[test]
    fn rebuild_filters_by_title() {
        let mut list = MyWorkItemsList::default();
        list.set_data(
            &[
                wi(1, "Fix auth bug", "Bug", "Active", None, None),
                wi(2, "Add feature", "Task", "New", None, None),
            ],
            "auth",
        );
        assert_eq!(list.filtered.len(), 1);
        assert_eq!(list.row_at(0).unwrap().id, 1);
    }

    #[test]
    fn rebuild_filters_by_id() {
        let mut list = MyWorkItemsList::default();
        list.set_data(
            &[
                wi(111, "A", "Bug", "Active", None, None),
                wi(222, "B", "Task", "New", None, None),
            ],
            "222",
        );
        assert_eq!(list.filtered.len(), 1);
        assert_eq!(list.row_at(0).unwrap().id, 222);
    }

    #[test]
    fn set_data_normalizes_duplicate_work_item_ids() {
        let mut list = MyWorkItemsList::default();
        list.set_data(
            &[
                wi(111, "Old title", "Bug", "Active", None, None),
                wi(111, "Updated title", "Bug", "Active", None, None),
            ],
            "",
        );

        assert_eq!(list.filtered, vec![111]);
        assert_eq!(list.row_at(0).unwrap().title, "Updated title");
    }

    #[test]
    fn next_generation_increments() {
        let mut list = MyWorkItemsList::default();
        assert_eq!(list.next_generation(), 1);
        assert_eq!(list.next_generation(), 2);
    }

    #[test]
    fn list_for_routes_by_view() {
        let my = MyWorkItems::default();
        assert!(my.list_for(View::BoardsAssignedToMe).is_some());
        assert!(my.list_for(View::BoardsCreatedByMe).is_some());
        assert!(my.list_for(View::Boards).is_none());
        assert!(my.list_for(View::Dashboard).is_none());
    }
}
