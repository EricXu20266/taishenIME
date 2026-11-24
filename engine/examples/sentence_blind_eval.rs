//! Q1.2 500 句整句盲测集独立只读评测器。
//!
//! 用法：
//!   cargo run --release --example sentence_blind_eval --manifest-path engine/Cargo.toml
//!   sentence_blind_eval.exe [--file <path>] [--category <name>]
//!
//! 统计：
//!   1. `Engine` 端到端逐键输入后的 Top-1 / Top-3 / Top-5 命中率与单句平均耗时；
//!   2. `sentence::decode(py, None, 5)` 直接整句解码的 Top-1 / Top-3 / Top-5 命中率；
//!   3. 按分类、全拼小计（450）、简拼/混拼小计（50）与总计（500）汇总输出，退出码为 0。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;
use taishen_engine::Engine;

struct BlindCase {
    id: String,
    category: String,
    pinyin: String,
    expected: String,
}

#[derive(Default, Clone, Copy)]
struct Stats {
    total: usize,
    eng_top1: usize,
    eng_top3: usize,
    eng_top5: usize,
    dec_top1: usize,
    dec_top3: usize,
    dec_top5: usize,
    elapsed_us: u128,
}

impl Stats {
    fn add_assign(&mut self, other: Stats) {
        self.total += other.total;
        self.eng_top1 += other.eng_top1;
        self.eng_top3 += other.eng_top3;
        self.eng_top5 += other.eng_top5;
        self.dec_top1 += other.dec_top1;
        self.dec_top3 += other.dec_top3;
        self.dec_top5 += other.dec_top5;
        self.elapsed_us += other.elapsed_us;
    }

    fn pct(n: usize, d: usize) -> f64 {
        if d == 0 {
            0.0
        } else {
            (n as f64) * 100.0 / (d as f64)
        }
    }

    fn print_row(&self, label: &str) {
        let avg_ms = if self.total == 0 {
            0.0
        } else {
            (self.elapsed_us as f64) / 1000.0 / (self.total as f64)
        };
        println!(
            "{label:<16} | {:>4} | {:>3}/{:<3} ({:>5.1}%) | {:>3}/{:<3} ({:>5.1}%) | {:>3}/{:<3} ({:>5.1}%) | {:>3}/{:<3} ({:>5.1}%) | {:>6.2}ms",
            self.total,
            self.eng_top1,
            self.total,
            Self::pct(self.eng_top1, self.total),
            self.eng_top3,
            self.total,
            Self::pct(self.eng_top3, self.total),
            self.eng_top5,
            self.total,
            Self::pct(self.eng_top5, self.total),
            self.dec_top1,
            self.total,
            Self::pct(self.dec_top1, self.total),
            avg_ms,
        );
    }
}

fn default_blind_file() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/sentence-blind-500.tsv")
}

fn load_cases(path: &Path, filter_cat: Option<&str>) -> Result<Vec<BlindCase>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("读取盲测集失败 {}: {e}", path.display()))?;
    let mut out = Vec::new();
    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split('\t');
        let (Some(id), Some(cat), Some(py), Some(exp)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(format!(
                "格式错误 {}:{}: {line}",
                path.display(),
                lineno + 1
            ));
        };
        if let Some(target) = filter_cat
            && cat != target
        {
            continue;
        }
        out.push(BlindCase {
            id: id.to_string(),
            category: cat.to_string(),
            pinyin: py.to_string(),
            expected: exp.to_string(),
        });
    }
    Ok(out)
}

fn hit_rank<'a, I>(mut iter: I, expected: &str) -> Option<usize>
where
    I: Iterator<Item = &'a str>,
{
    iter.position(|cand| cand == expected)
}

fn main() {
    let mut file_path = default_blind_file();
    let mut filter_cat: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--file" => {
                let val = args.next().unwrap_or_else(|| {
                    eprintln!("--file 缺少路径参数");
                    std::process::exit(2);
                });
                file_path = PathBuf::from(val);
            }
            "--category" => {
                let val = args.next().unwrap_or_else(|| {
                    eprintln!("--category 缺少分类名称");
                    std::process::exit(2);
                });
                filter_cat = Some(val);
            }
            other => {
                eprintln!("未知参数: {other}");
                std::process::exit(2);
            }
        }
    }

    let cases = match load_cases(&file_path, filter_cat.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

    let dict = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("resources/system_dict.db");
    taishen_engine::dictionary::init_blocking(Some(dict.as_path()));
    let bigram = dict.with_file_name("bigram.db");
    if bigram.exists() {
        let t = Instant::now();
        let pairs = taishen_engine::dictionary::set_bigram(&bigram, 1, usize::MAX);
        eprintln!(
            "bigram: {pairs} 对, {}ms | mode={} | cases={}",
            t.elapsed().as_millis(),
            if std::env::var_os("TS_SENTENCE_LEGACY").is_some() {
                "LEGACY"
            } else {
                "ACTIVE"
            },
            cases.len()
        );
    }

    let mut engine = Engine::new();
    let mut by_cat: BTreeMap<String, Stats> = BTreeMap::new();
    let mut full_sub = Stats::default();
    let mut abbrev_sub = Stats::default();
    let mut pure_init_sub = Stats::default();
    let mut mixed_sub = Stats::default();
    let mut total = Stats::default();

    for c in &cases {
        engine.reset();
        let t0 = Instant::now();
        for ch in c.pinyin.chars() {
            engine.process_key(ch);
        }
        let elapsed_us = t0.elapsed().as_micros();

        let eng_count = engine.candidate_count().min(5);
        let eng_rank = hit_rank(
            (0..eng_count).filter_map(|i| engine.candidate(i)),
            &c.expected,
        );
        let decoded = taishen_engine::sentence::decode(&c.pinyin, None, 5);
        let dec_rank = hit_rank(decoded.iter().map(String::as_str), &c.expected);

        let one = Stats {
            total: 1,
            eng_top1: usize::from(eng_rank == Some(0)),
            eng_top3: usize::from(eng_rank.is_some_and(|r| r < 3)),
            eng_top5: usize::from(eng_rank.is_some_and(|r| r < 5)),
            dec_top1: usize::from(dec_rank == Some(0)),
            dec_top3: usize::from(dec_rank.is_some_and(|r| r < 3)),
            dec_top5: usize::from(dec_rank.is_some_and(|r| r < 5)),
            elapsed_us,
        };

        by_cat
            .entry(c.category.clone())
            .or_default()
            .add_assign(one);
        if c.category == "abbrev_mixed" {
            abbrev_sub.add_assign(one);
            let is_pure = !c
                .pinyin
                .bytes()
                .any(|b| matches!(b, b'a' | b'e' | b'i' | b'o' | b'u' | b'v'));
            if is_pure {
                pure_init_sub.add_assign(one);
            } else {
                mixed_sub.add_assign(one);
            }
        } else {
            full_sub.add_assign(one);
        }
        total.add_assign(one);

        let _ = &c.id;
    }

    println!(
        "{:<16} | {:>4} | {:^15} | {:^15} | {:^15} | {:^15} | {:>8}",
        "Category", "N", "Engine Top-1", "Engine Top-3", "Engine Top-5", "Decode Top-1", "AvgTime"
    );
    println!("{}", "-".repeat(106));
    for (cat, st) in &by_cat {
        st.print_row(cat);
    }
    println!("{}", "-".repeat(106));
    full_sub.print_row("full_pinyin");
    abbrev_sub.print_row("abbrev_mixed");
    mixed_sub.print_row("  abbrev:mixed");
    pure_init_sub.print_row("  abbrev:pure");
    println!("{}", "=".repeat(106));
    total.print_row("TOTAL");
}
