//! Log viewer component for inspecting build stage, job, and task output.

mod follow;
mod state;
mod timeline;

pub use follow::ActiveTaskResult;
pub use state::LogViewer;
pub use timeline::TimelineRow;

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};

use super::Component;
use crate::client::models::{BuildResult, TaskState};
use crate::render::helpers::{
    checkpoint_status_icon, draw_view_frame, row_style, timeline_status_icon, view_block,
};
use crate::render::theme;
use crate::state::App;

/// Draws the log viewer. This is a free function rather than a method on `LogViewer`
/// because it needs `&mut App` (for `set_layout_areas` mouse hit-testing state)
/// while the component is itself a field of `App`.
pub fn draw_log_viewer(f: &mut Frame, app: &mut App, area: Rect) {
    let build_label = app.log_viewer.selected_build().map_or_else(
        || "Build".to_string(),
        |b| format!("{} #{}", b.definition.name, b.build_number),
    );
    let subtitle = Line::from(vec![
        Span::styled(format!(" {build_label}"), theme::SUBTLE),
        Span::styled(
            if app.log_viewer.is_following() {
                "  ·  Follow mode"
            } else {
                "  ·  Inspect mode"
            },
            if app.log_viewer.is_following() {
                theme::FOLLOW_TITLE
            } else {
                theme::MODE_ACTIVE
            },
        ),
    ]);
    let content_area = draw_view_frame(f, area, " Log Viewer ", Some(subtitle));

    let body = Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(content_area);

    // Store layout areas for mouse hit-testing.
    app.log_viewer.set_layout_areas(body[0], body[1]);

    draw_tree(f, app, body[0]);
    draw_log(f, app, body[1]);
}

impl Component for LogViewer {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn footer_hints(&self) -> &'static str {
        "↑↓ navigate  ←→ collapse/expand  Enter inspect  f follow  R retry  A approve  D reject  c cancel  o open  1–4 areas  q/Esc back"
    }
}

fn draw_tree(f: &mut Frame, app: &App, area: Rect) {
    if app.log_viewer.timeline_rows().is_empty() {
        let loading = Paragraph::new(" Loading timeline...")
            .style(theme::MUTED)
            .block(view_block(" Pipeline Stages ").title_style(theme::SECTION_HEADER));
        f.render_widget(loading, area);
        return;
    }

    let items: Vec<ListItem> = app
        .log_viewer
        .timeline_rows()
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let selected = i == app.log_viewer.nav().index();
            match row {
                TimelineRow::Stage {
                    name,
                    state,
                    result,
                    collapsed,
                    ..
                } => {
                    let arrow = if *collapsed { "▸" } else { "▾" };
                    let (icon, icon_color) = timeline_status_icon(*state, *result);
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{arrow} "), theme::ARROW),
                        Span::styled(format!("{icon} "), Style::new().fg(icon_color)),
                        Span::styled(
                            name.as_str(),
                            timeline_name_style(theme::STAGE, *state, *result),
                        ),
                    ]))
                    .style(row_style(selected))
                }
                TimelineRow::Job {
                    name,
                    state,
                    result,
                    collapsed,
                    ..
                } => {
                    let arrow = if *collapsed { "▸" } else { "▾" };
                    let (icon, icon_color) = timeline_status_icon(*state, *result);
                    ListItem::new(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(format!("{arrow} "), theme::JOB_ARROW),
                        Span::styled(format!("{icon} "), Style::new().fg(icon_color)),
                        Span::styled(
                            name.as_str(),
                            timeline_name_style(theme::JOB, *state, *result),
                        ),
                    ]))
                    .style(row_style(selected))
                }
                TimelineRow::Task {
                    name,
                    state,
                    result,
                    log_id,
                    ..
                } => {
                    let (icon, icon_color) = timeline_status_icon(*state, *result);
                    let log_indicator = if log_id.is_some() { "" } else { " ·" };
                    ListItem::new(Line::from(vec![
                        Span::raw("      "),
                        Span::styled(format!("{icon} "), Style::new().fg(icon_color)),
                        Span::styled(
                            name.as_str(),
                            timeline_name_style(theme::SUBTLE, *state, *result),
                        ),
                        Span::styled(log_indicator, theme::MUTED),
                    ]))
                    .style(row_style(selected))
                }
                TimelineRow::Checkpoint {
                    name,
                    state,
                    result,
                    ..
                } => {
                    let (icon, icon_color) = checkpoint_status_icon(*state, *result);
                    ListItem::new(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(format!("{icon} "), Style::new().fg(icon_color)),
                        Span::styled(name.as_str(), checkpoint_name_style(*state, *result)),
                    ]))
                    .style(row_style(selected))
                }
            }
        })
        .collect();

    let list =
        List::new(items).block(view_block(" Pipeline Stages ").title_style(theme::SECTION_HEADER));

    let mut state = ListState::default();
    state.select(Some(app.log_viewer.nav().index()));
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_log(f: &mut Frame, app: &App, area: Rect) {
    let title = if app.log_viewer.is_following() && !app.log_viewer.followed_task_name().is_empty()
    {
        format!(
            " Log Output — FOLLOW: {} ",
            app.log_viewer.followed_task_name()
        )
    } else if !app.log_viewer.is_following() {
        if let Some(TimelineRow::Task { name, .. }) = app
            .log_viewer
            .timeline_rows()
            .get(app.log_viewer.nav().index())
        {
            format!(" Log Output — {name} ")
        } else {
            " Log Output ".to_string()
        }
    } else {
        " Log Output ".to_string()
    };

    if app.log_viewer.log_content().is_empty() {
        let hint = Paragraph::new(" Select a task and press Enter to view its log")
            .style(theme::MUTED)
            .block(log_block(title, app.log_viewer.is_following()));
        f.render_widget(hint, area);
    } else {
        let dropped = app.log_viewer.log_content().dropped();
        let mut lines: Vec<Line> =
            Vec::with_capacity(app.log_viewer.log_content().len() + usize::from(dropped > 0));
        if dropped > 0 {
            lines.push(Line::styled(
                format!("… {dropped} earlier line(s) dropped (buffer limit)"),
                theme::MUTED,
            ));
        }
        lines.extend(
            app.log_viewer
                .log_content()
                .iter()
                .map(|l| Line::styled(l.as_str(), theme::TEXT)),
        );

        let total_lines = lines.len() as u32;
        let visible_height = u32::from(area.height.saturating_sub(2));
        let max_scroll = total_lines.saturating_sub(visible_height);

        let scroll_offset_u32 = if app.log_viewer.log_auto_scroll() {
            max_scroll
        } else {
            app.log_viewer.log_scroll_offset().min(max_scroll)
        };
        let scroll_offset = scroll_offset_u32.min(u32::from(u16::MAX)) as u16;

        let log = Paragraph::new(Text::from(lines))
            .style(theme::TEXT)
            .block(log_block(title, app.log_viewer.is_following()))
            .wrap(Wrap { trim: false })
            .scroll((scroll_offset, 0));
        f.render_widget(log, area);
    }
}

fn log_block<'a>(title: String, is_following: bool) -> ratatui::widgets::Block<'a> {
    let title_style = if is_following {
        theme::FOLLOW_TITLE
    } else {
        theme::TITLE
    };

    view_block(title).title_style(title_style)
}

fn timeline_name_style(
    base: Style,
    state: Option<TaskState>,
    result: Option<BuildResult>,
) -> Style {
    match result {
        Some(BuildResult::Succeeded) => base.fg(theme::SUCCESS_FG),
        Some(BuildResult::Failed) => base.fg(theme::ERROR_FG),
        Some(BuildResult::PartiallySucceeded) => base.fg(theme::WARNING_FG),
        Some(BuildResult::Canceled | BuildResult::Skipped) => base.fg(theme::PENDING_FG),
        _ if matches!(state, Some(TaskState::InProgress)) => base.fg(theme::WARNING_FG),
        _ => base,
    }
}

fn checkpoint_name_style(state: Option<TaskState>, result: Option<BuildResult>) -> Style {
    match result {
        Some(BuildResult::Succeeded) => theme::SUCCESS,
        Some(BuildResult::Failed | BuildResult::Canceled) => theme::ERROR,
        _ if matches!(state, Some(TaskState::Completed)) => theme::SUCCESS,
        _ => theme::APPROVAL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::models::{
        Build, BuildResult, BuildStatus, BuildTimeline, LogReference, TaskState, TimelineRecord,
    };
    use crate::state::View;
    use crate::test_helpers::{make_build, make_timeline_record};

    // --- Helpers ---

    /// Creates a timeline record with an optional log ID.
    /// Wraps the shared make_timeline_record and adds the log field.
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
        let mut rec = make_timeline_record(id, record_type, parent_id, name, order, state, result);
        rec.log = log_id.map(|id| LogReference { id });
        rec
    }

    fn make_test_build(status: BuildStatus, result: Option<BuildResult>) -> Build {
        make_build(1, status, result)
    }

    /// Builds a simple timeline: 1 stage -> 1 phase -> N tasks.
    fn simple_timeline(
        tasks: &[(&str, Option<TaskState>, Option<BuildResult>, u32)],
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

    /// Creates a LogViewer with a build, sets timeline, and expands all nodes.
    fn state_with_expanded_timeline(
        build_status: BuildStatus,
        build_result: Option<BuildResult>,
        timeline: BuildTimeline,
    ) -> LogViewer {
        let build = make_test_build(build_status, build_result);
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
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

    // --- Group 1: State API tests ---

    #[test]
    fn default_state_is_empty() {
        let state = LogViewer::default();
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
        let state = LogViewer::new_for_build(build, View::BuildHistory, 42);
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
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
        assert!(state.is_following());
        state.enter_inspect_mode();
        assert!(!state.is_following());
        state.enter_follow_mode();
        assert!(state.is_following());
    }

    #[test]
    fn set_followed_updates_both() {
        let mut state = LogViewer::default();
        state.set_followed("Initialize".to_string(), 42);
        assert_eq!(state.followed_task_name(), "Initialize");
        assert_eq!(state.followed_log_id(), Some(42));
    }

    #[test]
    fn scroll_up_and_down() {
        let mut state = LogViewer::default();
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
        let mut state = LogViewer::default();
        state.scroll_down(10);
        state.set_log_content("line1\nline2\nline3");
        let lines: Vec<&str> = state.log_content().iter().map(String::as_str).collect();
        assert_eq!(lines, vec!["line1", "line2", "line3"]);
        assert!(state.log_auto_scroll());
        assert_eq!(state.log_scroll_offset(), 0);
    }

    #[test]
    fn clear_log_empties_content() {
        let mut state = LogViewer::default();
        state.set_log_content("some log\ndata");
        assert!(!state.log_content().is_empty());
        state.clear_log();
        assert!(state.log_content().is_empty());
    }

    #[test]
    fn set_generation_updates() {
        let mut state = LogViewer::default();
        assert_eq!(state.generation(), 0);
        state.set_generation(99);
        assert_eq!(state.generation(), 99);
    }

    #[test]
    fn log_buffer_drops_oldest_when_cap_exceeded() {
        use crate::client::models::BuildStatus;
        use crate::shared::log_buffer::{LogBuffer, MIN_CAPACITY};

        // Use the smallest allowed cap so the test stays cheap.
        let build = make_build(1, BuildStatus::Completed, None);
        let mut state =
            LogViewer::new_for_build_with_cap(build, View::BuildHistory, 1, MIN_CAPACITY);
        // Replace the buffer with one sized to the test's cap — the
        // `new_for_build_with_cap` wiring must carry the cap through.
        assert_eq!(state.log_content.cap(), MIN_CAPACITY);

        let _ = LogBuffer::new(MIN_CAPACITY); // sanity check the symbol is reachable.

        let over = MIN_CAPACITY + 37;
        let input = (0..over)
            .map(|i| format!("l{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        state.set_log_content(&input);

        assert_eq!(state.log_content().len(), MIN_CAPACITY);
        assert_eq!(state.log_content().dropped(), 37);
        // Tail is preserved.
        assert_eq!(
            state.log_content().iter().last().unwrap(),
            &format!("l{}", over - 1)
        );
        // Scroll offset is reset on replace regardless of truncation.
        assert_eq!(state.log_scroll_offset(), 0);
        assert!(state.log_auto_scroll());
    }

    // --- Group 2: Timeline tree building tests ---

    #[test]
    fn rebuild_timeline_basic_structure() {
        let timeline = simple_timeline(&[
            (
                "Task A",
                Some(TaskState::Completed),
                Some(BuildResult::Succeeded),
                10,
            ),
            ("Task B", Some(TaskState::InProgress), None, 11),
        ]);
        let build = make_test_build(BuildStatus::InProgress, None);
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);

        // First rebuild pre-collapses all stages.
        state.rebuild_timeline_rows();
        assert_eq!(state.timeline_rows().len(), 1);
        assert!(matches!(
            state.timeline_rows()[0],
            TimelineRow::Stage {
                collapsed: true,
                ..
            }
        ));

        // Expand stage and job.
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
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
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
        let timeline = simple_timeline(&[(
            "Task A",
            Some(TaskState::Completed),
            Some(BuildResult::Succeeded),
            10,
        )]);
        let build = make_test_build(BuildStatus::InProgress, None);
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
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
        let timeline = simple_timeline(&[(
            "Task A",
            Some(TaskState::Completed),
            Some(BuildResult::Succeeded),
            10,
        )]);
        let build = make_test_build(BuildStatus::InProgress, None);
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
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
        let timeline = simple_timeline(&[(
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
        // Rows: [Stage(0), Job(1), TaskA(2), TaskB(3), Stage(4), Job(5), TaskC(6), TaskD(7)].
        assert_eq!(state.find_timeline_parent_index(5), Some(4));
    }

    #[test]
    fn timeline_row_kind_returns_correct_type() {
        let timeline = simple_timeline(&[(
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
        let timeline = simple_timeline(&[(
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
        let timeline = simple_timeline(&[
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

    // --- Group 3: Build status from timeline ---

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
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
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
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
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
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
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
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        state.refresh_build_status_from_timeline();
        let b = state.selected_build().unwrap();
        assert_eq!(b.status, BuildStatus::Completed);
        assert_eq!(b.result, Some(BuildResult::Succeeded));
    }

    #[test]
    fn refresh_status_noop_when_no_timeline() {
        let build = make_test_build(BuildStatus::InProgress, None);
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
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
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        state.refresh_build_status_from_timeline();
        let b = state.selected_build().unwrap();
        assert_eq!(b.status, BuildStatus::InProgress);
        assert!(b.result.is_none());
    }

    // --- Group 4: Auto-select and find_active_task ---

    #[test]
    fn auto_select_picks_in_progress_task_for_running_build() {
        let timeline = simple_timeline(&[
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
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        state.rebuild_timeline_rows();

        let result = state.auto_select_log_entry();
        assert!(result.is_some());
        let (_, log_id) = result.unwrap();
        assert_eq!(log_id, Some(11));
    }

    #[test]
    fn auto_select_picks_failed_task_for_completed_build() {
        let timeline = simple_timeline(&[
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
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        state.rebuild_timeline_rows();

        let result = state.auto_select_log_entry();
        assert!(result.is_some());
        let (_, log_id) = result.unwrap();
        assert_eq!(log_id, Some(11));
    }

    #[test]
    fn find_active_task_returns_in_progress() {
        let timeline = simple_timeline(&[
            (
                "Init",
                Some(TaskState::Completed),
                Some(BuildResult::Succeeded),
                10,
            ),
            ("Build", Some(TaskState::InProgress), None, 11),
        ]);
        let build = make_test_build(BuildStatus::InProgress, None);
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        let result = state.find_active_task();
        assert_eq!(
            result,
            ActiveTaskResult::Found {
                name: "Build".to_string(),
                log_id: 11
            }
        );
    }

    #[test]
    fn find_active_task_returns_none_when_no_timeline() {
        let build = make_test_build(BuildStatus::InProgress, None);
        let state = LogViewer::new_for_build(build, View::BuildHistory, 1);
        assert_eq!(state.find_active_task(), ActiveTaskResult::None);
    }

    #[test]
    fn find_active_task_returns_pending_when_in_progress_task_has_no_log() {
        let timeline = BuildTimeline {
            records: vec![
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
                // InProgress task with NO log — just started.
                make_record(
                    "t2",
                    Some("p1"),
                    "Build",
                    "Task",
                    2,
                    Some(TaskState::InProgress),
                    None,
                    None,
                ),
            ],
        };
        let build = make_test_build(BuildStatus::InProgress, None);
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        assert_eq!(
            state.find_active_task(),
            ActiveTaskResult::Pending {
                name: "Build".to_string()
            }
        );
    }

    #[test]
    fn find_active_task_returns_found_for_completed_build_with_failed_task() {
        let timeline = simple_timeline(&[
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
        ]);
        let build = make_test_build(BuildStatus::Completed, Some(BuildResult::Failed));
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        assert_eq!(
            state.find_active_task(),
            ActiveTaskResult::Found {
                name: "Build".to_string(),
                log_id: 11
            }
        );
    }

    #[test]
    fn auto_select_picks_in_progress_task_without_log() {
        let timeline = BuildTimeline {
            records: vec![
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
                // InProgress task with NO log.
                make_record(
                    "t2",
                    Some("p1"),
                    "Build",
                    "Task",
                    2,
                    Some(TaskState::InProgress),
                    None,
                    None,
                ),
            ],
        };
        let build = make_test_build(BuildStatus::InProgress, None);
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        state.rebuild_timeline_rows();

        let result = state.auto_select_log_entry();
        assert!(result.is_some());
        let (_, log_id) = result.unwrap();
        // No log available yet for the InProgress task.
        assert_eq!(log_id, None);
    }

    #[test]
    fn set_followed_pending_clears_log_id() {
        let mut state = LogViewer::default();
        state.set_followed("Init".to_string(), 42);
        assert_eq!(state.followed_log_id(), Some(42));

        state.set_followed_pending("Build".to_string());
        assert_eq!(state.followed_task_name(), "Build");
        assert_eq!(state.followed_log_id(), None);
    }

    #[test]
    fn find_active_task_picks_first_stage_in_parallel_build() {
        // Two parallel stages, each with an InProgress task.
        let timeline = BuildTimeline {
            records: vec![
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
                    "Build Job",
                    "Phase",
                    1,
                    Some(TaskState::InProgress),
                    None,
                    None,
                ),
                make_record(
                    "t1",
                    Some("p1"),
                    "Compile",
                    "Task",
                    1,
                    Some(TaskState::InProgress),
                    None,
                    Some(10),
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
                make_record(
                    "p2",
                    Some("s2"),
                    "Deploy Job",
                    "Phase",
                    1,
                    Some(TaskState::InProgress),
                    None,
                    None,
                ),
                make_record(
                    "t2",
                    Some("p2"),
                    "Deploy Step",
                    "Task",
                    1,
                    Some(TaskState::InProgress),
                    None,
                    Some(20),
                ),
            ],
        };
        let build = make_test_build(BuildStatus::InProgress, None);
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);

        // Should pick the first stage's task (Compile, log_id=10), not the second.
        assert_eq!(
            state.find_active_task(),
            ActiveTaskResult::Found {
                name: "Compile".to_string(),
                log_id: 10
            }
        );
    }

    #[test]
    fn auto_select_picks_first_stage_in_parallel_build() {
        let timeline = BuildTimeline {
            records: vec![
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
                    "Build Job",
                    "Phase",
                    1,
                    Some(TaskState::InProgress),
                    None,
                    None,
                ),
                make_record(
                    "t1",
                    Some("p1"),
                    "Compile",
                    "Task",
                    1,
                    Some(TaskState::InProgress),
                    None,
                    Some(10),
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
                make_record(
                    "p2",
                    Some("s2"),
                    "Deploy Job",
                    "Phase",
                    1,
                    Some(TaskState::InProgress),
                    None,
                    None,
                ),
                make_record(
                    "t2",
                    Some("p2"),
                    "Deploy Step",
                    "Task",
                    1,
                    Some(TaskState::InProgress),
                    None,
                    Some(20),
                ),
            ],
        };
        let build = make_test_build(BuildStatus::InProgress, None);
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        state.rebuild_timeline_rows();

        let result = state.auto_select_log_entry();
        assert!(result.is_some());
        let (_, log_id) = result.unwrap();
        // Should pick the first stage's task (log_id=10).
        assert_eq!(log_id, Some(10));
    }

    #[test]
    fn jump_to_followed_task_expands_parents_and_positions_cursor() {
        let timeline = simple_timeline(&[
            (
                "Init",
                Some(TaskState::Completed),
                Some(BuildResult::Succeeded),
                10,
            ),
            ("Build", Some(TaskState::InProgress), None, 11),
        ]);
        let build = make_test_build(BuildStatus::InProgress, None);
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
        state.set_build_timeline(timeline);
        state.rebuild_timeline_rows();

        // Everything is collapsed after first rebuild.
        assert!(state.is_stage_collapsed("s1"));

        // Set followed and jump.
        state.set_followed("Build".to_string(), 11);
        state.jump_to_followed_task();

        // Stage should be expanded and cursor should be on the "Build" task.
        assert!(!state.is_stage_collapsed("s1"));
        let idx = state.nav().index();
        assert!(matches!(
            state.timeline_rows().get(idx),
            Some(TimelineRow::Task { name, .. }) if name == "Build"
        ));
    }

    // --- Group 5: Checkpoint tests ---

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
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
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
        let mut state = LogViewer::new_for_build(build, View::BuildHistory, 1);
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

    // --- Regression: find_timeline_parent_index with large timelines ---

    #[test]
    fn find_timeline_parent_index_large_timeline() {
        // 2 stages × 2 jobs × 3 tasks = 14 rows when fully expanded.
        let timeline = BuildTimeline {
            records: vec![
                // Stage 1.
                make_record(
                    "s1",
                    None,
                    "Stage 1",
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
                    "t3",
                    Some("p1"),
                    "Task C",
                    "Task",
                    3,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    Some(12),
                ),
                make_record(
                    "p2",
                    Some("s1"),
                    "Job 2",
                    "Phase",
                    2,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    None,
                ),
                make_record(
                    "t4",
                    Some("p2"),
                    "Task D",
                    "Task",
                    1,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    Some(13),
                ),
                make_record(
                    "t5",
                    Some("p2"),
                    "Task E",
                    "Task",
                    2,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    Some(14),
                ),
                make_record(
                    "t6",
                    Some("p2"),
                    "Task F",
                    "Task",
                    3,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    Some(15),
                ),
                // Stage 2.
                make_record(
                    "s2",
                    None,
                    "Stage 2",
                    "Stage",
                    2,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    None,
                ),
                make_record(
                    "p3",
                    Some("s2"),
                    "Job 3",
                    "Phase",
                    1,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    None,
                ),
                make_record(
                    "t7",
                    Some("p3"),
                    "Task G",
                    "Task",
                    1,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    Some(16),
                ),
                make_record(
                    "t8",
                    Some("p3"),
                    "Task H",
                    "Task",
                    2,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    Some(17),
                ),
                make_record(
                    "t9",
                    Some("p3"),
                    "Task I",
                    "Task",
                    3,
                    Some(TaskState::Completed),
                    Some(BuildResult::Succeeded),
                    Some(18),
                ),
            ],
        };

        let state = state_with_expanded_timeline(
            BuildStatus::Completed,
            Some(BuildResult::Succeeded),
            timeline,
        );

        // Expected expanded layout.
        //  0: Stage "s1"
        //  1:   Job "p1"
        //  2:     Task "t1"  (parent_job_id = "p1")
        //  3:     Task "t2"
        //  4:     Task "t3"
        //  5:   Job "p2"
        //  6:     Task "t4"
        //  ...
        // 9:  Stage "s2"
        // 10:   Job "p3"
        // 11:     Task "t7"
        // ...

        let rows = state.timeline_rows();
        assert!(rows.len() >= 14, "expected ≥14 rows, got {}", rows.len());

        // Task at index 2 → parent should be Job at index 1.
        assert_eq!(
            state.find_timeline_parent_index(2),
            Some(1),
            "Task at idx 2 should find parent Job at idx 1"
        );

        // Job at index 1 → parent should be Stage at index 0.
        assert_eq!(
            state.find_timeline_parent_index(1),
            Some(0),
            "Job at idx 1 should find parent Stage at idx 0"
        );

        // Stage at index 0 → no parent.
        assert_eq!(
            state.find_timeline_parent_index(0),
            None,
            "Stage at idx 0 should have no parent"
        );

        // Task in stage 2 should also find its parent job correctly.
        let s2_task_idx = rows
            .iter()
            .position(|r| matches!(r, TimelineRow::Task { name, .. } if name == "Task G"))
            .expect("Task G should be in rows");
        let s2_job_idx = rows
            .iter()
            .position(|r| matches!(r, TimelineRow::Job { id, .. } if id == "p3"))
            .expect("Job p3 should be in rows");
        assert_eq!(
            state.find_timeline_parent_index(s2_task_idx),
            Some(s2_job_idx),
            "Task G should find parent Job p3"
        );
    }
}
