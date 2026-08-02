/// 双拼模块 — 双拼方案支持
///
/// 借鉴 RIME 的多方案双拼。双拼：一个汉字 = 声母键 + 韵母键（共两键）。
/// 方案：微软双拼（主流方案，Win10 自带输入法默认）
///
/// 编码规则（微软双拼）：
///   - 声母：zh→v, ch→i, sh→u，其余与全拼同键
///   - 韵母：映射到单个键（如 a→a, ai→l, an→j, ang→h, ...）
///   - 零声母音节：以 'o' 开头（如 "a"→"oa"、"ai"→"ol"）
///
/// 注意：微软双拼存在一键多韵母（如 k=ao/ing），decode 返回所有可能全拼。

/// 微软双拼声母 → 键
const SHENGMU_MAP: &[(&str, char)] = &[
    ("zh", 'v'), ("ch", 'i'), ("sh", 'u'),
];

/// 微软双拼韵母 → 键（正向：全拼→双拼）
const YUNMU_TO_KEY: &[(&str, char)] = &[
    ("a", 'a'), ("ai", 'l'), ("an", 'j'), ("ang", 'h'),
    ("ao", 'k'), ("e", 'e'), ("ei", 'z'), ("en", 'f'),
    ("eng", 'g'), ("er", 'r'), ("i", 'i'), ("ia", 'x'),
    ("ian", 'm'), ("iang", 'd'), ("iao", 'c'), ("ie", 'p'),
    ("in", 'b'), ("ing", 'k'), ("iong", 's'), ("iu", 'q'),
    ("o", 'o'), ("ong", 's'), ("ou", 'b'), ("u", 'u'),
    ("ua", 'x'), ("uai", 'k'), ("uan", 'r'), ("uang", 'd'),
    ("ue", 't'), ("ui", 'v'), ("un", 'y'), ("uo", 'o'),
    ("v", 'v'), ("ve", 't'),
];

/// 微软双拼键 → 韵母列表（反向，一键可多韵母）
const KEY_TO_YUNMU: &[(char, &[&str])] = &[
    ('a', &["a"]),
    ('b', &["in", "ou"]),
    ('c', &["iao"]),
    ('d', &["iang", "uang"]),
    ('e', &["e"]),
    ('f', &["en"]),
    ('g', &["eng"]),
    ('h', &["ang"]),
    ('i', &["i"]),
    ('j', &["an"]),
    ('k', &["ao", "ing"]),
    ('l', &["ai"]),
    ('m', &["ian"]),
    ('o', &["o", "uo"]),
    ('p', &["ie"]),
    ('q', &["iu"]),
    ('r', &["er", "uan"]),
    ('s', &["iong", "ong"]),
    ('t', &["ue", "ve"]),
    ('u', &["u"]),
    ('v', &["ui", "v"]),
    ('x', &["ia", "ua"]),
    ('y', &["un"]),
    ('z', &["ei"]),
];

/// 单字母声母集合
const SINGLE_SHENGMU: &str = "bpmfdtnlgkhjqxrzcsyw";

/// 是否为双拼模式下的合法声母键（v/i/u = zh/ch/sh，其余单声母）
fn is_shengmu_key(c: char) -> bool {
    c == 'v' || c == 'i' || c == 'u' || SINGLE_SHENGMU.contains(c)
}

pub mod codec {
    use super::*;

    /// 将单个拼音音节编码为双拼（正向，测试/显示用）。
    /// 零声母音节以 'o' 开头。
    pub fn encode(syllable: &str) -> Option<String> {
        if syllable.is_empty() {
            return None;
        }
        let (sheng, yun) = split(syllable);
        let mut code = String::with_capacity(2);
        if sheng.is_empty() {
            code.push('o');
        } else {
            match SHENGMU_MAP.iter().find(|(s, _)| *s == sheng) {
                Some((_, c)) => code.push(*c),
                None => code.push(sheng.chars().next().unwrap()),
            }
        }
        match YUNMU_TO_KEY.iter().find(|(y, _)| *y == yun) {
            Some((_, c)) => code.push(*c),
            None => code.push(yun.chars().next().unwrap()),
        }
        Some(code)
    }

    /// 切分音节为 (声母, 韵母)。零声母返回 ("", 全音节)。
    pub fn split(syllable: &str) -> (&str, &str) {
        for (sheng, _) in SHENGMU_MAP {
            if syllable.starts_with(sheng) && syllable.len() > sheng.len() {
                return (sheng, &syllable[sheng.len()..]);
            }
        }
        if syllable.len() > 1 {
            let first = &syllable[..1];
            if SINGLE_SHENGMU.contains(first) {
                return (first, &syllable[1..]);
            }
        }
        ("", syllable)
    }

    /// 解码单个双拼码（1-2 键）为可能的全拼音节。
    /// 双键：首键为声母键（或零声母 'o'），次键为韵母键 → 组合全拼。
    /// 单键：可能是完整零声母韵母键（如 "a"），或声母键前缀（如 "v"=zh）。
    /// 返回所有可能全拼（含歧义，如 "vs" → ["zhong"]，"kb" → ["kao","kou"]）。
    pub fn decode(code: &str) -> Vec<String> {
        let chars: Vec<char> = code.chars().collect();
        let mut result = Vec::new();

        if chars.len() == 1 {
            let c = chars[0];
            // 单键：可能是零声母韵母键（完整音节），或声母键（前缀）
            if let Some(yunmus) = KEY_TO_YUNMU.iter().find(|(k, _)| *k == c) {
                for y in yunmus.1 {
                    result.push(y.to_string()); // 如 "a" → "a", "l" → "ai"
                }
            }
            // 声母键前缀：v→zh, i→ch, u→sh, 单声母→自身
            match c {
                'v' => result.push("zh".to_string()),
                'i' => result.push("ch".to_string()),
                'u' => result.push("sh".to_string()),
                _ => {
                    if SINGLE_SHENGMU.contains(c) {
                        result.push(c.to_string());
                    }
                }
            }
            return result;
        }

        if chars.len() == 2 {
            let (sheng_key, yun_key) = (chars[0], chars[1]);
            // 零声母：首键 'o'，次键为韵母键
            if sheng_key == 'o' {
                if let Some(yunmus) = KEY_TO_YUNMU.iter().find(|(k, _)| *k == yun_key) {
                    for y in yunmus.1 {
                        result.push(y.to_string());
                    }
                }
                return result;
            }
            // 声母键 + 韵母键
            if !is_shengmu_key(sheng_key) {
                return result; // 非法首键
            }
            let sheng_owned: String = match sheng_key {
                'v' => "zh".to_string(),
                'i' => "ch".to_string(),
                'u' => "sh".to_string(),
                _ => sheng_key.to_string(),
            };
            if let Some(yunmus) = KEY_TO_YUNMU.iter().find(|(k, _)| *k == yun_key) {
                for y in yunmus.1 {
                    result.push(format!("{}{}", sheng_owned, y));
                }
            }
            return result;
        }

        result
    }

    /// 将双拼码串切分为音节解码（成对切分，最后不足 2 键作为前缀）。
    /// 返回所有可能的全拼组合（每音节取一个候选，笛卡尔积）。
    /// 输入 "vsni" → zhong + ni → ["zhongni"]
    /// 输入 "v"   → 前缀 ["zh"]（不完整音节）
    pub fn decode_string(input: &str) -> Vec<String> {
        let mut results = vec![String::new()];
        let chars: Vec<char> = input.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            // 取 2 键音节（若只剩 1 键则作为前缀）
            let end = (i + 2).min(chars.len());
            let code: String = chars[i..end].iter().collect();
            let options = decode(&code);
            if options.is_empty() {
                return Vec::new(); // 无法解码
            }
            let mut next = Vec::new();
            for prefix in &results {
                for opt in &options {
                    next.push(format!("{}{}", prefix, opt));
                }
            }
            results = next;
            i = end;
            // 若最后只剩 1 键，decode 返回的可能是声母前缀（如 "v"→zh）或完整零声母
        }
        results
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_split() {
            assert_eq!(split("zhong"), ("zh", "ong"));
            assert_eq!(split("ni"), ("n", "i"));
            assert_eq!(split("hao"), ("h", "ao"));
            assert_eq!(split("ai"), ("", "ai"));
        }

        #[test]
        fn test_encode_zhong() {
            // zhong: zh→v, ong→s → "vs"
            assert_eq!(encode("zhong").as_deref(), Some("vs"));
        }

        #[test]
        fn test_encode_ni() {
            // ni: n→n, i→i → "ni"
            assert_eq!(encode("ni").as_deref(), Some("ni"));
        }

        #[test]
        fn test_encode_zero_initial() {
            // ai: 零声母 → o + l(ai) → "ol"
            assert_eq!(encode("ai").as_deref(), Some("ol"));
        }

        #[test]
        fn test_decode_zhong() {
            // vs → zhong
            let decoded = decode("vs");
            assert!(decoded.iter().any(|d| d == "zhong"),
                    "vs 应解码出 zhong, got {:?}", decoded);
        }

        #[test]
        fn test_decode_ambiguous() {
            // kb → k(声母) + b(韵母 in/ou) → "kin" / "kou"
            let decoded = decode("kb");
            assert!(decoded.iter().any(|d| d == "kin"), "got {:?}", decoded);
            assert!(decoded.iter().any(|d| d == "kou"), "got {:?}", decoded);
        }

        #[test]
        fn test_decode_zero_initial() {
            // ol → ai（零声母 o + l=ai）
            let decoded = decode("ol");
            assert!(decoded.iter().any(|d| d == "ai"),
                    "ol 应解码出 ai, got {:?}", decoded);
        }

        #[test]
        fn test_decode_string() {
            // vs + ni → zhongni
            let decoded = decode_string("vsni");
            assert!(decoded.iter().any(|d| d == "zhongni"),
                    "vsni 应解码出 zhongni, got {:?}", decoded);
        }

        #[test]
        fn test_decode_prefix() {
            // v → zh（声母前缀）
            let decoded = decode("v");
            assert!(decoded.iter().any(|d| d == "zh"),
                    "v 应解码出 zh, got {:?}", decoded);
        }
    }
}

