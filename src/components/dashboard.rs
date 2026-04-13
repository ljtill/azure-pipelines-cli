//! Dashboard view component showing pinned pipelines and personal pull requests.

use std::collections::BTreeMap;

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};

use super::Component;
use crate::client::models::{Build, PipelineDefinition, PullRequest};
use crate::render::helpers::{
    build_elapsed, effective_status_icon, effective_status_label, pr_status_icon, truncate,
};
use crate::render::theme;
use crate::state::App;
use crate::state::nav::ListNav;

/// Represents a row in the dashboard view.
#[derive(Debug, Clone)]
pub enum DashboardRow {
    SectionHeader {
        title: String,
    },
    PinnedPipeline {
        definition: PipelineDefinition,
        latest_build: Option<Box<Build>>,
    },
    DashboardPullRequest {
        pull_request: Box<PullRequest>,
    },
    EmptyHint {
        message: String,
    },
}

/// Returns a string of `n` spaces for column padding.
fn pad(n: usize) -> String {
    " ".repeat(n)
}

/// Renders the personalised dashboard with pinned pipelines and pull requests.
#[derive(Debug, Default)]
pub struct Dashboard {
    pub rows: Vec<DashboardRow>,
    pub nav: ListNav,
}

impl Dashboard {
    /// Rebuilds the dashboard from pinned pipeline IDs, definitions, latest builds, and PRs.
    pub fn rebuild(
        &mut self,
        definitions: &[PipelineDefinition],
        latest_builds_by_def: &BTreeMap<u32, Build>,
        pinned_ids: &[u32],
        dashboard_prs: &[PullRequest],
    ) {
        let mut rows = Vec::new();

        // --- Pinned Pipelines section ---
        rows.push(DashboardRow::SectionHeader {
            title: "Pinned Pipelines".to_string(),
        });

        let mut pinned: Vec<(PipelineDefinition, Option<Build>)> = pinned_ids
            .iter()
            .filter_map(|id| {
                definitions
                    .iter()
                    .find(|d| d.id == *id)
                    .map(|d| (d.clone(), latest_builds_by_def.get(id).cloned()))
            })
            .collect();
        pinned.sort_by(|(a, _), (b, _)| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        if pinned.is_empty() {
            rows.push(DashboardRow::EmptyHint {
                message: "No pipelines pinned — press 'p' in the Pipelines view to pin".to_string(),
            });
        } else {
            for (def, build) in pinned {
                rows.push(DashboardRow::PinnedPipeline {
                    definition: def,
                    latest_build: build.map(Box::new),
                });
            }
        }

        // --- My Pull Requests section ---
        rows.push(DashboardRow::SectionHeader {
            title: "My Pull Requests".to_string(),
        });

        if dashboard_prs.is_empty() {
            rows.push(DashboardRow::EmptyHint {
                message: "No pull requests found".to_string(),
            });
        } else {
            for pr in dashboard_prs.iter().take(10) {
                rows.push(DashboardRow::DashboardPullRequest {
                    pull_request: Box::new(pr.clone()),
                });
            }
        }

        self.rows = rows;
        self.nav.set_len(self.rows.len());
    }

    /// Returns the pipeline definition at the given row index, if it is a pinned pipeline.
    pub fn pinned_definition_at(&self, index: usize) -> Option<&PipelineDefinition> {
        match self.rows.get(index) {
            Some(DashboardRow::PinnedPipeline { definition, .. }) => Some(definition),
            _ => None,
        }
    }

    /// Returns the pull request at the given row index, if it is a dashboard PR.
    pub fn pull_request_at(&self, index: usize) -> Option<&PullRequest> {
        match self.rows.get(index) {
            Some(DashboardRow::DashboardPullRequest { pull_request }) => Some(pull_request),
            _ => None,
        }
    }

    /// Renders the dashboard using data from the App.
    pub fn draw_with_app(&self, f: &mut Frame, app: &App, area: Rect) {
        // Shared column grid for both Pinned Pipeline and PR rows.
        let rects = Layout::horizontal([
            Constraint::Length(3),  // col 0: indent
            Constraint::Length(2),  // col 1: status icon
            Constraint::Length(11), // col 2: status label / PR id
            Constraint::Fill(2),    // col 3: name / title
            Constraint::Length(16), // col 4: build number / repo
            Constraint::Fill(1),    // col 5: branch
            Constraint::Fill(1),    // col 6: requestor / votes
            Constraint::Length(12), // col 7: elapsed
        ])
        .split(area);
        let w: Vec<usize> = rects.iter().map(|r| r.width as usize).collect();

        let items: Vec<ListItem> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let selected = i == self.nav.index();
                let sel_style = if selected {
                    theme::SELECTED
                } else {
                    Style::new()
                };

                match row {
                    DashboardRow::SectionHeader { title } => ListItem::new(Line::from(vec![
                        Span::raw(pad(w[0])),
                        Span::styled(title.clone(), theme::SECTION_HEADER),
                    ]))
                    .style(sel_style),

                    DashboardRow::EmptyHint { message } => ListItem::new(Line::from(vec![
                        Span::raw(pad(w[0])),
                        Span::raw(pad(w[1])),
                        Span::styled(message.clone(), theme::MUTED),
                    ]))
                    .style(sel_style),

                    DashboardRow::PinnedPipeline {
                        definition,
                        latest_build,
                    } => {
                        let (icon, icon_color) =
                            latest_build.as_ref().map_or(("○", Color::DarkGray), |b| {
                                let awaiting = app.data.pending_approval_build_ids.contains(&b.id);
                                effective_status_icon(b.status, b.result, awaiting)
                            });
                        let label = latest_build.as_ref().map_or("", |b| {
                            let awaiting = app.data.pending_approval_build_ids.contains(&b.id);
                            effective_status_label(b.status, b.result, awaiting)
                        });
                        let name_style = if latest_build.is_some() {
                            theme::TEXT
                        } else {
                            theme::MUTED
                        };

                        let mut spans = vec![
                            Span::raw(pad(w[0])),
                            Span::styled(
                                format!("{:<w$}", icon, w = w[1]),
                                Style::new().fg(icon_color),
                            ),
                            Span::styled(
                                format!("{:<w$}", label, w = w[2]),
                                Style::new().fg(icon_color),
                            ),
                            Span::styled(
                                format!("{:<w$}", truncate(&definition.name, w[3]), w = w[3]),
                                name_style,
                            ),
                        ];

                        if let Some(b) = latest_build {
                            let branch = b.branch_display();
                            let elapsed = build_elapsed(b);
                            spans.extend([
                                Span::styled(
                                    format!(
                                        "{:<w$}",
                                        format!(
                                            "#{}",
                                            truncate(&b.build_number, w[4].saturating_sub(2))
                                        ),
                                        w = w[4]
                                    ),
                                    theme::MUTED,
                                ),
                                Span::styled(
                                    format!("{:<w$}", truncate(&branch, w[5]), w = w[5]),
                                    theme::BRANCH,
                                ),
                                Span::styled(
                                    format!("{:<w$}", truncate(b.requestor(), w[6]), w = w[6]),
                                    theme::MUTED,
                                ),
                                Span::styled(format!("{:>w$}", elapsed, w = w[7]), theme::MUTED),
                            ]);
                        } else {
                            spans.push(Span::styled("no builds", theme::MUTED));
                        }

                        ListItem::new(Line::from(spans)).style(sel_style)
                    }

                    DashboardRow::DashboardPullRequest { pull_request } => {
                        let (icon, color) =
                            pr_status_icon(&pull_request.status, pull_request.is_draft);
                        let (approved, rejected, waiting, _) = pull_request.vote_summary();
                        let vote_summary = if pull_request.reviewers.is_empty() {
                            String::new()
                        } else {
                            format!("✓{approved} ✗{rejected} ●{waiting}")
                        };
                        let draft_marker = if pull_request.is_draft {
                            " [draft]"
                        } else {
                            ""
                        };
                        let title_text = format!(
                            "#{} {}{}",
                            pull_request.pull_request_id, pull_request.title, draft_marker
                        );

                        ListItem::new(Line::from(vec![
                            Span::raw(pad(w[0])),
                            Span::styled(format!("{:<w$}", icon, w = w[1]), Style::new().fg(color)),
                            // PR id + title span across col 2 + col 3.
                            Span::styled(
                                format!(
                                    "{:<w$}",
                                    truncate(&title_text, w[2] + w[3]),
                                    w = w[2] + w[3]
                                ),
                                theme::TEXT,
                            ),
                            Span::styled(
                                format!(
                                    "{:<w$}",
                                    truncate(pull_request.repo_name(), w[4]),
                                    w = w[4]
                                ),
                                theme::MUTED,
                            ),
                            Span::styled(
                                format!(
                                    "{:<w$}",
                                    format!(
                                        "→ {}",
                                        truncate(
                                            pull_request.short_target_branch(),
                                            w[5].saturating_sub(2)
                                        )
                                    ),
                                    w = w[5]
                                ),
                                theme::BRANCH,
                            ),
                            Span::styled(format!("{:<w$}", vote_summary, w = w[6]), theme::MUTED),
                        ]))
                        .style(sel_style)
                    }
                }
            })
            .collect();

        let list = List::new(items);

        let mut state = ListState::default();
        state.select(Some(self.nav.index()));
        f.render_stateful_widget(list, area, &mut state);
    }
}

impl Component for Dashboard {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn footer_hints(&self) -> &'static str {
        "↑↓ navigate  Enter drill-in  Q queue  o open  r refresh  , settings  ? help  q quit"
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::test_helpers::*;

    #[test]
    fn rebuild_with_pinned_definitions() {
        let defs = vec![
            make_definition(1, "CI", "\\"),
            make_definition(2, "Deploy", "\\Infra"),
            make_definition(3, "Lint", "\\"),
        ];
        let mut d = Dashboard::default();
        d.rebuild(&defs, &BTreeMap::new(), &[1, 3], &[]);
        // SectionHeader + 2 pinned + SectionHeader + EmptyHint(no PRs) = 5
        assert_eq!(d.rows.len(), 5);
        assert!(
            matches!(&d.rows[0], DashboardRow::SectionHeader { title } if title == "Pinned Pipelines")
        );
        assert!(
            matches!(&d.rows[1], DashboardRow::PinnedPipeline { definition, .. } if definition.id == 1)
        );
        assert!(
            matches!(&d.rows[3], DashboardRow::SectionHeader { title } if title == "My Pull Requests")
        );
    }

    #[test]
    fn rebuild_empty_pins_shows_hint() {
        let defs = vec![make_definition(1, "CI", "\\")];
        let mut d = Dashboard::default();
        d.rebuild(&defs, &BTreeMap::new(), &[], &[]);
        // SectionHeader + EmptyHint + SectionHeader + EmptyHint = 4
        assert_eq!(d.rows.len(), 4);
        assert!(matches!(&d.rows[1], DashboardRow::EmptyHint { .. }));
    }

    #[test]
    fn rebuild_with_prs() {
        let defs = vec![];
        let prs = vec![
            make_pull_request(1, "PR One", "active", "repo"),
            make_pull_request(2, "PR Two", "active", "repo"),
        ];
        let mut d = Dashboard::default();
        d.rebuild(&defs, &BTreeMap::new(), &[], &prs);
        // SectionHeader("Pinned") + EmptyHint + SectionHeader("My PRs") + 2 PRs = 5
        assert_eq!(d.rows.len(), 5);
        assert!(
            matches!(&d.rows[2], DashboardRow::SectionHeader { title } if title == "My Pull Requests")
        );
        assert!(matches!(
            &d.rows[3],
            DashboardRow::DashboardPullRequest { .. }
        ));
    }

    #[test]
    fn pinned_definition_at() {
        let defs = vec![make_definition(1, "CI", "\\")];
        let mut d = Dashboard::default();
        d.rebuild(&defs, &BTreeMap::new(), &[1], &[]);
        assert!(d.pinned_definition_at(0).is_none()); // section header
        assert_eq!(d.pinned_definition_at(1).unwrap().id, 1);
    }

    #[test]
    fn pull_request_at() {
        let prs = vec![make_pull_request(42, "Test", "active", "repo")];
        let mut d = Dashboard::default();
        d.rebuild(&[], &BTreeMap::new(), &[], &prs);
        // Row 0 = SectionHeader("Pinned"), 1 = EmptyHint, 2 = SectionHeader("My PRs"), 3 = PR.
        assert!(d.pull_request_at(2).is_none()); // Section header
        assert_eq!(d.pull_request_at(3).unwrap().pull_request_id, 42);
    }

    #[test]
    fn prs_limited_to_10() {
        let prs: Vec<PullRequest> = (0..15)
            .map(|i| make_pull_request(i, &format!("PR {i}"), "active", "repo"))
            .collect();
        let mut d = Dashboard::default();
        d.rebuild(&[], &BTreeMap::new(), &[], &prs);
        let pr_count = d
            .rows
            .iter()
            .filter(|r| matches!(r, DashboardRow::DashboardPullRequest { .. }))
            .count();
        assert_eq!(pr_count, 10);
    }
}
