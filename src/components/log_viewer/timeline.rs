use std::collections::HashMap;

use crate::api::models::{BuildResult, TaskState};

use super::LogViewer;

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

// ---------------------------------------------------------------------------
// Timeline tree building & queries
// ---------------------------------------------------------------------------

impl LogViewer {
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
                        .take(index)
                        .rev()
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
                        .take(index)
                        .rev()
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
