//! 长句整句候选探针：输出每个输入的前 3 条整句与全量候选头部，带耗时。
//!
//! 用法: cargo run --release --example probe_sentence -- [输入...]
//! 会按部署路径加载 bigram.db（拆组/整句的搭配打分依赖它）。

use std::path::Path;
use std::time::Instant;
use taishen_engine::Engine;

/// 内置用例：常见长句 + 含错输入，附期望文本（空表示只看行为）。
const CASES: &[(&str, &str)] = &[
    ("nimenjigezaizheliganshenme", "你们几个在这里干什么"),
    ("nimenjigerenzaizheli", "你们几个人在这里"),
    ("womenzaizheliganshenme", "我们在这里干什么"),
    ("womenxiangquchifan", "我们想去吃饭"),
    ("jintiantianqizhenbucuo", "今天天气真不错"),
    ("niyoumeiyoukong", "你有没有空"),
    ("womenmingtianqubeijing", "我们明天去北京"),
    ("zhegeshiwomendedianhua", "这个是我们的电话"),
    ("xianzaiwomenzaizheli", "现在我们在这里"),
    ("wojintianyaojiaban", "我今天要加班"),
    ("womenyiqiqukandianying", "我们一起去看电影"),
    ("zhegewentixuyaozhuyi", "这个问题需要注意"),
    ("zhongguorenminyinhang", "中国人民银行"),
    ("nimjigerenzaizhli", "你们几个人在这里"),
    ("nimjigerenzaizhliganshenm", "你们几个在这里干什么"),
];

/// 混拼用例：简拼与全拼混排，词级通道覆盖不到，只靠整句解码。
const CASES_MIXED: &[(&str, &str)] = &[
    ("wmzaizheliganshenme", "我们在这里干什么"),
    ("wmmingtianqubeijing", "我们明天去北京"),
    ("nimjigezaizheliganshenme", "你们几个在这里干什么"),
    ("nimjigerenzaizheli", "你们几个人在这里"),
];

fn main() {
    let dict = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("resources/system_dict.db");
    taishen_engine::dictionary::init(Some(dict.as_path()));
    eprintln!("等待词库就绪…");
    for _ in 0..600 {
        if taishen_engine::dictionary::is_ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let bigram = dict.with_file_name("bigram.db");
    if bigram.exists() {
        let t = Instant::now();
        let pairs = taishen_engine::dictionary::set_bigram(&bigram, 1, usize::MAX);
        eprintln!("bigram: {pairs} 对, {}ms", t.elapsed().as_millis());
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut engine = Engine::new();

    let run = |engine: &mut Engine, input: &str| {
        engine.reset();
        let mut elapsed = std::time::Duration::ZERO;
        for ch in input.chars() {
            let t = Instant::now();
            engine.process_key(ch);
            elapsed += t.elapsed();
        }
        let n = engine.candidate_count();
        let top = if n > 0 {
            engine.candidate(0).map(str::to_string)
        } else {
            None
        };
        let mut items = Vec::new();
        for i in 0..n.min(3) {
            if let Some(c) = engine.candidate(i) {
                items.push(c.to_string());
            }
        }
        // 直接调整句解码：用来区分「候选首位命中」是整句通道给的，
        // 还是既有词级查询碰巧拼出来的（后者不能证明整句解码生效）。
        let decoded = taishen_engine::sentence::decode(input, None, 1);
        println!(
            "{input:<32} 共{n:>3} 累计 {:>6.2}ms | {} | 整句:{}",
            elapsed.as_secs_f64() * 1000.0,
            items.join(" / "),
            decoded.first().map(|s| s.as_str()).unwrap_or("∅")
        );
        (top, decoded.first().cloned())
    };

    if args.is_empty() {
        let mut hit = 0;
        let mut hit_sentence = 0;
        for (input, want) in CASES {
            let (top, sentence) = run(&mut engine, input);
            let ok = top.as_deref() == Some(*want);
            if ok {
                hit += 1;
            }
            if sentence.as_deref() == Some(*want) {
                hit_sentence += 1;
            }
            if !ok && !want.is_empty() {
                println!("      期望: {want}");
            }
        }
        println!(
            "\n候选首位命中 {hit}/{}，整句解码命中 {hit_sentence}/{}",
            CASES.len(),
            CASES.len()
        );
        // 混拼组单独统计：这组是本轮新增能力，不和全拼组混在一起看。
        let mut mixed_hit = 0;
        let mut mixed_sentence = 0;
        for (input, want) in CASES_MIXED {
            let (top, sentence) = run(&mut engine, input);
            if top.as_deref() == Some(*want) {
                mixed_hit += 1;
            }
            if sentence.as_deref() == Some(*want) {
                mixed_sentence += 1;
            }
            if top.as_deref() != Some(*want) {
                println!("      混拼期望: {want}");
            }
        }
        println!(
            "混拼组：候选首位命中 {mixed_hit}/{}，整句解码命中 {mixed_sentence}/{}",
            CASES_MIXED.len(),
            CASES_MIXED.len()
        );
    } else {
        for input in &args {
            let _ = run(&mut engine, input);
        }
    }
}
