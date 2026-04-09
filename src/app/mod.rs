pub mod actions;
mod dashboard;
mod log_viewer;
mod messages;
pub mod nav;
pub mod notifications;
pub mod run;

pub use dashboard::DashboardRow;
pub use log_viewer::{LogViewerState, TimelineRow};

use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, Utc};

use crate::api::endpoints::Endpoints;
use crate::api::models::{Approval, Build, PipelineDefinition};

use notifications::Notifications;

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
    endpoints: Endpoints,

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

    // List navigation
    pub dashboard_nav: nav::ListNav,
    pub pipelines_nav: nav::ListNav,
    pub active_runs_nav: nav::ListNav,
    pub builds_nav: nav::ListNav,

    // Search
    pub search_query: String,
    pub filtered_pipelines: Vec<PipelineDefinition>,
    pub filtered_active_builds: Vec<Build>,

    // Status
    pub last_refresh: Option<DateTime<Utc>>,
    pub notifications: Notifications,
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
            endpoints: Endpoints::new(organization, project),
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

            dashboard_nav: nav::ListNav::default(),
            pipelines_nav: nav::ListNav::default(),
            active_runs_nav: nav::ListNav::default(),
            builds_nav: nav::ListNav::default(),

            search_query: String::new(),
            filtered_pipelines: Vec::new(),
            filtered_active_builds: Vec::new(),

            last_refresh: None,
            notifications: Notifications::new(10),
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
                let return_to = self.log_viewer.return_to_view();
                let next_gen = self.log_viewer.generation() + 1;
                self.log_viewer = LogViewerState::default();
                // Preserve generation across resets to invalidate stale messages.
                self.log_viewer.set_generation(next_gen);

                match return_to {
                    View::BuildHistory => {
                        self.view = View::BuildHistory;
                    }
                    _ => {
                        self.view = return_to;
                        self.selected_definition = None;
                        self.definition_builds.clear();
                        self.builds_nav.reset();
                    }
                }
            }
            View::BuildHistory => {
                self.view = self.previous_view.unwrap_or(View::Dashboard);
                self.selected_definition = None;
                self.definition_builds.clear();
                self.builds_nav.reset();
            }
            _ => {}
        }
    }

    pub fn navigate_to_build_history(&mut self, def: PipelineDefinition) {
        self.previous_view = Some(self.view);
        self.selected_definition = Some(def);
        self.definition_builds.clear();
        self.builds_nav.reset();
        self.view = View::BuildHistory;
    }

    pub fn navigate_to_log_viewer(&mut self, build: Build) {
        tracing::info!(build_id = build.id, "navigating to log viewer");
        let return_to = self.view;
        let next_gen = self.log_viewer.generation() + 1;
        self.log_viewer = LogViewerState::new_for_build(build, return_to, next_gen);
        self.view = View::LogViewer;
    }

    pub fn current_nav_mut(&mut self) -> &mut nav::ListNav {
        match self.view {
            View::Dashboard => &mut self.dashboard_nav,
            View::Pipelines => &mut self.pipelines_nav,
            View::ActiveRuns => &mut self.active_runs_nav,
            View::BuildHistory => &mut self.builds_nav,
            View::LogViewer => self.log_viewer.nav_mut(),
        }
    }

    pub fn endpoints_web_build(&self, build_id: u32) -> String {
        self.endpoints.web_build(build_id)
    }

    pub fn endpoints_web_definition(&self, definition_id: u32) -> String {
        self.endpoints.web_definition(definition_id)
    }

    /// Rebuild the filtered active builds list from search query.
    pub fn rebuild_filtered_active_builds(&mut self) {
        let base = self
            .active_builds
            .iter()
            .filter(|b| self.matches_build_filter(b));

        if self.search_query.is_empty() {
            self.filtered_active_builds = base.cloned().collect();
        } else {
            let q = self.search_query.to_lowercase();
            self.filtered_active_builds = base
                .filter(|b| {
                    b.definition.name.to_lowercase().contains(&q)
                        || b.build_number.to_lowercase().contains(&q)
                        || b.short_branch().to_lowercase().contains(&q)
                })
                .cloned()
                .collect();
        }
        self.active_runs_nav
            .set_len(self.filtered_active_builds.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::*;
    use crate::test_helpers::*;

    #[test]
    fn new_app_starts_on_dashboard() {
        let app = App::new("org", "proj", &make_config());
        assert_eq!(app.view, View::Dashboard);
        assert!(app.running);
        assert!(!app.show_help);
        assert_eq!(app.org_project_label, "org / proj");
    }

    #[test]
    fn navigate_to_build_history_sets_state() {
        let mut app = App::new("org", "proj", &make_config());
        let def = make_definition(1, "My Pipeline", "\\");
        app.navigate_to_build_history(def.clone());
        assert_eq!(app.view, View::BuildHistory);
        assert_eq!(app.previous_view, Some(View::Dashboard));
        assert_eq!(app.selected_definition.as_ref().unwrap().id, 1);
    }

    #[test]
    fn navigate_to_log_viewer_sets_state() {
        let mut app = App::new("org", "proj", &make_config());
        let build = make_build(42, BuildStatus::Completed, Some(BuildResult::Succeeded));
        let gen_before = app.log_viewer.generation();
        app.navigate_to_log_viewer(build);
        assert_eq!(app.view, View::LogViewer);
        assert_eq!(app.log_viewer.selected_build().unwrap().id, 42);
        assert!(app.log_viewer.generation() > gen_before);
        assert!(app.log_viewer.is_following());
    }

    #[test]
    fn go_back_from_log_viewer() {
        let mut app = App::new("org", "proj", &make_config());
        let def = make_definition(1, "P", "\\");
        app.navigate_to_build_history(def);
        let build = make_build(42, BuildStatus::Completed, Some(BuildResult::Succeeded));
        app.navigate_to_log_viewer(build);
        let generation = app.log_viewer.generation();

        app.go_back();
        assert_eq!(app.view, View::BuildHistory);
        assert!(app.log_viewer.selected_build().is_none());
        // Generation should be preserved (incremented)
        assert!(app.log_viewer.generation() > generation);
    }

    #[test]
    fn go_back_from_build_history() {
        let mut app = App::new("org", "proj", &make_config());
        app.view = View::Pipelines;
        let def = make_definition(1, "P", "\\");
        app.navigate_to_build_history(def);
        app.go_back();
        assert_eq!(app.view, View::Pipelines);
        assert!(app.selected_definition.is_none());
    }

    #[test]
    fn go_back_dismisses_help() {
        let mut app = App::new("org", "proj", &make_config());
        app.show_help = true;
        app.go_back();
        assert!(!app.show_help);
        assert_eq!(app.view, View::Dashboard); // didn't change view
    }

    #[test]
    fn go_back_exits_search_mode() {
        let mut app = App::new("org", "proj", &make_config());
        app.input_mode = InputMode::Search;
        app.search_query = "test".to_string();
        app.go_back();
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.search_query.is_empty());
    }

    #[test]
    fn current_nav_mut_returns_correct_nav_for_each_view() {
        let mut app = App::new("org", "proj", &make_config());

        app.view = View::Dashboard;
        app.current_nav_mut().set_len(5);
        assert_eq!(app.dashboard_nav.index(), 0);

        app.view = View::Pipelines;
        app.current_nav_mut().set_len(3);
        app.current_nav_mut().down();
        assert_eq!(app.pipelines_nav.index(), 1);

        app.view = View::ActiveRuns;
        app.current_nav_mut().set_len(2);
        assert_eq!(app.active_runs_nav.index(), 0);
    }

    #[test]
    fn web_url_helpers() {
        let app = App::new("myorg", "myproj", &make_config());
        assert_eq!(
            app.endpoints_web_build(42),
            "https://dev.azure.com/myorg/myproj/_build/results?buildId=42"
        );
        assert_eq!(
            app.endpoints_web_definition(10),
            "https://dev.azure.com/myorg/myproj/_build?definitionId=10"
        );
    }
}
