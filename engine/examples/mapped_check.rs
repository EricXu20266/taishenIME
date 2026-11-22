//! 一致性验证：`TSDICT05` 容器中三张平坦 mmap 索引
//! （`index` 主索引、`full_index` 完整拼音索引、`suffix_index` 混合简拼后缀索引）
//! 与内存 `HashMap` / `BTreeMap` 原表在全量 key 上是否产出**逐条相同**的结果。
//!
//! 模拟生产 `from_sqlite` 全量构建链路，作为 P2 切调用点与容器格式改动的安全阀：
//! 任何一项不一致即以非零状态退出。
//!
//! 用法: cargo run --release --example mapped_check

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use taishen_engine::dictionary::mapped::{
    build_container, build_index_section_pairs, build_index_section_triples,
    build_suffix_sections, ContainerView, MappedIndex, MappedSuffixIndex, WordPoolBuilder,
    CONTAINER_MAGIC, SEC_FULL_INDEX, SEC_INDEX_MAIN, SEC_SUFFIX_INDEX, SEC_SUFFIX_PREFIX_POOL,
    SEC_WORD_POOL,
};

const MAX_PREFIX_ENTRIES: usize = 400;

fn dict_syllables(raw: &str, clean: &str) -> Vec<String> {
    if raw.contains('\'') {
        raw.split('\'')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect()
    } else {
        let mut res = Vec::new();
        let mut rest = clean;
        while !rest.is_empty() {
            if let Some((syl, tail)) = taishen_engine::pinyin::split_first_syllable(rest) {
                res.push(syl.to_string());
                rest = tail;
            } else {
                res.push(rest.to_string());
                break;
            }
        }
        res
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let db = root.join("resources/system_dict.db");
    let conn = rusqlite::Connection::open(&db).expect("打开词库失败");

    // 1. 按生产 from_sqlite 逻辑生成三张参考内存表
    let mut index_map: HashMap<String, Vec<(String, u32, usize)>> = HashMap::new();
    let mut full_map: BTreeMap<String, Vec<(String, u32)>> = BTreeMap::new();
    let mut suffix_map: HashMap<String, Vec<(String, String, u32)>> = HashMap::new();

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
            full_map
                .entry(p.clone())
                .or_default()
                .push((word.clone(), freq));
            for i in 1..=p.len() {
                index_map
                    .entry(p[..i].to_string())
                    .or_default()
                    .push((word.clone(), freq, p.len()));
            }
            let syls = dict_syllables(&py_raw.to_lowercase(), &p);
            if syls.len() >= 2 {
                let mut prefix_acc = String::new();
                for k in 0..syls.len() {
                    prefix_acc.push_str(&taishen_engine::pinyin::to_initial_full(&syls[k]));
                    if k + 1 < syls.len() {
                        let suffix = syls[k + 1..].join("");
                        suffix_map.entry(suffix).or_default().push((
                            prefix_acc.clone(),
                            word.clone(),
                            freq,
                        ));
                    }
                }
            }
        }
    }

    for (prefix, entries) in index_map.iter_mut() {
        let key_len = prefix.len();
        entries.sort_by(|a, b| {
            let a_exact = (a.2 == key_len) as u8;
            let b_exact = (b.2 == key_len) as u8;
            b_exact.cmp(&a_exact).then(b.1.cmp(&a.1))
        });
        entries.truncate(MAX_PREFIX_ENTRIES);
    }
    for words in full_map.values_mut() {
        words.sort_by(|a, b| b.1.cmp(&a.1));
        words.truncate(MAX_PREFIX_ENTRIES);
    }
    for entries in suffix_map.values_mut() {
        entries.sort_by(|a, b| b.2.cmp(&a.2));
        entries.truncate(MAX_PREFIX_ENTRIES);
    }

    println!(
        "参考表规模: index={} key | full_index={} key | suffix_index={} key",
        index_map.len(),
        full_map.len(),
        suffix_map.len()
    );

    // 2. 构建 TSDICT05 容器（三表共享 SEC_WORD_POOL）写盘并 mmap 打开
    let tmp = std::env::temp_dir().join("taishen_mapped_check_v5.bin");
    let _ = std::fs::remove_file(&tmp);

    let t = std::time::Instant::now();
    let mut wp_builder =
        WordPoolBuilder::with_capacity(full_map.len().max(16), full_map.len().max(16) * 10);
    let main_sec = build_index_section_triples(&index_map, &mut wp_builder).expect("main 失败");
    let full_sec = build_index_section_pairs(&full_map, &mut wp_builder).expect("full 失败");
    let (pfx_sec, sfx_sec) =
        build_suffix_sections(&suffix_map, &mut wp_builder).expect("suffix 失败");
    let wp_sec = wp_builder.finish();
    let bytes = build_container(&[
        (SEC_WORD_POOL, &wp_sec),
        (SEC_INDEX_MAIN, &main_sec),
        (SEC_FULL_INDEX, &full_sec),
        (SEC_SUFFIX_PREFIX_POOL, &pfx_sec),
        (SEC_SUFFIX_INDEX, &sfx_sec),
    ]);
    assert_eq!(&bytes[..8], CONTAINER_MAGIC);
    println!(
        "构建耗时: {:?}  词池={} MB  main={} MB  full={} MB  suffix={} MB  总容器={} MB",
        t.elapsed(),
        wp_sec.len() / 1024 / 1024,
        main_sec.len() / 1024 / 1024,
        full_sec.len() / 1024 / 1024,
        (pfx_sec.len() + sfx_sec.len()) / 1024 / 1024,
        bytes.len() / 1024 / 1024
    );
    std::fs::write(&tmp, &bytes).expect("写临时索引失败");

    let t = std::time::Instant::now();
    let view = ContainerView::open(&tmp).expect("mmap 容器打开失败");
    let b = view.backing();
    let wp_r = view.section_range(SEC_WORD_POOL).unwrap();
    let idx = MappedIndex::from_container(
        Arc::clone(b),
        wp_r.clone(),
        view.section_range(SEC_INDEX_MAIN).unwrap(),
    )
    .expect("index 打开失败");
    let full_idx = MappedIndex::from_container(
        Arc::clone(b),
        wp_r.clone(),
        view.section_range(SEC_FULL_INDEX).unwrap(),
    )
    .expect("full_index 打开失败");
    let sfx_idx = MappedSuffixIndex::from_container(
        Arc::clone(b),
        wp_r,
        view.section_range(SEC_SUFFIX_PREFIX_POOL).unwrap(),
        view.section_range(SEC_SUFFIX_INDEX).unwrap(),
    )
    .expect("suffix_index 打开失败");
    drop(view);
    assert!(idx.is_mmap() && full_idx.is_mmap() && sfx_idx.is_mmap());
    println!(
        "mmap 打开耗时: {:?}  entry 数: index={} full={} suffix={}",
        t.elapsed(),
        idx.entry_count(),
        full_idx.entry_count(),
        sfx_idx.entry_count()
    );

    let mut mismatched = 0usize;
    let missing_keys = ["zzzzzzz", "qqqq", "vvvvv", "x", "", "nihaoma"];

    // 3a. 全量比对 index
    let mut main_checked = 0usize;
    let mut main_entries = 0usize;
    let check_main = |key: &str, mismatched: &mut usize, checked: &mut usize, total: &mut usize| {
        *checked += 1;
        match (index_map.get(key), idx.get(key)) {
            (None, None) => {}
            (Some(va), Some(vb)) => {
                *total += va.len();
                if va.len() != vb.len() {
                    if *mismatched < 5 {
                        eprintln!("✗ index[{key}]: 条数 {} vs {}", va.len(), vb.len());
                    }
                    *mismatched += 1;
                    return;
                }
                for (i, (w, f, pl)) in va.iter().enumerate() {
                    let r = vb.get(i).unwrap();
                    if r.word != w.as_str() || r.freq != *f || r.pinyin_len != *pl {
                        if *mismatched < 5 {
                            eprintln!(
                                "✗ index[{key}][{i}]: ({},{},{}) vs ({},{},{})",
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
                    eprintln!("✗ index[{key}]: 一侧命中一侧未命中");
                }
                *mismatched += 1;
            }
        }
    };
    for k in index_map.keys() {
        check_main(k, &mut mismatched, &mut main_checked, &mut main_entries);
    }
    for k in missing_keys {
        check_main(k, &mut mismatched, &mut main_checked, &mut main_entries);
    }

    // 3b. 全量比对 full_index + range_from 抽样
    let mut full_checked = 0usize;
    let mut full_entries = 0usize;
    let check_full = |key: &str, mismatched: &mut usize, checked: &mut usize, total: &mut usize| {
        *checked += 1;
        match (full_map.get(key), full_idx.get(key)) {
            (None, None) => {}
            (Some(va), Some(vb)) => {
                *total += va.len();
                if va.len() != vb.len() {
                    if *mismatched < 5 {
                        eprintln!("✗ full[{key}]: 条数 {} vs {}", va.len(), vb.len());
                    }
                    *mismatched += 1;
                    return;
                }
                for (i, (w, f)) in va.iter().enumerate() {
                    let r = vb.get(i).unwrap();
                    if r.word != w.as_str() || r.freq != *f {
                        if *mismatched < 5 {
                            eprintln!(
                                "✗ full[{key}][{i}]: ({},{}) vs ({},{})",
                                w, f, r.word, r.freq
                            );
                        }
                        *mismatched += 1;
                        return;
                    }
                }
            }
            _ => {
                if *mismatched < 5 {
                    eprintln!("✗ full[{key}]: 一侧命中一侧未命中");
                }
                *mismatched += 1;
            }
        }
    };
    for k in full_map.keys() {
        check_full(k, &mut mismatched, &mut full_checked, &mut full_entries);
    }
    for k in missing_keys {
        check_full(k, &mut mismatched, &mut full_checked, &mut full_entries);
    }

    for start in ["zhongguo", "nihao", "zh", "x", ""] {
        let expected: Vec<(&str, usize)> = full_map
            .range(start.to_string()..)
            .take(500)
            .map(|(k, v)| (k.as_str(), v.len()))
            .collect();
        let actual: Vec<(&str, usize)> = full_idx
            .range_from(start)
            .take(500)
            .map(|(k, v)| (k, v.len()))
            .collect();
        if actual != expected {
            eprintln!(
                "✗ full.range_from({start}) 不一致: {} vs {}",
                actual.len(),
                expected.len()
            );
            mismatched += 1;
        }
    }

    // 3c. 全量比对 suffix_index
    let mut sfx_checked = 0usize;
    let mut sfx_entries = 0usize;
    let check_sfx = |key: &str, mismatched: &mut usize, checked: &mut usize, total: &mut usize| {
        *checked += 1;
        match (suffix_map.get(key), sfx_idx.get(key)) {
            (None, None) => {}
            (Some(va), Some(vb)) => {
                *total += va.len();
                if va.len() != vb.len() {
                    if *mismatched < 5 {
                        eprintln!("✗ suffix[{key}]: 条数 {} vs {}", va.len(), vb.len());
                    }
                    *mismatched += 1;
                    return;
                }
                for (i, (pi, w, f)) in va.iter().enumerate() {
                    let r = vb.get(i).unwrap();
                    if r.prefix_initials != pi.as_str() || r.word != w.as_str() || r.freq != *f {
                        if *mismatched < 5 {
                            eprintln!(
                                "✗ suffix[{key}][{i}]: ({},{},{}) vs ({},{},{})",
                                pi, w, f, r.prefix_initials, r.word, r.freq
                            );
                        }
                        *mismatched += 1;
                        return;
                    }
                }
            }
            _ => {
                if *mismatched < 5 {
                    eprintln!("✗ suffix[{key}]: 一侧命中一侧未命中");
                }
                *mismatched += 1;
            }
        }
    };
    for k in suffix_map.keys() {
        check_sfx(k, &mut mismatched, &mut sfx_checked, &mut sfx_entries);
    }
    for k in missing_keys {
        check_sfx(k, &mut mismatched, &mut sfx_checked, &mut sfx_entries);
    }

    // 额外校验 range_prefix 顺序与覆盖数
    for pfx in ["zhongguo", "nihao", "zh", "x"] {
        let mut expected: Vec<&str> = index_map
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
    println!("index  比对: {main_checked} key / {main_entries} entry");
    println!("full   比对: {full_checked} key / {full_entries} entry (+ range_from)");
    println!("suffix 比对: {sfx_checked} key / {sfx_entries} entry");
    println!("不一致: {mismatched}");
    if mismatched == 0 {
        println!("✓ TSDICT05 三表全量完全一致");
    } else {
        println!("✗ 存在不一致，不可切换");
        std::process::exit(1);
    }

    drop(idx);
    drop(full_idx);
    drop(sfx_idx);
    let _ = std::fs::remove_file(&tmp);
}
