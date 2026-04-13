//! Azure Boards placeholder view component.

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use super::Component;
use crate::render::theme;
use crate::state::ListNav;

/// Renders a lightweight placeholder for future Azure Boards support.
#[derive(Debug, Default)]
pub struct Boards {
    pub nav: ListNav,
}

impl Boards {
    /// Renders the Boards placeholder view.
    pub fn draw_with_app(&self, f: &mut Frame, area: Rect) {
        let body = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                " Azure Boards support is planned for a future phase.",
                theme::TEXT,
            )]),
            Line::from(vec![Span::styled(
                " The top-level shell is in place so Boards can slot in cleanly when features are added.",
                theme::MUTED,
            )]),
        ];

        let paragraph = Paragraph::new(body).block(
            Block::bordered()
                .title(" Azure Boards ")
                .title_style(theme::TITLE),
        );
        f.render_widget(paragraph, area);
    }
}

impl Component for Boards {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn footer_hints(&self) -> &'static str {
        "1/2/3/4 areas  [/] views  , settings  ? help  q/Esc back"
    }
}
