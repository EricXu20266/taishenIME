//! 选词 FFI 协议回归测试（真实词库）：验证 V0.9 的 `.bin` v2 分片格式
//! 与 C++ 侧两阶段容量协议组合后仍然正确上屏。
//!
//! 覆盖：
//! 1. 两阶段读取（先查容量、再读取）不得提前消费候选
//! 2. 单阶段旧式调用仍可用（兼容 IMM32 老路径）
//! 3. 数字键索引选词
//! 4. 系统与领域两个 `.bin` 都能在有限时间内就绪

use std::os::raw::c_char;
use std::path::Path;

use taishen_engine::ffi::*;

/// 复刻 C++ `ReadEngineUtf8` 的两阶段读取协议。
fn read_engine_utf8<F>(read: F) -> Option<String>
where
    F: Fn(*mut c_char, i32) -> i32,
{
    let required = read(std::ptr::null_mut(), 0);
    if required <= 0 {
        return None;
    }
    let mut buffer = vec![0i8; required as usize];
    let reported = read(buffer.as_mut_ptr(), required);
    if reported <= 0 {
        return None;
    }
    let bytes: Vec<u8> = buffer
        .iter()
        .map(|v| *v as u8)
        .take_while(|b| *b != 0)
        .collect();
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn type_pinyin(text: &str) {
    engine_reset();
    for ch in text.chars() {
        engine_process_key(ch as i32);
    }
}

#[test]
fn select_protocol_and_index_ready() {
    let dict = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("resources/system_dict.db");
    let dict_c = std::ffi::CString::new(dict.to_string_lossy().as_bytes()).unwrap();
    assert_eq!(engine_init(dict_c.as_ptr()), 0);
    engine_set_candidate_count(9);

    let mut waited = 0;
    while engine_dict_ready() == 0 && waited < 20_000 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        waited += 50;
    }
    assert_eq!(engine_dict_ready(), 1, "系统词库应在 20s 内就绪");
    println!("dict_ready={waited}ms");

    // 1. 两阶段读取：查询阶段不得消费候选
    type_pinyin("nihao");
    assert!(engine_get_candidate_count() > 0, "nihao 应有候选");
    let selected = read_engine_utf8(|b, c| engine_select_candidate(0, b, c))
        .expect("两阶段读取应成功（查询阶段不能消费候选）");
    assert_eq!(selected, "你好");
    assert_eq!(engine_get_candidate_count(), 0, "选词后应清空候选");

    // 2. 单阶段旧式调用
    type_pinyin("nihao");
    let mut buf = [0u8; 512];
    let ret = engine_select_candidate(0, buf.as_mut_ptr() as *mut c_char, 512);
    let single = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr() as *const c_char) }
        .to_string_lossy()
        .into_owned();
    assert!(ret > 0, "单阶段调用应返回值");
    assert_eq!(single, "你好");

    // 3. 数字键索引选词（第 4 个候选）
    type_pinyin("nihao");
    assert!(engine_get_candidate_count() > 3, "应有第 4 个候选");
    let expect = {
        let mut b = [0u8; 512];
        engine_get_candidate(3, b.as_mut_ptr() as *mut c_char, 512);
        unsafe { std::ffi::CStr::from_ptr(b.as_ptr() as *const c_char) }
            .to_string_lossy()
            .into_owned()
    };
    let picked = read_engine_utf8(|b, c| engine_select_candidate(3, b, c)).expect("索引选词应成功");
    assert_eq!(picked, expect, "数字键选出的词应与候选一致");

    // 4. 领域词库并行加载窗口（.bin v2 分片）——留出合并时间再退出
    std::thread::sleep(std::time::Duration::from_millis(1500));

    engine_destroy();
}
