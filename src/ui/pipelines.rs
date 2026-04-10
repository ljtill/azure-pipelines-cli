use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState};

use super::helpers::{draw_search_bar, truncate};
use super::theme;
use crate::app::{App, InputMode};

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    use ratatui::layout::{Constraint, Layout};

    let show_search = app.search.mode == InputMode::Search || !app.search.query.is_empty();

    let chunks = if show_search {
        Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area)
    } else {
        Layout::vertical([Constraint::Min(0)]).split(area)
    };

    let list_area = if show_search { chunks[1] } else { chunks[0] };

    if show_search {
        draw_search_bar(f, chunks[0], &app.search.query, app.search.mode);
    }

    // Column layout: padding(1) | name(fill) | folder(fill)
    let rects = Layout::horizontal([
        Constraint::Length(1), // leading padding
        Constraint::Fill(2),   // pipeline name
        Constraint::Fill(3),   // folder path
    ])
    .split(area);
    let widths: Vec<usize> = rects.iter().map(|r| r.width as usize).collect();

    let items: Vec<ListItem> = app
        .pipelines
        .filtered
        .iter()
        .enumerate()
        .map(|(i, def)| {
            let folder = def.path.trim_start_matches('\\').replace('\\', " / ");

            ListItem::new(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    format!(
                        "{:<width$} ",
                        truncate(&def.name, widths[1]),
                        width = widths[1]
                    ),
                    theme::TEXT,
                ),
                Span::styled(truncate(&folder, widths[2]).to_string(), theme::MUTED),
            ]))
            .style(if i == app.pipelines.nav.index() {
                theme::SELECTED
            } else {
                Style::new()
            })
        })
        .collect();

    let title = format!(" All Pipelines ({}) ", app.pipelines.filtered.len());
    let list = List::new(items).block(Block::new().title(title).title_style(theme::TITLE));

    let mut state = ListState::default();
    state.select(Some(app.pipelines.nav.index()));
    f.render_stateful_widget(list, list_area, &mut state);
}
