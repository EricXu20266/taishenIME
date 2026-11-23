//! 组词态可达性验证：fule 进组词后，当前音节 fu 的单字能否翻到「服」。
//!
//! 用法: cargo run --release --example compose_check

use std::path::PathBuf;
use taishen_engine::Engine;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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
    let user_db = std::env::var("APPDATA")
        .ok()
        .map(|a| PathBuf::from(a).join("taishen-ime").join("user_dict.db"))
        .filter(|p| p.exists());
    if let Some(u) = &user_db {
        taishen_engine::dictionary::set_user_dict_path(Some(u.as_path()));
        println!("[env] user dict = {}", u.display());
    }

    let input = "fule";
    let mut e = Engine::new();
    e.set_candidate_limit(5);
    for ch in input.chars() {
        e.process_key(ch);
    }
    println!(
        "首页: {:?}",
        (0..e.candidate_count())
            .filter_map(|i| e.candidate_display(i))
            .collect::<Vec<_>>()
    );

    // 翻 3 次进组词
    for _ in 0..3 {
        e.page(1);
    }
    println!("compose_active = {}", e.compose_active());

    // 组词态下持续翻页，收集当前音节全部单字
    let mut all: Vec<String> = Vec::new();
    for page in 0..20 {
        let n = e.candidate_count();
        let cur: Vec<String> = (0..n).filter_map(|i| e.candidate_display(i)).collect();
        if page < 3 || cur.iter().any(|w| w == "服") {
            println!("page {page}: {:?}", cur);
        }
        all.extend(cur);
        if e.page(1) == 0 || !e.compose_active() {
            break;
        }
    }
    println!("组词态候选总数 = {}", all.len());
    println!("含「服」= {}", all.iter().any(|w| w == "服"));
    println!("单字候选（去重）= {:?}", {
        let mut v: Vec<String> = all.into_iter().filter(|w| w.chars().count() == 1).collect();
        v.dedup();
        v
    });
}
