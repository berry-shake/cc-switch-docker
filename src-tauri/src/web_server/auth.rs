//! HTTP Basic 认证。浏览器原生弹登录框，同源的 fetch / EventSource 会自动带上凭据。

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use subtle::ConstantTimeEq;

#[derive(Clone)]
pub(crate) struct Credentials {
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &"***")
            .finish()
    }
}

impl Credentials {
    fn matches(&self, header_value: &str) -> bool {
        let Some(encoded) = header_value
            .strip_prefix("Basic ")
            .or_else(|| header_value.strip_prefix("basic "))
        else {
            return false;
        };
        let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded.trim()) else {
            return false;
        };
        let expected = format!("{}:{}", self.username, self.password);
        decoded.ct_eq(expected.as_bytes()).into()
    }
}

pub(super) async fn require_basic_auth(
    State(credentials): State<Arc<Credentials>>,
    request: Request,
    next: Next,
) -> Response {
    let authorized = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| credentials.matches(value));
    if authorized {
        return next.run(request).await;
    }
    let mut response = (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Basic realm=\"CC Switch\", charset=\"UTF-8\""),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_auth_matches_only_exact_credentials() {
        let credentials = Credentials {
            username: "admin".into(),
            password: "s3cret".into(),
        };
        let good = base64::engine::general_purpose::STANDARD.encode("admin:s3cret");
        let bad = base64::engine::general_purpose::STANDARD.encode("admin:wrong");
        assert!(credentials.matches(&format!("Basic {good}")));
        assert!(!credentials.matches(&format!("Basic {bad}")));
        assert!(!credentials.matches("Bearer token"));
        assert!(!credentials.matches("Basic !!!"));
    }
}
