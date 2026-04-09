mod follow;
mod state;
mod timeline;

pub use state::LogViewerState;
pub use timeline::TimelineRow;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::{
        Build, BuildDefinitionRef, BuildResult, BuildStatus, BuildTimeline, LogReference,
        TaskState, TimelineRecord,
    };
    use crate::app::View;

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
