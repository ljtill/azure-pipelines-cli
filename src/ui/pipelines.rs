use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::app::{App, InputMode};

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    use ratatui::layout::{Constraint, Layout};

    let show_search = app.input_mode == InputMode::Search || !app.search_query.is_empty();

    let chunks = if show_search {
        Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area)
    } else {
        Layout::vertical([Constraint::Min(0)]).split(area)
    };

    let list_area = if show_search { chunks[1] } else { chunks[0] };

    if show_search {
        let search = Paragraph::new(Line::from(vec![
            Span::styled(" / ", Style::default().fg(Color::Yellow)),
            Span::raw(&app.search_query),
            if app.input_mode == InputMode::Search {
                Span::styled("▌", Style::default().fg(Color::Cyan))
            } else {
                Span::raw("")
            },
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Filter ")
                .title_style(Style::default().fg(Color::Yellow)),
        );
        f.render_widget(search, chunks[0]);
    }

    let items: Vec<ListItem> = app
        .filtered_pipelines
        .iter()
        .enumerate()
        .map(|(i, def)| {
            let folder = def
                .path
                .trim_start_matches('\\')
                .replace('\\', " / ");

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {:<42} ", def.name),
                    Style::default().fg(Color::White),
                ),
                Span::styled(folder, Style::default().fg(Color::DarkGray)),
            ]))
            .style(if i == app.pipelines_index {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            })
        })
        .collect();

    let title = format!(
        " All Pipelines ({}) ",
        app.filtered_pipelines.len()
    );
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::NONE)
            .title(title)
            .title_style(Style::default().fg(Color::Cyan)),
    );

    let mut state = ListState::default();
    state.select(Some(app.pipelines_index));
    f.render_stateful_widget(list, list_area, &mut state);
}
