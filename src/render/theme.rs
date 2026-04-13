//! Color constants and style definitions for the terminal UI.

use ratatui::style::{Color, Modifier, Style};

// --- Header / branding ---
pub const BRAND: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
pub const MUTED: Style = Style::new().fg(Color::DarkGray);
pub const TEXT: Style = Style::new().fg(Color::White);

// --- Status indicators ---
pub const SUCCESS: Style = Style::new().fg(Color::Green);
pub const ERROR: Style = Style::new().fg(Color::Red);
pub const WARNING: Style = Style::new().fg(Color::Yellow);
pub const PENDING: Style = Style::new().fg(Color::DarkGray);
pub const APPROVAL: Style = Style::new().fg(Color::Magenta);

// --- Interactive ---
pub const SELECTED: Style = Style::new().bg(Color::DarkGray);
pub const SEARCH_PROMPT: Style = Style::new().fg(Color::Yellow);
pub const CURSOR: Style = Style::new().fg(Color::Cyan);

// --- Tree / hierarchy ---
pub const FOLDER: Style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
pub const STAGE: Style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
pub const JOB: Style = Style::new().fg(Color::White);
pub const ARROW: Style = Style::new().fg(Color::Yellow);
pub const JOB_ARROW: Style = Style::new().fg(Color::Cyan);

// --- Titles ---
pub const TITLE: Style = Style::new().fg(Color::Cyan);
pub const FOLLOW_TITLE: Style = Style::new().fg(Color::Green);

// --- Misc ---
pub const SECTION_HEADER: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
pub const BRANCH: Style = Style::new().fg(Color::Blue);

// --- Pull Requests ---
pub const PR_ACTIVE: Style = Style::new().fg(Color::Green);
pub const PR_DRAFT: Style = Style::new().fg(Color::DarkGray);
pub const PR_COMPLETED: Style = Style::new().fg(Color::Cyan);
pub const PR_ABANDONED: Style = Style::new().fg(Color::Red);
pub const VOTE_APPROVED: Style = Style::new().fg(Color::Green);
pub const VOTE_REJECTED: Style = Style::new().fg(Color::Red);
pub const VOTE_WAITING: Style = Style::new().fg(Color::Yellow);
pub const VOTE_NONE: Style = Style::new().fg(Color::DarkGray);
pub const MODE_ACTIVE: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
pub const MODE_INACTIVE: Style = Style::new().fg(Color::DarkGray);
