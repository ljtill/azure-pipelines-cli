use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};

use super::helpers::{build_elapsed, effective_status_icon, effective_status_label, truncate};
use super::theme;
use crate::app::App;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    use ratatui::layout::{Constraint, Layout};

    let chunks = Layout::vertical([
        Constraint::Length(2), // pipeline name header
        Constraint::Min(0),    // builds list
    ])
    .split(area);

    // Pipeline name header
    let def_name = app
        .build_history
        .selected_definition
        .as_ref()
        .map(|d| d.name.as_str())
        .unwrap_or("Unknown");

    let header = Paragraph::new(Line::from(vec![
        Span::styled(" ← ", theme::MUTED),
        Span::styled(def_name, theme::BRAND),
        Span::styled(" — Build History", theme::MUTED),
    ]));
    f.render_widget(header, chunks[0]);

    // Column layout: check(2) | icon(3) | status(12) | build_number(18) | retained(2) | branch(fill) | requestor(fill) | elapsed(15)
    let rects = Layout::horizontal([
        Constraint::Length(2),  // check
        Constraint::Length(3),  // icon
        Constraint::Length(12), // status label
        Constraint::Length(18), // build number
        Constraint::Length(2),  // retained indicator
        Constraint::Fill(2),    // branch
        Constraint::Fill(2),    // requestor
        Constraint::Length(15), // elapsed
    ])
    .split(area);
    let mut widths: Vec<usize> = rects.iter().map(|r| r.width as usize).collect();
    widths[5] = widths[5].min(40); // branch
    widths[6] = widths[6].min(35); // requestor

    let mut items: Vec<ListItem> = app
        .build_history
        .builds
        .iter()
        .enumerate()
        .map(|(i, build)| {
            let awaiting = app.data.pending_approval_build_ids.contains(&build.id);
            let (icon, icon_color) = effective_status_icon(build.status, build.result, awaiting);
            let label = effective_status_label(build.status, build.result, awaiting);
            let time_info = build_elapsed(build);
            let branch = build.branch_display();
            let retained = app.retention_leases.retained_run_ids.contains(&build.id);
            let selected = app.build_history.selected.contains(&build.id);
            let check = if selected { "✓ " } else { "  " };

            let row_style = if i == app.build_history.nav.index() {
                theme::SELECTED
            } else {
                Style::new()
            };

            ListItem::new(Line::from(vec![
                Span::styled(
                    check,
                    if selected {
                        theme::SUCCESS
                    } else {
                        Style::new()
                    },
                ),
                Span::styled(format!(" {} ", icon), Style::new().fg(icon_color)),
                Span::styled(
                    format!("{:<width$}", label, width = widths[2]),
                    Style::new().fg(icon_color),
                ),
                Span::styled(
                    format!(
                        "#{:<width$}",
                        truncate(&build.build_number, widths[3] - 1),
                        width = widths[3] - 1
                    ),
                    theme::TEXT,
                ),
                Span::styled(if retained { "◈ " } else { "  " }, theme::WARNING),
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
                        truncate(build.requestor(), widths[6].saturating_sub(1)),
                        width = widths[6].saturating_sub(1)
                    ),
                    theme::MUTED,
                ),
                Span::styled(
                    format!("{:>width$}", time_info, width = widths[7]),
                    theme::MUTED,
                ),
            ]))
            .style(row_style)
        })
        .collect();

    // Show loading/more indicator at the bottom
    if app.build_history.loading_more {
        items.push(ListItem::new(Line::from(vec![Span::styled(
            "   ⟳ Loading more...",
            theme::MUTED,
        )])));
    } else if app.build_history.has_more {
        items.push(ListItem::new(Line::from(vec![Span::styled(
            "   ▾ ↓ for more",
            theme::MUTED,
        )])));
    }

    let sel_count = app.build_history.selected.len();
    let total = app.build_history.builds.len();
    let title = if sel_count > 0 {
        format!(" Builds ({}) — {} selected ", total, sel_count)
    } else {
        format!(" Builds ({}) ", total)
    };
    let list = List::new(items).block(Block::new().title(title).title_style(theme::TITLE));

    let mut state = ListState::default();
    state.select(Some(app.build_history.nav.index()));
    f.render_stateful_widget(list, chunks[1], &mut state);
}
