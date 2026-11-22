//! 分阶段延迟探针：对每个前缀逐段测量，定位 `query_all` 里最贵的一段。
//!
//! 用法: cargo run --release --example lat_stage
//!
//! 诊断用临时探针。只调用公开 API，不改产品代码。

use std::path::Path;
use std::time::Instant;

fn main() {
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

    for full in ["zhongguorenmin", "jintiantianqi", "womenyiqi"] {
        println!("\n=== {full} ===");
        println!(
            "{:<20} {:>9} {:>9} {:>9} {:>9} {:>9}",
            "前缀", "词库查询", "词频", "切分判定", "整句解码", "合计"
        );
        let mut prefix = String::new();
        for ch in full.chars() {
            prefix.push(ch);
            let norm = taishen_engine::pinyin::normalize_v(&prefix);

            let t = Instant::now();
            let qr = taishen_engine::dictionary::query_with_split_meta(&prefix);
            let query_us = t.elapsed().as_micros();

            let texts: Vec<&str> = qr.0.iter().take(qr.1).map(|e| e.word.as_str()).collect();
            let t = Instant::now();
            let _freqs = taishen_engine::dictionary::exact_word_freqs(&norm, &texts);
            let freq_us = t.elapsed().as_micros();

            let t = Instant::now();
            let _ok = taishen_engine::pinyin::is_complete_pinyin(&norm);
            let pinyin_us = t.elapsed().as_micros();

            let t = Instant::now();
            let _s = taishen_engine::sentence::decode_meta(&prefix, None, 9);
            let sent_us = t.elapsed().as_micros();

            let total = query_us + freq_us + pinyin_us + sent_us;
            println!(
                "{:<20} {:>9} {:>9} {:>9} {:>9} {:>9}",
                prefix, query_us, freq_us, pinyin_us, sent_us, total
            );
        }
    }
}
