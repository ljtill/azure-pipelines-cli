use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};

use super::helpers::{build_elapsed, status_icon, status_label, truncate};
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

    // Column layout: icon(3) | status(12) | build_number(18) | branch(fill) | requestor(fill) | elapsed(15)
    let rects = Layout::horizontal([
        Constraint::Length(3),  // icon
        Constraint::Length(12), // status label
        Constraint::Length(18), // build number
        Constraint::Fill(2),    // branch
        Constraint::Fill(2),    // requestor
        Constraint::Length(15), // elapsed
    ])
    .split(area);
    let widths: Vec<usize> = rects.iter().map(|r| r.width as usize).collect();

    let items: Vec<ListItem> = app
        .build_history
        .builds
        .iter()
        .enumerate()
        .map(|(i, build)| {
            let (icon, icon_color) = status_icon(build.status, build.result);
            let label = status_label(build.status, build.result);
            let time_info = build_elapsed(build);
            let branch = build.branch_display();

            ListItem::new(Line::from(vec![
                Span::styled(format!(" {} ", icon), Style::new().fg(icon_color)),
                Span::styled(
                    format!("{:<width$}", label, width = widths[1]),
                    Style::new().fg(icon_color),
                ),
                Span::styled(
                    format!(
                        "#{:<width$}",
                        truncate(&build.build_number, widths[2] - 1),
                        width = widths[2] - 1
                    ),
                    theme::TEXT,
                ),
                Span::styled(
                    format!(
                        "{:<width$} ",
                        truncate(&branch, widths[3].saturating_sub(1)),
                        width = widths[3].saturating_sub(1)
                    ),
                    theme::BRANCH,
                ),
                Span::styled(
                    format!(
                        "{:<width$} ",
                        truncate(build.requestor(), widths[4].saturating_sub(1)),
                        width = widths[4].saturating_sub(1)
                    ),
                    theme::MUTED,
                ),
                Span::styled(time_info, theme::MUTED),
            ]))
            .style(if i == app.build_history.nav.index() {
                theme::SELECTED
            } else {
                Style::new()
            })
        })
        .collect();

    let list = List::new(items).block(
        Block::new()
            .title(format!(" Builds ({}) ", app.build_history.builds.len()))
            .title_style(theme::TITLE),
    );

    let mut state = ListState::default();
    state.select(Some(app.build_history.nav.index()));
    f.render_stateful_widget(list, chunks[1], &mut state);
}
