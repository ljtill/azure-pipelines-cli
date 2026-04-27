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

use std::future::Future;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use futures::StreamExt;
use reqwest::{Client, RequestBuilder, StatusCode, Url, header::HeaderMap};

use super::auth::AdoAuth;
use super::endpoints::Endpoints;
use super::errors::{AdoError, BodyKind, RateLimitMetadata};
use super::models::ListResponse;
use crate::config::ConnectionTimeoutConfig;

/// Per-page pagination progress emitted by paginated fetchers.
#[derive(Debug, Clone, Copy)]
pub struct PaginationProgress {
    pub endpoint: &'static str,
    pub page: usize,
    pub items_so_far: usize,
}

/// Callback type passed to paginated fetchers to observe progress.
pub type PaginationProgressFn = dyn Fn(PaginationProgress) + Send + Sync;

struct PaginationOptions<'a> {
    endpoint: &'static str,
    max_pages: usize,
    progress: Option<&'a PaginationProgressFn>,
}

impl<'a> PaginationOptions<'a> {
    fn new(endpoint: &'static str, progress: Option<&'a PaginationProgressFn>) -> Self {
        Self {
            endpoint,
            max_pages: max_pages_cap(),
            progress,
        }
    }

    #[cfg(test)]
    fn for_testing(
        endpoint: &'static str,
        max_pages: usize,
        progress: Option<&'a PaginationProgressFn>,
    ) -> Self {
        Self {
            endpoint,
            max_pages,
            progress,
        }
    }
}

/// Describes whether a request can be retried after a transient failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RequestRetryPolicy {
    /// Allows transient retries because replaying the request is safe.
    Idempotent,
    /// Disables transient retries because replaying the request may duplicate side effects.
    NonIdempotent,
}

impl RequestRetryPolicy {
    const fn retries_transient_failures(self) -> bool {
        matches!(self, Self::Idempotent)
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Idempotent => "idempotent",
            Self::NonIdempotent => "non-idempotent",
        }
    }
}

#[derive(Clone, Copy)]
enum HttpMethod {
    Get,
    Post,
    Patch,
    Delete,
}

impl HttpMethod {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
        }
    }

    fn request_builder(self, http: &Client, url: &str) -> RequestBuilder {
        match self {
            Self::Get => http.get(url),
            Self::Post => http.post(url),
            Self::Patch => http.patch(url),
            Self::Delete => http.delete(url),
        }
    }
}

struct TransportResponse {
    response: reqwest::Response,
    display_url: String,
}

/// Authenticated HTTP client for the Azure DevOps REST API.
#[derive(Clone)]
pub struct AdoClient {
    pub(crate) http: Client,
    pub(crate) auth: AdoAuth,
    pub(crate) endpoints: Endpoints,
    log_timeout: Duration,
}

impl AdoClient {
    /// Creates a new client configured for the given organization and project.
    pub fn new(organization: &str, project: &str) -> Result<Self> {
        Self::new_with_timeouts(organization, project, ConnectionTimeoutConfig::default())
    }

    /// Creates a new client with explicit timeout settings.
    pub fn new_with_timeouts(
        organization: &str,
        project: &str,
        timeouts: ConnectionTimeoutConfig,
    ) -> Result<Self> {
        let auth = AdoAuth::new()?;
        let endpoints = Endpoints::new(organization, project);

        Self::from_parts(auth, endpoints, timeouts)
    }

    fn from_parts(
        auth: AdoAuth,
        endpoints: Endpoints,
        timeouts: ConnectionTimeoutConfig,
    ) -> Result<Self> {
        timeouts.validate()?;
        let http = Client::builder()
            .user_agent(concat!("devops/", env!("CARGO_PKG_VERSION")))
            .timeout(timeouts.request_timeout())
            .connect_timeout(timeouts.connect_timeout())
            .build()?;

        Ok(Self {
            http,
            auth,
            endpoints,
            log_timeout: timeouts.log_timeout(),
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
        Self::new_for_testing_with_timeouts(base_url, ConnectionTimeoutConfig::default())
    }

    /// Creates a test client with explicit timeout settings.
    #[doc(hidden)]
    pub fn new_for_testing_with_timeouts(
        base_url: &str,
        timeouts: ConnectionTimeoutConfig,
    ) -> Result<Self> {
        let auth = AdoAuth::with_static_token("test-token")?;
        let endpoints = Endpoints::with_base_url(base_url, "testorg", "testproj");

        Self::from_parts(auth, endpoints, timeouts)
    }

    /// Sends an authenticated GET request and deserializes the JSON response.
    pub(crate) async fn get<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let transport = self
            .execute(HttpMethod::Get, url, RequestRetryPolicy::Idempotent)
            .await?;
        let bytes = read_json_capped(transport.response, &transport.display_url).await?;
        decode_json::<T>(&bytes, &transport.display_url)
    }

    /// Sends a GET request and extracts the continuation token from the `x-ms-continuationtoken` header.
    pub(crate) async fn get_with_continuation<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
    ) -> Result<(T, Option<String>)> {
        let transport = self
            .execute(HttpMethod::Get, url, RequestRetryPolicy::Idempotent)
            .await?;
        let continuation = transport
            .response
            .headers()
            .get("x-ms-continuationtoken")
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let bytes = read_json_capped(transport.response, &transport.display_url).await?;
        let body = decode_json::<T>(&bytes, &transport.display_url)?;
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
        self.get_all_continuation_pages(url, endpoint, progress, |page: ListResponse<T>| page.value)
            .await
    }

    /// Fetches all continuation-token pages and extracts items from each response.
    pub(crate) async fn get_all_continuation_pages<
        R: serde::de::DeserializeOwned,
        T,
        F: Fn(R) -> Vec<T>,
    >(
        &self,
        url: &str,
        endpoint: &'static str,
        progress: Option<&PaginationProgressFn>,
        extract_items: F,
    ) -> Result<Vec<T>> {
        collect_continuation_pages(
            url,
            PaginationOptions::new(endpoint, progress),
            |full_url| async move { self.get_with_continuation::<R>(&full_url).await },
            extract_items,
        )
        .await
    }

    /// Sends an authenticated GET request and returns the response body as plain text.
    pub(crate) async fn get_text(&self, url: &str) -> Result<String> {
        let transport = self
            .execute_request(
                HttpMethod::Get,
                url,
                RequestRetryPolicy::Idempotent,
                |request| request.timeout(self.log_timeout),
            )
            .await?;
        let text = read_text_capped(transport.response).await?;
        tracing::debug!(
            method = "GET",
            url = %transport.display_url,
            bytes = text.len(),
            "api text body"
        );
        Ok(text)
    }

    /// Sends an authenticated PATCH request with a JSON body, discarding the response.
    pub(crate) async fn patch_json<B: serde::Serialize>(
        &self,
        url: &str,
        body: &B,
        retry_policy: RequestRetryPolicy,
    ) -> Result<()> {
        self.execute_json(HttpMethod::Patch, url, body, retry_policy)
            .await?;
        Ok(())
    }

    /// Sends an authenticated POST request with a JSON body and deserializes the response.
    pub(crate) async fn post_json<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        body: &B,
        retry_policy: RequestRetryPolicy,
    ) -> Result<T> {
        let transport = self
            .execute_json(HttpMethod::Post, url, body, retry_policy)
            .await?;
        let bytes = read_json_capped(transport.response, &transport.display_url).await?;
        decode_json::<T>(&bytes, &transport.display_url)
    }

    /// Sends an authenticated DELETE request, discarding the response.
    pub(crate) async fn delete(&self, url: &str, retry_policy: RequestRetryPolicy) -> Result<()> {
        self.execute(HttpMethod::Delete, url, retry_policy).await?;
        Ok(())
    }

    async fn execute(
        &self,
        method: HttpMethod,
        url: &str,
        retry_policy: RequestRetryPolicy,
    ) -> Result<TransportResponse> {
        self.execute_request(method, url, retry_policy, |request| request)
            .await
    }

    async fn execute_json<B: serde::Serialize>(
        &self,
        method: HttpMethod,
        url: &str,
        body: &B,
        retry_policy: RequestRetryPolicy,
    ) -> Result<TransportResponse> {
        self.execute_request(method, url, retry_policy, |request| request.json(body))
            .await
    }

    async fn execute_request<F>(
        &self,
        method: HttpMethod,
        url: &str,
        retry_policy: RequestRetryPolicy,
        configure: F,
    ) -> Result<TransportResponse>
    where
        F: Fn(RequestBuilder) -> RequestBuilder,
    {
        let token = self.auth.token().await?;
        let display_url = url_without_query(url).to_string();
        let method_label = method.as_str();
        let start = Instant::now();
        tracing::debug!(
            method = method_label,
            url = %display_url,
            retry_policy = retry_policy.label(),
            "api request"
        );
        let resp = self
            .send_with_retry(
                || {
                    let request = method
                        .request_builder(&self.http, url)
                        .bearer_auth(token.expose_secret());
                    configure(request)
                },
                method_label,
                &display_url,
                retry_policy,
            )
            .await?;
        let resp = self
            .ensure_success(resp, method_label, &display_url)
            .await?;
        let status = resp.status().as_u16();
        let rate_limit_metadata = parse_rate_limit_metadata(resp.headers());
        if !rate_limit_metadata.is_empty() {
            tracing::debug!(
                method = method_label,
                url = %display_url,
                ?rate_limit_metadata,
                "api throttling metadata"
            );
        }
        tracing::debug!(
            method = method_label,
            url = %display_url,
            status,
            elapsed_ms = start.elapsed().as_millis() as u64,
            "api response"
        );
        Ok(TransportResponse {
            response: resp,
            display_url,
        })
    }

    /// Validates that a response returned a success status.
    ///
    /// On failure, reads the body and returns typed [`AdoError`] variants for
    /// Azure DevOps' `api-version` rejection signature, rate limits, and other
    /// HTTP status failures.
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

        let rate_limit_metadata = parse_rate_limit_metadata(response.headers());
        let body = response.text().await.unwrap_or_default();
        let requested = self.endpoints.api_version.as_ref();

        if let Some(err) = detect_api_version_unsupported(&body, requested, display_url) {
            tracing::warn!(
                method,
                url = display_url,
                status = status.as_u16(),
                requested_api_version = requested,
                server_message = %err,
                "api-version unsupported"
            );
            return Err(anyhow::Error::new(err));
        }

        let body_preview = summarize_error_body(&body);
        if let Some(body_preview) = body_preview.as_deref() {
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

        if status == StatusCode::TOO_MANY_REQUESTS {
            tracing::warn!(
                method,
                url = display_url,
                status = status.as_u16(),
                ?rate_limit_metadata,
                "api rate limit response"
            );
            return Err(AdoError::RateLimit {
                method,
                url: display_url.to_string(),
                status,
                metadata: rate_limit_metadata,
                body: body_preview,
            }
            .into());
        }

        Err(AdoError::HttpStatus {
            method,
            url: display_url.to_string(),
            status,
            body: body_preview,
        }
        .into())
    }

    /// Sends an HTTP request with retry on safe transient failures.
    ///
    /// Retries up to [`MAX_RETRIES`] times on HTTP `429`/`503` (honouring the
    /// `Retry-After` header when present) and on `reqwest` timeout/connect
    /// errors when the request policy permits replay. Non-retryable responses
    /// (including other 5xx statuses), and retryable failures for
    /// non-idempotent requests, are returned unchanged so that
    /// [`AdoClient::ensure_success`] can produce the usual diagnostic error.
    async fn send_with_retry<F>(
        &self,
        make_request: F,
        operation_label: &'static str,
        display_url: &str,
        retry_policy: RequestRetryPolicy,
    ) -> Result<reqwest::Response>
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
                    if retry_policy.retries_transient_failures()
                        && retryable_status
                        && attempt < MAX_RETRIES
                    {
                        let rate_limit_metadata = parse_rate_limit_metadata(resp.headers());
                        let (sleep_for, source) = rate_limit_metadata.retry_after.map_or_else(
                            || (compute_backoff(attempt), "backoff"),
                            |d| (d.min(cap), "retry-after"),
                        );
                        tracing::debug!(
                            operation = operation_label,
                            attempt = attempt + 1,
                            status = status.as_u16(),
                            source,
                            sleep_ms = sleep_for.as_millis() as u64,
                            ?rate_limit_metadata,
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
                    if retry_policy.retries_transient_failures()
                        && transient
                        && attempt < MAX_RETRIES
                    {
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
                    if err.is_timeout() {
                        return Err(AdoError::Timeout {
                            method: operation_label,
                            url: display_url.to_string(),
                            source: Box::new(err),
                        }
                        .into());
                    }
                    return Err(err.into());
                }
            }
        }
    }
}

async fn collect_continuation_pages<R, T, Fetch, Fut, Extract>(
    url: &str,
    options: PaginationOptions<'_>,
    mut fetch_page: Fetch,
    extract_items: Extract,
) -> Result<Vec<T>>
where
    Fetch: FnMut(String) -> Fut,
    Fut: Future<Output = Result<(R, Option<String>)>>,
    Extract: Fn(R) -> Vec<T>,
{
    let mut all_items = Vec::new();
    let mut continuation_token: Option<String> = None;
    let mut page_count: usize = 0;
    let start = Instant::now();

    loop {
        if page_count >= options.max_pages {
            let display_url = url_without_query(url);
            let message = format!(
                "Pagination limit reached: fetched {} pages from {display_url}. \
                 If your organization has more data than this, set DEVOPS_MAX_PAGES \
                 (e.g., DEVOPS_MAX_PAGES=5000) or file an issue at \
                 https://github.com/ljtill/azure-devops-cli/issues.",
                options.max_pages
            );
            return Err(AdoError::PartialData {
                endpoint: options.endpoint,
                url: display_url.to_string(),
                completed_pages: page_count,
                items: all_items.len(),
                message,
            }
            .into());
        }

        let full_url = paginated_url(url, continuation_token.as_deref())?;
        let (page, next_token) = fetch_page(full_url.to_string()).await?;
        all_items.extend(extract_items(page));
        page_count += 1;

        emit_pagination_progress(
            options.progress,
            options.endpoint,
            page_count,
            all_items.len(),
        );

        match next_token {
            Some(t) if !t.trim().is_empty() => continuation_token = Some(t.trim().to_owned()),
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

async fn collect_continuation_item_pages<T, Fetch, Fut>(
    url: &str,
    options: PaginationOptions<'_>,
    mut fetch_page: Fetch,
) -> Result<Vec<T>>
where
    Fetch: FnMut(String) -> Fut,
    Fut: Future<Output = Result<(Vec<T>, Option<String>)>>,
{
    let mut all_items = Vec::new();
    let mut continuation_token: Option<String> = None;
    let mut page_count: usize = 0;
    let start = Instant::now();

    loop {
        if page_count >= options.max_pages {
            let display_url = url_without_query(url);
            let message = format!(
                "Pagination limit reached: fetched {} pages from {display_url}. \
                 If your organization has more data than this, set DEVOPS_MAX_PAGES \
                 (e.g., DEVOPS_MAX_PAGES=5000) or file an issue at \
                 https://github.com/ljtill/azure-devops-cli/issues.",
                options.max_pages
            );
            return Err(AdoError::PartialData {
                endpoint: options.endpoint,
                url: display_url.to_string(),
                completed_pages: page_count,
                items: all_items.len(),
                message,
            }
            .into());
        }

        let full_url = paginated_url(url, continuation_token.as_deref())?;
        let (items, next_token) = fetch_page(full_url.to_string()).await?;
        all_items.extend(items);
        page_count += 1;

        emit_pagination_progress(
            options.progress,
            options.endpoint,
            page_count,
            all_items.len(),
        );

        match next_token {
            Some(t) if !t.trim().is_empty() => continuation_token = Some(t.trim().to_owned()),
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

fn parse_rate_limit_metadata(headers: &HeaderMap) -> RateLimitMetadata {
    RateLimitMetadata {
        retry_after: parse_retry_after_header(headers),
        limit: parse_u64_header(headers, "x-ratelimit-limit"),
        remaining: parse_u64_header(headers, "x-ratelimit-remaining"),
        reset_epoch_seconds: parse_u64_header(headers, "x-ratelimit-reset"),
    }
}

fn parse_u64_header(headers: &HeaderMap, name: &'static str) -> Option<u64> {
    let raw = header_value(headers, name)?;
    match raw.trim().parse::<u64>() {
        Ok(value) => Some(value),
        Err(err) => {
            tracing::debug!(
                header = name,
                value = raw,
                error = %err,
                "ignoring malformed rate-limit header"
            );
            None
        }
    }
}

fn parse_retry_after_header(headers: &HeaderMap) -> Option<Duration> {
    let raw = header_value(headers, "retry-after")?;
    let parsed = parse_retry_after(raw);
    if parsed.is_none() {
        tracing::debug!(
            header = "retry-after",
            value = raw,
            "ignoring malformed retry header"
        );
    }
    parsed
}

fn header_value<'a>(headers: &'a HeaderMap, name: &'static str) -> Option<&'a str> {
    let value = headers.get(name)?;
    match value.to_str() {
        Ok(value) => Some(value),
        Err(err) => {
            tracing::debug!(
                header = name,
                error = %err,
                "ignoring non-UTF-8 response header"
            );
            None
        }
    }
}

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
        return Err(AdoError::BodyCap {
            url: display_url.to_string(),
            kind: BodyKind::Json,
            limit_bytes: limit,
            actual_bytes: Some(cl),
        }
        .into());
    }
    let mut buf: Vec<u8> = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.with_context(|| format!("Error reading response body from {display_url}"))?;
        if (buf.len() as u64).saturating_add(chunk.len() as u64) > limit {
            return Err(AdoError::BodyCap {
                url: display_url.to_string(),
                kind: BodyKind::Json,
                limit_bytes: limit,
                actual_bytes: None,
            }
            .into());
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

fn decode_json<T: serde::de::DeserializeOwned>(bytes: &[u8], display_url: &str) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|source| {
        AdoError::Decode {
            url: display_url.to_string(),
            source: Box::new(source),
        }
        .into()
    })
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
fn emit_pagination_progress(
    progress: Option<&PaginationProgressFn>,
    endpoint: &'static str,
    page: usize,
    items_so_far: usize,
) {
    tracing::debug!(endpoint, page, items_so_far, "pagination progress");
    if let Some(progress) = progress {
        progress(PaginationProgress {
            endpoint,
            page,
            items_so_far,
        });
    }
}

/// Detects the Azure DevOps "API version not supported" error signature in an
/// HTTP error body.
///
/// Returns `Some(AdoError::UnsupportedApiVersion)` when either:
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
) -> Option<AdoError> {
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

    Some(AdoError::UnsupportedApiVersion {
        requested: requested.to_string(),
        url: url.to_string(),
        server_message,
    })
}

#[cfg(test)]
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

/// Returns a URL with the continuation token appended as an encoded query parameter.
pub(crate) fn continuation_url(base_url: &str, continuation_token: &str) -> Result<String> {
    Ok(paginated_url(base_url, Some(continuation_token))?.to_string())
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
    fn partial_data_error_preserves_pagination_limit_message() {
        let err = AdoError::PartialData {
            endpoint: "definitions",
            url: "https://example.test/build/definitions".to_string(),
            completed_pages: 100,
            items: 2500,
            message: "Pagination limit reached: fetched 100 pages from https://example.test/build/definitions.".to_string(),
        };

        assert!(err.to_string().starts_with("Pagination limit reached"));
        let AdoError::PartialData {
            endpoint,
            completed_pages,
            items,
            ..
        } = err
        else {
            panic!("expected PartialData");
        };
        assert_eq!(endpoint, "definitions");
        assert_eq!(completed_pages, 100);
        assert_eq!(items, 2500);
    }

    #[test]
    fn detect_api_version_unsupported_json_shape() {
        let body = r#"{"$id":"1","message":"The API version '7.1' is not supported for this endpoint.","typeKey":"VersionNotSupportedException"}"#;
        let err = detect_api_version_unsupported(body, "7.1", "https://example.test/x")
            .expect("should detect JSON VersionNotSupportedException");

        let AdoError::UnsupportedApiVersion {
            requested,
            url,
            server_message,
        } = err
        else {
            panic!("expected UnsupportedApiVersion");
        };
        assert_eq!(requested, "7.1");
        assert_eq!(url, "https://example.test/x");
        assert_eq!(
            server_message,
            "The API version '7.1' is not supported for this endpoint."
        );
    }

    #[test]
    fn detect_api_version_unsupported_plain_text() {
        // No typeKey, but both trigger substrings present.
        let body = "The API version 7.1 is not supported.";
        let err = detect_api_version_unsupported(body, "7.1", "https://example.test/x")
            .expect("should detect plain text variant");

        let AdoError::UnsupportedApiVersion {
            requested,
            server_message,
            ..
        } = err
        else {
            panic!("expected UnsupportedApiVersion");
        };
        assert_eq!(requested, "7.1");
        assert_eq!(server_message, "The API version 7.1 is not supported.");
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
        use std::sync::{Arc, Mutex};
        let calls = Arc::new(Mutex::new(Vec::new()));
        let captured_calls = Arc::clone(&calls);
        let cb = move |p: PaginationProgress| {
            assert_eq!(p.endpoint, "test_endpoint");
            captured_calls
                .lock()
                .unwrap()
                .push((p.page, p.items_so_far));
        };

        emit_pagination_progress(Some(&cb), "test_endpoint", 1, 25);
        emit_pagination_progress(Some(&cb), "test_endpoint", 2, 57);
        emit_pagination_progress(Some(&cb), "test_endpoint", 3, 80);

        let calls = calls.lock().unwrap().clone();
        assert_eq!(calls, vec![(1, 25), (2, 57), (3, 80)]);
    }

    #[tokio::test]
    async fn pagination_contract_multi_page_follows_tokens_and_reports_progress() {
        use std::collections::VecDeque;
        use std::sync::{Arc, Mutex};

        let calls = Arc::new(Mutex::new(Vec::new()));
        let pages = Arc::new(Mutex::new(VecDeque::from([
            (vec![1, 2], Some("page 2".to_string())),
            (vec![3], None),
        ])));
        let progress_calls = Arc::new(Mutex::new(Vec::new()));
        let captured_progress_calls = Arc::clone(&progress_calls);
        let progress = move |p: PaginationProgress| {
            assert_eq!(p.endpoint, "numbers");
            captured_progress_calls
                .lock()
                .unwrap()
                .push((p.page, p.items_so_far));
        };

        let items = collect_continuation_pages(
            "https://example.test/items?api-version=7.1",
            PaginationOptions::for_testing("numbers", 10, Some(&progress)),
            {
                let calls = Arc::clone(&calls);
                let pages = Arc::clone(&pages);
                move |url| {
                    let calls = Arc::clone(&calls);
                    let pages = Arc::clone(&pages);
                    async move {
                        calls.lock().unwrap().push(url);
                        let (value, next) = pages.lock().unwrap().pop_front().unwrap();
                        Ok::<_, anyhow::Error>((ListResponse { value, count: None }, next))
                    }
                }
            },
            |page: ListResponse<u32>| page.value,
        )
        .await
        .expect("pagination should complete");

        assert_eq!(items, vec![1, 2, 3]);
        let calls = calls.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec![
                "https://example.test/items?api-version=7.1".to_string(),
                "https://example.test/items?api-version=7.1&continuationToken=page+2".to_string(),
            ]
        );
        let progress_calls = progress_calls.lock().unwrap().clone();
        assert_eq!(progress_calls, vec![(1, 2), (2, 3)]);
    }

    #[tokio::test]
    async fn pagination_contract_no_continuation_stops_after_first_page() {
        use std::sync::{Arc, Mutex};

        let calls = Arc::new(Mutex::new(Vec::new()));

        let items = collect_continuation_pages(
            "https://example.test/items?api-version=7.1",
            PaginationOptions::for_testing("numbers", 10, None),
            {
                let calls = Arc::clone(&calls);
                move |url| {
                    let calls = Arc::clone(&calls);
                    async move {
                        calls.lock().unwrap().push(url);
                        Ok::<_, anyhow::Error>((
                            ListResponse {
                                value: vec![42],
                                count: Some(1),
                            },
                            None,
                        ))
                    }
                }
            },
            |page: ListResponse<u32>| page.value,
        )
        .await
        .expect("single-page response should complete");

        assert_eq!(items, vec![42]);
        let calls = calls.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec!["https://example.test/items?api-version=7.1".to_string()]
        );
    }

    #[tokio::test]
    async fn pagination_contract_malformed_continuation_stops_after_first_page() {
        use std::sync::{Arc, Mutex};

        let calls = Arc::new(Mutex::new(Vec::new()));

        let items = collect_continuation_pages(
            "https://example.test/items?api-version=7.1",
            PaginationOptions::for_testing("numbers", 10, None),
            {
                let calls = Arc::clone(&calls);
                move |url| {
                    let calls = Arc::clone(&calls);
                    async move {
                        calls.lock().unwrap().push(url);
                        Ok::<_, anyhow::Error>((
                            ListResponse {
                                value: vec![7],
                                count: Some(1),
                            },
                            Some(" \t ".to_string()),
                        ))
                    }
                }
            },
            |page: ListResponse<u32>| page.value,
        )
        .await
        .expect("blank continuation token should be treated as absent");

        assert_eq!(items, vec![7]);
        let calls = calls.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec!["https://example.test/items?api-version=7.1".to_string()]
        );
    }

    #[tokio::test]
    async fn pagination_contract_preserves_duplicate_items() {
        use std::collections::VecDeque;
        use std::sync::{Arc, Mutex};

        let pages = Arc::new(Mutex::new(VecDeque::from([
            (vec![1, 2], Some("page 2".to_string())),
            (vec![2, 3], None),
        ])));

        let items = collect_continuation_pages(
            "https://example.test/items?api-version=7.1",
            PaginationOptions::for_testing("numbers", 10, None),
            {
                let pages = Arc::clone(&pages);
                move |_| {
                    let pages = Arc::clone(&pages);
                    async move {
                        let (value, next) = pages.lock().unwrap().pop_front().unwrap();
                        Ok::<_, anyhow::Error>((ListResponse { value, count: None }, next))
                    }
                }
            },
            |page: ListResponse<u32>| page.value,
        )
        .await
        .expect("duplicates across pages should not fail pagination");

        assert_eq!(items, vec![1, 2, 2, 3]);
    }

    #[tokio::test]
    async fn pagination_contract_cap_reached_returns_partial_data() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        let calls = Arc::new(AtomicUsize::new(0));
        let err = collect_continuation_pages(
            "https://example.test/items?api-version=7.1",
            PaginationOptions::for_testing("numbers", 2, None),
            {
                let calls = Arc::clone(&calls);
                move |_| {
                    let calls = Arc::clone(&calls);
                    async move {
                        let value = calls.fetch_add(1, Ordering::Relaxed) as u32;
                        Ok::<_, anyhow::Error>((
                            ListResponse {
                                value: vec![value],
                                count: None,
                            },
                            Some("next".to_string()),
                        ))
                    }
                }
            },
            |page: ListResponse<u32>| page.value,
        )
        .await
        .expect_err("cap should stop pagination");

        let AdoError::PartialData {
            endpoint,
            url,
            completed_pages,
            items,
            message,
            ..
        } = err
            .downcast_ref::<AdoError>()
            .expect("error should be typed")
        else {
            panic!("expected PartialData");
        };
        assert_eq!(*endpoint, "numbers");
        assert_eq!(url, "https://example.test/items");
        assert_eq!(*completed_pages, 2);
        assert_eq!(*items, 2);
        assert!(message.starts_with("Pagination limit reached"));
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn parse_rate_limit_metadata_reads_standard_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::RETRY_AFTER,
            reqwest::header::HeaderValue::from_static("3"),
        );
        headers.insert(
            reqwest::header::HeaderName::from_static("x-ratelimit-limit"),
            reqwest::header::HeaderValue::from_static("60"),
        );
        headers.insert(
            reqwest::header::HeaderName::from_static("x-ratelimit-remaining"),
            reqwest::header::HeaderValue::from_static("7"),
        );
        headers.insert(
            reqwest::header::HeaderName::from_static("x-ratelimit-reset"),
            reqwest::header::HeaderValue::from_static("1712345678"),
        );

        let metadata = parse_rate_limit_metadata(&headers);

        assert_eq!(metadata.retry_after, Some(Duration::from_secs(3)));
        assert_eq!(metadata.limit, Some(60));
        assert_eq!(metadata.remaining, Some(7));
        assert_eq!(metadata.reset_epoch_seconds, Some(1_712_345_678));
    }

    #[test]
    fn parse_rate_limit_metadata_ignores_malformed_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::RETRY_AFTER,
            reqwest::header::HeaderValue::from_static("not-a-date"),
        );
        headers.insert(
            reqwest::header::HeaderName::from_static("x-ratelimit-limit"),
            reqwest::header::HeaderValue::from_static("NaN"),
        );
        headers.insert(
            reqwest::header::HeaderName::from_static("x-ratelimit-remaining"),
            reqwest::header::HeaderValue::from_static("many"),
        );
        headers.insert(
            reqwest::header::HeaderName::from_static("x-ratelimit-reset"),
            reqwest::header::HeaderValue::from_static("soon"),
        );

        assert!(parse_rate_limit_metadata(&headers).is_empty());
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
