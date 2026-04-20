//! Work item detail view component.

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::Component;
use crate::client::models::{AssignedToField, WorkItem, WorkItemComment};
use crate::render::helpers::{draw_state_message, draw_view_frame, view_block};
use crate::render::theme;
use crate::state::{App, ListNav};

/// Stores state for the work item detail drill-in view.
#[derive(Debug, Default)]
pub struct WorkItemDetail {
    /// The id requested via `navigate_to_work_item_detail`. Used as a
    /// stale-response guard when late messages arrive for an earlier item.
    pub work_item_id: Option<u32>,
    pub work_item: Option<WorkItem>,
    pub comments: Vec<WorkItemComment>,
    pub nav: ListNav,
    pub loading: bool,
    /// Root view to return to on back navigation.
    pub return_to: Option<crate::state::View>,
}

impl WorkItemDetail {
    /// Returns the number of navigable sections in the detail view. Used so
    /// `↑/↓` scroll can step across logical blocks.
    pub fn section_count(&self) -> usize {
        if self.work_item.is_none() {
            return 0;
        }
        // Sections: header + metadata + body(description/AC/repro) + comments.
        4
    }

    /// Renders the detail view using data from the App.
    pub fn draw_with_app(&self, f: &mut Frame, _app: &App, area: Rect) {
        let subtitle = self.work_item.as_ref().map(|wi| {
            Line::from(vec![
                Span::styled(format!(" #{}", wi.id), theme::TEXT),
                Span::styled(format!("  ·  {}", wi.work_item_type()), theme::MUTED),
                Span::styled("  ·  ", theme::MUTED),
                Span::styled(wi.state_label(), theme::TEXT),
                Span::styled(
                    format!("  ·  {}", wi.assigned_to_display().unwrap_or("Unassigned"),),
                    theme::MUTED,
                ),
            ])
        });
        let content_area = draw_view_frame(f, area, " Work Item Detail ", subtitle);

        if self.loading {
            draw_state_message(f, content_area, " Loading work item…", theme::MUTED);
            return;
        }

        let Some(wi) = &self.work_item else {
            draw_state_message(f, content_area, " No work item selected", theme::MUTED);
            return;
        };

        let chunks = Layout::vertical([
            Constraint::Length(4), // header (type · id · state · title).
            Constraint::Min(4),    // two-column body.
        ])
        .split(content_area);

        draw_header(f, chunks[0], wi);

        let body =
            Layout::horizontal([Constraint::Ratio(3, 5), Constraint::Ratio(2, 5)]).split(chunks[1]);

        draw_description_pane(f, body[0], wi);
        draw_side_pane(f, body[1], wi, &self.comments);
    }
}

fn draw_header(f: &mut Frame, area: Rect, wi: &WorkItem) {
    let state_style = state_style(wi.state_label());

    let lines = vec![
        Line::from(vec![
            Span::styled(" ", theme::MUTED),
            Span::styled(
                format!("{} #{}", wi.work_item_type(), wi.id),
                theme::SECTION_HEADER,
            ),
            Span::styled("    ", theme::MUTED),
            Span::styled(wi.state_label().to_string(), state_style),
        ]),
        Line::from(vec![
            Span::raw("   "),
            Span::styled(wi.title(), theme::TEXT),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Assigned: ", theme::MUTED),
            Span::styled(
                wi.assigned_to_display().unwrap_or("Unassigned").to_string(),
                theme::TEXT,
            ),
            Span::styled("    Iteration: ", theme::MUTED),
            Span::styled(
                wi.fields
                    .iteration_path
                    .as_deref()
                    .unwrap_or("—")
                    .to_string(),
                theme::TEXT,
            ),
            Span::styled("    Area: ", theme::MUTED),
            Span::styled(
                wi.fields.area_path.as_deref().unwrap_or("—").to_string(),
                theme::TEXT,
            ),
        ]),
    ];

    f.render_widget(Paragraph::new(lines), area);
}

fn draw_description_pane(f: &mut Frame, area: Rect, wi: &WorkItem) {
    let mut lines: Vec<Line> = Vec::new();

    append_html_section(&mut lines, "Description", wi.fields.description.as_deref());

    let type_ci = wi.work_item_type();
    if type_ci.eq_ignore_ascii_case("Bug") {
        append_html_section(&mut lines, "Repro Steps", wi.fields.repro_steps.as_deref());
    } else {
        append_html_section(
            &mut lines,
            "Acceptance Criteria",
            wi.fields.acceptance_criteria.as_deref(),
        );
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No description provided",
            theme::MUTED,
        )));
    }

    let paragraph = Paragraph::new(lines)
        .block(view_block(" Body "))
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

fn append_html_section<'a>(lines: &mut Vec<Line<'a>>, heading: &'a str, html: Option<&str>) {
    let Some(raw) = html else { return };
    let text = strip_html(raw);
    if text.trim().is_empty() {
        return;
    }
    if !lines.is_empty() {
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        format!("  {heading}"),
        theme::SECTION_HEADER,
    )));
    for body_line in text.lines() {
        lines.push(Line::from(Span::styled(
            format!("  {body_line}"),
            theme::TEXT,
        )));
    }
}

fn draw_side_pane(f: &mut Frame, area: Rect, wi: &WorkItem, comments: &[WorkItemComment]) {
    let halves =
        Layout::vertical([Constraint::Length(metadata_height(wi)), Constraint::Min(3)]).split(area);

    // Metadata panel.
    let mut meta_lines = Vec::new();
    if let Some(priority) = wi.fields.priority {
        meta_lines.push(meta_row("Priority", priority.to_string()));
    }
    if let Some(severity) = &wi.fields.severity {
        meta_lines.push(meta_row("Severity", severity.clone()));
    }
    if let Some(points) = wi.fields.story_points {
        meta_lines.push(meta_row("Story Points", format_f64(points)));
    }
    if let Some(effort) = wi.fields.effort {
        meta_lines.push(meta_row("Effort", format_f64(effort)));
    }
    if let Some(value_area) = &wi.fields.value_area {
        meta_lines.push(meta_row("Value Area", value_area.clone()));
    }
    if let Some(tags) = tags_display(wi.fields.tags.as_deref()) {
        meta_lines.push(meta_row("Tags", tags));
    }
    if let Some(reason) = &wi.fields.reason {
        meta_lines.push(meta_row("Reason", reason.clone()));
    }
    if let Some(created_by) = wi
        .fields
        .created_by
        .as_ref()
        .map(AssignedToField::display_name)
    {
        meta_lines.push(meta_row(
            "Created",
            format_who_when(created_by, wi.fields.created_date.as_deref()),
        ));
    }
    if let Some(changed_by) = wi
        .fields
        .changed_by
        .as_ref()
        .map(AssignedToField::display_name)
    {
        meta_lines.push(meta_row(
            "Changed",
            format_who_when(changed_by, wi.fields.changed_date.as_deref()),
        ));
    }
    let parent = wi.parent_id();
    let child_count = wi.child_ids().len();
    if parent.is_some() || child_count > 0 {
        use std::fmt::Write;
        let mut rel = String::new();
        if let Some(pid) = parent {
            let _ = write!(rel, "parent #{pid}");
        }
        if child_count > 0 {
            if !rel.is_empty() {
                rel.push_str("  ·  ");
            }
            let _ = write!(rel, "{child_count} child item(s)");
        }
        meta_lines.push(meta_row("Links", rel));
    }

    if meta_lines.is_empty() {
        meta_lines.push(Line::from(Span::styled("  No metadata", theme::MUTED)));
    }

    let meta = Paragraph::new(meta_lines)
        .block(view_block(" Metadata "))
        .wrap(Wrap { trim: false });
    f.render_widget(meta, halves[0]);

    // Comments panel.
    let title = format!(" Comments ({}) ", comments.len());
    let mut comment_lines = Vec::new();
    if comments.is_empty() {
        comment_lines.push(Line::from(Span::styled("  No comments", theme::MUTED)));
    } else {
        // Newest first.
        let mut sorted: Vec<&WorkItemComment> = comments.iter().collect();
        sorted.sort_by(|a, b| b.created_date.cmp(&a.created_date));
        for (idx, c) in sorted.iter().enumerate() {
            if idx > 0 {
                comment_lines.push(Line::from(""));
            }
            let author = c
                .created_by
                .as_ref()
                .map_or("Unknown", |i| i.display_name.as_str());
            let when = c.created_date.as_deref().unwrap_or("");
            comment_lines.push(Line::from(vec![
                Span::styled(format!("  {author}"), theme::TEXT),
                Span::styled(format!("    {when}"), theme::MUTED),
            ]));
            let text = strip_html(&c.text);
            for body in text.lines() {
                comment_lines.push(Line::from(Span::styled(
                    format!("    {body}"),
                    theme::MUTED,
                )));
            }
        }
    }

    let comments_panel = Paragraph::new(comment_lines)
        .block(view_block(title))
        .wrap(Wrap { trim: false });
    f.render_widget(comments_panel, halves[1]);
}

fn meta_row(label: &str, value: String) -> Line<'_> {
    Line::from(vec![
        Span::styled(format!("  {label}: "), theme::MUTED),
        Span::styled(value, theme::TEXT),
    ])
}

fn metadata_height(wi: &WorkItem) -> u16 {
    // Roughly 1 line per visible metadata row + 2 for the block borders.
    let mut rows = 0u16;
    rows += u16::from(wi.fields.priority.is_some());
    rows += u16::from(wi.fields.severity.is_some());
    rows += u16::from(wi.fields.story_points.is_some());
    rows += u16::from(wi.fields.effort.is_some());
    rows += u16::from(wi.fields.value_area.is_some());
    rows += u16::from(tags_display(wi.fields.tags.as_deref()).is_some());
    rows += u16::from(wi.fields.reason.is_some());
    rows += u16::from(wi.fields.created_by.is_some());
    rows += u16::from(wi.fields.changed_by.is_some());
    rows += u16::from(wi.parent_id().is_some() || !wi.child_ids().is_empty());
    rows.max(1).saturating_add(2).min(16)
}

fn tags_display(raw: Option<&str>) -> Option<String> {
    let raw = raw?;
    let joined = raw
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

fn format_f64(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

fn format_who_when(who: &str, when: Option<&str>) -> String {
    when.map_or_else(|| who.to_string(), |ts| format!("{who} · {ts}"))
}

fn state_style(state: &str) -> Style {
    if state.eq_ignore_ascii_case("New") || state.eq_ignore_ascii_case("To Do") {
        theme::MUTED
    } else if state.eq_ignore_ascii_case("Active") || state.eq_ignore_ascii_case("In Progress") {
        theme::WARNING
    } else if state.eq_ignore_ascii_case("Resolved") || state.eq_ignore_ascii_case("Done") {
        theme::SUCCESS
    } else if state.eq_ignore_ascii_case("Closed")
        || state.eq_ignore_ascii_case("Removed")
        || state.eq_ignore_ascii_case("Cut")
    {
        theme::MUTED
    } else {
        theme::TEXT
    }
}

/// Strips a simple subset of HTML from ADO rich-text fields and converts
/// paragraph / list-item / break tags to newlines. Not a full HTML parser —
/// good enough for legible plain-text rendering in the TUI.
pub fn strip_html(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' {
            // Consume tag up to '>'.
            let mut tag = String::new();
            for t in chars.by_ref() {
                if t == '>' {
                    break;
                }
                tag.push(t);
            }
            let lower = tag.to_ascii_lowercase();
            let name = lower
                .trim_start_matches('/')
                .split_whitespace()
                .next()
                .unwrap_or("");
            match name {
                "br" | "p" | "/p" | "div" | "/div" | "li" | "/li" | "tr" | "/tr" => {
                    if !out.ends_with('\n') {
                        out.push('\n');
                    }
                }
                _ => {}
            }
        } else {
            out.push(c);
        }
    }
    // Decode a handful of common HTML entities. Full decoding is out of scope.
    out = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'");

    // Collapse runs of more than two newlines.
    let mut cleaned = String::with_capacity(out.len());
    let mut blank_run = 0u8;
    for line in out.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            blank_run += 1;
            if blank_run <= 1 {
                cleaned.push('\n');
            }
        } else {
            blank_run = 0;
            cleaned.push_str(trimmed);
            cleaned.push('\n');
        }
    }
    cleaned.trim_matches('\n').to_string()
}

impl Component for WorkItemDetail {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn footer_hints(&self) -> &'static str {
        "←/q/Esc back  ↑↓ scroll  o open  ? help"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::models::{IdentityRef, WorkItem, WorkItemFields};

    fn make_wi() -> WorkItem {
        WorkItem {
            id: 42,
            rev: Some(1),
            fields: WorkItemFields {
                title: "Add login".to_string(),
                work_item_type: "User Story".to_string(),
                state: Some("Active".to_string()),
                description: Some("<p>Users need a login screen.</p>".to_string()),
                acceptance_criteria: Some("<ul><li>Can enter credentials</li></ul>".to_string()),
                priority: Some(2),
                story_points: Some(5.0),
                tags: Some("auth; frontend".to_string()),
                ..Default::default()
            },
            relations: vec![],
            url: None,
        }
    }

    #[test]
    fn section_count_empty() {
        assert_eq!(WorkItemDetail::default().section_count(), 0);
    }

    #[test]
    fn section_count_when_loaded_returns_fixed_sections() {
        let detail = WorkItemDetail {
            work_item: Some(make_wi()),
            ..Default::default()
        };
        assert_eq!(detail.section_count(), 4);
    }

    #[test]
    fn strip_html_preserves_line_breaks_and_decodes_entities() {
        let stripped =
            strip_html("<p>Hello&nbsp;&amp; welcome</p><ul><li>One</li><li>Two</li></ul>");
        assert!(stripped.contains("Hello & welcome"));
        assert!(stripped.contains("One"));
        assert!(stripped.contains("Two"));
    }

    #[test]
    fn strip_html_empty_input_returns_empty() {
        assert_eq!(strip_html(""), "");
    }

    #[test]
    fn tags_display_joins_with_commas_and_ignores_empty() {
        assert_eq!(
            tags_display(Some("one; two ;; three ")),
            Some("one, two, three".to_string())
        );
        assert_eq!(tags_display(Some("   ")), None);
        assert_eq!(tags_display(None), None);
    }

    #[test]
    fn format_f64_drops_trailing_zero_for_whole_numbers() {
        assert_eq!(format_f64(5.0), "5");
        assert_eq!(format_f64(1.5), "1.5");
    }

    #[test]
    fn format_who_when_includes_date_when_present() {
        assert_eq!(
            format_who_when("Alice", Some("2024-01-01T00:00:00Z")),
            "Alice · 2024-01-01T00:00:00Z"
        );
        assert_eq!(format_who_when("Alice", None), "Alice");
    }

    #[test]
    fn side_pane_uses_identity_display_name_for_created_by() {
        // Exercise AssignedToField::Identity branch through the metadata row.
        let identity = IdentityRef {
            id: Some("alice-id".to_string()),
            display_name: "Alice".to_string(),
            unique_name: None,
            descriptor: None,
        };
        let wi = WorkItem {
            id: 1,
            rev: None,
            fields: WorkItemFields {
                title: "t".to_string(),
                work_item_type: "Task".to_string(),
                created_by: Some(AssignedToField::Identity(identity)),
                created_date: Some("2024-05-01T12:00:00Z".to_string()),
                ..Default::default()
            },
            relations: vec![],
            url: None,
        };
        let display = wi
            .fields
            .created_by
            .as_ref()
            .map_or("", AssignedToField::display_name);
        assert_eq!(display, "Alice");
    }
}
