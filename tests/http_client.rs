//! Integration tests for the HTTP transport layer (`send_with_retry`,
//! body-size caps) against a `wiremock` mock server.
//!
//! Each test constructs an `AdoClient` via the hidden `new_for_testing`
//! constructor, which pins the base URL to the wiremock server and seeds a
//! fake bearer token. Scenarios cover:
//!
//! 1. Happy-path GET + JSON deserialisation.
//! 2. `429` + `Retry-After: <seconds>` retry timing.
//! 3. `429` without header — jittered exponential backoff fallback.
//! 4. `503` + `Retry-After: <HTTP-date>` retry.
//! 5. Retry exhaustion on repeated `503` (4 attempts = 1 initial + 3 retries).
//! 6. Fast-fail on `Content-Length` > `MAX_JSON_BYTES` (32 MiB cap).
//! 7. Streaming JSON body cap — skipped, see inline TODO.
//! 8. Text body cap + truncation marker — skipped, see inline TODO.

use std::time::{Duration, Instant};

use azure_devops_cli::client::http::AdoClient;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// --- Helpers ---

/// Wire path the `get_build` URL resolves to when the client is rooted at
/// the mock server via `new_for_testing`.
const BUILD_PATH: &str = "/testorg/testproj/_apis/build/builds/1";

fn sample_build_json() -> serde_json::Value {
    serde_json::json!({
        "id": 1,
        "buildNumber": "20240101.1",
        "status": "completed",
        "result": "succeeded",
        "queueTime": "2024-01-01T10:00:00Z",
        "startTime": "2024-01-01T10:00:05Z",
        "finishTime": "2024-01-01T10:05:00Z",
        "definition": { "id": 42, "name": "CI Pipeline" },
        "sourceBranch": "refs/heads/main",
        "requestedFor": { "displayName": "Jane Doe" }
    })
}

// --- Happy path ---

#[tokio::test]
async fn happy_path_get_returns_deserialised_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(BUILD_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_build_json()))
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    let build = client
        .get_build(1)
        .await
        .expect("happy-path GET should succeed");

    assert_eq!(build.id, 1);
    assert_eq!(build.build_number, "20240101.1");
    assert_eq!(build.definition.id, 42);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

// --- 429 with Retry-After: <seconds> ---

#[tokio::test]
async fn retry_honours_retry_after_seconds_header() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(BUILD_PATH))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "2"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(BUILD_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_build_json()))
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    let start = Instant::now();
    let build = client
        .get_build(1)
        .await
        .expect("retry should eventually succeed");
    let elapsed = start.elapsed();

    assert_eq!(build.id, 1);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
    assert!(
        elapsed >= Duration::from_secs(2),
        "expected ≥2s (Retry-After=2), got {elapsed:?}"
    );
    assert!(
        elapsed <= Duration::from_secs(4),
        "expected ≤4s (sanity cap), got {elapsed:?}"
    );
}

// --- 429 without Retry-After — backoff fallback ---

#[tokio::test]
async fn retry_falls_back_to_backoff_when_no_header() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(BUILD_PATH))
        .respond_with(ResponseTemplate::new(429))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(BUILD_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_build_json()))
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    let start = Instant::now();
    let build = client
        .get_build(1)
        .await
        .expect("retry should succeed via backoff");
    let elapsed = start.elapsed();

    assert_eq!(build.id, 1);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
    // Jittered base 500 ms ± 20% → 400–600 ms; pad upper bound for CI overhead.
    assert!(
        elapsed >= Duration::from_millis(400),
        "expected ≥400 ms backoff, got {elapsed:?}"
    );
    assert!(
        elapsed <= Duration::from_secs(2),
        "expected ≤2s (no Retry-After), got {elapsed:?}"
    );
}

// --- 503 with Retry-After: HTTP-date ---

#[tokio::test]
async fn retry_honours_retry_after_http_date_on_503() {
    let server = MockServer::start().await;
    let future = chrono::Utc::now() + chrono::Duration::seconds(1);
    let http_date = future.to_rfc2822();

    Mock::given(method("GET"))
        .and(path(BUILD_PATH))
        .respond_with(ResponseTemplate::new(503).insert_header("Retry-After", http_date.as_str()))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(BUILD_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(sample_build_json()))
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    let build = client
        .get_build(1)
        .await
        .expect("retry should succeed after HTTP-date delay");

    assert_eq!(build.id, 1);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

// --- Retry exhaustion on 503 ---

#[tokio::test]
async fn retry_exhausts_after_max_attempts_on_503() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(BUILD_PATH))
        // No Retry-After so we use backoff; cap retries regardless.
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    let result = client.get_build(1).await;

    assert!(result.is_err(), "expected error after retry exhaustion");
    // 1 initial attempt + MAX_RETRIES (3) = 4 total requests.
    assert_eq!(server.received_requests().await.unwrap().len(), 4);
}

// --- 500 is not retried ---

#[tokio::test]
async fn non_retryable_5xx_is_not_retried() {
    // `send_with_retry` only retries 429 and 503; a plain 500 must surface
    // after exactly one request.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(BUILD_PATH))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    let result = client.get_build(1).await;

    assert!(result.is_err(), "expected error on 500");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

// --- Body cap: Content-Length > MAX_JSON_BYTES (32 MiB) ---

#[tokio::test]
async fn json_body_cap_rejects_oversized_content_length() {
    // Serve a 33 MiB JSON-ish body; wiremock will set Content-Length to match.
    // `read_json_capped` checks Content-Length before reading the stream and
    // bails with an error containing "body too large" / "JSON cap".
    let server = MockServer::start().await;
    let oversized = vec![b'a'; 33 * 1024 * 1024];
    Mock::given(method("GET"))
        .and(path(BUILD_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_raw(oversized, "application/json"))
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    let result = client.get_build(1).await;

    let err = result.expect_err("oversized body must fail");
    let msg = format!("{err:#}").to_lowercase();
    assert!(
        msg.contains("body too large") && msg.contains("json cap"),
        "unexpected error message: {err:#}"
    );
}

// --- Streaming JSON body cap (no Content-Length) ---
//
// TODO: wiremock 0.6 always sets a Content-Length matching the body size, so
// the streaming-overflow branch of `read_json_capped` (chunk accumulation
// crossing the cap without a Content-Length hint) cannot be exercised
// end-to-end without a bespoke test server. The fast-fail Content-Length
// path is covered by `json_body_cap_rejects_oversized_content_length` above,
// and both branches emit the same error shape ("body too large … JSON cap")
// so diagnostics remain consistent. Leaving this scenario unimplemented.

// --- Text body cap + truncation marker ---
//
// TODO: `MAX_TEXT_BYTES` is 128 MiB, so an end-to-end test would need to
// allocate and stream ≥128 MiB through localhost — prohibitively slow and
// memory-hungry for a unit test. `read_text_capped` is a thin streaming
// accumulator with a static marker string; its behaviour is better covered
// by a targeted unit test than an integration test. The truncation marker
// is `"\n… log truncated at 128 MiB, open in browser\n"`.
