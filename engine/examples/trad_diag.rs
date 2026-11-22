//! 繁体模式候选诊断：输出每个候选的 match_level 与 reasons（诊断用，不入库）。
//!
//! 用法：
//!   cargo run --release --example trad_diag                 # 干净引擎 + 繁体
//!   cargo run --release --example trad_diag -- --user       # 加载真实用户词库
//!
//! 用途：对照 b4e22ae 前后「原生繁体词是否仍落入 Match::Foreign、是否排到末尾」。

use std::path::PathBuf;
use taishen_engine::{dictionary, Engine};

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let dict = root.join("resources/system_dict.db");
    let use_user = std::env::args().any(|a| a == "--user");
    let user_db = std::env::var("APPDATA")
        .ok()
        .map(|a| PathBuf::from(a).join("taishen-ime").join("user_dict.db"))
        .filter(|p| p.exists());

    dictionary::init(Some(dict.as_path()));
    for _ in 0..600 {
        if dictionary::is_ready() && dictionary::is_common_word("你好") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    if use_user {
        if let Some(u) = &user_db {
            dictionary::set_user_dict_path(Some(u.as_path()));
        }
    }
    println!("[env] trad=on user={use_user}");

    let inputs = [
        "lai", "guo", "hui", "shuo", "kai", "shi", "ji", "ren", "fei", "cuo",
        "toufa", "xiangzhen", "xitong", "zhongguo", "ruanjian", "faxian", "women",
        "hou", "panming", "nezha", "jintiantianqi", "womenyiqi",
    ];
    for input in inputs {
        let mut e = Engine::new();
        e.set_traditional(true);
        e.set_candidate_limit(40);
        for ch in input.chars() {
            e.process_key(ch);
        }
        let n = e.candidate_count();
        let mut shown = 0;
        let mut line = String::new();
        for i in 0..n {
            let Some(text) = e.candidate_display(i) else { break };
            let m = e.candidate_explain(i).unwrap_or_default();
            let ml = m.match_level.unwrap_or(255);
            line.push_str(&format!("#{}({},{}) ", text, ml, m.reasons));
            shown += 1;
            if shown >= 12 {
                break;
            }
        }
        println!("{input:>14} n={n:>3} {line}");
    }
}
