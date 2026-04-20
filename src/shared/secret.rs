//! Newtype wrapper for sensitive string values whose `Debug`/`Display` impls redact the inner value.

use std::fmt;

/// Wraps a sensitive string value (token, credential) so its `Debug`
/// and `Display` impls redact the inner value.
///
/// Call [`SecretString::expose_secret`] only where the raw value must cross
/// an FFI or HTTP boundary — the explicit method name makes leak audits easy.
#[derive(Clone)]
pub struct SecretString(String);

impl SecretString {
    /// Wraps a value in a `SecretString`.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the raw secret. Call-sites using this name make review easier.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString([REDACTED])")
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl From<String> for SecretString {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for SecretString {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        // Best-effort wipe; not cryptographically guaranteed without the zeroize crate.
        // SAFETY: we overwrite the bytes the String owns via as_mut_vec; safe because
        // we do not read from it after, and the resulting bytes (all zero) remain
        // valid UTF-8.
        unsafe {
            let v = self.0.as_mut_vec();
            for b in v.iter_mut() {
                *b = 0;
            }
        }
    }
}

// Note: do NOT derive or implement `Serialize` for this type — we do not want
// secrets to accidentally round-trip through JSON logs or other serializers.

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &str = "super-secret-token-value-xyz";

    #[test]
    fn debug_format_redacts_value() {
        let s = SecretString::from(RAW);
        assert_eq!(format!("{s:?}"), "SecretString([REDACTED])");
    }

    #[test]
    fn display_format_redacts_value() {
        let s = SecretString::from(RAW);
        assert_eq!(format!("{s}"), "[REDACTED]");
    }

    #[test]
    fn expose_secret_returns_original_value() {
        let s = SecretString::new(RAW);
        assert_eq!(s.expose_secret(), RAW);
    }

    #[test]
    fn formatted_output_never_contains_raw_value() {
        let s = SecretString::from(RAW);
        let dbg = format!("{s:?}");
        let disp = format!("{s}");

        assert!(dbg.contains("REDACTED"));
        assert!(disp.contains("REDACTED"));
        assert!(!dbg.contains(RAW));
        assert!(!disp.contains(RAW));
    }

    #[test]
    fn from_string_and_str_both_work() {
        let a: SecretString = RAW.into();
        let b: SecretString = String::from(RAW).into();
        assert_eq!(a.expose_secret(), b.expose_secret());
    }
}
