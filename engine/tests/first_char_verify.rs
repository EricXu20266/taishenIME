//! 首字验证集成测试：真实词库 + FFI 链路。
//!
//! 默认测试只跑固定回归集与确定性常用词抽样，避免把 37 万词库的全量扫描
//! 放进普通 `cargo test` 热路径。全量验证保留为 ignored 测试：
//! `cargo test --release --test first_char_verify -- --ignored --nocapture`。

use std::ffi::CStr;
use std::os::raw::c_char;
use std::path::{Path, PathBuf};

use taishen_engine::ffi::*;

unsafe fn read_cstr(buf: &[u8]) -> String {
    unsafe {
        CStr::from_ptr(buf.as_ptr() as *const c_char)
            .to_string_lossy()
            .into_owned()
    }
}

/// 输入拼音，返回当前页候选。缓冲区留足空间，避免把候选截断误判为排序问题。
fn type_and_cands(py: &str) -> Vec<String> {
    engine_reset();
    for ch in py.chars() {
        engine_process_key(ch as i32);
    }
    let n = engine_get_candidate_count();
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n {
        let mut b = [0u8; 256];
        engine_get_candidate(i, b.as_mut_ptr() as *mut c_char, b.len() as i32);
        out.push(unsafe { read_cstr(&b) });
    }
    out
}

fn init_real_dict() -> PathBuf {
    let dict = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("resources/system_dict.db");
    let dict_c = std::ffi::CString::new(dict.to_string_lossy().as_bytes()).unwrap();
    assert_eq!(engine_init(dict_c.as_ptr()), 0, "engine_init 应成功");
    engine_set_candidate_count(9);
    let mut waited_ms = 0;
    while engine_dict_ready() == 0 && waited_ms < 20_000 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        waited_ms += 100;
    }
    assert_eq!(
        engine_dict_ready(),
        1,
        "真实词库应就绪（等待 {waited_ms}ms）"
    );
    println!("词库就绪: {dict:?}");
    dict
}

fn load_common_entries(res_dir: &Path) -> Vec<(String, String)> {
    let content =
        std::fs::read_to_string(res_dir.join("common_dict.txt")).expect("common_dict.txt 应存在");
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let mut it = line.split('\t');
            let py = it.next()?.trim();
            let word = it.next()?.trim();
            if py.is_empty() || word.is_empty() {
                None
            } else {
                Some((py.to_string(), word.to_string()))
            }
        })
        .collect()
}

fn verify_common_entries(entries: &[(String, String)], label: &str) {
    let mut not_on_screen = 0;
    for (py, word) in entries {
        let cands = type_and_cands(py);
        if !cands.iter().take(9).any(|candidate| candidate == word) {
            not_on_screen += 1;
            println!(
                "✗ {py}\t{word} 不在第一屏  实际:{}",
                cands.iter().take(9).cloned().collect::<Vec<_>>().join("/")
            );
        }
    }
    println!(
        "第一屏验证[{label}]: {} 常用词, {not_on_screen} 个不在第一屏",
        entries.len()
    );
    assert_eq!(
        not_on_screen,
        0,
        "{label}: {not_on_screen}/{} 常用词不在第一屏",
        entries.len()
    );
}

#[test]
fn first_char_verify_with_real_dict() {
    let dict = init_real_dict();
    let cases: &[(&str, &str)] = &[
        ("en", "嗯"),
        ("wo", "我"),
        ("ni", "你"),
        ("ta", "他"),
        ("hao", "好"),
        ("haode", "好的"),
        ("zhege", "这个"),
        ("name", "那么"),
        ("mei", "没"),
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
            "[{}] {py:<8} 期望首位:{expect} 第一屏:{}",
            if ok { "OK " } else { "FAIL" },
            cands
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join(" / ")
        );
    }
    assert_eq!(fail, 0, "{fail} 个用例首位不符合预期");

    let res_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("resources");
    let entries = load_common_entries(&res_dir);
    let stride = (entries.len() + 127) / 128;
    let sample: Vec<_> = entries.iter().step_by(stride.max(1)).cloned().collect();
    verify_common_entries(&sample, "deterministic-sample");
    engine_destroy();
    let _ = dict;
}

/// 全量常用词第一屏验证：只显式执行，不阻塞日常回归。
#[test]
#[ignore = "真实大词库全量候选回归耗时高，需显式运行"]
fn first_char_verify_common_dict_exhaustive() {
    let _dict = init_real_dict();
    let res_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("resources");
    let entries = load_common_entries(&res_dir);
    verify_common_entries(&entries, "exhaustive");
    engine_destroy();
}
