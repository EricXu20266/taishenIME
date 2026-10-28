/// 英文词模块 — 中英混输的英文候选。
///
/// 词表：google-10000-english（1 万高频英文词，按频次降序）；
/// 显示形式沿用原静态表（AI / GitHub 等保持原文写法）。
/// 编译期 `include_str!` 进二进制，首次查询惰性解析并缓存。
use std::sync::OnceLock;

/// 每行格式：`code<TAB>display<TAB>rank`（rank 越小越常用）
static WORDS_RAW: &str = include_str!("../../resources/english_words.txt");

static WORDS: OnceLock<Vec<(String, String, u32)>> = OnceLock::new();

fn words() -> &'static Vec<(String, String, u32)> {
    WORDS.get_or_init(|| {
        WORDS_RAW
            .lines()
            .filter_map(|line| {
                let mut it = line.split('\t');
                let code = it.next()?.trim();
                if code.is_empty() {
                    return None;
                }
                let display = it.next().unwrap_or(code).trim();
                let rank: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(99_999);
                Some((code.to_string(), display.to_string(), rank))
            })
            .collect()
    })
}

/// 英文前缀查询：精确匹配置首，其余按频次降序，最多 8 个。
/// 空前缀 / 无匹配返回空。
pub fn query(prefix: &str) -> Vec<String> {
    let p = prefix.to_lowercase();
    if p.is_empty() {
        return Vec::new();
    }
    // 精确匹配优先：输入 app 要的是 app，不是 application
    let mut exact: Option<&str> = None;
    let mut hits: Vec<(&str, u32)> = Vec::new();
    for (code, display, rank) in words() {
        if code == &p {
            exact = Some(display.as_str());
        } else if code.starts_with(&p) {
            hits.push((display.as_str(), *rank));
        }
    }
    hits.sort_by_key(|(_, rank)| *rank);
    let mut out: Vec<String> = Vec::with_capacity(8);
    if let Some(e) = exact {
        out.push(e.to_string());
    }
    for (d, _) in hits {
        if out.len() >= 8 {
            break;
        }
        out.push(d.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_hello() {
        let r = query("hel");
        assert!(
            r.iter().any(|w| w == "hello"),
            "hel 应命中 hello, got {r:?}"
        );
    }

    #[test]
    fn test_query_world() {
        let r = query("wor");
        assert!(
            r.iter().any(|w| w == "world"),
            "wor 应命中 world, got {r:?}"
        );
    }

    #[test]
    fn test_query_mixed_word() {
        // 混合词：ai → AI
        let r = query("ai");
        assert!(r.iter().any(|w| w == "AI"), "ai 应命中 AI, got {r:?}");
    }

    #[test]
    fn test_query_app() {
        let r = query("app");
        assert!(
            r.iter().any(|w| w.eq_ignore_ascii_case("app")),
            "app 应命中 app, got {r:?}"
        );
    }

    #[test]
    fn test_query_limit() {
        // 短前缀命中多，截断 8
        let r = query("a");
        assert!(r.len() <= 8, "应截断 8 个, got {}", r.len());
    }

    #[test]
    fn test_query_empty() {
        assert!(query("").is_empty());
        assert!(query("zzzz").is_empty());
    }

    #[test]
    fn test_query_case_insensitive() {
        let r1 = query("Hello");
        let r2 = query("hello");
        assert!(r1.iter().any(|w| w == "hello"));
        assert_eq!(r1, r2, "大小写不应影响结果");
    }

    #[test]
    fn test_freq_desc() {
        // the > and（频率序）
        let r = query("t");
        let the_pos = r.iter().position(|w| w == "the").unwrap_or(99);
        let r2 = query("a");
        let and_pos = r2.iter().position(|w| w == "and").unwrap_or(99);
        assert!(the_pos < 5, "the 应靠前, got {r:?}");
        assert!(and_pos < 5, "and 应靠前, got {r2:?}");
    }
}
