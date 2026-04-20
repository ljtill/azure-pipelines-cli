//! Global dashboard view component showing pinned pipelines and personal pull requests.

use std::collections::BTreeMap;

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};

use super::Component;
use crate::client::models::{Build, PipelineDefinition, PullRequest};
use crate::render::columns::{BuildRowOpts, build_row, pull_request_row};
use crate::render::helpers::{
    build_elapsed, draw_view_frame, effective_status_icon, effective_status_label, pr_status_icon,
    row_style, truncate,
};
use crate::render::table::resolve_widths;
use crate::render::theme;
use crate::state::nav::ListNav;
use crate::state::{App, DashboardPullRequestsState};

/// Represents a row in the dashboard view.
#[derive(Debug, Clone)]
pub enum DashboardRow {
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
    SectionHeader {
        label: String,
    },
}

/// Returns a string of `n` spaces for column padding.
fn pad(n: usize) -> String {
    " ".repeat(n)
}

/// Renders the cross-service dashboard with pinned pipelines and pull requests.
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
        dashboard_prs: &DashboardPullRequestsState,
    ) {
        let mut rows = Vec::new();

        rows.push(DashboardRow::SectionHeader {
            label: "Pinned Pipelines".to_string(),
        });

        // --- Pinned Pipelines section ---
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

        rows.push(DashboardRow::SectionHeader {
            label: "Pull Requests".to_string(),
        });

        // --- My Pull Requests section ---
        match dashboard_prs {
            DashboardPullRequestsState::Loading => rows.push(DashboardRow::EmptyHint {
                message: "Loading pull requests...".to_string(),
            }),
            DashboardPullRequestsState::Unavailable(message) => {
                rows.push(DashboardRow::EmptyHint {
                    message: message.clone(),
                });
            }
            DashboardPullRequestsState::EmptyVerified => rows.push(DashboardRow::EmptyHint {
                message: "No pull requests found".to_string(),
            }),
            DashboardPullRequestsState::Ready(prs) => {
                for pr in prs.iter().take(10) {
                    rows.push(DashboardRow::DashboardPullRequest {
                        pull_request: Box::new(pr.clone()),
                    });
                }
            }
        }

        self.rows = rows;
        self.nav.set_len(self.rows.len());
        if self.is_separator(self.nav.index()) {
            self.skip_separator(true);
        }
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

    /// Returns true if the row at the given index is a non-selectable section header.
    pub fn is_separator(&self, index: usize) -> bool {
        matches!(
            self.rows.get(index),
            Some(DashboardRow::SectionHeader { .. })
        )
    }

    /// Nudges the cursor off a separator row by stepping one more in the given direction.
    /// Falls back to the opposite direction if the requested step would leave nothing selectable.
    pub fn skip_separator(&mut self, forward: bool) {
        if !self.is_separator(self.nav.index()) {
            return;
        }
        if forward {
            self.nav.down();
            if self.is_separator(self.nav.index()) {
                self.nav.up();
            }
        } else {
            self.nav.up();
            if self.is_separator(self.nav.index()) {
                self.nav.down();
            }
        }
    }

    /// Renders the dashboard using data from the App.
    pub fn draw_with_app(&self, f: &mut Frame, app: &App, area: Rect) {
        let content_area = draw_view_frame(f, area, " Dashboard ", None);

        // Two schemas — dashboard has heterogeneous row kinds.
        let pinned_schema = build_row(BuildRowOpts {
            select: false,
            name: true,
            retained: false,
        });
        let pinned_widths: Vec<usize> = resolve_widths(&pinned_schema.columns, content_area.width)
            .iter()
            .map(|&w| w as usize)
            .collect();
        let pr_schema = pull_request_row(crate::render::columns::PullRequestRowOpts::default());
        let pr_widths: Vec<usize> = resolve_widths(&pr_schema.columns, content_area.width)
            .iter()
            .map(|&w| w as usize)
            .collect();

        let items: Vec<ListItem> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let sel_style = row_style(i == self.nav.index());

                match row {
                    DashboardRow::SectionHeader { label } => {
                        let total_w = content_area.width.saturating_sub(1) as usize;
                        let label_len = label.chars().count() + 2;
                        let rule_len = total_w.saturating_sub(label_len);
                        let rule = "─".repeat(rule_len);
                        ListItem::new(Line::from(vec![
                            Span::styled(format!(" {label} "), theme::TEXT),
                            Span::styled(rule, theme::MUTED),
                        ]))
                    }

                    DashboardRow::EmptyHint { message } => ListItem::new(Line::from(vec![
                        Span::raw(pad(pinned_widths[pinned_schema.icon])),
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
                        let w_icon = pinned_widths[pinned_schema.icon];
                        let w_status = pinned_widths[pinned_schema.status];
                        let w_name = pinned_widths[pinned_schema.name.unwrap()];
                        let w_build = pinned_widths[pinned_schema.build_number];
                        let w_branch = pinned_widths[pinned_schema.branch];
                        let w_req = pinned_widths[pinned_schema.requestor];
                        let w_elapsed = pinned_widths[pinned_schema.elapsed];

                        let mut spans = vec![
                            Span::styled(format!("{icon:<w_icon$}"), Style::new().fg(icon_color)),
                            Span::styled(
                                format!("{label:<w_status$}"),
                                Style::new().fg(icon_color),
                            ),
                            Span::styled(
                                format!("{:<w_name$}", truncate(&definition.name, w_name)),
                                name_style,
                            ),
                        ];

                        if let Some(b) = latest_build {
                            let branch = b.branch_display();
                            let elapsed = build_elapsed(b);
                            spans.extend([
                                Span::styled(
                                    format!(
                                        "{:<w_build$}",
                                        format!(
                                            "#{}",
                                            truncate(&b.build_number, w_build.saturating_sub(1))
                                        )
                                    ),
                                    theme::MUTED,
                                ),
                                Span::styled(
                                    format!("{:<w_branch$}", truncate(&branch, w_branch)),
                                    theme::BRANCH,
                                ),
                                Span::styled(
                                    format!("{:<w_req$}", truncate(b.requestor(), w_req)),
                                    theme::MUTED,
                                ),
                                Span::styled(format!("{elapsed:>w_elapsed$}"), theme::MUTED),
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
                        let w_icon = pr_widths[pr_schema.icon];
                        let w_title = pr_widths[pr_schema.title];
                        let w_repo = pr_widths[pr_schema.repo];
                        let w_branch = pr_widths[pr_schema.branch];
                        let w_votes = pr_widths[pr_schema.votes];

                        ListItem::new(Line::from(vec![
                            Span::styled(format!("{icon:<w_icon$}"), Style::new().fg(color)),
                            Span::styled(
                                format!("{:<w_title$}", truncate(&title_text, w_title)),
                                theme::TEXT,
                            ),
                            Span::styled(
                                format!("{:<w_repo$}", truncate(pull_request.repo_name(), w_repo)),
                                theme::MUTED,
                            ),
                            Span::styled(
                                format!(
                                    "{:<w_branch$}",
                                    format!(
                                        "→ {}",
                                        truncate(
                                            pull_request.short_target_branch(),
                                            w_branch.saturating_sub(2)
                                        )
                                    )
                                ),
                                theme::BRANCH,
                            ),
                            Span::styled(format!("{vote_summary:<w_votes$}"), theme::MUTED),
                        ]))
                        .style(sel_style)
                    }
                }
            })
            .collect();

        let list = List::new(items).scroll_padding(3);

        let mut state = ListState::default();
        state.select(Some(self.nav.index()));
        f.render_stateful_widget(list, content_area, &mut state);
    }
}

impl Component for Dashboard {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn footer_hints(&self) -> &'static str {
        "↑↓ navigate  Enter drill-in  1–4 areas  Q queue  o open  r refresh  , settings  ? help  q quit"
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
        d.rebuild(
            &defs,
            &BTreeMap::new(),
            &[1, 3],
            &DashboardPullRequestsState::EmptyVerified,
        );
        // Header + 2 pinned + Header + EmptyHint(no PRs) = 5
        assert_eq!(d.rows.len(), 5);
        assert!(matches!(&d.rows[0], DashboardRow::SectionHeader { .. }));
        assert!(
            matches!(&d.rows[1], DashboardRow::PinnedPipeline { definition, .. } if definition.id == 1)
        );
        assert!(
            matches!(&d.rows[2], DashboardRow::PinnedPipeline { definition, .. } if definition.id == 3)
        );
        assert!(matches!(&d.rows[3], DashboardRow::SectionHeader { .. }));
        assert!(matches!(&d.rows[4], DashboardRow::EmptyHint { .. }));
    }

    #[test]
    fn rebuild_empty_pins_shows_hint() {
        let defs = vec![make_definition(1, "CI", "\\")];
        let mut d = Dashboard::default();
        d.rebuild(
            &defs,
            &BTreeMap::new(),
            &[],
            &DashboardPullRequestsState::EmptyVerified,
        );
        // Header + EmptyHint(no pins) + Header + EmptyHint(no PRs) = 4
        assert_eq!(d.rows.len(), 4);
        assert!(matches!(&d.rows[0], DashboardRow::SectionHeader { .. }));
        assert!(matches!(&d.rows[1], DashboardRow::EmptyHint { .. }));
        assert!(matches!(&d.rows[2], DashboardRow::SectionHeader { .. }));
        assert!(matches!(&d.rows[3], DashboardRow::EmptyHint { .. }));
    }

    #[test]
    fn rebuild_with_prs() {
        let defs = vec![];
        let prs = vec![
            make_pull_request(1, "PR One", "active", "repo"),
            make_pull_request(2, "PR Two", "active", "repo"),
        ];
        let mut d = Dashboard::default();
        d.rebuild(
            &defs,
            &BTreeMap::new(),
            &[],
            &DashboardPullRequestsState::Ready(prs),
        );
        // Header + EmptyHint(no pins) + Header + 2 PRs = 5
        assert_eq!(d.rows.len(), 5);
        assert!(matches!(&d.rows[0], DashboardRow::SectionHeader { .. }));
        assert!(matches!(&d.rows[1], DashboardRow::EmptyHint { .. }));
        assert!(matches!(&d.rows[2], DashboardRow::SectionHeader { .. }));
        assert!(matches!(
            &d.rows[3],
            DashboardRow::DashboardPullRequest { .. }
        ));
    }

    #[test]
    fn pinned_definition_at() {
        let defs = vec![make_definition(1, "CI", "\\")];
        let mut d = Dashboard::default();
        d.rebuild(
            &defs,
            &BTreeMap::new(),
            &[1],
            &DashboardPullRequestsState::EmptyVerified,
        );
        assert_eq!(d.pinned_definition_at(1).unwrap().id, 1);
        assert!(d.pinned_definition_at(0).is_none()); // SectionHeader
        assert!(d.pinned_definition_at(2).is_none()); // SectionHeader
    }

    #[test]
    fn pull_request_at() {
        let prs = vec![make_pull_request(42, "Test", "active", "repo")];
        let mut d = Dashboard::default();
        d.rebuild(
            &[],
            &BTreeMap::new(),
            &[],
            &DashboardPullRequestsState::Ready(prs),
        );
        // Row 0 = Header, 1 = EmptyHint(no pins), 2 = Header, 3 = PR.
        assert!(d.pull_request_at(0).is_none());
        assert!(d.pull_request_at(1).is_none());
        assert!(d.pull_request_at(2).is_none());
        assert_eq!(d.pull_request_at(3).unwrap().pull_request_id, 42);
    }

    #[test]
    fn prs_limited_to_10() {
        let prs: Vec<PullRequest> = (0..15)
            .map(|i| make_pull_request(i, &format!("PR {i}"), "active", "repo"))
            .collect();
        let mut d = Dashboard::default();
        d.rebuild(
            &[],
            &BTreeMap::new(),
            &[],
            &DashboardPullRequestsState::Ready(prs),
        );
        let pr_count = d
            .rows
            .iter()
            .filter(|r| matches!(r, DashboardRow::DashboardPullRequest { .. }))
            .count();
        assert_eq!(pr_count, 10);
    }

    #[test]
    fn rebuild_loading_prs_shows_loading_hint() {
        let mut d = Dashboard::default();
        d.rebuild(
            &[],
            &BTreeMap::new(),
            &[],
            &DashboardPullRequestsState::Loading,
        );
        assert!(matches!(
            &d.rows[3],
            DashboardRow::EmptyHint { message } if message == "Loading pull requests..."
        ));
    }

    #[test]
    fn rebuild_unavailable_prs_shows_unavailable_hint() {
        let mut d = Dashboard::default();
        d.rebuild(
            &[],
            &BTreeMap::new(),
            &[],
            &DashboardPullRequestsState::Unavailable("Unavailable".to_string()),
        );
        assert!(matches!(
            &d.rows[3],
            DashboardRow::EmptyHint { message } if message == "Unavailable"
        ));
    }
}
