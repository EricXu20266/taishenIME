//! 词库只读索引的零拷贝容器与读取端（`TSDICT06`）。
//!
//! 特点：
//! - 单表全局字典序 + 257 槽首字节目录（支持 O(1) 缩界二分与 `range_from` / `range_prefix` 有序扫描）
//! - 多索引共享同一集中去重词池（`SEC_WORD_POOL`），词串全库仅存一份
//! - 主索引/完整拼音/简拼/繁体索引采用定长 8 字节无损词条：`word_id(24b) | pinyin_len(8b) | freq(32b)`
//! - 混合简拼后缀索引采用定长 12 字节词条：`prefix_id(32b) | word_id(32b) | freq(32b)`
//! - 统一只读视图 `Backing::{Mmap, Owned}`，保证文件映射与内存构建同路径零语义漂移

use std::collections::{BTreeMap, HashMap};
use std::ops::Deref;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use memmap2::Mmap;

/// 容器魔数（v6 格式）。
pub const CONTAINER_MAGIC: &[u8; 8] = b"TSDICT06";
/// 容器版本号。
pub const CONTAINER_VERSION: u32 = 6;

/// 容器头部长度（bytes）。
const CONTAINER_HEADER_LEN: usize = 32;
/// 容器目录项长度（bytes）。
const SECTION_DIR_LEN: usize = 24;

/// 段类型标识（系统容器 `TSDICT06`）。
pub const SEC_WORD_POOL: u32 = 1;
pub const SEC_INDEX_MAIN: u32 = 2;
pub const SEC_FULL_INDEX: u32 = 3;
pub const SEC_SUFFIX_INDEX: u32 = 4;
pub const SEC_SUFFIX_PREFIX_POOL: u32 = 5;
pub const SEC_SHORT_INDEX: u32 = 6;
pub const SEC_SHORT_FULL_INDEX: u32 = 7;
pub const SEC_TRAD_INDEX: u32 = 8;

/// 领域词库容器魔数（v3 格式）。
pub const DOMAIN_CONTAINER_MAGIC: &[u8; 8] = b"TSDOM003";
/// 领域词库容器版本号。
pub const DOMAIN_CONTAINER_VERSION: u32 = 3;

/// 段类型标识（领域容器 `TSDOM003`）。
pub const SEC_DOM_INDEX: u32 = 20;
pub const SEC_DOM_FULL: u32 = 21;
pub const SEC_DOM_SHORT: u32 = 22;
pub const SEC_DOM_EXACT_SHORT: u32 = 23;
pub const SEC_DOM_TRAD: u32 = 24;
pub const SEC_DOM_WORD_DOMAIN: u32 = 25;
pub const SEC_DOM_META: u32 = 26;

/// 24 位 word_id 上限（16,777,215）。
pub const MAX_WORD_ID: usize = 0x00FF_FFFF;

/// 只读字节底层：`Mmap`（零拷贝跨进程共享）或 `Owned`（内存构建/降级兜底）。
pub enum Backing {
    Mmap(Mmap),
    Owned(Vec<u8>),
}

impl std::fmt::Debug for Backing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mmap(m) => f.debug_tuple("Backing::Mmap").field(&m.len()).finish(),
            Self::Owned(v) => f.debug_tuple("Backing::Owned").field(&v.len()).finish(),
        }
    }
}

impl Deref for Backing {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        match self {
            Self::Mmap(m) => m,
            Self::Owned(v) => v,
        }
    }
}

impl Backing {
    /// 以只读内存映射方式打开文件。
    ///
    /// Windows 下显式指定 `FILE_SHARE_READ | FILE_SHARE_DELETE`（禁止 `FILE_SHARE_WRITE`）：
    /// 既允许多进程并发映射及安装器改名替换，又杜绝映射期间外部原地覆写触发页错误。
    pub fn open_mmap(path: &Path) -> Result<Arc<Self>, String> {
        #[cfg(windows)]
        let file = {
            use std::os::windows::fs::OpenOptionsExt;
            const FILE_SHARE_READ: u32 = 0x0000_0001;
            const FILE_SHARE_DELETE: u32 = 0x0000_0004;
            std::fs::OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
                .open(path)
                .map_err(|e| format!("打开索引失败: {e}"))?
        };
        #[cfg(not(windows))]
        let file = std::fs::File::open(path).map_err(|e| format!("打开索引失败: {e}"))?;

        let map = unsafe { Mmap::map(&file) }.map_err(|e| format!("映射索引失败: {e}"))?;
        Ok(Arc::new(Self::Mmap(map)))
    }

    pub fn from_vec(bytes: Vec<u8>) -> Arc<Self> {
        Arc::new(Self::Owned(bytes))
    }

    pub fn is_mmap(&self) -> bool {
        matches!(self, Self::Mmap(_))
    }
}

/// 将若干 `(kind, payload)` 段封装为 8 字节对齐的零拷贝容器字节流（可指定魔数与版本号）。
pub fn build_container_with_header(
    magic: &[u8; 8],
    version: u32,
    sections: &[(u32, &[u8])],
) -> Vec<u8> {
    let dir_end = CONTAINER_HEADER_LEN + sections.len() * SECTION_DIR_LEN;
    let mut cursor = align8(dir_end);
    let mut offsets: Vec<usize> = Vec::with_capacity(sections.len());
    for (_, data) in sections {
        cursor = align8(cursor);
        offsets.push(cursor);
        cursor += data.len();
    }
    let total = align8(cursor);

    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(magic);
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(&(sections.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes()); // flags
    out.extend_from_slice(&0u64.to_le_bytes()); // reserved

    for ((kind, data), &off) in sections.iter().zip(offsets.iter()) {
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved
        out.extend_from_slice(&(off as u64).to_le_bytes());
        out.extend_from_slice(&(data.len() as u64).to_le_bytes());
    }

    for ((_, data), &off) in sections.iter().zip(offsets.iter()) {
        if out.len() < off {
            out.resize(off, 0);
        }
        out.extend_from_slice(data);
    }
    if out.len() < total {
        out.resize(total, 0);
    }
    out
}

/// 将若干 `(kind, payload)` 段封装为 8 字节对齐的 `TSDICT06` 容器字节流。
pub fn build_container(sections: &[(u32, &[u8])]) -> Vec<u8> {
    build_container_with_header(CONTAINER_MAGIC, CONTAINER_VERSION, sections)
}

/// 解析后的零拷贝容器视图（支持系统 `TSDICT06` 与领域 `TSDOM003`）。
#[derive(Clone, Debug)]
pub struct ContainerView {
    backing: Arc<Backing>,
    sections: Vec<(u32, usize, usize)>,
}

impl ContainerView {
    pub fn open(path: &Path) -> Result<Self, String> {
        Self::open_with_header(path, CONTAINER_MAGIC, CONTAINER_VERSION)
    }

    pub fn open_with_header(
        path: &Path,
        expected_magic: &[u8; 8],
        expected_version: u32,
    ) -> Result<Self, String> {
        Self::parse_with_header(Backing::open_mmap(path)?, expected_magic, expected_version)
    }

    pub fn parse(backing: Arc<Backing>) -> Result<Self, String> {
        Self::parse_with_header(backing, CONTAINER_MAGIC, CONTAINER_VERSION)
    }

    pub fn parse_with_header(
        backing: Arc<Backing>,
        expected_magic: &[u8; 8],
        expected_version: u32,
    ) -> Result<Self, String> {
        let bytes: &[u8] = &backing;
        if bytes.len() < CONTAINER_HEADER_LEN || &bytes[..8] != expected_magic {
            return Err("索引不是预期的零拷贝容器格式".to_string());
        }
        let version = read_u32_at(bytes, 8);
        if version != expected_version {
            return Err(format!("索引容器版本不支持: {version}"));
        }
        let section_count = read_u32_at(bytes, 12) as usize;
        let dir_size = section_count
            .checked_mul(SECTION_DIR_LEN)
            .ok_or_else(|| "容器段目录溢出".to_string())?;
        let dir_end = CONTAINER_HEADER_LEN
            .checked_add(dir_size)
            .ok_or_else(|| "容器段目录溢出".to_string())?;
        if dir_end > bytes.len() {
            return Err("容器段目录越界".to_string());
        }

        let mut sections = Vec::with_capacity(section_count);
        for i in 0..section_count {
            let p = CONTAINER_HEADER_LEN + i * SECTION_DIR_LEN;
            let kind = read_u32_at(bytes, p);
            let off_u64 = read_u64_at(bytes, p + 8);
            let len_u64 = read_u64_at(bytes, p + 16);
            let off = usize::try_from(off_u64).map_err(|_| "段偏移溢出".to_string())?;
            let len = usize::try_from(len_u64).map_err(|_| "段长度溢出".to_string())?;
            let end = off
                .checked_add(len)
                .ok_or_else(|| "段区间溢出".to_string())?;
            if off < dir_end || end > bytes.len() {
                return Err(format!("容器段 kind={kind} 越界"));
            }
            sections.push((kind, off, len));
        }

        Ok(Self { backing, sections })
    }

    pub fn backing(&self) -> &Arc<Backing> {
        &self.backing
    }

    pub fn section_range(&self, kind: u32) -> Option<(usize, usize)> {
        self.sections
            .iter()
            .find(|&&(k, _, _)| k == kind)
            .map(|&(_, off, len)| (off, len))
    }

    pub fn section_bytes(&self, kind: u32) -> Option<&[u8]> {
        let (off, len) = self.section_range(kind)?;
        self.backing.get(off..off + len)
    }
}

/// 共享字符串池构建器（用于 `SEC_WORD_POOL` 与 `SEC_SUFFIX_PREFIX_POOL`）。
#[derive(Debug, Default)]
pub struct WordPoolBuilder {
    index: HashMap<String, u32>,
    offsets: Vec<u32>,
    bytes: Vec<u8>,
}

impl WordPoolBuilder {
    pub fn new() -> Self {
        Self {
            index: HashMap::new(),
            offsets: vec![0],
            bytes: Vec::new(),
        }
    }

    pub fn with_capacity(word_cap: usize, byte_cap: usize) -> Self {
        let mut offsets = Vec::with_capacity(word_cap + 1);
        offsets.push(0);
        Self {
            index: HashMap::with_capacity(word_cap),
            offsets,
            bytes: Vec::with_capacity(byte_cap),
        }
    }

    /// 驻留字符串并返回其唯一 ID（命中时不分配新 `String`）。
    pub fn intern(&mut self, s: &str) -> Result<u32, String> {
        if let Some(&id) = self.index.get(s) {
            return Ok(id);
        }
        let id = self.offsets.len() - 1;
        if id > MAX_WORD_ID {
            return Err(format!("字符串池条目数超出 24 位上限: {id}"));
        }
        if self
            .bytes
            .len()
            .checked_add(s.len())
            .is_none_or(|n| n > u32::MAX as usize)
        {
            return Err("字符串池字节数超出 u32 上限".to_string());
        }
        self.bytes.extend_from_slice(s.as_bytes());
        self.offsets.push(self.bytes.len() as u32);
        let id_u32 = id as u32;
        self.index.insert(s.to_string(), id_u32);
        Ok(id_u32)
    }

    /// 序列化池段载荷：`word_count(u32) + offsets([u32; word_count + 1]) + bytes`。
    pub fn finish(&self) -> Vec<u8> {
        let word_count = self.offsets.len() - 1;
        let mut out = Vec::with_capacity(4 + self.offsets.len() * 4 + self.bytes.len());
        out.extend_from_slice(&(word_count as u32).to_le_bytes());
        for v in &self.offsets {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&self.bytes);
        out
    }
}

/// 池段已校验元信息（偏移均相对于 `Backing` 起点）。
#[derive(Clone, Copy, Debug)]
struct PoolMeta {
    offsets_start: usize,
    offsets_end: usize,
    bytes_start: usize,
    bytes_end: usize,
}

impl PoolMeta {
    fn parse(all: &[u8], range: (usize, usize), max_items: usize, label: &str) -> Result<Self, String> {
        let (off, len) = range;
        let end = off
            .checked_add(len)
            .ok_or_else(|| format!("{label} 区间溢出"))?;
        let sec = all
            .get(off..end)
            .ok_or_else(|| format!("{label} 越界"))?;
        if sec.len() < 8 {
            return Err(format!("{label} 头部过短"));
        }
        let count = read_u32_at(sec, 0) as usize;
        if count > max_items {
            return Err(format!("{label} 条数超出上限: {count}"));
        }
        let offsets_len = (count + 1)
            .checked_mul(4)
            .ok_or_else(|| format!("{label} 偏移表溢出"))?;
        let offsets_start = off + 4;
        let offsets_end = offsets_start
            .checked_add(offsets_len)
            .ok_or_else(|| format!("{label} 偏移表越界"))?;
        if offsets_end > end {
            return Err(format!("{label} 偏移表越界"));
        }
        let bytes_len = read_u32_at(all, offsets_start + count * 4) as usize;
        let bytes_start = offsets_end;
        let bytes_end = bytes_start
            .checked_add(bytes_len)
            .ok_or_else(|| format!("{label} 字节区溢出"))?;
        if bytes_end > end {
            return Err(format!("{label} 字节区越界"));
        }
        Ok(Self {
            offsets_start,
            offsets_end,
            bytes_start,
            bytes_end,
        })
    }
}

/// 有序键表已校验布局（8B/12B 词条索引与 `u32` 键值映射共用）。
#[derive(Clone, Copy, Debug)]
struct KeyTableLayout {
    key_count: usize,
    dir_start: usize,
    key_offsets_start: usize,
    key_offsets_end: usize,
    key_pool_start: usize,
    key_pool_end: usize,
}

impl KeyTableLayout {
    fn parse(all: &[u8], off: usize, end: usize, label: &str) -> Result<Self, String> {
        let sec = all
            .get(off..end)
            .ok_or_else(|| format!("{label} 越界"))?;
        const DIR_BYTES: usize = 257 * 4;
        if sec.len() < 4 + DIR_BYTES + 4 {
            return Err(format!("{label} 头部过短"));
        }
        let key_count = read_u32_at(sec, 0) as usize;
        let dir_start = off + 4;

        let key_offsets_len = key_count
            .checked_add(1)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| format!("{label} 键偏移表溢出"))?;
        let key_offsets_start = dir_start + DIR_BYTES;
        let key_offsets_end = key_offsets_start
            .checked_add(key_offsets_len)
            .ok_or_else(|| format!("{label} 键偏移表越界"))?;
        if key_offsets_end > end {
            return Err(format!("{label} 键偏移表越界"));
        }

        let key_pool_len = read_u32_at(all, key_offsets_start + key_count * 4) as usize;
        let key_pool_start = key_offsets_end;
        let key_pool_end = key_pool_start
            .checked_add(key_pool_len)
            .ok_or_else(|| format!("{label} 键字节池溢出"))?;
        if key_pool_end > end {
            return Err(format!("{label} 键字节池越界"));
        }

        Ok(Self {
            key_count,
            dir_start,
            key_offsets_start,
            key_offsets_end,
            key_pool_start,
            key_pool_end,
        })
    }

    #[inline]
    fn dir_at(&self, all: &[u8], slot: usize) -> usize {
        read_u32_at(all, self.dir_start + slot * 4) as usize
    }

    #[inline]
    fn key_bytes_at<'a>(&self, all: &'a [u8], i: usize) -> &'a [u8] {
        let ko = &all[self.key_offsets_start..self.key_offsets_end];
        let kp = &all[self.key_pool_start..self.key_pool_end];
        let s = read_u32_at(ko, i * 4) as usize;
        let e = read_u32_at(ko, (i + 1) * 4) as usize;
        kp.get(s..e).unwrap_or(&[])
    }

    #[inline]
    fn find(&self, all: &[u8], key: &str) -> Option<usize> {
        if self.key_count == 0 {
            return None;
        }
        let kb = key.as_bytes();
        let (mut lo, mut hi) = match kb.first() {
            Some(&b) => {
                let b = b as usize;
                (self.dir_at(all, b), self.dir_at(all, b + 1))
            }
            None => (0, self.dir_at(all, 0)),
        };
        lo = lo.min(self.key_count);
        hi = hi.min(self.key_count);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match self.key_bytes_at(all, mid).cmp(kb) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return Some(mid),
            }
        }
        None
    }

    #[inline]
    fn lower_bound(&self, all: &[u8], from: &str) -> usize {
        if self.key_count == 0 {
            return 0;
        }
        let kb = from.as_bytes();
        let (mut lo, mut hi) = match kb.first() {
            Some(&b) => {
                let b = b as usize;
                (self.dir_at(all, b), self.dir_at(all, b + 1))
            }
            None => return 0,
        };
        lo = lo.min(self.key_count);
        hi = hi.min(self.key_count);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.key_bytes_at(all, mid) < kb {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }
}

/// 键值对有序索引段已校验布局（8B/12B 条目共用）。
#[derive(Clone, Copy, Debug)]
struct KeyEntryLayout {
    keys: KeyTableLayout,
    entry_count: usize,
    entry_offsets_start: usize,
    entry_offsets_end: usize,
    entries_start: usize,
    entries_end: usize,
}

impl KeyEntryLayout {
    fn parse(
        all: &[u8],
        range: (usize, usize),
        entry_stride: usize,
        label: &str,
    ) -> Result<Self, String> {
        let (off, len) = range;
        let end = off
            .checked_add(len)
            .ok_or_else(|| format!("{label} 区间溢出"))?;
        let keys = KeyTableLayout::parse(all, off, end, label)?;
        if keys.key_pool_end + 8 > end {
            return Err(format!("{label} 词条偏移区过短"));
        }

        let entry_count = read_u32_at(all, keys.key_pool_end) as usize;
        let entry_offsets_len = (keys.key_count + 1)
            .checked_mul(4)
            .ok_or_else(|| format!("{label} 词条偏移表溢出"))?;
        let entry_offsets_start = keys.key_pool_end + 4;
        let entry_offsets_end = entry_offsets_start
            .checked_add(entry_offsets_len)
            .ok_or_else(|| format!("{label} 词条偏移表越界"))?;
        if entry_offsets_end > end {
            return Err(format!("{label} 词条偏移表越界"));
        }

        let rel_after_eo = entry_offsets_end - off;
        let pad = (8 - (rel_after_eo % 8)) % 8;
        let entries_start = entry_offsets_end
            .checked_add(pad)
            .ok_or_else(|| format!("{label} 词条对齐溢出"))?;
        let entries_bytes_len = entry_count
            .checked_mul(entry_stride)
            .ok_or_else(|| format!("{label} 词条区溢出"))?;
        let entries_end = entries_start
            .checked_add(entries_bytes_len)
            .ok_or_else(|| format!("{label} 词条区越界"))?;
        if entries_end > end {
            return Err(format!("{label} 词条区越界"));
        }

        Ok(Self {
            keys,
            entry_count,
            entry_offsets_start,
            entry_offsets_end,
            entries_start,
            entries_end,
        })
    }

    #[inline]
    fn dir_at(&self, all: &[u8], slot: usize) -> usize {
        self.keys.dir_at(all, slot)
    }

    #[inline]
    fn key_bytes_at<'a>(&self, all: &'a [u8], i: usize) -> &'a [u8] {
        self.keys.key_bytes_at(all, i)
    }

    #[inline]
    fn find(&self, all: &[u8], key: &str) -> Option<usize> {
        self.keys.find(all, key)
    }

    #[inline]
    fn lower_bound(&self, all: &[u8], from: &str) -> usize {
        self.keys.lower_bound(all, from)
    }

    #[inline]
    fn entry_slice<'a>(&self, all: &'a [u8], key_idx: usize, stride: usize) -> (&'a [u8], usize) {
        let eo = &all[self.entry_offsets_start..self.entry_offsets_end];
        let s = read_u32_at(eo, key_idx * 4) as usize;
        let e = read_u32_at(eo, (key_idx + 1) * 4) as usize;
        let s_clamped = s.min(self.entry_count);
        let e_clamped = e.min(self.entry_count).max(s_clamped);
        let entries_all = &all[self.entries_start..self.entries_end];
        let bytes = entries_all
            .get(s_clamped * stride..e_clamped * stride)
            .unwrap_or(&[]);
        (bytes, e_clamped - s_clamped)
    }
}

fn build_first_byte_dir(sorted_keys: &[&str]) -> [u32; 257] {
    let mut dir = [sorted_keys.len() as u32; 257];
    let mut cur_b = 0usize;
    for (i, k) in sorted_keys.iter().enumerate() {
        if let Some(&b) = k.as_bytes().first() {
            let b = b as usize;
            while cur_b <= b {
                dir[cur_b] = i as u32;
                cur_b += 1;
            }
        }
    }
    dir
}

/// 通用的排序键段序列化：把有序键列表与已编码好的平坦条目字节写为一个索引段。
fn assemble_key_entry_section(
    sorted_keys: &[&str],
    key_pool: &[u8],
    key_offsets: &[u32],
    entry_offsets: &[u32],
    entry_count: usize,
    entry_bytes: &[u8],
) -> Vec<u8> {
    let key_count = sorted_keys.len();
    let dir = build_first_byte_dir(sorted_keys);

    let header_and_pools =
        4 + dir.len() * 4 + key_offsets.len() * 4 + key_pool.len() + 4 + entry_offsets.len() * 4;
    let aligned_entries = align8(header_and_pools);
    let mut out = Vec::with_capacity(aligned_entries + entry_bytes.len());
    out.extend_from_slice(&(key_count as u32).to_le_bytes());
    for v in &dir {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for v in key_offsets {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(key_pool);
    out.extend_from_slice(&(entry_count as u32).to_le_bytes());
    for v in entry_offsets {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.resize(aligned_entries, 0);
    out.extend_from_slice(entry_bytes);
    out
}

/// 将 `(word, freq, pinyin_len)` 索引构建为 `SEC_INDEX_MAIN` 字节流，词串写入共享 `pool`。
pub fn build_index_section_triples(
    map: &HashMap<String, Vec<(String, u32, usize)>>,
    pool: &mut WordPoolBuilder,
) -> Result<Vec<u8>, String> {
    let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
    keys.sort_unstable();

    let mut key_offsets: Vec<u32> = Vec::with_capacity(keys.len() + 1);
    key_offsets.push(0);
    let mut key_pool: Vec<u8> = Vec::with_capacity(keys.len() * 9);
    let mut entry_offsets: Vec<u32> = Vec::with_capacity(keys.len() + 1);
    entry_offsets.push(0);
    let total_est: usize = map.values().map(Vec::len).sum();
    let mut entry_bytes: Vec<u8> = Vec::with_capacity(total_est * 8);
    let mut entry_count = 0usize;

    for &k in &keys {
        if key_pool
            .len()
            .checked_add(k.len())
            .is_none_or(|n| n > u32::MAX as usize)
        {
            return Err("键字节池超出 u32 上限".to_string());
        }
        key_pool.extend_from_slice(k.as_bytes());
        key_offsets.push(key_pool.len() as u32);

        if let Some(list) = map.get(k) {
            for (w, freq, py_len) in list {
                if *py_len > u8::MAX as usize {
                    return Err(format!("词条拼音长度超出 255: {py_len} ({w})"));
                }
                let word_id = pool.intern(w)?;
                let packed = (word_id as u64)
                    | ((*py_len as u64) << 24)
                    | ((*freq as u64) << 32);
                entry_bytes.extend_from_slice(&packed.to_le_bytes());
                entry_count += 1;
            }
        }
        if entry_count > u32::MAX as usize {
            return Err("词条总数超出 u32 上限".to_string());
        }
        entry_offsets.push(entry_count as u32);
    }

    Ok(assemble_key_entry_section(
        &keys,
        &key_pool,
        &key_offsets,
        &entry_offsets,
        entry_count,
        &entry_bytes,
    ))
}

/// 将完整拼音 `(word, freq)` 索引构建为 `SEC_FULL_INDEX` 字节流，词串写入共享 `pool`。
pub fn build_index_section_pairs(
    map: &BTreeMap<String, Vec<(String, u32)>>,
    pool: &mut WordPoolBuilder,
) -> Result<Vec<u8>, String> {
    let keys: Vec<&str> = map.keys().map(String::as_str).collect();
    let mut key_offsets: Vec<u32> = Vec::with_capacity(keys.len() + 1);
    key_offsets.push(0);
    let mut key_pool: Vec<u8> = Vec::with_capacity(keys.len() * 10);
    let mut entry_offsets: Vec<u32> = Vec::with_capacity(keys.len() + 1);
    entry_offsets.push(0);
    let total_est: usize = map.values().map(Vec::len).sum();
    let mut entry_bytes: Vec<u8> = Vec::with_capacity(total_est * 8);
    let mut entry_count = 0usize;

    for (k, list) in map {
        if key_pool
            .len()
            .checked_add(k.len())
            .is_none_or(|n| n > u32::MAX as usize)
        {
            return Err("完整拼音键字节池超出 u32 上限".to_string());
        }
        key_pool.extend_from_slice(k.as_bytes());
        key_offsets.push(key_pool.len() as u32);

        let py_len = k.len().min(u8::MAX as usize);
        for (w, freq) in list {
            let word_id = pool.intern(w)?;
            let packed = (word_id as u64)
                | ((py_len as u64) << 24)
                | ((*freq as u64) << 32);
            entry_bytes.extend_from_slice(&packed.to_le_bytes());
            entry_count += 1;
        }
        if entry_count > u32::MAX as usize {
            return Err("完整拼音词条总数超出 u32 上限".to_string());
        }
        entry_offsets.push(entry_count as u32);
    }

    Ok(assemble_key_entry_section(
        &keys,
        &key_pool,
        &key_offsets,
        &entry_offsets,
        entry_count,
        &entry_bytes,
    ))
}

/// 将 `suffix_index` 构建为 `(SEC_SUFFIX_PREFIX_POOL, SEC_SUFFIX_INDEX)` 两段，词串写入共享 `word_pool`。
pub fn build_suffix_sections(
    map: &HashMap<String, Vec<(String, String, u32)>>,
    word_pool: &mut WordPoolBuilder,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
    keys.sort_unstable();

    let mut prefix_pool = WordPoolBuilder::new();
    let mut key_offsets: Vec<u32> = Vec::with_capacity(keys.len() + 1);
    key_offsets.push(0);
    let mut key_pool: Vec<u8> = Vec::with_capacity(keys.len() * 9);
    let mut entry_offsets: Vec<u32> = Vec::with_capacity(keys.len() + 1);
    entry_offsets.push(0);
    let total_est: usize = map.values().map(Vec::len).sum();
    let mut entry_bytes: Vec<u8> = Vec::with_capacity(total_est * 12);
    let mut entry_count = 0usize;

    for &k in &keys {
        if key_pool
            .len()
            .checked_add(k.len())
            .is_none_or(|n| n > u32::MAX as usize)
        {
            return Err("后缀键字节池超出 u32 上限".to_string());
        }
        key_pool.extend_from_slice(k.as_bytes());
        key_offsets.push(key_pool.len() as u32);

        if let Some(list) = map.get(k) {
            for (pfx_inits, word, freq) in list {
                let pfx_id = prefix_pool.intern(pfx_inits)?;
                let word_id = word_pool.intern(word)?;
                entry_bytes.extend_from_slice(&pfx_id.to_le_bytes());
                entry_bytes.extend_from_slice(&word_id.to_le_bytes());
                entry_bytes.extend_from_slice(&freq.to_le_bytes());
                entry_count += 1;
            }
        }
        if entry_count > u32::MAX as usize {
            return Err("后缀词条总数超出 u32 上限".to_string());
        }
        entry_offsets.push(entry_count as u32);
    }

    let suffix_sec = assemble_key_entry_section(
        &keys,
        &key_pool,
        &key_offsets,
        &entry_offsets,
        entry_count,
        &entry_bytes,
    );
    Ok((prefix_pool.finish(), suffix_sec))
}

/// 词条借用视图（零分配）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryRef<'a> {
    pub word: &'a str,
    pub freq: u32,
    pub pinyin_len: usize,
}

/// 某个前缀或完整拼音下的全部词条切片视图。
#[derive(Clone, Copy)]
pub struct EntriesView<'a> {
    entry_bytes: &'a [u8],
    count: usize,
    wp_offsets: &'a [u8],
    wp_bytes: &'a [u8],
}

impl<'a> EntriesView<'a> {
    #[inline]
    pub fn len(&self) -> usize {
        self.count
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    #[inline]
    pub fn get(&self, i: usize) -> Option<EntryRef<'a>> {
        if i >= self.count {
            return None;
        }
        let o = i * 8;
        let b = self.entry_bytes.get(o..o + 8)?;
        let raw = u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
        let word_id = (raw & 0x00FF_FFFF) as usize;
        let pinyin_len = ((raw >> 24) & 0xFF) as usize;
        let freq = (raw >> 32) as u32;
        let word = lookup_pool_str(self.wp_offsets, self.wp_bytes, word_id);
        Some(EntryRef {
            word,
            freq,
            pinyin_len,
        })
    }

    /// 精确匹配词串并返回词频：先比偏移长度与原始字节切片，跳过 `std::str::from_utf8`。
    #[inline]
    pub fn find_word_freq(&self, target: &str) -> Option<u32> {
        let tb = target.as_bytes();
        let tlen = tb.len();
        for b in self.entry_bytes.chunks_exact(8) {
            let raw = u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
            let word_id = (raw & 0x00FF_FFFF) as usize;
            let s = read_u32_at(self.wp_offsets, word_id * 4) as usize;
            let e = read_u32_at(self.wp_offsets, (word_id + 1) * 4) as usize;
            if e >= s && e - s == tlen && self.wp_bytes.get(s..e) == Some(tb) {
                return Some((raw >> 32) as u32);
            }
        }
        None
    }

    #[inline]
    pub fn iter(&self) -> EntriesIter<'a> {
        EntriesIter {
            view: *self,
            pos: 0,
        }
    }

    /// 预过滤拼音长度的迭代器：不匹配条目不访问词池与 UTF-8 校验。
    #[inline]
    pub fn iter_with_pinyin_len(&self, target_len: usize) -> FilteredEntriesIter<'a> {
        FilteredEntriesIter {
            view: *self,
            pos: 0,
            filter: EntryFilter::PinyinLen(target_len),
        }
    }

    /// 预过滤排除指定拼音长度的迭代器：精确匹配条目不访问词池与 UTF-8 校验。
    #[inline]
    pub fn iter_without_pinyin_len(&self, exclude_len: usize) -> FilteredEntriesIter<'a> {
        FilteredEntriesIter {
            view: *self,
            pos: 0,
            filter: EntryFilter::NotPinyinLen(exclude_len),
        }
    }

    /// 预过滤词串 UTF-8 字节长度区间的迭代器：先检查偏移差，不匹配条目不读 `wp_bytes` 与 UTF-8 校验。
    #[inline]
    pub fn iter_word_byte_len_range(
        &self,
        min_bytes: usize,
        max_bytes: usize,
    ) -> FilteredEntriesIter<'a> {
        FilteredEntriesIter {
            view: *self,
            pos: 0,
            filter: EntryFilter::WordByteRange(min_bytes, max_bytes),
        }
    }
}

#[derive(Clone, Copy)]
enum EntryFilter {
    PinyinLen(usize),
    NotPinyinLen(usize),
    WordByteRange(usize, usize),
}

pub struct FilteredEntriesIter<'a> {
    view: EntriesView<'a>,
    pos: usize,
    filter: EntryFilter,
}

impl<'a> Iterator for FilteredEntriesIter<'a> {
    type Item = EntryRef<'a>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        while self.pos < self.view.count {
            let o = self.pos * 8;
            self.pos += 1;
            let b = self.view.entry_bytes.get(o..o + 8)?;
            let raw = u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
            let pinyin_len = ((raw >> 24) & 0xFF) as usize;
            let word_id = (raw & 0x00FF_FFFF) as usize;
            let word = match self.filter {
                EntryFilter::PinyinLen(target_len) => {
                    if pinyin_len != target_len {
                        continue;
                    }
                    lookup_pool_str(self.view.wp_offsets, self.view.wp_bytes, word_id)
                }
                EntryFilter::NotPinyinLen(exclude_len) => {
                    if pinyin_len == exclude_len {
                        continue;
                    }
                    lookup_pool_str(self.view.wp_offsets, self.view.wp_bytes, word_id)
                }
                EntryFilter::WordByteRange(min_b, max_b) => {
                    let s = read_u32_at(self.view.wp_offsets, word_id * 4) as usize;
                    let e = read_u32_at(self.view.wp_offsets, (word_id + 1) * 4) as usize;
                    if e < s || !(min_b..=max_b).contains(&(e - s)) {
                        continue;
                    }
                    self.view
                        .wp_bytes
                        .get(s..e)
                        .and_then(|bytes| std::str::from_utf8(bytes).ok())
                        .unwrap_or("")
                }
            };
            let freq = (raw >> 32) as u32;
            return Some(EntryRef {
                word,
                freq,
                pinyin_len,
            });
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.view.count.saturating_sub(self.pos)))
    }
}

pub struct EntriesIter<'a> {
    view: EntriesView<'a>,
    pos: usize,
}

impl<'a> Iterator for EntriesIter<'a> {
    type Item = EntryRef<'a>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let item = self.view.get(self.pos)?;
        self.pos += 1;
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let rem = self.view.count.saturating_sub(self.pos);
        (rem, Some(rem))
    }
}

impl<'a> ExactSizeIterator for EntriesIter<'a> {}

/// 有序前缀范围扫描迭代器（只产出以 `prefix` 开头的键）。
pub struct RangeIter<'a> {
    index: &'a MappedIndex,
    pos: usize,
    end: usize,
    prefix: &'a [u8],
}

impl<'a> Iterator for RangeIter<'a> {
    type Item = (&'a str, EntriesView<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.end {
            return None;
        }
        let kb = self.index.key_bytes_at(self.pos);
        if !kb.starts_with(self.prefix) {
            self.pos = self.end;
            return None;
        }
        let key = std::str::from_utf8(kb).ok()?;
        let view = self.index.entries_of(self.pos);
        self.pos += 1;
        Some((key, view))
    }
}

/// 惰性词条句柄：仅记录键下标，命中过滤条件后再读取 `entry_offsets` 并构造 `EntriesView`。
#[derive(Clone, Copy)]
pub struct LazyEntries<'a> {
    index: &'a MappedIndex,
    key_idx: usize,
}

impl<'a> LazyEntries<'a> {
    #[inline]
    pub fn entries(self) -> EntriesView<'a> {
        self.index.entries_of(self.key_idx)
    }
}

/// 有序下界扫描迭代器（等价 `BTreeMap::range(from..)`，直到键尾）。
pub struct RangeFromIter<'a> {
    index: &'a MappedIndex,
    pos: usize,
    end: usize,
}

impl<'a> Iterator for RangeFromIter<'a> {
    type Item = (&'a str, EntriesView<'a>);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.end {
            return None;
        }
        let key = std::str::from_utf8(self.index.key_bytes_at(self.pos)).ok()?;
        let view = self.index.entries_of(self.pos);
        self.pos += 1;
        Some((key, view))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let rem = self.end.saturating_sub(self.pos);
        (rem, Some(rem))
    }
}

impl<'a> ExactSizeIterator for RangeFromIter<'a> {}

/// 惰性下界扫描迭代器：迭代键时零 `entry_offsets` 访存，跳过不匹配键成本极低。
pub struct LazyRangeFromIter<'a> {
    index: &'a MappedIndex,
    ko: &'a [u8],
    kp: &'a [u8],
    pos: usize,
    end: usize,
    cur_off: usize,
}

impl<'a> Iterator for LazyRangeFromIter<'a> {
    type Item = (&'a str, LazyEntries<'a>);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.end {
            return None;
        }
        let next_off = read_u32_at(self.ko, (self.pos + 1) * 4) as usize;
        let kb = self.kp.get(self.cur_off..next_off).unwrap_or(&[]);
        self.cur_off = next_off;
        let key = std::str::from_utf8(kb).ok()?;
        let lazy = LazyEntries {
            index: self.index,
            key_idx: self.pos,
        };
        self.pos += 1;
        Some((key, lazy))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let rem = self.end.saturating_sub(self.pos);
        (rem, Some(rem))
    }
}

impl<'a> ExactSizeIterator for LazyRangeFromIter<'a> {}

/// 单表全局有序零拷贝索引（支持 `Mmap` 与 `Owned` 两种底层，主索引与完整拼音索引共用）。
#[derive(Clone, Debug)]
pub struct MappedIndex {
    backing: Arc<Backing>,
    wp_range: (usize, usize),
    idx_range: (usize, usize),
    wp: PoolMeta,
    layout: KeyEntryLayout,
}

impl Default for MappedIndex {
    fn default() -> Self {
        Self::empty()
    }
}

impl MappedIndex {
    /// 返回共享空索引（可用于 `Dictionary::shell()` 等零词条场景，`to_bin` 仍可写出合法空段）。
    pub fn empty() -> Self {
        static EMPTY: OnceLock<MappedIndex> = OnceLock::new();
        EMPTY
            .get_or_init(|| {
                let empty_map: HashMap<String, Vec<(String, u32, usize)>> = HashMap::new();
                Self::from_map(&empty_map).expect("构建空 MappedIndex 不应失败")
            })
            .clone()
    }

    /// 直接从磁盘上的 `TSDICT06` 容器文件打开并映射主索引。
    pub fn open(path: &Path) -> Result<Self, String> {
        let container = ContainerView::open(path)?;
        let wp_range = container
            .section_range(SEC_WORD_POOL)
            .ok_or_else(|| "容器缺少 SEC_WORD_POOL".to_string())?;
        let idx_range = container
            .section_range(SEC_INDEX_MAIN)
            .ok_or_else(|| "容器缺少 SEC_INDEX_MAIN".to_string())?;
        Self::from_container(Arc::clone(container.backing()), wp_range, idx_range)
    }

    /// 从已解析好的容器区间构造 `MappedIndex`（打开时一次完成偏移校验）。
    pub fn from_container(
        backing: Arc<Backing>,
        wp_range: (usize, usize),
        idx_range: (usize, usize),
    ) -> Result<Self, String> {
        let all: &[u8] = &backing;
        let wp = PoolMeta::parse(all, wp_range, MAX_WORD_ID, "词池段")?;
        let layout = KeyEntryLayout::parse(all, idx_range, 8, "有序索引段")?;
        Ok(Self {
            backing,
            wp_range,
            idx_range,
            wp,
            layout,
        })
    }

    /// 从内存 `(word, freq, pinyin_len)` 映射构建紧凑单表索引并打包为 `Backing::Owned`。
    pub fn from_map(map: &HashMap<String, Vec<(String, u32, usize)>>) -> Result<Self, String> {
        let mut pool = WordPoolBuilder::with_capacity(map.len() / 2, map.len() * 4);
        let idx_sec = build_index_section_triples(map, &mut pool)?;
        let wp_sec = pool.finish();
        let container_bytes =
            build_container(&[(SEC_WORD_POOL, &wp_sec), (SEC_INDEX_MAIN, &idx_sec)]);
        let backing = Backing::from_vec(container_bytes);
        let container = ContainerView::parse(Arc::clone(&backing))?;
        let wp_range = container
            .section_range(SEC_WORD_POOL)
            .ok_or_else(|| "内部容器缺少词池段".to_string())?;
        let idx_range = container
            .section_range(SEC_INDEX_MAIN)
            .ok_or_else(|| "内部容器缺少主索引段".to_string())?;
        Self::from_container(backing, wp_range, idx_range)
    }

    /// 从完整拼音 `(word, freq)` 有序表构建紧凑单表索引并打包为 `Backing::Owned`。
    pub fn from_pairs(map: &BTreeMap<String, Vec<(String, u32)>>) -> Result<Self, String> {
        let mut pool = WordPoolBuilder::with_capacity(map.len(), map.len() * 8);
        let idx_sec = build_index_section_pairs(map, &mut pool)?;
        let wp_sec = pool.finish();
        let container_bytes =
            build_container(&[(SEC_WORD_POOL, &wp_sec), (SEC_FULL_INDEX, &idx_sec)]);
        let backing = Backing::from_vec(container_bytes);
        let container = ContainerView::parse(Arc::clone(&backing))?;
        let wp_range = container
            .section_range(SEC_WORD_POOL)
            .ok_or_else(|| "内部容器缺少词池段".to_string())?;
        let idx_range = container
            .section_range(SEC_FULL_INDEX)
            .ok_or_else(|| "内部容器缺少完整拼音索引段".to_string())?;
        Self::from_container(backing, wp_range, idx_range)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.layout.keys.key_count
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.layout.keys.key_count == 0
    }

    #[inline]
    pub fn entry_count(&self) -> usize {
        self.layout.entry_count
    }

    #[inline]
    pub fn is_mmap(&self) -> bool {
        self.backing.is_mmap()
    }

    #[inline]
    pub fn backing(&self) -> &Arc<Backing> {
        &self.backing
    }

    #[inline]
    pub fn wp_range(&self) -> (usize, usize) {
        self.wp_range
    }

    #[inline]
    pub fn shares_word_pool(&self, other_backing: &Arc<Backing>, other_wp_range: (usize, usize)) -> bool {
        Arc::ptr_eq(&self.backing, other_backing) && self.wp_range == other_wp_range
    }

    /// 词池段 + 索引段字节数（不含容器头与其他段）。
    #[inline]
    pub fn byte_size(&self) -> usize {
        self.wp_range.1 + self.idx_range.1
    }

    /// 仅当前索引段自身的字节数（多索引共享词池时避免重复计入词池）。
    #[inline]
    pub fn index_only_byte_size(&self) -> usize {
        self.idx_range.1
    }

    #[inline]
    pub fn raw_word_pool_bytes(&self) -> &[u8] {
        &self.backing[self.wp_range.0..self.wp_range.0 + self.wp_range.1]
    }

    #[inline]
    pub fn raw_index_bytes(&self) -> &[u8] {
        &self.backing[self.idx_range.0..self.idx_range.0 + self.idx_range.1]
    }

    #[inline]
    fn key_bytes_at(&self, i: usize) -> &[u8] {
        self.layout.key_bytes_at(&self.backing, i)
    }

    #[inline]
    fn entries_of(&self, key_idx: usize) -> EntriesView<'_> {
        let all: &[u8] = &self.backing;
        let (entry_bytes, count) = self.layout.entry_slice(all, key_idx, 8);
        EntriesView {
            entry_bytes,
            count,
            wp_offsets: &all[self.wp.offsets_start..self.wp.offsets_end],
            wp_bytes: &all[self.wp.bytes_start..self.wp.bytes_end],
        }
    }

    /// 精确查找：首字节目录缩界 + 字节切片二分（零分配）。
    pub fn get(&self, key: &str) -> Option<EntriesView<'_>> {
        let idx = self.layout.find(&self.backing, key)?;
        Some(self.entries_of(idx))
    }

    /// 有序前缀扫描：二分定位首个 `>= prefix` 的键，顺序遍历直至不再以 `prefix` 开头。
    pub fn range_prefix<'a>(&'a self, prefix: &'a str) -> RangeIter<'a> {
        let kb = prefix.as_bytes();
        let key_count = self.layout.keys.key_count;
        if key_count == 0 {
            return RangeIter {
                index: self,
                pos: 0,
                end: 0,
                prefix: kb,
            };
        }
        let all: &[u8] = &self.backing;
        let (mut lo, hi) = match kb.first() {
            Some(&b) => {
                let b = b as usize;
                (self.layout.dir_at(all, b), self.layout.dir_at(all, b + 1))
            }
            None => (0, key_count),
        };
        lo = lo.min(key_count);
        let mut r = hi.min(key_count);
        let end = r;
        while lo < r {
            let mid = lo + (r - lo) / 2;
            if self.layout.key_bytes_at(all, mid) < kb {
                lo = mid + 1;
            } else {
                r = mid;
            }
        }
        RangeIter {
            index: self,
            pos: lo,
            end,
            prefix: kb,
        }
    }

    /// 有序下界扫描：二分定位首个 `>= from` 的键，顺序遍历至末尾（等价 `BTreeMap::range(from..)`）。
    pub fn range_from(&self, from: &str) -> RangeFromIter<'_> {
        let pos = self.layout.lower_bound(&self.backing, from);
        RangeFromIter {
            index: self,
            pos,
            end: self.layout.keys.key_count,
        }
    }

    /// 惰性有序下界扫描：迭代键时零 `entry_offsets` 访存，命中后再通过 [`LazyEntries::entries`] 展开词条。
    pub fn range_from_lazy(&self, from: &str) -> LazyRangeFromIter<'_> {
        let pos = self.layout.lower_bound(&self.backing, from);
        let end = self.layout.keys.key_count;
        let all: &[u8] = &self.backing;
        let ko = &all[self.layout.keys.key_offsets_start..self.layout.keys.key_offsets_end];
        let kp = &all[self.layout.keys.key_pool_start..self.layout.keys.key_pool_end];
        let cur_off = if pos < end {
            read_u32_at(ko, pos * 4) as usize
        } else {
            0
        };
        LazyRangeFromIter {
            index: self,
            ko,
            kp,
            pos,
            end,
            cur_off,
        }
    }

    /// 全表有序迭代（字典序）。
    pub fn iter(&self) -> RangeFromIter<'_> {
        RangeFromIter {
            index: self,
            pos: 0,
            end: self.layout.keys.key_count,
        }
    }
}

/// 混合简拼后缀索引词条借用视图（零分配）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SuffixEntryRef<'a> {
    pub prefix_initials: &'a str,
    pub word: &'a str,
    pub freq: u32,
}

/// 某个拼音后缀下的全部词条切片视图。
#[derive(Clone, Copy)]
pub struct SuffixEntriesView<'a> {
    entry_bytes: &'a [u8],
    count: usize,
    pfx_offsets: &'a [u8],
    pfx_bytes: &'a [u8],
    wp_offsets: &'a [u8],
    wp_bytes: &'a [u8],
}

impl<'a> SuffixEntriesView<'a> {
    #[inline]
    pub fn len(&self) -> usize {
        self.count
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    #[inline]
    pub fn get(&self, i: usize) -> Option<SuffixEntryRef<'a>> {
        if i >= self.count {
            return None;
        }
        let o = i * 12;
        let b = self.entry_bytes.get(o..o + 12)?;
        let pfx_id = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
        let word_id = u32::from_le_bytes([b[4], b[5], b[6], b[7]]) as usize;
        let freq = u32::from_le_bytes([b[8], b[9], b[10], b[11]]);
        Some(SuffixEntryRef {
            prefix_initials: lookup_pool_str(self.pfx_offsets, self.pfx_bytes, pfx_id),
            word: lookup_pool_str(self.wp_offsets, self.wp_bytes, word_id),
            freq,
        })
    }

    #[inline]
    pub fn iter(&self) -> SuffixEntriesIter<'a> {
        SuffixEntriesIter {
            view: *self,
            pos: 0,
        }
    }
}

pub struct SuffixEntriesIter<'a> {
    view: SuffixEntriesView<'a>,
    pos: usize,
}

impl<'a> Iterator for SuffixEntriesIter<'a> {
    type Item = SuffixEntryRef<'a>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let item = self.view.get(self.pos)?;
        self.pos += 1;
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let rem = self.view.count.saturating_sub(self.pos);
        (rem, Some(rem))
    }
}

impl<'a> ExactSizeIterator for SuffixEntriesIter<'a> {}

/// `MappedSuffixIndex` 全表迭代器。
pub struct SuffixIter<'a> {
    index: &'a MappedSuffixIndex,
    pos: usize,
    end: usize,
}

impl<'a> Iterator for SuffixIter<'a> {
    type Item = (&'a str, SuffixEntriesView<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.end {
            return None;
        }
        let key = std::str::from_utf8(self.index.layout.key_bytes_at(&self.index.backing, self.pos))
            .ok()?;
        let view = self.index.entries_of(self.pos);
        self.pos += 1;
        Some((key, view))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let rem = self.end.saturating_sub(self.pos);
        (rem, Some(rem))
    }
}

impl<'a> ExactSizeIterator for SuffixIter<'a> {}

/// 混合简拼后缀只读零拷贝索引（12 字节词条：声母前缀 ID + 词串 ID + 词频）。
#[derive(Clone, Debug)]
pub struct MappedSuffixIndex {
    backing: Arc<Backing>,
    wp_range: (usize, usize),
    pfx_pool_range: (usize, usize),
    idx_range: (usize, usize),
    wp: PoolMeta,
    pfx_pool: PoolMeta,
    layout: KeyEntryLayout,
}

impl Default for MappedSuffixIndex {
    fn default() -> Self {
        Self::empty()
    }
}

impl MappedSuffixIndex {
    pub fn empty() -> Self {
        static EMPTY: OnceLock<MappedSuffixIndex> = OnceLock::new();
        EMPTY
            .get_or_init(|| {
                let empty_map: HashMap<String, Vec<(String, String, u32)>> = HashMap::new();
                Self::from_map(&empty_map).expect("构建空 MappedSuffixIndex 不应失败")
            })
            .clone()
    }

    pub fn from_container(
        backing: Arc<Backing>,
        wp_range: (usize, usize),
        pfx_pool_range: (usize, usize),
        idx_range: (usize, usize),
    ) -> Result<Self, String> {
        let all: &[u8] = &backing;
        let wp = PoolMeta::parse(all, wp_range, MAX_WORD_ID, "后缀词池段")?;
        let pfx_pool = PoolMeta::parse(all, pfx_pool_range, u32::MAX as usize - 1, "后缀声母池段")?;
        let layout = KeyEntryLayout::parse(all, idx_range, 12, "后缀索引段")?;
        Ok(Self {
            backing,
            wp_range,
            pfx_pool_range,
            idx_range,
            wp,
            pfx_pool,
            layout,
        })
    }

    pub fn from_map(map: &HashMap<String, Vec<(String, String, u32)>>) -> Result<Self, String> {
        let mut word_pool = WordPoolBuilder::with_capacity(map.len(), map.len() * 6);
        let (pfx_sec, idx_sec) = build_suffix_sections(map, &mut word_pool)?;
        let wp_sec = word_pool.finish();
        let container_bytes = build_container(&[
            (SEC_WORD_POOL, &wp_sec),
            (SEC_SUFFIX_PREFIX_POOL, &pfx_sec),
            (SEC_SUFFIX_INDEX, &idx_sec),
        ]);
        let backing = Backing::from_vec(container_bytes);
        let container = ContainerView::parse(Arc::clone(&backing))?;
        let wp_range = container
            .section_range(SEC_WORD_POOL)
            .ok_or_else(|| "内部容器缺少词池段".to_string())?;
        let pfx_range = container
            .section_range(SEC_SUFFIX_PREFIX_POOL)
            .ok_or_else(|| "内部容器缺少后缀声母池段".to_string())?;
        let idx_range = container
            .section_range(SEC_SUFFIX_INDEX)
            .ok_or_else(|| "内部容器缺少后缀索引段".to_string())?;
        Self::from_container(backing, wp_range, pfx_range, idx_range)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.layout.keys.key_count
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.layout.keys.key_count == 0
    }

    #[inline]
    pub fn entry_count(&self) -> usize {
        self.layout.entry_count
    }

    #[inline]
    pub fn is_mmap(&self) -> bool {
        self.backing.is_mmap()
    }

    #[inline]
    pub fn backing(&self) -> &Arc<Backing> {
        &self.backing
    }

    #[inline]
    pub fn wp_range(&self) -> (usize, usize) {
        self.wp_range
    }

    /// 声母前缀池 + 后缀索引段字节数（不含共享词池，避免与主索引重复统计）。
    #[inline]
    pub fn byte_size(&self) -> usize {
        self.pfx_pool_range.1 + self.idx_range.1
    }

    #[inline]
    pub fn raw_prefix_pool_bytes(&self) -> &[u8] {
        &self.backing[self.pfx_pool_range.0..self.pfx_pool_range.0 + self.pfx_pool_range.1]
    }

    #[inline]
    pub fn raw_index_bytes(&self) -> &[u8] {
        &self.backing[self.idx_range.0..self.idx_range.0 + self.idx_range.1]
    }

    #[inline]
    fn entries_of(&self, key_idx: usize) -> SuffixEntriesView<'_> {
        let all: &[u8] = &self.backing;
        let (entry_bytes, count) = self.layout.entry_slice(all, key_idx, 12);
        SuffixEntriesView {
            entry_bytes,
            count,
            pfx_offsets: &all[self.pfx_pool.offsets_start..self.pfx_pool.offsets_end],
            pfx_bytes: &all[self.pfx_pool.bytes_start..self.pfx_pool.bytes_end],
            wp_offsets: &all[self.wp.offsets_start..self.wp.offsets_end],
            wp_bytes: &all[self.wp.bytes_start..self.wp.bytes_end],
        }
    }

    pub fn get(&self, suffix: &str) -> Option<SuffixEntriesView<'_>> {
        let idx = self.layout.find(&self.backing, suffix)?;
        Some(self.entries_of(idx))
    }

    pub fn iter(&self) -> SuffixIter<'_> {
        SuffixIter {
            index: self,
            pos: 0,
            end: self.layout.keys.key_count,
        }
    }
}

/// 将 `word -> domain_id` 映射序列化为平坦只读键值对段（供 `MappedU32Map` 映射）。
pub fn build_u32_map_section(map: &HashMap<String, usize>) -> Result<Vec<u8>, String> {
    let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
    keys.sort_unstable();

    let dir = build_first_byte_dir(&keys);
    let mut key_offsets: Vec<u32> = Vec::with_capacity(keys.len() + 1);
    key_offsets.push(0);
    let mut key_pool: Vec<u8> = Vec::with_capacity(keys.len() * 10);
    let mut values: Vec<u32> = Vec::with_capacity(keys.len());

    for &k in &keys {
        if key_pool
            .len()
            .checked_add(k.len())
            .is_none_or(|n| n > u32::MAX as usize)
        {
            return Err("词域键字节池超出 u32 上限".to_string());
        }
        key_pool.extend_from_slice(k.as_bytes());
        key_offsets.push(key_pool.len() as u32);
        let val = map[k];
        let val_u32 =
            u32::try_from(val).map_err(|_| format!("词域编号超出 u32 上限: {val}"))?;
        values.push(val_u32);
    }

    let header_and_keys = 4 + dir.len() * 4 + key_offsets.len() * 4 + key_pool.len();
    let aligned_values = align8(header_and_keys);
    let mut out = Vec::with_capacity(aligned_values + values.len() * 4);
    out.extend_from_slice(&(keys.len() as u32).to_le_bytes());
    for v in &dir {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for v in &key_offsets {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&key_pool);
    out.resize(aligned_values, 0);
    for v in &values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    Ok(out)
}

/// [`MappedU32Map`] 全表有序迭代器。
pub struct MappedU32Iter<'a> {
    ko: &'a [u8],
    kp: &'a [u8],
    vals: &'a [u8],
    pos: usize,
    end: usize,
}

impl<'a> Iterator for MappedU32Iter<'a> {
    type Item = (&'a str, u32);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.end {
            return None;
        }
        let s = read_u32_at(self.ko, self.pos * 4) as usize;
        let e = read_u32_at(self.ko, (self.pos + 1) * 4) as usize;
        let kb = self.kp.get(s..e).unwrap_or(&[]);
        let key = std::str::from_utf8(kb).ok()?;
        let val = read_u32_at(self.vals, self.pos * 4);
        self.pos += 1;
        Some((key, val))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let rem = self.end.saturating_sub(self.pos);
        (rem, Some(rem))
    }
}

impl<'a> ExactSizeIterator for MappedU32Iter<'a> {}

/// 只读零拷贝 `String -> u32` 有序平坦映射（用于领域词 `word_domain` 等低频/只读查找表）。
#[derive(Clone, Debug)]
pub struct MappedU32Map {
    backing: Arc<Backing>,
    sec_range: (usize, usize),
    keys: KeyTableLayout,
    values_start: usize,
    values_end: usize,
}

impl Default for MappedU32Map {
    fn default() -> Self {
        Self::empty()
    }
}

impl MappedU32Map {
    /// 返回共享空表。
    pub fn empty() -> Self {
        static EMPTY: OnceLock<MappedU32Map> = OnceLock::new();
        EMPTY
            .get_or_init(|| {
                let empty_map: HashMap<String, usize> = HashMap::new();
                Self::from_map(&empty_map).expect("构建空 MappedU32Map 不应失败")
            })
            .clone()
    }

    /// 从容器内的 `SEC_DOM_WORD_DOMAIN` 段解析只读映射视图。
    pub fn from_container(
        backing: Arc<Backing>,
        sec_range: (usize, usize),
    ) -> Result<Self, String> {
        let all: &[u8] = &backing;
        let (off, len) = sec_range;
        let end = off
            .checked_add(len)
            .ok_or_else(|| "词域映射段区间溢出".to_string())?;
        let keys = KeyTableLayout::parse(all, off, end, "词域映射段")?;
        let rel_after_kp = keys.key_pool_end - off;
        let pad = (8 - (rel_after_kp % 8)) % 8;
        let values_start = keys
            .key_pool_end
            .checked_add(pad)
            .ok_or_else(|| "词域映射值表对齐溢出".to_string())?;
        let values_len = keys
            .key_count
            .checked_mul(4)
            .ok_or_else(|| "词域映射值表溢出".to_string())?;
        let values_end = values_start
            .checked_add(values_len)
            .ok_or_else(|| "词域映射值表越界".to_string())?;
        if values_end > end {
            return Err("词域映射值表越界".to_string());
        }
        Ok(Self {
            backing,
            sec_range,
            keys,
            values_start,
            values_end,
        })
    }

    /// 从内存 `HashMap<String, usize>` 构建平坦 `Backing::Owned` 只读视图。
    pub fn from_map(map: &HashMap<String, usize>) -> Result<Self, String> {
        let sec = build_u32_map_section(map)?;
        let len = sec.len();
        let backing = Backing::from_vec(sec);
        Self::from_container(backing, (0, len))
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.keys.key_count
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.keys.key_count == 0
    }

    #[inline]
    pub fn is_mmap(&self) -> bool {
        self.backing.is_mmap()
    }

    #[inline]
    pub fn backing(&self) -> &Arc<Backing> {
        &self.backing
    }

    #[inline]
    pub fn shares_backing(&self, other: &Arc<Backing>) -> bool {
        Arc::ptr_eq(&self.backing, other)
    }

    #[inline]
    pub fn byte_size(&self) -> usize {
        self.sec_range.1
    }

    #[inline]
    pub fn raw_section_bytes(&self) -> &[u8] {
        &self.backing[self.sec_range.0..self.sec_range.0 + self.sec_range.1]
    }

    /// 精确查找键对应 `u32` 值（257 槽目录缩界 + 字节切片二分，零分配）。
    #[inline]
    pub fn get(&self, key: &str) -> Option<u32> {
        let all: &[u8] = &self.backing;
        let idx = self.keys.find(all, key)?;
        Some(read_u32_at(all, self.values_start + idx * 4))
    }

    #[inline]
    pub fn contains_key(&self, key: &str) -> bool {
        self.keys.find(&self.backing, key).is_some()
    }

    #[inline]
    pub fn iter(&self) -> MappedU32Iter<'_> {
        let all: &[u8] = &self.backing;
        MappedU32Iter {
            ko: &all[self.keys.key_offsets_start..self.keys.key_offsets_end],
            kp: &all[self.keys.key_pool_start..self.keys.key_pool_end],
            vals: &all[self.values_start..self.values_end],
            pos: 0,
            end: self.keys.key_count,
        }
    }
}

#[inline]
fn align8(n: usize) -> usize {
    (n + 7) & !7
}

#[inline]
fn read_u32_at(bytes: &[u8], off: usize) -> u32 {
    match bytes.get(off..off + 4) {
        Some(b) => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        None => 0,
    }
}

#[inline]
fn read_u64_at(bytes: &[u8], off: usize) -> u64 {
    match bytes.get(off..off + 8) {
        Some(b) => u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]),
        None => 0,
    }
}

#[inline]
fn lookup_pool_str<'a>(offsets: &'a [u8], bytes: &'a [u8], id: usize) -> &'a str {
    let s = read_u32_at(offsets, id * 4) as usize;
    let e = read_u32_at(offsets, (id + 1) * 4) as usize;
    if s > e {
        return "";
    }
    bytes
        .get(s..e)
        .and_then(|b| std::str::from_utf8(b).ok())
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_map() -> HashMap<String, Vec<(String, u32, usize)>> {
        let mut map = HashMap::new();
        map.insert(
            "ni".to_string(),
            vec![("你".to_string(), 5000, 2), ("你好".to_string(), 2400, 5)],
        );
        map.insert("nihao".to_string(), vec![("你好".to_string(), 2400, 5)]);
        map.insert(
            "nihaoshijie".to_string(),
            vec![("你好世界".to_string(), 800, 11)],
        );
        map.insert("a".to_string(), vec![("啊".to_string(), 3000, 1)]);
        map.insert("你好".to_string(), vec![("UTF8测试".to_string(), 42, 6)]);
        map
    }

    #[test]
    fn test_empty_index() {
        let idx = MappedIndex::empty();
        assert_eq!(idx.len(), 0);
        assert!(idx.is_empty());
        assert!(idx.get("a").is_none());
        assert!(idx.get("").is_none());
        assert_eq!(idx.range_prefix("ni").count(), 0);
        assert_eq!(idx.range_from("ni").count(), 0);
        assert!(!idx.is_mmap());

        let sfx = MappedSuffixIndex::empty();
        assert_eq!(sfx.len(), 0);
        assert!(sfx.is_empty());
        assert!(sfx.get("zai").is_none());
        assert_eq!(sfx.iter().count(), 0);
    }

    #[test]
    fn test_owned_and_mmap_roundtrip_and_range() {
        let map = sample_map();
        let owned = MappedIndex::from_map(&map).expect("from_map");
        assert!(!owned.is_mmap());
        assert_eq!(owned.len(), map.len());

        let dir = std::env::temp_dir().join("taishen_mapped_unit_test_v5");
        let _ = std::fs::create_dir_all(&dir);
        let bin_path = dir.join("test_v5.bin");
        let bytes = build_container(&[
            (SEC_WORD_POOL, owned.raw_word_pool_bytes()),
            (SEC_INDEX_MAIN, owned.raw_index_bytes()),
        ]);
        std::fs::write(&bin_path, bytes).expect("write bin");

        let mapped = MappedIndex::open(&bin_path).expect("open mmap");
        assert!(mapped.is_mmap());
        assert_eq!(mapped.len(), owned.len());

        for idx in [&owned, &mapped] {
            let v = idx.get("ni").expect("hit ni");
            let items: Vec<_> = v.iter().collect();
            assert_eq!(
                items,
                vec![
                    EntryRef {
                        word: "你",
                        freq: 5000,
                        pinyin_len: 2,
                    },
                    EntryRef {
                        word: "你好",
                        freq: 2400,
                        pinyin_len: 5,
                    },
                ]
            );
            assert_eq!(
                idx.get("a").unwrap().get(0),
                Some(EntryRef {
                    word: "啊",
                    freq: 3000,
                    pinyin_len: 1,
                })
            );
            assert_eq!(
                idx.get("你好").unwrap().get(0),
                Some(EntryRef {
                    word: "UTF8测试",
                    freq: 42,
                    pinyin_len: 6,
                })
            );
            assert!(idx.get("nih").is_none());
            assert!(idx.get("zzz").is_none());
            assert!(idx.get("").is_none());

            let ranged: Vec<&str> = idx.range_prefix("nihao").map(|(k, _)| k).collect();
            assert_eq!(ranged, vec!["nihao", "nihaoshijie"]);
            let all_ranged: Vec<&str> = idx.range_prefix("").map(|(k, _)| k).collect();
            assert_eq!(all_ranged.len(), map.len());

            // range_from 边界：精确、空隙、越界、空串
            let from_ni: Vec<&str> = idx.range_from("ni").map(|(k, _)| k).collect();
            assert_eq!(from_ni, vec!["ni", "nihao", "nihaoshijie", "你好"]);
            let from_gap: Vec<&str> = idx.range_from("b").map(|(k, _)| k).collect();
            assert_eq!(from_gap, vec!["ni", "nihao", "nihaoshijie", "你好"]);
            let from_nih: Vec<&str> = idx.range_from("nih").map(|(k, _)| k).collect();
            assert_eq!(from_nih, vec!["nihao", "nihaoshijie", "你好"]);
            let from_zzzz: Vec<&str> = idx.range_from("zzzz").map(|(k, _)| k).collect();
            assert_eq!(from_zzzz, vec!["你好"]);
            assert_eq!(idx.range_from("\u{ffff}").count(), 0);
            assert_eq!(
                idx.range_from("").map(|(k, _)| k).collect::<Vec<_>>(),
                vec!["a", "ni", "nihao", "nihaoshijie", "你好"]
            );

            assert_eq!(v.find_word_freq("你"), Some(5000));
            assert_eq!(v.find_word_freq("你好"), Some(2400));
            assert_eq!(v.find_word_freq("他"), None);
            assert_eq!(v.find_word_freq("不存在的词"), None);

            let exact_ni: Vec<_> = v.iter_with_pinyin_len(2).map(|e| e.word).collect();
            assert_eq!(exact_ni, vec!["你"]);
            let not_exact_ni: Vec<_> = v.iter_without_pinyin_len(2).map(|e| e.word).collect();
            assert_eq!(not_exact_ni, vec!["你好"]);
            let three_b: Vec<_> = v.iter_word_byte_len_range(3, 3).map(|e| e.word).collect();
            assert_eq!(three_b, vec!["你"]);
            let six_b: Vec<_> = v.iter_word_byte_len_range(6, 6).map(|e| e.word).collect();
            assert_eq!(six_b, vec!["你好"]);

            for q in ["", "a", "b", "ni", "nih", "nihao", "zzzz", "\u{ffff}"] {
                let eager: Vec<(&str, Vec<EntryRef<'_>>)> = idx
                    .range_from(q)
                    .map(|(k, ev)| (k, ev.iter().collect()))
                    .collect();
                let lazy: Vec<(&str, Vec<EntryRef<'_>>)> = idx
                    .range_from_lazy(q)
                    .map(|(k, le)| (k, le.entries().iter().collect()))
                    .collect();
                assert_eq!(lazy, eager, "range_from_lazy({q}) 应与 range_from 等价");
            }
        }

        drop(mapped);
        let _ = std::fs::remove_file(&bin_path);
    }

    #[test]
    fn test_shared_word_pool_multi_indexes_and_suffix() {
        let mut main_map = HashMap::new();
        main_map.insert("ni".to_string(), vec![("你".to_string(), 5000, 2)]);
        main_map.insert("xianzai".to_string(), vec![("现在".to_string(), 3200, 7)]);

        let mut full_map = BTreeMap::new();
        full_map.insert("ni".to_string(), vec![("你".to_string(), 5000)]);
        full_map.insert("nihao".to_string(), vec![("你好".to_string(), 2400)]);
        full_map.insert("xianzai".to_string(), vec![("现在".to_string(), 3200)]);

        let mut suffix_map = HashMap::new();
        suffix_map.insert(
            "zai".to_string(),
            vec![
                ("x".to_string(), "现在".to_string(), 3200),
                ("zh".to_string(), "正在".to_string(), 2800),
            ],
        );

        let mut pool = WordPoolBuilder::new();
        let main_sec = build_index_section_triples(&main_map, &mut pool).expect("main");
        let full_sec = build_index_section_pairs(&full_map, &mut pool).expect("full");
        let (pfx_sec, sfx_sec) = build_suffix_sections(&suffix_map, &mut pool).expect("suffix");
        let wp_sec = pool.finish();

        let container_bytes = build_container(&[
            (SEC_WORD_POOL, &wp_sec),
            (SEC_INDEX_MAIN, &main_sec),
            (SEC_FULL_INDEX, &full_sec),
            (SEC_SUFFIX_PREFIX_POOL, &pfx_sec),
            (SEC_SUFFIX_INDEX, &sfx_sec),
        ]);

        let dir = std::env::temp_dir().join("taishen_mapped_multi_v5_test");
        let _ = std::fs::create_dir_all(&dir);
        let bin_path = dir.join("multi_v5.bin");
        std::fs::write(&bin_path, container_bytes).expect("write");

        let view = ContainerView::open(&bin_path).expect("open");
        let b = Arc::clone(view.backing());
        let wp_r = view.section_range(SEC_WORD_POOL).unwrap();
        let main_idx =
            MappedIndex::from_container(Arc::clone(&b), wp_r, view.section_range(SEC_INDEX_MAIN).unwrap())
                .unwrap();
        let full_idx =
            MappedIndex::from_container(Arc::clone(&b), wp_r, view.section_range(SEC_FULL_INDEX).unwrap())
                .unwrap();
        let suffix_idx = MappedSuffixIndex::from_container(
            Arc::clone(&b),
            wp_r,
            view.section_range(SEC_SUFFIX_PREFIX_POOL).unwrap(),
            view.section_range(SEC_SUFFIX_INDEX).unwrap(),
        )
        .unwrap();

        assert!(full_idx.shares_word_pool(&b, wp_r));
        assert_eq!(main_idx.get("xianzai").unwrap().get(0).unwrap().word, "现在");
        let full_keys: Vec<&str> = full_idx.range_from("ni").map(|(k, _)| k).collect();
        assert_eq!(full_keys, vec!["ni", "nihao", "xianzai"]);
        assert_eq!(full_idx.get("nihao").unwrap().get(0).unwrap().word, "你好");

        let sfx = suffix_idx.get("zai").expect("hit zai");
        let sfx_items: Vec<_> = sfx.iter().collect();
        assert_eq!(
            sfx_items,
            vec![
                SuffixEntryRef {
                    prefix_initials: "x",
                    word: "现在",
                    freq: 3200,
                },
                SuffixEntryRef {
                    prefix_initials: "zh",
                    word: "正在",
                    freq: 2800,
                },
            ]
        );
        assert!(suffix_idx.get("zzz").is_none());

        drop(main_idx);
        drop(full_idx);
        drop(suffix_idx);
        drop(b);
        drop(view);
        let _ = std::fs::remove_file(&bin_path);
    }

    #[cfg(windows)]
    #[test]
    fn test_windows_mmap_share_delete_allows_rename_forbids_write() {
        let map = sample_map();
        let owned = MappedIndex::from_map(&map).expect("from_map");
        let dir = std::env::temp_dir().join("taishen_mapped_win_lock_test");
        let _ = std::fs::create_dir_all(&dir);
        let bin_path = dir.join("active.bin");
        let old_path = dir.join("active.bin.old");
        let _ = std::fs::remove_file(&bin_path);
        let _ = std::fs::remove_file(&old_path);

        let bytes = build_container(&[
            (SEC_WORD_POOL, owned.raw_word_pool_bytes()),
            (SEC_INDEX_MAIN, owned.raw_index_bytes()),
        ]);
        std::fs::write(&bin_path, bytes).expect("write active");

        let mapped = MappedIndex::open(&bin_path).expect("open mmap");

        // 1. 禁止以可写模式原地打开同一文件（保护映射页不被外部改写）
        let write_attempt = std::fs::OpenOptions::new().write(true).open(&bin_path);
        assert!(
            write_attempt.is_err(),
            "已映射文件不应允许外部以 FILE_SHARE_WRITE 原地打开"
        );

        // 2. 允许安装器/更新器将其改名为 .old（FILE_SHARE_DELETE）
        std::fs::rename(&bin_path, &old_path).expect("已映射文件应允许 rename 腾出原路径");
        assert!(mapped.get("nihao").is_some(), "改名后已有映射句柄仍可读");

        drop(mapped);
        let _ = std::fs::remove_file(&old_path);
    }

    #[test]
    fn test_u32_map_empty_owned_mmap_and_custom_header() {
        let empty = MappedU32Map::empty();
        assert_eq!(empty.len(), 0);
        assert!(empty.is_empty());
        assert!(!empty.is_mmap());
        assert_eq!(empty.get("神经网络"), None);
        assert!(!empty.contains_key("神经网络"));
        assert_eq!(empty.iter().count(), 0);

        let mut map = HashMap::new();
        map.insert("神经网络".to_string(), 3usize);
        map.insert("微博".to_string(), 0usize);
        map.insert("AI".to_string(), 39usize);
        map.insert("量子计算".to_string(), 17usize);

        let owned = MappedU32Map::from_map(&map).expect("from_map u32");
        assert!(!owned.is_mmap());
        assert_eq!(owned.len(), map.len());

        let dir = std::env::temp_dir().join("taishen_mapped_u32_unit_test");
        let _ = std::fs::create_dir_all(&dir);
        let bin_path = dir.join("dom_u32.bin");
        let _ = std::fs::remove_file(&bin_path);
        let container_bytes = build_container_with_header(
            DOMAIN_CONTAINER_MAGIC,
            DOMAIN_CONTAINER_VERSION,
            &[(SEC_DOM_WORD_DOMAIN, owned.raw_section_bytes())],
        );
        std::fs::write(&bin_path, container_bytes).expect("write u32 bin");

        // 魔数或版本不匹配必须拒绝
        assert!(ContainerView::open(&bin_path).is_err());
        assert!(
            ContainerView::open_with_header(&bin_path, DOMAIN_CONTAINER_MAGIC, 2).is_err()
        );

        let view = ContainerView::open_with_header(
            &bin_path,
            DOMAIN_CONTAINER_MAGIC,
            DOMAIN_CONTAINER_VERSION,
        )
        .expect("open_with_header dom");
        let mapped = MappedU32Map::from_container(
            Arc::clone(view.backing()),
            view.section_range(SEC_DOM_WORD_DOMAIN).unwrap(),
        )
        .expect("from_container u32");
        assert!(mapped.is_mmap());
        drop(view);

        for m in [&owned, &mapped] {
            assert_eq!(m.len(), map.len());
            assert_eq!(m.get("神经网络"), Some(3));
            assert_eq!(m.get("微博"), Some(0));
            assert_eq!(m.get("AI"), Some(39));
            assert_eq!(m.get("量子计算"), Some(17));
            assert_eq!(m.get("不存在"), None);
            assert_eq!(m.get(""), None);
            assert!(m.contains_key("微博"));
            assert!(!m.contains_key("不存在"));

            let collected: HashMap<&str, u32> = m.iter().collect();
            assert_eq!(collected.len(), map.len());
            for (k, v) in &map {
                assert_eq!(collected.get(k.as_str()).copied(), Some(*v as u32));
            }
        }

        drop(mapped);
        let _ = std::fs::remove_file(&bin_path);
    }
}
