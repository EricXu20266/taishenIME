/// 日期/时间/星期/农历输入（V0.2.19）
///
/// 简码触发：rq 日期 / sj 时间 / xq 星期 / nl 农历。
/// 农历采用 1900-2100 年数据表查表算法（公历 → 农历月日）。

use std::time::{SystemTime, UNIX_EPOCH};

/// 当前本地时间（年/月/日/时/分/秒/星期）
fn now() -> (i32, u32, u32, u32, u32, u32, u32) {
    // 使用本地时间近似：Windows 下用系统时间 + 时区偏移由平台提供。
    // 这里通过 chrono 逻辑手工换算：先拿 UTC，再按本地偏移。
    // 简化：Rust std 无时区 API，用 1900-01-01 秒数 + 本地偏移估算。
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // 东八区（UTC+8）固定偏移——泰深输入法面向中国用户
    let local = secs + 8 * 3600;
    let days = local.div_euclid(86400);
    let rem = local.rem_euclid(86400);
    let hour = (rem / 3600) as u32;
    let minute = (rem / 60 % 60) as u32;
    let second = (rem % 60) as u32;
    // 1970-01-01 是星期四（4），day_of_week: 0=周日
    let weekday = ((days + 4).rem_euclid(7)) as u32;
    // 公历日期：从 1970-01-01 递增
    let (year, month, day) = days_to_date(days);
    (year, month, day, hour, minute, second, weekday)
}

/// 天数 → 公历年月日（1970-01-01 起）
fn days_to_date(mut days: i64) -> (i32, u32, u32) {
    let mut year = 1970i32;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let mut month = 1u32;
    loop {
        let dim = month_days(year, month);
        if days < dim as i64 {
            break;
        }
        days -= dim as i64;
        month += 1;
    }
    (year, month, (days + 1) as u32)
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn month_days(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// 日期候选（3 格式）：ISO / 中文 / 短
pub fn date_candidates() -> Vec<String> {
    let (y, m, d, _, _, _, _) = now();
    vec![
        format!("{y:04}-{m:02}-{d:02}"),
        format!("{y}年{m}月{d}日"),
        format!("{m}月{d}日"),
    ]
}

/// 时间候选（2 格式）：HH:MM / HH:MM:SS
pub fn time_candidates() -> Vec<String> {
    let (_, _, _, h, m, s, _) = now();
    vec![
        format!("{h:02}:{m:02}"),
        format!("{h:02}:{m:02}:{s:02}"),
    ]
}

/// 星期候选（3 格式）：星期一 / 周一 / Monday
pub fn weekday_candidates() -> Vec<String> {
    let (_, _, _, _, _, _, w) = now();
    let cn_full = ["星期日", "星期一", "星期二", "星期三", "星期四", "星期五", "星期六"];
    let cn_short = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
    let en = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
    vec![
        cn_full[w as usize].to_string(),
        cn_short[w as usize].to_string(),
        en[w as usize].to_string(),
    ]
}

/// 农历候选：月日（含闰月标记）。闰月格式"闰六月"
pub fn lunar_candidates() -> Vec<String> {
    let (y, m, d, _, _, _, _) = now();
    match solar_to_lunar(y, m, d) {
        Some((lm, ld, leap)) => {
            let mut text = String::new();
            if leap {
                text.push_str("闰");
            }
            text.push_str(cn_lunar_month(lm));
            text.push_str(&cn_lunar_day(ld));
            vec![text]
        }
        None => Vec::new(),
    }
}
fn cn_lunar_month(m: u8) -> &'static str {
    match m {
        1 => "正月",
        2 => "二月",
        3 => "三月",
        4 => "四月",
        5 => "五月",
        6 => "六月",
        7 => "七月",
        8 => "八月",
        9 => "九月",
        10 => "十月",
        11 => "冬月",
        12 => "腊月",
        _ => "",
    }
}

/// 农历日名：初一~三十
fn cn_lunar_day(d: u8) -> String {
    if d == 10 {
        return "初十".to_string();
    }
    if d == 20 {
        return "二十".to_string();
    }
    if d == 30 {
        return "三十".to_string();
    }
    let tens = d / 10;
    let ones = d % 10;
    if tens == 0 {
        return match ones {
            1 => "初一",
            2 => "初二",
            3 => "初三",
            4 => "初四",
            5 => "初五",
            6 => "初六",
            7 => "初七",
            8 => "初八",
            9 => "初九",
            _ => "",
        }
        .to_string();
    }
    let t = match tens {
        1 => "十",
        2 => "廿",
        _ => "",
    };
    let o = match ones {
        0 => "",
        1 => "一",
        2 => "二",
        3 => "三",
        4 => "四",
        5 => "五",
        6 => "六",
        7 => "七",
        8 => "八",
        9 => "九",
        _ => "",
    };
    format!("{t}{o}")
}

/// 农历数据表：1900-2100，每年一个 u32。
/// 编码：低 12 位 = 12 个月份大小（1=大月30，0=小月29，bit0=正月...），
/// 高 4 位 = 闰月月份（0=无闰月），闰月大小取当年第一个月的位。
/// 来源：公开农历算法标准数据。
const LUNAR_INFO: [u32; 201] = [
    0x04bd8, 0x04ae0, 0x0a570, 0x054d5, 0x0d260, 0x0d950, 0x16554, 0x056a0, 0x09ad0, 0x055d2, // 1900-1909
    0x04ae0, 0x0a5b6, 0x0a4d0, 0x0d250, 0x1d255, 0x0b540, 0x0d6a0, 0x0ada2, 0x095b0, 0x14977, // 1910-1919
    0x04970, 0x0a4b0, 0x0b4b5, 0x06a50, 0x06d40, 0x1ab54, 0x02b60, 0x09570, 0x052f2, 0x04970, // 1920-1929
    0x06566, 0x0d4a0, 0x0ea50, 0x06e95, 0x05ad0, 0x02b60, 0x186e3, 0x092e0, 0x1c8d7, 0x0c950, // 1930-1939
    0x0d4a0, 0x1d8a6, 0x0b550, 0x056a0, 0x1a5b4, 0x025d0, 0x092d0, 0x0d2b2, 0x0a950, 0x0b557, // 1940-1949
    0x06ca0, 0x0b550, 0x15355, 0x04da0, 0x0a5b0, 0x14573, 0x052b0, 0x0a9a8, 0x0e950, 0x06aa0, // 1950-1959
    0x0aea6, 0x0ab50, 0x04b60, 0x0aae4, 0x0a570, 0x05260, 0x0f263, 0x0d950, 0x05b57, 0x056a0, // 1960-1969
    0x096d0, 0x04dd5, 0x04ad0, 0x0a4d0, 0x0d4d4, 0x0d250, 0x0d558, 0x0b540, 0x0b6a0, 0x195a6, // 1970-1979
    0x095b0, 0x049b0, 0x0a974, 0x0a4b0, 0x0b27a, 0x06a50, 0x06d40, 0x0af46, 0x0ab60, 0x09570, // 1980-1989
    0x04af5, 0x04970, 0x064b0, 0x074a3, 0x0ea50, 0x06b58, 0x055c0, 0x0ab60, 0x096d5, 0x092e0, // 1990-1999
    0x0c960, 0x0d954, 0x0d4a0, 0x0da50, 0x07552, 0x056a0, 0x0abb7, 0x025d0, 0x092d0, 0x0cab5, // 2000-2009
    0x0a950, 0x0b4a0, 0x0baa4, 0x0ad50, 0x055d9, 0x04ba0, 0x0a5b0, 0x15176, 0x052b0, 0x0a930, // 2010-2019
    0x07954, 0x06aa0, 0x0ad50, 0x05b52, 0x04b60, 0x0a6e6, 0x0a4e0, 0x0d260, 0x0ea65, 0x0d530, // 2020-2029
    0x05aa0, 0x076a3, 0x096d0, 0x04afb, 0x04ad0, 0x0a4d0, 0x1d0b6, 0x0d250, 0x0d520, 0x0dd45, // 2030-2039
    0x0b5a0, 0x056d0, 0x055b2, 0x049b0, 0x0a577, 0x0a4b0, 0x0aa50, 0x1b255, 0x06d20, 0x0ada0, // 2040-2049
    0x14b63, 0x09370, 0x049f8, 0x04970, 0x064b0, 0x168a6, 0x0ea50, 0x06b20, 0x1a6c4, 0x0aae0, // 2050-2059
    0x092e0, 0x0d2e3, 0x0c960, 0x0d557, 0x0d4a0, 0x0da50, 0x05d55, 0x056a0, 0x0a6d0, 0x055d4, // 2060-2069
    0x052d0, 0x0a9b8, 0x0a950, 0x0b4a0, 0x0b6a6, 0x0ad50, 0x055a0, 0x0aba4, 0x0a5b0, 0x052b0, // 2070-2079
    0x0b273, 0x06930, 0x07337, 0x06aa0, 0x0ad50, 0x14b55, 0x04b60, 0x0a570, 0x054e4, 0x0d160, // 2080-2089
    0x0e968, 0x0d520, 0x0daa0, 0x16aa6, 0x056d0, 0x04ae0, 0x0a9d4, 0x0a2d0, 0x0d150, 0x0f252, // 2090-2099
    0x0d520, // 2100
];

/// 公历 → 农历（月, 日, 是否闰月）。范围 1900-2100。
pub fn solar_to_lunar(year: i32, month: u32, day: u32) -> Option<(u8, u8, bool)> {
    if year < 1900 || year > 2100 {
        return None;
    }
    let mut offset = 0i64;
    // 1900-01-31 是农历 1900 年正月初一
    // 先算公历距 1900-01-31 的天数
    let mut y = 1900i32;
    while y < year {
        offset += if is_leap(y) { 366 } else { 365 };
        y += 1;
    }
    for m in 1..month {
        offset += month_days(year, m) as i64;
    }
    offset += (day - 1) as i64;
    // 减去 1900-01-31 的偏移：1900-01-01 到 01-31 是 30 天
    let days_from_lunar_epoch = offset - 30;
    // 从 1900 正月初一逐月推进
    let mut lunar_year = 1900i32;
    let mut remaining = days_from_lunar_epoch;
    let mut idx = 0usize;
    loop {
        let info = LUNAR_INFO[idx];
        let mut year_days = lunar_year_days(info);
        if remaining < year_days {
            break;
        }
        remaining -= year_days;
        lunar_year += 1;
        idx += 1;
        if idx >= LUNAR_INFO.len() {
            return None;
        }
    }
    // 在 lunar_year 内逐月定位
    let info = LUNAR_INFO[idx];
    let leap_month = (info & 0xF) as u8; // bit0-3：闰月月份
    let leap_big = ((info >> 16) & 1) == 1; // bit16：闰月大小
    // 12 个普通月（bit15=正月 ... bit4=腊月）
    let mut months_days: Vec<(u8, u32, bool)> = Vec::new();
    for i in 0..12u8 {
        let big = ((info >> (15 - i)) & 1) == 1;
        months_days.push((i + 1, if big { 30 } else { 29 }, false));
    }
    // 插入闰月（在闰月月份之后）
    if leap_month > 0 {
        let mut inserted = Vec::new();
        for (mm, days, is_leap) in months_days {
            inserted.push((mm, days, is_leap));
            if mm == leap_month {
                inserted.push((leap_month, if leap_big { 30 } else { 29 }, true));
            }
        }
        months_days = inserted;
    }
    for (mm, days, lp) in months_days {
        if remaining < days as i64 {
            return Some((mm, (remaining + 1) as u8, lp));
        }
        remaining -= days as i64;
    }
    // 超出（理论上不会到）
    None
}

/// 农历某年总天数（普通月 12 + 闰月）
fn lunar_year_days(info: u32) -> i64 {
    let mut days = 0i64;
    for i in 0..12u8 {
        let big = ((info >> (15 - i)) & 1) == 1;
        days += if big { 30 } else { 29 };
    }
    let leap_month = (info & 0xF) as u8;
    if leap_month > 0 {
        let leap_big = ((info >> 16) & 1) == 1;
        days += if leap_big { 30 } else { 29 };
    }
    days
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_date_candidates_format() {
        let cands = date_candidates();
        assert_eq!(cands.len(), 3);
        // ISO 格式校验：YYYY-MM-DD
        assert!(cands[0].len() == 10);
        assert!(cands[0].chars().nth(4) == Some('-'));
        assert!(cands[1].contains('年') && cands[1].contains('日'));
    }

    #[test]
    fn test_time_candidates_format() {
        let cands = time_candidates();
        assert_eq!(cands.len(), 2);
        assert!(cands[0].contains(':'));
    }

    #[test]
    fn test_weekday_candidates_format() {
        let cands = weekday_candidates();
        assert_eq!(cands.len(), 3);
        assert!(cands[0].contains("星期") || cands[0].contains("周日"));
        assert!(cands[2].len() >= 6, "英文星期应为完整单词");
    }

    #[test]
    fn test_lunar_known_date() {
        // 2026-08-03（今天）应能算出农历（返回 Some）
        let (y, m, d, _, _, _, _) = now();
        let lunar = solar_to_lunar(y, m, d);
        assert!(lunar.is_some(), "当前日期应能转农历");
        let (lm, ld, _) = lunar.unwrap();
        assert!((1..=12).contains(&lm));
        assert!((1..=30).contains(&ld));
    }

    #[test]
    fn test_lunar_spring_festival() {
        // 2026-02-17 是 2026 春节（正月初一）
        let lunar = solar_to_lunar(2026, 2, 17);
        assert_eq!(lunar, Some((1, 1, false)));
    }

    #[test]
    fn test_lunar_out_of_range() {
        assert!(solar_to_lunar(2200, 1, 1).is_none());
        assert!(solar_to_lunar(1800, 1, 1).is_none());
    }

    #[test]
    fn test_cn_lunar_day_names() {
        assert_eq!(cn_lunar_day(1), "初一");
        assert_eq!(cn_lunar_day(10), "初十");
        assert_eq!(cn_lunar_day(15), "十五");
        assert_eq!(cn_lunar_day(20), "二十");
        assert_eq!(cn_lunar_day(23), "廿三");
        assert_eq!(cn_lunar_day(30), "三十");
    }

    #[test]
    fn test_days_to_date_epoch() {
        assert_eq!(days_to_date(0), (1970, 1, 1));
        assert_eq!(days_to_date(31), (1970, 2, 1));
        assert_eq!(days_to_date(365), (1971, 1, 1)); // 1970 非闰年
    }
}
