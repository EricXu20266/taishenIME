//! 双拼修复验证：全拼对照 / learn 键 / 分号键
fn feed(e: &mut taishen_engine::Engine, code: &str) -> Vec<String> {
    e.reset();
    for ch in code.chars() {
        e.process_key(ch);
    }
    (0..e.candidate_count().min(6))
        .filter_map(|i| e.candidate(i).map(|s| s.to_string()))
        .collect()
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

    println!("== 全拼对照（剩余 3 个双拼失败项）==");
    for py in ["shuangpin", "lv", "an"] {
        println!("  {py}: {:?}", feed(&mut e, py));
    }

    println!("\n== learn 键（双拼选「中」）==");
    e.set_shuangpin_mode(true);
    e.set_shuangpin_scheme("flypy");
    println!("  vs: {:?}", feed(&mut e, "vs"));
    let idx = (0..e.candidate_count()).find(|i| e.candidate(*i) == Some("中"));
    println!("  选中 {:?}", idx.map(|i| e.select_candidate(i)));
    println!(
        "  user_word_count vs={} zhong={}",
        taishen_engine::dictionary::user_word_count("vs"),
        taishen_engine::dictionary::user_word_count("zhong")
    );

    println!("\n== 分号键（微软 ing）==");
    e.set_shuangpin_scheme("mspy");
    println!("  x;: {:?}", feed(&mut e, "x;"));
    println!("  奇数键 vsv: {:?}", feed(&mut e, "vsv"));
    println!("  英文 hello: {:?}", feed(&mut e, "hello"));
    println!("  奇数键后解码 = {:?}", e.shuangpin_primary_pinyin());
}
