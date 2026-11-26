//! 2026-11-26 候选排序、单声母/简拼热词自动学习与右键提降权引擎侧回归测试。
//!
//! 覆盖：
//! 1. `chongzhi`：精确词「充值」居首（模糊音开/关均不被 弘治/虫子 反超），且 `fangyi`/`zhengze` 不被删字劫持
//! 2. `youjian` / `zuojian`：无用户词库时精确词「右键」「左键」进入首屏（前 5），不被冷门领域/中频前缀扩展挤出
//! 3. 单声母 / 简拼 / 补全选词自动学习回规范全拼（`l→了` 记 `le`、`srf→输入法` 记 `shurufa`、
//!    `hou→后面` 记 `houmian`、`h→行` 记 `hang`、`x→行` 记 `xing`），且不写入残缺别名行、拦截错拼
//! 4. 拥挤用户词库下（`lai=234/li=35/la=30/lun=30/le=10`）单声母 `l` 首位为「了」，且持续选字正常累加频次
//! 5. 置顶/降权互斥（已降权词置顶立即生效、已置顶词降权立即压出）+ 删除用户词同步清理 `is_user_word`

use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use taishen_engine::{Engine, dictionary};

struct TempDbGuard(PathBuf);

impl TempDbGuard {
    fn new(name: &str) -> Self {
        let p = std::env::temp_dir().join(name);
        cleanup_db_files(&p);
        Self(p)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDbGuard {
    fn drop(&mut self) {
        dictionary::write_back::flush(true);
        dictionary::clear_user_words();
        dictionary::set_user_dict_path(None);
        cleanup_db_files(&self.0);
    }
}

fn cleanup_db_files(p: &Path) {
    let _ = std::fs::remove_file(p);
    let _ = std::fs::remove_file(p.with_extension("db-wal"));
    let _ = std::fs::remove_file(p.with_extension("db-shm"));
}

fn init_real_dict() {
    let dict = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("resources/system_dict.db");
    dictionary::init(Some(dict.as_path()));
    for _ in 0..600 {
        if dictionary::is_ready() && dictionary::is_common_word("你好") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(
        dictionary::is_ready() && dictionary::is_common_word("你好"),
        "真实词库与常用词表应就绪"
    );
}

fn load_user_rows(db_path: &Path, rows: &[(&str, &str, u32)]) {
    dictionary::write_back::flush(true);
    dictionary::set_user_dict_path(None);
    cleanup_db_files(db_path);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    {
        let conn = Connection::open(db_path).expect("创建临时用户词库失败");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS user_dict (
                pinyin TEXT NOT NULL,
                word TEXT NOT NULL,
                frequency INTEGER DEFAULT 1,
                last_used INTEGER DEFAULT 0,
                PRIMARY KEY (pinyin, word)
            );",
        )
        .expect("建表 user_dict 失败");
        for &(py, word, freq) in rows {
            conn.execute(
                "INSERT OR REPLACE INTO user_dict (pinyin, word, frequency, last_used) VALUES (?1, ?2, ?3, ?4)",
                params![py, word, freq, now],
            )
            .expect("插入用户词条失败");
        }
    }
    dictionary::set_user_dict_path(Some(db_path));
}

fn top_candidates(engine: &mut Engine, input: &str, limit: usize) -> Vec<String> {
    engine.reset();
    engine.set_candidate_limit(limit);
    for ch in input.chars() {
        engine.process_key(ch);
    }
    (0..engine.candidate_count())
        .filter_map(|i| engine.candidate(i).map(|s| s.to_string()))
        .collect()
}

#[test]
fn candidate_quality_and_learning_regressions() {
    init_real_dict();
    dictionary::clear_user_words();
    let db = TempDbGuard::new("taishen_test_quality_20261126.db");
    dictionary::set_user_dict_path(None);

    // ── 1. chongzhi / fangyi / zhengze：不被删字纠错或非常用模糊音劫持 ──────────
    for fuzzy in [false, true] {
        let mut e = Engine::new();
        e.set_fuzzy_enabled(fuzzy);
        let cz = top_candidates(&mut e, "chongzhi", 5);
        assert_eq!(
            cz.first().map(String::as_str),
            Some("充值"),
            "fuzzy={fuzzy} 时 chongzhi 首位应为 充值：{cz:?}"
        );
        assert!(
            !cz.iter().any(|w| w == "弘治" || w == "虫子"),
            "fuzzy={fuzzy} 时 chongzhi 首屏不应被 弘治/虫子 抢占：{cz:?}"
        );

        let fy = top_candidates(&mut e, "fangyi", 5);
        if !fuzzy {
            assert_eq!(
                fy.first().map(String::as_str),
                Some("防疫"),
                "fuzzy=false 时 fangyi 首位应为 防疫：{fy:?}"
            );
        } else {
            assert!(
                fy.iter().take(2).any(|w| w == "防疫"),
                "fuzzy=true 时 fangyi 前 2 应含 防疫：{fy:?}"
            );
        }

        let zz = top_candidates(&mut e, "zhengze", 5);
        assert_eq!(
            zz.first().map(String::as_str),
            Some("正则"),
            "fuzzy={fuzzy} 时 zhengze 首位应为 正则：{zz:?}"
        );
    }

    // ── 2. youjian / zuojian：无用户词库时精确词进入前 5（首屏可见）─────────────
    {
        let mut e = Engine::new();
        let yj = top_candidates(&mut e, "youjian", 5);
        assert_eq!(
            yj.first().map(String::as_str),
            Some("邮件"),
            "youjian: {yj:?}"
        );
        assert!(
            yj.iter().any(|w| w == "右键"),
            "无用户词库时 youjian 前 5 应包含精确词 右键：{yj:?}"
        );

        let zj = top_candidates(&mut e, "zuojian", 5);
        assert!(
            zj.iter().any(|w| w == "左键"),
            "无用户词库时 zuojian 前 5 应包含精确词 左键：{zj:?}"
        );
    }

    // ── 3. 单声母 / 简拼 / 补全学习自动落到规范全拼，且拦截错拼 ─────────────────
    load_user_rows(db.path(), &[]);
    {
        let mut e = Engine::new();
        e.set_candidate_limit(9);

        // 3a. 单声母 `l` 选「了」→ 写入 `le → 了`（不写 `l → 了`）
        let l_cands = top_candidates(&mut e, "l", 9);
        let pos_le = l_cands.iter().position(|w| w == "了").expect("l 应含 了");
        assert_eq!(e.select_candidate(pos_le).as_deref(), Some("了"));

        // 3b. 简拼 `gsm` 选「干什么」→ 写入 `ganshenme → 干什么`
        let gsm = top_candidates(&mut e, "gsm", 9);
        if let Some(pos) = gsm.iter().position(|w| w == "干什么") {
            assert_eq!(e.select_candidate(pos).as_deref(), Some("干什么"));
        }

        // 3c. 补全 `hou` 选「后面」→ 写入 `houmian → 后面`
        let hou = top_candidates(&mut e, "hou", 9);
        let pos_hm = hou.iter().position(|w| w == "后面").expect("hou 应含 后面");
        assert_eq!(e.select_candidate(pos_hm).as_deref(), Some("后面"));

        // 3d. 外部提交 `record_external_commit("l", "了")` 同样累加 `le → 了`
        e.record_external_commit("l", "了");

        // 3e. 多音字反查按输入匹配正确读音：`h→行` 记 `hang`、`x→行` 记 `xing`；错拼拦截
        assert_eq!(
            dictionary::resolve_learn_pinyin("h", "行").as_deref(),
            Some("hang")
        );
        assert_eq!(
            dictionary::resolve_learn_pinyin("x", "行").as_deref(),
            Some("xing")
        );
        assert_eq!(
            dictionary::resolve_learn_pinyin("srf", "输入法").as_deref(),
            Some("shurufa")
        );
        assert_eq!(dictionary::resolve_learn_pinyin("nihap", "你好"), None);
        assert_eq!(dictionary::resolve_learn_pinyin("l", "你"), None);

        dictionary::write_back::flush(true);
        let conn = Connection::open(db.path()).expect("打开临时用户库");
        let le_freq: i64 = conn
            .query_row(
                "SELECT frequency FROM user_dict WHERE pinyin='le' AND word='了'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        assert_eq!(le_freq, 2, "两次经 l 选「了」应累加到 le→了");

        let dirty_l: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM user_dict WHERE pinyin='l' AND word='了'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        assert_eq!(dirty_l, 0, "不得写入残缺别名 l→了");

        let hm_freq: i64 = conn
            .query_row(
                "SELECT frequency FROM user_dict WHERE pinyin='houmian' AND word='后面'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        assert_eq!(hm_freq, 1, "经 hou 补全选「后面」应写入 houmian→后面");
    }

    // ── 4. 拥挤用户词库下 `l` 首位仍为「了」，且持续选用不掉位 ───────────────────
    load_user_rows(
        db.path(),
        &[
            ("lai", "来", 234),
            ("li", "里", 35),
            ("la", "拉", 30),
            ("lun", "轮", 30),
            ("le", "了", 10),
        ],
    );
    {
        let mut e = Engine::new();
        e.set_sort_mode(1);
        let l_cands = top_candidates(&mut e, "l", 5);
        assert_eq!(
            l_cands.first().map(String::as_str),
            Some("了"),
            "拥挤用户词库下 l 首位应为 了：{l_cands:?}"
        );
    }

    // ── 5. 置顶/降权互斥 + 删除用户词同步清理 is_user_word ───────────────────────
    load_user_rows(db.path(), &[("chongzhi", "虫豸", 2)]);
    {
        let mut e = Engine::new();
        let before = top_candidates(&mut e, "chongzhi", 5);
        assert_eq!(before.first().map(String::as_str), Some("虫豸"));
        assert!(dictionary::is_user_word("虫豸"));

        // 降权 → 压出首屏
        assert!(e.demote_word("虫豸"));
        assert!(e.is_demoted("虫豸"));
        let after_demote: Vec<String> = (0..e.candidate_count())
            .filter_map(|i| e.candidate(i).map(|s| s.to_string()))
            .collect();
        assert_eq!(after_demote.first().map(String::as_str), Some("充值"));

        // 对已降权词执行置顶 → 必须自动取消降权并立即置顶
        assert!(e.pin_word("虫豸"));
        assert!(e.is_pinned("虫豸"));
        assert!(!e.is_demoted("虫豸"));
        let after_pin: Vec<String> = (0..e.candidate_count())
            .filter_map(|i| e.candidate(i).map(|s| s.to_string()))
            .collect();
        assert_eq!(after_pin.first().map(String::as_str), Some("虫豸"));

        // 再降权 → 自动取消置顶
        assert!(e.demote_word("虫豸"));
        assert!(!e.is_pinned("虫豸"));
        assert!(e.is_demoted("虫豸"));
        assert!(e.undemote_word("虫豸"));

        // 删除用户词 → 同步清理 user_words
        let cur = top_candidates(&mut e, "chongzhi", 5);
        let idx = cur.iter().position(|w| w == "虫豸").unwrap();
        assert!(e.delete_candidate(idx));
        assert!(
            !dictionary::is_user_word("虫豸"),
            "delete_candidate 后 is_user_word 应立即为 false"
        );
    }
}
