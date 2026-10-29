//! bigram 搭配打分回归（V0.5.x）：拆组时同一拼音下的多个合法词，
//! 按相邻词搭配档位决定优先级——「他+使用」要压过「踏+实用」。
//!
//! 独立成文件的原因：bigram 表是进程级全局状态，和其他候选测试放同一
//! test binary 会互相污染。

use std::collections::HashMap;
use std::path::Path;

use taishen_engine::dictionary;

fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let dict = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("resources/system_dict.db");
        dictionary::init(Some(dict.as_path()));
        for _ in 0..600 {
            if dictionary::is_ready() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(dictionary::is_ready(), "真实词库应就绪");
    });
}

#[test]
fn bigram_reranks_ambiguous_segment_pair() {
    init();
    // 只给「他→使用」一对搭配：拆组里「他使用」(得分 6) 应压过「踏实用」(得分 0)
    let mut table: HashMap<String, Vec<(String, u32)>> = HashMap::new();
    table.insert("他".to_string(), vec![("使用".to_string(), 6)]);
    dictionary::install_bigram(table);

    let out = dictionary::phrase_group_guess("tashiyong");
    assert_eq!(
        out.first().map(|s| s.as_str()),
        Some("他使用"),
        "实际: {out:?}"
    );

    // 收尾：清空全局表，避免影响同进程内后续用例
    dictionary::install_bigram(HashMap::new());
}

#[test]
fn empty_bigram_table_keeps_working() {
    init();
    dictionary::install_bigram(HashMap::new());
    let out = dictionary::phrase_group_guess("dianjishiyong");
    assert!(
        !out.is_empty(),
        "无搭配表时应按原逻辑出候选，实际: {out:?}"
    );
}
