//! 方案 f（高扇出稀疏前缀）实测微基准：
//! 对比直接查 `index.get(prefix)` vs 从 `full_index.range_prefix(prefix)` 现算+排序，
//! 按扇出度分层（1、2..=8、9..=32、>32）统计 P50/P95/P99/Mean 耗时，
//! 并统计 `fanout <= 32` 全量前缀的候选序列与 `index` 现存结果的差异率（含 tie-break 差异）。
//!
//! 用法: cargo run --release --example sparse_prefix_bench

use std::collections::HashMap;
use std::hint::black_box;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use taishen_engine::dictionary::mapped::{
    ContainerView, MappedIndex, SEC_FULL_INDEX, SEC_INDEX_MAIN, SEC_WORD_POOL,
};

const MAX_PREFIX_ENTRIES: usize = 400;
const BENCH_PER_STRATUM: usize = 300;
const WARM_ITERS: usize = 5;

struct StatSummary {
    mean_ns: f64,
    p50_ns: u128,
    p95_ns: u128,
    p99_ns: u128,
}

fn summarize(samples: &mut [u128]) -> StatSummary {
    samples.sort_unstable();
    let n = samples.len().max(1);
    let sum: u128 = samples.iter().sum();
    let p50 = samples[n / 2];
    let p95 = samples[(n * 95 / 100).min(n - 1)];
    let p99 = samples[(n * 99 / 100).min(n - 1)];
    StatSummary {
        mean_ns: sum as f64 / n as f64,
        p50_ns: p50,
        p95_ns: p95,
        p99_ns: p99,
    }
}

#[inline]
fn query_direct<'a>(index: &'a MappedIndex, prefix: &str, out: &mut Vec<(&'a str, u32)>) {
    out.clear();
    if let Some(ev) = index.get(prefix) {
        for e in ev.iter() {
            out.push((e.word, e.freq));
        }
    }
}

#[inline]
fn query_reconstruct<'a>(
    full_index: &'a MappedIndex,
    prefix: &'a str,
    tmp: &mut Vec<(bool, u32, &'a str)>,
    out: &mut Vec<(&'a str, u32)>,
) {
    tmp.clear();
    out.clear();
    let key_len = prefix.len();
    for (py, ev) in full_index.range_prefix(prefix) {
        let exact = py.len() == key_len;
        for e in ev.iter() {
            tmp.push((exact, e.freq, e.word));
        }
    }
    tmp.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    tmp.truncate(MAX_PREFIX_ENTRIES);
    for &(_, f, w) in tmp.iter() {
        out.push((w, f));
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let bin_path = root.join("resources/system_dict.db.bin");
    let view = ContainerView::open(&bin_path).expect("打开 system_dict.db.bin 失败（需 TSDICT05）");
    let b = view.backing();
    let wp = view.section_range(SEC_WORD_POOL).expect("缺少 word_pool");
    let index = MappedIndex::from_container(
        Arc::clone(b),
        wp.clone(),
        view.section_range(SEC_INDEX_MAIN).expect("缺少 index"),
    )
    .expect("加载 index 失败");
    let full_index = MappedIndex::from_container(
        Arc::clone(b),
        wp,
        view.section_range(SEC_FULL_INDEX).expect("缺少 full_index"),
    )
    .expect("加载 full_index 失败");
    drop(view);

    // 1. 统计每个 index 前缀在 full_index 下命中的完整拼音键数（扇出度）
    let t0 = Instant::now();
    let mut fanout_map: HashMap<String, usize> = HashMap::with_capacity(index.len());
    for (py, _) in full_index.iter() {
        for i in 1..=py.len() {
            *fanout_map.entry(py[..i].to_string()).or_insert(0) += 1;
        }
    }

    let mut strata: [Vec<(String, usize)>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    let mut stratum_entries: [u64; 4] = [0; 4];
    let mut stratum_key_bytes: [u64; 4] = [0; 4];
    for (k, ev) in index.iter() {
        let fo = *fanout_map.get(k).unwrap_or(&0);
        let bucket = match fo {
            0..=1 => 0,
            2..=8 => 1,
            9..=32 => 2,
            _ => 3,
        };
        stratum_entries[bucket] += ev.len() as u64;
        stratum_key_bytes[bucket] += (k.len() + 16) as u64;
        strata[bucket].push((k.to_string(), fo));
    }
    println!(
        "扇出统计完成 ({:?}): index={} key (full={})",
        t0.elapsed(),
        index.len(),
        full_index.len()
    );
    let names = ["fanout=1  ", "fanout=2..8", "fanout=9..32", "fanout>32 "];
    for i in 0..4 {
        let keys = strata[i].len();
        let pct = keys as f64 * 100.0 / index.len().max(1) as f64;
        let data_bytes = stratum_entries[i] * 8 + stratum_key_bytes[i];
        println!(
            "  {}: {:>7} key ({:>5.1}%), {:>7} entry (~{:.1} MB 索引体)",
            names[i],
            keys,
            pct,
            stratum_entries[i],
            data_bytes as f64 / 1024.0 / 1024.0
        );
    }

    // 2. 分层抽取 >= 1,000 个样本做延迟微基准
    let mut direct_buf: Vec<(&str, u32)> = Vec::with_capacity(512);
    let mut tmp_buf: Vec<(bool, u32, &str)> = Vec::with_capacity(2048);
    let mut recon_buf: Vec<(&str, u32)> = Vec::with_capacity(512);

    println!();
    println!(
        "=== 微基准对比 (每层抽样 <= {}, warm={} 轮) ===",
        BENCH_PER_STRATUM, WARM_ITERS
    );
    println!(
        "{:<13} | {:>5} | {:>6} | {:>16} | {:>16} | {:>7}",
        "分层", "样本数", "平均entry", "direct P50/P99ns", "recon P50/P99ns", "倍率(B/A)"
    );

    let mut total_bench_keys = 0usize;
    for (b_idx, list) in strata.iter().enumerate() {
        if list.is_empty() {
            continue;
        }
        let step = (list.len() / BENCH_PER_STRATUM).max(1);
        let sample: Vec<&str> = list
            .iter()
            .step_by(step)
            .take(BENCH_PER_STRATUM)
            .map(|(k, _)| k.as_str())
            .collect();
        total_bench_keys += sample.len();

        // 预热页缓存
        for &k in &sample {
            query_direct(&index, k, &mut direct_buf);
            black_box(&direct_buf);
            query_reconstruct(&full_index, k, &mut tmp_buf, &mut recon_buf);
            black_box(&recon_buf);
        }

        let mut ns_direct = Vec::with_capacity(sample.len() * WARM_ITERS);
        let mut ns_recon = Vec::with_capacity(sample.len() * WARM_ITERS);
        let mut scanned_entries_sum = 0usize;

        for _ in 0..WARM_ITERS {
            for &k in &sample {
                let t = Instant::now();
                query_direct(&index, k, &mut direct_buf);
                black_box(&direct_buf);
                ns_direct.push(t.elapsed().as_nanos());

                let t = Instant::now();
                query_reconstruct(&full_index, k, &mut tmp_buf, &mut recon_buf);
                black_box(&recon_buf);
                ns_recon.push(t.elapsed().as_nanos());
                scanned_entries_sum += recon_buf.len();
            }
        }

        let sd = summarize(&mut ns_direct);
        let sr = summarize(&mut ns_recon);
        let avg_ent = scanned_entries_sum as f64 / (sample.len() * WARM_ITERS).max(1) as f64;
        let ratio_p50 = sr.p50_ns as f64 / sd.p50_ns.max(1) as f64;
        let ratio_mean = sr.mean_ns / sd.mean_ns.max(1.0);
        println!(
            "{:<13} | {:>5} | {:>9.1} | {:>7}/{:>7} | {:>7}/{:>7} | {:>5.2}x (mean {:.2}x: {:.0} vs {:.0} ns)",
            names[b_idx],
            sample.len(),
            avg_ent,
            sd.p50_ns,
            sd.p99_ns,
            sr.p50_ns,
            sr.p99_ns,
            ratio_p50,
            ratio_mean,
            sr.mean_ns,
            sd.mean_ns
        );
    }
    println!("合计基准样本数: {}", total_bench_keys);

    // 3. 一致性与同频序差异统计（fanout <= 32 全量检验）
    println!();
    println!("=== fanout <= 32 全量一致性检验 ===");
    let t1 = Instant::now();
    for (b_idx, list) in strata[..3].iter().enumerate() {
        let mut tested = 0usize;
        let mut exact_seq_match = 0usize;
        let mut top1_diff = 0usize;
        let mut multiset_diff = 0usize;
        let mut tie_only_diff = 0usize;

        for (k, _) in list {
            tested += 1;
            query_direct(&index, k, &mut direct_buf);
            query_reconstruct(&full_index, k, &mut tmp_buf, &mut recon_buf);

            if direct_buf == recon_buf {
                exact_seq_match += 1;
                continue;
            }
            if direct_buf.first().map(|x| x.0) != recon_buf.first().map(|x| x.0) {
                top1_diff += 1;
            }
            if direct_buf.len() != recon_buf.len() {
                multiset_diff += 1;
            } else {
                // 检查是否仅同频/同exact顺序不同，还是集合不同
                let mut a = direct_buf.clone();
                let mut b = recon_buf.clone();
                a.sort_unstable();
                b.sort_unstable();
                if a == b {
                    tie_only_diff += 1;
                } else {
                    multiset_diff += 1;
                }
            }
        }
        let diff_total = tested - exact_seq_match;
        println!(
            "{}: 测试 {} | 完全一致 {} ({:.2}%) | 差异 {} (仅tie序={}, 集合/截断={}, top1变={})",
            names[b_idx],
            tested,
            exact_seq_match,
            exact_seq_match as f64 * 100.0 / tested.max(1) as f64,
            diff_total,
            tie_only_diff,
            multiset_diff,
            top1_diff
        );
    }
    println!("全量差异统计耗时: {:?}", t1.elapsed());

    drop(index);
    drop(full_index);
}
