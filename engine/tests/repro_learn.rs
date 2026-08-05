//! 上词逻辑专项验证（Eric 反馈 2026-08-05）
//! 1. 全拼上词：wo 选"喔" → 再查 wo 位置变化
//! 2. 简拼上词：gsm 选"干什么" → 再查 gsm（query_short 是否查用户词）
//! 3. 真词库写入：user_dict.db 是否有词条

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

fn wait_ready() {
    unsafe {
        let t0 = Instant::now();
        while engine_dict_ready() == 0 {
            if t0.elapsed() > Duration::from_secs(20) {
                println!("!! 大词库加载超时");
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

fn select(idx: i32) -> String {
    unsafe {
        let mut buf = [0u8; 256];
        let len = engine_select_candidate(idx, buf.as_mut_ptr() as *mut c_char, buf.len() as i32);
        if len > 0 {
            read_cstr(&buf)
        } else {
            String::new()
        }
    }
}

#[test]
fn repro_learn_behavior() {
    unsafe {
        engine_init(std::ptr::null());
        let p = dict_path();
        let pstr = p.to_string_lossy().into_owned();
        engine_init(pstr.as_ptr() as *const c_char);
        wait_ready();

        // 测试用临时用户词库
        let tmp = std::env::temp_dir().join("taishen_repro_user.db");
        let _ = std::fs::remove_file(&tmp);
        let tp = tmp.to_string_lossy().into_owned();
        engine_set_user_dict_path(tp.as_ptr() as *const c_char);

        // ── 1. 全拼上词：wo 候选，选第 2 位"喔" ──
        engine_reset();
        let cands1 = type_str("wo");
        println!("wo 候选(1): {:?}", cands1);
        let sel = select(1);
        println!("  选 index1: {sel}");
        engine_reset();
        let cands2 = type_str("wo");
        println!("wo 候选(2): {:?}", cands2);

        // ── 2. 简拼上词：gsm 候选，选"干什么" ──
        engine_reset();
        let cands3 = type_str("gsm");
        println!("gsm 候选(1): {:?}", cands3);
        let pos = cands3.iter().position(|w| w == "干什么");
        if let Some(pi) = pos {
            let sel = select(pi as i32);
            println!("  选 干什么@index{pi}: {sel}");
            engine_reset();
            let cands4 = type_str("gsm");
            println!("gsm 候选(2): {:?}", cands4);
            engine_reset();
            let cands5 = type_str("ganshenme");
            println!("ganshenme 候选(全拼互查): {:?}", cands5);
        } else {
            println!("  !! gsm 无'干什么'候选，跳过简拼上词验证");
        }

        // ── 3. 真词库磁盘内容 ──
        if tmp.exists() {
            use rusqlite::Connection;
            if let Ok(conn) = Connection::open(&tmp) {
                let mut stmt = conn
                    .prepare("SELECT pinyin, word, frequency, last_used FROM user_dict")
                    .unwrap();
                let rows = stmt
                    .query_map([], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, u32>(2)?,
                            r.get::<_, i64>(3)?,
                        ))
                    })
                    .unwrap();
                println!("=== user_dict.db 内容 ===");
                for row in rows.flatten() {
                    println!("  {} → {} freq={} last={}", row.0, row.1, row.2, row.3);
                }
            }
        } else {
            println!("!! 用户词库文件不存在: {}", tmp.display());
        }

        engine_destroy();
    }
}
