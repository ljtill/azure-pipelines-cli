//! Auto-expiring notification queue for user-facing messages.

use std::collections::VecDeque;
use std::time::Instant;

/// Represents the severity level for a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationLevel {
    #[allow(dead_code)]
    Info,
    Success,
    Error,
}

/// Represents a timestamped user-facing notification.
#[derive(Debug, Clone)]
pub struct Notification {
    pub level: NotificationLevel,
    pub message: String,
    pub created_at: Instant,
    pub persistent: bool,
}

/// Manages an auto-expiring notification queue.
pub struct Notifications {
    queue: VecDeque<Notification>,
    ttl_secs: u64,
}

const MAX_QUEUE_SIZE: usize = 100;

impl Notifications {
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            queue: VecDeque::new(),
            ttl_secs,
        }
    }

    pub fn push(&mut self, level: NotificationLevel, message: impl Into<String>) {
        self.queue.push_back(Notification {
            level,
            message: message.into(),
            created_at: Instant::now(),
            persistent: false,
        });
        self.prune_overflow();
    }

    pub fn push_persistent(&mut self, level: NotificationLevel, message: impl Into<String>) {
        self.queue.push_back(Notification {
            level,
            message: message.into(),
            created_at: Instant::now(),
            persistent: true,
        });
        self.prune_overflow();
    }

    fn prune_overflow(&mut self) {
        while self.queue.len() > MAX_QUEUE_SIZE {
            self.queue.pop_front();
        }
    }

    #[allow(dead_code)]
    pub fn info(&mut self, message: impl Into<String>) {
        self.push(NotificationLevel::Info, message);
    }

    pub fn success(&mut self, message: impl Into<String>) {
        self.push(NotificationLevel::Success, message);
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.push(NotificationLevel::Error, message);
    }

    /// Pushes an error, but if the most recent notification has the same message,
    /// just refreshes its timestamp instead of adding a duplicate.
    pub fn error_dedup(&mut self, message: impl Into<String>) {
        let message = message.into();
        if let Some(last) = self.queue.back()
            && last.message == message
        {
            if let Some(last) = self.queue.back_mut() {
                last.created_at = Instant::now();
            }
            return;
        }
        self.push(NotificationLevel::Error, message);
    }

    /// Returns the most recent non-expired notification, pruning expired ones.
    #[allow(dead_code)]
    pub fn current(&mut self) -> Option<&Notification> {
        let cutoff = Instant::now() - std::time::Duration::from_secs(self.ttl_secs);
        // Remove expired (non-persistent) from the front.
        while self
            .queue
            .front()
            .is_some_and(|n| !n.persistent && n.created_at < cutoff)
        {
            self.queue.pop_front();
        }
        self.queue.back()
    }

    /// Returns a clone of the most recent non-expired notification (read-only).
    pub fn clone_current(&self) -> Option<Notification> {
        let cutoff = Instant::now() - std::time::Duration::from_secs(self.ttl_secs);
        self.queue
            .back()
            .filter(|n| n.persistent || n.created_at >= cutoff)
            .cloned()
    }

    /// Clears all notifications.
    pub fn clear(&mut self) {
        self.queue.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_current() {
        let mut n = Notifications::new(60);
        assert!(n.current().is_none());

        n.error("something broke");
        let current = n.current().unwrap();
        assert_eq!(current.level, NotificationLevel::Error);
        assert_eq!(current.message, "something broke");
    }

    #[test]
    fn most_recent_wins() {
        let mut n = Notifications::new(60);
        n.error("first");
        n.success("second");
        let current = n.current().unwrap();
        assert_eq!(current.level, NotificationLevel::Success);
        assert_eq!(current.message, "second");
    }

    #[test]
    fn clear_removes_all() {
        let mut n = Notifications::new(60);
        n.error("err");
        n.clear();
        assert!(n.current().is_none());
    }

    #[test]
    fn error_dedup_suppresses_identical() {
        let mut n = Notifications::new(60);
        n.error_dedup("network down");
        n.error_dedup("network down");
        // Only one notification should exist.
        assert_eq!(n.queue.len(), 1);
        assert_eq!(n.current().unwrap().message, "network down");
    }

    #[test]
    fn error_dedup_allows_different_messages() {
        let mut n = Notifications::new(60);
        n.error_dedup("network down");
        n.error_dedup("auth expired");
        assert_eq!(n.queue.len(), 2);
        assert_eq!(n.current().unwrap().message, "auth expired");
    }

    #[test]
    fn error_dedup_refreshes_timestamp() {
        let mut n = Notifications::new(60);
        n.error_dedup("network down");
        let first_time = n.queue.back().unwrap().created_at;
        // Spin briefly so Instant advances.
        std::thread::sleep(std::time::Duration::from_millis(5));
        n.error_dedup("network down");
        let second_time = n.queue.back().unwrap().created_at;
        assert!(second_time > first_time);
        assert_eq!(n.queue.len(), 1);
    }
}
