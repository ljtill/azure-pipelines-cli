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

impl App {
    /// Check if a definition passes the configured filters.
    fn matches_filter(&self, def: &PipelineDefinition) -> bool {
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

    /// Rebuild the dashboard rows from definitions + latest builds, grouped by folder.
    pub fn rebuild_dashboard_rows(&mut self) {
        let mut rows = Vec::new();
        let mut by_folder: BTreeMap<String, Vec<(PipelineDefinition, Option<Build>)>> =
            BTreeMap::new();

        for def in &self.definitions {
            if !self.matches_filter(def) {
                continue;
            }
            let folder = if def.path.is_empty() || def.path == "\\" {
                "\\".to_string()
            } else {
                def.path.clone()
            };
            let latest = self.latest_builds_by_def.get(&def.id).cloned();
            by_folder
                .entry(folder)
                .or_default()
                .push((def.clone(), latest));
        }

        for (folder, pipelines) in &by_folder {
            let display_path = folder.trim_start_matches('\\').replace('\\', " / ");
            let display_path = if display_path.is_empty() {
                "Root".to_string()
            } else {
                display_path
            };

            let collapsed = self.collapsed_folders.contains(folder);
            rows.push(DashboardRow::FolderHeader {
                path: display_path,
                collapsed,
            });

            if !collapsed {
                for (def, build) in pipelines {
                    rows.push(DashboardRow::Pipeline {
                        definition: def.clone(),
                        latest_build: build.clone().map(Box::new),
                    });
                }
            }
        }

        self.dashboard_rows = rows;
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
            let folder = if def.path.is_empty() || def.path == "\\" {
                "\\".to_string()
            } else {
                def.path.clone()
            };
            let display = folder.trim_start_matches('\\').replace('\\', " / ");
            let display = if display.is_empty() {
                "Root".to_string()
            } else {
                display
            };
            if display == display_path {
                return Some(folder);
            }
        }
        None
    }
}
