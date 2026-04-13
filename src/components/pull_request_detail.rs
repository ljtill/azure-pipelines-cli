//! Pull Request detail view component.

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::Component;
use crate::render::theme;
use crate::state::{App, ListNav};

/// Stores state for the Pull Request detail drill-in view.
#[derive(Debug, Default)]
pub struct PullRequestDetail {
    pub pull_request: Option<crate::client::models::PullRequest>,
    pub threads: Vec<crate::client::models::PullRequestThread>,
    pub nav: ListNav,
    pub loading: bool,
}

impl PullRequestDetail {
    /// Renders the PR detail view using data from the App.
    pub fn draw_with_app(&self, f: &mut Frame, _app: &App, area: Rect) {
        // Placeholder rendering until Phase 3 wires the full detail view.
        let text = if self.loading {
            Line::from(Span::styled("  Loading pull request…", theme::MUTED))
        } else if let Some(pr) = &self.pull_request {
            Line::from(vec![Span::styled(
                format!("  PR #{} — {}", pr.pull_request_id, pr.title),
                theme::SECTION_HEADER,
            )])
        } else {
            Line::from(Span::styled("  No pull request selected", theme::MUTED))
        };
        let paragraph = Paragraph::new(vec![text]);
        f.render_widget(paragraph, area);
    }
}

impl Component for PullRequestDetail {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn footer_hints(&self) -> &'static str {
        "←/q/Esc back  ↑↓ navigate  o open  1/2/3/4 tabs  ? help"
    }
}
