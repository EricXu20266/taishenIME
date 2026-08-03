//! 第一屏检查工具（V0.2.30 诊断）：枚举全部拼音音节 + 常用词表拼音，
//! 输出每个拼音的前 5 个候选（第一屏），两级检查：
//!   FAIL（硬）：常用词表中的词不在前 5（第一屏不可达）
//!   WARN（软）：前 5 候选含生僻字（扩展区 B+ / 罕见字）
//!
//! 用法: cargo run --release --example check_first > tmp/first_screen_report.txt

use std::path::Path;

use taishen_engine::dictionary::Dictionary;
use taishen_engine::pinyin;

/// 第一屏大小（与 engine page_size 默认一致）
const SCREEN: usize = 5;

/// 判定一个字符是否"生僻"（CJK 扩展 A 之外，或扩展 A 中非高频）
fn is_rare_char(c: char) -> bool {
    let cp = c as u32;
    // 扩展 A：0x3400-0x4DBF（多为罕见字）
    // 扩展 B+：0x20000+（生僻）
    // 兼容区/私有区也视为生僻
    cp >= 0x20000 || (0x3400..=0x4DBF).contains(&cp) || (0xF900..=0xFAFF).contains(&cp)
}

/// 判定候选词是否含生僻字
fn has_rare_char(word: &str) -> bool {
    word.chars().any(is_rare_char)
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

    println!("# 泰深输入法 — 第一屏检查报告（前 {SCREEN} 候选）");
    println!("# FAIL = 常用词不在前 {SCREEN}（第一屏不可达）  WARN = 前 {SCREEN} 含生僻字\n");

    // ── 0. 读常用词表 ──
    let res_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("resources");
    let content =
        std::fs::read_to_string(res_dir.join("common_dict.txt")).expect("common_dict.txt");
    // 每个拼音的全部 common 词（行序即优先级）
    let mut common_words: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split('\t');
        let py = it.next().unwrap_or("").trim().to_string();
        let w = it.next().unwrap_or("").trim().to_string();
        if py.is_empty() || w.is_empty() {
            continue;
        }
        common_words.entry(py).or_default().push(w);
    }

    // ── 1. 全部单音节第一屏候选 ──
    println!(
        "=== 单音节第一屏（全部 {} 音节）===",
        pinyin::all_syllables().len()
    );
    let mut warns: Vec<(&str, String, Vec<String>)> = Vec::new();
    let mut fails: Vec<(&str, String, Vec<String>)> = Vec::new();
    for &syl in pinyin::all_syllables() {
        let r = d.query(syl);
        let screen: Vec<String> = r.iter().take(SCREEN).cloned().collect();
        let flag = if screen.iter().any(|w| is_suspicious(w)) {
            warns.push((syl, screen[0].clone(), screen.clone()));
            "WARN"
        } else {
            ""
        };
        // 常用词前 5 可达检查（FAIL）
        if let Some(cw) = common_words.get(syl) {
            for w in cw {
                if !screen.contains(w) {
                    fails.push((syl, w.clone(), screen.clone()));
                }
            }
        }
        println!("{syl}\t{}\t{flag}", screen.join(" / "));
    }

    // ── 2. 常用词表拼音第一屏验证 ──
    println!("\n=== 常用词表拼音第一屏验证 ===");
    let mut fail2 = 0;
    for (py, words) in &common_words {
        let r = d.query(py);
        let screen: Vec<String> = r.iter().take(SCREEN).cloned().collect();
        for w in words {
            if !screen.contains(w) {
                fail2 += 1;
                println!("✗ {py}\t期望:{w}  不在前{SCREEN}  实际:{:?}", screen);
            }
        }
    }
    println!(
        "常用词表验证: {} 拼音, {} 个词不在第一屏",
        common_words.len(),
        fail2
    );

    // ── 3. 汇总 ──
    println!("\n=== FAIL 汇总（常用词不在第一屏，{} 个）===", fails.len());
    let mut seen = std::collections::HashSet::new();
    for (syl, w, _) in &fails {
        if seen.insert((*syl, w.clone())) {
            println!("{syl}\t{w}");
        }
    }
    println!("\n=== WARN 汇总（前 5 含生僻字，{} 个）===", warns.len());
    for (syl, first, screen) in &warns {
        println!("{syl}\t首位:{first}\t前5:{:?}", screen);
    }
    eprintln!(
        "检查完成: {} 音节, {} FAIL, {} WARN, 耗时 {:?}",
        pinyin::all_syllables().len(),
        fails.len(),
        warns.len(),
        now.elapsed()
    );
}
