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
    build_container, build_container_with_header, build_index_section_pairs,
    build_index_section_triples, build_suffix_sections, build_u32_map_section, ContainerView,
    MappedIndex, MappedSuffixIndex, MappedU32Map, WordPoolBuilder, CONTAINER_MAGIC,
    DOMAIN_CONTAINER_MAGIC, DOMAIN_CONTAINER_VERSION, SEC_DOM_EXACT_SHORT, SEC_DOM_FULL,
    SEC_DOM_INDEX, SEC_DOM_META, SEC_DOM_SHORT, SEC_DOM_TRAD, SEC_DOM_WORD_DOMAIN,
    SEC_FULL_INDEX, SEC_INDEX_MAIN, SEC_SHORT_FULL_INDEX, SEC_SHORT_INDEX, SEC_SUFFIX_INDEX,
    SEC_SUFFIX_PREFIX_POOL, SEC_TRAD_INDEX, SEC_WORD_POOL,
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

fn check_pair_table(
    label: &str,
    map: &BTreeMap<String, Vec<(String, u32)>>,
    idx: &MappedIndex,
    missing_keys: &[&str],
    range_starts: &[&str],
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
                for (i, (w, f)) in va.iter().enumerate() {
                    let r = vb.get(i).unwrap();
                    if r.word != w.as_str() || r.freq != *f {
                        if *mismatched < 5 {
                            eprintln!(
                                "✗ {label}[{key}][{i}]: ({},{}) vs ({},{})",
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
    for start in range_starts {
        let expected: Vec<(&str, usize)> = map
            .range(start.to_string()..)
            .take(500)
            .map(|(k, v)| (k.as_str(), v.len()))
            .collect();
        let actual: Vec<(&str, usize)> = idx
            .range_from(start)
            .take(500)
            .map(|(k, v)| (k, v.len()))
            .collect();
        if actual != expected {
            eprintln!(
                "✗ {label}.range_from({start}) 不一致: {} vs {}",
                actual.len(),
                expected.len()
            );
            *mismatched += 1;
        }
    }
    (checked, total_entries)
}

fn dict_pinyin(pinyin: &str) -> std::borrow::Cow<'_, str> {
    if pinyin.contains('\'') {
        std::borrow::Cow::Owned(pinyin.replace('\'', ""))
    } else {
        std::borrow::Cow::Borrowed(pinyin)
    }
}

fn domain_prefix_boundaries(pinyin_raw: &str, pinyin: &str) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    let mut acc = 0usize;
    if pinyin_raw.contains('\'') {
        for part in pinyin_raw.split('\'') {
            if part.is_empty() {
                continue;
            }
            acc += part.len();
            if acc <= pinyin.len() {
                out.push(acc);
            }
        }
    } else {
        let mut rest = pinyin;
        while !rest.is_empty() {
            match taishen_engine::pinyin::split_first_syllable(rest) {
                Some((syl, tail)) => {
                    acc += syl.len();
                    if acc <= pinyin.len() {
                        out.push(acc);
                    }
                    rest = tail;
                }
                None => break,
            }
        }
    }
    if out.last().copied() != Some(pinyin.len()) {
        out.push(pinyin.len());
    }
    out
}

fn is_hot_domain(name: &str) -> bool {
    matches!(
        name,
        "network_slang"
            | "conversation"
            | "modern"
            | "computer"
            | "computer_extra"
            | "sci_extra"
            | "idiom"
            | "food"
            | "economics"
            | "sport"
            | "thuocl_chengyu"
            | "thuocl_it"
            | "game"
    )
}

fn verify_domain_container(root: &Path, missing_keys: &[&str]) -> usize {
    let dom_db = root.join("resources/domains/domains.db");
    let conn = rusqlite::Connection::open(&dom_db).expect("打开 domains.db 失败");

    let mut dom_index: HashMap<String, Vec<(String, u32, usize)>> = HashMap::new();
    let mut dom_full: BTreeMap<String, Vec<(String, u32)>> = BTreeMap::new();
    let mut dom_short: HashMap<String, Vec<(String, u32, usize)>> = HashMap::new();
    let mut dom_exact: HashMap<String, Vec<(String, u32, usize)>> = HashMap::new();
    let mut dom_trad: HashMap<String, Vec<(String, u32, usize)>> = HashMap::new();
    let mut word_domain: HashMap<String, usize> = HashMap::new();
    let mut domain_names: Vec<String> = Vec::new();
    let mut domain_heat: Vec<i64> = Vec::new();

    {
        let mut stmt = conn
            .prepare(
                "SELECT word, pinyin, domain_id, domain_name FROM domain_words ORDER BY domain_id",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, usize>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .unwrap();
        let mut seen: HashSet<(String, String)> = HashSet::with_capacity(4096);
        for (word, py_raw, domain_id, domain_name) in rows.filter_map(|x| x.ok()) {
            let word = taishen_engine::trad::to_simplified(&word);
            let py = dict_pinyin(&py_raw).into_owned();
            if !seen.insert((py.clone(), word.clone())) {
                continue;
            }
            while domain_names.len() <= domain_id {
                domain_names.push(String::new());
                domain_heat.push(0);
            }
            if domain_names[domain_id].is_empty() {
                domain_names[domain_id] = domain_name.clone();
                if is_hot_domain(&domain_name) {
                    domain_heat[domain_id] = 1;
                }
            }
            let dom_id_u32 = domain_id as u32;
            word_domain.entry(word.clone()).or_insert(domain_id);
            for &i in &domain_prefix_boundaries(&py_raw, &py) {
                dom_index.entry(py[..i].to_string()).or_default().push((
                    word.clone(),
                    dom_id_u32,
                    py.len(),
                ));
            }
            dom_full
                .entry(py.clone())
                .or_default()
                .push((word.clone(), 0));
            let (short, _) = dict_initials(&py_raw, &py);
            if !short.is_empty() {
                for i in 1..=short.len().min(4) {
                    dom_short.entry(short[..i].to_string()).or_default().push((
                        word.clone(),
                        dom_id_u32,
                        py.len(),
                    ));
                }
                dom_exact
                    .entry(short)
                    .or_default()
                    .push((word, dom_id_u32, py.len()));
            }
        }
    }
    {
        let has_trad = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='domain_words_trad'",
            )
            .map(|mut s| {
                s.query_map([], |r| r.get::<_, String>(0))
                    .map(|rows| rows.count() > 0)
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if has_trad {
            let mut stmt = conn
                .prepare("SELECT word, pinyin FROM domain_words_trad")
                .unwrap();
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .unwrap();
            for (word, py_raw) in rows.filter_map(|x| x.ok()) {
                let py = dict_pinyin(&py_raw).into_owned();
                for i in 1..=py.len() {
                    dom_trad
                        .entry(py[..i].to_string())
                        .or_default()
                        .push((word.clone(), 0, py.len()));
                }
            }
        }
    }

    for v in dom_index.values_mut() {
        v.sort_by(|a, b| a.0.chars().count().cmp(&b.0.chars().count()));
    }
    for v in dom_short.values_mut() {
        v.sort_by(|a, b| a.0.chars().count().cmp(&b.0.chars().count()));
    }
    for v in dom_exact.values_mut() {
        v.sort_by(|a, b| {
            let la = a.0.chars().count();
            let lb = b.0.chars().count();
            let ha = domain_heat.get(a.1 as usize).copied().unwrap_or(0);
            let hb = domain_heat.get(b.1 as usize).copied().unwrap_or(0);
            la.cmp(&lb).then(hb.cmp(&ha)).then(a.0.cmp(&b.0))
        });
    }

    println!();
    println!(
        "领域参考表规模: dom_index={} | dom_full={} | dom_short={} | dom_exact={} | dom_trad={} | word_domain={}",
        dom_index.len(),
        dom_full.len(),
        dom_short.len(),
        dom_exact.len(),
        dom_trad.len(),
        word_domain.len()
    );

    let tmp = std::env::temp_dir().join("taishen_domain_mapped_check_v3.bin");
    let _ = std::fs::remove_file(&tmp);

    let t = std::time::Instant::now();
    let mut wp_builder =
        WordPoolBuilder::with_capacity(word_domain.len().max(16), word_domain.len().max(16) * 10);
    let idx_sec = build_index_section_triples(&dom_index, &mut wp_builder).expect("dom_index");
    let full_sec = build_index_section_pairs(&dom_full, &mut wp_builder).expect("dom_full");
    let short_sec = build_index_section_triples(&dom_short, &mut wp_builder).expect("dom_short");
    let exact_sec = build_index_section_triples(&dom_exact, &mut wp_builder).expect("dom_exact");
    let trad_sec = build_index_section_triples(&dom_trad, &mut wp_builder).expect("dom_trad");
    let wp_sec = wp_builder.finish();
    let wd_sec = build_u32_map_section(&word_domain).expect("word_domain");
    let meta_sec = (domain_names.len() as u32).to_le_bytes();
    let bytes = build_container_with_header(
        DOMAIN_CONTAINER_MAGIC,
        DOMAIN_CONTAINER_VERSION,
        &[
            (SEC_WORD_POOL, &wp_sec),
            (SEC_DOM_INDEX, &idx_sec),
            (SEC_DOM_FULL, &full_sec),
            (SEC_DOM_SHORT, &short_sec),
            (SEC_DOM_EXACT_SHORT, &exact_sec),
            (SEC_DOM_TRAD, &trad_sec),
            (SEC_DOM_WORD_DOMAIN, &wd_sec),
            (SEC_DOM_META, &meta_sec),
        ],
    );
    println!(
        "领域构建耗时: {:?}  词池={}MB dom_index={}MB dom_full={}MB dom_short={}MB dom_exact={}MB dom_trad={}MB word_domain={}MB 总容器={:.2}MB",
        t.elapsed(),
        wp_sec.len() / 1024 / 1024,
        idx_sec.len() / 1024 / 1024,
        full_sec.len() / 1024 / 1024,
        short_sec.len() / 1024 / 1024,
        exact_sec.len() / 1024 / 1024,
        trad_sec.len() / 1024 / 1024,
        wd_sec.len() / 1024 / 1024,
        bytes.len() as f64 / 1024.0 / 1024.0
    );
    std::fs::write(&tmp, &bytes).expect("写领域临时索引失败");

    let view =
        ContainerView::open_with_header(&tmp, DOMAIN_CONTAINER_MAGIC, DOMAIN_CONTAINER_VERSION)
            .expect("领域 mmap 容器打开失败");
    let b = view.backing();
    let wp_r = view.section_range(SEC_WORD_POOL).unwrap();
    let d_idx = MappedIndex::from_container(
        Arc::clone(b),
        wp_r,
        view.section_range(SEC_DOM_INDEX).unwrap(),
    )
    .unwrap();
    let d_full = MappedIndex::from_container(
        Arc::clone(b),
        wp_r,
        view.section_range(SEC_DOM_FULL).unwrap(),
    )
    .unwrap();
    let d_short = MappedIndex::from_container(
        Arc::clone(b),
        wp_r,
        view.section_range(SEC_DOM_SHORT).unwrap(),
    )
    .unwrap();
    let d_exact = MappedIndex::from_container(
        Arc::clone(b),
        wp_r,
        view.section_range(SEC_DOM_EXACT_SHORT).unwrap(),
    )
    .unwrap();
    let d_trad = MappedIndex::from_container(
        Arc::clone(b),
        wp_r,
        view.section_range(SEC_DOM_TRAD).unwrap(),
    )
    .unwrap();
    let wd_map = MappedU32Map::from_container(
        Arc::clone(b),
        view.section_range(SEC_DOM_WORD_DOMAIN).unwrap(),
    )
    .unwrap();
    drop(view);
    assert!(
        d_idx.is_mmap()
            && d_full.is_mmap()
            && d_short.is_mmap()
            && d_exact.is_mmap()
            && d_trad.is_mmap()
            && wd_map.is_mmap()
    );

    let mut mismatched = 0usize;
    let (di_k, di_e) =
        check_triple_table("dom_index", &dom_index, &d_idx, missing_keys, &mut mismatched);
    let (ds_k, ds_e) =
        check_triple_table("dom_short", &dom_short, &d_short, missing_keys, &mut mismatched);
    let (de_k, de_e) =
        check_triple_table("dom_exact", &dom_exact, &d_exact, missing_keys, &mut mismatched);
    let (dt_k, dt_e) =
        check_triple_table("dom_trad", &dom_trad, &d_trad, missing_keys, &mut mismatched);
    let (df_k, df_e) = check_pair_table(
        "dom_full",
        &dom_full,
        &d_full,
        missing_keys,
        &["liang", "tishi", "weibo", "x", ""],
        &mut mismatched,
    );

    let mut wd_k = 0usize;
    for (w, expected_id) in &word_domain {
        wd_k += 1;
        if wd_map.get(w) != Some(*expected_id as u32) {
            if mismatched < 5 {
                eprintln!("✗ word_domain[{w}]: {:?} vs {}", wd_map.get(w), expected_id);
            }
            mismatched += 1;
        }
    }
    for k in missing_keys {
        wd_k += 1;
        if wd_map.get(k).is_some() {
            mismatched += 1;
        }
    }
    if wd_map.iter().count() != word_domain.len() {
        mismatched += 1;
    }

    println!("dom_index  比对: {di_k} key / {di_e} entry");
    println!("dom_full   比对: {df_k} key / {df_e} entry (+ range_from)");
    println!("dom_short  比对: {ds_k} key / {ds_e} entry");
    println!("dom_exact  比对: {de_k} key / {de_e} entry");
    println!("dom_trad   比对: {dt_k} key / {dt_e} entry");
    println!("word_dom   比对: {wd_k} key (含 iter 计数)");
    println!("领域不一致: {mismatched}");

    drop(d_idx);
    drop(d_full);
    drop(d_short);
    drop(d_exact);
    drop(d_trad);
    drop(wd_map);
    let _ = std::fs::remove_file(&tmp);
    mismatched
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

    let (full_k, full_e) = check_pair_table(
        "full",
        &full_map,
        &full_idx,
        &missing_keys,
        &["zhongguo", "nihao", "zh", "x", ""],
        &mut mismatched,
    );

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
    println!("系统不一致: {mismatched}");
    drop(idx);
    drop(full_idx);
    drop(sfx_idx);
    drop(short_idx);
    drop(short_full_idx);
    drop(trad_idx);
    let _ = std::fs::remove_file(&tmp);

    let dom_mismatched = verify_domain_container(root, &missing_keys);
    if mismatched + dom_mismatched == 0 {
        println!("✓ TSDICT06 系统六表 + TSDOM003 领域六表全量完全一致");
    } else {
        println!("✗ 存在不一致，不可切换");
        std::process::exit(1);
    }
}
