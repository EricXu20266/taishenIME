//! 候选排序诊断与黑盒对齐探针：一次看清「词库原始顺序 / 整句解码 / 最终候选 / 形态簇 / L3 黑盒刻度对齐」。
//!
//! 用法:
//!   cargo run --release --example probe_rank -- [--emoji] youxianji
//!   cargo run --release --example probe_rank -- --shadow-cases engine/tests/exam-cases.tsv
//!   cargo run --release --example probe_rank -- --export-snapshot engine/tests/exam-cases.tsv tmp/snapshot.tsv
//!   cargo run --release --example probe_rank -- --align <ref.tsv>

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use taishen_engine::Engine;
use taishen_engine::ranking::{Form, Match};

fn run_shadow_cases(tsv_path: &Path) {
    let text = std::fs::read_to_string(tsv_path)
        .unwrap_or_else(|e| panic!("无法读取用例文件 {}: {e}", tsv_path.display()));
    let mut seen = HashSet::new();
    let mut inputs: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty()
            || line.starts_with('#')
            || (line.starts_with('[') && line.ends_with(']'))
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
                let dim = rhs
                    .split(':')
                    .next()
                    .unwrap_or("unknown")
                    .trim()
                    .to_string();
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

    println!(
        "=== Phase A 影子模式全量差异统计 ({}) ===",
        tsv_path.display()
    );
    println!("去重输入数: {total}");
    println!(
        "有任意顺序变化: {any_changed}/{total} ({:.1}%)",
        any_changed as f64 * 100.0 / total.max(1) as f64
    );
    println!(
        "首位变化: {top1_changed}/{total} ({:.1}%)",
        top1_changed as f64 * 100.0 / total.max(1) as f64
    );
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

/// 导出当前引擎对输入集的候选快照（TSV：input \t top1 \t top5_csv），供跨版本或黑盒基准对比。
fn run_export_snapshot(cases_path: &Path, out_path: &Path) {
    let text = std::fs::read_to_string(cases_path)
        .unwrap_or_else(|e| panic!("无法读取输入文件 {}: {e}", cases_path.display()));
    let mut seen = HashSet::new();
    let mut lines: Vec<String> = vec!["# 候选排序快照（input\\ttop1\\ttop5_csv）".to_string()];
    let mut count = 0usize;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            lines.push(line.to_string());
            continue;
        }
        let Some(first_col) = line.split('\t').next() else {
            continue;
        };
        let inp = first_col.trim();
        if inp.is_empty() || !seen.insert(inp.to_string()) {
            continue;
        }
        let mut e = Engine::new();
        e.set_candidate_limit(9);
        for ch in inp.chars() {
            e.process_key(ch);
        }
        let top5: Vec<String> = (0..e.candidate_count().min(5))
            .filter_map(|i| e.candidate(i).map(str::to_string))
            .collect();
        let top1 = top5.first().map(String::as_str).unwrap_or("");
        lines.push(format!("{inp}\t{top1}\t{}", top5.join(",")));
        count += 1;
    }

    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    std::fs::write(out_path, lines.join("\n") + "\n")
        .unwrap_or_else(|e| panic!("写入快照失败 {}: {e}", out_path.display()));
    println!("已导出 {count} 条快照至 {}", out_path.display());
}

/// L3 黑盒刻度对齐评测：读取 `input \t ref_top1 [\t ref_top5_csv]` 基准文件，
/// 统计首位对齐率、Top-3 / Top-5 覆盖率、分组明细与未对齐条目的 v2 归因。
fn run_align_eval(ref_path: &Path) {
    let text = std::fs::read_to_string(ref_path)
        .unwrap_or_else(|e| panic!("无法读取基准文件 {}: {e}", ref_path.display()));
    let mut group = "(未分组)".to_string();
    let mut total = 0usize;
    let mut top1_hit = 0usize;
    let mut top3_hit = 0usize;
    let mut top5_hit = 0usize;
    // group -> (total, top1, top3, top5)
    let mut group_stats: BTreeMap<String, (usize, usize, usize, usize)> = BTreeMap::new();
    let mut mismatches: Vec<String> = Vec::new();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            group = line[1..line.len() - 1].to_string();
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 2 {
            continue;
        }
        let inp = cols[0].trim();
        let ref_top1 = cols[1].trim();
        if inp.is_empty() || ref_top1.is_empty() {
            continue;
        }

        let mut e = Engine::new();
        e.set_candidate_limit(9);
        for ch in inp.chars() {
            e.process_key(ch);
        }
        let cands: Vec<String> = (0..e.candidate_count())
            .filter_map(|i| e.candidate(i).map(str::to_string))
            .collect();
        let pos = cands.iter().position(|w| w == ref_top1);
        let hit1 = pos == Some(0);
        let hit3 = pos.is_some_and(|p| p < 3);
        let hit5 = pos.is_some_and(|p| p < 5);

        total += 1;
        if hit1 {
            top1_hit += 1;
        }
        if hit3 {
            top3_hit += 1;
        }
        if hit5 {
            top5_hit += 1;
        }
        let entry = group_stats.entry(group.clone()).or_insert((0, 0, 0, 0));
        entry.0 += 1;
        entry.1 += usize::from(hit1);
        entry.2 += usize::from(hit3);
        entry.3 += usize::from(hit5);

        if !hit1 {
            let pos_str = match pos {
                Some(p) => format!("#{}", p + 1),
                None => "miss".to_string(),
            };
            let top3_diag: Vec<String> = (0..e.candidate_count().min(3))
                .map(|i| {
                    let w = e.candidate(i).unwrap_or("");
                    let ml = e
                        .candidate_explain(i)
                        .and_then(|m| m.match_level)
                        .and_then(Match::from_rank)
                        .map(Match::as_str)
                        .unwrap_or("-");
                    let tags = e
                        .candidate_explain(i)
                        .map(|m| taishen_engine::format_reasons(m.reasons))
                        .unwrap_or_default();
                    format!("#{} {w}({ml} [{tags}])", i + 1)
                })
                .collect();
            mismatches.push(format!(
                "  [{group}] {inp:<18} ref={ref_top1}({pos_str}) | {}",
                top3_diag.join("  ")
            ));
        }
    }

    let pct = |n: usize, d: usize| n as f64 * 100.0 / d.max(1) as f64;
    println!("=== L3 黑盒基准对齐评测 ({}) ===", ref_path.display());
    println!("总样本数: {total}");
    println!(
        "Top-1 对齐率: {top1_hit}/{total} ({:.1}%)",
        pct(top1_hit, total)
    );
    println!(
        "Top-3 覆盖率: {top3_hit}/{total} ({:.1}%)",
        pct(top3_hit, total)
    );
    println!(
        "Top-5 覆盖率: {top5_hit}/{total} ({:.1}%)",
        pct(top5_hit, total)
    );
    if group_stats.len() > 1 {
        println!("\n分组统计:");
        for (g, (gt, g1, g3, g5)) in &group_stats {
            println!(
                "  {g:<24} top1={g1}/{gt} ({:.1}%)  top3={g3}/{gt} ({:.1}%)  top5={g5}/{gt} ({:.1}%)",
                pct(*g1, *gt),
                pct(*g3, *gt),
                pct(*g5, *gt)
            );
        }
    }
    if !mismatches.is_empty() {
        println!("\n未对齐明细（共 {} 条，展示前 30 条）:", mismatches.len());
        for line in mismatches.iter().take(30) {
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
    match args.first().map(|s| s.as_str()) {
        Some("--shadow-cases") => {
            let path = args
                .get(1)
                .map(Path::new)
                .unwrap_or_else(|| Path::new("engine/tests/exam-cases.tsv"));
            run_shadow_cases(path);
            return;
        }
        Some("--export-snapshot") => {
            let cases = args
                .get(1)
                .map(Path::new)
                .unwrap_or_else(|| Path::new("engine/tests/exam-cases.tsv"));
            let out = args
                .get(2)
                .map(Path::new)
                .unwrap_or_else(|| Path::new("tmp/ranking-snapshot.tsv"));
            run_export_snapshot(cases, out);
            return;
        }
        Some("--align") => {
            let ref_path = args.get(1).map(Path::new).unwrap_or_else(|| {
                eprintln!("用法: probe_rank --align <ref.tsv>");
                std::process::exit(2);
            });
            run_align_eval(ref_path);
            return;
        }
        _ => {}
    }

    let mut emoji_on = false;
    let mut input: Option<String> = None;
    for arg in &args {
        if arg == "--emoji" {
            emoji_on = true;
        } else if input.is_none() {
            input = Some(arg.clone());
        }
    }
    let input = input.unwrap_or_else(|| "youxianji".to_string());

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
    e.set_emoji(emoji_on);
    for ch in input.chars() {
        e.process_key(ch);
    }
    println!(
        "=== engine candidates ({} 条, emoji={emoji_on}) ===",
        e.candidate_count()
    );
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
                let form_tag = Form::from_rank(m.form)
                    .filter(|&f| f != Form::Text)
                    .map(|f| format!(" form={}", f.as_str()))
                    .unwrap_or_default();
                let cluster_tag = e
                    .candidate_extra(i)
                    .and_then(|ex| ex.cluster)
                    .map(|cl| format!(" cluster={cl}"))
                    .unwrap_or_default();
                match m.score {
                    Some(sc) => format!(
                        "T{} {ml}{form_tag}{cluster_tag} score={sc} [{tags}]",
                        m.tier
                    ),
                    None => format!("T{} {ml}{form_tag}{cluster_tag} [{tags}]", m.tier),
                }
            }
            None => String::new(),
        };
        println!("  c[{i}] {word} ({src}) {explain}");
    }

    if let Some(diff) = e.shadow_diff_current() {
        println!(
            "=== ranking v2 影子对照 (变化 {} 条, top1_changed={}) ===",
            diff.changed.len(),
            diff.top1_changed()
        );
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
