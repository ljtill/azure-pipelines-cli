//! Typed error cases for the Azure DevOps client layer.

use std::time::Duration;

use reqwest::StatusCode;

/// Represents parsed throttling metadata from Azure DevOps response headers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RateLimitMetadata {
    /// Contains the parsed `Retry-After` delay when Azure DevOps provided one.
    pub retry_after: Option<Duration>,
    /// Contains the parsed `X-RateLimit-Limit` value when present.
    pub limit: Option<u64>,
    /// Contains the parsed `X-RateLimit-Remaining` value when present.
    pub remaining: Option<u64>,
    /// Contains the parsed `X-RateLimit-Reset` epoch-seconds value when present.
    pub reset_epoch_seconds: Option<u64>,
}

impl RateLimitMetadata {
    /// Returns `true` when no throttling metadata was present or parseable.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.retry_after.is_none()
            && self.limit.is_none()
            && self.remaining.is_none()
            && self.reset_epoch_seconds.is_none()
    }

    /// Returns a compact diagnostic summary suitable for logs and messages.
    #[must_use]
    pub fn diagnostic_summary(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(retry_after) = self.retry_after {
            parts.push(format!("retry after {}", format_duration(retry_after)));
        }
        if let Some(remaining) = self.remaining {
            parts.push(format!("remaining {remaining}"));
        }
        if let Some(limit) = self.limit {
            parts.push(format!("limit {limit}"));
        }
        if let Some(reset) = self.reset_epoch_seconds {
            parts.push(format!("reset epoch {reset}"));
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join(", "))
        }
    }
}

/// Represents the response body family guarded by a size cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    /// Represents JSON API response bodies.
    Json,
    /// Represents plain-text response bodies.
    Text,
}

impl std::fmt::Display for BodyKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json => f.write_str("JSON"),
            Self::Text => f.write_str("text"),
        }
    }
}

/// Represents typed Azure DevOps client failures that callers can downcast and match.
#[derive(Debug)]
pub enum AdoError {
    /// Represents authentication failures from the local Azure credential chain.
    Auth {
        message: String,
        source: Option<String>,
    },
    /// Represents non-success HTTP responses other than recognized rate limits.
    HttpStatus {
        method: &'static str,
        url: String,
        status: StatusCode,
        body: Option<String>,
    },
    /// Represents Azure DevOps rate limiting after retry exhaustion.
    RateLimit {
        method: &'static str,
        url: String,
        status: StatusCode,
        metadata: RateLimitMetadata,
        body: Option<String>,
    },
    /// Represents an HTTP timeout after retry exhaustion.
    Timeout {
        method: &'static str,
        url: String,
        source: Box<reqwest::Error>,
    },
    /// Represents a JSON response that could not be decoded into the expected model.
    Decode {
        url: String,
        source: Box<serde_json::Error>,
    },
    /// Represents a response body that exceeded the configured in-memory cap.
    BodyCap {
        url: String,
        kind: BodyKind,
        limit_bytes: u64,
        actual_bytes: Option<u64>,
    },
    /// Represents Azure DevOps rejecting the requested REST API version.
    UnsupportedApiVersion {
        requested: String,
        url: String,
        server_message: String,
    },
    /// Represents an operation that stopped after receiving only partial data.
    PartialData {
        endpoint: &'static str,
        url: String,
        completed_pages: usize,
        items: usize,
        message: String,
    },
}

impl std::fmt::Display for AdoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auth { message, source } => {
                write!(f, "{message}")?;
                if let Some(source) = source {
                    write!(f, "\n\nUnderlying error: {source}")?;
                }
                Ok(())
            }
            Self::HttpStatus {
                method,
                url,
                status,
                body,
            } => {
                write!(
                    f,
                    "HTTP status {} ({status}) from {method} {url}",
                    status_category(*status)
                )?;
                if let Some(body) = body {
                    write!(f, ": {body}")?;
                }
                Ok(())
            }
            Self::RateLimit {
                method,
                url,
                status,
                metadata,
                body,
            } => {
                write!(f, "Azure DevOps rate limit ({status}) from {method} {url}")?;
                if let Some(summary) = metadata.diagnostic_summary() {
                    write!(f, "; {summary}")?;
                }
                if let Some(body) = body {
                    write!(f, ": {body}")?;
                }
                Ok(())
            }
            Self::Timeout {
                method,
                url,
                source,
            } => write!(
                f,
                "{method} request to {url} timed out after retries: {source}"
            ),
            Self::Decode { url, source } => {
                write!(f, "Failed to decode JSON response from {url}: {source}")
            }
            Self::BodyCap {
                url,
                kind,
                limit_bytes,
                actual_bytes,
            } => {
                write!(f, "Response body too large for {url}: ")?;
                match actual_bytes {
                    Some(actual_bytes) => {
                        write!(
                            f,
                            "Content-Length {actual_bytes} bytes exceeds {limit_bytes}-byte {kind} cap"
                        )
                    }
                    None => write!(f, "exceeded {limit_bytes}-byte {kind} cap while streaming"),
                }
            }
            Self::UnsupportedApiVersion {
                requested,
                url,
                server_message,
            } => write!(
                f,
                "Azure DevOps rejected api-version={requested} for {url}: {server_message}"
            ),
            Self::PartialData { message, .. } => f.write_str(message),
        }
    }
}

impl std::error::Error for AdoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Timeout { source, .. } => Some(source.as_ref()),
            Self::Decode { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

fn status_category(status: StatusCode) -> &'static str {
    if status.is_client_error() {
        "client error"
    } else if status.is_server_error() {
        "server error"
    } else {
        "error"
    }
}

fn format_duration(duration: Duration) -> String {
    if duration == Duration::ZERO {
        return "0s".to_string();
    }
    if duration.as_millis() < 1000 {
        return format!("{}ms", duration.as_millis());
    }
    format!("{}s", duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_error_preserves_login_hint_and_source() {
        let err = AdoError::Auth {
            message: "Authentication failed — ensure you are logged in with `az login` or `azd auth login`.".to_string(),
            source: Some("credential unavailable".to_string()),
        };

        let rendered = err.to_string();
        assert!(rendered.starts_with("Authentication failed"));
        assert!(rendered.contains("credential unavailable"));
    }

    #[test]
    fn body_kind_displays_human_readable_names() {
        assert_eq!(BodyKind::Json.to_string(), "JSON");
        assert_eq!(BodyKind::Text.to_string(), "text");
    }
}
