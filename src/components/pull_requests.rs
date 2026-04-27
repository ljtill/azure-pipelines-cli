//! Pull Requests list view component.

use std::collections::BTreeMap;

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
    sub_view_tab_spans,
};
use crate::render::table::{
    Align, DEFAULT_SCROLL_PADDING, format_cell, render_header, resolve_widths, visible_rows,
};
use crate::render::theme;
use crate::state::{App, InputMode, ListNav};

/// Stores state for the Pull Requests list view.
#[derive(Debug, Default)]
pub struct PullRequests {
    /// All PRs received from the API, keyed by stable pull request ID.
    all: BTreeMap<u32, crate::client::models::PullRequest>,
    /// Source ordering from the API, de-duplicated by pull request ID.
    order: Vec<u32>,
    /// Filtered and sorted pull request IDs for display.
    pub filtered: Vec<u32>,
    pub nav: ListNav,
    /// Monotonic counter incremented on each fetch request to discard stale responses.
    pub generation: u64,
}

fn pr_status_style(status: &str, is_draft: bool) -> Style {
    if is_draft {
        return theme::PR_DRAFT;
    }

    match status.to_ascii_lowercase().as_str() {
        "active" => theme::PR_ACTIVE,
        "completed" => theme::PR_COMPLETED,
        "abandoned" => theme::PR_ABANDONED,
        _ => theme::VOTE_NONE,
    }
}

fn pr_title_style(status: &str, is_draft: bool) -> Style {
    if is_draft {
        return theme::SUBTLE;
    }

    match status.to_ascii_lowercase().as_str() {
        "completed" => theme::SUBTLE,
        "abandoned" => theme::MUTED,
        _ => theme::TEXT,
    }
}

fn pr_title_spans(pr: &crate::client::models::PullRequest, width: usize) -> Vec<Span<'static>> {
    let draft_marker = if pr.is_draft { " [draft]" } else { "" };
    let prefix = format!("#{} ", pr.pull_request_id);
    let title_text = format!("{prefix}{}{}", pr.title, draft_marker);
    let title_cell = format_cell(&title_text, width, Align::Left);
    let title_style = pr_title_style(&pr.status, pr.is_draft);
    let prefix_style = theme::SUBTLE;

    if !title_cell.starts_with(&prefix) {
        return vec![Span::styled(title_cell, title_style)];
    }

    let mut spans = vec![Span::styled(prefix.clone(), prefix_style)];
    let rest = &title_cell[prefix.len()..];
    if pr.is_draft
        && let Some(marker_start) = rest.find(draft_marker)
    {
        let (title, marker_and_padding) = rest.split_at(marker_start);
        let (marker, padding) = marker_and_padding.split_at(draft_marker.len());
        spans.push(Span::styled(title.to_string(), title_style));
        spans.push(Span::styled(marker.to_string(), theme::PR_DRAFT));
        if !padding.is_empty() {
            spans.push(Span::styled(padding.to_string(), title_style));
        }
    } else {
        spans.push(Span::styled(rest.to_string(), title_style));
    }
    spans
}

fn vote_spans(
    approved: usize,
    rejected: usize,
    waiting: usize,
    has_reviewers: bool,
    width: usize,
) -> Vec<Span<'static>> {
    if !has_reviewers {
        return vec![Span::styled(
            format_cell("", width, Align::Left),
            theme::VOTE_NONE,
        )];
    }

    let approved_text = format!("✓{approved}");
    let rejected_text = format!("✗{rejected}");
    let waiting_text = format!("●{waiting}");
    let vote_summary = format!("{approved_text} {rejected_text} {waiting_text}");
    let vote_cell = format_cell(&vote_summary, width, Align::Left);
    if !vote_cell.starts_with(&vote_summary) {
        return vec![Span::styled(vote_cell, theme::VOTE_NONE)];
    }
    let padding = vote_cell[vote_summary.len()..].to_string();

    vec![
        Span::styled(approved_text, theme::VOTE_APPROVED),
        Span::styled(" ".to_string(), theme::VOTE_NONE),
        Span::styled(rejected_text, theme::VOTE_REJECTED),
        Span::styled(" ".to_string(), theme::VOTE_NONE),
        Span::styled(waiting_text, theme::VOTE_WAITING),
        Span::styled(padding, theme::VOTE_NONE),
    ]
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
        self.order.clear();
        self.all.clear();
        for pull_request in pull_requests {
            let pull_request_id = pull_request.pull_request_id;
            if !self.all.contains_key(&pull_request_id) {
                self.order.push(pull_request_id);
            }
            self.all.insert(pull_request_id, pull_request);
        }
        self.rebuild(search_query);
    }

    /// Rebuilds the filtered list from `all` using the search query.
    pub fn rebuild(&mut self, search_query: &str) {
        if search_query.is_empty() {
            self.filtered = self.order.clone();
        } else {
            let q = search_query.to_lowercase();
            self.filtered = self
                .order
                .iter()
                .copied()
                .filter(|pull_request_id| {
                    self.all.get(pull_request_id).is_some_and(|pr| {
                        pr.title.to_lowercase().contains(&q)
                            || pr.repo_name().to_lowercase().contains(&q)
                            || pr.author().to_lowercase().contains(&q)
                    })
                })
                .collect();
        }
        let all = &self.all;
        self.filtered
            .sort_by_key(|pull_request_id| all[pull_request_id].is_draft);
        self.nav.set_len(self.filtered.len());
    }

    /// Returns the pull request at the filtered row index.
    pub fn pull_request_at(&self, index: usize) -> Option<&crate::client::models::PullRequest> {
        self.filtered
            .get(index)
            .and_then(|pull_request_id| self.all.get(pull_request_id))
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

        let window = visible_rows(
            self.filtered.len(),
            self.nav.index(),
            list_area.height,
            DEFAULT_SCROLL_PADDING,
        );
        let items: Vec<ListItem> = window
            .range()
            .filter_map(|i| {
                let pr = self.pull_request_at(i)?;
                let (icon, _) = pr_status_icon(&pr.status, pr.is_draft);
                let (approved, rejected, waiting, _no_vote) = pr.vote_summary();
                let is_selected = window.selected == Some(i - window.start);

                let w_icon = widths[schema.icon];
                let w_title = widths[schema.title];
                let w_repo = widths[schema.repo];
                let w_author = widths[schema.author.unwrap()];
                let w_branch = widths[schema.branch];
                let w_votes = widths[schema.votes];

                let mut spans = vec![Span::styled(
                    format_cell(icon, w_icon, Align::Left),
                    pr_status_style(&pr.status, pr.is_draft),
                )];
                spans.extend(pr_title_spans(pr, w_title));
                spans.extend([
                    Span::styled(
                        format_cell(pr.repo_name(), w_repo, Align::Left),
                        theme::SUBTLE,
                    ),
                    Span::styled(
                        format_cell(pr.author(), w_author, Align::Left),
                        theme::MUTED,
                    ),
                    Span::styled(
                        format_cell(pr.short_target_branch(), w_branch, Align::Left),
                        theme::BRANCH,
                    ),
                ]);
                spans.extend(vote_spans(
                    approved,
                    rejected,
                    waiting,
                    !pr.reviewers.is_empty(),
                    w_votes,
                ));

                Some(ListItem::new(Line::from(spans)).style(row_style(is_selected)))
            })
            .collect();

        let list = List::new(items).scroll_padding(DEFAULT_SCROLL_PADDING);
        let mut state = ListState::default();
        state.select(window.selected);
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
        assert_eq!(prs.pull_request_at(0).unwrap().pull_request_id, 1);
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
        assert_eq!(prs.pull_request_at(0).unwrap().pull_request_id, 2);
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
        assert_eq!(prs.pull_request_at(0).unwrap().pull_request_id, 2);
        assert_eq!(prs.pull_request_at(1).unwrap().pull_request_id, 1);
    }

    #[test]
    fn set_data_normalizes_duplicate_pull_request_ids() {
        let mut prs = PullRequests::default();
        let first = make_pull_request(1, "Old title", "active", "frontend");
        let updated = make_pull_request(1, "Updated title", "active", "frontend");

        prs.set_data(vec![first, updated], "");

        assert_eq!(prs.filtered, vec![1]);
        assert_eq!(prs.pull_request_at(0).unwrap().title, "Updated title");
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
