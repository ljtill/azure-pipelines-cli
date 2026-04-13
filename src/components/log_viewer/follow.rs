//! Follow and inspect mode logic for the log viewer.

use crate::client::models::{BuildResult, BuildStatus, TaskState};

use super::LogViewer;
use super::TimelineRow;

/// Result of searching the timeline for the active task during follow mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveTaskResult {
    /// An in-progress task with a fetchable log.
    Found { name: String, log_id: u32 },
    /// An in-progress task exists but has no log yet — the step just started.
    Pending { name: String },
    /// No in-progress task — build is done or idle.
    None,
}

/// A task record extracted from the timeline for auto-selection.
struct TaskCandidate {
    name: String,
    state: Option<TaskState>,
    result: Option<BuildResult>,
    parent_id: Option<String>,
    log_id: u32,
}

// --- Follow / inspect mode transitions ---
impl LogViewer {
    pub fn enter_follow_mode(&mut self) {
        self.follow_mode = true;
    }

    pub fn enter_inspect_mode(&mut self) {
        self.follow_mode = false;
    }

    pub fn set_followed(&mut self, task_name: String, log_id: u32) {
        self.followed_task_name = task_name;
        self.followed_log_id = Some(log_id);
    }

    /// Sets the followed task name when the task has no log yet.
    /// Clears `followed_log_id` so the next refresh picks up the log once it appears.
    pub fn set_followed_pending(&mut self, task_name: String) {
        self.followed_task_name = task_name;
        self.followed_log_id = None;
    }
}

// --- Auto-select and active-task detection ---
impl LogViewer {
    /// Picks the most relevant timeline task to auto-show logs for.
    /// Returns `(row_index, Some(log_id))` for tasks with logs, or
    /// `(row_index, None)` for in-progress tasks whose log hasn't appeared yet.
    /// Ensures parent stage/job are expanded.
    pub fn auto_select_log_entry(&mut self) -> Option<(usize, Option<u32>)> {
        let timeline = self.build_timeline.as_ref()?;

        // Collect candidates with logs for the normal path.
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

        let is_running = self
            .selected_build
            .as_ref()
            .is_some_and(|b| b.status.is_in_progress());

        // For running builds, also check for InProgress tasks without logs.
        let pending_task = if is_running {
            timeline
                .records
                .iter()
                .rev()
                .find(|r| {
                    r.record_type == "Task"
                        && r.log.is_none()
                        && r.state == Some(TaskState::InProgress)
                })
                .map(|r| (r.name.clone(), r.parent_id.clone()))
        } else {
            Option::None
        };

        // Determine the best task among candidates with logs.
        let best_with_log = if tasks.is_empty() {
            Option::None
        } else if is_running {
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

        // Prefer an InProgress task with a log; fall back to a pending InProgress
        // task; then fall back to the best completed task.
        let (best_name, parent_job_id, log_id) = if let Some(idx) =
            best_with_log.filter(|&i| tasks[i].state == Some(TaskState::InProgress))
        {
            let t = &tasks[idx];
            (t.name.clone(), t.parent_id.clone(), Some(t.log_id))
        } else if let Some((name, parent_id)) = pending_task {
            (name, parent_id, Option::None)
        } else if let Some(idx) = best_with_log {
            let t = &tasks[idx];
            (t.name.clone(), t.parent_id.clone(), Some(t.log_id))
        } else {
            return Option::None;
        };

        // Walk up the ancestor chain to expand all parent nodes.
        if let Some(timeline) = self.build_timeline.as_ref() {
            let records = &timeline.records;
            #[allow(clippy::redundant_clone)] // Used in closure below.
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

        // Find the row by log_id first, then by name + parent.
        let row_idx = if let Some(lid) = log_id {
            self.timeline_rows.iter().position(
                |row| matches!(row, TimelineRow::Task { log_id: Some(id), .. } if *id == lid),
            )
        } else {
            Option::None
        }
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
            Option::None
        }
    }

    /// Finds the currently active task without moving cursor or changing state.
    pub fn find_active_task(&self) -> ActiveTaskResult {
        let Some(timeline) = self.build_timeline.as_ref() else {
            return ActiveTaskResult::None;
        };

        let tasks_with_logs: Vec<_> = timeline
            .records
            .iter()
            .filter(|r| r.record_type == "Task" && r.log.is_some())
            .collect();

        let is_running = self
            .selected_build
            .as_ref()
            .is_some_and(|b| b.status.is_in_progress());

        if is_running {
            // First, look for an InProgress task that already has a log.
            if let Some(best) = tasks_with_logs
                .iter()
                .rev()
                .find(|r| r.state.is_some_and(TaskState::is_in_progress))
            {
                let log_id = best.log.as_ref().unwrap().id;
                return ActiveTaskResult::Found {
                    name: best.name.clone(),
                    log_id,
                };
            }

            // No InProgress task with a log — check if one exists without a log.
            if let Some(pending) = timeline.records.iter().rev().find(|r| {
                r.record_type == "Task"
                    && r.log.is_none()
                    && r.state.is_some_and(TaskState::is_in_progress)
            }) {
                return ActiveTaskResult::Pending {
                    name: pending.name.clone(),
                };
            }

            // No InProgress task at all — fall back to last task with a log.
            if let Some(best) = tasks_with_logs.last() {
                let log_id = best.log.as_ref().unwrap().id;
                return ActiveTaskResult::Found {
                    name: best.name.clone(),
                    log_id,
                };
            }

            ActiveTaskResult::None
        } else {
            // Build completed — find the failed task, or fall back to the last task.
            let best = tasks_with_logs
                .iter()
                .rev()
                .find(|r| r.result == Some(BuildResult::Failed))
                .or_else(|| tasks_with_logs.last());

            best.map_or(ActiveTaskResult::None, |best| {
                let log_id = best.log.as_ref().unwrap().id;
                ActiveTaskResult::Found {
                    name: best.name.clone(),
                    log_id,
                }
            })
        }
    }
}

// --- Build status derived from timeline ---
impl LogViewer {
    /// Updates `selected_build` status/result from timeline records.
    /// Called on each timeline refresh so the log viewer header stays current.
    pub fn refresh_build_status_from_timeline(&mut self) {
        let Some(timeline) = &self.build_timeline else {
            return;
        };
        let Some(build) = &mut self.selected_build else {
            return;
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
