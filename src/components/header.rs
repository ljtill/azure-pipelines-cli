//! Header bar component displaying app title and status indicators.

use anyhow::Result;
use chrono::Utc;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};

use super::Component;
use crate::render::helpers::truncate;
use crate::render::theme;
use crate::state::{App, View};

/// Defines a single tab in the header bar.
pub struct TabDef {
    pub label: &'static str,
    pub key: char,
    pub view: View,
}

/// Ordered tab definitions for the header bar.
pub const TABS: &[TabDef] = &[
    TabDef {
        label: "Dashboard",
        key: '1',
        view: View::Dashboard,
    },
    TabDef {
        label: "Pipelines",
        key: '2',
        view: View::Pipelines,
    },
    TabDef {
        label: "Active Runs",
        key: '3',
        view: View::ActiveRuns,
    },
    TabDef {
        label: "Pull Requests",
        key: '4',
        view: View::PullRequests,
    },
];

/// Returns the tab index for the given view, resolving drill-in views to their parent tab.
fn tab_index_for_view(app: &App) -> usize {
    match app.view {
        View::Dashboard => 0,
        View::Pipelines => 1,
        View::ActiveRuns => 2,
        View::PullRequests | View::PullRequestDetail => 3,
        View::BuildHistory | View::LogViewer => match app.build_history.return_to {
            Some(View::Pipelines) => 1,
            Some(View::ActiveRuns) => 2,
            _ => 0,
        },
    }
}

/// Renders the title, refresh status, notifications, and tab bar.
/// Always visible at the top of the screen. Not interactive.
#[derive(Default)]
pub struct Header;

impl Header {
    /// Renders the header using data from the App. This is a helper that takes
    /// `&App` since the header reads cross-cutting state (notifications, refresh
    /// timestamps, approvals, view, etc.).
    pub fn draw_with_app(&self, f: &mut Frame, app: &App, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Length(1), // title + status
            Constraint::Length(2), // tabs
        ])
        .split(area);

        // Title bar with org/project and refresh status.
        let refresh_text = if app.loading {
            " ⟳ refreshing...".to_string()
        } else if let Some(last) = app.last_refresh {
            let elapsed = Utc::now().signed_duration_since(last);
            if elapsed.num_seconds() < 60 {
                format!(" ⟳ {}s ago", elapsed.num_seconds())
            } else {
                format!(" ⟳ {}m ago", elapsed.num_minutes())
            }
        } else {
            " ⟳ --".to_string()
        };

        let error_span = if let Some(notif) = app.notifications.clone_current() {
            let (prefix, color) = match notif.level {
                crate::state::notifications::NotificationLevel::Error => ("⚠", Color::Red),
                crate::state::notifications::NotificationLevel::Success => ("✓", Color::Green),
                crate::state::notifications::NotificationLevel::Info => ("ℹ", Color::Cyan),
            };
            Span::styled(
                format!("  {} {}", prefix, truncate(&notif.message, 60)),
                Style::new().fg(color),
            )
        } else {
            Span::raw("")
        };

        let approvals_span = if app.data.pending_approvals.is_empty() {
            Span::raw("")
        } else {
            Span::styled(
                format!("  ⏸ {} pending", app.data.pending_approvals.len()),
                theme::APPROVAL,
            )
        };

        let title = Paragraph::new(Line::from(vec![
            Span::styled(" Azure DevOps", theme::BRAND),
            Span::styled("  ●  ", theme::MUTED),
            Span::styled(&app.org_project_label, theme::TEXT),
            Span::styled(refresh_text, theme::MUTED),
            approvals_span,
            error_span,
        ]));
        f.render_widget(title, chunks[0]);

        // Tab bar — rendered dynamically from TABS definitions.
        let tab_titles: Vec<String> = TABS
            .iter()
            .map(|t| format!("[{}] {}", t.key, t.label))
            .collect();
        let selected = tab_index_for_view(app);

        let tabs = Tabs::new(tab_titles)
            .block(Block::new().borders(Borders::BOTTOM))
            .select(selected)
            .style(theme::MUTED)
            .highlight_style(theme::BRAND);
        f.render_widget(tabs, chunks[1]);
    }
}

impl Component for Header {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        // Header rendering requires App context — use draw_with_app() instead.
        // This is a no-op to satisfy the trait; the App coordinator calls
        // draw_with_app() directly.
        Ok(())
    }
}
