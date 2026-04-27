//! Semantic color tokens and style definitions for the terminal UI.

use ratatui::style::{Color, Modifier, Style};

// --- Palette ---
pub const CANVAS_BG: Color = Color::Rgb(10, 12, 16);
pub const SURFACE_BG: Color = Color::Rgb(15, 18, 24);
pub const SURFACE_ELEVATED_BG: Color = Color::Rgb(20, 24, 32);
pub const SELECTED_BG: Color = Color::Rgb(31, 36, 48);
pub const BORDER_FG: Color = Color::Rgb(54, 61, 74);
pub const BORDER_FOCUSED_FG: Color = Color::Rgb(111, 130, 166);
pub const TEXT_FG: Color = Color::Rgb(229, 232, 238);
pub const TEXT_SUBTLE_FG: Color = Color::Rgb(148, 156, 171);
pub const TEXT_MUTED_FG: Color = Color::Rgb(104, 112, 128);
pub const ACCENT_FG: Color = Color::Rgb(116, 199, 236);
pub const ACCENT_ALT_FG: Color = Color::Rgb(166, 148, 255);
pub const SUCCESS_FG: Color = Color::Rgb(103, 214, 155);
pub const ERROR_FG: Color = Color::Rgb(255, 111, 119);
pub const WARNING_FG: Color = Color::Rgb(245, 184, 96);
pub const APPROVAL_FG: Color = Color::Rgb(211, 138, 255);
pub const PENDING_FG: Color = TEXT_MUTED_FG;
pub const BRANCH_FG: Color = Color::Rgb(132, 177, 255);

// --- Surfaces ---
pub const CANVAS: Style = Style::new().fg(TEXT_FG).bg(CANVAS_BG);
pub const PANEL: Style = Style::new().fg(TEXT_FG).bg(SURFACE_BG);
pub const PANEL_ELEVATED: Style = Style::new().fg(TEXT_FG).bg(SURFACE_ELEVATED_BG);
pub const PANEL_BORDER: Style = Style::new().fg(BORDER_FG);
pub const PANEL_BORDER_FOCUSED: Style = Style::new().fg(BORDER_FOCUSED_FG);

// --- Header / branding ---
pub const BRAND: Style = Style::new().fg(ACCENT_FG).add_modifier(Modifier::BOLD);
pub const MUTED: Style = Style::new().fg(TEXT_MUTED_FG);
pub const SUBTLE: Style = Style::new().fg(TEXT_SUBTLE_FG);
pub const TEXT: Style = Style::new().fg(TEXT_FG);

// --- Status indicators ---
pub const SUCCESS: Style = Style::new().fg(SUCCESS_FG);
pub const ERROR: Style = Style::new().fg(ERROR_FG);
pub const WARNING: Style = Style::new().fg(WARNING_FG);
pub const PENDING: Style = Style::new().fg(PENDING_FG);
pub const APPROVAL: Style = Style::new().fg(APPROVAL_FG);

// --- Interactive ---
pub const SELECTED: Style = Style::new().fg(TEXT_FG).bg(SELECTED_BG);
pub const SELECTED_ACCENT: Style = Style::new().fg(ACCENT_FG).bg(SELECTED_BG);
pub const SEARCH_PROMPT: Style = Style::new().fg(WARNING_FG);
pub const CURSOR: Style = Style::new().fg(ACCENT_FG);
pub const KEY: Style = Style::new().fg(ACCENT_FG).add_modifier(Modifier::BOLD);

// --- Tree / hierarchy ---
pub const FOLDER: Style = Style::new().fg(WARNING_FG).add_modifier(Modifier::BOLD);
pub const STAGE: Style = Style::new().fg(WARNING_FG).add_modifier(Modifier::BOLD);
pub const JOB: Style = Style::new().fg(TEXT_FG);
pub const ARROW: Style = Style::new().fg(WARNING_FG);
pub const JOB_ARROW: Style = Style::new().fg(ACCENT_FG);

// --- Titles ---
pub const TITLE: Style = Style::new().fg(ACCENT_FG);
pub const FOLLOW_TITLE: Style = Style::new().fg(SUCCESS_FG);

// --- Misc ---
pub const SECTION_HEADER: Style = Style::new().fg(ACCENT_FG).add_modifier(Modifier::BOLD);
pub const TABLE_HEADER: Style = Style::new().fg(TEXT_SUBTLE_FG).add_modifier(Modifier::BOLD);
pub const BRANCH: Style = Style::new().fg(BRANCH_FG);
pub const CHIP: Style = Style::new().fg(TEXT_SUBTLE_FG).bg(SURFACE_ELEVATED_BG);
pub const CHIP_ACTIVE: Style = Style::new().fg(ACCENT_FG).bg(SURFACE_ELEVATED_BG);

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
