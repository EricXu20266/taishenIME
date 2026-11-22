//! 一致性验证：`TSDICT06` 容器中全部 6 张系统平坦 mmap 索引
//! （`index`、`full_index`、`suffix_index`、`short_index`、`short_index_full`、`trad_index`）
//! 与内存 `HashMap` / `BTreeMap` 原表在全量 key 上是否产出**逐条相同**的结果。
//!
//! 模拟生产 `from_sqlite` 全量构建链路，作为 P3 切调用点与容器格式改动的安全阀：
//! 任何一项不一致即以非零状态退出。
//!
//! 用法: cargo run --release --example mapped_check

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use taishen_engine::dictionary::mapped::{
    build_container, build_index_section_pairs, build_index_section_triples,
    build_suffix_sections, ContainerView, MappedIndex, MappedSuffixIndex, WordPoolBuilder,
    CONTAINER_MAGIC, SEC_FULL_INDEX, SEC_INDEX_MAIN, SEC_SHORT_FULL_INDEX, SEC_SHORT_INDEX,
    SEC_SUFFIX_INDEX, SEC_SUFFIX_PREFIX_POOL, SEC_TRAD_INDEX, SEC_WORD_POOL,
};

const MAX_PREFIX_ENTRIES: usize = 400;

fn dict_initials(raw: &str, clean: &str) -> (String, String) {
    if !raw.contains('\'') {
        return (
            taishen_engine::pinyin::to_initial_string(clean),
            taishen_engine::pinyin::to_initial_full(clean),
        );
    }
    let syls: Vec<&str> = raw.split('\'').filter(|s| !s.is_empty()).collect();
    let compact: String = syls.iter().map(|s| s.chars().next().unwrap_or(' ')).collect();
    let full: String = syls
        .iter()
        .map(|s| {
            let b = s.as_bytes();
            if b.len() >= 2 && matches!(&b[..2], b"zh" | b"ch" | b"sh") {
                s[..2].to_string()
            } else {
                s.chars().next().unwrap_or(' ').to_string()
            }
        })
        .collect();
    (compact, full)
}

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

fn sort_by_exact_then_freq(entries: &mut [(String, u32, usize)], key_len: usize) {
    entries.sort_by(|a, b| {
        let a_exact = (a.2 == key_len) as u8;
        let b_exact = (b.2 == key_len) as u8;
        b_exact.cmp(&a_exact).then(b.1.cmp(&a.1))
    });
}

fn check_triple_table(
    label: &str,
    map: &HashMap<String, Vec<(String, u32, usize)>>,
    idx: &MappedIndex,
    missing_keys: &[&str],
    mismatched: &mut usize,
) -> (usize, usize) {
    let mut checked = 0usize;
    let mut total_entries = 0usize;
    let mut check = |key: &str| {
        checked += 1;
        match (map.get(key), idx.get(key)) {
            (None, None) => {}
            (Some(va), Some(vb)) => {
                total_entries += va.len();
                if va.len() != vb.len() {
                    if *mismatched < 5 {
                        eprintln!("✗ {label}[{key}]: 条数 {} vs {}", va.len(), vb.len());
                    }
                    *mismatched += 1;
                    return;
                }
                for (i, (w, f, pl)) in va.iter().enumerate() {
                    let r = vb.get(i).unwrap();
                    if r.word != w.as_str() || r.freq != *f || r.pinyin_len != *pl {
                        if *mismatched < 5 {
                            eprintln!(
                                "✗ {label}[{key}][{i}]: ({},{},{}) vs ({},{},{})",
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
                    eprintln!("✗ {label}[{key}]: 一侧命中一侧未命中");
                }
                *mismatched += 1;
            }
        }
    };
    for k in map.keys() {
        check(k);
    }
    for k in missing_keys {
        check(k);
    }
    (checked, total_entries)
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let db = root.join("resources/system_dict.db");
    let conn = rusqlite::Connection::open(&db).expect("打开词库失败");

    // 1. 按生产 from_sqlite 逻辑生成 6 张参考内存表
    let mut index_map: HashMap<String, Vec<(String, u32, usize)>> = HashMap::new();
    let mut full_map: BTreeMap<String, Vec<(String, u32)>> = BTreeMap::new();
    let mut suffix_map: HashMap<String, Vec<(String, String, u32)>> = HashMap::new();
    let mut short_map: HashMap<String, Vec<(String, u32, usize)>> = HashMap::new();
    let mut short_full_map: HashMap<String, Vec<(String, u32, usize)>> = HashMap::new();
    let mut trad_map: HashMap<String, Vec<(String, u32, usize)>> = HashMap::new();

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
            let py_raw_lower = py_raw.to_lowercase();
            let (short, short_full) = dict_initials(&py_raw_lower, &p);
            if !short.is_empty() {
                for i in 1..=short.len() {
                    short_map
                        .entry(short[..i].to_string())
                        .or_default()
                        .push((word.clone(), freq, p.len()));
                }
            }
            if !short_full.is_empty() && short_full != short {
                for i in 1..=short_full.len() {
                    short_full_map
                        .entry(short_full[..i].to_string())
                        .or_default()
                        .push((word.clone(), freq, short_full.len()));
                }
            }
            let syls = dict_syllables(&py_raw_lower, &p);
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

    // 读取 system_dict_trad 构建 trad_map（保持与 load_trad_map_from_db 完全一致）
    {
        let has_trad = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='system_dict_trad'",
            )
            .map(|mut s| {
                s.query_map([], |r| r.get::<_, String>(0))
                    .map(|rows| rows.count() > 0)
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if has_trad {
            let mut stmt = conn
                .prepare("SELECT pinyin, word, frequency FROM system_dict_trad ORDER BY frequency DESC")
                .unwrap();
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, u32>(2)?,
                    ))
                })
                .unwrap();
            for row in rows.filter_map(|x| x.ok()) {
                let (py_raw, word, freq) = row;
                let p = if py_raw.contains('\'') {
                    py_raw.replace('\'', "")
                } else {
                    py_raw
                };
                for i in 1..=p.len() {
                    trad_map
                        .entry(p[..i].to_string())
                        .or_default()
                        .push((word.clone(), freq, p.len()));
                }
            }
        }
    }

    for (prefix, entries) in index_map.iter_mut() {
        sort_by_exact_then_freq(entries, prefix.len());
        entries.truncate(MAX_PREFIX_ENTRIES);
    }
    for (prefix, entries) in short_map.iter_mut() {
        sort_by_exact_then_freq(entries, prefix.len());
        entries.truncate(MAX_PREFIX_ENTRIES);
    }
    for (prefix, entries) in short_full_map.iter_mut() {
        sort_by_exact_then_freq(entries, prefix.len());
        entries.truncate(MAX_PREFIX_ENTRIES);
    }
    for entries in suffix_map.values_mut() {
        entries.sort_by(|a, b| b.2.cmp(&a.2));
        entries.truncate(MAX_PREFIX_ENTRIES);
    }
    for words in full_map.values_mut() {
        words.sort_by(|a, b| b.1.cmp(&a.1));
        words.truncate(MAX_PREFIX_ENTRIES);
    }
    for entries in trad_map.values_mut() {
        entries.sort_by(|a, b| b.1.cmp(&a.1));
        entries.truncate(MAX_PREFIX_ENTRIES);
    }

    println!(
        "参考表规模: index={} | full={} | suffix={} | short={} | short_full={} | trad={}",
        index_map.len(),
        full_map.len(),
        suffix_map.len(),
        short_map.len(),
        short_full_map.len(),
        trad_map.len()
    );

    // 2. 构建 TSDICT06 容器（6 表共享 SEC_WORD_POOL）写盘并 mmap 打开
    let tmp = std::env::temp_dir().join("taishen_mapped_check_v6.bin");
    let _ = std::fs::remove_file(&tmp);

    let t = std::time::Instant::now();
    let mut wp_builder =
        WordPoolBuilder::with_capacity(full_map.len().max(16), full_map.len().max(16) * 10);
    let main_sec = build_index_section_triples(&index_map, &mut wp_builder).expect("main 失败");
    let full_sec = build_index_section_pairs(&full_map, &mut wp_builder).expect("full 失败");
    let (pfx_sec, sfx_sec) =
        build_suffix_sections(&suffix_map, &mut wp_builder).expect("suffix 失败");
    let short_sec = build_index_section_triples(&short_map, &mut wp_builder).expect("short 失败");
    let short_full_sec =
        build_index_section_triples(&short_full_map, &mut wp_builder).expect("short_full 失败");
    let trad_sec = build_index_section_triples(&trad_map, &mut wp_builder).expect("trad 失败");
    let wp_sec = wp_builder.finish();
    let bytes = build_container(&[
        (SEC_WORD_POOL, &wp_sec),
        (SEC_INDEX_MAIN, &main_sec),
        (SEC_FULL_INDEX, &full_sec),
        (SEC_SUFFIX_PREFIX_POOL, &pfx_sec),
        (SEC_SUFFIX_INDEX, &sfx_sec),
        (SEC_SHORT_INDEX, &short_sec),
        (SEC_SHORT_FULL_INDEX, &short_full_sec),
        (SEC_TRAD_INDEX, &trad_sec),
    ]);
    assert_eq!(&bytes[..8], CONTAINER_MAGIC);
    println!(
        "构建耗时: {:?}  词池={}MB main={}MB full={}MB suffix={}MB short={}MB short_full={}MB trad={}MB 总容器={}MB",
        t.elapsed(),
        wp_sec.len() / 1024 / 1024,
        main_sec.len() / 1024 / 1024,
        full_sec.len() / 1024 / 1024,
        (pfx_sec.len() + sfx_sec.len()) / 1024 / 1024,
        short_sec.len() / 1024 / 1024,
        short_full_sec.len() / 1024 / 1024,
        trad_sec.len() / 1024 / 1024,
        bytes.len() / 1024 / 1024
    );
    std::fs::write(&tmp, &bytes).expect("写临时索引失败");

    let t = std::time::Instant::now();
    let view = ContainerView::open(&tmp).expect("mmap 容器打开失败");
    let b = view.backing();
    let wp_r = view.section_range(SEC_WORD_POOL).unwrap();
    let idx = MappedIndex::from_container(
        Arc::clone(b),
        wp_r,
        view.section_range(SEC_INDEX_MAIN).unwrap(),
    )
    .expect("index 打开失败");
    let full_idx = MappedIndex::from_container(
        Arc::clone(b),
        wp_r,
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
    let short_idx = MappedIndex::from_container(
        Arc::clone(b),
        wp_r,
        view.section_range(SEC_SHORT_INDEX).unwrap(),
    )
    .expect("short_index 打开失败");
    let short_full_idx = MappedIndex::from_container(
        Arc::clone(b),
        wp_r,
        view.section_range(SEC_SHORT_FULL_INDEX).unwrap(),
    )
    .expect("short_index_full 打开失败");
    let trad_idx = MappedIndex::from_container(
        Arc::clone(b),
        wp_r,
        view.section_range(SEC_TRAD_INDEX).unwrap(),
    )
    .expect("trad_index 打开失败");
    drop(view);
    assert!(
        idx.is_mmap()
            && full_idx.is_mmap()
            && sfx_idx.is_mmap()
            && short_idx.is_mmap()
            && short_full_idx.is_mmap()
            && trad_idx.is_mmap()
    );
    println!(
        "mmap 打开耗时: {:?}  entry 数: index={} full={} suffix={} short={} short_full={} trad={}",
        t.elapsed(),
        idx.entry_count(),
        full_idx.entry_count(),
        sfx_idx.entry_count(),
        short_idx.entry_count(),
        short_full_idx.entry_count(),
        trad_idx.entry_count()
    );

    let mut mismatched = 0usize;
    let missing_keys = ["zzzzzzz", "qqqq", "vvvvv", "x", "", "nihaoma"];

    let (main_k, main_e) =
        check_triple_table("index", &index_map, &idx, &missing_keys, &mut mismatched);
    let (short_k, short_e) =
        check_triple_table("short", &short_map, &short_idx, &missing_keys, &mut mismatched);
    let (sf_k, sf_e) = check_triple_table(
        "short_full",
        &short_full_map,
        &short_full_idx,
        &missing_keys,
        &mut mismatched,
    );
    let (trad_k, trad_e) =
        check_triple_table("trad", &trad_map, &trad_idx, &missing_keys, &mut mismatched);

    // full_index 全量比对 + range_from
    let mut full_k = 0usize;
    let mut full_e = 0usize;
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
                            eprintln!("✗ full[{key}][{i}]: ({},{}) vs ({},{})", w, f, r.word, r.freq);
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
        check_full(k, &mut mismatched, &mut full_k, &mut full_e);
    }
    for k in missing_keys {
        check_full(k, &mut mismatched, &mut full_k, &mut full_e);
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
            eprintln!("✗ full.range_from({start}) 不一致: {} vs {}", actual.len(), expected.len());
            mismatched += 1;
        }
    }

    // suffix_index 全量比对
    let mut sfx_k = 0usize;
    let mut sfx_e = 0usize;
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
        check_sfx(k, &mut mismatched, &mut sfx_k, &mut sfx_e);
    }
    for k in missing_keys {
        check_sfx(k, &mut mismatched, &mut sfx_k, &mut sfx_e);
    }

    for pfx in ["zhongguo", "nihao", "zh", "x"] {
        let mut expected: Vec<&str> = index_map
            .keys()
            .filter(|k| k.starts_with(pfx))
            .map(String::as_str)
            .collect();
        expected.sort_unstable();
        let actual: Vec<&str> = idx.range_prefix(pfx).map(|(k, _)| k).collect();
        if actual != expected {
            eprintln!("✗ range_prefix({pfx}) 不一致: {} vs {}", actual.len(), expected.len());
            mismatched += 1;
        }
    }

    println!();
    println!("index      比对: {main_k} key / {main_e} entry");
    println!("full       比对: {full_k} key / {full_e} entry (+ range_from)");
    println!("suffix     比对: {sfx_k} key / {sfx_e} entry");
    println!("short      比对: {short_k} key / {short_e} entry");
    println!("short_full 比对: {sf_k} key / {sf_e} entry");
    println!("trad       比对: {trad_k} key / {trad_e} entry");
    println!("不一致: {mismatched}");
    if mismatched == 0 {
        println!("✓ TSDICT06 六表全量完全一致");
    } else {
        println!("✗ 存在不一致，不可切换");
        std::process::exit(1);
    }

    drop(idx);
    drop(full_idx);
    drop(sfx_idx);
    drop(short_idx);
    drop(short_full_idx);
    drop(trad_idx);
    let _ = std::fs::remove_file(&tmp);
}
