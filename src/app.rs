use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

use crate::api::models::{Build, BuildTimeline, PipelineDefinition};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Dashboard,
    Pipelines,
    ActiveRuns,
    BuildHistory,
    LogViewer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Search,
}

/// A row in the dashboard grouped view — either a folder header or a pipeline entry.
#[derive(Debug, Clone)]
pub enum DashboardRow {
    FolderHeader {
        path: String,
        collapsed: bool,
    },
    Pipeline {
        definition: PipelineDefinition,
        latest_build: Option<Build>,
    },
}

pub struct App {
    pub view: View,
    pub previous_view: Option<View>,
    pub input_mode: InputMode,
    pub running: bool,
    pub show_help: bool,

    // Data
    pub definitions: Vec<PipelineDefinition>,
    pub recent_builds: Vec<Build>,
    pub active_builds: Vec<Build>,
    pub latest_builds_by_def: BTreeMap<u32, Build>,
    pub dashboard_rows: Vec<DashboardRow>,
    pub collapsed_folders: std::collections::HashSet<String>,

    // Build history (for selected pipeline)
    pub selected_definition: Option<PipelineDefinition>,
    pub definition_builds: Vec<Build>,

    // Log viewer
    pub selected_build: Option<Build>,
    pub build_timeline: Option<BuildTimeline>,
    pub log_content: Vec<String>,
    pub log_auto_scroll: bool,

    // List state indices
    pub dashboard_index: usize,
    pub pipelines_index: usize,
    pub active_runs_index: usize,
    pub builds_index: usize,
    pub log_entries_index: usize,
    pub log_scroll_offset: u16,

    // Search
    pub search_query: String,
    pub filtered_pipelines: Vec<PipelineDefinition>,

    // Status
    pub last_refresh: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
    pub loading: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            view: View::Dashboard,
            previous_view: None,
            input_mode: InputMode::Normal,
            running: true,
            show_help: false,

            definitions: Vec::new(),
            recent_builds: Vec::new(),
            active_builds: Vec::new(),
            latest_builds_by_def: BTreeMap::new(),
            dashboard_rows: Vec::new(),
            collapsed_folders: std::collections::HashSet::new(),

            selected_definition: None,
            definition_builds: Vec::new(),

            selected_build: None,
            build_timeline: None,
            log_content: Vec::new(),
            log_auto_scroll: true,

            dashboard_index: 0,
            pipelines_index: 0,
            active_runs_index: 0,
            builds_index: 0,
            log_entries_index: 0,
            log_scroll_offset: 0,

            search_query: String::new(),
            filtered_pipelines: Vec::new(),

            last_refresh: None,
            error_message: None,
            loading: false,
        }
    }

    /// Rebuild the dashboard rows from definitions + latest builds, grouped by folder.
    pub fn rebuild_dashboard_rows(&mut self) {
        let mut rows = Vec::new();
        let mut by_folder: BTreeMap<String, Vec<(PipelineDefinition, Option<Build>)>> =
            BTreeMap::new();

        for def in &self.definitions {
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
            let display_path = folder
                .trim_start_matches('\\')
                .replace('\\', " / ");
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
                        latest_build: build.clone(),
                    });
                }
            }
        }

        self.dashboard_rows = rows;
    }

    /// Rebuild the filtered pipelines list from search query.
    pub fn rebuild_filtered_pipelines(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_pipelines = self.definitions.clone();
        } else {
            let q = self.search_query.to_lowercase();
            self.filtered_pipelines = self
                .definitions
                .iter()
                .filter(|d| d.name.to_lowercase().contains(&q) || d.path.to_lowercase().contains(&q))
                .cloned()
                .collect();
        }
    }

    pub fn go_back(&mut self) {
        if self.show_help {
            self.show_help = false;
            return;
        }
        if self.input_mode == InputMode::Search {
            self.input_mode = InputMode::Normal;
            self.search_query.clear();
            self.rebuild_filtered_pipelines();
            return;
        }
        match self.view {
            View::LogViewer => {
                self.view = View::BuildHistory;
                self.selected_build = None;
                self.build_timeline = None;
                self.log_content.clear();
                self.log_entries_index = 0;
                self.log_scroll_offset = 0;
            }
            View::BuildHistory => {
                self.view = self.previous_view.unwrap_or(View::Dashboard);
                self.selected_definition = None;
                self.definition_builds.clear();
                self.builds_index = 0;
            }
            _ => {}
        }
    }

    pub fn navigate_to_build_history(&mut self, def: PipelineDefinition) {
        self.previous_view = Some(self.view);
        self.selected_definition = Some(def);
        self.definition_builds.clear();
        self.builds_index = 0;
        self.view = View::BuildHistory;
    }

    pub fn navigate_to_log_viewer(&mut self, build: Build) {
        self.selected_build = Some(build);
        self.build_timeline = None;
        self.log_content.clear();
        self.log_entries_index = 0;
        self.log_scroll_offset = 0;
        self.log_auto_scroll = true;
        self.view = View::LogViewer;
    }

    pub fn current_list_len(&self) -> usize {
        match self.view {
            View::Dashboard => self.dashboard_rows.len(),
            View::Pipelines => self.filtered_pipelines.len(),
            View::ActiveRuns => self.active_builds.len(),
            View::BuildHistory => self.definition_builds.len(),
            View::LogViewer => {
                self.build_timeline
                    .as_ref()
                    .map(|t| t.records.iter().filter(|r| r.log.is_some()).count())
                    .unwrap_or(0)
            }
        }
    }

    pub fn current_index(&self) -> usize {
        match self.view {
            View::Dashboard => self.dashboard_index,
            View::Pipelines => self.pipelines_index,
            View::ActiveRuns => self.active_runs_index,
            View::BuildHistory => self.builds_index,
            View::LogViewer => self.log_entries_index,
        }
    }

    pub fn set_current_index(&mut self, idx: usize) {
        let max = self.current_list_len().saturating_sub(1);
        let clamped = idx.min(max);
        match self.view {
            View::Dashboard => self.dashboard_index = clamped,
            View::Pipelines => self.pipelines_index = clamped,
            View::ActiveRuns => self.active_runs_index = clamped,
            View::BuildHistory => self.builds_index = clamped,
            View::LogViewer => self.log_entries_index = clamped,
        }
    }

    pub fn move_up(&mut self) {
        let idx = self.current_index();
        if idx > 0 {
            self.set_current_index(idx - 1);
        }
    }

    pub fn move_down(&mut self) {
        let idx = self.current_index();
        self.set_current_index(idx + 1);
    }

    /// Pick the most relevant timeline step to auto-show logs for.
    /// Returns (step_index_in_filtered_list, log_id) if found.
    pub fn auto_select_log_entry(&mut self) -> Option<(usize, u32)> {
        let timeline = self.build_timeline.as_ref()?;
        let log_records: Vec<_> = timeline
            .records
            .iter()
            .filter(|r| r.log.is_some())
            .collect();

        if log_records.is_empty() {
            return None;
        }

        let is_running = self
            .selected_build
            .as_ref()
            .is_some_and(|b| b.status == "inProgress" || b.status == "InProgress");

        // 1. In-progress build: find the last currently-running task
        if is_running {
            if let Some((i, rec)) = log_records
                .iter()
                .enumerate()
                .rev()
                .find(|(_, r)| r.state.as_deref() == Some("inProgress"))
            {
                self.log_entries_index = i;
                return Some((i, rec.log.as_ref().unwrap().id));
            }
        }

        // 2. Failed build: find the last failed task
        if let Some((i, rec)) = log_records
            .iter()
            .enumerate()
            .rev()
            .find(|(_, r)| r.result.as_deref() == Some("failed"))
        {
            self.log_entries_index = i;
            return Some((i, rec.log.as_ref().unwrap().id));
        }

        // 3. Fallback: last task with a log
        let i = log_records.len() - 1;
        let rec = log_records[i];
        self.log_entries_index = i;
        Some((i, rec.log.as_ref().unwrap().id))
    }

    /// Toggle collapse state for a folder at the given dashboard row index.
    /// Returns true if the row was a folder header that was toggled.
    pub fn toggle_folder_at(&mut self, index: usize) -> bool {
        if let Some(row) = self.dashboard_rows.get(index) {
            if let DashboardRow::FolderHeader { path, .. } = row {
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
        }
        false
    }

    /// Collapse the folder at the given dashboard index. Returns true if it was expanded and is now collapsed.
    pub fn collapse_folder_at(&mut self, index: usize) -> bool {
        if let Some(row) = self.dashboard_rows.get(index) {
            if let DashboardRow::FolderHeader { path, collapsed, .. } = row {
                if !collapsed {
                    let folder_key = self.find_folder_key_for_display(path);
                    if let Some(key) = folder_key {
                        self.collapsed_folders.insert(key);
                        self.rebuild_dashboard_rows();
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Expand the folder at the given dashboard index. Returns true if it was collapsed and is now expanded.
    pub fn expand_folder_at(&mut self, index: usize) -> bool {
        if let Some(row) = self.dashboard_rows.get(index) {
            if let DashboardRow::FolderHeader { path, collapsed, .. } = row {
                if *collapsed {
                    let folder_key = self.find_folder_key_for_display(path);
                    if let Some(key) = folder_key {
                        self.collapsed_folders.remove(&key);
                        self.rebuild_dashboard_rows();
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Find the dashboard row index of the parent folder for a pipeline row.
    pub fn find_parent_folder_index(&self, pipeline_index: usize) -> Option<usize> {
        // Walk backwards from the pipeline row to find the nearest folder header
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
