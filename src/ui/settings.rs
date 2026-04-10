use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::app::settings::{FieldKind, SettingsState};

use super::theme;

pub fn draw(f: &mut Frame, settings: &SettingsState) {
    let area = centered_rect(60, 70, f.area());

    f.render_widget(Clear, area);

    let mut lines: Vec<Line> = Vec::new();

    // Track the current section so we can insert headers at group transitions.
    let mut current_section: Option<&str> = None;

    for (i, field) in settings.fields.iter().enumerate() {
        // Section header on group transition
        if current_section != Some(field.section) {
            if current_section.is_some() {
                lines.push(Line::from("")); // spacing between groups
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

    // Footer hints
    lines.push(Line::from(""));
    let hint_text = if settings.editing {
        "Enter confirm  Esc cancel  ←→ move cursor"
    } else {
        "↑↓ navigate  Enter/Space edit  Ctrl+S save  Esc close"
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

fn format_field_value(kind: FieldKind, value: &str) -> String {
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
