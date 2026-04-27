//! Help overlay component showing keyboard shortcuts.

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use super::Component;
use crate::render::helpers::centered_rect;
use crate::render::theme;

/// Displays the full keybinding reference.
/// This is a stateless overlay; any key press dismisses it (handled by App).
#[derive(Default)]
pub struct Help;

impl Component for Help {
    fn draw(&self, f: &mut Frame, area: Rect) -> Result<()> {
        let popup = centered_rect(60, 70, area);

        f.render_widget(Clear, popup);

        let help_text = vec![
            Line::from(""),
            Line::from(vec![Span::styled("  Navigation", theme::SECTION_HEADER)]),
            Line::from(""),
            shortcut_line("  ↑ / ↓          ", "Move selection up / down"),
            shortcut_line(
                "  →              ",
                "Drill into selected item / expand tree rows",
            ),
            shortcut_line(
                "  Enter          ",
                "Drill into selected item / toggle backlog row",
            ),
            shortcut_line(
                "  ← / q / Esc    ",
                "Go back (drill-in) / q to Dashboard (root)",
            ),
            shortcut_line("  ← / →          ", "Collapse / expand folder (Pipelines)"),
            shortcut_line(
                "  ← / →          ",
                "Collapse / expand timeline node (Log Viewer)",
            ),
            shortcut_line("  PgUp / PgDn    ", "Scroll log content"),
            shortcut_line("  Mouse wheel    ", "Scroll log content"),
            Line::from(""),
            Line::from(vec![Span::styled("  Views", theme::SECTION_HEADER)]),
            Line::from(""),
            shortcut_line(
                "  1–5            ",
                "Switch area: Dashboard / Boards / Repos / Pipelines / Active Runs",
            ),
            Line::from(""),
            shortcut_line("  Dashboard      ", "Overview"),
            shortcut_line("  Boards         ", "Read-only backlog tree"),
            shortcut_line("  Repos          ", "Pull Requests"),
            shortcut_line("  Pipelines      ", "Definitions / Active Runs"),
            Line::from(""),
            Line::from(vec![Span::styled("  Actions", theme::SECTION_HEADER)]),
            Line::from(""),
            shortcut_line(
                "  /              ",
                "Search / filter (Boards / Pipelines / Active Runs / Pull Requests)",
            ),
            shortcut_line(
                "  Space          ",
                "Select / deselect (Pipelines / Active Runs)",
            ),
            shortcut_line(
                "  p              ",
                "Pin / unpin (Pipelines / Boards / My Work Items)",
            ),
            shortcut_line(
                "  f              ",
                "Follow latest active task (Log Viewer)",
            ),
            shortcut_line("  n              ", "Queue pipeline run (new run)"),
            shortcut_line("  t              ", "Retry failed stage (Log Viewer)"),
            shortcut_line(
                "  a              ",
                "Approve check (Log Viewer, on checkpoint row)",
            ),
            shortcut_line(
                "  j              ",
                "Reject check (Log Viewer, on checkpoint row)",
            ),
            shortcut_line(
                "  c              ",
                "Cancel build (Active Runs / Log Viewer)",
            ),
            shortcut_line(
                "  d              ",
                "Delete retention leases (Build History)",
            ),
            shortcut_line("  o              ", "Open in browser"),
            shortcut_line("  r              ", "Force data refresh"),
            shortcut_line("  x              ", "Dismiss notification"),
            shortcut_line("  ,              ", "Open settings"),
            shortcut_line("  ?              ", "Toggle this help"),
            shortcut_line("  Ctrl+C         ", "Quit immediately"),
            Line::from(""),
        ];

        let block = Block::bordered()
            .title(" Help — Keybindings ")
            .title_style(theme::BRAND)
            .border_type(BorderType::Rounded)
            .border_style(theme::PANEL_BORDER_FOCUSED)
            .style(theme::PANEL_ELEVATED);

        let help = Paragraph::new(help_text)
            .style(theme::PANEL_ELEVATED)
            .block(block);
        f.render_widget(help, popup);
        Ok(())
    }
}

fn shortcut_line(prefix: &'static str, description: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled(prefix, theme::KEY),
        Span::styled(description, theme::TEXT),
    ])
}
