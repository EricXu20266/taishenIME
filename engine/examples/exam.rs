//! 输入法考试工具：用拼音出候选，逐条断言候选行为。
//!
//! 用途：每次引擎/词库改动后跑一遍，验证「常用输入能不能打出该打的词」，
//! 同时量化「纠错抢占」「无意义串强行组词」「补全灌满」这类病态行为。
//! 人工在宿主里敲键盘验证的东西，这里全部用引擎 API 复现。
//!
//! 用法：
//!   cargo run --release --example exam
//!   cargo run --release --example exam -- --cases engine/tests/exam-cases.tsv
//!   cargo run --release --example exam -- --dict <词库绝对路径> --top 9
//!   cargo run --release --example exam -- --dump zei ji nihao     # 打印候选+来源，写用例时校准
//!   cargo run --release --example exam -- --no-completion         # 全局关闭前缀补全
//!   cargo run --release --example exam -- --user-db <user_dict.db> --dump ces shi
//!       # 加载真实用户词库做手动回归（仅人工实测入口，不作为 CI 固定断言来源）
//!
//! 退出码：0 全部通过（已知缺陷组的 FAIL 不算失败）；1 有 FAIL；2 环境问题。
//!
//! 用例文件格式（TSV，`#` 开头为注释，`[xxx]` 为分组）：
//!   拼音<TAB>断言<TAB>范围<TAB>备注
//!
//! 断言语法（第 2 列）：
//!   期望词          正向：该词在「范围」内
//!   !词1,词2        反向：这些词不得出现在前「范围」
//!   >词A>词B>词C    顺序：A 必须排在 B 前，B 必须排在 C 前（任一缺失即 FAIL）
//!   @src=Dictionary    前「范围」候选全部来自该来源（至少 1 个）
//!   @has_src=Correction 前「范围」至少一个来自该来源
//!   @no_src=Correction,Combo 前「范围」不得有这些来源
//!   @multi=0 / @multi>=1 / @multi<=2   前「范围」多字词数量约束（按汉字计）
//!   @count<=N / @count>=N           候选总数约束（candidate_count）
//!
//! 第 3 列省略时默认 1。备注里写 `completion=off` 表示该条关闭前缀补全。
//! section 名含「已知缺陷」的，FAIL 不计入总失败、不影响退出码，单独统计。

use std::path::{Path, PathBuf};
use taishen_engine::Engine;

/// 数值比较方式（@multi / @count 共用）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cmp {
    Eq,
    Ge,
    Le,
    Gt,
    Lt,
}

impl Cmp {
    fn test(self, actual: usize, bound: usize) -> bool {
        match self {
            Cmp::Eq => actual == bound,
            Cmp::Ge => actual >= bound,
            Cmp::Le => actual <= bound,
            Cmp::Gt => actual > bound,
            Cmp::Lt => actual < bound,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Cmp::Eq => "=",
            Cmp::Ge => ">=",
            Cmp::Le => "<=",
            Cmp::Gt => ">",
            Cmp::Lt => "<",
        }
    }
}

#[derive(Clone, Debug)]
enum Assertion {
    /// 正向：期望词排在前 max_rank
    Word(String),
    /// 反向：这些词都不得出现在前 max_rank
    NotWords(Vec<String>),
    /// 顺序：words[0] 在 words[1] 前，依此类推
    Order(Vec<String>),
    /// 前 max_rank 全部来自该来源
    AllSrc(String),
    /// 前 max_rank 至少一个该来源
    HasSrc(String),
    /// 前 max_rank 不得有这些来源
    NoSrc(Vec<String>),
    /// 前 max_rank 的多字词数量约束
    Multi(Cmp, usize),
    /// 候选总数约束
    Count(Cmp, usize),
    /// 候选窗宽稳定性：以 page_size=n 重跑，前 n 个候选与本次前 n 一致
    PageStable(usize),
}

struct Case {
    section: String,
    input: String,
    /// 第 2 列原文，失败明细原样展示
    raw: String,
    assertion: Assertion,
    max_rank: usize,
    note: String,
    /// 该条是否启用前缀补全（备注 completion=off 时关闭）
    completion: bool,
    /// 该条是否启用繁体输出（备注 traditional=on 时开启）
    traditional: bool,
}

struct Outcome {
    section: String,
    input: String,
    raw: String,
    max_rank: usize,
    note: String,
    ok: bool,
    /// 全部取到的候选（文本, 来源名）
    top: Vec<(String, String)>,
    /// 断言的实测结论（失败时用）
    reason: String,
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

/// 逗号分隔列表（去空、去重、保序）
fn split_list(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in raw.split(',') {
        let w = item.trim();
        if !w.is_empty() && !out.iter().any(|x| x == w) {
            out.push(w.to_string());
        }
    }
    out
}

/// 解析 `=0` / `>=1` / `<=5` / `>0` / `<3` / `1`
fn parse_cmp_num(raw: &str) -> Result<(Cmp, usize), String> {
    let s = raw.trim();
    let (cmp, rest) = if let Some(v) = s.strip_prefix(">=") {
        (Cmp::Ge, v)
    } else if let Some(v) = s.strip_prefix("<=") {
        (Cmp::Le, v)
    } else if let Some(v) = s.strip_prefix('=') {
        (Cmp::Eq, v)
    } else if let Some(v) = s.strip_prefix('>') {
        (Cmp::Gt, v)
    } else if let Some(v) = s.strip_prefix('<') {
        (Cmp::Lt, v)
    } else {
        (Cmp::Eq, s)
    };
    let n = rest
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("数值不合法: {s}"))?;
    Ok((cmp, n))
}

fn parse_assertion(raw: &str) -> Result<Assertion, String> {
    if let Some(rest) = raw.strip_prefix('!') {
        let words = split_list(rest);
        if words.is_empty() {
            return Err(format!("反向断言缺少词: {raw}"));
        }
        return Ok(Assertion::NotWords(words));
    }
    if let Some(rest) = raw.strip_prefix('>') {
        let mut words: Vec<String> = Vec::new();
        if !rest.trim().is_empty() {
            for w in rest.split('>') {
                let w = w.trim();
                if !w.is_empty() && !words.iter().any(|x| x == w) {
                    words.push(w.to_string());
                }
            }
        }
        if words.len() < 2 {
            return Err(format!("顺序断言至少两个词: {raw}"));
        }
        return Ok(Assertion::Order(words));
    }
    if let Some(rest) = raw.strip_prefix('@') {
        // 键按最长优先匹配：`has_src=` / `no_src=` 必须先于 `src=`，
        // 且不能按 `=` 切分——`@count>=1` 的 '=' 属于比较符，不是键值分隔符。
        const KEYS: [&str; 6] = ["has_src", "no_src", "src", "multi", "count", "page_stable"];
        let (key, val) = KEYS
            .iter()
            .find_map(|k| {
                rest.strip_prefix(k)
                    .map(|v| (*k, v.trim_start_matches('=').trim()))
            })
            .ok_or_else(|| format!("未知断言键: {raw}"))?;
        return match key {
            "src" => {
                if val.is_empty() {
                    Err(format!("@src 缺少来源名: {raw}"))
                } else {
                    Ok(Assertion::AllSrc(val.to_string()))
                }
            }
            "has_src" => {
                if val.is_empty() {
                    Err(format!("@has_src 缺少来源名: {raw}"))
                } else {
                    Ok(Assertion::HasSrc(val.to_string()))
                }
            }
            "no_src" => {
                let names = split_list(val);
                if names.is_empty() {
                    Err(format!("@no_src 缺少来源名: {raw}"))
                } else {
                    Ok(Assertion::NoSrc(names))
                }
            }
            "multi" => parse_cmp_num(val).map(|(c, n)| Assertion::Multi(c, n)),
            "count" => parse_cmp_num(val).map(|(c, n)| Assertion::Count(c, n)),
            "page_stable" => val
                .trim()
                .parse::<usize>()
                .map(|n| Assertion::PageStable(n.max(1)))
                .map_err(|_| format!("@page_stable 需要正整数: {raw}")),
            other => Err(format!("未知断言键 @{other}: {raw}")),
        };
    }
    Ok(Assertion::Word(raw.to_string()))
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
                "第 {} 行格式不对（需要 拼音<TAB>断言）: {line}",
                lineno + 1
            ));
        }
        let expect = cols[1].trim();
        if expect.is_empty() {
            return Err(format!("第 {} 行断言为空", lineno + 1));
        }
        let max_rank = cols
            .get(2)
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(1)
            .max(1);
        let note = cols
            .get(3)
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let assertion =
            parse_assertion(expect).map_err(|e| format!("第 {} 行解析失败: {e}", lineno + 1))?;
        out.push(Case {
            section: section.clone(),
            input: cols[0].trim().to_string(),
            raw: expect.to_string(),
            assertion,
            max_rank,
            completion: !note.replace(' ', "").contains("completion=off"),
            traditional: note.replace(' ', "").contains("traditional=on"),
            note,
        });
    }
    Ok(out)
}

fn is_han(c: char) -> bool {
    matches!(
        c as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F
    )
}

fn han_count(s: &str) -> usize {
    s.chars().filter(|c| is_han(*c)).count()
}

/// 判定一条断言。失败时把「哪一条不满足」写进 reason。
fn judge(
    assertion: &Assertion,
    top: &[(String, String)],
    max_rank: usize,
    total: usize,
) -> (bool, String) {
    let window_len = max_rank.min(top.len());
    let window = &top[..window_len];
    match assertion {
        Assertion::Word(w) => match top.iter().position(|(text, _)| text == w) {
            Some(p) => {
                let rank = p + 1;
                (
                    rank <= max_rank,
                    if rank <= max_rank {
                        format!("#{rank}")
                    } else {
                        format!("排在 #{rank}，超出 #{max_rank}")
                    },
                )
            }
            None => (false, format!("未进前 {} 候选", top.len())),
        },
        Assertion::NotWords(words) => {
            let hits: Vec<String> = window
                .iter()
                .enumerate()
                .filter(|(_, (text, _))| words.iter().any(|w| w == text))
                .map(|(i, (text, _))| format!("#{} {text}", i + 1))
                .collect();
            (
                hits.is_empty(),
                if hits.is_empty() {
                    format!("前 {window_len} 无排除词")
                } else {
                    format!("出现排除词 {}", hits.join(", "))
                },
            )
        }
        Assertion::Order(words) => {
            let mut positions: Vec<Option<usize>> = Vec::new();
            for w in words {
                positions.push(top.iter().position(|(text, _)| text == w));
            }
            let missing: Vec<&str> = words
                .iter()
                .zip(positions.iter())
                .filter(|(_, p)| p.is_none())
                .map(|(w, _)| w.as_str())
                .collect();
            if !missing.is_empty() {
                return (false, format!("未进候选: {}", missing.join(", ")));
            }
            let ranks: Vec<usize> = positions.iter().map(|p| p.unwrap() + 1).collect();
            let over: Vec<String> = words
                .iter()
                .zip(ranks.iter())
                .filter(|(_, r)| **r > max_rank)
                .map(|(w, r)| format!("{w}=#{r}"))
                .collect();
            if !over.is_empty() {
                return (false, format!("超出 #{max_rank}: {}", over.join(", ")));
            }
            let ordered = ranks.windows(2).all(|pair| pair[0] < pair[1]);
            (
                ordered,
                if ordered {
                    let desc: Vec<String> = words
                        .iter()
                        .zip(ranks.iter())
                        .map(|(w, r)| format!("{w}=#{r}"))
                        .collect();
                    desc.join(" < ")
                } else {
                    let desc: Vec<String> = words
                        .iter()
                        .zip(ranks.iter())
                        .map(|(w, r)| format!("{w}=#{r}"))
                        .collect();
                    format!("顺序不符: {}", desc.join(", "))
                },
            )
        }
        Assertion::AllSrc(src) => {
            if window.is_empty() {
                return (false, "前 N 无候选".to_string());
            }
            let bad: Vec<String> = window
                .iter()
                .enumerate()
                .filter(|(_, (_, s))| s != src)
                .map(|(i, (text, s))| format!("#{} {text}({s})", i + 1))
                .collect();
            (
                bad.is_empty(),
                if bad.is_empty() {
                    format!("前 {window_len} 全是 {src}")
                } else {
                    format!("非 {src} 来源: {}", bad.join(", "))
                },
            )
        }
        Assertion::HasSrc(src) => {
            let hit = window.iter().position(|(_, s)| s == src);
            (
                hit.is_some(),
                match hit {
                    Some(i) => format!("#{} 为 {src}", i + 1),
                    None => format!("前 {window_len} 无 {src} 来源"),
                },
            )
        }
        Assertion::NoSrc(names) => {
            let hits: Vec<String> = window
                .iter()
                .enumerate()
                .filter(|(_, (_, s))| names.iter().any(|n| n == s))
                .map(|(i, (text, s))| format!("#{} {text}({s})", i + 1))
                .collect();
            (
                hits.is_empty(),
                if hits.is_empty() {
                    format!("前 {window_len} 无 {}", names.join("/"))
                } else {
                    format!("命中禁用来源: {}", hits.join(", "))
                },
            )
        }
        Assertion::Multi(cmp, bound) => {
            let n = window
                .iter()
                .filter(|(text, _)| han_count(text) >= 2)
                .count();
            (
                cmp.test(n, *bound),
                format!("多字词 {n} 个（要求 {} {}）", cmp.label(), bound),
            )
        }
        Assertion::Count(cmp, bound) => (
            cmp.test(total, *bound),
            format!("候选总数 {total}（要求 {} {}）", cmp.label(), bound),
        ),
        // 需要再跑一次引擎才能判定，交给 run_case 复核
        Assertion::PageStable(n) => (true, format!("窗宽 @{n} 待复核")),
    }
}

fn run_case(
    case: &Case,
    top_n: usize,
    completion_default: bool,
    traditional_default: bool,
    ranking_v2: bool,
) -> Outcome {
    let mut engine = Engine::new();
    // `candidate(i)` 是「当前页内索引」——不放开 page_size 时只能取到 5 条，
    // `--top 9` 会静默退化成 5 条，断言口径与声明不一致。
    engine.set_candidate_limit(top_n);
    engine.set_completion_enabled(completion_default && case.completion);
    engine.set_traditional(traditional_default || case.traditional);
    engine.set_ranking_v2(ranking_v2);
    // 预热：排除首查建页开销（与真实输入路径一致）
    engine.process_key('a');
    engine.reset();
    for ch in case.input.chars() {
        engine.process_key(ch);
    }
    let total = engine.candidate_count();
    let top: Vec<(String, String)> = (0..top_n)
        .filter_map(|i| {
            let text = engine.candidate_display(i)?;
            let src = engine
                .candidate_source_name(i)
                .unwrap_or("Unknown")
                .to_string();
            Some((text, src))
        })
        .collect();
    let (mut ok, mut reason) = judge(&case.assertion, &top, case.max_rank, total);
    if let Assertion::PageStable(n) = &case.assertion {
        // 窄窗重跑：候选内容不应随 page_size 变化（同输入同设置）
        let n = *n;
        let mut narrow = Engine::new();
        narrow.set_candidate_limit(n);
        narrow.set_completion_enabled(completion_default && case.completion);
        narrow.set_traditional(traditional_default || case.traditional);
        narrow.set_ranking_v2(ranking_v2);
        narrow.process_key('a');
        narrow.reset();
        for ch in case.input.chars() {
            narrow.process_key(ch);
        }
        let prefix: Vec<String> = (0..n).filter_map(|i| narrow.candidate_display(i)).collect();
        let head: Vec<String> = top.iter().take(n).map(|(t, _)| t.clone()).collect();
        ok = !prefix.is_empty() && prefix == head;
        reason = if ok {
            format!("窗宽 {} 与 {} 前 {} 候选一致", top_n, n, prefix.len())
        } else {
            format!(
                "窗宽不一致：宽 {} / 窄 {}",
                head.join(" "),
                prefix.join(" ")
            )
        };
    }
    Outcome {
        section: case.section.clone(),
        input: case.input.clone(),
        raw: case.raw.clone(),
        max_rank: case.max_rank,
        note: case.note.clone(),
        ok,
        top,
        reason,
    }
}

/// 从 section 名取分级前缀：`L1-A 全拼单字` → `L1`。
/// 不含 `-` 的分组按整名归集（保持旧用例可读）。
fn level_of(section: &str) -> String {
    match section.split_once('-') {
        Some((head, _)) if !head.trim().is_empty() => head.trim().to_string(),
        _ => section.to_string(),
    }
}

fn is_known_defect(section: &str) -> bool {
    section.contains("已知缺陷")
}

/// 用当前补全设置跑一个输入，打印候选 + 来源（写用例时校准用）
fn dump_inputs(inputs: &[String], top_n: usize, completion: bool, traditional: bool) {
    println!(
        "# dump（补全{}，繁体{}，每输入取前 {} 个候选）",
        if completion { "开" } else { "关" },
        if traditional { "开" } else { "关" },
        top_n
    );
    for input in inputs {
        let mut engine = Engine::new();
        engine.set_candidate_limit(top_n);
        engine.set_completion_enabled(completion);
        engine.set_traditional(traditional);
        engine.process_key('a');
        engine.reset();
        for ch in input.chars() {
            engine.process_key(ch);
        }
        let total = engine.candidate_count();
        let mut row: Vec<String> = Vec::new();
        for i in 0..top_n {
            match engine.candidate_display(i) {
                Some(text) => row.push(format!(
                    "#{} {}({})",
                    i + 1,
                    text,
                    engine.candidate_source_name(i).unwrap_or("?")
                )),
                None => break,
            }
        }
        println!("{input}\tcount={total}\t{}", row.join("  "));
    }
}

fn main() {
    let mut cases_path = default_cases_path();
    let mut top_n = 9usize;
    let mut dict_override: Option<PathBuf> = None;
    let mut completion = true;
    let mut traditional = false;
    let mut ranking_v2 = true;
    let mut dump: Vec<String> = Vec::new();
    let mut user_db: Option<PathBuf> = None;

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
            "--no-completion" => completion = false,
            "--traditional" => traditional = true,
            "--v2" => ranking_v2 = true,
            "--user-db" => {
                if let Some(v) = args.get(i + 1) {
                    user_db = Some(PathBuf::from(v));
                    i += 1;
                }
            }
            "--dump" => {
                i += 1;
                while i < args.len() && !args[i].starts_with("--") {
                    dump.push(args[i].clone());
                    i += 1;
                }
                continue;
            }
            "--help" | "-h" => {
                println!(
                    "用法: exam [--cases <tsv>] [--dict <db>] [--user-db <user_dict.db>] [--top <n>] [--no-completion] [--traditional] [--dump 输入...]"
                );
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

    // 真实用户词库（手动回归专用）：大词库就绪后再挂载，避免异步加载把
    // 用户词层覆盖掉。不传 --user-db 时保持干净引擎，零变化。
    if let Some(ref udb) = user_db {
        if !udb.is_file() {
            eprintln!("用户词库文件不存在: {}", udb.display());
            std::process::exit(2);
        }
        taishen_engine::dictionary::set_user_dict_path(Some(udb.as_path()));
    }

    if !dump.is_empty() {
        dump_inputs(&dump, top_n, completion, traditional);
        return;
    }

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

    println!("词库: {}", dict.display());
    if let Some(ref udb) = user_db {
        println!("用户词库: {}", udb.display());
    }
    println!(
        "用例: {}（{} 条，取前 {} 个候选，补全{}）",
        cases_path.display(),
        cases.len(),
        top_n,
        if completion { "开" } else { "关" }
    );
    if let Some(first) = cases.first() {
        println!("首条分组: {}", first.section);
    }
    println!();

    let outcomes: Vec<Outcome> = cases
        .iter()
        .map(|case| run_case(case, top_n, completion, traditional, ranking_v2))
        .collect();

    // 分级聚合（保持首次出现顺序）
    let mut levels: Vec<String> = Vec::new();
    for o in &outcomes {
        let lv = level_of(&o.section);
        if !levels.contains(&lv) {
            levels.push(lv);
        }
    }

    let mut real_failed: Vec<&Outcome> = Vec::new();
    let mut defect_failed: Vec<&Outcome> = Vec::new();
    let mut real_total = 0usize;
    let mut real_pass = 0usize;
    let mut defect_total = 0usize;
    let mut defect_pass = 0usize;
    for o in &outcomes {
        if is_known_defect(&o.section) {
            defect_total += 1;
            if o.ok {
                defect_pass += 1;
            } else {
                defect_failed.push(o);
            }
        } else {
            real_total += 1;
            if o.ok {
                real_pass += 1;
            } else {
                real_failed.push(o);
            }
        }
    }

    println!("分级统计：");
    for lv in &levels {
        let group: Vec<&Outcome> = outcomes
            .iter()
            .filter(|o| &level_of(&o.section) == lv)
            .collect();
        let pass = group.iter().filter(|o| o.ok).count();
        let pct = pass as f64 * 100.0 / group.len() as f64;
        let has_defect = group.iter().any(|o| is_known_defect(&o.section));
        let tail = if has_defect {
            "（含已知缺陷）"
        } else {
            ""
        };
        println!(
            "{:<10} 通过 {:>4}/{:<4} {:>6.1}%{}",
            lv,
            pass,
            group.len(),
            pct,
            tail
        );
        if has_defect {
            let real: Vec<&Outcome> = group
                .iter()
                .copied()
                .filter(|o| !is_known_defect(&o.section))
                .collect();
            let rp = real.iter().filter(|o| o.ok).count();
            println!(
                "  └ 不含已知缺陷 {:>4}/{:<4} {:>6.1}%",
                rp,
                real.len(),
                if real.is_empty() {
                    0.0
                } else {
                    rp as f64 * 100.0 / real.len() as f64
                }
            );
        }
    }
    println!();
    println!(
        "总计 {}/{} 通过（{:.1}%）",
        real_pass,
        real_total,
        if real_total == 0 {
            0.0
        } else {
            real_pass as f64 * 100.0 / real_total as f64
        }
    );
    println!(
        "已知缺陷 {}/{} 通过（{:.1}%，FAIL 不计入总计与退出码）",
        defect_pass,
        defect_total,
        if defect_total == 0 {
            0.0
        } else {
            defect_pass as f64 * 100.0 / defect_total as f64
        }
    );

    let print_failures = |title: &str, list: &[&Outcome]| {
        if list.is_empty() {
            return;
        }
        println!();
        println!("{title}：");
        for o in list {
            let hint = if o.note.is_empty() {
                String::new()
            } else {
                format!(" [{}]", o.note)
            };
            let actual: Vec<String> = o
                .top
                .iter()
                .enumerate()
                .map(|(i, (text, src))| format!("#{} {}({})", i + 1, text, src))
                .collect();
            println!(
                "  [{}] {} {} → {} {}",
                o.section,
                o.input,
                o.raw,
                format!("范围 {}，{}", o.max_rank, o.reason),
                hint
            );
            println!("        实际: {}", actual.join("  "));
        }
    };

    print_failures("失败明细（不计已知缺陷）", &real_failed);
    print_failures("已知缺陷 FAIL 明细", &defect_failed);

    if !real_failed.is_empty() {
        std::process::exit(1);
    }
}
