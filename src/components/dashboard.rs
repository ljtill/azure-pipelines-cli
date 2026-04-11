//! Dashboard view component showing pipeline status grouped by folder.

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
    build_elapsed, effective_status_icon, effective_status_label, truncate,
};
use crate::render::theme;
use crate::state::App;
use crate::state::nav::ListNav;

/// Represents a row in the dashboard grouped view — either a folder header or a pipeline entry.
#[derive(Debug, Clone)]
pub enum DashboardRow {
    FolderHeader {
        path: String,
        collapsed: bool,
    },
    Pipeline {
        definition: PipelineDefinition,
        latest_build: Option<Box<Build>>,
    },
}

/// Normalizes an ADO definition path to a canonical folder key.
/// Empty or `\\` paths become `\\`; everything else is kept as-is.
fn folder_key(path: &str) -> String {
    if path.is_empty() || path == "\\" {
        "\\".to_string()
    } else {
        path.to_string()
    }
}

/// Converts a raw folder key (e.g. `\\Infra\\Deploy`) to a display-friendly string.
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

/// Renders pipelines grouped by folder with collapse/expand.
#[derive(Debug, Default)]
pub struct Dashboard {
    pub rows: Vec<DashboardRow>,
    pub collapsed_folders: HashSet<String>,
    pub nav: ListNav,
}

impl Dashboard {
    /// Rebuilds the dashboard rows from definitions + latest builds, grouped by folder.
    pub fn rebuild(
        &mut self,
        definitions: &[PipelineDefinition],
        latest_builds_by_def: &BTreeMap<u32, Build>,
        filter_folders: &[String],
        filter_definition_ids: &[u32],
    ) {
        let mut rows = Vec::new();
        let mut by_folder: BTreeMap<String, Vec<(PipelineDefinition, Option<Build>)>> =
            BTreeMap::new();

        for def in definitions {
            if !matches_filter(def, filter_definition_ids, filter_folders) {
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
            let collapsed = self.collapsed_folders.contains(&key);
            rows.push(DashboardRow::FolderHeader {
                path: folder_display(&key),
                collapsed,
            });

            if !collapsed {
                for (def, build) in &pipelines {
                    rows.push(DashboardRow::Pipeline {
                        definition: def.clone(),
                        latest_build: build.clone().map(Box::new),
                    });
                }
            }
        }

        self.rows = rows;
        self.nav.set_len(self.rows.len());
    }

    /// Toggles collapse state for a folder at the given dashboard row index.
    pub fn toggle_folder_at(&mut self, index: usize, definitions: &[PipelineDefinition]) -> bool {
        if let Some(DashboardRow::FolderHeader { path, .. }) = self.rows.get(index) {
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

    /// Collapses the folder at the given dashboard index.
    pub fn collapse_folder_at(&mut self, index: usize, definitions: &[PipelineDefinition]) -> bool {
        if let Some(DashboardRow::FolderHeader {
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

    /// Expands the folder at the given dashboard index.
    pub fn expand_folder_at(&mut self, index: usize, definitions: &[PipelineDefinition]) -> bool {
        if let Some(DashboardRow::FolderHeader {
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

    /// Finds the dashboard row index of the parent folder for a pipeline row.
    pub fn find_parent_folder_index(&self, pipeline_index: usize) -> Option<usize> {
        for i in (0..pipeline_index).rev() {
            if let Some(DashboardRow::FolderHeader { .. }) = self.rows.get(i) {
                return Some(i);
            }
        }
        None
    }

    /// Checks if a dashboard row is a folder header.
    pub fn is_folder_header(&self, index: usize) -> bool {
        matches!(
            self.rows.get(index),
            Some(DashboardRow::FolderHeader { .. })
        )
    }

    pub fn draw_with_app(&self, f: &mut Frame, app: &App, area: Rect) {
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
        .split(area);
        let mut widths: Vec<usize> = rects.iter().map(|r| r.width as usize).collect();
        widths[3] = widths[3].min(40);
        widths[5] = widths[5].min(35);
        widths[6] = widths[6].min(35);

        let items: Vec<ListItem> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| match row {
                DashboardRow::FolderHeader { path, collapsed } => {
                    let icon = if *collapsed { "▸" } else { "▾" };
                    ListItem::new(Line::from(vec![
                        Span::styled(format!(" {} ", icon), theme::ARROW),
                        Span::styled(path.to_string(), theme::FOLDER),
                    ]))
                    .style(if i == self.nav.index() {
                        theme::SELECTED
                    } else {
                        Style::new()
                    })
                }
                DashboardRow::Pipeline {
                    definition,
                    latest_build,
                } => {
                    let row_style = if i == self.nav.index() {
                        theme::SELECTED
                    } else {
                        Style::new()
                    };

                    let (icon, icon_color) = match latest_build {
                        Some(b) => {
                            let awaiting = app.data.pending_approval_build_ids.contains(&b.id);
                            effective_status_icon(b.status, b.result, awaiting)
                        }
                        None => ("○", Color::DarkGray),
                    };
                    let label = match latest_build {
                        Some(b) => {
                            let awaiting = app.data.pending_approval_build_ids.contains(&b.id);
                            effective_status_label(b.status, b.result, awaiting)
                        }
                        None => "",
                    };

                    let build_spans = if let Some(b) = latest_build {
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
                    } else {
                        vec![Span::styled("no builds", theme::MUTED)]
                    };

                    let name_style = if latest_build.is_some() {
                        theme::TEXT
                    } else {
                        theme::MUTED
                    };

                    let mut spans = vec![
                        Span::raw("    "),
                        Span::styled(format!("{} ", icon), Style::new().fg(icon_color)),
                        Span::styled(
                            format!("{:<width$}", label, width = widths[2]),
                            Style::new().fg(icon_color),
                        ),
                        Span::styled(
                            format!(
                                "{:<width$} ",
                                truncate(&definition.name, widths[3].saturating_sub(1)),
                                width = widths[3].saturating_sub(1)
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
                .title(" Dashboard — Pipelines by Folder ")
                .title_style(theme::TITLE),
        );

        let mut state = ListState::default();
        state.select(Some(self.nav.index()));
        f.render_stateful_widget(list, area, &mut state);
    }
}

impl Component for Dashboard {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn footer_hints(&self) -> &str {
        "↑↓ navigate  ←→ collapse/expand  Enter drill-in  Q queue  o open  1/2/3 tabs  r refresh  , settings  ? help  q quit"
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;
    use crate::client::models::*;
    use crate::state::App;
    use crate::test_helpers::*;

    // --- folder_key / folder_display ---

    #[test]
    fn folder_key_root() {
        assert_eq!(folder_key(""), "\\");
        assert_eq!(folder_key("\\"), "\\");
    }

    #[test]
    fn folder_key_nested() {
        assert_eq!(folder_key("\\Infra"), "\\Infra");
        assert_eq!(folder_key("\\Infra\\Deploy"), "\\Infra\\Deploy");
    }

    #[test]
    fn folder_display_root() {
        assert_eq!(folder_display("\\"), "Root");
    }

    #[test]
    fn folder_display_nested() {
        assert_eq!(folder_display("\\Infra"), "Infra");
        assert_eq!(folder_display("\\Infra\\Deploy"), "Infra / Deploy");
    }

    // --- matches_filter ---

    #[test]
    fn matches_filter_no_filters_passes_all() {
        let def = make_definition(1, "P", "\\");
        assert!(matches_filter(&def, &[], &[]));
    }

    #[test]
    fn matches_filter_by_definition_id() {
        let ids = vec![1u32, 2];
        assert!(matches_filter(&make_definition(1, "P", "\\"), &ids, &[]));
        assert!(!matches_filter(&make_definition(99, "P", "\\"), &ids, &[]));
    }

    #[test]
    fn matches_filter_by_folder() {
        let folders = vec!["\\Infra".to_string()];
        assert!(matches_filter(
            &make_definition(1, "P", "\\Infra"),
            &[],
            &folders
        ));
        assert!(matches_filter(
            &make_definition(2, "P", "\\Infra\\Deploy"),
            &[],
            &folders
        ));
        assert!(!matches_filter(
            &make_definition(3, "P", "\\"),
            &[],
            &folders
        ));
    }

    // --- rebuild ---

    #[test]
    fn rebuild_dashboard_groups_by_folder() {
        let definitions = vec![
            make_definition(1, "CI", "\\"),
            make_definition(2, "Deploy", "\\Infra"),
            make_definition(3, "Lint", "\\"),
        ];
        let mut state = Dashboard::default();
        state.rebuild(&definitions, &BTreeMap::new(), &[], &[]);

        // Should have: Root folder header + 2 pipelines, then Infra folder header + 1 pipeline.
        // BTreeMap sorts keys, so "\" comes before "\Infra".
        assert_eq!(state.rows.len(), 5); // 2 headers + 3 pipelines.
        assert!(
            matches!(&state.rows[0], DashboardRow::FolderHeader { path, .. } if path == "Root")
        );
        assert!(
            matches!(&state.rows[3], DashboardRow::FolderHeader { path, .. } if path == "Infra")
        );
    }

    // --- Pipelines::rebuild ---

    #[test]
    fn rebuild_filtered_pipelines_with_search() {
        let mut app = App::new("o", "p", &make_config(), PathBuf::from("/tmp/test.toml"));
        app.data.definitions = vec![
            make_definition(1, "CI Pipeline", "\\"),
            make_definition(2, "Deploy", "\\Infra"),
        ];
        app.search.query = "ci".to_string();
        app.pipelines.rebuild(
            &app.data.definitions,
            &app.filters.folders,
            &app.filters.definition_ids,
            &app.search.query,
        );
        assert_eq!(app.pipelines.filtered.len(), 1);
        assert_eq!(app.pipelines.filtered[0].name, "CI Pipeline");
    }

    #[test]
    fn rebuild_filtered_pipelines_empty_search_shows_all() {
        let mut app = App::new("o", "p", &make_config(), PathBuf::from("/tmp/test.toml"));
        app.data.definitions = vec![
            make_definition(1, "CI", "\\"),
            make_definition(2, "Deploy", "\\Infra"),
        ];
        app.pipelines.rebuild(
            &app.data.definitions,
            &app.filters.folders,
            &app.filters.definition_ids,
            &app.search.query,
        );
        assert_eq!(app.pipelines.filtered.len(), 2);
    }

    // --- toggle/collapse/expand ---

    #[test]
    fn toggle_folder_collapses_and_expands() {
        let definitions = vec![
            make_definition(1, "CI", "\\"),
            make_definition(2, "Deploy", "\\"),
        ];
        let mut state = Dashboard::default();
        state.rebuild(&definitions, &BTreeMap::new(), &[], &[]);
        // Row 0 is Root folder header (expanded), rows 1-2 are pipelines.
        assert_eq!(state.rows.len(), 3);

        state.toggle_folder_at(0, &definitions); // collapse
        state.rebuild(&definitions, &BTreeMap::new(), &[], &[]);
        assert_eq!(state.rows.len(), 1); // only header

        state.toggle_folder_at(0, &definitions); // expand
        state.rebuild(&definitions, &BTreeMap::new(), &[], &[]);
        assert_eq!(state.rows.len(), 3);
    }

    // --- folder filter does not restrict active builds ---

    #[test]
    fn folder_filter_does_not_restrict_active_builds() {
        let mut cfg = make_config();
        cfg.filters.folders = vec!["\\Infra".to_string()];
        let mut app = App::new("o", "p", &cfg, PathBuf::from("/tmp/test.toml"));

        let mut build = make_build(1, BuildStatus::InProgress, None);
        build.definition.id = 99;
        build.definition.name = "Outside Infra".to_string();
        app.data.active_builds = vec![build];

        app.active_runs.rebuild(
            &app.data.active_builds,
            &app.filters.definition_ids,
            &app.search.query,
        );

        assert_eq!(app.active_runs.filtered.len(), 1);
        assert_eq!(app.active_runs.filtered[0].definition.id, 99);
    }

    // --- rebuild_filtered_active_builds ---

    #[test]
    fn rebuild_filtered_active_builds_applies_search() {
        let mut app = App::new("o", "p", &make_config(), PathBuf::from("/tmp/test.toml"));
        let mut b1 = make_build(1, BuildStatus::Completed, Some(BuildResult::Succeeded));
        b1.definition.name = "CI".to_string();
        b1.status = BuildStatus::InProgress;
        let mut b2 = make_build(2, BuildStatus::Completed, Some(BuildResult::Succeeded));
        b2.definition.name = "Deploy".to_string();
        b2.status = BuildStatus::InProgress;
        app.data.active_builds = vec![b1, b2];

        app.search.query = "deploy".to_string();
        app.active_runs.rebuild(
            &app.data.active_builds,
            &app.filters.definition_ids,
            &app.search.query,
        );
        assert_eq!(app.active_runs.filtered.len(), 1);
        assert_eq!(app.active_runs.filtered[0].definition.name, "Deploy");
    }
}
