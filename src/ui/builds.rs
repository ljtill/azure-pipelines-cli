use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use super::helpers::{build_elapsed, status_icon, truncate};
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

    // Fixed overhead: icon(3) + build_number(16) + requestor(21) + elapsed(10) = 50
    let branch_width = (area.width as usize).saturating_sub(51).clamp(10, 40);

    let items: Vec<ListItem> = app
        .build_history
        .builds
        .iter()
        .enumerate()
        .map(|(i, build)| {
            let (icon, icon_color) = status_icon(build.status, build.result);
            let time_info = build_elapsed(build);
            let branch = build.short_branch();

            ListItem::new(Line::from(vec![
                Span::styled(format!(" {} ", icon), Style::default().fg(icon_color)),
                Span::styled(format!("#{:<14} ", build.build_number), theme::TEXT),
                Span::styled(
                    format!(
                        "{:<width$} ",
                        truncate(&branch, branch_width),
                        width = branch_width
                    ),
                    theme::BRANCH,
                ),
                Span::styled(format!("{:<20} ", build.requestor()), theme::MUTED),
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
