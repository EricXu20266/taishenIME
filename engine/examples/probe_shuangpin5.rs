//! 双拼补漏验证：ei / uai / 微软 v 键 üe 别名
fn feed(e: &mut taishen_engine::Engine, code: &str) -> Vec<String> {
    e.reset();
    for ch in code.chars() {
        e.process_key(ch);
    }
    (0..e.candidate_count().min(5))
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
    e.set_shuangpin_mode(true);

    println!("== 小鹤：uai 键补漏 ==");
    e.set_shuangpin_scheme("flypy");
    for (code, want) in [("kk", "快"), ("kl", "怪"), ("kh", "块")] {
        println!("  {code} (期望含{want}): {:?}", feed(&mut e, code));
    }

    println!("== 微软：ei 键补漏 ==");
    e.set_shuangpin_scheme("mspy");
    for (code, want) in [("bz", "被"), ("bzbz", "北被"), ("mz", "美"), ("fz", "飞")] {
        println!("  {code} (期望含{want}): {:?}", feed(&mut e, code));
    }

    println!("== 微软：v 键兼 üe ==");
    for (code, want) in [("lv", "略"), ("xv", "学"), ("jt", "决")] {
        println!("  {code} (期望含{want}): {:?}", feed(&mut e, code));
    }

    println!("== 微软：ing 在分号，y 键不再混 ing ==");
    for (code, want) in [("y;", "应"), ("x;", "星"), ("ly", "略/里")] {
        println!("  {code} (期望含{want}): {:?}", feed(&mut e, code));
    }

    println!("== 自然码：ei/ou ==");
    e.set_shuangpin_scheme("zrm");
    for (code, want) in [("bz", "被"), ("ub", "手"), ("zh", "走")] {
        println!("  {code} (期望含{want}): {:?}", feed(&mut e, code));
    }
}
