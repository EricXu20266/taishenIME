//! 一致性验证：`MappedIndex`（mmap + 排序键二分）与现有 `HashMap` 索引
//! 对同一批 key 是否产出**逐条相同**的结果。
//!
//! 这是切调用点前的安全阀：不一致就不切。
//!
//! 用法: cargo run --release --example mapped_check

use std::collections::HashMap;
use std::path::Path;

use taishen_engine::dictionary::mapped::{MappedIndex, build_file};

fn shard_of(key: &str, n: usize) -> usize {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut h);
    (h.finish() as usize) % n
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let db = root.join("resources/system_dict.db");
    let conn = rusqlite::Connection::open(&db).expect("打开词库失败");

    // 1. 按现有构建逻辑生成 index
    let mut map: HashMap<String, Vec<(String, u32, usize)>> = HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT word, pinyin, frequency FROM system_dict")
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
        for row in rows.filter_map(|x| x.ok()) {
            let (word, py, freq) = row;
            let p = py.replace('\'', "").to_lowercase();
            for i in 1..=p.len() {
                map.entry(p[..i].to_string())
                    .or_default()
                    .push((word.clone(), freq, p.len()));
            }
        }
    }
    println!("index 前缀数: {}", map.len());

    // 2. 分 8 片构建
    const SHARDS: usize = 8;
    let mut shards: Vec<HashMap<String, Vec<(String, u32, usize)>>> =
        (0..SHARDS).map(|_| HashMap::new()).collect();
    for (k, v) in &map {
        shards[shard_of(k, SHARDS)].insert(k.clone(), v.clone());
    }

    // 3. 写文件并 mmap
    let tmp = std::env::temp_dir().join("taishen_mapped_check.bin");
    let t = std::time::Instant::now();
    let bytes = build_file(&shards);
    println!(
        "构建耗时: {:?}  文件大小: {} MB",
        t.elapsed(),
        bytes.len() / 1024 / 1024
    );
    std::fs::write(&tmp, &bytes).expect("写临时索引失败");

    let t = std::time::Instant::now();
    let idx = MappedIndex::open(&tmp).expect("mmap 失败");
    println!("mmap 打开耗时: {:?}  key 数: {}", t.elapsed(), idx.len());

    // 4. 全量比对：所有 key + 一批不存在的 key
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
                    if r.word != w.as_str() || r.freq != *f || r.pinyin_len != *pl as usize {
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
    // 不存在的 key（走二分未命中分支）
    for k in ["zzzzzzz", "qqqq", "vvvvv", "x", "", "nihaoma"] {
        compare(k, &mut mismatched, &mut checked, &mut total_entries);
    }

    println!();
    println!("比对 key 数: {checked}（含 {} 个不存在）", 6);
    println!("比对词条数: {total_entries}");
    println!("不一致: {mismatched}");
    if mismatched == 0 {
        println!("✓ 完全一致，可以切调用点");
    } else {
        println!("✗ 存在不一致，不可切换");
    }

    let _ = std::fs::remove_file(&tmp);
}
