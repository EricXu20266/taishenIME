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
/// 一次编辑（多打 / 打错 / 漏打）的代价。
const EDIT_COST: f64 = 5.0;
/// 词库总词频的对数（ln(2.59e8)，按当前系统词库统计）。
/// 只影响每个词的常数偏移，词库规模变化时排序几乎不变。
const LN_TOTAL: f64 = 19.37;
/// 词数惩罚系数：标准一元模型是 1.0，但词库词频来自书面语料，
/// 对高频单字过度偏好会让「相去」压过「想 / 去」，取 0.6 折中。
const WORD_COUNT_BETA: f64 = 0.6;
/// 成词奖励：偏好成词，而不是把高频单字一路拆碎。
const WORD_BONUS: f64 = 2.0;
/// 低频词阈值与额外扣分：这类词条多是书面语组合和专名，口语长句里几乎不该出现。
const LOW_FREQ_CUTOFF: u32 = 300;
const LOW_FREQ_PENALTY: f64 = 1.5;
/// 常用词人工优先级的加分幅度与量程。常用词表行序就是口语常用度，
/// 系统词库词频来自维基书面语料——「想」（rank 22）的词频低于介词「向」
/// （rank 490），两者只差 0.018 nats，整句就被读成「我们向去吃饭」。
/// 按 rank 给小额加分（rank 0 → +0.3，rank ≥2048 → 0），不改动词频尺度。
const COMMON_RANK_BONUS: f64 = 0.3;
const COMMON_RANK_SCALE: f64 = 2048.0;
/// 搭配档位的权重（档位 1-6）。
const BIGRAM_W: f64 = 0.6;
/// 搭配档位的证据门槛。档位 1 = 只共现过一次，实测占 bigram.db 的 98.6%，
/// 它带来的 0.42 分足以翻转同音字之间不到 0.4 的词频差（「想曲」压过
/// 「想去」就是一例）。1 档视为无证据，2 档起按相对增量计分。
const BIGRAM_MIN_COUNT: u32 = 2;
/// 用户词层级加成（常用词不再额外加成：按词给的加成会让拆碎的单字
/// 路径吃双份——「吃 + 反」两个常用字各加一次，反而压过「吃饭」）。
const USER_BONUS: f64 = 1.0;
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

/// 一个词的得分：词频（按词数折算）→ 层级加成 → 低频扣分 → 搭配加分。
fn step_score(context: Option<&str>, word: &str, freq: u32, layer: u8) -> f64 {
    let mut score = (f64::from(freq) + 1.0).ln() - LN_TOTAL * WORD_COUNT_BETA + WORD_BONUS;
    if freq < LOW_FREQ_CUTOFF {
        score -= LOW_FREQ_PENALTY;
    }
    score += match layer {
        2 => USER_BONUS,
        _ => 0.0,
    };
    // 常用词的人工口语优先级：纠正书面语词频偏差（详见 COMMON_RANK_BONUS）。
    if let Some(rank) = dictionary::common_rank(word) {
        score += COMMON_RANK_BONUS * (1.0 - (f64::from(rank) / COMMON_RANK_SCALE).min(1.0));
    }
    if let Some(previous) = context {
        let count = dictionary::bigram_score(previous, word);
        if count >= BIGRAM_MIN_COUNT {
            // 相对 1 档取增量：2 档 0.24 分，6 档 0.75 分。
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
