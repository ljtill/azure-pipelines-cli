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
use reqwest::{Client, StatusCode, Url};

use super::auth::AdoAuth;
use super::endpoints::Endpoints;
use super::models::ListResponse;

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

    /// Sends an authenticated GET request and deserializes the JSON response.
    pub(crate) async fn get<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let token = self.auth.token().await?;
        let display_url = url_without_query(url);
        let start = Instant::now();
        tracing::debug!(method = "GET", url = display_url, "api request");
        let resp = self
            .http
            .get(url)
            .bearer_auth(&token)
            .send()
            .await?
            .error_for_status()?;
        let status = resp.status().as_u16();
        let body = resp.json::<T>().await?;
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
            .http
            .get(url)
            .bearer_auth(&token)
            .send()
            .await?
            .error_for_status()?;
        let status = resp.status().as_u16();
        let continuation = resp
            .headers()
            .get("x-ms-continuationtoken")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty())
            .map(std::string::ToString::to_string);
        let body = resp.json::<T>().await?;
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

    /// Fetches all pages of a paginated list endpoint, following continuation tokens until exhausted.
    pub(crate) async fn get_all_pages<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
    ) -> Result<Vec<T>> {
        let mut all_items = Vec::new();
        let mut continuation_token: Option<String> = None;
        let mut page_count: u32 = 0;
        let start = Instant::now();

        loop {
            let full_url = paginated_url(url, continuation_token.as_deref())?;

            let token = self.auth.token().await?;
            tracing::debug!(method = "GET", url = %url_without_query(full_url.as_str()), page = page_count + 1, "api paginated request");
            let resp = self
                .http
                .get(full_url)
                .bearer_auth(&token)
                .send()
                .await?
                .error_for_status()?;

            let next_token = resp
                .headers()
                .get("x-ms-continuationtoken")
                .and_then(|v| v.to_str().ok())
                .map(std::string::ToString::to_string);

            let page: ListResponse<T> = resp.json().await?;
            all_items.extend(page.value);
            page_count += 1;

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
            .http
            .get(url)
            .bearer_auth(&token)
            .send()
            .await?
            .error_for_status()?;
        let status = resp.status().as_u16();
        let text = resp.text().await?;
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
            .http
            .patch(url)
            .bearer_auth(&token)
            .json(body)
            .send()
            .await?
            .error_for_status()?;
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
            .http
            .post(url)
            .bearer_auth(&token)
            .json(body)
            .send()
            .await?;
        let resp = ensure_success_with_body(resp, "POST", display_url).await?;
        let status = resp.status().as_u16();
        let body = resp.json::<T>().await?;
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
            .http
            .delete(url)
            .bearer_auth(&token)
            .send()
            .await?
            .error_for_status()?;
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
}

const MAX_ERROR_BODY_CHARS: usize = 300;

async fn ensure_success_with_body(
    response: reqwest::Response,
    method: &'static str,
    display_url: &str,
) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let body = response.text().await.unwrap_or_default();
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
}
