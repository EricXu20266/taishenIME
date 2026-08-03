//! 首字检查工具（V0.2.30 诊断）：枚举全部拼音音节 + 常用词表拼音，
//! 输出每个拼音的首位候选，标记可疑项（首位为生僻字/扩展区字）。
//!
//! 用法: cargo run --release --example check_first > tmp/first_char_report.txt

use std::path::Path;

use taishen_engine::dictionary::Dictionary;
use taishen_engine::pinyin;

/// 判定一个字符是否"生僻"（CJK 扩展 A 之外，或扩展 A 中非高频）
fn is_rare_char(c: char) -> bool {
    let cp = c as u32;
    // 扩展 A：0x3400-0x4DBF（多为罕见字）
    // 扩展 B+：0x20000+（生僻）
    // 兼容区/私有区也视为生僻
    cp >= 0x20000 || (0x3400..=0x4DBF).contains(&cp) || (0xF900..=0xFAFF).contains(&cp)
}

/// 判定候选词是否"可疑"（含生僻字或全是罕见字）
fn is_suspicious(word: &str) -> bool {
    // 单字：本身生僻
    if word.chars().count() == 1 {
        return is_rare_char(word.chars().next().unwrap());
    }
    // 多字：含生僻字
    word.chars().any(is_rare_char)
}

fn main() {
    let db = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("resources/system_dict.db");
    let d = Dictionary::from_sqlite(&db).expect("词库加载失败");
    let now = std::time::Instant::now();

    println!("# 泰深输入法 — 首字检查报告（全部单音节 + 常用词表）");
    println!("# 可疑 = 首位候选含生僻字（扩展区/罕见字）\n");

    // ── 1. 全部单音节拼音首位候选 ──
    println!("=== 单音节首位候选（全部 {} 音节）===", pinyin::all_syllables().len());
    let mut suspicious: Vec<(&str, String, String)> = Vec::new();
    for syl in pinyin::all_syllables() {
        let r = d.query(syl);
        let first = r.first().cloned().unwrap_or_default();
        let flag = if is_suspicious(&first) {
            suspicious.push((syl, first.clone(), r.get(1).cloned().unwrap_or_default()));
            "⚠"
        } else {
            ""
        };
        println!("{syl}\t{first}\t{flag}");
    }

    // ── 2. 常用词表拼音首位验证 ──
    println!("\n=== 常用词表拼音首位验证 ===");
    let res_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("resources");
    let content = std::fs::read_to_string(res_dir.join("common_dict.txt")).expect("common_dict.txt");
    // 每个拼音取第一个词作为期望首位（行序即优先级）
    let mut expected: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split('\t');
        let py = it.next().unwrap_or("").trim();
        let w = it.next().unwrap_or("").trim();
        if py.is_empty() || w.is_empty() {
            continue;
        }
        expected.entry(py.to_string()).or_insert_with(|| w.to_string());
    }
    let mut mismatch = 0;
    for (py, w) in &expected {
        let r = d.query(py);
        let first = r.first().cloned().unwrap_or_default();
        if first != *w {
            mismatch += 1;
            println!("✗ {py}\t期望:{w}\t实际:{first}\t全部:{:?}", r.iter().take(4).collect::<Vec<_>>());
        }
    }
    println!("常用词表验证: {} 拼音, {} 个不匹配", expected.len(), mismatch);

    // ── 3. 可疑首位汇总（供人工优化）──
    println!("\n=== 可疑首位汇总（{} 个）===", suspicious.len());
    for (syl, first, second) in &suspicious {
        println!("{syl}\t首位:{first}\t次位:{second}");
    }
    eprintln!("检查完成: {} 音节, {} 可疑, 耗时 {:?}", pinyin::all_syllables().len(), suspicious.len(), now.elapsed());
}
