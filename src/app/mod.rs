pub mod actions;
mod dashboard;
mod messages;
pub mod run;
mod timeline;

pub use dashboard::DashboardRow;
pub use timeline::TimelineRow;

use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, Utc};

use crate::api::models::{Approval, Build, BuildTimeline, PipelineDefinition};

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
    CancelBuild {
        build_id: u32,
    },
    CancelBuilds {
        build_ids: Vec<u32>,
    },
    RetryStage {
        build_id: u32,
        stage_ref_name: String,
    },
    QueuePipeline {
        definition_id: u32,
    },
    ApproveCheck {
        approval_id: String,
    },
    RejectCheck {
        approval_id: String,
    },
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
    pub pending_approvals: Vec<Approval>,
    pub latest_builds_by_def: BTreeMap<u32, Build>,
    pub dashboard_rows: Vec<DashboardRow>,
    pub collapsed_folders: HashSet<String>,

    // Build history (for selected pipeline)
    pub selected_definition: Option<PipelineDefinition>,
    pub definition_builds: Vec<Build>,

    // Log viewer state (grouped)
    pub log_viewer: LogViewerState,

    // Confirmation prompt
    pub confirm_prompt: Option<ConfirmPrompt>,

    // Multi-select (Active Runs)
    pub selected_builds: HashSet<u32>,

    // List state indices
    pub dashboard_index: usize,
    pub pipelines_index: usize,
    pub active_runs_index: usize,
    pub builds_index: usize,

    // Search
    pub search_query: String,
    pub filtered_pipelines: Vec<PipelineDefinition>,
    pub filtered_active_builds: Vec<Build>,

    // Status
    pub last_refresh: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
    pub loading: bool,
}

/// State for the log viewer screen — reset as a unit on navigation.
pub struct LogViewerState {
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
    pub log_entries_index: usize,
    pub log_scroll_offset: u16,
    /// The view to return to when pressing Esc from LogViewer.
    pub return_to_view: View,
}

impl Default for LogViewerState {
    fn default() -> Self {
        Self {
            selected_build: None,
            build_timeline: None,
            timeline_rows: Vec::new(),
            collapsed_stages: HashSet::new(),
            collapsed_jobs: HashSet::new(),
            log_content: Vec::new(),
            log_auto_scroll: false,
            log_generation: 0,
            timeline_initialized: false,
            follow_mode: false,
            followed_task_name: String::new(),
            followed_log_id: None,
            log_entries_index: 0,
            log_scroll_offset: 0,
            return_to_view: View::BuildHistory,
        }
    }
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
            pending_approvals: Vec::new(),
            latest_builds_by_def: BTreeMap::new(),
            dashboard_rows: Vec::new(),
            collapsed_folders: HashSet::new(),

            selected_definition: None,
            definition_builds: Vec::new(),

            log_viewer: LogViewerState::default(),

            confirm_prompt: None,

            selected_builds: HashSet::new(),

            dashboard_index: 0,
            pipelines_index: 0,
            active_runs_index: 0,
            builds_index: 0,

            search_query: String::new(),
            filtered_pipelines: Vec::new(),
            filtered_active_builds: Vec::new(),

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
                let return_to = self.log_viewer.return_to_view;
                let next_gen = self.log_viewer.log_generation + 1;
                self.log_viewer = LogViewerState::default();
                self.log_viewer.log_generation = next_gen;

                match return_to {
                    View::BuildHistory => {
                        self.view = View::BuildHistory;
                    }
                    _ => {
                        // Returning to a top-level view — clean up build history state
                        self.view = return_to;
                        self.selected_definition = None;
                        self.definition_builds.clear();
                        self.builds_index = 0;
                    }
                }
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

    /// Update `selected_build` status/result from timeline records.
    /// Called on each timeline refresh so the log viewer header stays current.
    pub fn refresh_build_status_from_timeline(&mut self) {
        use crate::api::models::{BuildResult, BuildStatus, TaskState};

        let timeline = match &self.log_viewer.build_timeline {
            Some(t) => t,
            None => return,
        };
        let build = match &mut self.log_viewer.selected_build {
            Some(b) => b,
            None => return,
        };

        // If all root stages are completed, the build is completed
        let stages: Vec<_> = timeline
            .records
            .iter()
            .filter(|r| r.record_type == "Stage" && r.parent_id.is_none())
            .collect();

        if stages.is_empty() {
            return;
        }

        let all_completed = stages.iter().all(|s| s.state == Some(TaskState::Completed));

        if all_completed && build.status.is_in_progress() {
            build.status = BuildStatus::Completed;
            // Derive overall result: Failed > PartiallySucceeded > Canceled > Succeeded
            let has_failed = stages.iter().any(|s| s.result == Some(BuildResult::Failed));
            let has_partial = stages
                .iter()
                .any(|s| s.result == Some(BuildResult::PartiallySucceeded));
            let has_canceled = stages
                .iter()
                .any(|s| s.result == Some(BuildResult::Canceled));

            build.result = Some(if has_failed {
                BuildResult::Failed
            } else if has_partial {
                BuildResult::PartiallySucceeded
            } else if has_canceled {
                BuildResult::Canceled
            } else {
                BuildResult::Succeeded
            });
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
        tracing::info!(build_id = build.id, "navigating to log viewer");
        let return_to = self.view;
        let next_gen = self.log_viewer.log_generation + 1;
        self.log_viewer = LogViewerState {
            selected_build: Some(build),
            log_auto_scroll: true,
            follow_mode: true,
            log_generation: next_gen,
            return_to_view: return_to,
            ..Default::default()
        };
        self.view = View::LogViewer;
    }

    pub fn current_list_len(&self) -> usize {
        match self.view {
            View::Dashboard => self.dashboard_rows.len(),
            View::Pipelines => self.filtered_pipelines.len(),
            View::ActiveRuns => self.filtered_active_builds.len(),
            View::BuildHistory => self.definition_builds.len(),
            View::LogViewer => self.log_viewer.timeline_rows.len(),
        }
    }

    pub fn current_index(&self) -> usize {
        match self.view {
            View::Dashboard => self.dashboard_index,
            View::Pipelines => self.pipelines_index,
            View::ActiveRuns => self.active_runs_index,
            View::BuildHistory => self.builds_index,
            View::LogViewer => self.log_viewer.log_entries_index,
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
            View::LogViewer => self.log_viewer.log_entries_index = clamped,
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

    /// Rebuild the filtered active builds list from search query.
    pub fn rebuild_filtered_active_builds(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_active_builds = self.active_builds.clone();
        } else {
            let q = self.search_query.to_lowercase();
            self.filtered_active_builds = self
                .active_builds
                .iter()
                .filter(|b| {
                    b.definition.name.to_lowercase().contains(&q)
                        || b.build_number.to_lowercase().contains(&q)
                        || b.short_branch().to_lowercase().contains(&q)
                })
                .cloned()
                .collect();
        }
    }
}
