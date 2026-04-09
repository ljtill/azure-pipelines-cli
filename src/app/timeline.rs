use std::collections::HashMap;

use crate::api::models::{BuildResult, TaskState};

use super::App;

type TaskEntry = (
    String,
    Option<TaskState>,
    Option<BuildResult>,
    Option<String>,
    u32,
);

/// A row in the timeline tree view — Stage, Job, or Task.
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
}

impl App {
    /// Pick the most relevant timeline task to auto-show logs for.
    /// Returns (row_index, log_id) if found. Ensures parent stage/job are expanded.
    pub fn auto_select_log_entry(&mut self) -> Option<(usize, u32)> {
        let timeline = self.build_timeline.as_ref()?;
        let tasks: Vec<TaskEntry> = timeline
            .records
            .iter()
            .filter(|r| r.record_type == "Task" && r.log.is_some())
            .map(|r| {
                (
                    r.name.clone(),
                    r.state,
                    r.result,
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
            .is_some_and(|b| b.status.is_in_progress());

        // Find best task index: in-progress > failed > last
        let best_idx = if is_running {
            tasks
                .iter()
                .rposition(|t| t.1 == Some(TaskState::InProgress))
                .or(Some(tasks.len() - 1))
        } else {
            tasks
                .iter()
                .rposition(|t| t.2 == Some(BuildResult::Failed))
                .or(Some(tasks.len() - 1))
        };
        let best_idx = best_idx?;
        let (best_name, _, _, parent_job_id, log_id) = &tasks[best_idx];
        let log_id = *log_id;
        let best_name = best_name.clone();
        let parent_job_id = parent_job_id.clone();

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
            self.log_entries_index = idx;
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
}
