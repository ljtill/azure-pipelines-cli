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
    Approval, Build, BuildResult, BuildStatus, PipelineDefinition, PullRequest, RetentionLease,
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
    pub pinned_definition_ids: Vec<u32>,
}

/// Represents the active top-level area in the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    Dashboard,
    Boards,
    Repos,
    Pipelines,
}

/// Represents the active view in the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Dashboard,
    Pipelines,
    ActiveRuns,
    BuildHistory,
    LogViewer,
    PullRequestsCreatedByMe,
    PullRequestsAssignedToMe,
    PullRequestsAllActive,
    PullRequestDetail,
    Boards,
    BoardsAssignedToMe,
    BoardsCreatedByMe,
    WorkItemDetail,
}

impl View {
    /// Returns `true` when this view is a root-level screen in the shell.
    pub fn is_root(self) -> bool {
        matches!(
            self,
            View::Dashboard
                | View::Pipelines
                | View::ActiveRuns
                | View::PullRequestsCreatedByMe
                | View::PullRequestsAssignedToMe
                | View::PullRequestsAllActive
                | View::Boards
                | View::BoardsAssignedToMe
                | View::BoardsCreatedByMe
        )
    }

    /// Returns `true` when this view is one of the Pull Requests list sub-views.
    pub fn is_pull_requests(self) -> bool {
        matches!(
            self,
            View::PullRequestsCreatedByMe
                | View::PullRequestsAssignedToMe
                | View::PullRequestsAllActive
        )
    }

    /// Returns `true` when this view is one of the personal Boards list sub-views.
    pub fn is_my_work_items(self) -> bool {
        matches!(self, View::BoardsAssignedToMe | View::BoardsCreatedByMe)
    }

    /// Returns the owning top-level area for this view.
    pub fn service(self) -> Service {
        match self {
            View::Dashboard => Service::Dashboard,
            View::Pipelines | View::ActiveRuns | View::BuildHistory | View::LogViewer => {
                Service::Pipelines
            }
            View::PullRequestsCreatedByMe
            | View::PullRequestsAssignedToMe
            | View::PullRequestsAllActive
            | View::PullRequestDetail => Service::Repos,
            View::Boards
            | View::BoardsAssignedToMe
            | View::BoardsCreatedByMe
            | View::WorkItemDetail => Service::Boards,
        }
    }

    /// Returns the user-facing label for this view when shown in the shell.
    pub fn root_label(self) -> &'static str {
        match self {
            View::Dashboard => "Overview",
            View::Pipelines => "Definitions",
            View::ActiveRuns => "Active Runs",
            View::PullRequestsCreatedByMe | View::BoardsCreatedByMe => "Created by me",
            View::PullRequestsAssignedToMe | View::BoardsAssignedToMe => "Assigned to me",
            View::PullRequestsAllActive => "All active",
            View::Boards => "Backlog",
            View::BuildHistory
            | View::LogViewer
            | View::PullRequestDetail
            | View::WorkItemDetail => "",
        }
    }
}

const DASHBOARD_ROOT_VIEWS: [View; 1] = [View::Dashboard];
const BOARDS_ROOT_VIEWS: [View; 3] = [
    View::Boards,
    View::BoardsAssignedToMe,
    View::BoardsCreatedByMe,
];
const REPOS_ROOT_VIEWS: [View; 3] = [
    View::PullRequestsCreatedByMe,
    View::PullRequestsAssignedToMe,
    View::PullRequestsAllActive,
];
const PIPELINES_ROOT_VIEWS: [View; 2] = [View::Pipelines, View::ActiveRuns];

impl Service {
    /// All top-level areas currently exposed in the shell.
    pub const ALL: [Service; 4] = [
        Service::Dashboard,
        Service::Boards,
        Service::Repos,
        Service::Pipelines,
    ];

    /// Returns the user-facing label for this area.
    pub fn label(self) -> &'static str {
        match self {
            Service::Dashboard => "Dashboard",
            Service::Boards => "Boards",
            Service::Repos => "Repos",
            Service::Pipelines => "Pipelines",
        }
    }

    /// Returns the keybinding used to jump directly to this area.
    pub fn key(self) -> char {
        match self {
            Service::Dashboard => '1',
            Service::Boards => '2',
            Service::Repos => '3',
            Service::Pipelines => '4',
        }
    }

    /// Returns the root views for this area in display order.
    pub fn root_views(self) -> &'static [View] {
        match self {
            Service::Dashboard => &DASHBOARD_ROOT_VIEWS,
            Service::Boards => &BOARDS_ROOT_VIEWS,
            Service::Repos => &REPOS_ROOT_VIEWS,
            Service::Pipelines => &PIPELINES_ROOT_VIEWS,
        }
    }
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

/// Stores the exact identity fields used for strict dashboard PR ownership checks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExactUserIdentity {
    pub id: Option<String>,
    pub unique_name: Option<String>,
    pub descriptor: Option<String>,
}

impl ExactUserIdentity {
    /// Returns `true` when at least one exact identity field is available.
    pub fn is_known(&self) -> bool {
        self.id.is_some() || self.unique_name.is_some() || self.descriptor.is_some()
    }
}

/// Represents the dashboard PR section state.
#[derive(Debug, Clone, Default)]
pub enum DashboardPullRequestsState {
    #[default]
    Loading,
    Ready(Vec<PullRequest>),
    EmptyVerified,
    Unavailable(String),
}

/// Holds the central application state, including views, data, and configuration.
pub struct App {
    pub service: Service,
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

    // --- Pull Requests ---
    pub pull_requests: crate::components::pull_requests::PullRequests,
    pub pull_request_detail: crate::components::pull_request_detail::PullRequestDetail,
    pub current_user: ExactUserIdentity,
    pub dashboard_pull_requests: DashboardPullRequestsState,

    // --- Boards ---
    pub boards: crate::components::boards::Boards,
    pub my_work_items: crate::components::my_work_items::MyWorkItems,
    pub work_item_detail: crate::components::work_item_detail::WorkItemDetail,

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

    // --- Log Viewer ---
    pub max_log_lines: usize,

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
            service: Service::Dashboard,
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
                pinned_definition_ids: config.filters.pinned_definition_ids.clone(),
            },

            data: CoreData::default(),

            dashboard: crate::components::dashboard::Dashboard::default(),

            build_history: crate::components::build_history::BuildHistory::default(),

            log_viewer: LogViewer::default(),

            confirm_prompt: None,

            active_runs: crate::components::active_runs::ActiveRuns::default(),

            pipelines: crate::components::pipelines::Pipelines::default(),

            pull_requests: crate::components::pull_requests::PullRequests::default(),
            pull_request_detail: crate::components::pull_request_detail::PullRequestDetail::default(
            ),
            current_user: ExactUserIdentity::default(),
            dashboard_pull_requests: DashboardPullRequestsState::Loading,
            boards: crate::components::boards::Boards::default(),
            my_work_items: crate::components::my_work_items::MyWorkItems::default(),
            work_item_detail: crate::components::work_item_detail::WorkItemDetail::default(),

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

            max_log_lines: config.display.max_log_lines,

            reload_requested: false,

            notifications_enabled: config.notifications.enabled,
            prev_latest_builds: BTreeMap::new(),
        }
    }

    /// Rebuilds the Pipelines view from current state.
    pub fn rebuild_pipelines(&mut self) {
        self.pipelines.rebuild(
            &self.data.definitions,
            &self.data.latest_builds_by_def,
            &self.filters.folders,
            &self.filters.definition_ids,
            &self.filters.pinned_definition_ids,
            &self.search.query,
        );
    }

    /// Rebuilds the Dashboard view from current state.
    pub fn rebuild_dashboard(&mut self) {
        self.dashboard.rebuild(
            &self.data.definitions,
            &self.data.latest_builds_by_def,
            &self.filters.pinned_definition_ids,
            &self.dashboard_pull_requests,
        );
    }

    /// Rebuilds the Boards view from current state.
    pub fn rebuild_boards(&mut self) {
        self.boards.rebuild(&self.search.query);
    }

    fn build_history_root_view(&self) -> View {
        match self.build_history.return_to.unwrap_or(View::Pipelines) {
            View::ActiveRuns => View::ActiveRuns,
            View::Dashboard => View::Dashboard,
            _ => View::Pipelines,
        }
    }

    fn log_viewer_root_view(&self) -> View {
        match self.log_viewer.return_to_view() {
            View::BuildHistory => self.build_history_root_view(),
            View::ActiveRuns => View::ActiveRuns,
            View::Dashboard => View::Dashboard,
            _ => View::Pipelines,
        }
    }

    fn pull_request_detail_root_view(&self) -> View {
        self.pull_request_detail
            .return_to
            .filter(|v| v.is_pull_requests())
            .unwrap_or(View::PullRequestsCreatedByMe)
    }

    fn work_item_detail_root_view(&self) -> View {
        self.work_item_detail
            .return_to
            .filter(|v| {
                matches!(
                    v,
                    View::Boards | View::BoardsAssignedToMe | View::BoardsCreatedByMe
                )
            })
            .unwrap_or(View::Boards)
    }

    /// Returns the current root view for the selected service, even from a drill-in.
    pub fn active_root_view(&self) -> View {
        match self.view {
            View::BuildHistory => self.build_history_root_view(),
            View::LogViewer => self.log_viewer_root_view(),
            View::PullRequestDetail => self.pull_request_detail_root_view(),
            View::WorkItemDetail => self.work_item_detail_root_view(),
            view => view,
        }
    }

    /// Returns the root view for a service.
    pub fn root_view_for_service(&self, service: Service) -> View {
        service.root_views()[0]
    }

    /// Selects a service and activates its root view.
    pub fn select_service(&mut self, service: Service) -> View {
        let target = self.root_view_for_service(service);
        self.activate_root_view(target);
        target
    }

    /// Activates a root-level view and resets detail-only state as needed.
    pub fn activate_root_view(&mut self, view: View) {
        debug_assert!(view.is_root());

        self.search.mode = InputMode::Normal;
        self.search.query.clear();

        self.reset_log_viewer();
        self.reset_build_history();
        self.reset_pull_request_detail();
        self.reset_work_item_detail();

        self.service = view.service();
        self.view = view;

        match view {
            View::Dashboard => self.rebuild_dashboard(),
            View::Pipelines => self.rebuild_pipelines(),
            View::ActiveRuns => self.active_runs.rebuild(
                &self.data.active_builds,
                &self.filters.definition_ids,
                &self.search.query,
            ),
            View::PullRequestsCreatedByMe
            | View::PullRequestsAssignedToMe
            | View::PullRequestsAllActive => self.pull_requests.rebuild(&self.search.query),
            View::Boards => self.rebuild_boards(),
            View::BoardsAssignedToMe | View::BoardsCreatedByMe => {
                if let Some(list) = self.my_work_items.list_for_mut(view) {
                    list.rebuild(&self.search.query);
                }
            }
            View::BuildHistory
            | View::LogViewer
            | View::PullRequestDetail
            | View::WorkItemDetail => {}
        }
    }

    /// Cycles to the next (or previous) root view within the currently active service.
    /// `delta` is `+1` to move forward and `-1` to move backward. Returns the new view.
    /// No-op when the active service has a single root view.
    pub fn cycle_root_view(&mut self, delta: i32) -> View {
        let views = self.service.root_views();
        if views.len() <= 1 {
            return self.view;
        }
        let current = self.active_root_view();
        let idx = views.iter().position(|v| *v == current).unwrap_or(0);
        let len = views.len() as i32;
        let next_idx = ((idx as i32 + delta).rem_euclid(len)) as usize;
        let next = views[next_idx];
        self.activate_root_view(next);
        next
    }

    fn reset_build_history(&mut self) {
        self.build_history.selected_definition = None;
        self.build_history.builds.clear();
        self.build_history.selected.clear();
        self.build_history.nav.reset();
        self.build_history.return_to = None;
        self.build_history.has_more = false;
        self.build_history.loading_more = false;
        self.build_history.continuation_token = None;
        self.build_history.pending_nav_index = None;
    }

    fn reset_log_viewer(&mut self) {
        let next_gen = self.log_viewer.generation() + 1;
        self.log_viewer = LogViewer::default();
        self.log_viewer.set_generation(next_gen);
    }

    fn reset_pull_request_detail(&mut self) {
        self.pull_request_detail =
            crate::components::pull_request_detail::PullRequestDetail::default();
    }

    fn reset_work_item_detail(&mut self) {
        self.work_item_detail = crate::components::work_item_detail::WorkItemDetail::default();
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
            self.rebuild_pipelines();
            return;
        }
        match self.view {
            View::LogViewer => {
                let return_to = self.log_viewer.return_to_view();
                tracing::info!(from = ?self.view, to = ?return_to, "navigating back");
                self.reset_log_viewer();

                if return_to == View::BuildHistory {
                    self.service = Service::Pipelines;
                    self.view = View::BuildHistory;
                } else {
                    self.service = return_to.service();
                    self.view = return_to;
                    self.reset_build_history();
                }
            }
            View::BuildHistory => {
                let return_to = self.build_history.return_to.unwrap_or(View::Dashboard);
                tracing::info!(from = ?self.view, to = ?return_to, "navigating back");
                self.service = return_to.service();
                self.view = return_to;
                self.reset_build_history();
            }
            View::PullRequestDetail => {
                let return_to = self.pull_request_detail_root_view();
                tracing::info!(from = ?self.view, to = ?return_to, "navigating back");
                self.service = Service::Repos;
                self.view = return_to;
                self.reset_pull_request_detail();
            }
            View::WorkItemDetail => {
                let return_to = self.work_item_detail_root_view();
                tracing::info!(from = ?self.view, to = ?return_to, "navigating back");
                self.service = Service::Boards;
                self.view = return_to;
                self.reset_work_item_detail();
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
        self.service = Service::Pipelines;
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
        self.service = return_to.service();
        let next_gen = self.log_viewer.generation() + 1;
        self.log_viewer =
            LogViewer::new_for_build_with_cap(build, return_to, next_gen, self.max_log_lines);
        self.view = View::LogViewer;
    }

    /// Navigates to the pull request detail view for the given PR.
    pub fn navigate_to_pr_detail(&mut self, pr: &crate::client::models::PullRequest) {
        tracing::info!(pr_id = pr.pull_request_id, "navigating to PR detail");
        let return_to = if self.view.is_pull_requests() {
            Some(self.view)
        } else {
            Some(View::PullRequestsCreatedByMe)
        };
        self.service = Service::Repos;
        self.pull_request_detail = crate::components::pull_request_detail::PullRequestDetail {
            pull_request: None,
            threads: vec![],
            nav: ListNav::default(),
            loading: true,
            return_to,
        };
        self.view = View::PullRequestDetail;
    }

    /// Navigates to the work item detail view for the given id. The source
    /// view (Boards root or a personal Boards sub-view) is captured so that
    /// back navigation returns the user where they came from.
    pub fn navigate_to_work_item_detail(&mut self, work_item_id: u32) {
        tracing::info!(work_item_id, "navigating to work item detail");
        let return_to = match self.view {
            View::Boards | View::BoardsAssignedToMe | View::BoardsCreatedByMe => Some(self.view),
            _ => Some(View::Boards),
        };
        self.service = Service::Boards;
        self.work_item_detail = crate::components::work_item_detail::WorkItemDetail {
            work_item_id: Some(work_item_id),
            work_item: None,
            comments: vec![],
            nav: ListNav::default(),
            loading: true,
            return_to,
        };
        self.view = View::WorkItemDetail;
    }

    pub fn current_nav_mut(&mut self) -> &mut nav::ListNav {
        match self.view {
            View::Dashboard => &mut self.dashboard.nav,
            View::Pipelines => &mut self.pipelines.nav,
            View::ActiveRuns => &mut self.active_runs.nav,
            View::BuildHistory => &mut self.build_history.nav,
            View::LogViewer => self.log_viewer.nav_mut(),
            View::PullRequestsCreatedByMe
            | View::PullRequestsAssignedToMe
            | View::PullRequestsAllActive => &mut self.pull_requests.nav,
            View::PullRequestDetail => &mut self.pull_request_detail.nav,
            View::Boards => &mut self.boards.nav,
            View::BoardsAssignedToMe => &mut self.my_work_items.assigned.nav,
            View::BoardsCreatedByMe => &mut self.my_work_items.created.nav,
            View::WorkItemDetail => &mut self.work_item_detail.nav,
        }
    }

    pub fn endpoints_web_build(&self, build_id: u32) -> String {
        self.endpoints.web_build(build_id)
    }

    pub fn endpoints_web_definition(&self, definition_id: u32) -> String {
        self.endpoints.web_definition(definition_id)
    }

    /// Constructs the web portal URL for viewing a pull request.
    pub fn endpoints_web_pull_request(&self, repo_name: &str, pr_id: u32) -> String {
        self.endpoints.web_pull_request(repo_name, pr_id)
    }

    /// Constructs the web portal URL for viewing a work item.
    pub fn endpoints_web_work_item(&self, work_item_id: u32) -> String {
        self.endpoints.web_work_item(work_item_id)
    }

    /// Builds a snapshot `Config` reflecting the current runtime state.
    /// Used to populate the settings overlay.
    pub fn current_config(&self) -> crate::config::Config {
        crate::config::Config {
            schema_version: Some(crate::config::CURRENT_SCHEMA_VERSION),
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
                pinned_definition_ids: self.filters.pinned_definition_ids.clone(),
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
        assert_eq!(app.service, Service::Dashboard);
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
    fn build_history_root_view_is_pipelines_when_launched_from_pipelines() {
        let mut app = App::new(
            "org",
            "proj",
            &make_config(),
            PathBuf::from("/tmp/test.toml"),
        );
        app.view = View::Pipelines;
        app.service = Service::Pipelines;

        let def = make_definition(1, "My Pipeline", "\\");
        app.navigate_to_build_history(def);

        assert_eq!(app.active_root_view(), View::Pipelines);
    }

    #[test]
    fn log_viewer_root_view_follows_build_history_origin() {
        let mut app = App::new(
            "org",
            "proj",
            &make_config(),
            PathBuf::from("/tmp/test.toml"),
        );
        app.view = View::Pipelines;
        app.service = Service::Pipelines;

        let def = make_definition(1, "My Pipeline", "\\");
        app.navigate_to_build_history(def);
        app.navigate_to_log_viewer(make_build(
            42,
            BuildStatus::Completed,
            Some(BuildResult::Succeeded),
        ));

        assert_eq!(app.active_root_view(), View::Pipelines);
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
    fn activate_root_view_rebuilds_boards_after_clearing_search() {
        let mut app = App::new(
            "org",
            "proj",
            &make_config(),
            PathBuf::from("/tmp/test.toml"),
        );
        app.boards.items = BTreeMap::from([
            (
                1,
                crate::components::boards::BoardItem {
                    id: 1,
                    title: "Root".to_string(),
                    work_item_type: "Epic".to_string(),
                    state: "Active".to_string(),
                    assigned_to: None,
                    iteration_path: None,
                    parent_id: None,
                    child_ids: vec![2],
                    stack_rank: Some(1.0),
                },
            ),
            (
                2,
                crate::components::boards::BoardItem {
                    id: 2,
                    title: "Needle child".to_string(),
                    work_item_type: "Feature".to_string(),
                    state: "Active".to_string(),
                    assigned_to: None,
                    iteration_path: None,
                    parent_id: Some(1),
                    child_ids: vec![],
                    stack_rank: Some(2.0),
                },
            ),
            (
                3,
                crate::components::boards::BoardItem {
                    id: 3,
                    title: "Other root".to_string(),
                    work_item_type: "Epic".to_string(),
                    state: "New".to_string(),
                    assigned_to: None,
                    iteration_path: None,
                    parent_id: None,
                    child_ids: vec![],
                    stack_rank: Some(3.0),
                },
            ),
        ]);
        app.boards.root_ids = vec![1, 3];
        app.search.query = "needle".to_string();
        app.boards.rebuild(&app.search.query);

        assert_eq!(app.boards.rows.len(), 2);

        app.activate_root_view(View::Boards);

        assert!(app.search.query.is_empty());
        assert_eq!(app.boards.rows.len(), 3);
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
