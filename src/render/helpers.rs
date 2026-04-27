//! Shared rendering utilities for status icons, elapsed time, and text truncation.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::theme;
use crate::client::models::{Build, BuildResult, BuildStatus, TaskState};
use crate::state::InputMode;

/// Returns a short human-readable label for a build's combined status and result.
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

/// Returns the status icon and color for a build's status and result.
pub fn status_icon(status: BuildStatus, result: Option<BuildResult>) -> (&'static str, Color) {
    if status.is_in_progress() {
        return ("●", theme::WARNING_FG);
    }
    match result {
        Some(BuildResult::Succeeded) => ("✓", theme::SUCCESS_FG),
        Some(BuildResult::Failed) => ("✗", theme::ERROR_FG),
        Some(BuildResult::PartiallySucceeded) => ("◐", theme::WARNING_FG),
        Some(BuildResult::Canceled | BuildResult::Skipped) => ("⊘", theme::PENDING_FG),
        _ => ("○", theme::PENDING_FG),
    }
}

/// Returns the effective status icon, showing "awaiting approval" for in-progress builds
/// that have a pending approval gate.
pub fn effective_status_icon(
    status: BuildStatus,
    result: Option<BuildResult>,
    has_pending_approval: bool,
) -> (&'static str, Color) {
    if has_pending_approval && status.is_in_progress() {
        return ("◆", theme::APPROVAL_FG);
    }
    status_icon(status, result)
}

/// Returns the effective status label, using "Awaiting" for in-progress builds
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

/// Returns the status icon for timeline records (stage/job/task) where state and result
/// are separate optional fields.
pub fn timeline_status_icon(
    state: Option<TaskState>,
    result: Option<BuildResult>,
) -> (&'static str, Color) {
    match result {
        Some(BuildResult::Succeeded) => ("✓", theme::SUCCESS_FG),
        Some(BuildResult::Failed) => ("✗", theme::ERROR_FG),
        Some(BuildResult::PartiallySucceeded) => ("◐", theme::WARNING_FG),
        Some(BuildResult::Canceled | BuildResult::Skipped) => ("⊘", theme::PENDING_FG),
        _ => match state {
            Some(TaskState::InProgress) => ("●", theme::WARNING_FG),
            Some(TaskState::Completed) => ("✓", theme::SUCCESS_FG),
            _ => ("○", theme::PENDING_FG),
        },
    }
}

/// Returns the status icon for checkpoint (approval) records.
pub fn checkpoint_status_icon(
    state: Option<TaskState>,
    result: Option<BuildResult>,
) -> (&'static str, Color) {
    match result {
        Some(BuildResult::Succeeded) => ("✓", theme::SUCCESS_FG),
        Some(BuildResult::Failed | BuildResult::Canceled) => ("✗", theme::ERROR_FG),
        _ => match state {
            Some(TaskState::Completed) => ("✓", theme::SUCCESS_FG),
            _ => ("◆", theme::APPROVAL_FG),
        },
    }
}

/// Formats a build's elapsed time or "ago" string.
pub fn build_elapsed(build: &Build) -> String {
    use chrono::Utc;

    if build.status.is_in_progress() {
        if let Some(start) = build.start_time {
            let elapsed = Utc::now().signed_duration_since(start);
            let mins = elapsed.num_minutes();
            if mins < 60 {
                return format!("running {mins}m");
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
        }
        return format!("{}d ago", ago.num_days());
    }

    String::new()
}

/// Renders a search/filter bar. Call only when the search bar should be visible.
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
    .block(view_block(" Filter ").title_style(theme::SEARCH_PROMPT));
    f.render_widget(search, area);
}

/// Returns the standard bordered block used for top-level view panels.
pub fn view_block<'a, T>(title: T) -> Block<'a>
where
    T: Into<Line<'a>>,
{
    Block::bordered().title(title).title_style(theme::TITLE)
}

/// Renders the standard outer frame for a view and returns the remaining body area.
pub fn draw_view_frame<'a, T>(
    f: &mut Frame,
    area: Rect,
    title: T,
    subtitle: Option<Line<'a>>,
) -> Rect
where
    T: Into<Line<'a>>,
{
    let block = view_block(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    subtitle.map_or(inner, |line| {
        let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(inner);
        f.render_widget(Paragraph::new(line), chunks[0]);
        chunks[1]
    })
}

/// Returns styled spans rendering a sub-view tab strip for services with more
/// than one root view. Produces an empty `Vec` when the service has a single
/// root view. The `current` view is highlighted with `theme::MODE_ACTIVE`.
pub fn sub_view_tab_spans<'a>(
    service: crate::state::Service,
    current: crate::state::View,
) -> Vec<Span<'a>> {
    let views = service.root_views();
    if views.len() <= 1 {
        return Vec::new();
    }
    let mut spans: Vec<Span<'a>> = Vec::with_capacity(views.len() * 2);
    for (i, v) in views.iter().enumerate() {
        let style = if *v == current {
            theme::MODE_ACTIVE
        } else {
            theme::MODE_INACTIVE
        };
        spans.push(Span::styled(format!(" {} ", v.root_label()), style));
        if i < views.len() - 1 {
            spans.push(Span::styled(" │ ", theme::MUTED));
        }
    }
    spans
}

/// Renders a concise placeholder message inside an already-framed view body.
pub fn draw_state_message<'a, T>(f: &mut Frame, area: Rect, message: T, style: Style)
where
    T: Into<Line<'a>>,
{
    f.render_widget(Paragraph::new(message.into()).style(style), area);
}

/// Returns the display width of a string in terminal cells.
pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Truncates a string to at most `max_width` terminal cells.
///
/// Appends `…` when the text is clipped so the user knows content was cut.
pub fn truncate(s: &str, max_width: usize) -> String {
    if display_width(s) <= max_width {
        return s.to_string();
    }
    if max_width == 0 {
        return String::new();
    }

    let ellipsis_width = UnicodeWidthChar::width('…').unwrap_or(1);
    if max_width <= ellipsis_width {
        return "…".to_string();
    }

    let content_width = max_width - ellipsis_width;
    let mut used = 0;
    let mut result = String::new();
    for ch in s.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width > content_width {
            break;
        }
        result.push(ch);
        used += ch_width;
    }
    result.push('…');
    result
}

/// Centers a popup overlay within the given area using percentage-based sizing.
pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}

/// Returns the SELECTED style if the row is at the nav cursor, otherwise default.
pub fn row_style(is_selected: bool) -> Style {
    if is_selected {
        theme::SELECTED
    } else {
        Style::new()
    }
}

// --- Pull Request helpers ---

/// Returns the status icon and color for a pull request status string.
pub fn pr_status_icon(status: &str, is_draft: bool) -> (&'static str, Color) {
    if is_draft {
        return ("◌", theme::PENDING_FG);
    }
    match status.to_ascii_lowercase().as_str() {
        "active" => ("●", theme::SUCCESS_FG),
        "completed" => ("✓", theme::ACCENT_FG),
        "abandoned" => ("⊘", theme::ERROR_FG),
        _ => ("○", theme::PENDING_FG),
    }
}

/// Returns the icon and color for a reviewer's vote value.
///
/// ADO vote values: 10 = approved, 5 = approved with suggestions,
/// 0 = no vote, -5 = waiting for author, -10 = rejected.
pub fn reviewer_vote_icon(vote: i32) -> (&'static str, Color) {
    match vote {
        10 | 5 => ("✓", theme::SUCCESS_FG),
        -10 => ("✗", theme::ERROR_FG),
        -5 => ("●", theme::WARNING_FG),
        _ => ("○", theme::PENDING_FG),
    }
}

/// Splits an area with an optional search bar at the top. Returns the list area.
/// Renders the search bar if visible.
pub fn split_with_search_bar(
    f: &mut Frame,
    area: Rect,
    query: &str,
    input_mode: InputMode,
    show_search: bool,
) -> Rect {
    if show_search {
        let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);
        draw_search_bar(f, chunks[0], query, input_mode);
        chunks[1]
    } else {
        Layout::vertical([Constraint::Min(0)]).split(area)[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::theme;

    // --- status_icon tests ---

    #[test]
    fn status_icon_in_progress() {
        let (icon, color) = status_icon(BuildStatus::InProgress, None);
        assert_eq!(icon, "●");
        assert_eq!(color, theme::WARNING_FG);
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
        assert_eq!(color, theme::SUCCESS_FG);
    }

    #[test]
    fn status_icon_failed() {
        let (icon, color) = status_icon(BuildStatus::Completed, Some(BuildResult::Failed));
        assert_eq!(icon, "✗");
        assert_eq!(color, theme::ERROR_FG);
    }

    #[test]
    fn status_icon_no_result() {
        let (icon, color) = status_icon(BuildStatus::Completed, None);
        assert_eq!(icon, "○");
        assert_eq!(color, theme::PENDING_FG);
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
        assert_eq!(color, theme::WARNING_FG);
    }

    #[test]
    fn timeline_pending_state() {
        let (icon, color) = timeline_status_icon(Some(TaskState::Pending), None);
        assert_eq!(icon, "○");
        assert_eq!(color, theme::PENDING_FG);
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
        assert_eq!(truncate("café au lait", 5), "café…");
    }

    #[test]
    fn truncate_keeps_text_that_fits_display_width() {
        assert_eq!(truncate("café", 4), "café");
    }

    #[test]
    fn truncate_wide_text_by_display_width() {
        let result = truncate("デプロイ", 5);
        assert_eq!(result, "デプ…");
        assert_eq!(display_width(&result), 5);
    }

    #[test]
    fn truncate_empty() {
        assert_eq!(truncate("", 5), "");
    }

    #[test]
    fn truncate_zero_len() {
        assert_eq!(truncate("hello", 0), "");
    }

    // --- build_elapsed tests ---

    use crate::client::models::BuildDefinitionRef;

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
        assert_eq!(color, theme::SUCCESS_FG);
    }

    #[test]
    fn checkpoint_rejected() {
        let (icon, color) = checkpoint_status_icon(None, Some(BuildResult::Failed));
        assert_eq!(icon, "✗");
        assert_eq!(color, theme::ERROR_FG);
    }

    #[test]
    fn checkpoint_pending_in_progress() {
        let (icon, color) = checkpoint_status_icon(Some(TaskState::InProgress), None);
        assert_eq!(icon, "◆");
        assert_eq!(color, theme::APPROVAL_FG);
    }

    #[test]
    fn checkpoint_pending_none() {
        let (icon, color) = checkpoint_status_icon(None, None);
        assert_eq!(icon, "◆");
        assert_eq!(color, theme::APPROVAL_FG);
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

    // --- pr_status_icon tests ---

    #[test]
    fn pr_status_icon_active() {
        let (icon, color) = pr_status_icon("active", false);
        assert_eq!(icon, "●");
        assert_eq!(color, theme::SUCCESS_FG);
    }

    #[test]
    fn pr_status_icon_draft() {
        let (icon, color) = pr_status_icon("active", true);
        assert_eq!(icon, "◌");
        assert_eq!(color, theme::PENDING_FG);
    }

    #[test]
    fn pr_status_icon_completed() {
        let (icon, color) = pr_status_icon("completed", false);
        assert_eq!(icon, "✓");
        assert_eq!(color, theme::ACCENT_FG);
    }

    #[test]
    fn pr_status_icon_abandoned() {
        let (icon, color) = pr_status_icon("abandoned", false);
        assert_eq!(icon, "⊘");
        assert_eq!(color, theme::ERROR_FG);
    }

    #[test]
    fn pr_status_icon_case_insensitive() {
        let (icon, _) = pr_status_icon("Active", false);
        assert_eq!(icon, "●");
    }

    // --- reviewer_vote_icon tests ---

    #[test]
    fn reviewer_vote_approved() {
        let (icon, color) = reviewer_vote_icon(10);
        assert_eq!(icon, "✓");
        assert_eq!(color, theme::SUCCESS_FG);
    }

    #[test]
    fn reviewer_vote_approved_with_suggestions() {
        let (icon, color) = reviewer_vote_icon(5);
        assert_eq!(icon, "✓");
        assert_eq!(color, theme::SUCCESS_FG);
    }

    #[test]
    fn reviewer_vote_rejected() {
        let (icon, color) = reviewer_vote_icon(-10);
        assert_eq!(icon, "✗");
        assert_eq!(color, theme::ERROR_FG);
    }

    #[test]
    fn reviewer_vote_waiting() {
        let (icon, color) = reviewer_vote_icon(-5);
        assert_eq!(icon, "●");
        assert_eq!(color, theme::WARNING_FG);
    }

    #[test]
    fn reviewer_vote_no_vote() {
        let (icon, color) = reviewer_vote_icon(0);
        assert_eq!(icon, "○");
        assert_eq!(color, theme::PENDING_FG);
    }
}
