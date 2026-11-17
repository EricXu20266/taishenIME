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

/// 词 → 词库认可的全部读音（系统词库 + 常用词表）。
///
/// 只用于判据二：判断「这个编码和这个词到底有没有读音关系」。
/// 多音字必须收全（行 xing/hang），否则会把合法的第二读音判成错位。
fn load_real_pinyins() -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let sources = [
        (root.join("resources/system_dict.db"), "system_dict"),
        (root.join("resources/common.db"), "common_words"),
    ];
    for (path, table) in sources {
        if !path.is_file() {
            eprintln!("[WARN] 词库缺失，跳过: {}", path.display());
            continue;
        }
        let Ok(conn) = Connection::open(&path) else {
            eprintln!("[WARN] 打开失败，跳过: {}", path.display());
            continue;
        };
        let sql = format!("SELECT pinyin, word FROM {table}");
        let Ok(mut stmt) = conn.prepare(&sql) else {
            eprintln!("[WARN] 表读取失败，跳过: {table}");
            continue;
        };
        if let Ok(iter) =
            stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        {
            for (py, word) in iter.flatten() {
                let entry = out.entry(word).or_default();
                if !entry.iter().any(|p| p == &py) {
                    entry.push(py);
                }
            }
        }
    }
    println!("读音表: {} 个词", out.len());
    out
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

    let real_map = load_real_pinyins();

    let mut deletes: Vec<i64> = Vec::new();
    let mut samples: Vec<String> = Vec::new();
    let mut misaligned = 0usize;
    for (rowid, py, word) in &rows {
        // 判据一：不完整拼音，且该词有合法读音兜底（打一半写进来的别名）
        if !pinyin::is_complete_pinyin(py) {
            if legal_per_word.get(word.as_str()).copied().unwrap_or(0) == 0 {
                continue; // 唯一读音，删了就打不出来了
            }
            if samples.len() < 20 {
                samples.push(format!("  [删·半成品] {word}  {py}"));
            }
            deletes.push(*rowid);
            continue;
        }
        // 判据二（2026-11-18）：完整拼音键，但与该词任何词库读音都不相干。
        // 这是「候选索引错位 / 误触」留下的错配（实测 li→你46、ni→了39），
        // 错配对会被热词机制不断加固，长期压住正确读音。
        // 放行规则：打一半（shenm→shenme 前缀）与缩写（o→wo 后缀）不算错配。
        let Some(reals) = real_map.get(word.as_str()) else {
            continue; // 自造词：输入键就是它的唯一读音
        };
        // 词库读音去掉隔音符（qi'ting → qiting），否则前缀/简拼比较全部失效
        let norm = |s: &str| s.replace('\'', "").to_lowercase();
        let py_n = norm(py);
        if reals.iter().any(|r| norm(r) == py_n) {
            continue;
        }
        let related = reals.iter().any(|r| {
            let r = norm(r);
            // 放行三类合法别名，只留真正的错配：
            //   1. 打一半：shenm → shenme
            //   2. 简拼：yg → yinggai 的声母串（用户主动学过的简拼习惯）
            //   3. 缩写后缀：o → wo
            r.starts_with(&py_n)
                || py_n.starts_with(&r)
                || r.ends_with(&py_n)
                || pinyin::to_initial_string(&r).starts_with(&py_n)
        });
        if related {
            continue;
        }
        misaligned += 1;
        if samples.len() < 20 {
            samples.push(format!(
                "  [删·错配] {word}  {py}  (词库读音: {})",
                reals.join("/")
            ));
        }
        deletes.push(*rowid);
    }

    println!("\n总行数 {}", rows.len());
    println!(
        "待删除 {} 行（错配 {} + 半成品拼音 {}）",
        deletes.len(),
        misaligned,
        deletes.len() - misaligned
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
