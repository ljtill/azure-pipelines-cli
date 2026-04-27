//! Core application state and the `App` struct.

pub mod actions;
pub mod effects;
pub(crate) mod messages;
pub mod nav;
pub mod notifications;
pub mod run;
pub mod settings;

pub use crate::components::dashboard::DashboardRow;
pub use crate::components::log_viewer::{LogViewer, TimelineRow};
pub use nav::ListNav;

/// Captures the most recent pagination progress event for display in the UI.
#[derive(Debug, Clone)]
pub struct PaginationStatus {
    pub endpoint: &'static str,
    pub page: usize,
    pub items: usize,
}

/// Stores cached retention lease data, refreshed alongside the periodic data refresh.
#[derive(Debug, Default)]
pub struct RetentionLeasesState {
    pub leases: Vec<RetentionLease>,
    pub leases_by_id: BTreeMap<u32, RetentionLease>,
    pub lease_ids_by_run: BTreeMap<u32, Vec<u32>>,
    pub lease_ids_by_definition: BTreeMap<u32, Vec<u32>>,
    /// Stores build IDs (run IDs) that have at least one retention lease.
    pub retained_run_ids: HashSet<u32>,
}

impl RetentionLeasesState {
    /// Replaces the lease list and rebuilds all stable-ID indexes.
    pub fn set_leases(&mut self, leases: Vec<RetentionLease>) {
        self.leases = leases;
        self.rebuild_index();
    }

    /// Updates the `retained_run_ids` index from the current lease list.
    pub fn rebuild_index(&mut self) {
        let leases = std::mem::take(&mut self.leases);
        let mut lease_order = Vec::new();
        let mut leases_by_id = BTreeMap::new();

        for lease in leases {
            if !leases_by_id.contains_key(&lease.lease_id) {
                lease_order.push(lease.lease_id);
            }
            leases_by_id.insert(lease.lease_id, lease);
        }

        self.leases = lease_order
            .iter()
            .filter_map(|lease_id| leases_by_id.get(lease_id).cloned())
            .collect();

        self.retained_run_ids = self.leases.iter().map(|lease| lease.run_id).collect();
        self.lease_ids_by_run.clear();
        self.lease_ids_by_definition.clear();
        for lease in &self.leases {
            self.lease_ids_by_run
                .entry(lease.run_id)
                .or_default()
                .push(lease.lease_id);
            self.lease_ids_by_definition
                .entry(lease.definition_id)
                .or_default()
                .push(lease.lease_id);
        }
        self.leases_by_id = leases_by_id;
    }

    /// Returns leases for a specific build/run ID.
    pub fn leases_for_run(&self, run_id: u32) -> Vec<&RetentionLease> {
        self.lease_ids_by_run
            .get(&run_id)
            .into_iter()
            .flat_map(|lease_ids| lease_ids.iter())
            .filter_map(|lease_id| self.leases_by_id.get(lease_id))
            .collect()
    }

    /// Returns the lease count for a specific pipeline definition.
    pub fn lease_count_for_definition(&self, definition_id: u32) -> usize {
        self.lease_ids_by_definition
            .get(&definition_id)
            .map_or(0, Vec::len)
    }
}

use std::collections::{BTreeMap, HashSet};
use std::ops::{Deref, DerefMut};
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::client::endpoints::{DEFAULT_API_VERSION, Endpoints};
use crate::client::models::{
    Approval, Build, BuildResult, BuildStatus, PipelineDefinition, PullRequest, RetentionLease,
    WorkItem,
};
use crate::shared::availability::{Availability, AvailabilityStatus};

use notifications::Notifications;

/// Stores shared API data refreshed periodically from Azure DevOps.
#[derive(Debug, Default)]
pub struct CoreData {
    pub definitions: Vec<PipelineDefinition>,
    pub definitions_by_id: BTreeMap<u32, PipelineDefinition>,
    pub recent_builds: Vec<Build>,
    pub recent_builds_by_id: BTreeMap<u32, Build>,
    pub active_builds: Vec<Build>,
    pub active_build_ids: HashSet<u32>,
    pub pending_approvals: Vec<Approval>,
    pub pending_approvals_by_id: BTreeMap<String, Approval>,
    pub latest_builds_by_def: BTreeMap<u32, Build>,
    /// Stores build IDs that have at least one pending approval gate.
    pub pending_approval_build_ids: HashSet<u32>,
}

impl CoreData {
    /// Replaces refresh data and rebuilds stable-ID indexes plus derived views.
    pub fn apply_refresh(
        &mut self,
        definitions: Vec<PipelineDefinition>,
        recent_builds: Vec<Build>,
        pending_approvals: Vec<Approval>,
    ) {
        let (definitions, definitions_by_id) =
            normalize_by_key(definitions, |definition| definition.id);
        let (recent_builds, recent_builds_by_id) =
            normalize_by_key(recent_builds, |build| build.id);
        let active_builds: Vec<Build> = recent_builds
            .iter()
            .filter(|build| build.status.is_in_progress())
            .cloned()
            .collect();
        let active_build_ids = active_builds.iter().map(|build| build.id).collect();
        let (pending_approvals, pending_approvals_by_id) =
            normalize_by_key(pending_approvals, |approval| approval.id.clone());
        let pending_approval_build_ids = pending_approvals
            .iter()
            .filter_map(Approval::build_id)
            .collect();
        let latest_builds_by_def = Self::latest_builds_by_definition(&definitions, &recent_builds);

        self.definitions = definitions;
        self.definitions_by_id = definitions_by_id;
        self.recent_builds = recent_builds;
        self.recent_builds_by_id = recent_builds_by_id;
        self.active_builds = active_builds;
        self.active_build_ids = active_build_ids;
        self.pending_approvals = pending_approvals;
        self.pending_approvals_by_id = pending_approvals_by_id;
        self.latest_builds_by_def = latest_builds_by_def;
        self.pending_approval_build_ids = pending_approval_build_ids;
    }

    /// Rebuilds stable-ID indexes for tests and callers that mutate vectors directly.
    pub fn rebuild_indexes(&mut self) {
        self.definitions_by_id = self
            .definitions
            .iter()
            .map(|definition| (definition.id, definition.clone()))
            .collect();
        self.recent_builds_by_id = self
            .recent_builds
            .iter()
            .map(|build| (build.id, build.clone()))
            .collect();
        self.active_build_ids = self.active_builds.iter().map(|build| build.id).collect();
        self.pending_approvals_by_id = self
            .pending_approvals
            .iter()
            .map(|approval| (approval.id.clone(), approval.clone()))
            .collect();
        self.pending_approval_build_ids = self
            .pending_approvals
            .iter()
            .filter_map(Approval::build_id)
            .collect();
    }

    /// Returns the pipeline definition with the given stable definition ID.
    pub fn definition(&self, definition_id: u32) -> Option<&PipelineDefinition> {
        self.definitions_by_id.get(&definition_id)
    }

    /// Returns the recent build with the given stable build ID.
    pub fn recent_build(&self, build_id: u32) -> Option<&Build> {
        self.recent_builds_by_id.get(&build_id)
    }

    /// Returns the pending approval with the given stable approval ID.
    pub fn pending_approval(&self, approval_id: &str) -> Option<&Approval> {
        self.pending_approvals_by_id.get(approval_id)
    }

    /// Builds the latest-build map seeded from definitions and overlaid by recent builds.
    pub fn latest_builds_by_definition(
        definitions: &[PipelineDefinition],
        recent_builds: &[Build],
    ) -> BTreeMap<u32, Build> {
        let mut map: BTreeMap<u32, Build> = BTreeMap::new();
        for definition in definitions {
            if let Some(build) = &definition.latest_build {
                map.insert(definition.id, *build.clone());
            }
        }
        for build in recent_builds {
            match map.entry(build.definition.id) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(build.clone());
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    if build.id > entry.get().id {
                        entry.insert(build.clone());
                    }
                }
            }
        }
        map
    }
}

/// Stores count metadata for the latest core data refresh snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoreDataSnapshot {
    pub definitions: usize,
    pub recent_builds: usize,
    pub active_builds: usize,
    pub pending_approvals: usize,
    pub retention_leases: usize,
}

impl CoreDataSnapshot {
    /// Returns a snapshot of the currently stored core data counts.
    pub fn from_data(data: &CoreData, retention_leases: &RetentionLeasesState) -> Self {
        Self {
            definitions: data.definitions.len(),
            recent_builds: data.recent_builds.len(),
            active_builds: data.active_builds.len(),
            pending_approvals: data.pending_approvals.len(),
            retention_leases: retention_leases.leases.len(),
        }
    }
}

/// Stores availability metadata for independently refreshed core data sections.
#[derive(Debug, Clone)]
pub struct CoreDataAvailability {
    pub refresh: Availability<CoreDataSnapshot>,
    pub definitions: Availability<Vec<PipelineDefinition>>,
    pub recent_builds: Availability<Vec<Build>>,
    pub pending_approvals: Availability<Vec<Approval>>,
    pub retention_leases: Availability<Vec<RetentionLease>>,
}

impl CoreDataAvailability {
    /// Returns labels for sections whose data is degraded or unavailable.
    pub fn degraded_section_labels(&self) -> Vec<&'static str> {
        let mut labels = Vec::new();
        if self.definitions.is_degraded() {
            labels.push("definitions");
        }
        if self.recent_builds.is_degraded() {
            labels.push("builds");
        }
        if self.pending_approvals.is_degraded() {
            labels.push("approvals");
        }
        if self.retention_leases.is_degraded() {
            labels.push("retention");
        }
        labels
    }
}

impl Default for CoreDataAvailability {
    fn default() -> Self {
        Self {
            refresh: Availability::unavailable("Data refresh has not completed"),
            definitions: Availability::unavailable("Pipeline definitions not loaded"),
            recent_builds: Availability::unavailable("Recent builds not loaded"),
            pending_approvals: Availability::unavailable("Approvals not loaded"),
            retention_leases: Availability::unavailable("Retention leases not loaded"),
        }
    }
}

fn normalize_by_key<T, K, F>(items: Vec<T>, key: F) -> (Vec<T>, BTreeMap<K, T>)
where
    T: Clone,
    K: Ord + Clone,
    F: Fn(&T) -> K,
{
    let mut order = Vec::new();
    let mut by_key = BTreeMap::new();

    for item in items {
        let item_key = key(&item);
        if !by_key.contains_key(&item_key) {
            order.push(item_key.clone());
        }
        by_key.insert(item_key, item);
    }

    let values = order
        .iter()
        .filter_map(|item_key| by_key.get(item_key).cloned())
        .collect();

    (values, by_key)
}

/// Stores filter configuration from config.toml.
#[derive(Debug, Default, Clone)]
pub struct FilterConfig {
    pub folders: Vec<String>,
    pub definition_ids: Vec<u32>,
    pub pinned_definition_ids: Vec<u32>,
    pub pinned_work_item_ids: Vec<u32>,
}

/// Stores shared data, filters, and service indexes owned by the app.
#[derive(Debug, Default)]
pub struct CoreDataStore {
    pub data: CoreData,
    pub availability: CoreDataAvailability,
    pub filters: FilterConfig,
    pub current_user: ExactUserIdentity,
    pub retention_leases: RetentionLeasesState,
}

impl CoreDataStore {
    /// Creates the core data store from the loaded configuration.
    pub fn from_config(config: &crate::config::Config) -> Self {
        Self {
            data: CoreData::default(),
            availability: CoreDataAvailability::default(),
            filters: FilterConfig {
                folders: config.devops.filters.folders.clone(),
                definition_ids: config.devops.filters.definition_ids.clone(),
                pinned_definition_ids: config.devops.filters.pinned_definition_ids.clone(),
                pinned_work_item_ids: config.devops.filters.pinned_work_item_ids.clone(),
            },
            current_user: ExactUserIdentity::default(),
            retention_leases: RetentionLeasesState::default(),
        }
    }
}

/// Stores the active Azure DevOps connection metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionMetadata {
    pub organization: String,
    pub project: String,
    pub api_version: String,
}

impl ConnectionMetadata {
    /// Creates connection metadata from the configured connection and API version.
    pub fn new(organization: &str, project: &str, api_version: &str) -> Self {
        Self {
            organization: organization.to_string(),
            project: project.to_string(),
            api_version: api_version.to_string(),
        }
    }

    /// Returns the organization/project label shown in the header.
    pub fn display_label(&self) -> String {
        format!("{} / {}", self.organization, self.project)
    }

    /// Returns `true` when the metadata matches a config connection.
    pub fn matches_config(&self, connection: &crate::config::ConnectionConfig) -> bool {
        self.organization == connection.organization && self.project == connection.project
    }

    /// Updates the active API version metadata.
    pub fn set_api_version(&mut self, api_version: &str) {
        self.api_version = api_version.to_string();
    }
}

/// Stores connection metadata and endpoint builders for Azure DevOps.
pub struct ConnectionState {
    pub metadata: ConnectionMetadata,
    endpoints: Endpoints,
}

impl ConnectionState {
    /// Creates the connection state for an organization/project pair.
    pub fn new(organization: &str, project: &str, api_version: &str) -> Self {
        Self {
            metadata: ConnectionMetadata::new(organization, project, api_version),
            endpoints: Endpoints::new(organization, project),
        }
    }

    /// Updates the active API version on metadata and endpoint builders.
    pub fn set_api_version(&mut self, api_version: &str) {
        self.metadata.set_api_version(api_version);
        self.endpoints.set_api_version(api_version);
    }
}

impl Deref for ConnectionState {
    type Target = ConnectionMetadata;

    fn deref(&self) -> &Self::Target {
        &self.metadata
    }
}

impl DerefMut for ConnectionState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.metadata
    }
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
    Partial {
        pull_requests: Vec<PullRequest>,
        errors: Vec<String>,
    },
    Stale {
        pull_requests: Vec<PullRequest>,
        message: String,
    },
    EmptyVerified,
    Unavailable(String),
}

impl DashboardPullRequestsState {
    /// Returns the availability status represented by this section state.
    pub fn status(&self) -> AvailabilityStatus {
        match self {
            Self::Loading | Self::Unavailable(_) => AvailabilityStatus::Unavailable,
            Self::Ready(_) | Self::EmptyVerified => AvailabilityStatus::Fresh,
            Self::Partial { .. } => AvailabilityStatus::Partial,
            Self::Stale { .. } => AvailabilityStatus::Stale,
        }
    }

    /// Returns pull request data when the section has a usable last-known result.
    pub fn pull_requests(&self) -> Option<&[PullRequest]> {
        match self {
            Self::Ready(pull_requests)
            | Self::Partial { pull_requests, .. }
            | Self::Stale { pull_requests, .. } => Some(pull_requests),
            Self::EmptyVerified => Some(&[]),
            Self::Loading | Self::Unavailable(_) => None,
        }
    }

    /// Returns a degraded-state message, if one should be surfaced.
    pub fn degraded_message(&self) -> Option<&str> {
        match self {
            Self::Partial { errors, .. } => errors.first().map(String::as_str),
            Self::Stale { message, .. } | Self::Unavailable(message) => Some(message),
            Self::Loading | Self::Ready(_) | Self::EmptyVerified => None,
        }
    }

    /// Returns `true` when refreshing should replace the section with a loading row.
    pub fn should_show_loading(&self) -> bool {
        self.pull_requests().is_none_or(<[PullRequest]>::is_empty)
    }

    /// Returns stale data when a last-known result exists, or unavailable otherwise.
    #[must_use]
    pub fn stale_or_unavailable(&self, message: String) -> Self {
        match self {
            Self::Ready(pull_requests)
            | Self::Partial { pull_requests, .. }
            | Self::Stale { pull_requests, .. } => Self::Stale {
                pull_requests: pull_requests.clone(),
                message,
            },
            Self::EmptyVerified => Self::Stale {
                pull_requests: Vec::new(),
                message,
            },
            Self::Loading | Self::Unavailable(_) => Self::Unavailable(message),
        }
    }
}

/// Represents the dashboard work items section state.
#[derive(Debug, Clone, Default)]
pub enum DashboardWorkItemsState {
    #[default]
    Loading,
    Ready(Vec<WorkItem>),
    Partial {
        work_items: Vec<WorkItem>,
        errors: Vec<String>,
    },
    Stale {
        work_items: Vec<WorkItem>,
        message: String,
    },
    EmptyVerified,
    Unavailable(String),
}

impl DashboardWorkItemsState {
    /// Returns the availability status represented by this section state.
    pub fn status(&self) -> AvailabilityStatus {
        match self {
            Self::Loading | Self::Unavailable(_) => AvailabilityStatus::Unavailable,
            Self::Ready(_) | Self::EmptyVerified => AvailabilityStatus::Fresh,
            Self::Partial { .. } => AvailabilityStatus::Partial,
            Self::Stale { .. } => AvailabilityStatus::Stale,
        }
    }

    /// Returns work item data when the section has a usable last-known result.
    pub fn work_items(&self) -> Option<&[WorkItem]> {
        match self {
            Self::Ready(work_items)
            | Self::Partial { work_items, .. }
            | Self::Stale { work_items, .. } => Some(work_items),
            Self::EmptyVerified => Some(&[]),
            Self::Loading | Self::Unavailable(_) => None,
        }
    }

    /// Returns a degraded-state message, if one should be surfaced.
    pub fn degraded_message(&self) -> Option<&str> {
        match self {
            Self::Partial { errors, .. } => errors.first().map(String::as_str),
            Self::Stale { message, .. } | Self::Unavailable(message) => Some(message),
            Self::Loading | Self::Ready(_) | Self::EmptyVerified => None,
        }
    }

    /// Returns `true` when refreshing should replace the section with a loading row.
    pub fn should_show_loading(&self) -> bool {
        self.work_items().is_none_or(<[WorkItem]>::is_empty)
    }

    /// Returns stale data when a last-known result exists, or unavailable otherwise.
    #[must_use]
    pub fn stale_or_unavailable(&self, message: String) -> Self {
        match self {
            Self::Ready(work_items)
            | Self::Partial { work_items, .. }
            | Self::Stale { work_items, .. } => Self::Stale {
                work_items: work_items.clone(),
                message,
            },
            Self::EmptyVerified => Self::Stale {
                work_items: Vec::new(),
                message,
            },
            Self::Loading | Self::Unavailable(_) => Self::Unavailable(message),
        }
    }
}

/// Represents the dashboard pinned-work-items section state.
#[derive(Debug, Clone, Default)]
pub enum PinnedWorkItemsState {
    #[default]
    Loading,
    Ready(Vec<WorkItem>),
    Partial {
        work_items: Vec<WorkItem>,
        errors: Vec<String>,
    },
    Stale {
        work_items: Vec<WorkItem>,
        message: String,
    },
    Unavailable(String),
}

impl PinnedWorkItemsState {
    /// Returns the availability status represented by this section state.
    pub fn status(&self) -> AvailabilityStatus {
        match self {
            Self::Loading | Self::Unavailable(_) => AvailabilityStatus::Unavailable,
            Self::Ready(_) => AvailabilityStatus::Fresh,
            Self::Partial { .. } => AvailabilityStatus::Partial,
            Self::Stale { .. } => AvailabilityStatus::Stale,
        }
    }

    /// Returns work item data when the section has a usable last-known result.
    pub fn work_items(&self) -> Option<&[WorkItem]> {
        match self {
            Self::Ready(work_items)
            | Self::Partial { work_items, .. }
            | Self::Stale { work_items, .. } => Some(work_items),
            Self::Loading | Self::Unavailable(_) => None,
        }
    }

    /// Returns a degraded-state message, if one should be surfaced.
    pub fn degraded_message(&self) -> Option<&str> {
        match self {
            Self::Partial { errors, .. } => errors.first().map(String::as_str),
            Self::Stale { message, .. } | Self::Unavailable(message) => Some(message),
            Self::Loading | Self::Ready(_) => None,
        }
    }

    /// Returns `true` when refreshing should replace the section with a loading row.
    pub fn should_show_loading(&self) -> bool {
        self.work_items().is_none_or(<[WorkItem]>::is_empty)
    }

    /// Returns stale data when a last-known result exists, or unavailable otherwise.
    #[must_use]
    pub fn stale_or_unavailable(&self, message: String) -> Self {
        match self {
            Self::Ready(work_items)
            | Self::Partial { work_items, .. }
            | Self::Stale { work_items, .. } => Self::Stale {
                work_items: work_items.clone(),
                message,
            },
            Self::Loading | Self::Unavailable(_) => Self::Unavailable(message),
        }
    }
}

/// Groups all per-view component states.
#[derive(Default)]
pub struct ViewStates {
    pub dashboard: crate::components::dashboard::Dashboard,
    pub build_history: crate::components::build_history::BuildHistory,
    pub log_viewer: LogViewer,
    pub active_runs: crate::components::active_runs::ActiveRuns,
    pub pipelines: crate::components::pipelines::Pipelines,
    pub pull_requests: crate::components::pull_requests::PullRequests,
    pub pull_request_detail: crate::components::pull_request_detail::PullRequestDetail,
    pub dashboard_pull_requests: DashboardPullRequestsState,
    pub dashboard_work_items: DashboardWorkItemsState,
    pub pinned_work_items: PinnedWorkItemsState,
    pub boards: crate::components::boards::Boards,
    pub my_work_items: crate::components::my_work_items::MyWorkItems,
    pub work_item_detail: crate::components::work_item_detail::WorkItemDetail,
}

/// Stores refresh, effect, and notification-diff state.
pub struct RefreshEffectState {
    pub last_refresh: Option<DateTime<Utc>>,
    pub loading: bool,
    pub data_refresh: crate::shared::refresh::RefreshState,
    pub log_refresh: crate::shared::refresh::RefreshState,
    pub effects: effects::EffectManager,
    /// Tracks the most recent pagination progress from long-running list
    /// operations. Cleared when the operation completes.
    pub pagination_status: Option<PaginationStatus>,
    pub refresh_interval: Duration,
    pub log_refresh_interval: Duration,
    pub max_log_lines: usize,
    pub notifications_enabled: bool,
    /// Stores the previous snapshot of (build_id, status, result) per definition,
    /// used to detect state changes between data refreshes.
    pub prev_latest_builds: BTreeMap<u32, (u32, BuildStatus, Option<BuildResult>)>,
}

impl RefreshEffectState {
    /// Creates refresh and effect state from display and notification settings.
    pub fn from_config(config: &crate::config::Config) -> Self {
        Self {
            last_refresh: None,
            loading: false,
            data_refresh: crate::shared::refresh::RefreshState::default(),
            log_refresh: crate::shared::refresh::RefreshState::default(),
            effects: effects::EffectManager::default(),
            pagination_status: None,
            refresh_interval: Duration::from_secs(config.devops.display.refresh_interval_secs),
            log_refresh_interval: Duration::from_secs(
                config.devops.display.log_refresh_interval_secs,
            ),
            max_log_lines: config.devops.display.max_log_lines,
            notifications_enabled: config.devops.notifications.enabled,
            prev_latest_builds: BTreeMap::new(),
        }
    }
}

/// Stores shell selection, overlays, notifications, and view state.
pub struct ShellUiState {
    pub service: Service,
    pub view: View,
    pub search: SearchState,
    pub running: bool,
    pub show_help: bool,
    pub show_settings: bool,
    pub confirm_prompt: Option<ConfirmPrompt>,
    pub settings: Option<settings::SettingsState>,
    pub header: crate::components::header::Header,
    pub help: crate::components::help::Help,
    pub settings_component: crate::components::settings::Settings,
    pub notifications: Notifications,
    pub reload_requested: bool,
    pub views: ViewStates,
}

impl ShellUiState {
    /// Creates the default shell state for a newly-started app.
    pub fn new() -> Self {
        Self {
            service: Service::Dashboard,
            view: View::Dashboard,
            search: SearchState::default(),
            running: true,
            show_help: false,
            show_settings: false,
            confirm_prompt: None,
            settings: None,
            header: crate::components::header::Header,
            help: crate::components::help::Help,
            settings_component: crate::components::settings::Settings,
            notifications: Notifications::new(10),
            reload_requested: false,
            views: ViewStates::default(),
        }
    }
}

impl Default for ShellUiState {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for ShellUiState {
    type Target = ViewStates;

    fn deref(&self) -> &Self::Target {
        &self.views
    }
}

impl DerefMut for ShellUiState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.views
    }
}

/// Holds the central application state, including views, data, and configuration.
pub struct App {
    pub connection: ConnectionState,
    pub config_path: std::path::PathBuf,
    pub core: CoreDataStore,
    pub refresh: RefreshEffectState,
    pub shell: ShellUiState,
}

impl Deref for App {
    type Target = ShellUiState;

    fn deref(&self) -> &Self::Target {
        &self.shell
    }
}

impl DerefMut for App {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.shell
    }
}

impl App {
    pub fn new(
        organization: &str,
        project: &str,
        config: &crate::config::Config,
        config_path: std::path::PathBuf,
    ) -> Self {
        Self {
            connection: ConnectionState::new(organization, project, DEFAULT_API_VERSION),
            config_path,
            core: CoreDataStore::from_config(config),
            refresh: RefreshEffectState::from_config(config),
            shell: ShellUiState::new(),
        }
    }

    /// Overrides the REST API version used by this app's endpoint URL builders.
    pub fn set_api_version(&mut self, api_version: &str) {
        self.connection.set_api_version(api_version);
    }

    /// Rebuilds the Pipelines view from current state.
    pub fn rebuild_pipelines(&mut self) {
        self.shell.views.pipelines.rebuild(
            &self.core.data.definitions,
            &self.core.data.latest_builds_by_def,
            &self.core.filters.folders,
            &self.core.filters.definition_ids,
            &self.core.filters.pinned_definition_ids,
            &self.shell.search.query,
        );
    }

    /// Rebuilds the Dashboard view from current state.
    pub fn rebuild_dashboard(&mut self) {
        self.shell.views.dashboard.rebuild_with_availability(
            &self.core.data.definitions,
            &self.core.filters.pinned_definition_ids,
            &self.shell.views.dashboard_pull_requests,
            &self.shell.views.dashboard_work_items,
            &self.shell.views.pinned_work_items,
            &self.core.availability,
        );
    }

    /// Rebuilds the Boards view from current state.
    pub fn rebuild_boards(&mut self) {
        self.shell.views.boards.rebuild(&self.shell.search.query);
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
        self.cancel_inactive_effects(view);

        self.service = view.service();
        self.view = view;

        match view {
            View::Dashboard => self.rebuild_dashboard(),
            View::Pipelines => self.rebuild_pipelines(),
            View::ActiveRuns => self.shell.views.active_runs.rebuild(
                &self.core.data.active_builds,
                &self.core.filters.definition_ids,
                &self.shell.search.query,
            ),
            View::PullRequestsCreatedByMe
            | View::PullRequestsAssignedToMe
            | View::PullRequestsAllActive => self
                .shell
                .views
                .pull_requests
                .rebuild(&self.shell.search.query),
            View::Boards => self.rebuild_boards(),
            View::BoardsAssignedToMe | View::BoardsCreatedByMe => {
                if let Some(list) = self.shell.views.my_work_items.list_for_mut(view) {
                    list.rebuild(&self.shell.search.query);
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
        self.cancel_effect(effects::EffectKind::BuildHistoryRefresh);
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
        self.cancel_effect(effects::EffectKind::TimelineFetch);
        self.cancel_effect(effects::EffectKind::LogFetch);
        self.cancel_effect(effects::EffectKind::LogRefresh);
        let next_gen = self.log_viewer.generation() + 1;
        self.log_viewer = LogViewer::default();
        self.log_viewer.set_generation(next_gen);
    }

    fn cancel_inactive_effects(&mut self, view: View) {
        for kind in effects::EffectKind::VIEW_SCOPED {
            if !kind.is_active_for_view(view) {
                self.cancel_effect(kind);
            }
        }
    }

    fn cancel_effect(&mut self, kind: effects::EffectKind) {
        let cancelled = self.refresh.effects.cancel(kind);
        if kind == effects::EffectKind::LogRefresh && self.refresh.log_refresh.in_flight {
            self.refresh.log_refresh.cancel();
        }

        if let Some(metadata) = cancelled {
            tracing::debug!(
                ?kind,
                generation = ?metadata.generation,
                "cancelled view-scoped background effect"
            );
        }
    }

    fn reset_pull_request_detail(&mut self) {
        self.cancel_effect(effects::EffectKind::PullRequestDetail);
        self.pull_request_detail =
            crate::components::pull_request_detail::PullRequestDetail::default();
    }

    fn reset_work_item_detail(&mut self) {
        self.cancel_effect(effects::EffectKind::WorkItemDetail);
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
                self.cancel_inactive_effects(self.view);
            }
            View::BuildHistory => {
                let return_to = self.build_history.return_to.unwrap_or(View::Dashboard);
                tracing::info!(from = ?self.view, to = ?return_to, "navigating back");
                self.service = return_to.service();
                self.view = return_to;
                self.reset_build_history();
                self.cancel_inactive_effects(self.view);
            }
            View::PullRequestDetail => {
                let return_to = self.pull_request_detail_root_view();
                tracing::info!(from = ?self.view, to = ?return_to, "navigating back");
                self.service = Service::Repos;
                self.view = return_to;
                self.reset_pull_request_detail();
                self.cancel_inactive_effects(self.view);
            }
            View::WorkItemDetail => {
                let return_to = self.work_item_detail_root_view();
                tracing::info!(from = ?self.view, to = ?return_to, "navigating back");
                self.service = Service::Boards;
                self.view = return_to;
                self.reset_work_item_detail();
                self.cancel_inactive_effects(self.view);
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
        self.cancel_inactive_effects(View::BuildHistory);
        self.service = Service::Pipelines;
        self.build_history.return_to = Some(self.view);
        self.build_history.selected_definition = Some(def);
        self.build_history.builds.clear();
        self.build_history.selected.clear();
        self.build_history.nav.reset();
        // Bump generation so any in-flight response for the previous definition is dropped.
        self.build_history.next_generation();
        self.view = View::BuildHistory;
    }

    pub fn navigate_to_log_viewer(&mut self, build: Build) {
        tracing::info!(build_id = build.id, "navigating to log viewer");
        self.cancel_effect(effects::EffectKind::TimelineFetch);
        self.cancel_effect(effects::EffectKind::LogFetch);
        self.cancel_effect(effects::EffectKind::LogRefresh);
        self.cancel_inactive_effects(View::LogViewer);
        let return_to = self.view;
        self.service = return_to.service();
        let next_gen = self.log_viewer.generation() + 1;
        self.log_viewer = LogViewer::new_for_build_with_cap(
            build,
            return_to,
            next_gen,
            self.refresh.max_log_lines,
        );
        self.view = View::LogViewer;
    }

    /// Navigates to the pull request detail view for the given PR.
    pub fn navigate_to_pr_detail(&mut self, pr: &crate::client::models::PullRequest) {
        tracing::info!(pr_id = pr.pull_request_id, "navigating to PR detail");
        self.cancel_inactive_effects(View::PullRequestDetail);
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
        self.cancel_inactive_effects(View::WorkItemDetail);
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
        self.connection.endpoints.web_build(build_id)
    }

    pub fn endpoints_web_definition(&self, definition_id: u32) -> String {
        self.connection.endpoints.web_definition(definition_id)
    }

    /// Constructs the web portal URL for viewing a pull request.
    pub fn endpoints_web_pull_request(&self, repo_name: &str, pr_id: u32) -> String {
        self.connection.endpoints.web_pull_request(repo_name, pr_id)
    }

    /// Constructs the web portal URL for viewing a work item.
    pub fn endpoints_web_work_item(&self, work_item_id: u32) -> String {
        self.connection.endpoints.web_work_item(work_item_id)
    }

    /// Builds a snapshot `Config` reflecting the current runtime state.
    /// Used to populate the settings overlay.
    pub fn current_config(&self) -> crate::config::Config {
        crate::config::Config {
            schema_version: Some(crate::config::CURRENT_SCHEMA_VERSION),
            devops: crate::config::DevOpsConfig {
                connection: crate::config::ConnectionConfig {
                    organization: self.connection.organization.clone(),
                    project: self.connection.project.clone(),
                    timeouts: crate::config::ConnectionTimeoutConfig::default(),
                },
                filters: crate::config::FiltersConfig {
                    folders: self.core.filters.folders.clone(),
                    definition_ids: self.core.filters.definition_ids.clone(),
                    pinned_definition_ids: self.core.filters.pinned_definition_ids.clone(),
                    pinned_work_item_ids: self.core.filters.pinned_work_item_ids.clone(),
                },
                update: crate::config::UpdateConfig::default(),
                logging: crate::config::LoggingConfig::default(),
                notifications: crate::config::NotificationsConfig {
                    enabled: self.refresh.notifications_enabled,
                },
                display: crate::config::DisplayConfig::default(),
            },
        }
    }

    /// Opens the settings overlay, populated from the on-disk config.
    pub fn open_settings(&mut self) {
        // Load the current config from disk to get the true persisted state.
        // `load_blocking` uses `block_in_place` so the fs read runs on tokio's
        // blocking pool while this worker yields.
        let config = crate::config::Config::load_blocking(Some(&self.config_path))
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
    use std::future;
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;
    use crate::client::models::*;
    use crate::test_helpers::*;
    use tokio::sync::oneshot;

    struct DropSignal(Option<oneshot::Sender<()>>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(tx) = self.0.take() {
                let _ = tx.send(());
            }
        }
    }

    fn pending_effect(tx: oneshot::Sender<()>) -> tokio::task::JoinHandle<()> {
        let signal = DropSignal(Some(tx));
        tokio::spawn(async move {
            let _signal = signal;
            future::pending::<()>().await;
        })
    }

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
        assert_eq!(app.connection.organization, "org");
        assert_eq!(app.connection.project, "proj");
        assert_eq!(app.connection.api_version, DEFAULT_API_VERSION);
        assert_eq!(app.connection.display_label(), "org / proj");
    }

    #[test]
    fn set_api_version_updates_connection_metadata() {
        let mut app = App::new(
            "org",
            "proj",
            &make_config(),
            PathBuf::from("target/test.toml"),
        );

        app.set_api_version("8.0-preview.2");

        assert_eq!(app.connection.api_version, "8.0-preview.2");
    }

    #[test]
    fn new_app_initializes_nested_state_groups() {
        let config = make_config();
        let app = App::new("org", "proj", &config, PathBuf::from("target/test.toml"));

        assert_eq!(app.shell.view, View::Dashboard);
        assert_eq!(app.core.filters.folders, config.devops.filters.folders);
        assert_eq!(
            app.refresh.max_log_lines,
            config.devops.display.max_log_lines
        );
        assert_eq!(app.connection.metadata.organization, "org");
        assert!(matches!(
            &app.views.dashboard_pull_requests,
            DashboardPullRequestsState::Loading
        ));
    }

    #[test]
    fn current_config_uses_structured_connection_metadata() {
        let app = App::new(
            "org / division",
            "proj / area",
            &make_config(),
            PathBuf::from("target/test.toml"),
        );

        let config = app.current_config();

        assert_eq!(config.devops.connection.organization, "org / division");
        assert_eq!(config.devops.connection.project, "proj / area");
        assert_eq!(
            app.connection.display_label(),
            "org / division / proj / area"
        );
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
        let query = app.search.query.clone();
        app.boards.rebuild(&query);

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

    #[tokio::test]
    async fn activate_root_view_cancels_inactive_effects() {
        let mut app = make_app();
        let (tx, rx) = oneshot::channel();
        let _ = app.refresh.effects.track(
            effects::EffectKind::PullRequestsRefresh,
            Some(1),
            pending_effect(tx),
        );

        app.activate_root_view(View::Dashboard);

        tokio::time::timeout(Duration::from_secs(1), rx)
            .await
            .expect("inactive effect was not aborted")
            .expect("drop signal was not delivered");
        assert!(
            !app.refresh
                .effects
                .is_running(effects::EffectKind::PullRequestsRefresh)
        );
    }

    #[tokio::test]
    async fn cancelling_log_refresh_effect_clears_in_flight_state() {
        let mut app = make_app();
        assert!(app.refresh.log_refresh.start());
        let (tx, rx) = oneshot::channel();
        let _ =
            app.refresh
                .effects
                .track(effects::EffectKind::LogRefresh, Some(1), pending_effect(tx));

        app.activate_root_view(View::Dashboard);

        tokio::time::timeout(Duration::from_secs(1), rx)
            .await
            .expect("log refresh effect was not aborted")
            .expect("drop signal was not delivered");
        assert!(!app.refresh.log_refresh.in_flight);
        assert_eq!(app.refresh.log_refresh.failures, 0);
    }

    #[tokio::test]
    async fn go_back_cancels_detail_effects() {
        let mut app = make_app();
        app.view = View::PullRequestDetail;
        let (tx, rx) = oneshot::channel();
        let _ = app.refresh.effects.track(
            effects::EffectKind::PullRequestDetail,
            None,
            pending_effect(tx),
        );

        app.go_back();

        tokio::time::timeout(Duration::from_secs(1), rx)
            .await
            .expect("detail effect was not aborted")
            .expect("drop signal was not delivered");
        assert!(
            !app.refresh
                .effects
                .is_running(effects::EffectKind::PullRequestDetail)
        );
    }
}
