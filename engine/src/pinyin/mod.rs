/// 拼音处理模块 — 全拼输入
///
/// 第一期 MVP：仅支持全拼小写字母输入。
/// 第二期将加入双拼方案和拼音切分算法。

/// 拼音音节表（简化版，第一期 MVP 基础覆盖）
const VALID_SYLLABLES: &[&str] = &[
    "a", "ai", "an", "ang", "ao", "ba", "bai", "ban", "bang", "bao", "bei", "ben", "beng", "bi",
    "bian", "biao", "bie", "bin", "bing", "bo", "bu", "ca", "cai", "can", "cang", "cao", "ce",
    "cen", "ceng", "cha", "chai", "chan", "chang", "chao", "che", "chen", "cheng", "chi", "chong",
    "chou", "chu", "chua", "chuai", "chuan", "chuang", "chui", "chun", "chuo", "ci", "cong", "cou",
    "cu", "cuan", "cui", "cun", "cuo", "da", "dai", "dan", "dang", "dao", "de", "dei", "den",
    "deng", "di", "dian", "diao", "die", "ding", "diu", "dong", "dou", "du", "duan", "dui", "dun",
    "duo", "e", "ei", "en", "eng", "er", "fa", "fan", "fang", "fei", "fen", "feng", "fo", "fou",
    "fu", "ga", "gai", "gan", "gang", "gao", "ge", "gei", "gen", "geng", "gong", "gou", "gu",
    "gua", "guai", "guan", "guang", "gui", "gun", "guo", "ha", "hai", "han", "hang", "hao", "he",
    "hei", "hen", "heng", "hong", "hou", "hu", "hua", "huai", "huan", "huang", "hui", "hun", "huo",
    "ji", "jia", "jian", "jiang", "jiao", "jie", "jin", "jing", "jiong", "jiu", "ju", "juan",
    "jue", "jun", "ka", "kai", "kan", "kang", "kao", "ke", "kei", "ken", "keng", "kong", "kou",
    "ku", "kua", "kuai", "kuan", "kuang", "kui", "kun", "kuo", "la", "lai", "lan", "lang", "lao",
    "le", "lei", "leng", "li", "lia", "lian", "liang", "liao", "lie", "lin", "ling", "liu", "long",
    "lou", "lu", "luan", "lun", "luo", "lv", "lve", "ma", "mai", "man", "mang", "mao", "me", "mei",
    "men", "meng", "mi", "mian", "miao", "mie", "min", "ming", "miu", "mo", "mou", "mu", "na",
    "nai", "nan", "nang", "nao", "ne", "nei", "nen", "neng", "ni", "nian", "niang", "niao", "nie",
    "nin", "ning", "niu", "nong", "nou", "nu", "nuan", "nuo", "nv", "nve", "o", "ou", "pa", "pai",
    "pan", "pang", "pao", "pei", "pen", "peng", "pi", "pian", "piao", "pie", "pin", "ping", "po",
    "pou", "pu", "qi", "qia", "qian", "qiang", "qiao", "qie", "qin", "qing", "qiong", "qiu", "qu",
    "quan", "que", "qun", "ran", "rang", "rao", "re", "ren", "reng", "ri", "rong", "rou", "ru",
    "rua", "ruan", "rui", "run", "ruo", "sa", "sai", "san", "sang", "sao", "se", "sen", "seng",
    "sha", "shai", "shan", "shang", "shao", "she", "shei", "shen", "sheng", "shi", "shou", "shu",
    "shua", "shuai", "shuan", "shuang", "shui", "shun", "shuo", "si", "song", "sou", "su", "suan",
    "sui", "sun", "suo", "ta", "tai", "tan", "tang", "tao", "te", "tei", "teng", "ti", "tian",
    "tiao", "tie", "ting", "tong", "tou", "tu", "tuan", "tui", "tun", "tuo", "wa", "wai", "wan",
    "wang", "wei", "wen", "weng", "wo", "wu", "xi", "xia", "xian", "xiang", "xiao", "xie", "xin",
    "xing", "xiong", "xiu", "xu", "xuan", "xue", "xun", "ya", "yan", "yang", "yao", "ye", "yi",
    "yin", "ying", "yo", "yong", "you", "yu", "yuan", "yue", "yun", "za", "zai", "zan", "zang",
    "zao", "ze", "zei", "zen", "zeng", "zha", "zhai", "zhan", "zhang", "zhao", "zhe", "zhei",
    "zhen", "zheng", "zhi", "zhong", "zhou", "zhu", "zhua", "zhuai", "zhuan", "zhuang", "zhui",
    "zhun", "zhuo", "zi", "zong", "zou", "zu", "zuan", "zui", "zun", "zuo",
];

/// 全部合法拼音音节表（诊断/首字检查工具用）
pub fn all_syllables() -> &'static [&'static str] {
    VALID_SYLLABLES
}

/// 音节集合与音节前缀集合（V0.5.x 性能）：原先 is_valid_syllable /
/// is_valid_pinyin_prefix 每次线性扫 400 条音节表，而它们处在 combo_match、
/// 音节切分 DP 的内层循环——query_combo 单键要跑上万次。改查表后是 O(1)。
fn syllable_set() -> &'static std::collections::HashSet<&'static str> {
    static SET: std::sync::OnceLock<std::collections::HashSet<&'static str>> =
        std::sync::OnceLock::new();
    SET.get_or_init(|| VALID_SYLLABLES.iter().copied().collect())
}

fn syllable_prefix_set() -> &'static std::collections::HashSet<&'static str> {
    static SET: std::sync::OnceLock<std::collections::HashSet<&'static str>> =
        std::sync::OnceLock::new();
    SET.get_or_init(|| {
        let mut set = std::collections::HashSet::with_capacity(VALID_SYLLABLES.len() * 4);
        for syl in VALID_SYLLABLES {
            for i in 1..=syl.len() {
                set.insert(&syl[..i]);
            }
        }
        set
    })
}

/// 检查给定的字符串是否是一个有效的拼音前缀
/// 用于判断用户输入是否可能构成合法的拼音
pub fn is_valid_pinyin_prefix(input: &str) -> bool {
    if input.is_empty() {
        return true;
    }
    syllable_prefix_set().contains(input)
}

/// 检查给定字符串是否是一个完整有效的拼音音节
pub fn is_valid_syllable(input: &str) -> bool {
    syllable_set().contains(input)
}

/// 判断输入串是否存在一条完整的合法音节路径。
/// 使用字节边界动态规划；输入来自 ASCII 拼音，因此每个切分点都是字符边界。
fn can_segment_completely(input: &str) -> bool {
    if input.is_empty() {
        return true;
    }
    let bytes = input.as_bytes();
    let n = bytes.len();
    // 快路径：可达位用 u64 位图（拼音长度远小于 64），零堆分配。
    // 原实现对每次调用都 vec![false; n+1]，而 split_first_syllable 循环里
    // 每个候选音节都会调一次——query_combo 单键上万次分配。
    if n + 1 <= 64 {
        let mut reachable: u64 = 1 << n;
        for start in (0..n).rev() {
            let max_end = (start + 6).min(n);
            for end in (start + 1)..=max_end {
                if reachable & (1 << end) != 0 && is_valid_syllable(&input[start..end]) {
                    reachable |= 1 << start;
                    break;
                }
            }
        }
        return reachable & 1 != 0;
    }
    let mut reachable = vec![false; n + 1];
    reachable[n] = true;
    for start in (0..n).rev() {
        let max_end = (start + 6).min(n);
        for end in (start + 1)..=max_end {
            if reachable[end] && is_valid_syllable(&input[start..end]) {
                reachable[start] = true;
                break;
            }
        }
    }
    reachable[0]
}

/// 在输入串中尝试切分出首个完整音节。
/// 优先选择能让整个输入完成切分的最长路径；没有完整路径时保留最长匹配，
/// 以支持用户仍在输入中的半成品拼音。
pub fn split_first_syllable(input: &str) -> Option<(&str, &str)> {
    let mut fallback = None;
    for i in (1..=input.len().min(6)).rev() {
        let candidate = &input[..i];
        if is_valid_syllable(candidate) {
            let rest = &input[i..];
            fallback.get_or_insert((candidate, rest));
            if can_segment_completely(rest) {
                return Some((candidate, rest));
            }
        }
    }
    fallback
}

/// 检查输入串能否完全切分为合法拼音音节序列（0.3.x fix）。
/// 用于区分"拼音输入"与"英文单词/简拼"，防模糊音/纠错误伤英文：
///   - "zhongguo" → zhong+guo 完全切分 ✓ → 拼音，可做模糊/纠错/错音联想
///   - "hello"    → he+llo，llo 非音节 → 英文，不做拼音联想（只英文混输）
///   - "zg"       → z 非音节 → 简拼，走简拼查询但不做模糊/纠错
pub fn is_complete_pinyin(input: &str) -> bool {
    if input.is_empty() {
        return false;
    }
    let mut rest = input;
    while !rest.is_empty() {
        match split_first_syllable(rest) {
            Some((_syl, remaining)) => rest = remaining,
            None => return false,
        }
    }
    true
}

/// P2-3 ü 兼容（对标 rime derive v/u 转换）：
/// j q x y 后的 v 视为 ü → 归一为 u（qv→qu、jv→ju、xv→xu、yv→yu）。
/// 词库按标准 u 注音，输入 v 也能命中（如 qv → qu 去）。
/// n/l 后的 v 保持（nü/lü 词库若用 v 注音则不转换——双向兼容由词库决定）。
pub fn normalize_v(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev = ' ';
    for c in input.chars() {
        if c == 'v' && "jqxy".contains(prev) {
            out.push('u');
        } else {
            out.push(c);
        }
        prev = c;
    }
    out
}

/// 将完整拼音串转换为声母串（简拼索引用）
/// 规则：每音节取声母首字母，zh/ch/sh 归一为 z/c/s，零声母取首字母。
/// 示例："zhongguo" → "zg"，"women" → "wm"，"ai" → "a"
pub fn to_initial_string(pinyin_str: &str) -> String {
    let mut result = String::new();
    let mut rest = pinyin_str;
    while !rest.is_empty() {
        match split_first_syllable(rest) {
            Some((syl, remaining)) => {
                let initial = match syl.as_bytes()[0] {
                    b'z' if syl.starts_with("zh") => 'z',
                    b'c' if syl.starts_with("ch") => 'c',
                    b's' if syl.starts_with("sh") => 's',
                    b => b as char,
                };
                result.push(initial);
                rest = remaining;
            }
            None => {
                // V0.5.2：无法切分（含单字母/非音节，如 b站→bzhan、up主→upzhu、
                // emo→emo）——逐字符兜底取字母，保证混词简拼可用（bzhan→bz）。
                // 纯拼音词总能切分，此分支只影响中英混词。
                let c = rest.as_bytes()[0];
                if c.is_ascii_alphabetic() {
                    result.push(c as char);
                }
                rest = &rest[1..];
            }
        }
    }
    result
}

/// 保留 zh/ch/sh 两字符的声母串（V0.3.x，对标 rime abbrev zh ch sh 整体）：
/// 每音节取声母：zh/ch/sh 保留两字符，其余取首字母，零声母取首字母。
/// 雾凇约定：`abbrev/^([zcs]h).+$/$1/` 使 zh/ch/sh 音节缩写为两字符整体。
/// 示例："shehui" → "shh"（sh+h），"zhengzai" → "zhz"（zh+z），"zhongguo" → "zhg"
pub fn to_initial_full(pinyin_str: &str) -> String {
    let mut result = String::new();
    let mut rest = pinyin_str;
    while !rest.is_empty() {
        match split_first_syllable(rest) {
            Some((syl, remaining)) => {
                let bytes = syl.as_bytes();
                if bytes.len() >= 2 && matches!(&bytes[..2], b"zh" | b"ch" | b"sh") {
                    result.push_str(&syl[..2]);
                } else {
                    result.push(bytes[0] as char);
                }
                rest = remaining;
            }
            None => break, // 无法切分——停止
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_prefix() {
        assert!(is_valid_pinyin_prefix("zh"));
        assert!(is_valid_pinyin_prefix("zho"));
        assert!(is_valid_pinyin_prefix("zhon"));
        assert!(is_valid_pinyin_prefix("zhong"));
        assert!(!is_valid_pinyin_prefix("zzz"));
    }

    #[test]
    fn test_valid_syllable() {
        assert!(is_valid_syllable("zhong"));
        assert!(is_valid_syllable("ni"));
        assert!(is_valid_syllable("hao"));
        assert!(!is_valid_syllable("zho"));
    }

    #[test]
    fn test_split_first() {
        let (first, rest) = split_first_syllable("zhongguo").unwrap();
        assert_eq!(first, "zhong");
        assert_eq!(rest, "guo");
    }

    #[test]
    fn test_ambiguous_complete_path() {
        let (first, rest) = split_first_syllable("rengongzhineng").unwrap();
        assert_eq!(first, "ren");
        assert_eq!(rest, "gongzhineng");
        assert_eq!(to_initial_string("rengongzhineng"), "rgzn");
    }

    #[test]
    fn test_to_initial_string() {
        assert_eq!(to_initial_string("zhongguo"), "zg");
        assert_eq!(to_initial_string("women"), "wm");
        assert_eq!(to_initial_string("nihaoshijie"), "nhsj");
        assert_eq!(to_initial_string("ai"), "a");
        assert_eq!(to_initial_string("shichang"), "sc");
        // V0.5.2：中英混词逐字符兜底（b站→bz、up主→upz、emo→em、C位→cw、yyds→yyds）
        assert_eq!(to_initial_string("bzhan"), "bz");
        assert_eq!(to_initial_string("upzhu"), "upz");
        assert_eq!(to_initial_string("emo"), "em");
        assert_eq!(to_initial_string("cwei"), "cw");
        assert_eq!(to_initial_string("yyds"), "yyds");
    }
}
