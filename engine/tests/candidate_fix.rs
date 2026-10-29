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
        for _ in 0..600 {
            if taishen_engine::dictionary::is_ready() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(
            taishen_engine::dictionary::is_ready(),
            "真实词库应就绪"
        );
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
    assert_eq!(c.first().map(|s| s.as_str()), Some("点击使用"), "实际: {c:?}");
}

#[test]
fn haode_first_is_haode() {
    init();
    let c = top("haode", 5);
    assert_eq!(c.first().map(|s| s.as_str()), Some("好的"), "实际: {c:?}");
}
