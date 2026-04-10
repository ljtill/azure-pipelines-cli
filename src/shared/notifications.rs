// Re-export from the canonical location so callers can use shared::notifications
// while the implementation stays in app::notifications during migration.
pub use crate::app::notifications::{NotificationLevel, Notifications};
