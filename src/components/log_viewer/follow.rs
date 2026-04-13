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
    state: Option<TaskState>,
    result: Option<BuildResult>,
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
    /// Ensures parent stage/job are expanded and the cursor is positioned.
    pub fn auto_select_log_entry(&mut self) -> Option<(usize, Option<u32>)> {
        let timeline = self.build_timeline.as_ref()?;

        // Collect candidates with logs for the normal path.
        let tasks: Vec<TaskCandidate> = timeline
            .records
            .iter()
            .filter(|r| r.record_type == "Task" && r.log.is_some())
            .map(|r| TaskCandidate {
                state: r.state,
                result: r.result,
                log_id: r.log.as_ref().unwrap().id,
            })
            .collect();

        let is_running = self
            .selected_build
            .as_ref()
            .is_some_and(|b| b.status.is_in_progress());

        // For running builds, also check for InProgress tasks without logs.
        // Forward search: prefer the first stage's task.
        let pending_task = if is_running {
            timeline
                .records
                .iter()
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
        // Forward search: for running builds, find the first InProgress task
        // (first stage when parallel stages are active).
        let best_with_log = if tasks.is_empty() {
            Option::None
        } else if is_running {
            tasks
                .iter()
                .position(|t| t.state == Some(TaskState::InProgress))
                .or(Some(tasks.len() - 1))
        } else {
            tasks
                .iter()
                .rposition(|t| t.result == Some(BuildResult::Failed))
                .or(Some(tasks.len() - 1))
        };

        // Prefer an InProgress task with a log; fall back to a pending InProgress
        // task; then fall back to the best completed task.
        let in_progress_log = best_with_log
            .filter(|&i| tasks[i].state == Some(TaskState::InProgress))
            .map(|i| tasks[i].log_id);
        let log_id = in_progress_log.or_else(|| {
            if pending_task.is_some() {
                // Pending task: no log yet, wait for it to appear.
                return Option::None;
            }
            best_with_log.map(|idx| tasks[idx].log_id)
        });

        if log_id.is_none() && pending_task.is_none() && best_with_log.is_none() {
            return Option::None;
        }

        // Set followed state and jump cursor.
        if let Some(lid) = log_id {
            // Temporarily set followed so jump_to_followed_task can find the row.
            self.followed_log_id = Some(lid);
        } else {
            self.followed_log_id = None;
        }
        self.jump_to_followed_task();

        let idx = self.log_entries_nav.index();
        Some((idx, log_id))
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
            // Forward search: find the first InProgress task (first stage).
            if let Some(best) = tasks_with_logs
                .iter()
                .find(|r| r.state.is_some_and(TaskState::is_in_progress))
            {
                let log_id = best.log.as_ref().unwrap().id;
                return ActiveTaskResult::Found {
                    name: best.name.clone(),
                    log_id,
                };
            }

            // No InProgress task with a log — check if one exists without a log.
            if let Some(pending) = timeline.records.iter().find(|r| {
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

// --- Cursor positioning ---
impl LogViewer {
    /// Expands parent nodes of the followed task and positions the cursor
    /// on its row in the Pipeline Stages tree.
    pub fn jump_to_followed_task(&mut self) {
        let Some(timeline) = &self.build_timeline else {
            return;
        };

        // Find the raw record matching the followed task.
        let followed_name = &self.followed_task_name;
        let record = self.followed_log_id.map_or_else(
            || {
                // Pending task: match by name + InProgress state.
                timeline.records.iter().find(|r| {
                    r.record_type == "Task"
                        && r.name == *followed_name
                        && r.state == Some(TaskState::InProgress)
                })
            },
            |log_id| {
                timeline
                    .records
                    .iter()
                    .find(|r| r.log.as_ref().is_some_and(|l| l.id == log_id))
            },
        );

        let Some(record) = record else { return };
        let mut current_id = record.parent_id.clone();

        // Walk up the ancestor chain to expand all parent nodes.
        while let Some(cid) = &current_id {
            if let Some(rec) = timeline.records.iter().find(|r| r.id == *cid) {
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

        self.rebuild_timeline_rows();

        // Find the row by log_id first, then by name + InProgress state.
        let row_idx = if let Some(log_id) = self.followed_log_id {
            self.timeline_rows.iter().position(
                |row| matches!(row, TimelineRow::Task { log_id: Some(id), .. } if *id == log_id),
            )
        } else {
            self.timeline_rows.iter().position(|row| {
                matches!(
                    row,
                    TimelineRow::Task { name, state: Some(TaskState::InProgress), .. }
                    if *name == self.followed_task_name
                )
            })
        };

        if let Some(idx) = row_idx {
            self.log_entries_nav.set_index(idx);
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
