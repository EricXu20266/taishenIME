//! 候选行为广度探针（诊断用）
//!
//! 用法: cargo run --release --example probe_candidates -- <输入串...>
//! 不传参数时跑内置用例集，输出每个输入串的前 8 候选 + 中英文构成。
//!
//! 目的：定位「key / dans / rug 全英文上屏」「xinashi 未纠错」这类行为差异。

use std::path::Path;
use taishen_engine::Engine;

/// 内置用例集：分五组覆盖不同输入形态
const CASES: &[(&str, &str)] = &[
    // A 组：合法全拼（基线）
    ("nihao", "A 合法全拼"),
    ("zhongguo", "A 合法全拼"),
    ("beijing", "A 合法全拼"),
    ("danshi", "A 合法全拼"),
    ("xian", "A 合法全拼"),
    // B 组：简拼（合法前缀）
    ("nh", "B 简拼"),
    ("zg", "B 简拼"),
    ("ky", "B 简拼"),
    ("bj", "B 简拼"),
    ("ds", "B 简拼"),
    // C 组：既非完整拼音、也不像英文词
    ("key", "C 疑英文精确"),
    ("dans", "C 疑英文精确"),
    ("rug", "C 疑英文精确"),
    ("rung", "C 疑英文精确"),
    ("car", "C 疑英文精确"),
    ("can", "C 疑英文精确"),
    ("gold", "C 疑英文精确"),
    ("sun", "C 疑英文精确"),
    ("zhongg", "C 拼音前缀截断"),
    // D 组：明显英文
    ("hello", "D 英文"),
    ("test", "D 英文"),
    ("app", "D 英文"),
    ("code", "D 英文"),
    ("python", "D 英文"),
    ("keyword", "D 英文"),
    ("keywords", "D 英文"),
    // E 组：纠错应命中
    ("logn", "E 纠错 long"),
    ("nihap", "E 纠错 nihao"),
    ("xinashi", "E 纠错 xianshi"),
    ("xianxi", "E 纠错 基线"),
    ("danshi", "E 纠错 基线"),
    ("zongguo", "E 纠错 zhongguo"),
];

fn is_ascii_word(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_graphic())
}

fn probe(engine: &mut Engine, input: &str, max_show: usize) {
    engine.reset();
    for ch in input.chars() {
        engine.process_key(ch);
    }
    let n = engine.candidate_count();
    let take = n.min(max_show);
    let mut items = Vec::new();
    let mut cn = 0usize;
    let mut en = 0usize;
    for i in 0..take {
        if let Some(c) = engine.candidate(i) {
            if is_ascii_word(c) {
                en += 1;
            } else {
                cn += 1;
            }
            // 带上来源：定位「某个候选从哪条线路来」必须看这个，
            // 光看文本无法区分 Dictionary / Completion / Combo。
            let src = engine
                .candidate_source(i)
                .map(|s| format!("{s:?}"))
                .unwrap_or_else(|| "-".to_string());
            items.push(format!("{c}({src})"));
        }
    }
    println!(
        "{} | {} | 共{} 前{}中{}英 | {}",
        input,
        engine.pinyin_str(),
        n,
        cn,
        en,
        items.join(" / ")
    );
}

fn main() {
    let dict = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("resources/system_dict.db");
    taishen_engine::dictionary::init(Some(dict.as_path()));
    // 预编译索引优先，确保走真实部署路径
    let bin = dict.with_extension("db.bin");
    if bin.exists() {
        eprintln!("索引存在: {}", bin.display());
    }
    eprintln!("等待词库就绪…");
    for _ in 0..600 {
        if taishen_engine::dictionary::is_ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    eprintln!("就绪: {}", taishen_engine::dictionary::is_ready());

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut engine = Engine::new();
    // --page-size N：按平台候选窗宽度取候选（平台 config 常见 5 / 9）
    let mut page_size: Option<usize> = None;
    let mut inputs: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--page-size" {
            page_size = it.next().and_then(|v| v.parse::<usize>().ok());
        } else {
            inputs.push(a.clone());
        }
    }
    if let Some(n) = page_size {
        engine.set_candidate_limit(n.max(1));
    }
    let show = page_size.unwrap_or(8);

    if inputs.first().map(|s| s.as_str()) == Some("--group") {
        for a in inputs.iter().skip(1) {
            let g = taishen_engine::dictionary::phrase_group_guess(a);
            println!("[group {a}] {} 条 {:?}", g.len(), g);
            let c = taishen_engine::dictionary::query_combo(a);
            println!("[combo {a}] {} 条 {:?}", c.len(), c);
            // 简拼 / 混拼 / 缩写+全拼：候选不足时的三类补充来源，
            // 定位「某条噪声由哪条线路拉进来」必须能分别看。
            let s = taishen_engine::dictionary::query_short(a);
            println!("[short {a}] {} 条 {:?}", s.len(), s);
            let m = taishen_engine::dictionary::query_mixed(a);
            println!("[mixed {a}] {} 条 {:?}", m.len(), m);
            let ab = taishen_engine::dictionary::query_abbrev_full(a);
            println!("[abbrev {a}] {} 条 {:?}", ab.len(), ab);
        }
        return;
    }

    if inputs.first().map(|s| s.as_str()) == Some("--variants") {
        for a in inputs.iter().skip(1) {
            println!("[{}]", a);
            println!(
                "  adjacent: {:?}",
                taishen_engine::correction::correction_variants(a)
            );
            println!(
                "  spelling: {:?}",
                taishen_engine::correction::spelling_variants(a)
            );
            println!(
                "  deletion: {:?}",
                taishen_engine::correction::deletion_variants(a)
            );
            println!(
                "  insertion: {:?}",
                taishen_engine::correction::insertion_variants(a)
            );
            // 逐个变体查词库，看哪些真的能命中（定位纠错失效点）
            for v in taishen_engine::correction::correction_variants(a)
                .iter()
                .take(6)
            {
                let r: Vec<String> = taishen_engine::dictionary::query(v)
                    .into_iter()
                    .take(3)
                    .collect();
                println!("  q({v}) => {r:?}");
            }
        }
        return;
    }

    if inputs.is_empty() {
        println!("=== 内置用例集 ===");
        for (input, group) in CASES {
            println!("[{group}]");
            probe(&mut engine, input, show);
        }
    } else {
        for a in &inputs {
            probe(&mut engine, a, show);
        }
    }
}
