//! 单音节候选体检（IME 侧）。
//!
//! 用**引擎真实候选**导出每个单音节拼音的前 N 个候选与来源，和词库侧 P0 基线
//! （`sessions/082febf2/outputs/audit/p0_single_syllable_candidates.jsonl`）同口径对比。
//!
//! 与基线的差异：基线用 `engine_get_candidate_source`（按词反查，同一个词在不同
//! 拼音下分不清来源），这里改用 `Engine::candidate_source(index)`（按索引），
//! 所以 `completion` 判定是准的。
//!
//! 用法：
//!   cargo run --release --example audit_single -- --page 30
//!   cargo run --release --example audit_single -- --page 30 --baseline <jsonl> --out <jsonl>
//!
//! 输出：每个拼音一行 jsonl（pinyin/db_count/shown/candidates[{rank,word,type,source}]），
//! 末尾打印对比表（补全侵占 / 首位类型 / 前 5 位被占）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use taishen_engine::{Engine, Source};

fn find_dict() -> Option<PathBuf> {
    for cand in ["resources/system_dict.db", "../resources/system_dict.db"] {
        let pb = PathBuf::from(cand);
        if pb.is_file() {
            return Some(pb);
        }
    }
    None
}

fn default_baseline() -> PathBuf {
    PathBuf::from("../sessions/082febf2/outputs/audit/p0_single_syllable_candidates.jsonl")
}

/// 从基线 jsonl 抽 (拼音, db_count)。基线是单行紧凑 JSON，字段顺序固定。
fn load_baseline(path: &Path) -> Vec<(String, usize)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let Some(p) = line.find("\"pinyin\": \"") else {
            continue;
        };
        let rest = &line[p + 11..];
        let Some(end) = rest.find('"') else { continue };
        let pinyin = rest[..end].to_string();
        let db_count = line
            .find("\"db_count\": ")
            .and_then(|i| {
                let r = &line[i + 12..];
                let e = r.find(|c: char| c == ',' || c == '}')?;
                r[..e].trim().parse::<usize>().ok()
            })
            .unwrap_or(0);
        out.push((pinyin, db_count));
    }
    out
}

fn source_name(s: Option<Source>) -> &'static str {
    match s {
        Some(Source::Completion) => "completion",
        Some(Source::Dictionary) => "dict",
        Some(Source::Phrase) => "phrase",
        Some(Source::Datetime) => "datetime",
        Some(Source::English) => "english",
        Some(Source::Emoji) => "emoji",
        Some(Source::Correction) => "correction",
        Some(Source::Combo) => "combo",
        Some(Source::Fuzzy) => "fuzzy",
        Some(Source::Mode) => "mode",
        Some(Source::Voice) => "voice",
        Some(Source::Sentence) => "sentence",
        None => "-",
        _ => "other",
    }
}

#[derive(Default)]
struct Stats {
    pinyins: usize,
    with_any_completion: usize,
    completion_total: usize,
    cand_total: usize,
    top5_blocked: usize,
    lead_completion: usize,
    /// 前 5 位里出现非单字（多字词或补全词）的拼音数——用户感知的「单字被压制」
    top5_non_single: usize,
    /// 候选充足（db_count ≥ 10）的拼音数
    rich: usize,
    /// 其中前 5 位含补全词的
    rich_top5_blocked: usize,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut page = 30usize;
    let mut baseline = default_baseline();
    let mut out_path: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--page" => {
                if let Some(v) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                    page = v;
                    i += 1;
                }
            }
            "--baseline" => {
                if let Some(v) = args.get(i + 1) {
                    baseline = PathBuf::from(v);
                    i += 1;
                }
            }
            "--out" => {
                if let Some(v) = args.get(i + 1) {
                    out_path = Some(PathBuf::from(v));
                    i += 1;
                }
            }
            other => eprintln!("未知参数: {other}"),
        }
        i += 1;
    }

    let pinyins = load_baseline(&baseline);
    if pinyins.is_empty() {
        eprintln!("基线读不到或为空: {}", baseline.display());
        std::process::exit(2);
    }
    let db_counts: BTreeMap<&str, usize> = pinyins.iter().map(|(p, c)| (p.as_str(), *c)).collect();

    let Some(dict) = find_dict() else {
        eprintln!("找不到 system_dict.db");
        std::process::exit(2);
    };
    taishen_engine::dictionary::init(Some(dict.as_path()));
    for _ in 0..600 {
        if taishen_engine::dictionary::is_ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    if !taishen_engine::dictionary::is_ready() {
        eprintln!("词库未就绪: {}", dict.display());
        std::process::exit(2);
    }

    let mut lines: Vec<String> = Vec::with_capacity(pinyins.len());
    let mut st = Stats::default();
    let mut lead_examples: Vec<String> = Vec::new();
    let mut blocked_examples: Vec<String> = Vec::new();

    for (pinyin, db_count) in &pinyins {
        let mut engine = Engine::new();
        engine.set_candidate_limit(page);
        for ch in pinyin.chars() {
            engine.process_key(ch);
        }
        let n = engine.candidate_count().min(page);
        let mut items: Vec<String> = Vec::with_capacity(n);
        let mut completions = 0usize;
        let mut top5_blocked = false;
        let mut lead_is_completion = false;
        let mut top5_non_single = false;
        for idx in 0..n {
            let word = engine.candidate(idx).unwrap_or("").to_string();
            if word.is_empty() {
                continue;
            }
            let src = engine.candidate_source(idx);
            let is_completion = src == Some(Source::Completion);
            let ty = if is_completion {
                "completion"
            } else if word.chars().count() == 1 {
                "single"
            } else {
                "word"
            };
            if is_completion {
                completions += 1;
                if idx < 5 {
                    top5_blocked = true;
                }
                if idx == 0 {
                    lead_is_completion = true;
                }
            }
            if idx < 5 && ty != "single" {
                top5_non_single = true;
            }
            items.push(format!(
                "{{\"rank\": {}, \"word\": \"{}\", \"type\": \"{}\", \"source\": \"{}\"}}",
                idx + 1,
                word,
                ty,
                source_name(src)
            ));
        }
        lines.push(format!(
            "{{\"pinyin\": \"{}\", \"syllables\": 1, \"db_count\": {}, \"shown\": {}, \"candidates\": [{}]}}",
            pinyin,
            db_count,
            items.len(),
            items.join(", ")
        ));

        st.pinyins += 1;
        st.cand_total += items.len();
        st.completion_total += completions;
        if completions > 0 {
            st.with_any_completion += 1;
        }
        if top5_non_single {
            st.top5_non_single += 1;
        }
        if top5_blocked {
            st.top5_blocked += 1;
            if blocked_examples.len() < 6 {
                let head: Vec<String> = items.iter().take(6).cloned().collect();
                blocked_examples.push(format!("  {pinyin}: {}", head.join(" ")));
            }
        }
        if lead_is_completion {
            st.lead_completion += 1;
            if lead_examples.len() < 6 {
                lead_examples.push(format!("  {pinyin}"));
            }
        }
        if *db_count >= 10 {
            st.rich += 1;
            if top5_blocked {
                st.rich_top5_blocked += 1;
            }
        }
    }

    if let Some(p) = &out_path {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, lines.join("\n"));
        println!("已写入: {}", p.display());
    }

    println!(
        "\n=== IME 侧体检（page={page}，{0} 个单音节拼音）===",
        st.pinyins
    );
    println!(
        "补全词总数 {}/{} ({:.1}%)",
        st.completion_total,
        st.cand_total,
        st.completion_total as f64 * 100.0 / st.cand_total.max(1) as f64
    );
    println!(
        "含补全词的拼音 {}/{} ({:.1}%)",
        st.with_any_completion,
        st.pinyins,
        st.with_any_completion as f64 * 100.0 / st.pinyins.max(1) as f64
    );
    println!(
        "前 5 位被补全词占位 {}/{} ({:.1}%)",
        st.top5_blocked,
        st.pinyins,
        st.top5_blocked as f64 * 100.0 / st.pinyins.max(1) as f64
    );
    println!(
        "候选充足（db≥10）里前 5 被占 {}/{} ({:.1}%)",
        st.rich_top5_blocked,
        st.rich,
        st.rich_top5_blocked as f64 * 100.0 / st.rich.max(1) as f64
    );
    println!("首位是补全词: {} 个", st.lead_completion);
    println!(
        "前 5 位含非单字 {}/{} ({:.1}%)",
        st.top5_non_single,
        st.pinyins,
        st.top5_non_single as f64 * 100.0 / st.pinyins.max(1) as f64
    );
    if !lead_examples.is_empty() {
        println!("  样例: {}", lead_examples.join(" "));
    }
    if !blocked_examples.is_empty() {
        println!("前 5 被占样例:");
        for line in &blocked_examples {
            println!("{line}");
        }
    }
}
