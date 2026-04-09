mod dashboard;
mod messages;
mod timeline;

pub use dashboard::DashboardRow;
pub use messages::AppMessage;
pub use timeline::TimelineRow;

use std::collections::{BTreeMap, HashSet};

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

/// Action pending user confirmation (y/n).
#[derive(Debug, Clone)]
pub enum ConfirmAction {
    CancelBuild { build_id: u32 },
    CancelBuilds { build_ids: Vec<u32> },
    RetryStage { build_id: u32, stage_ref_name: String },
    QueuePipeline { definition_id: u32 },
}

/// A pending confirmation prompt shown in the footer.
#[derive(Debug, Clone)]
pub struct ConfirmPrompt {
    pub message: String,
    pub action: ConfirmAction,
}

pub struct App {
    pub view: View,
    pub previous_view: Option<View>,
    pub input_mode: InputMode,
    pub running: bool,
    pub show_help: bool,
    pub org_project_label: String,
    web_base_url: String,

    // Filters
    pub filter_folders: Vec<String>,
    pub filter_definition_ids: Vec<u32>,

    // Data
    pub definitions: Vec<PipelineDefinition>,
    pub recent_builds: Vec<Build>,
    pub active_builds: Vec<Build>,
    pub latest_builds_by_def: BTreeMap<u32, Build>,
    pub dashboard_rows: Vec<DashboardRow>,
    pub collapsed_folders: HashSet<String>,

    // Build history (for selected pipeline)
    pub selected_definition: Option<PipelineDefinition>,
    pub definition_builds: Vec<Build>,

    // Log viewer
    pub selected_build: Option<Build>,
    pub build_timeline: Option<BuildTimeline>,
    pub timeline_rows: Vec<TimelineRow>,
    pub collapsed_stages: HashSet<String>,
    pub collapsed_jobs: HashSet<String>,
    pub log_content: Vec<String>,
    pub log_auto_scroll: bool,
    pub log_generation: u64,
    pub timeline_initialized: bool,
    pub follow_mode: bool,
    pub followed_task_name: String,
    pub followed_log_id: Option<u32>,

    // Confirmation prompt
    pub confirm_prompt: Option<ConfirmPrompt>,

    // Multi-select (Active Runs)
    pub selected_builds: HashSet<u32>,

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
    pub fn new(organization: &str, project: &str, config: &crate::config::Config) -> Self {
        Self {
            view: View::Dashboard,
            previous_view: None,
            input_mode: InputMode::Normal,
            running: true,
            show_help: false,
            org_project_label: format!("{} / {}", organization, project),
            web_base_url: format!("https://dev.azure.com/{}/{}", organization, project),
            filter_folders: config.filters.folders.clone(),
            filter_definition_ids: config.filters.definition_ids.clone(),

            definitions: Vec::new(),
            recent_builds: Vec::new(),
            active_builds: Vec::new(),
            latest_builds_by_def: BTreeMap::new(),
            dashboard_rows: Vec::new(),
            collapsed_folders: HashSet::new(),

            selected_definition: None,
            definition_builds: Vec::new(),

            selected_build: None,
            build_timeline: None,
            timeline_rows: Vec::new(),
            collapsed_stages: HashSet::new(),
            collapsed_jobs: HashSet::new(),
            log_content: Vec::new(),
            log_auto_scroll: true,
            log_generation: 0,
            timeline_initialized: false,
            follow_mode: true,
            followed_task_name: String::new(),
            followed_log_id: None,

            confirm_prompt: None,

            selected_builds: HashSet::new(),

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
                self.log_generation += 1;
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
        self.log_generation += 1;
        self.timeline_initialized = false;
        self.follow_mode = true;
        self.followed_task_name.clear();
        self.followed_log_id = None;
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

    // Web URL helpers for opening in browser

    pub fn endpoints_web_build(&self, build_id: u32) -> String {
        format!("{}/_build/results?buildId={}", self.web_base_url, build_id)
    }

    pub fn endpoints_web_definition(&self, definition_id: u32) -> String {
        format!(
            "{}/_build?definitionId={}",
            self.web_base_url, definition_id
        )
    }
}
