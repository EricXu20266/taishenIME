/// 计算器模块（V0.2.22）— c + 算式 → 结果
///
/// 支持：四则运算 + - * /、括号 ()、幂 ^、取模 %、小数。
/// 递归下降解析：表达式 → 项 → 因子 → 幂 → 一元 → 原子。
/// 错误：语法错误 / 除零 / 未知字符 → Err。

/// 表达式求值（如 "35*12" → 420.0）
pub fn eval(expr: &str) -> Result<f64, String> {
    let mut parser = Parser {
        chars: expr.chars().collect(),
        pos: 0,
    };
    let result = parser.parse_expr()?;
    // 表达式必须完整消费（防 "1+2x" 之类）
    parser.skip_ws();
    if parser.pos < parser.chars.len() {
        return Err(format!("多余字符: {}", parser.chars[parser.pos]));
    }
    Ok(result)
}

/// 结果格式化：去尾零，最多 6 位小数。
/// 整数 → 无小数点；浮点 → 保留最多 6 位去尾零。
pub fn format_result(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let s = format!("{:.6}", v);
    // 去尾零（保留小数点后至少 1 位）
    let s = s.trim_end_matches('0');
    s.trim_end_matches('.').to_string()
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn skip_ws(&mut self) {
        while self.pos < self.chars.len() && self.chars[self.pos].is_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.skip_ws();
        self.chars.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<char> {
        self.skip_ws();
        let ch = self.chars.get(self.pos).copied();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
    }

    /// 表达式 := 项 (('+'|'-') 项)*
    fn parse_expr(&mut self) -> Result<f64, String> {
        let mut left = self.parse_term()?;
        loop {
            match self.peek() {
                Some('+') => {
                    self.next();
                    let right = self.parse_term()?;
                    left += right;
                }
                Some('-') => {
                    self.next();
                    let right = self.parse_term()?;
                    left -= right;
                }
                _ => return Ok(left),
            }
        }
    }

    /// 项 := 因子 (('*'|'/'|'%') 因子)*
    fn parse_term(&mut self) -> Result<f64, String> {
        let mut left = self.parse_power()?;
        loop {
            match self.peek() {
                Some('*') => {
                    self.next();
                    let right = self.parse_power()?;
                    left *= right;
                }
                Some('/') => {
                    self.next();
                    let right = self.parse_power()?;
                    if right == 0.0 {
                        return Err("除零错误".to_string());
                    }
                    left /= right;
                }
                Some('%') => {
                    self.next();
                    let right = self.parse_power()?;
                    if right == 0.0 {
                        return Err("取模零错误".to_string());
                    }
                    left %= right;
                }
                _ => return Ok(left),
            }
        }
    }

    /// 因子 := 幂
    fn parse_power(&mut self) -> Result<f64, String> {
        // 幂是右结合的：2^3^2 = 2^(3^2)
        let base = self.parse_unary()?;
        if self.peek() == Some('^') {
            self.next();
            let exp = self.parse_power()?;
            return Ok(base.powf(exp));
        }
        Ok(base)
    }

    /// 一元 := ('+'|'-') 一元 | 原子
    fn parse_unary(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some('-') => {
                self.next();
                Ok(-self.parse_unary()?)
            }
            Some('+') => {
                self.next();
                self.parse_unary()
            }
            _ => self.parse_atom(),
        }
    }

    /// 原子 := 数字 | '(' 表达式 ')'
    fn parse_atom(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some('(') => {
                self.next();
                let v = self.parse_expr()?;
                if self.next() != Some(')') {
                    return Err("缺少右括号".to_string());
                }
                Ok(v)
            }
            Some(ch) if ch.is_ascii_digit() || ch == '.' => self.parse_number(),
            Some(ch) => Err(format!("非法字符: {ch}")),
            None => Err("表达式不完整".to_string()),
        }
    }

    fn parse_number(&mut self) -> Result<f64, String> {
        let mut num = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() || ch == '.' {
                num.push(ch);
                self.next();
            } else {
                break;
            }
        }
        num.parse::<f64>().map_err(|_| format!("非法数字: {num}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_arithmetic() {
        assert_eq!(eval("35*12").unwrap(), 420.0);
        assert_eq!(eval("1+2").unwrap(), 3.0);
        assert_eq!(eval("10-3").unwrap(), 7.0);
        assert_eq!(eval("100/4").unwrap(), 25.0);
    }

    #[test]
    fn test_parens() {
        assert_eq!(eval("(1+2)*3").unwrap(), 9.0);
        assert_eq!(eval("2*(3+4)").unwrap(), 14.0);
        assert_eq!(eval("(2+3)*(4+5)").unwrap(), 45.0);
    }

    #[test]
    fn test_power() {
        assert_eq!(eval("2^10").unwrap(), 1024.0);
        assert_eq!(eval("2^3^2").unwrap(), 512.0); // 右结合 2^(3^2)
        assert_eq!(eval("2^-2").unwrap(), 0.25);
    }

    #[test]
    fn test_mod() {
        assert_eq!(eval("10%3").unwrap(), 1.0);
        assert_eq!(eval("7%2.5").unwrap(), 2.0);
    }

    #[test]
    fn test_unary() {
        assert_eq!(eval("-5+3").unwrap(), -2.0);
        assert_eq!(eval("+5").unwrap(), 5.0);
        assert_eq!(eval("2*-3").unwrap(), -6.0);
    }

    #[test]
    fn test_decimal() {
        assert_eq!(eval("10/4").unwrap(), 2.5);
        assert_eq!(eval("0.5*2").unwrap(), 1.0);
        assert_eq!(eval("1.5+2.25").unwrap(), 3.75);
    }

    #[test]
    fn test_whitespace() {
        assert_eq!(eval(" 1 + 2 * 3 ").unwrap(), 7.0);
    }

    #[test]
    fn test_divide_by_zero_error() {
        assert!(eval("1/0").is_err());
        assert!(eval("1%0").is_err());
    }

    #[test]
    fn test_syntax_errors() {
        // 一元 +/- 合法（与 2*-3 对称），1++2 = 1+(+2) = 3
        assert_eq!(eval("1++2").unwrap(), 3.0);
        assert!(eval("1+").is_err());
        assert!(eval("(1+2").is_err());
        assert!(eval("1+2)").is_err());
        assert!(eval("abc").is_err());
        assert!(eval("1+2x").is_err());
        assert!(eval("").is_err());
    }

    #[test]
    fn test_format_result() {
        assert_eq!(format_result(420.0), "420");
        assert_eq!(format_result(2.5), "2.5");
        assert_eq!(format_result(1024.0), "1024");
        assert_eq!(format_result(1.0 / 3.0), "0.333333");
        assert_eq!(format_result(0.5), "0.5");
        assert_eq!(format_result(-2.0), "-2");
    }
}
