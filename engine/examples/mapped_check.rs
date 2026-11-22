//! 一致性验证：`MappedIndex`（`TSDICT04` mmap + 单表全局有序二分）与内存 `HashMap` 索引
//! 对同一批 key 是否产出**逐条相同**的结果。
//!
//! 模拟生产 `from_sqlite` 全量构建链路（`to_simplified` 去重 + `sort_by_exact_then_freq`
//! + `MAX_PREFIX_ENTRIES = 400` 截断），作为切调用点与修改格式的安全阀：不一致即失败。
//!
//! 用法: cargo run --release --example mapped_check

use std::collections::{HashMap, HashSet};
use std::path::Path;

use taishen_engine::dictionary::mapped::{
    build_container, MappedIndex, SEC_INDEX_MAIN, SEC_WORD_POOL,
};

const MAX_PREFIX_ENTRIES: usize = 400;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let db = root.join("resources/system_dict.db");
    let conn = rusqlite::Connection::open(&db).expect("打开词库失败");

    // 1. 按生产 from_sqlite 逻辑生成 index_map
    let mut map: HashMap<String, Vec<(String, u32, usize)>> = HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT pinyin, word, frequency FROM system_dict ORDER BY pinyin, frequency DESC")
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)? as u32,
                ))
            })
            .unwrap();
        let mut seen: HashSet<(String, String)> = HashSet::with_capacity(4096);
        for row in rows.filter_map(|x| x.ok()) {
            let (py_raw, word, freq) = row;
            let p = py_raw.replace('\'', "").to_lowercase();
            let word = taishen_engine::trad::to_simplified(&word);
            if !seen.insert((p.clone(), word.clone())) {
                continue;
            }
            for i in 1..=p.len() {
                map.entry(p[..i].to_string())
                    .or_default()
                    .push((word.clone(), freq, p.len()));
            }
        }
    }
    for (prefix, entries) in map.iter_mut() {
        let key_len = prefix.len();
        entries.sort_by(|a, b| {
            let a_exact = (a.2 == key_len) as u8;
            let b_exact = (b.2 == key_len) as u8;
            b_exact.cmp(&a_exact).then(b.1.cmp(&a.1))
        });
        entries.truncate(MAX_PREFIX_ENTRIES);
    }
    println!("index 前缀数: {}", map.len());

    // 2. 构建 TSDICT04 容器写盘并 mmap 打开
    let tmp = std::env::temp_dir().join("taishen_mapped_check_v4.bin");
    let _ = std::fs::remove_file(&tmp);

    let t = std::time::Instant::now();
    let owned = MappedIndex::from_map(&map).expect("from_map 失败");
    let bytes = build_container(&[
        (SEC_WORD_POOL, owned.raw_word_pool_bytes()),
        (SEC_INDEX_MAIN, owned.raw_index_bytes()),
    ]);
    println!(
        "构建耗时: {:?}  主索引+词池容器大小: {} MB",
        t.elapsed(),
        bytes.len() / 1024 / 1024
    );
    std::fs::write(&tmp, &bytes).expect("写临时索引失败");

    let t = std::time::Instant::now();
    let idx = MappedIndex::open(&tmp).expect("mmap 失败");
    println!(
        "mmap 打开耗时: {:?}  key 数: {}  entry 数: {}",
        t.elapsed(),
        idx.len(),
        idx.entry_count()
    );

    // 3. 全量比对：所有 key + 不存在的 key + range_prefix 抽样
    let mut checked = 0usize;
    let mut mismatched = 0usize;
    let mut total_entries = 0usize;

    let compare = |key: &str, mismatched: &mut usize, checked: &mut usize, total: &mut usize| {
        let a = map.get(key);
        let b = idx.get(key);
        *checked += 1;
        match (a, b) {
            (None, None) => {}
            (Some(va), Some(vb)) => {
                *total += va.len();
                if va.len() != vb.len() {
                    if *mismatched < 5 {
                        eprintln!("✗ {key}: 条数 {} vs {}", va.len(), vb.len());
                    }
                    *mismatched += 1;
                    return;
                }
                for (i, (w, f, pl)) in va.iter().enumerate() {
                    let r = vb.get(i).unwrap();
                    if r.word != w.as_str() || r.freq != *f || r.pinyin_len != *pl {
                        if *mismatched < 5 {
                            eprintln!(
                                "✗ {key}[{i}]: ({},{},{}) vs ({},{},{})",
                                w, f, pl, r.word, r.freq, r.pinyin_len
                            );
                        }
                        *mismatched += 1;
                        return;
                    }
                }
            }
            _ => {
                if *mismatched < 5 {
                    eprintln!("✗ {key}: 一侧命中一侧未命中");
                }
                *mismatched += 1;
            }
        }
    };

    for k in map.keys() {
        compare(k, &mut mismatched, &mut checked, &mut total_entries);
    }
    let missing_keys = ["zzzzzzz", "qqqq", "vvvvv", "x", "", "nihaoma"];
    for k in missing_keys {
        compare(k, &mut mismatched, &mut checked, &mut total_entries);
    }

    // 额外校验 range_prefix 顺序与覆盖数
    for pfx in ["zhongguo", "nihao", "zh", "x"] {
        let mut expected: Vec<&str> = map
            .keys()
            .filter(|k| k.starts_with(pfx))
            .map(String::as_str)
            .collect();
        expected.sort_unstable();
        let actual: Vec<&str> = idx.range_prefix(pfx).map(|(k, _)| k).collect();
        if actual != expected {
            eprintln!(
                "✗ range_prefix({pfx}) 不一致: {} vs {}",
                actual.len(),
                expected.len()
            );
            mismatched += 1;
        }
    }

    println!();
    println!("比对 key 数: {checked}（含 {} 个不存在）", missing_keys.len());
    println!("比对词条数: {total_entries}");
    println!("不一致: {mismatched}");
    if mismatched == 0 {
        println!("✓ 完全一致，可以切调用点");
    } else {
        println!("✗ 存在不一致，不可切换");
        std::process::exit(1);
    }

    drop(idx);
    let _ = std::fs::remove_file(&tmp);
}
