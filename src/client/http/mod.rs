//! HTTP transport layer for the Azure DevOps REST API.
//!
//! Provides [`AdoClient`] with authenticated GET, POST, PATCH, and DELETE helpers,
//! automatic pagination via continuation tokens, and structured tracing for every
//! request/response cycle.

mod approvals;
mod boards;
mod builds;
mod definitions;
mod pull_requests;
mod retention;

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use futures::StreamExt;
use reqwest::{Client, RequestBuilder, StatusCode, Url};

use super::auth::AdoAuth;
use super::endpoints::Endpoints;
use super::models::ListResponse;

/// Typed error returned when Azure DevOps rejects the requested `api-version`.
///
/// Callers can downcast via [`anyhow::Error::downcast_ref`] to recognise the
/// specific failure and surface a remediation hint (e.g. prompt the user to
/// pass `--api-version` or set `DEVOPS_API_VERSION`).
#[derive(Debug, Clone)]
pub struct ApiVersionUnsupported {
    pub requested: String,
    pub url: String,
    pub server_message: String,
}

impl std::fmt::Display for ApiVersionUnsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Azure DevOps rejected api-version={} for {}: {}",
            self.requested, self.url, self.server_message
        )
    }
}

impl std::error::Error for ApiVersionUnsupported {}

/// Per-page pagination progress emitted by paginated fetchers.
#[derive(Debug, Clone, Copy)]
pub struct PaginationProgress {
    pub endpoint: &'static str,
    pub page: usize,
    pub items_so_far: usize,
}

/// Callback type passed to paginated fetchers to observe progress.
pub type PaginationProgressFn = dyn Fn(PaginationProgress) + Send + Sync;

/// Authenticated HTTP client for the Azure DevOps REST API.
#[derive(Clone)]
pub struct AdoClient {
    pub(crate) http: Client,
    pub(crate) auth: AdoAuth,
    pub(crate) endpoints: Endpoints,
}

impl AdoClient {
    /// Creates a new client configured for the given organization and project.
    pub fn new(organization: &str, project: &str) -> Result<Self> {
        let auth = AdoAuth::new()?;
        let http = Client::builder()
            .user_agent(concat!("devops/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()?;
        let endpoints = Endpoints::new(organization, project);

        Ok(Self {
            http,
            auth,
            endpoints,
        })
    }

    /// Creates a client pointed at an arbitrary base URL with a pre-seeded
    /// bearer token.
    ///
    /// Intended for integration tests that exercise the real request/retry
    /// loop against a local mock server (e.g. `wiremock`). The credential
    /// chain is never invoked. Hidden from the rendered docs.
    #[doc(hidden)]
    pub fn new_for_testing(base_url: &str) -> Result<Self> {
        let auth = AdoAuth::with_static_token("test-token")?;
        let http = Client::builder()
            .user_agent(concat!("devops/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()?;
        let endpoints = Endpoints::with_base_url(base_url, "testorg", "testproj");

        Ok(Self {
            http,
            auth,
            endpoints,
        })
    }

    /// Sends an authenticated GET request and deserializes the JSON response.
    pub(crate) async fn get<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let token = self.auth.token().await?;
        let display_url = url_without_query(url);
        let start = Instant::now();
        tracing::debug!(method = "GET", url = display_url, "api request");
        let resp = self
            .send_with_retry(
                || self.http.get(url).bearer_auth(token.expose_secret()),
                "GET",
            )
            .await?;
        let resp = self.ensure_success(resp, "GET", display_url).await?;
        let status = resp.status().as_u16();
        let bytes = read_json_capped(resp, display_url).await?;
        let body = serde_json::from_slice::<T>(&bytes)
            .with_context(|| format!("Failed to decode JSON response from {display_url}"))?;
        tracing::debug!(
            method = "GET",
            url = display_url,
            status,
            elapsed_ms = start.elapsed().as_millis() as u64,
            "api response"
        );
        Ok(body)
    }

    /// Sends a GET request and extracts the continuation token from the `x-ms-continuationtoken` header.
    pub(crate) async fn get_with_continuation<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
    ) -> Result<(T, Option<String>)> {
        let token = self.auth.token().await?;
        let display_url = url_without_query(url);
        let start = Instant::now();
        tracing::debug!(method = "GET", url = display_url, "api request (paged)");
        let resp = self
            .send_with_retry(
                || self.http.get(url).bearer_auth(token.expose_secret()),
                "GET",
            )
            .await?;
        let resp = self.ensure_success(resp, "GET", display_url).await?;
        let status = resp.status().as_u16();
        let continuation = resp
            .headers()
            .get("x-ms-continuationtoken")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty())
            .map(std::string::ToString::to_string);
        let bytes = read_json_capped(resp, display_url).await?;
        let body = serde_json::from_slice::<T>(&bytes)
            .with_context(|| format!("Failed to decode JSON response from {display_url}"))?;
        tracing::debug!(
            method = "GET",
            url = display_url,
            status,
            has_continuation = continuation.is_some(),
            elapsed_ms = start.elapsed().as_millis() as u64,
            "api response (paged)"
        );
        Ok((body, continuation))
    }

    /// Fetches all pages of a paginated list endpoint, invoking an optional
    /// progress callback once per fetched page.
    pub(crate) async fn get_all_pages_with_progress<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        endpoint: &'static str,
        progress: Option<&PaginationProgressFn>,
    ) -> Result<Vec<T>> {
        let max_pages = max_pages_cap();

        let mut all_items = Vec::new();
        let mut continuation_token: Option<String> = None;
        let mut page_count: usize = 0;
        let start = Instant::now();

        loop {
            if page_count >= max_pages {
                let display_url = url_without_query(url);
                anyhow::bail!(
                    "Pagination limit reached: fetched {max_pages} pages from {display_url}. \
                     If your organization has more data than this, set DEVOPS_MAX_PAGES \
                     (e.g., DEVOPS_MAX_PAGES=5000) or file an issue at \
                     https://github.com/ljtill/azure-devops-cli/issues."
                );
            }

            let full_url = paginated_url(url, continuation_token.as_deref())?;
            let display_url = url_without_query(full_url.as_str()).to_string();

            let token = self.auth.token().await?;
            tracing::debug!(method = "GET", url = %display_url, page = page_count + 1, "api paginated request");
            let full_url_str = full_url.as_str().to_string();
            let resp = self
                .send_with_retry(
                    || {
                        self.http
                            .get(full_url_str.as_str())
                            .bearer_auth(token.expose_secret())
                    },
                    "GET",
                )
                .await?;
            let resp = self.ensure_success(resp, "GET", &display_url).await?;

            let next_token = resp
                .headers()
                .get("x-ms-continuationtoken")
                .and_then(|v| v.to_str().ok())
                .map(std::string::ToString::to_string);

            let bytes = read_json_capped(resp, &display_url).await?;
            let page: ListResponse<T> = serde_json::from_slice(&bytes)
                .with_context(|| format!("Failed to decode JSON response from {display_url}"))?;
            all_items.extend(page.value);
            page_count += 1;

            if let Some(cb) = progress {
                emit_pagination_progress(cb, endpoint, page_count, all_items.len());
            }

            match next_token {
                Some(t) if !t.is_empty() => continuation_token = Some(t),
                _ => break,
            }
        }

        tracing::debug!(
            method = "GET",
            url = url_without_query(url),
            pages = page_count,
            total_items = all_items.len(),
            elapsed_ms = start.elapsed().as_millis() as u64,
            "api paginated complete"
        );
        Ok(all_items)
    }

    /// Sends an authenticated GET request and returns the response body as plain text.
    pub(crate) async fn get_text(&self, url: &str) -> Result<String> {
        let token = self.auth.token().await?;
        let display_url = url_without_query(url);
        let start = Instant::now();
        tracing::debug!(method = "GET", url = display_url, "api text request");
        let resp = self
            .send_with_retry(
                || self.http.get(url).bearer_auth(token.expose_secret()),
                "GET",
            )
            .await?;
        let resp = self.ensure_success(resp, "GET", display_url).await?;
        let status = resp.status().as_u16();
        let text = read_text_capped(resp).await?;
        tracing::debug!(
            method = "GET",
            url = display_url,
            status,
            bytes = text.len(),
            elapsed_ms = start.elapsed().as_millis() as u64,
            "api text response"
        );
        Ok(text)
    }

    /// Sends an authenticated PATCH request with a JSON body, discarding the response.
    pub(crate) async fn patch_json<B: serde::Serialize>(&self, url: &str, body: &B) -> Result<()> {
        let token = self.auth.token().await?;
        let display_url = url_without_query(url);
        let start = Instant::now();
        tracing::debug!(method = "PATCH", url = display_url, "api request");
        let resp = self
            .send_with_retry(
                || {
                    self.http
                        .patch(url)
                        .bearer_auth(token.expose_secret())
                        .json(body)
                },
                "PATCH",
            )
            .await?;
        let resp = self.ensure_success(resp, "PATCH", display_url).await?;
        let status = resp.status().as_u16();
        tracing::debug!(
            method = "PATCH",
            url = display_url,
            status,
            elapsed_ms = start.elapsed().as_millis() as u64,
            "api response"
        );
        Ok(())
    }

    /// Sends an authenticated POST request with a JSON body and deserializes the response.
    pub(crate) async fn post_json<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        body: &B,
    ) -> Result<T> {
        let token = self.auth.token().await?;
        let display_url = url_without_query(url);
        let start = Instant::now();
        tracing::debug!(method = "POST", url = display_url, "api request");
        let resp = self
            .send_with_retry(
                || {
                    self.http
                        .post(url)
                        .bearer_auth(token.expose_secret())
                        .json(body)
                },
                "POST",
            )
            .await?;
        let resp = self.ensure_success(resp, "POST", display_url).await?;
        let status = resp.status().as_u16();
        let bytes = read_json_capped(resp, display_url).await?;
        let body = serde_json::from_slice::<T>(&bytes)
            .with_context(|| format!("Failed to decode JSON response from {display_url}"))?;
        tracing::debug!(
            method = "POST",
            url = display_url,
            status,
            elapsed_ms = start.elapsed().as_millis() as u64,
            "api response"
        );
        Ok(body)
    }

    /// Sends an authenticated DELETE request, discarding the response.
    pub(crate) async fn delete(&self, url: &str) -> Result<()> {
        let token = self.auth.token().await?;
        let display_url = url_without_query(url);
        let start = Instant::now();
        tracing::debug!(method = "DELETE", url = display_url, "api request");
        let resp = self
            .send_with_retry(
                || self.http.delete(url).bearer_auth(token.expose_secret()),
                "DELETE",
            )
            .await?;
        let resp = self.ensure_success(resp, "DELETE", display_url).await?;
        let status = resp.status().as_u16();
        tracing::debug!(
            method = "DELETE",
            url = display_url,
            status,
            elapsed_ms = start.elapsed().as_millis() as u64,
            "api response"
        );
        Ok(())
    }

    /// Validates that a response returned a success status. On failure, reads
    /// the body, checks for Azure DevOps' `api-version` rejection signature,
    /// and returns a typed [`ApiVersionUnsupported`] error when detected.
    /// Otherwise returns a generic formatted HTTP status error.
    pub(crate) async fn ensure_success(
        &self,
        response: reqwest::Response,
        method: &'static str,
        display_url: &str,
    ) -> Result<reqwest::Response> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }

        let body = response.text().await.unwrap_or_default();
        let requested = self.endpoints.api_version.as_ref();

        if let Some(err) = detect_api_version_unsupported(&body, requested, display_url) {
            tracing::warn!(
                method,
                url = display_url,
                status = status.as_u16(),
                requested_api_version = requested,
                server_message = %err.server_message,
                "api-version unsupported"
            );
            return Err(anyhow::Error::new(err));
        }

        let error = format_http_status_error(status, &body);
        if let Some(body_preview) = summarize_error_body(&body) {
            tracing::warn!(
                method,
                url = display_url,
                status = status.as_u16(),
                body = body_preview,
                "api error response"
            );
        } else {
            tracing::warn!(
                method,
                url = display_url,
                status = status.as_u16(),
                "api error response"
            );
        }

        Err(anyhow::anyhow!(error))
    }

    /// Sends an HTTP request with retry on transient failures.
    ///
    /// Retries up to [`MAX_RETRIES`] times on HTTP `429`/`503` (honouring the
    /// `Retry-After` header when present) and on `reqwest` timeout/connect
    /// errors. Non-retryable responses (including other 5xx statuses) are
    /// returned unchanged so that [`AdoClient::ensure_success`] can produce
    /// the usual diagnostic error.
    async fn send_with_retry<F>(
        &self,
        make_request: F,
        operation_label: &str,
    ) -> reqwest::Result<reqwest::Response>
    where
        F: Fn() -> RequestBuilder,
    {
        let cap = Duration::from_secs(RETRY_CAP_SECS);
        let mut attempt: u32 = 0;
        loop {
            match make_request().send().await {
                Ok(resp) => {
                    let status = resp.status();
                    let retryable_status = status == StatusCode::TOO_MANY_REQUESTS
                        || status == StatusCode::SERVICE_UNAVAILABLE;
                    if retryable_status && attempt < MAX_RETRIES {
                        let retry_after = resp
                            .headers()
                            .get(reqwest::header::RETRY_AFTER)
                            .and_then(|v| v.to_str().ok())
                            .and_then(parse_retry_after);
                        let (sleep_for, source) = retry_after.map_or_else(
                            || (compute_backoff(attempt), "backoff"),
                            |d| (d.min(cap), "retry-after"),
                        );
                        tracing::debug!(
                            operation = operation_label,
                            attempt = attempt + 1,
                            status = status.as_u16(),
                            source,
                            sleep_ms = sleep_for.as_millis() as u64,
                            "retrying HTTP request"
                        );
                        tokio::time::sleep(sleep_for).await;
                        attempt += 1;
                        continue;
                    }
                    return Ok(resp);
                }
                Err(err) => {
                    let transient = err.is_timeout() || err.is_connect();
                    if transient && attempt < MAX_RETRIES {
                        let sleep_for = compute_backoff(attempt);
                        tracing::debug!(
                            operation = operation_label,
                            attempt = attempt + 1,
                            error = %err,
                            sleep_ms = sleep_for.as_millis() as u64,
                            "retrying transient network error"
                        );
                        tokio::time::sleep(sleep_for).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(err);
                }
            }
        }
    }
}

// --- Retry Configuration ---

const MAX_RETRIES: u32 = 3;
const RETRY_BASE_MS: u64 = 500;
const RETRY_CAP_SECS: u64 = 30;
const RETRY_JITTER_LOW: f64 = 0.8;
const RETRY_JITTER_SPAN: f64 = 0.4;

// --- Body Size Caps ---

const MAX_JSON_BYTES: u64 = 32 * 1024 * 1024;
const MAX_TEXT_BYTES: u64 = 128 * 1024 * 1024;

const MAX_ERROR_BODY_CHARS: usize = 300;

/// Parses an HTTP `Retry-After` header value into a `Duration`.
///
/// Accepts either an integer number of seconds (RFC 7231 delta-seconds) or
/// an HTTP-date. For HTTP-date values the delta is computed against
/// `chrono::Utc::now()`; past dates yield `Duration::ZERO`.
fn parse_retry_after(value: &str) -> Option<Duration> {
    let trimmed = value.trim();
    if let Ok(secs) = trimmed.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(trimmed) {
        let delta = dt
            .with_timezone(&chrono::Utc)
            .signed_duration_since(chrono::Utc::now());
        return Some(delta.to_std().unwrap_or(Duration::ZERO));
    }
    None
}

/// Computes an exponential backoff duration with ±20% jitter, capped at
/// [`RETRY_CAP_SECS`]. `attempt` is 0-based (first retry uses `attempt=0`).
fn compute_backoff(attempt: u32) -> Duration {
    let cap_ms = RETRY_CAP_SECS * 1000;
    let shift = attempt.min(16);
    let base_ms = RETRY_BASE_MS.saturating_mul(1u64 << shift).min(cap_ms);
    let jitter01 = f64::from(jitter_seed()) / f64::from(u32::MAX);
    let multiplier = RETRY_JITTER_LOW + RETRY_JITTER_SPAN * jitter01;
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let scaled = ((base_ms as f64) * multiplier) as u64;
    Duration::from_millis(scaled.min(cap_ms))
}

/// Returns a pseudo-random `u32` seeded from the current system time.
///
/// Good enough for retry jitter; not cryptographically secure. Avoids
/// pulling in `rand` as a new dependency.
fn jitter_seed() -> u32 {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(nanos ^ counter);
    hasher.finish() as u32
}

/// Reads a response body into memory, enforcing [`MAX_JSON_BYTES`].
///
/// Fails fast when the advertised `Content-Length` exceeds the cap; otherwise
/// streams chunks and errors out as soon as the cumulative size crosses the
/// threshold.
async fn read_json_capped(resp: reqwest::Response, display_url: &str) -> Result<Vec<u8>> {
    let limit = MAX_JSON_BYTES;
    if let Some(cl) = resp.content_length()
        && cl > limit
    {
        anyhow::bail!(
            "Response body too large for {display_url}:                  Content-Length {cl} bytes exceeds {limit}-byte JSON cap"
        );
    }
    let mut buf: Vec<u8> = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.with_context(|| format!("Error reading response body from {display_url}"))?;
        if (buf.len() as u64).saturating_add(chunk.len() as u64) > limit {
            anyhow::bail!(
                "Response body too large for {display_url}:                  exceeded {limit}-byte JSON cap while streaming"
            );
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

/// Reads a response body as text, enforcing [`MAX_TEXT_BYTES`].
///
/// On overflow returns the accumulated prefix (lossy UTF-8 decoded) with a
/// trailing truncation marker indicating the cap.
async fn read_text_capped(resp: reqwest::Response) -> Result<String> {
    let limit = MAX_TEXT_BYTES;
    let limit_mib = limit / (1024 * 1024);
    let marker = format!("\n… log truncated at {limit_mib} MiB, open in browser\n");

    let pre_exceeds = resp.content_length().is_some_and(|cl| cl > limit);
    let mut buf: Vec<u8> = Vec::new();
    let mut truncated = pre_exceeds;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Error reading text response body")?;
        let remaining = limit.saturating_sub(buf.len() as u64);
        if chunk.len() as u64 > remaining {
            let take = remaining as usize;
            buf.extend_from_slice(&chunk[..take]);
            truncated = true;
            break;
        }
        buf.extend_from_slice(&chunk);
    }
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if truncated {
        text.push_str(&marker);
    }
    Ok(text)
}

/// Invokes the progress callback with the current page information.
///
/// Emits a `tracing::debug` record even when no callback is provided so the
/// pagination rhythm can be observed in logs.
fn emit_pagination_progress<'a>(
    progress: &(dyn Fn(PaginationProgress) + Send + Sync + 'a),
    endpoint: &'static str,
    page: usize,
    items_so_far: usize,
) {
    tracing::debug!(endpoint, page, items_so_far, "pagination progress");
    progress(PaginationProgress {
        endpoint,
        page,
        items_so_far,
    });
}

/// Detects the Azure DevOps "API version not supported" error signature in an
/// HTTP error body.
///
/// Returns `Some(ApiVersionUnsupported)` when either:
/// - the JSON body contains `typeKey = "VersionNotSupportedException"`, or
/// - the body (JSON or plain text) contains both `api version` and
///   `not supported` substrings (case-insensitive).
///
/// Extracts `server_message` from the JSON `message` field when present; falls
/// back to a truncated flattened view of the body.
pub(crate) fn detect_api_version_unsupported(
    body: &str,
    requested: &str,
    url: &str,
) -> Option<ApiVersionUnsupported> {
    let parsed_json = serde_json::from_str::<serde_json::Value>(body).ok();

    let type_key_matches = parsed_json
        .as_ref()
        .and_then(|v| v.get("typeKey"))
        .and_then(|v| v.as_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("VersionNotSupportedException"));

    let body_lower = body.to_ascii_lowercase();
    let substring_matches =
        body_lower.contains("api version") && body_lower.contains("not supported");

    if !type_key_matches && !substring_matches {
        return None;
    }

    let server_message = parsed_json
        .as_ref()
        .and_then(|v| v.get("message"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| summarize_error_body(body))
        .unwrap_or_else(|| "API version is not supported for this endpoint.".to_string());

    Some(ApiVersionUnsupported {
        requested: requested.to_string(),
        url: url.to_string(),
        server_message,
    })
}

fn format_http_status_error(status: StatusCode, body: &str) -> String {
    let category = if status.is_client_error() {
        "client error"
    } else if status.is_server_error() {
        "server error"
    } else {
        "error"
    };
    let base = format!("HTTP status {category} ({status})");

    if let Some(preview) = summarize_error_body(body) {
        format!("{base}: {preview}")
    } else {
        base
    }
}

fn summarize_error_body(body: &str) -> Option<String> {
    let flattened = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = flattened.trim();
    if trimmed.is_empty() {
        return None;
    }

    let truncated: String = trimmed.chars().take(MAX_ERROR_BODY_CHARS).collect();
    if trimmed.chars().count() > MAX_ERROR_BODY_CHARS {
        Some(format!("{truncated}…"))
    } else {
        Some(truncated)
    }
}

/// Appends a continuation token query parameter to a base URL.
fn paginated_url(base_url: &str, continuation_token: Option<&str>) -> Result<Url> {
    let mut url =
        Url::parse(base_url).with_context(|| format!("Failed to parse URL: {base_url}"))?;
    if let Some(token) = continuation_token {
        url.query_pairs_mut()
            .append_pair("continuationToken", token);
    }
    Ok(url)
}

/// Returns the URL portion before the query string for logging.
fn url_without_query(url: &str) -> &str {
    url.split('?').next().unwrap_or(url)
}

/// Returns the maximum number of pages to follow during pagination.
///
/// Read once from the `DEVOPS_MAX_PAGES` environment variable at first call
/// and cached for the process lifetime. Falls back to 1000 on missing or
/// unparseable values. Values below 100 are silently clamped up to 100 to
/// preserve a usable floor.
fn max_pages_cap() -> usize {
    use std::sync::OnceLock;
    static CAP: OnceLock<usize> = OnceLock::new();
    *CAP.get_or_init(|| {
        const DEFAULT_MAX_PAGES: usize = 1000;
        const MIN_MAX_PAGES: usize = 100;
        let raw = std::env::var("DEVOPS_MAX_PAGES").unwrap_or_default();
        let parsed = raw.parse::<usize>().unwrap_or(DEFAULT_MAX_PAGES);
        parsed.max(MIN_MAX_PAGES)
    })
}

/// Percent-encodes a continuation token for safe inclusion in a URL query string.
pub(crate) fn encode_continuation_token(token: &str) -> String {
    let dummy = format!("https://x?t={token}");
    Url::parse(&dummy).map_or_else(
        |_| token.to_string(),
        |url| {
            url.query_pairs()
                .find(|(k, _)| k == "t")
                .map_or_else(|| token.to_string(), |(_, v)| v.into_owned())
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paginated_url_preserves_existing_query_params() {
        let url = paginated_url(
            "https://example.test/builds?api-version=7.1&$top=100",
            Some("page-2"),
        )
        .unwrap();

        let query: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        assert_eq!(
            query,
            vec![
                ("api-version".into(), "7.1".into()),
                ("$top".into(), "100".into()),
                ("continuationToken".into(), "page-2".into()),
            ]
        );
    }

    #[test]
    fn paginated_url_percent_encodes_opaque_tokens() {
        let url = paginated_url(
            "https://example.test/builds?api-version=7.1",
            Some("abc+/=?&value"),
        )
        .unwrap();

        assert_eq!(
            url.as_str(),
            "https://example.test/builds?api-version=7.1&continuationToken=abc%2B%2F%3D%3F%26value"
        );
    }

    #[test]
    fn summarize_error_body_flattens_and_truncates() {
        let input = format!("  {{\n  \"message\": \"{}\"\n}}  ", "x".repeat(350));
        let summary = summarize_error_body(&input).unwrap();

        assert!(summary.starts_with("{ \"message\": \""));
        assert!(summary.ends_with('…'));
        assert_eq!(summary.chars().count(), MAX_ERROR_BODY_CHARS + 1);
    }

    #[test]
    fn format_http_status_error_includes_body_preview() {
        let message = format_http_status_error(
            StatusCode::BAD_REQUEST,
            "{\n  \"message\": \"Field 'Foo' is invalid.\"\n}",
        );

        assert_eq!(
            message,
            "HTTP status client error (400 Bad Request): { \"message\": \"Field 'Foo' is invalid.\" }"
        );
    }

    #[test]
    fn detect_api_version_unsupported_json_shape() {
        let body = r#"{"$id":"1","message":"The API version '7.1' is not supported for this endpoint.","typeKey":"VersionNotSupportedException"}"#;
        let err = detect_api_version_unsupported(body, "7.1", "https://example.test/x")
            .expect("should detect JSON VersionNotSupportedException");

        assert_eq!(err.requested, "7.1");
        assert_eq!(err.url, "https://example.test/x");
        assert_eq!(
            err.server_message,
            "The API version '7.1' is not supported for this endpoint."
        );
    }

    #[test]
    fn detect_api_version_unsupported_plain_text() {
        // No typeKey, but both trigger substrings present.
        let body = "The API version 7.1 is not supported.";
        let err = detect_api_version_unsupported(body, "7.1", "https://example.test/x")
            .expect("should detect plain text variant");

        assert_eq!(err.requested, "7.1");
        assert_eq!(err.server_message, "The API version 7.1 is not supported.");
    }

    #[test]
    fn detect_api_version_unsupported_mixed_case_substrings() {
        // Matching must be case-insensitive.
        let body = "Error: Api Version is Not Supported for this resource.";
        assert!(
            detect_api_version_unsupported(body, "7.1", "https://example.test/x").is_some(),
            "case-insensitive substring match should succeed"
        );
    }

    #[test]
    fn detect_api_version_unsupported_ignores_normal_error() {
        let body = r#"{"$id":"1","message":"The resource cannot be found.","typeKey":"ResourceNotFoundException"}"#;
        assert!(
            detect_api_version_unsupported(body, "7.1", "https://example.test/x").is_none(),
            "a regular not-found error must not be misdetected"
        );

        let body_plain = "Something went wrong.";
        assert!(
            detect_api_version_unsupported(body_plain, "7.1", "https://example.test/x").is_none()
        );
    }

    #[test]
    fn emit_pagination_progress_invokes_callback() {
        use std::sync::Mutex;
        let calls: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());
        let cb = |p: PaginationProgress| {
            assert_eq!(p.endpoint, "test_endpoint");
            calls.lock().unwrap().push((p.page, p.items_so_far));
        };

        emit_pagination_progress(&cb, "test_endpoint", 1, 25);
        emit_pagination_progress(&cb, "test_endpoint", 2, 57);
        emit_pagination_progress(&cb, "test_endpoint", 3, 80);

        let calls = calls.into_inner().unwrap();
        assert_eq!(calls, vec![(1, 25), (2, 57), (3, 80)]);
    }

    #[test]
    fn parse_retry_after_delta_seconds() {
        assert_eq!(parse_retry_after("0"), Some(Duration::from_secs(0)));
        assert_eq!(parse_retry_after("3"), Some(Duration::from_secs(3)));
        assert_eq!(parse_retry_after("  120 "), Some(Duration::from_mins(2)));
    }

    #[test]
    fn parse_retry_after_http_date_future() {
        let future = chrono::Utc::now() + chrono::Duration::seconds(30);
        let header = future.to_rfc2822();
        let got = parse_retry_after(&header).expect("future date should parse");
        assert!(
            got <= Duration::from_secs(31) && got >= Duration::from_secs(25),
            "expected ~30s delay, got {got:?}"
        );
    }

    #[test]
    fn parse_retry_after_http_date_past_clamps_to_zero() {
        let past = chrono::Utc::now() - chrono::Duration::seconds(60);
        let header = past.to_rfc2822();
        assert_eq!(parse_retry_after(&header), Some(Duration::ZERO));
    }

    #[test]
    fn parse_retry_after_invalid_returns_none() {
        assert!(parse_retry_after("not-a-date").is_none());
        assert!(parse_retry_after("").is_none());
    }

    #[test]
    fn compute_backoff_respects_cap() {
        let d = compute_backoff(20);
        assert!(
            d <= Duration::from_secs(RETRY_CAP_SECS),
            "backoff {d:?} exceeded cap"
        );
    }

    #[test]
    fn compute_backoff_scales_exponentially() {
        let a0 = compute_backoff(0);
        let a3 = compute_backoff(3);
        assert!(a0 < Duration::from_millis(700));
        assert!(a3 >= Duration::from_secs(3));
    }
}
