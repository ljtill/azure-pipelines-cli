//! Core application state and the `App` struct.

pub mod actions;
mod messages;
pub mod nav;
pub mod notifications;
pub mod run;
pub mod settings;

pub use crate::components::dashboard::DashboardRow;
pub use crate::components::log_viewer::{LogViewer, TimelineRow};
pub use nav::ListNav;

/// Stores cached retention lease data, refreshed alongside the periodic data refresh.
#[derive(Debug, Default)]
pub struct RetentionLeasesState {
    pub leases: Vec<RetentionLease>,
    /// Stores build IDs (run IDs) that have at least one retention lease.
    pub retained_run_ids: HashSet<u32>,
}

impl RetentionLeasesState {
    /// Updates the `retained_run_ids` index from the current lease list.
    pub fn rebuild_index(&mut self) {
        self.retained_run_ids = self.leases.iter().map(|l| l.run_id).collect();
    }

    /// Returns leases for a specific build/run ID.
    pub fn leases_for_run(&self, run_id: u32) -> Vec<&RetentionLease> {
        self.leases.iter().filter(|l| l.run_id == run_id).collect()
    }

    /// Returns the lease count for a specific pipeline definition.
    pub fn lease_count_for_definition(&self, definition_id: u32) -> usize {
        self.leases
            .iter()
            .filter(|l| l.definition_id == definition_id)
            .count()
    }
}

use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::client::endpoints::Endpoints;
use crate::client::models::{
    Approval, Build, BuildResult, BuildStatus, PipelineDefinition, RetentionLease,
};

use notifications::Notifications;

/// Stores shared API data refreshed periodically from Azure DevOps.
#[derive(Debug, Default)]
pub struct CoreData {
    pub definitions: Vec<PipelineDefinition>,
    pub recent_builds: Vec<Build>,
    pub active_builds: Vec<Build>,
    pub pending_approvals: Vec<Approval>,
    pub latest_builds_by_def: BTreeMap<u32, Build>,
    /// Stores build IDs that have at least one pending approval gate.
    pub pending_approval_build_ids: HashSet<u32>,
}

/// Stores filter configuration from config.toml.
#[derive(Debug, Default, Clone)]
pub struct FilterConfig {
    pub folders: Vec<String>,
    pub definition_ids: Vec<u32>,
}

/// Represents the active view in the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Dashboard,
    Pipelines,
    ActiveRuns,
    BuildHistory,
    LogViewer,
}

/// Represents the current input mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    #[default]
    Normal,
    Search,
}

/// Stores cross-cutting search/filter state shared by Pipelines and Active Runs views.
#[derive(Debug, Default)]
pub struct SearchState {
    pub query: String,
    pub mode: InputMode,
}

/// Represents an action pending user confirmation (y/n).
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
    DeleteBuildLeases {
        lease_ids: Vec<u32>,
    },
    Quit,
}

/// Represents a pending confirmation prompt shown in the footer.
#[derive(Debug, Clone)]
pub struct ConfirmPrompt {
    pub message: String,
    pub action: ConfirmAction,
}

/// Holds the central application state, including views, data, and configuration.
pub struct App {
    pub view: View,
    pub search: SearchState,
    pub running: bool,
    pub show_help: bool,
    pub show_settings: bool,
    pub org_project_label: String,
    endpoints: Endpoints,
    pub config_path: std::path::PathBuf,

    // --- Filters ---
    pub filters: FilterConfig,

    // --- Data ---
    pub data: CoreData,

    // --- Dashboard ---
    pub dashboard: crate::components::dashboard::Dashboard,

    // --- Build History ---
    pub build_history: crate::components::build_history::BuildHistory,

    // --- Log Viewer ---
    pub log_viewer: LogViewer,

    // --- Confirmation ---
    pub confirm_prompt: Option<ConfirmPrompt>,

    // --- Active Runs ---
    pub active_runs: crate::components::active_runs::ActiveRuns,

    // --- Pipelines ---
    pub pipelines: crate::components::pipelines::Pipelines,

    // --- Retention Leases ---
    pub retention_leases: RetentionLeasesState,

    // --- Settings ---
    pub settings: Option<settings::SettingsState>,

    // --- Components ---
    pub header: crate::components::header::Header,
    pub help: crate::components::help::Help,
    pub settings_component: crate::components::settings::Settings,

    // --- Status ---
    pub last_refresh: Option<DateTime<Utc>>,
    pub notifications: Notifications,
    pub loading: bool,
    pub data_refresh: crate::shared::refresh::RefreshState,
    pub log_refresh: crate::shared::refresh::RefreshState,

    // --- Refresh Timing ---
    pub refresh_interval: Duration,
    pub log_refresh_interval: Duration,

    // --- Reload ---
    pub reload_requested: bool,

    // --- State-Change Notifications ---
    pub notifications_enabled: bool,
    /// Stores the previous snapshot of (build_id, status, result) per definition,
    /// used to detect state changes between data refreshes.
    pub prev_latest_builds: BTreeMap<u32, (u32, BuildStatus, Option<BuildResult>)>,
}

impl App {
    pub fn new(
        organization: &str,
        project: &str,
        config: &crate::config::Config,
        config_path: std::path::PathBuf,
    ) -> Self {
        Self {
            view: View::Dashboard,
            search: SearchState::default(),
            running: true,
            show_help: false,
            show_settings: false,
            org_project_label: format!("{organization} / {project}"),
            endpoints: Endpoints::new(organization, project),
            config_path,
            filters: FilterConfig {
                folders: config.filters.folders.clone(),
                definition_ids: config.filters.definition_ids.clone(),
            },

            data: CoreData::default(),

            dashboard: crate::components::dashboard::Dashboard::default(),

            build_history: crate::components::build_history::BuildHistory::default(),

            log_viewer: LogViewer::default(),

            confirm_prompt: None,

            active_runs: crate::components::active_runs::ActiveRuns::default(),

            pipelines: crate::components::pipelines::Pipelines::default(),

            retention_leases: RetentionLeasesState::default(),

            settings: None,

            header: crate::components::header::Header,
            help: crate::components::help::Help,
            settings_component: crate::components::settings::Settings,

            last_refresh: None,
            notifications: Notifications::new(10),
            loading: false,
            data_refresh: crate::shared::refresh::RefreshState::default(),
            log_refresh: crate::shared::refresh::RefreshState::default(),

            refresh_interval: Duration::from_secs(config.display.refresh_interval_secs),
            log_refresh_interval: Duration::from_secs(config.display.log_refresh_interval_secs),

            reload_requested: false,

            notifications_enabled: config.notifications.enabled,
            prev_latest_builds: BTreeMap::new(),
        }
    }

    pub fn go_back(&mut self) {
        if self.show_settings {
            tracing::debug!("closing settings");
            self.show_settings = false;
            self.settings = None;
            return;
        }
        if self.show_help {
            tracing::debug!("closing help");
            self.show_help = false;
            return;
        }
        if self.search.mode == InputMode::Search {
            tracing::debug!(query = &*self.search.query, "exiting search mode");
            self.search.mode = InputMode::Normal;
            self.search.query.clear();
            self.pipelines.rebuild(
                &self.data.definitions,
                &self.filters.folders,
                &self.filters.definition_ids,
                &self.search.query,
            );
            return;
        }
        match self.view {
            View::LogViewer => {
                let return_to = self.log_viewer.return_to_view();
                tracing::info!(from = ?self.view, to = ?return_to, "navigating back");
                let next_gen = self.log_viewer.generation() + 1;
                self.log_viewer = LogViewer::default();
                // Preserve generation across resets to invalidate stale messages.
                self.log_viewer.set_generation(next_gen);

                if return_to == View::BuildHistory {
                    self.view = View::BuildHistory;
                } else {
                    self.view = return_to;
                    self.build_history.selected_definition = None;
                    self.build_history.builds.clear();
                    self.build_history.nav.reset();
                }
            }
            View::BuildHistory => {
                let return_to = self.build_history.return_to.unwrap_or(View::Dashboard);
                tracing::info!(from = ?self.view, to = ?return_to, "navigating back");
                self.view = return_to;
                self.build_history.selected_definition = None;
                self.build_history.builds.clear();
                self.build_history.selected.clear();
                self.build_history.nav.reset();
            }
            _ => {}
        }
    }

    pub fn navigate_to_build_history(&mut self, def: PipelineDefinition) {
        tracing::info!(
            definition_id = def.id,
            definition_name = &*def.name,
            "navigating to build history"
        );
        self.build_history.return_to = Some(self.view);
        self.build_history.selected_definition = Some(def);
        self.build_history.builds.clear();
        self.build_history.selected.clear();
        self.build_history.nav.reset();
        self.view = View::BuildHistory;
    }

    pub fn navigate_to_log_viewer(&mut self, build: Build) {
        tracing::info!(build_id = build.id, "navigating to log viewer");
        let return_to = self.view;
        let next_gen = self.log_viewer.generation() + 1;
        self.log_viewer = LogViewer::new_for_build(build, return_to, next_gen);
        self.view = View::LogViewer;
    }

    pub fn current_nav_mut(&mut self) -> &mut nav::ListNav {
        match self.view {
            View::Dashboard => &mut self.dashboard.nav,
            View::Pipelines => &mut self.pipelines.nav,
            View::ActiveRuns => &mut self.active_runs.nav,
            View::BuildHistory => &mut self.build_history.nav,
            View::LogViewer => self.log_viewer.nav_mut(),
        }
    }

    pub fn endpoints_web_build(&self, build_id: u32) -> String {
        self.endpoints.web_build(build_id)
    }

    pub fn endpoints_web_definition(&self, definition_id: u32) -> String {
        self.endpoints.web_definition(definition_id)
    }

    /// Builds a snapshot `Config` reflecting the current runtime state.
    /// Used to populate the settings overlay.
    pub fn current_config(&self) -> crate::config::Config {
        crate::config::Config {
            azure_devops: crate::config::AzureDevOpsConfig {
                organization: self
                    .org_project_label
                    .split(" / ")
                    .next()
                    .unwrap_or("")
                    .to_string(),
                project: self
                    .org_project_label
                    .split(" / ")
                    .nth(1)
                    .unwrap_or("")
                    .to_string(),
            },
            filters: crate::config::FiltersConfig {
                folders: self.filters.folders.clone(),
                definition_ids: self.filters.definition_ids.clone(),
            },
            update: crate::config::UpdateConfig::default(),
            logging: crate::config::LoggingConfig::default(),
            notifications: crate::config::NotificationsConfig {
                enabled: self.notifications_enabled,
            },
            display: crate::config::DisplayConfig::default(),
        }
    }

    /// Opens the settings overlay, populated from the on-disk config.
    pub fn open_settings(&mut self) {
        // Load the current config from disk to get the true persisted state.
        let config = crate::config::Config::load(Some(&self.config_path))
            .unwrap_or_else(|_| self.current_config());
        self.settings = Some(settings::SettingsState::from_config(
            &config,
            self.config_path.clone(),
        ));
        self.show_settings = true;
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::client::models::*;
    use crate::test_helpers::*;

    #[test]
    fn new_app_starts_on_dashboard() {
        let app = App::new(
            "org",
            "proj",
            &make_config(),
            PathBuf::from("/tmp/test.toml"),
        );
        assert_eq!(app.view, View::Dashboard);
        assert!(app.running);
        assert!(!app.show_help);
        assert_eq!(app.org_project_label, "org / proj");
    }

    #[test]
    fn navigate_to_build_history_sets_state() {
        let mut app = App::new(
            "org",
            "proj",
            &make_config(),
            PathBuf::from("/tmp/test.toml"),
        );
        let def = make_definition(1, "My Pipeline", "\\");
        app.navigate_to_build_history(def);
        assert_eq!(app.view, View::BuildHistory);
        assert_eq!(app.build_history.return_to, Some(View::Dashboard));
        assert_eq!(
            app.build_history.selected_definition.as_ref().unwrap().id,
            1
        );
    }

    #[test]
    fn navigate_to_log_viewer_sets_state() {
        let mut app = App::new(
            "org",
            "proj",
            &make_config(),
            PathBuf::from("/tmp/test.toml"),
        );
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
        let mut app = App::new(
            "org",
            "proj",
            &make_config(),
            PathBuf::from("/tmp/test.toml"),
        );
        let def = make_definition(1, "P", "\\");
        app.navigate_to_build_history(def);
        let build = make_build(42, BuildStatus::Completed, Some(BuildResult::Succeeded));
        app.navigate_to_log_viewer(build);
        let generation = app.log_viewer.generation();

        app.go_back();
        assert_eq!(app.view, View::BuildHistory);
        assert!(app.log_viewer.selected_build().is_none());
        // Generation should be preserved (incremented).
        assert!(app.log_viewer.generation() > generation);
    }

    #[test]
    fn go_back_from_build_history() {
        let mut app = App::new(
            "org",
            "proj",
            &make_config(),
            PathBuf::from("/tmp/test.toml"),
        );
        app.view = View::Pipelines;
        let def = make_definition(1, "P", "\\");
        app.navigate_to_build_history(def);
        app.go_back();
        assert_eq!(app.view, View::Pipelines);
        assert!(app.build_history.selected_definition.is_none());
    }

    #[test]
    fn go_back_dismisses_help() {
        let mut app = App::new(
            "org",
            "proj",
            &make_config(),
            PathBuf::from("/tmp/test.toml"),
        );
        app.show_help = true;
        app.go_back();
        assert!(!app.show_help);
        assert_eq!(app.view, View::Dashboard); // Didn't change view.
    }

    #[test]
    fn go_back_exits_search_mode() {
        let mut app = App::new(
            "org",
            "proj",
            &make_config(),
            PathBuf::from("/tmp/test.toml"),
        );
        app.search.mode = InputMode::Search;
        app.search.query = "test".to_string();
        app.go_back();
        assert_eq!(app.search.mode, InputMode::Normal);
        assert!(app.search.query.is_empty());
    }

    #[test]
    fn current_nav_mut_returns_correct_nav_for_each_view() {
        let mut app = App::new(
            "org",
            "proj",
            &make_config(),
            PathBuf::from("/tmp/test.toml"),
        );

        app.view = View::Dashboard;
        app.current_nav_mut().set_len(5);
        assert_eq!(app.dashboard.nav.index(), 0);

        app.view = View::Pipelines;
        app.current_nav_mut().set_len(3);
        app.current_nav_mut().down();
        assert_eq!(app.pipelines.nav.index(), 1);

        app.view = View::ActiveRuns;
        app.current_nav_mut().set_len(2);
        assert_eq!(app.active_runs.nav.index(), 0);
    }

    #[test]
    fn web_url_helpers() {
        let app = App::new(
            "myorg",
            "myproj",
            &make_config(),
            PathBuf::from("/tmp/test.toml"),
        );
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
