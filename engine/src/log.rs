/// 轻量日志模块 — 零依赖文件日志
///
/// 对应 SPEC: docs/modules/reliability/SPEC.md
/// 覆盖 DEV-TRACKER: 0.1.10 FFI panic 守卫 + 日志
///
/// 日志文件：%APPDATA%/taishen-ime/logs/engine.log
/// 级别：INFO（生命周期事件）/ ERROR（错误与 panic 捕获）
/// 二期换 tracing crate（依赖重，MVP 用零依赖实现）。
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

/// 日志文件句柄（全局单例）
static LOG_FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

/// 获取 %APPDATA%/taishen-ime/logs 目录（Windows）
fn logs_dir() -> Option<PathBuf> {
    let appdata = std::env::var("APPDATA").ok()?;
    let dir = PathBuf::from(appdata).join("taishen-ime").join("logs");
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// 当前时间戳（UTC+8，本地时间），格式 HH:MM:SS.mmm
fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();

    // 本地时间（系统时区）
    let local = std::time::SystemTime::now();
    let _ = local;

    // 简单方式：UNIX 秒 → 时区偏移（+8）
    let utc_secs = secs + 8 * 3600; // UTC+8
    let days = utc_secs / 86400;
    let rem = utc_secs % 86400;
    let hh = rem / 3600;
    let mm = (rem % 3600) / 60;
    let ss = rem % 60;

    let _ = days; // 不显示日期，MVP 简化
    format!("{hh:02}:{mm:02}:{ss:02}.{millis:03}")
}

/// 初始化日志（打开文件，失败静默——日志不可用时不影响引擎）
pub fn init() {
    let mut guard = LOG_FILE.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_some() {
        return;
    }
    if let Some(dir) = logs_dir() {
        if let Ok(file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("engine.log"))
        {
            *guard = Some(file);
        }
    }
}

/// 写入一行日志
fn write(level: &str, msg: &str) {
    let line = format!("[{}] [{}] {}\n", timestamp(), level, msg);
    // 写文件
    if let Ok(mut guard) = LOG_FILE.lock() {
        if let Some(file) = guard.as_mut() {
            let _ = file.write_all(line.as_bytes());
            let _ = file.flush();
        }
    }
    // 同时输出到 stderr（调试可见）
    eprint!("{}", line);
}

/// INFO 级日志
pub fn info(msg: &str) {
    write("INFO", msg);
}

/// ERROR 级日志
pub fn error(msg: &str) {
    write("ERROR", msg);
}
