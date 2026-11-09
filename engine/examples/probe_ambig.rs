//! 歧义切分诊断探针：检查「预案 / 西安 / 提案」这类
//! 「连写拼音同时是单音节」的词在候选中的真实位置，以及组词模式是否可进入。
//!
//! 用法: cargo run --release --example probe_ambig -- yuan xian tian

use std::path::Path;
use taishen_engine::Engine;

fn dump(engine: &mut Engine, input: &str, targets: &[&str]) {
    engine.reset();
    for ch in input.chars() {
        engine.process_key(ch);
    }
    let pages = engine.total_pages();
    let mut all: Vec<String> = Vec::new();
    for _ in 0..pages {
        let n = engine.candidate_count();
        for i in 0..n {
            if let Some(c) = engine.candidate(i) {
                all.push(c.to_string());
            }
        }
        if engine.page(1) == 0 {
            break;
        }
    }
    println!("[{}] 总页数={} 候选数={}", input, pages, all.len());
    for t in targets {
        let pos = all.iter().position(|w| w == t);
        println!("  {} → 位置 {:?}", t, pos.map(|p| p + 1));
    }
    let head: Vec<&String> = all.iter().take(12).collect();
    println!("  前12: {:?}", head);

    // 组词模式是否可进入
    engine.reset();
    for ch in input.chars() {
        engine.process_key(ch);
    }
    engine.page(1);
    engine.page(1);
    engine.page(1);
    println!(
        "  组词模式: active={} 首音节={:?}",
        engine.compose_active(),
        engine.compose_current_syllable()
    );

    // 组词流程实测：yuan → 翻页找「预」→ 找「案」
    if engine.compose_active() {
        let stage = |e: &Engine| -> Vec<String> {
            (0..e.candidate_count())
                .filter_map(|i| e.candidate(i).map(|s| s.to_string()))
                .collect()
        };
        // 组词模式内翻页找「预」（yu 的单字候选不止一页）
        let mut picked = None;
        for _ in 0..8 {
            let cur = stage(&engine);
            if let Some(i) = cur.iter().position(|w| w == "预") {
                picked = Some((i, cur.clone()));
                break;
            }
            if engine.page(1) == 0 {
                break;
            }
        }
        match picked {
            Some((i, cur)) => {
                let got = engine.select_candidate(i);
                let second = stage(&engine);
                println!(
                    "  第1段命中 {:?}（页内候选 {:?}）→ 第2段: {:?}",
                    got,
                    &cur[..cur.len().min(5)],
                    &second[..second.len().min(8)]
                );
                if let Some(j) = second.iter().position(|w| w == "案") {
                    println!("  选中 {:?} → 组词完成", engine.select_candidate(j));
                }
            }
            None => println!("  第1段未找到「预」"),
        }
    }
}

fn main() {
    let dict = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("resources/system_dict.db");
    taishen_engine::dictionary::init(Some(dict.as_path()));
    // 组词实测会 learn 用户词 → 重定向到临时库，避免污染真实用户词库
    let tmp_user = std::env::temp_dir().join("taishen_probe_ambig_user.db");
    let _ = std::fs::remove_file(&tmp_user);
    taishen_engine::dictionary::set_user_dict_path(Some(&tmp_user));
    for _ in 0..600 {
        if taishen_engine::dictionary::is_ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    let cases: Vec<(String, Vec<&'static str>)> = if args.is_empty() {
        vec![
            ("yuan".into(), vec!["预案", "元", "原"]),
            ("xian".into(), vec!["西安", "先", "现"]),
            ("tian".into(), vec!["提案", "天", "田"]),
            ("fangan".into(), vec!["方案"]),
        ]
    } else {
        args.iter()
            .map(|a| (a.clone(), vec!["预案", "西安", "提案", "方案"]))
            .collect()
    };
    let mut engine = Engine::new();
    for (input, targets) in cases {
        dump(&mut engine, &input, &targets);
    }
}
