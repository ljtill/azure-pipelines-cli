//! Feature reducers that apply async messages to application state.

pub(super) mod boards;
pub(super) mod dashboard;
pub(super) mod logs;
pub(super) mod pipelines;
pub(super) mod repos;
pub(super) mod updates;

pub(super) const DATA_REFRESH_BACKOFF_BASE_SECS: u64 = 30;
pub(super) const DATA_REFRESH_BACKOFF_MAX_SECS: u64 = 300;
pub(super) const LOG_REFRESH_BACKOFF_BASE_SECS: u64 = 5;
pub(super) const LOG_REFRESH_BACKOFF_MAX_SECS: u64 = 60;
