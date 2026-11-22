//! 词库主索引的零拷贝容器与读取端（`TSDICT04`）。
//!
//! 特点：
//! - 单表全局字典序 + 首字节目录（支持 O(1) 缩界二分与有序 `range_prefix` 扫描，无哈希分片）
//! - 词串集中池化（`SEC_WORD_POOL`），全表去重仅存一份
//! - 定长 8 字节无损词条编码：`word_id(24b) | pinyin_len(8b) | freq(32b)`
//! - 统一只读视图 `Backing::{Mmap, Owned}`，保证文件映射与内存构建同路径零语义漂移

use std::collections::HashMap;
use std::ops::Deref;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use memmap2::Mmap;

/// 新容器魔数（v4 格式）。
pub const CONTAINER_MAGIC: &[u8; 8] = b"TSDICT04";
/// 容器版本号。
pub const CONTAINER_VERSION: u32 = 4;

/// 容器头部长度（bytes）。
const CONTAINER_HEADER_LEN: usize = 32;
/// 容器目录项长度（bytes）。
const SECTION_DIR_LEN: usize = 24;

/// 段类型标识。
pub const SEC_WORD_POOL: u32 = 1;
pub const SEC_INDEX_MAIN: u32 = 2;
pub const SEC_META_SHORT: u32 = 10;
pub const SEC_META_SHORT_FULL: u32 = 11;
pub const SEC_META_SUFFIX: u32 = 12;
pub const SEC_META_FULL: u32 = 13;
pub const SEC_META_TRAD: u32 = 14;

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

/// 将若干 `(kind, payload)` 段封装为 8 字节对齐的 `TSDICT04` 容器字节流。
pub fn build_container(sections: &[(u32, &[u8])]) -> Vec<u8> {
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
    out.extend_from_slice(CONTAINER_MAGIC);
    out.extend_from_slice(&CONTAINER_VERSION.to_le_bytes());
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

/// 解析后的 `TSDICT04` 容器视图。
#[derive(Clone, Debug)]
pub struct ContainerView {
    backing: Arc<Backing>,
    sections: Vec<(u32, usize, usize)>,
}

impl ContainerView {
    pub fn open(path: &Path) -> Result<Self, String> {
        Self::parse(Backing::open_mmap(path)?)
    }

    pub fn parse(backing: Arc<Backing>) -> Result<Self, String> {
        let bytes: &[u8] = &backing;
        if bytes.len() < CONTAINER_HEADER_LEN || &bytes[..8] != CONTAINER_MAGIC {
            return Err("索引不是 v4 容器格式".to_string());
        }
        let version = read_u32_at(bytes, 8);
        if version != CONTAINER_VERSION {
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

/// 词条借用视图（零分配）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryRef<'a> {
    pub word: &'a str,
    pub freq: u32,
    pub pinyin_len: usize,
}

/// 某个前缀下的全部词条切片视图。
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
        let word = lookup_word(self.wp_offsets, self.wp_bytes, word_id);
        Some(EntryRef {
            word,
            freq,
            pinyin_len,
        })
    }

    #[inline]
    pub fn iter(&self) -> EntriesIter<'a> {
        EntriesIter {
            view: *self,
            pos: 0,
        }
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

/// 有序键范围扫描迭代器（用于 `range_prefix`）。
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

/// 单表全局有序零拷贝索引（支持 `Mmap` 与 `Owned` 两种底层）。
#[derive(Clone, Debug)]
pub struct MappedIndex {
    backing: Arc<Backing>,
    wp_range: (usize, usize),
    idx_range: (usize, usize),
    wp_offsets_start: usize,
    wp_offsets_end: usize,
    wp_bytes_start: usize,
    wp_bytes_end: usize,
    key_count: usize,
    dir_start: usize,
    key_offsets_start: usize,
    key_offsets_end: usize,
    key_pool_start: usize,
    key_pool_end: usize,
    entry_count: usize,
    entry_offsets_start: usize,
    entry_offsets_end: usize,
    entries_start: usize,
    entries_end: usize,
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

    /// 直接从磁盘上的 `TSDICT04` 容器文件打开并映射主索引。
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

        // 1. 解析并校验 SEC_WORD_POOL
        let (wp_off, wp_len) = wp_range;
        let wp_end = wp_off
            .checked_add(wp_len)
            .ok_or_else(|| "词池段区间溢出".to_string())?;
        let wp = all
            .get(wp_off..wp_end)
            .ok_or_else(|| "词池段越界".to_string())?;
        if wp.len() < 8 {
            return Err("词池段头部过短".to_string());
        }
        let word_count = read_u32_at(wp, 0) as usize;
        if word_count > MAX_WORD_ID {
            return Err(format!("词池词数超出 24 位上限: {word_count}"));
        }
        let wp_offsets_len = (word_count + 1)
            .checked_mul(4)
            .ok_or_else(|| "词池偏移表溢出".to_string())?;
        let wp_offsets_start = wp_off + 4;
        let wp_offsets_end = wp_offsets_start
            .checked_add(wp_offsets_len)
            .ok_or_else(|| "词池偏移表越界".to_string())?;
        if wp_offsets_end > wp_end {
            return Err("词池偏移表越界".to_string());
        }
        let wp_bytes_len = read_u32_at(all, wp_offsets_start + word_count * 4) as usize;
        let wp_bytes_start = wp_offsets_end;
        let wp_bytes_end = wp_bytes_start
            .checked_add(wp_bytes_len)
            .ok_or_else(|| "词池字节区溢出".to_string())?;
        if wp_bytes_end > wp_end {
            return Err("词池字节区越界".to_string());
        }

        // 2. 解析并校验 SEC_INDEX_MAIN
        let (idx_off, idx_len) = idx_range;
        let idx_end = idx_off
            .checked_add(idx_len)
            .ok_or_else(|| "主索引段区间溢出".to_string())?;
        let sec = all
            .get(idx_off..idx_end)
            .ok_or_else(|| "主索引段越界".to_string())?;
        const DIR_BYTES: usize = 257 * 4;
        if sec.len() < 4 + DIR_BYTES + 12 {
            return Err("主索引段头部过短".to_string());
        }
        let key_count = read_u32_at(sec, 0) as usize;
        let dir_start = idx_off + 4;

        let key_offsets_len = (key_count + 1)
            .checked_mul(4)
            .ok_or_else(|| "键偏移表溢出".to_string())?;
        let key_offsets_start = dir_start + DIR_BYTES;
        let key_offsets_end = key_offsets_start
            .checked_add(key_offsets_len)
            .ok_or_else(|| "键偏移表越界".to_string())?;
        if key_offsets_end > idx_end {
            return Err("键偏移表越界".to_string());
        }

        let key_pool_len = read_u32_at(all, key_offsets_start + key_count * 4) as usize;
        let key_pool_start = key_offsets_end;
        let key_pool_end = key_pool_start
            .checked_add(key_pool_len)
            .ok_or_else(|| "键字节池溢出".to_string())?;
        if key_pool_end + 4 > idx_end {
            return Err("键字节池越界".to_string());
        }

        let entry_count = read_u32_at(all, key_pool_end) as usize;
        let entry_offsets_len = (key_count + 1)
            .checked_mul(4)
            .ok_or_else(|| "词条偏移表溢出".to_string())?;
        let entry_offsets_start = key_pool_end + 4;
        let entry_offsets_end = entry_offsets_start
            .checked_add(entry_offsets_len)
            .ok_or_else(|| "词条偏移表越界".to_string())?;
        if entry_offsets_end > idx_end {
            return Err("词条偏移表越界".to_string());
        }

        let rel_after_eo = entry_offsets_end - idx_off;
        let pad = (8 - (rel_after_eo % 8)) % 8;
        let entries_start = entry_offsets_end
            .checked_add(pad)
            .ok_or_else(|| "词条对齐溢出".to_string())?;
        let entries_bytes_len = entry_count
            .checked_mul(8)
            .ok_or_else(|| "词条区溢出".to_string())?;
        let entries_end = entries_start
            .checked_add(entries_bytes_len)
            .ok_or_else(|| "词条区越界".to_string())?;
        if entries_end > idx_end {
            return Err("词条区越界".to_string());
        }

        Ok(Self {
            backing,
            wp_range,
            idx_range,
            wp_offsets_start,
            wp_offsets_end,
            wp_bytes_start,
            wp_bytes_end,
            key_count,
            dir_start,
            key_offsets_start,
            key_offsets_end,
            key_pool_start,
            key_pool_end,
            entry_count,
            entry_offsets_start,
            entry_offsets_end,
            entries_start,
            entries_end,
        })
    }

    /// 从内存 `HashMap` 构建紧凑单表索引并打包为 `Backing::Owned`。
    pub fn from_map(map: &HashMap<String, Vec<(String, u32, usize)>>) -> Result<Self, String> {
        let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
        keys.sort_unstable();

        // 估算容量减少构建期重分配
        let mut word_map: HashMap<&str, u32> = HashMap::with_capacity(keys.len() / 2);
        let mut wp_offsets: Vec<u32> = Vec::with_capacity(keys.len() / 2 + 1);
        wp_offsets.push(0);
        let mut wp_bytes: Vec<u8> = Vec::with_capacity(4 * 1024 * 1024);

        let mut dir = [keys.len() as u32; 257];
        let mut cur_b = 0usize;
        for (i, k) in keys.iter().enumerate() {
            if let Some(&b) = k.as_bytes().first() {
                let b = b as usize;
                while cur_b <= b {
                    dir[cur_b] = i as u32;
                    cur_b += 1;
                }
            }
        }

        let mut key_offsets: Vec<u32> = Vec::with_capacity(keys.len() + 1);
        key_offsets.push(0);
        let mut key_pool: Vec<u8> = Vec::with_capacity(keys.len() * 9);

        let mut entry_offsets: Vec<u32> = Vec::with_capacity(keys.len() + 1);
        entry_offsets.push(0);
        let total_entries_est: usize = map.values().map(Vec::len).sum();
        let mut packed_entries: Vec<u64> = Vec::with_capacity(total_entries_est);

        for &k in &keys {
            if key_pool.len().checked_add(k.len()).is_none_or(|n| n > u32::MAX as usize) {
                return Err("键字节池超出 u32 上限".to_string());
            }
            key_pool.extend_from_slice(k.as_bytes());
            key_offsets.push(key_pool.len() as u32);

            if let Some(list) = map.get(k) {
                for (w, freq, py_len) in list {
                    if *py_len > u8::MAX as usize {
                        return Err(format!("词条拼音长度超出 255: {py_len} ({w})"));
                    }
                    let word_id = match word_map.get(w.as_str()) {
                        Some(&id) => id,
                        None => {
                            let id = word_map.len();
                            if id > MAX_WORD_ID {
                                return Err("词库去重词数超出 24 位上限".to_string());
                            }
                            if wp_bytes
                                .len()
                                .checked_add(w.len())
                                .is_none_or(|n| n > u32::MAX as usize)
                            {
                                return Err("词串池超出 u32 上限".to_string());
                            }
                            wp_bytes.extend_from_slice(w.as_bytes());
                            wp_offsets.push(wp_bytes.len() as u32);
                            let id_u32 = id as u32;
                            word_map.insert(w.as_str(), id_u32);
                            id_u32
                        }
                    };
                    let packed = (word_id as u64)
                        | ((*py_len as u64) << 24)
                        | ((*freq as u64) << 32);
                    packed_entries.push(packed);
                }
            }
            if packed_entries.len() > u32::MAX as usize {
                return Err("词条总数超出 u32 上限".to_string());
            }
            entry_offsets.push(packed_entries.len() as u32);
        }

        let word_count = wp_offsets.len() - 1;
        let mut wp_sec = Vec::with_capacity(4 + wp_offsets.len() * 4 + wp_bytes.len());
        wp_sec.extend_from_slice(&(word_count as u32).to_le_bytes());
        for v in &wp_offsets {
            wp_sec.extend_from_slice(&v.to_le_bytes());
        }
        wp_sec.extend_from_slice(&wp_bytes);

        let key_count = keys.len();
        let entry_count = packed_entries.len();
        let header_and_pools = 4
            + dir.len() * 4
            + key_offsets.len() * 4
            + key_pool.len()
            + 4
            + entry_offsets.len() * 4;
        let aligned_entries = align8(header_and_pools);
        let mut idx_sec = Vec::with_capacity(aligned_entries + entry_count * 8);
        idx_sec.extend_from_slice(&(key_count as u32).to_le_bytes());
        for v in &dir {
            idx_sec.extend_from_slice(&v.to_le_bytes());
        }
        for v in &key_offsets {
            idx_sec.extend_from_slice(&v.to_le_bytes());
        }
        idx_sec.extend_from_slice(&key_pool);
        idx_sec.extend_from_slice(&(entry_count as u32).to_le_bytes());
        for v in &entry_offsets {
            idx_sec.extend_from_slice(&v.to_le_bytes());
        }
        idx_sec.resize(aligned_entries, 0);
        for v in &packed_entries {
            idx_sec.extend_from_slice(&v.to_le_bytes());
        }

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

    #[inline]
    pub fn len(&self) -> usize {
        self.key_count
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.key_count == 0
    }

    #[inline]
    pub fn entry_count(&self) -> usize {
        self.entry_count
    }

    #[inline]
    pub fn is_mmap(&self) -> bool {
        self.backing.is_mmap()
    }

    /// 词池段 + 主索引段字节数（不含容器头与其他 meta 段）。
    #[inline]
    pub fn byte_size(&self) -> usize {
        self.wp_range.1 + self.idx_range.1
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
    fn dir_at(&self, slot: usize) -> usize {
        read_u32_at(&self.backing, self.dir_start + slot * 4) as usize
    }

    #[inline]
    fn key_bytes_at(&self, i: usize) -> &[u8] {
        let all: &[u8] = &self.backing;
        let ko = &all[self.key_offsets_start..self.key_offsets_end];
        let kp = &all[self.key_pool_start..self.key_pool_end];
        let s = read_u32_at(ko, i * 4) as usize;
        let e = read_u32_at(ko, (i + 1) * 4) as usize;
        kp.get(s..e).unwrap_or(&[])
    }

    #[inline]
    fn entries_of(&self, key_idx: usize) -> EntriesView<'_> {
        let all: &[u8] = &self.backing;
        let eo = &all[self.entry_offsets_start..self.entry_offsets_end];
        let s = read_u32_at(eo, key_idx * 4) as usize;
        let e = read_u32_at(eo, (key_idx + 1) * 4) as usize;
        let s_clamped = s.min(self.entry_count);
        let e_clamped = e.min(self.entry_count).max(s_clamped);
        let entries_all = &all[self.entries_start..self.entries_end];
        let byte_start = s_clamped * 8;
        let byte_end = e_clamped * 8;
        EntriesView {
            entry_bytes: entries_all.get(byte_start..byte_end).unwrap_or(&[]),
            count: e_clamped - s_clamped,
            wp_offsets: &all[self.wp_offsets_start..self.wp_offsets_end],
            wp_bytes: &all[self.wp_bytes_start..self.wp_bytes_end],
        }
    }

    /// 精确查找：首字节目录缩界 + 字节切片二分（零分配）。
    pub fn get(&self, key: &str) -> Option<EntriesView<'_>> {
        if self.key_count == 0 {
            return None;
        }
        let kb = key.as_bytes();
        let (mut lo, mut hi) = match kb.first() {
            Some(&b) => {
                let b = b as usize;
                (self.dir_at(b), self.dir_at(b + 1))
            }
            None => (0, self.dir_at(0)),
        };
        lo = lo.min(self.key_count);
        hi = hi.min(self.key_count);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match self.key_bytes_at(mid).cmp(kb) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return Some(self.entries_of(mid)),
            }
        }
        None
    }

    /// 有序前缀扫描：二分定位首个 `>= prefix` 的键，顺序遍历直至不再以 `prefix` 开头。
    pub fn range_prefix<'a>(&'a self, prefix: &'a str) -> RangeIter<'a> {
        let kb = prefix.as_bytes();
        if self.key_count == 0 {
            return RangeIter {
                index: self,
                pos: 0,
                end: 0,
                prefix: kb,
            };
        }
        let (mut lo, hi) = match kb.first() {
            Some(&b) => {
                let b = b as usize;
                (self.dir_at(b), self.dir_at(b + 1))
            }
            None => (0, self.key_count),
        };
        lo = lo.min(self.key_count);
        let mut r = hi.min(self.key_count);
        let end = r;
        while lo < r {
            let mid = lo + (r - lo) / 2;
            if self.key_bytes_at(mid) < kb {
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
fn lookup_word<'a>(wp_offsets: &'a [u8], wp_bytes: &'a [u8], word_id: usize) -> &'a str {
    let s = read_u32_at(wp_offsets, word_id * 4) as usize;
    let e = read_u32_at(wp_offsets, (word_id + 1) * 4) as usize;
    if s > e {
        return "";
    }
    wp_bytes
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
        assert!(!idx.is_mmap());
    }

    #[test]
    fn test_owned_and_mmap_roundtrip_and_range() {
        let map = sample_map();
        let owned = MappedIndex::from_map(&map).expect("from_map");
        assert!(!owned.is_mmap());
        assert_eq!(owned.len(), map.len());

        let dir = std::env::temp_dir().join("taishen_mapped_unit_test");
        let _ = std::fs::create_dir_all(&dir);
        let bin_path = dir.join("test_v4.bin");
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
        }

        drop(mapped);
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
}
