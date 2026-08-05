//! 复现测试 v2（Eric 反馈 2026-08-05）— 等待大词库就绪后验证真词库行为
//! 1. nimzai/ganshm/rgshni/yaowoquz 无候选（联想能力）
//! 2. hel/o 提交后状态（候选窗口隐藏条件）
//! 3. 上词逻辑 / 真词库命中

use std::ffi::CStr;
use std::os::raw::c_char;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use taishen_engine::ffi::*;

unsafe fn read_cstr(buf: &[u8]) -> String {
    unsafe {
        CStr::from_ptr(buf.as_ptr() as *const c_char)
            .to_string_lossy()
            .into_owned()
    }
}

fn dict_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("resources")
        .join("system_dict.db")
}

/// 等大词库就绪（最多 15s）
fn wait_ready() {
    unsafe {
        let t0 = Instant::now();
        while engine_dict_ready() == 0 {
            if t0.elapsed() > Duration::from_secs(15) {
                println!("!! 大词库加载超时，继续用兜底词库");
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        println!("大词库就绪: {}ms", t0.elapsed().as_millis());
    }
}

fn type_str(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        for ch in s.chars() {
            engine_process_key(ch as i32);
        }
        let count = engine_get_candidate_count();
        for i in 0..count {
            let mut buf = [0u8; 256];
            let len = engine_get_candidate(i, buf.as_mut_ptr() as *mut c_char, buf.len() as i32);
            if len > 0 {
                out.push(read_cstr(&buf));
            }
        }
    }
    out
}

#[test]
fn repro_all_issues_serial() {
    unsafe {
        engine_init(std::ptr::null());
        let p = dict_path();
        let pstr = p.to_string_lossy().into_owned();
        engine_init(pstr.as_ptr() as *const c_char);
        wait_ready();

        // ── 问题1：简拼/混合输入联想 ──
        for input in [
            "nimzai", "ganshm", "rgshni", "yaowoquz", "nz", "gsm", "yaoqu",
        ] {
            engine_reset();
            let cands = type_str(input);
            println!("输入 {input}: 候选 {} 个: {:?}", cands.len(), cands);
        }

        // 字典层直查（定位联想路径）
        {
            for input in ["rgshni", "yaowoquz", "ganshm"] {
                let combo = taishen_engine::dictionary::query_combo(input);
                let guess = taishen_engine::dictionary::combo_guess(input);
                println!("query_combo({input}) = {:?}", combo);
                println!("combo_guess({input}) = {:?}", guess);
            }
        }

        // ── 词库验证：这些词在不在 ──
        for (py, w) in [
            ("nimazai", "你在吗"),
            ("ganshenme", "干什么"),
            ("woqu", "我去"),
            ("yaoqu", "要去"),
        ] {
            engine_reset();
            let cands = type_str(py);
            println!("全拼 {py} (期望 {w}): {:?}", cands);
        }

        // ── 问题4：hel/o 提交后状态 ──
        engine_reset();
        let cands = type_str("hel");
        println!("hel 候选: {:?}", cands);
        if engine_get_candidate_count() > 0 {
            let mut sel_buf = [0u8; 128];
            let len = engine_select_candidate(
                0,
                sel_buf.as_mut_ptr() as *mut c_char,
                sel_buf.len() as i32,
            );
            println!(
                "  hel 选0上屏: {:?}",
                if len > 0 {
                    read_cstr(&sel_buf)
                } else {
                    String::new()
                }
            );
            let py_len = engine_get_pinyin_str(std::ptr::null_mut(), 0);
            println!(
                "  hel 提交后 pinyin_len={py_len} (1=空) cands={}",
                engine_get_candidate_count()
            );
        }

        engine_reset();
        let cands = type_str("o");
        println!("o 候选: {:?}", cands);
        if engine_get_candidate_count() > 0 {
            let mut sel_buf = [0u8; 128];
            let len = engine_select_candidate(
                0,
                sel_buf.as_mut_ptr() as *mut c_char,
                sel_buf.len() as i32,
            );
            println!(
                "  o 选0上屏: {:?}",
                if len > 0 {
                    read_cstr(&sel_buf)
                } else {
                    String::new()
                }
            );
            let py_len = engine_get_pinyin_str(std::ptr::null_mut(), 0);
            println!(
                "  o 提交后 pinyin_len={py_len} (1=空) cands={}",
                engine_get_candidate_count()
            );
        }

        // ── 问题5：单字上词。查 wo → 选"窝"(0) → 再查 wo 看位置变化 ──
        engine_reset();
        let cands = type_str("wo");
        println!("wo 候选(第1次): {:?}", cands);
        let mut sel_buf = [0u8; 128];
        let len =
            engine_select_candidate(0, sel_buf.as_mut_ptr() as *mut c_char, sel_buf.len() as i32);
        println!(
            "  选0: {:?}",
            if len > 0 {
                read_cstr(&sel_buf)
            } else {
                String::new()
            }
        );
        engine_reset();
        let cands2 = type_str("wo");
        println!("wo 候选(第2次): {:?}", cands2);

        engine_destroy();
    }
}
