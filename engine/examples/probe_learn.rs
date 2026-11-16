//! 学习机制验证：选中非首选候选后，同一拼音再次输入时该词应升到首位。
//!
//! 必须用隔离的 APPDATA 运行，避免污染真实用户词库：
//!   $env:APPDATA = "E:\AllinDeepSeek\taishenIME\tmp\learn-appdata"
//!   cargo run --release --example probe_learn
//!
//! 判定：选词后二次输入的候选首位 == 刚选中的词 → 学习生效。
//!
//! 注意选样：必须选「同拼音的词库词」。纠错来源（Source::Correction，
//! 如 nihao 下的「利好」=lihao、danshi 下的「当时」=dangshi）按设计不写入
//! 用户词库——引擎不把一次猜测当成读音事实。拿它测学习会误判为「学习失效」。

use std::path::Path;
use taishen_engine::Engine;

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
    println!("APPDATA={}", std::env::var("APPDATA").unwrap_or_default());

    // 选「同拼音的词库词」而非纠错候选：纠错来源（Source::Correction）按设计
    // 不写入用户词库，拿它测学习机制会误判。
    let cases: &[(&str, usize)] = &[("zhongguo", 1), ("danshi", 2), ("wo", 1)];
    let mut all_ok = true;

    for &(input, pick) in cases {
        let mut e = Engine::new();
        for ch in input.chars() {
            e.process_key(ch);
        }
        let before: Vec<String> = (0..e.candidate_count().min(6))
            .filter_map(|i| e.candidate(i).map(|s| s.to_string()))
            .collect();
        let chosen = e.candidate(pick).unwrap_or("").to_string();
        if chosen.is_empty() {
            println!("[{input}] SKIP: no candidate at {pick}");
            continue;
        }
        let committed = e.select_candidate(pick);
        println!("[{input}] before={before:?} chosen={chosen:?} committed={committed:?}");
        println!(
            "[{input}] is_user_word({chosen})={} is_user_word(首位)={}",
            taishen_engine::dictionary::is_user_word(&chosen),
            taishen_engine::dictionary::is_user_word(&before[0])
        );

        // 二次输入：学习后的候选
        let mut e2 = Engine::new();
        for ch in input.chars() {
            e2.process_key(ch);
        }
        let after: Vec<String> = (0..e2.candidate_count().min(6))
            .filter_map(|i| e2.candidate(i).map(|s| s.to_string()))
            .collect();
        let hit = after.first().map(|s| s.as_str()) == Some(chosen.as_str());
        println!(
            "[{input}] after={after:?} learned_first={} {}",
            hit,
            if hit { "OK" } else { "FAIL" }
        );
        if !hit {
            all_ok = false;
        }
    }

    println!("RESULT: {}", if all_ok { "ALL OK" } else { "HAS FAILURE" });
}
