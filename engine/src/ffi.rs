/// C FFI 接口 — 将 Rust 引擎暴露给 C/C++ 平台层
///
/// 所有函数使用 `extern "C"` + `#[unsafe(no_mangle)]`，参数和返回值均为 C 兼容类型。

use std::os::raw::c_char;
use std::sync::Mutex;

use crate::Engine;

/// 全局引擎实例（线程安全）
static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);

/// 初始化引擎。dict_path 为系统词库路径（可为 NULL 则回退内置词库）
#[unsafe(no_mangle)]
pub extern "C" fn engine_init(dict_path: *const c_char) -> i32 {
    let path = unsafe {
        if dict_path.is_null() {
            None
        } else {
            Some(std::ffi::CStr::from_ptr(dict_path))
        }
    };

    crate::dictionary::init(path.map(|p| std::path::Path::new(p.to_str().unwrap_or(""))));

    let mut engine = ENGINE.lock().unwrap();
    *engine = Some(Engine::new());
    0
}

/// 处理按键，返回候选词数量。-1 表示无效按键
#[unsafe(no_mangle)]
pub extern "C" fn engine_process_key(ch: i32) -> i32 {
    let ch = ch as u8 as char;
    let mut engine = ENGINE.lock().unwrap();
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
}

/// 退格删除一个字符，返回当前候选词数量
#[unsafe(no_mangle)]
pub extern "C" fn engine_backspace() -> i32 {
    let mut engine = ENGINE.lock().unwrap();
    match engine.as_mut() {
        Some(e) => {
            e.backspace();
            e.candidate_count() as i32
        }
        None => -1,
    }
}

/// 获取当前拼音串，返回字符串长度。buf 不足时返回所需长度（不含 null）
#[unsafe(no_mangle)]
pub extern "C" fn engine_get_pinyin_str(buf: *mut c_char, buf_len: i32) -> i32 {
    let engine = ENGINE.lock().unwrap();
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
}

/// 获取候选词总数
#[unsafe(no_mangle)]
pub extern "C" fn engine_get_candidate_count() -> i32 {
    let engine = ENGINE.lock().unwrap();
    match engine.as_ref() {
        Some(e) => e.candidate_count() as i32,
        None => 0,
    }
}

/// 获取指定候选词，返回字符串长度。buf 不足时返回所需长度（不含 null）
#[unsafe(no_mangle)]
pub extern "C" fn engine_get_candidate(index: i32, buf: *mut c_char, buf_len: i32) -> i32 {
    let engine = ENGINE.lock().unwrap();
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
}

/// 选择候选词，提交文本写入 buf，返回文本长度
#[unsafe(no_mangle)]
pub extern "C" fn engine_select_candidate(index: i32, buf: *mut c_char, buf_len: i32) -> i32 {
    let mut engine = ENGINE.lock().unwrap();
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
}

/// 设置候选词数量上限，返回 0 成功 / -1 引擎未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_set_candidate_count(count: i32) -> i32 {
    let mut engine = ENGINE.lock().unwrap();
    match engine.as_mut() {
        Some(e) => {
            e.set_candidate_limit(count.max(0) as usize);
            0
        }
        None => -1,
    }
}

/// 清空引擎状态
#[unsafe(no_mangle)]
pub extern "C" fn engine_reset() {
    let mut engine = ENGINE.lock().unwrap();
    if let Some(e) = engine.as_mut() {
        e.reset();
    }
}

/// 销毁引擎
#[unsafe(no_mangle)]
pub extern "C" fn engine_destroy() {
    let mut engine = ENGINE.lock().unwrap();
    *engine = None;
}
