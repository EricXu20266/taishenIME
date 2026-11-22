//! 查询与打分成本探针：模拟 `char_graph` 对某前缀会发起的全部子串查询，
//! 分别累计「跨度候选查询」和「边打分」的耗时。
//!
//! 用法: cargo run --release --example lat_span

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

    const MAX_EDGE_CHARS: usize = 12;

    for input in ["zhongg", "zhongguor", "womeny", "jintia", "zhongguorenmin"] {
        let n = input.len();
        let mut span_us: u128 = 0;
        let mut span_calls = 0usize;
        let mut span_words = 0usize;

        // 模拟 char_graph：每个起点、每个跨度都查一次跨度候选
        for start in 0..n {
            let max_end = n.min(start + MAX_EDGE_CHARS);
            for end in (start + 1)..=max_end {
                let seg = &input[start..end];
                let t = Instant::now();
                let words = taishen_engine::dictionary::span_candidates(seg, 8);
                span_us += t.elapsed().as_micros();
                span_calls += 1;
                span_words += words.len();
            }
        }

        // 多音字/常用度提示查询（每条边都会调 word_hints）
        let mut hint_us: u128 = 0;
        let mut hint_calls = 0usize;
        for start in 0..n {
            let max_end = n.min(start + MAX_EDGE_CHARS);
            for end in (start + 1)..=max_end {
                let seg = &input[start..end];
                let words = taishen_engine::dictionary::span_candidates(seg, 8);
                for (w, f, _l) in words.iter().take(8) {
                    let t = Instant::now();
                    let _h = taishen_engine::dictionary::word_hints(seg, w, *f);
                    hint_us += t.elapsed().as_micros();
                    hint_calls += 1;
                }
            }
        }

        println!(
            "{input:>16} 跨度查询={:>5} 次 {:>8}us  命中词={:>6}  提示查询={:>5} 次 {:>8}us  合计={:>8}us",
            span_calls,
            span_us,
            span_words,
            hint_calls,
            hint_us,
            span_us + hint_us
        );
    }
}
