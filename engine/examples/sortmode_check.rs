//! 排序模式对照：同一输入在 sort_mode 0/1/2 下的候选差异。
//!
//! 用法: cargo run --release --example sortmode_check -- ces fax haiy

use std::path::Path;

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

    let args: Vec<String> = std::env::args().skip(1).collect();
    let inputs: Vec<&str> = if args.is_empty() {
        vec!["ces", "fax", "haiy"]
    } else {
        args.iter().map(String::as_str).collect()
    };

    for mode in [0i32, 1, 2] {
        let label = match mode {
            0 => "0=默认",
            1 => "1=单字优先",
            _ => "2=长词优先",
        };
        println!("\n=== sort_mode {label} ===");
        for input in &inputs {
            let mut e = taishen_engine::Engine::new();
            e.set_candidate_limit(9);
            e.set_sort_mode(mode);
            for ch in input.chars() {
                e.process_key(ch);
            }
            let top: Vec<String> = (0..9)
                .filter_map(|i| e.candidate_display(i).map(|t| t.to_string()))
                .collect();
            println!("  {input:>6} → {}", top.join(" | "));
        }
    }
}
