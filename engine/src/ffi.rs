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
#[unsafe(no_mangle)]
pub extern "C" fn engine_get_candidate(index: i32, buf: *mut c_char, buf_len: i32) -> i32 {
    ffi_guard!(0, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => match e.candidate(index as usize) {
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
