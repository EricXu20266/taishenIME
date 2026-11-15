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
        let top = if n > 0 { engine.candidate(0).map(str::to_string) } else { None };
        let mut items = Vec::new();
        for i in 0..n.min(3) {
            if let Some(c) = engine.candidate(i) {
                items.push(c.to_string());
            }
        }
        println!(
            "{input:<32} 共{n:>3} 累计 {:>6.2}ms | {}",
            elapsed.as_secs_f64() * 1000.0,
            items.join(" / ")
        );
        top
    };

    if args.is_empty() {
        let mut hit = 0;
        for (input, want) in CASES {
            let top = run(&mut engine, input);
            let ok = top.as_deref() == Some(*want);
            if ok {
                hit += 1;
            }
            if !ok && !want.is_empty() {
                println!("      期望: {want}");
            }
        }
        println!("\n首位命中 {hit}/{}", CASES.len());
    } else {
        for input in &args {
            run(&mut engine, input);
        }
    }
}
