//! 复现：同一词语既有规范全拼学习、又有简拼别名学习时，
//! 合并后精确层（e.3 == key_len）判定失效 → 热用户词掉出首位。
//!
//! 场景（真实用户库）：
//!   ('huihua','会话',6)   —— 规范全拼
//!   ('huih','会话',134)   —— 简拼别名，今天刚用过
//! query("huih") 首位却是「绘画」（别名 freq 22，72 天前），会话落到第二位。
//!
//! 用法：cargo test --test user_word_alias_exact -- --test-threads=1

use taishen_engine::dictionary;

fn temp_db(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(name);
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(p.with_extension("db-wal"));
    let _ = std::fs::remove_file(p.with_extension("db-shm"));
    p
}

#[test]
fn alias_learn_keeps_exact_layer() {
    dictionary::init(Some(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("resources/system_dict.db")
            .as_path(),
    ));
    for _ in 0..600 {
        if dictionary::is_ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    dictionary::clear_user_words();
    let db = temp_db("taishen_test_alias_exact.db");
    dictionary::set_user_dict_path(Some(db.as_path()));

    // 先学规范全拼（长拼音），再学简拼别名（短拼音）——与真实库加载顺序一致
    dictionary::learn("huihua", "会话");
    dictionary::learn("huih", "会话");
    // 同前缀的竞争词：它的完整拼音就是别名 huih（e.3 == key_len）
    dictionary::learn("huih", "绘画");

    let q = dictionary::query("huih");
    let pos = q.iter().position(|w| w == "会话");
    println!("query(huih) 前6 = {:?}", &q[..q.len().min(6)]);
    assert_eq!(
        pos,
        Some(0),
        "简拼别名学习后「会话」应回到首位，实际位置 {:?}：{:?}",
        pos,
        &q[..q.len().min(6)]
    );
    dictionary::set_user_dict_path(None);
}
