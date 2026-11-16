//! 预置搭配表（bigram.db）效果验证：前词上下文能否把后继词提前，
//! 以及不同档位门槛（min_count）对效果的影响。
//!
//! 用法: cargo run --release --example probe_context
//!
//! 方法：同一组「前词 + 后词拼音」跑多遍，每遍换一个 min_count 重新装表，
//! 对比后词候选里目标词的位置。min_count=1 表示档位 1（只共现一次）也参与。

use std::path::Path;
use taishen_engine::Engine;

fn run(prev_input: &str, prev_pick: usize, next_input: &str, context: bool) -> Vec<String> {
    let mut e = Engine::new();
    e.set_context_enabled(context);
    for ch in prev_input.chars() {
        e.process_key(ch);
    }
    if e.select_candidate(prev_pick).is_none() {
        return Vec::new();
    }
    for ch in next_input.chars() {
        e.process_key(ch);
    }
    (0..e.candidate_count().min(6))
        .filter_map(|i| e.candidate(i).map(|s| s.to_string()))
        .collect()
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

    // (前词输入, 前词选第几个, 后词输入, 期望搭配词)
    let cases: &[(&str, usize, &str, &str)] = &[
        ("wo", 0, "xiang", "想"),
        ("wo", 0, "yao", "要"),
        ("wo", 0, "men", "们"),
        ("ni", 0, "hao", "好"),
        ("jintian", 0, "tianqi", "天气"),
        ("xiexie", 0, "ni", "你"),
        ("women", 0, "yiqi", "一起"),
        ("zhongguo", 0, "renmin", "人民"),
    ];

    for min_count in [0u32, 1, 2, 3, 4] {
        if min_count == 0 {
            println!("=== 未装 bigram（baseline）===");
        } else {
            let table = taishen_engine::dictionary::load_bigram(&bigram, min_count, 64);
            let pairs: usize = table.values().map(|v| v.len()).sum();
            println!(
                "=== min_count={min_count}: {} 前词 / {} 对 ===",
                table.len(),
                pairs
            );
            taishen_engine::dictionary::install_bigram(table);
            println!(
                "  direct bigram_scores(我,[想,响,向])={:?} available={}",
                taishen_engine::dictionary::bigram_scores("我", &["想", "响", "向"]),
                taishen_engine::dictionary::bigram_available()
            );
        }
        let mut improved = 0;
        for &(prev, pick, next, expect) in cases {
            let off = run(prev, pick, next, false);
            let on = run(prev, pick, next, true);
            let pos_off = off.iter().position(|w| w == expect);
            let pos_on = on.iter().position(|w| w == expect);
            let better = match (pos_off, pos_on) {
                (Some(a), Some(b)) => b < a,
                (None, Some(_)) => true,
                _ => false,
            };
            if better {
                improved += 1;
            }
            println!("  {prev}+{next} 「{expect}」 off={pos_off:?} on={pos_on:?} {}", if better { "IMPROVED" } else { "-" });
        }
        println!("  小结: {improved}/{} 例提前", cases.len());
    }

    // === 机制验证：人工注入「我→响」高权重，看 xiang 首位是否变成「响」 ===
    // 上面 8 个真实案例的目标词本来就在首位，测不出提升；这里用可控数据证明
    // apply_context_boost → bigram_scores → permute 这条链是否真的接通。
    {
        let mut injected: std::collections::HashMap<String, Vec<(String, u32)>> =
            std::collections::HashMap::new();
        injected.insert("我".to_string(), vec![("响".to_string(), 6u32)]);
        taishen_engine::dictionary::install_bigram(injected);
        let off = run("wo", 0, "xiang", false);
        let on = run("wo", 0, "xiang", true);
        let pos_off = off.iter().position(|w| w == "响");
        let pos_on = on.iter().position(|w| w == "响");
        println!(
            "  INJECT 我→响(6): off={pos_off:?} on={pos_on:?}  off候选={off:?} on候选={on:?}"
        );
        let engaged = match (pos_off, pos_on) {
            (None, Some(_)) => true,
            (Some(a), Some(b)) => b < a,
            _ => false,
        };
        println!(
            "  RESULT: {}",
            if engaged {
                "搭配链已接通"
            } else {
                "搭配链未生效（疑似 bug）"
            }
        );
    }
}
