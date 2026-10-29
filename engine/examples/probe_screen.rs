//! 首屏候选探测：用完整 Engine 管道（含短语/简拼/词长分区/英文混输）输出前 6 个候选。
//!
//! 用法: cargo run --example probe_screen
//!
//! 与微软拼音实测对照时，两边跑同一组输入。

use std::path::Path;

use taishen_engine::Engine;
use taishen_engine::dictionary;

const INPUTS: &[&str] = &[
    // 单字母（首字竞争）
    "a", "w", "n", "z", "s", "d", "h", "j",
    // 双字母（音节 vs 简拼）
    "ni", "wo", "zh", "zg", "nh", "sj",
    // 完整拼音（常用双字词）
    "nihao", "zhongguo", "gongzuo", "shijie", "women", "dianhua",
    "pingguo", "xiexie", "ceshi", "shuoming",
    // 易歧义（音节边界）
    "xian", "fan", "shiyan", "fangan", "pian",
    // 混合简拼
    "ces", "jianp", "shurf",
    // 纠错探针
    "xinashi", "xianshi", "xinshi", "zhongg", "nihap",
    // 英文混输
    "hello", "app", "abcd",
];

fn main() {
    let db = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("resources/system_dict.db");
    if !db.exists() {
        eprintln!("词库不存在: {}", db.display());
        return;
    }
    dictionary::init_blocking(Some(&db));

    println!("# 泰深输入法 — 首屏候选（前 6 个，完整 Engine 管道）\n");
    println!("input\t1\t2\t3\t4\t5\t6");
    for input in INPUTS {
        let mut e = Engine::new();
        for ch in input.chars() {
            e.process_key(ch);
        }
        let n = e.candidate_count().min(6);
        let mut row: Vec<String> = Vec::with_capacity(n);
        for i in 0..n {
            row.push(e.candidate(i).unwrap_or("").to_string());
        }
        while row.len() < 6 {
            row.push(String::new());
        }
        println!("{}\t{}", input, row.join("\t"));
    }
}
