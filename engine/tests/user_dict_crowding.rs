//! 用户词库在简拼/半音节串下的拥挤治理与读音校验回归测试。
//!
//! 说明：真实 `user_dict.db` 里存在历史遗留的简拼/错音别名记录（`dictionary::learn`
//! 当前会拦截部分非规范写入），因此本测试通过 SQLite 写入历史词条并用
//! `set_user_dict_path` 加载，精准复现真实用户词库场景：
//! 1. 繁体模式单字与用户词库繁体单字不被误标 `DEMOTED`
//! 2. 单音节模糊音单字不被误标 `DEMOTED`
//! 3. 单音节错音历史别名（ji→就）拦截，同时保留合法读音（shi→是）
//! 4. 简拼 Combo 不得把系统对数词频泄漏为用户线性频次
//! 5. 非完整拼音 `freq=1` 误学别名（hello→和了咯）不劫持英文词
//! 6. 拥挤阈值（N=2 / M=15）、完整拼音优先级与补全别名词频还原

use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use taishen_engine::{Engine, Source, dictionary, reason};

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

fn type_input(engine: &mut Engine, input: &str) {
    engine.reset();
    for ch in input.chars() {
        engine.process_key(ch);
    }
}

fn display_candidates(engine: &mut Engine, input: &str, limit: usize) -> Vec<String> {
    engine.set_candidate_limit(limit);
    type_input(engine, input);
    (0..engine.candidate_count())
        .filter_map(|i| engine.candidate_display(i))
        .collect()
}

#[test]
fn user_dict_crowding_and_reading_validation() {
    init_real_dict();
    dictionary::clear_user_words();
    let db = TempDbGuard::new("taishen_test_user_dict_crowding.db");
    dictionary::set_user_dict_path(None);

    // ── 1. 繁体单字与用户繁体字不被误标 DEMOTED ───────────────────────────────
    {
        let mut e = Engine::new();
        e.set_traditional(true);
        e.set_candidate_limit(40);
        for input in ["lai", "guo", "hui", "shuo", "kai", "shi"] {
            type_input(&mut e, input);
            let n = e.candidate_count();
            assert!(n > 0, "繁体模式下 {input} 应产出候选");
            for i in 0..n {
                let meta = e.candidate_explain(i).unwrap_or_default();
                assert_eq!(
                    meta.reasons & reason::DEMOTED,
                    0,
                    "繁体模式下 {input} 第 {i} 项 `{:?}` 不得被误标 DEMOTED",
                    e.candidate_display(i)
                );
            }
        }
        let shi = display_candidates(&mut e, "shi", 40);
        assert_eq!(shi.first().map(String::as_str), Some("是"), "shi: {shi:?}");
        assert_eq!(shi.get(1).map(String::as_str), Some("時"), "shi: {shi:?}");
        let ji = display_candidates(&mut e, "ji", 40);
        for ch in ["及", "極", "幾"] {
            assert!(
                ji.iter().any(|w| w == ch),
                "繁体模式下 ji 应含 {ch}：{ji:?}"
            );
        }

        // 用户词库学习繁体单字 `lai→來` 后，首位应为 `來` 且不带 DEMOTED
        load_user_rows(db.path(), &[("lai", "來", 2)]);
        let lai = display_candidates(&mut e, "lai", 40);
        assert_eq!(
            lai.first().map(String::as_str),
            Some("來"),
            "学习繁体单字后 lai 首位应为 來：{lai:?}"
        );
        let meta0 = e.candidate_explain(0).unwrap_or_default();
        assert_eq!(
            meta0.reasons & reason::DEMOTED,
            0,
            "合法用户繁体单字 來 不得带 DEMOTED"
        );
    }

    // ── 2. 单音节模糊音单字不被误标 DEMOTED ─────────────────────────────────
    dictionary::set_user_dict_path(None);
    {
        let mut e = Engine::new();
        e.set_fuzzy_enabled(true);
        e.set_candidate_limit(40);
        type_input(&mut e, "seng");
        let n = e.candidate_count();
        let mut fuzzy_seen = 0usize;
        for i in 0..n {
            if e.candidate_source(i) == Some(Source::Fuzzy) {
                fuzzy_seen += 1;
                let meta = e.candidate_explain(i).unwrap_or_default();
                assert_eq!(
                    meta.reasons & reason::DEMOTED,
                    0,
                    "模糊音候选 `{:?}` 不得被误标 DEMOTED",
                    e.candidate(i)
                );
            }
        }
        assert!(fuzzy_seen > 0, "seng 前 40 候选应包含模糊音候选");
    }

    // ── 3. 单音节错音别名拦截 & 合法读音保留 ────────────────────────────────
    load_user_rows(db.path(), &[("ji", "就", 26), ("shi", "是", 20)]);
    {
        let mut e = Engine::new();
        let ji = display_candidates(&mut e, "ji", 9);
        assert!(
            !ji.iter().any(|w| w == "就"),
            "错音别名 ji→就 应被压出首屏：{ji:?}"
        );
        assert_eq!(
            ji.first().map(String::as_str),
            Some("及"),
            "ji 首位应保持 及：{ji:?}"
        );

        let shi = display_candidates(&mut e, "shi", 9);
        assert_eq!(
            shi.first().map(String::as_str),
            Some("是"),
            "shi 合法高频用户词 是 应居首：{shi:?}"
        );
    }

    // ── 4. 系统对数词频不泄漏到用户频次阈值 ────────────────────────────────
    // 仅在单字母 `z` 下写入一次，使其进入 `user_words` 但不在 `user_short_index["zg"]`
    load_user_rows(db.path(), &[("z", "职工", 1)]);
    {
        let mut e = Engine::new();
        let zg = display_candidates(&mut e, "zg", 9);
        assert_eq!(
            zg.first().map(String::as_str),
            Some("中国"),
            "职工不得借系统对数词频冒充高频用户词抢占 zg 首位：{zg:?}"
        );
    }

    // ── 5. 非完整拼音 `freq=1` 别名不劫持英文词 ─────────────────────────────
    load_user_rows(db.path(), &[("hello", "和了咯", 1)]);
    {
        let mut e = Engine::new();
        let hello = display_candidates(&mut e, "hello", 9);
        assert_eq!(
            hello.first().map(String::as_str),
            Some("hello"),
            "freq=1 误学别名不应劫持英文词 hello：{hello:?}"
        );
        assert_eq!(
            e.candidate_source(0),
            Some(Source::English),
            "hello 首位来源应为 English"
        );
    }

    // ── 6. 拥挤阈值、完整拼音优先级与补全别名词频还原 ───────────────────────
    load_user_rows(
        db.path(),
        &[
            ("ces", "侧重", 1),
            ("ces", "层面", 1),
            ("ces", "测识别", 1),
            ("ces", "测量", 1),
            ("ces", "策时", 1),
            ("fax", "发现", 2),
            ("ceshi", "侧室", 1),
            ("hou", "后面", 20),
            ("houlai", "后来", 20),
            ("houxuan", "候选", 20),
            ("houqi", "后期", 20),
            ("houzhu", "后主", 20),
        ],
    );
    {
        let mut e = Engine::new();
        let ces = display_candidates(&mut e, "ces", 9);
        assert_eq!(
            ces.first().map(String::as_str),
            Some("测试"),
            "ces 首位应保持 测试：{ces:?}"
        );

        let fax = display_candidates(&mut e, "fax", 9);
        assert_eq!(
            fax.first().map(String::as_str),
            Some("发现"),
            "不拥挤串 fax 频次>=2 用户词应居首：{fax:?}"
        );

        let ceshi = display_candidates(&mut e, "ceshi", 9);
        assert_eq!(
            ceshi.first().map(String::as_str),
            Some("侧室"),
            "多音节完整拼音 ceshi 用户词应居首：{ceshi:?}"
        );

        let hou = display_candidates(&mut e, "hou", 9);
        assert!(
            hou.iter().any(|w| w == "后面"),
            "补全升级时同步规范拼音后，`hou` 首屏应保留「后面」：{hou:?}"
        );
    }
}
