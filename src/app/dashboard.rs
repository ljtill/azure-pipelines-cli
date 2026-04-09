use std::collections::BTreeMap;

use crate::api::models::{Build, PipelineDefinition};

use super::App;

/// A row in the dashboard grouped view — either a folder header or a pipeline entry.
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

/// Normalize an ADO definition path to a canonical folder key.
/// Empty or `\\` paths become `\\`; everything else is kept as-is.
fn folder_key(path: &str) -> String {
    if path.is_empty() || path == "\\" {
        "\\".to_string()
    } else {
        path.to_string()
    }
}

/// Convert a raw folder key (e.g. `\\Infra\\Deploy`) to a display-friendly string.
fn folder_display(key: &str) -> String {
    let display = key.trim_start_matches('\\').replace('\\', " / ");
    if display.is_empty() {
        "Root".to_string()
    } else {
        display
    }
}

impl App {
    /// Check if a definition passes the configured filters.
    pub fn matches_filter(&self, def: &PipelineDefinition) -> bool {
        if !self.filter_definition_ids.is_empty() && !self.filter_definition_ids.contains(&def.id) {
            return false;
        }
        if !self.filter_folders.is_empty()
            && !self.filter_folders.iter().any(|f| def.path.starts_with(f))
        {
            return false;
        }
        true
    }

    /// Check if a build's definition passes the configured ID filter.
    ///
    /// Only `filter_definition_ids` is checked here. Folder filters are **not**
    /// applied because [`Build`] payloads from the ADO API do not include the
    /// definition's folder path — they only carry a [`BuildDefinitionRef`] with
    /// `id` and `name`. This means a folder-only filter config (no ID filter)
    /// will show *all* active builds regardless of which folder their definition
    /// lives in. If both folder and ID filters are set, only the ID filter
    /// narrows the Active Runs view.
    pub fn matches_build_filter(&self, build: &Build) -> bool {
        if !self.filter_definition_ids.is_empty()
            && !self.filter_definition_ids.contains(&build.definition.id)
        {
            return false;
        }
        true
    }

    /// Rebuild the dashboard rows from definitions + latest builds, grouped by folder.
    pub fn rebuild_dashboard_rows(&mut self) {
        let mut rows = Vec::new();
        let mut by_folder: BTreeMap<String, Vec<(PipelineDefinition, Option<Build>)>> =
            BTreeMap::new();

        for def in &self.definitions {
            if !self.matches_filter(def) {
                continue;
            }
            let folder = folder_key(&def.path);
            let latest = self.latest_builds_by_def.get(&def.id).cloned();
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

        self.dashboard_rows = rows;
        self.dashboard_nav.set_len(self.dashboard_rows.len());
    }

    /// Rebuild the filtered pipelines list from search query.
    pub fn rebuild_filtered_pipelines(&mut self) {
        let base = self.definitions.iter().filter(|d| self.matches_filter(d));

        if self.search_query.is_empty() {
            self.filtered_pipelines = base.cloned().collect();
        } else {
            let q = self.search_query.to_lowercase();
            self.filtered_pipelines = base
                .filter(|d| {
                    d.name.to_lowercase().contains(&q) || d.path.to_lowercase().contains(&q)
                })
                .cloned()
                .collect();
        }
        self.filtered_pipelines
            .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.pipelines_nav.set_len(self.filtered_pipelines.len());
    }

    /// Toggle collapse state for a folder at the given dashboard row index.
    pub fn toggle_folder_at(&mut self, index: usize) -> bool {
        if let Some(DashboardRow::FolderHeader { path, .. }) = self.dashboard_rows.get(index) {
            let folder_key = self.find_folder_key_for_display(path);
            if let Some(key) = folder_key {
                if self.collapsed_folders.contains(&key) {
                    self.collapsed_folders.remove(&key);
                } else {
                    self.collapsed_folders.insert(key);
                }
                self.rebuild_dashboard_rows();
                return true;
            }
        }
        false
    }

    /// Collapse the folder at the given dashboard index.
    pub fn collapse_folder_at(&mut self, index: usize) -> bool {
        if let Some(DashboardRow::FolderHeader {
            path, collapsed, ..
        }) = self.dashboard_rows.get(index)
            && !collapsed
        {
            let folder_key = self.find_folder_key_for_display(path);
            if let Some(key) = folder_key {
                self.collapsed_folders.insert(key);
                self.rebuild_dashboard_rows();
                return true;
            }
        }
        false
    }

    /// Expand the folder at the given dashboard index.
    pub fn expand_folder_at(&mut self, index: usize) -> bool {
        if let Some(DashboardRow::FolderHeader {
            path, collapsed, ..
        }) = self.dashboard_rows.get(index)
            && *collapsed
        {
            let folder_key = self.find_folder_key_for_display(path);
            if let Some(key) = folder_key {
                self.collapsed_folders.remove(&key);
                self.rebuild_dashboard_rows();
                return true;
            }
        }
        false
    }

    /// Find the dashboard row index of the parent folder for a pipeline row.
    pub fn find_parent_folder_index(&self, pipeline_index: usize) -> Option<usize> {
        for i in (0..pipeline_index).rev() {
            if let Some(DashboardRow::FolderHeader { .. }) = self.dashboard_rows.get(i) {
                return Some(i);
            }
        }
        None
    }

    /// Check if a dashboard row is a folder header.
    pub fn is_folder_header(&self, index: usize) -> bool {
        matches!(
            self.dashboard_rows.get(index),
            Some(DashboardRow::FolderHeader { .. })
        )
    }

    fn find_folder_key_for_display(&self, display_path: &str) -> Option<String> {
        for def in &self.definitions {
            let key = folder_key(&def.path);
            if folder_display(&key) == display_path {
                return Some(key);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::*;
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
        let app = App::new("o", "p", &make_config());
        let def = make_definition(1, "P", "\\");
        assert!(app.matches_filter(&def));
    }

    #[test]
    fn matches_filter_by_definition_id() {
        let mut cfg = make_config();
        cfg.filters.definition_ids = vec![1, 2];
        let app = App::new("o", "p", &cfg);
        assert!(app.matches_filter(&make_definition(1, "P", "\\")));
        assert!(!app.matches_filter(&make_definition(99, "P", "\\")));
    }

    #[test]
    fn matches_filter_by_folder() {
        let mut cfg = make_config();
        cfg.filters.folders = vec!["\\Infra".to_string()];
        let app = App::new("o", "p", &cfg);
        assert!(app.matches_filter(&make_definition(1, "P", "\\Infra")));
        assert!(app.matches_filter(&make_definition(2, "P", "\\Infra\\Deploy")));
        assert!(!app.matches_filter(&make_definition(3, "P", "\\")));
    }

    #[test]
    fn matches_build_filter_by_definition_id() {
        let mut cfg = make_config();
        cfg.filters.definition_ids = vec![1];
        let app = App::new("o", "p", &cfg);
        let mut build = make_build(1, BuildStatus::Completed, Some(BuildResult::Succeeded));
        build.definition.id = 1;
        assert!(app.matches_build_filter(&build));
        build.definition.id = 99;
        assert!(!app.matches_build_filter(&build));
    }

    // --- rebuild_dashboard_rows ---

    #[test]
    fn rebuild_dashboard_groups_by_folder() {
        let mut app = App::new("o", "p", &make_config());
        app.definitions = vec![
            make_definition(1, "CI", "\\"),
            make_definition(2, "Deploy", "\\Infra"),
            make_definition(3, "Lint", "\\"),
        ];
        app.rebuild_dashboard_rows();

        // Should have: Root folder header + 2 pipelines, then Infra folder header + 1 pipeline
        // BTreeMap sorts keys, so "\" comes before "\Infra"
        assert_eq!(app.dashboard_rows.len(), 5); // 2 headers + 3 pipelines
        assert!(
            matches!(&app.dashboard_rows[0], DashboardRow::FolderHeader { path, .. } if path == "Root")
        );
        assert!(
            matches!(&app.dashboard_rows[3], DashboardRow::FolderHeader { path, .. } if path == "Infra")
        );
    }

    // --- rebuild_filtered_pipelines ---

    #[test]
    fn rebuild_filtered_pipelines_with_search() {
        let mut app = App::new("o", "p", &make_config());
        app.definitions = vec![
            make_definition(1, "CI Pipeline", "\\"),
            make_definition(2, "Deploy", "\\Infra"),
        ];
        app.search_query = "ci".to_string();
        app.rebuild_filtered_pipelines();
        assert_eq!(app.filtered_pipelines.len(), 1);
        assert_eq!(app.filtered_pipelines[0].name, "CI Pipeline");
    }

    #[test]
    fn rebuild_filtered_pipelines_empty_search_shows_all() {
        let mut app = App::new("o", "p", &make_config());
        app.definitions = vec![
            make_definition(1, "CI", "\\"),
            make_definition(2, "Deploy", "\\Infra"),
        ];
        app.rebuild_filtered_pipelines();
        assert_eq!(app.filtered_pipelines.len(), 2);
    }

    // --- toggle/collapse/expand ---

    #[test]
    fn toggle_folder_collapses_and_expands() {
        let mut app = App::new("o", "p", &make_config());
        app.definitions = vec![
            make_definition(1, "CI", "\\"),
            make_definition(2, "Deploy", "\\"),
        ];
        app.rebuild_dashboard_rows();
        // Row 0 is Root folder header (expanded), rows 1-2 are pipelines
        assert_eq!(app.dashboard_rows.len(), 3);

        app.toggle_folder_at(0); // collapse
        assert_eq!(app.dashboard_rows.len(), 1); // only header

        app.toggle_folder_at(0); // expand
        assert_eq!(app.dashboard_rows.len(), 3);
    }

    // --- matches_build_filter / folder limitation ---

    #[test]
    fn folder_filter_does_not_restrict_active_builds() {
        // A folder-only filter hides definitions outside the folder in the
        // Dashboard/Pipelines views, but cannot restrict Active Runs because
        // builds don't carry definition folder paths.
        let mut cfg = make_config();
        cfg.filters.folders = vec!["\\Infra".to_string()];
        // No definition_ids filter — only folder filter is active.
        let mut app = App::new("o", "p", &cfg);

        let mut build = make_build(1, BuildStatus::InProgress, None);
        build.definition.id = 99; // not in any ID allowlist
        build.definition.name = "Outside Infra".to_string();
        app.active_builds = vec![build];

        app.rebuild_filtered_active_builds();

        // Build should still appear because folder filters can't apply to builds.
        assert_eq!(app.filtered_active_builds.len(), 1);
        assert_eq!(app.filtered_active_builds[0].definition.id, 99);
    }

    // --- rebuild_filtered_active_builds ---

    #[test]
    fn rebuild_filtered_active_builds_applies_search() {
        let mut app = App::new("o", "p", &make_config());
        let mut b1 = make_build(1, BuildStatus::Completed, Some(BuildResult::Succeeded));
        b1.definition.name = "CI".to_string();
        b1.status = BuildStatus::InProgress;
        let mut b2 = make_build(2, BuildStatus::Completed, Some(BuildResult::Succeeded));
        b2.definition.name = "Deploy".to_string();
        b2.status = BuildStatus::InProgress;
        app.active_builds = vec![b1, b2];

        app.search_query = "deploy".to_string();
        app.rebuild_filtered_active_builds();
        assert_eq!(app.filtered_active_builds.len(), 1);
        assert_eq!(app.filtered_active_builds[0].definition.name, "Deploy");
    }
}
