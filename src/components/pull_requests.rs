//! Pull Requests list view component.

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::Component;
use crate::render::theme;
use crate::state::{App, ListNav};

/// Represents the sub-mode filter for the pull requests list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrViewMode {
    #[default]
    CreatedByMe,
    AssignedToMe,
    AllActive,
}

impl PrViewMode {
    /// Cycles to the next sub-mode.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            PrViewMode::CreatedByMe => PrViewMode::AssignedToMe,
            PrViewMode::AssignedToMe => PrViewMode::AllActive,
            PrViewMode::AllActive => PrViewMode::CreatedByMe,
        }
    }

    /// Returns a user-facing label for the mode.
    pub fn label(self) -> &'static str {
        match self {
            PrViewMode::CreatedByMe => "Created by me",
            PrViewMode::AssignedToMe => "Assigned to me",
            PrViewMode::AllActive => "All active",
        }
    }
}

impl std::fmt::Display for PrViewMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Stores state for the Pull Requests list view.
#[derive(Debug, Default)]
pub struct PullRequests {
    pub filtered: Vec<crate::client::models::PullRequest>,
    pub nav: ListNav,
    pub mode: PrViewMode,
}

impl PullRequests {
    /// Renders the pull requests list using data from the App.
    pub fn draw_with_app(&self, f: &mut Frame, _app: &App, area: Rect) {
        // Placeholder rendering until Phase 2 wires the full data flow.
        let text = Line::from(vec![
            Span::styled(" Pull Requests", theme::SECTION_HEADER),
            Span::styled(
                format!("  —  {}  (Tab to switch mode)", self.mode),
                theme::MUTED,
            ),
        ]);
        let paragraph = Paragraph::new(vec![
            text,
            Line::from(""),
            Line::from(Span::styled(
                "  Press 'r' to refresh or switch to this view to load PRs.",
                theme::MUTED,
            )),
        ]);
        f.render_widget(paragraph, area);
    }
}

impl Component for PullRequests {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn footer_hints(&self) -> &'static str {
        "Tab mode  ↑↓ navigate  →/Enter detail  / search  o open  r refresh  1/2/3/4 tabs  ? help"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pr_view_mode_cycles() {
        assert_eq!(PrViewMode::CreatedByMe.next(), PrViewMode::AssignedToMe);
        assert_eq!(PrViewMode::AssignedToMe.next(), PrViewMode::AllActive);
        assert_eq!(PrViewMode::AllActive.next(), PrViewMode::CreatedByMe);
    }

    #[test]
    fn pr_view_mode_labels() {
        assert_eq!(PrViewMode::CreatedByMe.label(), "Created by me");
        assert_eq!(PrViewMode::AssignedToMe.label(), "Assigned to me");
        assert_eq!(PrViewMode::AllActive.label(), "All active");
    }

    #[test]
    fn pr_view_mode_display() {
        assert_eq!(format!("{}", PrViewMode::CreatedByMe), "Created by me");
    }

    #[test]
    fn default_mode_is_created_by_me() {
        let pr = PullRequests::default();
        assert_eq!(pr.mode, PrViewMode::CreatedByMe);
        assert!(pr.filtered.is_empty());
        assert_eq!(pr.nav.index(), 0);
    }
}
