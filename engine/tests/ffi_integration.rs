//! FFI 集成测试（0.1.12）— 通过 C ABI 验证完整输入链路
//!
//! 与单元测试的区别：单元测试直接调用 Rust API，这里走 extern "C" 边界，
//! 模拟 C++ 平台层（TSF DLL）的调用方式，验证 FFI 契约与缓冲协议。
//!
//! 性能基准（release 模式）：cargo test --release --test ffi_integration perf_ -- --ignored --nocapture

use std::ffi::CStr;
use std::os::raw::c_char;

use taishen_engine::ffi::*;

/// 从 FFI 缓冲区读取 C 字符串
unsafe fn read_cstr(buf: &[u8]) -> String {
    unsafe {
        CStr::from_ptr(buf.as_ptr() as *const c_char)
            .to_string_lossy()
            .into_owned()
    }
}

/// 集成测试共用全局状态（static ENGINE），多测试并行会互相干扰，
/// 因此全部 FFI 断言收敛为单个测试串行验证。
#[test]
fn ffi_full_input_chain() {
    unsafe {
        // ── 初始化（NULL 路径 = 内置词库）──
        assert_eq!(engine_init(std::ptr::null()), 0);
        assert_eq!(engine_get_candidate_count(), 0);
        assert_eq!(engine_get_ascii_mode(), 0); // 默认中文

        // ── 幂等初始化（0.3.x）：相同路径重复 init 跳过词库重载 ──
        // TSF 切换输入法反复 Deactivate→Activate→engine_init，
        // 全量重载 62 万词条是切换卡顿根因，幂等后第二次 init 应毫秒级返回。
        let t0 = std::time::Instant::now();
        assert_eq!(engine_init(std::ptr::null()), 0);
        let el = t0.elapsed();
        assert!(el < std::time::Duration::from_millis(50),
                "重复 init 应幂等跳过词库加载（<50ms），实际 {el:?}");

        // ── 按键累积 → 拼音 + 候选 ──
        // "zhongguo" → 中国
        for ch in "zhongguo".chars() {
            let n = engine_process_key(ch as i32);
            let mut dbg = [0u8; 64];
            engine_get_pinyin_str(dbg.as_mut_ptr() as *mut c_char, dbg.len() as i32);
            println!("键 {ch}: 返回={n} 拼音={}", read_cstr(&dbg));
            assert!(n > 0, "按键 {ch} 应产生候选");
        }
        let mut buf = [0u8; 64];
        engine_get_pinyin_str(buf.as_mut_ptr() as *mut c_char, buf.len() as i32);
        assert_eq!(read_cstr(&buf), "zhongguo");
        assert!(engine_get_candidate_count() > 0, "应有候选");

        // ── 缓冲协议：buf 不足返回所需长度（不含 null）──
        let needed = engine_get_pinyin_str(std::ptr::null_mut(), 0);
        assert_eq!(needed, 9, "zhongguo 8 字符 + null = 9");

        // ── 选词上屏 ──
        let mut sel_buf = [0u8; 64];
        let len = engine_select_candidate(0, sel_buf.as_mut_ptr() as *mut c_char, sel_buf.len() as i32);
        assert!(len > 0, "选词应返回文本");
        assert_eq!(read_cstr(&sel_buf), "中国");
        // 选词后状态重置
        assert_eq!(engine_get_pinyin_str(std::ptr::null_mut(), 0), 1); // 空串 = 1
        assert_eq!(engine_get_candidate_count(), 0);

        // ── 退格 ──
        engine_process_key('n' as i32);
        engine_process_key('i' as i32);
        assert_eq!(engine_get_pinyin_str(std::ptr::null_mut(), 0), 3); // "ni"
        engine_backspace();
        assert_eq!(engine_get_pinyin_str(std::ptr::null_mut(), 0), 2); // "n"
        engine_backspace();
        assert_eq!(engine_get_pinyin_str(std::ptr::null_mut(), 0), 1); // 空
        assert_eq!(engine_get_candidate_count(), 0);

        // ── 翻页（zh 多候选，page_size=2 必翻页）──
        engine_set_candidate_count(2);
        engine_process_key('z' as i32);
        engine_process_key('h' as i32);
        assert!(engine_get_total_pages() > 1, "zh 应多页");
        assert_eq!(engine_get_current_page(), 0);
        engine_page(1);
        assert_eq!(engine_get_current_page(), 1);
        engine_page(-1);
        assert_eq!(engine_get_current_page(), 0);
        engine_reset();

        // ── 英文模式 ──
        engine_set_ascii_mode(1);
        assert_eq!(engine_get_ascii_mode(), 1);
        assert_eq!(engine_process_key('a' as i32), 0, "英文模式字母直通");
        assert_eq!(engine_get_pinyin_str(std::ptr::null_mut(), 0), 1);
        engine_set_ascii_mode(0);
        assert_eq!(engine_get_ascii_mode(), 0);

        // ── 模糊音 / 双拼开关 ──
        assert_eq!(engine_get_fuzzy(), 1); // 默认开
        engine_set_fuzzy(0);
        assert_eq!(engine_get_fuzzy(), 0);
        engine_set_fuzzy(1);
        assert_eq!(engine_get_shuangpin(), 0); // 默认关
        engine_set_shuangpin(1);
        assert_eq!(engine_get_shuangpin(), 1);
        engine_set_shuangpin(0);

        // ── V0.2.10 智能纠错：logn → long → 龙（真实词库） ──
        assert_eq!(engine_get_correction(), 1); // 默认开
        // 切换到真实词库（内置词库无"龙"）
        let dict_path = std::ffi::CString::new(
            std::env::current_dir().unwrap().join("../resources/system_dict.db").to_string_lossy().as_bytes()
        ).unwrap();
        engine_destroy();
        assert_eq!(engine_init(dict_path.as_ptr()), 0);
        // 输入 logn（误触：n/g 相邻交换）→ 纠错变体 long → 龙
        for ch in "logn".chars() {
            engine_process_key(ch as i32);
        }
        // 翻页遍历全部候选找"龙"（query(long) 63 条，第一页可能没有）
        let mut has_long = false;
        let total_pages = engine_get_total_pages();
        for _ in 0..total_pages {
            for i in 0..engine_get_candidate_count() {
                let mut b = [0u8; 64];
                engine_get_candidate(i, b.as_mut_ptr() as *mut c_char, b.len() as i32);
                if read_cstr(&b) == "龙" {
                    has_long = true;
                }
            }
            if engine_page(1) <= 0 {
                break;
            }
        }
        assert!(has_long, "logn 应纠错出 龙（真实词库）");
        println!("智能纠错 OK: logn → 龙");
        engine_reset();
        // 关闭后不纠错
        engine_set_correction(0);
        assert_eq!(engine_get_correction(), 0);
        for ch in "logn".chars() {
            engine_process_key(ch as i32);
        }
        let mut has_long_off = false;
        let total_pages_off = engine_get_total_pages();
        for _ in 0..total_pages_off {
            for i in 0..engine_get_candidate_count() {
                let mut b = [0u8; 64];
                engine_get_candidate(i, b.as_mut_ptr() as *mut c_char, b.len() as i32);
                if read_cstr(&b) == "龙" {
                    has_long_off = true;
                }
            }
            if engine_page(1) <= 0 {
                break;
            }
        }
        assert!(!has_long_off, "关闭纠错后 logn 不应出 龙");
        engine_reset();
        engine_set_correction(1);
        // 恢复内置词库
        engine_destroy();
        assert_eq!(engine_init(std::ptr::null()), 0);

        // ── V0.2.2 用户词库：学习 → 持久化 → 插队 ──
        let user_path = std::env::temp_dir()
            .join(format!("tsh_ime_ffi_user_{}.db", std::process::id()));
        let path_c = std::ffi::CString::new(user_path.to_string_lossy().as_bytes()).unwrap();
        let _ = std::fs::remove_file(&user_path);
        assert_eq!(engine_set_user_dict_path(path_c.as_ptr()), 0);

        // 输入 nicheng 选候选 0 学习（自动 learn）
        for ch in "nicheng".chars() {
            engine_process_key(ch as i32);
        }
        assert!(engine_get_candidate_count() > 0, "nicheng 应有候选");
        let mut sel = [0u8; 64];
        let len = engine_select_candidate(0, sel.as_mut_ptr() as *mut c_char, sel.len() as i32);
        assert!(len > 0);
        let learned_word = read_cstr(&sel);
        println!("用户词库学习: nicheng → {learned_word}");

        // 重启加载验证持久化 + 用户词置顶
        engine_destroy();
        assert_eq!(engine_init(std::ptr::null()), 0);
        assert_eq!(engine_set_user_dict_path(path_c.as_ptr()), 0);
        for ch in "nicheng".chars() {
            engine_process_key(ch as i32);
        }
        assert!(engine_get_candidate_count() > 0, "重启后 nicheng 应仍命中");
        let mut cand = [0u8; 64];
        engine_get_candidate(0, cand.as_mut_ptr() as *mut c_char, cand.len() as i32);
        assert_eq!(read_cstr(&cand), learned_word, "重启后用户词应置顶");
        println!("用户词库持久化 OK: {learned_word}");

        // 清理用户词库文件
        engine_destroy();
        let _ = std::fs::remove_file(&user_path);

        // ── V0.2.8 中英混输：中文模式候选末尾英文候选 ──
        assert_eq!(engine_init(std::ptr::null()), 0);
        assert_eq!(engine_get_mix_mode(), 1); // 默认开
        for ch in "hello".chars() {
            engine_process_key(ch as i32);
        }
        // 英文候选恒在末尾
        let last_idx = engine_get_candidate_count() - 1;
        let mut lb = [0u8; 64];
        engine_get_candidate(last_idx, lb.as_mut_ptr() as *mut c_char, lb.len() as i32);
        assert_eq!(read_cstr(&lb), "hello", "末尾应为英文候选");
        // 选中英文候选 → 上屏原文
        let mut eb = [0u8; 64];
        let elen = engine_select_candidate(last_idx, eb.as_mut_ptr() as *mut c_char, eb.len() as i32);
        assert!(elen > 0);
        assert_eq!(read_cstr(&eb), "hello");
        println!("中英混输 OK: hello 英文候选上屏");
        // 关闭混输 → 无英文候选
        engine_set_mix_mode(0);
        assert_eq!(engine_get_mix_mode(), 0);
        for ch in "hello".chars() {
            engine_process_key(ch as i32);
        }
        let mut has_eng = false;
        for i in 0..engine_get_candidate_count() {
            let mut b = [0u8; 64];
            engine_get_candidate(i, b.as_mut_ptr() as *mut c_char, b.len() as i32);
            if read_cstr(&b) == "hello" {
                has_eng = true;
            }
        }
        assert!(!has_eng, "关闭混输后不应有英文候选");
        engine_reset();
        engine_set_mix_mode(1);
        engine_destroy();

        // ── V0.2.11 简繁转换：zhongguo → 中國 ──
        assert_eq!(engine_init(std::ptr::null()), 0);
        assert_eq!(engine_get_traditional(), 0); // 默认关
        for ch in "zhongguo".chars() {
            engine_process_key(ch as i32);
        }
        // 简体候选正常
        let mut sb = [0u8; 64];
        engine_get_candidate(0, sb.as_mut_ptr() as *mut c_char, sb.len() as i32);
        println!("简体候选: {}", read_cstr(&sb));
        engine_reset();
        // 开启简繁 → 候选/上屏繁体
        engine_set_traditional(1);
        assert_eq!(engine_get_traditional(), 1);
        for ch in "zhongguo".chars() {
            engine_process_key(ch as i32);
        }
        let mut tb = [0u8; 64];
        engine_get_candidate(0, tb.as_mut_ptr() as *mut c_char, tb.len() as i32);
        assert_eq!(read_cstr(&tb), "中國", "简繁模式候选应为中國");
        let mut sb2 = [0u8; 64];
        let slen = engine_select_candidate(0, sb2.as_mut_ptr() as *mut c_char, sb2.len() as i32);
        assert!(slen > 0);
        assert_eq!(read_cstr(&sb2), "中國", "简繁模式上屏应为中國");
        println!("简繁转换 OK: 中国 → 中國");
        engine_set_traditional(0);
        engine_destroy();

        // ── V0.2.12 快捷短语：bq → 不客气 ──
        assert_eq!(engine_init(std::ptr::null()), 0);
        assert_eq!(engine_get_phrase_enabled(), 1); // 默认开
        engine_process_key('b' as i32);
        engine_process_key('q' as i32);
        // 短语排最前
        let mut pb = [0u8; 64];
        engine_get_candidate(0, pb.as_mut_ptr() as *mut c_char, pb.len() as i32);
        assert_eq!(read_cstr(&pb), "不客气", "bq 短语应排最前");
        let mut pb2 = [0u8; 64];
        let plen = engine_select_candidate(0, pb2.as_mut_ptr() as *mut c_char, pb2.len() as i32);
        assert!(plen > 0);
        assert_eq!(read_cstr(&pb2), "不客气");
        println!("快捷短语 OK: bq → 不客气");
        // 关闭后无短语
        engine_set_phrase_enabled(0);
        assert_eq!(engine_get_phrase_enabled(), 0);
        engine_process_key('b' as i32);
        engine_process_key('q' as i32);
        let mut has_phrase = false;
        for i in 0..engine_get_candidate_count() {
            let mut b = [0u8; 64];
            engine_get_candidate(i, b.as_mut_ptr() as *mut c_char, b.len() as i32);
            if read_cstr(&b) == "不客气" {
                has_phrase = true;
            }
        }
        assert!(!has_phrase, "关闭短语后 bq 不应出短语");
        engine_reset();
        engine_set_phrase_enabled(1);
        engine_destroy();

        // ── 销毁 ──
        engine_destroy();
        // 销毁后 FFI 返回错误码而非崩溃（0.1.10 可靠性契约）
        assert_eq!(engine_get_ascii_mode(), -1);
        assert_eq!(engine_process_key('a' as i32), -1);
        assert_eq!(engine_set_candidate_count(5), -1);
        assert_eq!(engine_get_fuzzy(), -1);
        assert_eq!(engine_get_shuangpin(), -1);
        assert_eq!(engine_page(1), 0);
        // 重新初始化恢复
        assert_eq!(engine_init(std::ptr::null()), 0);
        assert_eq!(engine_get_ascii_mode(), 0);
        engine_destroy();
    }
}

/// 性能基准：按键→查询全链路延迟（release 模式运行）
///
/// 运行：cargo test --release --test ffi_integration perf_ -- --ignored --nocapture
/// 指标：单键平均查询延迟（含锁 + 词库前缀匹配 + 模糊音变体）
#[test]
#[ignore]
fn perf_key_query_latency() {
    // SAFETY: FFI 调用，单线程测试环境
    unsafe {
        engine_init(std::ptr::null());
    }

    // 预热
    for ch in "zhongguo".chars() {
        let _ = engine_process_key(ch as i32);
    }
    engine_reset();

    const ROUNDS: u32 = 20_000;
    let start = std::time::Instant::now();
    for _ in 0..ROUNDS {
        for ch in "zhongguo".chars() {
            let _ = engine_process_key(ch as i32);
        }
        engine_reset();
    }
    let elapsed = start.elapsed();

    let keys = ROUNDS as u64 * 8; // zhongguo = 8 键
    let per_key_us = elapsed.as_micros() as f64 / keys as f64;
    let per_round_us = elapsed.as_micros() as f64 / ROUNDS as f64;
    println!("性能基准: {keys} 键 / {:.2?} → 单键 {per_key_us:.2}µs, 整串(8键) {per_round_us:.2}µs", elapsed);

    // 宽松断言防极端回归（正常应 < 50µs/键）
    assert!(per_key_us < 500.0, "单键延迟 {per_key_us:.2}µs 超阈值 500µs");

    // SAFETY: FFI 调用，单线程测试环境
    unsafe {
        engine_destroy();
    }
}
