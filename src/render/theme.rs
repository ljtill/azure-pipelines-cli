//! Semantic color tokens and style definitions for the terminal UI.

use ratatui::style::{Color, Modifier, Style};

// --- Palette ---
pub const BORDER_FG: Color = Color::DarkGray;
pub const BORDER_FOCUSED_FG: Color = Color::LightBlue;
pub const TEXT_FG: Color = Color::Reset;
pub const TEXT_SUBTLE_FG: Color = Color::Gray;
pub const TEXT_MUTED_FG: Color = Color::DarkGray;
pub const ACCENT_FG: Color = Color::LightBlue;
pub const ACCENT_ALT_FG: Color = Color::Cyan;
pub const SUCCESS_FG: Color = Color::Green;
pub const ERROR_FG: Color = Color::Red;
pub const WARNING_FG: Color = Color::Yellow;
pub const APPROVAL_FG: Color = Color::Magenta;
pub const PENDING_FG: Color = TEXT_MUTED_FG;
pub const BRANCH_FG: Color = Color::Cyan;

// --- Surfaces ---
pub const CANVAS: Style = Style::new();
pub const PANEL: Style = Style::new();
pub const PANEL_ELEVATED: Style = Style::new();
pub const PANEL_BORDER: Style = Style::new().fg(BORDER_FG);
pub const PANEL_BORDER_FOCUSED: Style = Style::new().fg(BORDER_FOCUSED_FG);

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
pub const SELECTED: Style = Style::new().add_modifier(Modifier::REVERSED);
pub const SELECTED_ACCENT: Style = Style::new()
    .fg(ACCENT_FG)
    .add_modifier(Modifier::BOLD.union(Modifier::REVERSED));
pub const SEARCH_PROMPT: Style = Style::new().fg(WARNING_FG);
pub const CURSOR: Style = Style::new().fg(ACCENT_FG);
pub const KEY: Style = Style::new().fg(ACCENT_FG).add_modifier(Modifier::BOLD);

// --- Tree / hierarchy ---
pub const FOLDER: Style = Style::new().fg(WARNING_FG).add_modifier(Modifier::BOLD);
pub const STAGE: Style = Style::new().fg(WARNING_FG).add_modifier(Modifier::BOLD);
pub const JOB: Style = Style::new();
pub const ARROW: Style = Style::new().fg(WARNING_FG);
pub const JOB_ARROW: Style = Style::new().fg(ACCENT_FG);

// --- Titles ---
pub const TITLE: Style = Style::new().fg(ACCENT_FG);
pub const FOLLOW_TITLE: Style = Style::new().fg(SUCCESS_FG);

// --- Misc ---
pub const SECTION_HEADER: Style = Style::new().fg(ACCENT_FG).add_modifier(Modifier::BOLD);
pub const TABLE_HEADER: Style = Style::new().fg(TEXT_SUBTLE_FG).add_modifier(Modifier::BOLD);
pub const BRANCH: Style = Style::new().fg(BRANCH_FG);
pub const CHIP: Style = Style::new().fg(TEXT_SUBTLE_FG);
pub const CHIP_ACTIVE: Style = Style::new().fg(ACCENT_FG).add_modifier(Modifier::BOLD);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_styles_do_not_override_terminal_background() {
        assert_eq!(CANVAS.bg, None);
        assert_eq!(PANEL.bg, None);
        assert_eq!(PANEL_ELEVATED.bg, None);
        assert_eq!(CHIP.bg, None);
        assert_eq!(CHIP_ACTIVE.bg, None);
    }

    #[test]
    fn selected_rows_use_terminal_reverse_video() {
        assert_eq!(SELECTED.bg, None);
        assert!(SELECTED.add_modifier.contains(Modifier::REVERSED));
        assert_eq!(SELECTED_ACCENT.bg, None);
        assert!(SELECTED_ACCENT.add_modifier.contains(Modifier::REVERSED));
    }
}
