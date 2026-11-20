//! 整句解码：把一串拼音读成最可能的词序列。
//!
//! 逐段贪心解决不了长句——某个词单独看词频高，不等于它在整句里成立
//! （「几个」后面接「认栽」也能拼成合法词序列）。这里在音节图上走 Viterbi +
//! 束搜索，比较的是「整句所有词得分之和」，不让探索顺序决定结果。
//!
//! 三个打分来源：词库词频（按词数折算成可比尺度）、相邻词搭配档位、层级权重。
//! 搭配表缺失或稀疏时退化为词频打分，不会因为缺数据让整句失效。

use std::collections::HashMap;

use crate::dictionary;

/// 一条整句最多多少音节。再长按粘贴处理，不猜。
const MAX_SYLLABLES: usize = 30;
/// 输入串字符上限。
const MAX_INPUT_CHARS: usize = 90;
/// 少于这个音节数交给词级候选，不猜整句。
pub const MIN_SYLLABLES: usize = 3;
/// 一个词最多覆盖几个音节。
const MAX_WORD_SYLLABLES: usize = 8;
/// 每个格子取几个词。
const SPAN_TAKE: usize = 6;
/// 束宽：每个位置保留几条部分路径。
const BEAM_WIDTH: usize = 8;
/// 一次编辑（多打 / 打错 / 漏打）的代价（自然对数尺度）。
const EDIT_COST: f64 = 8.0;
/// 系统词库 frequency 列的对数缩放系数：`system_dict.db` 的 `frequency`
/// 存储的是 `round(365.0 * ln(raw_count))`（实测 count=2→253、3→401、4→506、
/// 8→759、9→802，精确吻合），本身已是自然对数尺度。
/// 此前对它再取 `ln(freq + 1)` 等于双重对数，把 5 倍词频差压成 0.2，导致：
///   1. 同音字分不清高下（`是` 4962 vs `使` 4045 只差 0.20，被 bigram 翻转）；
///   2. 拆碎单字与成词的差距被压平（`组词` 401 输给 `组+次`、`遇到` 3120 只赢 `与+到` 0.32）。
/// 还原真实对数词频 `freq / FREQ_LOG_SCALE` 后，词频差与词数惩罚回到同一自然对数单位（nats）。
const FREQ_LOG_SCALE: f64 = 365.0;
/// 系统词库总原始词频的自然对数：`ln(sum(exp(freq / 365.0))) ≈ ln(6.21e7) = 17.944`。
/// 在标准一元语言模型里，每个词的得分就是 `ln(P(w)) = freq / 365.0 - LN_RAW_TOTAL`。
const LN_RAW_TOTAL: f64 = 17.944;
/// 低频词阈值与额外扣分：raw_count ≤ 2（freq ≤ 253）多是偶发专名或噪声组合。
/// 注意不能设高于 400（raw_count = 3），否则会把「组词」(401) 这类真实短词误伤。
const LOW_FREQ_CUTOFF: u32 = 260;
const LOW_FREQ_PENALTY: f64 = 1.0;
/// 常用词人工优先级的加分幅度与指数衰减尺度（nats）。
/// 常用词表行序代表口语常用度，用来纠正书面语词频偏差（如「想」rank 22 词频 4029
/// 低于介词「向」rank 490 词频 4103，真实对数差 0.203 nats）。
/// 采用指数衰减 `exp(-rank / 300)` 而非线性衰减：头部 100 个超高频口语字能有效拉开
/// （想 +0.42 vs 向 +0.09，差 +0.33 > 0.203），中后部单字（rank 400+）加分迅速归零，
/// 不会让两个普通单字叠加双份加分反压多字词（如「组+次」反压「组词」）。
const COMMON_RANK_BONUS: f64 = 0.45;
const COMMON_RANK_DECAY: f64 = 300.0;
/// 多字常用词加成（nats，仅给 ≥2 字词）：
/// 常用词表（common.db）里的多字词是人工校验过的口语真词（如「组词」在维基语料里
/// 词频只有 401，而「足」「此」的单字字频各在 3400-3887）。只给 ≥2 字常用词加成，
/// 绝不给单字叠加，因此只鼓励成词、不奖励拆碎。
const COMMON_MULTI_BONUS: f64 = 1.2;
/// 搭配档位的权重（档位 2-6，自然对数尺度：2 档 +0.49，6 档 +1.50 nats）。
const BIGRAM_W: f64 = 1.2;
/// 搭配档位的证据门槛。档位 1 = 只共现过一次，实测占 bigram.db 的 98.6%，
/// 1 档视为无证据，2 档起按相对增量计分。
const BIGRAM_MIN_COUNT: u32 = 2;
/// 用户词层级加成（自然对数尺度，仅给多字用户词）：
/// 日常打字几乎把所有常用单字都学成了用户词（实测用户词库 675 个单字，
/// 会 176 次、到 160 次、与 11 次），单字也加成时「会 + 与 + 到」吃三份加成
/// 反而压过「会 + 遇到」两份，huiyudao 首选成了「会与到」；
/// 同一机制让「数入有误 / 以个做次 / 明先不是」压过「输入有误 / 一个组词 / 明显不是」。
/// 只给 ≥2 字用户词加分，既保留用户个人词组优先级，又不奖励把词拆碎成单字。
const USER_BONUS: f64 = 1.5;
/// 最多留几条切分路径（音节边界歧义，如 yuan = yu+an）。
const MAX_SEGMENTATIONS: usize = 8;
/// 容错切分允许的总编辑数。
const MAX_EDITS: u8 = 3;
/// 容错切分里单次编辑的候选上限（每段只试这么多个音节）。
const NEAR_TAKE: usize = 3;

/// 一条部分路径的末尾节点。
struct Node {
    /// 这个词从第几个音节开始。
    start: usize,
    /// 词。
    text: String,
    /// 到此为止的累计得分。
    score: f64,
    /// 前驱在 `nodes[start]` 里的下标。
    back: usize,
}

/// 整句解码入口：返回得分最高的前 `limit` 条整句（文本去重）。
///
/// `prev` 是前文已上屏的最后一个词，参与句首搭配打分。
/// 不能在拼音上读通的输入（含英文、数字）返回空，交给调用方的其他候选来源。
pub fn decode(input: &str, prev: Option<&str>, limit: usize) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }
    let input = crate::pinyin::normalize_v(&input.to_lowercase());
    if input.len() < MIN_SYLLABLES * 2
        || input.len() > MAX_INPUT_CHARS
        || !input.bytes().all(|b| b.is_ascii_lowercase())
    {
        return Vec::new();
    }
    let segs = segmentations(&input);
    if segs.is_empty() {
        return Vec::new();
    }
    let mut cache: HashMap<String, Vec<(String, u32, u8)>> = HashMap::new();
    let mut scored: Vec<(String, f64)> = Vec::new();
    // 音节数下限按**最短切分**判断：zhongguo 同时能切成 zhong+guo（2）与
    // zhong+gu+o（3，guo 拆成 gu 和叹词 o），只看最长切分会让双音节词
    // 混进整句通道，解出「中过」插到首位，把词级命中的「中国」压到第 3。
    // 整句只处理真正的长串，两三音节的输入交给词级候选。
    if segs.iter().map(|s| s.len()).min().unwrap_or(0) < MIN_SYLLABLES {
        return Vec::new();
    }
    for syls in &segs {
        if syls.len() > MAX_SYLLABLES {
            continue;
        }
        scored.extend(viterbi(syls, prev, limit, &mut cache));
    }
    if scored.is_empty() {
        return Vec::new();
    }
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<String> = Vec::new();
    for (text, _) in &scored {
        if out.len() >= limit {
            break;
        }
        if text.is_empty() || out.contains(text) {
            continue;
        }
        out.push(text.clone());
    }
    if std::env::var("TS_SENTENCE_DEBUG").is_ok() {
        eprintln!(
            "[decode] {input} -> {out:?}  (scored 前 6: {:?})",
            scored.iter().take(6).collect::<Vec<_>>()
        );
    }
    out
}

/// 音节切分：能整串读成拼音就按原样读，读不通（有多打/打错的字母）才引入编辑边。
fn segmentations(input: &str) -> Vec<Vec<(String, u8)>> {
    let exact = segment_exact(input, MAX_SEGMENTATIONS);
    if !exact.is_empty() {
        return exact;
    }
    segment_tolerant(input, MAX_SEGMENTATIONS)
}

/// 精确切分：每个位置必须是一个合法音节。按音节长优先展开，最多留 `limit` 条。
fn segment_exact(input: &str, limit: usize) -> Vec<Vec<(String, u8)>> {
    let mut out: Vec<Vec<(String, u8)>> = Vec::new();
    let mut path: Vec<(String, u8)> = Vec::new();
    let mut budget = EXACT_NODE_BUDGET;
    collect_exact(input, 0, &mut path, &mut out, limit, &mut budget);
    out
}

/// 精确切分的递归节点预算：长输入的音节歧义组合同样可能爆炸。
const EXACT_NODE_BUDGET: usize = 20_000;

#[allow(clippy::too_many_arguments)]
fn collect_exact(
    input: &str,
    pos: usize,
    path: &mut Vec<(String, u8)>,
    out: &mut Vec<Vec<(String, u8)>>,
    limit: usize,
    budget: &mut usize,
) {
    if *budget == 0 || out.len() >= limit || path.len() >= MAX_SYLLABLES {
        return;
    }
    *budget -= 1;
    if pos == input.len() {
        if !path.is_empty() {
            out.push(path.clone());
        }
        return;
    }
    let max_end = input.len().min(pos + 6);
    for end in (pos + 1..=max_end).rev() {
        let seg = &input[pos..end];
        if crate::pinyin::is_valid_syllable(seg) {
            path.push((seg.to_string(), 0));
            collect_exact(input, end, path, out, limit, budget);
            path.pop();
            if out.len() >= limit || *budget == 0 {
                return;
            }
        }
    }
}

/// 容错切分：允许跳过单字符（多打）、音节替换/插入/删除一个字符（打错/漏打）。
fn segment_tolerant(input: &str, limit: usize) -> Vec<Vec<(String, u8)>> {
    let mut out: Vec<Vec<(String, u8)>> = Vec::new();
    let mut path: Vec<(String, u8)> = Vec::new();
    let mut budget = TOLERANT_NODE_BUDGET;
    collect_tolerant(input, 0, 0, 0, &mut path, &mut out, limit, &mut budget);
    out
}

/// 容错切分的递归节点预算。
///
/// 「跳过字符」在每一层都是合法分支，而结果只在整串读通时才计入输出——
/// 走不到末尾的分支不会推进输出上限，没有预算就会按分支数指数展开
/// （实测 25 字符输入直接把按键线程卡死）。预算内穷尽，耗尽就放弃剩余分支。
const TOLERANT_NODE_BUDGET: usize = 20_000;

#[allow(clippy::too_many_arguments)]
fn collect_tolerant(
    input: &str,
    pos: usize,
    edits: u8,
    pending_cost: u8,
    path: &mut Vec<(String, u8)>,
    out: &mut Vec<Vec<(String, u8)>>,
    limit: usize,
    budget: &mut usize,
) {
    if *budget == 0 || out.len() >= limit || path.len() >= MAX_SYLLABLES {
        return;
    }
    *budget -= 1;
    if pos == input.len() {
        if !path.is_empty() {
            out.push(path.clone());
        }
        return;
    }
    let max_end = input.len().min(pos + 6);
    // 每段只算一次候选，再按代价分两轮展开：`near_syllables` 要全量扫音节表，
    // 放在 cost 循环里会被白算两遍。
    let mut spans: Vec<(usize, Vec<(String, u8)>)> = Vec::with_capacity(max_end - pos);
    for end in (pos + 1..=max_end).rev() {
        let seg = &input[pos..end];
        spans.push((end, near_syllables(seg)));
    }
    // 先走原样音节（代价 0），再走编辑音节，最后才跳过字符。
    // 顺序决定谁先占满输出配额：长句容错里最有价值的是「大部分音节原样」的路径。
    for cost_filter in [0u8, 1] {
        for (end, hits) in &spans {
            for (syl, cost) in hits {
                if *cost != cost_filter || edits + cost > MAX_EDITS {
                    continue;
                }
                path.push((syl.clone(), (*cost).saturating_add(pending_cost)));
                collect_tolerant(input, *end, edits + cost, 0, path, out, limit, budget);
                path.pop();
                if out.len() >= limit || *budget == 0 {
                    return;
                }
            }
        }
    }
    // 最后：跳过这个字符（多打的孤字母，`nim` 里的 m），代价必须计入已走路径
    if edits < MAX_EDITS {
        if let Some(last) = path.last_mut() {
            last.1 = last.1.saturating_add(1);
            collect_tolerant(
                input,
                pos + 1,
                edits + 1,
                pending_cost,
                path,
                out,
                limit,
                budget,
            );
            path.last_mut().unwrap().1 -= 1;
        } else {
            collect_tolerant(
                input,
                pos + 1,
                edits + 1,
                pending_cost.saturating_add(1),
                path,
                out,
                limit,
                budget,
            );
        }
    }
}

/// 一段输入可能读成哪些音节：原样优先，其次单字符编辑（首字母相同优先）。
fn near_syllables(seg: &str) -> Vec<(String, u8)> {
    if crate::pinyin::is_valid_syllable(seg) {
        return vec![(seg.to_string(), 0)];
    }
    // 单字母不做音节替换：`m` 能被纠成 ma / me / mi，但用户更可能是多打了
    // 这个字母，或者只想打个声母——交给上层的「跳过」分支，别造出「你妈几个」
    if seg.len() < 2 {
        return Vec::new();
    }
    let mut out: Vec<(&'static str, u8)> = Vec::new();
    let mut fallback: Vec<(&'static str, u8)> = Vec::new();
    for group in syllable_groups() {
        for &syl in group {
            if !edit_distance_le_one(seg, syl) {
                continue;
            }
            let bucket = if seg.as_bytes().first() == syl.as_bytes().first() {
                &mut out
            } else {
                &mut fallback
            };
            if bucket.len() < NEAR_TAKE && !bucket.iter().any(|(s, _)| *s == syl) {
                bucket.push((syl, 1));
            }
        }
    }
    out.extend(fallback);
    out.truncate(NEAR_TAKE);
    out.into_iter()
        .map(|(syl, cost)| (syl.to_string(), cost))
        .collect()
}

/// 按字节长度分组的音节表（音节最长 6 字符）。
fn syllable_groups() -> &'static [Vec<&'static str>; 7] {
    static GROUPS: std::sync::OnceLock<[Vec<&'static str>; 7]> = std::sync::OnceLock::new();
    GROUPS.get_or_init(|| {
        let mut groups: [Vec<&'static str>; 7] = Default::default();
        for &syl in crate::pinyin::all_syllables() {
            if syl.len() < groups.len() {
                groups[syl.len()].push(syl);
            }
        }
        groups
    })
}

/// 两个字符串的编辑距离是否为 1（长度差不超过 1）。
fn edit_distance_le_one(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let mut i = 0;
    while i < a.len() && i < b.len() && a[i] == b[i] {
        i += 1;
    }
    match a.len().cmp(&b.len()) {
        std::cmp::Ordering::Equal => a.get(i + 1..) == b.get(i + 1..),
        std::cmp::Ordering::Greater => a.get(i + 1..) == b.get(i..),
        std::cmp::Ordering::Less => a.get(i..) == b.get(i + 1..),
    }
}

/// 音节图上的 Viterbi + 束搜索：返回得分最高的前 `limit` 条路径。
fn viterbi(
    syls: &[(String, u8)],
    prev: Option<&str>,
    limit: usize,
    cache: &mut HashMap<String, Vec<(String, u32, u8)>>,
) -> Vec<(String, f64)> {
    let n = syls.len();
    let mut nodes: Vec<Vec<Node>> = (0..=n).map(|_| Vec::new()).collect();
    nodes[0].push(Node {
        start: 0,
        text: String::new(),
        score: 0.0,
        back: 0,
    });
    for start in 0..n {
        if nodes[start].is_empty() {
            continue;
        }
        prune(&mut nodes[start]);
        // 先取一份前驱快照：下面要写 nodes[end]，不能同时持有 nodes[start] 的借用
        let preds: Vec<(f64, String)> = nodes[start]
            .iter()
            .map(|node| (node.score, node.text.clone()))
            .collect();
        for end in start + 1..=n.min(start + MAX_WORD_SYLLABLES) {
            let span = &syls[start..end];
            let edits: u8 = span.iter().map(|(_, cost)| *cost).sum();
            let key: String = span.iter().map(|(s, _)| s.as_str()).collect();
            if !cache.contains_key(&key) {
                let words = dictionary::span_candidates(&key, SPAN_TAKE);
                cache.insert(key.clone(), words);
            }
            let Some(hits) = cache.get(&key) else {
                continue;
            };
            for (word, freq, layer) in hits {
                // 用户词的拼音键可能是早期学进去的半截别名（实测 ta→他们、da→到、
                // e→哦）：字数和这段音节数对不上就不是这段的读音，跳过。
                // 只管用户层——系统词有「T恤」这类字母词，字数本来就不等于音节数。
                if *layer == 2 && word.chars().count() != end - start {
                    continue;
                }
                let mut best_score = f64::NEG_INFINITY;
                let mut best_back = 0usize;
                for (index, (pred_score, pred_text)) in preds.iter().enumerate() {
                    let context = if start == 0 {
                        prev
                    } else {
                        Some(pred_text.as_str())
                    };
                    let score = pred_score + step_score(context, word, *freq, *layer)
                        - f64::from(edits) * EDIT_COST;
                    if score > best_score {
                        best_score = score;
                        best_back = index;
                    }
                }
                nodes[end].push(Node {
                    start,
                    text: word.clone(),
                    score: best_score,
                    back: best_back,
                });
            }
        }
    }
    nodes[n].sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if std::env::var("TS_SENTENCE_DEBUG").is_ok() {
        eprintln!("[sentence] n={n}");
        for (index, node) in nodes[n].iter().take(8).enumerate() {
            let (text, _, words) = backtrack(&nodes, n, index);
            eprintln!(
                "   {:>8.3} start={} word={} -> {text} ({words}词)",
                node.score, node.start, node.text
            );
        }
    }
    let mut out: Vec<(String, f64)> = Vec::new();
    for index in 0..nodes[n].len() {
        if out.len() >= limit {
            break;
        }
        let (text, score, words) = backtrack(&nodes, n, index);
        // 整段读成一个词交给词级候选，这里只出自造的整句
        if words < 2 {
            continue;
        }
        if !out.iter().any(|(t, _)| *t == text) {
            out.push((text, score));
        }
    }
    out
}

/// 从一个末尾节点回溯整条路径，返回 (整句文本, 得分, 词数)。
fn backtrack(nodes: &[Vec<Node>], mut position: usize, mut index: usize) -> (String, f64, usize) {
    let score = nodes[position][index].score;
    let mut words: Vec<&str> = Vec::new();
    while position > 0 {
        let node = &nodes[position][index];
        words.push(&node.text);
        index = node.back;
        position = node.start;
    }
    words.reverse();
    let count = words.len();
    (words.concat(), score, count)
}

/// 只保留得分最高的束宽条部分路径。
fn prune(nodes: &mut Vec<Node>) {
    nodes.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    nodes.truncate(BEAM_WIDTH);
}

/// 一个词的得分：真实对数概率 → 低频扣分 → 多字用户词加成 → 口语常用度 → 搭配加分。
fn step_score(context: Option<&str>, word: &str, freq: u32, layer: u8) -> f64 {
    let mut score = f64::from(freq) / FREQ_LOG_SCALE - LN_RAW_TOTAL;
    if freq < LOW_FREQ_CUTOFF {
        score -= LOW_FREQ_PENALTY;
    }
    // 用户词加成只给多字词（见 USER_BONUS 注释）：单字用户词按词加分会让
    // 拆碎的路径每多切一刀就多吃一份加成。
    if layer == 2 && word.chars().count() >= 2 {
        score += USER_BONUS;
    }
    // 常用词的人工口语优先级：纠正书面语词频偏差（详见 COMMON_RANK_BONUS / COMMON_MULTI_BONUS）。
    if let Some(rank) = dictionary::common_rank(word) {
        score += COMMON_RANK_BONUS * (-f64::from(rank) / COMMON_RANK_DECAY).exp();
        if word.chars().count() >= 2 {
            score += COMMON_MULTI_BONUS;
        }
    }
    if let Some(previous) = context {
        let count = dictionary::bigram_score(previous, word);
        if count >= BIGRAM_MIN_COUNT {
            // 相对 1 档取增量：2 档 +0.49，6 档 +1.50 nats。
            score += BIGRAM_W * ((1.0 + f64::from(count)).ln() - 2f64.ln());
        }
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_distance_one_matches_substitution_insert_delete() {
        assert!(edit_distance_le_one("zhl", "zhe")); // 打错一位
        assert!(edit_distance_le_one("zhli", "zhi")); // 多打一位
        assert!(edit_distance_le_one("zh", "zhi")); // 漏打一位
        assert!(!edit_distance_le_one("zhli", "zhe"));
        assert!(!edit_distance_le_one("ni", "men"));
    }

    #[test]
    fn exact_segmentation_enumerates_ambiguous_splits() {
        // xian 既是单音节，也是 xi+an（西安）
        let segs = segment_exact("xian", 8);
        assert!(segs.iter().any(|s| s.len() == 1 && s[0].0 == "xian"));
        assert!(
            segs.iter()
                .any(|s| s.len() == 2 && s[0].0 == "xi" && s[1].0 == "an")
        );
    }

    #[test]
    fn exact_segmentation_rejects_broken_input() {
        // nim 读不成音节序列（m 不是音节）
        assert!(segment_exact("nimjige", 8).is_empty());
    }

    #[test]
    fn tolerant_segmentation_skips_stray_letter() {
        // 开头多打一个字母：跳过 j 后能完整读成 ni+hao
        let segs = segment_tolerant("jnihao", 8);
        assert!(!segs.is_empty(), "容错切分应能读通");
        assert!(
            segs.iter().any(|path| {
                path.iter().map(|(syl, _)| syl.as_str()).collect::<String>() == "nihao"
            }),
            "{segs:?}"
        );
    }

    /// `nimjige`：m 会和后面的 i 拼成 mi（切分层无法判断用户想打 men 还是 mi），
    /// 这里只断言「读不通的输入能被容错切分读出来」，不涉及选词正确性。
    #[test]
    fn tolerant_segmentation_reads_unbroken_input() {
        let segs = segment_tolerant("nimjige", 8);
        assert!(!segs.is_empty());
        assert!(
            segs.iter().any(|path| {
                let joined: String = path.iter().map(|(syl, _)| syl.as_str()).collect();
                joined == "nimige" || joined == "nijige"
            }),
            "{segs:?}"
        );
    }

    #[test]
    fn near_syllables_fixes_one_typo() {
        // zhli 若按 zhl 读，应能纠到 zhe（l→e 或 l→i 的合法音节）
        let near: Vec<String> = near_syllables("zhl").into_iter().map(|(s, _)| s).collect();
        assert!(near.iter().any(|s| s == "zhe" || s == "zhi"), "{near:?}");
    }

    #[test]
    fn decode_rejects_non_pinyin() {
        assert!(decode("hello world", None, 3).is_empty());
        assert!(decode("abc", None, 3).is_empty());
    }
}
