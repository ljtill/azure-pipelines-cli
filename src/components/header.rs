//! Header bar component displaying app title and area-aware navigation.

use anyhow::Result;
use chrono::Utc;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};

use super::Component;
use crate::render::helpers::truncate;
use crate::render::theme;
use crate::state::{App, PaginationStatus, Service, View};

fn breadcrumb(app: &App) -> Line<'_> {
    let segments: Vec<&str> = match app.view {
        View::Dashboard => vec!["Dashboard"],
        View::Pipelines => vec!["Pipelines", "Definitions"],
        View::ActiveRuns => vec!["Pipelines", "Active Runs"],
        View::BuildHistory => vec!["Pipelines", "Build History"],
        View::LogViewer => vec!["Pipelines", "Log Viewer"],
        View::PullRequestsCreatedByMe => vec!["Repos", "Created by me"],
        View::PullRequestsAssignedToMe => vec!["Repos", "Assigned to me"],
        View::PullRequestsAllActive => vec!["Repos", "All active"],
        View::PullRequestDetail => vec!["Repos", "Detail"],
        View::Boards => vec!["Boards", "Backlog"],
        View::BoardsAssignedToMe => vec!["Boards", "Assigned to me"],
        View::BoardsCreatedByMe => vec!["Boards", "Created by me"],
        View::WorkItemDetail => vec!["Boards", "Detail"],
    };
    let mut spans = Vec::new();
    for (i, seg) in segments.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" › ", theme::MUTED));
        }
        let style = if i == segments.len() - 1 {
            theme::TEXT
        } else {
            theme::MUTED
        };
        spans.push(Span::styled(*seg, style));
    }
    Line::from(spans)
}

/// Maps a pagination endpoint tag to a human-friendly noun for the header.
fn pagination_noun(endpoint: &str) -> &'static str {
    match endpoint {
        "definitions" => "definitions",
        "retention_leases" => "leases",
        "builds" => "builds",
        _ => "items",
    }
}

/// Formats a pagination progress event for display in the header.
/// Returns a short bracketed string such as
/// `[loading page 3 — 142 definitions]`.
pub(crate) fn format_pagination_status(status: &PaginationStatus) -> String {
    format!(
        "[loading page {} — {} {}]",
        status.page,
        status.items,
        pagination_noun(status.endpoint),
    )
}

/// Renders the title, refresh status, notifications, and top-level shell.
/// Always visible at the top of the screen. Not interactive.
#[derive(Default)]
pub struct Header;

impl Header {
    /// Renders the header using data from the App.
    pub fn draw_with_app(&self, f: &mut Frame, app: &App, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Length(1), // title + status
            Constraint::Length(2), // service tabs with bottom border
        ])
        .split(area);

        let refresh_span = if app.loading {
            Span::styled("⟳ refreshing", theme::TITLE)
        } else if let Some(last) = app.last_refresh {
            let elapsed = Utc::now().signed_duration_since(last);
            if elapsed.num_seconds() < 60 {
                Span::styled(format!("⟳ {}s ago", elapsed.num_seconds()), theme::SUBTLE)
            } else {
                Span::styled(format!("⟳ {}m ago", elapsed.num_minutes()), theme::SUBTLE)
            }
        } else {
            Span::styled("⟳ --", theme::SUBTLE)
        };

        let error_span = if let Some(notif) = app.notifications.clone_current() {
            let (prefix, style) = match notif.level {
                crate::state::notifications::NotificationLevel::Error => ("⚠", theme::ERROR),
                crate::state::notifications::NotificationLevel::Success => ("✓", theme::SUCCESS),
                crate::state::notifications::NotificationLevel::Info => ("ℹ", theme::TITLE),
            };
            Span::styled(
                format!("  {} {}", prefix, truncate(&notif.message, 48)),
                style,
            )
        } else {
            Span::raw("")
        };

        let approvals_span = if app.data.pending_approvals.is_empty() {
            Span::raw("")
        } else {
            Span::styled(
                format!("  │  ⏸ {} pending", app.data.pending_approvals.len()),
                theme::APPROVAL,
            )
        };

        // Transient pagination progress, shown while a paginated fetch is in
        // flight. Cleared by the dispatcher when the terminal result message
        // lands. Rendered in the muted style so it blends with the refresh
        // indicator and doesn't compete with error/success notifications.
        let pagination_span = app.pagination_status.as_ref().map_or_else(
            || Span::raw(""),
            |status| {
                Span::styled(
                    format!("  │  {}", format_pagination_status(status)),
                    theme::MUTED,
                )
            },
        );

        let bc = breadcrumb(app);
        let mut title_spans = vec![
            Span::styled(" devops", theme::BRAND),
            Span::styled("  ", theme::MUTED),
            Span::styled(app.org_project_label.clone(), theme::SUBTLE),
            Span::styled("  │  ", theme::MUTED),
            refresh_span,
            Span::styled("  │  ", theme::MUTED),
        ];
        title_spans.extend(bc.spans);
        title_spans.push(approvals_span);
        title_spans.push(pagination_span);
        title_spans.push(error_span);

        let title = Paragraph::new(Line::from(title_spans));
        f.render_widget(title, chunks[0]);

        let service_titles: Vec<String> = Service::ALL
            .iter()
            .map(|service| format!("[{}] {}", service.key(), service.label()))
            .collect();
        let selected_service = Service::ALL
            .iter()
            .position(|service| *service == app.service)
            .unwrap_or(0);
        let service_tabs = Tabs::new(service_titles)
            .block(Block::new().borders(Borders::BOTTOM))
            .select(selected_service)
            .style(theme::MUTED)
            .highlight_style(theme::MODE_ACTIVE);
        f.render_widget(service_tabs, chunks[1]);
    }
}

impl Component for Header {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_pagination_status_renders_known_endpoints() {
        let status = PaginationStatus {
            endpoint: "definitions",
            page: 3,
            items: 142,
        };
        assert_eq!(
            format_pagination_status(&status),
            "[loading page 3 — 142 definitions]"
        );

        let status = PaginationStatus {
            endpoint: "retention_leases",
            page: 2,
            items: 18,
        };
        assert_eq!(
            format_pagination_status(&status),
            "[loading page 2 — 18 leases]"
        );
    }

    #[test]
    fn format_pagination_status_falls_back_for_unknown_endpoint() {
        let status = PaginationStatus {
            endpoint: "mystery",
            page: 1,
            items: 7,
        };
        assert_eq!(
            format_pagination_status(&status),
            "[loading page 1 — 7 items]"
        );
    }
}
