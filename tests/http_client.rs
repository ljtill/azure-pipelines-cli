//! Integration tests for the HTTP transport layer (`send_with_retry`,
//! body-size caps) against a `wiremock` mock server.
//!
//! Each test constructs an `AdoClient` via the hidden `new_for_testing`
//! constructor, which pins the base URL to the wiremock server and seeds a
//! fake bearer token. Scenarios cover:
//!
//! 1. Happy-path GET + JSON deserialisation.
//! 2. Per-request retry policy across GET, POST, PATCH, and DELETE.
//! 3. Continuation-token pagination over multiple pages.
//! 4. `429` + `Retry-After: <seconds>` retry timing.
//! 5. `429` without header — jittered exponential backoff fallback.
//! 6. `503` + `Retry-After: <HTTP-date>` retry.
//! 7. Retry exhaustion on repeated `503` (4 attempts = 1 initial + 3 retries).
//! 8. Fast-fail on `Content-Length` > `MAX_JSON_BYTES` (32 MiB cap).
//! 9. Streaming JSON body cap — skipped, see inline TODO.
//! 10. Text body cap + truncation marker — skipped, see inline TODO.

use std::time::{Duration, Instant};

use azure_devops_cli::client::endpoints::pull_requests::PullRequestListRequest;
use azure_devops_cli::client::errors::{AdoError, BodyKind};
use azure_devops_cli::client::http::AdoClient;
use azure_devops_cli::config::ConnectionTimeoutConfig;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

// --- Helpers ---

/// Wire path the `get_build` URL resolves to when the client is rooted at
/// the mock server via `new_for_testing`.
const BUILD_PATH: &str = "/testorg/testproj/_apis/build/builds/1";
const BUILD_LIST_PATH: &str = "/testorg/testproj/_apis/build/builds";
const BUILD_STAGE_PATH: &str = "/testorg/testproj/_apis/build/builds/1/stages/__default";
const DEFINITIONS_PATH: &str = "/testorg/testproj/_apis/build/definitions";
const PULL_REQUESTS_PATH: &str = "/testorg/testproj/_apis/git/pullrequests";
const APPROVALS_PATH: &str = "/testorg/testproj/_apis/pipelines/approvals";
const PIPELINE_RUN_PATH: &str = "/testorg/testproj/_apis/pipelines/42/runs";
const RETENTION_LEASES_PATH: &str = "/testorg/testproj/_apis/build/retention/leases";
const WIQL_PATH: &str = "/testorg/testproj/_apis/wit/wiql";
const PULL_REQUEST_THREADS_PATH: &str =
    "/testorg/testproj/_apis/git/repositories/repo-1/pullrequests/42/threads";
const WORK_ITEM_COMMENTS_PATH: &str = "/testorg/testproj/_apis/wit/workItems/42/comments";

fn sample_build_json() -> serde_json::Value {
    sample_build_json_with_id(1)
}

fn sample_build_json_with_id(id: u32) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "buildNumber": format!("20240101.{id}"),
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

fn sample_pull_request_json(id: u32) -> serde_json::Value {
    serde_json::json!({
        "pullRequestId": id,
        "title": format!("PR {id}"),
        "status": "active",
        "repository": {
            "id": format!("repo-{id}"),
            "name": "repo"
        }
    })
}

fn ado_error(error: &anyhow::Error) -> &AdoError {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<AdoError>())
        .expect("error chain should contain AdoError")
}

#[test]
fn client_rejects_invalid_timeout_config() {
    let result = AdoClient::new_for_testing_with_timeouts(
        "http://127.0.0.1:1",
        ConnectionTimeoutConfig {
            request_timeout_secs: 0,
            connect_timeout_secs: 10,
            log_timeout_secs: 60,
        },
    );
    let Err(err) = result else {
        panic!("zero request timeout must fail");
    };

    assert!(err.to_string().contains("request_timeout_secs"));
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

// --- Continuation-token pagination ---

#[tokio::test]
async fn definitions_pagination_follows_continuation_headers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(DEFINITIONS_PATH))
        .and(query_param("api-version", "7.1"))
        .and(query_param("includeLatestBuilds", "true"))
        .and(query_param("$top", "1000"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-ms-continuationtoken", "page 2")
                .set_body_json(serde_json::json!({
                    "value": [
                        {"id": 1, "name": "CI", "path": "\\"}
                    ],
                    "count": 1
                })),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(DEFINITIONS_PATH))
        .and(query_param("api-version", "7.1"))
        .and(query_param("includeLatestBuilds", "true"))
        .and(query_param("$top", "1000"))
        .and(query_param("continuationToken", "page 2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 2, "name": "CD", "path": "\\Deploy"}
            ],
            "count": 1
        })))
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    let definitions = client
        .list_definitions()
        .await
        .expect("pagination should return both pages");

    let ids: Vec<u32> = definitions
        .into_iter()
        .map(|definition| definition.id)
        .collect();
    assert_eq!(ids, vec![1, 2]);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn recent_builds_pagination_follows_continuation_headers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(BUILD_LIST_PATH))
        .and(query_param("api-version", "7.1"))
        .and(query_param("$top", "1000"))
        .and(query_param("queryOrder", "queueTimeDescending"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-ms-continuationtoken", "page 2")
                .set_body_json(serde_json::json!({
                    "value": [sample_build_json_with_id(1)],
                    "count": 1
                })),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(BUILD_LIST_PATH))
        .and(query_param("api-version", "7.1"))
        .and(query_param("$top", "1000"))
        .and(query_param("queryOrder", "queueTimeDescending"))
        .and(query_param("continuationToken", "page 2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [sample_build_json_with_id(2)],
            "count": 1
        })))
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    let builds = client
        .list_recent_builds()
        .await
        .expect("recent builds pagination should return both pages");

    let ids: Vec<u32> = builds.into_iter().map(|build| build.id).collect();
    assert_eq!(ids, vec![1, 2]);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn definition_builds_expose_continuation_for_partial_page() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(BUILD_LIST_PATH))
        .and(query_param("definitions", "42"))
        .and(query_param("api-version", "7.1"))
        .and(query_param("$top", "20"))
        .and(query_param("queryOrder", "queueTimeDescending"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-ms-continuationtoken", "page 2")
                .set_body_json(serde_json::json!({
                    "value": [sample_build_json_with_id(1)],
                    "count": 1
                })),
        )
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    let (builds, continuation_token) = client
        .list_builds_for_definition(42)
        .await
        .expect("definition builds should expose continuation state");

    let ids: Vec<u32> = builds.into_iter().map(|build| build.id).collect();
    assert_eq!(ids, vec![1]);
    assert_eq!(continuation_token.as_deref(), Some("page 2"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn pull_requests_pagination_follows_continuation_headers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(PULL_REQUESTS_PATH))
        .and(query_param("api-version", "7.1"))
        .and(query_param("searchCriteria.status", "active"))
        .and(query_param("$top", "100"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-ms-continuationtoken", "page 2")
                .set_body_json(serde_json::json!({
                    "value": [sample_pull_request_json(1)],
                    "count": 1
                })),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(PULL_REQUESTS_PATH))
        .and(query_param("api-version", "7.1"))
        .and(query_param("searchCriteria.status", "active"))
        .and(query_param("$top", "100"))
        .and(query_param("continuationToken", "page 2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [sample_pull_request_json(2)],
            "count": 1
        })))
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    let prs = client
        .list_pull_requests(PullRequestListRequest::active())
        .await
        .expect("pull request pagination should return both pages");

    let ids: Vec<u32> = prs.into_iter().map(|pr| pr.pull_request_id).collect();
    assert_eq!(ids, vec![1, 2]);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn pull_request_threads_pagination_follows_continuation_headers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(PULL_REQUEST_THREADS_PATH))
        .and(query_param("api-version", "7.1"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-ms-continuationtoken", "page 2")
                .set_body_json(serde_json::json!({
                    "value": [
                        {"id": 1, "status": "active", "comments": []}
                    ],
                    "count": 1
                })),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(PULL_REQUEST_THREADS_PATH))
        .and(query_param("api-version", "7.1"))
        .and(query_param("continuationToken", "page 2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {"id": 2, "status": "closed", "comments": []}
            ],
            "count": 1
        })))
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    let threads = client
        .list_pull_request_threads("repo-1", 42)
        .await
        .expect("thread pagination should return both pages");

    let ids: Vec<u32> = threads.into_iter().map(|thread| thread.id).collect();
    assert_eq!(ids, vec![1, 2]);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn work_item_comments_pagination_follows_body_continuation_tokens() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(WORK_ITEM_COMMENTS_PATH))
        .and(query_param("api-version", "7.1-preview.3"))
        .and(query_param("$top", "200"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "comments": [
                {"id": 1, "text": "first"}
            ],
            "totalCount": 2,
            "continuationToken": "page 2"
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(WORK_ITEM_COMMENTS_PATH))
        .and(query_param("api-version", "7.1-preview.3"))
        .and(query_param("$top", "200"))
        .and(query_param("continuationToken", "page 2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "comments": [
                {"id": 2, "text": "second"}
            ],
            "totalCount": 2
        })))
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    let comments = client
        .list_work_item_comments(42)
        .await
        .expect("comment pagination should return both pages");

    let ids: Vec<u32> = comments.into_iter().map(|comment| comment.id).collect();
    assert_eq!(ids, vec![1, 2]);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

// --- Request retry policy across verbs ---

#[tokio::test]
async fn get_retries_transient_status_by_default() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(BUILD_PATH))
        .respond_with(ResponseTemplate::new(503).insert_header("Retry-After", "0"))
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
        .expect("idempotent GET should retry through central transport");

    assert_eq!(build.id, 1);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn read_only_post_retries_when_marked_idempotent() {
    let server = MockServer::start().await;
    let query = "SELECT [System.Id] FROM WorkItems";
    let body = serde_json::json!({ "query": query });
    Mock::given(method("POST"))
        .and(path(WIQL_PATH))
        .and(query_param("api-version", "7.1"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_json(body.clone()))
        .respond_with(ResponseTemplate::new(503).insert_header("Retry-After", "0"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(WIQL_PATH))
        .and(query_param("api-version", "7.1"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_json(body))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "queryType": "flat",
            "queryResultType": "workItem",
            "workItems": [{ "id": 123, "url": "https://example.test/workItems/123" }]
        })))
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    let result = client
        .query_by_wiql(query)
        .await
        .expect("read-only POST should retry when marked idempotent");

    assert_eq!(result.work_items[0].id, 123);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn non_idempotent_post_is_not_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(PIPELINE_RUN_PATH))
        .and(query_param("api-version", "7.1"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_json(serde_json::json!({})))
        .respond_with(ResponseTemplate::new(503).insert_header("Retry-After", "0"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(PIPELINE_RUN_PATH))
        .and(query_param("api-version", "7.1"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_json(serde_json::json!({})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 7,
            "name": "queued"
        })))
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    let err = client
        .run_pipeline(42)
        .await
        .expect_err("non-idempotent POST should not be retried");

    let AdoError::HttpStatus { status, .. } = ado_error(&err) else {
        panic!("expected typed HTTP status error");
    };
    assert_eq!(status.as_u16(), 503);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn idempotent_patch_retries_transient_status() {
    let server = MockServer::start().await;
    let body = serde_json::json!({ "status": "cancelling" });
    Mock::given(method("PATCH"))
        .and(path(BUILD_PATH))
        .and(query_param("api-version", "7.1"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_json(body.clone()))
        .respond_with(ResponseTemplate::new(503).insert_header("Retry-After", "0"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(BUILD_PATH))
        .and(query_param("api-version", "7.1"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_json(body))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    client
        .cancel_build(1)
        .await
        .expect("idempotent PATCH should retry through central transport");

    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn non_idempotent_patch_is_not_retried() {
    let server = MockServer::start().await;
    let body = serde_json::json!([{
        "approvalId": "approval-1",
        "status": "approved",
        "comment": "ship it"
    }]);
    Mock::given(method("PATCH"))
        .and(path(APPROVALS_PATH))
        .and(query_param("api-version", "7.1"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_json(body.clone()))
        .respond_with(ResponseTemplate::new(503).insert_header("Retry-After", "0"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(APPROVALS_PATH))
        .and(query_param("api-version", "7.1"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_json(body))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    let err = client
        .update_approval("approval-1", "approved", "ship it")
        .await
        .expect_err("non-idempotent PATCH should not be retried");

    let AdoError::HttpStatus { status, .. } = ado_error(&err) else {
        panic!("expected typed HTTP status error");
    };
    assert_eq!(status.as_u16(), 503);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn non_idempotent_stage_retry_patch_is_not_retried() {
    let server = MockServer::start().await;
    let body = serde_json::json!({ "forceRetryAllJobs": true, "state": 1 });
    Mock::given(method("PATCH"))
        .and(path(BUILD_STAGE_PATH))
        .and(query_param("api-version", "7.1-preview.1"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_json(body.clone()))
        .respond_with(ResponseTemplate::new(503).insert_header("Retry-After", "0"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(BUILD_STAGE_PATH))
        .and(query_param("api-version", "7.1-preview.1"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_json(body))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    let err = client
        .retry_stage(1, "__default")
        .await
        .expect_err("non-idempotent stage retry PATCH should not be retried");

    let AdoError::HttpStatus { status, .. } = ado_error(&err) else {
        panic!("expected typed HTTP status error");
    };
    assert_eq!(status.as_u16(), 503);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn delete_retries_when_marked_idempotent() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(RETENTION_LEASES_PATH))
        .and(query_param("ids", "1,2"))
        .and(query_param("api-version", "7.1"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(503).insert_header("Retry-After", "0"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(RETENTION_LEASES_PATH))
        .and(query_param("ids", "1,2"))
        .and(query_param("api-version", "7.1"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    client
        .delete_retention_leases(&[1, 2])
        .await
        .expect("idempotent DELETE should retry through central transport");

    assert_eq!(server.received_requests().await.unwrap().len(), 2);
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
    let err = result.expect_err("503 should fail after retries");
    let AdoError::HttpStatus { status, .. } = ado_error(&err) else {
        panic!("expected typed HTTP status error");
    };
    assert_eq!(status.as_u16(), 503);
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

    let err = result.expect_err("expected error on 500");
    let AdoError::HttpStatus { status, .. } = ado_error(&err) else {
        panic!("expected typed HTTP status error");
    };
    assert_eq!(status.as_u16(), 500);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

// --- 429 is typed as rate limit after retry exhaustion ---

#[tokio::test]
async fn rate_limit_error_is_typed_after_retry_exhaustion() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(BUILD_PATH))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "0")
                .insert_header("X-RateLimit-Limit", "60")
                .insert_header("X-RateLimit-Remaining", "0")
                .insert_header("X-RateLimit-Reset", "1712345678"),
        )
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    let err = client
        .get_build(1)
        .await
        .expect_err("429 should fail after retries");

    let AdoError::RateLimit {
        status, metadata, ..
    } = ado_error(&err)
    else {
        panic!("expected typed rate limit error");
    };
    assert_eq!(status.as_u16(), 429);
    assert_eq!(metadata.retry_after, Some(Duration::ZERO));
    assert_eq!(metadata.limit, Some(60));
    assert_eq!(metadata.remaining, Some(0));
    assert_eq!(metadata.reset_epoch_seconds, Some(1_712_345_678));
    assert_eq!(
        metadata.diagnostic_summary().as_deref(),
        Some("retry after 0s, remaining 0, limit 60, reset epoch 1712345678")
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 4);
}

// --- Unsupported API version is typed ---

#[tokio::test]
async fn unsupported_api_version_error_is_typed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(BUILD_PATH))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "message": "The API version '7.1' is not supported for this endpoint.",
            "typeKey": "VersionNotSupportedException",
        })))
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    let err = client
        .get_build(1)
        .await
        .expect_err("unsupported api-version should fail");

    let AdoError::UnsupportedApiVersion {
        requested,
        server_message,
        ..
    } = ado_error(&err)
    else {
        panic!("expected typed unsupported api-version error");
    };
    assert_eq!(requested, "7.1");
    assert_eq!(
        server_message,
        "The API version '7.1' is not supported for this endpoint."
    );
}

// --- Decode errors are typed ---

#[tokio::test]
async fn decode_error_is_typed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(BUILD_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_string("{not-json"))
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    let err = client
        .get_build(1)
        .await
        .expect_err("invalid JSON should fail");

    let AdoError::Decode { url, .. } = ado_error(&err) else {
        panic!("expected typed decode error");
    };
    assert!(url.ends_with(BUILD_PATH));
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
    let AdoError::BodyCap {
        kind, actual_bytes, ..
    } = ado_error(&err)
    else {
        panic!("expected typed body cap error");
    };
    assert_eq!(*kind, BodyKind::Json);
    assert!(actual_bytes.is_some_and(|bytes| bytes > 32 * 1024 * 1024));
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
