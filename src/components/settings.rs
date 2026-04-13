//! Settings overlay component for runtime configuration.

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use super::Component;
use crate::render::helpers::centered_rect;
use crate::render::theme;
use crate::state::settings::{FieldKind, SettingsState};

/// Renders the config editor overlay.
pub struct Settings;

impl Settings {
    /// Renders the settings overlay for a given settings state.
    pub fn draw_with_state(&self, f: &mut Frame, settings: &SettingsState) {
        let area = centered_rect(60, 70, f.area());

        f.render_widget(Clear, area);

        let mut lines: Vec<Line> = Vec::new();

        let mut current_section: Option<&str> = None;

        for (i, field) in settings.fields.iter().enumerate() {
            if current_section != Some(field.section) {
                if current_section.is_some() {
                    lines.push(Line::from(""));
                }
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    format!("  {}", field.section),
                    theme::SECTION_HEADER,
                )]));
                lines.push(Line::from(""));
                current_section = Some(field.section);
            }

            let is_selected = i == settings.selected;
            let is_editing = is_selected && settings.editing;

            let label_style = if is_selected {
                theme::BRAND
            } else {
                theme::MUTED
            };

            let value_display = format_field_value(field.kind, &field.value);

            let mut spans = vec![Span::styled(format!("  {:<30}", field.label), label_style)];

            if is_editing {
                let (before, after) = field.value.split_at(settings.cursor.min(field.value.len()));
                spans.push(Span::styled(before.to_string(), theme::TEXT));
                spans.push(Span::styled("█", theme::CURSOR));
                spans.push(Span::styled(after.to_string(), theme::TEXT));
            } else {
                spans.push(Span::styled(value_display, theme::TEXT));
            }

            if !field.hint.is_empty() && !is_editing {
                spans.push(Span::styled(format!("  ({})", field.hint), theme::MUTED));
            }

            lines.push(Line::from(spans));
        }

        lines.push(Line::from(""));
        let hint_text = if settings.editing {
            "Enter confirm  Esc cancel  ←→ move cursor"
        } else {
            "↑↓ navigate  Enter/Space edit  Ctrl+S save  q close"
        };
        lines.push(Line::from(vec![Span::styled(
            format!("  {hint_text}"),
            theme::MUTED,
        )]));
        lines.push(Line::from(""));

        let block = Block::bordered()
            .title(" Settings ")
            .title_style(theme::BRAND);

        let content = Paragraph::new(lines).style(theme::TEXT).block(block);
        f.render_widget(content, area);
    }
}

impl Component for Settings {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }
}

fn format_field_value(kind: FieldKind, value: &str) -> String {
    match kind {
        FieldKind::Toggle => {
            if value == "true" {
                "● on".to_string()
            } else {
                "○ off".to_string()
            }
        }
        _ => value.to_string(),
    }
}
