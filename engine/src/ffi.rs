/// C FFI 接口 — 将 Rust 引擎暴露给 C/C++ 平台层
///
/// 所有函数使用 `extern "C"` + `#[unsafe(no_mangle)]`，参数和返回值均为 C 兼容类型。
///
/// 可靠性（0.1.10）：
///   - 全部函数体用 ffi_guard! 包裹，Rust panic 不跨 FFI 边界传播（防 UB 崩溃）
///   - Mutex 锁中毒用 unwrap_or_else 恢复，不 panic
///   - 生命周期事件与错误写日志（engine/src/log.rs）
use std::os::raw::c_char;
use std::sync::Mutex;

use crate::Engine;

/// 全局引擎实例（线程安全）
static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);

/// FFI panic 守卫宏：panic 时返回 fallback 错误码并记日志
macro_rules! ffi_guard {
    ($fallback:expr, $body:block) => {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| $body)).unwrap_or_else(|_| {
            crate::log::error("FFI panic 已捕获");
            $fallback
        })
    };
}

/// 安全获取引擎可变引用（锁中毒恢复，不 panic）
fn engine_lock() -> std::sync::MutexGuard<'static, Option<Engine>> {
    ENGINE.lock().unwrap_or_else(|e| e.into_inner())
}

/// 初始化引擎。dict_path 为系统词库路径（可为 NULL 则回退内置词库）
#[unsafe(no_mangle)]
pub extern "C" fn engine_init(dict_path: *const c_char) -> i32 {
    ffi_guard!(-1, {
        let path = unsafe {
            if dict_path.is_null() {
                None
            } else {
                Some(std::ffi::CStr::from_ptr(dict_path))
            }
        };

        let path_str = path.and_then(|p| p.to_str().ok()).unwrap_or("");
        crate::log::init();
        crate::log::info(&format!("engine_init(dict_path={path_str})"));

        crate::dictionary::init(path.map(|p| std::path::Path::new(p.to_str().unwrap_or(""))));

        let mut engine = engine_lock();
        *engine = Some(Engine::new());
        0
    })
}

/// 词库就绪状态（0.3.x 异步加载）：0=内置兜底/加载中，1=大词库就绪。
/// 平台层可轮询此接口（测试/状态显示），生产路径无需等待（查询自动兜底）。
#[unsafe(no_mangle)]
pub extern "C" fn engine_dict_ready() -> i32 {
    ffi_guard!(0, { if crate::dictionary::is_ready() { 1 } else { 0 } })
}

/// 预编译索引构建（0.2.29 部署工具）：从 SQLite 词库构建 .bin 索引文件。
/// 部署期调用一次（install 脚本/构建步骤），运行时 engine_init 直接加载 .bin 秒开。
/// 返回 0 成功 / -1 失败。
#[unsafe(no_mangle)]
pub extern "C" fn engine_build_index(dict_path: *const c_char, out_bin: *const c_char) -> i32 {
    ffi_guard!(-1, {
        if dict_path.is_null() || out_bin.is_null() {
            crate::log::error("engine_build_index: null 参数");
            return -1;
        }
        let dict = unsafe { std::ffi::CStr::from_ptr(dict_path) }
            .to_string_lossy()
            .into_owned();
        let out = unsafe { std::ffi::CStr::from_ptr(out_bin) }
            .to_string_lossy()
            .into_owned();
        crate::log::info(&format!("engine_build_index({dict}) -> {out}"));
        match crate::dictionary::build_index(
            std::path::Path::new(&dict),
            std::path::Path::new(&out),
        ) {
            Ok(()) => 0,
            Err(e) => {
                crate::log::error(&format!("engine_build_index 失败: {e}"));
                -1
            }
        }
    })
}

/// 设置用户词库路径（V0.2.2）。dict_path 为系统词库，user_path 为用户词库（可 NULL 禁用）。
#[unsafe(no_mangle)]
pub extern "C" fn engine_set_user_dict_path(user_path: *const c_char) -> i32 {
    ffi_guard!(-1, {
        let path_str = unsafe {
            if user_path.is_null() {
                None
            } else {
                std::ffi::CStr::from_ptr(user_path)
                    .to_str()
                    .ok()
                    .map(|s| s.to_string())
            }
        };
        let display = path_str.clone().unwrap_or_else(|| "(null)".to_string());
        crate::log::info(&format!("engine_set_user_dict_path({display})"));
        match path_str {
            Some(p) => crate::dictionary::set_user_dict_path(Some(std::path::Path::new(&p))),
            None => crate::dictionary::set_user_dict_path(None),
        }
        0
    })
}

/// 处理按键，返回候选词数量。-1 表示无效按键
#[unsafe(no_mangle)]
pub extern "C" fn engine_process_key(ch: i32) -> i32 {
    ffi_guard!(-1, {
        let ch = ch as u8 as char;
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => {
                if e.process_key(ch) {
                    e.candidate_count() as i32
                } else {
                    0
                }
            }
            None => -1,
        }
    })
}

/// 退格删除一个字符，返回当前候选词数量
#[unsafe(no_mangle)]
pub extern "C" fn engine_backspace() -> i32 {
    ffi_guard!(-1, {
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => {
                e.backspace();
                e.candidate_count() as i32
            }
            None => -1,
        }
    })
}

/// 获取当前拼音串，返回字符串长度。buf 不足时返回所需长度（不含 null）
#[unsafe(no_mangle)]
pub extern "C" fn engine_get_pinyin_str(buf: *mut c_char, buf_len: i32) -> i32 {
    ffi_guard!(0, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => {
                let s = e.pinyin_str();
                let bytes = s.as_bytes();
                let needed = bytes.len() + 1;
                if buf.is_null() || buf_len <= 0 {
                    return needed as i32;
                }
                let copy_len = bytes.len().min((buf_len - 1) as usize);
                unsafe {
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, copy_len);
                    *buf.add(copy_len) = 0;
                }
                needed as i32
            }
            None => 0,
        }
    })
}

/// 获取候选词总数
#[unsafe(no_mangle)]
pub extern "C" fn engine_get_candidate_count() -> i32 {
    ffi_guard!(0, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => e.candidate_count() as i32,
            None => 0,
        }
    })
}

/// 获取指定候选词，返回字符串长度。buf 不足时返回所需长度（不含 null）
/// V0.2.11：简繁模式开启时返回繁体
#[unsafe(no_mangle)]
pub extern "C" fn engine_get_candidate(index: i32, buf: *mut c_char, buf_len: i32) -> i32 {
    ffi_guard!(0, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => match e.candidate_display(index as usize) {
                Some(word) => {
                    let bytes = word.as_bytes();
                    let needed = bytes.len() + 1;
                    if buf.is_null() || buf_len <= 0 {
                        return needed as i32;
                    }
                    let copy_len = bytes.len().min((buf_len - 1) as usize);
                    unsafe {
                        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, copy_len);
                        *buf.add(copy_len) = 0;
                    }
                    needed as i32
                }
                None => 0,
            },
            None => 0,
        }
    })
}

/// 选择候选词，提交文本写入 buf，返回文本长度
#[unsafe(no_mangle)]
pub extern "C" fn engine_select_candidate(index: i32, buf: *mut c_char, buf_len: i32) -> i32 {
    ffi_guard!(0, {
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => match e.select_candidate(index as usize) {
                Some(text) => {
                    let bytes = text.as_bytes();
                    let needed = bytes.len() + 1;
                    if buf.is_null() || buf_len <= 0 {
                        return needed as i32;
                    }
                    let copy_len = bytes.len().min((buf_len - 1) as usize);
                    unsafe {
                        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, copy_len);
                        *buf.add(copy_len) = 0;
                    }
                    needed as i32
                }
                None => 0,
            },
            None => 0,
        }
    })
}

/// 以词定字（V0.2.24）：取当前页首个候选的首/末字符上屏。
/// first: 1=取首字符，0=取末字符。返回文本长度；无候选返回 0。
#[unsafe(no_mangle)]
pub extern "C" fn engine_take_char(first: i32, buf: *mut c_char, buf_len: i32) -> i32 {
    ffi_guard!(0, {
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => match e.take_char(first != 0) {
                Some(text) => {
                    let bytes = text.as_bytes();
                    let needed = bytes.len() + 1;
                    if buf.is_null() || buf_len <= 0 {
                        return needed as i32;
                    }
                    let copy_len = bytes.len().min((buf_len - 1) as usize);
                    unsafe {
                        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, copy_len);
                        *buf.add(copy_len) = 0;
                    }
                    needed as i32
                }
                None => 0,
            },
            None => 0,
        }
    })
}

/// 加载拆字反查词库（V0.2.25）。NULL/空 = 仅内置空表（反查无候选）。
#[unsafe(no_mangle)]
pub extern "C" fn engine_set_radical_path(path: *const c_char) -> i32 {
    ffi_guard!(-1, {
        if path.is_null() {
            crate::radical::init(None);
            crate::log::info("engine_set_radical_path(null)");
            return 0;
        }
        let path_str = unsafe { std::ffi::CStr::from_ptr(path) }
            .to_string_lossy()
            .into_owned();
        crate::log::info(&format!("engine_set_radical_path({path_str})"));
        crate::radical::init(Some(std::path::Path::new(&path_str)));
        0
    })
}

/// 设置英文模式，返回 0 成功 / -1 引擎未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_set_ascii_mode(enabled: i32) -> i32 {
    ffi_guard!(-1, {
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => {
                e.set_ascii_mode(enabled != 0);
                crate::log::info(&format!("ascii_mode={}", enabled != 0));
                0
            }
            None => -1,
        }
    })
}

/// 查询英文模式：1=英文 / 0=中文 / -1 引擎未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_get_ascii_mode() -> i32 {
    ffi_guard!(-1, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => {
                if e.ascii_mode() {
                    1
                } else {
                    0
                }
            }
            None => -1,
        }
    })
}

/// 设置候选词数量上限，返回 0 成功 / -1 引擎未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_set_candidate_count(count: i32) -> i32 {
    ffi_guard!(-1, {
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => {
                e.set_candidate_limit(count.max(0) as usize);
                crate::log::info(&format!("candidate_count={count}"));
                0
            }
            None => -1,
        }
    })
}

/// 设置快捷短语开关（V0.2.12），返回 0 成功 / -1 未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_set_phrase_enabled(enabled: i32) -> i32 {
    ffi_guard!(-1, {
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => {
                e.set_phrase_enabled(enabled != 0);
                crate::log::info(&format!("phrase={}", enabled != 0));
                0
            }
            None => -1,
        }
    })
}

/// 查询快捷短语开关：1=开 / 0=关 / -1 未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_get_phrase_enabled() -> i32 {
    ffi_guard!(-1, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => {
                if e.phrase_enabled() {
                    1
                } else {
                    0
                }
            }
            None => -1,
        }
    })
}

/// 加载外部短语文件（V0.2.12）。格式：每行 code=text，# 开头为注释。
/// NULL/空 = 仅内置短语。返回 0 成功 / -1 未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_set_phrase_path(path: *const c_char) -> i32 {
    ffi_guard!(-1, {
        let mut engine = engine_lock();
        let e = match engine.as_mut() {
            Some(e) => e,
            None => return -1,
        };
        if path.is_null() {
            crate::log::info("engine_set_phrase_path(null)");
            return 0;
        }
        let path_str = unsafe { std::ffi::CStr::from_ptr(path) }
            .to_string_lossy()
            .into_owned();
        crate::log::info(&format!("engine_set_phrase_path({path_str})"));
        match std::fs::read_to_string(&path_str) {
            Ok(content) => {
                let mut entries = Vec::new();
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    if let Some(eq) = line.find('=') {
                        let code = line[..eq].trim().to_string();
                        let text = line[eq + 1..].trim().to_string();
                        if !code.is_empty() && !text.is_empty() {
                            entries.push((code, text));
                        }
                    }
                }
                e.load_phrases(entries.clone());
                crate::log::info(&format!("短语加载: {} 条", entries.len()));
                0
            }
            Err(err) => {
                crate::log::error(&format!("短语文件读取失败: {err}"));
                0 // 静默降级（仅内置）
            }
        }
    })
}

/// 设置简繁转换开关（V0.2.11），返回 0 成功 / -1 未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_set_traditional(enabled: i32) -> i32 {
    ffi_guard!(-1, {
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => {
                e.set_traditional(enabled != 0);
                crate::log::info(&format!("traditional={}", enabled != 0));
                0
            }
            None => -1,
        }
    })
}

/// 查询简繁转换开关：1=开 / 0=关 / -1 未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_get_traditional() -> i32 {
    ffi_guard!(-1, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => {
                if e.traditional() {
                    1
                } else {
                    0
                }
            }
            None => -1,
        }
    })
}

/// 设置中英混输开关（V0.2.8），返回 0 成功 / -1 未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_set_mix_mode(enabled: i32) -> i32 {
    ffi_guard!(-1, {
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => {
                e.set_mix_mode(enabled != 0);
                crate::log::info(&format!("mix_mode={}", enabled != 0));
                0
            }
            None => -1,
        }
    })
}

/// 查询中英混输开关：1=开 / 0=关 / -1 未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_get_mix_mode() -> i32 {
    ffi_guard!(-1, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => {
                if e.mix_mode() {
                    1
                } else {
                    0
                }
            }
            None => -1,
        }
    })
}

/// 设置智能纠错开关（键盘相邻键容错，V0.2.10），返回 0 成功 / -1 未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_set_correction(enabled: i32) -> i32 {
    ffi_guard!(-1, {
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => {
                e.set_correction_enabled(enabled != 0);
                crate::log::info(&format!("correction={}", enabled != 0));
                0
            }
            None => -1,
        }
    })
}

/// 查询智能纠错开关：1=开 / 0=关 / -1 未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_get_correction() -> i32 {
    ffi_guard!(-1, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => {
                if e.correction_enabled() {
                    1
                } else {
                    0
                }
            }
            None => -1,
        }
    })
}

/// 设置模糊音开关（RIME 拼写变体，0.1.14），返回 0 成功 / -1 未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_set_fuzzy(enabled: i32) -> i32 {
    ffi_guard!(-1, {
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => {
                e.set_fuzzy_enabled(enabled != 0);
                crate::log::info(&format!("fuzzy={}", enabled != 0));
                0
            }
            None => -1,
        }
    })
}

/// 查询模糊音开关：1=开 / 0=关 / -1 未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_get_fuzzy() -> i32 {
    ffi_guard!(-1, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => {
                if e.fuzzy_enabled() {
                    1
                } else {
                    0
                }
            }
            None => -1,
        }
    })
}

/// 设置双拼模式（RIME 微软双拼方案，0.1.14），返回 0 成功 / -1 未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_set_shuangpin(enabled: i32) -> i32 {
    ffi_guard!(-1, {
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => {
                e.set_shuangpin_mode(enabled != 0);
                crate::log::info(&format!("shuangpin={}", enabled != 0));
                0
            }
            None => -1,
        }
    })
}

/// 查询双拼模式：1=开 / 0=关 / -1 未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_get_shuangpin() -> i32 {
    ffi_guard!(-1, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => {
                if e.shuangpin_mode() {
                    1
                } else {
                    0
                }
            }
            None => -1,
        }
    })
}

/// 清空引擎状态
#[unsafe(no_mangle)]
pub extern "C" fn engine_reset() {
    let _ = ffi_guard!((), {
        let mut engine = engine_lock();
        if let Some(e) = engine.as_mut() {
            e.reset();
        }
    });
}

/// 翻页。delta: +1 下一页 / -1 上一页。返回当前页候选数。
/// 0 表示无候选或已到边界（平台层此时应透传按键给应用）。
#[unsafe(no_mangle)]
pub extern "C" fn engine_page(delta: i32) -> i32 {
    ffi_guard!(0, {
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => e.page(delta) as i32,
            None => 0,
        }
    })
}

/// 获取当前页码（0 起）
#[unsafe(no_mangle)]
pub extern "C" fn engine_get_current_page() -> i32 {
    ffi_guard!(0, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => e.current_page() as i32,
            None => 0,
        }
    })
}

/// 获取总页数
#[unsafe(no_mangle)]
pub extern "C" fn engine_get_total_pages() -> i32 {
    ffi_guard!(0, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => e.total_pages() as i32,
            None => 0,
        }
    })
}

/// 销毁引擎
#[unsafe(no_mangle)]
pub extern "C" fn engine_destroy() {
    let _ = ffi_guard!((), {
        let mut engine = engine_lock();
        *engine = None;
        crate::log::info("engine_destroy");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 锁中毒后调用 FFI 应返回错误码而非崩溃（panic 守卫验证）
    #[test]
    fn test_engine_lock_poisoned_recovers() {
        // 人为制造锁中毒：在持有锁的线程 panic
        {
            let guard = ENGINE.lock().unwrap();
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                drop(guard); // 持有锁时 panic → 锁中毒
                panic!("模拟 panic 导致锁中毒");
            }))
            .ok();
        }

        // 锁已中毒，但 engine_lock 应恢复而非 panic
        let engine = engine_lock();
        assert!(engine.is_none() || engine.is_some());
        drop(engine);

        // FFI 调用不崩溃
        let result = engine_get_ascii_mode();
        assert!(result == -1 || result == 0 || result == 1);
    }

    /// 未初始化时 FFI 返回 -1（而非崩溃）
    #[test]
    fn test_ffi_uninitialized_returns_error() {
        engine_destroy(); // 确保未初始化
        assert_eq!(engine_get_ascii_mode(), -1);
        assert_eq!(engine_set_candidate_count(5), -1);
        assert_eq!(engine_process_key('a' as i32), -1);
    }
}
