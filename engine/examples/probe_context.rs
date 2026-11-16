//! 搭配表效果验证：以「强搭配但后词不在词库首位」的真实案例，对比
//! 上下文开关前后的候选位次。
//!
//! 用法:
//!   cargo run --release --example probe_context            # 跑内置案例集
//!   cargo run --release --example probe_context -- <tsv>   # 跑指定 TSV
//!
//! TSV 格式（由挖案例脚本生成）：
//!   prev_word \t prev_pinyin \t next_pinyin \t next_word \t rank \t tier
//!
//! 前置条件（挖案例时已保证）：前词是其拼音的第一候选，可直接 select(0)。
//! 内置案例的目标词都在词库第 2-6 位，测得出提升；首位案例测不出（已剔除）。

use std::path::Path;
use taishen_engine::Engine;

fn run_tsv(prev_py: &str, next_py: &str, context: bool) -> Vec<String> {
    let mut e = Engine::new();
    e.set_context_enabled(context);
    for ch in prev_py.chars() {
        e.process_key(ch);
    }
    if e.select_candidate(0).is_none() {
        return Vec::new();
    }
    for ch in next_py.chars() {
        e.process_key(ch);
    }
    (0..e.candidate_count())
        .filter_map(|i| e.candidate(i).map(|s| s.to_string()))
        .collect()
}

struct Case {
    prev: String,
    prev_py: String,
    next_py: String,
    expect: String,
    dict_rank: usize,
    tier: u32,
}

/// 内置案例：都是从搭配表挖出来的强搭配（档位 6），目标词在词库第 2-6 位
fn builtin_cases() -> Vec<Case> {
    let raw = [
        ("政治", "zhengzhi", "renwu", "人物", 2, 6),
        ("社会", "shehui", "zhuyi", "主义", 3, 6),
        ("中央", "zhongyang", "yanjiuyuan", "研究院", 2, 6),
        ("生产", "shengchan", "zongzhi", "总值", 3, 6),
        ("重点", "zhongdian", "shiyanshi", "实验室", 2, 6),
        ("人工", "rengong", "zhineng", "智能", 3, 6),
        ("森林", "senlin", "gongyuan", "公园", 2, 6),
        ("科学院", "kexueyuan", "yuanshi", "院士", 2, 6),
        ("文化", "wenhua", "mingcheng", "名城", 2, 6),
        ("航空", "hangkong", "mujian", "母舰", 2, 6),
        ("自由", "ziyou", "yizhi", "意志", 6, 6),
        ("中文", "zhongwen", "jinqu", "金曲", 5, 6),
        ("二二八", "ererba", "shijian", "事件", 2, 6),
        ("德川", "dechuan", "jiakang", "家康", 2, 6),
    ];
    raw.iter()
        .map(|(p, pp, np, e, r, t)| Case {
            prev: p.to_string(),
            prev_py: pp.to_string(),
            next_py: np.to_string(),
            expect: e.to_string(),
            dict_rank: *r,
            tier: *t,
        })
        .collect()
}

fn read_tsv(path: &Path) -> Vec<Case> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 5 {
            continue;
        }
        out.push(Case {
            prev: f[0].to_string(),
            prev_py: f[1].to_string(),
            next_py: f[2].to_string(),
            expect: f[3].to_string(),
            dict_rank: f[4].parse().unwrap_or(0),
            tier: f.get(5).and_then(|s| s.parse().ok()).unwrap_or(0),
        });
    }
    out
}

fn main() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let dict = base.join("resources/system_dict.db");
    let bigram = base.join("resources/bigram.db");
    taishen_engine::dictionary::init(Some(dict.as_path()));
    for _ in 0..600 {
        if taishen_engine::dictionary::is_ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    // 加载真实搭配表（min_count=2：与生产消费侧口径一致，档位 1 已不存在）
    let table = taishen_engine::dictionary::load_bigram(&bigram, 2, 64);
    let pairs: usize = table.values().map(|v| v.len()).sum();
    println!(
        "bigram: {} 前词 / {} 对（min_count=2）",
        table.len(),
        pairs
    );
    taishen_engine::dictionary::install_bigram(table);

    let cases = match std::env::args().nth(1) {
        Some(p) => {
            let v = read_tsv(Path::new(&p));
            if v.is_empty() {
                println!("TSV 读取为空，回退内置案例");
                builtin_cases()
            } else {
                v
            }
        }
        None => builtin_cases(),
    };

    let mut improved = 0;
    let mut same = 0;
    let mut worse = 0;
    for c in &cases {
        let off = run_tsv(&c.prev_py, &c.next_py, false);
        let on = run_tsv(&c.prev_py, &c.next_py, true);
        let pos_off = off.iter().position(|w| w == &c.expect);
        let pos_on = on.iter().position(|w| w == &c.expect);
        let tag = match (pos_off, pos_on) {
            (Some(a), Some(b)) if b < a => {
                improved += 1;
                format!("IMPROVED {a}->{b}")
            }
            (Some(a), Some(b)) if b > a => {
                worse += 1;
                format!("WORSE {a}->{b}")
            }
            (None, Some(b)) => {
                improved += 1;
                format!("IMPROVED 无->{b}")
            }
            (Some(_), None) => {
                worse += 1;
                "WORSE 有->无".to_string()
            }
            _ => {
                same += 1;
                "-".to_string()
            }
        };
        println!(
            "{} + {} 「{}」(档位{} 词库第{}位): off={:?} on={:?} {}",
            c.prev, c.next_py, c.expect, c.tier, c.dict_rank, pos_off, pos_on, tag
        );
    }
    println!(
        "\nRESULT: {} 例提升 / {} 例不变 / {} 例变差（共 {}）",
        improved,
        same,
        worse,
        cases.len()
    );
    if improved > 0 {
        println!("判定: 搭配表在真实案例上生效");
    } else {
        println!("判定: 搭配表未产生可观测提升（数据或权重需复查）");
    }
}
