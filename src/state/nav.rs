//! Reusable list-navigation state shared across views.

/// Stores reusable navigation state for a scrollable list.
#[derive(Debug, Default)]
pub struct ListNav {
    index: usize,
    len: usize,
}

impl ListNav {
    pub fn index(&self) -> usize {
        self.index
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.len
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Updates the list length and clamps the index.
    pub fn set_len(&mut self, len: usize) {
        self.len = len;
        self.clamp();
    }

    pub fn set_index(&mut self, index: usize) {
        self.index = index;
        self.clamp();
    }

    pub fn up(&mut self) {
        self.index = self.index.saturating_sub(1);
    }

    pub fn down(&mut self) {
        if self.len > 0 {
            self.index = (self.index + 1).min(self.len - 1);
        }
    }

    pub fn home(&mut self) {
        self.index = 0;
    }

    pub fn end(&mut self) {
        if self.len > 0 {
            self.index = self.len - 1;
        }
    }

    /// Returns whether the cursor is on the last item in the list.
    pub fn is_at_bottom(&self) -> bool {
        self.len > 0 && self.index == self.len - 1
    }

    #[allow(dead_code)]
    pub fn page_up(&mut self, page_size: usize) {
        self.index = self.index.saturating_sub(page_size);
    }

    #[allow(dead_code)]
    pub fn page_down(&mut self, page_size: usize) {
        if self.len > 0 {
            self.index = (self.index + page_size).min(self.len - 1);
        }
    }

    pub fn reset(&mut self) {
        self.index = 0;
        self.len = 0;
    }

    fn clamp(&mut self) {
        if self.len == 0 {
            self.index = 0;
        } else {
            self.index = self.index.min(self.len - 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_zero() {
        let nav = ListNav::default();
        assert_eq!(nav.index(), 0);
        assert_eq!(nav.len(), 0);
    }

    #[test]
    fn up_at_zero_stays() {
        let mut nav = ListNav::default();
        nav.set_len(5);
        nav.up();
        assert_eq!(nav.index(), 0);
    }

    #[test]
    fn down_advances() {
        let mut nav = ListNav::default();
        nav.set_len(5);
        nav.down();
        assert_eq!(nav.index(), 1);
    }

    #[test]
    fn down_clamps_at_end() {
        let mut nav = ListNav::default();
        nav.set_len(3);
        nav.down();
        nav.down();
        nav.down();
        nav.down();
        assert_eq!(nav.index(), 2);
    }

    #[test]
    fn home_and_end() {
        let mut nav = ListNav::default();
        nav.set_len(10);
        nav.end();
        assert_eq!(nav.index(), 9);
        nav.home();
        assert_eq!(nav.index(), 0);
    }

    #[test]
    fn set_len_clamps_index() {
        let mut nav = ListNav::default();
        nav.set_len(10);
        nav.set_index(8);
        nav.set_len(5);
        assert_eq!(nav.index(), 4);
    }

    #[test]
    fn empty_list_stays_zero() {
        let mut nav = ListNav::default();
        nav.down();
        assert_eq!(nav.index(), 0);
        nav.end();
        assert_eq!(nav.index(), 0);
    }

    #[test]
    fn reset_clears_both() {
        let mut nav = ListNav::default();
        nav.set_len(10);
        nav.set_index(5);
        nav.reset();
        assert_eq!(nav.index(), 0);
        assert_eq!(nav.len(), 0);
    }

    #[test]
    fn is_at_bottom_true_at_last_item() {
        let mut nav = ListNav::default();
        nav.set_len(5);
        nav.end();
        assert!(nav.is_at_bottom());
    }

    #[test]
    fn is_at_bottom_false_when_not_at_end() {
        let mut nav = ListNav::default();
        nav.set_len(5);
        nav.set_index(3);
        assert!(!nav.is_at_bottom());
    }

    #[test]
    fn is_at_bottom_false_when_empty() {
        let nav = ListNav::default();
        assert!(!nav.is_at_bottom());
    }
}
