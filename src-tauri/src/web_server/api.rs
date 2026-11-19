//! `/api/*` 处理：把 Tauri 命令映射成 HTTP 调用。
//!
//! 参数和返回值与桌面版命令保持一致（参数名 camelCase，同 Tauri invoke 的约定），
//! Web 面板的 `@tauri-apps/api/core` shim 原样转发 invoke 即可。
//! 只开放用量页用到的命令；其余命令返回 404。

use std::convert::Infallible;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::Stream;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::ServerContext;
use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::services::model_pricing::{ModelPricingInfo, ModelsDevSyncConfig};
use crate::services::usage_stats::LogFilters;

enum InvokeError {
    UnknownCommand(String),
    BadArgs(String),
    Failed(String),
}

impl From<AppError> for InvokeError {
    fn from(error: AppError) -> Self {
        Self::Failed(error.to_string())
    }
}

impl IntoResponse for InvokeError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::UnknownCommand(cmd) => {
                (StatusCode::NOT_FOUND, format!("Web 版不支持该操作: {cmd}"))
            }
            Self::BadArgs(message) => (StatusCode::BAD_REQUEST, message),
            Self::Failed(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
        };
        (status, Json(Value::String(message))).into_response()
    }
}

type InvokeResult = Result<Value, InvokeError>;

pub(super) async fn health() -> Json<Value> {
    Json(json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") }))
}

pub(super) async fn invoke(
    State(ctx): State<Arc<ServerContext>>,
    Path(cmd): Path<String>,
    body: Bytes,
) -> Response {
    let args = if body.iter().all(u8::is_ascii_whitespace) {
        Value::Object(Default::default())
    } else {
        match serde_json::from_slice::<Value>(&body) {
            Ok(value) => value,
            Err(error) => {
                return InvokeError::BadArgs(format!("请求体不是合法 JSON: {error}"))
                    .into_response()
            }
        }
    };
    match dispatch(&ctx, &cmd, args).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => {
            if let InvokeError::Failed(message) = &error {
                log::warn!("command {cmd} failed: {message}");
            }
            error.into_response()
        }
    }
}

/// SSE：把后端事件以 `{"event": name, "payload": null}` 的形式推给前端。
pub(super) async fn events(
    State(ctx): State<Arc<ServerContext>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut receiver = ctx.events.subscribe();
    let mut shutdown = ctx.shutdown.clone();
    let stream = async_stream::stream! {
        loop {
            // 服务退出时结束流，否则优雅关闭会一直等这条长连接
            let received = tokio::select! {
                received = receiver.recv() => received,
                _ = shutdown.wait_for(|stopping| *stopping) => break,
            };
            let name = match received {
                Ok(name) => name,
                // 落后太多被跳过的消息只是刷新通知，合并成一次即可
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    super::EVENT_USAGE_LOG_RECORDED
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };
            let data = json!({ "event": name, "payload": null }).to_string();
            yield Ok(Event::default().data(data));
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

fn parse<T: DeserializeOwned>(args: Value) -> Result<T, InvokeError> {
    serde_json::from_value(args).map_err(|error| InvokeError::BadArgs(format!("参数错误: {error}")))
}

fn to_value<T: Serialize>(value: T) -> InvokeResult {
    serde_json::to_value(value).map_err(|error| InvokeError::Failed(error.to_string()))
}

/// 数据库查询放到阻塞线程池（同桌面版 `run_db_query`）。
async fn blocking<T, F>(ctx: &ServerContext, query: F) -> InvokeResult
where
    T: Serialize + Send + 'static,
    F: FnOnce(&Database) -> Result<T, AppError> + Send + 'static,
{
    let db = ctx.db.clone();
    let result = tokio::task::spawn_blocking(move || query(&db))
        .await
        .map_err(|error| InvokeError::Failed(format!("用量查询任务失败: {error}")))??;
    to_value(result)
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RangeArgs {
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<String>,
    provider_name: Option<String>,
    model: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionSummaryArgs {
    app_type: String,
    session_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequestLogsArgs {
    #[serde(default)]
    filters: LogFilters,
    page: u32,
    page_size: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequestDetailArgs {
    request_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdatePricingArgs {
    model_id: String,
    display_name: String,
    input_cost: String,
    output_cost: String,
    cache_read_cost: String,
    cache_creation_cost: String,
}

#[derive(Deserialize)]
struct PricingBatchArgs {
    entries: Vec<ModelPricingInfo>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelIdArgs {
    model_id: String,
}

#[derive(Deserialize)]
struct ModelsDevConfigArgs {
    config: ModelsDevSyncConfig,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ModelsDevResultArgs {
    synced_at: Option<i64>,
    error: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PricingSourceArgs {
    app_type: String,
    #[serde(default)]
    value: Option<String>,
}

#[derive(Deserialize)]
struct SaveSettingsArgs {
    settings: crate::settings::AppSettings,
}

fn app_type(raw: &str) -> Result<AppType, InvokeError> {
    AppType::from_str(raw).map_err(|error| InvokeError::BadArgs(error.to_string()))
}

async fn dispatch(ctx: &ServerContext, cmd: &str, args: Value) -> InvokeResult {
    match cmd {
        // ── 统计查询 ────────────────────────────────────────────────
        "get_usage_summary" => {
            let a: RangeArgs = parse(args)?;
            blocking(ctx, move |db| {
                db.get_usage_summary(
                    a.start_date,
                    a.end_date,
                    a.app_type.as_deref(),
                    a.provider_name.as_deref(),
                    a.model.as_deref(),
                )
            })
            .await
        }
        "get_session_usage_summary" => {
            let a: SessionSummaryArgs = parse(args)?;
            blocking(ctx, move |db| {
                db.get_session_usage_summary(&a.app_type, &a.session_id)
            })
            .await
        }
        "get_usage_summary_by_app" => {
            let a: RangeArgs = parse(args)?;
            blocking(ctx, move |db| {
                db.get_usage_summary_by_app(
                    a.start_date,
                    a.end_date,
                    a.provider_name.as_deref(),
                    a.model.as_deref(),
                )
            })
            .await
        }
        "get_usage_trends" => {
            let a: RangeArgs = parse(args)?;
            blocking(ctx, move |db| {
                db.get_daily_trends(
                    a.start_date,
                    a.end_date,
                    a.app_type.as_deref(),
                    a.provider_name.as_deref(),
                    a.model.as_deref(),
                )
            })
            .await
        }
        "get_usage_first_date" => {
            let a: RangeArgs = parse(args)?;
            blocking(ctx, move |db| {
                db.get_first_usage_date(
                    a.app_type.as_deref(),
                    a.provider_name.as_deref(),
                    a.model.as_deref(),
                )
            })
            .await
        }
        "get_provider_stats" => {
            let a: RangeArgs = parse(args)?;
            blocking(ctx, move |db| {
                db.get_provider_stats(
                    a.start_date,
                    a.end_date,
                    a.app_type.as_deref(),
                    a.provider_name.as_deref(),
                    a.model.as_deref(),
                )
            })
            .await
        }
        "get_model_stats" => {
            let a: RangeArgs = parse(args)?;
            blocking(ctx, move |db| {
                db.get_model_stats(
                    a.start_date,
                    a.end_date,
                    a.app_type.as_deref(),
                    a.provider_name.as_deref(),
                    a.model.as_deref(),
                )
            })
            .await
        }
        "get_request_logs" => {
            let a: RequestLogsArgs = parse(args)?;
            blocking(ctx, move |db| {
                db.get_request_logs(&a.filters, a.page, a.page_size)
            })
            .await
        }
        "get_request_detail" => {
            let a: RequestDetailArgs = parse(args)?;
            blocking(ctx, move |db| db.get_request_detail(&a.request_id)).await
        }

        // ── 会话日志扫描 ────────────────────────────────────────────
        "sync_session_usage" => to_value(
            ctx.sync_sessions(false)
                .await
                .map_err(InvokeError::Failed)?,
        ),
        "get_session_usage_last_sync" => {
            to_value(crate::services::session_usage::last_sync_completed_at())
        }
        "rebuild_codex_usage" => to_value(
            ctx.rebuild_codex_usage()
                .await
                .map_err(InvokeError::Failed)?,
        ),

        // ── 模型定价 ────────────────────────────────────────────────
        "get_model_pricing" => blocking(ctx, crate::commands::get_model_pricing_blocking).await,
        "update_model_pricing" => {
            let a: UpdatePricingArgs = parse(args)?;
            blocking(ctx, move |db| {
                crate::services::model_pricing::update_model_pricing(
                    db,
                    ModelPricingInfo {
                        model_id: a.model_id,
                        display_name: a.display_name,
                        input_cost_per_million: a.input_cost,
                        output_cost_per_million: a.output_cost,
                        cache_read_cost_per_million: a.cache_read_cost,
                        cache_creation_cost_per_million: a.cache_creation_cost,
                    },
                )?;
                Ok(())
            })
            .await
        }
        "update_model_pricing_batch" => {
            let a: PricingBatchArgs = parse(args)?;
            blocking(ctx, move |db| {
                crate::services::model_pricing::update_model_pricing_batch(db, a.entries)
            })
            .await
        }
        "delete_model_pricing" => {
            let a: ModelIdArgs = parse(args)?;
            blocking(ctx, move |db| {
                crate::services::model_pricing::delete_model_pricing(db, &a.model_id)
            })
            .await
        }
        "get_models_dev_sync_config" => {
            blocking(
                ctx,
                crate::services::model_pricing::get_models_dev_sync_state,
            )
            .await
        }
        "save_models_dev_sync_config" => {
            let a: ModelsDevConfigArgs = parse(args)?;
            blocking(ctx, move |db| {
                crate::services::model_pricing::save_models_dev_sync_config(db, a.config)
            })
            .await
        }
        "record_models_dev_sync_result" => {
            let a: ModelsDevResultArgs = parse(args)?;
            blocking(ctx, move |db| {
                crate::services::model_pricing::record_models_dev_sync_result(
                    db,
                    a.synced_at,
                    a.error,
                )
            })
            .await
        }
        "fetch_models_dev" => to_value(
            crate::services::model_pricing::fetch_models_dev()
                .await
                .map_err(InvokeError::Failed)?,
        ),
        "get_pricing_model_source" => {
            let a: PricingSourceArgs = parse(args)?;
            to_value(
                ctx.db
                    .get_pricing_model_source(&app_type(&a.app_type)?)
                    .await?,
            )
        }
        "set_pricing_model_source" => {
            let a: PricingSourceArgs = parse(args)?;
            let value = a
                .value
                .ok_or_else(|| InvokeError::BadArgs("缺少参数 value".into()))?;
            ctx.db
                .set_pricing_model_source(&app_type(&a.app_type)?, &value)
                .await?;
            Ok(Value::Null)
        }

        // ── 汇率 / 外部额度 ──────────────────────────────────────────
        "fetch_exchange_rates" => {
            to_value(crate::services::exchange_rates::fetch_usd_rates().await?)
        }
        "get_codex_quota_snapshot" => to_value(
            crate::services::codex_official_usage::query_codex_quota_snapshot()
                .await
                .map_err(InvokeError::Failed)?,
        ),
        "get_codex_official_usage_snapshot" => to_value(
            crate::services::codex_official_usage::query_codex_official_usage_snapshot()
                .await
                .map_err(InvokeError::Failed)?,
        ),

        // ── 设置（只读全部；只允许写用量页自己的几项）────────────────
        "get_settings" => to_value(crate::settings::get_settings_for_frontend()),
        "save_settings" => {
            let a: SaveSettingsArgs = parse(args)?;
            let mut next = crate::settings::get_settings();
            next.usage_display = a.settings.usage_display;
            next.usage_dashboard_refresh_interval_ms =
                a.settings.usage_dashboard_refresh_interval_ms;
            next.session_auto_sync_enabled = a.settings.session_auto_sync_enabled;
            crate::settings::update_settings(next)?;
            Ok(Value::Bool(true))
        }
        // 用量页「数据来源」只读显示「记录请求用量」开关；服务端没有代理，统计全部来自会话日志
        "get_global_proxy_config" => to_value(ctx.db.get_global_proxy_config().await?),

        _ => Err(InvokeError::UnknownCommand(cmd.to_owned())),
    }
}
