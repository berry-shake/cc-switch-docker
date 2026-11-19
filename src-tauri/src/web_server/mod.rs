//! cc-switch-server：无界面的用量统计服务，面向服务器 / Docker / WSL 环境。
//!
//! 复用桌面版的会话日志扫描（`session_usage::sync_all_unlocked`）、SQLite 存储和
//! 统计查询，把用量页需要的命令通过 HTTP 暴露给 Web 面板（`pnpm build:web` 的产物）。
//!
//! 只读扫描各 CLI 的会话目录，不写任何客户端配置；数据库在 `$HOME/.cc-switch/cc-switch.db`。
//!
//! ```text
//! GET  /api/health          健康检查（不需要认证）
//! GET  /api/session         会话状态（不需要认证）
//! POST /api/login           登录，下发会话 Cookie（不需要认证）
//! POST /api/logout          清除会话 Cookie
//! POST /api/invoke/:cmd     与 Tauri invoke 同名同参的命令调用
//! GET  /api/events          SSE，推送 usage-log-recorded / session-index-status
//! GET  /*                   Web 面板静态文件
//! ```

mod api;
mod assets;
mod auth;
mod logger;
mod sessions;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::routing::{get, post};
use axum::Router;
use serde_json::Value;
use tokio::sync::{broadcast, watch};

use crate::database::Database;
use crate::services::session_usage::{self, SessionSyncResult};

/// 前端监听的用量刷新事件名，与桌面版 `usage_events::EVENT_USAGE_LOG_RECORDED` 一致。
pub(crate) const EVENT_USAGE_LOG_RECORDED: &str = "usage-log-recorded";

/// 经 SSE 推给前端的事件，对应桌面版的 `app.emit(name, payload)`。
#[derive(Clone, Debug)]
pub(crate) struct ServerEvent {
    pub name: &'static str,
    pub payload: Value,
}

const DEFAULT_LISTEN: &str = "127.0.0.1:8765";
const DEFAULT_SYNC_INTERVAL_SECS: u64 = 10 * 60;
const MAINTENANCE_INTERVAL_SECS: u64 = 24 * 60 * 60;
/// 收到退出信号后最多等这么久让在途请求结束，超时直接退出（docker stop 默认只给 10 秒）。
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

const HELP: &str = "\
cc-switch-server — 无界面的 AI CLI 用量统计服务（CC Switch Web 版）

用法: cc-switch-server [--help] [--version] [--healthcheck]

  --healthcheck  请求本机 /api/health，成功退出码 0（给容器健康检查用）

环境变量:
  CC_SWITCH_WEB_LISTEN          监听地址，默认 127.0.0.1:8765（Docker 镜像里是 0.0.0.0:8765）
  CC_SWITCH_WEB_USERNAME        登录用户名，默认 admin
  CC_SWITCH_WEB_PASSWORD        登录密码；不设置则不启用登录
  CC_SWITCH_WEB_INSECURE        设为 1 时允许在非回环地址上不设密码运行
  CC_SWITCH_WEB_DIST            Web 面板静态文件目录，默认可执行文件旁的 web/，其次 ./dist-web
  CC_SWITCH_SYNC_INTERVAL_SECS  后台扫描会话日志的间隔秒数，默认 600；0 表示只在启动时扫描
  CC_SWITCH_CLAUDE_EXTRA_DIRS   额外的 Claude 配置目录（含 projects/），多个用 : 分隔
  CC_SWITCH_LOG_LEVEL           日志级别 error|warn|info|debug|trace，默认 info

数据目录: $HOME/.cc-switch（数据库 cc-switch.db、设置 settings.json）
会话目录: 与桌面版相同，默认 $HOME/.claude、$HOME/.codex、$HOME/.gemini 等
";

/// 服务配置，全部来自环境变量。
#[derive(Clone, Debug)]
pub(crate) struct ServerConfig {
    pub listen: SocketAddr,
    pub credentials: Option<auth::Credentials>,
    pub dist_dir: Option<PathBuf>,
    pub sync_interval: Option<Duration>,
}

impl ServerConfig {
    fn from_env() -> Result<Self, String> {
        let listen_raw =
            env_nonempty("CC_SWITCH_WEB_LISTEN").unwrap_or_else(|| DEFAULT_LISTEN.into());
        let listen: SocketAddr = listen_raw
            .parse()
            .map_err(|e| format!("CC_SWITCH_WEB_LISTEN 无效（{listen_raw}）: {e}"))?;

        let credentials =
            env_nonempty("CC_SWITCH_WEB_PASSWORD").map(|password| auth::Credentials {
                username: env_nonempty("CC_SWITCH_WEB_USERNAME").unwrap_or_else(|| "admin".into()),
                password,
            });
        if credentials.is_none() && !listen.ip().is_loopback() {
            let insecure = env_nonempty("CC_SWITCH_WEB_INSECURE")
                .is_some_and(|v| matches!(v.as_str(), "1" | "true" | "yes"));
            if !insecure {
                return Err(format!(
                    "监听在非回环地址 {listen} 时必须设置 CC_SWITCH_WEB_PASSWORD；\
                     确认不需要认证（例如前面有带认证的反向代理）请设置 CC_SWITCH_WEB_INSECURE=1"
                ));
            }
            log::warn!("未设置 CC_SWITCH_WEB_PASSWORD，{listen} 上的面板和接口没有认证保护");
        }

        let sync_secs = match env_nonempty("CC_SWITCH_SYNC_INTERVAL_SECS") {
            Some(raw) => raw
                .parse::<u64>()
                .map_err(|e| format!("CC_SWITCH_SYNC_INTERVAL_SECS 无效（{raw}）: {e}"))?,
            None => DEFAULT_SYNC_INTERVAL_SECS,
        };

        Ok(Self {
            listen,
            credentials,
            dist_dir: assets::resolve_dist_dir(env_nonempty("CC_SWITCH_WEB_DIST")),
            sync_interval: (sync_secs > 0).then(|| Duration::from_secs(sync_secs.max(30))),
        })
    }
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

/// 各请求共享的服务状态。
pub(crate) struct ServerContext {
    pub db: Arc<Database>,
    pub events: broadcast::Sender<ServerEvent>,
    /// 进程准备退出时变为 true；SSE 等长连接据此主动结束，优雅关闭才不会被它们卡住。
    pub shutdown: watch::Receiver<bool>,
}

impl ServerContext {
    fn notify_usage_changed(&self) {
        // 没有订阅者时 send 返回 Err，属正常情况
        let _ = self.events.send(ServerEvent {
            name: EVENT_USAGE_LOG_RECORDED,
            payload: Value::Null,
        });
    }

    /// 扫描一轮会话日志。与桌面版共用同一把同步锁，避免手动同步与后台扫描并发。
    pub(crate) async fn sync_sessions(&self, backfill: bool) -> Result<SessionSyncResult, String> {
        let _guard = session_usage::session_sync_mutex().lock().await;
        let db = self.db.clone();
        let result = tokio::task::spawn_blocking(move || {
            if backfill {
                if let Err(error) = db.backfill_missing_usage_costs() {
                    log::warn!("Usage cost startup backfill failed: {error}");
                }
            }
            session_usage::sync_all_unlocked(&db)
        })
        .await
        .map_err(|error| format!("会话用量同步任务失败: {error}"))?;

        if !result.errors.is_empty() {
            log::warn!(
                "Session usage sync finished with {} error(s): {:?}",
                result.errors.len(),
                result.errors
            );
        }
        if result.imported > 0 {
            log::info!(
                "Session usage sync imported {} record(s) from {} file(s)",
                result.imported,
                result.files_scanned
            );
            self.notify_usage_changed();
        }
        Ok(result)
    }

    /// 备份数据库后安全重建 Codex 会话用量（同桌面版 `rebuild_codex_usage`）。
    pub(crate) async fn rebuild_codex_usage(&self) -> Result<SessionSyncResult, String> {
        let _guard = session_usage::session_sync_mutex().lock().await;
        let db = self.db.clone();
        let result = tokio::task::spawn_blocking(move || {
            db.backup_database_file()?;
            crate::services::session_usage_codex::rebuild_codex_usage_preserving_history(&db)
        })
        .await
        .map_err(|error| format!("Codex 用量重建任务失败: {error}"))?
        .map_err(|error| error.to_string())?;
        self.notify_usage_changed();
        Ok(result)
    }
}

/// 进程入口，返回退出码。
pub fn run() -> i32 {
    let mut args = std::env::args().skip(1);
    if let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return 0;
            }
            "-V" | "--version" => {
                println!("cc-switch-server {}", env!("CARGO_PKG_VERSION"));
                return 0;
            }
            "--healthcheck" => return healthcheck(),
            other => {
                eprintln!("未知参数: {other}\n\n{HELP}");
                return 2;
            }
        }
    }

    logger::init(env_nonempty("CC_SWITCH_LOG_LEVEL").as_deref());

    let config = match ServerConfig::from_env() {
        Ok(config) => config,
        Err(error) => {
            log::error!("{error}");
            return 2;
        }
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("cc-switch-server")
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            log::error!("创建 tokio 运行时失败: {error}");
            return 1;
        }
    };
    // 复用到的服务层代码里有 tauri::async_runtime::spawn，让它们跑在同一个运行时上
    tauri::async_runtime::set(runtime.handle().clone());

    match runtime.block_on(serve(config)) {
        Ok(()) => 0,
        Err(error) => {
            log::error!("{error}");
            1
        }
    }
}

/// 不依赖 curl 的健康检查：直连监听端口请求 /api/health。
fn healthcheck() -> i32 {
    use std::io::{Read, Write};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpStream};

    let listen = env_nonempty("CC_SWITCH_WEB_LISTEN")
        .unwrap_or_else(|| DEFAULT_LISTEN.into())
        .parse::<SocketAddr>();
    let Ok(mut addr) = listen else {
        eprintln!("CC_SWITCH_WEB_LISTEN 无效");
        return 1;
    };
    if addr.ip().is_unspecified() {
        addr.set_ip(match addr.ip() {
            IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::LOCALHOST),
        });
    }
    let timeout = Duration::from_secs(3);
    let result = (|| -> std::io::Result<String> {
        let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        stream.write_all(b"GET /api/health HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        Ok(response)
    })();
    match result {
        Ok(response)
            if response.starts_with("HTTP/1.0 200") || response.starts_with("HTTP/1.1 200") =>
        {
            0
        }
        Ok(response) => {
            eprintln!(
                "health check failed: {}",
                response.lines().next().unwrap_or("")
            );
            1
        }
        Err(error) => {
            eprintln!("health check failed: {error}");
            1
        }
    }
}

async fn serve(config: ServerConfig) -> Result<(), String> {
    log::info!("cc-switch-server {} 启动", env!("CARGO_PKG_VERSION"));
    log::info!("Home 目录: {}", crate::config::get_home_dir().display());

    apply_claude_extra_dirs_from_env()?;
    let db = open_database().await?;
    log::info!(
        "数据库: {}",
        crate::config::get_app_config_dir()
            .join("cc-switch.db")
            .display()
    );
    for root in crate::claude_session_roots::roots() {
        log::info!("Claude 会话目录: {}", root.projects_dir.display());
    }

    let (events, _) = broadcast::channel(64);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let ctx = Arc::new(ServerContext {
        db,
        events,
        shutdown: shutdown_rx.clone(),
    });

    spawn_background_tasks(ctx.clone(), config.sync_interval);

    let auth_state = Arc::new(auth::AuthState::new(
        config.credentials.clone(),
        &crate::config::get_app_config_dir(),
    ));

    let api_routes = Router::new()
        .route("/invoke/:cmd", post(api::invoke))
        .route("/events", get(api::events))
        .with_state(ctx.clone());
    let auth_routes = Router::new()
        .route("/session", get(auth::session))
        .route("/login", post(auth::login))
        .route("/logout", post(auth::logout))
        .with_state(auth_state.clone());

    let mut app = Router::new()
        .route("/api/health", get(api::health))
        .nest("/api", api_routes.merge(auth_routes))
        .fallback(
            get(assets::serve)
                .with_state(Arc::new(assets::StaticAssets::new(config.dist_dir.clone()))),
        );

    if auth_state.enabled() {
        // 只拦 /api/*（健康检查、登录相关除外）；页面本身不含数据，由前端显示登录页
        app = app.layer(axum::middleware::from_fn_with_state(
            auth_state,
            auth::require_auth,
        ));
        log::info!("已启用登录认证");
    }

    match &config.dist_dir {
        Some(dir) => log::info!("Web 面板目录: {}", dir.display()),
        None => log::warn!("没有找到 Web 面板静态文件（CC_SWITCH_WEB_DIST），只提供 API"),
    }

    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .map_err(|e| format!("监听 {} 失败: {e}", config.listen))?;
    log::info!("Web 面板: http://{}", config.listen);

    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown_signal().await;
        let _ = shutdown_tx.send(true);
    });
    let mut grace_rx = shutdown_rx;
    let grace_expired = async move {
        let _ = grace_rx.wait_for(|stopping| *stopping).await;
        tokio::time::sleep(SHUTDOWN_GRACE).await;
    };
    tokio::select! {
        result = server => result.map_err(|e| format!("HTTP 服务异常退出: {e}"))?,
        _ = grace_expired => log::warn!("仍有连接未关闭，等待 {SHUTDOWN_GRACE:?} 后强制退出"),
    }
    log::info!("已退出");
    Ok(())
}

/// 打开（必要时创建 / 迁移）数据库。版本过新时拒绝启动，避免旧程序写坏新库。
async fn open_database() -> Result<Arc<Database>, String> {
    tokio::task::spawn_blocking(|| {
        let db_path = crate::config::get_app_config_dir().join("cc-switch.db");
        if let Ok(Some(version)) = Database::stored_user_version_exceeds_supported(&db_path) {
            return Err(format!(
                "数据库版本过新（v{version}），当前程序仅支持 v{}，请升级 cc-switch-server",
                crate::database::SCHEMA_VERSION
            ));
        }
        Database::init()
            .map(Arc::new)
            .map_err(|e| format!("初始化数据库失败: {e}"))
    })
    .await
    .map_err(|e| format!("初始化数据库任务失败: {e}"))?
}

/// `CC_SWITCH_CLAUDE_EXTRA_DIRS` 设置时覆盖设置里的额外 Claude 扫描目录。
fn apply_claude_extra_dirs_from_env() -> Result<(), String> {
    let Some(raw) = env_nonempty("CC_SWITCH_CLAUDE_EXTRA_DIRS") else {
        return Ok(());
    };
    let mut dirs: Vec<String> = raw.split(':').map(str::to_owned).collect();
    crate::claude_session_roots::normalize_directories(&mut dirs);
    crate::claude_session_roots::validate_directories(&dirs)
        .map_err(|e| format!("CC_SWITCH_CLAUDE_EXTRA_DIRS 无效: {e}"))?;
    let mut settings = crate::settings::get_settings();
    if settings.claude_additional_config_dirs != dirs {
        settings.claude_additional_config_dirs = dirs;
        crate::settings::update_settings(settings).map_err(|e| format!("保存设置失败: {e}"))?;
    }
    Ok(())
}

fn spawn_background_tasks(ctx: Arc<ServerContext>, sync_interval: Option<Duration>) {
    // 会话日志扫描：启动时扫一轮（含费用回填），之后按间隔扫描
    let sync_ctx = ctx.clone();
    tokio::spawn(async move {
        if let Err(error) = sync_ctx.sync_sessions(true).await {
            log::warn!("{error}");
        }
        let Some(period) = sync_interval else {
            log::info!("后台定时扫描已关闭（CC_SWITCH_SYNC_INTERVAL_SECS=0）");
            return;
        };
        let mut interval = tokio::time::interval(period);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            interval.tick().await;
            if !crate::settings::get_settings().session_auto_sync_enabled {
                continue;
            }
            if let Err(error) = sync_ctx.sync_sessions(false).await {
                log::warn!("{error}");
            }
        }
    });

    // 每日维护：数据库备份、30 天前明细汇总、清理
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(MAINTENANCE_INTERVAL_SECS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let db = ctx.db.clone();
            match tokio::task::spawn_blocking(move || db.periodic_backup_if_needed()).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log::warn!("Periodic maintenance failed: {e}"),
                Err(e) => log::warn!("Periodic maintenance task failed: {e}"),
            }
        }
    });
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    log::info!("收到退出信号，正在关闭");
}
