//! 双拼第二批诊断：全拼基线 / learn 污染 / 英文混输 / 自然码官方表
use taishen_engine::pinyin::split_first_syllable;
use taishen_engine::shuangpin;

fn feed(e: &mut taishen_engine::Engine, code: &str) -> (usize, Vec<String>) {
    e.reset();
    for ch in code.chars() {
        e.process_key(ch);
    }
    let n = e.candidate_count();
    let top = (0..n.min(6))
        .filter_map(|i| e.candidate(i).map(|s| s.to_string()))
        .collect();
    (n, top)
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

const OFZRM: &[(&str, char)] = &[
    ("iu", 'q'), ("ia", 'w'), ("ua", 'w'), ("uan", 'r'), ("van", 'r'), ("ing", 'y'),
    ("uai", 'y'), ("uo", 'o'), ("un", 'p'), ("vn", 'p'), ("iong", 's'), ("ong", 's'),
    ("iang", 'd'), ("uang", 'd'), ("en", 'f'), ("eng", 'g'), ("ang", 'h'), ("ian", 'm'),
    ("an", 'j'), ("iao", 'c'), ("ao", 'k'), ("ai", 'l'), ("ei", 'z'), ("ie", 'x'),
    ("ui", 'v'), ("ou", 'b'), ("in", 'n'), ("er", 'r'), ("ue", 't'), ("ve", 't'),
    ("a", 'a'), ("e", 'e'), ("i", 'i'), ("o", 'o'), ("u", 'u'), ("v", 'v'),
];

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
        let key = if let Some((_, k)) = y2k.iter().find(|(y, _)| *y == yun) {
            *k
        } else if yun == "u" {
            *y2k.iter().find(|(y, _)| *y == "v").map(|(_, k)| k)?
        } else {
            return None;
        };
        out.push(key);
    }
    Some(out)
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
    println!("ready={}", taishen_engine::dictionary::is_ready());

    let mut e = taishen_engine::Engine::new();

    println!("\n== 全拼基线（热词库）==");
    for (w, py) in [
        ("这个", "zhege"),
        ("中国", "zhongguo"),
        ("我们", "women"),
        ("输入法", "shurufa"),
        ("为什么", "weishenme"),
        ("系统", "xitong"),
    ] {
        let (n, top) = feed(&mut e, py);
        println!("  {w}={py}: n={n} top={:?}", &top[..top.len().min(4)]);
    }

    println!("\n== 自然码：官方表 vs 引擎表 ==");
    e.set_shuangpin_mode(true);
    e.set_shuangpin_scheme("zrm");
    for (w, py) in [("这个", "zhege"), ("中国", "zhongguo"), ("学习", "xuexi")] {
        let eng = shuangpin::find_scheme("zrm").unwrap();
        let mut code = String::new();
        let mut rest = py;
        while !rest.is_empty() {
            let (s, n) = split_first_syllable(rest).unwrap();
            rest = n;
            code.push_str(&eng.encode(s).unwrap());
        }
        let off = encode_official(OFZRM, false, py).unwrap();
        let (_, t1) = feed(&mut e, &code);
        let (_, t2) = feed(&mut e, &off);
        println!("  {w}: 引擎码 {code} → {:?} | 官方码 {off} → {:?}", &t1[..t1.len().min(3)], &t2[..t2.len().min(3)]);
    }

    println!("\n== learn 污染检查（小鹤）==");
    e.set_shuangpin_scheme("flypy");
    let (n, top) = feed(&mut e, "vs");
    println!("  vs 候选 n={n} top={:?}", &top[..top.len().min(4)]);
    let idx = (0..e.candidate_count()).find(|i| e.candidate(*i) == Some("中"));
    if let Some(i) = idx {
        let out = e.select_candidate(i);
        println!("  选中 = {:?}", out);
    }
    println!(
        "  user_word_count(\"vs\")={} \"zhong\"={}",
        taishen_engine::dictionary::user_word_count("vs"),
        taishen_engine::dictionary::user_word_count("zhong")
    );

    println!("\n== 双拼下的英文/数字/长输入 ==");
    for s in ["hello", "nihao", "abc"] {
        let (n, top) = feed(&mut e, s);
        println!("  {s}: n={n} top={:?}", &top[..top.len().min(3)]);
    }
}
