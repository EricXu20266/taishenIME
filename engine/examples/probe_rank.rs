//! 候选排序诊断探针：一次看清「词库原始顺序 / 整句解码 / 最终候选 / 排序 v2 影子对照」。
//!
//! 用法:
//!   cargo run --release --example probe_rank -- youxianji
//!   cargo run --release --example probe_rank -- --shadow-cases engine/tests/exam-cases.tsv

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use taishen_engine::ranking::Match;
use taishen_engine::Engine;

fn run_shadow_cases(tsv_path: &Path) {
    let text = std::fs::read_to_string(tsv_path)
        .unwrap_or_else(|e| panic!("无法读取用例文件 {}: {e}", tsv_path.display()));
    let mut seen = HashSet::new();
    let mut inputs: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || (line.starts_with('[') && line.ends_with(']'))
        {
            continue;
        }
        if let Some(first_col) = line.split('\t').next() {
            let inp = first_col.trim().to_string();
            if !inp.is_empty() && seen.insert(inp.clone()) {
                inputs.push(inp);
            }
        }
    }

    let mut total = 0usize;
    let mut any_changed = 0usize;
    let mut top1_changed = 0usize;
    let mut unattributed = 0usize;
    let mut dim_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut top1_examples: Vec<String> = Vec::new();

    let probe = Engine::new();
    for inp in &inputs {
        let Some(diff) = probe.diff_ranking(inp) else {
            continue;
        };
        total += 1;
        if !diff.changed.is_empty() {
            any_changed += 1;
        }
        if diff.changed.len() != diff.explanation.len()
            || diff.explanation.iter().any(|s| s.is_empty())
        {
            unattributed += 1;
        }
        for exp in &diff.explanation {
            // 格式: "<word> (a -> b): <dim>: ..."
            if let Some((_, rhs)) = exp.split_once("): ") {
                let dim = rhs.split(':').next().unwrap_or("unknown").trim().to_string();
                *dim_counts.entry(dim).or_insert(0) += 1;
            }
        }
        if diff.top1_changed() {
            top1_changed += 1;
            let old0 = diff.old_order.first().map(|s| s.as_str()).unwrap_or("-");
            let new0 = diff.new_order.first().map(|s| s.as_str()).unwrap_or("-");
            let reason = diff
                .explanation
                .iter()
                .find(|e| e.starts_with(&format!("{new0} (")))
                .cloned()
                .unwrap_or_default();
            top1_examples.push(format!("  {inp:<18} old={old0}  new={new0}  | {reason}"));
        }
    }

    println!("=== Phase A 影子模式全量差异统计 ({}) ===", tsv_path.display());
    println!("去重输入数: {total}");
    println!("有任意顺序变化: {any_changed}/{total} ({:.1}%)", any_changed as f64 * 100.0 / total.max(1) as f64);
    println!("首位变化: {top1_changed}/{total} ({:.1}%)", top1_changed as f64 * 100.0 / total.max(1) as f64);
    println!("不可归因差异数: {unattributed}");
    println!();
    println!("差异维度分布（按变动条次计）:");
    for (dim, cnt) in &dim_counts {
        println!("  {dim:<18} {cnt}");
    }
    if !top1_examples.is_empty() {
        println!();
        println!("首位变化明细 ({} 条):", top1_examples.len());
        for line in &top1_examples {
            println!("{line}");
        }
    }
}

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

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|s| s.as_str()) == Some("--shadow-cases") {
        let path = args
            .get(1)
            .map(Path::new)
            .unwrap_or_else(|| Path::new("engine/tests/exam-cases.tsv"));
        run_shadow_cases(path);
        return;
    }

    let input = args
        .first()
        .cloned()
        .unwrap_or_else(|| "youxianji".to_string());

    println!("=== dictionary::query(\"{input}\") ===");
    for (i, w) in taishen_engine::dictionary::query(&input)
        .iter()
        .enumerate()
        .take(12)
    {
        println!(
            "  q[{i}] {w} common={}",
            taishen_engine::dictionary::is_common_word(w)
        );
    }

    println!("=== sentence::decode_meta ===");
    for (i, entry) in taishen_engine::sentence::decode_meta(&input, None, 3)
        .iter()
        .enumerate()
    {
        println!(
            "  s[{i}] {} (segs={}, clean={}, score={:.2})",
            entry.text, entry.segment_count, entry.clean_coverage, entry.score
        );
    }

    let mut e = Engine::new();
    e.set_candidate_limit(12);
    for ch in input.chars() {
        e.process_key(ch);
    }
    println!("=== engine candidates ({} 条) ===", e.candidate_count());
    for i in 0..e.candidate_count().min(12) {
        let word = e.candidate(i).unwrap_or("");
        let src = e.candidate_source_name(i).unwrap_or("?");
        let explain = match e.candidate_explain(i) {
            Some(m) => {
                let tags = taishen_engine::format_reasons(m.reasons);
                let ml = m
                    .match_level
                    .and_then(Match::from_rank)
                    .map(Match::as_str)
                    .unwrap_or("-");
                match m.score {
                    Some(sc) => format!("T{} {ml} score={sc} [{tags}]", m.tier),
                    None => format!("T{} {ml} [{tags}]", m.tier),
                }
            }
            None => String::new(),
        };
        println!("  c[{i}] {word} ({src}) {explain}");
    }

    if let Some(diff) = e.shadow_diff_current() {
        println!("=== ranking v2 影子对照 (变化 {} 条, top1_changed={}) ===", diff.changed.len(), diff.top1_changed());
        println!(
            "  v2 前 12: {}",
            diff.new_order
                .iter()
                .take(12)
                .enumerate()
                .map(|(i, w)| format!("#{i} {w}"))
                .collect::<Vec<_>>()
                .join("  ")
        );
        for exp in diff.explanation.iter().take(12) {
            println!("  diff: {exp}");
        }
    }
}
