use std::collections::VecDeque;
use std::time::Instant;

/// Severity level for a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationLevel {
    #[allow(dead_code)]
    Info,
    Success,
    Error,
}

/// A timestamped user-facing notification.
#[derive(Debug, Clone)]
pub struct Notification {
    pub level: NotificationLevel,
    pub message: String,
    pub created_at: Instant,
    pub persistent: bool,
}

/// Auto-expiring notification queue.
pub struct Notifications {
    queue: VecDeque<Notification>,
    ttl_secs: u64,
}

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
    }

    pub fn push_persistent(&mut self, level: NotificationLevel, message: impl Into<String>) {
        self.queue.push_back(Notification {
            level,
            message: message.into(),
            created_at: Instant::now(),
            persistent: true,
        });
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

    /// Return the most recent non-expired notification, pruning expired ones.
    #[allow(dead_code)]
    pub fn current(&mut self) -> Option<&Notification> {
        let cutoff = Instant::now() - std::time::Duration::from_secs(self.ttl_secs);
        // Remove expired (non-persistent) from the front
        while self
            .queue
            .front()
            .is_some_and(|n| !n.persistent && n.created_at < cutoff)
        {
            self.queue.pop_front();
        }
        self.queue.back()
    }

    /// Return a clone of the most recent non-expired notification (read-only).
    pub fn clone_current(&self) -> Option<Notification> {
        let cutoff = Instant::now() - std::time::Duration::from_secs(self.ttl_secs);
        self.queue
            .back()
            .filter(|n| n.persistent || n.created_at >= cutoff)
            .cloned()
    }

    /// Clear all notifications.
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
}
