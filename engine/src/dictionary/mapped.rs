//! 词库主索引的零拷贝读取端。
//!
//! 把 `.bin` 直接映射进地址空间，`get` 返回指向映射区的视图（不分配、不复制）。
//! 格式见 `docs/modules/dict-memory/SPEC-mmap-index.md`。
//!
//! 只覆盖主索引（`index`：全拼前缀 → 词条）。其余索引仍走原有反序列化路径。

use std::collections::HashMap;
use std::path::Path;

use memmap2::Mmap;

/// 文件魔数（v2 格式）。
pub const MAGIC: &[u8; 8] = b"TSDICT02";
/// 头部长度。
const HEADER_LEN: usize = 32;

/// 定长词条（16 字节）——字段全部按字节序列化，读取端用 `from_le_bytes` 还原，
/// 因此不依赖内存对齐，可安全指向映射区任意偏移。
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub word_off: u32,
    pub word_len: u16,
    pub freq: u32,
    pub pinyin_len: u16,
}

const ENTRY_LEN: usize = 12;

impl Entry {
    fn read(bytes: &[u8], i: usize) -> Self {
        let b = &bytes[i * ENTRY_LEN..(i + 1) * ENTRY_LEN];
        Self {
            word_off: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            word_len: u16::from_le_bytes([b[4], b[5]]),
            freq: u32::from_le_bytes([b[6], b[7], b[8], b[9]]),
            pinyin_len: u16::from_le_bytes([b[10], b[11]]),
        }
    }
}

/// 词条的借用视图（零分配）。
#[derive(Clone, Copy, Debug)]
pub struct EntryRef<'a> {
    pub word: &'a str,
    pub freq: u32,
    pub pinyin_len: usize,
}

/// 一个前缀下的全部词条视图。
#[derive(Clone, Copy)]
pub struct EntriesView<'a> {
    entry_bytes: &'a [u8],
    count: usize,
    word_pool: &'a [u8],
}

impl<'a> EntriesView<'a> {
    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn get(&self, i: usize) -> Option<EntryRef<'a>> {
        if i >= self.count {
            return None;
        }
        let e = Entry::read(self.entry_bytes, i);
        let s = e.word_off as usize;
        let t = s + e.word_len as usize;
        Some(EntryRef {
            word: self
                .word_pool
                .get(s..t)
                .and_then(|b| std::str::from_utf8(b).ok())
                .unwrap_or(""),
            freq: e.freq,
            pinyin_len: e.pinyin_len as usize,
        })
    }

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
    fn next(&mut self) -> Option<Self::Item> {
        let item = self.view.get(self.pos)?;
        self.pos += 1;
        Some(item)
    }
}

/// 单个分片的只读视图。
struct ShardView<'a> {
    key_pool: &'a [u8],
    key_offsets: &'a [u8],
    key_count: usize,
    entry_offsets: &'a [u8],
    entry_bytes: &'a [u8],
    entry_count: usize,
    word_pool: &'a [u8],
}

impl<'a> ShardView<'a> {
    fn key_at(&self, i: usize) -> &'a str {
        let s = read_u32_at(self.key_offsets, i * 4) as usize;
        let e = read_u32_at(self.key_offsets, (i + 1) * 4) as usize;
        self.key_pool
            .get(s..e)
            .and_then(|b| std::str::from_utf8(b).ok())
            .unwrap_or("")
    }

    fn find(&self, key: &str) -> Option<usize> {
        let mut lo = 0usize;
        let mut hi = self.key_count;
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.key_at(mid).cmp(key) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return Some(mid),
            }
        }
        None
    }

    fn entries_of(&self, key_idx: usize) -> EntriesView<'a> {
        let s = read_u32_at(self.entry_offsets, key_idx * 4) as usize;
        let e = read_u32_at(self.entry_offsets, (key_idx + 1) * 4) as usize;
        let end = (e.min(self.entry_count)) * ENTRY_LEN;
        let start = (s.min(self.entry_count)) * ENTRY_LEN;
        EntriesView {
            entry_bytes: self.entry_bytes.get(start..end).unwrap_or(&[]),
            count: e.saturating_sub(s),
            word_pool: self.word_pool,
        }
    }
}

fn read_u32_at(bytes: &[u8], off: usize) -> u32 {
    match bytes.get(off..off + 4) {
        Some(b) => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        None => 0,
    }
}

/// mmap 主索引。
pub struct MappedIndex {
    map: Mmap,
    /// 每分片的 (offset, len)，偏移相对文件起点。
    shards: Vec<(usize, usize)>,
}

unsafe impl Send for MappedIndex {}
unsafe impl Sync for MappedIndex {}

impl MappedIndex {
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|e| format!("打开索引失败: {e}"))?;
        let map = unsafe { Mmap::map(&file) }.map_err(|e| format!("映射索引失败: {e}"))?;
        let bytes: &[u8] = &map;

        if bytes.len() < HEADER_LEN || &bytes[..8] != MAGIC {
            return Err("索引不是 v2 格式".to_string());
        }
        let shard_count = read_u32_at(bytes, 12) as usize;
        let meta_len = u64::from_le_bytes(bytes[16..24].try_into().unwrap_or([0; 8])) as usize;
        if shard_count == 0 {
            return Err("索引头损坏".to_string());
        }
        // 目录布局：先 N 个 u64 长度槽，再依次是各分片 body
        let mut p = HEADER_LEN + meta_len;
        let mut lens: Vec<usize> = Vec::with_capacity(shard_count);
        for _ in 0..shard_count {
            if p + 8 > bytes.len() {
                return Err("分片目录损坏".to_string());
            }
            lens.push(u64::from_le_bytes(bytes[p..p + 8].try_into().unwrap_or([0; 8])) as usize);
            p += 8;
        }
        let mut shards = Vec::with_capacity(shard_count);
        for len in lens {
            if p + len > bytes.len() {
                return Err("分片长度越界".to_string());
            }
            shards.push((p, len));
            p += len;
        }
        Ok(Self { map, shards })
    }

    fn shard_view(&self, i: usize) -> Option<ShardView<'_>> {
        let (off, len) = *self.shards.get(i)?;
        let all: &[u8] = &self.map;
        let s = all.get(off..off + len)?;

        let mut p = 0usize;
        let key_count = read_u32_at(s, p) as usize;
        p += 4;
        let key_offsets = s.get(p..p + (key_count + 1) * 4)?;
        p += (key_count + 1) * 4;
        let key_pool_len = read_u32_at(key_offsets, key_count * 4) as usize;
        let key_pool = s.get(p..p + key_pool_len)?;
        p += key_pool_len;

        let entry_count = read_u32_at(s, p) as usize;
        p += 4;
        let entry_offsets = s.get(p..p + (key_count + 1) * 4)?;
        p += (key_count + 1) * 4;
        let pad = (4 - (p % 4)) % 4;
        p += pad;
        let entry_bytes = s.get(p..p + entry_count * ENTRY_LEN)?;
        p += entry_count * ENTRY_LEN;

        let word_pool_len = read_u32_at(s, p) as usize;
        p += 4;
        let word_pool = s.get(p..p + word_pool_len)?;

        Some(ShardView {
            key_pool,
            key_offsets,
            key_count,
            entry_offsets,
            entry_bytes,
            entry_count,
            word_pool,
        })
    }

    fn shard_of(&self, key: &str) -> usize {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut hasher);
        (hasher.finish() as usize) % self.shards.len()
    }

    /// 精确查找：返回该前缀下的词条视图（零分配）。
    pub fn get(&self, key: &str) -> Option<EntriesView<'_>> {
        let view = self.shard_view(self.shard_of(key))?;
        let idx = view.find(key)?;
        Some(view.entries_of(idx))
    }

    pub fn len(&self) -> usize {
        (0..self.shards.len())
            .filter_map(|i| self.shard_view(i))
            .map(|v| v.key_count)
            .sum()
    }
}

// ── 构建端 ────────────────────────────────────────────────

/// 构建整个索引文件（多分片）。`shards` 顺序即分片顺序，
/// 查询端按 `DefaultHasher` 取模定位，故必须与写入时同序。
pub fn build_file(shards: &[HashMap<String, Vec<(String, u32, usize)>>]) -> Vec<u8> {
    let bodies: Vec<Vec<u8>> = shards.iter().map(build_shard).collect();
    let mut out: Vec<u8> = Vec::with_capacity(
        HEADER_LEN + bodies.iter().map(Vec::len).sum::<usize>() + 8 * bodies.len(),
    );
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(shards.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes()); // meta_len
    out.extend_from_slice(&0u64.to_le_bytes()); // reserved
    for b in &bodies {
        out.extend_from_slice(&(b.len() as u64).to_le_bytes());
    }
    for b in &bodies {
        out.extend_from_slice(b);
    }
    out
}

/// 单分片体。
fn build_shard(map: &HashMap<String, Vec<(String, u32, usize)>>) -> Vec<u8> {
    let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
    keys.sort_unstable();

    let mut word_pool: Vec<u8> = Vec::new();
    let mut word_index: HashMap<&str, (u32, u16)> = HashMap::new();
    let mut key_pool: Vec<u8> = Vec::new();
    let mut key_offsets: Vec<u32> = vec![0];
    let mut entries: Vec<u8> = Vec::new();
    let mut entry_offsets: Vec<u32> = vec![0];
    let mut entry_count = 0u32;

    for k in &keys {
        key_pool.extend_from_slice(k.as_bytes());
        key_offsets.push(key_pool.len() as u32);

        if let Some(list) = map.get(*k) {
            for (w, f, pl) in list {
                let (off, len) = *word_index.entry(w.as_str()).or_insert_with(|| {
                    let off = word_pool.len() as u32;
                    word_pool.extend_from_slice(w.as_bytes());
                    (off, w.len() as u16)
                });
                entries.extend_from_slice(&off.to_le_bytes());
                entries.extend_from_slice(&len.to_le_bytes());
                entries.extend_from_slice(&f.to_le_bytes());
                entries.extend_from_slice(&(*pl as u16).to_le_bytes());
                entry_count += 1;
            }
        }
        entry_offsets.push(entry_count);
    }

    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&(keys.len() as u32).to_le_bytes());
    for v in &key_offsets {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&key_pool);
    out.extend_from_slice(&entry_count.to_le_bytes());
    for v in &entry_offsets {
        out.extend_from_slice(&v.to_le_bytes());
    }
    let align = (4 - (out.len() % 4)) % 4;
    out.extend(std::iter::repeat_n(0u8, align));
    out.extend_from_slice(&entries);
    out.extend_from_slice(&(word_pool.len() as u32).to_le_bytes());
    out.extend_from_slice(&word_pool);
    out
}
