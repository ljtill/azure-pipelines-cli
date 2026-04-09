use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use super::helpers::{build_elapsed, draw_search_bar};
use crate::app::{App, InputMode};

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    use ratatui::layout::{Constraint, Layout};

    let show_search = app.view == crate::app::View::ActiveRuns
        && (app.input_mode == InputMode::Search || !app.search_query.is_empty());

    let chunks = if show_search {
        Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area)
    } else {
        Layout::vertical([Constraint::Min(0)]).split(area)
    };

    let list_area = if show_search { chunks[1] } else { chunks[0] };

    if show_search {
        draw_search_bar(f, chunks[0], &app.search_query, app.input_mode);
    }

    let items: Vec<ListItem> = app
        .filtered_active_builds
        .iter()
        .enumerate()
        .map(|(i, build)| {
            let elapsed = build_elapsed(build);
            let selected = app.selected_builds.contains(&build.id);
            let check = if selected { "✓ " } else { "  " };

            ListItem::new(Line::from(vec![
                Span::styled(
                    check,
                    if selected {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default()
                    },
                ),
                Span::styled(" ⏳ ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    format!("{:<36} ", build.definition.name),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("#{:<14} ", build.build_number),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{:<26} ", build.short_branch()),
                    Style::default().fg(Color::Blue),
                ),
                Span::styled(
                    format!("{:<20} ", build.requestor()),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(elapsed, Style::default().fg(Color::Yellow)),
            ]))
            .style(if i == app.active_runs_nav.index() {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            })
        })
        .collect();

    let sel_count = app.selected_builds.len();
    let filtered = app.filtered_active_builds.len();
    let total = app.active_builds.len();
    let title = if sel_count > 0 {
        format!(
            " Active Runs ({} / {}) — {} selected ",
            filtered, total, sel_count
        )
    } else if filtered != total {
        format!(" Active Runs ({} / {}) ", filtered, total)
    } else {
        format!(" Active Runs ({}) ", total)
    };
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::NONE)
            .title(title)
            .title_style(Style::default().fg(Color::Cyan)),
    );

    let mut state = ListState::default();
    state.select(Some(app.active_runs_nav.index()));
    f.render_stateful_widget(list, list_area, &mut state);
}
