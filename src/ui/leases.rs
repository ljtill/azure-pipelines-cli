use chrono::Utc;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};

use super::helpers::truncate;
use super::theme;
use crate::app::App;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    use ratatui::layout::{Constraint, Layout};

    if app.retention_leases.loading && app.retention_leases.leases.is_empty() {
        let msg = Paragraph::new(Line::from(vec![Span::styled(
            " Loading retention leases...",
            theme::MUTED,
        )]));
        f.render_widget(msg, area);
        return;
    }

    if app.retention_leases.leases.is_empty() {
        let msg = Paragraph::new(Line::from(vec![Span::styled(
            " No retention leases found",
            theme::MUTED,
        )]));
        f.render_widget(msg, area);
        return;
    }

    // Column layout: icon(4) | pipeline(fill) | run#(12) | owner(fill) | protect(4) | valid_until(22) | created(22)
    let rects = Layout::horizontal([
        Constraint::Length(4),  // lock icon
        Constraint::Fill(3),    // pipeline name
        Constraint::Length(12), // run ID
        Constraint::Fill(2),    // owner
        Constraint::Length(4),  // protect pipeline
        Constraint::Length(22), // valid until
        Constraint::Length(22), // created
    ])
    .split(area);
    let mut widths: Vec<usize> = rects.iter().map(|r| r.width as usize).collect();
    widths[1] = widths[1].min(40); // pipeline name
    widths[3] = widths[3].min(30); // owner

    let items: Vec<ListItem> = app
        .retention_leases
        .leases
        .iter()
        .enumerate()
        .map(|(i, lease)| {
            let row_style = if i == app.retention_leases.nav.index() {
                theme::SELECTED
            } else {
                Style::new()
            };

            let pipeline_name = app
                .data
                .definitions
                .iter()
                .find(|d| d.id == lease.definition_id)
                .map(|d| d.name.as_str())
                .unwrap_or("unknown");

            let protect_icon = if lease.protect_pipeline { "🛡" } else { " " };

            let valid_until = lease
                .valid_until
                .map(|dt| {
                    let now = Utc::now();
                    if dt < now {
                        format!("expired {}", format_relative(now.signed_duration_since(dt)))
                    } else {
                        format!("expires {}", format_relative(dt.signed_duration_since(now)))
                    }
                })
                .unwrap_or_else(|| "—".to_string());

            let created = lease
                .created_on
                .map(|dt| {
                    let elapsed = Utc::now().signed_duration_since(dt);
                    format!("{} ago", format_relative(elapsed))
                })
                .unwrap_or_else(|| "—".to_string());

            // Shorten owner ID for display
            let owner = truncate_owner(&lease.owner_id);

            ListItem::new(Line::from(vec![
                Span::styled(" 🔒 ", theme::WARNING),
                Span::styled(
                    format!(
                        "{:<width$} ",
                        truncate(pipeline_name, widths[1].saturating_sub(1)),
                        width = widths[1].saturating_sub(1)
                    ),
                    theme::TEXT,
                ),
                Span::styled(
                    format!("#{:<width$}", lease.run_id, width = widths[2] - 1),
                    theme::MUTED,
                ),
                Span::styled(
                    format!(
                        "{:<width$} ",
                        truncate(&owner, widths[3].saturating_sub(1)),
                        width = widths[3].saturating_sub(1)
                    ),
                    theme::BRANCH,
                ),
                Span::styled(format!("{:<4}", protect_icon), theme::APPROVAL),
                Span::styled(
                    format!("{:<width$}", valid_until, width = widths[5]),
                    if lease.valid_until.is_some_and(|dt| dt < Utc::now()) {
                        theme::ERROR
                    } else {
                        theme::SUCCESS
                    },
                ),
                Span::styled(
                    format!("{:>width$}", created, width = widths[6]),
                    theme::MUTED,
                ),
            ]))
            .style(row_style)
        })
        .collect();

    let loading_indicator = if app.retention_leases.loading {
        " ⟳"
    } else {
        ""
    };
    let title = format!(
        " Retention Leases ({}) {}",
        app.retention_leases.leases.len(),
        loading_indicator
    );
    let list = List::new(items).block(Block::new().title(title).title_style(theme::TITLE));

    let mut state = ListState::default();
    state.select(Some(app.retention_leases.nav.index()));
    f.render_stateful_widget(list, area, &mut state);
}

/// Format a chrono Duration into a human-readable relative string.
fn format_relative(duration: chrono::Duration) -> String {
    let days = duration.num_days();
    if days > 365 {
        let years = days / 365;
        if years == 1 {
            "1 year".to_string()
        } else {
            format!("{years} years")
        }
    } else if days > 30 {
        let months = days / 30;
        if months == 1 {
            "1 month".to_string()
        } else {
            format!("{months} months")
        }
    } else if days > 0 {
        if days == 1 {
            "1 day".to_string()
        } else {
            format!("{days} days")
        }
    } else {
        let hours = duration.num_hours();
        if hours > 0 {
            if hours == 1 {
                "1 hour".to_string()
            } else {
                format!("{hours} hours")
            }
        } else {
            "< 1 hour".to_string()
        }
    }
}

/// Shorten the owner ID for display (e.g., "System:Pipeline" → "Pipeline").
fn truncate_owner(owner_id: &str) -> String {
    if let Some((_prefix, rest)) = owner_id.split_once(':') {
        rest.to_string()
    } else {
        owner_id.to_string()
    }
}
