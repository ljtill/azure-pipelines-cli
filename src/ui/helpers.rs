use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use super::theme;
use crate::api::models::{Build, BuildResult, BuildStatus, TaskState};
use crate::app::InputMode;

/// Short human-readable label for a build's combined status and result.
pub fn status_label(status: BuildStatus, result: Option<BuildResult>) -> &'static str {
    if status.is_in_progress() {
        return "Running";
    }
    if status == BuildStatus::Cancelling {
        return "Cancelling";
    }
    if status == BuildStatus::NotStarted {
        return "Queued";
    }
    match result {
        Some(BuildResult::Succeeded) => "Succeeded",
        Some(BuildResult::Failed) => "Failed",
        Some(BuildResult::PartiallySucceeded) => "Partial",
        Some(BuildResult::Canceled) => "Canceled",
        Some(BuildResult::Skipped) => "Skipped",
        _ => "",
    }
}

/// Shared status → (icon, color) mapping for build status and result.
pub fn status_icon(status: BuildStatus, result: Option<BuildResult>) -> (&'static str, Color) {
    if status.is_in_progress() {
        return ("●", Color::Yellow);
    }
    match result {
        Some(BuildResult::Succeeded) => ("✓", Color::Green),
        Some(BuildResult::Failed) => ("✗", Color::Red),
        Some(BuildResult::PartiallySucceeded) => ("◐", Color::Yellow),
        Some(BuildResult::Canceled) => ("⊘", Color::DarkGray),
        Some(BuildResult::Skipped) => ("⊘", Color::DarkGray),
        _ => ("○", Color::DarkGray),
    }
}

/// Like `status_icon`, but shows "awaiting approval" for in-progress builds
/// that have a pending approval gate.
pub fn effective_status_icon(
    status: BuildStatus,
    result: Option<BuildResult>,
    has_pending_approval: bool,
) -> (&'static str, Color) {
    if has_pending_approval && status.is_in_progress() {
        return ("◆", Color::Magenta);
    }
    status_icon(status, result)
}

/// Like `status_label`, but returns "Awaiting" for in-progress builds
/// that have a pending approval gate.
pub fn effective_status_label(
    status: BuildStatus,
    result: Option<BuildResult>,
    has_pending_approval: bool,
) -> &'static str {
    if has_pending_approval && status.is_in_progress() {
        return "Awaiting";
    }
    status_label(status, result)
}

/// Status icon for timeline records (stage/job/task) where state and result
/// are separate optional fields.
pub fn timeline_status_icon(
    state: Option<TaskState>,
    result: Option<BuildResult>,
) -> (&'static str, Color) {
    match result {
        Some(BuildResult::Succeeded) => ("✓", Color::Green),
        Some(BuildResult::Failed) => ("✗", Color::Red),
        Some(BuildResult::PartiallySucceeded) => ("◐", Color::Yellow),
        Some(BuildResult::Canceled) | Some(BuildResult::Skipped) => ("⊘", Color::DarkGray),
        _ => match state {
            Some(TaskState::InProgress) => ("●", Color::Yellow),
            Some(TaskState::Completed) => ("✓", Color::Green),
            Some(TaskState::Pending) => ("○", Color::DarkGray),
            _ => ("○", Color::DarkGray),
        },
    }
}

/// Status icon for checkpoint (approval) records.
pub fn checkpoint_status_icon(
    state: Option<TaskState>,
    result: Option<BuildResult>,
) -> (&'static str, Color) {
    match result {
        Some(BuildResult::Succeeded) => ("✓", Color::Green),
        Some(BuildResult::Failed) | Some(BuildResult::Canceled) => ("✗", Color::Red),
        _ => match state {
            Some(TaskState::InProgress) | Some(TaskState::Pending) => ("◆", Color::Magenta),
            Some(TaskState::Completed) => ("✓", Color::Green),
            _ => ("◆", Color::Magenta),
        },
    }
}

/// Format a build's elapsed time or "ago" string.
pub fn build_elapsed(build: &Build) -> String {
    use chrono::Utc;

    if build.status.is_in_progress() {
        if let Some(start) = build.start_time {
            let elapsed = Utc::now().signed_duration_since(start);
            let mins = elapsed.num_minutes();
            if mins < 60 {
                return format!("running {}m", mins);
            }
            let hours = elapsed.num_hours();
            if hours < 24 {
                return format!("running {}h{}m", hours, mins % 60);
            }
            let days = elapsed.num_days();
            return format!("running {}d{}h", days, hours % 24);
        }
        return "queued".to_string();
    }

    if let Some(finish) = build.finish_time {
        let ago = Utc::now().signed_duration_since(finish);
        if ago.num_hours() < 1 {
            return format!("{}m ago", ago.num_minutes());
        } else if ago.num_hours() < 24 {
            return format!("{}h ago", ago.num_hours());
        } else {
            return format!("{}d ago", ago.num_days());
        }
    }

    String::new()
}

/// Render a search/filter bar. Call only when the search bar should be visible.
pub fn draw_search_bar(f: &mut Frame, area: Rect, query: &str, input_mode: InputMode) {
    let search = Paragraph::new(Line::from(vec![
        Span::styled(" / ", theme::SEARCH_PROMPT),
        Span::raw(query),
        if input_mode == InputMode::Search {
            Span::styled("▌", theme::CURSOR)
        } else {
            Span::raw("")
        },
    ]))
    .block(
        Block::bordered()
            .title(" Filter ")
            .title_style(theme::SEARCH_PROMPT),
    );
    f.render_widget(search, area);
}

/// Truncate a string to at most `max_len` characters, safe for multi-byte UTF-8.
/// Appends `…` when the text is clipped so the user knows content was cut.
pub fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }
    if max_len <= 1 {
        return "…".to_string();
    }
    // Reserve one char for the ellipsis.
    let mut end = max_len - 1;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut result = s[..end].to_string();
    result.push('…');
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- status_icon tests ---

    #[test]
    fn status_icon_in_progress() {
        let (icon, color) = status_icon(BuildStatus::InProgress, None);
        assert_eq!(icon, "●");
        assert_eq!(color, Color::Yellow);
    }

    #[test]
    fn status_icon_in_progress_overrides_result() {
        let (icon, _) = status_icon(BuildStatus::InProgress, Some(BuildResult::Failed));
        assert_eq!(icon, "●");
    }

    #[test]
    fn status_icon_succeeded() {
        let (icon, color) = status_icon(BuildStatus::Completed, Some(BuildResult::Succeeded));
        assert_eq!(icon, "✓");
        assert_eq!(color, Color::Green);
    }

    #[test]
    fn status_icon_failed() {
        let (icon, color) = status_icon(BuildStatus::Completed, Some(BuildResult::Failed));
        assert_eq!(icon, "✗");
        assert_eq!(color, Color::Red);
    }

    #[test]
    fn status_icon_no_result() {
        let (icon, color) = status_icon(BuildStatus::Completed, None);
        assert_eq!(icon, "○");
        assert_eq!(color, Color::DarkGray);
    }

    // --- timeline_status_icon tests ---

    #[test]
    fn timeline_result_takes_priority() {
        let (icon, _) =
            timeline_status_icon(Some(TaskState::InProgress), Some(BuildResult::Succeeded));
        assert_eq!(icon, "✓");
    }

    #[test]
    fn timeline_in_progress_state() {
        let (icon, color) = timeline_status_icon(Some(TaskState::InProgress), None);
        assert_eq!(icon, "●");
        assert_eq!(color, Color::Yellow);
    }

    #[test]
    fn timeline_pending_state() {
        let (icon, color) = timeline_status_icon(Some(TaskState::Pending), None);
        assert_eq!(icon, "○");
        assert_eq!(color, Color::DarkGray);
    }

    // --- truncate tests ---

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_exact_length() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_long_string() {
        assert_eq!(truncate("hello world", 5), "hell…");
    }

    #[test]
    fn truncate_multibyte_safe() {
        // "café" = 5 bytes (é = 2 bytes), truncate at 4 should not split é
        let result = truncate("café", 4);
        assert_eq!(result, "caf…");
    }

    #[test]
    fn truncate_empty() {
        assert_eq!(truncate("", 5), "");
    }

    #[test]
    fn truncate_zero_len() {
        assert_eq!(truncate("hello", 0), "…");
    }

    // --- build_elapsed tests ---

    use crate::api::models::BuildDefinitionRef;

    fn make_build(
        status: BuildStatus,
        result: Option<BuildResult>,
        start_time: Option<chrono::DateTime<chrono::Utc>>,
        finish_time: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Build {
        Build {
            id: 1,
            build_number: "1".to_string(),
            status,
            result,
            queue_time: None,
            start_time,
            finish_time,
            definition: BuildDefinitionRef {
                id: 1,
                name: "test".to_string(),
            },
            source_branch: None,
            requested_for: None,
            reason: None,
            trigger_info: None,
        }
    }

    #[test]
    fn build_elapsed_running() {
        use chrono::{TimeDelta, Utc};
        let build = make_build(
            BuildStatus::InProgress,
            None,
            Some(Utc::now() - TimeDelta::minutes(5)),
            None,
        );
        let result = build_elapsed(&build);
        assert!(
            result.contains("running"),
            "expected 'running' in: {result}"
        );
        assert!(result.contains("5m"), "expected '5m' in: {result}");
    }

    #[test]
    fn build_elapsed_running_hours() {
        use chrono::{TimeDelta, Utc};
        let build = make_build(
            BuildStatus::InProgress,
            None,
            Some(Utc::now() - TimeDelta::minutes(185)),
            None,
        );
        let result = build_elapsed(&build);
        assert_eq!(result, "running 3h5m");
    }

    #[test]
    fn build_elapsed_running_days() {
        use chrono::{TimeDelta, Utc};
        let build = make_build(
            BuildStatus::InProgress,
            None,
            Some(Utc::now() - TimeDelta::hours(50)),
            None,
        );
        let result = build_elapsed(&build);
        assert_eq!(result, "running 2d2h");
    }

    #[test]
    fn build_elapsed_queued() {
        let build = make_build(BuildStatus::InProgress, None, None, None);
        assert_eq!(build_elapsed(&build), "queued");
    }

    #[test]
    fn build_elapsed_recent() {
        use chrono::{TimeDelta, Utc};
        let build = make_build(
            BuildStatus::Completed,
            Some(BuildResult::Succeeded),
            None,
            Some(Utc::now() - TimeDelta::minutes(30)),
        );
        assert_eq!(build_elapsed(&build), "30m ago");
    }

    #[test]
    fn build_elapsed_hours_ago() {
        use chrono::{TimeDelta, Utc};
        let build = make_build(
            BuildStatus::Completed,
            Some(BuildResult::Succeeded),
            None,
            Some(Utc::now() - TimeDelta::hours(3)),
        );
        assert_eq!(build_elapsed(&build), "3h ago");
    }

    #[test]
    fn build_elapsed_days_ago() {
        use chrono::{TimeDelta, Utc};
        let build = make_build(
            BuildStatus::Completed,
            Some(BuildResult::Succeeded),
            None,
            Some(Utc::now() - TimeDelta::days(2)),
        );
        assert_eq!(build_elapsed(&build), "2d ago");
    }

    // --- checkpoint_status_icon tests ---

    #[test]
    fn checkpoint_approved() {
        let (icon, color) = checkpoint_status_icon(None, Some(BuildResult::Succeeded));
        assert_eq!(icon, "✓");
        assert_eq!(color, Color::Green);
    }

    #[test]
    fn checkpoint_rejected() {
        let (icon, color) = checkpoint_status_icon(None, Some(BuildResult::Failed));
        assert_eq!(icon, "✗");
        assert_eq!(color, Color::Red);
    }

    #[test]
    fn checkpoint_pending_in_progress() {
        let (icon, color) = checkpoint_status_icon(Some(TaskState::InProgress), None);
        assert_eq!(icon, "◆");
        assert_eq!(color, Color::Magenta);
    }

    #[test]
    fn checkpoint_pending_none() {
        let (icon, color) = checkpoint_status_icon(None, None);
        assert_eq!(icon, "◆");
        assert_eq!(color, Color::Magenta);
    }

    // --- status_label tests ---

    #[test]
    fn status_label_running() {
        assert_eq!(status_label(BuildStatus::InProgress, None), "Running");
    }

    #[test]
    fn status_label_running_overrides_result() {
        assert_eq!(
            status_label(BuildStatus::InProgress, Some(BuildResult::Failed)),
            "Running"
        );
    }

    #[test]
    fn status_label_succeeded() {
        assert_eq!(
            status_label(BuildStatus::Completed, Some(BuildResult::Succeeded)),
            "Succeeded"
        );
    }

    #[test]
    fn status_label_failed() {
        assert_eq!(
            status_label(BuildStatus::Completed, Some(BuildResult::Failed)),
            "Failed"
        );
    }

    #[test]
    fn status_label_canceled() {
        assert_eq!(
            status_label(BuildStatus::Completed, Some(BuildResult::Canceled)),
            "Canceled"
        );
    }

    #[test]
    fn status_label_partial() {
        assert_eq!(
            status_label(
                BuildStatus::Completed,
                Some(BuildResult::PartiallySucceeded)
            ),
            "Partial"
        );
    }

    #[test]
    fn status_label_cancelling() {
        assert_eq!(status_label(BuildStatus::Cancelling, None), "Cancelling");
    }

    #[test]
    fn status_label_queued() {
        assert_eq!(status_label(BuildStatus::NotStarted, None), "Queued");
    }

    #[test]
    fn status_label_unknown() {
        assert_eq!(status_label(BuildStatus::Completed, None), "");
    }
}
