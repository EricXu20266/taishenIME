//! 双拼第三批：小鹤逐词候选明细 + 微软键位差异字对照
use taishen_engine::pinyin::split_first_syllable;
use taishen_engine::shuangpin;

fn feed(e: &mut taishen_engine::Engine, code: &str, k: usize) -> (usize, Vec<String>) {
    e.reset();
    for ch in code.chars() {
        e.process_key(ch);
    }
    let n = e.candidate_count();
    let top = (0..n.min(k))
        .filter_map(|i| e.candidate(i).map(|s| s.to_string()))
        .collect();
    (n, top)
}

fn enc(id: &str, py: &str) -> String {
    let s = shuangpin::find_scheme(id).unwrap();
    let mut out = String::new();
    let mut rest = py;
    while !rest.is_empty() {
        let (syl, n) = split_first_syllable(rest).unwrap();
        rest = n;
        out.push_str(&s.encode(syl).unwrap());
    }
    out
}

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

/// 权威微软双拼韵母表（= 自然码改 ing→; ü→Y）
const MS_AUTH: &[(&str, char)] = &[
    ("iu", 'q'), ("ia", 'w'), ("ua", 'w'), ("er", 'r'), ("uan", 'r'), ("ue", 't'),
    ("ve", 't'), ("v", 'y'), ("uai", 'y'), ("uo", 'o'), ("un", 'p'), ("iong", 's'),
    ("ong", 's'), ("iang", 'd'), ("uang", 'd'), ("en", 'f'), ("eng", 'g'), ("ang", 'h'),
    ("ian", 'm'), ("an", 'j'), ("iao", 'c'), ("ao", 'k'), ("ai", 'l'), ("ei", 'z'),
    ("ie", 'x'), ("ui", 'v'), ("ou", 'b'), ("in", 'n'), ("ing", ';'), ("a", 'a'),
    ("e", 'e'), ("i", 'i'), ("o", 'o'), ("u", 'u'),
];

fn enc_auth(py: &str) -> String {
    let mut out = String::new();
    let mut rest = py;
    while !rest.is_empty() {
        let (syl, n) = split_first_syllable(rest).unwrap();
        rest = n;
        let (sheng, yun) = split_syl(syl);
        match sheng {
            "zh" => out.push('v'),
            "ch" => out.push('i'),
            "sh" => out.push('u'),
            "" => out.push('o'),
            s => out.push_str(s),
        }
        let key = if let Some((_, k)) = MS_AUTH.iter().find(|(y, _)| *y == yun) {
            *k
        } else if yun == "u" {
            'y'
        } else {
            '?'
        };
        out.push(key);
    }
    out
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

    println!("== 小鹤 34 词逐条（top6）==");
    e.set_shuangpin_scheme("flypy");
    for (w, py) in [
        ("这个", "zhege"), ("我们", "women"), ("中国", "zhongguo"), ("你好", "nihao"),
        ("测试", "ceshi"), ("双拼", "shuangpin"), ("输入法", "shurufa"), ("电脑", "diannao"),
        ("手机", "shouji"), ("工作", "gongzuo"), ("时间", "shijian"), ("问题", "wenti"),
        ("学习", "xuexi"), ("朋友", "pengyou"), ("可以", "keyi"), ("没有", "meiyou"),
        ("什么", "shenme"), ("为什么", "weishenme"), ("谢谢", "xiexie"), ("今天", "jintian"),
        ("明天", "mingtian"), ("昨天", "zuotian"), ("需要", "xuyao"), ("应该", "yinggai"),
        ("系统", "xitong"), ("程序", "chengxu"), ("软件", "ruanjian"), ("硬件", "yingjian"),
        ("女", "nv"), ("绿", "lv"), ("去", "qu"), ("安", "an"), ("爱", "ai"), ("昂", "ang"),
    ] {
        let code = enc("flypy", py);
        let (_, top) = feed(&mut e, &code, 6);
        let hit = if top.first().map(|s| s.as_str()) == Some(w) { "✓" } else { "✗" };
        println!("  {hit} {w:<5} {code:<10} {:?}", top);
    }

    println!("\n== 微软键位差异字：权威码 vs 引擎码 ==");
    e.set_shuangpin_scheme("mspy");
    for (w, py) in [
        ("家", "jia"), ("别", "bie"), ("新", "xin"), ("星", "xing"),
        ("快", "kuai"), ("论", "lun"), ("女", "nv"), ("花", "hua"),
        ("外面", "waimian"), ("解决", "jiejue"), ("文化", "wenhua"),
    ] {
        let auth = enc_auth(py);
        let mine = enc("mspy", py);
        let (_, t1) = feed(&mut e, &auth, 4);
        let (_, t2) = feed(&mut e, &mine, 4);
        println!("  {w}: 权威 {auth} → {:?} | 引擎 {mine} → {:?}", t1, t2);
    }
}
