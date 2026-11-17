//! 输入法考试工具：用拼音出候选，逐条断言期望词落在第几位。
//!
//! 用途：每次引擎/词库改动后跑一遍，验证「常用输入能不能打出该打的词」。
//! 人工在宿主里敲键盘验证的东西，这里全部用引擎 API 复现。
//!
//! 用法：
//!   cargo run --release --example exam
//!   cargo run --release --example exam -- --cases engine/tests/exam-cases.tsv
//!   cargo run --release --example exam -- --dict <词库绝对路径> --top 9
//!
//! 退出码：0 全部通过；1 有 FAIL；2 环境问题（词库缺失/未就绪/用例文件读不到）。
//!
//! 用例文件格式（TSV，`#` 开头为注释，`[xxx]` 为分组）：
//!   拼音<TAB>期望词<TAB>允许的最大排名<TAB>备注
//! 排名从 1 开始；写 1 表示必须首位。第 3 列省略时默认 1。

use std::path::{Path, PathBuf};
use taishen_engine::Engine;

struct Case {
    section: String,
    input: String,
    expect: String,
    max_rank: usize,
    note: String,
}

struct Outcome {
    section: String,
    input: String,
    expect: String,
    max_rank: usize,
    note: String,
    actual_rank: Option<usize>,
    top: Vec<String>,
    ok: bool,
}

fn find_dict() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TAISHEN_DICT") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    for cand in ["resources/system_dict.db", "../resources/system_dict.db"] {
        let pb = PathBuf::from(cand);
        if pb.is_file() {
            return Some(pb);
        }
    }
    None
}

fn default_cases_path() -> PathBuf {
    for cand in ["engine/tests/exam-cases.tsv", "tests/exam-cases.tsv"] {
        let pb = PathBuf::from(cand);
        if pb.is_file() {
            return pb;
        }
    }
    PathBuf::from("engine/tests/exam-cases.tsv")
}

fn load_cases(path: &Path) -> Result<Vec<Case>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("读不到用例文件 {}: {e}", path.display()))?;
    let mut out = Vec::new();
    let mut section = String::from("未分组");
    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed[1..trimmed.len() - 1].to_string();
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 2 {
            return Err(format!(
                "第 {} 行格式不对（需要 拼音<TAB>期望词）",
                lineno + 1
            ));
        }
        let max_rank = cols
            .get(2)
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(1);
        out.push(Case {
            section: section.clone(),
            input: cols[0].trim().to_string(),
            expect: cols[1].trim().to_string(),
            max_rank,
            note: cols
                .get(3)
                .map(|s| s.trim().to_string())
                .unwrap_or_default(),
        });
    }
    Ok(out)
}

fn run_case(case: &Case, top_n: usize) -> Outcome {
    let mut engine = Engine::new();
    // `candidate(i)` 是「当前页内索引」——不放开 page_size 时只能取到 5 条，
    // `--top 9` 会静默退化成 5 条，断言口径与声明不一致。
    engine.set_candidate_limit(top_n);
    // 预热：排除首查建页开销（与真实输入路径一致）
    for ch in "a".chars() {
        engine.process_key(ch);
    }
    engine.reset();
    for ch in case.input.chars() {
        engine.process_key(ch);
    }
    let top: Vec<String> = (0..top_n)
        .filter_map(|i| engine.candidate(i).map(|s| s.to_string()))
        .collect();
    let actual_rank = top.iter().position(|w| w == &case.expect).map(|p| p + 1);
    let ok = actual_rank.is_some_and(|r| r <= case.max_rank);
    Outcome {
        section: case.section.clone(),
        input: case.input.clone(),
        expect: case.expect.clone(),
        max_rank: case.max_rank,
        note: case.note.clone(),
        actual_rank,
        top,
        ok,
    }
}

fn main() {
    let mut cases_path = default_cases_path();
    let mut top_n = 9usize;
    let mut dict_override: Option<PathBuf> = None;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--cases" | "-c" => {
                if let Some(v) = args.get(i + 1) {
                    cases_path = PathBuf::from(v);
                    i += 1;
                }
            }
            "--dict" | "-d" => {
                if let Some(v) = args.get(i + 1) {
                    dict_override = Some(PathBuf::from(v));
                    i += 1;
                }
            }
            "--top" | "-t" => {
                if let Some(v) = args.get(i + 1).and_then(|s| s.parse::<usize>().ok()) {
                    top_n = v.max(1);
                    i += 1;
                }
            }
            "--help" | "-h" => {
                println!("用法: exam [--cases <tsv>] [--dict <db>] [--top <n>]");
                return;
            }
            other => {
                eprintln!("未知参数: {other}");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let dict = match dict_override.or_else(find_dict) {
        Some(p) => p,
        None => {
            eprintln!("找不到词库：用 --dict 指定，或设 TAISHEN_DICT");
            std::process::exit(2);
        }
    };
    let cases = match load_cases(&cases_path) {
        Ok(c) if !c.is_empty() => c,
        Ok(_) => {
            eprintln!("用例文件为空: {}", cases_path.display());
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

    taishen_engine::dictionary::init(Some(dict.as_path()));
    // 等待异步加载（上限 60 秒）
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

    println!("词库: {}", dict.display());
    println!(
        "用例: {}（{} 条，取前 {} 个候选）",
        cases_path.display(),
        cases.len(),
        top_n
    );
    println!();
    println!(
        "{:<10} {:<22} {:<10} {:>4} {:>4}  {}",
        "结果", "输入", "期望", "限位", "实际", "首候选"
    );

    let mut outcomes = Vec::with_capacity(cases.len());
    for case in &cases {
        let o = run_case(case, top_n);
        let mark = if o.ok { "PASS" } else { "FAIL" };
        let actual = match o.actual_rank {
            Some(r) => format!("#{r}"),
            None => format!("未进前{}", top_n),
        };
        let head = o.top.first().cloned().unwrap_or_else(|| "-".to_string());
        println!(
            "{:<10} {:<22} {:<10} {:>4} {:>4}  {}",
            mark, o.input, o.expect, o.max_rank, actual, head
        );
        outcomes.push(o);
    }

    // 分组统计
    println!();
    let mut sections: Vec<String> = Vec::new();
    for o in &outcomes {
        if !sections.contains(&o.section) {
            sections.push(o.section.clone());
        }
    }
    println!("{:<20} {:>5} {:>5} {:>8}", "分组", "通过", "总数", "通过率");
    for sec in &sections {
        let group: Vec<&Outcome> = outcomes.iter().filter(|o| &o.section == sec).collect();
        let pass = group.iter().filter(|o| o.ok).count();
        println!(
            "{:<20} {:>5} {:>5} {:>7.1}%",
            sec,
            pass,
            group.len(),
            pass as f64 * 100.0 / group.len() as f64
        );
    }

    let failed: Vec<&Outcome> = outcomes.iter().filter(|o| !o.ok).collect();
    let passed = outcomes.len() - failed.len();
    println!();
    println!(
        "总计 {}/{} 通过（{:.1}%）",
        passed,
        outcomes.len(),
        passed as f64 * 100.0 / outcomes.len() as f64
    );

    if !failed.is_empty() {
        println!();
        println!("失败明细：");
        for o in &failed {
            let hint = if o.note.is_empty() {
                String::new()
            } else {
                format!(" [{}]", o.note)
            };
            println!(
                "  {} → 期望「{}」在 #{} 内，实际 {}；前 {} 候选: {}{}",
                o.input,
                o.expect,
                o.max_rank,
                match o.actual_rank {
                    Some(r) => format!("#{r}"),
                    None => "未命中".to_string(),
                },
                top_n,
                o.top.join(" / "),
                hint
            );
        }
        std::process::exit(1);
    }
}
