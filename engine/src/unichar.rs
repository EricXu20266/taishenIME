/// Unicode 字符输入模块（P1-4，对标 rime unicode）
///
/// U+ 前缀模式：U1F600 → 😀。识别在 lib.rs（raw_input 以大写 U 开头）。
/// 输入十六进制码点，输出对应 Unicode 字符。

/// 查询：hex 码点 → Unicode 字符。非法/超范围返回空。
pub fn query(hex: &str) -> Vec<String> {
    let hex = hex.trim();
    if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Vec::new();
    }
    // 十六进制 → 码点
    match u32::from_str_radix(hex, 16) {
        Ok(cp) => {
            // 合法 Unicode 标量值（排除代理区 0xD800-0xDFFF）
            match char::from_u32(cp) {
                Some(c) => vec![c.to_string()],
                None => Vec::new(),
            }
        }
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emoji() {
        // U1F600 → 😀
        let r = query("1F600");
        assert_eq!(r, vec!["😀"]);
    }

    #[test]
    fn test_ascii() {
        let r = query("41");
        assert_eq!(r, vec!["A"]);
    }

    #[test]
    fn test_cjk() {
        // 4E2D → 中
        let r = query("4E2D");
        assert_eq!(r, vec!["中"]);
    }

    #[test]
    fn test_invalid() {
        assert!(query("").is_empty());
        assert!(query("xyz").is_empty());
        assert!(query("D800").is_empty(), "代理区应无效");
        assert!(query("110000").is_empty(), "超范围应无效");
    }
}
