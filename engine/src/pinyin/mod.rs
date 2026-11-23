//! 拼音处理模块 — 全拼输入
//!
//! 第一期 MVP：仅支持全拼小写字母输入。
//! 第二期将加入双拼方案和拼音切分算法。

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

/// 26 路平坦 ASCII 音节前缀树（V0.6 性能）：
/// 仅 411 条全小写拼音音节，构建后约 500 节点（~28 KB），常驻 L1 缓存；
/// 彻底消除 `HashSet` SipHash 与每步切片回查，供 `split_first_syllable` 与
/// 零分配整串切分共享。
#[derive(Clone, Copy)]
struct TrieNode {
    next: [u16; 26],
    is_syl: bool,
}

impl TrieNode {
    const EMPTY: Self = Self {
        next: [0; 26],
        is_syl: false,
    };
}

struct SyllableTrie {
    nodes: Vec<TrieNode>,
}

impl SyllableTrie {
    fn build() -> Self {
        let mut nodes = Vec::with_capacity(560);
        nodes.push(TrieNode::EMPTY);
        for &syl in VALID_SYLLABLES {
            let mut cur = 0usize;
            for &b in syl.as_bytes() {
                debug_assert!(b.is_ascii_lowercase());
                let slot = (b - b'a') as usize;
                let mut next = nodes[cur].next[slot];
                if next == 0 {
                    next = nodes.len() as u16;
                    nodes.push(TrieNode::EMPTY);
                    nodes[cur].next[slot] = next;
                }
                cur = next as usize;
            }
            nodes[cur].is_syl = true;
        }
        Self { nodes }
    }

    #[inline]
    fn walk(&self, bytes: &[u8]) -> Option<usize> {
        let mut cur = 0usize;
        for &b in bytes {
            if !b.is_ascii_lowercase() {
                return None;
            }
            let next = self.nodes[cur].next[(b - b'a') as usize];
            if next == 0 {
                return None;
            }
            cur = next as usize;
        }
        Some(cur)
    }

    #[inline]
    fn contains_prefix(&self, bytes: &[u8]) -> bool {
        self.walk(bytes).is_some()
    }

    #[inline]
    fn contains_syllable(&self, bytes: &[u8]) -> bool {
        !bytes.is_empty() && self.walk(bytes).is_some_and(|idx| self.nodes[idx].is_syl)
    }
}

fn syllable_trie() -> &'static SyllableTrie {
    static TRIE: std::sync::OnceLock<SyllableTrie> = std::sync::OnceLock::new();
    TRIE.get_or_init(SyllableTrie::build)
}

/// 检查给定的字符串是否是一个有效的拼音前缀
/// 用于判断用户输入是否可能构成合法的拼音
pub fn is_valid_pinyin_prefix(input: &str) -> bool {
    if input.is_empty() {
        return true;
    }
    syllable_trie().contains_prefix(input.as_bytes())
}

/// 检查给定字符串是否是一个完整有效的拼音音节
pub fn is_valid_syllable(input: &str) -> bool {
    syllable_trie().contains_syllable(input.as_bytes())
}

#[inline]
fn segment_reachability_u64(bytes: &[u8], trie: &SyllableTrie) -> u64 {
    let n = bytes.len();
    debug_assert!(n < 64);
    let mut reachable: u64 = 1u64 << n;
    for start in (0..n).rev() {
        let mut cur = 0usize;
        let max_end = (start + 6).min(n);
        for end in (start + 1)..=max_end {
            let b = bytes[end - 1];
            if !b.is_ascii_lowercase() {
                break;
            }
            let next = trie.nodes[cur].next[(b - b'a') as usize];
            if next == 0 {
                break;
            }
            cur = next as usize;
            if trie.nodes[cur].is_syl && (reachable & (1u64 << end)) != 0 {
                reachable |= 1u64 << start;
                break;
            }
        }
    }
    reachable
}

/// 判断输入串是否存在一条完整的合法音节路径。
/// 沿 `SyllableTrie` 从每个起点单向前推，无哈希且不切分非 ASCII 字符。
fn can_segment_completely(input: &str) -> bool {
    if input.is_empty() {
        return true;
    }
    let bytes = input.as_bytes();
    let n = bytes.len();
    let trie = syllable_trie();
    if n < 64 {
        return segment_reachability_u64(bytes, trie) & 1 != 0;
    }
    let mut reachable = vec![false; n + 1];
    reachable[n] = true;
    for start in (0..n).rev() {
        let mut cur = 0usize;
        let max_end = (start + 6).min(n);
        for end in (start + 1)..=max_end {
            let b = bytes[end - 1];
            if !b.is_ascii_lowercase() {
                break;
            }
            let next = trie.nodes[cur].next[(b - b'a') as usize];
            if next == 0 {
                break;
            }
            cur = next as usize;
            if trie.nodes[cur].is_syl && reachable[end] {
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
    let bytes = input.as_bytes();
    let max_len = bytes.len().min(6);
    if max_len == 0 {
        return None;
    }
    let trie = syllable_trie();
    let mut lens = [0usize; 6];
    let mut len_count = 0usize;
    let mut cur = 0usize;
    for (i, &b) in bytes[..max_len].iter().enumerate() {
        if !b.is_ascii_lowercase() {
            break;
        }
        let next = trie.nodes[cur].next[(b - b'a') as usize];
        if next == 0 {
            break;
        }
        cur = next as usize;
        if trie.nodes[cur].is_syl {
            lens[len_count] = i + 1;
            len_count += 1;
        }
    }
    if len_count == 0 {
        return None;
    }
    let fallback_len = lens[len_count - 1];
    for &len in lens[..len_count].iter().rev() {
        let rest = &input[len..];
        if can_segment_completely(rest) {
            return Some((&input[..len], rest));
        }
    }
    Some((&input[..fallback_len], &input[fallback_len..]))
}

/// `O(n)` 零堆分配整串音节切分（写入调用方栈数组）。
///
/// 与 `while split_first_syllable` 完全等价：仅当整串可完全切分为合法音节且
/// 音节数不超过 16、字节数不超过 63 时返回 `Some(count)`；否则返回 `None`
/// （不可切分串直接剪枝，超长串改走 [`split_syllables_slices`]）。
pub fn split_syllables_into<'a>(input: &'a str, out: &mut [&'a str; 16]) -> Option<usize> {
    let bytes = input.as_bytes();
    let n = bytes.len();
    if n == 0 {
        return Some(0);
    }
    if n > 63 {
        return None;
    }
    let trie = syllable_trie();
    let reachable = segment_reachability_u64(bytes, trie);
    if reachable & 1 == 0 {
        return None;
    }
    let mut pos = 0usize;
    let mut count = 0usize;
    while pos < n {
        if count >= out.len() {
            return None;
        }
        let mut cur = 0usize;
        let max_end = (pos + 6).min(n);
        let mut best_end = 0usize;
        for end in (pos + 1)..=max_end {
            let b = bytes[end - 1];
            if !b.is_ascii_lowercase() {
                break;
            }
            let next = trie.nodes[cur].next[(b - b'a') as usize];
            if next == 0 {
                break;
            }
            cur = next as usize;
            if trie.nodes[cur].is_syl && (reachable & (1u64 << end)) != 0 {
                best_end = end;
            }
        }
        if best_end == 0 {
            return None;
        }
        out[count] = &input[pos..best_end];
        count += 1;
        pos = best_end;
    }
    Some(count)
}

/// 超长串（>63 字节或 >16 音节）兜底切分：零 `String` 分配返回借用切片。
pub fn split_syllables_slices(input: &str) -> Option<Vec<&str>> {
    let mut res = Vec::new();
    let mut rest = input;
    while !rest.is_empty() {
        let (syl, tail) = split_first_syllable(rest)?;
        res.push(syl);
        rest = tail;
    }
    Some(res)
}

/// 枚举输入串的全部完整音节切分路径（V0.6，隔音歧义支持）。
///
/// 背景：拼音注音去掉了音节边界，yuan 既是单音节 yuan（元），
/// 也是 yu+an（预案）；xian 既是单音节（先），也是 xi+an（西安）。
/// `split_first_syllable` 只给最长音节一条路径，跨音节词因此被当成
/// 单音节输入处理（词长分区压后、组词模式不触发）。
///
/// 返回上限 `limit` 条路径，按音节长优先展开（首条与 split_first_syllable 一致）。
/// 非纯 ASCII 小写输入（含大写/数字/符号）返回空。
pub fn syllable_split_variants(input: &str, limit: usize) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    if input.is_empty() || limit == 0 || !input.bytes().all(|b| b.is_ascii_lowercase()) {
        return out;
    }
    let mut path: Vec<String> = Vec::new();
    collect_split_variants(input, &mut path, &mut out, limit);
    out
}

fn collect_split_variants(
    rest: &str,
    path: &mut Vec<String>,
    out: &mut Vec<Vec<String>>,
    limit: usize,
) {
    if out.len() >= limit {
        return;
    }
    if rest.is_empty() {
        out.push(path.clone());
        return;
    }
    for i in (1..=rest.len().min(6)).rev() {
        let seg = &rest[..i];
        if !is_valid_syllable(seg) {
            continue;
        }
        path.push(seg.to_string());
        collect_split_variants(&rest[i..], path, out, limit);
        path.pop();
        if out.len() >= limit {
            return;
        }
    }
}

/// 检查输入串能否完全切分为合法拼音音节序列（0.3.x fix）。
/// 用于区分"拼音输入"与"英文单词/简拼"，防模糊音/纠错误伤英文：
///   - "zhongguo" → zhong+guo 完全切分 ✓ → 拼音，可做模糊/纠错/错音联想
///   - "hello"    → he+llo，llo 非音节 → 英文，不做拼音联想（只英文混输）
///   - "zg"       → z 非音节 → 简拼，走简拼查询但不做模糊/纠错
pub fn is_complete_pinyin(input: &str) -> bool {
    !input.is_empty() && can_segment_completely(input)
}

/// Combo 组词前置门槛：输入能否作为"拼音串"参与组词/简拼猜词。
///
/// 为什么需要它：`query_combo` / `combo_guess` / `query_short` 的匹配是宽松的
/// （段可以是单声母、半截音节），键盘连打串也能凑出词库里的真实词条——
/// `qwer` 组出「气味儿/去问让」、`sdfg` 组出「宋代法规/是都发个」。
/// 这类串的完整音节只是零碎巧合（q-w-er、s-d-f-g），拼音主体全是声母，
/// 组出来的词只是首字母碰巧对上，必然是噪声。
///
/// 判据（放行条件，任一满足）：
///   1. 整串可切分为完整音节序列（zhongguo / zhongguoo / dianjishiyong / huar）；
///   2. 首段是长度 ≥ 2 的完整音节（nimzai / heshenm / ganshm / yaowoquz / zhegew）；
///   3. 声母起头但音节为主导：完整音节覆盖的字母数 > 声母段字母数
///      （bru→b+ru、zhyang→zh+yang、shurf→shu+r+f）。
///      纯声母串（qwer/sdfg/wert/nhsj/zg）音节为 0 或与声母等长，拦。
///
/// 含数字/符号等非小写字母的输入返回 false，交给英文/混输路径处理。
pub fn is_combo_syllable_plausible(input: &str) -> bool {
    if input.is_empty() || !input.bytes().all(|b| b.is_ascii_lowercase()) {
        return false;
    }
    if is_complete_pinyin(input) {
        return true;
    }
    if matches!(split_first_syllable(input), Some((syl, _)) if syl.len() >= 2) {
        return true;
    }
    let mut budget = 1024;
    combo_syllable_dominant(input, 0, 0, &mut budget)
}

/// 递归判断是否存在一条切分，使完整音节覆盖的字母数严格多于声母段。
/// `syl` / `init` 是两条路径上已累计的字母数；`budget` 防病态输入指数展开。
fn combo_syllable_dominant(rest: &str, syl: usize, init: usize, budget: &mut usize) -> bool {
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    if rest.is_empty() {
        return syl > init;
    }
    // 剩余字母全算音节也不够反超 → 剪枝
    if syl + rest.len() <= init {
        return false;
    }
    let max = rest.len().min(6);
    // 完整音节（长度 ≥ 2）。a/e/o 这类单字母音节归声母侧——
    // 否则 qwer 里的 e、wert 里的 e 会把纯键盘串洗成"音节为主导"。
    for i in (2..=max).rev() {
        if is_valid_syllable(&rest[..i])
            && combo_syllable_dominant(&rest[i..], syl + i, init, budget)
        {
            return true;
        }
    }
    // 声母段：zh/ch/sh 两字符整体，或单字母声母
    if (rest.starts_with("zh") || rest.starts_with("ch") || rest.starts_with("sh"))
        && combo_syllable_dominant(&rest[2..], syl, init + 2, budget)
    {
        return true;
    }
    let first = rest.as_bytes()[0] as char;
    "bpmfdtnlgkhjqxzcsrwyaeo".contains(first)
        && combo_syllable_dominant(&rest[1..], syl, init + 1, budget)
}

/// 键盘连打判据（2026-11-19，简拼过滤用）：输入串是否像「手在键盘上划过去」。
///
/// 为什么需要它：纯声母串既可能是合法简拼（cyl→产业链、gyl→供应链、aqsp→安全水平、
/// xdh→现代化），也可能是无意义连打（sdfg→宋代法规、zzz→自治州/张召重、qwer/wert）。
/// 两者在「声母合法性」上无法区分——s/d/f/g/z 全是合法声母，所以
/// `is_combo_syllable_plausible` 判不了简拼系的真假，硬套常用词表又会误杀
/// 真实简拼（产业链/供应链都不在 2732 条常用词里）。
///
/// 判据：存在长度 ≥ 3 的连续段，段内相邻字符满足下列之一——
///   · 同一键重复（zzz、aaa）；
///   · 同一行左右相邻（sdfg、qwer、asdf）；
///   · 同一列上下相邻（qaz、wsx）。
/// 斜向（q→s、a→w）不算连续：aqsp 的 a→q 是竖排（长度 2），q→s 是斜向，
/// 最长段只有 2，因此不会误伤真实简拼。
///
/// 只在「非完整拼音」分支使用——完整拼音有真实读音，不需要这个判据。
pub fn is_keyboard_run(input: &str) -> bool {
    if input.len() < 3 || !input.bytes().all(|b| b.is_ascii_lowercase()) {
        return false;
    }
    const ROWS: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];
    let pos = |c: char| -> Option<(usize, usize)> {
        ROWS.iter()
            .enumerate()
            .find_map(|(r, row)| row.find(c).map(|col| (r, col)))
    };
    let chars: Vec<char> = input.chars().collect();
    let mut run = 1usize;
    for i in 1..chars.len() {
        let continuous = match (pos(chars[i - 1]), pos(chars[i])) {
            (Some(a), Some(b)) => {
                (a == b)                                     // 同键重复
                    || (a.0 == b.0 && a.1.abs_diff(b.1) == 1) // 同行左右
                    || (a.1 == b.1 && a.0.abs_diff(b.0) == 1) // 同列上下
            }
            _ => false,
        };
        run = if continuous { run + 1 } else { 1 };
        if run >= 3 {
            return true;
        }
    }
    false
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
    fn test_combo_syllable_gate() {
        // 完整拼音与错拼：放行
        assert!(is_combo_syllable_plausible("zhongguo"));
        assert!(is_combo_syllable_plausible("zhongguoo"));
        assert!(is_combo_syllable_plausible("dianjishiyong"));
        assert!(is_combo_syllable_plausible("huar"));
        // 混拼：含真音节，放行
        assert!(is_combo_syllable_plausible("nimzai"));
        assert!(is_combo_syllable_plausible("yaowoquz"));
        assert!(is_combo_syllable_plausible("heshenm"));
        assert!(is_combo_syllable_plausible("ganshm"));
        // 声母起头、音节为主导的混拼（bru→b+ru、zhyang→zh+yang、shurf→shu+r+f）
        assert!(is_combo_syllable_plausible("bru"));
        assert!(is_combo_syllable_plausible("zhyang"));
        assert!(is_combo_syllable_plausible("shurf"));
        // 键盘连打 / 纯声母：不从真实音节起头，拦截
        assert!(!is_combo_syllable_plausible("qwer"));
        assert!(!is_combo_syllable_plausible("sdfg"));
        assert!(!is_combo_syllable_plausible("wert"));
        assert!(!is_combo_syllable_plausible("zxcvb"));
        assert!(!is_combo_syllable_plausible("nhsj"));
        assert!(!is_combo_syllable_plausible("rgshni"));
        assert!(!is_combo_syllable_plausible("zg"));
    }

    #[test]
    fn test_keyboard_run() {
        // 键盘连打：拦
        assert!(is_keyboard_run("sdfg"));
        assert!(is_keyboard_run("qwer"));
        assert!(is_keyboard_run("wert"));
        assert!(is_keyboard_run("asdfg"));
        assert!(is_keyboard_run("zxcvb"));
        assert!(is_keyboard_run("zzz"));
        assert!(is_keyboard_run("qaz"));
        // 真实简拼：放行（含相邻对但连续段 < 3）
        assert!(!is_keyboard_run("cyl"));
        assert!(!is_keyboard_run("gyl"));
        assert!(!is_keyboard_run("aqsp"));
        assert!(!is_keyboard_run("xjzzy"));
        assert!(!is_keyboard_run("xdh"));
        assert!(!is_keyboard_run("tx"));
        assert!(!is_keyboard_run("rx"));
        assert!(!is_keyboard_run("gj"));
        assert!(!is_keyboard_run("gg"));
        assert!(!is_keyboard_run("nh"));
        assert!(!is_keyboard_run("zg"));
    }

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

    #[test]
    fn test_trie_and_zero_alloc_split_equivalence() {
        for &syl in VALID_SYLLABLES {
            assert!(is_valid_syllable(syl), "syllable {syl} must be valid");
            for i in 1..=syl.len() {
                assert!(
                    is_valid_pinyin_prefix(&syl[..i]),
                    "prefix {} must be valid",
                    &syl[..i]
                );
            }
        }
        assert!(is_valid_pinyin_prefix(""));
        assert!(!is_valid_syllable(""));
        assert!(!is_valid_syllable("中文"));
        assert!(!is_valid_pinyin_prefix("中文"));

        let samples = [
            "",
            "a",
            "zhong",
            "zhongguo",
            "zhongguorenmin",
            "rengongzhineng",
            "xian",
            "xianzai",
            "yuan",
            "jintiantianqi",
            "womenyiqi",
            "zhegewomende",
            "zhchsh",
            "hello",
            "nihao123",
            "A",
            "中文",
            "ni好",
            // > 63 字节 / > 16 音节超长串
            "zhonghuarenmingongheguozhonghuarenmingongheguozhonghuarenmingongheguo",
            "aiaiaiaiaiaiaiaiaiaiaiaiaiaiaiaiai",
        ];

        for &s in &samples {
            let mut expected = Vec::new();
            let mut rest = s;
            let mut ok = !s.is_empty();
            while !rest.is_empty() {
                match split_first_syllable(rest) {
                    Some((syl, tail)) => {
                        expected.push(syl);
                        rest = tail;
                    }
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            assert_eq!(
                is_complete_pinyin(s),
                ok,
                "is_complete_pinyin mismatch on {s:?}"
            );

            let mut buf = [""; 16];
            let fast = split_syllables_into(s, &mut buf);
            let slices = split_syllables_slices(s);
            if ok {
                assert_eq!(
                    slices.as_deref(),
                    Some(expected.as_slice()),
                    "split_syllables_slices mismatch on {s:?}"
                );
                if s.len() <= 63 && expected.len() <= 16 {
                    assert_eq!(fast, Some(expected.len()), "fast count mismatch on {s:?}");
                    assert_eq!(&buf[..fast.unwrap()], expected.as_slice());
                } else {
                    assert_eq!(fast, None);
                }
            } else if s.is_empty() {
                assert_eq!(fast, Some(0));
                assert_eq!(slices.as_deref(), Some([].as_slice()));
            } else {
                assert_eq!(fast, None);
            }
        }
    }
}
