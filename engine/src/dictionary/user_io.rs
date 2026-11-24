//! 用户词库统计、导出、导入与清空（Q2.2）
//!
//! 为设置页「词库与数据」页提供安全的读写接口：
//!   - 全部 SQLite 访问统一经 `write_back::open_user_db()`（WAL + busy_timeout=2000）
//!   - 导入支持常见 UTF-8 词表格式（`词<Tab>拼音[<Tab>词频]` 与 `拼音<Tab>词[<Tab>词频]`）
//!   - 变更后自动同步刷新内存索引并清理整句格子缓存

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Transaction, params};

use super::write_back;

/// 解析目标 `user_dict.db` 路径：优先显式路径，否则回退当前配置/默认 `%APPDATA%` 路径。
pub fn resolve_target_path(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = explicit
        && !p.as_os_str().is_empty()
    {
        return Some(p.to_path_buf());
    }
    super::user_dict_db_path()
}

fn table_exists(conn: &rusqlite::Connection, table: &str) -> Result<bool, String> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            params![table],
            |row| row.get(0),
        )
        .map_err(|e| format!("查询表元数据失败: {e}"))?;
    Ok(count > 0)
}

fn delete_table_if_exists(tx: &Transaction<'_>, table: &str) -> Result<(), String> {
    let count: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            params![table],
            |row| row.get(0),
        )
        .map_err(|e| format!("查询表元数据失败: {e}"))?;
    if count > 0 {
        let sql = format!("DELETE FROM {table}");
        tx.execute(&sql, [])
            .map_err(|e| format!("清空表 {table} 失败: {e}"))?;
    }
    Ok(())
}

fn current_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn is_pinyin_col(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphabetic() || b == b'\'' || b == b' ')
        && s.bytes().any(|b| b.is_ascii_alphabetic())
}

/// 统计用户词条数与上下文搭配数。数据库尚不存在时返回 `(0, 0)`。
pub fn user_dict_stats(explicit_db: Option<&Path>) -> Result<(usize, usize), String> {
    write_back::flush(true);
    let Some(path) = resolve_target_path(explicit_db) else {
        return Err("用户词库路径未配置".to_string());
    };
    if !path.exists() {
        return Ok((0, 0));
    }
    let conn = write_back::open_user_db(&path).map_err(|e| format!("打开用户词库失败: {e}"))?;
    let words = if table_exists(&conn, "user_dict")? {
        conn.query_row("SELECT COUNT(*) FROM user_dict", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|e| format!("统计 user_dict 失败: {e}"))?
        .max(0) as usize
    } else {
        0
    };
    let pairs = if table_exists(&conn, "context_pairs")? {
        conn.query_row("SELECT COUNT(*) FROM context_pairs", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|e| format!("统计 context_pairs 失败: {e}"))?
        .max(0) as usize
    } else {
        0
    };
    Ok((words, pairs))
}

/// 导出用户词库为 UTF-8 TSV 文本（`词\t拼音\t词频`）。返回导出条数。
pub fn export_user_dict(explicit_db: Option<&Path>, out_path: &Path) -> Result<usize, String> {
    write_back::flush(true);
    let Some(db_path) = resolve_target_path(explicit_db) else {
        return Err("用户词库路径未配置".to_string());
    };
    if let Some(parent) = out_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建导出目录失败: {e}"))?;
    }

    let mut rows_out: Vec<(String, String, u32)> = Vec::new();
    if db_path.exists() {
        let conn =
            write_back::open_user_db(&db_path).map_err(|e| format!("打开用户词库失败: {e}"))?;
        if table_exists(&conn, "user_dict")? {
            let mut stmt = conn
                .prepare(
                    "SELECT word, pinyin, frequency FROM user_dict
                     ORDER BY frequency DESC, pinyin ASC, word ASC",
                )
                .map_err(|e| format!("准备导出查询失败: {e}"))?;
            let mapped = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?.max(1) as u32,
                    ))
                })
                .map_err(|e| format!("读取导出记录失败: {e}"))?;
            for item in mapped {
                let row = item.map_err(|e| format!("解析导出记录失败: {e}"))?;
                rows_out.push(row);
            }
        }
    }

    let mut content =
        String::from("# Taishen IME User Dictionary (UTF-8 TSV: word\\tpinyin\\tfrequency)\n");
    for (word, pinyin, freq) in &rows_out {
        content.push_str(&format!("{word}\t{pinyin}\t{freq}\n"));
    }
    std::fs::write(out_path, content.as_bytes()).map_err(|e| format!("写入导出文件失败: {e}"))?;
    Ok(rows_out.len())
}

/// 从 UTF-8 文本词表导入用户词库。返回成功导入条数。
pub fn import_user_dict(explicit_db: Option<&Path>, in_path: &Path) -> Result<usize, String> {
    write_back::flush(true);
    let Some(db_path) = resolve_target_path(explicit_db) else {
        return Err("用户词库路径未配置".to_string());
    };
    let raw = std::fs::read_to_string(in_path).map_err(|e| format!("读取导入文件失败: {e}"))?;
    let text = raw.strip_prefix('\u{FEFF}').unwrap_or(&raw);

    let mut entries: Vec<(String, String, u32)> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = if trimmed.contains('\t') {
            trimmed
                .split('\t')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            trimmed.split_whitespace().collect()
        };
        if cols.len() < 2 {
            continue;
        }
        let (word, raw_py) = if is_pinyin_col(cols[1]) && !cols[0].is_empty() {
            (cols[0], cols[1])
        } else if is_pinyin_col(cols[0]) && !cols[1].is_empty() {
            (cols[1], cols[0])
        } else {
            continue;
        };
        let pinyin: String = raw_py
            .bytes()
            .filter(|b| b.is_ascii_alphabetic())
            .map(|b| b.to_ascii_lowercase() as char)
            .collect();
        if pinyin.is_empty() || pinyin.len() > 64 || word.is_empty() || word.chars().count() > 32 {
            continue;
        }
        let freq = cols
            .get(2)
            .and_then(|s| s.parse::<u32>().ok())
            .map(|f| f.clamp(1, 100_000))
            .unwrap_or(1);
        entries.push((pinyin, word.to_string(), freq));
    }

    if let Some(parent) = db_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建用户词库目录失败: {e}"))?;
    }

    let mut conn =
        write_back::open_user_db(&db_path).map_err(|e| format!("打开用户词库失败: {e}"))?;
    let now = current_unix_secs();
    let tx = conn
        .transaction()
        .map_err(|e| format!("开启导入事务失败: {e}"))?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS user_dict (
            pinyin TEXT NOT NULL,
            word TEXT NOT NULL,
            frequency INTEGER DEFAULT 1,
            last_used INTEGER DEFAULT 0,
            PRIMARY KEY (pinyin, word)
        );
        CREATE INDEX IF NOT EXISTS idx_user_pinyin ON user_dict(pinyin);",
    )
    .map_err(|e| format!("初始化 user_dict 表失败: {e}"))?;

    for (pinyin, word, freq) in &entries {
        tx.execute(
            "INSERT INTO user_dict (pinyin, word, frequency, last_used)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(pinyin, word)
             DO UPDATE SET frequency = MAX(frequency, excluded.frequency),
                           last_used = MAX(last_used, excluded.last_used)",
            params![pinyin, word, *freq as i64, now],
        )
        .map_err(|e| format!("导入词条失败: {e}"))?;
    }
    tx.commit().map_err(|e| format!("提交导入事务失败: {e}"))?;
    drop(conn);

    super::reload_user_dict_if_loaded(&db_path);
    Ok(entries.len())
}

/// 清空用户词库与学习型上下文搭配表，并同步清空内存索引。
pub fn clear_user_dict(explicit_db: Option<&Path>) -> Result<(), String> {
    write_back::close();
    let Some(db_path) = resolve_target_path(explicit_db) else {
        return Err("用户词库路径未配置".to_string());
    };
    if db_path.exists() {
        let mut conn =
            write_back::open_user_db(&db_path).map_err(|e| format!("打开用户词库失败: {e}"))?;
        let tx = conn
            .transaction()
            .map_err(|e| format!("开启清空事务失败: {e}"))?;
        delete_table_if_exists(&tx, "user_dict")?;
        delete_table_if_exists(&tx, "context_pairs")?;
        tx.commit().map_err(|e| format!("提交清空事务失败: {e}"))?;
    }
    super::clear_user_words();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(name);
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(p.with_extension("db-wal"));
        let _ = std::fs::remove_file(p.with_extension("db-shm"));
        p
    }

    #[test]
    fn test_export_import_roundtrip_and_formats() {
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        write_back::close();

        let db = temp_file("taishen_user_io_roundtrip.db");
        let import_txt = temp_file("taishen_user_io_in.txt");
        let export_txt = temp_file("taishen_user_io_out.txt");

        let payload = "\u{FEFF}# 注释行\n\n泰深测试\ttai'shen ce shi\t12\nni'hao\t你好词\n测试两列 tai'shen\n无效行仅有单列\n12345\t67890\n";
        std::fs::write(&import_txt, payload.as_bytes()).expect("写入导入测试文本");

        let imported = import_user_dict(Some(&db), &import_txt).expect("导入应成功");
        assert_eq!(imported, 3, "应跳过注释/无效行，导入 3 条合法词条");

        let (words, pairs) = user_dict_stats(Some(&db)).expect("统计应成功");
        assert_eq!(words, 3);
        assert_eq!(pairs, 0);

        let exported = export_user_dict(Some(&db), &export_txt).expect("导出应成功");
        assert_eq!(exported, 3);

        let out_text = std::fs::read_to_string(&export_txt).expect("读取导出文本");
        assert!(out_text.contains("泰深测试\ttaishenceshi\t12\n"));
        assert!(out_text.contains("你好词\tnihao\t1\n"));
        assert!(out_text.contains("测试两列\ttaishen\t1\n"));

        // 空文件再次导入时仅保留已有词频，MAX(frequency, excluded.frequency) 不降频
        let roundtrip_db = temp_file("taishen_user_io_roundtrip2.db");
        let reimported = import_user_dict(Some(&roundtrip_db), &export_txt).expect("复导应成功");
        assert_eq!(reimported, 3);
        let (w2, _) = user_dict_stats(Some(&roundtrip_db)).expect("统计复导库");
        assert_eq!(w2, 3);

        write_back::close();
        for p in [&db, &roundtrip_db, &import_txt, &export_txt] {
            let _ = std::fs::remove_file(p);
            let _ = std::fs::remove_file(p.with_extension("db-wal"));
            let _ = std::fs::remove_file(p.with_extension("db-shm"));
        }
    }

    #[test]
    fn test_clear_user_dict_clears_words_and_pairs() {
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        write_back::close();

        let db = temp_file("taishen_user_io_clear.db");
        write_back::enqueue_user_word(&db, "taishen", "泰深", 100);
        write_back::enqueue_context_pair(&db, "泰深", "输入");
        write_back::flush(true);

        let (w0, p0) = user_dict_stats(Some(&db)).expect("清空前统计");
        assert_eq!((w0, p0), (1, 1));

        clear_user_dict(Some(&db)).expect("清空应成功");
        let (w1, p1) = user_dict_stats(Some(&db)).expect("清空后统计");
        assert_eq!((w1, p1), (0, 0));

        write_back::close();
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(db.with_extension("db-wal"));
        let _ = std::fs::remove_file(db.with_extension("db-shm"));
    }
}
