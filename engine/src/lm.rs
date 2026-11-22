//! 语言模型概率数据（可选数据源，缺文件时整体降级）。
//!
//! 用途：给 `sentence` 的 Viterbi 提供真实 bigram 概率，替代
//! `bigram.db` 的 1~6 档位。数据文件放 `resources/lm/`：
//!   - 概率数据文件（容器 v2：44B 头 + 段A + 段B + TOC）
//!   - `lm_words.json`   `{"词": 词ID}` 反向映射
//!
//! 结构依据：`reference/lm-format.md`（本机格式笔记，不入库）。
//! 单位：文件内是 log10 概率；对外统一返回 log10，调用方按需换算。

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

/// 全局单例：`None` = 未加载或加载失败。
static LM: OnceLock<Option<Lm>> = OnceLock::new();

/// 开关：默认开启；设置 `TS_LM_OFF=1` 时关闭（降级测试用）。
static ENABLED: OnceLock<bool> = OnceLock::new();

fn enabled() -> bool {
    *ENABLED.get_or_init(|| std::env::var_os("TS_LM_OFF").is_none())
}

enum WordIds {
    One(u32),
    Many(Vec<u32>),
}

impl WordIds {
    fn push(&mut self, id: u32) {
        match self {
            WordIds::One(first) if *first == id => {}
            WordIds::One(first) => {
                *self = WordIds::Many(vec![*first, id]);
            }
            WordIds::Many(v) => {
                if !v.contains(&id) {
                    v.push(id);
                }
            }
        }
    }

    fn as_slice(&self) -> &[u32] {
        match self {
            WordIds::One(id) => std::slice::from_ref(id),
            WordIds::Many(v) => v.as_slice(),
        }
    }

    fn best(&self) -> u32 {
        match self {
            WordIds::One(id) => *id,
            WordIds::Many(v) => v[0],
        }
    }
}

struct Lm {
    w2i: HashMap<String, WordIds>,
    seg_b: Vec<u8>,
    start: Vec<u32>,
    uni_idx: Vec<u8>,
    bo_idx: Vec<u8>,
    uni_tab: Vec<f32>,
    bg_tab: Vec<f32>,
    bo_tab: Vec<f32>,
}

impl Lm {
    fn load(dir: &Path) -> Option<Self> {
        let raw = match std::fs::read(dir.join("lm_data.bin")) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[lm] 读 lm_data.bin 失败: {e}");
                return None;
            }
        };
        let json = match std::fs::read_to_string(dir.join("lm_words.json")) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[lm] 读 lm_words.json 失败: {e}");
                return None;
            }
        };
        let map: HashMap<String, u32> = match serde_json::from_str(&json) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[lm] 解析 lm_words.json 失败: {e}");
                return None;
            }
        };
        let (seg_a, seg_b, uni_tab, bg_tab, bo_tab) = match parse_container(&raw) {
            Some(v) => v,
            None => {
                eprintln!("[lm] 模型容器解析失败（{} 字节）", raw.len());
                return None;
            }
        };

        let n = seg_a.len() / 5;
        if n == 0 {
            return None;
        }
        let mut start = Vec::with_capacity(n);
        let mut uni_idx = Vec::with_capacity(n);
        let mut bo_idx = Vec::with_capacity(n);
        for k in 0..n {
            let o = k * 5;
            let v = u32::from_le_bytes([seg_a[o], seg_a[o + 1], seg_a[o + 2], seg_a[o + 3]]);
            let hi = seg_a[o + 4] as u32;
            start.push(v & 0x7F_FFFF);
            uni_idx.push(((v >> 23) & 0xFF) as u8);
            bo_idx.push((((v >> 31) as u8) | (((hi & 0x7F) as u8) << 1)) as u8);
        }

        let mut w2i: HashMap<String, WordIds> = HashMap::with_capacity(map.len());
        let mut orig_i2w: Vec<Option<String>> = vec![None; n];
        for (w, id) in map {
            if (id as usize) >= n {
                continue;
            }
            let clean: String = w.chars().filter(|&c| c >= ' ').collect();
            if clean.is_empty() {
                continue;
            }
            w2i.entry(clean.clone())
                .and_modify(|e| e.push(id))
                .or_insert(WordIds::One(id));
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
            let deg = start[i + 1].saturating_sub(start[i]);
            let boundary = deg >= 100
                || orig_i2w[i - 1]
                    .as_deref()
                    .is_some_and(|p| !p.starts_with(c1));
            if boundary {
                let key = c1.to_string();
                w2i.entry(key)
                    .and_modify(|e| e.push(i as u32))
                    .or_insert(WordIds::One(i as u32));
            }
        }

        // 多音/多读音节点按 unigram 降序、出度降序排列，最多保留 4 个 ID
        for ids in w2i.values_mut() {
            if let WordIds::Many(v) = ids {
                v.sort_by(|&a, &b| {
                    let ua = uni_tab[uni_idx[a as usize] as usize];
                    let ub = uni_tab[uni_idx[b as usize] as usize];
                    ub.total_cmp(&ua).then_with(|| {
                        let da = if (a as usize) + 1 < n {
                            start[a as usize + 1].saturating_sub(start[a as usize])
                        } else {
                            0
                        };
                        let db = if (b as usize) + 1 < n {
                            start[b as usize + 1].saturating_sub(start[b as usize])
                        } else {
                            0
                        };
                        db.cmp(&da)
                    })
                });
                v.truncate(4);
            }
        }

        Some(Self {
            w2i,
            seg_b,
            start,
            uni_idx,
            bo_idx,
            uni_tab,
            bg_tab,
            bo_tab,
        })
    }

    fn unigram(&self, id: u32) -> f32 {
        let i = id as usize;
        if i >= self.uni_idx.len() {
            return -6.0;
        }
        self.uni_tab[self.uni_idx[i] as usize]
    }

    fn backoff(&self, id: u32) -> f32 {
        let i = id as usize;
        if i >= self.bo_idx.len() {
            return 0.0;
        }
        self.bo_tab[self.bo_idx[i] as usize]
    }

    fn bigram(&self, prev: u32, curr: u32) -> (f32, bool) {
        let p = prev as usize;
        if p + 1 >= self.start.len() || (curr as usize) >= self.uni_idx.len() {
            return (self.unigram(curr), false);
        }
        let mut lo = self.start[p] as usize;
        let mut hi = self.start[p + 1] as usize;
        if hi == 0 || lo >= hi {
            return (self.unigram(curr) + self.backoff(prev), false);
        }
        hi -= 1;
        while lo <= hi {
            let mid = (lo + hi) / 2;
            let o = mid * 3;
            let v = self.seg_b[o] as u32
                | (self.seg_b[o + 1] as u32) << 8
                | (self.seg_b[o + 2] as u32) << 16;
            let wid = v & 0xF_FFFF;
            if wid == curr {
                let bucket = ((v >> 20) & 0xF) as usize;
                return (self.bg_tab.get(bucket).copied().unwrap_or(-6.0), true);
            }
            if wid < curr {
                lo = mid + 1;
            } else if mid == 0 {
                break;
            } else {
                hi = mid - 1;
            }
        }
        (self.unigram(curr) + self.backoff(prev), false)
    }

    fn ids_of(&self, word: &str) -> &[u32] {
        self.w2i.get(word).map(WordIds::as_slice).unwrap_or(&[])
    }

    fn id_of(&self, word: &str) -> Option<u32> {
        self.w2i.get(word).map(WordIds::best)
    }
}

/// 从模型容器原始字节里切出段 A / 段 B / 三张量化表。
fn parse_container(buf: &[u8]) -> Option<(Vec<u8>, Vec<u8>, Vec<f32>, Vec<f32>, Vec<f32>)> {
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
    let mut uni_tab = Vec::with_capacity(256);
    for i in 0..256 {
        let o = 4 + i * 4;
        uni_tab.push(f32::from_le_bytes([t6[o], t6[o + 1], t6[o + 2], t6[o + 3]]));
    }
    let mut bg_tab = Vec::with_capacity(16);
    let bg_base = 4 + 1024 + 4;
    for i in 0..16 {
        let o = bg_base + i * 4;
        bg_tab.push(f32::from_le_bytes([t6[o], t6[o + 1], t6[o + 2], t6[o + 3]]));
    }
    let mut bo_tab = Vec::with_capacity(256);
    for i in 0..256 {
        let o = 4 + i * 4;
        bo_tab.push(f32::from_le_bytes([t7[o], t7[o + 1], t7[o + 2], t7[o + 3]]));
    }
    Some((
        buf[a0..a1].to_vec(),
        buf[a1..b1].to_vec(),
        uni_tab,
        bg_tab,
        bo_tab,
    ))
}

/// 初始化（幂等）。`dir` 下缺文件时静默降级，不影响主链路。
pub fn init(dir: &Path) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    if !enabled() {
        ONCE.call_once(|| eprintln!("[lm] 已禁用（TS_LM_OFF）"));
        return;
    }
    let lm = LM.get_or_init(|| Lm::load(dir));
    ONCE.call_once(|| match lm {
        Some(l) => eprintln!("[lm] 载入 {} 词：{}", l.w2i.len(), dir.display()),
        None => eprintln!("[lm] 载入失败：{}", dir.display()),
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
    let cids = lm.ids_of(curr);
    if cids.is_empty() {
        return None;
    }
    let pids = lm.ids_of(prev);
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
    let cids = lm.ids_of(curr);
    if cids.is_empty() {
        return None;
    }
    let curr_uni = lm.unigram(cids[0]);
    let pids: &[u32] = match prev {
        None => &[0],
        Some(p) => {
            let ids = lm.ids_of(p);
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
    if let Some((b1, _)) = rev.next() {
        if let Some((b2, _)) = rev.next() {
            if rev.next().is_some() {
                if let Some(lift) = direct_bigram_lift(Some(&p[b2..]), curr) {
                    return Some(lift * 0.85);
                }
            }
            if let Some(lift) = direct_bigram_lift(Some(&p[b1..]), curr) {
                return Some(lift * 0.75);
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_files_degrade() {
        assert!(Lm::load(Path::new(r"Z:\no\such\dir")).is_none());
    }
}
