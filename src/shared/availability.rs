//! Reusable availability states for independently refreshed data sections.

/// Represents the coarse availability status for a refreshed data section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvailabilityStatus {
    /// Indicates that the current data is fresh and complete.
    Fresh,
    /// Indicates that current data is usable but some sections reported errors.
    Partial,
    /// Indicates that last-known-good data is being shown after a refresh error.
    Stale,
    /// Indicates that no usable data is available.
    Unavailable,
}

/// Represents fresh, partial, stale, or unavailable data for a section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability<T> {
    Fresh(T),
    Partial { data: T, errors: Vec<String> },
    Stale { data: T, error: String },
    Unavailable { error: String },
}

impl<T> Availability<T> {
    /// Returns a fresh availability state for complete data.
    pub fn fresh(data: T) -> Self {
        Self::Fresh(data)
    }

    /// Returns a partial availability state, or fresh when no errors exist.
    pub fn partial(data: T, errors: Vec<String>) -> Self {
        if errors.is_empty() {
            Self::Fresh(data)
        } else {
            Self::Partial { data, errors }
        }
    }

    /// Returns a stale availability state for last-known-good data.
    pub fn stale(data: T, error: impl Into<String>) -> Self {
        Self::Stale {
            data,
            error: error.into(),
        }
    }

    /// Returns an unavailable availability state.
    pub fn unavailable(error: impl Into<String>) -> Self {
        Self::Unavailable {
            error: error.into(),
        }
    }

    /// Returns the current availability status.
    pub fn status(&self) -> AvailabilityStatus {
        match self {
            Self::Fresh(_) => AvailabilityStatus::Fresh,
            Self::Partial { .. } => AvailabilityStatus::Partial,
            Self::Stale { .. } => AvailabilityStatus::Stale,
            Self::Unavailable { .. } => AvailabilityStatus::Unavailable,
        }
    }

    /// Returns `true` when data is fresh and complete.
    pub fn is_fresh(&self) -> bool {
        self.status() == AvailabilityStatus::Fresh
    }

    /// Returns `true` when data exists, even if it is partial or stale.
    pub fn is_available(&self) -> bool {
        self.data().is_some()
    }

    /// Returns `true` when the state should be surfaced as degraded.
    pub fn is_degraded(&self) -> bool {
        !self.is_fresh()
    }

    /// Returns a shared reference to the usable data, if any exists.
    pub fn data(&self) -> Option<&T> {
        match self {
            Self::Fresh(data) | Self::Partial { data, .. } | Self::Stale { data, .. } => Some(data),
            Self::Unavailable { .. } => None,
        }
    }

    /// Returns the first error message associated with a degraded state.
    pub fn primary_error(&self) -> Option<&str> {
        match self {
            Self::Fresh(_) => None,
            Self::Partial { errors, .. } => errors.first().map(String::as_str),
            Self::Stale { error, .. } | Self::Unavailable { error } => Some(error),
        }
    }

    /// Returns partial-state errors, or an empty slice for other states.
    pub fn errors(&self) -> &[String] {
        match self {
            Self::Partial { errors, .. } => errors,
            _ => &[],
        }
    }
}

impl<T: Clone> Availability<T> {
    /// Returns stale data when data exists, or unavailable when it does not.
    #[must_use]
    pub fn stale_or_unavailable(&self, error: impl Into<String>) -> Self {
        let error = error.into();
        match self.data() {
            Some(data) => Self::stale(data.clone(), error),
            None => Self::unavailable(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_data_can_transition_to_partial() {
        let state = Availability::partial(vec![1, 2], vec!["approvals unavailable".to_string()]);

        assert_eq!(state.status(), AvailabilityStatus::Partial);
        assert_eq!(state.data(), Some(&vec![1, 2]));
        assert_eq!(state.primary_error(), Some("approvals unavailable"));
    }

    #[test]
    fn fresh_data_can_transition_to_stale() {
        let state = Availability::fresh(vec![42]).stale_or_unavailable("timeout");

        assert_eq!(state.status(), AvailabilityStatus::Stale);
        assert_eq!(state.data(), Some(&vec![42]));
        assert_eq!(state.primary_error(), Some("timeout"));
    }

    #[test]
    fn missing_data_transitions_to_unavailable() {
        let state = Availability::<Vec<i32>>::unavailable("not loaded")
            .stale_or_unavailable("network down");

        assert_eq!(state.status(), AvailabilityStatus::Unavailable);
        assert!(state.data().is_none());
        assert_eq!(state.primary_error(), Some("network down"));
    }
}
