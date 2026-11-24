//! Q1.1 全量词库整句二元条件概率与平滑回退集成测试（独立进程，不污染单元测试内置词库）。

use std::path::Path;

fn init_full_dict() {
    let p = if Path::new("../resources/system_dict.db").exists() {
        Path::new("../resources/system_dict.db")
    } else {
        Path::new("resources/system_dict.db")
    };
    taishen_engine::dictionary::init_blocking(Some(p));
}

#[test]
fn q11_conditional_scoring_and_sentence_regressions() {
    init_full_dict();

    // 1. TSLM 直接条件概率与有界 Katz 退避接口就绪
    let bg = taishen_engine::lm::direct_bigram_prob(Some("今天"), "天气")
        .expect("LM 应显式命中 今天 -> 天气 bigram");
    assert!(bg.is_finite() && bg <= 0.0, "direct_bigram_prob={bg}");

    let bo_prev = taishen_engine::lm::backoff("爸爸").expect("LM 应含 爸爸 backoff");
    let bo_bos = taishen_engine::lm::backoff_bos().expect("LM 应含 BOS backoff");
    assert!(bo_prev.is_finite() && bo_prev <= 0.5, "bo_prev={bo_prev}");
    assert!(bo_bos.is_finite() && bo_bos <= 0.5, "bo_bos={bo_bos}");

    // 2. 常见长句在条件概率评分下（无 -LN_RAW_TOTAL 常数扣分）保持高精度与合理真对数概率尺度
    for (py, want) in [
        ("womenzaizheliganshenme", "我们在这里干什么"),
        ("jintiantianqizhenbucuo", "今天天气真不错"),
        ("wojintianyaojiaban", "我今天要加班"),
        ("zhegewentixuyaozhuyi", "这个问题需要注意"),
        ("nimenjigerenzaizheli", "你们几个人在这里"),
    ] {
        let entries = taishen_engine::sentence::decode_meta(py, None, 3);
        assert_eq!(
            entries.first().map(|e| e.text.as_str()),
            Some(want),
            "Q1.1 长句 `{py}` 解码退化: {entries:?}"
        );
        let top_score = entries[0].score;
        assert!(
            top_score.is_finite() && top_score > -35.0,
            "`{py}` 条件概率得分应处于真对数概率尺度 (> -35)，实际 {top_score}"
        );
    }

    // 3. bababa 叠词+语气词：Jelinek-Mercer 0.75 + 0.65 backoff 确保「爸爸吧」稳居前 3，不挤出 PREFIX_WORD「爸爸」
    let bababa = taishen_engine::sentence::decode_meta("bababa", None, 3);
    assert!(
        bababa.iter().any(|e| e.text == "爸爸吧"),
        "bababa 整句前 3 应含 爸爸吧，实际: {bababa:?}"
    );

    // 4. 超长冷门/缺边句平滑退避不崩、得分有限
    let ultra = taishen_engine::sentence::decode_meta(
        "nihaoshijiezhongguorenminyinhanggongzuodewentiwomenzenmebannizhidaoma",
        None,
        3,
    );
    assert!(!ultra.is_empty(), "超长句在显式退避下不应为空");
    assert!(
        ultra.iter().all(|e| e.score.is_finite()),
        "超长句得分应全部有限: {ultra:?}"
    );
}

#[test]
fn q12_dual_track_and_pure_initials_integration() {
    init_full_dict();

    // 1. 修复 ("de", "地") 主读豁免
    assert!(
        taishen_engine::dictionary::is_primary_reading("de", "地", 3400),
        "结构助词 de->地 应判定为主要读音"
    );

    // 2. 含「地」长句前 3 恢复出现正确「地」候选
    for (py, with_di) in [
        ("tamenmanmandezoule", "他们慢慢地走了"),
        ("tahenrenzhendewanchenglerenwu", "他很认真地完成了任务"),
    ] {
        let top3 = taishen_engine::sentence::decode(py, None, 3);
        assert!(
            top3.iter().any(|t| t == with_di),
            "`{py}` 前 3 应含「{with_di}」，实际: {top3:?}"
        );
    }

    // 3. 纯声母长串（6–8 字符）端到端首选命中中文整句/机构名
    let mut engine = taishen_engine::Engine::new();
    for (py, want) in [
        ("wmmtqbj", "我们明天去北京"),
        ("jttqzbc", "今天天气真不错"),
        ("zgrmyh", "中国人民银行"),
        ("zhrmghg", "中华人民共和国"),
    ] {
        engine.reset();
        for ch in py.chars() {
            engine.process_key(ch);
        }
        assert_eq!(
            engine.candidate(0),
            Some(want),
            "纯声母 `{py}` 首选应为「{want}」，实际={:?}",
            (0..engine.candidate_count().min(5))
                .filter_map(|i| engine.candidate(i))
                .collect::<Vec<_>>()
        );
    }

    // 4. 5 字符纯声母在 sentence::decode 已放开而 Engine 仍不受 4-5 位乱码干扰
    let wmqbj = taishen_engine::sentence::decode("wmqbj", None, 3);
    assert!(!wmqbj.is_empty(), "sentence::decode(wmqbj) 应返回非空候选");

    for junk in ["ghjkl", "zxcvb"] {
        engine.reset();
        for ch in junk.chars() {
            engine.process_key(ch);
        }
        assert_ne!(
            engine.candidate_source(0),
            Some(taishen_engine::Source::Sentence),
            "乱码 `{junk}` 不得被整句通道劫持"
        );
    }
}
