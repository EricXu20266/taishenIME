/// 双拼模块 — 多方案支持（P2-7，对标 rime double_pinyin_*）
///
/// 双拼：一个汉字 = 声母键 + 韵母键（共两键）。
/// 方案：微软 mspy（默认）/ 小鹤 flypy / 搜狗 sogou / 自然码 zrm。
/// 紫光 ziguang / 拼音加加 jiajia 差异极小，映射到 mspy 表。
/// 智能ABC 结构特殊（ch→c/sh→s 与单声母冲突），暂不提供。
///
/// 方案数据：声母 zh/ch/sh 的键 + 韵母→键 + 键→韵母（一键可多韵母，decode 返回全部可能）。

/// 单字母声母集合
const SINGLE_SHENGMU: &str = "bpmfdtnlgkhjqxrzcsyw";

/// 双拼方案定义
pub struct Scheme {
    /// 方案 ID（配置用）：mspy/flypy/sogou/zrm
    pub id: &'static str,
    /// 声母 zh/ch/sh → 键
    pub shengmu: &'static [(&'static str, char)],
    /// 韵母 → 键（正向：全拼→双拼）
    pub yunmu_to_key: &'static [(&'static str, char)],
    /// 键 → 韵母列表（反向，一键可多韵母）
    pub key_to_yunmu: &'static [(char, &'static [&'static str])],
}

/// 声母表（zh/ch/sh 映射，微软系通用；智能ABC 除外——未提供）
const SM_MS: &[(&str, char)] = &[("zh", 'v'), ("ch", 'i'), ("sh", 'u')];

// ── 微软双拼（mspy，默认）──
const MS_Y2K: &[(&str, char)] = &[
    ("a", 'a'),
    ("ai", 'l'),
    ("an", 'j'),
    ("ang", 'h'),
    ("ao", 'k'),
    ("e", 'e'),
    ("ei", 'z'),
    ("en", 'f'),
    ("eng", 'g'),
    ("er", 'r'),
    ("i", 'i'),
    ("ia", 'x'),
    ("ian", 'm'),
    ("iang", 'd'),
    ("iao", 'c'),
    ("ie", 'p'),
    ("in", 'b'),
    ("ing", 'k'),
    ("iong", 's'),
    ("iu", 'q'),
    ("o", 'o'),
    ("ong", 's'),
    ("ou", 'b'),
    ("u", 'u'),
    ("ua", 'x'),
    ("uai", 'k'),
    ("uan", 'r'),
    ("uang", 'd'),
    ("ue", 't'),
    ("ui", 'v'),
    ("un", 'y'),
    ("uo", 'o'),
    ("v", 'v'),
    ("ve", 't'),
];
const MS_K2Y: &[(char, &[&str])] = &[
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

// ── 小鹤双拼（flypy）──
const FY_Y2K: &[(&str, char)] = &[
    ("a", 'a'),
    ("ai", 'd'),
    ("an", 'j'),
    ("ang", 'h'),
    ("ao", 'c'),
    ("e", 'e'),
    ("ei", 'w'),
    ("en", 'f'),
    ("eng", 'g'),
    ("er", 'r'),
    ("i", 'i'),
    ("ia", 'x'),
    ("ian", 'm'),
    ("iang", 'l'),
    ("iao", 'n'),
    ("ie", 'p'),
    ("in", 'b'),
    ("ing", 'k'),
    ("iong", 's'),
    ("iu", 'q'),
    ("o", 'o'),
    ("ong", 's'),
    ("ou", 'z'),
    ("u", 'u'),
    ("ua", 'x'),
    ("uai", 'k'),
    ("uan", 'r'),
    ("uang", 'l'),
    ("ue", 't'),
    ("ui", 'v'),
    ("un", 'y'),
    ("uo", 'o'),
    ("v", 'v'),
    ("ve", 't'),
];
const FY_K2Y: &[(char, &[&str])] = &[
    ('a', &["a"]),
    ('b', &["in"]),
    ('c', &["ao"]),
    ('d', &["ai"]),
    ('e', &["e"]),
    ('f', &["en"]),
    ('g', &["eng"]),
    ('h', &["ang"]),
    ('i', &["i"]),
    ('j', &["an"]),
    ('k', &["ing"]),
    ('l', &["iang", "uang"]),
    ('m', &["ian"]),
    ('n', &["iao"]),
    ('o', &["o", "uo"]),
    ('p', &["ie"]),
    ('q', &["iu"]),
    ('r', &["er", "uan"]),
    ('s', &["ong", "iong"]),
    ('t', &["ue", "ve"]),
    ('u', &["u"]),
    ('v', &["ui", "v"]),
    ('w', &["ei"]),
    ('x', &["ia", "ua"]),
    ('y', &["un"]),
    ('z', &["ou"]),
];

// ── 搜狗双拼（sogou）──
const SG_Y2K: &[(&str, char)] = &[
    ("a", 'a'),
    ("ai", 'd'),
    ("an", 'j'),
    ("ang", 'h'),
    ("ao", 'c'),
    ("e", 'e'),
    ("ei", 'z'),
    ("en", 'f'),
    ("eng", 'g'),
    ("er", 'r'),
    ("i", 'i'),
    ("ia", 'x'),
    ("ian", 'm'),
    ("iang", 'l'),
    ("iao", 'n'),
    ("ie", 'p'),
    ("in", 'b'),
    ("ing", 'k'),
    ("iong", 's'),
    ("iu", 'q'),
    ("o", 'o'),
    ("ong", 's'),
    ("ou", 'b'),
    ("u", 'u'),
    ("ua", 'x'),
    ("uai", 'k'),
    ("uan", 'r'),
    ("uang", 'l'),
    ("ue", 't'),
    ("ui", 'v'),
    ("un", 'y'),
    ("uo", 'o'),
    ("v", 'v'),
    ("ve", 't'),
];
const SG_K2Y: &[(char, &[&str])] = &[
    ('a', &["a"]),
    ('b', &["in", "ou"]),
    ('c', &["ao"]),
    ('d', &["ai"]),
    ('e', &["e"]),
    ('f', &["en"]),
    ('g', &["eng"]),
    ('h', &["ang"]),
    ('i', &["i"]),
    ('j', &["an"]),
    ('k', &["ing"]),
    ('l', &["iang", "uang"]),
    ('m', &["ian"]),
    ('n', &["iao"]),
    ('o', &["o", "uo"]),
    ('p', &["ie"]),
    ('q', &["iu"]),
    ('r', &["er", "uan"]),
    ('s', &["ong", "iong"]),
    ('t', &["ue", "ve"]),
    ('u', &["u"]),
    ('v', &["ui", "v"]),
    ('x', &["ia", "ua"]),
    ('y', &["un"]),
    ('z', &["ei"]),
];

// ── 自然码（zrm）──
const ZR_Y2K: &[(&str, char)] = &[
    ("a", 'a'),
    ("ai", 'l'),
    ("an", 'j'),
    ("ang", 'h'),
    ("ao", 'k'),
    ("e", 'e'),
    ("ei", 'z'),
    ("en", 'f'),
    ("eng", 'g'),
    ("er", 'r'),
    ("i", 'i'),
    ("ia", 'x'),
    ("ian", 'm'),
    ("iang", 'd'),
    ("iao", 'c'),
    ("ie", 'p'),
    ("in", 'b'),
    ("ing", 'y'),
    ("iong", 's'),
    ("iu", 'q'),
    ("o", 'o'),
    ("ong", 's'),
    ("ou", 'z'),
    ("u", 'u'),
    ("ua", 'x'),
    ("uai", 'k'),
    ("uan", 'r'),
    ("uang", 'd'),
    ("ue", 't'),
    ("ui", 'v'),
    ("un", 'n'),
    ("uo", 'o'),
    ("v", 'v'),
    ("ve", 't'),
];
const ZR_K2Y: &[(char, &[&str])] = &[
    ('a', &["a"]),
    ('b', &["in"]),
    ('c', &["iao"]),
    ('d', &["iang", "uang"]),
    ('e', &["e"]),
    ('f', &["en"]),
    ('g', &["eng"]),
    ('h', &["ang"]),
    ('i', &["i"]),
    ('j', &["an"]),
    ('k', &["ao", "uai"]),
    ('l', &["ai"]),
    ('m', &["ian"]),
    ('n', &["un"]),
    ('o', &["o", "uo"]),
    ('p', &["ie"]),
    ('q', &["iu"]),
    ('r', &["er", "uan"]),
    ('s', &["ong", "iong"]),
    ('t', &["ue", "ve"]),
    ('u', &["u"]),
    ('v', &["ui", "v"]),
    ('x', &["ia", "ua"]),
    ('y', &["ing"]),
    ('z', &["ei", "ou"]),
];

/// 方案表（紫光/加加映射到微软表——差异极小）
pub static SCHEMES: &[Scheme] = &[
    Scheme {
        id: "mspy",
        shengmu: SM_MS,
        yunmu_to_key: MS_Y2K,
        key_to_yunmu: MS_K2Y,
    },
    Scheme {
        id: "flypy",
        shengmu: SM_MS,
        yunmu_to_key: FY_Y2K,
        key_to_yunmu: FY_K2Y,
    },
    Scheme {
        id: "sogou",
        shengmu: SM_MS,
        yunmu_to_key: SG_Y2K,
        key_to_yunmu: SG_K2Y,
    },
    Scheme {
        id: "zrm",
        shengmu: SM_MS,
        yunmu_to_key: ZR_Y2K,
        key_to_yunmu: ZR_K2Y,
    },
];

/// 按 ID 查找方案（ziguang/jiajia 别名映射到 mspy）。未知名返回 None。
pub fn find_scheme(id: &str) -> Option<&'static Scheme> {
    match id {
        "ziguang" | "jiajia" => SCHEMES.iter().find(|s| s.id == "mspy"),
        _ => SCHEMES.iter().find(|s| s.id == id),
    }
}

/// 是否为双拼模式下的合法声母键（v/i/u = zh/ch/sh，其余单声母）
fn is_shengmu_key(c: char) -> bool {
    c == 'v' || c == 'i' || c == 'u' || SINGLE_SHENGMU.contains(c)
}

impl Scheme {
    /// 将单个拼音音节编码为双拼（正向，测试/显示用）。零声母音节以 'o' 开头。
    pub fn encode(&self, syllable: &str) -> Option<String> {
        if syllable.is_empty() {
            return None;
        }
        let (sheng, yun) = self.split(syllable);
        let mut code = String::with_capacity(2);
        if sheng.is_empty() {
            code.push('o');
        } else {
            match self.shengmu.iter().find(|(s, _)| *s == sheng) {
                Some((_, c)) => code.push(*c),
                None => code.push(sheng.chars().next().unwrap()),
            }
        }
        match self.yunmu_to_key.iter().find(|(y, _)| *y == yun) {
            Some((_, c)) => code.push(*c),
            None => code.push(yun.chars().next().unwrap()),
        }
        Some(code)
    }

    /// 切分音节为 (声母, 韵母)。零声母返回 ("", 全音节)。
    pub fn split<'a>(&self, syllable: &'a str) -> (&'a str, &'a str) {
        for (sheng, _) in self.shengmu {
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

    /// 解码单个双拼码（1-2 键）为可能的全拼音节。返回所有可能全拼（含歧义）。
    pub fn decode(&self, code: &str) -> Vec<String> {
        let chars: Vec<char> = code.chars().collect();
        let mut result = Vec::new();

        if chars.len() == 1 {
            let c = chars[0];
            if let Some(yunmus) = self.key_to_yunmu.iter().find(|(k, _)| *k == c) {
                for y in yunmus.1 {
                    result.push(y.to_string());
                }
            }
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
            if sheng_key == 'o' {
                if let Some(yunmus) = self.key_to_yunmu.iter().find(|(k, _)| *k == yun_key) {
                    for y in yunmus.1 {
                        result.push(y.to_string());
                    }
                }
                return result;
            }
            if !is_shengmu_key(sheng_key) {
                return result;
            }
            let sheng_owned: String = match sheng_key {
                'v' => "zh".to_string(),
                'i' => "ch".to_string(),
                'u' => "sh".to_string(),
                _ => sheng_key.to_string(),
            };
            if let Some(yunmus) = self.key_to_yunmu.iter().find(|(k, _)| *k == yun_key) {
                for y in yunmus.1 {
                    result.push(format!("{}{}", sheng_owned, y));
                }
            }
            return result;
        }

        result
    }

    /// 将双拼码串切分为音节解码（成对切分，最后不足 2 键作为前缀）。
    pub fn decode_string(&self, input: &str) -> Vec<String> {
        let mut results = vec![String::new()];
        let chars: Vec<char> = input.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let end = (i + 2).min(chars.len());
            let code: String = chars[i..end].iter().collect();
            let options = self.decode(&code);
            if options.is_empty() {
                return Vec::new();
            }
            let mut next = Vec::new();
            for prefix in &results {
                for opt in &options {
                    next.push(format!("{}{}", prefix, opt));
                }
            }
            results = next;
            i = end;
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms() -> &'static Scheme {
        find_scheme("mspy").unwrap()
    }
    fn fy() -> &'static Scheme {
        find_scheme("flypy").unwrap()
    }
    fn sg() -> &'static Scheme {
        find_scheme("sogou").unwrap()
    }
    fn zr() -> &'static Scheme {
        find_scheme("zrm").unwrap()
    }

    #[test]
    fn test_split() {
        assert_eq!(ms().split("zhong"), ("zh", "ong"));
        assert_eq!(ms().split("ni"), ("n", "i"));
        assert_eq!(ms().split("ai"), ("", "ai"));
    }

    #[test]
    fn test_ms_encode_zhong() {
        // zhong: zh→v, ong→s → "vs"
        assert_eq!(ms().encode("zhong").as_deref(), Some("vs"));
    }

    #[test]
    fn test_flypy_encode_zhong() {
        // 小鹤：zh→v, ong→s → "vs"
        assert_eq!(fy().encode("zhong").as_deref(), Some("vs"));
    }

    #[test]
    fn test_flypy_encode_ai() {
        // 小鹤：ai→d（微软 l）→ 零声母 od
        assert_eq!(fy().encode("ai").as_deref(), Some("od"));
        assert_eq!(ms().encode("ai").as_deref(), Some("ol"), "微软 ai→ol");
    }

    #[test]
    fn test_flypy_encode_jia() {
        // 小鹤：j + ia→x → "jx"
        assert_eq!(fy().encode("jia").as_deref(), Some("jx"));
    }

    #[test]
    fn test_zrm_ing_y() {
        // 自然码：ing→y（微软 k）
        assert_eq!(zr().encode("xing").as_deref(), Some("xy"));
        assert_eq!(ms().encode("xing").as_deref(), Some("xk"));
    }

    #[test]
    fn test_sogou_ai_d() {
        // 搜狗：ai→d（微软 l）
        assert_eq!(sg().encode("ai").as_deref(), Some("od"));
    }

    #[test]
    fn test_decode_zhong() {
        let decoded = ms().decode("vs");
        assert!(decoded.iter().any(|d| d == "zhong"), "got {:?}", decoded);
    }

    #[test]
    fn test_decode_string() {
        let decoded = ms().decode_string("vsni");
        assert!(decoded.iter().any(|d| d == "zhongni"), "got {:?}", decoded);
    }

    #[test]
    fn test_find_scheme_alias() {
        assert_eq!(find_scheme("ziguang").map(|s| s.id), Some("mspy"));
        assert_eq!(find_scheme("jiajia").map(|s| s.id), Some("mspy"));
        assert!(find_scheme("abc").is_none());
        assert!(find_scheme("unknown").is_none());
    }

    #[test]
    fn test_all_schemes_available() {
        for id in ["mspy", "flypy", "sogou", "zrm"] {
            assert!(find_scheme(id).is_some(), "{id} 应可用");
        }
    }
}
