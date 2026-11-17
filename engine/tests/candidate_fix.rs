//! 候选回归测试（V0.5.x）：Eric 反馈的「按下」「点击使用」场景。
//!
//! 这些断言此前只靠人工 probe 验证，没有任何自动化守护；
//! 改动组合/纠错/取词逻辑时容易静默回归。

use std::path::Path;

use taishen_engine::Engine;

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let dict = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("resources/system_dict.db");
        taishen_engine::dictionary::init(Some(dict.as_path()));
        // is_ready() 只代表系统词库可查，常用词表/领域库还在异步合并。
        // 只等 is_ready 会拿到半加载词典，纠错/排序断言随机失败（实测同一条
        // 用例单独跑通过、串行跑失败）。用 common 表命中作为「全量就绪」信号。
        for _ in 0..600 {
            if taishen_engine::dictionary::is_ready()
                && taishen_engine::dictionary::is_common_word("你好")
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(taishen_engine::dictionary::is_ready(), "真实词库应就绪");
    });
}

fn top(input: &str, n: usize) -> Vec<String> {
    let mut e = Engine::new();
    for ch in input.chars() {
        e.process_key(ch);
    }
    (0..n)
        .filter_map(|i| e.candidate(i).map(|s| s.to_string()))
        .collect()
}

#[test]
fn anxia_first_is_anxia() {
    init();
    let c = top("anxia", 5);
    assert_eq!(c.first().map(|s| s.as_str()), Some("按下"), "实际: {c:?}");
}

#[test]
fn dianjishiyong_first_is_click_use() {
    init();
    let c = top("dianjishiyong", 5);
    assert_eq!(
        c.first().map(|s| s.as_str()),
        Some("点击使用"),
        "实际: {c:?}"
    );
}

#[test]
fn dianjishiy_prefix_completes_to_click_use() {
    init();
    let c = top("dianjishiy", 5);
    assert_eq!(
        c.first().map(|s| s.as_str()),
        Some("点击使用"),
        "实际: {c:?}"
    );
}

#[test]
fn haode_first_is_haode() {
    init();
    let c = top("haode", 5);
    assert_eq!(c.first().map(|s| s.as_str()), Some("好的"), "实际: {c:?}");
}

// ── 2026-11-16：首候选被「另一条拼音」抢位 ──────────────────────────

/// houxuan 的 h 与 g 相邻，邻键纠错会变出 gouxuan →「勾选」（常用词）。
/// 精确命中的「候选」不在常用词表，于是曾被「勾选」挤到第 3 位。
#[test]
fn houxuan_first_is_houxuan() {
    init();
    let c = top("houxuan", 5);
    assert_eq!(c.first().map(|s| s.as_str()), Some("候选"), "实际: {c:?}");
}

#[test]
fn houxuan_neighbor_correction_not_in_top3() {
    init();
    let c = top("houxuan", 20);
    let pos = c.iter().position(|w| w == "勾选");
    assert!(
        pos.is_none_or(|p| p >= 3),
        "勾选是邻键纠错结果，不应挤进前 3：{c:?}"
    );
}

/// zhongguo 能被切成 zhong+gu+o（3 音节），整句解码曾解出「中过」插到首位，
/// 把词库词频最高（4298）的「中国」压到第 3。
#[test]
fn zhongguo_first_is_zhongguo() {
    init();
    let c = top("zhongguo", 5);
    assert_eq!(c.first().map(|s| s.as_str()), Some("中国"), "实际: {c:?}");
}

/// zhonggou 精确命中只有冷僻词（重构/中沟组），纠错变体 zhongguo → 中国
/// 必须能前置到首位（不能被「精确候选含常用词」的判据误挡）。
#[test]
fn zhonggou_correction_first_is_zhongguo() {
    init();
    let c = top("zhonggou", 5);
    assert_eq!(c.first().map(|s| s.as_str()), Some("中国"), "实际: {c:?}");
}

/// nihap 切不干净，纠错出的「你好」必须前置；不能因为词长分区被降组。
#[test]
fn nihap_first_is_nihao() {
    init();
    let c = top("nihap", 5);
    assert_eq!(c.first().map(|s| s.as_str()), Some("你好"), "实际: {c:?}");
}
