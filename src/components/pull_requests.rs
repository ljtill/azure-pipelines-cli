//! Pull Requests list view component.

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};

use super::Component;
use crate::render::columns::{PullRequestRowOpts, pull_request_row};
use crate::render::helpers::{
    draw_state_message, draw_view_frame, pr_status_icon, row_style, split_with_search_bar,
    sub_view_tab_spans, truncate,
};
use crate::render::table::{render_header, resolve_widths};
use crate::render::theme;
use crate::state::{App, InputMode, ListNav};

/// Stores state for the Pull Requests list view.
#[derive(Debug, Default)]
pub struct PullRequests {
    /// All PRs received from the API (unfiltered).
    all: Vec<crate::client::models::PullRequest>,
    /// Filtered and sorted list for display.
    pub filtered: Vec<crate::client::models::PullRequest>,
    pub nav: ListNav,
    /// Monotonic counter incremented on each fetch request to discard stale responses.
    pub generation: u64,
}

impl PullRequests {
    /// Increments the generation counter and returns the new value.
    pub fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    /// Replaces the underlying data and rebuilds the filtered list.
    pub fn set_data(
        &mut self,
        pull_requests: Vec<crate::client::models::PullRequest>,
        search_query: &str,
    ) {
        self.all = pull_requests;
        self.rebuild(search_query);
    }

    /// Rebuilds the filtered list from `all` using the search query.
    pub fn rebuild(&mut self, search_query: &str) {
        if search_query.is_empty() {
            self.filtered = self.all.clone();
        } else {
            let q = search_query.to_lowercase();
            self.filtered = self
                .all
                .iter()
                .filter(|pr| {
                    pr.title.to_lowercase().contains(&q)
                        || pr.repo_name().to_lowercase().contains(&q)
                        || pr.author().to_lowercase().contains(&q)
                })
                .cloned()
                .collect();
        }
        self.filtered.sort_by_key(|pr| pr.is_draft);
        self.nav.set_len(self.filtered.len());
    }

    /// Renders the pull requests list using data from the App.
    pub fn draw_with_app(&self, f: &mut Frame, app: &App, area: Rect) {
        let show_search = app.search.mode == InputMode::Search || !app.search.query.is_empty();
        let mut subtitle_spans = sub_view_tab_spans(app.service, app.view);
        subtitle_spans.push(Span::styled(
            format!("  ·  {} shown", self.filtered.len()),
            theme::MUTED,
        ));
        let frame_area =
            draw_view_frame(f, area, " Pull Requests ", Some(Line::from(subtitle_spans)));

        let list_area = split_with_search_bar(
            f,
            frame_area,
            &app.search.query,
            app.search.mode,
            show_search,
        );

        if self.filtered.is_empty() {
            let hint = if show_search {
                " No pull requests match the current search"
            } else {
                " No pull requests found"
            };
            draw_state_message(f, list_area, hint, theme::MUTED);
            return;
        }

        // Compute column widths via the shared pull-request schema (with author).
        let schema = pull_request_row(PullRequestRowOpts { author: true });
        let list_area = render_header(f, list_area, &schema.columns);
        let widths: Vec<usize> = resolve_widths(&schema.columns, list_area.width)
            .iter()
            .map(|&w| w as usize)
            .collect();

        let items: Vec<ListItem> = self
            .filtered
            .iter()
            .enumerate()
            .map(|(i, pr)| {
                let (icon, color) = pr_status_icon(&pr.status, pr.is_draft);
                let (approved, rejected, waiting, _no_vote) = pr.vote_summary();

                let vote_summary = if pr.reviewers.is_empty() {
                    String::new()
                } else {
                    format!("✓{approved} ✗{rejected} ●{waiting}")
                };

                let draft_marker = if pr.is_draft { " [draft]" } else { "" };
                let w_icon = widths[schema.icon];
                let w_title = widths[schema.title];
                let w_repo = widths[schema.repo];
                let w_author = widths[schema.author.unwrap()];
                let w_branch = widths[schema.branch];
                let w_votes = widths[schema.votes];

                ListItem::new(Line::from(vec![
                    Span::styled(format!("{icon:<w_icon$}"), Style::new().fg(color)),
                    Span::styled(
                        format!(
                            "{:<w_title$}",
                            truncate(
                                &format!("#{} {}{}", pr.pull_request_id, pr.title, draft_marker),
                                w_title
                            )
                        ),
                        theme::TEXT,
                    ),
                    Span::styled(
                        format!("{:<w_repo$}", truncate(pr.repo_name(), w_repo)),
                        theme::MUTED,
                    ),
                    Span::styled(
                        format!("{:<w_author$}", truncate(pr.author(), w_author)),
                        theme::MUTED,
                    ),
                    Span::styled(
                        format!(
                            "{:<w_branch$}",
                            truncate(pr.short_target_branch(), w_branch)
                        ),
                        theme::BRANCH,
                    ),
                    Span::styled(format!("{vote_summary:<w_votes$}"), theme::MUTED),
                ]))
                .style(row_style(i == self.nav.index()))
            })
            .collect();

        let list = List::new(items).scroll_padding(3);
        let mut state = ListState::default();
        state.select(Some(self.nav.index()));
        f.render_stateful_widget(list, list_area, &mut state);
    }
}

impl Component for PullRequests {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn footer_hints(&self) -> &'static str {
        "Tab/Shift-Tab view  ↑↓ navigate  →/Enter detail  / search  o open  r refresh  1–4 areas  ? help"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;

    #[test]
    fn set_data_populates_filtered() {
        let mut prs = PullRequests::default();
        let data = vec![
            make_pull_request(1, "Add feature", "active", "frontend"),
            make_pull_request(2, "Fix bug", "active", "backend"),
        ];
        prs.set_data(data, "");
        assert_eq!(prs.filtered.len(), 2);
        assert_eq!(prs.nav.len(), 2);
    }

    #[test]
    fn rebuild_filters_by_title() {
        let mut prs = PullRequests::default();
        let data = vec![
            make_pull_request(1, "Add feature", "active", "frontend"),
            make_pull_request(2, "Fix bug", "active", "backend"),
        ];
        prs.set_data(data, "feature");
        assert_eq!(prs.filtered.len(), 1);
        assert_eq!(prs.filtered[0].pull_request_id, 1);
    }

    #[test]
    fn rebuild_filters_by_repo() {
        let mut prs = PullRequests::default();
        let data = vec![
            make_pull_request(1, "Add feature", "active", "frontend"),
            make_pull_request(2, "Fix bug", "active", "backend"),
        ];
        prs.set_data(data, "backend");
        assert_eq!(prs.filtered.len(), 1);
        assert_eq!(prs.filtered[0].pull_request_id, 2);
    }

    #[test]
    fn rebuild_filters_by_author() {
        let mut prs = PullRequests::default();
        let data = vec![make_pull_request(1, "Add feature", "active", "frontend")];
        prs.set_data(data, "test user");
        assert_eq!(prs.filtered.len(), 1);
    }

    #[test]
    fn rebuild_empty_query_shows_all() {
        let mut prs = PullRequests::default();
        let data = vec![
            make_pull_request(1, "A", "active", "r"),
            make_pull_request(2, "B", "active", "r"),
        ];
        prs.set_data(data, "");
        assert_eq!(prs.filtered.len(), 2);
    }

    #[test]
    fn rebuild_orders_non_drafts_before_drafts() {
        let mut prs = PullRequests::default();
        let mut draft = make_pull_request(1, "Draft", "active", "r");
        draft.is_draft = true;
        let active = make_pull_request(2, "Active", "active", "r");

        prs.set_data(vec![draft, active], "");

        assert_eq!(prs.filtered.len(), 2);
        assert_eq!(prs.filtered[0].pull_request_id, 2);
        assert_eq!(prs.filtered[1].pull_request_id, 1);
    }

    #[test]
    fn rebuild_no_match() {
        let mut prs = PullRequests::default();
        let data = vec![make_pull_request(1, "Add feature", "active", "frontend")];
        prs.set_data(data, "nonexistent");
        assert_eq!(prs.filtered.len(), 0);
    }

    #[test]
    fn default_generation_is_zero() {
        let prs = PullRequests::default();
        assert_eq!(prs.generation, 0);
    }

    #[test]
    fn next_generation_increments() {
        let mut prs = PullRequests::default();
        assert_eq!(prs.next_generation(), 1);
        assert_eq!(prs.next_generation(), 2);
        assert_eq!(prs.next_generation(), 3);
        assert_eq!(prs.generation, 3);
    }
}
