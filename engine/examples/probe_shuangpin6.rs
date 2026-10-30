//! 复现用户报告：mspy x; / kk 的组合区与候选
fn feed(e: &mut taishen_engine::Engine, code: &str) -> (String, Vec<String>) {
    e.reset();
    for ch in code.chars() {
        e.process_key(ch);
    }
    let top = (0..e.candidate_count().min(5))
        .filter_map(|i| e.candidate(i).map(|s| s.to_string()))
        .collect();
    (e.preedit_string(), top)
}

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    taishen_engine::dictionary::init(Some(root.join("resources/system_dict.db").as_path()));
    for _ in 0..600 {
        if taishen_engine::dictionary::is_ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    std::thread::sleep(std::time::Duration::from_secs(4));
    let mut e = taishen_engine::Engine::new();
    e.set_shuangpin_mode(true);

    for (scheme, code) in [
        ("mspy", "x;"),
        ("mspy", "kk"),
        ("mspy", "ky"),
        ("mspy", "j;"),
        ("flypy", "kk"),
        ("flypy", "x;"),
    ] {
        e.set_shuangpin_scheme(scheme);
        let (pre, top) = feed(&mut e, code);
        println!("{scheme:<6} {code:<4} 组合区={pre:<12} 候选={top:?}");
    }
    println!("\n-- 单键 k (mspy) --");
    e.set_shuangpin_scheme("mspy");
    let (pre, top) = feed(&mut e, "k");
    println!("组合区={pre} 候选={top:?}");
}
