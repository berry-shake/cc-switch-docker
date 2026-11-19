//! 简单的 stderr 日志（桌面版用 tauri-plugin-log，服务端不加载 Tauri 插件）。

use log::{LevelFilter, Log, Metadata, Record};

struct StderrLogger;

/// 依赖库的 debug 日志太吵，只在 trace 级别放行。
const NOISY_TARGETS: &[&str] = &["hyper", "reqwest", "rustls", "h2", "tower", "mio"];

impl Log for StderrLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        if metadata.level() > log::Level::Info
            && log::max_level() < LevelFilter::Trace
            && NOISY_TARGETS
                .iter()
                .any(|target| metadata.target().starts_with(target))
        {
            return false;
        }
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        eprintln!(
            "{} {:<5} [{}] {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            record.level(),
            record.target(),
            record.args()
        );
    }

    fn flush(&self) {}
}

static LOGGER: StderrLogger = StderrLogger;

pub(super) fn init(level: Option<&str>) {
    let filter = match level.map(str::to_ascii_lowercase).as_deref() {
        Some("error") => LevelFilter::Error,
        Some("warn") => LevelFilter::Warn,
        Some("debug") => LevelFilter::Debug,
        Some("trace") => LevelFilter::Trace,
        Some("off") => LevelFilter::Off,
        _ => LevelFilter::Info,
    };
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(filter);
    }
}
