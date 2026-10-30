//! 双拼自主测试探针
//! 1) 官方 RIME 双拼表（rime-double-pinyin）编码 → 喂引擎 → 看能否出正确词
//! 2) 引擎自身表编码 → 看候选质量（首选/top5）
//! 3) 全拼基线对比 + 单键/奇数键/长串性能
//! 用法: cargo run --release --example probe_shuangpin

use taishen_engine::pinyin::split_first_syllable;
use taishen_engine::shuangpin;

const WORDS: &[(&str, &str)] = &[
    ("这个", "zhege"),
    ("我们", "women"),
    ("中国", "zhongguo"),
    ("你好", "nihao"),
    ("测试", "ceshi"),
    ("双拼", "shuangpin"),
    ("输入法", "shurufa"),
    ("电脑", "diannao"),
    ("手机", "shouji"),
    ("工作", "gongzuo"),
    ("时间", "shijian"),
    ("问题", "wenti"),
    ("学习", "xuexi"),
    ("朋友", "pengyou"),
    ("可以", "keyi"),
    ("没有", "meiyou"),
    ("什么", "shenme"),
    ("为什么", "weishenme"),
    ("谢谢", "xiexie"),
    ("今天", "jintian"),
    ("明天", "mingtian"),
    ("昨天", "zuotian"),
    ("需要", "xuyao"),
    ("应该", "yinggai"),
    ("系统", "xitong"),
    ("程序", "chengxu"),
    ("软件", "ruanjian"),
    ("硬件", "yingjian"),
    ("女", "nv"),
    ("绿", "lv"),
    ("去", "qu"),
    ("安", "an"),
    ("爱", "ai"),
    ("昂", "ang"),
];

/// 官方 RIME 小鹤韵母表
const OFFLY: &[(&str, char)] = &[
    ("iu", 'q'), ("ei", 'w'), ("uan", 'r'), ("ue", 't'), ("ve", 't'), ("un", 'y'),
    ("ia", 'x'), ("ua", 'x'), ("uo", 'o'), ("ie", 'p'), ("iong", 's'), ("ong", 's'),
    ("ing", 'k'), ("uai", 'k'), ("ai", 'd'), ("en", 'f'), ("eng", 'g'), ("iang", 'l'),
    ("uang", 'l'), ("ang", 'h'), ("ian", 'm'), ("an", 'j'), ("ou", 'z'), ("iao", 'n'),
    ("ao", 'c'), ("ui", 'v'), ("in", 'b'), ("er", 'r'), ("a", 'a'), ("e", 'e'),
    ("i", 'i'), ("o", 'o'), ("u", 'u'), ("v", 'v'),
];

/// 官方 RIME 微软双拼韵母表（ing 在分号键，本探针用 ';' 原样，观察引擎是否接受）
const OFMS: &[(&str, char)] = &[
    ("iu", 'q'), ("ia", 'w'), ("ua", 'w'), ("er", 'r'), ("uan", 'r'), ("ve", 't'),
    ("ue", 't'), ("v", 'y'), ("uai", 'y'), ("uo", 'o'), ("un", 'p'), ("iong", 's'),
    ("ong", 's'), ("iang", 'd'), ("uang", 'd'), ("en", 'f'), ("eng", 'g'), ("ang", 'h'),
    ("ian", 'm'), ("an", 'j'), ("iao", 'c'), ("ao", 'k'), ("ai", 'l'), ("ei", 'z'),
    ("ie", 'x'), ("ui", 'v'), ("ou", 'b'), ("in", 'n'), ("ing", ';'), ("a", 'a'),
    ("e", 'e'), ("i", 'i'), ("o", 'o'), ("u", 'u'),
];

const SINGLE_SM: &str = "bpmfdtnlgkhjqxrzcsyw";

fn split_syl(s: &str) -> (&str, &str) {
    for p in ["zh", "ch", "sh"] {
        if s.len() > p.len() && s.starts_with(p) {
            return (p, &s[p.len()..]);
        }
    }
    if s.len() > 1 && SINGLE_SM.contains(&s[..1]) {
        return (&s[..1], &s[1..]);
    }
    ("", s)
}

fn yun_key(y2k: &[(&str, char)], yun: &str) -> Option<char> {
    if let Some((_, k)) = y2k.iter().find(|(y, _)| *y == yun) {
        return Some(*k);
    }
    // jqxy + u 按 RIME derive 规则视为 ü
    if yun == "u" {
        return y2k.iter().find(|(y, _)| *y == "v").map(|(_, k)| *k);
    }
    None
}

/// zero: true = 零声母加 o 前缀（微软）；false = 直接用韵母键（小鹤/自然码）
fn encode_official(y2k: &[(&str, char)], zero_o: bool, pinyin: &str) -> Option<String> {
    let mut out = String::new();
    let mut rest = pinyin;
    while !rest.is_empty() {
        let (syl, next) = split_first_syllable(rest)?;
        rest = next;
        let (sheng, yun) = split_syl(syl);
        match sheng {
            "zh" => out.push('v'),
            "ch" => out.push('i'),
            "sh" => out.push('u'),
            "" => {
                if zero_o {
                    out.push('o');
                }
            }
            s => out.push_str(s),
        }
        out.push(yun_key(y2k, yun)?);
    }
    Some(out)
}

fn encode_engine(id: &str, pinyin: &str) -> Option<String> {
    let scheme = shuangpin::find_scheme(id)?;
    let mut out = String::new();
    let mut rest = pinyin;
    while !rest.is_empty() {
        let (syl, next) = split_first_syllable(rest)?;
        rest = next;
        out.push_str(&scheme.encode(syl)?);
    }
    Some(out)
}

fn feed(engine: &mut taishen_engine::Engine, code: &str) -> (usize, Vec<String>) {
    engine.reset();
    for ch in code.chars() {
        engine.process_key(ch);
    }
    let n = engine.candidate_count();
    let top = (0..n.min(6))
        .filter_map(|i| engine.candidate(i).map(|s| s.to_string()))
        .collect();
    (n, top)
}

fn run_set(engine: &mut taishen_engine::Engine, label: &str, enc: impl Fn(&str) -> Option<String>) {
    let mut first = 0;
    let mut top5 = 0;
    let mut none = 0;
    let mut fails = Vec::new();
    let total = WORDS.len();
    for (word, py) in WORDS {
        let Some(code) = enc(py) else {
            fails.push(format!("{word}({py}) 编码失败"));
            continue;
        };
        let (n, cands) = feed(engine, &code);
        if n == 0 {
            none += 1;
            fails.push(format!("{word}={code} 无候选"));
            continue;
        }
        if cands.first().map(|s| s.as_str()) == Some(word) {
            first += 1;
        }
        if cands.iter().any(|c| c == word) {
            top5 += 1;
        } else {
            fails.push(format!("{word}={code} → {:?}", &cands[..cands.len().min(3)]));
        }
    }
    println!(
        "  {label:<14} 首选 {first:>2}/{total}  top5 {top5:>2}/{total}  无候选 {none:>2}",
    );
    for f in fails.iter().take(6) {
        println!("      ! {f}");
    }
}

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let dict = root.join("resources/system_dict.db");
    taishen_engine::dictionary::init(Some(dict.as_path()));
    for _ in 0..600 {
        if taishen_engine::dictionary::is_ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    println!("词库 ready = {}\n", taishen_engine::dictionary::is_ready());
    let mut e = taishen_engine::Engine::new();
    e.set_shuangpin_mode(true);

    println!("== 基线：全拼 ==");
    run_set(&mut e, "全拼", |py| Some(py.to_string()));

    for (id, name, official, zero_o) in [
        ("flypy", "小鹤", Some(OFFLY), false),
        ("mspy", "微软", Some(OFMS), true),
        ("zrm", "自然码", None, false),
        ("sogou", "搜狗", None, true),
    ] {
        println!("\n== {name} ({id}) ==");
        e.set_shuangpin_scheme(id);
        println!("  引擎方案 = {}", e.shuangpin_scheme_id());
        run_set(&mut e, "引擎表(自家)", |py| encode_engine(id, py));
        if let Some(tbl) = official {
            run_set(&mut e, "官方表(RIME)", |py| encode_official(tbl, zero_o, py));
        }
    }

    // 单键顶码 + 奇数键 + 长串
    println!("\n== 边界行为（小鹤）==");
    e.set_shuangpin_scheme("flypy");
    for k in ["v", "s", "n", "d", "a"] {
        let (n, top) = feed(&mut e, k);
        println!("  单键 {k}: 候选 {n} 前3 {:?}", &top[..top.len().min(3)]);
    }
    let (n, top) = feed(&mut e, "vs"); // zhong
    println!("  vs(=zhong): {n} {:?}", &top[..top.len().min(3)]);
    let (n, top) = feed(&mut e, "vsv"); // 奇数键：vsv
    println!("  vsv(奇数键): {n} {:?}", &top[..top.len().min(3)]);
    let t = std::time::Instant::now();
    let (n, _) = feed(&mut e, "vegevegevegevege");
    println!("  16 键 vege×4: {n} 候选, 耗时 {}ms", t.elapsed().as_millis());
}
