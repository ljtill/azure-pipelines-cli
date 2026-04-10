use chrono::Utc;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};

use super::helpers::{draw_search_bar, truncate};
use super::theme;
use crate::app::{App, InputMode};

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

    let show_search = app.view == crate::app::View::RetentionLeases
        && (app.search.mode == InputMode::Search || !app.search.query.is_empty());

    let chunks = if show_search {
        Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area)
    } else {
        Layout::vertical([Constraint::Min(0)]).split(area)
    };

    let list_area = if show_search { chunks[1] } else { chunks[0] };

    if show_search {
        draw_search_bar(f, chunks[0], &app.search.query, app.search.mode);
    }

    // Column layout: check(2) | icon(4) | pipeline(fill) | run#(12) | owner(fill) | protect(4) | valid_until(22) | created(22)
    let rects = Layout::horizontal([
        Constraint::Length(2),  // check
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
    widths[2] = widths[2].min(40); // pipeline name
    widths[4] = widths[4].min(30); // owner

    let items: Vec<ListItem> = app
        .retention_leases
        .filtered
        .iter()
        .enumerate()
        .map(|(i, lease)| {
            let selected = app.retention_leases.selected.contains(&lease.lease_id);
            let check = if selected { "✓ " } else { "  " };

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

            let owner = truncate_owner(&lease.owner_id);

            ListItem::new(Line::from(vec![
                Span::styled(
                    check,
                    if selected {
                        theme::SUCCESS
                    } else {
                        Style::new()
                    },
                ),
                Span::styled(" 🔒 ", theme::WARNING),
                Span::styled(
                    format!(
                        "{:<width$} ",
                        truncate(pipeline_name, widths[2].saturating_sub(1)),
                        width = widths[2].saturating_sub(1)
                    ),
                    theme::TEXT,
                ),
                Span::styled(
                    format!("#{:<width$}", lease.run_id, width = widths[3] - 1),
                    theme::MUTED,
                ),
                Span::styled(
                    format!(
                        "{:<width$} ",
                        truncate(&owner, widths[4].saturating_sub(1)),
                        width = widths[4].saturating_sub(1)
                    ),
                    theme::BRANCH,
                ),
                Span::styled(format!("{:<4}", protect_icon), theme::APPROVAL),
                Span::styled(
                    format!("{:<width$}", valid_until, width = widths[6]),
                    if lease.valid_until.is_some_and(|dt| dt < Utc::now()) {
                        theme::ERROR
                    } else {
                        theme::SUCCESS
                    },
                ),
                Span::styled(
                    format!("{:>width$}", created, width = widths[7]),
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
    let sel_count = app.retention_leases.selected.len();
    let filtered = app.retention_leases.filtered.len();
    let total = app.retention_leases.leases.len();
    let title = if sel_count > 0 {
        format!(
            " Retention Leases ({} / {}) — {} selected {}",
            filtered, total, sel_count, loading_indicator
        )
    } else if filtered != total {
        format!(
            " Retention Leases ({} / {}) {}",
            filtered, total, loading_indicator
        )
    } else {
        format!(" Retention Leases ({}) {}", total, loading_indicator)
    };
    let list = List::new(items).block(Block::new().title(title).title_style(theme::TITLE));

    let mut state = ListState::default();
    state.select(Some(app.retention_leases.nav.index()));
    f.render_stateful_widget(list, list_area, &mut state);
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
