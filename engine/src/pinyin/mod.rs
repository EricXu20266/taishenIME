/// 拼音处理模块 — 全拼输入
///
/// 第一期 MVP：仅支持全拼小写字母输入。
/// 第二期将加入双拼方案和拼音切分算法。

/// 拼音音节表（简化版，第一期 MVP 基础覆盖）
const VALID_SYLLABLES: &[&str] = &[
    "a", "ai", "an", "ang", "ao",
    "ba", "bai", "ban", "bang", "bao", "bei", "ben", "beng", "bi", "bian", "biao", "bie", "bin", "bing", "bo", "bu",
    "ca", "cai", "can", "cang", "cao", "ce", "cen", "ceng", "cha", "chai", "chan", "chang", "chao", "che", "chen", "cheng", "chi", "chong", "chou", "chu", "chua", "chuai", "chuan", "chuang", "chui", "chun", "chuo", "ci", "cong", "cou", "cu", "cuan", "cui", "cun", "cuo",
    "da", "dai", "dan", "dang", "dao", "de", "dei", "den", "deng", "di", "dian", "diao", "die", "ding", "diu", "dong", "dou", "du", "duan", "dui", "dun", "duo",
    "e", "ei", "en", "eng", "er",
    "fa", "fan", "fang", "fei", "fen", "feng", "fo", "fou", "fu",
    "ga", "gai", "gan", "gang", "gao", "ge", "gei", "gen", "geng", "gong", "gou", "gu", "gua", "guai", "guan", "guang", "gui", "gun", "guo",
    "ha", "hai", "han", "hang", "hao", "he", "hei", "hen", "heng", "hong", "hou", "hu", "hua", "huai", "huan", "huang", "hui", "hun", "huo",
    "ji", "jia", "jian", "jiang", "jiao", "jie", "jin", "jing", "jiong", "jiu", "ju", "juan", "jue", "jun",
    "ka", "kai", "kan", "kang", "kao", "ke", "kei", "ken", "keng", "kong", "kou", "ku", "kua", "kuai", "kuan", "kuang", "kui", "kun", "kuo",
    "la", "lai", "lan", "lang", "lao", "le", "lei", "leng", "li", "lia", "lian", "liang", "liao", "lie", "lin", "ling", "liu", "long", "lou", "lu", "luan", "lun", "luo", "lv", "lve",
    "ma", "mai", "man", "mang", "mao", "me", "mei", "men", "meng", "mi", "mian", "miao", "mie", "min", "ming", "miu", "mo", "mou", "mu",
    "na", "nai", "nan", "nang", "nao", "ne", "nei", "nen", "neng", "ni", "nian", "niang", "niao", "nie", "nin", "ning", "niu", "nong", "nou", "nu", "nuan", "nuo", "nv", "nve",
    "o", "ou",
    "pa", "pai", "pan", "pang", "pao", "pei", "pen", "peng", "pi", "pian", "piao", "pie", "pin", "ping", "po", "pou", "pu",
    "qi", "qia", "qian", "qiang", "qiao", "qie", "qin", "qing", "qiong", "qiu", "qu", "quan", "que", "qun",
    "ran", "rang", "rao", "re", "ren", "reng", "ri", "rong", "rou", "ru", "rua", "ruan", "rui", "run", "ruo",
    "sa", "sai", "san", "sang", "sao", "se", "sen", "seng", "sha", "shai", "shan", "shang", "shao", "she", "shei", "shen", "sheng", "shi", "shou", "shu", "shua", "shuai", "shuan", "shuang", "shui", "shun", "shuo", "si", "song", "sou", "su", "suan", "sui", "sun", "suo",
    "ta", "tai", "tan", "tang", "tao", "te", "tei", "teng", "ti", "tian", "tiao", "tie", "ting", "tong", "tou", "tu", "tuan", "tui", "tun", "tuo",
    "wa", "wai", "wan", "wang", "wei", "wen", "weng", "wo", "wu",
    "xi", "xia", "xian", "xiang", "xiao", "xie", "xin", "xing", "xiong", "xiu", "xu", "xuan", "xue", "xun",
    "ya", "yan", "yang", "yao", "ye", "yi", "yin", "ying", "yo", "yong", "you", "yu", "yuan", "yue", "yun",
    "za", "zai", "zan", "zang", "zao", "ze", "zei", "zen", "zeng", "zha", "zhai", "zhan", "zhang", "zhao", "zhe", "zhei", "zhen", "zheng", "zhi", "zhong", "zhou", "zhu", "zhua", "zhuai", "zhuan", "zhuang", "zhui", "zhun", "zhuo", "zi", "zong", "zou", "zu", "zuan", "zui", "zun", "zuo",
];

/// 检查给定的字符串是否是一个有效的拼音前缀
/// 用于判断用户输入是否可能构成合法的拼音
pub fn is_valid_pinyin_prefix(input: &str) -> bool {
    if input.is_empty() {
        return true;
    }
    VALID_SYLLABLES.iter().any(|s| s.starts_with(input))
}

/// 检查给定字符串是否是一个完整有效的拼音音节
pub fn is_valid_syllable(input: &str) -> bool {
    VALID_SYLLABLES.contains(&input)
}

/// 在输入串中尝试切分出首个完整音节
/// 返回 (音节, 剩余部分)
pub fn split_first_syllable(input: &str) -> Option<(&str, &str)> {
    for i in (1..=input.len().min(6)).rev() {
        let candidate = &input[..i];
        if is_valid_syllable(candidate) {
            let rest = &input[i..];
            return Some((candidate, rest));
        }
    }
    None
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
}
