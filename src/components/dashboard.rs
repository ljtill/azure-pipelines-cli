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
        let rects = Layout::horizontal([
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Length(12),
            Constraint::Fill(2),
            Constraint::Length(18),
            Constraint::Fill(2),
            Constraint::Fill(2),
            Constraint::Length(15),
        ])
        .split(area);
        let mut widths: Vec<usize> = rects.iter().map(|r| r.width as usize).collect();
        widths[3] = widths[3].min(40);
        widths[5] = widths[5].min(35);
        widths[6] = widths[6].min(35);

        let items: Vec<ListItem> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| match row {
                DashboardRow::SectionHeader { title } => ListItem::new(Line::from(vec![
                    Span::raw(" "),
                    Span::styled(title.clone(), theme::SECTION_HEADER),
                ]))
                .style(if i == self.nav.index() {
                    theme::SELECTED
                } else {
                    Style::new()
                }),
                DashboardRow::EmptyHint { message } => ListItem::new(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(message.clone(), theme::MUTED),
                ]))
                .style(if i == self.nav.index() {
                    theme::SELECTED
                } else {
                    Style::new()
                }),
                DashboardRow::PinnedPipeline {
                    definition,
                    latest_build,
                } => {
                    let row_style = if i == self.nav.index() {
                        theme::SELECTED
                    } else {
                        Style::new()
                    };

                    let (icon, icon_color) =
                        latest_build.as_ref().map_or(("○", Color::DarkGray), |b| {
                            let awaiting = app.data.pending_approval_build_ids.contains(&b.id);
                            effective_status_icon(b.status, b.result, awaiting)
                        });
                    let label = latest_build.as_ref().map_or("", |b| {
                        let awaiting = app.data.pending_approval_build_ids.contains(&b.id);
                        effective_status_label(b.status, b.result, awaiting)
                    });

                    let build_spans = latest_build.as_ref().map_or_else(
                        || vec![Span::styled("no builds", theme::MUTED)],
                        |b| {
                            let branch = b.branch_display();
                            let elapsed = build_elapsed(b);
                            vec![
                                Span::styled(
                                    format!(
                                        "#{:<width$}",
                                        truncate(&b.build_number, widths[4] - 1),
                                        width = widths[4] - 1
                                    ),
                                    theme::MUTED,
                                ),
                                Span::styled(
                                    format!(
                                        "{:<width$} ",
                                        truncate(&branch, widths[5].saturating_sub(1)),
                                        width = widths[5].saturating_sub(1)
                                    ),
                                    theme::BRANCH,
                                ),
                                Span::styled(
                                    format!(
                                        "{:<width$} ",
                                        truncate(b.requestor(), widths[6].saturating_sub(1)),
                                        width = widths[6].saturating_sub(1)
                                    ),
                                    theme::MUTED,
                                ),
                                Span::styled(
                                    format!("{:>width$}", elapsed, width = widths[7]),
                                    theme::MUTED,
                                ),
                            ]
                        },
                    );

                    let name_style = if latest_build.is_some() {
                        theme::TEXT
                    } else {
                        theme::MUTED
                    };

                    let mut spans = vec![
                        Span::raw("    "),
                        Span::styled(format!("{icon} "), Style::new().fg(icon_color)),
                        Span::styled(
                            format!("{:<width$}", label, width = widths[2]),
                            Style::new().fg(icon_color),
                        ),
                        Span::styled(
                            format!(
                                "{:<width$} ",
                                truncate(&definition.name, widths[3].saturating_sub(1),),
                                width = widths[3].saturating_sub(1)
                            ),
                            name_style,
                        ),
                    ];
                    spans.extend(build_spans);

                    ListItem::new(Line::from(spans)).style(row_style)
                }
                DashboardRow::DashboardPullRequest { pull_request } => {
                    let row_style = if i == self.nav.index() {
                        theme::SELECTED
                    } else {
                        Style::new()
                    };
                    let (icon, color) = pr_status_icon(&pull_request.status, pull_request.is_draft);
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

                    ListItem::new(Line::from(vec![
                        Span::raw("    "),
                        Span::styled(format!("{icon} "), Style::new().fg(color)),
                        Span::styled(
                            format!(
                                "#{} {}{}  ",
                                pull_request.pull_request_id,
                                truncate(&pull_request.title, 30),
                                draft_marker,
                            ),
                            theme::TEXT,
                        ),
                        Span::styled(
                            format!("{}  ", truncate(pull_request.repo_name(), 15)),
                            theme::MUTED,
                        ),
                        Span::styled(
                            format!("→ {}  ", pull_request.short_target_branch()),
                            theme::BRANCH,
                        ),
                        Span::styled(vote_summary, theme::MUTED),
                    ]))
                    .style(row_style)
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
