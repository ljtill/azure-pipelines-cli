//! Pull Request detail view component.

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use super::Component;
use crate::render::helpers::{pr_status_icon, reviewer_vote_icon};
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
    /// Returns the number of navigable sections in the detail view.
    pub fn section_count(&self) -> usize {
        if self.pull_request.is_none() {
            return 0;
        }
        let reviewers = self
            .pull_request
            .as_ref()
            .map_or(0, |pr| pr.reviewers.len());
        // Sections: header(1) + reviewers + threads summary(1).
        1 + reviewers.max(1) + 1
    }

    /// Renders the PR detail view using data from the App.
    pub fn draw_with_app(&self, f: &mut Frame, _app: &App, area: Rect) {
        if self.loading {
            let loading = Paragraph::new(Line::from(Span::styled(
                "  Loading pull request…",
                theme::MUTED,
            )));
            f.render_widget(loading, area);
            return;
        }

        let Some(pr) = &self.pull_request else {
            let empty = Paragraph::new(Line::from(Span::styled(
                "  No pull request selected",
                theme::MUTED,
            )));
            f.render_widget(empty, area);
            return;
        };

        let chunks = Layout::vertical([
            Constraint::Length(5), // header
            Constraint::Length(3), // metadata
            Constraint::Min(4),    // reviewers + threads
        ])
        .split(area);

        // Header section.
        let (status_icon, status_color) = pr_status_icon(&pr.status, pr.is_draft);
        let draft_badge = if pr.is_draft { " [DRAFT]" } else { "" };
        let header_lines = vec![
            Line::from(vec![
                Span::styled(
                    format!(" {status_icon} "),
                    ratatui::style::Style::new().fg(status_color),
                ),
                Span::styled(
                    format!("PR #{}{draft_badge}", pr.pull_request_id),
                    theme::SECTION_HEADER,
                ),
            ]),
            Line::from(vec![Span::raw("   "), Span::styled(&pr.title, theme::TEXT)]),
            Line::from(""),
            Line::from(vec![
                Span::styled("   Author: ", theme::MUTED),
                Span::styled(pr.author(), theme::TEXT),
                Span::styled("    Status: ", theme::MUTED),
                Span::styled(&pr.status, ratatui::style::Style::new().fg(status_color)),
                Span::styled(
                    format!("    Merge: {}", pr.merge_status.as_deref().unwrap_or("—")),
                    theme::MUTED,
                ),
            ]),
        ];
        let header = Paragraph::new(header_lines);
        f.render_widget(header, chunks[0]);

        // Metadata section.
        let meta_lines = vec![Line::from(vec![
            Span::styled("   ", theme::MUTED),
            Span::styled(pr.short_source_branch(), theme::BRANCH),
            Span::styled(" → ", theme::ARROW),
            Span::styled(pr.short_target_branch(), theme::BRANCH),
            Span::styled(format!("    Repo: {}", pr.repo_name()), theme::MUTED),
        ])];
        let meta = Paragraph::new(meta_lines);
        f.render_widget(meta, chunks[1]);

        // Reviewers + threads section.
        let bottom = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[2]);

        // Reviewers panel.
        let mut reviewer_lines = vec![Line::from(Span::styled(
            " Reviewers",
            theme::SECTION_HEADER,
        ))];
        if pr.reviewers.is_empty() {
            reviewer_lines.push(Line::from(Span::styled("  No reviewers", theme::MUTED)));
        } else {
            for r in &pr.reviewers {
                let (icon, color) = reviewer_vote_icon(r.vote);
                let required = if r.is_required { " (required)" } else { "" };
                reviewer_lines.push(Line::from(vec![
                    Span::styled(format!("  {icon} "), ratatui::style::Style::new().fg(color)),
                    Span::styled(&r.display_name, theme::TEXT),
                    Span::styled(required, theme::MUTED),
                ]));
            }
        }
        let reviewers_panel =
            Paragraph::new(reviewer_lines).block(Block::new().borders(Borders::RIGHT));
        f.render_widget(reviewers_panel, bottom[0]);

        // Threads panel.
        let total = self.threads.len();
        let active = self.threads.iter().filter(|t| t.is_active()).count();
        let resolved = total - active;
        let mut thread_lines = vec![
            Line::from(Span::styled(" Threads", theme::SECTION_HEADER)),
            Line::from(vec![
                Span::styled(format!("  {total} total"), theme::TEXT),
                Span::styled(format!("  ·  {active} active"), theme::WARNING),
                Span::styled(format!("  ·  {resolved} resolved"), theme::SUCCESS),
            ]),
        ];
        if let Some(desc) = &pr.description {
            thread_lines.push(Line::from(""));
            thread_lines.push(Line::from(Span::styled(
                " Description",
                theme::SECTION_HEADER,
            )));
            for line in desc.lines().take(8) {
                thread_lines.push(Line::from(Span::styled(format!("  {line}"), theme::MUTED)));
            }
        }
        let threads_panel = Paragraph::new(thread_lines).wrap(Wrap { trim: false });
        f.render_widget(threads_panel, bottom[1]);
    }
}

impl Component for PullRequestDetail {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn footer_hints(&self) -> &'static str {
        "←/q/Esc back  ↑↓ navigate  o open  [/] views  1/2/3/4 areas  ? help"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;

    #[test]
    fn section_count_empty() {
        let detail = PullRequestDetail::default();
        assert_eq!(detail.section_count(), 0);
    }

    #[test]
    fn section_count_with_pr() {
        let mut pr = make_pull_request(1, "Test", "active", "repo");
        pr.reviewers = vec![make_reviewer("Alice", 10), make_reviewer("Bob", 0)];
        let detail = PullRequestDetail {
            pull_request: Some(pr),
            ..Default::default()
        };
        // 1 (header) + 2 (reviewers) + 1 (threads) = 4
        assert_eq!(detail.section_count(), 4);
    }

    #[test]
    fn section_count_no_reviewers() {
        let detail = PullRequestDetail {
            pull_request: Some(make_pull_request(1, "Test", "active", "repo")),
            ..Default::default()
        };
        // 1 (header) + 1 (min reviewers section) + 1 (threads) = 3
        assert_eq!(detail.section_count(), 3);
    }
}
