//! 候选排序架构 v2（Phase A · 影子模式核心模块）
//!
//! 设计文档见 `docs/modules/candidate-ranking/DESIGN-V2-RANKING-REFACTOR.md`。
//! 本模块实现三层分离（Evidence → Reduction → Decision）中的纯计算部分：
//! - [`InputShape`]：输入形态特征，一次性构造，全链只读；
//! - [`Match`] / [`match_of`] / [`extent_of`]：由输入与候选的关系决定匹配层；
//! - [`Attestation`] / [`RankKey`]：单次全序排序键与逐维差异归因；
//! - [`RankDiff`]：影子模式新旧排序对比产物。

use crate::{Source, reason};

/// 匹配关系。全序，声明顺序即优先级（越小越优）。
/// 不带来源身份语义：由「输入形态」与「候选拼音/形态」的关系决定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Match {
    /// L1：精确匹配（去分隔拼音相等且字数等于主切分音节数；或全精确覆盖的整句）
    Exact = 0,
    /// L2：形态匹配（同串异切如 xian→西安，或非完整拼音的简拼/混拼精确模式如 zg→中国）
    Shape = 1,
    /// L3：前缀扩展/补全（输入是候选的严格前缀，如 hou→后面、niha→你好）
    Extension = 2,
    /// L4：截断容错（候选拼音是输入的严格前缀，如 zhongguoo→中国、nihaoa→你好）
    Truncation = 3,
    /// L5：变体推断（模糊音 / 邻键 / 拼写 / 漏打 / 错音）
    Inferred = 4,
    /// L6：多段拼凑（逐字硬拼 / 拆组 / 非全精确覆盖整句）
    Composed = 5,
    /// L7：外文 / 符号 / 兜底
    Foreign = 6,
}

impl Match {
    #[inline]
    pub const fn rank(self) -> u8 {
        self as u8
    }

    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Match::Exact => "L1:Exact",
            Match::Shape => "L2:Shape",
            Match::Extension => "L3:Extension",
            Match::Truncation => "L4:Truncation",
            Match::Inferred => "L5:Inferred",
            Match::Composed => "L6:Composed",
            Match::Foreign => "L7:Foreign",
        }
    }

    #[inline]
    pub const fn from_rank(v: u8) -> Option<Self> {
        match v {
            0 => Some(Match::Exact),
            1 => Some(Match::Shape),
            2 => Some(Match::Extension),
            3 => Some(Match::Truncation),
            4 => Some(Match::Inferred),
            5 => Some(Match::Composed),
            6 => Some(Match::Foreign),
            _ => None,
        }
    }
}

/// 候选形态。簇内排序用，不参与跨簇比较。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Form {
    #[default]
    Text = 0,
    Emoji = 1,
    Symbol = 2,
}

impl Form {
    #[inline]
    pub const fn rank(self) -> u8 {
        self as u8
    }

    #[inline]
    pub const fn from_rank(r: u8) -> Option<Self> {
        match r {
            0 => Some(Form::Text),
            1 => Some(Form::Emoji),
            2 => Some(Form::Symbol),
            _ => None,
        }
    }

    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Form::Text => "Text",
            Form::Emoji => "Emoji",
            Form::Symbol => "Symbol",
        }
    }
}

/// 显式意图子级（越小越优先，4 表示普通自然候选）。
pub mod explicit {
    pub const DATETIME: u8 = 0;
    pub const PHRASE: u8 = 1;
    pub const PINNED: u8 = 2;
    pub const PROMOTED: u8 = 3; // ENG_EXACT_TOP / 驼峰混输 / Mode / Voice
    pub const NATURAL: u8 = 4;
}

/// 输入形态特征：一次性构造，全部阶段只读。
#[derive(Debug, Clone)]
pub struct InputShape {
    pub raw: String,
    /// 小写且 jqxy 后 v→u 归一的串。
    pub norm: String,
    /// 能否整串切分为合法拼音音节。
    pub splits_clean: bool,
    /// 合法时取过滤后的最短切分；否则取贪心最长合法前缀切分。
    pub best_split: Vec<String>,
    /// 不可整串切分时的剩余后缀（合法时为空）。
    pub tail: String,
    /// 全部可行切分路径（已按隔音歧义规则过滤荒谬单字母切分）。
    pub all_splits: Vec<Vec<String>>,
    /// 各可行解释的目标汉字数（升序去重）。
    pub target_lens: Vec<usize>,
    /// 主目标字数（`target_lens[0]`，无目标时取 `best_split.len()`）。
    pub syllable_count: usize,
}

impl InputShape {
    /// 一次性构造输入形态特征。
    pub fn build(raw: &str) -> Self {
        let lower = raw.to_lowercase();
        let norm = crate::pinyin::normalize_v(&lower);
        let splits_clean = crate::pinyin::is_complete_pinyin(&norm);

        let all_splits = if splits_clean {
            filtered_complete_splits(&norm)
        } else {
            Vec::new()
        };

        let (best_split, tail) = if splits_clean {
            let shortest = all_splits
                .iter()
                .min_by_key(|p| p.len())
                .cloned()
                .unwrap_or_else(|| greedy_split(&norm).0);
            (shortest, String::new())
        } else {
            greedy_split(&norm)
        };

        let target_lens: Vec<usize> = if !all_splits.is_empty() {
            let mut counts: Vec<usize> = all_splits.iter().map(|p| p.len()).collect();
            counts.sort_unstable();
            counts.dedup();
            counts
        } else if let Some(n) = crate::mixed_syllable_count(&norm) {
            vec![n]
        } else if let Some(n) = crate::short_pinyin_word_len(&norm) {
            vec![n]
        } else if norm.len() == 1 && norm.bytes().all(|b| b.is_ascii_lowercase()) {
            vec![1]
        } else {
            Vec::new()
        };

        let syllable_count = target_lens.first().copied().unwrap_or(best_split.len());

        Self {
            raw: raw.to_string(),
            norm,
            splits_clean,
            best_split,
            tail,
            all_splits,
            target_lens,
            syllable_count,
        }
    }

    /// L2 判据辅助：候选字数是否命中可行目标字数集合。
    #[inline]
    pub fn accepts_len(&self, chars: usize) -> bool {
        self.target_lens.contains(&chars)
    }

    /// 切分相等：候选音节序列是否等于主切分。
    #[inline]
    pub fn split_eq(&self, cand_syls: &[String]) -> bool {
        self.best_split.as_slice() == cand_syls
    }

    /// 严格前缀：输入主切分是否是候选音节序列的严格前缀（L3）。
    #[inline]
    pub fn is_prefix_of(&self, cand_syls: &[String]) -> bool {
        !self.best_split.is_empty()
            && self.best_split.len() < cand_syls.len()
            && cand_syls.starts_with(&self.best_split)
    }

    /// 严格反前缀：候选音节序列是否是输入主切分的严格前缀（L4）。
    #[inline]
    pub fn starts_with(&self, cand_syls: &[String]) -> bool {
        !cand_syls.is_empty()
            && cand_syls.len() < self.best_split.len()
            && self.best_split.starts_with(cand_syls)
    }
}

/// 枚举并过滤完整切分（与 `lib.rs::complete_syllable_counts` 保持严格一致）。
fn filtered_complete_splits(norm: &str) -> Vec<Vec<String>> {
    let variants = crate::pinyin::syllable_split_variants(norm, 256);
    if variants.is_empty() {
        return Vec::new();
    }
    let input_is_single_syllable =
        crate::pinyin::split_first_syllable(norm).is_some_and(|(_, rest)| rest.is_empty());
    let filtered: Vec<Vec<String>> = variants
        .iter()
        .filter(|p| {
            let true_glide =
                input_is_single_syllable && p.len() == 2 && p[1] == "e" && p[0].chars().count() > 1;
            true_glide || p.len() < 2 || p.iter().all(|s| s.chars().count() > 1)
        })
        .cloned()
        .collect();
    if filtered.is_empty() {
        variants
    } else {
        filtered
    }
}

/// 贪心切分前导合法音节，返回 `(已切音节, 剩余尾部)`。
fn greedy_split(norm: &str) -> (Vec<String>, String) {
    let mut syls = Vec::new();
    let mut rest = norm;
    while !rest.is_empty() {
        match crate::pinyin::split_first_syllable(rest) {
            Some((s, rem)) => {
                syls.push(s.to_string());
                rest = rem;
            }
            None => break,
        }
    }
    (syls, rest.to_string())
}

/// 单字高频可信门槛（对数频尺度平移）：
/// `CREDIBLE_EXACT_FREQ = 1600` 按 2 字词校准；单字原始频度约比 2 字词高 1 个数量级，
/// 对应 `365 * ln(10) ≈ 840` 的自然对数平移（1600 + 840 ≈ 2500）。
/// 低于该门槛的非用户、非常用单字（如 瘊/垕/齁/郈 = 2336）不压过常用隔音词与前缀补全。
pub const CREDIBLE_SINGLE_CHAR_FREQ: u32 = 2500;

/// 多字精确词与前缀扩展的低频降权门槛（对数频尺度）：
/// 系统词库词频按 `round(365 * ln(raw))` 离散化，`freq <= 401` 聚集了 15 万余条长尾词，
/// `[402..=499]` 区间为空，下一离散档为 `506`。
/// 当当前输入已存在可信精确词（`exact_has_credible`）时，词频低于该门槛的非用户、非常用
/// 多字精确词与非高频前缀扩展（如 `kankan` 下的 `砍砍=401`、`侃侃訚訚=401`）降权至 tier 6，
/// 让位于常规前缀扩展（tier 3/5），同层内精确词（`extent=0`）仍严格优于扩展词（`extent>=1`），
/// 且整体仍高于模糊音与纠错（tier 7），不删除且翻页可达。
pub const LOW_FREQ_EXACT_WORD_THRESHOLD: u32 = 500;

/// `match_of` / `extent_of` 所需的只读候选切片视图。
#[derive(Debug, Clone, Copy)]
pub struct MatchInput<'a> {
    pub source: Source,
    pub reasons: u32,
    pub pinyin: Option<&'a str>,
    pub pinyin_len: usize,
    pub char_count: usize,
    pub total_chars: usize,
    pub clean_coverage: bool,
    /// 来自宽松音节前缀组合（`query_combo`），非简拼/混拼精确模式。
    pub is_loose_combo: bool,
}

/// 纯函数：根据输入形态与候选特征判定匹配关系（L1..L7）。
pub fn match_of(input: &InputShape, cand: &MatchInput<'_>) -> Match {
    // 整句解码：按全精确覆盖归 L1，否则归 L6
    if cand.source == Source::Sentence {
        return if cand.clean_coverage {
            Match::Exact
        } else {
            Match::Composed
        };
    }

    // 外文 / Emoji：外文兜底层（强英文置顶通过 explicit_rank 表达）
    if matches!(cand.source, Source::English | Source::Emoji) {
        return Match::Foreign;
    }

    // 显式非拼音来源（日期 / 短语 / 模式 / 语音）
    if matches!(
        cand.source,
        Source::Datetime | Source::Phrase | Source::Mode | Source::Voice
    ) {
        return Match::Exact;
    }

    // 前缀引导词（≥3 音节前 2 音节常用词，如 mamama→妈妈、fuhyuqi→符合）→ L4 Truncation
    if cand.reasons & reason::PREFIX_WORD != 0 {
        return Match::Truncation;
    }

    // 变体来源（模糊音 / 纠错）：必须先于 L2 判定，防止同字数变体被 L2 截胡
    if matches!(cand.source, Source::Fuzzy | Source::Correction) {
        if let Some(cp) = cand.pinyin {
            if !cp.is_empty() && cp.len() < input.norm.len() && input.norm.starts_with(cp) {
                return Match::Truncation;
            }
        }
        return Match::Inferred;
    }

    // 拼凑来源（逐字硬拼 / 拆组 / 宽松组合 query_combo）：必须先于 L2 判定
    if cand.source == Source::Combo
        && ((cand.reasons & reason::STITCHED != 0) || cand.is_loose_combo)
    {
        return Match::Composed;
    }

    // 前缀补全：输入停在音节边界，候选多一个音节
    if cand.source == Source::Completion {
        return Match::Extension;
    }

    // 带规范拼音的词库 / 简拼混拼候选
    if let Some(cp) = cand.pinyin {
        // L1 精确：去分隔拼音相等 且 候选字数 == 主切分音节数
        if cp == input.norm {
            // 纯字母/非汉字词库条（如 yyds）不压过汉字匹配
            if cand.char_count == 0 {
                return Match::Foreign;
            }
            if input.splits_clean && cand.char_count == input.best_split.len() {
                return Match::Exact;
            }
            // 同串异切（xian→西安、yuan→预案）→ L2 Shape
            if input.accepts_len(cand.char_count) {
                return Match::Shape;
            }
            // 非完整拼音（半音节串，如 ces 下的 3 字「测识别」）不满足目标字数时
            // 是「前缀扩展」，不得落回 Exact 抢占首屏。
            if !input.splits_clean {
                return Match::Extension;
            }
            return Match::Exact;
        }

        // 非完整拼音下的简拼/混拼词库精确模式（zg→中国、zhyang→张扬、shurf→输入法）
        if !input.splits_clean && cand.source == Source::Combo && input.accepts_len(cand.char_count)
        {
            return Match::Shape;
        }

        // L3 多说：输入是候选拼音的严格前缀
        if cp.len() > input.norm.len() && cp.starts_with(&input.norm) {
            if input.norm.len() == 1 && !input.splits_clean {
                if cand.char_count == 1 && cand.total_chars == 1 {
                    return Match::Shape;
                }
                return Match::Extension;
            }
            // 非完整拼音（半截音节，如 shij→时间、huar→华人）：命中目标字数归 L2 Shape
            if !input.splits_clean && input.accepts_len(cand.char_count) {
                return Match::Shape;
            }
            return Match::Extension;
        }

        // L4 少说：候选拼音是输入的严格前缀（覆盖多打元音如 zhongguoo→中国）
        if !cp.is_empty() && cp.len() < input.norm.len() && input.norm.starts_with(cp) {
            return Match::Truncation;
        }

        if cand.source == Source::Combo {
            return Match::Composed;
        }
        return Match::Foreign;
    }

    // 无完整拼音字符串时的结构化归类（前缀扩展段 / 简拼混拼段）
    if cand.source == Source::Dictionary && (cand.reasons & reason::PREFIX_EXT != 0) {
        // 非完整单字母声母（w→我）：仅纯单字优先归 Shape（排除 a/e/o 合法单音节与 up主 等混合串）
        if input.norm.len() == 1 && !input.splits_clean {
            if cand.char_count == 1 && cand.total_chars == 1 {
                return Match::Shape;
            }
            return Match::Extension;
        }
        // 非完整拼音（输入正在打末音节，如 shij→时间、key→可以、huar→华人）：
        // 命中目标字数的词是当前目标形态（L2 Shape），更长的词才是扩展（L3 Extension）
        if !input.splits_clean && input.accepts_len(cand.char_count) {
            return Match::Shape;
        }
        return Match::Extension;
    }

    if cand.source == Source::Dictionary && (cand.reasons & reason::EXACT_PINYIN != 0) {
        if cand.char_count == 0 {
            return Match::Foreign;
        }
        if input.splits_clean && cand.char_count == input.best_split.len() {
            return Match::Exact;
        }
        if input.accepts_len(cand.char_count) {
            return Match::Shape;
        }
        // 同上：非完整拼音下的超长历史别名归 Extension，不再冒充精确命中。
        if !input.splits_clean {
            return Match::Extension;
        }
        return Match::Exact;
    }

    if !input.splits_clean && cand.source == Source::Combo {
        if input.accepts_len(cand.char_count) {
            return Match::Shape;
        }
        return Match::Composed;
    }

    if cand.source == Source::Combo {
        return Match::Composed;
    }

    Match::Foreign
}

/// 计算 L3 / L4 层内的偏离量（越小越优；其他层恒为 0）。
pub fn extent_of(input: &InputShape, cand: &MatchInput<'_>, level: Match) -> u8 {
    match level {
        Match::Extension => {
            if cand.source == Source::Completion {
                return 1;
            }
            if let Some(cp) = cand.pinyin {
                if let Some(rest) = cp.strip_prefix(&input.norm) {
                    if !rest.is_empty() {
                        if !input.splits_clean {
                            return cand
                                .char_count
                                .saturating_sub(input.syllable_count)
                                .clamp(1, 50) as u8;
                        }
                        if crate::pinyin::is_complete_pinyin(rest) {
                            let extra_syls = greedy_split(rest).0.len();
                            return extra_syls.clamp(1, 50) as u8;
                        }
                        // 完整拼音下的非音节边界字符前缀（如 lian → liang），排在音节边界扩展之后
                        return (100usize.saturating_add(rest.len())).min(u8::MAX as usize) as u8;
                    }
                }
            }
            // 无完整拼音的前缀扩展段：
            // - 非完整拼音（shij / huar / w）：字数越接近 syllable_count 越优
            // - 完整拼音（lian）：字数未增加但拼音更长 = 字符前缀（两/量），压在真扩展后
            if !input.splits_clean || cand.char_count > input.syllable_count {
                cand.char_count
                    .saturating_sub(input.syllable_count)
                    .clamp(1, 50) as u8
            } else {
                let extra_bytes = cand.pinyin_len.saturating_sub(input.norm.len());
                (100usize.saturating_add(extra_bytes)).min(u8::MAX as usize) as u8
            }
        }
        Match::Truncation => {
            if let Some(cp) = cand.pinyin {
                let byte_diff = input.norm.len().saturating_sub(cp.len());
                let syl_diff = input.best_split.len().saturating_sub(cand.char_count);
                let d = if syl_diff > 0 { syl_diff } else { byte_diff };
                d.clamp(1, u8::MAX as usize) as u8
            } else {
                input
                    .syllable_count
                    .saturating_sub(cand.char_count)
                    .clamp(1, u8::MAX as usize) as u8
            }
        }
        Match::Composed => {
            if input.syllable_count > 0 && cand.char_count > 0 {
                cand.char_count
                    .abs_diff(input.syllable_count)
                    .min(u8::MAX as usize) as u8
            } else {
                0
            }
        }
        _ => 0,
    }
}

/// 排序键 12 元组（与设计稿 §4.1 严格一一对应）。
pub type RankKey = (
    u8,  // 0: explicit_rank
    u8,  // 1: demoted (压过 sort_mode 确保降权词绝不被拉回首屏)
    u8,  // 2: pref_rank
    u8,  // 3: rare_rank
    u8,  // 4: effective_match_tier
    u8,  // 5: extent_rank
    u8,  // 6: user_tier
    u8,  // 7: segment_count
    u8,  // 8: context_rank (u8::MAX - context_lift)
    u32, // 9: common_order
    u32, // 10: freq_rank (u32::MAX - freq)
    u8,  // 11: form_rank
);

/// 排序键不含末位 `form_rank` 的 11 元前缀（形态簇跨簇锚点键，§5.4）。
pub type RankKeyPrefix = (u8, u8, u8, u8, u8, u8, u8, u8, u8, u32, u32);

#[inline]
pub const fn rank_key_prefix(k: &RankKey) -> RankKeyPrefix {
    (k.0, k.1, k.2, k.3, k.4, k.5, k.6, k.7, k.8, k.9, k.10)
}

/// 纯函数单次簇感知排序（§5.3–5.4）：
/// - 同一簇（共享 `cluster_key`）由其文本本体（若存在，否则取簇内最小前缀键）决定整体位置；
/// - 簇内按 `form_rank`（Text < Emoji < Symbol）→ 变体自身频率降序 → 文本字典序排列；
/// - 满足公理 1（全序 / 输入打乱不变性）。
pub fn cluster_sort_order<'a, F>(len: usize, has_clusters: bool, item: F) -> Vec<usize>
where
    F: Fn(usize) -> (&'a str, &'a str, Form, RankKey),
{
    let mut order: Vec<usize> = (0..len).collect();
    if len <= 1 {
        return order;
    }
    if !has_clusters {
        let keys: Vec<RankKey> = (0..len).map(|i| item(i).3).collect();
        order.sort_by(|&a, &b| keys[a].cmp(&keys[b]).then_with(|| item(a).0.cmp(item(b).0)));
        return order;
    }
    // 为每个簇计算确定性锚点前缀键（优先取 Text 本体的最小前缀键，保证与输入顺序无关）
    let mut anchors: std::collections::HashMap<&'a str, (bool, RankKeyPrefix)> =
        std::collections::HashMap::with_capacity(len);
    let mut rows: Vec<(&'a str, &'a str, Form, RankKey)> = Vec::with_capacity(len);
    for i in 0..len {
        let row = item(i);
        let is_non_text = row.2 != Form::Text;
        let prefix = rank_key_prefix(&row.3);
        anchors
            .entry(row.1)
            .and_modify(|cur| {
                if (is_non_text, prefix) < *cur {
                    *cur = (is_non_text, prefix);
                }
            })
            .or_insert((is_non_text, prefix));
        rows.push(row);
    }
    order.sort_by(|&a, &b| {
        let (ta, ca, fa, ka) = rows[a];
        let (tb, cb, fb, kb) = rows[b];
        let pa = anchors[ca].1;
        let pb = anchors[cb].1;
        pa.cmp(&pb)
            .then_with(|| ca.cmp(cb))
            .then_with(|| fa.rank().cmp(&fb.rank()))
            .then_with(|| ka.10.cmp(&kb.10))
            .then_with(|| ta.cmp(tb))
    });
    order
}

/// 归约证据：Layer 2 一次性计算，Layer 3 只读。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attestation {
    pub match_level: Match,
    pub extent_rank: u8,
    /// 切分段数：词级恒 1，整句为内部拼接块数。
    pub segment_count: u8,
    pub char_count: usize,
    pub freq: Option<u32>,
    /// 0 = 热/温用户词，1 = 常用词，2 = 过期用户词，3 = 普通词
    pub user_tier: u8,
    /// 常用词人工行序（升序，非常用词为 None）。
    pub common_order: Option<u32>,
    pub is_user: bool,
    /// 是否享有用户优先提权（拥挤输入串下低频用户词不享有）。
    ///
    /// `is_user` 只表示「这个词出现在用户词库」，供诊断位标与生僻字豁免使用；
    /// 置顶/可信判定必须看 `user_boosted`，否则用户词库里 freq=1 的历史别名
    /// 会把系统高频词挤出首屏。
    pub user_boosted: bool,
    pub is_common: bool,
    pub has_rare: bool,
    /// 上下文提权分（0..=20，档位 1 归零）。
    pub context_lift: u8,
    /// 显式意图子级（见 `explicit::*`）。
    pub explicit_rank: u8,
    pub demoted: bool,
    /// 是否来自纠错或模糊音变体通道（用于在 `!exact_has_credible` 时决定是否允许高可信变体补位）。
    pub is_variant: bool,
    /// 变体候选是否来自变体键的前缀扩展（非变体精确命中，如 niha→niba→你帮[nibang]）；
    /// 前缀变体不得压过可信补全（tier 3，如 niha→你好），落在 tier 4 保留可见。
    pub is_variant_prefix: bool,
    /// 是否是前缀补全来源（单音节/多音节边界恰好 +1 音节）。
    pub is_completion: bool,
    /// 是否是非完整拼音下的「末尾声母截断」（如 heshenm→合身，不是手误，禁止抢占 3 字词）。
    pub is_trailing_initial_trunc: bool,
    /// 是否来自词库锚定拆组（`phrase_group_guess`），区别于逐字硬拼与宽松组合。
    pub is_phrase_group: bool,
    /// 是否来自前缀引导词（≥3 音节前 2 音节常用词，如 mamama→妈妈、bianyiqi→变异）。
    pub is_prefix_word: bool,
    /// 是否是整句拼接产物（不参与词级可信度判定）。
    pub is_sentence: bool,
}

impl Default for Attestation {
    fn default() -> Self {
        Self {
            match_level: Match::Foreign,
            extent_rank: 0,
            segment_count: 1,
            char_count: 0,
            freq: None,
            user_tier: 3,
            common_order: None,
            is_user: false,
            user_boosted: false,
            is_common: false,
            has_rare: false,
            context_lift: 0,
            explicit_rank: explicit::NATURAL,
            demoted: false,
            is_variant: false,
            is_variant_prefix: false,
            is_completion: false,
            is_trailing_initial_trunc: false,
            is_phrase_group: false,
            is_prefix_word: false,
            is_sentence: false,
        }
    }
}

impl Attestation {
    /// 可信中文词判定（B2）：非整句 且（提权用户词 ∪ 常用词 ∪ 精确键词频 >= CREDIBLE_EXACT_FREQ）。
    #[inline]
    pub fn credible(&self) -> bool {
        !self.is_sentence
            && (self.user_boosted
                || self.is_common
                || self.freq.is_some_and(|f| f >= crate::CREDIBLE_EXACT_FREQ))
    }

    /// 单字高频可信（用于区分常见单字与罕见单字 瘊/垕，不影响 B2 精确阻断）。
    #[inline]
    pub fn high_freq_single_char(&self) -> bool {
        self.char_count == 1 && self.freq.is_some_and(|f| f >= CREDIBLE_SINGLE_CHAR_FREQ)
    }

    /// B6 生僻字降级：含 Ext-A/B+ 生僻字且非用户/非常用/非显式意图。
    #[inline]
    pub fn rare_demoted(&self) -> bool {
        self.has_rare && !self.is_user && !self.is_common && self.explicit_rank == explicit::NATURAL
    }

    /// 单调、统一的有效匹配层（0..=9，越小越优）。
    #[inline]
    pub fn effective_match_tier(&self, exact_has_credible: bool) -> u8 {
        let cred = self.credible();
        match self.match_level {
            Match::Exact => {
                if self.is_sentence {
                    4
                } else if self.user_boosted || self.is_common || self.context_lift >= 2 {
                    0
                } else if self.char_count >= 2 && cred {
                    1
                } else if self.high_freq_single_char() {
                    2
                } else if exact_has_credible
                    && self.char_count >= 2
                    && self.freq.is_some_and(|f| f < LOW_FREQ_EXACT_WORD_THRESHOLD)
                {
                    6
                } else {
                    4
                }
            }
            Match::Shape => {
                if cred {
                    1
                } else {
                    5
                }
            }
            Match::Extension => {
                // 仅用户/常用前缀补全（如 hou→后面、niha→你好、zai→再见）升至 tier 3，
                // 避免非高常用系统词补全（如 ang→昂贵）反超精确单字（昂）。
                if self.is_completion && (self.is_user || self.is_common) {
                    3
                } else if exact_has_credible
                    && !self.is_user
                    && !self.is_common
                    && self.context_lift < 2
                    && self.freq.is_some_and(|f| f < LOW_FREQ_EXACT_WORD_THRESHOLD)
                {
                    6
                } else {
                    5
                }
            }
            Match::Truncation => {
                if !exact_has_credible
                    && self.is_variant
                    && cred
                    && self.char_count >= 2
                    && !self.is_trailing_initial_trunc
                {
                    0
                } else if self.is_prefix_word {
                    5
                } else if self.is_trailing_initial_trunc {
                    8
                } else {
                    6
                }
            }
            Match::Inferred => {
                if self.is_trailing_initial_trunc {
                    8
                } else if !exact_has_credible && self.is_variant && cred && self.char_count >= 2 {
                    if self.is_variant_prefix { 4 } else { 0 }
                } else {
                    7
                }
            }
            Match::Composed => {
                // 整句解码（含混拼/单编辑）与前 2 条字数对齐的词库锚定拆组（extent=0）
                // 均覆盖全串输入，处于 tier 5（extent=0），优先于 PREFIX_WORD（tier 5, extent=1）
                // 与非高常用 PREFIX_EXT（tier 5, extent>=1, user_tier=3）。
                if self.is_sentence || (self.is_phrase_group && self.extent_rank == 0) {
                    5
                } else {
                    8
                }
            }
            Match::Foreign => 9,
        }
    }

    /// 计算全序排序键（12 元组，纯函数，无 I/O）。
    #[inline]
    pub fn rank_key(&self, pref_rank: u8, form: Form, exact_has_credible: bool) -> RankKey {
        (
            self.explicit_rank,
            u8::from(self.demoted),
            pref_rank,
            u8::from(self.rare_demoted()),
            self.effective_match_tier(exact_has_credible),
            self.extent_rank,
            self.user_tier,
            self.segment_count,
            u8::MAX.saturating_sub(self.context_lift),
            self.common_order.unwrap_or(u32::MAX),
            u32::MAX.saturating_sub(self.freq.unwrap_or(0)),
            form.rank(),
        )
    }
}

/// 根据 `sort_mode`（0=默认, 1=单字优先, 2=长词优先）计算用户偏好位。
/// 英文候选恒排中文偏好组与非偏好组之后（返回 2），绝不借 `sort_mode` 反超中文候选。
#[inline]
pub fn pref_rank(sort_mode: u8, char_count: usize, is_english: bool) -> u8 {
    if sort_mode == 0 {
        return 0;
    }
    if is_english {
        return 2;
    }
    let single = char_count == 1;
    match sort_mode {
        1 => u8::from(!single),
        2 => u8::from(single),
        _ => 0,
    }
}

/// 比较两条候选的 `RankKey`，返回决定胜负的第一个维度归因（100% 机械可解释）。
pub fn explain_pair(
    winner_text: &str,
    winner_key: &RankKey,
    winner_match: Match,
    loser_text: &str,
    loser_key: &RankKey,
    loser_match: Match,
) -> String {
    if winner_key.0 != loser_key.0 {
        return format!(
            "explicit: {winner_text}({}) < {loser_text}({})",
            winner_key.0, loser_key.0
        );
    }
    if winner_key.1 != loser_key.1 {
        return format!(
            "demoted: {winner_text}({}) < {loser_text}({})",
            winner_key.1, loser_key.1
        );
    }
    if winner_key.2 != loser_key.2 {
        return format!(
            "pref: {winner_text}({}) < {loser_text}({})",
            winner_key.2, loser_key.2
        );
    }
    if winner_key.3 != loser_key.3 {
        return format!(
            "rare: {winner_text}({}) < {loser_text}({})",
            winner_key.3, loser_key.3
        );
    }
    if winner_key.4 != loser_key.4 {
        return format!(
            "match_tier: {winner_text}({}:{}) < {loser_text}({}:{})",
            winner_match.as_str(),
            winner_key.4,
            loser_match.as_str(),
            loser_key.4
        );
    }
    if winner_key.5 != loser_key.5 {
        return format!(
            "extent: {winner_text}({}) < {loser_text}({})",
            winner_key.5, loser_key.5
        );
    }
    if winner_key.6 != loser_key.6 {
        return format!(
            "user_tier: {winner_text}({}) < {loser_text}({})",
            winner_key.6, loser_key.6
        );
    }
    if winner_key.7 != loser_key.7 {
        return format!(
            "segment: {winner_text}({}) < {loser_text}({})",
            winner_key.7, loser_key.7
        );
    }
    if winner_key.8 != loser_key.8 {
        return format!(
            "context: {winner_text}(lift={}) > {loser_text}(lift={})",
            u8::MAX - winner_key.8,
            u8::MAX - loser_key.8
        );
    }
    if winner_key.9 != loser_key.9 {
        return format!(
            "common_order: {winner_text}({}) < {loser_text}({})",
            winner_key.9, loser_key.9
        );
    }
    if winner_key.10 != loser_key.10 {
        return format!(
            "freq: {winner_text}({}) > {loser_text}({})",
            u32::MAX - winner_key.10,
            u32::MAX - loser_key.10
        );
    }
    if winner_key.11 != loser_key.11 {
        return format!(
            "form: {winner_text}({}) < {loser_text}({})",
            winner_key.11, loser_key.11
        );
    }
    format!("tiebreak(text): {winner_text} < {loser_text}")
}

/// 影子模式产物：新旧两套顺序 + 逐条差异归因。
#[derive(Debug, Clone)]
pub struct RankDiff {
    pub input: String,
    pub old_order: Vec<String>,
    pub new_order: Vec<String>,
    /// `(词, 旧位次, 新位次)`（只列位次发生变化的条目，按新顺序排列）
    pub changed: Vec<(String, usize, usize)>,
    /// 每条差异的归因（如 `"match_tier: 你好(L1:Exact:0) < 利好(L5:Inferred:4)"`）
    pub explanation: Vec<String>,
}

impl RankDiff {
    /// 首位是否发生变化。
    #[inline]
    pub fn top1_changed(&self) -> bool {
        self.old_order.first() != self.new_order.first()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    // ── Step 1：InputShape::build 与三个比较方法（20+ 代表性输入）────────────

    #[test]
    fn test_input_shape_20_representative_inputs() {
        // 1. 普通双音节
        let sh = InputShape::build("nihao");
        assert!(sh.splits_clean);
        assert_eq!(sh.best_split, s(&["ni", "hao"]));
        assert!(sh.tail.is_empty());
        assert_eq!(sh.target_lens, vec![2]);
        assert!(sh.accepts_len(2));
        assert!(!sh.accepts_len(3));

        // 2. 隔音歧义 xian（先 vs 西安）
        let sh = InputShape::build("xian");
        assert!(sh.splits_clean);
        assert_eq!(sh.best_split, s(&["xian"]));
        assert_eq!(sh.target_lens, vec![1, 2]);
        assert!(sh.accepts_len(1));
        assert!(sh.accepts_len(2));

        // 3. 隔音歧义 yuan（元 vs 预案）
        let sh = InputShape::build("yuan");
        assert!(sh.splits_clean);
        assert_eq!(sh.best_split, s(&["yuan"]));
        assert_eq!(sh.target_lens, vec![1, 2]);

        // 4. 隔音歧义 tian（天 vs 提案）
        let sh = InputShape::build("tian");
        assert!(sh.splits_clean);
        assert_eq!(sh.target_lens, vec![1, 2]);

        // 5. 真滑音末段 e：yue（月 / 余额）
        let sh = InputShape::build("yue");
        assert!(sh.splits_clean);
        assert_eq!(sh.target_lens, vec![1, 2]);

        // 6. 非真滑音单字母过滤：zhongguo 过滤 zhong+gu+o，只留 [2]
        let sh = InputShape::build("zhongguo");
        assert!(sh.splits_clean);
        assert_eq!(sh.best_split, s(&["zhong", "guo"]));
        assert_eq!(sh.target_lens, vec![2]);

        // 7. 同音歧义双切分等长：fangan（fang+an / fan+gan）
        let sh = InputShape::build("fangan");
        assert!(sh.splits_clean);
        assert_eq!(sh.target_lens, vec![2]);

        // 8. v→u 归一：qv → qu
        let sh = InputShape::build("qv");
        assert_eq!(sh.norm, "qu");
        assert!(sh.splits_clean);
        assert_eq!(sh.best_split, s(&["qu"]));
        assert_eq!(sh.target_lens, vec![1]);

        // 9. 长整句：zhongguorenminyinhang（6 音节）
        let sh = InputShape::build("zhongguorenminyinhang");
        assert!(sh.splits_clean);
        assert_eq!(sh.best_split.len(), 6);
        assert_eq!(sh.target_lens, vec![6]);

        // 10. 单音节：hou
        let sh = InputShape::build("hou");
        assert!(sh.splits_clean);
        assert_eq!(sh.best_split, s(&["hou"]));
        assert_eq!(sh.target_lens, vec![1]);

        // 11. 单字母声母：w
        let sh = InputShape::build("w");
        assert!(!sh.splits_clean);
        assert!(sh.best_split.is_empty());
        assert_eq!(sh.tail, "w");
        assert_eq!(sh.target_lens, vec![1]);

        // 12. 单字母元音：a
        let sh = InputShape::build("a");
        assert!(sh.splits_clean);
        assert_eq!(sh.best_split, s(&["a"]));
        assert_eq!(sh.target_lens, vec![1]);

        // 13. 纯声母简拼：zg
        let sh = InputShape::build("zg");
        assert!(!sh.splits_clean);
        assert_eq!(sh.target_lens, vec![2]);

        // 14. 完整声母简拼：shh（sh+h）
        let sh = InputShape::build("shh");
        assert!(!sh.splits_clean);
        assert_eq!(sh.target_lens, vec![2]);

        // 15. 声母+全拼混拼：zhyang（zh+yang）
        let sh = InputShape::build("zhyang");
        assert!(!sh.splits_clean);
        assert_eq!(sh.target_lens, vec![2]);

        // 16. 全拼+声母混拼：shurf（shu+r+f）
        let sh = InputShape::build("shurf");
        assert!(!sh.splits_clean);
        assert_eq!(sh.best_split, s(&["shu"]));
        assert_eq!(sh.tail, "rf");
        assert_eq!(sh.target_lens, vec![3]);

        // 17. 手误末尾多余辅音：nihap（ni+ha+p）
        let sh = InputShape::build("nihap");
        assert!(!sh.splits_clean);
        assert_eq!(sh.best_split, s(&["ni", "ha"]));
        assert_eq!(sh.tail, "p");

        // 18. 手误末尾多余元音：zhongguoo（o 是合法音节，splits_clean=true）
        let sh = InputShape::build("zhongguoo");
        assert!(sh.splits_clean);
        assert!(sh.tail.is_empty());

        // 19. 英文单词：hello（he+llo）
        let sh = InputShape::build("hello");
        assert!(!sh.splits_clean);
        assert_eq!(sh.best_split, s(&["he"]));
        assert_eq!(sh.tail, "llo");

        // 20. 英文单词：api（a+pi 是合法音节，需靠英文词典判定）
        let sh = InputShape::build("api");
        assert!(sh.splits_clean);
        assert_eq!(sh.best_split, s(&["a", "pi"]));

        // 21. 大小写混合：NiHao
        let sh = InputShape::build("NiHao");
        assert_eq!(sh.norm, "nihao");
        assert!(sh.splits_clean);

        // 22. 三个切分比较方法
        let sh = InputShape::build("nihao");
        assert!(sh.split_eq(&s(&["ni", "hao"])));
        assert!(!sh.split_eq(&s(&["ni"])));
        assert!(sh.is_prefix_of(&s(&["ni", "hao", "ma"])));
        assert!(!sh.is_prefix_of(&s(&["ni", "hao"])));
        assert!(sh.starts_with(&s(&["ni"])));
        assert!(!sh.starts_with(&s(&["ni", "hao"])));
    }

    // ── Step 2：Match + match_of（每层至少 2 正例 + 1 反例）───────────────────

    #[test]
    fn test_match_of_all_levels_positive_and_negative() {
        // L1 Exact 正例 1：nihao → 你好
        let sh_nihao = InputShape::build("nihao");
        assert_eq!(
            match_of(
                &sh_nihao,
                &MatchInput {
                    source: Source::Dictionary,
                    reasons: reason::EXACT_PINYIN,
                    pinyin: Some("nihao"),
                    pinyin_len: 5,
                    char_count: 2,
                    total_chars: 2,
                    clean_coverage: false,
                    is_loose_combo: false,
                }
            ),
            Match::Exact
        );

        // L1 Exact 正例 2：xian → 先（单字匹配主切分 len=1）
        let sh_xian = InputShape::build("xian");
        assert_eq!(
            match_of(
                &sh_xian,
                &MatchInput {
                    source: Source::Dictionary,
                    reasons: reason::EXACT_PINYIN,
                    pinyin: Some("xian"),
                    pinyin_len: 4,
                    char_count: 1,
                    total_chars: 1,
                    clean_coverage: false,
                    is_loose_combo: false,
                }
            ),
            Match::Exact
        );

        // L1 Exact 正例 3：全覆盖整句 → Exact
        let sh_sent = InputShape::build("zhongguorenminyinhang");
        assert_eq!(
            match_of(
                &sh_sent,
                &MatchInput {
                    source: Source::Sentence,
                    reasons: 0,
                    pinyin: None,
                    pinyin_len: 0,
                    char_count: 6,
                    total_chars: 6,
                    clean_coverage: true,
                    is_loose_combo: false,
                }
            ),
            Match::Exact
        );

        // L1 反例 / L2 Shape 正例 1：xian → 西安（同拼音但 2 字 != best_split.len()=1）
        assert_eq!(
            match_of(
                &sh_xian,
                &MatchInput {
                    source: Source::Dictionary,
                    reasons: reason::EXACT_PINYIN,
                    pinyin: Some("xian"),
                    pinyin_len: 4,
                    char_count: 2,
                    total_chars: 2,
                    clean_coverage: false,
                    is_loose_combo: false,
                }
            ),
            Match::Shape
        );

        // L2 Shape 正例 2：zg → 中国（简拼 2 字命中 target_lens=[2]）
        let sh_zg = InputShape::build("zg");
        assert_eq!(
            match_of(
                &sh_zg,
                &MatchInput {
                    source: Source::Combo,
                    reasons: 0,
                    pinyin: None,
                    pinyin_len: 0,
                    char_count: 2,
                    total_chars: 2,
                    clean_coverage: false,
                    is_loose_combo: false,
                }
            ),
            Match::Shape
        );

        // L2 反例：nihao 下的 Fuzzy「利好」(2 字)绝不能被 L2 截胡，必须落 L5
        assert_eq!(
            match_of(
                &sh_nihao,
                &MatchInput {
                    source: Source::Fuzzy,
                    reasons: 0,
                    pinyin: Some("lihao"),
                    pinyin_len: 5,
                    char_count: 2,
                    total_chars: 2,
                    clean_coverage: false,
                    is_loose_combo: false,
                }
            ),
            Match::Inferred
        );

        // L3 Extension 正例 1：hou → 后面（Completion）
        let sh_hou = InputShape::build("hou");
        let m_ext = match_of(
            &sh_hou,
            &MatchInput {
                source: Source::Completion,
                reasons: reason::COMPLETION,
                pinyin: Some("houmian"),
                pinyin_len: 7,
                char_count: 2,
                total_chars: 2,
                clean_coverage: false,
                is_loose_combo: false,
            },
        );
        assert_eq!(m_ext, Match::Extension);

        // L3 Extension 正例 2：niha → 你好（带完整拼音前缀）
        let sh_niha = InputShape::build("niha");
        assert_eq!(
            match_of(
                &sh_niha,
                &MatchInput {
                    source: Source::Dictionary,
                    reasons: reason::PREFIX_EXT,
                    pinyin: Some("nihao"),
                    pinyin_len: 5,
                    char_count: 2,
                    total_chars: 2,
                    clean_coverage: false,
                    is_loose_combo: false,
                }
            ),
            Match::Extension
        );

        // L4 Truncation 正例 1：zhongguoo → 中国（末尾多打元音 o，splits_clean=true 仍正确命中 L4！）
        let sh_zgoo = InputShape::build("zhongguoo");
        assert_eq!(
            match_of(
                &sh_zgoo,
                &MatchInput {
                    source: Source::Correction,
                    reasons: 0,
                    pinyin: Some("zhongguo"),
                    pinyin_len: 8,
                    char_count: 2,
                    total_chars: 2,
                    clean_coverage: false,
                    is_loose_combo: false,
                }
            ),
            Match::Truncation
        );

        // L4 Truncation 正例 2：nihaoa → 你好
        let sh_nihaoa = InputShape::build("nihaoa");
        assert_eq!(
            match_of(
                &sh_nihaoa,
                &MatchInput {
                    source: Source::Correction,
                    reasons: 0,
                    pinyin: Some("nihao"),
                    pinyin_len: 5,
                    char_count: 2,
                    total_chars: 2,
                    clean_coverage: false,
                    is_loose_combo: false,
                }
            ),
            Match::Truncation
        );

        // L5 Inferred 正例 1：suanfa → 断法（邻键纠错）
        let sh_suanfa = InputShape::build("suanfa");
        assert_eq!(
            match_of(
                &sh_suanfa,
                &MatchInput {
                    source: Source::Correction,
                    reasons: 0,
                    pinyin: Some("duanfa"),
                    pinyin_len: 6,
                    char_count: 2,
                    total_chars: 2,
                    clean_coverage: false,
                    is_loose_combo: false,
                }
            ),
            Match::Inferred
        );

        // L6 Composed 正例 1：STITCHED 拼凑词（即使字数对也必须落 L6）
        assert_eq!(
            match_of(
                &sh_zg,
                &MatchInput {
                    source: Source::Combo,
                    reasons: reason::STITCHED,
                    pinyin: None,
                    pinyin_len: 0,
                    char_count: 2,
                    total_chars: 2,
                    clean_coverage: false,
                    is_loose_combo: false,
                }
            ),
            Match::Composed
        );

        // L6 Composed 正例 2：带编辑回退的整句（clean_coverage = false）
        assert_eq!(
            match_of(
                &sh_sent,
                &MatchInput {
                    source: Source::Sentence,
                    reasons: 0,
                    pinyin: None,
                    pinyin_len: 0,
                    char_count: 6,
                    total_chars: 6,
                    clean_coverage: false,
                    is_loose_combo: false,
                }
            ),
            Match::Composed
        );

        // L7 Foreign 正例 1 & 2：English / Emoji
        assert_eq!(
            match_of(
                &sh_nihao,
                &MatchInput {
                    source: Source::English,
                    reasons: 0,
                    pinyin: None,
                    pinyin_len: 0,
                    char_count: 0,
                    total_chars: 5,
                    clean_coverage: false,
                    is_loose_combo: false,
                }
            ),
            Match::Foreign
        );
        assert_eq!(
            match_of(
                &sh_nihao,
                &MatchInput {
                    source: Source::Emoji,
                    reasons: 0,
                    pinyin: None,
                    pinyin_len: 0,
                    char_count: 2,
                    total_chars: 3,
                    clean_coverage: false,
                    is_loose_combo: false,
                }
            ),
            Match::Foreign
        );
    }

    // ── 公理 1（全序性）：打乱候选数组重跑排序，结果严格一致 ──────────────────

    #[test]
    fn test_axiom1_total_order_shuffle_invariant() {
        let items: Vec<(&str, Attestation, Form)> = vec![
            (
                "你好",
                Attestation {
                    match_level: Match::Exact,
                    char_count: 2,
                    freq: Some(5000),
                    user_tier: 1,
                    common_order: Some(10),
                    is_common: true,
                    ..Default::default()
                },
                Form::Text,
            ),
            (
                "你号",
                Attestation {
                    match_level: Match::Exact,
                    char_count: 2,
                    freq: Some(300),
                    user_tier: 3,
                    ..Default::default()
                },
                Form::Text,
            ),
            (
                "利好",
                Attestation {
                    match_level: Match::Inferred,
                    char_count: 2,
                    freq: Some(4000),
                    user_tier: 1,
                    common_order: Some(80),
                    is_common: true,
                    ..Default::default()
                },
                Form::Text,
            ),
            (
                "你好世界",
                Attestation {
                    match_level: Match::Extension,
                    extent_rank: 2,
                    char_count: 4,
                    freq: Some(2000),
                    user_tier: 3,
                    ..Default::default()
                },
                Form::Text,
            ),
            (
                "你好吗",
                Attestation {
                    match_level: Match::Extension,
                    extent_rank: 1,
                    char_count: 3,
                    freq: Some(1500),
                    user_tier: 1,
                    common_order: Some(50),
                    is_common: true,
                    ..Default::default()
                },
                Form::Text,
            ),
            (
                "nihao",
                Attestation {
                    match_level: Match::Foreign,
                    char_count: 0,
                    ..Default::default()
                },
                Form::Text,
            ),
            (
                "拟好",
                Attestation {
                    match_level: Match::Exact,
                    char_count: 2,
                    freq: Some(300),
                    user_tier: 3,
                    ..Default::default()
                },
                Form::Text,
            ),
        ];

        let sort_all = |mut list: Vec<(&'static str, Attestation, Form)>| -> Vec<&'static str> {
            list.sort_by(|a, b| {
                a.1.rank_key(0, a.2, true)
                    .cmp(&b.1.rank_key(0, b.2, true))
                    .then_with(|| a.0.cmp(b.0))
            });
            list.into_iter().map(|x| x.0).collect()
        };

        let expected = sort_all(items.clone());
        assert_eq!(expected[0], "你好");
        assert_eq!(expected.last().copied(), Some("nihao"));

        // 多种确定性打乱（反转、循环移位、伪随机置换）全部必须产出完全相同序列
        let mut rev = items.clone();
        rev.reverse();
        assert_eq!(sort_all(rev), expected);

        for rot in 1..items.len() {
            let mut r = items.clone();
            r.rotate_left(rot);
            assert_eq!(sort_all(r), expected);
        }

        // LCG 伪随机洗牌 20 轮
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        for _ in 0..20 {
            let mut shuf = items.clone();
            for i in (1..shuf.len()).rev() {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let j = (seed as usize) % (i + 1);
                shuf.swap(i, j);
            }
            assert_eq!(sort_all(shuf), expected);
        }
    }

    // ── 公理 2（相对性）：同一个词在不同输入下落在不同 Match 层 ──────────────

    #[test]
    fn test_axiom2_relativity_same_word_different_levels() {
        let word = MatchInput {
            source: Source::Dictionary,
            reasons: reason::EXACT_PINYIN,
            pinyin: Some("nihao"),
            pinyin_len: 5,
            char_count: 2,
            total_chars: 2,
            clean_coverage: false,
            is_loose_combo: false,
        };
        let l_exact = match_of(&InputShape::build("nihao"), &word);
        let l_ext = match_of(&InputShape::build("niha"), &word);
        let l_trunc = match_of(&InputShape::build("nihaoa"), &word);

        assert_eq!(l_exact, Match::Exact);
        assert_eq!(l_ext, Match::Extension);
        assert_eq!(l_trunc, Match::Truncation);
        assert_ne!(l_exact, l_ext);
        assert_ne!(l_exact, l_trunc);
    }

    // ── 公理 3（单调性）：增强正向信号绝不使位次变差 ─────────────────────────

    #[test]
    fn test_axiom3_monotonicity_positive_signals() {
        let base = Attestation {
            match_level: Match::Exact,
            char_count: 2,
            freq: Some(2000),
            user_tier: 3,
            ..Default::default()
        };
        let k_base = base.rank_key(0, Form::Text, true);

        // 1. 变为用户词
        let mut as_user = base;
        as_user.is_user = true;
        as_user.user_tier = 0;
        assert!(as_user.rank_key(0, Form::Text, true) <= k_base);

        // 2. 增加上下文提权（且能翻转词频差）
        let mut with_ctx = base;
        with_ctx.context_lift = 4;
        let mut higher_freq = base;
        higher_freq.freq = Some(5000);
        assert!(with_ctx.rank_key(0, Form::Text, true) < higher_freq.rank_key(0, Form::Text, true));

        // 3. 减少切分段数（整词 1 段胜过 3 段整句）
        let mut multi_seg = base;
        multi_seg.segment_count = 3;
        assert!(base.rank_key(0, Form::Text, true) < multi_seg.rank_key(0, Form::Text, true));

        // 4. 常用词人工行序（想=22 胜过 向=490，尽管系统词频 4029 < 4103）
        let xiang_think = Attestation {
            match_level: Match::Exact,
            char_count: 1,
            freq: Some(4029),
            user_tier: 1,
            common_order: Some(22),
            is_common: true,
            ..Default::default()
        };
        let xiang_dir = Attestation {
            match_level: Match::Exact,
            char_count: 1,
            freq: Some(4103),
            user_tier: 1,
            common_order: Some(490),
            is_common: true,
            ..Default::default()
        };
        assert!(
            xiang_think.rank_key(0, Form::Text, true) < xiang_dir.rank_key(0, Form::Text, true)
        );
    }

    // ── Step 3 校验：query_with_split_meta / completion_candidates_meta 逐条一致 ─

    #[test]
    fn test_dict_meta_parity_with_legacy_api() {
        let samples = [
            "nihao",
            "zhongguo",
            "xian",
            "yuan",
            "hou",
            "lian",
            "w",
            "a",
            "qv",
            "suanfa",
            "youxianji",
            "shuru",
        ];
        for input in samples {
            let (old_words, old_exact) = crate::dictionary::query_with_split(input);
            let (meta_entries, meta_exact) = crate::dictionary::query_with_split_meta(input);
            let meta_words: Vec<String> = meta_entries.iter().map(|e| e.word.clone()).collect();
            assert_eq!(
                old_exact, meta_exact,
                "exact_len mismatch on input `{input}`"
            );
            assert_eq!(
                old_words, meta_words,
                "word order mismatch on input `{input}`"
            );
            // 精确段必须携带 Some(norm_key)
            let norm = crate::pinyin::normalize_v(&input.to_lowercase());
            for entry in meta_entries.iter().take(meta_exact) {
                assert_eq!(
                    entry.pinyin.as_deref(),
                    Some(norm.as_str()),
                    "exact entry `{}` missing pinyin on `{input}`",
                    entry.word
                );
            }

            // completion_candidates_meta 必须与 completion_candidates 完全同序
            let old_comp = crate::dictionary::completion_candidates(input, 5);
            let meta_comp = crate::dictionary::completion_candidates_meta(input, 5);
            let meta_comp_words: Vec<String> = meta_comp.iter().map(|e| e.word.clone()).collect();
            assert_eq!(
                old_comp, meta_comp_words,
                "completion_candidates mismatch on `{input}`"
            );
            for entry in &meta_comp {
                assert!(
                    entry.pinyin.is_some(),
                    "completion entry `{}` missing full pinyin on `{input}`",
                    entry.word
                );
            }
        }
    }

    // ── Phase C 验收：v2 默认启用，CandidateMeta.match_level 完整填充，重排幂等 ──

    #[test]
    fn test_shadow_mode_meta_and_diff_attribution() {
        let mut engine = crate::Engine::new();
        assert!(engine.ranking_v2(), "Phase C 后 ranking_v2 默认启用");
        for ch in "nihao".chars() {
            engine.process_key(ch);
        }
        assert!(engine.candidate_count() > 0);
        // 统一 v2 归约已把 match_level 写入 CandidateMeta
        let m0 = engine.candidate_explain(0).expect("meta[0] should exist");
        assert!(m0.match_level.is_some());

        let diff = engine
            .diff_ranking("nihao")
            .expect("diff_ranking should return Some");
        assert_eq!(
            diff.old_order, diff.new_order,
            "v2 已转正，再排序应完全幂等"
        );
        assert!(diff.changed.is_empty());
        assert!(diff.explanation.is_empty());
    }

    // ── Phase D：形态簇单次排序与公理 1（打乱不变性）─────────────────────────────

    #[test]
    fn test_cluster_sort_keeps_variants_adjacent_and_shuffle_invariant() {
        let exact_hi = Attestation {
            match_level: Match::Exact,
            char_count: 2,
            is_common: true,
            user_tier: 1,
            freq: Some(5000),
            ..Default::default()
        };
        let exact_lo = Attestation {
            match_level: Match::Exact,
            char_count: 2,
            is_common: true,
            user_tier: 1,
            freq: Some(2000),
            ..Default::default()
        };
        let variant_att = Attestation {
            match_level: Match::Foreign,
            freq: Some(100),
            ..Default::default()
        };
        let variant_att_lo = Attestation {
            match_level: Match::Foreign,
            freq: Some(99),
            ..Default::default()
        };
        let base: Vec<(&str, &str, Form, RankKey)> = vec![
            (
                "喜欢",
                "喜欢",
                Form::Text,
                exact_hi.rank_key(0, Form::Text, true),
            ),
            (
                "喜欢😍",
                "喜欢",
                Form::Emoji,
                variant_att.rank_key(0, Form::Emoji, true),
            ),
            (
                "喜欢💕",
                "喜欢",
                Form::Emoji,
                variant_att_lo.rank_key(0, Form::Emoji, true),
            ),
            (
                "(^▽^)ノ",
                "喜欢",
                Form::Symbol,
                variant_att.rank_key(0, Form::Symbol, true),
            ),
            (
                "西欢",
                "西欢",
                Form::Text,
                exact_lo.rank_key(0, Form::Text, true),
            ),
        ];

        fn run<'a>(items: &[(&'a str, &'a str, Form, RankKey)]) -> Vec<&'a str> {
            let ord = cluster_sort_order(items.len(), true, |i| items[i]);
            ord.into_iter().map(|i| items[i].0).collect()
        }

        let expected = vec!["喜欢", "喜欢😍", "喜欢💕", "(^▽^)ノ", "西欢"];
        assert_eq!(run(&base), expected);

        // 逆序与多种伪随机打乱下，簇锚定顺序必须严格一致（公理 1）
        let mut rev = base.clone();
        rev.reverse();
        assert_eq!(run(&rev), expected);

        let mut perm = base.clone();
        let mut seed = 0x517cc1b727220a95u64;
        for _ in 0..8 {
            for i in (1..perm.len()).rev() {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let j = (seed as usize) % (i + 1);
                perm.swap(i, j);
            }
            assert_eq!(run(&perm), expected, "打乱输入后簇排序结果必须完全一致");
        }
    }

    #[test]
    fn test_low_freq_exact_multi_char_demotion() {
        // 1. 有可信精确词时，低频多字精确词（< 500）降权至 tier 6：位于前缀扩展(tier 5)之后，
        //    但仍严格优先于模糊音/纠错(tier 7)与外文(tier 9)
        let kankan_rare = Attestation {
            match_level: Match::Exact,
            char_count: 2,
            freq: Some(401),
            user_tier: 3,
            ..Default::default()
        };
        let kankan_mid = Attestation {
            match_level: Match::Exact,
            char_count: 2,
            freq: Some(1442),
            user_tier: 3,
            ..Default::default()
        };
        let kankan_ext = Attestation {
            match_level: Match::Extension,
            extent_rank: 1,
            char_count: 3,
            freq: Some(936),
            user_tier: 3,
            ..Default::default()
        };
        let fuzzy = Attestation {
            match_level: Match::Inferred,
            char_count: 3,
            freq: Some(2000),
            user_tier: 3,
            is_variant: true,
            ..Default::default()
        };
        assert_eq!(kankan_mid.effective_match_tier(true), 4);
        assert_eq!(kankan_ext.effective_match_tier(true), 5);
        assert_eq!(kankan_rare.effective_match_tier(true), 6);
        assert_eq!(fuzzy.effective_match_tier(true), 7);
        assert!(
            kankan_mid.rank_key(0, Form::Text, true) < kankan_ext.rank_key(0, Form::Text, true)
        );
        assert!(
            kankan_ext.rank_key(0, Form::Text, true) < kankan_rare.rank_key(0, Form::Text, true)
        );
        assert!(kankan_rare.rank_key(0, Form::Text, true) < fuzzy.rank_key(0, Form::Text, true));

        // 2. 阈值边界：>= 500 保持 tier 4，不受影响
        let boundary = Attestation {
            match_level: Match::Exact,
            char_count: 2,
            freq: Some(LOW_FREQ_EXACT_WORD_THRESHOLD),
            user_tier: 3,
            ..Default::default()
        };
        assert_eq!(boundary.effective_match_tier(true), 4);

        // 3. 单字豁免：即使词频很低也留在 tier 4，不降权
        let single_low = Attestation {
            match_level: Match::Exact,
            char_count: 1,
            freq: Some(30),
            user_tier: 3,
            ..Default::default()
        };
        assert_eq!(single_low.effective_match_tier(true), 4);

        // 4. 无可信精确词时（如长专名「机器学习=1」「神经网络=401」），保留 tier 4 不被整句/扩展压过
        assert_eq!(kankan_rare.effective_match_tier(false), 4);

        // 5. 领域词无系统词频（freq=None，如「中华人民共和国」），不降权
        let domain_only = Attestation {
            match_level: Match::Exact,
            char_count: 7,
            freq: None,
            user_tier: 3,
            ..Default::default()
        };
        assert_eq!(domain_only.effective_match_tier(true), 4);
    }

    #[test]
    fn test_low_freq_extension_demotion_and_tier6_order() {
        // 1. 高频扩展（看看有没有=2082, extent=3）保持 tier 5，压过低频精确（砍砍=401, tier 6, extent=0）
        //    与低频扩展（侃侃訚訚=401, tier 6, extent=2）；
        //    同落 tier 6 时，精确词（extent=0）必须严格优于扩展词（extent>=1），
        //    且低频扩展仍严格优于模糊音/纠错（tier 7）。
        let ext_high = Attestation {
            match_level: Match::Extension,
            extent_rank: 3,
            char_count: 5,
            freq: Some(2082),
            user_tier: 3,
            ..Default::default()
        };
        let exact_low = Attestation {
            match_level: Match::Exact,
            extent_rank: 0,
            char_count: 2,
            freq: Some(401),
            user_tier: 3,
            ..Default::default()
        };
        let ext_low = Attestation {
            match_level: Match::Extension,
            extent_rank: 2,
            char_count: 4,
            freq: Some(401),
            user_tier: 3,
            ..Default::default()
        };
        let ext_low_min_extent = Attestation {
            match_level: Match::Extension,
            extent_rank: 1,
            char_count: 3,
            freq: Some(499),
            user_tier: 3,
            ..Default::default()
        };
        let fuzzy = Attestation {
            match_level: Match::Inferred,
            char_count: 3,
            freq: Some(3000),
            user_tier: 3,
            is_variant: true,
            ..Default::default()
        };

        assert_eq!(ext_high.effective_match_tier(true), 5);
        assert_eq!(exact_low.effective_match_tier(true), 6);
        assert_eq!(ext_low.effective_match_tier(true), 6);
        assert_eq!(ext_low_min_extent.effective_match_tier(true), 6);
        assert_eq!(fuzzy.effective_match_tier(true), 7);

        let k_ext_hi = ext_high.rank_key(0, Form::Text, true);
        let k_ex_lo = exact_low.rank_key(0, Form::Text, true);
        let k_ext_lo_1 = ext_low_min_extent.rank_key(0, Form::Text, true);
        let k_ext_lo_2 = ext_low.rank_key(0, Form::Text, true);
        let k_fuzzy = fuzzy.rank_key(0, Form::Text, true);
        assert!(
            k_ext_hi < k_ex_lo,
            "高频扩展(tier 5) 应优于低频精确(tier 6)"
        );
        assert!(
            k_ex_lo < k_ext_lo_1,
            "同在 tier 6 时低频精确(extent=0) 必须优于低频扩展(extent=1，即便扩展词频更高)"
        );
        assert!(k_ext_lo_1 < k_ext_lo_2);
        assert!(
            k_ext_lo_2 < k_fuzzy,
            "低频扩展(tier 6) 仍必须优于模糊音/纠错(tier 7)"
        );

        // 2. 常用/用户前缀补全（tier 3）完全不受低频门槛影响
        let comp_common = Attestation {
            match_level: Match::Extension,
            extent_rank: 1,
            char_count: 2,
            freq: Some(3055),
            is_completion: true,
            is_common: true,
            user_tier: 1,
            ..Default::default()
        };
        let comp_user_low = Attestation {
            match_level: Match::Extension,
            extent_rank: 1,
            char_count: 2,
            freq: Some(100),
            is_completion: true,
            is_user: true,
            user_tier: 0,
            ..Default::default()
        };
        assert_eq!(comp_common.effective_match_tier(true), 3);
        assert_eq!(comp_user_low.effective_match_tier(true), 3);

        // 3. 阈值边界：freq == 500 留在 tier 5，不降权
        let ext_boundary = Attestation {
            match_level: Match::Extension,
            extent_rank: 1,
            char_count: 3,
            freq: Some(LOW_FREQ_EXACT_WORD_THRESHOLD),
            user_tier: 3,
            ..Default::default()
        };
        assert_eq!(ext_boundary.effective_match_tier(true), 5);

        // 4. 护栏：exact_has_credible == false、无词频领域扩展(freq=None)、用户/常用/上下文提权均不降权
        assert_eq!(ext_low.effective_match_tier(false), 5);
        let ext_domain = Attestation {
            match_level: Match::Extension,
            extent_rank: 1,
            char_count: 4,
            freq: None,
            user_tier: 3,
            ..Default::default()
        };
        assert_eq!(ext_domain.effective_match_tier(true), 5);
        let mut ext_common = ext_low;
        ext_common.is_common = true;
        assert_eq!(ext_common.effective_match_tier(true), 5);
        let mut ext_ctx = ext_low;
        ext_ctx.context_lift = 4;
        assert_eq!(ext_ctx.effective_match_tier(true), 5);
    }
}
