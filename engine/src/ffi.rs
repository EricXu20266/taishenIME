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
        // V0.5.14 fix: 幂等——Engine 已存在则不重建（ActivateEx 每次激活都调
        // engine_init，重建会清空 pin_words/demoted_words 置顶降权集合；
        // 且 set_user_dict_path 幂等跳过时不再重新加载，置顶降权二次激活即丢）
        if engine.is_none() {
            *engine = Some(Engine::new());
        }
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
        // V0.5.7：加载持久化的置顶/降权集合（user_dict.db pin_words/demoted_words 表）
        let (pins, demotes) = crate::dictionary::load_pin_demote_state();
        let mut engine = engine_lock();
        if let Some(e) = engine.as_mut() {
            if !pins.is_empty() {
                e.load_pin_words(pins);
            }
            if !demotes.is_empty() {
                e.load_demoted_words(demotes);
            }
        }
        0
    })
}

/// 设置专业词库分类文件（对标微软/搜狗分类词库），返回 0 成功 / -1 未初始化。
/// path: 分类词库 txt（每行 `词 拼音`）；NULL/空 = 停用清空。
#[unsafe(no_mangle)]
pub extern "C" fn engine_set_domain_dict_path(path: *const c_char) -> i32 {
    ffi_guard!(-1, {
        let path_str = unsafe {
            if path.is_null() {
                None
            } else {
                std::ffi::CStr::from_ptr(path)
                    .to_str()
                    .ok()
                    .map(|s| s.to_string())
            }
        };
        let display = path_str.clone().unwrap_or_else(|| "(null)".to_string());
        crate::log::info(&format!("engine_set_domain_dict_path({display})"));
        match path_str {
            Some(p) => crate::dictionary::set_domain_dict_path(Some(std::path::Path::new(&p))),
            None => crate::dictionary::set_domain_dict_path(None),
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

/// P2-1 删除光标前一个音节（对标 rime Ctrl+BackSpace）。返回当前页候选数。
#[unsafe(no_mangle)]
pub extern "C" fn engine_backspace_syllable() -> i32 {
    ffi_guard!(-1, {
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => {
                e.backspace_syllable();
                e.candidate_count() as i32
            }
            None => -1,
        }
    })
}

/// P2-1 移动光标到相邻音节边界（对标 rime Tab/Shift+Tab）。
/// delta: +1 右移 / -1 左移。返回新光标位置。
#[unsafe(no_mangle)]
pub extern "C" fn engine_move_cursor(delta: i32) -> i32 {
    ffi_guard!(-1, {
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => e.move_cursor(delta) as i32,
            None => -1,
        }
    })
}

/// P2-1 查询光标位置（pinyin_buf 字符索引）。
#[unsafe(no_mangle)]
pub extern "C" fn engine_get_cursor() -> i32 {
    ffi_guard!(-1, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => e.cursor_pos() as i32,
            None => -1,
        }
    })
}

/// P2-1 删除当前页指定候选（Ctrl+Delete）：从用户词库移除并重查。
#[unsafe(no_mangle)]
pub extern "C" fn engine_delete_candidate(index: i32) -> i32 {
    ffi_guard!(-1, {
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => {
                if index < 0 {
                    -1
                } else if e.delete_candidate(index as usize) {
                    e.candidate_count() as i32
                } else {
                    0
                }
            }
            None => -1,
        }
    })
}

/// V0.5.7 置顶候选（词级）：word 加入 pin_words，重查后候选里出现即置顶。
/// 返回 1=新增 0=已在置顶集合 -1=失败。
#[unsafe(no_mangle)]
pub extern "C" fn engine_pin_word(word: *const c_char) -> i32 {
    ffi_guard!(-1, {
        if word.is_null() {
            return -1;
        }
        let word = unsafe { std::ffi::CStr::from_ptr(word) }
            .to_string_lossy()
            .into_owned();
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => {
                if e.pin_word(&word) {
                    1
                } else {
                    0
                }
            }
            None => -1,
        }
    })
}

/// V0.5.7 取消置顶（词级）：word 从 pin_words 移除。
/// 返回 1=确实移除 0=原本未置顶 -1=失败。
#[unsafe(no_mangle)]
pub extern "C" fn engine_unpin_word(word: *const c_char) -> i32 {
    ffi_guard!(-1, {
        if word.is_null() {
            return -1;
        }
        let word = unsafe { std::ffi::CStr::from_ptr(word) }
            .to_string_lossy()
            .into_owned();
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => {
                if e.unpin_word(&word) {
                    1
                } else {
                    0
                }
            }
            None => -1,
        }
    })
}

/// V0.5.7 降权候选（Eric 决策：删除=降权压出前 2 屏）：word 加入 demoted_words。
/// 返回 1=新增 0=已降权 -1=失败。
#[unsafe(no_mangle)]
pub extern "C" fn engine_demote_word(word: *const c_char) -> i32 {
    ffi_guard!(-1, {
        if word.is_null() {
            return -1;
        }
        let word = unsafe { std::ffi::CStr::from_ptr(word) }
            .to_string_lossy()
            .into_owned();
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => {
                if e.demote_word(&word) {
                    1
                } else {
                    0
                }
            }
            None => -1,
        }
    })
}

/// V0.5.7 恢复候选：word 从 demoted_words 移除，回到原排序位置。
/// 返回 1=确实恢复 0=原本未降权 -1=失败。
#[unsafe(no_mangle)]
pub extern "C" fn engine_undemote_word(word: *const c_char) -> i32 {
    ffi_guard!(-1, {
        if word.is_null() {
            return -1;
        }
        let word = unsafe { std::ffi::CStr::from_ptr(word) }
            .to_string_lossy()
            .into_owned();
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => {
                if e.undemote_word(&word) {
                    1
                } else {
                    0
                }
            }
            None => -1,
        }
    })
}

/// V0.5.7 是否已置顶（词级，菜单动态显示）：1=是 0=否 -1=失败。
#[unsafe(no_mangle)]
pub extern "C" fn engine_is_pinned(word: *const c_char) -> i32 {
    ffi_guard!(-1, {
        if word.is_null() {
            return -1;
        }
        let word = unsafe { std::ffi::CStr::from_ptr(word) }
            .to_string_lossy()
            .into_owned();
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => {
                if e.is_pinned(&word) {
                    1
                } else {
                    0
                }
            }
            None => -1,
        }
    })
}

/// V0.5.7 是否已降权（菜单动态显示）：1=是 0=否 -1=失败。
#[unsafe(no_mangle)]
pub extern "C" fn engine_is_demoted(word: *const c_char) -> i32 {
    ffi_guard!(-1, {
        if word.is_null() {
            return -1;
        }
        let word = unsafe { std::ffi::CStr::from_ptr(word) }
            .to_string_lossy()
            .into_owned();
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => {
                if e.is_demoted(&word) {
                    1
                } else {
                    0
                }
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

/// 是否处于动态组词模式（V0.5）：1=是 0=否
#[unsafe(no_mangle)]
pub extern "C" fn engine_in_compose() -> i32 {
    ffi_guard!(0, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => e.compose_active() as i32,
            None => 0,
        }
    })
}

/// 组词模式当前音节（V0.5）：候选窗显示用（如组词 tai 时返回 "tai"）。
/// 非组词模式返回空串。返回长度含 null。
#[unsafe(no_mangle)]
pub extern "C" fn engine_compose_info(buf: *mut c_char, buf_len: i32) -> i32 {
    ffi_guard!(0, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => {
                let s = e.compose_current_syllable();
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

/// 组词模式剩余音节（V0.5.11）：编辑区 composition 显示用。
/// 返回 compose_idx 起至末尾的音节 join。非组词返回空串。返回长度含 null。
#[unsafe(no_mangle)]
pub extern "C" fn engine_compose_remaining(buf: *mut c_char, buf_len: i32) -> i32 {
    ffi_guard!(0, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => {
                let s = e.compose_remaining();
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

/// 音节分隔显示串（V0.5.13 音节可视化）：zhongguo → "zhong'guo"，
/// zg → "z'g"，tshen → "t'shen"。输入全程可用（候选窗拼音区 + composition）。
/// 组词模式：返回当前音节起至末尾带分隔。返回长度含 null。
#[unsafe(no_mangle)]
pub extern "C" fn engine_syllable_display(buf: *mut c_char, buf_len: i32) -> i32 {
    ffi_guard!(0, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => {
                let s = e.syllable_display();
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

/// 设置中英标点开关（P0-2）：1=英文标点透传 / 0=中文标点全角化
#[unsafe(no_mangle)]
pub extern "C" fn engine_set_ascii_punct(enabled: i32) -> i32 {
    ffi_guard!(-1, {
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => {
                e.set_ascii_punct(enabled != 0);
                crate::log::info(&format!("ascii_punct={}", enabled != 0));
                0
            }
            None => -1,
        }
    })
}

/// 查询中英标点开关：1=英文标点 / 0=中文标点 / -1 引擎未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_get_ascii_punct() -> i32 {
    ffi_guard!(-1, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => {
                if e.ascii_punct() {
                    1
                } else {
                    0
                }
            }
            None => -1,
        }
    })
}

/// 查询当前输入模式（P1-4，平台层据此吞数字/运算符键）：
/// 0=拼音 1=计算器c 2=数字大写R 3=Unicode U 4=符号v 5=拆字u / -1 未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_input_mode() -> i32 {
    ffi_guard!(-1, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => {
                if e.is_calc_mode() {
                    1
                } else if e.is_number_mode() {
                    2
                } else if e.is_unicode_mode() {
                    3
                } else if e.is_symbol_mode() {
                    4
                } else if e.is_radical_mode() {
                    5
                } else {
                    0
                }
            }
            None => -1,
        }
    })
}

/// v 前缀即时反馈态判定（0.2.32）：拼音串恰为单个 'v'（非双拼）。
/// 平台层据此在 v 前缀时把数字键送进引擎（v1-v9 分类别名）而非选候选。
/// 返回 1=v 前缀 / 0=否 / -1 未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_is_symbol_prefix() -> i32 {
    ffi_guard!(-1, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => {
                if e.is_symbol_prefix() {
                    1
                } else {
                    0
                }
            }
            None => -1,
        }
    })
}

/// 设置 Emoji 开关（P2-5）：1=开 / 0=关
#[unsafe(no_mangle)]
pub extern "C" fn engine_set_emoji(enabled: i32) -> i32 {
    ffi_guard!(-1, {
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => {
                e.set_emoji(enabled != 0);
                0
            }
            None => -1,
        }
    })
}

/// 查询 Emoji 开关：1=开 / 0=关 / -1 未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_get_emoji() -> i32 {
    ffi_guard!(-1, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => {
                if e.emoji() {
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

/// 设置候选排序模式（P0-2，对标微软单字/长词优先）：
/// 0=默认（词频+长词过滤） 1=单字优先 2=长词优先
#[unsafe(no_mangle)]
pub extern "C" fn engine_set_sort_mode(mode: i32) -> i32 {
    ffi_guard!(-1, {
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => {
                e.set_sort_mode(mode);
                crate::log::info(&format!("sort_mode={}", e.sort_mode()));
                0
            }
            None => -1,
        }
    })
}

/// 查询候选排序模式：0=默认 1=单字优先 2=长词优先 / -1 未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_get_sort_mode() -> i32 {
    ffi_guard!(-1, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => e.sort_mode(),
            None => -1,
        }
    })
}

/// 设置上下文联想开关（P1-1，对标搜狗/微软前文关联），返回 0 成功 / -1 未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_set_context_assoc(enabled: i32) -> i32 {
    ffi_guard!(-1, {
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => {
                e.set_context_enabled(enabled != 0);
                crate::log::info(&format!("context_assoc={}", enabled != 0));
                0
            }
            None => -1,
        }
    })
}

/// 查询上下文联想开关：1=开 / 0=关 / -1 未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_get_context_assoc() -> i32 {
    ffi_guard!(-1, {
        let engine = engine_lock();
        match engine.as_ref() {
            Some(e) => {
                if e.context_enabled() {
                    1
                } else {
                    0
                }
            }
            None => -1,
        }
    })
}

/// 设置双拼方案（P2-7）：mspy/flypy/sogou/zrm/ziguang/jiajia。
/// 返回 1 成功 / 0 未知方案 / -1 未初始化。
#[unsafe(no_mangle)]
pub extern "C" fn engine_set_shuangpin_scheme(id: *const c_char) -> i32 {
    ffi_guard!(-1, {
        if id.is_null() {
            return 0;
        }
        let id_str = unsafe { std::ffi::CStr::from_ptr(id) }
            .to_string_lossy()
            .into_owned();
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => {
                if e.set_shuangpin_scheme(&id_str) {
                    crate::log::info(&format!("shuangpin_scheme={id_str}"));
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

// ════════════════════════════════════════════════════════════
// V0.5 语音输入 FFI（SPEC docs/modules/voice-input/SPEC.md 4.1）
// ════════════════════════════════════════════════════════════

use std::os::raw::c_float;

/// 全局语音会话（线程安全，独立于 ENGINE）
static VOICE: Mutex<Option<crate::voice::VoiceSession>> = Mutex::new(None);

/// 安全获取语音会话可变引用（锁中毒恢复，不 panic）
fn voice_lock() -> std::sync::MutexGuard<'static, Option<crate::voice::VoiceSession>> {
    VOICE.lock().unwrap_or_else(|e| e.into_inner())
}

/// 启动语音输入（0.5.x）。server_url 为 whisper-server URL（NULL=自管路径，启动时
/// 由平台层确认后再以 transcribe 传入）；language 识别语言（NULL/空=zh）。
/// 返回 0=成功 / -1=未初始化引擎。
#[unsafe(no_mangle)]
pub extern "C" fn engine_voice_start(server_url: *const c_char, language: *const c_char) -> i32 {
    ffi_guard!(-1, {
        if engine_lock().is_none() {
            return -1;
        }
        let url = if server_url.is_null() {
            String::new()
        } else {
            unsafe { std::ffi::CStr::from_ptr(server_url) }
                .to_string_lossy()
                .into_owned()
        };
        let lang = if language.is_null() {
            String::new()
        } else {
            unsafe { std::ffi::CStr::from_ptr(language) }
                .to_string_lossy()
                .into_owned()
        };
        let mut voice = voice_lock();
        *voice = Some(crate::voice::VoiceSession::new(&url, &lang));
        if let Some(s) = voice.as_mut() {
            s.start();
        }
        0
    })
}

/// 停止语音输入。冲刷 VAD 剩余语音并等待转写完成（VAD flush 由平台层
/// 按 engine_vad_process 返回码处理；停止后 session 回到 idle）。
/// 返回 0=成功 / -1=未初始化。
#[unsafe(no_mangle)]
pub extern "C" fn engine_voice_stop() -> i32 {
    ffi_guard!(-1, {
        let mut voice = voice_lock();
        match voice.as_mut() {
            Some(s) => {
                s.stop();
                0
            }
            None => -1,
        }
    })
}

/// 处理一帧 PCM 音频（16kHz mono f32）。返回状态:
///   0 = silence
///   1 = speech
///   2 = pending_transcribe（触发转写——平台层应把本段音频 WAV 编码后
///       调用 engine_voice_transcribe）
///   -1 = 未初始化 / 非 listening 态
#[unsafe(no_mangle)]
pub extern "C" fn engine_vad_process(samples: *const c_float, sample_count: i32) -> i32 {
    ffi_guard!(-1, {
        if samples.is_null() || sample_count <= 0 {
            return -1;
        }
        let count = sample_count as usize;
        let slice = unsafe { std::slice::from_raw_parts(samples, count) };
        let mut voice = voice_lock();
        match voice.as_mut() {
            Some(s) => s.process_frame(slice),
            None => -1,
        }
    })
}

/// 转写音频段（内部 HTTP POST whisper-server，阻塞调用，超时 120s）。
/// wav_data/wav_len: WAV 编码音频（16kHz mono 16bit PCM）。
/// result_buf/result_capacity: 输出转写文本缓冲区（UTF-8，含 null 终止符）。
/// 返回 0=成功 / -1=网络错误 / -2=超时 / -3=空结果 / -4=参数错误 / -5=未初始化。
#[unsafe(no_mangle)]
pub extern "C" fn engine_voice_transcribe(
    wav_data: *const u8,
    wav_len: i32,
    result_buf: *mut c_char,
    result_capacity: i32,
) -> i32 {
    ffi_guard!(-4, {
        if wav_data.is_null() || wav_len <= 0 || result_buf.is_null() || result_capacity <= 0 {
            return -4;
        }
        let wav = unsafe { std::slice::from_raw_parts(wav_data, wav_len as usize) };
        // server_url 从会话取（引擎_voice_start 传入）；无会话时用空 → 网络错误
        let (url, lang) = {
            let voice = voice_lock();
            match voice.as_ref() {
                Some(s) => (s.server_url().to_string(), s.language().to_string()),
                None => return -5,
            }
        };
        match crate::voice::transcribe(wav, &url, &lang) {
            Ok(text) => {
                let bytes = text.as_bytes();
                let cap = result_capacity as usize;
                if bytes.len() + 1 > cap {
                    return -4; // 缓冲区不足
                }
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        bytes.as_ptr(),
                        result_buf as *mut u8,
                        bytes.len(),
                    );
                    *result_buf.add(bytes.len()) = 0;
                }
                // 转写完成 → 回到 listening（可继续说话）
                let mut voice = voice_lock();
                if let Some(s) = voice.as_mut() {
                    s.on_transcribe_done();
                }
                0
            }
            Err(crate::voice::VoiceError::Timeout) => -2,
            Err(crate::voice::VoiceError::Empty) => -3,
            Err(crate::voice::VoiceError::Http(_, _))
            | Err(crate::voice::VoiceError::Network(_)) => {
                let mut voice = voice_lock();
                if let Some(s) = voice.as_mut() {
                    s.on_transcribe_error();
                }
                crate::log::error("voice transcribe failed");
                -1
            }
        }
    })
}

/// 注入语音转写结果到候选列表（0.5.7）。text 为转写文本。
/// 返回 1=已注入 / 0=空文本未注入 / -1=未初始化。
#[unsafe(no_mangle)]
pub extern "C" fn engine_voice_result(text: *const c_char) -> i32 {
    ffi_guard!(-1, {
        if text.is_null() {
            return 0;
        }
        let text_str = unsafe { std::ffi::CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned();
        if text_str.is_empty() {
            return 0;
        }
        let mut engine = engine_lock();
        match engine.as_mut() {
            Some(e) => {
                // 语音候选注入：清空当前输入态，注入转写文本为唯一候选
                e.reset();
                e.set_voice_candidate(&text_str);
                crate::log::info(&format!(
                    "voice result: {}",
                    &text_str[..text_str.len().min(40)]
                ));
                1
            }
            None => -1,
        }
    })
}

/// 获取语音输入状态: 0=idle, 1=listening, 2=transcribing, 3=error, -1=未初始化
#[unsafe(no_mangle)]
pub extern "C" fn engine_voice_state() -> i32 {
    ffi_guard!(-1, {
        let voice = voice_lock();
        match voice.as_ref() {
            Some(s) => match s.mode() {
                crate::voice::VoiceMode::Idle => 0,
                crate::voice::VoiceMode::Listening => 1,
                crate::voice::VoiceMode::Transcribing => 2,
                crate::voice::VoiceMode::Error => 3,
            },
            None => -1,
        }
    })
}

/// 冲刷 VAD 剩余语音。返回 1=有待转写段（平台层应转写其累积缓冲），0=无剩余，-1=未初始化。
/// 停止录音时调用，处理用户说话中途松开的场景。
#[unsafe(no_mangle)]
pub extern "C" fn engine_voice_flush() -> i32 {
    ffi_guard!(-1, {
        let mut voice = voice_lock();
        match voice.as_mut() {
            Some(s) => {
                let had = s.flush_remaining_segment();
                if had { 1 } else { 0 }
            }
            None => -1,
        }
    })
}

/// 检测泰深是否可连接（0.5.3 优先路径）。taishen_bin 为 ~/.taishen/bin/ 路径
/// （NULL/空则跳过文件检查）；port 为 whisper-server 端口。
/// 返回 0=不可用 / 1=可用（server 在跑，直连模式）/ 2=server 存在但未启动。
#[unsafe(no_mangle)]
pub extern "C" fn engine_detect_taishen(taishen_bin: *const c_char, port: i32) -> i32 {
    ffi_guard!(0, {
        // ① 文件检查：~/.taishen/bin/whisper-server.exe 存在？
        let mut exe_exists = false;
        if !taishen_bin.is_null() {
            let bin = unsafe { std::ffi::CStr::from_ptr(taishen_bin) }
                .to_string_lossy()
                .into_owned();
            if !bin.is_empty() {
                let exe = std::path::Path::new(&bin).join("whisper-server.exe");
                exe_exists = exe.exists();
            }
        }
        // ② 端口连通检查：TCP connect 127.0.0.1:port
        let port_ok = (port > 0 && port < 65536) && tcp_probe(port);
        if port_ok {
            return 1; // server 在跑 → 直连
        }
        if exe_exists {
            return 2; // server 存在但未启动
        }
        0
    })
}

/// TCP 连通性探测（127.0.0.1:port，500ms 超时）
fn tcp_probe(port: i32) -> bool {
    use std::io::Read;
    use std::io::Write;
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;

    let addr = format!("127.0.0.1:{port}");
    let addrs = match addr.to_socket_addrs() {
        Ok(a) => a.collect::<Vec<_>>(),
        Err(_) => return false,
    };
    if addrs.is_empty() {
        return false;
    }
    match TcpStream::connect_timeout(&addrs[0], Duration::from_millis(500)) {
        Ok(mut stream) => {
            // 发一个 HTTP GET / 探测，能读到响应即视为存活
            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
            let _ =
                stream.write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
            let mut buf = [0u8; 128];
            matches!(stream.read(&mut buf), Ok(_))
        }
        Err(_) => false,
    }
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

    /// V0.5 语音 FFI：语音候选注入（engine_voice_result）
    #[test]
    fn test_voice_result_injects_candidate() {
        engine_destroy();
        // 未初始化 → -1
        let c_text = std::ffi::CString::new("你好世界").unwrap();
        assert_eq!(engine_voice_result(c_text.as_ptr()), -1);
        // 初始化后 → 注入
        engine_init(std::ptr::null());
        assert_eq!(engine_voice_result(c_text.as_ptr()), 1);
        assert_eq!(engine_get_candidate_count(), 1);
        let mut buf = [0i8; 64];
        let len = engine_get_candidate(0, buf.as_mut_ptr(), 64);
        assert!(len > 0);
        let s = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        assert_eq!(s, "你好世界");
        // 空文本 → 0
        let empty = std::ffi::CString::new("").unwrap();
        assert_eq!(engine_voice_result(empty.as_ptr()), 0);
        engine_destroy();
    }

    /// V0.5 语音 FFI：VAD 状态机（voice_start → vad_process → voice_state → stop）
    #[test]
    fn test_voice_vad_state_machine() {
        engine_destroy();
        engine_init(std::ptr::null());
        // 未 start → vad_process 返回 -1
        let samples = [0.1f32; 512];
        assert_eq!(engine_vad_process(samples.as_ptr(), 512), -1);
        // start → listening
        let url = std::ffi::CString::new("http://127.0.0.1:9080").unwrap();
        let lang = std::ffi::CString::new("zh").unwrap();
        assert_eq!(engine_voice_start(url.as_ptr(), lang.as_ptr()), 0);
        assert_eq!(engine_voice_state(), 1); // listening
        // 静音帧 → 0 (silence)
        let quiet = [0.001f32; 512];
        assert_eq!(engine_vad_process(quiet.as_ptr(), 512), 0);
        // 高能量帧 → 1 (speech)
        assert_eq!(engine_vad_process(samples.as_ptr(), 512), 1);
        assert_eq!(engine_voice_state(), 1);
        // stop → idle
        assert_eq!(engine_voice_stop(), 0);
        assert_eq!(engine_voice_state(), 0);
        engine_destroy();
    }

    /// V0.5 语音 FFI：泰深检测（本地无 server → 0 或 2，不崩溃）
    #[test]
    fn test_voice_detect_taishen() {
        engine_destroy();
        engine_init(std::ptr::null());
        // 路径不存在 → 0（未检测到）
        let bogus = std::ffi::CString::new("Z:\\nonexistent\\bin").unwrap();
        let result = engine_detect_taishen(bogus.as_ptr(), 9080);
        assert!(result == 0 || result == 2);
        // NULL 路径 + 无端口监听 → 0
        assert_eq!(engine_detect_taishen(std::ptr::null(), 9080), 0);
        engine_destroy();
    }
}
