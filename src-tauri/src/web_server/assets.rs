//! Web 面板静态文件：SPA 回退到 web.html，带 hash 的 assets 长缓存，支持 gzip。
//!
//! 文件不可变（镜像里随程序一起发布），压缩结果按路径缓存在内存里。

use std::collections::HashMap;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};

const INDEX_FILE: &str = "web.html";

/// 确定静态文件目录：显式配置 > 可执行文件旁的 web/ > 当前目录的 dist-web/。
pub(super) fn resolve_dist_dir(configured: Option<String>) -> Option<PathBuf> {
    if let Some(dir) = configured {
        return Some(PathBuf::from(dir));
    }
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("web")));
    [beside_exe, Some(PathBuf::from("dist-web"))]
        .into_iter()
        .flatten()
        .find(|dir| dir.join(INDEX_FILE).is_file())
}

#[derive(Clone)]
struct CachedFile {
    raw: Arc<Vec<u8>>,
    gzip: Option<Arc<Vec<u8>>>,
}

pub(super) struct StaticAssets {
    root: Option<PathBuf>,
    cache: Mutex<HashMap<PathBuf, CachedFile>>,
}

impl StaticAssets {
    pub(super) fn new(root: Option<PathBuf>) -> Self {
        Self {
            root,
            cache: Mutex::new(HashMap::new()),
        }
    }

    async fn load(&self, path: &Path) -> Option<CachedFile> {
        if let Some(hit) = self.cache.lock().ok()?.get(path).cloned() {
            return Some(hit);
        }
        let raw = tokio::fs::read(path).await.ok()?;
        let gzip = if is_compressible(path) && raw.len() > 1024 {
            gzip(&raw).map(Arc::new)
        } else {
            None
        };
        let file = CachedFile {
            raw: Arc::new(raw),
            gzip,
        };
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(path.to_path_buf(), file.clone());
        }
        Some(file)
    }
}

fn gzip(data: &[u8]) -> Option<Vec<u8>> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).ok()?;
    encoder.finish().ok()
}

fn is_compressible(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("html" | "js" | "mjs" | "css" | "json" | "svg" | "txt" | "map")
    )
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// 把 URL 路径映射到 root 下的相对路径；含 `..` 等越界成分时返回 None。
fn sanitize(uri_path: &str) -> Option<PathBuf> {
    let decoded = percent_decode(uri_path.trim_start_matches('/'))?;
    let mut out = PathBuf::new();
    for component in Path::new(&decoded).components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(out)
}

fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

pub(super) async fn serve(
    State(assets): State<Arc<StaticAssets>>,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    let Some(root) = assets.root.as_ref() else {
        return (
            StatusCode::NOT_FOUND,
            "Web 面板未安装：请设置 CC_SWITCH_WEB_DIST 指向 pnpm build:web 的产物（dist-web）",
        )
            .into_response();
    };
    let Some(relative) = sanitize(uri.path()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };

    let requested = root.join(&relative);
    let is_asset_request = relative.extension().is_some();
    let path = if !relative.as_os_str().is_empty() && requested.is_file() {
        requested
    } else if is_asset_request {
        // 缺失的静态资源直接 404，不回退到页面，避免浏览器把 HTML 当脚本解析
        return StatusCode::NOT_FOUND.into_response();
    } else {
        root.join(INDEX_FILE)
    };

    let Some(file) = assets.load(&path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let accepts_gzip = headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("gzip"));
    let cache_control = if relative.starts_with("assets") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };

    let mut response = match (&file.gzip, accepts_gzip) {
        (Some(gz), true) => {
            let mut response = Response::new(Body::from(gz.as_ref().clone()));
            response
                .headers_mut()
                .insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
            response
        }
        _ => Response::new(Body::from(file.raw.as_ref().clone())),
    };
    let response_headers = response.headers_mut();
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type(&path)),
    );
    response_headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control),
    );
    response_headers.insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_rejects_traversal() {
        assert_eq!(sanitize("/assets/a.js"), Some(PathBuf::from("assets/a.js")));
        assert_eq!(sanitize("/"), Some(PathBuf::new()));
        assert_eq!(sanitize("/../etc/passwd"), None);
        assert_eq!(sanitize("/assets/%2e%2e/%2e%2e/x"), None);
        assert_eq!(sanitize("/a/./b"), Some(PathBuf::from("a/b")));
    }
}
