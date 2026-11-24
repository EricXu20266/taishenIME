//! 模糊音模块 — 拼写变体（容错拼写，Q2.1 重构为 9 组独立开关 + 音节边界替换）
//!
//! 借鉴 RIME 的 Spelling Algebra 思路：允许拼音输入中的细微发音混用。
//! 默认 9 组全部关闭；开启对应分组后，仅在音节的「声母」或「韵母」边界做一次替换，
//! 再校验产出音节合法性，彻底消除旧版子串替换产出的 `zhhang`、`zhangg`、`fal` 等非法串。

use std::collections::HashSet;

pub const FUZZY_NONE: u32 = 0;
pub const FUZZY_Z_ZH: u32 = 1 << 0;
pub const FUZZY_C_CH: u32 = 1 << 1;
pub const FUZZY_S_SH: u32 = 1 << 2;
pub const FUZZY_AN_ANG: u32 = 1 << 3;
pub const FUZZY_EN_ENG: u32 = 1 << 4;
pub const FUZZY_IN_ING: u32 = 1 << 5;
pub const FUZZY_N_L: u32 = 1 << 6;
pub const FUZZY_F_H: u32 = 1 << 7;
pub const FUZZY_R_L: u32 = 1 << 8;
pub const FUZZY_ALL: u32 = 0x1FF;

/// 是否为可能在给定开关掩码下产生模糊音变体的输入串（零分配快速预判）。
pub fn may_have_fuzzy_with_flags(input: &str, flags: u32) -> bool {
    let flags = flags & FUZZY_ALL;
    if flags == 0 || input.is_empty() {
        return false;
    }
    let bytes = input.as_bytes();
    if (flags & FUZZY_Z_ZH) != 0 && bytes.contains(&b'z') {
        return true;
    }
    if (flags & FUZZY_C_CH) != 0 && bytes.contains(&b'c') {
        return true;
    }
    if (flags & FUZZY_S_SH) != 0 && bytes.contains(&b's') {
        return true;
    }
    if (flags & FUZZY_AN_ANG) != 0 && input.contains("an") {
        return true;
    }
    if (flags & FUZZY_EN_ENG) != 0 && input.contains("en") {
        return true;
    }
    if (flags & FUZZY_IN_ING) != 0 && input.contains("in") {
        return true;
    }
    if (flags & FUZZY_N_L) != 0 && (bytes.contains(&b'n') || bytes.contains(&b'l')) {
        return true;
    }
    if (flags & FUZZY_F_H) != 0 && (bytes.contains(&b'f') || bytes.contains(&b'h')) {
        return true;
    }
    if (flags & FUZZY_R_L) != 0 && (bytes.contains(&b'r') || bytes.contains(&b'l')) {
        return true;
    }
    false
}

/// 兼容旧签名：按 9 组全开判定是否存在模糊音触发条件。
pub fn may_have_fuzzy(input: &str) -> bool {
    may_have_fuzzy_with_flags(input, FUZZY_ALL)
}

/// 拆出合法音节（或疑似单音节）的 `(声母, 韵母)`：优先匹配双字母翘舌声母。
fn split_initial_final(syl: &str) -> (&str, &str) {
    if syl.starts_with("zh") || syl.starts_with("ch") || syl.starts_with("sh") {
        return (&syl[..2], &syl[2..]);
    }
    if let Some(&b) = syl.as_bytes().first()
        && matches!(
            b,
            b'b' | b'p'
                | b'm'
                | b'f'
                | b'd'
                | b't'
                | b'n'
                | b'l'
                | b'g'
                | b'k'
                | b'h'
                | b'j'
                | b'q'
                | b'x'
                | b'r'
                | b'z'
                | b'c'
                | b's'
                | b'y'
                | b'w'
        )
    {
        return (&syl[..1], &syl[1..]);
    }
    ("", syl)
}

fn push_if_valid_syllable(out: &mut Vec<String>, cand: String) {
    if crate::pinyin::is_valid_syllable(&cand) && !out.contains(&cand) {
        out.push(cand);
    }
}

fn swap_final_suffix(fin: &str, long_sfx: &str, short_sfx: &str) -> Option<String> {
    if let Some(stem) = fin.strip_suffix(long_sfx) {
        Some(format!("{stem}{short_sfx}"))
    } else {
        fin.strip_suffix(short_sfx)
            .map(|stem| format!("{stem}{long_sfx}"))
    }
}

/// 在单个音节内按「声母边界」和「韵母鼻音尾边界」生成一级模糊音变体。
fn syllable_variants(syl: &str, flags: u32, out: &mut Vec<String>) {
    out.clear();
    if flags == 0 || syl.is_empty() {
        return;
    }
    let (ini, fin) = split_initial_final(syl);

    // 1. 声母边界互换（绝不修改韵母内部字母）
    let mut try_swap_ini = |a: &str, b: &str| {
        let next = if ini == a {
            b
        } else if ini == b {
            a
        } else {
            return;
        };
        push_if_valid_syllable(out, format!("{next}{fin}"));
    };
    if (flags & FUZZY_Z_ZH) != 0 {
        try_swap_ini("z", "zh");
    }
    if (flags & FUZZY_C_CH) != 0 {
        try_swap_ini("c", "ch");
    }
    if (flags & FUZZY_S_SH) != 0 {
        try_swap_ini("s", "sh");
    }
    if (flags & FUZZY_N_L) != 0 {
        try_swap_ini("n", "l");
    }
    if (flags & FUZZY_F_H) != 0 {
        try_swap_ini("f", "h");
    }
    if (flags & FUZZY_R_L) != 0 {
        try_swap_ini("r", "l");
    }

    // 2. 韵母尾边界互换（优先剥除 ang/eng/ing，防止产出 zhangg/zhhang）
    for (bit, long_sfx, short_sfx) in [
        (FUZZY_AN_ANG, "ang", "an"),
        (FUZZY_EN_ENG, "eng", "en"),
        (FUZZY_IN_ING, "ing", "in"),
    ] {
        if (flags & bit) != 0
            && let Some(new_fin) = swap_final_suffix(fin, long_sfx, short_sfx)
        {
            push_if_valid_syllable(out, format!("{ini}{new_fin}"));
        }
    }
}

fn push_unique_variant(
    variants: &mut Vec<String>,
    seen: &mut HashSet<String>,
    input: &str,
    cand: String,
) {
    if cand != input && seen.insert(cand.clone()) {
        variants.push(cand);
    }
}

/// 按给定 9 组独立开关掩码，对输入拼音做音节边界一级模糊音替换（最多改一个音节）。
pub fn fuzzy_variants_with_flags(input: &str, flags: u32) -> Vec<String> {
    let flags = flags & FUZZY_ALL;
    if flags == 0 || input.is_empty() {
        return Vec::new();
    }

    let mut variants = Vec::new();
    let mut seen = HashSet::new();
    let mut tmp = Vec::new();

    // 优先按完整音节切分逐音节做声/韵母边界替换
    let mut stack_buf = [""; 16];
    let segmented_count = crate::pinyin::split_syllables_into(input, &mut stack_buf);
    let long_seg: Option<Vec<&str>> = if segmented_count.is_none() && input.len() > 63 {
        crate::pinyin::split_syllables_slices(input)
    } else {
        None
    };
    let syls: Option<&[&str]> = match (&segmented_count, &long_seg) {
        (Some(n), _) => Some(&stack_buf[..*n]),
        (None, Some(v)) => Some(v.as_slice()),
        _ => None,
    };

    if let Some(syls) = syls {
        for i in 0..syls.len() {
            syllable_variants(syls[i], flags, &mut tmp);
            for rep in &tmp {
                let mut joined = String::with_capacity(input.len() + 2);
                for (j, &s) in syls.iter().enumerate() {
                    joined.push_str(if j == i { rep.as_str() } else { s });
                }
                push_unique_variant(&mut variants, &mut seen, input, joined);
            }
        }
        return variants;
    }

    // 兜底：切分失败的疑似模糊拼写单音节（如 suang→shuang、fong→hong、ho→fo），
    // 非法非字母或无合法变体串（如 uoq）仍直接被 is_valid_syllable 拒掉。
    if input.len() <= 7 && input.bytes().all(|b| b.is_ascii_lowercase()) {
        syllable_variants(input, flags, &mut tmp);
        for cand in tmp.drain(..) {
            push_unique_variant(&mut variants, &mut seen, input, cand);
        }
    }
    variants
}

/// 兼容旧签名：按 9 组全开生成输入串的一级音节边界模糊音变体。
pub fn fuzzy_variants(input: &str) -> Vec<String> {
    fuzzy_variants_with_flags(input, FUZZY_ALL)
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
        assert!(!may_have_fuzzy_with_flags("zang", FUZZY_NONE));
    }

    #[test]
    fn test_q2_audit_zhang_and_fan_boundaries() {
        let zhang = fuzzy_variants("zhang");
        assert!(zhang.contains(&"zang".to_string()), "got {zhang:?}");
        assert!(zhang.contains(&"zhan".to_string()), "got {zhang:?}");
        assert!(!zhang.contains(&"zhhang".to_string()), "got {zhang:?}");
        assert!(!zhang.contains(&"zhangg".to_string()), "got {zhang:?}");
        assert!(!zhang.contains(&"zhalg".to_string()), "got {zhang:?}");

        let fan = fuzzy_variants("fan");
        assert!(fan.contains(&"fang".to_string()), "got {fan:?}");
        assert!(fan.contains(&"han".to_string()), "got {fan:?}");
        assert!(!fan.contains(&"fal".to_string()), "got {fan:?}");
    }

    #[test]
    fn test_z_zh_variant() {
        let variants = fuzzy_variants("zang");
        assert!(variants.contains(&"zhang".to_string()), "got {variants:?}");
        assert!(variants.contains(&"zan".to_string()), "got {variants:?}");
    }

    #[test]
    fn test_n_l_variant() {
        let variants = fuzzy_variants("nian");
        assert!(variants.contains(&"lian".to_string()), "got {variants:?}");
        assert!(variants.contains(&"niang".to_string()), "got {variants:?}");
    }

    #[test]
    fn test_multi_syllable_boundary_isolation() {
        assert_eq!(fuzzy_variants("zongguo"), vec!["zhongguo"]);
        let shanghai = fuzzy_variants("shanghai");
        assert!(
            !shanghai.contains(&"sfanghai".to_string()),
            "got {shanghai:?}"
        );
        assert!(
            !shanghai.contains(&"shhanghai".to_string()),
            "got {shanghai:?}"
        );
        assert!(
            !shanghai.contains(&"shangfai".to_string()),
            "got {shanghai:?}"
        );
        assert!(
            shanghai.contains(&"sanghai".to_string()),
            "got {shanghai:?}"
        );
        assert!(
            shanghai.contains(&"shanhai".to_string()),
            "got {shanghai:?}"
        );
    }

    #[test]
    fn test_independent_flags() {
        assert!(fuzzy_variants_with_flags("zhang", FUZZY_NONE).is_empty());
        assert_eq!(fuzzy_variants_with_flags("zhang", FUZZY_Z_ZH), vec!["zang"]);
        assert_eq!(
            fuzzy_variants_with_flags("zhang", FUZZY_AN_ANG),
            vec!["zhan"]
        );
        assert!(fuzzy_variants_with_flags("fan", FUZZY_N_L).is_empty());
        assert_eq!(fuzzy_variants_with_flags("fan", FUZZY_F_H), vec!["han"]);
        assert_eq!(fuzzy_variants_with_flags("ran", FUZZY_R_L), vec!["lan"]);
    }

    #[test]
    fn test_fallback_fuzzy_only_syllables_and_invalid() {
        assert!(fuzzy_variants("suang").contains(&"shuang".to_string()));
        assert!(fuzzy_variants("fong").contains(&"hong".to_string()));
        assert!(fuzzy_variants("ho").contains(&"fo".to_string()));
        assert!(fuzzy_variants("uoq").is_empty());
    }

    #[test]
    fn test_no_duplicate() {
        for sample in ["zang", "zhang", "fan", "shanghai", "zongguo"] {
            let variants = fuzzy_variants(sample);
            let unique: HashSet<&str> = variants.iter().map(String::as_str).collect();
            assert_eq!(
                variants.len(),
                unique.len(),
                "重复变体 in {sample}: {variants:?}"
            );
        }
    }
}
