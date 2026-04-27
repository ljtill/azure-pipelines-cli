//! Internal implementation crate for the Azure DevOps terminal dashboard.
//!
//! This package is released as the `devops` CLI. It does not expose a stable
//! Rust library API; the modules below are public only so the package binary
//! and integration tests can share implementation code. Stable surfaces are
//! documented in `docs/stability.md`.

#[doc(hidden)]
pub mod client;
#[doc(hidden)]
pub mod components;
#[doc(hidden)]
pub mod config;
#[doc(hidden)]
pub mod events;
#[doc(hidden)]
pub mod render;
#[doc(hidden)]
pub mod shared;
#[doc(hidden)]
pub mod state;
#[doc(hidden)]
pub mod update;

/// Provides internal test factory functions for unit and integration tests.
///
/// Available in local test/debug builds, or explicitly through the
/// `internal-test-helpers` feature for release-profile test runs. Not a
/// stable downstream API.
#[cfg(any(test, debug_assertions, feature = "internal-test-helpers"))]
#[doc(hidden)]
pub mod test_helpers;
