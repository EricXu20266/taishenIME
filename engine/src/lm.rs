//! 外部语言模型（可选数据源，缺文件时整体降级）。
//!
//! 用途：给 `sentence` 的 Viterbi 提供真实 bigram 概率，替代
//! `bigram.db` 的 1~6 档位。数据文件放 `resources/lm/`：
//!   - 模型文件（外部模型容器 v2：44B 头 + 段A + 段B + TOC）
//!   - `lm_words.json`   `{"词": 词ID}` 反向映射
//!
//! 结构依据：`reference/lm-format.md`（本机格式笔记，不入库）。
//! 单位：文件内是 log10 概率；对外统一返回 log10，调用方按需换算。

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

/// 全局单例：`None` = 未加载或加载失败。
static LM: OnceLock<Option<Lm>> = OnceLock::new();

/// 开关：默认关闭。实测与现有 `bigram.db` 档位表冗余（exam 272/291 无变化），
/// `TS_LM_ON=1` 才加载 16 MB 模型做对比或后续实验。
static ENABLED: OnceLock<bool> = OnceLock::new();

fn enabled() -> bool {
    *ENABLED.get_or_init(|| std::env::var_os("TS_LM_ON").is_some())
}

struct Lm {
    w2i: HashMap<String, u32>,
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
        let raw = match std::fs::read(dir.join("ext_lm.dat")) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[lm] 读 ext_lm.dat 失败: {e}");
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

        let mut w2i = HashMap::with_capacity(map.len());
        for (w, id) in map {
            if (id as usize) < n {
                w2i.insert(w, id);
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

    fn id_of(&self, word: &str) -> Option<u32> {
        self.w2i.get(word).copied()
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
        ONCE.call_once(|| eprintln!("[lm] 未启用（需要 TS_LM_ON=1）"));
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
    let c = lm.id_of(curr)?;
    let out = match lm.id_of(prev) {
        Some(p) => lm.bigram(p, c),
        None => (lm.unigram(c), false),
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
    let lm = LM.get()?.as_ref()?;
    lm.id_of(word).map(|id| lm.unigram(id))
}

/// 前词的退避权重（log10），用于「未命中 bigram」时校正 unigram。
pub fn backoff(prev: &str) -> Option<f32> {
    let lm = LM.get()?.as_ref()?;
    lm.id_of(prev).map(|id| lm.backoff(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_files_degrade() {
        init(Path::new(r"Z:\no\such\dir"));
        assert!(!available());
        assert!(score("我", "是").is_none());
    }
}
