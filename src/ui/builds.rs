use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use super::helpers::{Column, build_elapsed, compute_columns, status_icon, status_label, truncate};
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

    // Column layout: icon(3) | status(12) | build_number(16) | branch(flex) | requestor(flex) | elapsed(12)
    let col_spec = [
        Column::Fixed(3),  // icon
        Column::Fixed(12), // status label
        Column::Fixed(16), // build number
        Column::Flex {
            weight: 3,
            min: 10,
            max: 50,
        }, // branch
        Column::Flex {
            weight: 2,
            min: 10,
            max: 30,
        }, // requestor
        Column::Fixed(16), // elapsed
    ];
    let widths = compute_columns(&col_spec, area.width as usize);

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
                Span::styled(format!(" {} ", icon), Style::default().fg(icon_color)),
                Span::styled(
                    format!("{:<width$}", label, width = widths[1]),
                    Style::default().fg(icon_color),
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
                Style::default()
            })
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::NONE)
            .title(format!(" Builds ({}) ", app.build_history.builds.len()))
            .title_style(theme::TITLE),
    );

    let mut state = ListState::default();
    state.select(Some(app.build_history.nav.index()));
    f.render_stateful_widget(list, chunks[1], &mut state);
}
