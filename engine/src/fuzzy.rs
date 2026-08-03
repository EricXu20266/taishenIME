/// 模糊音模块 — 拼写变体（容错拼写）
///
/// 借鉴 RIME 的 Spelling Algebra 思路：允许拼音输入中的细微变化。
/// 开启模糊音后，查询时对输入串生成容错变体（如 z↔zh、an↔ang、n↔l），
/// 变体命中词条补入候选（排在精确命中之后，不干扰精确输入）。
///
/// 常见模糊音组（RIME/主流输入法默认集合）：
///   - 平翘舌：z↔zh、c↔ch、s↔sh
///   - 前后鼻音：an↔ang、en↔eng、in↔ing、un↔ong
///   - n↔l
///   - f↔h
///   - r↔l

/// 双向模糊音规则组：每组内任意两个可互相替换
const FUZZY_GROUPS: &[&[&str]] = &[
    &["z", "zh"],
    &["c", "ch"],
    &["s", "sh"],
    &["an", "ang"],
    &["en", "eng"],
    &["in", "ing"],
    &["n", "l"],
    &["f", "h"],
    &["r", "l"],
];

/// 是否为可能产生模糊音变体的输入串（快速预判，避免无谓计算）
pub fn may_have_fuzzy(input: &str) -> bool {
    if input.is_empty() {
        return false;
    }
    FUZZY_GROUPS
        .iter()
        .any(|group| group.iter().any(|variant| input.contains(variant)))
}

/// 生成输入串的所有一级模糊音变体（只做一次替换，不递归）。
/// 返回去重后的变体列表（不含原串）。
pub fn fuzzy_variants(input: &str) -> Vec<String> {
    if input.is_empty() {
        return Vec::new();
    }
    let mut variants = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for group in FUZZY_GROUPS {
        for from in *group {
            // 找到 input 中所有 from 出现的位置
            let mut pos = 0;
            while let Some(rel) = input[pos..].find(from) {
                let start = pos + rel;
                let end = start + from.len();
                // 对组内其他变体做替换
                for to in *group {
                    if *to == *from {
                        continue;
                    }
                    let mut v = String::with_capacity(input.len() + 2);
                    v.push_str(&input[..start]);
                    v.push_str(to);
                    v.push_str(&input[end..]);
                    if !seen.contains(&v) && v != input {
                        seen.insert(v.clone());
                        variants.push(v);
                    }
                }
                pos = end;
            }
        }
    }
    variants
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_may_have_fuzzy() {
        assert!(may_have_fuzzy("zang"));
        assert!(may_have_fuzzy("zhong"));
        assert!(may_have_fuzzy("nin"));
        assert!(!may_have_fuzzy("uoq"));
    }

    #[test]
    fn test_z_zh_variant() {
        // zang → zhang（平翘舌）
        let variants = fuzzy_variants("zang");
        assert!(
            variants.iter().any(|v| v == "zhang"),
            "zang 应生成 zhang 变体, got {:?}",
            variants
        );
    }

    #[test]
    fn test_an_ang_variant() {
        // fan → fang（前后鼻音）
        let variants = fuzzy_variants("fan");
        assert!(
            variants.iter().any(|v| v == "fang"),
            "fan 应生成 fang 变体, got {:?}",
            variants
        );
    }

    #[test]
    fn test_n_l_variant() {
        // nian → lian（n/l 不分）
        let variants = fuzzy_variants("nian");
        assert!(
            variants.iter().any(|v| v == "lian"),
            "nian 应生成 lian 变体, got {:?}",
            variants
        );
    }

    #[test]
    fn test_no_variant_for_exact() {
        // 不含任何模糊组的串无变体
        let variants = fuzzy_variants("uoq");
        assert!(variants.is_empty());
    }

    #[test]
    fn test_no_duplicate() {
        let variants = fuzzy_variants("zang");
        let mut dedup = variants.clone();
        dedup.sort();
        dedup.dedup();
        assert_eq!(variants.len(), dedup.len(), "不应有重复变体");
    }
}
