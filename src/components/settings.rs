//! Settings overlay component for runtime configuration.

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
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
                theme::KEY
            } else {
                theme::MUTED
            };

            let value_display = format_field_value(field.kind, &field.value);

            let mut spans = vec![Span::styled(format!("  {:<30}", field.label), label_style)];

            if is_editing {
                let (before, after) = field.value.split_at(settings.cursor.min(field.value.len()));
                spans.push(Span::styled(
                    before.to_string(),
                    field_value_style(is_selected),
                ));
                spans.push(Span::styled("█", theme::CURSOR));
                spans.push(Span::styled(
                    after.to_string(),
                    field_value_style(is_selected),
                ));
            } else {
                spans.push(Span::styled(value_display, field_value_style(is_selected)));
            }

            if !field.hint.is_empty() && !is_editing {
                spans.push(Span::styled(format!("  ({})", field.hint), theme::MUTED));
            }

            let line = if is_selected {
                Line::from(spans).style(theme::SELECTED)
            } else {
                Line::from(spans)
            };
            lines.push(line);
        }

        lines.push(Line::from(""));
        let hint_line = if settings.editing {
            command_hints(&[
                ("Enter", "confirm"),
                ("Esc", "cancel"),
                ("←→", "move cursor"),
            ])
        } else {
            command_hints(&[
                ("↑↓", "navigate"),
                ("Enter/Space", "edit"),
                ("Ctrl+S", "save"),
                ("q", "close"),
            ])
        };
        lines.push(hint_line);
        lines.push(Line::from(""));

        let block = Block::bordered()
            .title(" Settings ")
            .title_style(theme::BRAND);

        let content = Paragraph::new(lines).block(block);
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

fn field_value_style(selected: bool) -> ratatui::style::Style {
    if selected {
        theme::TEXT.add_modifier(Modifier::BOLD)
    } else {
        theme::TEXT
    }
}

fn command_hints(commands: &[(&str, &str)]) -> Line<'static> {
    let mut spans = vec![Span::raw("  ")];
    for (i, (key, label)) in commands.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled((*key).to_string(), theme::KEY));
        spans.push(Span::styled(format!(" {label}"), theme::MUTED));
    }
    Line::from(spans)
}
