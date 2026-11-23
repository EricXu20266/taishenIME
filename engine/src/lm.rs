//! 语言模型概率数据（可选数据源，缺文件时整体降级）。
//!
//! 用途：给 `sentence` 的 Viterbi 提供真实 bigram 概率，替代
//! `bigram.db` 的 1~6 档位。数据文件放 `resources/lm/`：
//!   - 概率数据文件（容器 v2：44B 头 + 段A + 段B + TOC）
//!   - `lm_words.json`   `{"词": 词ID}` 反向映射
//!   - `lm_cache.bin`    零拷贝 mmap 容器缓存（`TSLM0001` v1，由引擎按需生成并映射）
//!
//! 结构依据：`reference/lm-format.md`（本机格式笔记，不入库）。
//! 单位：文件内是 log10 概率；对外统一返回 log10，调用方按需换算。

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use crate::dictionary::mapped::{
    self, Backing, ContainerView, LM_CONTAINER_MAGIC, LM_CONTAINER_VERSION, MappedU32ListMap,
    SEC_LM_META, SEC_LM_SEG_A, SEC_LM_SEG_B, SEC_LM_W2I,
};

/// 全局单例：`None` = 未加载或加载失败。
static LM: OnceLock<Option<Lm>> = OnceLock::new();

/// 开关：默认开启；设置 `TS_LM_OFF=1` 时关闭（降级测试用）。
static ENABLED: OnceLock<bool> = OnceLock::new();

/// 多音/多读音节点每词最多保留的节点 ID 上限。
const MAX_WORD_IDS: usize = 4;

/// `SEC_LM_META` 固定载荷字节数：
/// `node_count(4) + src_data_len(8) + src_words_len(8) + uni_tab(1024) + bg_tab(64) + bo_tab(1024)`。
const LM_META_LEN: usize = 4 + 8 + 8 + 256 * 4 + 16 * 4 + 256 * 4;

fn enabled() -> bool {
    *ENABLED.get_or_init(|| std::env::var_os("TS_LM_OFF").is_none())
}

/// 栈上定长词 ID 列表（零堆分配，最多容纳 4 个 ID）。
#[derive(Clone, Copy)]
struct SmallIds {
    buf: [u32; MAX_WORD_IDS],
    len: u8,
}

impl SmallIds {
    const EMPTY: Self = Self {
        buf: [0; MAX_WORD_IDS],
        len: 0,
    };

    #[inline]
    fn as_slice(&self) -> &[u32] {
        &self.buf[..self.len as usize]
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[inline]
fn decode_seg_a_entry(seg_a: &[u8], k: usize) -> (u32, u8, u8) {
    let o = k * 5;
    let v = u32::from_le_bytes([seg_a[o], seg_a[o + 1], seg_a[o + 2], seg_a[o + 3]]);
    let hi = seg_a[o + 4] as u32;
    let start = v & 0x7F_FFFF;
    let uni_idx = ((v >> 23) & 0xFF) as u8;
    let bo_idx = ((v >> 31) as u8) | (((hi & 0x7F) as u8) << 1);
    (start, uni_idx, bo_idx)
}

#[inline]
fn lookup_unigram(seg_a: &[u8], node_count: usize, uni_tab: &[f32; 256], id: u32) -> f32 {
    let i = id as usize;
    if i >= node_count {
        return -6.0;
    }
    let (_, uni_idx, _) = decode_seg_a_entry(seg_a, i);
    uni_tab[uni_idx as usize]
}

#[inline]
fn lookup_backoff(seg_a: &[u8], node_count: usize, bo_tab: &[f32; 256], id: u32) -> f32 {
    let i = id as usize;
    if i >= node_count {
        return 0.0;
    }
    let (_, _, bo_idx) = decode_seg_a_entry(seg_a, i);
    bo_tab[bo_idx as usize]
}

struct LmTables<'a> {
    seg_a: &'a [u8],
    seg_b: &'a [u8],
    node_count: usize,
    uni_tab: &'a [f32; 256],
    bg_tab: &'a [f32; 16],
    bo_tab: &'a [f32; 256],
}

fn lookup_bigram(t: LmTables<'_>, prev: u32, curr: u32) -> (f32, bool) {
    let p = prev as usize;
    if p + 1 >= t.node_count || (curr as usize) >= t.node_count {
        return (
            lookup_unigram(t.seg_a, t.node_count, t.uni_tab, curr),
            false,
        );
    }
    let max_edge = t.seg_b.len() / 3;
    let mut lo = decode_seg_a_entry(t.seg_a, p).0 as usize;
    let mut hi = (decode_seg_a_entry(t.seg_a, p + 1).0 as usize).min(max_edge);
    if hi == 0 || lo >= hi {
        return (
            lookup_unigram(t.seg_a, t.node_count, t.uni_tab, curr)
                + lookup_backoff(t.seg_a, t.node_count, t.bo_tab, prev),
            false,
        );
    }
    hi -= 1;
    while lo <= hi {
        let mid = (lo + hi) / 2;
        let o = mid * 3;
        let v =
            t.seg_b[o] as u32 | ((t.seg_b[o + 1] as u32) << 8) | ((t.seg_b[o + 2] as u32) << 16);
        let wid = v & 0xF_FFFF;
        if wid == curr {
            let bucket = ((v >> 20) & 0xF) as usize;
            return (t.bg_tab[bucket], true);
        }
        if wid < curr {
            lo = mid + 1;
        } else if mid == 0 {
            break;
        } else {
            hi = mid - 1;
        }
    }
    (
        lookup_unigram(t.seg_a, t.node_count, t.uni_tab, curr)
            + lookup_backoff(t.seg_a, t.node_count, t.bo_tab, prev),
        false,
    )
}

/// 源数据构建端临时视图（负责从 `lm_data.bin` + `lm_words.json` 生成 `TSLM0001` 容器与全量比对）。
struct SourceLm {
    raw: Vec<u8>,
    seg_a_range: (usize, usize),
    seg_b_range: (usize, usize),
    node_count: usize,
    data_len: u64,
    words_len: u64,
    uni_tab: [f32; 256],
    bg_tab: [f32; 16],
    bo_tab: [f32; 256],
    w2i: HashMap<String, Vec<u32>>,
}

impl SourceLm {
    fn load(dir: &Path) -> Option<Self> {
        let raw = match std::fs::read(dir.join("lm_data.bin")) {
            Ok(v) => v,
            Err(e) => {
                crate::log::warn(&format!("[lm] 读 lm_data.bin 失败: {e}"));
                return None;
            }
        };
        let json = match std::fs::read_to_string(dir.join("lm_words.json")) {
            Ok(v) => v,
            Err(e) => {
                crate::log::warn(&format!("[lm] 读 lm_words.json 失败: {e}"));
                return None;
            }
        };
        let data_len = raw.len() as u64;
        let words_len = json.len() as u64;
        let map: HashMap<String, u32> = match serde_json::from_str(&json) {
            Ok(v) => v,
            Err(e) => {
                crate::log::error(&format!("[lm] 解析 lm_words.json 失败: {e}"));
                return None;
            }
        };
        drop(json);

        let (seg_a_range, seg_b_range, uni_tab, bg_tab, bo_tab) = match parse_container(&raw) {
            Some(v) => v,
            None => {
                crate::log::error(&format!("[lm] 模型容器解析失败（{} 字节）", raw.len()));
                return None;
            }
        };
        let seg_a = &raw[seg_a_range.0..seg_a_range.1];
        let n = seg_a.len() / 5;
        if n == 0 {
            return None;
        }

        let mut w2i: HashMap<String, Vec<u32>> = HashMap::with_capacity(map.len());
        let mut orig_i2w: Vec<Option<String>> = vec![None; n];
        for (w, id) in map {
            if (id as usize) >= n {
                continue;
            }
            let clean: String = w.chars().filter(|&c| c >= ' ').collect();
            if clean.is_empty() {
                continue;
            }
            push_unique_id(&mut w2i, clean.clone(), id);
            if orig_i2w[id as usize].is_none() {
                orig_i2w[id as usize] = Some(clean);
            }
        }

        // 前序 Trie 邻居恢复：补回词表缺失的多音单字父节点（如 的/被/了/和/都/还/会/没/这）
        for i in 1..n.saturating_sub(2) {
            if orig_i2w[i].is_some() {
                continue;
            }
            let (Some(w1), Some(w2)) = (orig_i2w[i + 1].as_deref(), orig_i2w[i + 2].as_deref())
            else {
                continue;
            };
            let mut it1 = w1.chars();
            let mut it2 = w2.chars();
            let (Some(c1), Some(s1), Some(c2), Some(s2)) =
                (it1.next(), it1.next(), it2.next(), it2.next())
            else {
                continue;
            };
            if c1 != c2 || s1 == s2 || c1 <= '\u{7F}' {
                continue;
            }
            let deg = decode_seg_a_entry(seg_a, i + 1)
                .0
                .saturating_sub(decode_seg_a_entry(seg_a, i).0);
            let boundary = deg >= 100
                || orig_i2w[i - 1]
                    .as_deref()
                    .is_some_and(|p| !p.starts_with(c1));
            if boundary {
                push_unique_id(&mut w2i, c1.to_string(), i as u32);
            }
        }
        drop(orig_i2w);

        // 多音/多读音节点按 unigram 降序、出度降序、ID 升序排列，最多保留 4 个 ID
        for ids in w2i.values_mut() {
            if ids.len() > 1 {
                ids.sort_by(|&a, &b| {
                    let ua = lookup_unigram(seg_a, n, &uni_tab, a);
                    let ub = lookup_unigram(seg_a, n, &uni_tab, b);
                    ub.total_cmp(&ua)
                        .then_with(|| {
                            let da = out_degree(seg_a, n, a as usize);
                            let db = out_degree(seg_a, n, b as usize);
                            db.cmp(&da)
                        })
                        .then_with(|| a.cmp(&b))
                });
                ids.truncate(MAX_WORD_IDS);
            }
        }

        Some(Self {
            raw,
            seg_a_range,
            seg_b_range,
            node_count: n,
            data_len,
            words_len,
            uni_tab,
            bg_tab,
            bo_tab,
            w2i,
        })
    }

    #[inline]
    fn seg_a(&self) -> &[u8] {
        &self.raw[self.seg_a_range.0..self.seg_a_range.1]
    }

    #[inline]
    fn seg_b(&self) -> &[u8] {
        &self.raw[self.seg_b_range.0..self.seg_b_range.1]
    }

    #[inline]
    fn unigram(&self, id: u32) -> f32 {
        lookup_unigram(self.seg_a(), self.node_count, &self.uni_tab, id)
    }

    #[inline]
    fn backoff(&self, id: u32) -> f32 {
        lookup_backoff(self.seg_a(), self.node_count, &self.bo_tab, id)
    }

    fn bigram(&self, prev: u32, curr: u32) -> (f32, bool) {
        lookup_bigram(
            LmTables {
                seg_a: self.seg_a(),
                seg_b: self.seg_b(),
                node_count: self.node_count,
                uni_tab: &self.uni_tab,
                bg_tab: &self.bg_tab,
                bo_tab: &self.bo_tab,
            },
            prev,
            curr,
        )
    }

    fn to_container_bytes(&self) -> Result<Vec<u8>, String> {
        let mut meta = Vec::with_capacity(LM_META_LEN);
        let node_count_u32 =
            u32::try_from(self.node_count).map_err(|_| "LM 节点数超出 u32 上限".to_string())?;
        meta.extend_from_slice(&node_count_u32.to_le_bytes());
        meta.extend_from_slice(&self.data_len.to_le_bytes());
        meta.extend_from_slice(&self.words_len.to_le_bytes());
        for &v in &self.uni_tab {
            meta.extend_from_slice(&v.to_le_bytes());
        }
        for &v in &self.bg_tab {
            meta.extend_from_slice(&v.to_le_bytes());
        }
        for &v in &self.bo_tab {
            meta.extend_from_slice(&v.to_le_bytes());
        }

        let w2i_sec = mapped::build_u32_list_map_section(
            self.w2i.iter().map(|(k, v)| (k.as_str(), v.as_slice())),
        )?;

        Ok(mapped::build_container_with_header(
            LM_CONTAINER_MAGIC,
            LM_CONTAINER_VERSION,
            &[
                (SEC_LM_META, &meta),
                (SEC_LM_SEG_A, self.seg_a()),
                (SEC_LM_SEG_B, self.seg_b()),
                (SEC_LM_W2I, &w2i_sec),
            ],
        ))
    }
}

#[inline]
fn push_unique_id(map: &mut HashMap<String, Vec<u32>>, key: String, id: u32) {
    map.entry(key)
        .and_modify(|v| {
            if !v.contains(&id) {
                v.push(id);
            }
        })
        .or_insert_with(|| vec![id]);
}

#[inline]
fn out_degree(seg_a: &[u8], node_count: usize, idx: usize) -> u32 {
    if idx + 1 < node_count {
        decode_seg_a_entry(seg_a, idx + 1)
            .0
            .saturating_sub(decode_seg_a_entry(seg_a, idx).0)
    } else {
        0
    }
}

struct Lm {
    backing: Arc<Backing>,
    seg_a_range: (usize, usize),
    seg_b_range: (usize, usize),
    node_count: usize,
    w2i: MappedU32ListMap,
    uni_tab: [f32; 256],
    bg_tab: [f32; 16],
    bo_tab: [f32; 256],
}

impl Lm {
    fn from_container(
        view: ContainerView,
        expected_src_lens: Option<(u64, u64)>,
    ) -> Result<Self, String> {
        let meta_range = view
            .section_range(SEC_LM_META)
            .ok_or_else(|| "LM 缓存缺少元数据段".to_string())?;
        let seg_a_range = view
            .section_range(SEC_LM_SEG_A)
            .ok_or_else(|| "LM 缓存缺少段A".to_string())?;
        let seg_b_range = view
            .section_range(SEC_LM_SEG_B)
            .ok_or_else(|| "LM 缓存缺少段B".to_string())?;
        let w2i_range = view
            .section_range(SEC_LM_W2I)
            .ok_or_else(|| "LM 缓存缺少词表段".to_string())?;

        if meta_range.1 != LM_META_LEN {
            return Err(format!("LM 元数据长度异常: {}", meta_range.1));
        }

        let backing = Arc::clone(view.backing());
        let meta = &backing[meta_range.0..meta_range.0 + meta_range.1];
        let node_count = u32::from_le_bytes([meta[0], meta[1], meta[2], meta[3]]) as usize;
        if node_count == 0 {
            return Err("LM 节点数为 0".to_string());
        }
        let expected_seg_a_len = node_count
            .checked_mul(5)
            .ok_or_else(|| "LM 段A长度溢出".to_string())?;
        if seg_a_range.1 != expected_seg_a_len {
            return Err(format!(
                "LM 段A长度不匹配: {} vs {}",
                seg_a_range.1, expected_seg_a_len
            ));
        }

        let src_data_len = u64::from_le_bytes([
            meta[4], meta[5], meta[6], meta[7], meta[8], meta[9], meta[10], meta[11],
        ]);
        let src_words_len = u64::from_le_bytes([
            meta[12], meta[13], meta[14], meta[15], meta[16], meta[17], meta[18], meta[19],
        ]);
        if let Some((dl, wl)) = expected_src_lens
            && (src_data_len != dl || src_words_len != wl)
        {
            return Err("LM 源文件大小变更，需重建缓存".to_string());
        }

        let mut uni_tab = [0f32; 256];
        for (i, slot) in uni_tab.iter_mut().enumerate() {
            let o = 20 + i * 4;
            *slot = f32::from_le_bytes([meta[o], meta[o + 1], meta[o + 2], meta[o + 3]]);
        }
        let mut bg_tab = [0f32; 16];
        for (i, slot) in bg_tab.iter_mut().enumerate() {
            let o = 1044 + i * 4;
            *slot = f32::from_le_bytes([meta[o], meta[o + 1], meta[o + 2], meta[o + 3]]);
        }
        let mut bo_tab = [0f32; 256];
        for (i, slot) in bo_tab.iter_mut().enumerate() {
            let o = 1108 + i * 4;
            *slot = f32::from_le_bytes([meta[o], meta[o + 1], meta[o + 2], meta[o + 3]]);
        }

        let w2i = MappedU32ListMap::from_container(Arc::clone(&backing), w2i_range)?;
        Ok(Self {
            backing,
            seg_a_range,
            seg_b_range,
            node_count,
            w2i,
            uni_tab,
            bg_tab,
            bo_tab,
        })
    }

    fn load_from_cache(cache_path: &Path, data_len: u64, words_len: u64) -> Result<Self, String> {
        let view =
            ContainerView::open_with_header(cache_path, LM_CONTAINER_MAGIC, LM_CONTAINER_VERSION)?;
        Self::from_container(view, Some((data_len, words_len)))
    }

    fn load(dir: &Path) -> Option<Self> {
        let data_path = dir.join("lm_data.bin");
        let words_path = dir.join("lm_words.json");

        let data_meta = match std::fs::metadata(&data_path) {
            Ok(m) => m,
            Err(e) => {
                crate::log::warn(&format!("[lm] 读 lm_data.bin 失败: {e}"));
                return None;
            }
        };
        let words_meta = match std::fs::metadata(&words_path) {
            Ok(m) => m,
            Err(e) => {
                crate::log::warn(&format!("[lm] 读 lm_words.json 失败: {e}"));
                return None;
            }
        };
        let data_len = data_meta.len();
        let words_len = words_meta.len();
        let data_mtime = data_meta.modified().ok();
        let words_mtime = words_meta.modified().ok();

        let cache_path = dir.join("lm_cache.bin");
        if let Ok(cache_meta) = std::fs::metadata(&cache_path) {
            let fresh = match (cache_meta.modified().ok(), data_mtime, words_mtime) {
                (Some(ct), Some(dt), Some(wt)) => ct >= dt && ct >= wt,
                _ => false,
            };
            if fresh {
                match Self::load_from_cache(&cache_path, data_len, words_len) {
                    Ok(lm) => return Some(lm),
                    Err(e) => {
                        crate::log::warn(&format!("[lm] 缓存无效，准备重建: {e}"));
                    }
                }
            }
        }

        let src = SourceLm::load(dir)?;
        let bytes = match src.to_container_bytes() {
            Ok(b) => b,
            Err(e) => {
                crate::log::error(&format!("[lm] 构建缓存失败: {e}"));
                return None;
            }
        };
        drop(src);

        match mapped::write_bin_atomic(&cache_path, &bytes) {
            Ok(()) => {
                let backing = match Backing::open_mmap(&cache_path) {
                    Ok(b) => {
                        drop(bytes);
                        b
                    }
                    Err(_) => Backing::from_vec(bytes),
                };
                ContainerView::parse_with_header(backing, LM_CONTAINER_MAGIC, LM_CONTAINER_VERSION)
                    .and_then(|v| Self::from_container(v, Some((data_len, words_len))))
                    .map_err(|e| crate::log::error(&format!("[lm] 解析刚写出的缓存失败: {e}")))
                    .ok()
            }
            Err(e) => {
                crate::log::warn(&format!("[lm] 写 lm_cache.bin 失败（降级内存模式）: {e}"));
                ContainerView::parse_with_header(
                    Backing::from_vec(bytes),
                    LM_CONTAINER_MAGIC,
                    LM_CONTAINER_VERSION,
                )
                .and_then(|v| Self::from_container(v, Some((data_len, words_len))))
                .map_err(|err| crate::log::error(&format!("[lm] 内存解析失败: {err}")))
                .ok()
            }
        }
    }

    #[inline]
    fn seg_a(&self) -> &[u8] {
        &self.backing[self.seg_a_range.0..self.seg_a_range.0 + self.seg_a_range.1]
    }

    #[inline]
    fn seg_b(&self) -> &[u8] {
        &self.backing[self.seg_b_range.0..self.seg_b_range.0 + self.seg_b_range.1]
    }

    #[inline]
    fn unigram(&self, id: u32) -> f32 {
        lookup_unigram(self.seg_a(), self.node_count, &self.uni_tab, id)
    }

    #[inline]
    fn backoff(&self, id: u32) -> f32 {
        lookup_backoff(self.seg_a(), self.node_count, &self.bo_tab, id)
    }

    fn bigram(&self, prev: u32, curr: u32) -> (f32, bool) {
        lookup_bigram(
            LmTables {
                seg_a: self.seg_a(),
                seg_b: self.seg_b(),
                node_count: self.node_count,
                uni_tab: &self.uni_tab,
                bg_tab: &self.bg_tab,
                bo_tab: &self.bo_tab,
            },
            prev,
            curr,
        )
    }

    #[inline]
    fn ids_of(&self, word: &str) -> SmallIds {
        match self.w2i.get(word) {
            Some(view) => {
                let (buf, count) = view.to_small_array::<MAX_WORD_IDS>();
                SmallIds {
                    buf,
                    len: count as u8,
                }
            }
            None => SmallIds::EMPTY,
        }
    }

    #[inline]
    fn id_of(&self, word: &str) -> Option<u32> {
        self.w2i.get_first(word)
    }
}

type ContainerSections = (
    (usize, usize),
    (usize, usize),
    [f32; 256],
    [f32; 16],
    [f32; 256],
);

/// 从模型容器原始字节里切出段 A / 段 B 偏移范围及三张量化表。
fn parse_container(buf: &[u8]) -> Option<ContainerSections> {
    if buf.len() < 0x30 || &buf[0..4] != b"PBLM" {
        return None;
    }
    let data_len = u32::from_le_bytes([buf[0x24], buf[0x25], buf[0x26], buf[0x27]]) as usize;
    let toc_off = 0x2Cusize.checked_add(data_len)?;
    if toc_off + 8 > buf.len() {
        return None;
    }
    let mut blocks: HashMap<u32, &[u8]> = HashMap::new();
    let mut p = toc_off;
    while p + 8 <= buf.len() {
        let t = u32::from_le_bytes([buf[p], buf[p + 1], buf[p + 2], buf[p + 3]]);
        let len = u32::from_le_bytes([buf[p + 4], buf[p + 5], buf[p + 6], buf[p + 7]]) as usize;
        if p + 8 + len > buf.len() {
            break;
        }
        blocks.insert(t, &buf[p + 8..p + 8 + len]);
        p += 8 + len;
    }

    let seg6 = blocks.get(&6)?;
    if seg6.len() < 24 {
        return None;
    }
    let off = |i: usize| -> usize {
        let o = i * 8;
        u64::from_le_bytes([
            seg6[o],
            seg6[o + 1],
            seg6[o + 2],
            seg6[o + 3],
            seg6[o + 4],
            seg6[o + 5],
            seg6[o + 6],
            seg6[o + 7],
        ]) as usize
    };
    let a0 = off(0);
    let a1 = off(1);
    let b1 = off(2);
    if !(a0 < a1 && a1 <= b1 && b1 <= buf.len()) {
        return None;
    }

    let t6 = blocks.get(&12294)?;
    let t7 = blocks.get(&12295)?;
    if t6.len() < 4 + 1024 + 4 + 64 || t7.len() < 4 + 1024 {
        return None;
    }
    let mut uni_tab = [0f32; 256];
    for (i, slot) in uni_tab.iter_mut().enumerate() {
        let o = 4 + i * 4;
        *slot = f32::from_le_bytes([t6[o], t6[o + 1], t6[o + 2], t6[o + 3]]);
    }
    let mut bg_tab = [0f32; 16];
    let bg_base = 4 + 1024 + 4;
    for (i, slot) in bg_tab.iter_mut().enumerate() {
        let o = bg_base + i * 4;
        *slot = f32::from_le_bytes([t6[o], t6[o + 1], t6[o + 2], t6[o + 3]]);
    }
    let mut bo_tab = [0f32; 256];
    for (i, slot) in bo_tab.iter_mut().enumerate() {
        let o = 4 + i * 4;
        *slot = f32::from_le_bytes([t7[o], t7[o + 1], t7[o + 2], t7[o + 3]]);
    }
    Some(((a0, a1), (a1, b1), uni_tab, bg_tab, bo_tab))
}

/// 初始化（幂等）。`dir` 下缺文件时静默降级，不影响主链路。
pub fn init(dir: &Path) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    if !enabled() {
        ONCE.call_once(|| crate::log::info("[lm] 已禁用（TS_LM_OFF）"));
        return;
    }
    let lm = LM.get_or_init(|| Lm::load(dir));
    ONCE.call_once(|| match lm {
        Some(l) => {
            crate::log::info(&format!("[lm] 载入 {} 词：{}", l.w2i.len(), dir.display()));
            if std::env::var_os("TS_MEM_REPORT").is_some() {
                let is_mmap = l.backing.is_mmap();
                let mb = l.backing.len() / 1024 / 1024;
                let extra = if is_mmap {
                    format!(" (mmap={mb} MB)")
                } else {
                    String::new()
                };
                crate::log::info(&format!(
                    "[mem] {:<12} keys={:<8} est={} MB{extra}",
                    "lm",
                    l.w2i.len(),
                    if is_mmap { 0 } else { mb }
                ));
            }
        }
        None => crate::log::warn(&format!("[lm] 载入失败：{}", dir.display())),
    });
}

pub fn available() -> bool {
    enabled() && matches!(LM.get(), Some(Some(_)))
}

/// `prev → curr` 的 log10 条件概率；未知词返回 `None`。
pub fn score(prev: &str, curr: &str) -> Option<f32> {
    score_hit(prev, curr).map(|(v, _)| v)
}

/// 同 [`score`]，额外返回「是否命中 bigram」（false 时为 unigram+backoff 退避值）。
pub fn score_hit(prev: &str, curr: &str) -> Option<(f32, bool)> {
    if !enabled() {
        return None;
    }
    let lm = LM.get()?.as_ref()?;
    let cids_buf = lm.ids_of(curr);
    let cids = cids_buf.as_slice();
    if cids.is_empty() {
        return None;
    }
    let pids_buf = lm.ids_of(prev);
    let pids = pids_buf.as_slice();
    let out = if pids.is_empty() {
        (lm.unigram(cids[0]), false)
    } else {
        let mut best_hit: Option<f32> = None;
        for &p in pids {
            for &c in cids {
                let (bg, hit) = lm.bigram(p, c);
                if hit {
                    best_hit = Some(best_hit.map_or(bg, |cur| cur.max(bg)));
                }
            }
        }
        match best_hit {
            Some(bg) => (bg, true),
            None => (lm.unigram(cids[0]) + lm.backoff(pids[0]), false),
        }
    };
    if debug() {
        eprintln!("[lm] {prev} -> {curr}: {:.3} hit={}", out.0, out.1);
    }
    Some(out)
}

fn debug() -> bool {
    static D: OnceLock<bool> = OnceLock::new();
    *D.get_or_init(|| std::env::var_os("TS_LM_DEBUG").is_some())
}

/// 单字/单词语料概率（log10），未知词返回 `None`。
pub fn unigram(word: &str) -> Option<f32> {
    if !enabled() {
        return None;
    }
    let lm = LM.get()?.as_ref()?;
    lm.id_of(word).map(|id| lm.unigram(id))
}

/// 前词的退避权重（log10），用于「未命中 bigram」时校正 unigram。
pub fn backoff(prev: &str) -> Option<f32> {
    if !enabled() {
        return None;
    }
    let lm = LM.get()?.as_ref()?;
    lm.id_of(prev).map(|id| lm.backoff(id))
}

/// 直接查询 `prev -> curr` 的 bigram lift（log10 PMI = bg - uni(curr)）。
/// `prev == None` 时查询句首（BOS id=0）-> `curr`；仅命中 bigram 表时返回 `Some`。
pub fn direct_bigram_lift(prev: Option<&str>, curr: &str) -> Option<f32> {
    if !enabled() {
        return None;
    }
    let lm = LM.get()?.as_ref()?;
    let cids_buf = lm.ids_of(curr);
    let cids = cids_buf.as_slice();
    if cids.is_empty() {
        return None;
    }
    let curr_uni = lm.unigram(cids[0]);
    let pids_buf: SmallIds;
    let pids: &[u32] = match prev {
        None => &[0],
        Some(p) => {
            pids_buf = lm.ids_of(p);
            let ids = pids_buf.as_slice();
            if ids.is_empty() {
                return None;
            }
            ids
        }
    };
    let mut best_bg: Option<f32> = None;
    for &p in pids {
        for &c in cids {
            let (bg, hit) = lm.bigram(p, c);
            if hit {
                best_bg = Some(best_bg.map_or(bg, |cur| cur.max(bg)));
            }
        }
    }
    best_bg.map(|bg| bg - curr_uni)
}

/// 带前后缀退避的 `prev -> curr` bigram lift（log10）：
/// 1. 精确查询 `prev -> curr`（含句首 `prev == None`）；
/// 2. 未命中且 `prev` 为多字词时，先后退避末两字（×0.85）/ 末字（×0.75）；
/// 3. 仍未命中且 `prev` 存在、`curr` 为多字词时，退避 `curr` 首字（×0.75）。
pub fn bigram_lift(prev: Option<&str>, curr: &str) -> Option<f32> {
    if !available() {
        return None;
    }
    if let Some(lift) = direct_bigram_lift(prev, curr) {
        return Some(lift);
    }
    let p = prev?;
    let mut rev = p.char_indices().rev();
    if let Some((b1, _)) = rev.next()
        && let Some((b2, _)) = rev.next()
    {
        if rev.next().is_some()
            && let Some(lift) = direct_bigram_lift(Some(&p[b2..]), curr)
        {
            return Some(lift * 0.85);
        }
        if let Some(lift) = direct_bigram_lift(Some(&p[b1..]), curr) {
            return Some(lift * 0.75);
        }
    }
    let mut c_it = curr.chars();
    if let (Some(c0), Some(_)) = (c_it.next(), c_it.next()) {
        let mut buf = [0u8; 4];
        let c1 = c0.encode_utf8(&mut buf);
        if let Some(lift) = direct_bigram_lift(Some(p), c1) {
            return Some(lift * 0.75);
        }
    }
    None
}

/// 全量一致性校验：将 `dir` 下源文件解析结果与 `TSLM0001` mmap 容器逐词、逐概率比对。
/// 返回 `(已比对词数, 已比对 bigram 对数)`。
pub fn verify_cache_consistency(dir: &Path) -> Result<(usize, usize), String> {
    let src = SourceLm::load(dir).ok_or_else(|| "加载 LM 源文件失败".to_string())?;
    let bytes = src.to_container_bytes()?;
    let tmp_path = dir.join("lm_cache.bin.verify_tmp");
    mapped::write_bin_atomic(&tmp_path, &bytes)?;
    drop(bytes);

    let res = (|| -> Result<(usize, usize), String> {
        let lm = Lm::load_from_cache(&tmp_path, src.data_len, src.words_len)?;
        if !lm.backing.is_mmap() {
            return Err("校验句柄未启用 mmap".to_string());
        }
        verify_lm_against_source(&src, &lm)
    })();

    let _ = std::fs::remove_file(&tmp_path);
    res
}

fn verify_lm_against_source(src: &SourceLm, lm: &Lm) -> Result<(usize, usize), String> {
    if lm.w2i.len() != src.w2i.len() {
        return Err(format!(
            "LM 词表大小不一致: mmap {} vs 源 {}",
            lm.w2i.len(),
            src.w2i.len()
        ));
    }

    let mut checked_words = 0usize;
    for (word, expected_ids) in &src.w2i {
        checked_words += 1;
        let got = lm.ids_of(word);
        if got.as_slice() != expected_ids.as_slice() {
            return Err(format!(
                "LM word[{word}] ID 列表不一致: {:?} vs {:?}",
                got.as_slice(),
                expected_ids
            ));
        }
        if lm.id_of(word) != expected_ids.first().copied() {
            return Err(format!("LM word[{word}] 首 ID 不一致"));
        }
        for &id in expected_ids {
            if lm.unigram(id).to_bits() != src.unigram(id).to_bits() {
                return Err(format!("LM id={id} unigram 不一致"));
            }
            if lm.backoff(id).to_bits() != src.backoff(id).to_bits() {
                return Err(format!("LM id={id} backoff 不一致"));
            }
        }
    }

    for missing in [
        "",
        "\x01",
        "___nonexistent_lm_word___",
        "不存在的测试词条xyz",
    ] {
        if !lm.ids_of(missing).is_empty() || lm.id_of(missing).is_some() {
            return Err(format!("LM 缺失键 {missing:?} 意外命中"));
        }
    }

    let mut bigram_pairs = 0usize;
    let mut check_pair = |p: u32, c: u32| -> Result<(), String> {
        bigram_pairs += 1;
        let (ea, eh) = src.bigram(p, c);
        let (ga, gh) = lm.bigram(p, c);
        if eh != gh || ea.to_bits() != ga.to_bits() {
            return Err(format!(
                "LM bigram({p}, {c}) 不一致: ({ga}, {gh}) vs ({ea}, {eh})"
            ));
        }
        Ok(())
    };

    let sample_words = [
        "的", "了", "和", "被", "都", "还", "会", "没", "这", "中", "国", "人", "民", "中国",
        "人民", "今天", "天气", "我们", "一起",
    ];
    let mut sample_ids: Vec<u32> = Vec::new();
    for w in sample_words {
        if let Some(ids) = src.w2i.get(w) {
            for &id in ids {
                sample_ids.push(id);
                check_pair(0, id)?;
            }
        }
    }
    for &p in &sample_ids {
        for &c in &sample_ids {
            check_pair(p, c)?;
        }
    }

    let seg_a = src.seg_a();
    let seg_b = src.seg_b();
    let max_edge = seg_b.len() / 3;
    let n = src.node_count;
    let step = (n / 6500).max(1);
    for p in (0..n.saturating_sub(1)).step_by(step) {
        let lo = decode_seg_a_entry(seg_a, p).0 as usize;
        let hi = (decode_seg_a_entry(seg_a, p + 1).0 as usize).min(max_edge);
        if lo >= hi {
            continue;
        }
        let read_target = |idx: usize| -> u32 {
            let o = idx * 3;
            let v = seg_b[o] as u32 | ((seg_b[o + 1] as u32) << 8) | ((seg_b[o + 2] as u32) << 16);
            v & 0xF_FFFF
        };
        let first_c = read_target(lo);
        check_pair(p as u32, first_c)?;
        if hi - lo >= 3 {
            check_pair(p as u32, read_target(lo + (hi - lo) / 2))?;
        }
        if hi - lo >= 2 {
            check_pair(p as u32, read_target(hi - 1))?;
        }
        let miss_c = first_c.saturating_add(1);
        if (miss_c as usize) < n {
            check_pair(p as u32, miss_c)?;
        }
    }

    Ok((checked_words, bigram_pairs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_files_degrade() {
        assert!(Lm::load(Path::new(r"Z:\no\such\dir")).is_none());
    }

    /// 构造最小合法 PBLM 容器与词表，供缓存生成、mmap 重载与失效重建测试使用。
    fn write_synthetic_lm_fixture(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir).expect("create fixture dir");

        // 10 个节点：0(BOS)、1(缺词表的父节点)、2("的啊")、3("的呢")、4~8("多"的5个同词节点)、9("中国")
        let n = 10usize;
        // 边列表：
        // 0 -> [1(0), 2(1), 9(2)]       start=0..3
        // 1 -> [0..=104] (模拟 deg>=100 以触发父节点恢复) start=3..108
        // 2..=9 无出边                  start=108
        let mut edges: Vec<(u32, u8)> = vec![(1, 0), (2, 1), (9, 2)];
        for target in 0..105u32 {
            edges.push((target % (n as u32), (target % 16) as u8));
        }

        let starts: [u32; 10] = [0, 3, 108, 108, 108, 108, 108, 108, 108, 108];
        // uni_idx 设计成让 id=7 > id=8 > id=6 > id=5 > id=4 在 "多" 下按 unigram 降序截断为 4 个
        let uni_idxs: [u8; 10] = [0, 5, 1, 2, 4, 6, 8, 12, 10, 15];
        let bo_idxs: [u8; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];

        let mut seg_a = Vec::with_capacity(n * 5);
        for i in 0..n {
            let v: u32 = (starts[i] & 0x7F_FFFF)
                | ((uni_idxs[i] as u32) << 23)
                | (((bo_idxs[i] as u32) & 1) << 31);
            let hi: u8 = bo_idxs[i] >> 1;
            seg_a.extend_from_slice(&v.to_le_bytes());
            seg_a.push(hi);
        }

        let mut seg_b = Vec::with_capacity(edges.len() * 3);
        for (wid, bucket) in &edges {
            let v = (wid & 0xF_FFFF) | (((*bucket as u32) & 0xF) << 20);
            seg_b.push((v & 0xFF) as u8);
            seg_b.push(((v >> 8) & 0xFF) as u8);
            seg_b.push(((v >> 16) & 0xFF) as u8);
        }

        let mut t6 = Vec::with_capacity(4 + 1024 + 4 + 64);
        t6.extend_from_slice(&256u32.to_le_bytes());
        for i in 0..256 {
            let val = -6.0f32 + (i as f32) * 0.25;
            t6.extend_from_slice(&val.to_le_bytes());
        }
        t6.extend_from_slice(&16u32.to_le_bytes());
        for i in 0..16 {
            let val = -3.0f32 + (i as f32) * 0.2;
            t6.extend_from_slice(&val.to_le_bytes());
        }

        let mut t7 = Vec::with_capacity(4 + 1024);
        t7.extend_from_slice(&256u32.to_le_bytes());
        for i in 0..256 {
            let val = -(i as f32) * 0.1;
            t7.extend_from_slice(&val.to_le_bytes());
        }

        let data_len = (seg_a.len() + seg_b.len()) as u32;
        let a1 = 0x2C + seg_a.len();
        let b1 = 0x2C + seg_a.len() + seg_b.len();

        let mut seg6 = Vec::with_capacity(24);
        seg6.extend_from_slice(&(0x2Cu64).to_le_bytes());
        seg6.extend_from_slice(&(a1 as u64).to_le_bytes());
        seg6.extend_from_slice(&(b1 as u64).to_le_bytes());

        let mut buf = vec![0u8; 0x2C];
        buf[0..4].copy_from_slice(b"PBLM");
        buf[0x24..0x28].copy_from_slice(&data_len.to_le_bytes());
        buf.extend_from_slice(&seg_a);
        buf.extend_from_slice(&seg_b);
        for (tag, payload) in [
            (6u32, seg6.as_slice()),
            (12294u32, t6.as_slice()),
            (12295u32, t7.as_slice()),
        ] {
            buf.extend_from_slice(&tag.to_le_bytes());
            buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            buf.extend_from_slice(payload);
        }

        std::fs::write(dir.join("lm_data.bin"), buf).expect("write lm_data.bin");

        // id=1 不在 json 中，由邻居 2("的啊") / 3("的呢") + deg>=100 自动恢复为 "的"
        let words_json = concat!(
            "{",
            "\"的啊\":2,",
            "\"的呢\":3,",
            "\"多\":4,",
            "\"多\\u0001\":5,",
            "\"多\\u0002\":6,",
            "\"多\\u0003\":7,",
            "\"多\\u0004\":8,",
            "\"中国\":9,",
            "\"\\u0001\":9",
            "}"
        );
        std::fs::write(dir.join("lm_words.json"), words_json).expect("write lm_words.json");
    }

    #[test]
    fn tslm_cache_roundtrip_and_invalidation() {
        let dir = std::env::temp_dir().join("taishen_lm_tslm_roundtrip_test");
        write_synthetic_lm_fixture(&dir);
        let cache_path = dir.join("lm_cache.bin");
        assert!(!cache_path.exists());

        // 1. 首次加载：自动生成 lm_cache.bin 并转为 mmap
        let lm1 = Lm::load(&dir).expect("first load builds cache");
        assert!(cache_path.exists());
        assert!(lm1.backing.is_mmap());
        assert_eq!(lm1.id_of("的"), Some(1));
        assert_eq!(lm1.ids_of("多").as_slice(), &[7, 8, 6, 5]);
        assert!(lm1.ids_of("非词").is_empty());
        assert_eq!(lm1.bigram(0, 9).1, true);

        // 一致性全检也必须通过
        let (w_cnt, bg_cnt) = verify_cache_consistency(&dir).expect("verify consistency");
        assert_eq!(w_cnt, 5); // 的(恢复), 的啊, 的呢, 多, 中国
        assert!(bg_cnt > 0);
        drop(lm1);

        // 2. 二次加载：直接走缓存 mmap
        let lm2 = Lm::load(&dir).expect("second load hits mmap cache");
        assert!(lm2.backing.is_mmap());
        assert_eq!(lm2.ids_of("多").as_slice(), &[7, 8, 6, 5]);
        drop(lm2);

        // 3. 篡改损坏 lm_cache.bin 后自动回退源构建并覆盖修复
        std::fs::write(&cache_path, b"CORRUPTED_LM_CACHE").expect("corrupt cache");
        let lm3 = Lm::load(&dir).expect("corrupted cache rebuilds cleanly");
        assert!(lm3.backing.is_mmap());
        assert_eq!(lm3.id_of("的"), Some(1));
        assert_eq!(lm3.id_of("中国"), Some(9));
        drop(lm3);

        // 验证源大小不匹配时 load_from_cache 拒绝过期缓存
        assert!(Lm::load_from_cache(&cache_path, 1, 1).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
