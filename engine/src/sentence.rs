//! 整句解码：把一串拼音读成最可能的词序列。
//!
//! 逐段贪心解决不了长句——某个词单独看词频高，不等于它在整句里成立
//! （「几个」后面接「认栽」也能拼成合法词序列）。这里在音节图上走 Viterbi +
//! 束搜索，比较的是「整句所有词得分之和」，不让探索顺序决定结果。
//!
//! 三个打分来源：词库词频（按词数折算成可比尺度）、相邻词搭配档位、层级权重。
//! 搭配表缺失或稀疏时退化为词频打分，不会因为缺数据让整句失效。

use std::collections::{HashMap, HashSet};

use crate::dictionary;

/// 一条整句最多多少音节。再长按粘贴处理，不猜。
const MAX_SYLLABLES: usize = 30;
/// 输入串字符上限。
const MAX_INPUT_CHARS: usize = 90;
/// 少于这个音节数交给词级候选，不猜整句。
pub const MIN_SYLLABLES: usize = 3;
/// 一个词最多覆盖几个音节。
const MAX_WORD_SYLLABLES: usize = 8;
/// 每个格子取几个词（默认值）。
const SPAN_TAKE_DEFAULT: usize = 8;
/// 束宽：每个位置保留几条部分路径（默认值）。
const BEAM_WIDTH_DEFAULT: usize = 12;

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(default)
}

/// 每个格子取几个词。`TS_SPAN_TAKE` 可覆盖（性能与质量的权衡实验用）。
fn span_take() -> usize {
    static V: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *V.get_or_init(|| env_usize("TS_SPAN_TAKE", SPAN_TAKE_DEFAULT))
}

/// 束宽。`TS_BEAM_WIDTH` 可覆盖。
fn beam_width() -> usize {
    static V: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *V.get_or_init(|| env_usize("TS_BEAM_WIDTH", BEAM_WIDTH_DEFAULT))
}
/// 一次编辑（多打 / 打错 / 漏打）的代价（自然对数尺度）。
///
/// 字符级图里一次编辑只影响它覆盖的那几个字母，粒度细；代价取 8——够高到
/// 「打错一个字仍能纠正」，够低到不会为了省一次编辑而重组输入。
const EDIT_COST: f64 = 8.0;
/// 系统词库 frequency 列的对数缩放系数：`system_dict.db` 的 `frequency`
/// 存储的是 `round(365.0 * ln(raw_count))`（实测 count=2→253、3→401、4→506、
/// 8→759、9→802，精确吻合），本身已是自然对数尺度。
/// 此前对它再取 `ln(freq + 1)` 等于双重对数，把 5 倍词频差压成 0.2，导致：
///   1. 同音字分不清高下（`是` 4962 vs `使` 4045 只差 0.20，被 bigram 翻转）；
///   2. 拆碎单字与成词的差距被压平（`组词` 401 输给 `组+次`、`遇到` 3120 只赢 `与+到` 0.32）。
///
/// 还原真实对数词频 `freq / FREQ_LOG_SCALE` 后，词频差与词数惩罚回到同一自然对数单位（nats）。
const FREQ_LOG_SCALE: f64 = 365.0;
/// 系统词库总原始词频的自然对数：`ln(sum(exp(freq / 365.0))) ≈ ln(6.21e7) = 17.944`。
///
/// Q1.1 起默认整句路径改为直接二元条件概率 `ln P(w | prev)` + 显式边缺失平滑回退：
/// 仅 OOV/无 LM 的字典一元概率兜底与旧打分回退（`sentence_legacy` / `TS_SENTENCE_LEGACY`）使用本常数，
/// 不再对已命中 LM bigram 的多词块额外重复扣除一元常数项。
const LN_RAW_TOTAL: f64 = 17.944;
/// 二元条件概率权重（Q1.1）：`bg_tab` 为 16 桶量化（步长 ≈1.07 nats），
/// 取 75% `ln P(w | prev)` + 25% `ln P(w)` 做 Jelinek-Mercer 方差平滑，同时借 256 桶 unigram 打破同桶并列。
const COND_BIGRAM_WEIGHT: f64 = 0.75;
/// 显式 Katz 退避缩放系数与自然对数下界（nats）：柔和阻尼并钳位极低桶位（最低可达 -13.69 nats），防止单条缺边打崩长句。
const BACKOFF_SCALE: f64 = 0.65;
const BACKOFF_MIN_NATS: f64 = -4.0;
/// 前词在 LM 中未收录（OOV）时的显式平滑退避代价（nats）。
const OOV_PREV_BACKOFF_NATS: f64 = -0.8;
/// 词缀退避命中的 Katz 退避缩放系数：保留完整整词 bigram 严格优于前后缀近似。
const AFFIX_BACKOFF_SCALE: f64 = 0.5;
/// `bigram.db` 共现（`count >= BIGRAM_MIN_COUNT`）在 TSLM 缺边时的退避软化系数。
const DB_RESCUE_BACKOFF_SCALE: f64 = 0.5;
/// LM 可用但句首（BOS）缺边时的轻度平滑回退代价（nats）。
const BOS_MISS_BACKOFF_NATS: f64 = -0.35;
/// LM 不可用（如 `TS_LM_OFF`）且 `bigram.db` 也缺边时的轻度平滑惩罚（nats）。
const NO_LM_MISS_BACKOFF_NATS: f64 = -0.5;

#[inline]
fn use_legacy_scoring() -> bool {
    cfg!(feature = "sentence_legacy") || std::env::var_os("TS_SENTENCE_LEGACY").is_some()
}
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
const COMMON_MULTI_BASE: f64 = 1.2;
const COMMON_MULTI_BONUS: f64 = 1.5;
const COMMON_MULTI_DECAY: f64 = 800.0;
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
/// 伪段（含元音的混拼字母串，如 `wm`、`tqi`、`zhli`）的最大长度。
/// 保留 4 字符上限，避免 `zhongg` 等未成词过渡前缀触发全量词库区间扫描拉高尾延迟。
const ABBREV_MAX_LEN: usize = 4;
/// 纯声母伪段（无 `a/e/i/o/u/v`）的最大长度（与 `dictionary::ABBREV_SEG_MAX_LEN` 对齐）。
/// 纯声母走 O(1) `pure_initial_span_candidates` 精确索引，不扫区间，可安全放宽至 8。
const ABBREV_PURE_INITIALS_MAX_LEN: usize = 8;
/// 纯声母词图通道边惩罚（nats）：单声母（3.0）略重于 2–8 字母多字简拼（1.5），鼓励成词。
const PURE_INIT_SINGLE_PENALTY: f64 = 3.0;
const PURE_INIT_MULTI_PENALTY: f64 = 1.5;
/// 高冲突同音字双轨束搜索参数（在/再、的/得/地、他/她/它、做/作/坐、那/哪）。
const CONFLICT_CLUSTER_COUNT: usize = 5;
const CONFLICT_RESERVE_SLOTS: usize = 2;
const CONFLICT_CLUSTER_CAP: usize = 6;
const CONFLICT_VARIANT_CAP: usize = 4;
const CONFLICT_SCORE_MARGIN: f64 = 6.5;
/// 伪段标记前缀。真音节是纯字母，加 `^` 区分，切分与 span 查询据此分流。
const ABBREV_MARK: char = '^';
/// 伪段候选的固定降权（nats）：简拼只给出声母/前缀，信息量远低于全拼，
/// 必须高到「有全拼解释时绝不让位」。实测 2.5 时 `renz`（伪段）被读成「认真」，
/// 抢掉「人+在」的正确切分；10 与「少切一个词」的常数惩罚同量级，边界刚好。
/// 伪段候选的固定降权（nats）：简拼只给出声母/前缀，信息量低于全拼。
///
/// 实测：音节图时代 2.5 不够（`renz` 被读成「认真」，抢掉「人+在」的切分），
/// 需要与「少切一个词」的常数惩罚（约 17.9）同量级才压得住；字符级图里每条边
/// 精确消耗自己的字母，不会跨段重组，4 就足够让有全拼解释的边优先。
const ABBREV_PENALTY: f64 = 4.0;

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
    /// 本条边是否无编辑/无混拼惩罚（精确拼音全覆盖）。
    clean_edge: bool,
}

/// 整句解码带元数据结果（供排序 v2 取得切分段数与覆盖率，不改解码算法）。
#[derive(Debug, Clone)]
pub struct SentenceEntry {
    pub text: String,
    pub score: f64,
    /// 整句内部拼接的词块数（用于 `segment_rank`，词级=1，整句>=2）。
    pub segment_count: u8,
    /// 是否全部由精确拼音边覆盖（无容错编辑/无混拼伪段惩罚）。
    pub clean_coverage: bool,
}

/// 整句解码入口：返回得分最高的前 `limit` 条整句（文本去重）。
///
/// `prev` 是前文已上屏的最后一个词，参与句首搭配打分。
/// 不能在拼音上读通的输入（含英文、数字）返回空，交给调用方的其他候选来源。
pub fn decode(input: &str, prev: Option<&str>, limit: usize) -> Vec<String> {
    decode_meta(input, prev, limit)
        .into_iter()
        .map(|e| e.text)
        .collect()
}

type SpanWordCache = HashMap<String, std::rc::Rc<Vec<(String, u32, u8)>>>;

/// 整句解码（带元数据版本）：与 [`decode`] 同算法、同排序、同去重，额外返回
/// 每条整句的内部块数与是否全精确覆盖。
pub fn decode_meta(input: &str, prev: Option<&str>, limit: usize) -> Vec<SentenceEntry> {
    if limit == 0 {
        return Vec::new();
    }
    let input = crate::pinyin::normalize_v(&input.to_lowercase());
    let min_len = if is_pure_initials(&input) {
        4
    } else {
        MIN_SYLLABLES * 2
    };
    if input.len() < min_len
        || input.len() > MAX_INPUT_CHARS
        || !input.bytes().all(|b| b.is_ascii_lowercase())
    {
        return Vec::new();
    }
    let mut cache: SpanWordCache = HashMap::new();
    let mut scored: Vec<SentenceEntry> = Vec::new();
    if std::env::var("TS_SENTENCE_LEGACY").is_ok() {
        // 旧路径：先切音节，再在音节图上走 Viterbi。保留一个版本用于对照回归，
        // 确认字符级图在所有既有用例上不退化后再删除。
        let segs = segmentations(&input);
        if segs.is_empty() {
            return Vec::new();
        }
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
    } else {
        // 音节数下限（旧路径在切分之后判断）：两三音节的输入交给词级候选，
        // 整句解出的「中/过」会插到「中国」前面。混拼输入（`wmzaizhe...`）
        // 根本切不满音节，不在这里拦。
        let exact = segment_exact(&input, MAX_SEGMENTATIONS);
        let exact_clean = !exact.is_empty();
        if exact_clean && exact.iter().map(|s| s.len()).min().unwrap_or(0) < MIN_SYLLABLES {
            return Vec::new();
        }
        scored = char_graph(&input, prev, exact_clean, &mut cache);
    }
    if scored.is_empty() {
        return Vec::new();
    }
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.text.cmp(&b.text))
    });
    let mut out: Vec<SentenceEntry> = Vec::new();
    for entry in &scored {
        if out.len() >= limit {
            break;
        }
        if entry.text.is_empty() || out.iter().any(|e| e.text == entry.text) {
            continue;
        }
        out.push(entry.clone());
    }
    if std::env::var("TS_SENTENCE_DEBUG").is_ok() {
        let out_texts: Vec<&str> = out.iter().map(|e| e.text.as_str()).collect();
        let top6: Vec<(&str, f64, u8, bool)> = scored
            .iter()
            .take(6)
            .map(|e| (e.text.as_str(), e.score, e.segment_count, e.clean_coverage))
            .collect();
        eprintln!("[decode] {input} -> {out_texts:?}  (scored 前 6: {top6:?})");
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
    let mut budget = tolerant_node_budget();
    collect_tolerant(input, 0, 0, 0, &mut path, &mut out, limit, &mut budget);
    out
}

/// 容错切分的递归节点预算（默认值）。
///
/// 「跳过字符」在每一层都是合法分支，而结果只在整串读通时才计入输出——
/// 走不到末尾的分支不会推进输出上限，没有预算就会按分支数指数展开
/// （实测 25 字符输入直接把按键线程卡死）。预算内穷尽，耗尽就放弃剩余分支。
///
/// 实测：预算 20,000 时「最后一个音节只打了半个」（`zhongg` / `zhongguor`）
/// 会跑满预算，单键 40ms；完整音节只要 0.1ms。降到 3,000 可把该场景压到个位数
/// 毫秒，代价是超长错拼串的纠错覆盖变浅。
const TOLERANT_NODE_BUDGET_DEFAULT: usize = 20_000;

/// 容错切分节点预算。`TS_TOLERANT_BUDGET` 可覆盖（性能与纠错覆盖的权衡）。
fn tolerant_node_budget() -> usize {
    static V: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *V.get_or_init(|| env_usize("TS_TOLERANT_BUDGET", TOLERANT_NODE_BUDGET_DEFAULT))
}

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
        let mut hits = near_syllables(seg);
        // 混拼段：整段不是合法音节时，再给一条「整体当简拼/混拼读」的边。
        // 仅 `TS_SENTENCE_LEGACY` 的音节图路径会走到这里（默认路径是 char_graph，
        // 不需要切分）。长度 2 起——单字母段无意义（`w` 读我/为/问 全都可以，
        // 只会放大分支）。必须先问词库：字母段穷举会产出大量无候选的死边，
        // 把 MAX_SEGMENTATIONS 的配额占满，真正的混拼切分反而进不来。
        if (2..=ABBREV_MAX_LEN).contains(&seg.len())
            && !crate::pinyin::is_valid_syllable(seg)
            && !dictionary::abbrev_span_candidates(seg, 1).is_empty()
        {
            let mut marked = String::with_capacity(seg.len() + 1);
            marked.push(ABBREV_MARK);
            marked.push_str(seg);
            let at = hits.iter().take_while(|(_, cost)| *cost == 0).count();
            hits.insert(at, (marked, 0));
        }
        spans.push((end, hits));
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
static PROF_NEAR_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PROF_NEAR_CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 纠错候选统计包装：`TS_SENTENCE_PROFILE=1` 时用于定位解码耗时。
fn near_syllables(seg: &str) -> Vec<(String, u8)> {
    let t = std::time::Instant::now();
    let r = near_syllables_inner(seg);
    PROF_NEAR_US.fetch_add(
        u64::try_from(t.elapsed().as_micros()).unwrap_or(u64::MAX),
        std::sync::atomic::Ordering::Relaxed,
    );
    PROF_NEAR_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    r
}

fn near_syllables_inner(seg: &str) -> Vec<(String, u8)> {
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
    cache: &mut SpanWordCache,
) -> Vec<SentenceEntry> {
    let n = syls.len();
    let mut nodes: Vec<Vec<Node>> = (0..=n).map(|_| Vec::new()).collect();
    nodes[0].push(Node {
        start: 0,
        text: String::new(),
        score: 0.0,
        back: 0,
        clean_edge: true,
    });
    let prev_lm = prev.and_then(crate::lm::prepare_context);
    for start in 0..n {
        if nodes[start].is_empty() {
            continue;
        }
        prune_at(&mut nodes, start);
        // 先取一份前驱快照：下面要写 nodes[end]，不能同时持有 nodes[start] 的借用
        let preds: Vec<(f64, String)> = nodes[start]
            .iter()
            .map(|node| (node.score, node.text.clone()))
            .collect();
        let pred_lms: Vec<Option<crate::lm::ContextKey>> = if start == 0 {
            vec![prev_lm]
        } else {
            preds
                .iter()
                .map(|(_, t)| crate::lm::prepare_context(t))
                .collect()
        };
        for end in start + 1..=n.min(start + MAX_WORD_SYLLABLES) {
            let span = &syls[start..end];
            let edits: u8 = span.iter().map(|(_, cost)| *cost).sum();
            let abbrev = span.iter().all(|(s, _)| s.starts_with(ABBREV_MARK));
            let has_abbrev = span.iter().any(|(s, _)| s.starts_with(ABBREV_MARK));
            // 伪段与真音节混在同一个 span 里没有意义：一段要么读成一个词的
            // 全拼音节（`zheli`），要么整体读成一个词的简拼/混拼串（`wm`、`zhli`）。
            // 混着走会让段匹配的语义失真，宁可少一条边。
            if has_abbrev && !abbrev {
                continue;
            }
            let key: String = span
                .iter()
                .map(|(s, _)| s.trim_start_matches(ABBREV_MARK))
                .collect();
            let cache_key = if abbrev {
                format!("{ABBREV_MARK}{key}")
            } else {
                key.clone()
            };
            if !cache.contains_key(&cache_key) {
                let take = span_take();
                let words = if abbrev {
                    dictionary::abbrev_span_candidates(&key, take)
                } else {
                    dictionary::span_candidates(&key, take)
                };
                cache.insert(cache_key.clone(), std::rc::Rc::new(words));
            }
            let Some(hits) = cache.get(&cache_key) else {
                continue;
            };
            let penalty =
                f64::from(edits) * EDIT_COST + if abbrev { ABBREV_PENALTY * 0.5 } else { 0.0 };
            for (word, freq, layer) in hits.iter() {
                // 用户词的拼音键可能是早期学进去的半截别名（实测 ta→他们、da→到、
                // e→哦）：字数和这段音节数对不上就不是这段的读音，跳过。
                // 只管用户层——系统词有「T恤」这类字母词，字数本来就不等于音节数。
                // 伪段的段数不等于字数，不做这项校验。
                if *layer == 2 && !abbrev && word.chars().count() != end - start {
                    continue;
                }
                push_top_two_preds(
                    &mut nodes[end],
                    start,
                    &preds,
                    &pred_lms,
                    prev,
                    Some(key.as_str()),
                    word,
                    *freq,
                    *layer,
                    penalty,
                    edits == 0 && !abbrev,
                    n >= 4,
                );
            }
        }
    }
    nodes[n].sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.text.cmp(&b.text))
            .then_with(|| a.start.cmp(&b.start))
            .then_with(|| a.back.cmp(&b.back))
    });
    if std::env::var("TS_SENTENCE_DEBUG").is_ok() {
        eprintln!("[sentence] n={n}");
        for (index, node) in nodes[n].iter().take(8).enumerate() {
            let (text, _, words, _) = backtrack(&nodes, n, index);
            eprintln!(
                "   {:>8.3} start={} word={} -> {text} ({words}词) [{}]",
                node.score,
                node.start,
                node.text,
                path_words(&nodes, n, index).join("|")
            );
        }
    }
    let mut out: Vec<SentenceEntry> = Vec::new();
    for index in 0..nodes[n].len() {
        if out.len() >= limit {
            break;
        }
        let (text, score, words, clean) = backtrack(&nodes, n, index);
        // 整段读成一个词交给词级候选，这里只出自造的整句
        if words < 2 {
            continue;
        }
        if !out.iter().any(|e| e.text == text) {
            out.push(SentenceEntry {
                text,
                score,
                segment_count: words.min(u8::MAX as usize) as u8,
                clean_coverage: clean,
            });
        }
    }
    out
}

/// 从一个末尾节点回溯整条路径，返回 (整句文本, 得分, 词数, 是否全精确覆盖)。
fn backtrack(
    nodes: &[Vec<Node>],
    mut position: usize,
    mut index: usize,
) -> (String, f64, usize, bool) {
    let score = nodes[position][index].score;
    let mut words: Vec<&str> = Vec::new();
    let mut clean = true;
    while position > 0 {
        let node = &nodes[position][index];
        words.push(&node.text);
        clean &= node.clean_edge;
        index = node.back;
        position = node.start;
    }
    words.reverse();
    let count = words.len();
    (words.concat(), score, count, clean)
}

/// 回溯出一条路径的分词序列（诊断用，只在 TS_SENTENCE_DEBUG 下调用）。
fn path_words(nodes: &[Vec<Node>], mut position: usize, mut index: usize) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    while position > 0 {
        let node = &nodes[position][index];
        words.push(node.text.clone());
        index = node.back;
        position = node.start;
    }
    words.reverse();
    words
}

/// 一条边最多覆盖多少个字母。
///
/// 不按「8 个音节 × 3 字母」给到 24：长边带来的词级长词在整句通道里收益很低
/// （词级候选已有），代价却是每个起点多试一倍的段，实测 zhongguorenmin 单键
/// 峰值 78ms→24 时接近 100ms。12 覆盖到 4 音节词（zhongguorenmin 的 3-4 音节），
/// 更长的词交给多边拼接。
const MAX_EDGE_CHARS: usize = 12;
/// 一次整句解码里允许发起多少次混拼查询。混拼查询要扫区间，长输入段多，
/// 不设上界时单键会明显变慢；正常中文长句的混拼段远少于这个数。
const ABBREV_QUERY_BUDGET_DEFAULT: usize = 256;

/// 混拼查询预算。`TS_ABBREV_BUDGET` 可覆盖。
fn abbrev_query_budget() -> usize {
    static V: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *V.get_or_init(|| env_usize("TS_ABBREV_BUDGET", ABBREV_QUERY_BUDGET_DEFAULT))
}

pub(crate) fn is_pure_initials(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|b| {
            b.is_ascii_lowercase() && !matches!(b, b'a' | b'e' | b'i' | b'o' | b'u' | b'v')
        })
}

fn seg_pure_initial_words(
    cache: &mut SpanWordCache,
    seg: &str,
) -> std::rc::Rc<Vec<(String, u32, u8)>> {
    let key = format!("#{seg}");
    if let Some(hit) = cache.get(&key) {
        return hit.clone();
    }
    let words = dictionary::pure_initial_span_candidates(seg, span_take());
    let words = std::rc::Rc::new(words);
    cache.insert(key, words.clone());
    words
}

/// 字符级格子解码：节点是输入串的位置，边覆盖一段连续字母。
///
/// 与音节图（[`viterbi`]）的区别：每条边必须**精确消耗** `[start, end)` 这段
/// 字母，候选词要能被这几个字母完整读通。音节图的漏洞是「切分先于查词」——
/// `zhel` 可以先被替换成 `zhen`，再继续消费后面的字母，解码器于是能把输入
/// 重组成任意高频词（实测「你们几个在这里」被解成「你们几个认真癌症里」）。
///
/// 唯一允许改写字母的是「纠错边」：整段读不成任何词时，用一个编辑距离 1 的
/// 音节替换它，代价 [`EDIT_COST`]。粒度只到这一段，不会连带影响相邻段。
fn char_graph(
    input: &str,
    prev: Option<&str>,
    exact_clean: bool,
    cache: &mut SpanWordCache,
) -> Vec<SentenceEntry> {
    let prof = std::env::var("TS_SENTENCE_PROFILE").is_ok();
    let prof_t0 = std::time::Instant::now();
    let prof_near0 = (
        PROF_NEAR_CALLS.load(std::sync::atomic::Ordering::Relaxed),
        PROF_NEAR_US.load(std::sync::atomic::Ordering::Relaxed),
    );
    let prof_prune0 = (
        PROF_PRUNE_CALLS.load(std::sync::atomic::Ordering::Relaxed),
        PROF_PRUNE_US.load(std::sync::atomic::Ordering::Relaxed),
        PROF_PRUNE_NODES.load(std::sync::atomic::Ordering::Relaxed),
    );
    let prof_push0 = (
        PROF_PUSH_CALLS.load(std::sync::atomic::Ordering::Relaxed),
        PROF_PUSH_US.load(std::sync::atomic::Ordering::Relaxed),
    );
    let n = input.len();
    let pure_initials = input.len() >= 4 && !exact_clean && is_pure_initials(input);
    let mut nodes: Vec<Vec<Node>> = (0..=n).map(|_| Vec::new()).collect();
    // 纠错候选按段缓存：near_syllables 会全量扫音节表，同一段在长输入里被
    // 多个起点重复尝试，不缓存会把单键拖到几十毫秒。
    let mut near_cache: HashMap<String, Vec<(String, u8)>> = HashMap::new();
    // 混拼查询预算：只按未命中缓存的新段扣减，确保长句与虚词简拼全覆盖。
    let mut abbrev_budget = abbrev_query_budget();
    nodes[0].push(Node {
        start: 0,
        text: String::new(),
        score: 0.0,
        back: 0,
        clean_edge: true,
    });
    let prev_lm = prev.and_then(crate::lm::prepare_context);
    for start in 0..n {
        if nodes[start].is_empty() {
            continue;
        }
        prune_at(&mut nodes, start);
        let preds: Vec<(f64, String)> = nodes[start]
            .iter()
            .map(|node| (node.score, node.text.clone()))
            .collect();
        let pred_lms: Vec<Option<crate::lm::ContextKey>> = if start == 0 {
            vec![prev_lm]
        } else {
            preds
                .iter()
                .map(|(_, t)| crate::lm::prepare_context(t))
                .collect()
        };
        if pure_initials {
            for end in start + 1..=n.min(start + ABBREV_PURE_INITIALS_MAX_LEN) {
                let seg = &input[start..end];
                let penalty = if seg.len() == 1 {
                    PURE_INIT_SINGLE_PENALTY
                } else {
                    PURE_INIT_MULTI_PENALTY
                };
                for (word, freq, layer) in seg_pure_initial_words(cache, seg).iter() {
                    push_top_two_preds(
                        &mut nodes[end],
                        start,
                        &preds,
                        &pred_lms,
                        prev,
                        Some(seg),
                        word,
                        *freq,
                        *layer,
                        penalty,
                        false,
                        n >= 11,
                    );
                }
            }
            continue;
        }
        for end in start + 1..=n.min(start + MAX_EDGE_CHARS) {
            let seg = &input[start..end];
            // 候选来源按信度递减：整段全拼 → 纯声母/混拼伪段 → 单段纠错。
            let mut sources: Vec<(String, u32, u8, f64, String)> = Vec::new();
            if !(seg.len() == 1 && !matches!(seg, "a" | "o" | "e")) {
                for (word, freq, layer) in seg_words(cache, seg, false).iter() {
                    sources.push((word.clone(), *freq, *layer, 0.0, seg.to_string()));
                }
            }
            if sources.is_empty() && !exact_clean {
                if is_pure_initials(seg) && (1..=ABBREV_PURE_INITIALS_MAX_LEN).contains(&seg.len())
                {
                    let penalty = if seg.len() == 1 {
                        ABBREV_PENALTY * 0.65
                    } else {
                        ABBREV_PENALTY * 0.5
                    };
                    for (word, freq, layer) in seg_pure_initial_words(cache, seg).iter() {
                        sources.push((word.clone(), *freq, *layer, penalty, seg.to_string()));
                    }
                } else if (1..=ABBREV_MAX_LEN).contains(&seg.len()) && abbrev_budget > 0 {
                    let allow = if seg.len() == 1 {
                        !matches!(seg, "a" | "o" | "e")
                    } else {
                        true
                    };
                    if allow {
                        let abbrev_key = format!("{ABBREV_MARK}{seg}");
                        let was_cached = cache.contains_key(&abbrev_key);
                        let mixed = seg_words(cache, seg, true);
                        if !was_cached && !mixed.is_empty() {
                            abbrev_budget -= 1;
                        }
                        let penalty = if seg.len() == 1 {
                            ABBREV_PENALTY * 0.65
                        } else {
                            ABBREV_PENALTY * 0.5
                        };
                        for (word, freq, layer) in mixed.iter() {
                            sources.push((word.clone(), *freq, *layer, penalty, seg.to_string()));
                        }
                    }
                }
            }
            if !pure_initials && sources.is_empty() && (2..=6).contains(&seg.len()) {
                // near_syllables 要全量扫音节表（几百个音节逐个算编辑距离），
                // 每个起点每段都算一次会把单键拖到几十毫秒；按段缓存结果。
                let near = near_cache
                    .entry(seg.to_string())
                    .or_insert_with(|| near_syllables(seg));
                for (syl, cost) in near.iter() {
                    if *cost == 0 {
                        continue;
                    }
                    for (word, freq, layer) in seg_words(cache, syl, false).iter() {
                        sources.push((word.clone(), *freq, *layer, EDIT_COST, syl.clone()));
                    }
                }
            }
            if sources.is_empty() {
                continue;
            }
            for (word, freq, layer, penalty, edge_seg) in sources {
                push_top_two_preds(
                    &mut nodes[end],
                    start,
                    &preds,
                    &pred_lms,
                    prev,
                    Some(edge_seg.as_str()),
                    &word,
                    freq,
                    layer,
                    penalty,
                    penalty == 0.0,
                    n >= 11,
                );
            }
        }
    }
    nodes[n].sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.text.cmp(&b.text))
            .then_with(|| a.start.cmp(&b.start))
            .then_with(|| a.back.cmp(&b.back))
    });
    if std::env::var("TS_SENTENCE_DEBUG").is_ok() {
        eprintln!("[graph] n={n} 节点 {}", nodes[n].len());
        for (index, node) in nodes[n].iter().take(6).enumerate() {
            eprintln!(
                "   {:>8.3} [{}]",
                node.score,
                path_words(&nodes, n, index).join("|")
            );
        }
    }
    if prof {
        eprintln!(
            "[profile] char_graph input={input} clean={exact_clean} elapsed={:?} near={}us/{} prune={}us/{}call/{}in",
            prof_t0.elapsed(),
            PROF_NEAR_US.load(std::sync::atomic::Ordering::Relaxed) - prof_near0.1,
            PROF_NEAR_CALLS.load(std::sync::atomic::Ordering::Relaxed) - prof_near0.0,
            PROF_PRUNE_US.load(std::sync::atomic::Ordering::Relaxed) - prof_prune0.1,
            PROF_PRUNE_CALLS.load(std::sync::atomic::Ordering::Relaxed) - prof_prune0.0,
            PROF_PRUNE_NODES.load(std::sync::atomic::Ordering::Relaxed) - prof_prune0.2,
        );
        eprintln!(
            "[profile]   push={}us/{}call  edge_in={}",
            PROF_PUSH_US.load(std::sync::atomic::Ordering::Relaxed) - prof_push0.1,
            PROF_PUSH_CALLS.load(std::sync::atomic::Ordering::Relaxed) - prof_push0.0,
            PROF_PRUNE_NODES.load(std::sync::atomic::Ordering::Relaxed) - prof_prune0.2,
        );
    }
    let mut out: Vec<SentenceEntry> = Vec::new();
    let min_words = if pure_initials && n >= 6 { 1 } else { 2 };
    for index in 0..nodes[n].len() {
        let (text, score, words, clean) = backtrack(&nodes, n, index);
        // 整段全拼只出一词时交给词级候选；6–8 字母纯声母词级往往未覆整词，允许 1 词直出。
        if words < min_words {
            continue;
        }
        if !out.iter().any(|e| e.text == text) {
            out.push(SentenceEntry {
                text,
                score,
                segment_count: words.min(u8::MAX as usize) as u8,
                clean_coverage: clean,
            });
        }
    }
    out
}

/// 取一段字母的候选词（带缓存）：`abbrev` 为真时走混拼段匹配。
///
/// 返回 `Rc` 而不是 `Vec`：同一段在一屏输入里会被反复取用，深拷贝 6 条候选
/// 是热路径上的主要开销（实测 zhongguorenmin 单键峰值从 78ms 降到 47ms）。
fn seg_words(
    cache: &mut SpanWordCache,
    seg: &str,
    abbrev: bool,
) -> std::rc::Rc<Vec<(String, u32, u8)>> {
    let key = if abbrev {
        format!("{ABBREV_MARK}{seg}")
    } else {
        seg.to_string()
    };
    if let Some(hit) = cache.get(&key) {
        return hit.clone();
    }
    let take = span_take();
    let words = if abbrev {
        dictionary::abbrev_span_candidates(seg, take)
    } else {
        dictionary::span_candidates(seg, take)
    };
    let words = std::rc::Rc::new(words);
    cache.insert(key, words.clone());
    words
}

#[inline]
fn conflict_cluster_of(word: &str) -> Option<(usize, usize)> {
    match word.chars().last()? {
        '在' => Some((0, 0)),
        '再' => Some((0, 1)),
        '的' => Some((1, 0)),
        '得' => Some((1, 1)),
        '地' => Some((1, 2)),
        '他' => Some((2, 0)),
        '她' => Some((2, 1)),
        '它' => Some((2, 2)),
        '做' => Some((3, 0)),
        '作' => Some((3, 1)),
        '坐' => Some((3, 2)),
        '那' => Some((4, 0)),
        '哪' => Some((4, 1)),
        _ => None,
    }
}

/// 位置 `pos` 的双轨束剪枝：按累计得分降序，对完整路径文本去重后截断至 [`BEAM_WIDTH`]。
/// 当高冲突同音簇（在/再、的/得/地、他/她/它、做/作/坐、那/哪）在同一步存在多种异字竞争时，
/// 主轨限制单簇/单异字配额，并从总束宽预留 2 槽保活被压出前排的合理异字分支，防止束塌缩。
static PROF_PRUNE_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PROF_PRUNE_CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PROF_PRUNE_NODES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn prune_at(nodes: &mut [Vec<Node>], pos: usize) {
    if pos == 0 || nodes[pos].len() <= 1 {
        return;
    }
    let prof_t = std::time::Instant::now();
    let prof_nodes_in = nodes[pos].len();
    let texts: Vec<String> = (0..nodes[pos].len())
        .map(|idx| backtrack(nodes, pos, idx).0)
        .collect();
    let cur = std::mem::take(&mut nodes[pos]);
    let mut tagged: Vec<(String, Node)> = texts.into_iter().zip(cur).collect();
    tagged.sort_by(|a, b| {
        b.1.score
            .partial_cmp(&a.1.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    let width = beam_width();
    let mut seen: HashSet<String> = HashSet::with_capacity(tagged.len().min(width * 3));
    let mut unique: Vec<(String, Node)> = Vec::with_capacity(tagged.len().min(width * 3));
    for (full_text, node) in tagged {
        if seen.insert(full_text.clone()) {
            unique.push((full_text, node));
        }
    }

    let mut variant_mask = [0u8; CONFLICT_CLUSTER_COUNT];
    let mut cluster_best = [f64::NEG_INFINITY; CONFLICT_CLUSTER_COUNT];
    for (_, node) in &unique {
        if let Some((cid, vidx)) = conflict_cluster_of(&node.text) {
            variant_mask[cid] |= 1 << vidx;
            if node.score > cluster_best[cid] {
                cluster_best[cid] = node.score;
            }
        }
    }
    let contested: [bool; CONFLICT_CLUSTER_COUNT] =
        std::array::from_fn(|cid| variant_mask[cid].count_ones() >= 2);
    let any_contested = contested.iter().any(|&c| c);

    let mut kept: Vec<Node> = if !any_contested || width < 4 || unique.len() <= width {
        unique.into_iter().take(width).map(|(_, n)| n).collect()
    } else {
        let primary_cap = width.saturating_sub(CONFLICT_RESERVE_SLOTS);
        let mut used = vec![false; unique.len()];
        let mut out: Vec<Node> = Vec::with_capacity(width);
        let mut cluster_total = [0usize; CONFLICT_CLUSTER_COUNT];
        let mut variant_count = [[0usize; 3]; CONFLICT_CLUSTER_COUNT];
        let mut kept_variants = [0u8; CONFLICT_CLUSTER_COUNT];

        // Track 1：主轨按得分降序入选，对存在异字竞争的簇施加单簇/单异字上限与分差约束。
        for (idx, (_, node)) in unique.iter().enumerate() {
            if out.len() >= primary_cap {
                break;
            }
            if let Some((cid, vidx)) = conflict_cluster_of(&node.text)
                && contested[cid]
            {
                if cluster_total[cid] >= CONFLICT_CLUSTER_CAP
                    || variant_count[cid][vidx] >= CONFLICT_VARIANT_CAP
                    || node.score < cluster_best[cid] - CONFLICT_SCORE_MARGIN
                {
                    continue;
                }
                cluster_total[cid] += 1;
                variant_count[cid][vidx] += 1;
                kept_variants[cid] |= 1 << vidx;
            }
            used[idx] = true;
            out.push(Node {
                start: node.start,
                text: node.text.clone(),
                score: node.score,
                back: node.back,
                clean_edge: node.clean_edge,
            });
        }

        // Track 2：保活预留轨（最多 CONFLICT_RESERVE_SLOTS 槽），挽救被高词频分支挤出前排的合理异字。
        let mut reserved = 0usize;
        for (idx, (_, node)) in unique.iter().enumerate() {
            if reserved >= CONFLICT_RESERVE_SLOTS || out.len() >= width {
                break;
            }
            if used[idx] {
                continue;
            }
            if let Some((cid, vidx)) = conflict_cluster_of(&node.text)
                && contested[cid]
                && (kept_variants[cid] & (1 << vidx)) == 0
                && node.score >= cluster_best[cid] - CONFLICT_SCORE_MARGIN
            {
                used[idx] = true;
                kept_variants[cid] |= 1 << vidx;
                reserved += 1;
                out.push(Node {
                    start: node.start,
                    text: node.text.clone(),
                    score: node.score,
                    back: node.back,
                    clean_edge: node.clean_edge,
                });
            }
        }

        // Track 3：若仍未满束宽，按得分降序从剩余去重节点补齐，确保零容量损失。
        for (idx, (_, node)) in unique.into_iter().enumerate() {
            if out.len() >= width {
                break;
            }
            if !used[idx] {
                out.push(node);
            }
        }
        out
    };
    kept.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.text.cmp(&b.text))
            .then_with(|| a.start.cmp(&b.start))
            .then_with(|| a.back.cmp(&b.back))
    });
    nodes[pos] = kept;
    PROF_PRUNE_US.fetch_add(
        u64::try_from(prof_t.elapsed().as_micros()).unwrap_or(u64::MAX),
        std::sync::atomic::Ordering::Relaxed,
    );
    PROF_PRUNE_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    PROF_PRUNE_NODES.fetch_add(
        u64::try_from(prof_nodes_in).unwrap_or(u64::MAX),
        std::sync::atomic::Ordering::Relaxed,
    );
}

static PROF_PUSH_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PROF_PUSH_CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 为每条候选边保留最优与次优两位独立前驱，避免高频无歧义边把前序束宽度压成 1。
#[allow(clippy::too_many_arguments)]
fn push_top_two_preds(
    dest: &mut Vec<Node>,
    start: usize,
    preds: &[(f64, String)],
    pred_lms: &[Option<crate::lm::ContextKey>],
    prev: Option<&str>,
    edge_seg: Option<&str>,
    word: &str,
    freq: u32,
    layer: u8,
    penalty: f64,
    clean_edge: bool,
    allow_second: bool,
) {
    let prof_t = std::time::Instant::now();
    let legacy = use_legacy_scoring();
    let (legacy_base, legacy_primary) = if legacy {
        word_base_score_legacy(edge_seg, word, freq, layer)
    } else {
        (0.0, false)
    };
    let (prior, word_lm) = if legacy {
        (None, None)
    } else {
        let (p, wk) = compute_word_prior_and_lm(edge_seg, word, freq, layer);
        (Some(p), wk)
    };
    let mut b1 = (f64::NEG_INFINITY, 0usize);
    let mut b2 = (f64::NEG_INFINITY, 0usize);
    let mut b3 = (f64::NEG_INFINITY, 0usize);
    // 单遍记录语法冲突簇（0: 在/再, 1: 的/得/地）每个异字的最佳前驱，避免长句次优同形体挤杀异字。
    let mut gram_best = [[(f64::NEG_INFINITY, 0usize); 3]; 2];
    let mut eval_preds = |bg_idx: Option<&dictionary::mapped::MappedIndex>| {
        for (index, (pred_score, pred_text)) in preds.iter().enumerate() {
            let context = if start == 0 {
                prev
            } else {
                Some(pred_text.as_str())
            };
            let ctx_lm = pred_lms.get(index).and_then(|k| k.as_ref());
            let edge_score = match (prior.as_ref(), bg_idx) {
                (Some(p), Some(bg)) => {
                    conditional_edge_score_fast(context, ctx_lm, word, word_lm.as_ref(), p, bg)
                }
                (Some(p), None) => p.uni_log_prob + p.extra_bonus,
                (None, _) => legacy_base + transition_score_legacy(context, word, legacy_primary),
            };
            let score = pred_score + edge_score - penalty;
            if start > 0
                && let Some((cid, vidx)) = conflict_cluster_of(pred_text)
                && cid <= 1
                && score > gram_best[cid][vidx].0
            {
                gram_best[cid][vidx] = (score, index);
            }
            if score > b1.0 {
                b3 = b2;
                b2 = b1;
                b1 = (score, index);
            } else if score > b2.0 {
                b3 = b2;
                b2 = (score, index);
            } else if score > b3.0 {
                b3 = (score, index);
            }
        }
    };
    if prior.as_ref().is_some_and(|p| p.primary) {
        dictionary::with_bigram_table(|bg| eval_preds(Some(bg)));
    } else {
        eval_preds(None);
    }
    let mut push_pred = |pred: (f64, usize)| {
        dest.push(Node {
            start,
            text: word.to_string(),
            score: pred.0,
            back: pred.1,
            clean_edge,
        });
    };
    if b1.0.is_finite() {
        push_pred(b1);
    }
    if allow_second {
        if b2.0.is_finite() {
            push_pred(b2);
        }
        if b3.0.is_finite() {
            push_pred(b3);
        }
        if b1.0.is_finite()
            && start > 0
            && let Some((c1, v1)) = conflict_cluster_of(&preds[b1.1].1)
            && c1 <= 1
        {
            let has_alt_in_top3 = [b2, b3].into_iter().any(|p| {
                p.0.is_finite()
                    && matches!(
                        conflict_cluster_of(&preds[p.1].1),
                        Some((c, v)) if c == c1 && v != v1
                    )
            });
            if !has_alt_in_top3 {
                let mut best_alt = (f64::NEG_INFINITY, 0usize);
                for (v, &(s, idx)) in gram_best[c1].iter().enumerate() {
                    if v != v1
                        && s.is_finite()
                        && s >= b1.0 - CONFLICT_SCORE_MARGIN
                        && s > best_alt.0
                    {
                        best_alt = (s, idx);
                    }
                }
                if best_alt.0.is_finite() {
                    push_pred(best_alt);
                }
            }
        }
    } else if b2.0.is_finite()
        && start > 0
        && matches!(
            (
                conflict_cluster_of(&preds[b1.1].1),
                conflict_cluster_of(&preds[b2.1].1),
            ),
            (Some((c1, v1)), Some((c2, v2))) if c1 <= 1 && c1 == c2 && v1 != v2
        )
    {
        push_pred(b2);
    }
    PROF_PUSH_US.fetch_add(
        u64::try_from(prof_t.elapsed().as_micros()).unwrap_or(u64::MAX),
        std::sync::atomic::Ordering::Relaxed,
    );
    PROF_PUSH_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

#[inline]
fn synthetic_two_char_lm_log10(word: &str) -> Option<f32> {
    let mut it = word.chars();
    let (Some(c1), Some(c2)) = (it.next(), it.next()) else {
        return None;
    };
    let mut b1 = [0u8; 4];
    let mut b2 = [0u8; 4];
    let u1 = crate::lm::unigram(c1.encode_utf8(&mut b1))?;
    let u2 = crate::lm::unigram(c2.encode_utf8(&mut b2))?;
    Some((u1 + u2 + 1.2).min(-3.8))
}

#[inline]
fn common_and_layer_bonus(
    char_len: usize,
    freq: u32,
    layer: u8,
    has_lm_u: bool,
    common_rank: Option<u32>,
) -> f64 {
    let mut bonus = 0.0;
    if freq < LOW_FREQ_CUTOFF && !has_lm_u {
        bonus -= LOW_FREQ_PENALTY;
    }
    if layer == 2 && char_len >= 2 {
        bonus += USER_BONUS;
    }
    if let Some(rank) = common_rank {
        bonus += COMMON_RANK_BONUS * (-f64::from(rank) / COMMON_RANK_DECAY).exp();
        if char_len >= 2 {
            bonus += COMMON_MULTI_BASE
                + COMMON_MULTI_BONUS * (-f64::from(rank) / COMMON_MULTI_DECAY).exp();
        }
    }
    bonus
}

#[derive(Clone, Copy)]
struct WordPrior {
    uni_log_prob: f64,
    extra_bonus: f64,
    primary: bool,
}

/// 预计算单条边的前文无关先验（Q1.1 尺度为真自然对数 `ln P(w)` + 用户/常用词加权）及 LM 句柄。
fn compute_word_prior_and_lm(
    seg: Option<&str>,
    word: &str,
    freq: u32,
    layer: u8,
) -> (WordPrior, Option<crate::lm::WordKey>) {
    let sys_uni = f64::from(freq) / FREQ_LOG_SCALE - LN_RAW_TOTAL;
    let char_len = word.chars().count();
    let (primary, common_rank) = match seg {
        Some(s) => dictionary::word_hints(s, word, freq),
        None => (true, dictionary::common_rank(word)),
    };
    let word_lm = if primary {
        crate::lm::prepare_word(word)
    } else {
        None
    };
    let lm_u = word_lm.as_ref().and_then(crate::lm::word_unigram);
    let uni_log_prob = match lm_u {
        Some(u) => {
            let lm_uni = f64::from(u) * std::f64::consts::LN_10;
            if freq > 0 && layer == 0 {
                0.85 * lm_uni + 0.15 * sys_uni
            } else {
                lm_uni
            }
        }
        None if !primary => sys_uni - 2.0,
        None if char_len == 2 => match synthetic_two_char_lm_log10(word) {
            Some(comp_log10) => {
                let comp_uni = f64::from(comp_log10) * std::f64::consts::LN_10;
                0.5 * comp_uni + 0.5 * sys_uni
            }
            None => sys_uni,
        },
        None => sys_uni,
    };
    let extra_bonus = common_and_layer_bonus(char_len, freq, layer, lm_u.is_some(), common_rank);
    (
        WordPrior {
            uni_log_prob,
            extra_bonus,
            primary,
        },
        word_lm,
    )
}

#[inline]
fn blend_conditional_prob(bg_log10: f32, uni_log_prob: f64) -> f64 {
    let bg_nats = f64::from(bg_log10) * std::f64::consts::LN_10;
    COND_BIGRAM_WEIGHT * bg_nats + (1.0 - COND_BIGRAM_WEIGHT) * uni_log_prob
}

#[inline]
fn scale_katz_backoff(bo_log10: f32) -> f64 {
    (f64::from(bo_log10) * std::f64::consts::LN_10 * BACKOFF_SCALE).clamp(BACKOFF_MIN_NATS, 0.0)
}

/// 二元条件概率转移评分（Q1.1 + Q4.2 预解析句柄路径）：
/// - Tier 1：TSLM 显式命中 `ln P(word | context)`，直接以条件概率主导（不再叠加 `-LN_RAW_TOTAL`）；
/// - Tier 2：前后缀退避命中，叠加折半 Katz backoff + 词缀 lift；
/// - Tier 3：显式边缺失平滑回退：先看 `bigram.db` 共现（软化退避并给对数增益），否则走有界 Katz backoff / 缺省惩罚。
#[allow(clippy::too_many_arguments)]
fn conditional_edge_score_fast(
    context: Option<&str>,
    ctx_lm: Option<&crate::lm::ContextKey>,
    word: &str,
    word_lm: Option<&crate::lm::WordKey>,
    prior: &WordPrior,
    bg_idx: &dictionary::mapped::MappedIndex,
) -> f64 {
    if !prior.primary {
        return prior.uni_log_prob + prior.extra_bonus;
    }
    let Some(previous) = context else {
        if let Some(bos_bg) = word_lm.and_then(crate::lm::lookup_bos_bigram) {
            let bos_nats = f64::from(bos_bg) * std::f64::consts::LN_10;
            let delta = (0.5 * (bos_nats - prior.uni_log_prob)).clamp(-1.5, 2.5);
            return prior.uni_log_prob + delta + prior.extra_bonus;
        }
        let bos_bo = if word_lm.is_some() {
            crate::lm::backoff_bos()
                .map(|b| (f64::from(b) * std::f64::consts::LN_10).clamp(-1.5, 0.0))
                .unwrap_or(BOS_MISS_BACKOFF_NATS)
        } else {
            0.0
        };
        return prior.uni_log_prob + bos_bo + prior.extra_bonus;
    };

    let (lm_ready, edge_lm) = match (ctx_lm, word_lm) {
        (Some(ctx), Some(wk)) => (true, crate::lm::lookup_edge(ctx, wk)),
        _ => (false, None),
    };
    let bo_raw = match edge_lm {
        Some(crate::lm::EdgeLm::DirectProb(bg_log10)) => {
            return blend_conditional_prob(bg_log10, prior.uni_log_prob) + prior.extra_bonus;
        }
        Some(crate::lm::EdgeLm::AffixLift { bo, lift }) => {
            let bo_nats = bo.map(scale_katz_backoff).unwrap_or(OOV_PREV_BACKOFF_NATS);
            let lift_nats = (f64::from(lift) * std::f64::consts::LN_10).clamp(-3.0, 6.5);
            return prior.uni_log_prob
                + AFFIX_BACKOFF_SCALE * bo_nats
                + lift_nats
                + prior.extra_bonus;
        }
        Some(crate::lm::EdgeLm::Miss { bo }) => bo,
        None => None,
    };

    let bo_nats = if lm_ready {
        bo_raw
            .map(scale_katz_backoff)
            .unwrap_or(OOV_PREV_BACKOFF_NATS)
    } else {
        0.0
    };

    let count = dictionary::bigram_score_in(bg_idx, previous, word);
    if count >= BIGRAM_MIN_COUNT {
        let db_bonus = BIGRAM_W * ((1.0 + f64::from(count)).ln() - 2f64.ln());
        let eff_bo = if lm_ready {
            DB_RESCUE_BACKOFF_SCALE * bo_nats
        } else {
            0.0
        };
        return prior.uni_log_prob + eff_bo + db_bonus + prior.extra_bonus;
    }

    let eff_bo = if lm_ready {
        bo_nats
    } else {
        NO_LM_MISS_BACKOFF_NATS
    };
    prior.uni_log_prob + eff_bo + prior.extra_bonus
}

/// 旧版单词自身得分（`sentence_legacy` 编译开关与 `TS_SENTENCE_LEGACY` 对照回归用）。
fn word_base_score_legacy(seg: Option<&str>, word: &str, freq: u32, layer: u8) -> (f64, bool) {
    let sys_s = f64::from(freq) / FREQ_LOG_SCALE - LN_RAW_TOTAL;
    let char_len = word.chars().count();
    let (primary, common_rank) = match seg {
        Some(s) => dictionary::word_hints(s, word, freq),
        None => (true, dictionary::common_rank(word)),
    };
    let lm_u = if primary {
        crate::lm::unigram(word)
    } else {
        None
    };
    let mut score = match lm_u {
        Some(u) => {
            let lm_s = f64::from(u) * std::f64::consts::LN_10 - 1.2;
            if freq > 0 && layer == 0 {
                0.65 * lm_s + 0.35 * sys_s
            } else {
                0.8 * lm_s + 0.2 * sys_s
            }
        }
        None if !primary => sys_s - 2.0,
        None if char_len == 2 => match synthetic_two_char_lm_log10(word) {
            Some(comp_log10) => {
                let comp_s = f64::from(comp_log10) * std::f64::consts::LN_10 - 1.2;
                0.5 * comp_s + 0.5 * sys_s
            }
            None => sys_s,
        },
        None => sys_s,
    };
    score += common_and_layer_bonus(char_len, freq, layer, lm_u.is_some(), common_rank);
    (score, primary)
}

/// 旧版转移搭配得分（`sentence_legacy` 编译开关与 `TS_SENTENCE_LEGACY` 对照回归用）。
fn transition_score_legacy(context: Option<&str>, word: &str, primary: bool) -> f64 {
    if !primary {
        return 0.0;
    }
    match context {
        Some(previous) => {
            if let Some(lift) = crate::lm::bigram_lift(Some(previous), word) {
                (f64::from(lift) * std::f64::consts::LN_10).clamp(-3.5, 9.0)
            } else {
                let count = dictionary::bigram_score(previous, word);
                if count >= BIGRAM_MIN_COUNT {
                    BIGRAM_W * ((1.0 + f64::from(count)).ln() - 2f64.ln())
                } else {
                    0.0
                }
            }
        }
        None => {
            if let Some(lift) = crate::lm::direct_bigram_lift(None, word) {
                (f64::from(lift) * std::f64::consts::LN_10 * 0.5).clamp(-1.5, 2.5)
            } else {
                0.0
            }
        }
    }
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
        // 必须存在一条「不跳字、不编辑」的完整覆盖路径（剥掉伪段标记后等于输入）。
        // 编辑/跳字路径同样是合法输出，但完整覆盖的那条不能被伪段挤掉。
        assert!(
            segs.iter().any(|path| {
                let joined: String = path
                    .iter()
                    .map(|(syl, _)| syl.trim_start_matches(ABBREV_MARK))
                    .collect();
                joined == "nimjige"
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

    #[test]
    fn conditional_blend_avoids_ln_raw_total_constant() {
        let uni_log_prob = -8.0_f64;
        let blended = blend_conditional_prob(-2.0_f32, uni_log_prob);
        let expected = COND_BIGRAM_WEIGHT * (-2.0 * std::f64::consts::LN_10)
            + (1.0 - COND_BIGRAM_WEIGHT) * uni_log_prob;
        assert!((blended - expected).abs() < 1e-12);
        // 条件概率边本身处于真对数概率尺度，不再叠加 -LN_RAW_TOTAL(-17.944)
        assert!(blended > -10.0 && blended > uni_log_prob - LN_RAW_TOTAL + 5.0);
    }

    #[test]
    fn katz_backoff_scaling_is_bounded_and_monotonic() {
        let mild = scale_katz_backoff(-0.563);
        let steep = scale_katz_backoff(-5.945);
        assert!(mild.is_finite() && mild <= 0.0 && mild > BACKOFF_MIN_NATS);
        assert!((steep - BACKOFF_MIN_NATS).abs() < 1e-12);
        assert!(scale_katz_backoff(0.2).abs() < 1e-12);
        assert!(mild > steep);
    }

    #[test]
    fn prune_at_dual_track_preserves_de_homophones() {
        let mut nodes: Vec<Vec<Node>> = vec![
            vec![Node {
                start: 0,
                text: String::new(),
                score: 0.0,
                back: 0,
                clean_edge: true,
            }],
            Vec::new(),
            Vec::new(),
        ];
        for i in 0..12 {
            nodes[1].push(Node {
                start: 0,
                text: format!("前缀{i}"),
                score: -(i as f64) * 0.1,
                back: 0,
                clean_edge: true,
            });
            nodes[2].push(Node {
                start: 1,
                text: "的".to_string(),
                score: -10.0 - (i as f64) * 0.1,
                back: i,
                clean_edge: true,
            });
        }
        nodes[2].push(Node {
            start: 1,
            text: "得".to_string(),
            score: -12.0,
            back: 0,
            clean_edge: true,
        });
        nodes[2].push(Node {
            start: 1,
            text: "地".to_string(),
            score: -12.5,
            back: 1,
            clean_edge: true,
        });

        prune_at(&mut nodes, 2);
        assert_eq!(nodes[2].len(), BEAM_WIDTH_DEFAULT);
        let texts: Vec<&str> = nodes[2].iter().map(|n| n.text.as_str()).collect();
        assert!(texts.contains(&"的"), "{texts:?}");
        assert!(texts.contains(&"得"), "双轨束搜索应保活「得」: {texts:?}");
        assert!(texts.contains(&"地"), "双轨束搜索应保活「地」: {texts:?}");
    }

    #[test]
    fn prune_at_dual_track_preserves_zai_homophones_and_respects_margin() {
        let build = |zai2_score: f64| {
            let mut nodes: Vec<Vec<Node>> = vec![
                vec![Node {
                    start: 0,
                    text: String::new(),
                    score: 0.0,
                    back: 0,
                    clean_edge: true,
                }],
                Vec::new(),
                Vec::new(),
            ];
            for i in 0..12 {
                nodes[1].push(Node {
                    start: 0,
                    text: format!("他{i}"),
                    score: -(i as f64) * 0.1,
                    back: 0,
                    clean_edge: true,
                });
                nodes[2].push(Node {
                    start: 1,
                    text: "在".to_string(),
                    score: -10.0 - (i as f64) * 0.1,
                    back: i,
                    clean_edge: true,
                });
            }
            nodes[2].push(Node {
                start: 1,
                text: "再".to_string(),
                score: zai2_score,
                back: 0,
                clean_edge: true,
            });
            prune_at(&mut nodes, 2);
            nodes[2].iter().map(|n| n.text.clone()).collect::<Vec<_>>()
        };

        let within_margin = build(-13.0);
        assert!(
            within_margin.iter().any(|t| t == "再"),
            "分差在 MARGIN 内的「再」应被保留: {within_margin:?}"
        );
        let outside_margin = build(-20.0);
        assert!(
            !outside_margin.iter().any(|t| t == "再"),
            "超过 CONFLICT_SCORE_MARGIN 的低分「再」不得挤占束槽: {outside_margin:?}"
        );
    }

    #[test]
    fn prune_at_non_contested_keeps_pure_top_k() {
        let mut nodes: Vec<Vec<Node>> = vec![
            vec![Node {
                start: 0,
                text: String::new(),
                score: 0.0,
                back: 0,
                clean_edge: true,
            }],
            Vec::new(),
            Vec::new(),
        ];
        for i in 0..15 {
            nodes[1].push(Node {
                start: 0,
                text: format!("前{i}"),
                score: 0.0,
                back: 0,
                clean_edge: true,
            });
            nodes[2].push(Node {
                start: 1,
                text: format!("词{i}"),
                score: -10.0 - (i as f64),
                back: i,
                clean_edge: true,
            });
        }
        prune_at(&mut nodes, 2);
        assert_eq!(nodes[2].len(), BEAM_WIDTH_DEFAULT);
        assert_eq!(nodes[2][0].text, "词0");
        assert_eq!(nodes[2][BEAM_WIDTH_DEFAULT - 1].text, "词11");
    }

    #[test]
    fn pure_initials_predicate_covers_consonants_only() {
        assert!(is_pure_initials("wmmtqbj"));
        assert!(is_pure_initials("zgrmyh"));
        assert!(is_pure_initials("zhrmghg"));
        assert!(!is_pure_initials(""));
        assert!(!is_pure_initials("wmmingtian"));
        assert!(!is_pure_initials("a"));
        assert!(!is_pure_initials("lv"));
    }
}
