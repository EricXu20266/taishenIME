//! 用户真实环境复现探针：加载用户词库副本 + 用户 config 里影响排序的开关。
//!
//! 用法: cargo run --release --example probe_live -- <user_dict.db 副本> <输入串...>
//! 环境变量: TS_SORT_MODE(默认 1) TS_LIMIT(默认 5) TS_FUZZY(默认 1)
//!
//! 存在的意义：probe_rank 只加载系统词库，复现不了「用户词层 + 单字优先 +
//! 模糊音」叠加后才出现的错排（huiyudao → 会与到）。

use std::path::Path;
use taishen_engine::Engine;

fn main() {
    let dict = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("resources/system_dict.db");
    taishen_engine::dictionary::init(Some(dict.as_path()));
    for _ in 0..600 {
        if taishen_engine::dictionary::is_ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let bigram = dict.with_file_name("bigram.db");
    if bigram.exists() {
        let _ = taishen_engine::dictionary::set_bigram(&bigram, 1, usize::MAX);
    }
    let mut args = std::env::args().skip(1);
    let user = args.next().expect("缺用户词库路径");
    if user != "-" {
        taishen_engine::dictionary::set_user_dict_path(Some(Path::new(&user)));
    }
    let env_i = |k: &str, d: i32| {
        std::env::var(k)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(d)
    };
    for input in args {
        println!("=== sentence::decode({input}) ===");
        for (i, w) in taishen_engine::sentence::decode(&input, None, 5)
            .iter()
            .enumerate()
        {
            println!("  s[{i}] {w}");
        }
        let mut e = Engine::new();
        e.set_sort_mode(env_i("TS_SORT_MODE", 1));
        e.set_fuzzy_enabled(env_i("TS_FUZZY", 1) != 0);
        e.set_candidate_limit(env_i("TS_LIMIT", 5) as usize);
        for ch in input.chars() {
            e.process_key(ch);
        }
        println!("=== engine({input}) {} 条 ===", e.candidate_count());
        for i in 0..e.candidate_count().min(10) {
            let word = e.candidate(i).unwrap_or("");
            let src = e.candidate_source_name(i).unwrap_or("?");
            let explain = match e.candidate_explain(i) {
                Some(m) => format!(
                    "T{} [{}]",
                    m.tier,
                    taishen_engine::format_reasons(m.reasons)
                ),
                None => String::new(),
            };
            println!("  c[{i}] {word} ({src}) {explain}");
        }
    }
}
