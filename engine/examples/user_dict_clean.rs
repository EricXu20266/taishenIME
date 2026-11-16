//! 用户词库（user_dict.db）脏数据清理。
//!
//! 背景：旧版学习逻辑把「打一半的拼音」「简拼」「错拼」都当成词的读音写进
//! user_dict，导致同一个词攒出十几种拼音（实测 53% 的行是同词多拼音），
//! 下次打半边就能命中错词。学习入口已修，这个工具清理存量。
//!
//! 只删一种确定的脏行：**拼音不是合法完整拼音，且该词还有一条合法读音兜底**。
//!   - 合法完整拼音一律保留——「喔 wo」这类异读系统词库只收了一个读音，
//!     按规范读音判会误删，用户真实打过的读音不该被工具裁决。
//!   - 词只有这一条不完整读音时也保留（自造词/简拼记忆的唯一来源）。
//! 命中的典型行：`shuruf 输入法`、`xz 现在`、`qieh 切换`、`wop 我怕`。
//!
//! 用法：
//!   cargo run --manifest-path engine/Cargo.toml --example user_dict_clean
//!   cargo run --manifest-path engine/Cargo.toml --example user_dict_clean -- --apply
//!
//! 不加 --apply 时只输出统计与样例；加 --apply 会先备份到
//! ~/.taishen/backups/taishenIME/ 再删除。

use std::collections::HashMap;
use std::path::PathBuf;

use rusqlite::Connection;
use taishen_engine::pinyin;

fn default_user_dict() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(|a| PathBuf::from(a).join("taishen-ime").join("user_dict.db"))
        .unwrap_or_else(|| PathBuf::from("user_dict.db"))
}

fn timestamp_secs() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let apply = args.iter().any(|a| a == "--apply");
    let db_path = args
        .iter()
        .find_map(|a| a.strip_prefix("--db=").map(PathBuf::from))
        .unwrap_or_else(default_user_dict);

    if !db_path.is_file() {
        eprintln!("[ERROR] 用户词库不存在: {}", db_path.display());
        std::process::exit(1);
    }
    println!("用户词库: {}", db_path.display());

    let conn = match Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[ERROR] 打开失败: {e}");
            std::process::exit(1);
        }
    };
    // 先合并 WAL，否则直接读主文件可能看不到最新写入
    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");

    let mut rows: Vec<(i64, String, String)> = Vec::new();
    {
        let mut stmt = match conn.prepare("SELECT rowid, pinyin, word FROM user_dict") {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[ERROR] 读取失败: {e}");
                std::process::exit(1);
            }
        };
        if let Ok(iter) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        }) {
            rows.extend(iter.flatten());
        }
    }

    // 每个词有多少条合法读音：决定不完整读音是「冗余」还是「唯一兜底」
    let mut legal_per_word: HashMap<&str, usize> = HashMap::new();
    for (_, py, word) in &rows {
        if pinyin::is_complete_pinyin(py) {
            *legal_per_word.entry(word.as_str()).or_insert(0) += 1;
        }
    }

    let mut deletes: Vec<i64> = Vec::new();
    let mut samples: Vec<String> = Vec::new();
    for (rowid, py, word) in &rows {
        if pinyin::is_complete_pinyin(py) {
            continue;
        }
        if legal_per_word.get(word.as_str()).copied().unwrap_or(0) == 0 {
            continue; // 唯一读音，删了就打不出来了
        }
        if samples.len() < 20 {
            samples.push(format!("  [删] {word}  {py}"));
        }
        deletes.push(*rowid);
    }

    println!("\n总行数 {}", rows.len());
    println!(
        "待删除 {} 行（不完整拼音，且该词有合法读音兜底）",
        deletes.len()
    );
    if !samples.is_empty() {
        println!("\n样例:");
        for line in &samples {
            println!("{line}");
        }
    }

    if !apply {
        println!("\n[DRY-RUN] 未改动任何数据。确认后加 --apply 执行（会先备份）。");
        return;
    }
    if deletes.is_empty() {
        println!("\n[OK] 没有需要清理的行。");
        return;
    }

    let backup_dir = std::env::var_os("USERPROFILE")
        .map(|h| {
            PathBuf::from(h)
                .join(".taishen")
                .join("backups")
                .join("taishenIME")
        })
        .unwrap_or_else(|| {
            db_path
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .to_path_buf()
        });
    let _ = std::fs::create_dir_all(&backup_dir);
    let backup = backup_dir.join(format!("user_dict.db.bak-{}", timestamp_secs()));
    match std::fs::copy(&db_path, &backup) {
        Ok(size) => println!("\n[BACKUP] {} ({size} 字节)", backup.display()),
        Err(e) => {
            eprintln!("[ERROR] 备份失败，已中止: {e}");
            std::process::exit(1);
        }
    }

    let Ok(tx) = conn.unchecked_transaction() else {
        eprintln!("[ERROR] 开启事务失败，数据未改动");
        std::process::exit(1);
    };
    let mut deleted = 0usize;
    for rowid in &deletes {
        if tx
            .execute("DELETE FROM user_dict WHERE rowid=?", [rowid])
            .is_ok()
        {
            deleted += 1;
        }
    }
    if tx.commit().is_ok() {
        println!("[OK] 已删除 {deleted} 行");
    } else {
        eprintln!("[ERROR] 提交失败，数据未改动");
    }
    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
}
