//! Pull Request detail view component.

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};

use super::Component;
use crate::render::helpers::{
    draw_state_message, draw_view_frame, pr_status_icon, reviewer_vote_icon,
};
use crate::render::theme;
use crate::state::{App, ListNav};

/// Stores state for the Pull Request detail drill-in view.
#[derive(Debug, Default)]
pub struct PullRequestDetail {
    pub pull_request: Option<crate::client::models::PullRequest>,
    pub threads: Vec<crate::client::models::PullRequestThread>,
    pub nav: ListNav,
    pub loading: bool,
    /// Root view to return to on back navigation (which PR sub-view was active on drill-in).
    pub return_to: Option<crate::state::View>,
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
        let subtitle = self.pull_request.as_ref().map(|pr| {
            Line::from(vec![
                Span::styled(format!(" #{}", pr.pull_request_id), theme::TEXT),
                Span::styled(format!("  ·  {}", pr.repo_name()), theme::MUTED),
                Span::styled("  ·  ", theme::MUTED),
                Span::styled(pr.short_source_branch(), theme::BRANCH),
                Span::styled(" → ", theme::ARROW),
                Span::styled(pr.short_target_branch(), theme::BRANCH),
            ])
        });
        let content_area = draw_view_frame(f, area, " Pull Request Detail ", subtitle);

        if self.loading {
            draw_state_message(f, content_area, " Loading pull request…", theme::MUTED);
            return;
        }

        let Some(pr) = &self.pull_request else {
            draw_state_message(f, content_area, " No pull request selected", theme::MUTED);
            return;
        };

        let nav_index = self.nav.index();
        let reviewer_sections = pr.reviewers.len().max(1);
        let header_active = nav_index == 0;
        let reviewers_active = (1..=reviewer_sections).contains(&nav_index);
        let threads_active = nav_index == reviewer_sections + 1;

        let chunks = Layout::vertical([
            Constraint::Length(4), // header
            Constraint::Min(4),    // reviewers + threads
        ])
        .split(content_area);

        // Header section.
        let (status_icon, status_color) = pr_status_icon(&pr.status, pr.is_draft);
        let status_style = pr_status_style(&pr.status, pr.is_draft);
        let draft_badge = if pr.is_draft { " [DRAFT]" } else { "" };
        let header_lines = vec![
            Line::from(vec![
                Span::styled(format!(" {status_icon} "), theme::foreground(status_color)),
                Span::styled(
                    format!("PR #{}{draft_badge}", pr.pull_request_id),
                    section_title_style(header_active),
                ),
            ]),
            Line::from(vec![Span::raw("   "), Span::styled(&pr.title, theme::TEXT)]),
            Line::from(""),
            Line::from(vec![
                Span::styled(" Author: ", theme::SUBTLE),
                Span::styled(pr.author(), theme::TEXT),
                Span::styled("    Status: ", theme::SUBTLE),
                Span::styled(&pr.status, status_style),
                Span::styled("    Merge: ", theme::SUBTLE),
                Span::styled(pr.merge_status.as_deref().unwrap_or("—"), theme::TEXT),
            ]),
        ];
        let header = Paragraph::new(header_lines);
        f.render_widget(header, chunks[0]);

        // Reviewers + threads section (reviewers gets 2/5, threads gets 3/5).
        let bottom =
            Layout::horizontal([Constraint::Ratio(2, 5), Constraint::Ratio(3, 5)]).split(chunks[1]);

        // Reviewers panel.
        let mut reviewer_lines = Vec::new();
        if pr.reviewers.is_empty() {
            reviewer_lines.push(Line::from(Span::styled("  No reviewers", theme::SUBTLE)));
        } else {
            for r in &pr.reviewers {
                let (icon, _) = reviewer_vote_icon(r.vote);
                let vote_style = reviewer_vote_style(r.vote);
                let name_style = theme::TEXT;
                let required = if r.is_required { " (required)" } else { "" };
                reviewer_lines.push(Line::from(vec![
                    Span::styled(format!("  {icon} "), vote_style),
                    Span::styled(&r.display_name, name_style),
                    Span::styled(required, theme::SUBTLE),
                ]));
            }
        }
        let reviewers_panel =
            Paragraph::new(reviewer_lines).block(detail_block(" Reviewers ", reviewers_active));
        f.render_widget(reviewers_panel, bottom[0]);

        // Threads panel.
        let total = self.threads.len();
        let active = self.threads.iter().filter(|t| t.is_active()).count();
        let resolved = total - active;
        let mut thread_lines = vec![Line::from(vec![
            Span::styled(format!("  {total} total"), theme::TEXT),
            Span::styled(format!("  ·  {active} active"), theme::WARNING),
            Span::styled(format!("  ·  {resolved} resolved"), theme::SUCCESS),
        ])];
        if let Some(desc) = &pr.description {
            thread_lines.push(Line::from(""));
            for line in desc.lines().take(8) {
                thread_lines.push(Line::from(Span::styled(format!("  {line}"), theme::SUBTLE)));
            }
        }
        let threads_panel = Paragraph::new(thread_lines)
            .block(detail_block(" Threads ", threads_active))
            .wrap(Wrap { trim: false });
        f.render_widget(threads_panel, bottom[1]);
    }
}

fn detail_block<'a, T>(title: T, is_active: bool) -> Block<'a>
where
    T: Into<Line<'a>>,
{
    Block::bordered()
        .title(title)
        .title_style(section_title_style(is_active))
}

fn section_title_style(is_active: bool) -> Style {
    if is_active {
        theme::SECTION_HEADER
    } else {
        theme::TITLE
    }
}

fn pr_status_style(status: &str, is_draft: bool) -> Style {
    if is_draft {
        return theme::PR_DRAFT;
    }
    if status.eq_ignore_ascii_case("active") {
        theme::PR_ACTIVE
    } else if status.eq_ignore_ascii_case("completed") {
        theme::PR_COMPLETED
    } else if status.eq_ignore_ascii_case("abandoned") {
        theme::PR_ABANDONED
    } else {
        theme::PENDING
    }
}

fn reviewer_vote_style(vote: i32) -> Style {
    match vote {
        10 | 5 => theme::VOTE_APPROVED,
        -10 => theme::VOTE_REJECTED,
        -5 => theme::VOTE_WAITING,
        _ => theme::VOTE_NONE,
    }
}

impl Component for PullRequestDetail {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn footer_hints(&self) -> &'static str {
        "←/q/Esc back  ↑↓ navigate  o open  1–4 areas  ? help"
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
