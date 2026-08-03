/// 数字金额大写模块（P1-4，对标 rime number_translator）
///
/// R+ 前缀模式：R1234.56 → 人民币大写。识别在 lib.rs（raw_input 以大写 R 开头）。
/// 候选输出：
///   - 整数 → 壹仟贰佰叁拾肆（中文大写数字）
///   - 含小数 → 壹仟贰佰叁拾肆元伍角陆分

/// 中文大写数字与单位
const CN_N: [&str; 10] = ["零", "壹", "贰", "叁", "肆", "伍", "陆", "柒", "捌", "玖"];

/// 整数部分转中文大写（支持到亿）。如 1234 → 壹仟贰佰叁拾肆
fn int_to_cn(num: u64) -> String {
    if num == 0 {
        return "零".to_string();
    }
    // 亿级 / 万级 / 个级 分段
    let yi = num / 100_000_000;
    let wan = (num % 100_000_000) / 10_000;
    let ge = num % 10_000;

    let mut parts = Vec::new();
    if yi > 0 {
        parts.push(format!("{}亿", section_to_cn(yi)));
    }
    if wan > 0 {
        let s = section_to_cn(wan);
        // 万级与亿级衔接处的零处理
        if yi > 0 && wan < 1000 {
            parts.push(format!("零{}万", s));
        } else {
            parts.push(format!("{s}万"));
        }
    }
    if ge > 0 {
        // 万级后 ge < 1000 时补零
        if (yi > 0 || wan > 0) && ge < 1000 {
            parts.push(format!("零{}", section_to_cn(ge)));
        } else {
            parts.push(section_to_cn(ge));
        }
    }
    parts.concat()
}

/// 4 位段转中文大写（千/百/十/个）
fn section_to_cn(mut num: u64) -> String {
    if num == 0 {
        return "零".to_string();
    }
    let q = num / 1000;
    num %= 1000;
    let b = num / 100;
    num %= 100;
    let s = num / 10;
    let g = num % 10;

    let mut out = String::new();
    if q > 0 {
        out.push_str(CN_N[q as usize]);
        out.push('仟');
    }
    if b > 0 {
        if q > 0 && b > 0 {
            // 千位有值百位有值直接连
        } else if q > 0 && b == 0 {
            out.push('零');
        }
        out.push_str(CN_N[b as usize]);
        out.push('佰');
    } else if q > 0 && (s > 0 || g > 0) {
        out.push('零');
    }
    if s > 0 {
        if b > 0 {
            // 正常
        } else if q > 0 && b == 0 {
            // 已补零
        }
        out.push_str(CN_N[s as usize]);
        out.push('拾');
    } else if b > 0 && g > 0 {
        out.push('零');
    }
    if g > 0 {
        out.push_str(CN_N[g as usize]);
    }
    out
}

/// 解析输入串（数字 + 可选小数点），输出中文大写候选。
/// 非法输入返回空。
pub fn to_cn(input: &str) -> Vec<String> {
    let input = input.trim();
    if input.is_empty() || !input.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Vec::new();
    }
    // 拆分整数/小数
    let mut parts = input.splitn(2, '.');
    let int_part = parts.next().unwrap_or("0");
    let dec_part = parts.next().unwrap_or("");
    // 整数部分解析（去前导零）
    let int_val: u64 = match int_part.parse() {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    if int_val > 999_999_999_999_999_999u64 {
        return Vec::new(); // 超范围（< 1e18）
    }

    let mut out = Vec::new();
    // 1. 纯整数大写
    out.push(int_to_cn(int_val));

    // 2. 金额大写（含元角分）
    if !dec_part.is_empty() {
        let d: String = dec_part.chars().take(2).collect();
        let jiao = d.chars().nth(0).and_then(|c| c.to_digit(10)).unwrap_or(0);
        let fen = d.chars().nth(1).and_then(|c| c.to_digit(10)).unwrap_or(0);
        let mut amount = String::new();
        if int_val > 0 {
            amount.push_str(&int_to_cn(int_val));
            amount.push('元');
        }
        if jiao > 0 {
            amount.push_str(CN_N[jiao as usize]);
            amount.push('角');
        }
        if fen > 0 {
            amount.push_str(CN_N[fen as usize]);
            amount.push('分');
        }
        if amount.is_empty() {
            amount.push_str("零元");
        }
        out.push(amount);
    } else {
        out.push(format!("{}元整", int_to_cn(int_val)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_int_small() {
        assert_eq!(int_to_cn(1), "壹");
        assert_eq!(int_to_cn(12), "壹拾贰");
        assert_eq!(int_to_cn(123), "壹佰贰拾叁");
        assert_eq!(int_to_cn(1234), "壹仟贰佰叁拾肆");
    }

    #[test]
    fn test_int_zero_fill() {
        assert_eq!(int_to_cn(1001), "壹仟零壹");
        assert_eq!(int_to_cn(100), "壹佰");
        assert_eq!(int_to_cn(101), "壹佰零壹");
        assert_eq!(int_to_cn(10000), "壹万");
    }

    #[test]
    fn test_int_wan() {
        assert_eq!(int_to_cn(123456), "壹拾贰万叁仟肆佰伍拾陆");
    }

    #[test]
    fn test_int_yi() {
        assert_eq!(int_to_cn(100000000), "壹亿");
        assert_eq!(int_to_cn(100010000), "壹亿零壹万");
    }

    #[test]
    fn test_to_cn_integer() {
        let r = to_cn("1234");
        assert!(r.iter().any(|s| s == "壹仟贰佰叁拾肆"));
        assert!(r.iter().any(|s| s == "壹仟贰佰叁拾肆元整"));
    }

    #[test]
    fn test_to_cn_decimal() {
        let r = to_cn("1234.5");
        assert!(r.iter().any(|s| s == "壹仟贰佰叁拾肆元伍角"),
                "got {r:?}");
    }

    #[test]
    fn test_to_cn_decimal_fen() {
        let r = to_cn("1234.56");
        assert!(r.iter().any(|s| s == "壹仟贰佰叁拾肆元伍角陆分"),
                "got {r:?}");
    }

    #[test]
    fn test_to_cn_invalid() {
        assert!(to_cn("").is_empty());
        assert!(to_cn("abc").is_empty());
        assert!(to_cn("12a").is_empty());
    }

    #[test]
    fn test_to_cn_zero() {
        let r = to_cn("0");
        assert_eq!(r[0], "零");
    }
}
