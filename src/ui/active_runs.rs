use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState};

use super::helpers::{build_elapsed, draw_search_bar, status_icon, status_label, truncate};
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

    // Column layout: check(2) | icon(4) | status(12) | name(fill) | build_number(18) | branch(fill) | requestor(fill) | elapsed(15)
    let rects = Layout::horizontal([
        Constraint::Length(2),  // check
        Constraint::Length(4),  // icon
        Constraint::Length(12), // status label
        Constraint::Fill(2),    // pipeline name
        Constraint::Length(18), // build number
        Constraint::Fill(2),    // branch
        Constraint::Fill(2),    // requestor
        Constraint::Length(15), // elapsed
    ])
    .split(area);
    let widths: Vec<usize> = rects.iter().map(|r| r.width as usize).collect();

    let items: Vec<ListItem> = app
        .active_runs
        .filtered
        .iter()
        .enumerate()
        .map(|(i, build)| {
            let elapsed = build_elapsed(build);
            let selected = app.active_runs.selected.contains(&build.id);
            let check = if selected { "✓ " } else { "  " };
            let (icon, icon_color) = status_icon(build.status, build.result);
            let label = status_label(build.status, build.result);

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
                        "{:<width$} ",
                        truncate(&build.definition.name, widths[3].saturating_sub(1)),
                        width = widths[3].saturating_sub(1)
                    ),
                    theme::TEXT,
                ),
                Span::styled(
                    format!(
                        "#{:<width$}",
                        truncate(&build.build_number, widths[4] - 1),
                        width = widths[4] - 1
                    ),
                    theme::MUTED,
                ),
                Span::styled(
                    format!(
                        "{:<width$} ",
                        truncate(&build.branch_display(), widths[5].saturating_sub(1)),
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
                Span::styled(elapsed, theme::WARNING),
            ]))
            .style(if i == app.active_runs.nav.index() {
                theme::SELECTED
            } else {
                Style::new()
            })
        })
        .collect();

    let sel_count = app.active_runs.selected.len();
    let filtered = app.active_runs.filtered.len();
    let total = app.data.active_builds.len();
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
    let list = List::new(items).block(Block::new().title(title).title_style(theme::TITLE));

    let mut state = ListState::default();
    state.select(Some(app.active_runs.nav.index()));
    f.render_stateful_widget(list, list_area, &mut state);
}
