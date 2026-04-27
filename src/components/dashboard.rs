//! Global dashboard view component showing pinned pipelines and personal pull requests.

use std::collections::BTreeMap;

use std::ops::Range;

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

use super::Component;
use crate::client::models::{Build, PipelineDefinition, PullRequest, WorkItem};
use crate::render::columns::{BuildRowOpts, build_row, pull_request_row, work_item_row};
use crate::render::helpers::{
    build_elapsed, draw_view_frame, effective_status_icon, effective_status_label, pr_status_icon,
    row_style,
};
use crate::render::table::{
    Align, DEFAULT_SCROLL_PADDING, format_cell, resolve_widths, visible_rows,
};
use crate::render::theme;
use crate::shared::availability::{Availability, AvailabilityStatus};
use crate::state::nav::ListNav;
use crate::state::{
    App, CoreDataAvailability, DashboardPullRequestsState, DashboardWorkItemsState,
    PinnedWorkItemsState,
};

/// Represents a selectable row in the dashboard view.
#[derive(Debug, Clone)]
pub enum DashboardRow {
    PinnedPipeline {
        definition_id: u32,
    },
    DashboardPullRequest {
        pull_request_id: u32,
    },
    DashboardWorkItem {
        source: DashboardWorkItemSource,
        work_item_id: u32,
    },
    EmptyHint {
        message: String,
    },
    DegradedHint {
        message: String,
        status: AvailabilityStatus,
    },
}

/// Identifies which dashboard work-item section owns a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardWorkItemSource {
    Pinned,
    Active,
}

/// Number of sections rendered as separate panels on the dashboard.
const SECTION_COUNT: usize = 4;

/// Stable section identifiers in display order.
const SECTION_LABELS: [&str; SECTION_COUNT] = [
    "Pipelines (Pinned)",
    "Work Items (Pinned)",
    "Pull Requests (Active)",
    "Work Items (Active)",
];

/// Returns a string of `n` spaces for column padding.
fn pad(n: usize) -> String {
    " ".repeat(n)
}

fn dashboard_work_item_at<'a>(
    source: DashboardWorkItemSource,
    work_item_id: u32,
    dashboard_wis: &'a DashboardWorkItemsState,
    pinned_wis: &'a PinnedWorkItemsState,
) -> Option<&'a WorkItem> {
    match source {
        DashboardWorkItemSource::Pinned => pinned_wis
            .work_items()?
            .iter()
            .find(|wi| wi.id == work_item_id),
        DashboardWorkItemSource::Active => dashboard_wis
            .work_items()?
            .iter()
            .find(|wi| wi.id == work_item_id),
    }
}

fn availability_rank(status: AvailabilityStatus) -> u8 {
    match status {
        AvailabilityStatus::Fresh => 0,
        AvailabilityStatus::Partial => 1,
        AvailabilityStatus::Stale => 2,
        AvailabilityStatus::Unavailable => 3,
    }
}

fn worst_status(left: AvailabilityStatus, right: AvailabilityStatus) -> AvailabilityStatus {
    if availability_rank(left) >= availability_rank(right) {
        left
    } else {
        right
    }
}

fn status_label(status: AvailabilityStatus) -> &'static str {
    match status {
        AvailabilityStatus::Fresh => "fresh",
        AvailabilityStatus::Partial => "partial",
        AvailabilityStatus::Stale => "stale",
        AvailabilityStatus::Unavailable => "unavailable",
    }
}

fn degraded_message<T>(label: &str, availability: &Availability<Vec<T>>) -> Option<String> {
    if availability.status() == AvailabilityStatus::Fresh {
        return None;
    }
    let detail = if availability.errors().is_empty() {
        availability.primary_error()?.to_string()
    } else {
        availability.errors().join("; ")
    };
    Some(format!("{label}: {detail}"))
}

fn collect_core_degradation<T>(
    status: &mut AvailabilityStatus,
    messages: &mut Vec<String>,
    label: &str,
    availability: &Availability<Vec<T>>,
) {
    *status = worst_status(*status, availability.status());
    if let Some(message) = degraded_message(label, availability) {
        messages.push(message);
    }
}

fn pinned_pipeline_degradation(
    availability: Option<&CoreDataAvailability>,
) -> Option<(AvailabilityStatus, String)> {
    let availability = availability?;
    let mut status = AvailabilityStatus::Fresh;
    let mut messages = Vec::new();
    collect_core_degradation(
        &mut status,
        &mut messages,
        "definitions",
        &availability.definitions,
    );
    collect_core_degradation(
        &mut status,
        &mut messages,
        "builds",
        &availability.recent_builds,
    );
    collect_core_degradation(
        &mut status,
        &mut messages,
        "approvals",
        &availability.pending_approvals,
    );

    if status == AvailabilityStatus::Fresh {
        None
    } else {
        Some((status, messages.join("; ")))
    }
}

fn dashboard_pull_requests_degraded_status(
    state: &DashboardPullRequestsState,
) -> Option<AvailabilityStatus> {
    match state {
        DashboardPullRequestsState::Partial { .. } => Some(AvailabilityStatus::Partial),
        DashboardPullRequestsState::Stale { .. } => Some(AvailabilityStatus::Stale),
        DashboardPullRequestsState::Unavailable(_) => Some(AvailabilityStatus::Unavailable),
        DashboardPullRequestsState::Loading
        | DashboardPullRequestsState::Ready(_)
        | DashboardPullRequestsState::EmptyVerified => None,
    }
}

fn dashboard_work_items_degraded_status(
    state: &DashboardWorkItemsState,
) -> Option<AvailabilityStatus> {
    match state {
        DashboardWorkItemsState::Partial { .. } => Some(AvailabilityStatus::Partial),
        DashboardWorkItemsState::Stale { .. } => Some(AvailabilityStatus::Stale),
        DashboardWorkItemsState::Unavailable(_) => Some(AvailabilityStatus::Unavailable),
        DashboardWorkItemsState::Loading
        | DashboardWorkItemsState::Ready(_)
        | DashboardWorkItemsState::EmptyVerified => None,
    }
}

fn pinned_work_items_degraded_status(state: &PinnedWorkItemsState) -> Option<AvailabilityStatus> {
    match state {
        PinnedWorkItemsState::Partial { .. } => Some(AvailabilityStatus::Partial),
        PinnedWorkItemsState::Stale { .. } => Some(AvailabilityStatus::Stale),
        PinnedWorkItemsState::Unavailable(_) => Some(AvailabilityStatus::Unavailable),
        PinnedWorkItemsState::Loading | PinnedWorkItemsState::Ready(_) => None,
    }
}

fn section_degraded_status(section_idx: usize, app: &App) -> Option<AvailabilityStatus> {
    match section_idx {
        0 => pinned_pipeline_degradation(Some(&app.core.availability)).map(|(status, _)| status),
        1 => pinned_work_items_degraded_status(&app.pinned_work_items),
        2 => dashboard_pull_requests_degraded_status(&app.dashboard_pull_requests),
        3 => dashboard_work_items_degraded_status(&app.dashboard_work_items),
        _ => None,
    }
}

fn push_degraded_hint(
    rows: &mut Vec<DashboardRow>,
    status: AvailabilityStatus,
    section: &str,
    detail: &str,
) {
    let prefix = match status {
        AvailabilityStatus::Fresh => return,
        AvailabilityStatus::Partial => "Partial",
        AvailabilityStatus::Stale => "Showing stale",
        AvailabilityStatus::Unavailable => "Unavailable",
    };
    rows.push(DashboardRow::DegradedHint {
        message: format!("{prefix} {section}: {detail}"),
        status,
    });
}

/// Renders the cross-service dashboard with pinned pipelines and pull requests.
#[derive(Debug, Default)]
pub struct Dashboard {
    /// Flat list of selectable rows across all sections, in display order.
    pub rows: Vec<DashboardRow>,
    /// Index range into `rows` for each of the four sections (in `SECTION_LABELS` order).
    pub section_ranges: [Range<usize>; SECTION_COUNT],
    pub nav: ListNav,
}

impl Dashboard {
    /// Rebuilds the dashboard from pinned pipeline IDs, definitions, latest builds, PRs, and work items.
    pub fn rebuild(
        &mut self,
        definitions: &[PipelineDefinition],
        _latest_builds_by_def: &BTreeMap<u32, Build>,
        pinned_ids: &[u32],
        dashboard_prs: &DashboardPullRequestsState,
        dashboard_wis: &DashboardWorkItemsState,
        pinned_wis: &PinnedWorkItemsState,
    ) {
        self.rebuild_inner(
            definitions,
            pinned_ids,
            dashboard_prs,
            dashboard_wis,
            pinned_wis,
            None,
        );
    }

    /// Rebuilds the dashboard while surfacing core data availability in sections.
    pub fn rebuild_with_availability(
        &mut self,
        definitions: &[PipelineDefinition],
        pinned_ids: &[u32],
        dashboard_prs: &DashboardPullRequestsState,
        dashboard_wis: &DashboardWorkItemsState,
        pinned_wis: &PinnedWorkItemsState,
        core_availability: &CoreDataAvailability,
    ) {
        self.rebuild_inner(
            definitions,
            pinned_ids,
            dashboard_prs,
            dashboard_wis,
            pinned_wis,
            Some(core_availability),
        );
    }

    fn rebuild_inner(
        &mut self,
        definitions: &[PipelineDefinition],
        pinned_ids: &[u32],
        dashboard_prs: &DashboardPullRequestsState,
        dashboard_wis: &DashboardWorkItemsState,
        pinned_wis: &PinnedWorkItemsState,
        core_availability: Option<&CoreDataAvailability>,
    ) {
        let mut rows: Vec<DashboardRow> = Vec::new();
        let mut ranges: [Range<usize>; SECTION_COUNT] = std::array::from_fn(|_| 0..0);

        // --- Section 0: Pinned Pipelines ---
        let start = rows.len();
        let pipeline_degradation = pinned_pipeline_degradation(core_availability);
        if let Some((status, message)) = &pipeline_degradation {
            push_degraded_hint(&mut rows, *status, "pipeline data", message);
        }
        let definitions_by_id: BTreeMap<u32, &PipelineDefinition> = definitions
            .iter()
            .map(|definition| (definition.id, definition))
            .collect();
        let mut pinned: Vec<u32> = pinned_ids
            .iter()
            .copied()
            .filter(|id| definitions_by_id.contains_key(id))
            .collect();
        pinned.sort_by_cached_key(|id| {
            definitions_by_id
                .get(id)
                .map(|definition| definition.name.to_lowercase())
                .unwrap_or_default()
        });

        if pinned.is_empty() && !(pipeline_degradation.is_some() && definitions.is_empty()) {
            rows.push(DashboardRow::EmptyHint {
                message: "No pipelines pinned — press 'p' in the Pipelines view to pin".to_string(),
            });
        } else {
            for definition_id in pinned {
                rows.push(DashboardRow::PinnedPipeline { definition_id });
            }
        }
        ranges[0] = start..rows.len();

        // --- Section 1: Pinned Work Items ---
        let start = rows.len();
        if let Some(message) = pinned_wis.degraded_message() {
            push_degraded_hint(&mut rows, pinned_wis.status(), "pinned work items", message);
        }
        match pinned_wis.work_items() {
            Some(wis) if !wis.is_empty() => {
                for work_item in wis {
                    rows.push(DashboardRow::DashboardWorkItem {
                        source: DashboardWorkItemSource::Pinned,
                        work_item_id: work_item.id,
                    });
                }
            }
            Some(_) if pinned_wis.status() == AvailabilityStatus::Fresh => {
                rows.push(DashboardRow::EmptyHint {
                    message: "No work items pinned — press 'P' in a Boards view to pin".to_string(),
                });
            }
            None if matches!(pinned_wis, PinnedWorkItemsState::Loading) => {
                rows.push(DashboardRow::EmptyHint {
                    message: "Loading pinned work items...".to_string(),
                });
            }
            Some(_) | None => {}
        }
        ranges[1] = start..rows.len();

        // --- Section 2: Pull Requests ---
        let start = rows.len();
        if let Some(message) = dashboard_prs.degraded_message() {
            push_degraded_hint(&mut rows, dashboard_prs.status(), "pull requests", message);
        }
        match dashboard_prs.pull_requests() {
            Some(prs) if !prs.is_empty() => {
                for pull_request in prs.iter().take(10) {
                    rows.push(DashboardRow::DashboardPullRequest {
                        pull_request_id: pull_request.pull_request_id,
                    });
                }
            }
            Some(_) if dashboard_prs.status() == AvailabilityStatus::Fresh => {
                rows.push(DashboardRow::EmptyHint {
                    message: "No pull requests found".to_string(),
                });
            }
            None if matches!(dashboard_prs, DashboardPullRequestsState::Loading) => {
                rows.push(DashboardRow::EmptyHint {
                    message: "Loading pull requests...".to_string(),
                });
            }
            Some(_) | None => {}
        }
        ranges[2] = start..rows.len();

        // --- Section 3: Work Items ---
        let start = rows.len();
        if let Some(message) = dashboard_wis.degraded_message() {
            push_degraded_hint(&mut rows, dashboard_wis.status(), "work items", message);
        }
        match dashboard_wis.work_items() {
            Some(wis) if !wis.is_empty() => {
                for work_item in wis.iter().take(10) {
                    rows.push(DashboardRow::DashboardWorkItem {
                        source: DashboardWorkItemSource::Active,
                        work_item_id: work_item.id,
                    });
                }
            }
            Some(_) if dashboard_wis.status() == AvailabilityStatus::Fresh => {
                rows.push(DashboardRow::EmptyHint {
                    message: "No work items found".to_string(),
                });
            }
            None if matches!(dashboard_wis, DashboardWorkItemsState::Loading) => {
                rows.push(DashboardRow::EmptyHint {
                    message: "Loading work items...".to_string(),
                });
            }
            Some(_) | None => {}
        }
        ranges[3] = start..rows.len();

        self.rows = rows;
        self.section_ranges = ranges;
        self.nav.set_len(self.rows.len());
    }

    /// Returns the pipeline definition at the given row index, if it is a pinned pipeline.
    pub fn pinned_definition_at<'a>(
        &self,
        index: usize,
        definitions: &'a [PipelineDefinition],
    ) -> Option<&'a PipelineDefinition> {
        match self.rows.get(index) {
            Some(DashboardRow::PinnedPipeline { definition_id }) => definitions
                .iter()
                .find(|definition| definition.id == *definition_id),
            _ => None,
        }
    }

    /// Returns the pull request at the given row index, if it is a dashboard PR.
    pub fn pull_request_at<'a>(
        &self,
        index: usize,
        dashboard_prs: &'a DashboardPullRequestsState,
    ) -> Option<&'a PullRequest> {
        match self.rows.get(index) {
            Some(DashboardRow::DashboardPullRequest { pull_request_id }) => dashboard_prs
                .pull_requests()?
                .iter()
                .find(|pull_request| pull_request.pull_request_id == *pull_request_id),
            _ => None,
        }
    }

    /// Returns the work item at the given row index, if it is a dashboard work item.
    pub fn work_item_at<'a>(
        &self,
        index: usize,
        dashboard_wis: &'a DashboardWorkItemsState,
        pinned_wis: &'a PinnedWorkItemsState,
    ) -> Option<&'a WorkItem> {
        match self.rows.get(index) {
            Some(DashboardRow::DashboardWorkItem {
                source,
                work_item_id,
            }) => dashboard_work_item_at(*source, *work_item_id, dashboard_wis, pinned_wis),
            _ => None,
        }
    }

    /// Returns the index of the section containing `flat_index`, if any.
    fn section_for(&self, flat_index: usize) -> Option<usize> {
        self.section_ranges
            .iter()
            .position(|r| r.contains(&flat_index))
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
        let wi_schema = work_item_row();
        let wi_widths: Vec<usize> = resolve_widths(&wi_schema.columns, content_area.width)
            .iter()
            .map(|&w| w as usize)
            .collect();

        // Compute proportional weights from each section's row count (clamped),
        // and place a 1-row gap between adjacent panels for visual breathing room.
        let weights: [u16; SECTION_COUNT] = std::array::from_fn(|i| {
            let n = self.section_ranges[i].len() as u16;
            n.clamp(3, 12)
        });
        let constraints = [
            Constraint::Fill(weights[0]),
            Constraint::Length(1),
            Constraint::Fill(weights[1]),
            Constraint::Length(1),
            Constraint::Fill(weights[2]),
            Constraint::Length(1),
            Constraint::Fill(weights[3]),
        ];
        let chunks = Layout::vertical(constraints).split(content_area);
        let panel_chunks = [chunks[0], chunks[2], chunks[4], chunks[6]];

        let selected_section = self.section_for(self.nav.index());

        for (section_idx, panel_rect) in panel_chunks.iter().enumerate() {
            self.draw_panel(
                f,
                *panel_rect,
                section_idx,
                selected_section == Some(section_idx),
                app,
                &pinned_schema,
                &pinned_widths,
                &pr_schema,
                &pr_widths,
                &wi_schema,
                &wi_widths,
            );
        }
    }

    /// Renders a single dashboard panel: a header line followed by a list of its rows.
    #[allow(clippy::too_many_arguments)]
    fn draw_panel(
        &self,
        f: &mut Frame,
        area: Rect,
        section_idx: usize,
        is_active_section: bool,
        app: &App,
        pinned_schema: &crate::render::columns::BuildRowSchema,
        pinned_widths: &[usize],
        pr_schema: &crate::render::columns::PullRequestSchema,
        pr_widths: &[usize],
        wi_schema: &crate::render::columns::WorkItemSchema,
        wi_widths: &[usize],
    ) {
        if area.height == 0 {
            return;
        }

        // --- Header line ---
        let status = section_degraded_status(section_idx, app);
        let label = status.map_or_else(
            || SECTION_LABELS[section_idx].to_string(),
            |status| format!("{} [{}]", SECTION_LABELS[section_idx], status_label(status)),
        );
        let total_w = area.width.saturating_sub(1) as usize;
        let label_len = label.chars().count() + 2;
        let rule_len = total_w.saturating_sub(label_len);
        let rule = "─".repeat(rule_len);
        let label_style = if status.is_some() {
            theme::WARNING
        } else if is_active_section {
            theme::SECTION_HEADER
        } else {
            theme::SUBTLE
        };
        let header_line = Line::from(vec![
            Span::styled(format!(" {label} "), label_style),
            Span::styled(rule, theme::MUTED),
        ]);
        let header_rect = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        };
        f.render_widget(Paragraph::new(header_line), header_rect);

        if area.height < 2 {
            return;
        }
        let body_rect = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: area.height - 1,
        };

        // --- Body list ---
        let range = self.section_ranges[section_idx].clone();
        let local_selected = if is_active_section {
            Some(self.nav.index() - range.start)
        } else {
            None
        };

        let window = visible_rows(
            range.len(),
            local_selected.unwrap_or(0),
            body_rect.height,
            DEFAULT_SCROLL_PADDING,
        );
        let selected = local_selected.and_then(|index| window.local_index(index));
        let items: Vec<ListItem> = window
            .range()
            .map(|local_idx| {
                let row = &self.rows[range.start + local_idx];
                let is_selected = selected == Some(local_idx - window.start);
                let sel_style = row_style(is_selected);
                Self::render_row(
                    row,
                    sel_style,
                    app,
                    pinned_schema,
                    pinned_widths,
                    pr_schema,
                    pr_widths,
                    wi_schema,
                    wi_widths,
                )
            })
            .collect();

        let list = List::new(items).scroll_padding(DEFAULT_SCROLL_PADDING);
        let mut state = ListState::default();
        state.select(selected);
        f.render_stateful_widget(list, body_rect, &mut state);
    }

    /// Builds a single `ListItem` for the given row, sharing styling helpers across panels.
    #[allow(clippy::too_many_arguments)]
    fn render_row(
        row: &DashboardRow,
        sel_style: Style,
        app: &App,
        pinned_schema: &crate::render::columns::BuildRowSchema,
        pinned_widths: &[usize],
        pr_schema: &crate::render::columns::PullRequestSchema,
        pr_widths: &[usize],
        wi_schema: &crate::render::columns::WorkItemSchema,
        wi_widths: &[usize],
    ) -> ListItem<'static> {
        match row {
            DashboardRow::EmptyHint { message } => ListItem::new(Line::from(vec![
                Span::raw(pad(pinned_widths[pinned_schema.icon])),
                Span::styled(message.clone(), theme::MUTED),
            ]))
            .style(sel_style),

            DashboardRow::DegradedHint { message, status } => {
                let style = match status {
                    AvailabilityStatus::Fresh => theme::MUTED,
                    AvailabilityStatus::Partial | AvailabilityStatus::Stale => theme::WARNING,
                    AvailabilityStatus::Unavailable => theme::ERROR,
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format_cell("⚠", pinned_widths[pinned_schema.icon], Align::Left),
                        style,
                    ),
                    Span::styled(message.clone(), style),
                ]))
                .style(sel_style)
            }

            DashboardRow::PinnedPipeline { definition_id } => {
                let definition = app
                    .core
                    .data
                    .definitions
                    .iter()
                    .find(|definition| definition.id == *definition_id);
                let latest_build = app.core.data.latest_builds_by_def.get(definition_id);
                let (icon, icon_color) = latest_build.map_or(("○", theme::PENDING_FG), |b| {
                    let awaiting = app.core.data.pending_approval_build_ids.contains(&b.id);
                    effective_status_icon(b.status, b.result, awaiting)
                });
                let label = latest_build.map_or("", |b| {
                    let awaiting = app.core.data.pending_approval_build_ids.contains(&b.id);
                    effective_status_label(b.status, b.result, awaiting)
                });
                let name_style = if latest_build.is_some() && definition.is_some() {
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
                    Span::styled(
                        format_cell(icon, w_icon, Align::Left),
                        theme::foreground(icon_color),
                    ),
                    Span::styled(
                        format_cell(label, w_status, Align::Left),
                        theme::foreground(icon_color),
                    ),
                    Span::styled(
                        format_cell(
                            definition
                                .map_or("Unknown pipeline", |definition| definition.name.as_str()),
                            w_name,
                            Align::Left,
                        ),
                        name_style,
                    ),
                ];

                if let Some(b) = latest_build {
                    let branch = b.branch_display();
                    let elapsed = build_elapsed(b);
                    spans.extend([
                        Span::styled(
                            format_cell(&format!("#{}", b.build_number), w_build, Align::Left),
                            theme::MUTED,
                        ),
                        Span::styled(format_cell(&branch, w_branch, Align::Left), theme::BRANCH),
                        Span::styled(format_cell(b.requestor(), w_req, Align::Left), theme::MUTED),
                        Span::styled(format_cell(&elapsed, w_elapsed, Align::Right), theme::MUTED),
                    ]);
                } else {
                    spans.push(Span::styled("no builds", theme::MUTED));
                }

                ListItem::new(Line::from(spans)).style(sel_style)
            }

            DashboardRow::DashboardPullRequest { pull_request_id } => {
                let Some(pull_request) = app
                    .dashboard_pull_requests
                    .pull_requests()
                    .and_then(|prs| prs.iter().find(|pr| pr.pull_request_id == *pull_request_id))
                else {
                    return ListItem::new(Line::from(vec![
                        Span::raw(pad(pr_widths[pr_schema.icon])),
                        Span::styled("Pull request unavailable", theme::MUTED),
                    ]))
                    .style(sel_style);
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
                    Span::styled(
                        format_cell(icon, w_icon, Align::Left),
                        theme::foreground(color),
                    ),
                    Span::styled(format_cell(&title_text, w_title, Align::Left), theme::TEXT),
                    Span::styled(
                        format_cell(pull_request.repo_name(), w_repo, Align::Left),
                        theme::MUTED,
                    ),
                    Span::styled(
                        format_cell(
                            &format!("→ {}", pull_request.short_target_branch()),
                            w_branch,
                            Align::Left,
                        ),
                        theme::BRANCH,
                    ),
                    Span::styled(
                        format_cell(&vote_summary, w_votes, Align::Left),
                        theme::MUTED,
                    ),
                ]))
                .style(sel_style)
            }

            DashboardRow::DashboardWorkItem {
                source,
                work_item_id,
            } => {
                let Some(work_item) = dashboard_work_item_at(
                    *source,
                    *work_item_id,
                    &app.dashboard_work_items,
                    &app.pinned_work_items,
                ) else {
                    return ListItem::new(Line::from(vec![
                        Span::raw(pad(wi_widths[wi_schema.id])),
                        Span::styled("Work item unavailable", theme::MUTED),
                    ]))
                    .style(sel_style);
                };
                let w_id = wi_widths[wi_schema.id];
                let w_type = wi_widths[wi_schema.work_item_type];
                let w_title = wi_widths[wi_schema.title];
                let w_state = wi_widths[wi_schema.state];
                let w_assigned = wi_widths[wi_schema.assigned];
                let w_iter = wi_widths[wi_schema.iteration];

                ListItem::new(Line::from(vec![
                    Span::styled(
                        format_cell(&format!("#{}", work_item.id), w_id, Align::Left),
                        theme::MUTED,
                    ),
                    Span::styled(
                        format_cell(work_item.work_item_type(), w_type, Align::Left),
                        theme::work_item_type_style(work_item.work_item_type()),
                    ),
                    Span::styled(
                        format_cell(work_item.title(), w_title, Align::Left),
                        theme::TEXT,
                    ),
                    Span::styled(
                        format_cell(work_item.state_label(), w_state, Align::Left),
                        theme::work_item_state_style(work_item.state_label()),
                    ),
                    Span::styled(
                        format_cell(
                            work_item.assigned_to_display().unwrap_or("—"),
                            w_assigned,
                            Align::Left,
                        ),
                        theme::MUTED,
                    ),
                    Span::styled(
                        format_cell(
                            work_item.fields.iteration_path.as_deref().unwrap_or(""),
                            w_iter,
                            Align::Left,
                        ),
                        theme::MUTED,
                    ),
                ]))
                .style(sel_style)
            }
        }
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
            &DashboardWorkItemsState::EmptyVerified,
            &PinnedWorkItemsState::Ready(Vec::new()),
        );
        // Pinned Pipelines: 2 pinned
        // Pinned Work Items: 1 EmptyHint
        // Pull Requests: 1 EmptyHint
        // Work Items: 1 EmptyHint = 5
        assert_eq!(d.rows.len(), 5);
        assert_eq!(d.section_ranges[0], 0..2);
        assert_eq!(d.section_ranges[1], 2..3);
        assert_eq!(d.section_ranges[2], 3..4);
        assert_eq!(d.section_ranges[3], 4..5);
        assert!(
            matches!(&d.rows[0], DashboardRow::PinnedPipeline { definition_id } if *definition_id == 1)
        );
        assert!(
            matches!(&d.rows[1], DashboardRow::PinnedPipeline { definition_id } if *definition_id == 3)
        );
        assert!(matches!(&d.rows[2], DashboardRow::EmptyHint { .. }));
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
            &DashboardWorkItemsState::EmptyVerified,
            &PinnedWorkItemsState::Ready(Vec::new()),
        );
        // 1 EmptyHint per section = 4
        assert_eq!(d.rows.len(), 4);
        for row in &d.rows {
            assert!(matches!(row, DashboardRow::EmptyHint { .. }));
        }
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
            &DashboardWorkItemsState::EmptyVerified,
            &PinnedWorkItemsState::Ready(Vec::new()),
        );
        // Pinned Pipelines: 1 EmptyHint
        // Pinned Work Items: 1 EmptyHint
        // Pull Requests: 2 PRs
        // Work Items: 1 EmptyHint = 5
        assert_eq!(d.rows.len(), 5);
        assert_eq!(d.section_ranges[2], 2..4);
        assert!(matches!(
            &d.rows[2],
            DashboardRow::DashboardPullRequest { .. }
        ));
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
            &DashboardWorkItemsState::EmptyVerified,
            &PinnedWorkItemsState::Ready(Vec::new()),
        );
        assert_eq!(d.pinned_definition_at(0, &defs).unwrap().id, 1);
        assert!(d.pinned_definition_at(1, &defs).is_none());
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
            &DashboardWorkItemsState::EmptyVerified,
            &PinnedWorkItemsState::Ready(Vec::new()),
        );
        // Layout: [0]=Pinned Pipelines empty, [1]=Pinned WIs empty, [2]=PR, [3]=WIs empty.
        let dashboard_prs = DashboardPullRequestsState::Ready(vec![make_pull_request(
            42, "Test", "active", "repo",
        )]);
        assert!(d.pull_request_at(0, &dashboard_prs).is_none());
        assert!(d.pull_request_at(1, &dashboard_prs).is_none());
        assert_eq!(
            d.pull_request_at(2, &dashboard_prs)
                .unwrap()
                .pull_request_id,
            42
        );
        assert!(d.pull_request_at(3, &dashboard_prs).is_none());
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
            &DashboardWorkItemsState::EmptyVerified,
            &PinnedWorkItemsState::Ready(Vec::new()),
        );
        let pr_count = d
            .rows
            .iter()
            .filter(|r| matches!(r, DashboardRow::DashboardPullRequest { .. }))
            .count();
        assert_eq!(pr_count, 10);
        assert_eq!(d.section_ranges[2].len(), 10);
    }

    #[test]
    fn rebuild_loading_prs_shows_loading_hint() {
        let mut d = Dashboard::default();
        d.rebuild(
            &[],
            &BTreeMap::new(),
            &[],
            &DashboardPullRequestsState::Loading,
            &DashboardWorkItemsState::EmptyVerified,
            &PinnedWorkItemsState::Ready(Vec::new()),
        );
        let pr_idx = d.section_ranges[2].start;
        assert!(matches!(
            &d.rows[pr_idx],
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
            &DashboardWorkItemsState::EmptyVerified,
            &PinnedWorkItemsState::Ready(Vec::new()),
        );
        let pr_idx = d.section_ranges[2].start;
        assert!(matches!(
            &d.rows[pr_idx],
            DashboardRow::DegradedHint { message, status }
                if message == "Unavailable pull requests: Unavailable"
                    && *status == AvailabilityStatus::Unavailable
        ));
    }

    #[test]
    fn rebuild_stale_prs_keeps_rows_with_warning_hint() {
        let mut d = Dashboard::default();
        let prs = vec![make_pull_request(42, "Stale PR", "active", "repo")];

        d.rebuild(
            &[],
            &BTreeMap::new(),
            &[],
            &DashboardPullRequestsState::Stale {
                pull_requests: prs,
                message: "timeout".to_string(),
            },
            &DashboardWorkItemsState::EmptyVerified,
            &PinnedWorkItemsState::Ready(Vec::new()),
        );

        let pr_idx = d.section_ranges[2].start;
        assert!(matches!(
            &d.rows[pr_idx],
            DashboardRow::DegradedHint { message, status }
                if message == "Showing stale pull requests: timeout"
                    && *status == AvailabilityStatus::Stale
        ));
        assert!(matches!(
            &d.rows[pr_idx + 1],
            DashboardRow::DashboardPullRequest { pull_request_id } if *pull_request_id == 42
        ));
    }

    #[test]
    fn nav_flows_across_panels() {
        // 2 pinned pipelines, then empty pinned WIs, then 2 PRs, then empty WIs.
        let defs = vec![
            make_definition(1, "CI", "\\"),
            make_definition(2, "Deploy", "\\"),
        ];
        let prs = vec![
            make_pull_request(10, "PR A", "active", "repo"),
            make_pull_request(11, "PR B", "active", "repo"),
        ];
        let mut d = Dashboard::default();
        d.rebuild(
            &defs,
            &BTreeMap::new(),
            &[1, 2],
            &DashboardPullRequestsState::Ready(prs),
            &DashboardWorkItemsState::EmptyVerified,
            &PinnedWorkItemsState::Ready(Vec::new()),
        );
        // Layout: [0,1]=pipelines, [2]=pinned WIs hint, [3,4]=PRs, [5]=WIs hint.
        assert_eq!(d.rows.len(), 6);
        assert_eq!(d.section_ranges[0], 0..2);
        assert_eq!(d.section_ranges[1], 2..3);
        assert_eq!(d.section_ranges[2], 3..5);
        assert_eq!(d.section_ranges[3], 5..6);
        // Selection at end of pipelines panel...
        d.nav.set_index(1);
        d.nav.down();
        // ...lands on the pinned-WIs hint, which is the next selectable row.
        assert_eq!(d.nav.index(), 2);
        assert!(matches!(
            d.rows[d.nav.index()],
            DashboardRow::EmptyHint { .. }
        ));
        d.nav.down();
        // Then continues into the first PR.
        assert_eq!(d.nav.index(), 3);
        assert!(matches!(
            d.rows[d.nav.index()],
            DashboardRow::DashboardPullRequest { .. }
        ));
    }
}
