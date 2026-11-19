//! 登录与会话。
//!
//! - 浏览器：`POST /api/login` 校验用户名密码后下发 HMAC 签名的 HttpOnly Cookie，
//!   之后同源的 fetch / EventSource 自动带上。服务端不存会话状态。
//! - 脚本：`/api/*` 仍接受 `Authorization: Basic`（`curl -u`），但 401 不再带
//!   `WWW-Authenticate`，浏览器不会弹原生登录框。
//! - 浏览器发来的请求（带 `Sec-Fetch-Mode`）只认 Cookie：用过旧版 Basic 认证的浏览器
//!   会一直自动附带缓存的凭据，如果也认，「退出登录」就退不掉。
//!
//! 签名密钥 = HMAC(数据目录里持久化的随机密钥, 用户名 + 密码)：重启服务会话仍有效，
//! 改密码后旧会话全部失效。

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

const COOKIE_NAME: &str = "ccs_session";
const TOKEN_VERSION: &str = "v1";
/// 勾选「保持登录」：持久 Cookie，30 天
const REMEMBER_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// 不勾选：浏览器关闭即失效的会话 Cookie，服务端最多认 12 小时
const SESSION_TTL: Duration = Duration::from_secs(12 * 60 * 60);
/// 同一 IP 在窗口内失败这么多次后暂时拒绝登录
const MAX_FAILURES: u32 = 10;
const FAILURE_WINDOW: Duration = Duration::from_secs(15 * 60);
/// 每次失败都慢一点回应，拖慢暴力尝试
const FAILURE_DELAY: Duration = Duration::from_millis(600);
const SECRET_FILE: &str = "web-session.key";

/// 不需要登录就能访问的接口；其余 `/api/*` 都要登录。静态页面本身不含数据，不拦。
const PUBLIC_API_PATHS: &[&str] = &["/api/health", "/api/session", "/api/login", "/api/logout"];

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
    /// 用户名、密码都比较完，不因前者不匹配提前返回（常量时间）。
    fn verify(&self, username: &str, password: &str) -> bool {
        let user_ok = username.as_bytes().ct_eq(self.username.as_bytes());
        let pass_ok = password.as_bytes().ct_eq(self.password.as_bytes());
        (user_ok & pass_ok).into()
    }

    fn verify_basic_header(&self, header_value: &str) -> bool {
        let Some(encoded) = header_value
            .strip_prefix("Basic ")
            .or_else(|| header_value.strip_prefix("basic "))
        else {
            return false;
        };
        let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded.trim()) else {
            return false;
        };
        let Ok(decoded) = String::from_utf8(decoded) else {
            return false;
        };
        let Some((username, password)) = decoded.split_once(':') else {
            return false;
        };
        self.verify(username, password)
    }
}

struct FailureRecord {
    count: u32,
    first_at: Instant,
}

pub(crate) struct AuthState {
    credentials: Option<Credentials>,
    key: [u8; 32],
    failures: Mutex<HashMap<IpAddr, FailureRecord>>,
}

impl AuthState {
    pub(crate) fn new(credentials: Option<Credentials>, data_dir: &Path) -> Self {
        let key = match &credentials {
            Some(credentials) => {
                let secret = load_or_create_secret(&data_dir.join(SECRET_FILE));
                let mut mac =
                    HmacSha256::new_from_slice(&secret).expect("HMAC accepts any key size");
                mac.update(b"cc-switch-web-session\0");
                mac.update(credentials.username.as_bytes());
                mac.update(b"\0");
                mac.update(credentials.password.as_bytes());
                mac.finalize().into_bytes().into()
            }
            None => [0; 32],
        };
        Self {
            credentials,
            key,
            failures: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.credentials.is_some()
    }

    fn sign(&self, payload: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.key).expect("HMAC accepts any key size");
        mac.update(payload.as_bytes());
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    }

    fn issue_token(&self, remember: bool) -> String {
        let ttl = if remember { REMEMBER_TTL } else { SESSION_TTL };
        let expires = unix_now() + ttl.as_secs();
        let payload = format!("{TOKEN_VERSION}.{expires}.{}", u8::from(remember));
        let signature = self.sign(&payload);
        format!("{payload}.{signature}")
    }

    fn verify_token(&self, token: &str) -> bool {
        let Some((payload, signature)) = token.rsplit_once('.') else {
            return false;
        };
        let mut parts = payload.split('.');
        let (Some(TOKEN_VERSION), Some(expires), Some(_remember), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return false;
        };
        let Ok(expires) = expires.parse::<u64>() else {
            return false;
        };
        if expires <= unix_now() {
            return false;
        }
        let Ok(signature) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(signature)
        else {
            return false;
        };
        let mut mac = HmacSha256::new_from_slice(&self.key).expect("HMAC accepts any key size");
        mac.update(payload.as_bytes());
        mac.verify_slice(&signature).is_ok()
    }

    fn is_authorized(&self, headers: &HeaderMap) -> bool {
        let Some(credentials) = &self.credentials else {
            return true;
        };
        if session_cookie(headers).is_some_and(|token| self.verify_token(token)) {
            return true;
        }
        if is_browser_request(headers) {
            return false;
        }
        headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| credentials.verify_basic_header(value))
    }

    /// 还在锁定期时返回剩余等待时间。
    fn locked_for(&self, ip: IpAddr) -> Option<Duration> {
        let mut failures = self.failures.lock().ok()?;
        let record = failures.get(&ip)?;
        let elapsed = record.first_at.elapsed();
        if elapsed >= FAILURE_WINDOW {
            failures.remove(&ip);
            return None;
        }
        (record.count >= MAX_FAILURES).then(|| FAILURE_WINDOW - elapsed)
    }

    fn record_failure(&self, ip: IpAddr) {
        let Ok(mut failures) = self.failures.lock() else {
            return;
        };
        if failures.len() > 4096 {
            failures.retain(|_, record| record.first_at.elapsed() < FAILURE_WINDOW);
        }
        let record = failures.entry(ip).or_insert(FailureRecord {
            count: 0,
            first_at: Instant::now(),
        });
        if record.first_at.elapsed() >= FAILURE_WINDOW {
            *record = FailureRecord {
                count: 0,
                first_at: Instant::now(),
            };
        }
        record.count += 1;
    }

    fn clear_failures(&self, ip: IpAddr) {
        if let Ok(mut failures) = self.failures.lock() {
            failures.remove(&ip);
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 读取持久化的随机密钥；没有就生成一份（0600）。写不进去时只在内存里用，重启后需重新登录。
fn load_or_create_secret(path: &Path) -> Vec<u8> {
    if let Ok(text) = std::fs::read_to_string(path) {
        let text = text.trim();
        if text.len() >= 64 && text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return text.as_bytes().to_vec();
        }
    }
    // uuid v4 走系统 CSPRNG，两个拼起来有 244 位随机量
    let secret: String = (0..2)
        .map(|_| uuid::Uuid::new_v4().simple().to_string())
        .collect();
    if let Err(error) = write_secret(path, &secret) {
        log::warn!(
            "无法保存登录密钥 {}（{error}），服务重启后需要重新登录",
            path.display()
        );
    }
    secret.into_bytes()
}

fn write_secret(path: &Path, secret: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, secret)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn session_cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|pair| {
            let (name, value) = pair.trim().split_once('=')?;
            (name == COOKIE_NAME).then_some(value)
        })
}

/// 浏览器发起的 fetch / EventSource / 页面导航都带 `Sec-Fetch-Mode`（脚本改不了，
/// curl 等不带），据此区分浏览器和脚本。
fn is_browser_request(headers: &HeaderMap) -> bool {
    headers.contains_key("sec-fetch-mode")
}

/// 经 HTTPS 反向代理访问时给 Cookie 加 Secure。
fn is_https(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("https"))
}

fn cookie_header(value: &str, max_age: Option<Duration>, secure: bool) -> HeaderValue {
    let mut cookie = format!("{COOKIE_NAME}={value}; Path=/; HttpOnly; SameSite=Lax");
    if let Some(max_age) = max_age {
        cookie.push_str(&format!("; Max-Age={}", max_age.as_secs()));
    }
    if secure {
        cookie.push_str("; Secure");
    }
    HeaderValue::from_str(&cookie).expect("cookie is ASCII")
}

fn json_error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(Value::String(message.into()))).into_response()
}

pub(super) async fn require_auth(
    State(auth): State<Arc<AuthState>>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    let needs_auth = path.starts_with("/api/") && !PUBLIC_API_PATHS.contains(&path);
    if !needs_auth || auth.is_authorized(request.headers()) {
        return next.run(request).await;
    }
    json_error(StatusCode::UNAUTHORIZED, "未登录或登录已过期")
}

pub(super) async fn session(State(auth): State<Arc<AuthState>>, headers: HeaderMap) -> Json<Value> {
    let authenticated = auth.is_authorized(&headers);
    Json(json!({
        "authRequired": auth.enabled(),
        "authenticated": authenticated,
        "username": auth
            .credentials
            .as_ref()
            .filter(|_| authenticated)
            .map(|c| c.username.clone()),
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

#[derive(Deserialize)]
pub(super) struct LoginRequest {
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    remember: bool,
}

pub(super) async fn login(
    State(auth): State<Arc<AuthState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> Response {
    let Some(credentials) = &auth.credentials else {
        return json_error(StatusCode::BAD_REQUEST, "服务未启用登录");
    };
    let ip = peer.ip();
    if let Some(wait) = auth.locked_for(ip) {
        let minutes = wait.as_secs().div_ceil(60).max(1);
        return json_error(
            StatusCode::TOO_MANY_REQUESTS,
            format!("登录失败次数过多，请 {minutes} 分钟后再试"),
        );
    }
    if !credentials.verify(&body.username, &body.password) {
        auth.record_failure(ip);
        log::warn!("登录失败：来源 {ip}，用户名 {:?}", body.username);
        tokio::time::sleep(FAILURE_DELAY).await;
        return json_error(StatusCode::UNAUTHORIZED, "用户名或密码错误");
    }
    auth.clear_failures(ip);
    log::info!("登录成功：来源 {ip}");

    let token = auth.issue_token(body.remember);
    let max_age = body.remember.then_some(REMEMBER_TTL);
    let mut response = Json(json!({ "username": credentials.username })).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        cookie_header(&token, max_age, is_https(&headers)),
    );
    response
}

pub(super) async fn logout(headers: HeaderMap) -> Response {
    let mut response = Json(json!({ "ok": true })).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        cookie_header("", Some(Duration::ZERO), is_https(&headers)),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> AuthState {
        let dir = tempfile::tempdir().unwrap();
        AuthState::new(
            Some(Credentials {
                username: "admin".into(),
                password: "s3cret".into(),
            }),
            dir.path(),
        )
    }

    #[test]
    fn credentials_verify_exact_match_only() {
        let credentials = Credentials {
            username: "admin".into(),
            password: "s3cret".into(),
        };
        assert!(credentials.verify("admin", "s3cret"));
        assert!(!credentials.verify("admin", "wrong"));
        assert!(!credentials.verify("root", "s3cret"));
        let good = base64::engine::general_purpose::STANDARD.encode("admin:s3cret");
        let bad = base64::engine::general_purpose::STANDARD.encode("admin:wrong");
        assert!(credentials.verify_basic_header(&format!("Basic {good}")));
        assert!(!credentials.verify_basic_header(&format!("Basic {bad}")));
        assert!(!credentials.verify_basic_header("Bearer token"));
    }

    #[test]
    fn issued_token_verifies_and_tampering_fails() {
        let auth = state();
        let token = auth.issue_token(true);
        assert!(auth.verify_token(&token));
        assert!(!auth.verify_token(&token.replace("v1.", "v2.")));
        let (payload, sig) = token.rsplit_once('.').unwrap();
        let far_future = format!("v1.{}.1", u64::MAX / 2);
        assert!(!auth.verify_token(&format!("{far_future}.{sig}")));
        assert!(!auth.verify_token(payload));
        assert!(!auth.verify_token(""));
    }

    #[test]
    fn expired_token_is_rejected() {
        let auth = state();
        let payload = format!("v1.{}.0", unix_now() - 1);
        let token = format!("{payload}.{}", auth.sign(&payload));
        assert!(!auth.verify_token(&token));
    }

    #[test]
    fn changing_password_invalidates_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let make = |password: &str| {
            AuthState::new(
                Some(Credentials {
                    username: "admin".into(),
                    password: password.into(),
                }),
                dir.path(),
            )
        };
        let token = make("old").issue_token(false);
        assert!(
            make("old").verify_token(&token),
            "same secret file + password"
        );
        assert!(!make("new").verify_token(&token));
    }

    #[test]
    fn lockout_after_repeated_failures() {
        let auth = state();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        for _ in 0..MAX_FAILURES {
            assert!(auth.locked_for(ip).is_none());
            auth.record_failure(ip);
        }
        assert!(auth.locked_for(ip).is_some());
        auth.clear_failures(ip);
        assert!(auth.locked_for(ip).is_none());
    }

    #[test]
    fn browser_requests_ignore_basic_credentials() {
        let auth = state();
        let basic = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode("admin:s3cret")
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&basic).unwrap(),
        );
        assert!(auth.is_authorized(&headers), "scripts may use Basic");

        headers.insert("sec-fetch-mode", HeaderValue::from_static("cors"));
        assert!(
            !auth.is_authorized(&headers),
            "browser-cached Basic must not keep a logged-out browser signed in"
        );

        let cookie = format!("{COOKIE_NAME}={}", auth.issue_token(false));
        headers.insert(header::COOKIE, HeaderValue::from_str(&cookie).unwrap());
        assert!(auth.is_authorized(&headers));
    }

    #[test]
    fn reads_session_cookie_among_others() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("theme=dark; ccs_session=abc.def; other=1"),
        );
        assert_eq!(session_cookie(&headers), Some("abc.def"));
    }
}
