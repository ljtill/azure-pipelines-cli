use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use super::helpers::build_elapsed;
use crate::app::App;

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .active_builds
        .iter()
        .enumerate()
        .map(|(i, build)| {
            let elapsed = build_elapsed(build);

            ListItem::new(Line::from(vec![
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
            .style(if i == app.active_runs_index {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            })
        })
        .collect();

    let title = format!(" Active Runs ({}) ", app.active_builds.len());
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::NONE)
            .title(title)
            .title_style(Style::default().fg(Color::Cyan)),
    );

    let mut state = ListState::default();
    state.select(Some(app.active_runs_index));
    f.render_stateful_widget(list, area, &mut state);
}
