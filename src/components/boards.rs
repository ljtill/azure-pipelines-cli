//! Azure Boards backlog tree view component.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

use super::Component;
use crate::client::models::{BacklogLevelConfiguration, WorkItem};
use crate::render::columns::board_row;
use crate::render::helpers::{
    draw_state_message, draw_view_frame, row_style, split_with_search_bar, sub_view_tab_spans,
};
use crate::render::table::{
    Align, DEFAULT_SCROLL_PADDING, format_cell, render_header, resolve_widths, visible_rows,
};
use crate::render::theme;
use crate::shared::availability::AvailabilityStatus;
use crate::state::{App, InputMode, ListNav};

/// Represents a single work item in the Boards backlog tree.
#[derive(Debug, Clone)]
pub struct BoardItem {
    pub id: u32,
    pub title: String,
    pub work_item_type: String,
    pub state: String,
    pub assigned_to: Option<String>,
    pub iteration_path: Option<String>,
    pub parent_id: Option<u32>,
    pub child_ids: Vec<u32>,
    pub stack_rank: Option<f64>,
}

impl BoardItem {
    fn matches(&self, query: &str) -> bool {
        let id = self.id.to_string();
        id.contains(query)
            || self.title.to_lowercase().contains(query)
            || self.work_item_type.to_lowercase().contains(query)
            || self.state.to_lowercase().contains(query)
            || self
                .assigned_to
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .contains(query)
            || self
                .iteration_path
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .contains(query)
    }
}

/// Represents a rendered row in the Boards tree view.
#[derive(Debug, Clone)]
pub struct BoardRow {
    pub work_item_id: u32,
    pub depth: usize,
    pub has_children: bool,
    pub collapsed: bool,
}

/// Stores state for the Boards backlog tree view.
#[derive(Debug)]
pub struct Boards {
    pub nav: ListNav,
    pub items: BTreeMap<u32, BoardItem>,
    pub root_ids: Vec<u32>,
    pub rows: Vec<BoardRow>,
    pub collapsed: HashSet<u32>,
    pub loading: bool,
    pub error: Option<String>,
    pub availability: AvailabilityStatus,
    pub generation: u64,
    pub team_name: Option<String>,
    pub backlog_names: Vec<String>,
    initialized: bool,
}

impl Default for Boards {
    fn default() -> Self {
        Self {
            nav: ListNav::default(),
            items: BTreeMap::new(),
            root_ids: Vec::new(),
            rows: Vec::new(),
            collapsed: HashSet::new(),
            loading: false,
            error: None,
            availability: AvailabilityStatus::Unavailable,
            generation: 0,
            team_name: None,
            backlog_names: Vec::new(),
            initialized: false,
        }
    }
}

impl Boards {
    /// Increments the fetch generation and returns the new value.
    pub fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    /// Marks the Boards view as loading fresh data.
    pub fn start_loading(&mut self) {
        self.loading = true;
        if self.rows.is_empty() {
            self.error = None;
            self.availability = AvailabilityStatus::Unavailable;
        }
    }

    /// Stores an error message for the Boards view.
    pub fn set_error(&mut self, message: String) {
        self.loading = false;
        self.availability = if self.rows.is_empty() {
            AvailabilityStatus::Unavailable
        } else {
            AvailabilityStatus::Stale
        };
        self.error = Some(message);
    }

    /// Replaces the Boards data and rebuilds visible rows.
    pub fn set_data(
        &mut self,
        team_name: String,
        backlogs: Vec<BacklogLevelConfiguration>,
        work_items: Vec<WorkItem>,
        search_query: &str,
    ) {
        self.set_data_with_errors(team_name, backlogs, work_items, search_query, &[]);
    }

    /// Replaces the Boards data, records partial errors, and rebuilds visible rows.
    pub fn set_data_with_errors(
        &mut self,
        team_name: String,
        backlogs: Vec<BacklogLevelConfiguration>,
        work_items: Vec<WorkItem>,
        search_query: &str,
        partial_errors: &[String],
    ) {
        let backlog_names: Vec<String> = backlogs
            .into_iter()
            .filter(BacklogLevelConfiguration::is_visible)
            .map(|backlog| backlog.name)
            .collect();

        let mut items: BTreeMap<u32, BoardItem> = work_items
            .into_iter()
            .map(|work_item| {
                let board_item = BoardItem {
                    id: work_item.id,
                    title: work_item.title().to_string(),
                    work_item_type: work_item.work_item_type().to_string(),
                    state: work_item.state_label().to_string(),
                    assigned_to: work_item
                        .assigned_to_display()
                        .map(std::string::ToString::to_string),
                    iteration_path: work_item.fields.iteration_path.clone(),
                    parent_id: work_item.parent_id(),
                    child_ids: work_item.child_ids(),
                    stack_rank: work_item.fields.stack_rank,
                };
                (board_item.id, board_item)
            })
            .collect();

        // Normalize children from the hydrated parent field so stale relation payloads cannot
        // duplicate rows or attach a work item to the wrong parent.
        derive_child_ids_from_parents(&mut items);

        let ids: HashSet<u32> = items.keys().copied().collect();
        let mut root_ids: Vec<u32> = items
            .values()
            .filter(|item| {
                item.parent_id
                    .is_none_or(|parent_id| !ids.contains(&parent_id))
            })
            .map(|item| item.id)
            .collect();
        sort_ids_by_rank_and_id(&items, &mut root_ids);

        self.loading = false;
        self.availability = if partial_errors.is_empty() {
            AvailabilityStatus::Fresh
        } else {
            AvailabilityStatus::Partial
        };
        self.error = if partial_errors.is_empty() {
            None
        } else {
            Some(partial_errors.join("; "))
        };
        self.team_name = Some(team_name);
        self.backlog_names = backlog_names;
        self.items = items;
        self.root_ids = root_ids;

        if self.initialized {
            self.collapsed.retain(|id| self.items.contains_key(id));
        } else {
            self.initialized = true;
            self.collapsed = self
                .items
                .values()
                .filter(|item| !item.child_ids.is_empty())
                .map(|item| item.id)
                .collect();
        }

        self.rebuild(search_query);
    }

    /// Rebuilds the visible rows from the current tree and search query.
    pub fn rebuild(&mut self, search_query: &str) {
        let visible_ids = self.visible_ids(search_query);
        let expand_all = visible_ids.is_some();

        self.rows.clear();
        for root_id in self.root_ids.clone() {
            self.append_rows(root_id, 0, visible_ids.as_ref(), expand_all);
        }
        self.nav.set_len(self.rows.len());
    }

    /// Returns the currently selected work item ID, if any.
    pub fn selected_work_item_id(&self) -> Option<u32> {
        self.rows.get(self.nav.index()).map(|row| row.work_item_id)
    }

    /// Toggles collapse for the row at the given index.
    pub fn toggle_row(&mut self, index: usize, search_query: &str) -> bool {
        let Some(row) = self.rows.get(index) else {
            return false;
        };
        if !row.has_children {
            return false;
        }

        if self.collapsed.contains(&row.work_item_id) {
            self.collapsed.remove(&row.work_item_id);
        } else {
            self.collapsed.insert(row.work_item_id);
        }
        self.rebuild(search_query);
        true
    }

    /// Collapses the row at the given index if possible.
    pub fn collapse_row(&mut self, index: usize, search_query: &str) -> bool {
        let Some(row) = self.rows.get(index) else {
            return false;
        };
        if !row.has_children || row.collapsed {
            return false;
        }

        self.collapsed.insert(row.work_item_id);
        self.rebuild(search_query);
        true
    }

    /// Expands the row at the given index if possible.
    pub fn expand_row(&mut self, index: usize, search_query: &str) -> bool {
        let Some(row) = self.rows.get(index) else {
            return false;
        };
        if !row.has_children || !row.collapsed {
            return false;
        }

        self.collapsed.remove(&row.work_item_id);
        self.rebuild(search_query);
        true
    }

    /// Returns the parent row index for the row at the given index.
    pub fn parent_index(&self, index: usize) -> Option<usize> {
        let row = self.rows.get(index)?;
        if row.depth == 0 {
            return None;
        }

        self.rows
            .iter()
            .enumerate()
            .take(index)
            .rev()
            .find(|(_, candidate)| candidate.depth + 1 == row.depth)
            .map(|(candidate_index, _)| candidate_index)
    }

    /// Renders the Boards view using data from the App.
    pub fn draw_with_app(&self, f: &mut Frame, app: &App, area: Rect) {
        let show_search = app.search.mode == InputMode::Search || !app.search.query.is_empty();
        let mut subtitle_spans = sub_view_tab_spans(app.service, app.view);
        if !subtitle_spans.is_empty() {
            subtitle_spans.push(Span::styled("  ·  ", theme::MUTED));
        }
        let team = self.team_name.as_deref().unwrap_or("Backlog");
        subtitle_spans.push(Span::styled(format!(" {team}"), theme::TEXT));
        if !self.backlog_names.is_empty() {
            subtitle_spans.push(Span::styled("  ·  ", theme::MUTED));
            subtitle_spans.push(Span::styled(self.backlog_names.join(" / "), theme::MUTED));
        }
        subtitle_spans.push(Span::styled(
            format!("  ·  {} items", self.rows.len()),
            theme::MUTED,
        ));
        if self.loading && !self.rows.is_empty() {
            subtitle_spans.push(Span::styled("  ·  refreshing", theme::TITLE));
        }
        if let Some(label) = self.availability_label() {
            subtitle_spans.push(Span::styled("  ·  ", theme::MUTED));
            subtitle_spans.push(Span::styled(label, theme::WARNING));
        }
        let frame_area = draw_view_frame(f, area, " Boards ", Some(Line::from(subtitle_spans)));
        let mut list_area = split_with_search_bar(
            f,
            frame_area,
            &app.search.query,
            app.search.mode,
            show_search,
        );

        if self.loading && self.rows.is_empty() {
            draw_state_message(f, list_area, " Loading backlog...", theme::SUBTLE);
            return;
        }

        if let Some(message) = &self.error
            && self.rows.is_empty()
        {
            draw_state_message(f, list_area, format!(" {message}"), theme::WARNING);
            return;
        }

        if self.rows.is_empty() {
            let hint = if show_search {
                " No backlog items match the current search"
            } else {
                " No backlog items found"
            };
            draw_state_message(f, list_area, hint, theme::SUBTLE);
            return;
        }

        if let Some(line) = self.degraded_line()
            && list_area.height > 0
        {
            let chunks =
                Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(list_area);
            f.render_widget(Paragraph::new(line), chunks[0]);
            list_area = chunks[1];
        }

        let schema = board_row();
        let list_area = render_header(f, list_area, &schema.columns);
        let widths: Vec<usize> = resolve_widths(&schema.columns, list_area.width)
            .iter()
            .map(|&w| w as usize)
            .collect();

        let window = visible_rows(
            self.rows.len(),
            self.nav.index(),
            list_area.height,
            DEFAULT_SCROLL_PADDING,
        );
        let items: Vec<ListItem> = window
            .range()
            .filter_map(|index| {
                let row = self.rows.get(index)?;
                let item = self.items.get(&row.work_item_id)?;
                let is_selected = window.selected == Some(index - window.start);
                let arrow = if row.has_children {
                    if row.collapsed { "▸" } else { "▾" }
                } else {
                    " "
                };
                let indent = "  ".repeat(row.depth);
                let title = format!("{indent}{arrow} {}", item.title);
                let w_type = widths[schema.work_item_type];
                let w_id = widths[schema.id];
                let w_title = widths[schema.title];
                let w_state = widths[schema.state];
                let w_assigned = widths[schema.assigned];
                let w_iter = widths[schema.iteration];

                Some(
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format_cell(&item.work_item_type, w_type, Align::Left),
                            theme::work_item_type_style(&item.work_item_type),
                        ),
                        Span::styled(
                            format_cell(&format!("#{}", item.id), w_id, Align::Left),
                            id_style(),
                        ),
                        Span::styled(format_cell(&title, w_title, Align::Left), title_style()),
                        Span::styled(
                            format_cell(&item.state, w_state, Align::Left),
                            theme::work_item_state_style(&item.state),
                        ),
                        Span::styled(
                            format_cell(
                                item.assigned_to.as_deref().unwrap_or(""),
                                w_assigned,
                                Align::Left,
                            ),
                            theme::SUBTLE,
                        ),
                        Span::styled(
                            format_cell(
                                item.iteration_path.as_deref().unwrap_or(""),
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

        let list = List::new(items).scroll_padding(DEFAULT_SCROLL_PADDING);
        let mut state = ListState::default();
        state.select(window.selected);
        f.render_stateful_widget(list, list_area, &mut state);
    }

    fn visible_ids(&self, search_query: &str) -> Option<HashSet<u32>> {
        if search_query.is_empty() {
            return None;
        }

        let query = search_query.to_lowercase();
        let mut visible = HashSet::new();

        for item in self.items.values().filter(|item| item.matches(&query)) {
            let mut current = Some(item.id);
            while let Some(id) = current {
                if !visible.insert(id) {
                    break;
                }
                current = self
                    .items
                    .get(&id)
                    .and_then(|candidate| candidate.parent_id);
            }
        }

        Some(visible)
    }

    fn availability_label(&self) -> Option<&'static str> {
        match self.availability {
            AvailabilityStatus::Fresh => None,
            AvailabilityStatus::Partial => Some("partial"),
            AvailabilityStatus::Stale => Some("stale"),
            AvailabilityStatus::Unavailable => self.error.as_ref().map(|_| "unavailable"),
        }
    }

    fn degraded_line(&self) -> Option<Line<'static>> {
        let message = self.error.as_ref()?;
        let (prefix, style) = match self.availability {
            AvailabilityStatus::Fresh => return None,
            AvailabilityStatus::Partial => ("⚠ Partial backlog data: ", theme::WARNING),
            AvailabilityStatus::Stale => ("⚠ Showing stale backlog data: ", theme::WARNING),
            AvailabilityStatus::Unavailable => ("⚠ Backlog unavailable: ", theme::ERROR),
        };
        Some(Line::from(vec![
            Span::styled(prefix.to_string(), style),
            Span::styled(message.clone(), style),
        ]))
    }

    fn append_rows(
        &mut self,
        work_item_id: u32,
        depth: usize,
        visible_ids: Option<&HashSet<u32>>,
        expand_all: bool,
    ) {
        let Some(item) = self.items.get(&work_item_id) else {
            return;
        };
        if let Some(visible_ids) = visible_ids
            && !visible_ids.contains(&work_item_id)
        {
            return;
        }

        let visible_children: Vec<u32> = item
            .child_ids
            .iter()
            .copied()
            .filter(|child_id| visible_ids.is_none_or(|ids| ids.contains(child_id)))
            .collect();
        let has_children = !visible_children.is_empty();
        let collapsed = has_children && self.collapsed.contains(&work_item_id) && !expand_all;

        self.rows.push(BoardRow {
            work_item_id,
            depth,
            has_children,
            collapsed,
        });

        if collapsed {
            return;
        }

        for child_id in visible_children {
            self.append_rows(child_id, depth + 1, visible_ids, expand_all);
        }
    }
}

fn derive_child_ids_from_parents(items: &mut BTreeMap<u32, BoardItem>) {
    let ids: HashSet<u32> = items.keys().copied().collect();
    let parent_links: Vec<(u32, u32)> = items
        .values()
        .filter_map(|item| {
            item.parent_id
                .filter(|parent_id| ids.contains(parent_id))
                .map(|parent_id| (parent_id, item.id))
        })
        .collect();

    for item in items.values_mut() {
        item.child_ids.clear();
    }

    for (parent_id, child_id) in parent_links {
        if let Some(parent) = items.get_mut(&parent_id) {
            parent.child_ids.push(child_id);
        }
    }

    let item_ids: Vec<u32> = items.keys().copied().collect();
    for item_id in item_ids {
        let Some(existing_child_ids) = items.get(&item_id).map(|item| item.child_ids.clone())
        else {
            continue;
        };
        let mut child_ids = existing_child_ids;
        sort_ids_by_rank_and_id(items, &mut child_ids);
        if let Some(item) = items.get_mut(&item_id) {
            item.child_ids = child_ids;
        }
    }
}

impl Component for Boards {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn footer_hints(&self) -> &'static str {
        "↑↓ navigate  ←→ collapse/expand  Enter toggle  / search  o open  r refresh  1–4 areas  ? help"
    }
}

fn sort_ids_by_rank_and_id(items: &BTreeMap<u32, BoardItem>, ids: &mut [u32]) {
    ids.sort_by(|left_id, right_id| {
        let left = items.get(left_id).unwrap();
        let right = items.get(right_id).unwrap();
        // Push terminal-state items (Closed / Done / Removed / Cut) below active
        // items at every level of the hierarchy.
        let terminal_order = is_terminal_state(&left.state).cmp(&is_terminal_state(&right.state));
        if terminal_order != Ordering::Equal {
            return terminal_order;
        }
        match (left.stack_rank, right.stack_rank) {
            (Some(left_rank), Some(right_rank)) => left_rank
                .partial_cmp(&right_rank)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left_id.cmp(right_id)),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => left_id.cmp(right_id),
        }
    });
}

/// Returns `true` for work item states considered terminal (Closed, Done,
/// Removed, Cut). Used to sort completed items below active work.
fn is_terminal_state(state: &str) -> bool {
    matches!(
        state.to_ascii_lowercase().as_str(),
        "closed" | "done" | "removed" | "cut"
    )
}

fn id_style() -> Style {
    theme::MUTED
}

fn title_style() -> Style {
    theme::TEXT
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::models::{
        BacklogLevelConfiguration, WorkItem, WorkItemFields, WorkItemRelation,
        WorkItemTypeReference,
    };

    fn item(id: u32, parent_id: Option<u32>, title: &str) -> BoardItem {
        BoardItem {
            id,
            title: title.to_string(),
            work_item_type: "Task".to_string(),
            state: "Active".to_string(),
            assigned_to: None,
            iteration_path: None,
            parent_id,
            child_ids: vec![],
            stack_rank: Some(f64::from(id)),
        }
    }

    fn backlog(name: &str, rank: u32, is_hidden: bool) -> BacklogLevelConfiguration {
        BacklogLevelConfiguration {
            id: format!("backlog-{name}"),
            name: name.to_string(),
            rank,
            work_item_count_limit: None,
            work_item_types: vec![WorkItemTypeReference {
                name: name.to_string(),
                url: None,
            }],
            default_work_item_type: None,
            color: None,
            is_hidden,
            backlog_type: None,
        }
    }

    fn work_item(
        id: u32,
        parent_id: Option<u32>,
        child_ids: Vec<u32>,
        title: &str,
        stack_rank: f64,
    ) -> WorkItem {
        WorkItem {
            id,
            rev: None,
            fields: WorkItemFields {
                title: title.to_string(),
                work_item_type: if parent_id.is_none() {
                    "Epic".to_string()
                } else {
                    "Feature".to_string()
                },
                state: Some("Active".to_string()),
                assigned_to: None,
                iteration_path: Some("Project\\Iteration".to_string()),
                area_path: None,
                parent: parent_id,
                board_column: None,
                stack_rank: Some(stack_rank),
                ..Default::default()
            },
            relations: child_ids
                .into_iter()
                .map(|child_id| WorkItemRelation {
                    rel: Some("System.LinkTypes.Hierarchy-Forward".to_string()),
                    url: format!("https://dev.azure.com/org/_apis/wit/workItems/{child_id}"),
                    attributes: std::collections::HashMap::new(),
                })
                .collect(),
            url: None,
        }
    }

    #[test]
    fn rebuild_creates_tree_rows() {
        let mut boards = Boards {
            items: BTreeMap::from([
                (
                    1,
                    BoardItem {
                        child_ids: vec![2],
                        ..item(1, None, "Root")
                    },
                ),
                (2, item(2, Some(1), "Child")),
            ]),
            root_ids: vec![1],
            collapsed: HashSet::new(),
            ..Default::default()
        };

        boards.rebuild("");

        assert_eq!(boards.rows.len(), 2);
        assert_eq!(boards.rows[0].depth, 0);
        assert_eq!(boards.rows[1].depth, 1);
    }

    #[test]
    fn search_keeps_matching_ancestors() {
        let mut boards = Boards {
            items: BTreeMap::from([
                (
                    1,
                    BoardItem {
                        child_ids: vec![2],
                        ..item(1, None, "Root")
                    },
                ),
                (2, item(2, Some(1), "Needle")),
            ]),
            root_ids: vec![1],
            collapsed: HashSet::from([1]),
            ..Default::default()
        };

        boards.rebuild("needle");

        assert_eq!(boards.rows.len(), 2);
        assert_eq!(boards.rows[0].work_item_id, 1);
        assert_eq!(boards.rows[1].work_item_id, 2);
    }

    #[test]
    fn toggle_row_updates_collapse_state() {
        let mut boards = Boards {
            items: BTreeMap::from([
                (
                    1,
                    BoardItem {
                        child_ids: vec![2],
                        ..item(1, None, "Root")
                    },
                ),
                (2, item(2, Some(1), "Child")),
            ]),
            root_ids: vec![1],
            collapsed: HashSet::new(),
            ..Default::default()
        };
        boards.rebuild("");

        assert!(boards.toggle_row(0, ""));
        assert!(boards.rows[0].collapsed);
        assert_eq!(boards.rows.len(), 1);
    }

    #[test]
    fn set_data_filters_hidden_backlogs_and_sorts_tree_by_rank() {
        let mut boards = Boards::default();

        boards.set_data(
            "Project Team".to_string(),
            vec![
                backlog("Hidden", 0, true),
                backlog("Epics", 1, false),
                backlog("Features", 2, false),
            ],
            vec![
                work_item(30, None, vec![], "Later root", 30.0),
                work_item(10, None, vec![20], "Earlier root", 10.0),
                work_item(20, Some(10), vec![], "Child", 20.0),
            ],
            "",
        );

        assert_eq!(boards.team_name.as_deref(), Some("Project Team"));
        assert_eq!(boards.backlog_names, vec!["Epics", "Features"]);
        assert_eq!(boards.root_ids, vec![10, 30]);
        assert_eq!(
            boards
                .rows
                .iter()
                .map(|row| row.work_item_id)
                .collect::<Vec<_>>(),
            vec![10, 30]
        );
        assert!(boards.rows[0].collapsed);

        assert!(boards.expand_row(0, ""));
        assert_eq!(
            boards
                .rows
                .iter()
                .map(|row| row.work_item_id)
                .collect::<Vec<_>>(),
            vec![10, 20, 30]
        );
    }

    #[test]
    fn set_data_prunes_stale_collapsed_ids_on_refresh() {
        let mut boards = Boards::default();
        let backlogs = vec![backlog("Epics", 1, false)];

        boards.set_data(
            "Project Team".to_string(),
            backlogs.clone(),
            vec![
                work_item(1, None, vec![2], "Root", 1.0),
                work_item(2, Some(1), vec![], "Child", 2.0),
                work_item(3, None, vec![4], "Stale root", 3.0),
                work_item(4, Some(3), vec![], "Stale child", 4.0),
            ],
            "",
        );
        boards.collapsed.remove(&1);

        boards.set_data(
            "Project Team".to_string(),
            backlogs,
            vec![
                work_item(1, None, vec![2], "Root", 1.0),
                work_item(2, Some(1), vec![], "Child", 2.0),
            ],
            "",
        );

        assert!(boards.collapsed.is_empty());
        assert_eq!(
            boards
                .rows
                .iter()
                .map(|row| row.work_item_id)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[test]
    fn set_error_marks_existing_backlog_stale_without_dropping_rows() {
        let mut boards = Boards::default();
        boards.set_data(
            "Project Team".to_string(),
            vec![backlog("Epics", 1, false)],
            vec![work_item(10, None, vec![], "Root", 10.0)],
            "",
        );

        boards.set_error("Failed to load backlog: timeout".to_string());

        assert_eq!(boards.availability, AvailabilityStatus::Stale);
        assert_eq!(
            boards.error.as_deref(),
            Some("Failed to load backlog: timeout")
        );
        assert_eq!(boards.rows.len(), 1);
        assert_eq!(boards.rows[0].work_item_id, 10);
    }

    #[test]
    fn set_data_with_errors_marks_backlog_partial() {
        let mut boards = Boards::default();

        boards.set_data_with_errors(
            "Project Team".to_string(),
            vec![backlog("Epics", 1, false)],
            vec![work_item(10, None, vec![], "Root", 10.0)],
            "",
            &["Descendant hierarchy unavailable".to_string()],
        );

        assert_eq!(boards.availability, AvailabilityStatus::Partial);
        assert_eq!(
            boards.error.as_deref(),
            Some("Descendant hierarchy unavailable")
        );
        assert_eq!(boards.rows.len(), 1);
    }

    #[test]
    fn set_data_derives_children_from_parent_field_when_relations_are_absent() {
        let mut boards = Boards::default();

        boards.set_data(
            "Project Team".to_string(),
            vec![backlog("Epics", 1, false)],
            vec![
                work_item(10, None, vec![], "Parent", 10.0),
                work_item(20, Some(10), vec![], "Child", 20.0),
            ],
            "",
        );

        assert_eq!(
            boards.items.get(&10).map(|item| item.child_ids.as_slice()),
            Some(&[20][..])
        );
        assert!(boards.expand_row(0, ""));
        assert_eq!(
            boards
                .rows
                .iter()
                .map(|row| row.work_item_id)
                .collect::<Vec<_>>(),
            vec![10, 20]
        );
    }

    #[test]
    fn set_data_prefers_parent_field_over_stale_relation_children() {
        let mut boards = Boards::default();

        boards.set_data(
            "Project Team".to_string(),
            vec![backlog("Epics", 1, false)],
            vec![
                work_item(10, None, vec![30], "Stale relation parent", 10.0),
                work_item(20, None, vec![], "Authoritative parent", 20.0),
                work_item(30, Some(20), vec![], "Child", 30.0),
            ],
            "",
        );

        assert_eq!(
            boards.items.get(&10).map(|item| item.child_ids.as_slice()),
            Some(&[][..])
        );
        assert_eq!(
            boards.items.get(&20).map(|item| item.child_ids.as_slice()),
            Some(&[30][..])
        );
        assert_eq!(
            boards
                .rows
                .iter()
                .map(|row| row.work_item_id)
                .collect::<Vec<_>>(),
            vec![10, 20]
        );
        assert!(!boards.rows[0].has_children);
        assert!(boards.rows[1].collapsed);

        assert!(boards.expand_row(1, ""));
        assert_eq!(
            boards
                .rows
                .iter()
                .map(|row| row.work_item_id)
                .collect::<Vec<_>>(),
            vec![10, 20, 30]
        );
        assert_eq!(
            boards
                .rows
                .iter()
                .filter(|row| row.work_item_id == 30)
                .count(),
            1
        );
    }

    fn item_with_state(id: u32, state: &str, rank: Option<f64>) -> BoardItem {
        BoardItem {
            id,
            title: format!("item {id}"),
            work_item_type: "Task".to_string(),
            state: state.to_string(),
            assigned_to: None,
            iteration_path: None,
            parent_id: None,
            child_ids: vec![],
            stack_rank: rank,
        }
    }

    #[test]
    fn sort_places_terminal_states_after_active_regardless_of_rank() {
        let items: BTreeMap<u32, BoardItem> = [
            item_with_state(1, "Closed", Some(1.0)),
            item_with_state(2, "Active", Some(5.0)),
            item_with_state(3, "Done", Some(2.0)),
            item_with_state(4, "New", Some(10.0)),
        ]
        .into_iter()
        .map(|i| (i.id, i))
        .collect();

        let mut ids = vec![1, 2, 3, 4];
        sort_ids_by_rank_and_id(&items, &mut ids);
        // Active (rank 5, 10) before terminal (rank 1, 2).
        assert_eq!(ids, vec![2, 4, 1, 3]);
    }

    #[test]
    fn sort_matches_terminal_states_case_insensitively() {
        assert!(is_terminal_state("Closed"));
        assert!(is_terminal_state("closed"));
        assert!(is_terminal_state("DONE"));
        assert!(is_terminal_state("Removed"));
        assert!(is_terminal_state("Cut"));
        assert!(!is_terminal_state("Active"));
        assert!(!is_terminal_state("In Progress"));
        assert!(!is_terminal_state(""));
    }

    #[test]
    fn sort_falls_back_to_rank_then_id_within_same_state_bucket() {
        let items: BTreeMap<u32, BoardItem> = [
            item_with_state(1, "Active", None),
            item_with_state(2, "Active", Some(5.0)),
            item_with_state(3, "Active", Some(5.0)),
            item_with_state(4, "Closed", Some(1.0)),
            item_with_state(5, "Closed", None),
        ]
        .into_iter()
        .map(|i| (i.id, i))
        .collect();

        let mut ids = vec![1, 2, 3, 4, 5];
        sort_ids_by_rank_and_id(&items, &mut ids);
        // Active: rank-first (2,3 tied -> id), then rankless (1).
        // Closed last: rank-first (4), then rankless (5).
        assert_eq!(ids, vec![2, 3, 1, 4, 5]);
    }
}
