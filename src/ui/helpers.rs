use ratatui::style::Color;

use crate::api::models::Build;

/// Shared status → (icon, color) mapping for build status and result strings.
pub fn status_icon(status: &str, result: Option<&str>) -> (&'static str, Color) {
    match status {
        s if s.eq_ignore_ascii_case("inProgress") => ("⏳", Color::Yellow),
        _ => match result {
            Some(r) if r.eq_ignore_ascii_case("succeeded") => ("✓", Color::Green),
            Some(r) if r.eq_ignore_ascii_case("failed") => ("✗", Color::Red),
            Some(r) if r.eq_ignore_ascii_case("partiallySucceeded") => ("◐", Color::Yellow),
            Some(r)
                if r.eq_ignore_ascii_case("canceled") || r.eq_ignore_ascii_case("cancelled") =>
            {
                ("⊘", Color::DarkGray)
            }
            Some(r) if r.eq_ignore_ascii_case("skipped") => ("⊘", Color::DarkGray),
            _ => ("○", Color::DarkGray),
        },
    }
}

/// Status icon for timeline records (stage/job/task) where state and result
/// are separate optional fields.
pub fn timeline_status_icon(state: Option<&str>, result: Option<&str>) -> (&'static str, Color) {
    match result {
        Some(r) if r.eq_ignore_ascii_case("succeeded") => ("✓", Color::Green),
        Some(r) if r.eq_ignore_ascii_case("failed") => ("✗", Color::Red),
        Some(r) if r.eq_ignore_ascii_case("partiallySucceeded") => ("◐", Color::Yellow),
        Some(r)
            if r.eq_ignore_ascii_case("canceled")
                || r.eq_ignore_ascii_case("cancelled")
                || r.eq_ignore_ascii_case("skipped") =>
        {
            ("⊘", Color::DarkGray)
        }
        _ => match state {
            Some(s) if s.eq_ignore_ascii_case("inProgress") => ("⏳", Color::Yellow),
            Some(s) if s.eq_ignore_ascii_case("completed") => ("✓", Color::Green),
            Some(s) if s.eq_ignore_ascii_case("pending") => ("○", Color::DarkGray),
            _ => ("○", Color::DarkGray),
        },
    }
}

/// Format a build's elapsed time or "ago" string.
pub fn build_elapsed(build: &Build) -> String {
    use chrono::Utc;

    if build.status.eq_ignore_ascii_case("inProgress") {
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
