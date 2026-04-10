use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use crate::app::settings::{FieldKind, SettingsState};

use super::theme;

pub fn draw(f: &mut Frame, settings: &SettingsState) {
    let height = (settings.field_count() as u16) + 6; // fields + border + title + hints + spacing
    let width = 64;
    let area = centered_rect(width, height, f.area());

    f.render_widget(Clear, area);

    let block = Block::bordered()
        .title(" Settings ")
        .title_style(theme::BRAND)
        .border_type(BorderType::Rounded);
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Split inner into field rows + hints row
    let mut constraints: Vec<Constraint> = settings
        .fields
        .iter()
        .map(|_| Constraint::Length(1))
        .collect();
    constraints.push(Constraint::Length(1)); // blank
    constraints.push(Constraint::Length(1)); // hints

    let rows = Layout::vertical(constraints).split(inner);

    for (i, field) in settings.fields.iter().enumerate() {
        let is_selected = i == settings.selected;
        let is_editing = is_selected && settings.editing;

        let label_style = if is_selected {
            theme::BRAND
        } else {
            theme::MUTED
        };

        let value_display = format_field_value(field.kind, &field.value, is_editing);

        let cursor_indicator = if is_editing { "█" } else { "" };

        let mut spans = vec![Span::styled(format!("  {:<28}", field.label), label_style)];

        if is_editing {
            // Show value with cursor
            let (before, after) = field.value.split_at(settings.cursor.min(field.value.len()));
            spans.push(Span::styled(before.to_string(), theme::TEXT));
            spans.push(Span::styled(cursor_indicator, theme::CURSOR));
            spans.push(Span::styled(after.to_string(), theme::TEXT));
        } else {
            spans.push(Span::styled(value_display, theme::TEXT));
        }

        if !field.hint.is_empty() && !is_editing {
            spans.push(Span::styled(format!("  ({})", field.hint), theme::MUTED));
        }

        let line = Line::from(spans);
        f.render_widget(Paragraph::new(line), rows[i]);
    }

    // Hints footer
    let hint_text = if settings.editing {
        "Enter confirm  Esc cancel  ←→ move cursor"
    } else {
        "↑↓ navigate  Enter/Space edit  s save  Esc close"
    };
    let hints = Line::from(vec![Span::styled(format!("  {hint_text}"), theme::MUTED)]);
    let hint_row = settings.field_count() + 1;
    f.render_widget(Paragraph::new(hints), rows[hint_row]);
}

fn format_field_value(kind: FieldKind, value: &str, _editing: bool) -> String {
    match kind {
        FieldKind::Toggle => {
            if value == "true" {
                "● on".to_string()
            } else {
                "○ off".to_string()
            }
        }
        FieldKind::Cycle => value.to_string(),
        _ => value.to_string(),
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .split(area);
    Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .split(vertical[0])[0]
}
