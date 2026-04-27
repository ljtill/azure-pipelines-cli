//! Declarative column layout primitive for list-style views.
//!
//! Views declare a `Vec<Column>`; `resolve_widths` allocates fixed widths
//! first, then distributes the remainder across flex columns by weight
//! (respecting per-column `min`/`max` clamps). `render_header` draws a
//! muted header row above the list body and returns the remaining rect.

use std::ops::Range;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::helpers::{display_width, truncate};
use super::theme;

/// Defines the default number of rows preserved around the selected list item.
pub const DEFAULT_SCROLL_PADDING: usize = 3;

/// Represents the visible slice of a virtualized list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleRows {
    /// Stores the first absolute row index to render.
    pub start: usize,
    /// Stores the exclusive absolute row index after the last row to render.
    pub end: usize,
    /// Stores the selected row index relative to this window.
    pub selected: Option<usize>,
}

impl VisibleRows {
    /// Returns the absolute row range to render.
    pub fn range(self) -> Range<usize> {
        self.start..self.end
    }

    /// Converts an absolute row index into the visible window's local index.
    pub fn local_index(self, absolute_index: usize) -> Option<usize> {
        (self.start..self.end)
            .contains(&absolute_index)
            .then(|| absolute_index - self.start)
    }
}

/// Returns the visible row window for a list viewport.
pub fn visible_rows(
    total_len: usize,
    selected_index: usize,
    viewport_height: u16,
    scroll_padding: usize,
) -> VisibleRows {
    if total_len == 0 || viewport_height == 0 {
        return VisibleRows {
            start: 0,
            end: 0,
            selected: None,
        };
    }

    let height = usize::from(viewport_height).min(total_len);
    let selected = selected_index.min(total_len - 1);
    let padding = scroll_padding.min(height.saturating_sub(1));
    let mut start = selected.saturating_sub(height.saturating_sub(1).saturating_sub(padding));

    if selected < start.saturating_add(padding) {
        start = selected.saturating_sub(padding);
    }

    let max_start = total_len - height;
    start = start.min(max_start);
    let end = start + height;

    VisibleRows {
        start,
        end,
        selected: Some(selected - start),
    }
}

/// Horizontal alignment for a column's cell contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

/// Width policy for a single column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnWidth {
    /// Fixed width; the column always receives exactly this many cells.
    Fixed(u16),
    /// Flexible width; `weight` controls share of remaining space,
    /// clamped to `[min, max]` (if `max` is `Some`).
    Flex {
        weight: u16,
        min: u16,
        max: Option<u16>,
    },
}

/// A single column declaration used by `resolve_widths` / `render_header`.
#[derive(Debug, Clone, Copy)]
pub struct Column {
    /// Header label; empty string means no header text for this cell.
    pub label: &'static str,
    pub width: ColumnWidth,
    pub align: Align,
}

impl Column {
    pub const fn fixed(label: &'static str, width: u16, align: Align) -> Self {
        Self {
            label,
            width: ColumnWidth::Fixed(width),
            align,
        }
    }

    pub const fn flex(label: &'static str, weight: u16, min: u16, max: Option<u16>) -> Self {
        Self {
            label,
            width: ColumnWidth::Flex { weight, min, max },
            align: Align::Left,
        }
    }

    pub const fn flex_right(label: &'static str, weight: u16, min: u16, max: Option<u16>) -> Self {
        Self {
            label,
            width: ColumnWidth::Flex { weight, min, max },
            align: Align::Right,
        }
    }
}

/// Resolves column widths for the given total area width.
///
/// Algorithm:
/// 1. Sum the fixed columns. If they already meet or exceed `total_width`,
///    every flex column collapses to its `min` (accepting overflow —
///    rendering will clip naturally at the area edge).
/// 2. Allocate each flex column its `min`.
/// 3. Distribute the remaining space across flex columns proportional to
///    `weight`, clamped to `max` (if set).
/// 4. Any leftover after all flex caps are hit is appended to the last
///    un-capped flex column (or, if all are capped, dropped — the row
///    simply has trailing whitespace).
pub fn resolve_widths(cols: &[Column], total_width: u16) -> Vec<u16> {
    let mut widths = vec![0u16; cols.len()];
    let mut fixed_sum: u32 = 0;
    let mut flex_min_sum: u32 = 0;
    let mut flex_weight_sum: u32 = 0;

    for (i, col) in cols.iter().enumerate() {
        match col.width {
            ColumnWidth::Fixed(w) => {
                widths[i] = w;
                fixed_sum += u32::from(w);
            }
            ColumnWidth::Flex { weight, min, .. } => {
                widths[i] = min;
                flex_min_sum += u32::from(min);
                flex_weight_sum += u32::from(weight);
            }
        }
    }

    let reserved = fixed_sum + flex_min_sum;
    let total = u32::from(total_width);
    if reserved >= total || flex_weight_sum == 0 {
        return widths;
    }

    let mut remaining = total - reserved;

    // --- Proportional pass: each flex column gets a share by weight,
    // clamped to its (max - min) slack. Capped columns release their
    // unused share back for a later pass.
    let mut capped = vec![false; cols.len()];
    loop {
        let mut active_weight: u32 = 0;
        for (i, col) in cols.iter().enumerate() {
            if let ColumnWidth::Flex { weight, .. } = col.width
                && !capped[i]
            {
                active_weight += u32::from(weight);
            }
        }
        if active_weight == 0 || remaining == 0 {
            break;
        }

        let mut any_capped_this_pass = false;
        let mut distributed: u32 = 0;
        // Snapshot remaining so each column in this pass divides the same pool.
        let pool = remaining;
        for (i, col) in cols.iter().enumerate() {
            if capped[i] {
                continue;
            }
            if let ColumnWidth::Flex { weight, min, max } = col.width {
                let share = (pool * u32::from(weight)) / active_weight;
                let slack = max.map_or(u32::MAX, |m| u32::from(m.saturating_sub(min)));
                let take = share.min(slack);
                widths[i] += u16::try_from(take).unwrap_or(u16::MAX);
                distributed += take;
                if take == slack && max.is_some() {
                    capped[i] = true;
                    any_capped_this_pass = true;
                }
            }
        }

        remaining = remaining.saturating_sub(distributed);
        if !any_capped_this_pass {
            break;
        }
    }

    // --- Tail pass: any space still left from rounding goes to the first
    // active (un-capped, flex) column. This prevents 1–2 cells of lost
    // space when weights don't divide evenly.
    if remaining > 0 {
        for (i, col) in cols.iter().enumerate() {
            if capped[i] {
                continue;
            }
            if matches!(col.width, ColumnWidth::Flex { .. }) {
                widths[i] = widths[i].saturating_add(u16::try_from(remaining).unwrap_or(u16::MAX));
                break;
            }
        }
    }

    widths
}

/// Renders a single-line muted header row at the top of `area` and
/// returns the remaining rect below it for the list body.
///
/// If the area has fewer than 2 rows (header + at least one body row),
/// the header is skipped and the full area is returned unchanged.
pub fn render_header(f: &mut Frame, area: Rect, cols: &[Column]) -> Rect {
    if area.height < 2 {
        return area;
    }

    let widths = resolve_widths(cols, area.width);
    let header_rect = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };

    let spans: Vec<Span<'static>> = cols
        .iter()
        .zip(widths.iter())
        .map(|(col, &w)| header_cell(col.label, w, col.align))
        .collect();

    f.render_widget(
        Paragraph::new(Line::from(spans)).style(theme::TABLE_HEADER),
        header_rect,
    );

    Rect {
        x: area.x,
        y: area.y + 1,
        width: area.width,
        height: area.height - 1,
    }
}

/// Formats a header label into a padded cell of exactly `width` cells.
fn header_cell(label: &str, width: u16, align: Align) -> Span<'static> {
    Span::raw(format_cell(label, usize::from(width), align))
}

/// Formats text into a padded cell of exactly `width` terminal cells.
pub fn format_cell(text: &str, width: usize, align: Align) -> String {
    let truncated = truncate(text, width);
    let padding = " ".repeat(width.saturating_sub(display_width(&truncated)));
    match align {
        Align::Left => format!("{truncated}{padding}"),
        Align::Right => format!("{padding}{truncated}"),
    }
}

/// Formats a data cell of exactly `width` cells, truncating and padding.
pub fn row_cell(text: &str, width: u16, align: Align, style: Style) -> Span<'static> {
    Span::styled(format_cell(text, usize::from(width), align), style)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed(n: u16) -> Column {
        Column::fixed("", n, Align::Left)
    }
    fn flex(weight: u16, min: u16, max: Option<u16>) -> Column {
        Column::flex("", weight, min, max)
    }

    #[test]
    fn fixed_only_returns_fixed_widths() {
        let cols = [fixed(3), fixed(10), fixed(5)];
        let w = resolve_widths(&cols, 100);
        assert_eq!(w, vec![3, 10, 5]);
    }

    #[test]
    fn narrow_total_returns_mins() {
        // Fixed 10 + flex min 5 + flex min 5 = 20; total 15 < 20, so flex
        // columns get their mins and the row overflows (clipping handled
        // by ratatui).
        let cols = [fixed(10), flex(1, 5, None), flex(1, 5, None)];
        let w = resolve_widths(&cols, 15);
        assert_eq!(w, vec![10, 5, 5]);
    }

    #[test]
    fn wide_distributes_by_weight() {
        // Total 40, fixed 10, remainder 30 split 1:2 between two uncapped flex
        // columns (mins 0/0): first gets 10, second gets 20.
        let cols = [fixed(10), flex(1, 0, None), flex(2, 0, None)];
        let w = resolve_widths(&cols, 40);
        assert_eq!(w[0], 10);
        assert_eq!(w[1] + w[2], 30);
        assert_eq!(w[2], 2 * w[1]);
    }

    #[test]
    fn flex_max_caps_and_remainder_goes_to_uncapped() {
        // Total 100, fixed 20, flex cap 10, flex uncapped => cap honored,
        // uncapped absorbs rest.
        let cols = [fixed(20), flex(1, 0, Some(10)), flex(1, 0, None)];
        let w = resolve_widths(&cols, 100);
        assert_eq!(w[0], 20);
        assert_eq!(w[1], 10);
        assert_eq!(w[2], 70);
    }

    #[test]
    fn flex_mins_respected_with_cap() {
        // Min 12 should always be honored even when cap is higher.
        let cols = [flex(1, 12, Some(30))];
        let w = resolve_widths(&cols, 5);
        assert_eq!(w[0], 12); // min honored, overflow accepted
    }

    #[test]
    fn all_flex_caps_leaves_leftover_dropped() {
        // Fixed 10, two flex caps of 5 each, total 100 => 10 + 5 + 5 = 20
        // rendered; 80 cells of tail whitespace dropped (no uncapped column
        // to absorb). widths sum to 20.
        let cols = [fixed(10), flex(1, 0, Some(5)), flex(1, 0, Some(5))];
        let w = resolve_widths(&cols, 100);
        assert_eq!(w, vec![10, 5, 5]);
    }

    #[test]
    fn zero_flex_weight_acts_like_fixed_mins() {
        // A flex column with weight 0 never receives extra share.
        let cols = [flex(0, 5, Some(30)), flex(1, 0, None)];
        let w = resolve_widths(&cols, 50);
        assert_eq!(w[0], 5);
        assert_eq!(w[1], 45);
    }

    #[test]
    fn row_cell_left_pads() {
        let span = row_cell("hi", 5, Align::Left, Style::default());
        assert_eq!(&*span.content, "hi   ");
    }

    #[test]
    fn row_cell_right_pads() {
        let span = row_cell("hi", 5, Align::Right, Style::default());
        assert_eq!(&*span.content, "   hi");
    }

    #[test]
    fn row_cell_truncates_long_text() {
        let span = row_cell("abcdef", 3, Align::Left, Style::default());
        assert_eq!(&*span.content, "ab…");
    }

    #[test]
    fn row_cell_pads_wide_text_by_display_width() {
        let span = row_cell("デ", 4, Align::Left, Style::default());
        assert_eq!(&*span.content, "デ  ");
        assert_eq!(display_width(&span.content), 4);
    }

    #[test]
    fn row_cell_right_pads_wide_text_by_display_width() {
        let span = row_cell("デ", 4, Align::Right, Style::default());
        assert_eq!(&*span.content, "  デ");
        assert_eq!(display_width(&span.content), 4);
    }

    #[test]
    fn row_cell_truncates_wide_text_by_display_width() {
        let span = row_cell("デプロイ", 5, Align::Left, Style::default());
        assert_eq!(&*span.content, "デプ…");
        assert_eq!(display_width(&span.content), 5);
    }

    #[test]
    fn header_cell_uses_display_width() {
        let span = header_cell("デプロイ", 5, Align::Left);
        assert_eq!(&*span.content, "デプ…");
        assert_eq!(display_width(&span.content), 5);
    }

    #[test]
    fn row_cell_empty_width_produces_empty_span() {
        let span = row_cell("hi", 0, Align::Left, Style::default());
        assert_eq!(&*span.content, "");
    }

    #[test]
    fn visible_rows_empty_when_no_rows_or_height() {
        assert_eq!(
            visible_rows(0, 0, 10, DEFAULT_SCROLL_PADDING),
            VisibleRows {
                start: 0,
                end: 0,
                selected: None
            }
        );
        assert_eq!(
            visible_rows(10, 0, 0, DEFAULT_SCROLL_PADDING),
            VisibleRows {
                start: 0,
                end: 0,
                selected: None
            }
        );
    }

    #[test]
    fn visible_rows_keeps_selection_near_tail_visible() {
        let rows = visible_rows(5_000, 4_999, 12, DEFAULT_SCROLL_PADDING);
        assert_eq!(rows.end, 5_000);
        assert_eq!(rows.selected, Some(11));
        assert_eq!(rows.range().count(), 12);
    }

    #[test]
    fn visible_rows_keeps_selection_with_scroll_padding() {
        let rows = visible_rows(100, 20, 10, DEFAULT_SCROLL_PADDING);
        assert_eq!(
            rows,
            VisibleRows {
                start: 14,
                end: 24,
                selected: Some(6),
            }
        );
        assert_eq!(rows.local_index(20), Some(6));
        assert_eq!(rows.local_index(13), None);
    }
}
