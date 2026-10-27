//! 索引加载耗时剖析：把 5.8s 拆成「读文件」与「bincode 反序列化」两段。
//!
//! 用法：cargo run --release --example bench_index -- resources/system_dict.db.bin

use std::time::Instant;

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "resources/system_dict.db.bin".to_string());
    let path = std::path::Path::new(&path);

    let t0 = Instant::now();
    let bytes = std::fs::read(path).expect("读取 .bin 失败");
    let read_ms = t0.elapsed().as_millis();
    println!(
        "fs::read            : {:>6} ms  ({} MB)",
        read_ms,
        bytes.len() / 1024 / 1024
    );

    let t1 = Instant::now();
    let d1: taishen_engine::dictionary::Dictionary =
        bincode::deserialize(&bytes).expect("反序列化失败");
    let de1_ms = t1.elapsed().as_millis();
    println!("bincode 反序列化(冷) : {:>6} ms", de1_ms);
    drop(d1);

    let t2 = Instant::now();
    let _d2: taishen_engine::dictionary::Dictionary =
        bincode::deserialize(&bytes).expect("反序列化失败");
    let de2_ms = t2.elapsed().as_millis();
    println!("bincode 反序列化(热) : {:>6} ms", de2_ms);

    println!("--------------------------------------------------");
    println!(
        "首段合计            : {:>6} ms",
        read_ms + de1_ms
    );

    let d3: taishen_engine::dictionary::Dictionary =
        bincode::deserialize(&bytes).expect("反序列化失败");
    println!("---- 各索引体积 ----");
    let mut total = 0usize;
    for (name, keys, size) in d3.index_sizes() {
        total += size;
        println!(
            "{:<18} 键 {:>9}  序列化 {:>7} MB",
            name,
            keys,
            size / 1024 / 1024
        );
    }
    println!("合计               {:<24} {:>7} MB", "", total / 1024 / 1024);
}
