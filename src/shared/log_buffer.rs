//! A bounded, FIFO ring buffer for log lines.
//!
//! Wraps a `VecDeque<String>` with a hard capacity. When the cap is reached,
//! the oldest lines are dropped so the tail (most recent output) is always
//! preserved — matching the follow-mode expectation that the newest lines
//! are always visible.

use std::collections::VecDeque;

/// Minimum capacity accepted by `LogBuffer::new`. Values below this are
/// clamped up to avoid effectively-unusable buffers.
pub const MIN_CAPACITY: usize = 1_000;

/// Default capacity for log buffers when no explicit cap is configured.
pub const DEFAULT_CAPACITY: usize = 100_000;

/// A FIFO ring buffer of log lines with a fixed capacity.
///
/// Insertion past `cap` drops the oldest line and increments a dropped-line
/// counter so callers can surface a truncation indicator to users.
#[derive(Debug, Clone)]
pub struct LogBuffer {
    lines: VecDeque<String>,
    cap: usize,
    dropped_from_front: u64,
}

impl LogBuffer {
    /// Creates a new ring buffer with the given capacity, clamped to
    /// [`MIN_CAPACITY`] to prevent pathologically small buffers.
    pub fn new(cap: usize) -> Self {
        let cap = cap.max(MIN_CAPACITY);
        Self {
            lines: VecDeque::with_capacity(cap.min(DEFAULT_CAPACITY)),
            cap,
            dropped_from_front: 0,
        }
    }

    /// Returns the number of lines currently held.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Returns `true` if the buffer holds no lines.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Returns the configured capacity (after clamping).
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// Returns the total number of lines dropped from the front since the
    /// last `clear()` or `replace_from_str()`. Used to render a truncation
    /// banner in the log viewer.
    pub fn dropped(&self) -> u64 {
        self.dropped_from_front
    }

    /// Returns an iterator over the live lines, oldest to newest.
    pub fn iter(&self) -> std::collections::vec_deque::Iter<'_, String> {
        self.lines.iter()
    }

    /// Returns the line at logical offset `idx` (0 = oldest currently held),
    /// or `None` if out of range.
    pub fn get(&self, idx: usize) -> Option<&String> {
        self.lines.get(idx)
    }
    /// Replaces the entire buffer with the lines split from `content`.
    ///
    /// If `content` contains more lines than `cap`, only the last `cap`
    /// lines are retained and `dropped_from_front` is set to the number of
    /// truncated lines. This is the common case for log viewer updates,
    /// which overwrite the whole buffer on each fetch.
    pub fn replace_from_str(&mut self, content: &str) {
        self.lines.clear();
        self.dropped_from_front = 0;

        let total = content.lines().count();
        if total > self.cap {
            let skip = total - self.cap;
            self.dropped_from_front = skip as u64;
            for line in content.lines().skip(skip) {
                self.lines.push_back(line.to_string());
            }
        } else {
            for line in content.lines() {
                self.lines.push_back(line.to_string());
            }
        }
    }

    /// Clears all held lines and resets the dropped counter.
    pub fn clear(&mut self) {
        self.lines.clear();
        self.dropped_from_front = 0;
    }
}

impl Default for LogBuffer {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

impl<'a> IntoIterator for &'a LogBuffer {
    type Item = &'a String;
    type IntoIter = std::collections::vec_deque::Iter<'a, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.lines.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_clamps_below_minimum() {
        let buf = LogBuffer::new(10);
        assert_eq!(buf.cap(), MIN_CAPACITY);
    }

    #[test]
    fn new_respects_explicit_capacity() {
        let buf = LogBuffer::new(50_000);
        assert_eq!(buf.cap(), 50_000);
    }

    #[test]
    fn replace_under_cap_keeps_all_lines() {
        let mut buf = LogBuffer::new(MIN_CAPACITY);
        buf.replace_from_str("a\nb\nc");
        assert_eq!(buf.len(), 3);
        assert_eq!(buf.dropped(), 0);
        let lines: Vec<&String> = buf.iter().collect();
        assert_eq!(lines[0], "a");
        assert_eq!(lines[2], "c");
    }

    #[test]
    fn replace_at_cap_keeps_exactly_cap() {
        let mut buf = LogBuffer::new(MIN_CAPACITY);
        let input = (0..MIN_CAPACITY)
            .map(|i| format!("line-{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        buf.replace_from_str(&input);
        assert_eq!(buf.len(), MIN_CAPACITY);
        assert_eq!(buf.dropped(), 0);
    }

    #[test]
    fn replace_over_cap_drops_oldest_and_preserves_tail() {
        let mut buf = LogBuffer::new(MIN_CAPACITY);
        let extra = 250;
        let total = MIN_CAPACITY + extra;
        let input = (0..total)
            .map(|i| format!("line-{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        buf.replace_from_str(&input);

        assert_eq!(buf.len(), MIN_CAPACITY);
        assert_eq!(buf.dropped(), extra as u64);

        // Oldest held line is line-{extra}; newest is line-{total-1}.
        assert_eq!(buf.get(0).unwrap(), &format!("line-{extra}"));
        assert_eq!(buf.iter().last().unwrap(), &format!("line-{}", total - 1));
    }

    #[test]
    fn replace_resets_dropped_counter() {
        let mut buf = LogBuffer::new(MIN_CAPACITY);
        let over = (0..MIN_CAPACITY + 5)
            .map(|i| format!("l{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        buf.replace_from_str(&over);
        assert_eq!(buf.dropped(), 5);

        buf.replace_from_str("a\nb");
        assert_eq!(buf.dropped(), 0);
        assert_eq!(buf.len(), 2);
    }

    #[test]
    fn clear_resets_state() {
        let mut buf = LogBuffer::new(MIN_CAPACITY);
        buf.replace_from_str("x\ny\nz");
        buf.clear();
        assert!(buf.is_empty());
        assert_eq!(buf.dropped(), 0);
    }

    #[test]
    fn empty_input_yields_empty_buffer() {
        let mut buf = LogBuffer::new(MIN_CAPACITY);
        buf.replace_from_str("");
        assert!(buf.is_empty());
        assert_eq!(buf.dropped(), 0);
    }
}
