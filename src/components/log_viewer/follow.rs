//! Follow and inspect mode logic for the log viewer.

use crate::client::models::{BuildResult, BuildStatus, TaskState};

use super::LogViewer;
use super::TimelineRow;

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
}

// --- Auto-select and active-task detection ---
impl LogViewer {
    /// Picks the most relevant timeline task to auto-show logs for.
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

    /// Finds the currently active task without moving cursor or changing state.
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
}

// --- Build status derived from timeline ---
impl LogViewer {
    /// Updates `selected_build` status/result from timeline records.
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
