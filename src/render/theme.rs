//! Semantic color tokens and style definitions for the terminal UI.

use ratatui::style::{Color, Modifier, Style};

// --- Palette ---
// Accent colors are for navigation and focus; status colors are reserved for state.
pub const TEXT_SUBTLE_FG: Color = Color::Gray;
pub const TEXT_MUTED_FG: Color = Color::DarkGray;
pub const ACCENT_FG: Color = Color::LightBlue;
pub const SUCCESS_FG: Color = Color::Green;
pub const ERROR_FG: Color = Color::Red;
pub const WARNING_FG: Color = Color::Yellow;
pub const APPROVAL_FG: Color = Color::Magenta;
pub const PENDING_FG: Color = TEXT_MUTED_FG;
pub const BRANCH_FG: Color = Color::Cyan;

// --- Header / branding ---
pub const BRAND: Style = Style::new().fg(ACCENT_FG).add_modifier(Modifier::BOLD);
pub const MUTED: Style = Style::new().fg(TEXT_MUTED_FG);
pub const SUBTLE: Style = Style::new().fg(TEXT_SUBTLE_FG);
pub const TEXT: Style = Style::new();

// --- Status indicators ---
pub const SUCCESS: Style = Style::new().fg(SUCCESS_FG);
pub const ERROR: Style = Style::new().fg(ERROR_FG);
pub const WARNING: Style = Style::new().fg(WARNING_FG);
pub const PENDING: Style = Style::new().fg(PENDING_FG);
pub const APPROVAL: Style = Style::new().fg(APPROVAL_FG);

// --- Interactive ---
pub const SELECTED: Style = Style::new().fg(ACCENT_FG).add_modifier(Modifier::BOLD);
pub const SEARCH_PROMPT: Style = Style::new().fg(ACCENT_FG);
pub const CURSOR: Style = Style::new().fg(ACCENT_FG);
pub const KEY: Style = Style::new().fg(ACCENT_FG).add_modifier(Modifier::BOLD);

// --- Tree / hierarchy ---
pub const FOLDER: Style = Style::new().fg(ACCENT_FG).add_modifier(Modifier::BOLD);
pub const STAGE: Style = Style::new().fg(ACCENT_FG).add_modifier(Modifier::BOLD);
pub const JOB: Style = Style::new();
pub const ARROW: Style = Style::new().fg(TEXT_MUTED_FG);
pub const JOB_ARROW: Style = Style::new().fg(TEXT_MUTED_FG);

// --- Titles ---
pub const TITLE: Style = Style::new().fg(ACCENT_FG);
pub const FOLLOW_TITLE: Style = Style::new().fg(SUCCESS_FG);

// --- Misc ---
pub const SECTION_HEADER: Style = Style::new().fg(ACCENT_FG).add_modifier(Modifier::BOLD);
pub const TABLE_HEADER: Style = Style::new().fg(TEXT_SUBTLE_FG).add_modifier(Modifier::BOLD);
pub const BRANCH: Style = Style::new().fg(BRANCH_FG);

// --- Pull Requests ---
pub const PR_ACTIVE: Style = Style::new().fg(SUCCESS_FG);
pub const PR_DRAFT: Style = Style::new().fg(PENDING_FG);
pub const PR_COMPLETED: Style = Style::new().fg(ACCENT_FG);
pub const PR_ABANDONED: Style = Style::new().fg(ERROR_FG);
pub const VOTE_APPROVED: Style = Style::new().fg(SUCCESS_FG);
pub const VOTE_REJECTED: Style = Style::new().fg(ERROR_FG);
pub const VOTE_WAITING: Style = Style::new().fg(WARNING_FG);
pub const VOTE_NONE: Style = Style::new().fg(PENDING_FG);
pub const MODE_ACTIVE: Style = Style::new().fg(ACCENT_FG).add_modifier(Modifier::BOLD);
pub const MODE_INACTIVE: Style = Style::new().fg(TEXT_MUTED_FG);

/// Returns a foreground-only style for dynamic semantic colors.
pub fn foreground(color: Color) -> Style {
    Style::new().fg(color)
}

/// Returns a style with a dynamic semantic foreground color applied.
pub fn with_foreground(style: Style, color: Color) -> Style {
    style.fg(color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_styles_do_not_override_terminal_background() {
        assert_eq!(TEXT.bg, None);
        assert_eq!(MUTED.bg, None);
        assert_eq!(SUBTLE.bg, None);
        assert_eq!(BRAND.bg, None);
        assert_eq!(SECTION_HEADER.bg, None);
    }

    #[test]
    fn selected_rows_do_not_override_terminal_background() {
        assert_eq!(SELECTED.bg, None);
        assert_eq!(SELECTED.fg, Some(ACCENT_FG));
        assert!(SELECTED.add_modifier.contains(Modifier::BOLD));
        assert!(!SELECTED.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn dynamic_foreground_styles_do_not_override_terminal_background() {
        assert_eq!(foreground(SUCCESS_FG).fg, Some(SUCCESS_FG));
        assert_eq!(foreground(SUCCESS_FG).bg, None);
        assert_eq!(with_foreground(Style::new(), ERROR_FG).fg, Some(ERROR_FG));
        assert_eq!(with_foreground(Style::new(), ERROR_FG).bg, None);
    }

    #[test]
    fn structural_styles_do_not_use_status_warning_color() {
        assert_eq!(SEARCH_PROMPT.fg, Some(ACCENT_FG));
        assert_eq!(FOLDER.fg, Some(ACCENT_FG));
        assert_eq!(STAGE.fg, Some(ACCENT_FG));
        assert_eq!(ARROW.fg, Some(TEXT_MUTED_FG));
        assert_eq!(JOB_ARROW.fg, Some(TEXT_MUTED_FG));
    }
}
