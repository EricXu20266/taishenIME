/// 智能纠错模块 — 键盘相邻键位容错（V0.2.10）
///
/// 快速打字时误触相邻键（logn→long→龙、nihap→nihao→你好）。
/// 与模糊音（fuzzy.rs，发音相近）不同，这里是**键位相邻**（手指误触层面）。
///
/// 两类变体：
///   1. 单键替换：任一位字符 → 相邻键（覆盖误触）
///   2. 相邻交换：任两位相邻字符互换（覆盖打字顺序错，logn→long 即 n/g 交换）
use std::collections::HashMap;
use std::sync::OnceLock;

/// QWERTY 键盘相邻键映射（每行相邻 + 上下行斜邻）
fn build_nearby_map() -> HashMap<char, Vec<char>> {
    // 三行键位
    let row1 = ['q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p'];
    let row2 = ['a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l'];
    let row3 = ['z', 'x', 'c', 'v', 'b', 'n', 'm'];

    let mut map: HashMap<char, Vec<char>> = HashMap::new();

    // 同行左右相邻
    for row in [row1.as_slice(), row2.as_slice(), row3.as_slice()] {
        for i in 0..row.len() {
            let ch = row[i];
            let entry = map.entry(ch).or_default();
            if i > 0 {
                entry.push(row[i - 1]);
            }
            if i + 1 < row.len() {
                entry.push(row[i + 1]);
            }
        }
    }

    // 上下行斜邻：上一行键与下一行键的关系
    // row2 键的正上方 + 左上/右上（row1），正下方 + 左下/右下（row3）
    let upper_of_row2 = ['q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p'];
    let lower_of_row2 = ['z', 'x', 'c', 'v', 'b', 'n', 'm'];
    // row2[i] 上方是 row1[i] 和 row1[i+1]（斜邻）
    for i in 0..row2.len() {
        let ch = row2[i];
        let entry = map.entry(ch).or_default();
        // 上方：row1[i]、row1[i+1]（若存在）
        if i < upper_of_row2.len() {
            entry.push(upper_of_row2[i]);
        }
        if i + 1 < upper_of_row2.len() {
            entry.push(upper_of_row2[i + 1]);
        }
        // 下方：row3[i-1]、row3[i]（若存在）
        if i > 0 && i - 1 < lower_of_row2.len() {
            entry.push(lower_of_row2[i - 1]);
        }
        if i < lower_of_row2.len() {
            entry.push(lower_of_row2[i]);
        }
    }
    // row1 键的下方是 row2（正下 + 斜下）
    for i in 0..row1.len() {
        let ch = row1[i];
        let entry = map.entry(ch).or_default();
        if i < row2.len() {
            entry.push(row2[i]);
        }
        if i > 0 && i - 1 < row2.len() {
            entry.push(row2[i - 1]);
        }
    }
    // row3 键的上方是 row2（正上 + 斜上）
    for i in 0..row3.len() {
        let ch = row3[i];
        let entry = map.entry(ch).or_default();
        // row3[i] 上方是 row2[i] 和 row2[i+1]
        if i < row2.len() {
            entry.push(row2[i]);
        }
        if i + 1 < row2.len() {
            entry.push(row2[i + 1]);
        }
    }

    // 去重 + 排序（确定性输出）
    for entry in map.values_mut() {
        entry.sort_unstable();
        entry.dedup();
    }
    map
}

/// 获取相邻键映射（惰性构建）
pub fn nearby_map() -> &'static HashMap<char, Vec<char>> {
    static MAP: OnceLock<HashMap<char, Vec<char>>> = OnceLock::new();
    MAP.get_or_init(build_nearby_map)
}

/// 查询单个键的相邻键
pub fn nearby_keys(ch: char) -> Vec<char> {
    nearby_map().get(&ch).cloned().unwrap_or_default()
}

/// 最大变体数（防膨胀）
const MAX_VARIANTS: usize = 24;

/// 生成纠错变体：相邻交换优先 + 单键替换，去重，最多 MAX_VARIANTS 个（不含原串）
pub fn correction_variants(input: &str) -> Vec<String> {
    if input.len() < 3 {
        return Vec::new();
    }
    let mut variants: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // 1. 相邻交换优先（打字顺序错的最高频错因，logn→long）
    let chars: Vec<char> = input.chars().collect();
    for i in 0..chars.len().saturating_sub(1) {
        let mut v: String = chars[..i].iter().collect();
        v.push(chars[i + 1]);
        v.push(chars[i]);
        v.extend(chars[i + 2..].iter());
        if v != input && !seen.contains(&v) {
            seen.insert(v.clone());
            variants.push(v);
            if variants.len() >= MAX_VARIANTS {
                return variants;
            }
        }
    }

    // 2. 单键替换：任一位字符 → 相邻键
    for (i, ch) in input.char_indices() {
        for near in nearby_keys(ch) {
            let mut v = String::with_capacity(input.len());
            v.push_str(&input[..i]);
            v.push(near);
            v.push_str(&input[i + ch.len_utf8()..]);
            if v != input && !seen.contains(&v) {
                seen.insert(v.clone());
                variants.push(v);
                if variants.len() >= MAX_VARIANTS {
                    return variants;
                }
            }
        }
    }

    variants
}

/// 是否需要纠错（快速预判：长度 >= 3 且含可替换字符）
pub fn may_need_correction(input: &str) -> bool {
    if input.len() < 3 {
        return false;
    }
    // 只要长度足够就尝试（避免复杂预判）；空串/纯符号不处理
    input.chars().all(|c| c.is_ascii_alphabetic())
}

// ─────────────────────────────────────────────────────────────
// V0.3.x 拼写纠错（对标 rime-ice speller algebra 的 derive 规则）
// 雾凇的"自动纠错"是拼音拼写错误纠正（非按键相邻）：
//   - zh/ch/sh 声母错位：hzi→zhi、zih→zhi
//   - 韵母写反：wia→wai、wie→wei、jei→jie、oa→ao、uo→ou
//   - 后鼻音错位：ang→nag/agn、eng→neg/egn、ing→nig/ign、ong→nog/ogn
//   - 复合韵母错位：iao→ioa/oia、ui↔iu、iang→aing/inag、ua→au、uai→aui、
//     uan→aun、ue→eu、uang→aung/uagn/unag/augn、iong→inog/oing/iogn/oign
//   - 其他：do→dou/dong、lon→long、ten→teng、lng→lang/leng/ling/long
// 这些模式是拼音特有的，对英文单词天然不匹配（hello 不产生任何变体）——
// 因此不受 is_full_pinyin 限制（与按键纠错不同）。
// ─────────────────────────────────────────────────────────────

/// 后缀替换规则：(前缀末字符类, 错误后缀, 正确后缀)
/// 输入以错误后缀结尾且前缀末字符在类中 → 错误后缀替换为正确后缀。
/// 例：("wia") → 末字符 w ∈ [wghk]，ia→ai → "wai"
const SUFFIX_RULES: &[(&str, &str, &str)] = &[
    ("wghk", "ia", "ai"),   // wia → wai
    ("wfghkz", "ie", "ei"), // wie → wei
    ("jqx", "ei", "ie"),    // jei → jie
    ("rtypsdghklzcbnm", "oa", "ao"),
    ("ypfm", "uo", "ou"),
    ("wrtypsdfghklzcbnm", "nag", "ang"),
    ("wrtypsdfghklzcbnm", "agn", "ang"),
    ("wrtpsdfghklzcbnm", "neg", "eng"),
    ("wrtpsdfghklzcbnm", "egn", "eng"),
    ("qtypdjlxbnm", "nig", "ing"),
    ("qtypdjlxbnm", "ign", "ing"),
    ("rtysdghklzcn", "nog", "ong"),
    ("rtysdghklzcn", "ogn", "ong"),
    ("qtpdjlxbnm", "ioa", "iao"),
    ("qtpdjlxbnm", "oia", "iao"),
    ("rtsghkzc", "iu", "ui"), // dui 类（对）
    ("qjlxnm", "ui", "iu"),   // qiu 类（求）
    ("qjlxn", "aing", "iang"),
    ("qjlxn", "inag", "iang"),
    ("ghkshzh", "au", "ua"), // g/k/h/zh/sh 后
    ("ghkshzh", "aui", "uai"),
    ("qrtysdghjklzxcn", "aun", "uan"),
    ("nlyjqx", "eu", "ue"),
    ("ghkshzh", "aung", "uang"),
    ("ghkshzh", "uagn", "uang"),
    ("ghkshzh", "unag", "uang"),
    ("ghkshzh", "augn", "uang"),
    ("jqx", "inog", "iong"),
    ("jqx", "oing", "iong"),
    ("jqx", "iogn", "iong"),
    ("jqx", "oign", "iong"),
];

/// 拼写纠错变体（对标 rime derive 规则，拼音特有模式）
pub fn spelling_variants(input: &str) -> Vec<String> {
    if input.len() < 2 || input.len() > 6 {
        return Vec::new();
    }
    let b = input.as_bytes();
    let n = b.len();
    if !b.iter().all(|c| c.is_ascii_alphabetic()) {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    let mut push = |v: String| {
        if v != input && !out.contains(&v) {
            out.push(v);
        }
    };

    // ── zh/ch/sh 声母错位 ──
    // hzi → zhi / hci → chi / hsi → shi（h 误置于声母前）
    for i in 0..n.saturating_sub(2) {
        if b[i] == b'h' && matches!(b[i + 1], b'z' | b'c' | b's') {
            let mut v = String::with_capacity(n);
            v.push_str(&input[..i]);
            v.push(b[i + 1] as char);
            v.push('h');
            v.push_str(&input[i + 2..]);
            push(v);
        }
    }
    // zih → zhi / cih → chi / sih → shi（h 误置于末尾）
    for i in 0..n.saturating_sub(2) {
        if matches!(b[i], b'z' | b'c' | b's') && b[i + 1] == b'i' && b[i + 2] == b'h' {
            let mut v = String::with_capacity(n);
            v.push_str(&input[..i + 1]);
            v.push('h');
            v.push('i');
            v.push_str(&input[i + 3..]);
            push(v);
        }
    }

    // ── 后缀替换规则 ──
    for (cls, err, corr) in SUFFIX_RULES {
        if input.ends_with(err) {
            let prefix = &input[..n - err.len()];
            if let Some(&last) = prefix.as_bytes().last() {
                if cls.as_bytes().contains(&last) {
                    let mut v = String::with_capacity(n + corr.len() - err.len());
                    v.push_str(prefix);
                    v.push_str(corr);
                    push(v);
                }
            }
        }
    }

    // ── 单音节尾韵特殊规则 ──
    // do → dou / dong（rtsdghkzc + o 结尾，可能是 ou/ong 的简写）
    if n >= 2 && b[n - 1] == b'o' {
        let prefix = &input[..n - 1];
        if let Some(&last) = prefix.as_bytes().last() {
            if b"rtsdghkzc".contains(&last) {
                push(format!("{prefix}ou"));
                push(format!("{prefix}ong"));
            }
        }
    }
    // lon → long（on 结尾补 g）
    if n >= 3 && input.ends_with("on") && !input.ends_with("ong") {
        push(format!("{input}g"));
    }
    // ten → teng / len → leng（t/l + en 结尾补 g）
    if n >= 3 && input.ends_with("en") && !input.ends_with("eng") {
        let prefix = &input[..n - 2];
        if let Some(&last) = prefix.as_bytes().last() {
            if b"tl".contains(&last) {
                push(format!("{input}g"));
            }
        }
    }
    // lng → lang/leng/ling/long（[声母]+ng，缺中间韵母）
    if n == 3 && input.ends_with("ng") {
        let first = b[0];
        if b"qwrtypsdfghjklzxcbnm".contains(&first) {
            for v in [b'a', b'e', b'i', b'o'] {
                push(format!("{}{}{}", first as char, v as char, "ng"));
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nearby_keys() {
        // l 相邻：k、o、p（同行）+ 上方 o/p（row1）
        let l_near = nearby_keys('l');
        assert!(l_near.contains(&'k'), "l 相邻应含 k, got {l_near:?}");
        // o 相邻：i、p
        let o_near = nearby_keys('o');
        assert!(
            o_near.contains(&'i') && o_near.contains(&'p'),
            "o 相邻应含 i,p, got {o_near:?}"
        );
        // n 相邻：b、m（同行）+ 上方 h/j（row2）
        let n_near = nearby_keys('n');
        assert!(
            n_near.contains(&'b') && n_near.contains(&'m'),
            "n 相邻应含 b,m, got {n_near:?}"
        );
        assert!(
            n_near.contains(&'h'),
            "n 上方应含 h（斜邻）, got {n_near:?}"
        );
    }

    #[test]
    fn test_correction_swap_logn_to_long() {
        // logn → 相邻交换 n/g → long
        let variants = correction_variants("logn");
        assert!(
            variants.iter().any(|v| v == "long"),
            "logn 应生成 long 变体, got {variants:?}"
        );
    }

    #[test]
    fn test_correction_replace_nihap_to_nihao() {
        // nihap → p 误触（o 相邻）→ nihao
        let variants = correction_variants("nihap");
        assert!(
            variants.iter().any(|v| v == "nihao"),
            "nihap 应生成 nihao 变体, got {variants:?}"
        );
    }

    #[test]
    fn test_correction_short_input_no_variants() {
        // 短输入不纠错（误伤率高）
        assert!(correction_variants("ni").is_empty());
    }

    #[test]
    fn test_correction_no_duplicate() {
        let variants = correction_variants("nihao");
        let mut dedup = variants.clone();
        dedup.sort();
        dedup.dedup();
        assert_eq!(variants.len(), dedup.len(), "不应有重复变体");
    }

    #[test]
    fn test_may_need_correction() {
        assert!(may_need_correction("logn"));
        assert!(may_need_correction("nihap"));
        assert!(!may_need_correction("ni"));
        assert!(!may_need_correction(""));
    }

    // ── V0.3.x 拼写纠错（对标 rime-ice derive 规则）──

    #[test]
    fn test_spelling_wia_to_wai() {
        let v = spelling_variants("wia");
        assert!(v.iter().any(|s| s == "wai"), "wia 应纠为 wai, got {v:?}");
    }

    #[test]
    fn test_spelling_hzi_to_zhi() {
        let v = spelling_variants("hzi");
        assert!(v.iter().any(|s| s == "zhi"), "hzi 应纠为 zhi, got {v:?}");
    }

    #[test]
    fn test_spelling_zih_to_zhi() {
        let v = spelling_variants("zih");
        assert!(v.iter().any(|s| s == "zhi"), "zih 应纠为 zhi, got {v:?}");
    }

    #[test]
    fn test_spelling_lng_variants() {
        let v = spelling_variants("lng");
        for expected in ["lang", "leng", "ling", "long"] {
            assert!(
                v.iter().any(|s| s == expected),
                "lng 应含 {expected}, got {v:?}"
            );
        }
    }

    #[test]
    fn test_spelling_do_variants() {
        let v = spelling_variants("do");
        assert!(
            v.iter().any(|s| s == "dou") && v.iter().any(|s| s == "dong"),
            "do 应纠为 dou/dong, got {v:?}"
        );
    }

    #[test]
    fn test_spelling_ten_to_teng() {
        let v = spelling_variants("ten");
        assert!(v.iter().any(|s| s == "teng"), "ten 应纠为 teng, got {v:?}");
    }

    #[test]
    fn test_spelling_agn_to_ang() {
        let v = spelling_variants("zagn");
        assert!(v.iter().any(|s| s == "zang"), "zagn 应纠为 zang, got {v:?}");
    }

    #[test]
    fn test_spelling_english_safe() {
        // 英文单词不产生拼写变体（模式是拼音特有的）
        assert!(spelling_variants("hello").is_empty());
        assert!(spelling_variants("world").is_empty());
        assert!(spelling_variants("welcome").is_empty());
    }
}
