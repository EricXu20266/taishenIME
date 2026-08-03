//! 首字验证集成测试（V0.2.30）— 走完整 FFI 链路 + 真实词库，
//! 与部署 DLL 内的引擎代码一致（引擎静态链接进 taishen_ime.dll）。
//!
//! 运行: cargo test --release --test first_char_verify -- --nocapture
//! 单独文件运行（不与其他 FFI 测试并行，避免全局 ENGINE 冲突）。

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

/// 输入拼音，返回全部候选（当前页）
unsafe fn type_and_cands(py: &str) -> Vec<String> {
    engine_reset();
    for ch in py.chars() {
        engine_process_key(ch as i32);
    }
    let n = engine_get_candidate_count();
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n {
        let mut b = [0u8; 64];
        engine_get_candidate(i, b.as_mut_ptr() as *mut c_char, b.len() as i32);
        out.push(read_cstr(&b));
    }
    out
}

#[test]
fn first_char_verify_with_real_dict() {
    unsafe {
        // ── 真实词库初始化（与部署目录同源 resources/system_dict.db）──
        let dict = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("resources/system_dict.db");
        let dict_c = std::ffi::CString::new(dict.to_string_lossy().as_bytes()).unwrap();
        assert_eq!(engine_init(dict_c.as_ptr()), 0, "engine_init 应成功");
        let mut waited_ms = 0;
        while engine_dict_ready() == 0 && waited_ms < 20_000 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            waited_ms += 100;
        }
        assert_eq!(engine_dict_ready(), 1, "真实词库应就绪（等待 {waited_ms}ms）");
        println!("词库就绪: {dict:?}");

        // ── 验证用例：期望首位 ──
        let cases: &[(&str, &str)] = &[
            // Eric 指定高频词
            ("en", "嗯"),
            ("wo", "我"),
            ("ni", "你"),
            ("ta", "他"),
            ("hao", "好"),
            ("haode", "好的"),
            ("zhege", "这个"),
            ("name", "那么"),
            ("mei", "没"),
            // 首字检查修正的多音字/生僻字
            ("hu", "胡"),
            ("huo", "活"),
            ("kuai", "快"),
            ("dai", "带"),
            ("ku", "哭"),
            ("pa", "怕"),
            ("ao", "奥"),
            ("eng", "嗯"),
            ("wang", "王"),
            ("ka", "卡"),
            ("juan", "卷"),
            ("meng", "梦"),
            ("ou", "欧"),
            ("pei", "配"),
            ("gou", "够"),
            ("chong", "冲"),
            ("ruan", "软"),
            ("shuai", "帅"),
            ("tun", "吞"),
            ("yo", "哟"),
        ];
        let mut fail = 0;
        for (py, expect) in cases {
            let cands = type_and_cands(py);
            let first = cands.first().cloned().unwrap_or_default();
            let ok = &first == expect;
            if !ok {
                fail += 1;
            }
            println!(
                "[{}] {py:<8} 期望:{expect}  首位:{first}  候选:{}",
                if ok { "OK " } else { "FAIL" },
                cands.iter().take(4).cloned().collect::<Vec<_>>().join("/")
            );
        }
        assert_eq!(fail, 0, "{fail} 个用例首位不符合预期");
        engine_destroy();
    }
}
