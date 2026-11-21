//! 混拼段查词探针（诊断用）：打印 abbrev_span_candidates 的返回。
//!
//! 用法: cargo run --release --example probe_abbrev -- [段...]

use std::path::Path;
use taishen_engine::dictionary;

fn main() {
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
    // 词库没就绪时继续查，会拿内置小词库的结果冒充真实词库结果，外观上分不出来。
    assert!(
        dictionary::is_ready(),
        "词库未就绪，请检查 resources/system_dict.db"
    );
    let args: Vec<String> = std::env::args().skip(1).collect();
    let keys: Vec<String> = if args.is_empty() {
        ["nim", "jige", "wm", "tqi", "zhli", "zgrmyh", "bc"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        args
    };
    for key in keys {
        let words = dictionary::abbrev_span_candidates(&key, 20);
        println!("{key:<8} -> {words:?}");
    }
}
