//! 渐进式输入命中测试（probe_prefix）
//!
//! 目的：真机反馈不是必需的——用真实输入留痕就能给排序质量打分。
//!
//! 评价的不是「全拼打完首位对不对」，而是「**用户打到一半时，想要的词出现了没有**」。
//! 依据是实际输入习惯：先打一段拼音看候选，候选里出现想要的字就不再补全。
//!
//! 用法:
//!   cargo run --release --example probe_prefix -- ..\tmp\prefix_cases.tsv
//!
//! TSV 格式：`word \t user_pinyin \t frequency`（由 user_dict 导出）
//!
//! 输出两类指标：
//!   1. 真实拼音命中：直接输入用户当时打的串，看目标词位次
//!   2. 渐进命中：输入全拼的每个前缀，找「首次进入前 N 位」的最短前缀比例
//!
//! 本探针**不加载用户词库**（不调 set_user_dict_path），测的是
//! 「系统词库 + 常用词表 + 搭配表」的冷启动表现——排除了个人学习对结果的掩盖。

use std::path::Path;
use taishen_engine::Engine;

/// 打满拼音时目标词应进入的位次（首位）
const TOP: usize = 0;
/// 「首屏可见」的位次上限（与 page_size 默认 5 一致）
const SCREEN: usize = 5;
/// 「翻一页能看到」的上限
const PAGE: usize = 9;

struct Case {
    word: String,
    user_pinyin: String,
    freq: u32,
}

fn read_cases(path: &Path) -> Vec<Case> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 2 {
            continue;
        }
        out.push(Case {
            word: f[0].to_string(),
            user_pinyin: f[1].to_string(),
            freq: f.get(2).and_then(|s| s.parse().ok()).unwrap_or(0),
        });
    }
    out
}

/// 输入一个拼音串，返回候选列表
fn candidates(pinyin: &str) -> Vec<String> {
    let mut e = Engine::new();
    for ch in pinyin.chars() {
        e.process_key(ch);
    }
    (0..e.candidate_count())
        .filter_map(|i| e.candidate(i).map(|s| s.to_string()))
        .collect()
}

fn position(cands: &[String], word: &str) -> Option<usize> {
    cands.iter().position(|w| w == word)
}

#[derive(Default)]
struct Stats {
    total: usize,
    top: usize,
    screen: usize,
    page: usize,
    miss: usize,
}

impl Stats {
    fn record(&mut self, pos: Option<usize>) {
        self.total += 1;
        match pos {
            Some(0) => self.top += 1,
            Some(p) if p < SCREEN => self.screen += 1,
            Some(p) if p < PAGE => self.page += 1,
            _ => self.miss += 1,
        }
    }
    fn report(&self, label: &str) {
        if self.total == 0 {
            return;
        }
        let pct = |n: usize| 100.0 * n as f64 / self.total as f64;
        println!(
            "{label}: 共 {} | 首位 {:.1}% | 首屏 {:.1}% | 一页 {:.1}% | 未命中 {:.1}%",
            self.total,
            pct(self.top),
            pct(self.top + self.screen),
            pct(self.top + self.screen + self.page),
            pct(self.miss)
        );
    }
}

/// 按音节切分（用引擎自己的切分器，保证与实际输入一致）
fn syllables(full: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = full;
    while !rest.is_empty() {
        match taishen_engine::pinyin::split_first_syllable(rest) {
            Some((s, next)) => {
                out.push(s.to_string());
                rest = next;
            }
            None => {
                out.push(rest.to_string());
                break;
            }
        }
    }
    out
}

fn main() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let dict = base.join("resources/system_dict.db");
    taishen_engine::dictionary::init(Some(dict.as_path()));
    for _ in 0..600 {
        if taishen_engine::dictionary::is_ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    let path = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| base.join("tmp/prefix_cases.tsv"));
    let cases = read_cases(&path);
    if cases.is_empty() {
        println!("测试卷为空：{}", path.display());
        return;
    }
    println!("测试卷: {} 条（{}）\n", cases.len(), path.display());

    // ── 指标 1：真实拼音命中（用户当时打到一半的串） ──
    let mut real = Stats::default();
    // 按输入形态分组：用户打的串 == 规范全拼 → 全拼组；否则是简拼/混杂组。
    // 分开统计的原因：简拼/混杂串在冷启动（无用户学习）下命中率天然低，
    // 混在一起会把「需要学习」误报成「引擎排序错误」。
    let mut full_group = Stats::default();
    let mut mixed_group = Stats::default();
    // ── 指标 2：按字数分组 ──
    let mut by_len: std::collections::BTreeMap<usize, Stats> = Default::default();
    // ── 指标 3：按音节粒度的命中曲线（在下方单独循环） ──
    let mut misses: Vec<(String, String, Option<usize>)> = Vec::new();
    let mut mixed_misses = 0usize;

    for c in &cases {
        let full = taishen_engine::dictionary::canonical_pinyin_for_word(&c.word)
            .unwrap_or_else(|| c.user_pinyin.clone());
        let is_full = full == c.user_pinyin;

        let cands = candidates(&c.user_pinyin);
        let pos = position(&cands, &c.word);
        real.record(pos);
        if is_full {
            full_group.record(pos);
        } else {
            mixed_group.record(pos);
            if pos.is_none() || pos.map(|p| p >= SCREEN).unwrap_or(false) {
                mixed_misses += 1;
            }
        }
        by_len
            .entry(c.word.chars().count())
            .or_default()
            .record(pos);
        if pos.is_none() || pos.unwrap() >= SCREEN {
            misses.push((c.word.clone(), c.user_pinyin.clone(), pos));
        }
    }

    println!("══ 指标 1：真实拼音命中（用户当时的输入串）══");
    real.report("总体");
    println!("\n  ── 按输入形态分组 ──");
    full_group.report("全拼输入");
    mixed_group.report("简拼/混杂输入");
    if mixed_group.total > 0 {
        println!(
            "    （简拼/混杂组中 {} 条未达首屏：这类串依赖个人学习，冷启动下未命中属预期，不计入引擎缺陷）",
            mixed_misses
        );
    }

    println!("\n══ 指标 2：按目标词字数 ══");
    for (len, s) in &by_len {
        s.report(&format!("{len} 字词"));
    }

    // ── 指标 3：按音节粒度的命中曲线 ──
    //
    // 字符级前缀对 2 字词没有中间态（`lian` 出不了「联系」，这不算缺陷）。
    // 用户的实际行为是「打完一个音节看候选」：打 `lian` 看到「联/连」，
    // 不选就继续打 `xi`。所以按音节数统计才有意义。
    //
    // 对 n 个音节的词，测打完前 k 个音节（k=1..n）时的命中位次，
    // 输出「打满 / 差一个音节 / 只打第一个音节」三个点的首屏率。
    println!("\n══ 指标 3：按音节粒度的命中曲线 ══");
    let mut at_full = Stats::default(); // 打满全部音节
    let mut at_n1 = Stats::default(); // 只差最后一个音节
    let mut at_first = Stats::default(); // 只打第一个音节
    let mut by_syl_count: std::collections::BTreeMap<usize, (usize, usize)> = Default::default();

    for c in &cases {
        let full = taishen_engine::dictionary::canonical_pinyin_for_word(&c.word)
            .unwrap_or_else(|| c.user_pinyin.clone());
        let syls = syllables(&full);
        let n = syls.len();
        if n == 0 {
            continue;
        }
        let hit_at = |k: usize| -> bool {
            let prefix: String = syls[..k].concat();
            let cands = candidates(&prefix);
            position(&cands, &c.word)
                .map(|p| p < SCREEN)
                .unwrap_or(false)
        };
        at_full.record(if hit_at(n) { Some(0) } else { None });
        let entry = by_syl_count.entry(n).or_insert((0, 0));
        entry.1 += 1;
        if hit_at(n) {
            entry.0 += 1;
        }
        if n >= 2 {
            at_n1.record(if hit_at(n - 1) { Some(0) } else { None });
            at_first.record(if hit_at(1) { Some(0) } else { None });
        }
    }

    println!("  打满全部音节  : 前 {SCREEN} 命中率 {:.1}%（{} 例）", {
        let pct = |s: &Stats| 100.0 * (s.top + s.screen) as f64 / s.total.max(1) as f64;
        pct(&at_full)
    }, at_full.total);
    if at_n1.total > 0 {
        let pct = |s: &Stats| 100.0 * (s.top + s.screen) as f64 / s.total.max(1) as f64;
        println!("  只差一个音节  : 前 {SCREEN} 命中率 {:.1}%（{} 例）", pct(&at_n1), at_n1.total);
        println!("  只打第一个音节: 前 {SCREEN} 命中率 {:.1}%（{} 例）", pct(&at_first), at_first.total);
    }
    println!("  ── 按音节数（打满时的命中率）──");
    for (n, (hit, total)) in &by_syl_count {
        println!(
            "    {n} 音节: {}/{} ({:.1}%)",
            hit,
            total,
            100.0 * *hit as f64 / (*total).max(1) as f64
        );
    }

    println!("\n══ 未达首屏的样本（前 25 条）══");
    for (w, py, pos) in misses.iter().take(25) {
        match pos {
            Some(p) => println!("  {py:<14} {w:<10} 第 {} 位", p + 1),
            None => println!("  {py:<14} {w:<10} 不在候选"),
        }
    }
    if misses.len() > 25 {
        println!("  ... 另有 {} 条", misses.len() - 25);
    }
}
