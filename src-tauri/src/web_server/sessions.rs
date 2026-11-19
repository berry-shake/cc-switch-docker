//! 会话浏览（只读）：会话列表、正文、工具输出全文、图片、正文搜索。
//!
//! 与桌面版 `commands::session_manager` 同名同参。删除会话、在终端恢复、
//! 在文件管理器中显示这些改动本机或依赖桌面环境的操作不开放（返回 404）。
//!
//! 前端传来的 `sourcePath` 一律经 `content::validate_source` 校验：解析符号链接后
//! 必须落在该应用的会话目录内，读不到目录外的任意文件。

use std::convert::Infallible;

use axum::body::{Body, Bytes};
use axum::http::{header, HeaderValue};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::Value;

use super::api::{parse, to_value, InvokeError, InvokeResult};
use super::{ServerContext, ServerEvent};
use crate::session_manager::{
    self,
    cache::chunk_ranges,
    content,
    model::{ContentRef, ImageRef, TranscriptChunk},
};

/// 正文索引同步进度，与桌面版 `sync_session_index` 发的事件同名。
const EVENT_SESSION_INDEX_STATUS: &str = "session-index-status";

/// 走 JSON 返回值的会话命令（流式正文和图片在 `api::invoke` 里单独处理）。
const JSON_COMMANDS: &[&str] = &[
    "list_sessions",
    "get_session_messages",
    "get_session_block_content",
    "search_session_content",
    "sync_session_index",
    "clear_session_index",
    "get_pi_session_discovery",
    "get_omp_session_discovery",
];

pub(super) fn handles(cmd: &str) -> bool {
    JSON_COMMANDS.contains(&cmd)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceArgs {
    provider_id: String,
    source_path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlockContentArgs {
    provider_id: String,
    source_path: String,
    content_ref: ContentRef,
    #[serde(default)]
    offset: Option<u32>,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImageArgs {
    provider_id: String,
    source_path: String,
    image: ImageRef,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchArgs {
    query: String,
    #[serde(default)]
    provider_ids: Option<Vec<String>>,
    #[serde(default)]
    limit: Option<usize>,
}

/// 会话解析都是文件 IO + CPU，放到阻塞线程池。
async fn run_blocking<T, F>(task: F) -> Result<T, InvokeError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tokio::task::spawn_blocking(task)
        .await
        .map_err(|error| InvokeError::Failed(format!("会话任务失败: {error}")))?
        .map_err(InvokeError::Failed)
}

pub(super) async fn dispatch(ctx: &ServerContext, cmd: &str, args: Value) -> InvokeResult {
    match cmd {
        "list_sessions" => to_value(run_blocking(|| Ok(session_manager::scan_sessions())).await?),
        "get_session_messages" => {
            let a: SourceArgs = parse(args)?;
            let loaded = run_blocking(move || {
                session_manager::load_transcript(&a.provider_id, &a.source_path)
            })
            .await?;
            to_value(&loaded.transcript.messages)
        }
        "get_session_block_content" => {
            let a: BlockContentArgs = parse(args)?;
            to_value(
                run_blocking(move || {
                    let source = content::validate_source(&a.provider_id, &a.source_path)?;
                    let full = content::resolve_content_ref(&source, &a.content_ref)?;
                    Ok(content::paginate(&full, a.offset, a.limit))
                })
                .await?,
            )
        }
        "search_session_content" => {
            let a: SearchArgs = parse(args)?;
            to_value(
                run_blocking(move || {
                    Ok(session_manager::search::search(
                        &a.query,
                        a.provider_ids,
                        a.limit.unwrap_or(session_manager::search::DEFAULT_LIMIT),
                    ))
                })
                .await?,
            )
        }
        "sync_session_index" => {
            let events = ctx.events.clone();
            to_value(session_manager::search::start_sync(move |status| {
                if let Ok(payload) = serde_json::to_value(status) {
                    // 没人订阅时 send 返回 Err，属正常情况
                    let _ = events.send(ServerEvent {
                        name: EVENT_SESSION_INDEX_STATUS,
                        payload,
                    });
                }
            }))
        }
        "clear_session_index" => {
            run_blocking(session_manager::search::clear).await?;
            Ok(Value::Bool(true))
        }
        "get_pi_session_discovery" => to_value(
            run_blocking(|| Ok(session_manager::providers::pi::session_discovery())).await?,
        ),
        "get_omp_session_discovery" => to_value(
            run_blocking(|| Ok(session_manager::providers::omp::session_discovery())).await?,
        ),
        _ => Err(InvokeError::UnknownCommand(cmd.to_owned())),
    }
}

/// `stream_session_messages`：桌面版经 Tauri Channel 依次推 `Header` → 若干 `Messages` → `Done`，
/// 这里每个分块写成一行 JSON（NDJSON），前端 shim 边读边转给 Channel 的 onmessage。
///
/// 读取失败时直接返回错误状态码（对应桌面版的 `Error` 包 + reject）。
pub(super) async fn stream_messages(args: Value) -> Response {
    let a: SourceArgs = match parse(args) {
        Ok(a) => a,
        Err(error) => return error.into_response(),
    };
    let loaded = match run_blocking(move || {
        session_manager::load_transcript(&a.provider_id, &a.source_path)
    })
    .await
    {
        Ok(loaded) => loaded,
        Err(error) => return error.into_response(),
    };

    let (tx, mut rx) = tokio::sync::mpsc::channel::<Bytes>(4);
    tokio::task::spawn_blocking(move || {
        let transcript = &loaded.transcript;
        // 浏览器断开后接收端被丢弃，send 失败即停止序列化
        let send = |chunk: TranscriptChunk<'_>| -> bool {
            match serde_json::to_vec(&chunk) {
                Ok(mut line) => {
                    line.push(b'\n');
                    tx.blocking_send(Bytes::from(line)).is_ok()
                }
                Err(error) => {
                    log::warn!("序列化会话分块失败: {error}");
                    false
                }
            }
        };
        if !send(TranscriptChunk::Header {
            total: transcript.messages.len(),
            turns: &transcript.turns,
            cached: loaded.cached,
            parse_ms: loaded.parse_ms,
        }) {
            return;
        }
        for (start, end) in chunk_ranges(&transcript.message_bytes) {
            if !send(TranscriptChunk::Messages {
                start,
                messages: &transcript.messages[start..end],
            }) {
                return;
            }
        }
        send(TranscriptChunk::Done {
            payload_bytes: transcript.approx_bytes as u64,
        });
    });

    let stream = async_stream::stream! {
        while let Some(line) = rx.recv().await {
            yield Ok::<_, Infallible>(line);
        }
    };
    let mut response = Body::from_stream(stream).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// `get_session_image`：返回图片原始字节。一律按 octet-stream 下发（前端按 `ImageRef.mediaType`
/// 自己生成 Blob），浏览器不会把它当页面或 SVG 直接渲染。
pub(super) async fn image(args: Value) -> Response {
    let a: ImageArgs = match parse(args) {
        Ok(a) => a,
        Err(error) => return error.into_response(),
    };
    match run_blocking(move || {
        let source = content::validate_source(&a.provider_id, &a.source_path)?;
        content::load_image(&source, &a.image)
    })
    .await
    {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, "application/octet-stream"),
                (header::CACHE_CONTROL, "no-store"),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            ],
            bytes,
        )
            .into_response(),
        Err(error) => error.into_response(),
    }
}
