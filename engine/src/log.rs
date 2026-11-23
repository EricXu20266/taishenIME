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

#[cfg(windows)]
#[repr(C)]
struct CTm {
    tm_sec: i32,
    tm_min: i32,
    tm_hour: i32,
    tm_mday: i32,
    tm_mon: i32,
    tm_year: i32,
    tm_wday: i32,
    tm_yday: i32,
    tm_isdst: i32,
}

#[cfg(windows)]
unsafe extern "C" {
    fn _localtime64_s(tm: *mut CTm, time: *const i64) -> i32;
}

#[cfg(unix)]
#[repr(C)]
struct CTm {
    tm_sec: i32,
    tm_min: i32,
    tm_hour: i32,
    tm_mday: i32,
    tm_mon: i32,
    tm_year: i32,
    tm_wday: i32,
    tm_yday: i32,
    tm_isdst: i32,
    tm_gmtoff: std::os::raw::c_long,
    tm_zone: *const std::os::raw::c_char,
}

#[cfg(unix)]
unsafe extern "C" {
    fn localtime_r(time: *const i64, tm: *mut CTm) -> *mut CTm;
}

/// 将 Unix 秒转换为本地时分秒；不支持的系统或转换失败回退 UTC。
fn local_hms(secs: u64) -> (u32, u32, u32) {
    #[cfg(windows)]
    {
        if let Ok(t) = i64::try_from(secs) {
            let mut tm = CTm {
                tm_sec: 0,
                tm_min: 0,
                tm_hour: 0,
                tm_mday: 0,
                tm_mon: 0,
                tm_year: 0,
                tm_wday: 0,
                tm_yday: 0,
                tm_isdst: 0,
            };
            // SAFETY: tm 与 t 均为有效栈变量指针，符合 UCRT _localtime64_s 契约。
            if unsafe { _localtime64_s(&mut tm, &t) } == 0
                && (0..24).contains(&tm.tm_hour)
                && (0..60).contains(&tm.tm_min)
                && (0..=60).contains(&tm.tm_sec)
            {
                return (
                    tm.tm_hour as u32,
                    tm.tm_min as u32,
                    (tm.tm_sec.min(59)) as u32,
                );
            }
        }
    }
    #[cfg(unix)]
    {
        if let Ok(t) = i64::try_from(secs) {
            let mut tm = CTm {
                tm_sec: 0,
                tm_min: 0,
                tm_hour: 0,
                tm_mday: 0,
                tm_mon: 0,
                tm_year: 0,
                tm_wday: 0,
                tm_yday: 0,
                tm_isdst: 0,
                tm_gmtoff: 0,
                tm_zone: std::ptr::null(),
            };
            // SAFETY: tm 与 t 均为有效栈变量指针，符合 POSIX localtime_r 契约。
            if !unsafe { localtime_r(&t, &mut tm) }.is_null()
                && (0..24).contains(&tm.tm_hour)
                && (0..60).contains(&tm.tm_min)
                && (0..=60).contains(&tm.tm_sec)
            {
                return (
                    tm.tm_hour as u32,
                    tm.tm_min as u32,
                    (tm.tm_sec.min(59)) as u32,
                );
            }
        }
    }
    let rem = secs % 86400;
    (
        (rem / 3600) as u32,
        ((rem % 3600) / 60) as u32,
        (rem % 60) as u32,
    )
}

/// 当前本地时间戳，格式 HH:MM:SS.mmm
fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    let (hh, mm, ss) = local_hms(secs);
    format!("{hh:02}:{mm:02}:{ss:02}.{millis:03}")
}

fn stderr_env_enabled(env_val: Option<&str>) -> bool {
    matches!(env_val, Some(v) if v == "1" || v.eq_ignore_ascii_case("true"))
}

/// 初始化日志（打开文件，失败静默——日志不可用时不影响引擎）
pub fn init() {
    let mut guard = LOG_FILE.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_some() {
        return;
    }
    if let Some(dir) = logs_dir()
        && let Ok(file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("engine.log"))
    {
        *guard = Some(file);
    }
}

/// 写入一行日志
fn write(level: &str, msg: &str) {
    let line = format!("[{}] [{}] {}\n", timestamp(), level, msg);
    // 写文件
    if let Ok(mut guard) = LOG_FILE.lock()
        && let Some(file) = guard.as_mut()
    {
        let _ = file.write_all(line.as_bytes());
        let _ = file.flush();
    }
    // 仅测试构建或显式设置 TAISHEN_STDERR_LOG=1 时输出到 stderr，避免污染控制台宿主
    if cfg!(test) || stderr_env_enabled(std::env::var("TAISHEN_STDERR_LOG").ok().as_deref()) {
        eprint!("{}", line);
    }
}

/// INFO 级日志
pub fn info(msg: &str) {
    write("INFO", msg);
}

/// WARN 级日志（降级但可继续运行：词库回退内置、容器内功能受限等）
pub fn warn(msg: &str) {
    write("WARN", msg);
}

/// ERROR 级日志
pub fn error(msg: &str) {
    write("ERROR", msg);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stderr_env_flag() {
        assert!(!stderr_env_enabled(None));
        assert!(!stderr_env_enabled(Some("")));
        assert!(!stderr_env_enabled(Some("0")));
        assert!(stderr_env_enabled(Some("1")));
        assert!(stderr_env_enabled(Some("true")));
        assert!(stderr_env_enabled(Some("TRUE")));
    }

    #[test]
    fn test_timestamp_format_hms_millis() {
        let ts = timestamp();
        assert_eq!(ts.len(), 12, "timestamp={ts}");
        let bytes = ts.as_bytes();
        assert_eq!(bytes[2], b':');
        assert_eq!(bytes[5], b':');
        assert_eq!(bytes[8], b'.');
        let hh: u32 = ts[0..2].parse().expect("hours");
        let mm: u32 = ts[3..5].parse().expect("minutes");
        let ss: u32 = ts[6..8].parse().expect("seconds");
        assert!(hh < 24 && mm < 60 && ss < 60, "invalid time {ts}");
    }
}
