//! 繁体模式接入 ranking_v2 回归测试（方案 A）。
//!
//! 覆盖：
//! 1. 常见高频单字（lai→來、guo→國/過、hui→會、shuo→說、kai→開、wei→為、ge→個、xue→學、shi→是/時/師）保留首屏且无 DEMOTED
//! 2. 多字词与多音歧义词组（toufa→頭髮、xiangzhen→鄉鎮、xitong→系統 等）保持首位
//! 3. `system_dict_trad` 中编码错码孤字（紝/鐨/剉）不抢占首屏
//! 4. 原生繁体/一简多繁异形词（hou→後、panming→拚命、nezha→哪吒）正常接入 ranking_v2（非 Match::Foreign）

use std::path::Path;
use taishen_engine::{dictionary, reason, Engine};

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
fn traditional_mode_keeps_high_freq_and_native_trad() {
    init_real_dict();
    dictionary::clear_user_words();
    dictionary::set_user_dict_path(None);

    let mut e = Engine::new();
    e.set_traditional(true);

    // ── 1. 高频单字回归与零 DEMOTED ──────────────────────────────────────────
    for input in ["lai", "guo", "hui", "shuo", "kai", "wei", "ge", "xue", "shi"] {
        let cands = display_candidates(&mut e, input, 40);
        assert!(!cands.is_empty(), "繁体模式下 {input} 应有候选");
        for i in 0..e.candidate_count() {
            let meta = e.candidate_explain(i).unwrap_or_default();
            assert_eq!(
                meta.reasons & reason::DEMOTED,
                0,
                "繁体模式下 {input} 第 {i} 项 `{:?}` 不得被误标 DEMOTED",
                e.candidate_display(i)
            );
        }
    }

    assert_eq!(display_candidates(&mut e, "lai", 9).first().map(String::as_str), Some("來"));
    assert_eq!(display_candidates(&mut e, "kai", 9).first().map(String::as_str), Some("開"));
    assert_eq!(display_candidates(&mut e, "shuo", 9).first().map(String::as_str), Some("說"));

    let guo5 = display_candidates(&mut e, "guo", 5);
    assert!(guo5.iter().any(|w| w == "國"), "guo 前 5 应含 國：{guo5:?}");
    assert!(guo5.iter().any(|w| w == "過"), "guo 前 5 应含 過：{guo5:?}");

    let hui5 = display_candidates(&mut e, "hui", 5);
    assert!(hui5.iter().any(|w| w == "會"), "hui 前 5 应含 會：{hui5:?}");

    let wei5 = display_candidates(&mut e, "wei", 5);
    assert!(wei5.iter().any(|w| w == "為"), "wei 前 5 应含 為：{wei5:?}");

    let ge5 = display_candidates(&mut e, "ge", 5);
    assert!(ge5.iter().any(|w| w == "個"), "ge 前 5 应含 個：{ge5:?}");

    let xue5 = display_candidates(&mut e, "xue", 5);
    assert!(xue5.iter().any(|w| w == "學"), "xue 前 5 应含 學：{xue5:?}");

    let shi9 = display_candidates(&mut e, "shi", 9);
    for ch in ["是", "時", "師"] {
        assert!(shi9.iter().any(|w| w == ch), "shi 前 9 应含 {ch}：{shi9:?}");
    }

    // ── 2. 多字词与歧义词组回归 ─────────────────────────────────────────────
    for (py, want) in [
        ("toufa", "頭髮"),
        ("xiangzhen", "鄉鎮"),
        ("xitong", "系統"),
        ("zhongguo", "中國"),
        ("ruanjian", "軟體"),
        ("faxian", "發現"),
        ("women", "我們"),
    ] {
        let got = display_candidates(&mut e, py, 9);
        assert_eq!(
            got.first().map(String::as_str),
            Some(want),
            "{py} 首位应为 {want}，实际：{got:?}"
        );
    }

    // ── 3. 编码错码孤字不抢占首屏 ───────────────────────────────────────────
    let ren5 = display_candidates(&mut e, "ren", 5);
    for ch in ["認", "任", "人"] {
        assert!(ren5.iter().any(|w| w == ch), "ren 前 5 应含 {ch}：{ren5:?}");
    }
    assert!(!ren5.iter().any(|w| w == "紝"), "错码 紝 不得进 ren 前 5：{ren5:?}");

    let fei5 = display_candidates(&mut e, "fei", 5);
    for ch in ["非", "飛", "費"] {
        assert!(fei5.iter().any(|w| w == ch), "fei 前 5 应含 {ch}：{fei5:?}");
    }
    assert!(!fei5.iter().any(|w| w == "鐨"), "錯碼 鐨 不得进 fei 前 5：{fei5:?}");

    let cuo5 = display_candidates(&mut e, "cuo", 5);
    assert_eq!(cuo5.first().map(String::as_str), Some("錯"), "cuo: {cuo5:?}");
    assert!(!cuo5.iter().any(|w| w == "剉"), "錯碼 剉 不得进 cuo 前 5：{cuo5:?}");

    // ── 4. 原生繁体与一简多繁异形词正常接入 ranking_v2（非 Match::Foreign） ──
    for (py, limit, want) in [("hou", 40, "後"), ("panming", 20, "拚命"), ("nezha", 9, "哪吒")] {
        let list = display_candidates(&mut e, py, limit);
        let pos = list
            .iter()
            .position(|w| w == want)
            .unwrap_or_else(|| panic!("{py} 前 {limit} 候选应包含原生繁体词 `{want}`：{list:?}"));
        let meta = e.candidate_explain(pos).unwrap_or_default();
        assert_ne!(
            meta.match_level,
            Some(6),
            "{py}->{want} 不得落入 Match::Foreign(6)：{meta:?}"
        );
        assert_eq!(
            meta.reasons & reason::DEMOTED,
            0,
            "{py}->{want} 不得带有 DEMOTED：{meta:?}"
        );
    }
}
