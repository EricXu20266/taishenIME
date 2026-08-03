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
}
