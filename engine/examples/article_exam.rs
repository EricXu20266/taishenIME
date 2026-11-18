//! 文章模式考试工具：按真实文章逐词敲拼音、逐词上屏，量化候选排名分布。
//!
//! 与 exam.rs 的分工：
//!   exam.rs        —— 人造用例（单个输入串 + 断言），跑通过率修 bug
//!   article_exam   —— 真实文章（连续输入 + 上屏），测分布与连续输入退化
//!
//! 关键在「逐词上屏」：`select_candidate` 会把选中词写进 `last_committed`
//! 并调用 `learn_context_pair`，随后 `reset()` 只清缓冲不清 `last_committed`——
//! 于是下一个词的候选真实受前一个词影响，这是人造用例做不到的。
//!
//! 用法：
//!   cargo run --release --example article_exam -- --article <article.json>
//!   cargo run --release --example article_exam -- --article a.json --out r.jsonl --limit 20
//!   cargo run --release --example article_exam -- --article a.json --no-context   # 关上下文联想做对照
//!   cargo run --release --example article_exam -- --article a.json --dict <词库绝对路径>
//!
//! 退出码：0 跑完（有 miss 也算跑完，本工具是测量不是断言）；2 参数/环境问题。

use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use taishen_engine::Engine;

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;
const TOP_SHOW: usize = 3;

// ───────────────────────── 输入模型 ─────────────────────────

#[derive(Deserialize)]
struct ArticleFile {
    source: SourceInfo,
    segments: Vec<Segment>,
}

#[derive(Deserialize, Default)]
struct SourceInfo {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    fetched_at: String,
}

#[derive(Deserialize)]
struct Segment {
    index: u32,
    mode: String,
    #[serde(default)]
    #[allow(dead_code)]
    text: String,
    words: Vec<Word>,
}

#[derive(Deserialize)]
struct Word {
    text: String,
    pinyin: String,
}

// ───────────────────────── 输出模型 ─────────────────────────

/// result.jsonl 的一行
#[derive(Serialize)]
struct RowOut {
    seg: u32,
    mode: String,
    word: String,
    pinyin: String,
    rank: Option<usize>,
    status: &'static str,
    top3: Vec<String>,
    src3: Vec<String>,
}

#[derive(Default, Clone, Copy)]
struct Counts {
    ok: usize,
    late: usize,
    page: usize,
    miss: usize,
}

impl Counts {
    fn add(&mut self, status: &str) {
        match status {
            "ok" => self.ok += 1,
            "late" => self.late += 1,
            "page" => self.page += 1,
            _ => self.miss += 1,
        }
    }

    fn total(&self) -> usize {
        self.ok + self.late + self.page + self.miss
    }

    fn merge(&mut self, other: &Counts) {
        self.ok += other.ok;
        self.late += other.late;
        self.page += other.page;
        self.miss += other.miss;
    }

    fn line(&self) -> String {
        format!(
            "ok {} / late {} / page {} / miss {}",
            self.ok, self.late, self.page, self.miss
        )
    }
}

/// 一条需要进「问题明细」的记录
struct Issue {
    status: &'static str,
    word: String,
    pinyin: String,
    rank: Option<usize>,
    preview: String,
}

// ───────────────────────── 工具函数 ─────────────────────────

/// 候选排名 → 分级。rank 从 1 开始。
fn status_of(rank: Option<usize>) -> &'static str {
    match rank {
        Some(1..=3) => "ok",
        Some(4..=9) => "late",
        Some(_) => "page",
        None => "miss",
    }
}

/// 规范拼音：只保留 ASCII 字母并转小写。
/// 真人输入不会有分隔符；LLM 手写 JSON 里出现空格/撇号时静默清掉。
fn normalize_pinyin(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphabetic())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// 终端显示宽度：ASCII 记 1，其余（中文）记 2，用于明细对齐。
fn display_width(s: &str) -> usize {
    s.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum()
}

fn pad(s: &str, width: usize) -> String {
    let w = display_width(s);
    if w >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - w))
    }
}

fn find_dict() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TAISHEN_DICT") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    let mut cands = vec![
        PathBuf::from("resources/system_dict.db"),
        PathBuf::from("../resources/system_dict.db"),
    ];
    // 从任意 cwd 启动时用 crate 位置兜底（engine/ 的上一级即项目根）
    if let Some(root) = Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
        cands.push(root.join("resources/system_dict.db"));
    }
    cands.into_iter().find(|p| p.is_file())
}

fn usage() {
    eprintln!(
        "用法: cargo run --release --example article_exam -- --article <article.json> [选项]\n\
         \n\
         选项:\n\
         \x20 --out <path>     结果 JSONL 路径（默认：article 同目录 result.jsonl）\n\
         \x20 --dict <path>    指定词库（默认自动探测 resources/system_dict.db）\n\
         \x20 --limit <n>      每词取前 n 个候选（默认 {DEFAULT_LIMIT}，上限 {MAX_LIMIT}）\n\
         \x20 --no-context     关闭上下文联想（做「上屏影响下一词」的对照组）\n\
         \x20 --miss-fill      miss 时上屏首个候选（模拟用户将就选一个，保持上下文连贯）"
    );
}

// ───────────────────────── 主流程 ─────────────────────────

fn main() {
    let mut article_path: Option<PathBuf> = None;
    let mut out_path: Option<PathBuf> = None;
    let mut dict_override: Option<PathBuf> = None;
    let mut limit = DEFAULT_LIMIT;
    let mut context = true;
    let mut miss_fill = false;
    let mut isolate = true;
    let mut user_dict: Option<PathBuf> = None;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--article" => {
                i += 1;
                article_path = args.get(i).map(PathBuf::from);
            }
            "--out" => {
                i += 1;
                out_path = args.get(i).map(PathBuf::from);
            }
            "--dict" => {
                i += 1;
                dict_override = args.get(i).map(PathBuf::from);
            }
            "--limit" => {
                i += 1;
                limit = args
                    .get(i)
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(DEFAULT_LIMIT);
            }
            "--no-context" => context = false,
            "--miss-fill" => miss_fill = true,
            "--no-isolate" => isolate = false,
            "--user-dict" => {
                i += 1;
                user_dict = args.get(i).map(PathBuf::from);
            }
            "-h" | "--help" => {
                usage();
                return;
            }
            other => {
                eprintln!("未知参数: {other}");
                usage();
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let article_path = match article_path {
        Some(p) => p,
        None => {
            eprintln!("缺少 --article <article.json>");
            usage();
            std::process::exit(2);
        }
    };
    limit = limit.clamp(1, MAX_LIMIT);

    // ── 读文章（容忍 UTF-8 BOM：Windows 记事本写出的文件常带） ──
    let raw = match std::fs::read_to_string(&article_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("读取失败 {}: {e}", article_path.display());
            std::process::exit(2);
        }
    };
    let article: ArticleFile = match serde_json::from_str(raw.trim_start_matches('\u{feff}')) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("解析 JSON 失败 {}: {e}", article_path.display());
            std::process::exit(2);
        }
    };

    // ── 词库加载（与 exam.rs 同路径：异步 init + 轮询就绪） ──
    let dict = match dict_override.or_else(find_dict) {
        Some(p) => p,
        None => {
            eprintln!("找不到词库：用 --dict 指定，或设 TAISHEN_DICT");
            std::process::exit(2);
        }
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
    // 领域词库与系统词库并行加载，就绪标志只等系统词库；
    // 留一点窗口让 domains 合并完，避免同一篇文章两次跑出不同排名。
    std::thread::sleep(std::time::Duration::from_millis(500));

    // ── 搭配表（上下文联想的真实数据源）──
    // 生产端在 ActivateEx 里装 bigram.db；不装就只有内置几十条搭配表，
    // 上下文联想几乎不生效（实测「政治→人物」位次不变）。
    let bigram_path = dict
        .parent()
        .map(|d| d.join("bigram.db"))
        .unwrap_or_else(|| PathBuf::from("bigram.db"));
    let table = taishen_engine::dictionary::load_bigram(&bigram_path, 2, 64);
    let prev_count = table.len();
    let pair_count: usize = table.values().map(|v| v.len()).sum();
    taishen_engine::dictionary::install_bigram(table);
    println!(
        "搭配表: {} （{} 前词 / {} 对，min_count=2）",
        bigram_path.display(),
        prev_count,
        pair_count
    );

    // ── 用户词库隔离 ──
    // select_candidate 内部会 learn(拼音, 词)。默认落 %APPDATA%/taishen-ime/user_dict.db，
    // 带来两个问题：
    //   ① 污染本机真实用户词库；
    //   ② 前一段全拼把「产业链」学成用户词后，后一段简拼 cyl 会被 is_user_word 放行，
    //      MISS 就复现不出来了（实测：一段 chanyelian 足以让第二段的 cyl 拿到 Combo 候选）。
    // 默认把用户词库指到 tmp 下的独立文件，并在每段开始前清内存用户词。
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    if isolate {
        let user_db =
            user_dict.unwrap_or_else(|| project_root.join("tmp").join("article_exam_user_dict.db"));
        if let Some(dir) = user_db.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // 每次全新开始：残留的旧学习记录会让首发词就带用户词
        let _ = std::fs::remove_file(&user_db);
        taishen_engine::dictionary::set_user_dict_path(Some(user_db.as_path()));
        println!(
            "用户词库: {} （隔离，段间清空内存用户词）",
            user_db.display()
        );
    }

    // ── 输出文件 ──
    let out_path = out_path.unwrap_or_else(|| {
        article_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("result.jsonl")
    });
    let out_file = match File::create(&out_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("创建输出失败 {}: {e}", out_path.display());
            std::process::exit(2);
        }
    };
    let mut writer = BufWriter::new(out_file);

    // ── 引擎 ──
    let mut engine = Engine::new();
    engine.set_candidate_limit(limit);
    if !context {
        engine.set_context_enabled(false);
    }
    // 触发一次查询，确保索引的惰性初始化发生在这里（与 exam --dump 同法），
    // 否则第一个词会承担初始化开销、甚至拿到内置兜底词库的结果。
    engine.process_key('a');
    engine.reset();

    println!("词库: {}", dict.display());
    println!("文章: {}", article_path.display());
    println!("结果: {}", out_path.display());
    println!(
        "配置: 每词取前 {} 个候选 / 上下文联想 {} / miss-fill {} / 段间隔离 {}",
        limit,
        if context { "开" } else { "关" },
        if miss_fill { "开" } else { "关" },
        if isolate { "开" } else { "关" }
    );
    println!();
    println!(
        "【来源】{} / {} / {}",
        article.source.title, article.source.url, article.source.fetched_at
    );

    let mut total = Counts::default();
    let mut by_mode: Vec<(String, Counts)> = Vec::new();
    let mut issues: Vec<Issue> = Vec::new();

    for seg in &article.segments {
        // 段间清空学习痕迹：只测「这一段拼音本身行不行」，不测「上一段教会不会它」
        if isolate {
            taishen_engine::dictionary::clear_user_words();
        }
        let mut counts = Counts::default();

        for w in &seg.words {
            let py = normalize_pinyin(&w.pinyin);
            if py.is_empty() {
                eprintln!("跳过空拼音：段{} 词「{}」", seg.index, w.text);
                continue;
            }

            // 清上一词缓冲，不清 last_committed（上一个上屏词仍参与上下文打分）
            engine.reset();
            for ch in py.chars() {
                engine.process_key(ch);
            }

            let n = engine.candidate_count().min(limit);
            let mut rank: Option<usize> = None;
            for i in 0..n {
                if engine.candidate(i) == Some(w.text.as_str()) {
                    rank = Some(i + 1);
                    break;
                }
            }

            let shown = n.min(TOP_SHOW);
            let top: Vec<String> = (0..shown)
                .filter_map(|i| engine.candidate(i).map(str::to_string))
                .collect();
            let src: Vec<String> = (0..shown)
                .map(|i| engine.candidate_source_name(i).unwrap_or("-").to_string())
                .collect();

            let status = status_of(rank);
            counts.add(status);
            total.add(status);

            if status != "ok" {
                let preview = if top.is_empty() {
                    "(无候选)".to_string()
                } else {
                    top.iter()
                        .zip(src.iter())
                        .map(|(t, s)| format!("{t}({s})"))
                        .collect::<Vec<_>>()
                        .join(" / ")
                };
                issues.push(Issue {
                    status,
                    word: w.text.clone(),
                    pinyin: py.clone(),
                    rank,
                    preview,
                });
            }

            let row = RowOut {
                seg: seg.index,
                mode: seg.mode.clone(),
                word: w.text.clone(),
                pinyin: py.clone(),
                rank,
                status,
                top3: top,
                src3: src,
            };
            match serde_json::to_string(&row) {
                Ok(line) => {
                    if let Err(e) = writeln!(writer, "{line}") {
                        eprintln!("写结果失败: {e}");
                        std::process::exit(2);
                    }
                }
                Err(e) => eprintln!("序列化失败: {e}"),
            }

            // 上屏：设 last_committed + learn_context_pair，reset 后仍保留
            match rank {
                Some(r) => {
                    engine.select_candidate(r - 1);
                }
                None if miss_fill && n > 0 => {
                    engine.select_candidate(0);
                }
                None => {}
            }
        }

        println!(
            "【段{} {}】 {} 词：{}",
            seg.index,
            seg.mode,
            counts.total(),
            counts.line()
        );

        match by_mode.iter_mut().find(|(m, _)| *m == seg.mode) {
            Some((_, c)) => c.merge(&counts),
            None => by_mode.push((seg.mode.clone(), counts)),
        }
    }

    if let Err(e) = writer.flush() {
        eprintln!("刷新结果失败: {e}");
    }

    println!();
    println!("【总计】 {} （共 {} 词）", total.line(), total.total());

    if !by_mode.is_empty() {
        println!();
        println!("按文体/模式：");
        for (mode, c) in &by_mode {
            println!("  {:<6} {} 词：{}", mode, c.total(), c.line());
        }
    }

    if issues.is_empty() {
        println!();
        println!("问题明细：无");
    } else {
        println!();
        println!("问题明细（ok 不列）：");
        for it in &issues {
            let rank_txt = match it.rank {
                Some(r) => format!("#{r}"),
                None => "—".to_string(),
            };
            println!(
                "  [{}] {} {} {:<4} 前{}: {}",
                it.status,
                pad(&it.word, 12),
                pad(&it.pinyin, 20),
                rank_txt,
                TOP_SHOW,
                it.preview
            );
        }
    }
}
