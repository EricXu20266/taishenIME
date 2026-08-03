/// 符号输入 v 模式（V0.2.17）
///
/// 对标 rime-ice symbols.schema.yaml：v + 分类码 → 符号候选。
/// 本期覆盖 4 类高频符号：箭头/数学/单位/标点。
///
/// 分类码采用拼音首字母缩写：
///   jt → 箭头（jian tou）
///   sx → 数学（shu xue）
///   dw → 单位（dan wei）
///   bd → 标点（biao dian）

/// 分类码 → 符号列表
const SYMBOL_TABLE: &[(&str, &[&str])] = &[
    ("jt", &["→", "←", "↑", "↓", "↔", "⇒", "⇐", "⇔", "➜", "↵"]),
    (
        "sx",
        &[
            "≈", "≠", "≤", "≥", "±", "×", "÷", "∞", "∑", "∏", "√", "°", "′", "″",
        ],
    ),
    (
        "dw",
        &["℃", "℉", "㎡", "㎞", "㎏", "㎝", "㎜", "μ", "Ω", "§", "№"],
    ),
    (
        "bd",
        &[
            "·", "―", "…", "—", "『", "』", "「", "」", "《", "》", "〈", "〉", "〖", "〗", "【",
            "】",
        ],
    ),
];

/// 查询符号列表。category 为分类码（如 "jt"），未知分类返回空。
pub fn query(category: &str) -> Vec<&'static str> {
    SYMBOL_TABLE
        .iter()
        .find(|(code, _)| *code == category)
        .map(|(_, symbols)| symbols.to_vec())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_arrow() {
        let s = query("jt");
        assert_eq!(s, vec!["→", "←", "↑", "↓", "↔", "⇒", "⇐", "⇔", "➜", "↵"]);
    }

    #[test]
    fn test_query_math() {
        let s = query("sx");
        assert!(s.contains(&"≈"));
        assert!(s.contains(&"≠"));
        assert!(s.contains(&"≤"));
    }

    #[test]
    fn test_query_unit() {
        let s = query("dw");
        assert!(s.contains(&"℃"));
        assert!(s.contains(&"㎡"));
    }

    #[test]
    fn test_query_punct() {
        let s = query("bd");
        assert!(s.contains(&"·"));
        assert!(s.contains(&"…"));
    }

    #[test]
    fn test_query_unknown_category_empty() {
        assert!(query("zz").is_empty());
        assert!(query("").is_empty());
        assert!(query("j").is_empty()); // 分类码必须完整匹配
    }
}
