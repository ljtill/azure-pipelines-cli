//! Declarative column layout primitive for list-style views.
//!
//! Views declare a `Vec<Column>`; `resolve_widths` allocates fixed widths
//! first, then distributes the remainder across flex columns by weight
//! (respecting per-column `min`/`max` clamps). `render_header` draws a
//! muted header row above the list body and returns the remaining rect.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::theme;

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
        Paragraph::new(Line::from(spans)).style(theme::MUTED),
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
    let w = width as usize;
    let truncated: String = label.chars().take(w).collect();
    let content = match align {
        Align::Left => format!("{truncated:<w$}"),
        Align::Right => format!("{truncated:>w$}"),
    };
    Span::raw(content)
}

/// Formats a data cell of exactly `width` cells, truncating and padding.
/// Character-boundary safe; grapheme width is not accounted for (we render
/// ASCII + a small set of single-width icons in practice).
pub fn row_cell(text: &str, width: u16, align: Align, style: Style) -> Span<'static> {
    let w = width as usize;
    let truncated: String = text.chars().take(w).collect();
    let content = match align {
        Align::Left => format!("{truncated:<w$}"),
        Align::Right => format!("{truncated:>w$}"),
    };
    Span::styled(content, style)
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
        assert_eq!(&*span.content, "abc");
    }

    #[test]
    fn row_cell_empty_width_produces_empty_span() {
        let span = row_cell("hi", 0, Align::Left, Style::default());
        assert_eq!(&*span.content, "");
    }
}
