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

    // ── 6. Phase 1：页宽解耦、单字优先不跨层、双音节精确词不走拆组、连续衰减 ────
    dictionary::clear_user_words();
    {
        let mut e5 = Engine::new();
        let mut e9 = Engine::new();
        for py in [
            "rengongzhineng",
            "meiyhouxuan",
            "youjian",
            "chongzhi",
            "tanchuang",
        ] {
            let c5 = top_candidates(&mut e5, py, 5);
            let c9 = top_candidates(&mut e9, py, 9);
            assert_eq!(
                c5.as_slice(),
                &c9[..c5.len().min(c9.len())],
                "page_size=5 与 page_size=9 下 {py} 前 5 候选必须严格一致: 5={c5:?} vs 9={c9:?}"
            );
        }

        // tanchuang：有精确双字词「弹窗」，不应触发 phrase_group_guess 噪声（他暗疮 / 弹出昂贵）
        let mut e = Engine::new();
        for sort_mode in [0, 1] {
            e.set_sort_mode(sort_mode);
            let tc = top_candidates(&mut e, "tanchuang", 5);
            assert!(
                tc.iter().take(2).any(|w| w == "弹窗"),
                "sort_mode={sort_mode} 时 tanchuang 前 2 应含 弹窗：{tc:?}"
            );
            assert!(
                !tc.iter().any(|w| w == "他暗疮" || w == "弹出昂贵"),
                "sort_mode={sort_mode} 时 tanchuang 不应含拆组噪声：{tc:?}"
            );
        }

        // 单字优先（sort_mode=1）不得跨 effective_match_tier 让扩展/非精确单字压过精确多字词
        e.set_sort_mode(1);
        let yj = top_candidates(&mut e, "youjian", 5);
        assert_eq!(
            yj.first().map(String::as_str),
            Some("邮件"),
            "sort_mode=1 下 youjian 首位仍应为精确词 邮件：{yj:?}"
        );
    }

    // 连续衰减与短期预热：
    // - 20 天前用过 5 次的词（右键）有效衰减分 ~1.86 >= 1.0，仍留在活跃区，击败 邮件；
    // - 刚用过 2 次的新词（友见）凭借短期热身平滑反超 20 天前用过 5 次的老词；
    // - 10 天前只用过 1 次的偶发词（油煎）衰减 < 1.0，退出活跃区不压常用词 邮件。
    {
        dictionary::write_back::flush(true);
        dictionary::set_user_dict_path(None);
        cleanup_db_files(db.path());
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        {
            let conn = Connection::open(db.path()).expect("创建临时用户词库失败");
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
            conn.execute(
                "INSERT INTO user_dict (pinyin, word, frequency, last_used) VALUES (?1, ?2, ?3, ?4)",
                params!["youjian", "右键", 5u32, now - 20 * 86400],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO user_dict (pinyin, word, frequency, last_used) VALUES (?1, ?2, ?3, ?4)",
                params!["youjian", "油煎", 1u32, now - 10 * 86400],
            )
            .unwrap();
        }
        dictionary::set_user_dict_path(Some(db.path()));
        let mut e = Engine::new();
        let c1 = top_candidates(&mut e, "youjian", 5);
        assert_eq!(
            c1.first().map(String::as_str),
            Some("右键"),
            "20 天前 5 次的习惯词应平滑保持活跃，压过未学常用词：{c1:?}"
        );
        assert_eq!(
            c1.get(1).map(String::as_str),
            Some("邮件"),
            "10 天前 1 次的偶发词应自然降温，不压过常用词 邮件：{c1:?}"
        );

        // 刚连选 2 次的「友见」通过短期热身反超 20 天前 5 次的「右键」
        dictionary::learn("youjian", "友见");
        dictionary::learn("youjian", "友见");
        let c2 = top_candidates(&mut e, "youjian", 5);
        assert_eq!(
            c2.first().map(String::as_str),
            Some("友见"),
            "刚连选 2 次的新热词应通过短期热身平滑升至首位：{c2:?}"
        );
        assert_eq!(
            c2.get(1).map(String::as_str),
            Some("右键"),
            "20 天前的老热词仍保留在次位而非断崖失忆：{c2:?}"
        );
    }

    // ── 7. Phase 2：商业输入法式原生增量选段 + 统一管线 + 退格逐段回退 + 繁体/置顶联动 ──
    load_user_rows(db.path(), &[]);
    {
        let mut e = Engine::new();
        e.set_candidate_limit(5);
        e.set_sort_mode(1);

        // 7a. dianjiyoujian (13 字符，突破旧 12 字符上限)：首屏原生含前缀词「点击」
        let dj = top_candidates(&mut e, "dianjiyoujian", 5);
        let idx_dj = dj
            .iter()
            .position(|w| w == "点击")
            .unwrap_or_else(|| panic!("dianjiyoujian 前 5 应原生含前缀词 点击：{dj:?}"));
        assert_eq!(e.select_candidate(idx_dj).as_deref(), Some("点击"));
        assert!(e.compose_active(), "选中前缀词后应进入增量组词态");
        assert_eq!(e.compose_prefix(), "点击");
        assert_eq!(e.compose_remaining(), "youjian");

        // 剩余 youjian 走统一 ranking_v2：首屏含「右键」，且不含超长前缀扩展「右键菜单」
        let rem_yj: Vec<String> = (0..e.candidate_count())
            .filter_map(|i| e.candidate(i).map(str::to_string))
            .collect();
        assert!(
            rem_yj.iter().any(|w| w == "右键"),
            "增量剩余 youjian 首屏应含 右键：{rem_yj:?}"
        );
        assert!(
            !rem_yj.iter().any(|w| w.chars().count() > 2),
            "剩余 2 音节时不得混入超长扩展词：{rem_yj:?}"
        );
        let idx_yj = rem_yj.iter().position(|w| w == "右键").unwrap();
        assert_eq!(e.select_candidate(idx_yj).as_deref(), Some("点击右键"));
        assert!(!e.compose_active());

        // 统一提交收尾学习生效：再次输入 dianjiyoujian，「点击右键」居首；输入 youjian，「右键」居首
        let dj_after = top_candidates(&mut e, "dianjiyoujian", 5);
        assert_eq!(
            dj_after.first().map(String::as_str),
            Some("点击右键"),
            "增量选段完成后 dianjiyoujian 应学入 点击右键：{dj_after:?}"
        );
        let yj_after = top_candidates(&mut e, "youjian", 5);
        assert_eq!(
            yj_after.first().map(String::as_str),
            Some("右键"),
            "多字子段 右键 同步学习回规范全拼 youjian：{yj_after:?}"
        );

        // 7b. womendeshurufa (14 字符 / 5 音节)：原生支持多级前缀词「我们的」/「我们」与退格逐段回退
        let wm = top_candidates(&mut e, "womendeshurufa", 5);
        assert!(
            wm.iter().any(|w| w == "我们的") || wm.iter().any(|w| w == "我们"),
            "womendeshurufa 首屏应含多音节前缀词：{wm:?}"
        );
        let idx_wm = wm
            .iter()
            .position(|w| w == "我们")
            .or_else(|| wm.iter().position(|w| w == "我们的"))
            .unwrap();
        let picked = wm[idx_wm].clone();
        assert_eq!(e.select_candidate(idx_wm).as_deref(), Some(picked.as_str()));
        assert!(e.compose_active());
        // Backspace 弹出已冻结段并自动退出增量态，恢复全串输入
        assert!(e.backspace());
        assert!(
            !e.compose_active(),
            "原生增量选段回退至空前缀时应退出增量态恢复全串候选"
        );
        assert_eq!(e.pinyin_str(), "womendeshurufa");

        // 7c. 增量组词态复用统一管线：繁体转换 + 置顶词生效
        dictionary::clear_user_words();
        e.set_traditional(true);
        let dj_t = top_candidates(&mut e, "dianjiyoujian", 5);
        let idx_dj_t = dj_t
            .iter()
            .position(|w| w == "点击" || w == "點擊")
            .unwrap_or_else(|| panic!("繁体模式下 dianjiyoujian 前 5 应含 点击/點擊：{dj_t:?}"));
        assert_eq!(e.select_candidate(idx_dj_t).as_deref(), Some("點擊"));
        assert_eq!(e.compose_prefix(), "點擊");
        let rem_t: Vec<String> = (0..e.candidate_count())
            .filter_map(|i| e.candidate(i).map(str::to_string))
            .collect();
        let idx_yj_t = rem_t
            .iter()
            .position(|w| w == "右键" || w == "右鍵")
            .unwrap_or_else(|| panic!("繁体模式增量剩余 youjian 应含 右键/右鍵：{rem_t:?}"));
        assert_eq!(e.select_candidate(idx_yj_t).as_deref(), Some("點擊右鍵"));
        e.set_traditional(false);

        // 置顶在增量态剩余音节同样生效
        e.reset();
        dictionary::clear_user_words();
        assert!(e.pin_word("右键"));
        let dj_p = top_candidates(&mut e, "dianjiyoujian", 5);
        let idx_dj_p = dj_p.iter().position(|w| w == "点击").unwrap();
        let _ = e.select_candidate(idx_dj_p);
        assert_eq!(
            e.candidate(0),
            Some("右键"),
            "增量组词剩余音节应原生尊重置顶词"
        );
        assert!(e.unpin_word("右键"));

        // 7d. 2 音节自定义词（xinmao → 辛茂）无需翻页 3 次：通过原生首音节单字直接部分选段
        e.reset();
        e.set_candidate_limit(9);
        dictionary::clear_user_words();
        for ch in "xinmao".chars() {
            e.process_key(ch);
        }
        assert!(!e.compose_active());
        let mut found_xin = None;
        for _ in 0..2 {
            if let Some(pos) = (0..e.candidate_count()).position(|i| e.candidate(i) == Some("新"))
            {
                found_xin = Some(pos);
                break;
            }
            if e.page(1) == 0 {
                break;
            }
        }
        let pos_xin = found_xin.expect("xinmao 不必翻页 3 次即应原生可达首音节单字 新");
        assert!(!e.compose_active(), "仅浏览前 2 页不应强制切换孤立组词模式");
        assert_eq!(e.select_candidate(pos_xin).as_deref(), Some("新"));
        assert!(
            e.compose_active(),
            "选中首音节单字后应原生进入剩余音节增量组词"
        );
        assert_eq!(e.compose_prefix(), "新");
        assert_eq!(e.compose_remaining(), "mao");
    }

    // ── 8. Phase 3：短混拼词图统一（heshenm）、纯辅音不误纠（npm）、显式隔音符 ' ──
    load_user_rows(db.path(), &[]);
    {
        let mut e = Engine::new();
        e.set_candidate_limit(9);

        // 8a. heshenm（7 字节 3 音节混拼）走 sentence.rs 词图，Top-3 命中「喝什么」
        let hs = top_candidates(&mut e, "heshenm", 9);
        assert!(
            hs.iter().take(3).any(|w| w == "喝什么"),
            "heshenm 前 3 应包含 喝什么：{hs:?}"
        );

        // 8b. 纯辅音英文缩写 npm 不走 deletion_variants 误纠出 嗯么么哒
        let npm = top_candidates(&mut e, "npm", 5);
        assert_eq!(
            npm.first().map(String::as_str),
            Some("npm"),
            "npm 首位应为 npm：{npm:?}"
        );

        // 8c. 显式隔音符 `'`：xi'an → 西安（排除 先）、fan'gan → 反感（排除 方案）、ping'an → 平安、jin'e → 金额
        let xa = top_candidates(&mut e, "xi'an", 5);
        assert_eq!(e.syllable_display(), "xi'an");
        assert_eq!(
            xa.first().map(String::as_str),
            Some("西安"),
            "xi'an 首位应为 西安：{xa:?}"
        );
        assert!(
            !xa.iter().any(|w| w == "先"),
            "xi'an 显式隔音不应混入单字 先：{xa:?}"
        );
        // 选词学习剥离隔音符，写入规范键 xian → 西安（不写 xi'an）
        assert_eq!(e.select_candidate(0).as_deref(), Some("西安"));
        dictionary::write_back::flush(true);
        let conn = Connection::open(db.path()).expect("打开临时用户库");
        let clean_xian: i64 = conn
            .query_row(
                "SELECT frequency FROM user_dict WHERE pinyin='xian' AND word='西安'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        assert_eq!(clean_xian, 1, "xi'an 选「西安」应学入规范键 xian");
        let dirty_delim: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM user_dict WHERE pinyin LIKE '%''%'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        assert_eq!(dirty_delim, 0, "user_dict 不得写入含隔音符的键");

        let fg = top_candidates(&mut e, "fan'gan", 5);
        assert_eq!(e.syllable_display(), "fan'gan");
        assert_eq!(
            fg.first().map(String::as_str),
            Some("反感"),
            "fan'gan 首位应为 反感：{fg:?}"
        );
        assert!(
            !fg.iter().any(|w| w == "方案"),
            "fan'gan 显式隔音不应混入 fang'an 的 方案：{fg:?}"
        );

        let pa = top_candidates(&mut e, "ping'an", 5);
        assert_eq!(
            pa.first().map(String::as_str),
            Some("平安"),
            "ping'an 首位应为 平安：{pa:?}"
        );

        let je = top_candidates(&mut e, "jin'e", 5);
        assert_eq!(
            je.first().map(String::as_str),
            Some("金额"),
            "jin'e 首位应为 金额：{je:?}"
        );

        // 末尾挂起隔音符反馈 `xi'`
        e.reset();
        for ch in "xi'".chars() {
            assert!(e.process_key(ch));
        }
        assert_eq!(e.syllable_display(), "xi'");
        // 连续双隔音符拦截
        assert!(!e.process_key('\''));
    }
}
