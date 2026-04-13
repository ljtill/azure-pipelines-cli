//! Pull Requests list view component.

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

use super::Component;
use crate::render::helpers::{pr_status_icon, row_style, split_with_search_bar, truncate};
use crate::render::theme;
use crate::state::{App, InputMode, ListNav};

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
    /// All PRs received from the API (unfiltered).
    all: Vec<crate::client::models::PullRequest>,
    /// Filtered and sorted list for display.
    pub filtered: Vec<crate::client::models::PullRequest>,
    pub nav: ListNav,
    pub mode: PrViewMode,
}

impl PullRequests {
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
        self.nav.set_len(self.filtered.len());
    }

    /// Renders the pull requests list using data from the App.
    pub fn draw_with_app(&self, f: &mut Frame, app: &App, area: Rect) {
        let show_search = app.search.mode == InputMode::Search || !app.search.query.is_empty();

        // Split area: mode bar | search bar (optional) | list.
        let top_chunks = Layout::vertical([
            Constraint::Length(1), // mode bar
            Constraint::Min(0),    // rest (search + list)
        ])
        .split(area);

        // Mode indicator bar.
        let mode_spans: Vec<Span> = PrViewMode::ALL
            .iter()
            .enumerate()
            .flat_map(|(i, m)| {
                let style = if *m == self.mode {
                    theme::MODE_ACTIVE
                } else {
                    theme::MODE_INACTIVE
                };
                let mut spans = vec![Span::styled(format!(" {} ", m.label()), style)];
                if i < PrViewMode::ALL.len() - 1 {
                    spans.push(Span::styled(" │ ", theme::MUTED));
                }
                spans
            })
            .collect();
        let mode_line = Paragraph::new(Line::from(mode_spans));
        f.render_widget(mode_line, top_chunks[0]);

        // Search bar + list area.
        let list_area = split_with_search_bar(
            f,
            top_chunks[1],
            &app.search.query,
            app.search.mode,
            show_search,
        );

        if self.filtered.is_empty() {
            let empty = Paragraph::new(Line::from(Span::styled(
                "  No pull requests found",
                theme::MUTED,
            )));
            f.render_widget(empty, list_area);
            return;
        }

        // Compute column widths.
        let widths = Layout::horizontal([
            Constraint::Length(3),  // status icon
            Constraint::Fill(3),    // title
            Constraint::Fill(1),    // repo
            Constraint::Length(12), // author
            Constraint::Length(10), // target branch
            Constraint::Length(12), // votes summary
        ])
        .split(list_area);

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

                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {icon} "), Style::new().fg(color)),
                    Span::styled(
                        format!(
                            "{:<w$} ",
                            truncate(
                                &format!("#{} {}{}", pr.pull_request_id, pr.title, draft_marker),
                                widths[1].width as usize
                            ),
                            w = widths[1].width as usize
                        ),
                        theme::TEXT,
                    ),
                    Span::styled(
                        format!(
                            "{:<w$} ",
                            truncate(pr.repo_name(), widths[2].width as usize),
                            w = widths[2].width as usize
                        ),
                        theme::MUTED,
                    ),
                    Span::styled(
                        format!(
                            "{:<w$} ",
                            truncate(pr.author(), widths[3].width as usize),
                            w = widths[3].width as usize
                        ),
                        theme::MUTED,
                    ),
                    Span::styled(
                        format!(
                            "{:<w$} ",
                            truncate(pr.short_target_branch(), widths[4].width as usize),
                            w = widths[4].width as usize
                        ),
                        theme::BRANCH,
                    ),
                    Span::styled(vote_summary, theme::MUTED),
                ]))
                .style(row_style(i == self.nav.index()))
            })
            .collect();

        let list = List::new(items);
        let mut state = ListState::default();
        state.select(Some(self.nav.index()));
        f.render_stateful_widget(list, list_area, &mut state);
    }
}

impl PrViewMode {
    /// All modes in order, for iteration.
    const ALL: [PrViewMode; 3] = [
        PrViewMode::CreatedByMe,
        PrViewMode::AssignedToMe,
        PrViewMode::AllActive,
    ];
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
    use crate::test_helpers::*;

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
    fn rebuild_no_match() {
        let mut prs = PullRequests::default();
        let data = vec![make_pull_request(1, "Add feature", "active", "frontend")];
        prs.set_data(data, "nonexistent");
        assert_eq!(prs.filtered.len(), 0);
    }
}
