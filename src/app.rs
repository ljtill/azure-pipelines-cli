use std::collections::{BTreeMap, HashMap};

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

/// A row in the timeline tree view — Stage, Job, or Task.
#[derive(Debug, Clone)]
pub enum TimelineRow {
    Stage {
        id: String,
        name: String,
        state: Option<String>,
        result: Option<String>,
        collapsed: bool,
    },
    Job {
        id: String,
        name: String,
        state: Option<String>,
        result: Option<String>,
        collapsed: bool,
        parent_stage_id: String,
    },
    Task {
        name: String,
        state: Option<String>,
        result: Option<String>,
        log_id: Option<u32>,
        parent_job_id: String,
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
    pub timeline_rows: Vec<TimelineRow>,
    pub collapsed_stages: std::collections::HashSet<String>,
    pub collapsed_jobs: std::collections::HashSet<String>,
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
            timeline_rows: Vec::new(),
            collapsed_stages: std::collections::HashSet::new(),
            collapsed_jobs: std::collections::HashSet::new(),
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
                self.timeline_rows.clear();
                self.collapsed_stages.clear();
                self.collapsed_jobs.clear();
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
        self.timeline_rows.clear();
        self.collapsed_stages.clear();
        self.collapsed_jobs.clear();
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
            View::LogViewer => self.timeline_rows.len(),
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

    /// Pick the most relevant timeline task to auto-show logs for.
    /// Returns (row_index, log_id) if found. Ensures parent stage/job are expanded.
    pub fn auto_select_log_entry(&mut self) -> Option<(usize, u32)> {
        // Extract what we need from the timeline into owned values
        let timeline = self.build_timeline.as_ref()?;
        let tasks: Vec<(String, Option<String>, Option<String>, Option<String>, u32)> = timeline
            .records
            .iter()
            .filter(|r| r.record_type == "Task" && r.log.is_some())
            .map(|r| {
                (
                    r.name.clone(),
                    r.state.clone(),
                    r.result.clone(),
                    r.parent_id.clone(),
                    r.log.as_ref().unwrap().id,
                )
            })
            .collect();

        if tasks.is_empty() {
            return None;
        }

        let is_running = self
            .selected_build
            .as_ref()
            .is_some_and(|b| b.status == "inProgress" || b.status == "InProgress");

        // Find best task index: in-progress > failed > last
        let best_idx = if is_running {
            tasks
                .iter()
                .rposition(|t| t.1.as_deref() == Some("inProgress"))
                .or(Some(tasks.len() - 1))
        } else {
            tasks
                .iter()
                .rposition(|t| t.2.as_deref() == Some("failed"))
                .or(Some(tasks.len() - 1))
        };
        let best_idx = best_idx?;
        let (best_name, _, _, parent_job_id, log_id) = &tasks[best_idx];
        let log_id = *log_id;
        let best_name = best_name.clone();
        let parent_job_id = parent_job_id.clone();

        // Find the parent stage id through the job
        let parent_stage_id = parent_job_id.as_ref().and_then(|job_id| {
            self.build_timeline
                .as_ref()?
                .records
                .iter()
                .find(|r| r.id == *job_id)
                .and_then(|r| r.parent_id.clone())
        });

        // Ensure parent job and stage are expanded
        if let Some(job_id) = &parent_job_id {
            self.collapsed_jobs.remove(job_id.as_str());
        }
        if let Some(stage_id) = &parent_stage_id {
            self.collapsed_stages.remove(stage_id.as_str());
        }

        // Rebuild rows with the expanded state, then find the task's row index
        self.rebuild_timeline_rows();

        let row_idx = self
            .timeline_rows
            .iter()
            .position(|row| matches!(row, TimelineRow::Task { log_id: Some(lid), .. } if *lid == log_id))
            .or_else(|| {
                self.timeline_rows.iter().position(|row| {
                    matches!(row, TimelineRow::Task { name, parent_job_id: pjid, .. }
                        if parent_job_id.as_ref().is_some_and(|pid| pid == pjid) && *name == best_name)
                })
            });

        if let Some(idx) = row_idx {
            self.log_entries_index = idx;
            Some((idx, log_id))
        } else {
            None
        }
    }

    /// Build the timeline tree rows from the raw timeline records.
    pub fn rebuild_timeline_rows(&mut self) {
        let timeline = match &self.build_timeline {
            Some(t) => t,
            None => {
                self.timeline_rows.clear();
                return;
            }
        };

        // Index records by type and parentId
        let mut stages = Vec::new();
        let mut jobs_by_stage: HashMap<String, Vec<&crate::api::models::TimelineRecord>> =
            HashMap::new();
        let mut tasks_by_job: HashMap<String, Vec<&crate::api::models::TimelineRecord>> =
            HashMap::new();

        for rec in &timeline.records {
            match rec.record_type.as_str() {
                "Stage" if rec.parent_id.is_none() => {
                    stages.push(rec);
                }
                "Job" | "Phase" => {
                    if let Some(pid) = &rec.parent_id {
                        jobs_by_stage.entry(pid.clone()).or_default().push(rec);
                    }
                }
                "Task" => {
                    if let Some(pid) = &rec.parent_id {
                        tasks_by_job.entry(pid.clone()).or_default().push(rec);
                    }
                }
                _ => {}
            }
        }

        // Sort stages by order
        stages.sort_by_key(|s| s.order.unwrap_or(999));

        let mut rows = Vec::new();

        for stage in &stages {
            let stage_collapsed = self.collapsed_stages.contains(&stage.id);
            rows.push(TimelineRow::Stage {
                id: stage.id.clone(),
                name: stage.name.clone(),
                state: stage.state.clone(),
                result: stage.result.clone(),
                collapsed: stage_collapsed,
            });

            if stage_collapsed {
                continue;
            }

            // Jobs under this stage
            let mut jobs = jobs_by_stage
                .get(&stage.id)
                .cloned()
                .unwrap_or_default();
            jobs.sort_by_key(|j| j.order.unwrap_or(999));

            for job in &jobs {
                let job_collapsed = self.collapsed_jobs.contains(&job.id);
                rows.push(TimelineRow::Job {
                    id: job.id.clone(),
                    name: job.name.clone(),
                    state: job.state.clone(),
                    result: job.result.clone(),
                    collapsed: job_collapsed,
                    parent_stage_id: stage.id.clone(),
                });

                if job_collapsed {
                    continue;
                }

                // Tasks under this job
                let mut tasks = tasks_by_job
                    .get(&job.id)
                    .cloned()
                    .unwrap_or_default();
                tasks.sort_by_key(|t| t.order.unwrap_or(999));

                for task in &tasks {
                    rows.push(TimelineRow::Task {
                        name: task.name.clone(),
                        state: task.state.clone(),
                        result: task.result.clone(),
                        log_id: task.log.as_ref().map(|l| l.id),
                        parent_job_id: job.id.clone(),
                    });
                }
            }
        }

        self.timeline_rows = rows;
    }

    /// Toggle collapse for a timeline stage or job at the given row index.
    pub fn toggle_timeline_node(&mut self, index: usize) -> bool {
        if let Some(row) = self.timeline_rows.get(index) {
            match row {
                TimelineRow::Stage { id, .. } => {
                    let id = id.clone();
                    if self.collapsed_stages.contains(&id) {
                        self.collapsed_stages.remove(&id);
                    } else {
                        self.collapsed_stages.insert(id);
                    }
                    self.rebuild_timeline_rows();
                    return true;
                }
                TimelineRow::Job { id, .. } => {
                    let id = id.clone();
                    if self.collapsed_jobs.contains(&id) {
                        self.collapsed_jobs.remove(&id);
                    } else {
                        self.collapsed_jobs.insert(id);
                    }
                    self.rebuild_timeline_rows();
                    return true;
                }
                TimelineRow::Task { .. } => {}
            }
        }
        false
    }

    /// Collapse a timeline stage or job. Returns true if it was expanded.
    pub fn collapse_timeline_node(&mut self, index: usize) -> bool {
        if let Some(row) = self.timeline_rows.get(index) {
            match row {
                TimelineRow::Stage { id, collapsed, .. } if !collapsed => {
                    let id = id.clone();
                    self.collapsed_stages.insert(id);
                    self.rebuild_timeline_rows();
                    return true;
                }
                TimelineRow::Job { id, collapsed, .. } if !collapsed => {
                    let id = id.clone();
                    self.collapsed_jobs.insert(id);
                    self.rebuild_timeline_rows();
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    /// Expand a timeline stage or job. Returns true if it was collapsed.
    pub fn expand_timeline_node(&mut self, index: usize) -> bool {
        if let Some(row) = self.timeline_rows.get(index) {
            match row {
                TimelineRow::Stage { id, collapsed, .. } if *collapsed => {
                    let id = id.clone();
                    self.collapsed_stages.remove(&id);
                    self.rebuild_timeline_rows();
                    return true;
                }
                TimelineRow::Job { id, collapsed, .. } if *collapsed => {
                    let id = id.clone();
                    self.collapsed_jobs.remove(&id);
                    self.rebuild_timeline_rows();
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    /// Find the parent row index for a timeline row (job→stage, task→job).
    pub fn find_timeline_parent_index(&self, index: usize) -> Option<usize> {
        if let Some(row) = self.timeline_rows.get(index) {
            match row {
                TimelineRow::Task { parent_job_id, .. } => {
                    return self.timeline_rows.iter().enumerate().rev()
                        .take(index + 1)
                        .find(|(_, r)| matches!(r, TimelineRow::Job { id, .. } if id == parent_job_id))
                        .map(|(i, _)| i);
                }
                TimelineRow::Job { parent_stage_id, .. } => {
                    return self.timeline_rows.iter().enumerate().rev()
                        .take(index + 1)
                        .find(|(_, r)| matches!(r, TimelineRow::Stage { id, .. } if id == parent_stage_id))
                        .map(|(i, _)| i);
                }
                TimelineRow::Stage { .. } => {}
            }
        }
        None
    }

    /// Check what kind of timeline row is at the given index.
    pub fn timeline_row_kind(&self, index: usize) -> Option<&str> {
        self.timeline_rows.get(index).map(|row| match row {
            TimelineRow::Stage { .. } => "stage",
            TimelineRow::Job { .. } => "job",
            TimelineRow::Task { .. } => "task",
        })
    }

    /// Get the log_id for a Task timeline row at the given index.
    pub fn timeline_task_log_id(&self, index: usize) -> Option<u32> {
        if let Some(TimelineRow::Task { log_id, .. }) = self.timeline_rows.get(index) {
            *log_id
        } else {
            None
        }
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
