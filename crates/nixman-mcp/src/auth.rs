//! Auth middleware for the MCP HTTP transport.
//!
//! Validates `Authorization: Bearer <token>` header against the configured
//! token. If no token is configured, all requests are allowed (localhost
//! mode).

use axum::{
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use std::future::Future;
use std::pin::Pin;

/// Create an auth middleware layer that enforces Bearer token auth.
///
/// If `token` is `None`, all requests pass through (localhost mode).
/// If `token` is `Some`, only requests with `Authorization: Bearer <token>`
/// are allowed; everything else gets a 401 JSON response.
///
/// Use with `axum::middleware::from_fn(require_auth(token))`.
pub fn require_auth(
    token: Option<String>,
) -> impl Fn(Request, Next) -> Pin<Box<dyn Future<Output = Result<Response, Response>> + Send>> + Clone
{
    move |req: Request, next: Next| {
        let token = token.clone();
        Box::pin(async move {
            if let Some(expected) = token {
                let auth = req
                    .headers()
                    .get(header::AUTHORIZATION)
                    .and_then(|v| v.to_str().ok());

                if auth != Some(&format!("Bearer {expected}")) {
                    let body = axum::body::Body::from(
                        serde_json::to_string(&serde_json::json!({
                            "error": "Unauthorized",
                            "message": "Missing or invalid API token"
                        }))
                        .unwrap(),
                    );
                    return Err(
                        Response::builder()
                            .status(StatusCode::UNAUTHORIZED)
                            .header(header::CONTENT_TYPE, "application/json")
                            .body(body)
                            .unwrap(),
                    );
                }
            }

            Ok(next.run(req).await)
        })
    }
}

#[cfg(test)]
#[cfg(feature = "http")]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request, middleware, routing::get, Router};
    use tower::ServiceExt;

    #[tokio::test]
    async fn no_token_allows_all() {
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(middleware::from_fn(require_auth(None)));

        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "no token configured should allow all requests");
    }

    #[tokio::test]
    async fn correct_token_passes() {
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(middleware::from_fn(require_auth(Some("test-token".into()))));

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "correct token should pass auth");
    }

    #[tokio::test]
    async fn wrong_token_returns_401() {
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(middleware::from_fn(require_auth(Some("test-token".into()))));

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header("Authorization", "Bearer wrong-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 401, "wrong token should be rejected");
    }

    #[tokio::test]
    async fn no_header_returns_401() {
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(middleware::from_fn(require_auth(Some("test-token".into()))));

        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), 401, "missing auth header should return 401");
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/json",
            "401 response should be JSON"
        );
    }

    #[tokio::test]
    async fn wrong_scheme_returns_401() {
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(middleware::from_fn(require_auth(Some("test-token".into()))));

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header("Authorization", "Basic dGVzdDp0ZXN0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 401, "Basic auth scheme should be rejected");
    }
}
