use ratatui::style::Color;

use crate::api::models::{Build, BuildResult, BuildStatus, TaskState};

/// Shared status → (icon, color) mapping for build status and result.
pub fn status_icon(status: BuildStatus, result: Option<BuildResult>) -> (&'static str, Color) {
    if status.is_in_progress() {
        return ("⏳", Color::Yellow);
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
            Some(TaskState::InProgress) => ("⏳", Color::Yellow),
            Some(TaskState::Completed) => ("✓", Color::Green),
            Some(TaskState::Pending) => ("○", Color::DarkGray),
            _ => ("○", Color::DarkGray),
        },
    }
}

/// Format a build's elapsed time or "ago" string.
pub fn build_elapsed(build: &Build) -> String {
    use chrono::Utc;

    if build.status.is_in_progress() {
        if let Some(start) = build.start_time {
            let elapsed = Utc::now().signed_duration_since(start);
            return format!("running {}m", elapsed.num_minutes());
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

/// Truncate a string to at most `max_len` characters, safe for multi-byte UTF-8.
pub fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        return s;
    }
    let mut end = max_len;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}
