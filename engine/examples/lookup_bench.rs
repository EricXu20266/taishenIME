//! P0 验证：排序键 + 二分查找，能否在 13 万 key/分片的规模上满足单键 < 1ms。
//!
//! 对照对象是当前的 `HashMap<String, Vec<...>>`（哈希查找）。
//! 若二分显著慢于哈希，mmap 方案需要改成「哈希前置 + 偏移表」。
//!
//! 用法: cargo run --release --example lookup_bench

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

fn main() {
    let db = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("resources/system_dict.db");
    let conn = rusqlite::Connection::open(&db).expect("打开词库失败");

    // 生成全部前缀（与 index 构建逻辑一致：逐字符）
    let mut prefixes: Vec<String> = Vec::new();
    {
        let mut stmt = conn.prepare("SELECT pinyin FROM system_dict").unwrap();
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .filter_map(|x| x.ok());
        for py in rows {
            let p = py.replace('\'', "").to_lowercase();
            for i in 1..=p.len() {
                prefixes.push(p[..i].to_string());
            }
        }
    }
    prefixes.sort();
    prefixes.dedup();
    let total = prefixes.len();

    // 8 片（与现有 .bin 分片一致）
    let shard_count = 8usize;
    let per_shard = total / shard_count;
    println!("前缀总数 {total}，分 {shard_count} 片，每片约 {per_shard}");

    // 结构 A：HashMap（现状）
    let mut hash_shards: Vec<HashMap<&str, u32>> = Vec::new();
    // 结构 B：排序数组（mmap 方案的形状）
    let mut sorted_shards: Vec<Vec<&str>> = Vec::new();
    for s in 0..shard_count {
        let lo = s * per_shard;
        let hi = if s + 1 == shard_count {
            total
        } else {
            (s + 1) * per_shard
        };
        let slice = &prefixes[lo..hi];
        let mut m = HashMap::with_capacity(slice.len());
        for (i, k) in slice.iter().enumerate() {
            m.insert(k.as_str(), i as u32);
        }
        hash_shards.push(m);
        let mut v: Vec<&str> = slice.iter().map(|s| s.as_str()).collect();
        v.sort_unstable();
        sorted_shards.push(v);
    }

    // 探测样本：取每片中间的若干 key（命中最坏路径附近）
    let mut probes: Vec<&str> = Vec::new();
    for shard in sorted_shards.iter() {
        for frac in [0.0, 0.25, 0.5, 0.75, 0.99] {
            let idx = ((shard.len() as f64 - 1.0) * frac) as usize;
            probes.push(shard[idx]);
        }
    }
    let miss = "zzzzzzzzzz";
    const ROUNDS: usize = 2000;

    // 哈希查找
    let t = Instant::now();
    let mut acc = 0u64;
    for _ in 0..ROUNDS {
        for (i, p) in probes.iter().enumerate() {
            let shard = i % shard_count;
            if let Some(v) = hash_shards[shard].get(*p) {
                acc += u64::from(*v);
            }
            if hash_shards[shard].get(miss).is_none() {
                acc += 1;
            }
        }
    }
    let hash_ns = t.elapsed().as_nanos() as f64 / (ROUNDS * probes.len() * 2) as f64;

    // 二分查找
    let t = Instant::now();
    for _ in 0..ROUNDS {
        for (i, p) in probes.iter().enumerate() {
            let shard = i % shard_count;
            if let Ok(pos) = sorted_shards[shard].binary_search(p) {
                acc += pos as u64;
            }
            if sorted_shards[shard].binary_search(&miss).is_err() {
                acc += 1;
            }
        }
    }
    let bin_ns = t.elapsed().as_nanos() as f64 / (ROUNDS * probes.len() * 2) as f64;

    println!();
    println!("单次查找（含命中和未命中各半）：");
    println!("  HashMap : {:.1} ns", hash_ns);
    println!("  排序二分: {:.1} ns", bin_ns);
    println!("  比值    : {:.1}x", bin_ns / hash_ns);
    println!();
    println!("单键要求 < 1ms = 1,000,000 ns。按每键约 10 次前缀查询估算：");
    println!("  二分总开销 ≈ {:.1} us", bin_ns * 10.0 / 1000.0);
    println!("(acc={acc} 仅用于防止优化掉查询)");
}
