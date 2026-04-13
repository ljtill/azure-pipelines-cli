//! Header bar component displaying app title and area-aware navigation.

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
use crate::state::{App, Service, View};

fn breadcrumb(app: &App) -> String {
    match app.view {
        View::Dashboard => "Dashboard / Overview".to_string(),
        View::Pipelines => "Pipelines / Definitions".to_string(),
        View::ActiveRuns => "Pipelines / Active Runs".to_string(),
        View::BuildHistory => format!(
            "Pipelines / {} / Build History",
            app.active_root_view().root_label()
        ),
        View::LogViewer => format!(
            "Pipelines / {} / Log Viewer",
            app.active_root_view().root_label()
        ),
        View::PullRequests => "Repos / Pull Requests".to_string(),
        View::PullRequestDetail => "Repos / Pull Requests / Detail".to_string(),
        View::Boards => "Boards / Coming soon".to_string(),
    }
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
            Constraint::Length(1), // primary area tabs
            Constraint::Length(2), // area-local tabs
        ])
        .split(area);

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
                format!("  {} {}", prefix, truncate(&notif.message, 48)),
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
            Span::styled(format!("  │  {}", breadcrumb(app)), theme::MUTED),
            approvals_span,
            error_span,
        ]));
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
            .select(selected_service)
            .style(theme::MUTED)
            .highlight_style(theme::BRAND);
        f.render_widget(service_tabs, chunks[1]);

        let local_titles: Vec<String> = app
            .service
            .root_views()
            .iter()
            .map(|view| view.root_label().to_string())
            .collect();
        let selected_local = app
            .service
            .root_views()
            .iter()
            .position(|view| *view == app.active_root_view())
            .unwrap_or(0);
        let local_tabs = Tabs::new(local_titles)
            .block(Block::new().borders(Borders::BOTTOM))
            .select(selected_local)
            .style(theme::MUTED)
            .highlight_style(theme::TEXT);
        f.render_widget(local_tabs, chunks[2]);
    }
}

impl Component for Header {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }
}
