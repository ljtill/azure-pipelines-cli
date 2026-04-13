//! Pipelines view component showing all pipeline definitions grouped by folder.

use std::collections::{BTreeMap, HashSet};

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState};

use super::Component;
use crate::client::models::{Build, PipelineDefinition};
use crate::render::helpers::{
    build_elapsed, effective_status_icon, effective_status_label, split_with_search_bar, truncate,
};
use crate::render::theme;
use crate::state::nav::ListNav;
use crate::state::{App, InputMode};

/// Represents a row in the folder-grouped pipeline view.
#[derive(Debug, Clone)]
pub enum PipelineRow {
    FolderHeader {
        path: String,
        collapsed: bool,
    },
    Pipeline {
        definition: PipelineDefinition,
        latest_build: Option<Box<Build>>,
        pinned: bool,
    },
}

/// Normalizes an ADO definition path to a canonical folder key.
fn folder_key(path: &str) -> String {
    if path.is_empty() || path == "\\" {
        "\\".to_string()
    } else {
        path.to_string()
    }
}

/// Converts a raw folder key to a display-friendly string.
fn folder_display(key: &str) -> String {
    let display = key.trim_start_matches('\\').replace('\\', " / ");
    if display.is_empty() {
        "Root".to_string()
    } else {
        display
    }
}

/// Checks if a definition passes the configured filters.
fn matches_filter(
    def: &PipelineDefinition,
    filter_definition_ids: &[u32],
    filter_folders: &[String],
) -> bool {
    if !filter_definition_ids.is_empty() && !filter_definition_ids.contains(&def.id) {
        return false;
    }
    if !filter_folders.is_empty() && !filter_folders.iter().any(|f| def.path.starts_with(f)) {
        return false;
    }
    true
}

/// Reverse-looks up a folder key from a display path.
fn find_folder_key_for_display(
    display_path: &str,
    definitions: &[PipelineDefinition],
) -> Option<String> {
    for def in definitions {
        let key = folder_key(&def.path);
        if folder_display(&key) == display_path {
            return Some(key);
        }
    }
    None
}

/// Renders pipelines grouped by folder with collapse/expand, search, and pinning.
#[derive(Debug, Default)]
pub struct Pipelines {
    pub rows: Vec<PipelineRow>,
    pub collapsed_folders: HashSet<String>,
    pub nav: ListNav,
    pub selected: HashSet<u32>,
}

impl Pipelines {
    /// Rebuilds the pipeline rows from definitions + latest builds, grouped by folder.
    pub fn rebuild(
        &mut self,
        definitions: &[PipelineDefinition],
        latest_builds_by_def: &BTreeMap<u32, Build>,
        filter_folders: &[String],
        filter_definition_ids: &[u32],
        pinned_definition_ids: &[u32],
        search_query: &str,
    ) {
        let mut rows = Vec::new();
        let mut by_folder: BTreeMap<String, Vec<(PipelineDefinition, Option<Build>)>> =
            BTreeMap::new();
        let query_lower = search_query.to_lowercase();

        for def in definitions {
            if !matches_filter(def, filter_definition_ids, filter_folders) {
                continue;
            }
            if !search_query.is_empty()
                && !def.name.to_lowercase().contains(&query_lower)
                && !def.path.to_lowercase().contains(&query_lower)
            {
                continue;
            }
            let folder = folder_key(&def.path);
            let latest = latest_builds_by_def.get(&def.id).cloned();
            by_folder
                .entry(folder)
                .or_default()
                .push((def.clone(), latest));
        }

        for (key, mut pipelines) in by_folder {
            pipelines.sort_by(|(a, _), (b, _)| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            let collapsed = search_query.is_empty() && self.collapsed_folders.contains(&key);
            rows.push(PipelineRow::FolderHeader {
                path: folder_display(&key),
                collapsed,
            });

            if !collapsed {
                for (def, build) in &pipelines {
                    let pinned = pinned_definition_ids.contains(&def.id);
                    rows.push(PipelineRow::Pipeline {
                        definition: def.clone(),
                        latest_build: build.clone().map(Box::new),
                        pinned,
                    });
                }
            }
        }

        self.rows = rows;
        self.nav.set_len(self.rows.len());
    }

    /// Toggles collapse state for a folder at the given row index.
    pub fn toggle_folder_at(&mut self, index: usize, definitions: &[PipelineDefinition]) -> bool {
        if let Some(PipelineRow::FolderHeader { path, .. }) = self.rows.get(index) {
            let fk = find_folder_key_for_display(path, definitions);
            if let Some(key) = fk {
                if self.collapsed_folders.contains(&key) {
                    self.collapsed_folders.remove(&key);
                } else {
                    self.collapsed_folders.insert(key);
                }
                return true;
            }
        }
        false
    }

    /// Collapses the folder at the given index.
    pub fn collapse_folder_at(&mut self, index: usize, definitions: &[PipelineDefinition]) -> bool {
        if let Some(PipelineRow::FolderHeader {
            path, collapsed, ..
        }) = self.rows.get(index)
            && !collapsed
        {
            let fk = find_folder_key_for_display(path, definitions);
            if let Some(key) = fk {
                self.collapsed_folders.insert(key);
                return true;
            }
        }
        false
    }

    /// Expands the folder at the given index.
    pub fn expand_folder_at(&mut self, index: usize, definitions: &[PipelineDefinition]) -> bool {
        if let Some(PipelineRow::FolderHeader {
            path, collapsed, ..
        }) = self.rows.get(index)
            && *collapsed
        {
            let fk = find_folder_key_for_display(path, definitions);
            if let Some(key) = fk {
                self.collapsed_folders.remove(&key);
                return true;
            }
        }
        false
    }

    /// Finds the row index of the parent folder for a pipeline row.
    pub fn find_parent_folder_index(&self, pipeline_index: usize) -> Option<usize> {
        for i in (0..pipeline_index).rev() {
            if let Some(PipelineRow::FolderHeader { .. }) = self.rows.get(i) {
                return Some(i);
            }
        }
        None
    }

    /// Checks if a row is a folder header.
    pub fn is_folder_header(&self, index: usize) -> bool {
        matches!(self.rows.get(index), Some(PipelineRow::FolderHeader { .. }))
    }

    /// Returns the definition at the given row index, if it is a pipeline row.
    pub fn definition_at(&self, index: usize) -> Option<&PipelineDefinition> {
        match self.rows.get(index) {
            Some(PipelineRow::Pipeline { definition, .. }) => Some(definition),
            _ => None,
        }
    }

    /// Renders the folder-grouped pipeline list.
    pub fn draw_with_app(&self, f: &mut Frame, app: &App, area: Rect) {
        let show_search = app.search.mode == InputMode::Search || !app.search.query.is_empty();
        let list_area =
            split_with_search_bar(f, area, &app.search.query, app.search.mode, show_search);

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
        .split(list_area);
        let mut widths: Vec<usize> = rects.iter().map(|r| r.width as usize).collect();
        widths[3] = widths[3].min(40);
        widths[5] = widths[5].min(35);
        widths[6] = widths[6].min(35);

        let items: Vec<ListItem> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| match row {
                PipelineRow::FolderHeader { path, collapsed } => {
                    let icon = if *collapsed { "▸" } else { "▾" };
                    ListItem::new(Line::from(vec![
                        Span::styled(format!(" {icon} "), theme::ARROW),
                        Span::styled(path.clone(), theme::FOLDER),
                    ]))
                    .style(if i == self.nav.index() {
                        theme::SELECTED
                    } else {
                        Style::new()
                    })
                }
                PipelineRow::Pipeline {
                    definition,
                    latest_build,
                    pinned,
                } => {
                    let row_style = if i == self.nav.index() {
                        theme::SELECTED
                    } else {
                        Style::new()
                    };

                    let (icon, icon_color) =
                        latest_build.as_ref().map_or(("○", Color::DarkGray), |b| {
                            let awaiting = app.data.pending_approval_build_ids.contains(&b.id);
                            effective_status_icon(b.status, b.result, awaiting)
                        });
                    let label = latest_build.as_ref().map_or("", |b| {
                        let awaiting = app.data.pending_approval_build_ids.contains(&b.id);
                        effective_status_label(b.status, b.result, awaiting)
                    });

                    let build_spans = latest_build.as_ref().map_or_else(
                        || vec![Span::styled("no builds", theme::MUTED)],
                        |b| {
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
                        },
                    );

                    let pin_indicator = if *pinned { "★ " } else { "" };
                    let selected_indicator = if self.selected.contains(&definition.id) {
                        "✓ "
                    } else {
                        "  "
                    };
                    let name_style = if latest_build.is_some() {
                        theme::TEXT
                    } else {
                        theme::MUTED
                    };

                    let mut spans = vec![
                        Span::styled(selected_indicator, theme::SUCCESS),
                        Span::styled(format!("{icon} "), Style::new().fg(icon_color)),
                        Span::styled(
                            format!("{:<width$}", label, width = widths[2]),
                            Style::new().fg(icon_color),
                        ),
                        Span::styled(
                            format!(
                                "{}{:<width$} ",
                                pin_indicator,
                                truncate(
                                    &definition.name,
                                    widths[3]
                                        .saturating_sub(1)
                                        .saturating_sub(pin_indicator.len()),
                                ),
                                width = widths[3]
                                    .saturating_sub(1)
                                    .saturating_sub(pin_indicator.len())
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
                .title(format!(" Pipelines ({}) ", self.pipeline_count()))
                .title_style(theme::TITLE),
        );

        let mut state = ListState::default();
        state.select(Some(self.nav.index()));
        f.render_stateful_widget(list, list_area, &mut state);
    }

    /// Returns the number of pipeline rows (excluding folder headers).
    fn pipeline_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| matches!(r, PipelineRow::Pipeline { .. }))
            .count()
    }
}

impl Component for Pipelines {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn footer_hints(&self) -> &'static str {
        "↑↓ navigate  ←→ collapse/expand  Enter drill-in  Space select  p pin  Q queue  o open  / search  ? help"
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::test_helpers::*;

    #[test]
    fn folder_key_root() {
        assert_eq!(folder_key(""), "\\");
        assert_eq!(folder_key("\\"), "\\");
    }

    #[test]
    fn folder_key_nested() {
        assert_eq!(folder_key("\\Infra"), "\\Infra");
    }

    #[test]
    fn folder_display_root() {
        assert_eq!(folder_display("\\"), "Root");
    }

    #[test]
    fn folder_display_nested() {
        assert_eq!(folder_display("\\Infra\\Deploy"), "Infra / Deploy");
    }

    #[test]
    fn rebuild_groups_by_folder() {
        let defs = vec![
            make_definition(1, "CI", "\\"),
            make_definition(2, "Deploy", "\\Infra"),
            make_definition(3, "Lint", "\\"),
        ];
        let mut p = Pipelines::default();
        p.rebuild(&defs, &BTreeMap::new(), &[], &[], &[], "");
        assert_eq!(p.rows.len(), 5);
        assert!(matches!(&p.rows[0], PipelineRow::FolderHeader { path, .. } if path == "Root"));
        assert!(matches!(&p.rows[3], PipelineRow::FolderHeader { path, .. } if path == "Infra"));
    }

    #[test]
    fn rebuild_search_filters_pipelines() {
        let defs = vec![
            make_definition(1, "CI Pipeline", "\\"),
            make_definition(2, "Deploy", "\\Infra"),
        ];
        let mut p = Pipelines::default();
        p.rebuild(&defs, &BTreeMap::new(), &[], &[], &[], "ci");
        assert_eq!(p.rows.len(), 2);
    }

    #[test]
    fn rebuild_search_auto_expands_folders() {
        let defs = vec![make_definition(1, "CI", "\\")];
        let mut p = Pipelines::default();
        p.collapsed_folders.insert("\\".to_string());
        p.rebuild(&defs, &BTreeMap::new(), &[], &[], &[], "CI");
        assert_eq!(p.rows.len(), 2);
    }

    #[test]
    fn rebuild_marks_pinned() {
        let defs = vec![
            make_definition(1, "CI", "\\"),
            make_definition(2, "Deploy", "\\"),
        ];
        let mut p = Pipelines::default();
        p.rebuild(&defs, &BTreeMap::new(), &[], &[], &[1], "");
        let pinned: Vec<bool> = p
            .rows
            .iter()
            .filter_map(|r| match r {
                PipelineRow::Pipeline { pinned, .. } => Some(*pinned),
                PipelineRow::FolderHeader { .. } => None,
            })
            .collect();
        assert_eq!(pinned, vec![true, false]);
    }

    #[test]
    fn toggle_folder_collapses_and_expands() {
        let defs = vec![
            make_definition(1, "CI", "\\"),
            make_definition(2, "Deploy", "\\"),
        ];
        let mut p = Pipelines::default();
        p.rebuild(&defs, &BTreeMap::new(), &[], &[], &[], "");
        assert_eq!(p.rows.len(), 3);

        p.toggle_folder_at(0, &defs);
        p.rebuild(&defs, &BTreeMap::new(), &[], &[], &[], "");
        assert_eq!(p.rows.len(), 1);

        p.toggle_folder_at(0, &defs);
        p.rebuild(&defs, &BTreeMap::new(), &[], &[], &[], "");
        assert_eq!(p.rows.len(), 3);
    }

    #[test]
    fn matches_filter_no_filters() {
        assert!(matches_filter(&make_definition(1, "P", "\\"), &[], &[]));
    }

    #[test]
    fn matches_filter_by_id() {
        assert!(matches_filter(&make_definition(1, "P", "\\"), &[1, 2], &[]));
        assert!(!matches_filter(
            &make_definition(99, "P", "\\"),
            &[1, 2],
            &[]
        ));
    }

    #[test]
    fn matches_filter_by_folder() {
        let folders = vec!["\\Infra".to_string()];
        assert!(matches_filter(
            &make_definition(1, "P", "\\Infra"),
            &[],
            &folders
        ));
        assert!(!matches_filter(
            &make_definition(2, "P", "\\"),
            &[],
            &folders
        ));
    }
}
