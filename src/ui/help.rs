use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use super::theme;

pub fn draw(f: &mut Frame) {
    let area = centered_rect(60, 70, f.area());

    f.render_widget(Clear, area);

    let help_text = vec![
        Line::from(""),
        Line::from(vec![Span::styled("  Navigation", theme::SECTION_HEADER)]),
        Line::from(""),
        Line::from("  ↑ / ↓          Move selection up / down"),
        Line::from("  → / Enter      Drill into selected item / expand (tree views)"),
        Line::from("  ← / q / Esc    Go back / collapse (tree views)"),
        Line::from("  ← / →          Collapse / expand folder (Dashboard)"),
        Line::from("  ← / →          Collapse / expand timeline node (Log Viewer)"),
        Line::from("  PgUp / PgDn    Scroll log content"),
        Line::from("  Mouse wheel    Scroll log content"),
        Line::from(""),
        Line::from(vec![Span::styled("  Views", theme::SECTION_HEADER)]),
        Line::from(""),
        Line::from("  1              Dashboard (grouped by folder)"),
        Line::from("  2              All Pipelines (flat list)"),
        Line::from("  3              Active Runs"),
        Line::from(""),
        Line::from(vec![Span::styled("  Actions", theme::SECTION_HEADER)]),
        Line::from(""),
        Line::from("  /              Search / filter (Pipelines / Active Runs)"),
        Line::from("  Space          Select / deselect (Active Runs)"),
        Line::from("  f              Follow latest active task (Log Viewer)"),
        Line::from("  Q              Queue pipeline run"),
        Line::from("  R              Retry failed stage (Log Viewer)"),
        Line::from("  A              Approve check (Log Viewer, on checkpoint row)"),
        Line::from("  D              Reject check (Log Viewer, on checkpoint row)"),
        Line::from("  c              Cancel build (Active Runs / Log Viewer)"),
        Line::from("  o              Open in browser"),
        Line::from("  r              Force data refresh"),
        Line::from("  x              Dismiss notification"),
        Line::from("  ,              Open settings"),
        Line::from("  ?              Toggle this help"),
        Line::from("  Ctrl+C         Quit immediately"),
        Line::from(""),
    ];

    let block = Block::bordered()
        .title(" Help — Keybindings ")
        .title_style(theme::BRAND);

    let help = Paragraph::new(help_text).style(theme::TEXT).block(block);
    f.render_widget(help, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}
