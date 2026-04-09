use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use super::helpers::{build_elapsed, draw_search_bar, truncate};
use super::theme;
use crate::app::{App, InputMode};

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    use ratatui::layout::{Constraint, Layout};

    let show_search = app.view == crate::app::View::ActiveRuns
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

    // Fixed overhead: check(2) + icon(4) + build_number(16) + branch(27) + requestor(21) + elapsed(10) = 80
    let name_width = (area.width as usize).saturating_sub(81).clamp(15, 60);

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
                        theme::SUCCESS
                    } else {
                        Style::default()
                    },
                ),
                Span::styled(" ⏳ ", theme::WARNING),
                Span::styled(
                    format!(
                        "{:<width$} ",
                        truncate(&build.definition.name, name_width),
                        width = name_width
                    ),
                    theme::TEXT,
                ),
                Span::styled(format!("#{:<14} ", build.build_number), theme::MUTED),
                Span::styled(format!("{:<26} ", build.short_branch()), theme::BRANCH),
                Span::styled(format!("{:<20} ", build.requestor()), theme::MUTED),
                Span::styled(elapsed, theme::WARNING),
            ]))
            .style(if i == app.active_runs_nav.index() {
                theme::SELECTED
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
            .title_style(theme::TITLE),
    );

    let mut state = ListState::default();
    state.select(Some(app.active_runs_nav.index()));
    f.render_stateful_widget(list, list_area, &mut state);
}
