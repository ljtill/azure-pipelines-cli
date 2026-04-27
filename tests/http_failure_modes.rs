//! Integration tests for additional typed HTTP client failure modes.

use azure_devops_cli::client::errors::AdoError;
use azure_devops_cli::client::http::AdoClient;
use reqwest::StatusCode;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const BUILD_PATH: &str = "/testorg/testproj/_apis/build/builds/1";

fn ado_error(error: &anyhow::Error) -> &AdoError {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<AdoError>())
        .expect("error chain should contain AdoError")
}

#[tokio::test]
async fn auth_and_gateway_failures_return_typed_status_errors() {
    let cases = [
        (StatusCode::UNAUTHORIZED, "Access token is missing."),
        (StatusCode::FORBIDDEN, "The caller is not authorized."),
        (StatusCode::BAD_GATEWAY, "Upstream gateway failed."),
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "Service unavailable after retries.",
        ),
        (StatusCode::GATEWAY_TIMEOUT, "Upstream gateway timed out."),
    ];

    for (status, message) in cases {
        assert_typed_status_error(status, message).await;
    }
}

async fn assert_typed_status_error(status: StatusCode, message: &str) {
    let server = MockServer::start().await;
    let mut response = ResponseTemplate::new(status.as_u16()).set_body_json(serde_json::json!({
        "message": message,
    }));
    if status.is_server_error() {
        response = response.insert_header("Retry-After", "0");
    }

    Mock::given(method("GET"))
        .and(path(BUILD_PATH))
        .respond_with(response)
        .mount(&server)
        .await;

    let client = AdoClient::new_for_testing(&server.uri()).unwrap();
    let err = client
        .get_build(1)
        .await
        .expect_err("HTTP failure should return an error");

    let AdoError::HttpStatus {
        method,
        url,
        status: actual_status,
        body,
    } = ado_error(&err)
    else {
        panic!("expected typed HTTP status error");
    };
    assert_eq!(*method, "GET");
    assert_eq!(*actual_status, status);
    assert!(url.ends_with(BUILD_PATH));
    assert!(
        body.as_deref().is_some_and(|body| body.contains(message)),
        "expected body preview to include {message:?}, got {body:?}"
    );
}
