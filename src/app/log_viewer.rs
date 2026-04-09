use std::collections::{HashMap, HashSet};

use crate::api::models::{Build, BuildResult, BuildStatus, BuildTimeline, TaskState};

use super::View;
use super::nav::ListNav;

/// A row in the timeline tree view — Stage, Job, Task, or Checkpoint.
#[derive(Debug, Clone)]
pub enum TimelineRow {
    Stage {
        id: String,
        identifier: Option<String>,
        name: String,
        state: Option<TaskState>,
        result: Option<BuildResult>,
        collapsed: bool,
    },
    Job {
        id: String,
        name: String,
        state: Option<TaskState>,
        result: Option<BuildResult>,
        collapsed: bool,
        parent_stage_id: String,
    },
    Task {
        name: String,
        state: Option<TaskState>,
        result: Option<BuildResult>,
        log_id: Option<u32>,
        parent_job_id: String,
    },
    Checkpoint {
        name: String,
        #[allow(dead_code)]
        record_type: String,
        state: Option<TaskState>,
        result: Option<BuildResult>,
        approval_id: Option<String>,
        #[allow(dead_code)]
        parent_stage_id: String,
    },
}

/// State for the log viewer screen — reset as a unit on navigation.
pub struct LogViewerState {
    selected_build: Option<Build>,
    build_timeline: Option<BuildTimeline>,
    timeline_rows: Vec<TimelineRow>,
    collapsed_stages: HashSet<String>,
    collapsed_jobs: HashSet<String>,
    log_content: Vec<String>,
    log_auto_scroll: bool,
    log_generation: u64,
    timeline_initialized: bool,
    follow_mode: bool,
    followed_task_name: String,
    followed_log_id: Option<u32>,
    log_entries_nav: ListNav,
    log_scroll_offset: u16,
    /// The view to return to when pressing Esc from LogViewer.
    return_to_view: View,
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
            log_entries_nav: ListNav::default(),
            log_scroll_offset: 0,
            return_to_view: View::BuildHistory,
        }
    }
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------
impl LogViewerState {
    /// Create a new log viewer state for navigating to a specific build.
    pub(super) fn new_for_build(build: Build, return_to: View, generation: u64) -> Self {
        Self {
            selected_build: Some(build),
            log_auto_scroll: true,
            follow_mode: true,
            log_generation: generation,
            return_to_view: return_to,
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Getters
// ---------------------------------------------------------------------------
impl LogViewerState {
    pub fn selected_build(&self) -> Option<&Build> {
        self.selected_build.as_ref()
    }

    #[allow(dead_code)]
    pub fn build_timeline(&self) -> Option<&BuildTimeline> {
        self.build_timeline.as_ref()
    }

    pub fn timeline_rows(&self) -> &[TimelineRow] {
        &self.timeline_rows
    }

    pub fn log_content(&self) -> &[String] {
        &self.log_content
    }

    pub fn log_auto_scroll(&self) -> bool {
        self.log_auto_scroll
    }

    pub fn generation(&self) -> u64 {
        self.log_generation
    }

    pub fn is_following(&self) -> bool {
        self.follow_mode
    }

    pub fn followed_task_name(&self) -> &str {
        &self.followed_task_name
    }

    pub fn followed_log_id(&self) -> Option<u32> {
        self.followed_log_id
    }

    pub fn log_scroll_offset(&self) -> u16 {
        self.log_scroll_offset
    }

    pub fn return_to_view(&self) -> View {
        self.return_to_view
    }

    pub fn nav(&self) -> &ListNav {
        &self.log_entries_nav
    }

    pub fn nav_mut(&mut self) -> &mut ListNav {
        &mut self.log_entries_nav
    }
}

// ---------------------------------------------------------------------------
// State transitions
// ---------------------------------------------------------------------------
impl LogViewerState {
    pub fn enter_follow_mode(&mut self) {
        self.follow_mode = true;
    }

    pub fn enter_inspect_mode(&mut self) {
        self.follow_mode = false;
    }
}

// ---------------------------------------------------------------------------
// Mutators
// ---------------------------------------------------------------------------
impl LogViewerState {
    pub fn set_build_timeline(&mut self, timeline: BuildTimeline) {
        self.build_timeline = Some(timeline);
    }

    pub fn set_log_content(&mut self, content: String) {
        self.log_content = content.lines().map(String::from).collect();
        self.log_auto_scroll = true;
        self.log_scroll_offset = 0;
    }

    pub fn set_followed(&mut self, task_name: String, log_id: u32) {
        self.followed_task_name = task_name;
        self.followed_log_id = Some(log_id);
    }

    pub fn clear_log(&mut self) {
        self.log_content.clear();
    }

    #[allow(dead_code)]
    pub fn set_log_auto_scroll(&mut self, auto: bool) {
        self.log_auto_scroll = auto;
    }

    #[allow(dead_code)]
    pub fn set_log_scroll_offset(&mut self, offset: u16) {
        self.log_scroll_offset = offset;
    }

    pub fn set_generation(&mut self, generation: u64) {
        self.log_generation = generation;
    }

    pub fn scroll_up(&mut self, amount: u16) {
        self.log_auto_scroll = false;
        self.log_scroll_offset = self.log_scroll_offset.saturating_sub(amount);
    }

    pub fn scroll_down(&mut self, amount: u16) {
        self.log_scroll_offset = self.log_scroll_offset.saturating_add(amount);
    }

    #[allow(dead_code)]
    pub fn set_timeline_rows(&mut self, rows: Vec<TimelineRow>) {
        self.timeline_rows = rows;
        self.log_entries_nav.set_len(self.timeline_rows.len());
    }
}

// ---------------------------------------------------------------------------
// Timeline collapse state
// ---------------------------------------------------------------------------
impl LogViewerState {
    #[allow(dead_code)]
    pub fn is_stage_collapsed(&self, id: &str) -> bool {
        self.collapsed_stages.contains(id)
    }

    #[allow(dead_code)]
    pub fn is_job_collapsed(&self, id: &str) -> bool {
        self.collapsed_jobs.contains(id)
    }

    pub fn collapse_stage(&mut self, id: String) {
        self.collapsed_stages.insert(id);
    }

    pub fn expand_stage(&mut self, id: &str) {
        self.collapsed_stages.remove(id);
    }

    pub fn collapse_job(&mut self, id: String) {
        self.collapsed_jobs.insert(id);
    }

    pub fn expand_job(&mut self, id: &str) {
        self.collapsed_jobs.remove(id);
    }

    pub fn toggle_stage(&mut self, id: &str) -> bool {
        if self.collapsed_stages.contains(id) {
            self.collapsed_stages.remove(id);
            false
        } else {
            self.collapsed_stages.insert(id.to_owned());
            true
        }
    }

    pub fn toggle_job(&mut self, id: &str) -> bool {
        if self.collapsed_jobs.contains(id) {
            self.collapsed_jobs.remove(id);
            false
        } else {
            self.collapsed_jobs.insert(id.to_owned());
            true
        }
    }

    #[allow(dead_code)]
    pub fn is_timeline_initialized(&self) -> bool {
        self.timeline_initialized
    }

    #[allow(dead_code)]
    pub fn set_timeline_initialized(&mut self) {
        self.timeline_initialized = true;
    }
}

// ---------------------------------------------------------------------------
// Build status derived from timeline
// ---------------------------------------------------------------------------
impl LogViewerState {
    /// Update `selected_build` status/result from timeline records.
    /// Called on each timeline refresh so the log viewer header stays current.
    pub fn refresh_build_status_from_timeline(&mut self) {
        let timeline = match &self.build_timeline {
            Some(t) => t,
            None => return,
        };
        let build = match &mut self.selected_build {
            Some(b) => b,
            None => return,
        };

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
}

// ---------------------------------------------------------------------------
// Timeline tree building & queries (moved from app/timeline.rs)
// ---------------------------------------------------------------------------

/// A task record extracted from the timeline for auto-selection.
struct TaskCandidate {
    name: String,
    state: Option<TaskState>,
    result: Option<BuildResult>,
    parent_id: Option<String>,
    log_id: u32,
}

impl LogViewerState {
    /// Pick the most relevant timeline task to auto-show logs for.
    /// Returns (row_index, log_id) if found. Ensures parent stage/job are expanded.
    pub fn auto_select_log_entry(&mut self) -> Option<(usize, u32)> {
        let timeline = self.build_timeline.as_ref()?;
        let tasks: Vec<TaskCandidate> = timeline
            .records
            .iter()
            .filter(|r| r.record_type == "Task" && r.log.is_some())
            .map(|r| TaskCandidate {
                name: r.name.clone(),
                state: r.state,
                result: r.result,
                parent_id: r.parent_id.clone(),
                log_id: r.log.as_ref().unwrap().id,
            })
            .collect();

        if tasks.is_empty() {
            return None;
        }

        let is_running = self
            .selected_build
            .as_ref()
            .is_some_and(|b| b.status.is_in_progress());

        let best_idx = if is_running {
            tasks
                .iter()
                .rposition(|t| t.state == Some(TaskState::InProgress))
                .or(Some(tasks.len() - 1))
        } else {
            tasks
                .iter()
                .rposition(|t| t.result == Some(BuildResult::Failed))
                .or(Some(tasks.len() - 1))
        };
        let best_idx = best_idx?;
        let best = &tasks[best_idx];
        let log_id = best.log_id;
        let best_name = best.name.clone();
        let parent_job_id = best.parent_id.clone();

        // Walk up the ancestor chain to expand all parent nodes.
        if let Some(timeline) = self.build_timeline.as_ref() {
            let records = &timeline.records;
            let mut current_id = parent_job_id.clone();
            while let Some(cid) = &current_id {
                if let Some(rec) = records.iter().find(|r| r.id == *cid) {
                    match rec.record_type.as_str() {
                        "Stage" => {
                            self.collapsed_stages.remove(cid.as_str());
                            break;
                        }
                        "Phase" | "Job" => {
                            self.collapsed_jobs.remove(cid.as_str());
                        }
                        _ => {}
                    }
                    current_id = rec.parent_id.clone();
                } else {
                    break;
                }
            }
        }

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
            self.log_entries_nav.set_index(idx);
            Some((idx, log_id))
        } else {
            None
        }
    }

    /// Find the currently active task without moving cursor or changing state.
    pub fn find_active_task(&self) -> Option<(String, u32)> {
        let timeline = self.build_timeline.as_ref()?;
        let tasks: Vec<_> = timeline
            .records
            .iter()
            .filter(|r| r.record_type == "Task" && r.log.is_some())
            .collect();

        if tasks.is_empty() {
            return None;
        }

        let is_running = self
            .selected_build
            .as_ref()
            .is_some_and(|b| b.status.is_in_progress());

        let best = if is_running {
            tasks
                .iter()
                .rev()
                .find(|r| r.state.is_some_and(|s| s.is_in_progress()))
                .or(tasks.last())
        } else {
            tasks
                .iter()
                .rev()
                .find(|r| r.result == Some(BuildResult::Failed))
                .or(tasks.last())
        };

        let best = best?;
        let log_id = best.log.as_ref()?.id;
        Some((best.name.clone(), log_id))
    }

    /// Build the timeline tree rows from the raw timeline records.
    ///
    /// ADO timeline hierarchy: Stage → Phase → Job → Task
    /// We display: Stage → Phase (as "Job" row) → Task
    pub fn rebuild_timeline_rows(&mut self) {
        let timeline = match &self.build_timeline {
            Some(t) => t,
            None => {
                self.timeline_rows.clear();
                return;
            }
        };

        let mut children_of: HashMap<String, Vec<&crate::api::models::TimelineRecord>> =
            HashMap::new();
        let mut stages = Vec::new();

        for rec in &timeline.records {
            match rec.record_type.as_str() {
                "Stage" if rec.parent_id.is_none() => {
                    stages.push(rec);
                }
                _ => {
                    if let Some(pid) = &rec.parent_id {
                        children_of.entry(pid.clone()).or_default().push(rec);
                    }
                }
            }
        }

        stages.sort_by_key(|s| s.order.unwrap_or(999));

        // Pre-collapse all on first load for a compact overview
        if !self.timeline_initialized {
            self.timeline_initialized = true;
            for stage in &stages {
                self.collapsed_stages.insert(stage.id.clone());
                if let Some(phase_children) = children_of.get(&stage.id) {
                    for child in phase_children {
                        if child.record_type == "Phase" || child.record_type == "Job" {
                            self.collapsed_jobs.insert(child.id.clone());
                        }
                    }
                }
            }
        }

        let mut rows = Vec::new();

        for stage in &stages {
            let stage_collapsed = self.collapsed_stages.contains(&stage.id);
            rows.push(TimelineRow::Stage {
                id: stage.id.clone(),
                identifier: stage.identifier.clone(),
                name: stage.name.clone(),
                state: stage.state,
                result: stage.result,
                collapsed: stage_collapsed,
            });

            if stage_collapsed {
                continue;
            }

            // Insert checkpoint rows (approval gates) before jobs
            if let Some(stage_children) = children_of.get(&stage.id) {
                for child in stage_children.iter() {
                    if child.record_type == "Checkpoint"
                        && let Some(cp_children) = children_of.get(&child.id)
                    {
                        for cp_child in cp_children.iter() {
                            if cp_child.record_type.starts_with("Checkpoint.Approval") {
                                rows.push(TimelineRow::Checkpoint {
                                    name: cp_child.name.clone(),
                                    record_type: cp_child.record_type.clone(),
                                    state: cp_child.state,
                                    result: cp_child.result,
                                    approval_id: cp_child.identifier.clone(),
                                    parent_stage_id: stage.id.clone(),
                                });
                            }
                        }
                    }
                }
            }

            let mut phases: Vec<_> = children_of
                .get(&stage.id)
                .map(|v| {
                    v.iter()
                        .filter(|r| r.record_type == "Phase" || r.record_type == "Job")
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            phases.sort_by_key(|p| p.order.unwrap_or(999));

            for phase in &phases {
                let phase_collapsed = self.collapsed_jobs.contains(&phase.id);
                rows.push(TimelineRow::Job {
                    id: phase.id.clone(),
                    name: phase.name.clone(),
                    state: phase.state,
                    result: phase.result,
                    collapsed: phase_collapsed,
                    parent_stage_id: stage.id.clone(),
                });

                if phase_collapsed {
                    continue;
                }

                let mut tasks: Vec<&crate::api::models::TimelineRecord> = Vec::new();

                if let Some(phase_children) = children_of.get(&phase.id) {
                    for child in phase_children {
                        match child.record_type.as_str() {
                            "Task" => tasks.push(child),
                            "Job" | "Phase" => {
                                if let Some(job_children) = children_of.get(&child.id) {
                                    for grandchild in job_children {
                                        if grandchild.record_type == "Task" {
                                            tasks.push(grandchild);
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }

                tasks.sort_by_key(|t| t.order.unwrap_or(999));

                for task in &tasks {
                    rows.push(TimelineRow::Task {
                        name: task.name.clone(),
                        state: task.state,
                        result: task.result,
                        log_id: task.log.as_ref().map(|l| l.id),
                        parent_job_id: phase.id.clone(),
                    });
                }
            }
        }

        self.timeline_rows = rows;
        self.log_entries_nav.set_len(self.timeline_rows.len());
    }

    /// Toggle collapse for a timeline stage or job at the given row index.
    pub fn toggle_timeline_node(&mut self, index: usize) -> bool {
        if let Some(row) = self.timeline_rows.get(index) {
            match row {
                TimelineRow::Stage { id, .. } => {
                    let id = id.clone();
                    self.toggle_stage(&id);
                    self.rebuild_timeline_rows();
                    return true;
                }
                TimelineRow::Job { id, .. } => {
                    let id = id.clone();
                    self.toggle_job(&id);
                    self.rebuild_timeline_rows();
                    return true;
                }
                TimelineRow::Task { .. } | TimelineRow::Checkpoint { .. } => {}
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
                    self.collapse_stage(id);
                    self.rebuild_timeline_rows();
                    return true;
                }
                TimelineRow::Job { id, collapsed, .. } if !collapsed => {
                    let id = id.clone();
                    self.collapse_job(id);
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
                    self.expand_stage(&id);
                    self.rebuild_timeline_rows();
                    return true;
                }
                TimelineRow::Job { id, collapsed, .. } if *collapsed => {
                    let id = id.clone();
                    self.expand_job(&id);
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
                    return self
                        .timeline_rows
                        .iter()
                        .enumerate()
                        .rev()
                        .take(index + 1)
                        .find(|(_, r)| {
                            matches!(r, TimelineRow::Job { id, .. } if id == parent_job_id)
                        })
                        .map(|(i, _)| i);
                }
                TimelineRow::Job {
                    parent_stage_id, ..
                } => {
                    return self
                        .timeline_rows
                        .iter()
                        .enumerate()
                        .rev()
                        .take(index + 1)
                        .find(|(_, r)| {
                            matches!(r, TimelineRow::Stage { id, .. } if id == parent_stage_id)
                        })
                        .map(|(i, _)| i);
                }
                TimelineRow::Stage { .. } | TimelineRow::Checkpoint { .. } => {}
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
            TimelineRow::Checkpoint { .. } => "checkpoint",
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

    /// Get the stage ref name (identifier) for a Stage timeline row at the given index.
    pub fn timeline_stage_ref_name(&self, index: usize) -> Option<String> {
        if let Some(TimelineRow::Stage {
            identifier, name, ..
        }) = self.timeline_rows.get(index)
        {
            Some(identifier.as_ref().unwrap_or(name).clone())
        } else {
            None
        }
    }

    /// Get the approval ID for a Checkpoint timeline row at the given index.
    pub fn timeline_approval_id(&self, index: usize) -> Option<String> {
        if let Some(TimelineRow::Checkpoint { approval_id, .. }) = self.timeline_rows.get(index) {
            approval_id.clone()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::{
        Build, BuildDefinitionRef, BuildResult, BuildStatus, BuildTimeline, LogReference,
        TaskState, TimelineRecord,
    };

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn make_record(
        id: &str,
        parent_id: Option<&str>,
        name: &str,
        record_type: &str,
        order: i32,
        state: Option<TaskState>,
        result: Option<BuildResult>,
        log_id: Option<u32>,
    ) -> TimelineRecord {
        TimelineRecord {
            id: id.to_string(),
            parent_id: parent_id.map(|s| s.to_string()),
            name: name.to_string(),
            identifier: None,
            record_type: record_type.to_string(),
            state,
            result,
            order: Some(order),
            log: log_id.map(|id| LogReference { id }),
        }
    }

    fn make_test_build(status: BuildStatus, result: Option<BuildResult>) -> Build {
        Build {
            id: 1,
            build_number: "1".to_string(),
            status,
            result,
            queue_time: None,
            start_time: None,
            finish_time: None,
            definition: BuildDefinitionRef {
                id: 1,
                name: "test".to_string(),
            },
            source_branch: Some("refs/heads/main".to_string()),
            requested_for: None,
        }
    }

    /// Build a simple timeline: 1 stage -> 1 phase -> N tasks.
    fn simple_timeline(
        tasks: Vec<(&str, Option<TaskState>, Option<BuildResult>, u32)>,
    ) -> BuildTimeline {
        let mut records = vec![
            make_record(
                "s1",
                None,
                "Build",
                "Stage",
                1,
                Some(TaskState::InProgress),
                None,
                None,
            ),
            make_record(
                "p1",
                Some("s1"),
                "Job 1",
                "Phase",
                1,
                Some(TaskState::InProgress),
                None,
                None,
            ),
        ];
        for (i, (name, state, result, log_id)) in tasks.iter().enumerate() {
            records.push(make_record(
                &format!("t{}", i + 1),
                Some("p1"),
                name,
                "Task",
                (i + 1) as i32,
                *state,
                *result,
                Some(*log_id),
            ));
        }
        BuildTimeline { records }
    }

    /// Create a LogViewerState with a build, set timeline, and expand all nodes.
    fn state_with_expanded_timeline(
        build_status: BuildStatus,
        build_result: Option<BuildResult>,
        timeline: BuildTimeline,
    ) -> LogViewerState {
        let build = make_test_build(build_status, build_result);
        let mut state = LogViewerState::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        state.rebuild_timeline_rows();
        let stage_ids: Vec<String> = state.collapsed_stages.iter().cloned().collect();
        for id in &stage_ids {
            state.expand_stage(id);
        }
        let job_ids: Vec<String> = state.collapsed_jobs.iter().cloned().collect();
        for id in &job_ids {
            state.expand_job(id);
        }
        state.rebuild_timeline_rows();
        state
    }

    // =======================================================================
    // Group 1: State API tests
    // =======================================================================

    #[test]
    fn default_state_is_empty() {
        let state = LogViewerState::default();
        assert!(state.selected_build().is_none());
        assert!(state.timeline_rows().is_empty());
        assert!(!state.is_following());
        assert_eq!(state.generation(), 0);
        assert!(state.log_content().is_empty());
        assert!(!state.log_auto_scroll());
    }

    #[test]
    fn new_for_build_sets_fields() {
        let build = make_test_build(BuildStatus::InProgress, None);
        let state = LogViewerState::new_for_build(build, View::BuildHistory, 42);
        assert!(state.selected_build().is_some());
        assert_eq!(state.selected_build().unwrap().id, 1);
        assert!(state.is_following());
        assert!(state.log_auto_scroll());
        assert_eq!(state.generation(), 42);
        assert_eq!(state.return_to_view(), View::BuildHistory);
    }

    #[test]
    fn enter_follow_and_inspect_modes() {
        let build = make_test_build(BuildStatus::InProgress, None);
        let mut state = LogViewerState::new_for_build(build, View::BuildHistory, 1);
        assert!(state.is_following());
        state.enter_inspect_mode();
        assert!(!state.is_following());
        state.enter_follow_mode();
        assert!(state.is_following());
    }

    #[test]
    fn set_followed_updates_both() {
        let mut state = LogViewerState::default();
        state.set_followed("Initialize".to_string(), 42);
        assert_eq!(state.followed_task_name(), "Initialize");
        assert_eq!(state.followed_log_id(), Some(42));
    }

    #[test]
    fn scroll_up_and_down() {
        let mut state = LogViewerState::default();
        assert_eq!(state.log_scroll_offset(), 0);
        state.scroll_down(5);
        assert_eq!(state.log_scroll_offset(), 5);
        state.scroll_down(3);
        assert_eq!(state.log_scroll_offset(), 8);
        state.scroll_up(2);
        assert_eq!(state.log_scroll_offset(), 6);
        assert!(!state.log_auto_scroll());
        state.scroll_up(100);
        assert_eq!(state.log_scroll_offset(), 0);
    }

    #[test]
    fn set_log_content_splits_lines_and_resets_scroll() {
        let mut state = LogViewerState::default();
        state.scroll_down(10);
        state.set_log_content("line1\nline2\nline3".to_string());
        assert_eq!(state.log_content(), &["line1", "line2", "line3"]);
        assert!(state.log_auto_scroll());
        assert_eq!(state.log_scroll_offset(), 0);
    }

    #[test]
    fn clear_log_empties_content() {
        let mut state = LogViewerState::default();
        state.set_log_content("some log\ndata".to_string());
        assert!(!state.log_content().is_empty());
        state.clear_log();
        assert!(state.log_content().is_empty());
    }

    #[test]
    fn set_generation_updates() {
        let mut state = LogViewerState::default();
        assert_eq!(state.generation(), 0);
        state.set_generation(99);
        assert_eq!(state.generation(), 99);
    }

    // =======================================================================
    // Group 2: Timeline tree building tests
    // =======================================================================

    #[test]
    fn rebuild_timeline_basic_structure() {
        let timeline = simple_timeline(vec![
            (
                "Task A",
                Some(TaskState::Completed),
                Some(BuildResult::Succeeded),
                10,
            ),
            ("Task B", Some(TaskState::InProgress), None, 11),
        ]);
        let build = make_test_build(BuildStatus::InProgress, None);
        let mut state = LogViewerState::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);

        // First rebuild pre-collapses all stages
        state.rebuild_timeline_rows();
        assert_eq!(state.timeline_rows().len(), 1);
        assert!(matches!(
            state.timeline_rows()[0],
            TimelineRow::Stage {
                collapsed: true,
                ..
            }
        ));

        // Expand stage and job
        state.expand_stage("s1");
        state.expand_job("p1");
        state.rebuild_timeline_rows();

        assert_eq!(state.timeline_rows().len(), 4);
        assert!(
            matches!(&state.timeline_rows()[0], TimelineRow::Stage { name, .. } if name == "Build")
        );
        assert!(
            matches!(&state.timeline_rows()[1], TimelineRow::Job { name, .. } if name == "Job 1")
        );
        assert!(
            matches!(&state.timeline_rows()[2], TimelineRow::Task { name, .. } if name == "Task A")
        );
        assert!(
            matches!(&state.timeline_rows()[3], TimelineRow::Task { name, .. } if name == "Task B")
        );
    }

    #[test]
    fn rebuild_timeline_respects_order() {
        let timeline = BuildTimeline {
            records: vec![
                make_record(
                    "s2",
                    None,
                    "Deploy",
                    "Stage",
                    2,
                    Some(TaskState::Pending),
                    None,
                    None,
                ),
                make_record(
                    "s1",
                    None,
                    "Build",
                    "Stage",
                    1,
                    Some(TaskState::InProgress),
                    None,
                    None,
                ),
            ],
        };
        let build = make_test_build(BuildStatus::InProgress, None);
        let mut state = LogViewerState::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        state.rebuild_timeline_rows();

        assert_eq!(state.timeline_rows().len(), 2);
        assert!(
            matches!(&state.timeline_rows()[0], TimelineRow::Stage { name, .. } if name == "Build")
        );
        assert!(
            matches!(&state.timeline_rows()[1], TimelineRow::Stage { name, .. } if name == "Deploy")
        );
    }

    #[test]
    fn rebuild_timeline_pre_collapses_on_first_load() {
        let timeline = simple_timeline(vec![(
            "Task A",
            Some(TaskState::Completed),
            Some(BuildResult::Succeeded),
            10,
        )]);
        let build = make_test_build(BuildStatus::InProgress, None);
        let mut state = LogViewerState::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);

        assert!(!state.is_timeline_initialized());
        state.rebuild_timeline_rows();
        assert!(state.is_timeline_initialized());
        assert_eq!(state.timeline_rows().len(), 1);
        assert!(state.is_stage_collapsed("s1"));
        assert!(state.is_job_collapsed("p1"));
    }

    #[test]
    fn toggle_expand_collapse_stage() {
        let timeline = simple_timeline(vec![(
            "Task A",
            Some(TaskState::Completed),
            Some(BuildResult::Succeeded),
            10,
        )]);
        let build = make_test_build(BuildStatus::InProgress, None);
        let mut state = LogViewerState::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        state.rebuild_timeline_rows();
        assert_eq!(state.timeline_rows().len(), 1);

        state.toggle_timeline_node(0);
        assert!(state.timeline_rows().len() >= 2);

        state.toggle_timeline_node(1);
        assert_eq!(state.timeline_rows().len(), 3);

        state.toggle_timeline_node(0);
        assert_eq!(state.timeline_rows().len(), 1);
    }

    #[test]
    fn find_timeline_parent_index_task_to_job() {
        let timeline = simple_timeline(vec![(
            "Task A",
            Some(TaskState::Completed),
            Some(BuildResult::Succeeded),
            10,
        )]);
        let state = state_with_expanded_timeline(BuildStatus::InProgress, None, timeline);
        assert_eq!(state.timeline_rows().len(), 3);
        assert_eq!(state.find_timeline_parent_index(2), Some(1));
    }

    #[test]
    fn find_timeline_parent_index_job_to_stage() {
        let timeline = BuildTimeline {
            records: vec![
                make_record(
                    "s1",
                    None,
                    "Build",
                    "Stage",
                    1,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    None,
                ),
                make_record(
                    "p1",
                    Some("s1"),
                    "Job 1",
                    "Phase",
                    1,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    None,
                ),
                make_record(
                    "t1",
                    Some("p1"),
                    "Task A",
                    "Task",
                    1,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    Some(10),
                ),
                make_record(
                    "t2",
                    Some("p1"),
                    "Task B",
                    "Task",
                    2,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    Some(11),
                ),
                make_record(
                    "s2",
                    None,
                    "Deploy",
                    "Stage",
                    2,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    None,
                ),
                make_record(
                    "p2",
                    Some("s2"),
                    "Job 2",
                    "Phase",
                    1,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    None,
                ),
                make_record(
                    "t3",
                    Some("p2"),
                    "Task C",
                    "Task",
                    1,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    Some(12),
                ),
                make_record(
                    "t4",
                    Some("p2"),
                    "Task D",
                    "Task",
                    2,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    Some(13),
                ),
            ],
        };
        let state = state_with_expanded_timeline(
            BuildStatus::Completed,
            Some(BuildResult::Succeeded),
            timeline,
        );
        // Rows: [Stage(0), Job(1), TaskA(2), TaskB(3), Stage(4), Job(5), TaskC(6), TaskD(7)]
        assert_eq!(state.find_timeline_parent_index(5), Some(4));
    }

    #[test]
    fn timeline_row_kind_returns_correct_type() {
        let timeline = simple_timeline(vec![(
            "Task A",
            Some(TaskState::Completed),
            Some(BuildResult::Succeeded),
            10,
        )]);
        let state = state_with_expanded_timeline(BuildStatus::InProgress, None, timeline);
        assert_eq!(state.timeline_row_kind(0), Some("stage"));
        assert_eq!(state.timeline_row_kind(1), Some("job"));
        assert_eq!(state.timeline_row_kind(2), Some("task"));
        assert_eq!(state.timeline_row_kind(99), None);
    }

    #[test]
    fn timeline_task_log_id_returns_correct_id() {
        let timeline = simple_timeline(vec![(
            "Task A",
            Some(TaskState::Completed),
            Some(BuildResult::Succeeded),
            42,
        )]);
        let state = state_with_expanded_timeline(BuildStatus::InProgress, None, timeline);
        assert_eq!(state.timeline_task_log_id(2), Some(42));
        assert_eq!(state.timeline_task_log_id(0), None);
        assert_eq!(state.timeline_task_log_id(1), None);
    }

    #[test]
    fn timeline_nav_length_synced_after_rebuild() {
        let timeline = simple_timeline(vec![
            (
                "Task A",
                Some(TaskState::Completed),
                Some(BuildResult::Succeeded),
                10,
            ),
            ("Task B", Some(TaskState::InProgress), None, 11),
        ]);
        let state = state_with_expanded_timeline(BuildStatus::InProgress, None, timeline);
        assert_eq!(state.nav().len(), state.timeline_rows().len());
        assert_eq!(state.nav().len(), 4);
        assert!(state.nav().index() < state.nav().len());
    }

    // =======================================================================
    // Group 3: Build status from timeline
    // =======================================================================

    #[test]
    fn refresh_status_all_succeeded() {
        let timeline = BuildTimeline {
            records: vec![
                make_record(
                    "s1",
                    None,
                    "Build",
                    "Stage",
                    1,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    None,
                ),
                make_record(
                    "s2",
                    None,
                    "Deploy",
                    "Stage",
                    2,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    None,
                ),
            ],
        };
        let build = make_test_build(BuildStatus::InProgress, None);
        let mut state = LogViewerState::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        state.refresh_build_status_from_timeline();
        let b = state.selected_build().unwrap();
        assert_eq!(b.status, BuildStatus::Completed);
        assert_eq!(b.result, Some(BuildResult::Succeeded));
    }

    #[test]
    fn refresh_status_one_failed() {
        let timeline = BuildTimeline {
            records: vec![
                make_record(
                    "s1",
                    None,
                    "Build",
                    "Stage",
                    1,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    None,
                ),
                make_record(
                    "s2",
                    None,
                    "Deploy",
                    "Stage",
                    2,
                    Some(TaskState::Completed),
                    Some(BuildResult::Failed),
                    None,
                ),
            ],
        };
        let build = make_test_build(BuildStatus::InProgress, None);
        let mut state = LogViewerState::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        state.refresh_build_status_from_timeline();
        let b = state.selected_build().unwrap();
        assert_eq!(b.status, BuildStatus::Completed);
        assert_eq!(b.result, Some(BuildResult::Failed));
    }

    #[test]
    fn refresh_status_partial() {
        let timeline = BuildTimeline {
            records: vec![
                make_record(
                    "s1",
                    None,
                    "Build",
                    "Stage",
                    1,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    None,
                ),
                make_record(
                    "s2",
                    None,
                    "Deploy",
                    "Stage",
                    2,
                    Some(TaskState::Completed),
                    Some(BuildResult::PartiallySucceeded),
                    None,
                ),
            ],
        };
        let build = make_test_build(BuildStatus::InProgress, None);
        let mut state = LogViewerState::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        state.refresh_build_status_from_timeline();
        let b = state.selected_build().unwrap();
        assert_eq!(b.status, BuildStatus::Completed);
        assert_eq!(b.result, Some(BuildResult::PartiallySucceeded));
    }

    #[test]
    fn refresh_status_noop_when_already_completed() {
        let timeline = BuildTimeline {
            records: vec![make_record(
                "s1",
                None,
                "Build",
                "Stage",
                1,
                Some(TaskState::Completed),
                Some(BuildResult::Succeeded),
                None,
            )],
        };
        let build = make_test_build(BuildStatus::Completed, Some(BuildResult::Succeeded));
        let mut state = LogViewerState::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        state.refresh_build_status_from_timeline();
        let b = state.selected_build().unwrap();
        assert_eq!(b.status, BuildStatus::Completed);
        assert_eq!(b.result, Some(BuildResult::Succeeded));
    }

    #[test]
    fn refresh_status_noop_when_no_timeline() {
        let build = make_test_build(BuildStatus::InProgress, None);
        let mut state = LogViewerState::new_for_build(build, View::BuildHistory, 1);
        state.refresh_build_status_from_timeline();
        let b = state.selected_build().unwrap();
        assert_eq!(b.status, BuildStatus::InProgress);
        assert!(b.result.is_none());
    }

    #[test]
    fn refresh_status_noop_when_stages_still_running() {
        let timeline = BuildTimeline {
            records: vec![
                make_record(
                    "s1",
                    None,
                    "Build",
                    "Stage",
                    1,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    None,
                ),
                make_record(
                    "s2",
                    None,
                    "Deploy",
                    "Stage",
                    2,
                    Some(TaskState::InProgress),
                    None,
                    None,
                ),
            ],
        };
        let build = make_test_build(BuildStatus::InProgress, None);
        let mut state = LogViewerState::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        state.refresh_build_status_from_timeline();
        let b = state.selected_build().unwrap();
        assert_eq!(b.status, BuildStatus::InProgress);
        assert!(b.result.is_none());
    }

    // =======================================================================
    // Group 4: Auto-select and find_active_task
    // =======================================================================

    #[test]
    fn auto_select_picks_in_progress_task_for_running_build() {
        let timeline = simple_timeline(vec![
            (
                "Init",
                Some(TaskState::Completed),
                Some(BuildResult::Succeeded),
                10,
            ),
            ("Build", Some(TaskState::InProgress), None, 11),
            ("Test", Some(TaskState::Pending), None, 12),
        ]);
        let build = make_test_build(BuildStatus::InProgress, None);
        let mut state = LogViewerState::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        state.rebuild_timeline_rows();

        let result = state.auto_select_log_entry();
        assert!(result.is_some());
        let (_, log_id) = result.unwrap();
        assert_eq!(log_id, 11);
    }

    #[test]
    fn auto_select_picks_failed_task_for_completed_build() {
        let timeline = simple_timeline(vec![
            (
                "Init",
                Some(TaskState::Completed),
                Some(BuildResult::Succeeded),
                10,
            ),
            (
                "Build",
                Some(TaskState::Completed),
                Some(BuildResult::Failed),
                11,
            ),
            (
                "Cleanup",
                Some(TaskState::Completed),
                Some(BuildResult::Succeeded),
                12,
            ),
        ]);
        let build = make_test_build(BuildStatus::Completed, Some(BuildResult::Failed));
        let mut state = LogViewerState::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        state.rebuild_timeline_rows();

        let result = state.auto_select_log_entry();
        assert!(result.is_some());
        let (_, log_id) = result.unwrap();
        assert_eq!(log_id, 11);
    }

    #[test]
    fn find_active_task_returns_in_progress() {
        let timeline = simple_timeline(vec![
            (
                "Init",
                Some(TaskState::Completed),
                Some(BuildResult::Succeeded),
                10,
            ),
            ("Build", Some(TaskState::InProgress), None, 11),
        ]);
        let build = make_test_build(BuildStatus::InProgress, None);
        let mut state = LogViewerState::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        let result = state.find_active_task();
        assert!(result.is_some());
        let (name, log_id) = result.unwrap();
        assert_eq!(name, "Build");
        assert_eq!(log_id, 11);
    }

    #[test]
    fn find_active_task_returns_none_when_no_timeline() {
        let build = make_test_build(BuildStatus::InProgress, None);
        let state = LogViewerState::new_for_build(build, View::BuildHistory, 1);
        assert!(state.find_active_task().is_none());
    }

    // =======================================================================
    // Group 5: Checkpoint tests
    // =======================================================================

    #[test]
    fn rebuild_timeline_includes_checkpoints() {
        let mut approval_record = make_record(
            "ap1",
            Some("cp1"),
            "Waiting for approval",
            "Checkpoint.Approval",
            1,
            Some(TaskState::Completed),
            Some(BuildResult::Succeeded),
            None,
        );
        approval_record.identifier = Some("approval-gate-1".to_string());

        let timeline = BuildTimeline {
            records: vec![
                make_record(
                    "s1",
                    None,
                    "Deploy",
                    "Stage",
                    1,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    None,
                ),
                make_record(
                    "p1",
                    Some("s1"),
                    "Job 1",
                    "Phase",
                    1,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    None,
                ),
                make_record(
                    "t1",
                    Some("p1"),
                    "Init",
                    "Task",
                    1,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    Some(10),
                ),
                make_record(
                    "cp1",
                    Some("s1"),
                    "Checkpoint",
                    "Checkpoint",
                    0,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    None,
                ),
                approval_record,
            ],
        };
        let build = make_test_build(BuildStatus::Completed, Some(BuildResult::Succeeded));
        let mut state = LogViewerState::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        state.rebuild_timeline_rows();
        state.expand_stage("s1");
        state.expand_job("p1");
        state.rebuild_timeline_rows();

        let kinds: Vec<&str> = state
            .timeline_rows()
            .iter()
            .map(|r| match r {
                TimelineRow::Stage { .. } => "stage",
                TimelineRow::Job { .. } => "job",
                TimelineRow::Task { .. } => "task",
                TimelineRow::Checkpoint { .. } => "checkpoint",
            })
            .collect();
        assert!(
            kinds.contains(&"checkpoint"),
            "Expected checkpoint, got: {kinds:?}"
        );
        assert!(kinds.contains(&"task"), "Expected task, got: {kinds:?}");
    }

    #[test]
    fn timeline_approval_id_returns_identifier() {
        let mut approval_record = make_record(
            "ap1",
            Some("cp1"),
            "Waiting for approval",
            "Checkpoint.Approval",
            1,
            Some(TaskState::Completed),
            Some(BuildResult::Succeeded),
            None,
        );
        approval_record.identifier = Some("approval-gate-1".to_string());

        let timeline = BuildTimeline {
            records: vec![
                make_record(
                    "s1",
                    None,
                    "Deploy",
                    "Stage",
                    1,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    None,
                ),
                make_record(
                    "cp1",
                    Some("s1"),
                    "Checkpoint",
                    "Checkpoint",
                    0,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    None,
                ),
                approval_record,
            ],
        };
        let build = make_test_build(BuildStatus::Completed, Some(BuildResult::Succeeded));
        let mut state = LogViewerState::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        state.rebuild_timeline_rows();
        state.expand_stage("s1");
        state.rebuild_timeline_rows();

        let cp_idx = state
            .timeline_rows()
            .iter()
            .position(|r| matches!(r, TimelineRow::Checkpoint { .. }));
        assert!(cp_idx.is_some(), "Expected a checkpoint row");
        assert_eq!(
            state.timeline_approval_id(cp_idx.unwrap()),
            Some("approval-gate-1".to_string())
        );
    }
}
