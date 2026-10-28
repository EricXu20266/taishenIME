//! 用户数据写入缓冲。
//!
//! 上屏学习与搭配记账原本每次都是一条独立 SQLite 事务（打开连接 + 建表检查 +
//! 单条 INSERT）。连续选词时每条都要落盘，磁盘等待直接进入上屏路径。
//!
//! 这里改成批量：写入先进内存队列，累计 20 条或距上次落盘满 2 秒时，
//! 按库文件分组合并成一个事务写入；连接按路径常驻复用。
//!
//! 代价是进程被杀时最多丢队列内未落盘的条目（上限 20 条）。退出、
//! 切换或禁用用户词库时强制落盘，日常使用不会丢已上屏的词。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::Mutex;
use std::time::Instant;

use rusqlite::Connection;

/// 队列达到该条数立即落盘
const FLUSH_ENTRIES: usize = 20;
/// 距上次落盘满该毫秒数即落盘（可在写入路径或空闲定时器触发）
const FLUSH_INTERVAL_MS: u128 = 2000;
/// 写入连续失败时队列上限，避免无限膨胀
const QUEUE_HARD_CAP: usize = 400;

/// 待写入队列与常驻连接
struct WriteBuffer {
    /// (库文件, 拼音, 词, 时间戳)
    words: Vec<(PathBuf, String, String, i64)>,
    /// (库文件, 前词, 后词)
    pairs: Vec<(PathBuf, String, String)>,
    /// 库文件 → 常驻连接
    conns: HashMap<PathBuf, Connection>,
    last_flush: Instant,
}

impl WriteBuffer {
    fn new() -> Self {
        Self {
            words: Vec::new(),
            pairs: Vec::new(),
            conns: HashMap::new(),
            last_flush: Instant::now(),
        }
    }

    fn pending(&self) -> usize {
        self.words.len() + self.pairs.len()
    }

    fn clear_queues(&mut self) {
        self.words.clear();
        self.pairs.clear();
        self.last_flush = Instant::now();
    }

    fn reset(&mut self) {
        self.clear_queues();
        self.conns.clear();
    }
}

static BUFFER: LazyLock<Mutex<WriteBuffer>> = LazyLock::new(|| Mutex::new(WriteBuffer::new()));

/// 是否到达落盘阈值
fn due(buf: &WriteBuffer) -> bool {
    let pending = buf.pending();
    pending >= FLUSH_ENTRIES
        || (pending > 0 && buf.last_flush.elapsed().as_millis() >= FLUSH_INTERVAL_MS)
}

/// 记录一次用户词学习。内存索引已由调用方更新，这里只排磁盘写入。
pub fn enqueue_user_word(path: &Path, pinyin: &str, word: &str, now: i64) {
    if pinyin.is_empty() || word.is_empty() || path.as_os_str().is_empty() {
        return;
    }
    let need = {
        let mut buf = BUFFER.lock().unwrap_or_else(|e| e.into_inner());
        buf.words.push((
            path.to_path_buf(),
            pinyin.to_string(),
            word.to_string(),
            now,
        ));
        due(&buf)
    };
    if need {
        flush(false);
    }
}

/// 记录一次上下文搭配共现。
pub fn enqueue_context_pair(path: &Path, prev: &str, next: &str) {
    if prev.is_empty() || next.is_empty() || prev == next || path.as_os_str().is_empty() {
        return;
    }
    let need = {
        let mut buf = BUFFER.lock().unwrap_or_else(|e| e.into_inner());
        buf.pairs.push((path.to_path_buf(), prev.to_string(), next.to_string()));
        due(&buf)
    };
    if need {
        flush(false);
    }
}

/// 落盘待写队列。`force=false` 时仅在阈值到达才写（供空闲定时器调用）。
pub fn flush(force: bool) {
    let mut buf = BUFFER.lock().unwrap_or_else(|e| e.into_inner());
    if buf.pending() == 0 {
        return;
    }
    if !force && !due(&buf) {
        return;
    }
    write_pending(&mut buf);
}

/// 强制落盘并关闭全部连接（切换/禁用用户词库、进程退出前调用）。
pub fn close() {
    let mut buf = BUFFER.lock().unwrap_or_else(|e| e.into_inner());
    if buf.pending() > 0 {
        write_pending(&mut buf);
    }
    buf.reset();
}

/// 当前待写条数（诊断与测试用）
pub fn pending_count() -> usize {
    BUFFER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .pending()
}

/// 取出指定库文件的待写条目并清空队列，避免持锁执行 SQL 之外的等待
fn take_queues(buf: &mut WriteBuffer) -> (
    HashMap<PathBuf, Vec<(String, String, i64)>>,
    HashMap<PathBuf, Vec<(String, String)>>,
    bool,
) {
    let mut words: HashMap<PathBuf, Vec<(String, String, i64)>> = HashMap::new();
    for (path, pinyin, word, ts) in buf.words.drain(..) {
        words.entry(path).or_default().push((pinyin, word, ts));
    }
    let mut pairs: HashMap<PathBuf, Vec<(String, String)>> = HashMap::new();
    for (path, prev, next) in buf.pairs.drain(..) {
        pairs.entry(path).or_default().push((prev, next));
    }
    buf.last_flush = Instant::now();
    let overflow = words.values().map(Vec::len).sum::<usize>()
        + pairs.values().map(Vec::len).sum::<usize>()
        > QUEUE_HARD_CAP;
    (words, pairs, overflow)
}

fn write_pending(buf: &mut WriteBuffer) {
    if buf.pending() == 0 {
        return;
    }
    let (mut words, mut pairs, _overflow) = take_queues(buf);
    let mut paths: Vec<PathBuf> = words.keys().chain(pairs.keys()).cloned().collect();
    paths.sort();
    paths.dedup();

    for path in paths {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if !buf.conns.contains_key(&path) {
            match open_with_pragmas(&path) {
                Ok(conn) => {
                    buf.conns.insert(path.clone(), conn);
                }
                Err(e) => {
                    crate::log::error(&format!("用户词库打开失败（保留待写）: {e}"));
                    // 本库条目退回队列，等下次重试
                    if let Some(list) = words.remove(&path) {
                        for (pinyin, word, ts) in list {
                            buf.words.push((path.clone(), pinyin, word, ts));
                        }
                    }
                    if let Some(list) = pairs.remove(&path) {
                        for (prev, next) in list {
                            buf.pairs.push((path.clone(), prev, next));
                        }
                    }
                    continue;
                }
            }
        }
        let word_list = words.remove(&path).unwrap_or_default();
        let pair_list = pairs.remove(&path).unwrap_or_default();
        let Some(conn) = buf.conns.get_mut(&path) else {
            continue;
        };
        if let Err(e) = apply_pending(conn, &word_list, &pair_list) {
            crate::log::error(&format!("用户词库批量写入失败: {e}"));
            for (pinyin, word, ts) in word_list {
                buf.words.push((path.clone(), pinyin, word, ts));
            }
            for (prev, next) in pair_list {
                buf.pairs.push((path.clone(), prev, next));
            }
            // 连接可能已损坏，下次重开
            buf.conns.remove(&path);
        }
    }

    // 连续失败时截断，防止队列无限膨胀
    if buf.pending() > QUEUE_HARD_CAP {
        let keep_words = buf.words.len().min(FLUSH_ENTRIES);
        buf.words.truncate(keep_words);
        let keep_pairs = buf.pairs.len().min(FLUSH_ENTRIES);
        buf.pairs.truncate(keep_pairs);
        crate::log::error("用户词库待写队列超限，已丢弃最旧条目");
    }
}

fn open_with_pragmas(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    // WAL + NORMAL：批量写入时不再每条 fsync，崩溃最多丢最后一个事务
    let _ = conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA busy_timeout=2000;",
    );
    Ok(conn)
}

fn apply_pending(
    conn: &mut Connection,
    words: &[(String, String, i64)],
    pairs: &[(String, String)],
) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS user_dict (
                pinyin TEXT NOT NULL,
                word TEXT NOT NULL,
                frequency INTEGER DEFAULT 1,
                last_used INTEGER DEFAULT 0,
                PRIMARY KEY (pinyin, word)
            );
        CREATE TABLE IF NOT EXISTS context_pairs (
                prev TEXT NOT NULL,
                next TEXT NOT NULL,
                count INTEGER NOT NULL DEFAULT 1,
                PRIMARY KEY (prev, next)
            );",
    )?;

    // 同批次相同词合并，一次 UPSERT 累加
    let mut word_agg: HashMap<(&str, &str), (i64, i64)> = HashMap::new();
    for (pinyin, word, ts) in words {
        let slot = word_agg
            .entry((pinyin.as_str(), word.as_str()))
            .or_insert((0, 0));
        slot.0 += 1;
        slot.1 = slot.1.max(*ts);
    }
    for ((pinyin, word), (delta, ts)) in word_agg {
        tx.execute(
            "INSERT INTO user_dict (pinyin, word, frequency, last_used)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(pinyin, word)
                 DO UPDATE SET frequency = frequency + ?3, last_used = ?4",
            rusqlite::params![pinyin, word, delta, ts],
        )?;
    }

    let mut pair_agg: HashMap<(&str, &str), i64> = HashMap::new();
    for (prev, next) in pairs {
        *pair_agg
            .entry((prev.as_str(), next.as_str()))
            .or_insert(0) += 1;
    }
    for ((prev, next), delta) in pair_agg {
        tx.execute(
            "INSERT INTO context_pairs (prev, next, count) VALUES (?1, ?2, ?3)
                 ON CONFLICT(prev, next) DO UPDATE SET count = count + ?3",
            rusqlite::params![prev, next, delta],
        )?;
    }
    tx.commit()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(name);
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(p.with_extension("db-wal"));
        let _ = std::fs::remove_file(p.with_extension("db-shm"));
        p
    }

    #[test]
    fn test_batch_aggregates_same_word() {
        close();
        let db = temp_db("taishen_write_buffer_agg.db");
        for _ in 0..5 {
            enqueue_user_word(&db, "taishen", "泰深", 100);
        }
        flush(true);
        assert_eq!(pending_count(), 0);

        let conn = Connection::open(&db).expect("打开测试库");
        let freq: i64 = conn
            .query_row(
                "SELECT frequency FROM user_dict WHERE pinyin=?1 AND word=?2",
                rusqlite::params!["taishen", "泰深"],
                |row| row.get(0),
            )
            .expect("应有聚合后的词条");
        assert_eq!(freq, 5, "同一批次的重复词应合并累加");
        drop(conn);
        close();
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn test_batch_pairs_accumulate() {
        close();
        let db = temp_db("taishen_write_buffer_pairs.db");
        for _ in 0..3 {
            enqueue_context_pair(&db, "北京", "大学");
        }
        flush(true);

        let conn = Connection::open(&db).expect("打开测试库");
        let count: i64 = conn
            .query_row(
                "SELECT count FROM context_pairs WHERE prev=?1 AND next=?2",
                rusqlite::params!["北京", "大学"],
                |row| row.get(0),
            )
            .expect("应有搭配记录");
        assert_eq!(count, 3);
        drop(conn);
        close();
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn test_flush_threshold_triggers_write() {
        close();
        let db = temp_db("taishen_write_buffer_threshold.db");
        for i in 0..FLUSH_ENTRIES {
            enqueue_user_word(&db, "ci", &format!("词{i}"), 1);
        }
        // 队列达到阈值时 enqueue 内部已触发落盘
        assert_eq!(pending_count(), 0, "达到 {FLUSH_ENTRIES} 条应自动落盘");
        close();
        let _ = std::fs::remove_file(&db);
    }
}
