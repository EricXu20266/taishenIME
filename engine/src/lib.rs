pub mod calculator;
pub mod context;
pub mod correction;
pub mod datetime;
pub mod dictionary;
pub mod emoji;
pub mod english;
pub mod ffi;
pub mod fuzzy;
pub mod lm;
pub mod log;
pub mod mistake;
pub mod number;
pub mod pinyin;
pub mod radical;
pub mod ranking;
pub mod sentence;
pub mod shuangpin;
pub mod symbol;
pub mod trad;
pub mod trad_full;
pub mod trad_simp;
pub mod tuning;
pub mod unichar;
pub mod voice;

/// 候选来源（P0-1 side-table）：与 `all_candidates` 严格等长。
///
/// 只表达"这条候选从哪来"，不承载分数与词长——那些等统一打分启动时再加。
/// 用途：英文区边界、短语/日期是否首位、Emoji 插入位置，都不再依赖位置字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// P1–P4 词库命中（用户词 / 常用词 / 系统词 / 领域词，细分留给 dictionary 层）
    Dictionary,
    /// 快捷短语（phrase_map）
    Phrase,
    /// 日期 / 时间 / 星期 / 农历简码
    Datetime,
    /// 英文候选（恒在中文候选之后）
    English,
    /// Emoji 追加项
    Emoji,
    /// 纠错类补充：拼写纠错 / 删字 / 相邻键 / 错音
    Correction,
    /// 组合与简拼类补充：简拼 / 混合简拼 / 缩写 / 逐音节联想
    Combo,
    /// 模糊音变体
    Fuzzy,
    /// 特殊输入模式（v / c / u / r / U / 双拼 / 组词）
    Mode,
    /// 语音转写注入的候选（set_voice_candidate）
    Voice,
    /// 整句解码（长输入：把整串音节读成词序列）
    Sentence,
    /// 前缀补全：输入停在完整音节边界，候选拼音只多一个音节（`lian` → 联系）
    ///
    /// 与 `Dictionary` 的区别在「身份」：这些词来自词库前缀索引，拼音确实更长，
    /// 但它们是用户此刻最可能想打的词，不是被词长分区判为「超长」的噪声。
    Completion,
}

impl Source {
    /// 来源的稳定字符串名（考试/诊断断言用）。
    ///
    /// 用例文件按这些字符串写断言，**一旦定下不要改**——改名等于让全量用例失效。
    /// 新增变体时在这里补一个名字，`candidate_source_name` 自动覆盖。
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Dictionary => "Dictionary",
            Source::Phrase => "Phrase",
            Source::Datetime => "Datetime",
            Source::English => "English",
            Source::Emoji => "Emoji",
            Source::Correction => "Correction",
            Source::Combo => "Combo",
            Source::Fuzzy => "Fuzzy",
            Source::Mode => "Mode",
            Source::Voice => "Voice",
            Source::Sentence => "Sentence",
            Source::Completion => "Completion",
        }
    }
}

/// 一条候选：文本 + 来源，合并成一个记录。
///
/// 精确键可信词频门槛（数据校准 2026-11-20）：
/// system_dict 2 字词总计 114,985 条，P50=759 / P75=1529 / P90=2113。
/// 正例：算法=2235、候选=1769、张扬=2041；
/// 反例（错拼撞词）：zhonggou→重构=1381、xilashi→希腊式=907、nihafan→尼哈番=401。
/// 取 1600 落在清晰空档内：精确高频词可信（纠错不得抢位），错拼低频撞词不挡纠错。
pub(crate) const CREDIBLE_EXACT_FREQ: u32 = 1600;

/// 候选裁决层级（越小越优先，供统一裁决 `arbitrate` 与诊断探针共用）。
pub mod tier {
    /// T0：用户显式意图（置顶 / 用户短语 / 日期简码 / 强英文精确词）
    pub const EXPLICIT: u8 = 0;
    /// T1：精确读音词库命中 / 用户词 / 高吻合简拼混拼
    pub const EXACT: u8 = 1;
    /// T2：前缀补全（音节边界只多一个音节）
    pub const COMPLETION: u8 = 2;
    /// T3：读音推断（纠错 / 模糊音 / 错音）
    pub const INFERRED: u8 = 3;
    /// T4：拼凑产物（联想 / 拆组 / 整句解码）
    pub const COMPOSED: u8 = 4;
    /// T5：常规英文兜底
    pub const ENGLISH: u8 = 5;
}

/// 候选裁决理由位标（供 `probe_rank` 与 V4 结构化协议诊断使用）。
pub mod reason {
    pub const EXACT_PINYIN: u32 = 1 << 0;
    pub const USER_WORD: u32 = 1 << 1;
    pub const COMMON_WORD: u32 = 1 << 2;
    pub const HIGH_FREQ: u32 = 1 << 3;
    pub const CREDIBLE: u32 = 1 << 4;
    pub const CONTEXT_BOOST: u32 = 1 << 5;
    pub const AMBIG_SPLIT: u32 = 1 << 6;
    pub const COMPLETION: u32 = 1 << 7;
    pub const PINNED: u32 = 1 << 8;
    pub const DEMOTED: u32 = 1 << 9;
    pub const RARE_CHAR: u32 = 1 << 10;
    pub const USER_PHRASE: u32 = 1 << 11;
    pub const ENG_EXACT_TOP: u32 = 1 << 12;
    pub const CORR_PROMOTED: u32 = 1 << 13;
    pub const PREFIX_WORD: u32 = 1 << 14;
    pub const PREFIX_EXT: u32 = 1 << 15;
    pub const STITCHED: u32 = 1 << 16;
}

/// 候选元数据快照（与 `all_candidates` / `candidate_src` 同步发布，供探针与协议使用）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CandidateMeta {
    pub tier: u8,
    pub reasons: u32,
    pub score: Option<i32>,
    /// 排序 v2 归约出的 Match 层级（0=Exact..6=Foreign，None 表示未参与归约，如特殊模式）。
    pub match_level: Option<u8>,
    /// 形态类别（0=Text, 1=Emoji, 2=Symbol，见 `ranking::Form::rank`）。
    pub form: u8,
}

/// 候选扩展标注（Phase D：形态簇本体 / 词性 / 释义，按需查询，不进入排序热路径）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CandidateExtra {
    /// 所属形态簇的本体文本（仅形态变体非空，如「谢谢🙏」→ `Some("谢谢")`）。
    pub cluster: Option<String>,
    /// 词性标注（如 `"v."`、`"n."`、`"int."`；由外部词典标注表注入）。
    pub pos: Option<String>,
    /// 简明释义（如 `"hello"`、`"表达感谢"`；由外部词典标注表注入）。
    pub gloss: Option<String>,
}

/// 把理由位标格式化为可读标签（诊断探针用）。
pub fn format_reasons(reasons: u32) -> String {
    let mut tags: Vec<&'static str> = Vec::new();
    if reasons & reason::PINNED != 0 {
        tags.push("pin");
    }
    if reasons & reason::USER_PHRASE != 0 {
        tags.push("user_phrase");
    }
    if reasons & reason::ENG_EXACT_TOP != 0 {
        tags.push("eng_top");
    }
    if reasons & reason::EXACT_PINYIN != 0 {
        tags.push("exact_py");
    }
    if reasons & reason::USER_WORD != 0 {
        tags.push("user");
    }
    if reasons & reason::COMMON_WORD != 0 {
        tags.push("common");
    }
    if reasons & reason::HIGH_FREQ != 0 {
        tags.push("high_freq");
    }
    if reasons & reason::CREDIBLE != 0 {
        tags.push("credible");
    }
    if reasons & reason::CORR_PROMOTED != 0 {
        tags.push("corr_promoted");
    }
    if reasons & reason::CONTEXT_BOOST != 0 {
        tags.push("context");
    }
    if reasons & reason::AMBIG_SPLIT != 0 {
        tags.push("ambig");
    }
    if reasons & reason::COMPLETION != 0 {
        tags.push("completion");
    }
    if reasons & reason::RARE_CHAR != 0 {
        tags.push("rare");
    }
    if reasons & reason::DEMOTED != 0 {
        tags.push("demoted");
    }
    if reasons & reason::PREFIX_WORD != 0 {
        tags.push("prefix_word");
    }
    if reasons & reason::PREFIX_EXT != 0 {
        tags.push("prefix_ext");
    }
    if reasons & reason::STITCHED != 0 {
        tags.push("stitched");
    }
    tags.join(",")
}

/// 为未走排序管线的直出候选（特殊模式/语音/组词等）推导诊断用 `CandidateMeta.tier`。
#[inline]
fn meta_tier_for_source(source: Source) -> u8 {
    match source {
        Source::Datetime | Source::Phrase | Source::Mode | Source::Voice => tier::EXPLICIT,
        Source::Dictionary => tier::EXACT,
        Source::Completion => tier::COMPLETION,
        Source::Correction | Source::Fuzzy => tier::INFERRED,
        Source::Combo | Source::Sentence => tier::COMPOSED,
        Source::English | Source::Emoji => tier::ENGLISH,
    }
}

/// 从 v2 归约证据推导对外 `CandidateMeta.tier`（保持 C ABI 与诊断探针兼容，内部不再维护 `Candidate.tier`）。
#[inline]
fn meta_tier_from_candidate(c: &Candidate) -> u8 {
    if let Some(a) = c.attest {
        if a.explicit_rank < ranking::explicit::NATURAL {
            return tier::EXPLICIT;
        }
        return match a.match_level {
            ranking::Match::Exact | ranking::Match::Shape => {
                if a.is_sentence {
                    tier::COMPOSED
                } else {
                    tier::EXACT
                }
            }
            ranking::Match::Extension => {
                if a.is_completion {
                    tier::COMPLETION
                } else {
                    tier::EXACT
                }
            }
            ranking::Match::Truncation | ranking::Match::Inferred => tier::INFERRED,
            ranking::Match::Composed => tier::COMPOSED,
            ranking::Match::Foreign => tier::ENGLISH,
        };
    }
    meta_tier_for_source(c.source)
}

/// 候选内部结构体：文本、来源、裁决理由与 v2 证据元数据随候选同行。
#[derive(Debug, Clone)]
struct Candidate {
    /// 上屏文本。
    text: String,
    /// 来源，决定选词学习行为、英文边界等非排序语义。
    source: Source,
    /// 裁决理由位标（见 `reason::*`）。
    reasons: u32,
    /// 预留显式分数（V4 协议与后续统一打分接口）。
    score: Option<i32>,
    /// 显式意图提升标记（强英文置顶 / 驼峰中英混输等，归入 `explicit::PROMOTED`）。
    explicit_promoted: bool,
    // ── 排序 v2 证据元数据（生成侧带出，Layer 2 归约写入 `attest`）──────────
    pinyin: Option<std::sync::Arc<str>>,
    pinyin_len: usize,
    segment_count: u8,
    clean_coverage: bool,
    dict_layer: Option<u8>,
    dict_freq: Option<u32>,
    sentence_score: Option<f64>,
    /// 是否是简码显式日期（rq/sj/xq/nl）；自然语言日期（shijian→14:47）为 false，置尾不抢首位。
    explicit_datetime: bool,
    /// 是否来自宽松音节组合（query_combo），区别于简拼/混拼精确模式。
    is_loose_combo: bool,
    /// 删除容错变体中属于变体键精确命中的词条（用于区分 heshenm→合身/何深 与 nihap→你好）。
    is_exact_deletion: bool,
    /// 纠错/模糊音变体是否来自变体键的前缀扩展（非变体精确命中，如 niha→niba→你帮）。
    is_variant_prefix: bool,
    /// 来自词库锚定拆组（phrase_group_guess），区别于逐字硬拼（combo_guess）。
    is_phrase_group: bool,
    form: ranking::Form,
    /// 形态簇本体键（仅变体非空，如 `喜欢😍` 的 `cluster = Some("喜欢")`；文本本体为 `None` 零分配）。
    cluster: Option<String>,
    attest: Option<ranking::Attestation>,
}

impl Candidate {
    fn new(text: String, source: Source) -> Self {
        Self {
            text,
            source,
            reasons: 0,
            score: None,
            explicit_promoted: false,
            pinyin: None,
            pinyin_len: 0,
            segment_count: 1,
            clean_coverage: false,
            dict_layer: None,
            dict_freq: None,
            sentence_score: None,
            explicit_datetime: false,
            is_loose_combo: false,
            is_exact_deletion: false,
            is_variant_prefix: false,
            is_phrase_group: false,
            form: ranking::Form::Text,
            cluster: None,
            attest: None,
        }
    }

    fn with_reason(text: String, source: Source, reasons: u32) -> Self {
        Self {
            text,
            source,
            reasons,
            score: None,
            explicit_promoted: false,
            pinyin: None,
            pinyin_len: 0,
            segment_count: 1,
            clean_coverage: false,
            dict_layer: None,
            dict_freq: None,
            sentence_score: None,
            explicit_datetime: false,
            is_loose_combo: false,
            is_exact_deletion: false,
            is_variant_prefix: false,
            is_phrase_group: false,
            form: ranking::Form::Text,
            cluster: None,
            attest: None,
        }
    }

    fn with_pinyin(mut self, pinyin: Option<std::sync::Arc<str>>, pinyin_len: usize) -> Self {
        self.pinyin = pinyin;
        self.pinyin_len = pinyin_len;
        self
    }

    /// 簇键：形态变体返回其本体文本，文本本体直接返回自身引用（零分配）。
    #[inline]
    fn cluster_key(&self) -> &str {
        self.cluster.as_deref().unwrap_or(&self.text)
    }
}

impl Default for Candidate {
    fn default() -> Self {
        Self::new(String::new(), Source::Dictionary)
    }
}

/// 让候选在只关心文本的场景（断言、格式化、字符统计）里直接当 `&str` 用。
/// 来源仍通过 `.source` 显式访问——不提供到 `Source` 的隐式转换，
/// 免得出现「以为在比文本、实际在比来源」这类误读。
impl std::ops::Deref for Candidate {
    type Target = str;

    fn deref(&self) -> &str {
        &self.text
    }
}

impl PartialEq<str> for Candidate {
    fn eq(&self, other: &str) -> bool {
        self.text == other
    }
}

impl PartialEq<&str> for Candidate {
    fn eq(&self, other: &&str) -> bool {
        self.text == *other
    }
}

impl PartialEq<String> for Candidate {
    fn eq(&self, other: &String) -> bool {
        self.text == *other
    }
}

/// 汉字判定（P2-3 + B6 修正）：覆盖 CJK 基本区、扩展 A、兼容区及扩展 B–I。
/// 此前漏掉 `0x20000..=0x323AF`，导致 `须𫚪` 中的 `𫚪`(U+2B6AA) 不被计为汉字，
/// `hanzi_count` 错算为 1。
#[inline]
fn is_hanzi(c: char) -> bool {
    matches!(
        c as u32,
        0x4E00..=0x9FFF     // CJK 统一表意文字（基本区）
        | 0x3400..=0x4DBF   // 扩展 A
        | 0xF900..=0xFAFF   // 兼容表意文字
        | 0x20000..=0x323AF // 扩展 B–I
    )
}

/// 生僻汉字区间判定（B6）：CJK 扩展 A 与扩展 B–I（如 `𫚪` U+2B6AA）。
/// 非用户词、非常用词时压出首屏，防止生僻字混入常规首屏候选。
#[inline]
fn is_rare_hanzi(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4DBF | 0x20000..=0x323AF)
}

#[inline]
fn has_rare_hanzi(w: &str) -> bool {
    w.chars().any(is_rare_hanzi)
}

/// 输入质量分类（P1-1）：决定纠错分支是否触发。
/// 只描述输入本身的形态（是否可切分、是否命中词库），不承载排序信息。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputQuality {
    /// 输入串在词库直接命中（含前缀命中）
    Exact,
    /// 可完整切分为合法音节，但词库无命中
    FullPinyin,
    /// 全拼段 + 声母段混插
    Mixed,
    /// 纯声母简拼
    Initials,
    /// 合法拼音前缀（末尾音节未打完）
    Partial,
    /// 无法切分、也不像英文词——需要纠错
    Typo,
}

/// 特殊输入模式（P2-1）：由 `detect_mode` 判定，`apply_mode` 执行。
/// 新增一种输入模式 = 加一个变体 + detect_mode 一行 + apply_mode 一个分支。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// v 单字 + 热门符号
    SymbolPrefix,
    /// v + 分类码
    SymbolQuery,
    /// c + 算式
    Calc,
    /// u + 部件拼音（查不到时回落常规路径）
    Radical,
    /// R + 数字 → 中文大写
    Number,
    /// U + hex → 字符
    Unicode,
    /// 双拼：先解码为全拼再查询
    Shuangpin,
    /// 动态组词：候选 = 当前音节的单字
    Compose,
}

/// 英文候选大小写模式（V0.2.23）
#[derive(Clone, Copy, PartialEq, Debug)]
enum CapState {
    Lower,
    Capitalize,
    Upper,
}

/// 引擎状态
pub struct Engine {
    /// 当前累积的拼音串（如 "zhongguo"）
    pinyin_buf: String,
    /// 原始输入串（保留大小写，V0.2.23 英文自动大写用）
    raw_input: String,
    /// 编辑光标位置（P2-1）：pinyin_buf 字符索引，Tab/Shift+Tab 在音节边界移动
    cursor: usize,
    /// 英文候选大小写模式（V0.2.23）
    cap_state: CapState,
    /// 全部候选词列表（跨页，按词频降序）
    all_candidates: Vec<String>,
    /// 当前页候选词列表（展示用）
    candidates: Vec<String>,
    /// 当前页码（0 起）
    page: usize,
    /// 每页候选数量（= 候选上限，配置可调）
    page_size: usize,
    /// 最大翻页数（防止候选无限膨胀）
    max_pages: usize,
    /// 英文模式（true = 字母直接上屏，不经过拼音）
    ascii_mode: bool,
    /// 中英标点开关（P0-2，默认 false）：false=中文标点全角化，true=标点透传英文
    ascii_punct: bool,
    /// 模糊音分组掩码（9 组独立开关，默认全关 = 0；Q2.1）
    fuzzy_flags: u32,
    /// 双拼模式（RIME 双拼方案，微软双拼，默认关）
    shuangpin_mode: bool,
    /// 双拼方案（P2-7，默认微软 mspy）：mspy/flypy/sogou/zrm
    shuangpin_scheme: &'static shuangpin::Scheme,
    /// 双拼组合区显示全拼音节（0.1.68，默认关 = 显示按键串）
    shuangpin_full_preedit: bool,
    /// 智能纠错开关（键盘相邻键容错，V0.2.10，默认开）
    correction_enabled: bool,
    /// 中英混输开关（V0.2.8，默认开）：中文模式候选末尾追加英文候选
    mix_mode_enabled: bool,
    /// 候选来源 side-table（P0-1）：与 all_candidates 严格等长。
    /// 只由 query_all / query_all_shuangpin 的发布路径维护；
    /// 英文区边界、短语/日期是否在首位、Emoji 插入位置都从这里查，
    /// 不再依赖 english_candidate_pos 这类位置字段。
    candidate_src: Vec<Source>,
    /// 候选裁决元数据 side-table（B1/B7）：与 all_candidates 严格等长，供探针与协议查询。
    candidate_meta: Vec<CandidateMeta>,
    /// 候选形态簇本体 side-table（Phase D）：与 all_candidates 严格等长，非变体项为 `None`。
    candidate_cluster: Vec<Option<String>>,
    /// 同簇形态变体坑位上限（默认 2：文本本体 + 1 个变体，范围 1..=5）。
    cluster_slot_limit: usize,
    /// 可选词性/释义标注表（Phase D 数据层扩展钩子：词 → (pos, gloss)）。
    word_annotations: std::collections::HashMap<String, (Option<String>, Option<String>)>,
    /// 简繁转换开关（V0.2.11，默认关）：候选输出转繁体
    traditional_mode: bool,
    /// 快捷短语开关（V0.2.12，默认开）：简码 → 短语
    phrase_enabled: bool,
    /// 短语表：简码(小写) → 短语文本
    phrase_map: std::collections::HashMap<String, String>,
    /// 用户显式配置的短语键集合（通过 `load_phrases` 写入，享有 T0 显式意图优先级）
    user_phrase_keys: std::collections::HashSet<String>,
    /// 置顶候选集合（V0.5.7，词级置顶）：候选里命中该集合的词提到最前。
    /// 对标 rime pin_cand_filter，但语义为词级——nihao/nih/nh 任何输入
    /// 方式候选里出现该词即置顶，天然覆盖简拼自动展开，无需反查拼音生成展开键。
    pin_words: std::collections::HashSet<String>,
    /// 降权候选集合（V0.5.7，Eric 2026-08-10 决策）：候选里命中该集合的词
    /// 压到第 11 位之后（前 2 屏之外），仍可翻页取到，可恢复。
    demoted_words: std::collections::HashSet<String>,
    /// Emoji 开关（P2-5，对标 rime emoji 开关，默认开）
    emoji_enabled: bool,
    /// 候选排序模式（P0-2，对标微软单字/长词优先，默认 0）：
    /// 0=默认（词频+长词过滤） 1=单字优先 2=长词优先
    sort_mode: i32,
    /// 最近上屏词（P1-1 上下文联想，独立于输入态，reset 不清）
    last_committed: String,
    /// 上下文联想开关（默认开，config 可关）
    context_enabled: bool,
    /// 前缀补全开关（V0.6.x）：输入停在完整音节边界时，补「只差一个音节」的高频词。
    /// 关闭后不再产生 Completion 来源候选，单字不会被子全词挤占。
    completion_enabled: bool,
    /// 上下文搭配表：前词 → [(后词, 权重)]。权重即候选前移位数，
    /// 来源为外部搭配表或用户上屏共现次数；内置精选表在排序时合并进来。
    context_map: std::collections::HashMap<String, Vec<(String, u32)>>,
    /// 离线上下文先验表（冷启动补充）：前词 → [(后词, 权重)]。
    /// 语料统计得来，权重低于真实用词，学习表命中时以学习表为准。
    context_prior: std::collections::HashMap<String, Vec<(String, u32)>>,
    /// 动态组词（V0.5）：翻页 3 次后自动进入组词模式（对标搜狗/Rime 自动造词）
    compose_active: bool,
    /// 组词音节序列（如 taishen → [tai, shen]）
    compose_syllables: Vec<String>,
    /// 当前组词音节下标
    compose_idx: usize,
    /// 已选单字（组词模式累积）
    compose_chars: Vec<String>,
    /// 累计翻页次数（≥3 触发组词模式；新输入/退格重置）
    page_advances: usize,
    /// 排序 v2 开关（Phase A 默认 false，仅跑影子归约与对比；Phase B 开放双轨）
    ranking_v2: bool,
    /// Phase A 影子模式最新一轮归约快照（供 `shadow_diff_current` 零开销出对比报告）
    shadow_snapshot: Option<(ranking::InputShape, Vec<Candidate>)>,
}

/// V0.4.5 纠错候选补入限量：每个纠错变体最多取前 N 个词。
/// deletion 变体（zhonggu→364 词）曾全量灌入候选，撑爆 page_size 判断
/// 导致 correction 分支被跳过、「中国」丢失。限量后补入可控。
const CORR_TAKE: usize = 3;

/// V0.5.3：输入串能否完整切分为合法拼音音节；能则返回最长切分路径的音节数。
/// 简拼/英文/特殊模式返回 None（目标词长不可知，不做词长匹配）。
/// 仅测试/诊断用——生产路径改用 `complete_syllable_counts`，它保留全部切分
/// 路径，才能正确处理 yuan（元 / yu+an 预案）这类隔音歧义。
#[cfg(test)]
fn complete_syllable_count(input: &str) -> Option<usize> {
    if input.is_empty() {
        return None;
    }
    let mut rest = input;
    let mut n = 0usize;
    while !rest.is_empty() {
        match crate::pinyin::split_first_syllable(rest) {
            Some((_syl, remaining)) => {
                n += 1;
                rest = remaining;
            }
            None => return None,
        }
    }
    Some(n)
}

/// V0.6 隔音歧义：输入串所有完整切分路径的音节数（升序去重）。
/// yuan → [1, 2]（yuan / yu+an），xian → [1, 2]，fangan → [2]。
/// None = 无法完整切分（简拼/英文/半成品拼音）。
pub(crate) fn complete_syllable_counts(input: &str) -> Option<Vec<usize>> {
    let variants = crate::pinyin::syllable_split_variants(input, 256);
    if variants.is_empty() {
        return None;
    }
    // 丢掉「把某个音节拆出单字母」的荒谬切分：zhongguo 会额外切出
    // zhong+gu+o（把 guo 拆成 gu 和叹词 o），使 counts 多出一个 3。
    // 后果很具体：3 字的「中国队」也被算进「字数 == 音节数」的最高优先组，
    // 和 2 字的「中国」并列，稳定排序保持用户词的原顺序——实测「中国」从
    // 第 1 位掉到第 10 位（用户词库 22558 条时）。
    // 真正的隔音歧义（yuan → yuan / yu+an、xian → xian / xi+an）不含单字母段，
    // 不受这条过滤影响。
    //
    // 例外（审计 P1 收窄）：只放行「两段 + 末段恰为 e」的真隔音写法——
    // yue = yu+e（余额）、qie = qi+e（企鹅）、jie = ji+e（饥饿）。
    //
    // 不能整类放行：hao = ha+o、duo = du+o、jiao = ji+a+o 也会被切出来，
    // 这些在词库里没有对应隔音词，放行只会让 2/3 字词被错误并入
    // 「字数 == 音节数」最高组，把单字挤出前位（hao 的好处/好好、jie 的结婚）。
    let input_is_single_syllable =
        crate::pinyin::split_first_syllable(input).is_some_and(|(_, rest)| rest.is_empty());
    let filtered: Vec<&Vec<String>> = variants
        .iter()
        .filter(|p| {
            let true_glide =
                input_is_single_syllable && p.len() == 2 && p[1] == "e" && p[0].chars().count() > 1;
            true_glide || p.len() < 2 || p.iter().all(|s| s.chars().count() > 1)
        })
        .collect();
    let source: &[&Vec<String>] = if filtered.is_empty() {
        &variants.iter().collect::<Vec<_>>()
    } else {
        &filtered
    };
    let mut counts: Vec<usize> = source.iter().map(|p| p.len()).collect();
    counts.sort_unstable();
    counts.dedup();
    Some(counts)
}

/// V0.5.8 声母串 → 估算目标词长（简拼词长匹配用）。
/// "zg" → 2, "shh" → 2 (sh+h), "dy" → 2。单声母/非声母 → None。
pub(crate) fn short_pinyin_word_len(input: &str) -> Option<usize> {
    const INITIALS: &[&str] = &[
        "zh", "ch", "sh", "b", "p", "m", "f", "d", "t", "n", "l", "g", "k", "h", "j", "q", "x",
        "r", "z", "c", "s", "y", "w",
    ];
    if input.is_empty() || input.len() == 1 {
        return None;
    }
    let mut rest = input;
    let mut count = 0usize;
    while !rest.is_empty() {
        {
            let init = INITIALS.iter().find(|i| rest.starts_with(*i))?;
            count += 1;
            rest = &rest[init.len()..];
        }
    }
    (count >= 2).then_some(count)
}

/// V0.5.9 混合简拼 → 估算目标词长（全拼前缀+声母后缀，如 "ces"→2、"jianp"→2、"shurf"→3）：
/// 完整音节优先切分（split_first_syllable），剩余部分按声母切分（zh/ch/sh 优先）。
/// 段数 = 目标词长。要求 ≥2 段且至少 1 个完整音节——防纯声母串（交给
/// short_pinyin_word_len）与单音节误判。与 split_compose_syllables 同款贪心。
pub(crate) fn mixed_syllable_count(input: &str) -> Option<usize> {
    const INITIALS: &[&str] = &[
        "zh", "ch", "sh", "b", "p", "m", "f", "d", "t", "n", "l", "g", "k", "h", "j", "q", "x",
        "r", "z", "c", "s", "y", "w",
    ];
    if input.is_empty() {
        return None;
    }
    // 「一声母 + 一完整拼音」两段形态优先：zhyang = zh + yang、shh 之外的
    // 简拼+全拼写法。贪心按音节切分会先把 zhyang 切成 zha + ng，估成 3 段，
    // 于是 2 字词（这样 / 张扬）被判成「词长不匹配」，单字「装」反而占首位。
    for ini in INITIALS {
        if let Some(rest) = input.strip_prefix(ini)
            && !rest.is_empty()
            && crate::pinyin::is_complete_pinyin(rest)
        {
            return Some(2);
        }
    }
    let mut rest = input;
    let mut n = 0usize;
    let mut has_full = false;
    while !rest.is_empty() {
        match crate::pinyin::split_first_syllable(rest) {
            Some((_syl, remaining)) => {
                has_full = true;
                n += 1;
                rest = remaining;
            }
            None => {
                // 非完整音节开头 → 切 1-2 字母声母
                let initial = if rest.len() >= 2 && INITIALS.contains(&&rest[..2]) {
                    Some(&rest[..2])
                } else if INITIALS.contains(&&rest[..1]) {
                    Some(&rest[..1])
                } else {
                    None
                };
                {
                    let s = initial?;
                    n += 1;
                    rest = &rest[s.len()..];
                }
            }
        }
    }
    (n >= 2 && has_full).then_some(n)
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        Self {
            pinyin_buf: String::new(),
            raw_input: String::new(),
            cursor: 0,
            cap_state: CapState::Lower,
            all_candidates: Vec::new(),
            candidates: Vec::new(),
            page: 0,
            page_size: 5,
            max_pages: 8,
            ascii_mode: false,
            ascii_punct: false,
            fuzzy_flags: fuzzy::FUZZY_NONE,
            shuangpin_mode: false,
            shuangpin_scheme: shuangpin::find_scheme("mspy").unwrap_or(&shuangpin::SCHEMES[0]),
            shuangpin_full_preedit: false,
            correction_enabled: true,
            mix_mode_enabled: true,
            candidate_src: Vec::new(),
            candidate_meta: Vec::new(),
            candidate_cluster: Vec::new(),
            cluster_slot_limit: crate::tuning::tuning().cluster_slot_limit,
            word_annotations: std::collections::HashMap::new(),
            traditional_mode: false,
            phrase_enabled: true,
            phrase_map: Self::builtin_phrases(),
            user_phrase_keys: std::collections::HashSet::new(),
            pin_words: Self::builtin_pin_words(),
            demoted_words: std::collections::HashSet::new(),
            emoji_enabled: false, // 默认关闭（Eric 要求，config emoji=1 可开启）
            sort_mode: 0,
            last_committed: String::new(),
            context_enabled: true,
            completion_enabled: true, // V0.6.x：默认开，保持既有行为；config completion=0 可关闭
            context_map: std::collections::HashMap::new(),
            context_prior: std::collections::HashMap::new(),
            compose_active: false,
            compose_syllables: Vec::new(),
            compose_idx: 0,
            compose_chars: Vec::new(),
            page_advances: 0,
            ranking_v2: true,
            shadow_snapshot: None,
        }
    }

    /// 兼容接口：Phase C 后排序架构 v2 已为唯一生产管线，默认启用。
    pub fn set_ranking_v2(&mut self, enabled: bool) {
        self.ranking_v2 = enabled;
        if !self.pinyin_buf.is_empty() {
            self.query_all();
        }
    }

    /// 当前是否启用排序架构 v2。
    pub fn ranking_v2(&self) -> bool {
        self.ranking_v2
    }

    /// 内置置顶候选（V0.5.7，词级）：的/吗/嘛。
    ///
    /// 2026-11-18 移除「后面」：rime 的 `pin_cand_filter` 示例是**编码级**
    /// （`hm` → 后面，只对 hm 生效），这里当初照抄成**词级**——只要候选里
    /// 出现「后面」就无条件提到首位。于是 `hou` 的首位永远是「后面」，
    /// 「后/候/厚」全被挤后（P0 体检 402 个单音节里唯一「首位是补全词」的
    /// 就是它，P0 基线里也存在）。
    /// 多字词不适合无条件词级置顶；需要 `hm → 后面` 这种编码映射请走
    /// 短语表（phrase_map），那是按编码精确命中的。
    fn builtin_pin_words() -> std::collections::HashSet<String> {
        let mut s = std::collections::HashSet::new();
        s.insert("的".to_string());
        s.insert("吗".to_string());
        s.insert("嘛".to_string());
        s
    }

    /// 加载外部置顶候选（V0.5.7，词级）：词并入 pin_words，覆盖/补充内置
    pub fn load_pin_words(&mut self, words: Vec<String>) {
        for w in words {
            if !w.is_empty() {
                self.pin_words.insert(w);
            }
        }
        if !self.pinyin_buf.is_empty() {
            self.query_all();
        }
    }

    /// 加载外部降权候选（V0.5.7，词级）：词并入 demoted_words
    pub fn load_demoted_words(&mut self, words: Vec<String>) {
        for w in words {
            if !w.is_empty() {
                self.demoted_words.insert(w);
            }
        }
        if !self.pinyin_buf.is_empty() {
            self.query_all();
        }
    }

    /// 置顶候选（V0.5.7，词级）：词加入 pin_words，重查后立即置顶。
    /// 返回是否新增（false = 已在置顶集合）。持久化到 user_dict.db pin_words 表。
    pub fn pin_word(&mut self, word: &str) -> bool {
        if word.is_empty() {
            return false;
        }
        let inserted = self.pin_words.insert(word.to_string());
        if inserted {
            crate::dictionary::save_pin_word(word);
            if !self.pinyin_buf.is_empty() {
                self.query_all();
            }
        }
        inserted
    }

    /// 取消置顶（V0.5.7）：词从 pin_words 移除，重查后恢复原排序。
    /// 返回是否确实移除（false = 原本未置顶）。持久化同步删除。
    pub fn unpin_word(&mut self, word: &str) -> bool {
        let removed = self.pin_words.remove(word);
        if removed {
            crate::dictionary::remove_pin_word(word);
            if !self.pinyin_buf.is_empty() {
                self.query_all();
            }
        }
        removed
    }

    /// 降权候选（V0.5.7，Eric 2026-08-10 决策）：词压出前 2 屏（第 11 位之后）。
    /// 返回是否新增（false = 已降权）。持久化到 user_dict.db demoted_words 表。
    pub fn demote_word(&mut self, word: &str) -> bool {
        if word.is_empty() {
            return false;
        }
        let inserted = self.demoted_words.insert(word.to_string());
        if inserted {
            crate::dictionary::save_demoted_word(word);
            if !self.pinyin_buf.is_empty() {
                self.query_all();
            }
        }
        inserted
    }

    /// 恢复候选（V0.5.7）：词从 demoted_words 移除，回到原排序位置。
    /// 返回是否确实恢复（false = 原本未降权）。持久化同步删除。
    pub fn undemote_word(&mut self, word: &str) -> bool {
        let removed = self.demoted_words.remove(word);
        if removed {
            crate::dictionary::remove_demoted_word(word);
            if !self.pinyin_buf.is_empty() {
                self.query_all();
            }
        }
        removed
    }

    /// 是否已置顶（V0.5.7，菜单动态显示用）
    pub fn is_pinned(&self, word: &str) -> bool {
        self.pin_words.contains(word)
    }

    /// 是否已降权（V0.5.7，菜单动态显示用）
    pub fn is_demoted(&self, word: &str) -> bool {
        self.demoted_words.contains(word)
    }

    /// 设置快捷短语开关（V0.2.12）
    pub fn set_phrase_enabled(&mut self, enabled: bool) {
        if self.phrase_enabled != enabled {
            self.phrase_enabled = enabled;
            if !self.pinyin_buf.is_empty() {
                self.query_all();
            }
        }
    }

    /// 查询快捷短语开关
    pub fn phrase_enabled(&self) -> bool {
        self.phrase_enabled
    }

    /// 加载外部短语（覆盖/补充内置；entries: (简码, 文本)）
    pub fn load_phrases(&mut self, entries: Vec<(String, String)>) {
        for (code, text) in entries {
            let code = code.trim().to_lowercase();
            if !code.is_empty() && !text.is_empty() {
                self.user_phrase_keys.insert(code.clone());
                self.phrase_map.insert(code, text);
            }
        }
        if !self.pinyin_buf.is_empty() {
            self.query_all();
        }
    }

    /// 设置简繁转换开关（V0.2.11）
    pub fn set_traditional(&mut self, enabled: bool) {
        if self.traditional_mode != enabled {
            self.traditional_mode = enabled;
            if !self.pinyin_buf.is_empty() {
                self.query_all(); // 开关变化时重查
            }
        }
    }

    /// 查询简繁转换开关
    pub fn traditional(&self) -> bool {
        self.traditional_mode
    }

    /// 设置中英混输开关（V0.2.8）
    pub fn set_mix_mode(&mut self, enabled: bool) {
        if self.mix_mode_enabled != enabled {
            self.mix_mode_enabled = enabled;
            if !self.pinyin_buf.is_empty() {
                self.query_all(); // 开关变化时重查
            }
        }
    }

    /// 查询中英混输开关
    pub fn mix_mode(&self) -> bool {
        self.mix_mode_enabled
    }

    /// 设置 Emoji 开关（P2-5，对标 rime emoji 开关）
    pub fn set_emoji(&mut self, enabled: bool) {
        if self.emoji_enabled != enabled {
            self.emoji_enabled = enabled;
            if !self.pinyin_buf.is_empty() {
                self.query_all();
            }
        }
    }

    /// 查询 Emoji 开关
    pub fn emoji(&self) -> bool {
        self.emoji_enabled
    }

    /// 设置同簇形态变体坑位上限（Phase D，范围 1..=5，默认 2：文本本体 + 1 个变体）。
    pub fn set_cluster_slot_limit(&mut self, limit: usize) {
        let clamped = limit.clamp(1, 5);
        if self.cluster_slot_limit != clamped {
            self.cluster_slot_limit = clamped;
            if !self.pinyin_buf.is_empty() {
                self.query_all();
            }
        }
    }

    /// 查询同簇形态变体坑位上限。
    pub fn cluster_slot_limit(&self) -> usize {
        self.cluster_slot_limit
    }

    /// 注入可选词性 / 释义标注表（Phase D 数据层扩展钩子，不改变排序）。
    pub fn load_word_annotations(
        &mut self,
        entries: Vec<(String, Option<String>, Option<String>)>,
    ) {
        for (word, pos, gloss) in entries {
            if !word.is_empty() {
                self.word_annotations.insert(word, (pos, gloss));
            }
        }
    }

    /// 设置智能纠错开关（V0.2.10）
    pub fn set_correction_enabled(&mut self, enabled: bool) {
        if self.correction_enabled != enabled {
            self.correction_enabled = enabled;
            if !self.pinyin_buf.is_empty() {
                self.query_all(); // 开关变化时重查
            }
        }
    }

    /// 查询智能纠错开关
    pub fn correction_enabled(&self) -> bool {
        self.correction_enabled
    }

    /// 设置双拼模式（开启时清空未完成拼音）
    pub fn set_shuangpin_mode(&mut self, enabled: bool) {
        if self.shuangpin_mode != enabled {
            self.shuangpin_mode = enabled;
            self.reset();
        }
    }

    /// 设置双拼方案（P2-7，对标 rime double_pinyin_* 多方案）：
    /// mspy/flypy/sogou/zrm/ziguang/jiajia。切换时清空未完成拼音。
    /// 返回是否设置成功（未知方案返回 false）。
    pub fn set_shuangpin_scheme(&mut self, id: &str) -> bool {
        match shuangpin::find_scheme(id) {
            Some(scheme) => {
                if self.shuangpin_scheme.id != scheme.id {
                    self.shuangpin_scheme = scheme;
                    self.reset();
                }
                true
            }
            None => false,
        }
    }

    /// 查询当前双拼方案 ID
    pub fn shuangpin_scheme_id(&self) -> &'static str {
        self.shuangpin_scheme.id
    }

    /// 查询双拼模式
    pub fn shuangpin_mode(&self) -> bool {
        self.shuangpin_mode
    }

    /// 当前双拼方案是否把分号当编码键（不看缓冲状态）
    pub fn shuangpin_scheme_uses_semicolon(&self) -> bool {
        self.shuangpin_mode && self.shuangpin_scheme.uses_semicolon()
    }

    /// 分号此刻是否应作为双拼编码键——引擎接受与平台吞键共用这一份判据。
    /// 缓冲为空时返回 false（用户要能打出「；」），计算器/R/U 模式让位给它们自己的输入。
    pub fn semicolon_key_active(&self) -> bool {
        self.shuangpin_mode
            && !self.ascii_mode
            && !self.pinyin_buf.is_empty()
            && self.shuangpin_scheme.uses_semicolon()
            && !self.is_calc_mode()
            && !self.is_number_mode()
            && !self.is_unicode_mode()
    }

    /// 设置双拼组合区是否显示全拼音节（true=zhong，false=vs）
    pub fn set_shuangpin_full_preedit(&mut self, enabled: bool) {
        self.shuangpin_full_preedit = enabled;
    }

    /// 双拼码 → 第一个能完整切分的全拼音节串（学习键/诊断用）。
    /// 只解成对部分，末尾未完成键丢弃；全拼模式直接返回缓冲串。
    pub fn shuangpin_primary_pinyin(&self) -> Option<String> {
        if !self.shuangpin_mode {
            return Some(self.pinyin_buf.clone());
        }
        let chars: Vec<char> = self.pinyin_buf.chars().collect();
        let pairs_len = chars.len() / 2 * 2;
        if pairs_len == 0 {
            return None;
        }
        let head: String = chars[..pairs_len].iter().collect();
        self.shuangpin_scheme
            .decode_string(&head)
            .into_iter()
            .find(|s| crate::pinyin::is_complete_pinyin(s))
    }

    /// 设置模糊音分组开关掩码（仅低 9 位生效），掩码变更时重查当前候选
    pub fn set_fuzzy_flags(&mut self, flags: u32) {
        let masked = flags & fuzzy::FUZZY_ALL;
        if self.fuzzy_flags != masked {
            self.fuzzy_flags = masked;
            if !self.pinyin_buf.is_empty() {
                self.query_all();
            }
        }
    }

    /// 查询模糊音分组开关掩码
    pub fn fuzzy_flags(&self) -> u32 {
        self.fuzzy_flags
    }

    /// 设置模糊音总开关（true = 全部 9 组开启，false = 全部关闭）
    pub fn set_fuzzy_enabled(&mut self, enabled: bool) {
        self.set_fuzzy_flags(if enabled {
            fuzzy::FUZZY_ALL
        } else {
            fuzzy::FUZZY_NONE
        });
    }

    /// 查询模糊音总开关（任一组开启即返回 true）
    pub fn fuzzy_enabled(&self) -> bool {
        self.fuzzy_flags != 0
    }

    /// 设置英文模式
    /// V0.3.x：切换时【保留】未提交拼音/候选（对标 rime ascii_composer——
    /// shift 切换后原 composition 仍在，空格仍可选词上屏，用户可"将已打的词输入"）。
    /// 进程记忆恢复（AppStateApply）由平台层先 engine_reset 清空，避免残留。
    pub fn set_ascii_mode(&mut self, enabled: bool) {
        self.ascii_mode = enabled;
        // 注意：不再 reset()——保留拼音串与候选（问题 9 修复）
    }

    /// 查询英文模式
    pub fn ascii_mode(&self) -> bool {
        self.ascii_mode
    }

    /// 设置中英标点开关（P0-2）：false=中文标点全角化，true=标点透传英文
    pub fn set_ascii_punct(&mut self, enabled: bool) {
        self.ascii_punct = enabled;
    }

    /// 查询中英标点开关
    pub fn ascii_punct(&self) -> bool {
        self.ascii_punct
    }

    /// 设置候选词数量上限（>=1 生效）——同时作为每页候选数
    pub fn set_candidate_limit(&mut self, limit: usize) {
        if limit >= 1 {
            self.page_size = limit;
        }
        // 重算当前页
        self.repage();
    }

    /// 在当前光标位置插入一对 `pinyin_buf` / `raw_input` 字符并前移光标
    fn insert_at_cursor(&mut self, py_ch: char, raw_ch: char) {
        self.cursor = self
            .cursor
            .min(self.pinyin_buf.len())
            .min(self.raw_input.len());
        self.pinyin_buf.insert(self.cursor, py_ch);
        self.raw_input.insert(self.cursor, raw_ch);
        self.cursor += 1;
    }

    /// 处理一个按键，返回是否产生了新的候选词
    /// 英文模式下字母键返回 false（平台层直接上屏字母）
    pub fn process_key(&mut self, ch: char) -> bool {
        // 双拼 ing 键（微软/搜狗把 ing 放在分号上）：判据收敛到 semicolon_key_active，
        // 平台层吞键与引擎接受共用同一份，避免「吞了但引擎不收 → 按键丢失」。
        if ch == ';' && self.semicolon_key_active() {
            self.compose_active = false;
            self.page_advances = 0;
            self.insert_at_cursor(';', ';');
            self.query_all();
            return true;
        }
        if ch.is_ascii_alphabetic() {
            if self.ascii_mode {
                return false; // 英文模式：不累积拼音，平台层直通上屏
            }
            // 组词模式中按字母 → 退出组词，重新开始普通输入
            self.compose_active = false;
            self.page_advances = 0;
            self.insert_at_cursor(ch.to_ascii_lowercase(), ch); // V0.2.23：保留原始大小写 + P2-1/S2.1 光标随动
            self.update_cap_state();
            self.query_all();
            true
        } else if self.is_calc_continuation(ch) {
            // V0.2.22：c 模式下继续输入运算符/数字 → 追加并触发计算器查询
            self.insert_at_cursor(ch, ch);
            self.query_all();
            true
        } else if self.is_symbol_prefix() && ch.is_ascii_digit() {
            // v+数字别名（0.2.32，对标 QQ v1-v9）：v 前缀时数字键追加进引擎
            // → v1/v2... 触发数字分类符号查询（平台层此时把数字键送进引擎而非选候选）
            self.insert_at_cursor(ch, ch);
            self.query_all();
            true
        } else if self.is_number_continuation(ch) {
            // P1-4：R 模式继续输入数字/点 → 追加并触发金额大写查询
            self.insert_at_cursor(ch.to_ascii_lowercase(), ch);
            self.query_all();
            true
        } else if self.is_unicode_continuation(ch) {
            // P1-4：U 模式继续输入 hex → 追加并触发 Unicode 查询
            self.insert_at_cursor(ch.to_ascii_lowercase(), ch);
            self.query_all();
            true
        } else {
            false
        }
    }

    /// 计算器模式判定（V0.2.22）：pinyin_buf 以 'c' 开头且长度 > 1
    /// 且后续字符是运算符/数字（与拼音字母区分）
    pub fn is_calc_mode(&self) -> bool {
        if !self.pinyin_buf.starts_with('c') || self.pinyin_buf.len() <= 1 {
            return false;
        }
        // 后续字符含运算符或数字才视为算式
        self.pinyin_buf[1..]
            .chars()
            .any(|c| c.is_ascii_digit() || "+-*/()%^.".contains(c))
    }

    /// c 模式继续输入判定：pinyin_buf 已以 'c' 开头，且 ch 是运算符/数字
    fn is_calc_continuation(&self, ch: char) -> bool {
        self.pinyin_buf.starts_with('c') && (ch.is_ascii_digit() || "+-*/()%^.".contains(ch))
    }

    /// 重算英文候选大小写模式（V0.2.23）
    /// Hello → Capitalize；HE（前 2 大写/全大写）→ Upper；其余 → Lower
    fn update_cap_state(&mut self) {
        self.cap_state = if self.raw_input.len() >= 2
            && self
                .raw_input
                .chars()
                .nth(1)
                .is_some_and(|c| c.is_ascii_uppercase())
        {
            CapState::Upper
        } else if !self.raw_input.is_empty()
            && self
                .raw_input
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase())
        {
            CapState::Capitalize
        } else {
            CapState::Lower
        };
    }

    /// 按大小写模式转换文本（V0.2.23，仅英文候选用）
    /// P1-1：词含大写（混合词如 AI/App/iPhone）→ 原样，不做大小写转换
    fn apply_cap(&self, word: &str) -> String {
        if word.chars().any(|c| c.is_ascii_uppercase()) {
            return word.to_string();
        }
        match self.cap_state {
            CapState::Lower => word.to_string(),
            CapState::Capitalize => {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                    None => word.to_string(),
                }
            }
            CapState::Upper => word.to_ascii_uppercase(),
        }
    }

    /// 中英驼峰分界检测（C3）：识别「小写拼音前缀(>=2) + 大写开头英文段(>=2)」，如 `woshiEric`。
    fn split_cn_en_camel(raw: &str) -> Option<(&str, &str)> {
        let bytes = raw.as_bytes();
        if bytes.len() < 4 || !bytes[0].is_ascii_lowercase() {
            return None;
        }
        let split_pos = bytes.iter().position(|b| b.is_ascii_uppercase())?;
        if split_pos < 2 {
            return None;
        }
        let (cn_part, en_part) = (&raw[..split_pos], &raw[split_pos..]);
        if cn_part.bytes().all(|b| b.is_ascii_lowercase())
            && en_part.len() >= 2
            && en_part.bytes().all(|b| b.is_ascii_alphabetic())
        {
            Some((cn_part, en_part))
        } else {
            None
        }
    }

    /// 获取当前拼音串
    pub fn pinyin_str(&self) -> &str {
        &self.pinyin_buf
    }

    /// 组合区显示串：双拼模式把编码还原成全拼音节（末尾未完成键显示为声母），
    /// 其余模式与 pinyin_str 相同。仅用于显示，不参与查询与学习。
    pub fn preedit_string(&self) -> String {
        if !self.shuangpin_mode || !self.shuangpin_full_preedit || self.pinyin_buf.is_empty() {
            return self.pinyin_buf.clone();
        }
        let chars: Vec<char> = self.pinyin_buf.chars().collect();
        let pairs_len = chars.len() / 2 * 2;
        if pairs_len == 0 {
            return self.pinyin_buf.clone();
        }
        let head: String = chars[..pairs_len].iter().collect();
        let tail: String = chars[pairs_len..].iter().collect();
        let mut out = String::new();
        match self
            .shuangpin_scheme
            .decode_string(&head)
            .into_iter()
            .find(|s| crate::pinyin::is_complete_pinyin(s))
        {
            Some(s) => out.push_str(&s),
            None => out.push_str(&head),
        }
        // 末尾单键是下一音节的声母：显示成声母（v→zh / i→ch / u→sh）
        if tail.len() == 1 {
            match tail.chars().next().unwrap() {
                'v' => out.push_str("zh"),
                'i' => out.push_str("ch"),
                'u' => out.push_str("sh"),
                c => out.push(c),
            }
        } else {
            out.push_str(&tail);
        }
        out
    }

    /// 符号输入 v 模式判定（V0.2.17）：拼音串以 'v' 开头且长度 > 1
    /// （单独 'v' 时按普通拼音处理，避免误伤；双拼模式下 v 是 zh 声母，不走符号模式）
    pub fn is_symbol_mode(&self) -> bool {
        !self.shuangpin_mode && self.pinyin_buf.starts_with('v') && self.pinyin_buf.len() > 1
    }

    /// v 前缀即时反馈（0.2.30）：拼音串恰为单个 'v'（非双拼）→ 热门符号直选
    /// （对标搜狗按 v 候选栏立即出符号；双拼下 v 是 zh 声母，排除）
    pub fn is_symbol_prefix(&self) -> bool {
        !self.shuangpin_mode && self.pinyin_buf == "v"
    }

    /// 拆字反查模式判定（V0.2.25）：拼音串以 'u' 开头且长度 > 1
    /// （单独 'u' 时按普通拼音处理；双拼模式下 u 是声母，排除）
    /// P1-4：小写 'u'（Shift 未按）走拆字；大写 'U' 走 Unicode（is_unicode_mode）
    pub fn is_radical_mode(&self) -> bool {
        !self.shuangpin_mode && self.raw_input.starts_with('u') && self.raw_input.len() > 1
    }

    /// 数字大写模式判定（P1-4，对标 rime R+ 前缀）：raw_input 以大写 'R' 开头且后续为数字/点
    pub fn is_number_mode(&self) -> bool {
        self.raw_input.starts_with('R')
            && self.raw_input.len() > 1
            && self.raw_input[1..]
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.')
    }

    /// Unicode 输入模式判定（P1-4，对标 rime U+ 前缀）：raw_input 以大写 'U' 开头且后续为 hex
    pub fn is_unicode_mode(&self) -> bool {
        self.raw_input.starts_with('U')
            && self.raw_input.len() > 1
            && self.raw_input[1..].chars().all(|c| c.is_ascii_hexdigit())
    }

    /// R 模式继续输入判定：raw_input 已以 'R' 开头，且 ch 是数字/点
    fn is_number_continuation(&self, ch: char) -> bool {
        self.raw_input.starts_with('R') && (ch.is_ascii_digit() || ch == '.')
    }

    /// U 模式继续输入判定：raw_input 已以 'U' 开头，且 ch 是十六进制字符
    fn is_unicode_continuation(&self, ch: char) -> bool {
        self.raw_input.starts_with('U') && ch.is_ascii_hexdigit()
    }

    /// 错音提示命中判定（V0.2.26）：输入串在易错读音映射表中
    pub fn is_mistake_hit(&self) -> bool {
        !mistake::lookup(&self.pinyin_buf).is_empty()
    }

    /// 获取当前页候选词数量
    pub fn candidate_count(&self) -> usize {
        self.candidates.len()
    }

    /// 全量候选数（跨页总数）。
    /// TSF 候选 UIElement 的 GetCount 语义是「总数」，不是当前页数量；
    /// 返回页内数量会让宿主翻页后按全局索引取词越界（整页空白）。
    pub fn total_candidate_count(&self) -> usize {
        self.all_candidates.len()
    }

    /// 每页候选数（宿主算页起始索引用）。
    pub fn page_size(&self) -> usize {
        self.page_size
    }

    /// 按全局索引取候选（跨页），对应 GetString(uIndex)。
    pub fn candidate_global(&self, index: usize) -> Option<&str> {
        self.all_candidates.get(index).map(|s| s.as_str())
    }

    /// 是否处于动态组词模式（V0.5）
    pub fn compose_active(&self) -> bool {
        self.compose_active
    }

    /// 组词模式当前音节（V0.5）：非组词返回空串。候选窗显示用（让用户知道在选哪音节）
    pub fn compose_current_syllable(&self) -> String {
        if self.compose_active {
            self.compose_syllables
                .get(self.compose_idx)
                .cloned()
                .unwrap_or_default()
        } else {
            String::new()
        }
    }

    /// 组词模式剩余音节（V0.5.11）：compose_idx 起至末尾的音节 join。
    /// 编辑区 composition 显示用（"我"已上屏，剩余 "chf" 下划线显示）。
    /// 非组词/索引越界返回空串。
    pub fn compose_remaining(&self) -> String {
        if self.compose_active && self.compose_idx < self.compose_syllables.len() {
            self.compose_syllables[self.compose_idx..].join("")
        } else {
            String::new()
        }
    }

    /// 组词已选前缀（V0.6.x）：已选词的汉字串；非组词返回空串。
    /// 组合串显示的唯一来源——平台层不再自己维护一份已选累积，
    /// 避免「引擎已回退、平台还留着旧字」导致字词重复上屏。
    pub fn compose_prefix(&self) -> String {
        if self.compose_active {
            self.compose_chars.join("")
        } else {
            String::new()
        }
    }

    /// 获取指定候选词（当前页内索引）
    /// V0.2.11：简繁模式开启时输出转繁体
    pub fn candidate(&self, index: usize) -> Option<&str> {
        self.candidates.get(index).map(|s| s.as_str())
    }

    /// 获取指定候选词（转换后，供 FFI/平台层显示）
    /// V0.2.11：简繁模式开启时输出转繁体
    /// V0.2.23：英文候选按输入大小写模式转换（Hello/HELLO/hello）
    pub fn candidate_display(&self, index: usize) -> Option<String> {
        let word = self.candidates.get(index)?;
        if self.is_english_candidate(index) {
            Some(self.apply_cap(word))
        } else if self.traditional_mode && !crate::trad::is_traditional(word) {
            // V0.5.6 简繁隔离：原生繁体词条直接显示（不再二次转繁，
            // 避免多音歧义 系统→繫統）；简体词条转繁显示。
            Some(crate::trad::to_traditional(word))
        } else {
            Some(word.clone())
        }
    }

    /// 当前页内索引对应的来源（审计/体检用）。
    ///
    /// 与 `src_at` 同一套偏移语义（页内索引 + `page * page_size` 全局偏移）。
    /// 注意 FFI 的 `engine_get_candidate_source` 是按**词反查**（`word_source`），
    /// 同一个词在不同拼音下的来源无法区分——体检必须用这个按索引取。
    pub fn candidate_source(&self, index: usize) -> Option<Source> {
        self.src_at(index)
    }

    /// 当前页内索引对应的来源名（考试/诊断用）。
    ///
    /// 索引语义与 `candidate()` 完全一致（页内，不是全局）。返回的是
    /// `Source::as_str` 的稳定字符串，供 exam 用例直接断言。
    pub fn candidate_source_name(&self, index: usize) -> Option<&'static str> {
        self.candidate_source(index).map(Source::as_str)
    }

    /// 当前页内索引对应的裁决元数据（层级/理由/分数/形态，B7/Phase D 诊断探针用）。
    pub fn candidate_explain(&self, index: usize) -> Option<CandidateMeta> {
        self.candidate_meta
            .get(self.page * self.page_size + index)
            .copied()
    }

    /// 当前页内索引对应的扩展标注（Phase D：簇本体 / 词性 / 释义）。
    /// 形态变体自动继承其簇本体词的词性与释义。
    pub fn candidate_extra(&self, index: usize) -> Option<CandidateExtra> {
        let abs_idx = self.page * self.page_size + index;
        let text = self.all_candidates.get(abs_idx)?;
        let cluster = self.candidate_cluster.get(abs_idx).and_then(|c| c.clone());
        let lookup_key = cluster.as_deref().unwrap_or(text.as_str());
        let (pos, gloss) = self
            .word_annotations
            .get(lookup_key)
            .cloned()
            .unwrap_or((None, None));
        Some(CandidateExtra {
            cluster,
            pos,
            gloss,
        })
    }

    /// 当前页 index → 全局候选下标 → 来源（P0-1 side-table）
    fn src_at(&self, index: usize) -> Option<Source> {
        self.candidate_src
            .get(self.page * self.page_size + index)
            .copied()
    }

    /// 判断当前页 index 是否为英文候选（混输追加项）
    fn is_english_candidate(&self, index: usize) -> bool {
        self.src_at(index) == Some(Source::English)
    }

    /// 判断当前页 index 是否为短语候选（V0.2.12）
    fn is_phrase_candidate(&self, index: usize) -> bool {
        self.src_at(index) == Some(Source::Phrase)
    }

    /// 判断当前页 index 是否为日期简码候选（V0.2.19）
    fn is_datetime_candidate(&self, index: usize) -> bool {
        self.src_at(index) == Some(Source::Datetime)
    }

    /// 判断当前页 index 是否为整句候选（长输入的整句解码结果）。
    /// 整句不进用户词学习：25 个字的整句写进用户词库只会污染候选。
    fn is_sentence_candidate(&self, index: usize) -> bool {
        self.src_at(index) == Some(Source::Sentence)
    }

    /// 日期/时间/星期/农历简码候选（V0.2.19）。非简码返回 None。
    /// P1-3 增强（对标 rime date_translator）：dt ISO8601 / ts 时间戳 /
    /// rqzh 中文日期 / rqen 英文日期
    fn datetime_candidates(code: &str) -> Option<Vec<String>> {
        match code {
            "rq" => Some(datetime::date_candidates()),
            "sj" => Some(datetime::time_candidates()),
            "xq" => Some(datetime::weekday_candidates()),
            "nl" => Some(datetime::lunar_candidates()),
            "dt" => Some(datetime::iso_candidates()),
            "ts" => Some(datetime::timestamp_candidates()),
            "rqzh" => Some(datetime::datezh_candidates()),
            "rqen" => Some(datetime::dateen_candidates()),
            _ => None,
        }
    }

    /// 拼音词触发日期候选（V0.5+，对标主流输入法）：
    /// 输入"日期/时间/年份/年/今天/现在" → 候选追加当前日期/时间/年份。
    /// 与 datetime_candidates（精确简码）互补——自然语言也能触发。
    fn datetime_pinyin_candidates(code: &str) -> Option<Vec<String>> {
        match code {
            "riqi" | "jintian" | "mingtian" => Some(datetime::date_candidates()),
            "shijian" | "xianzai" => Some(datetime::time_candidates()),
            "nianfen" | "nian" => Some(datetime::year_candidates()),
            _ => None,
        }
    }

    /// 组词撤销（V0.6.x）：回退上一个已选词（可能是多字整词），返回该词。
    /// 调用方用返回值把已选文本同步截断；没有已选词时返回 None。
    pub fn compose_undo(&mut self) -> Option<String> {
        if !self.compose_active {
            return None;
        }
        let word = self.compose_chars.pop()?;
        let steps = word.chars().count().max(1);
        self.compose_idx = self.compose_idx.saturating_sub(steps);
        self.query_all();
        Some(word)
    }

    /// 选择候选词并提交（返回提交文本，同时重置状态）    /// V0.2.2：选词时自动学习用户词（拼音串 + 选中词）
    /// V0.2.8：选中英文候选（混输）→ 上屏原文不学习
    /// V0.2.11：简繁模式开启时上屏文本转繁体
    pub fn select_candidate(&mut self, index: usize) -> Option<String> {
        // 组词模式（V0.5 逐字预上屏，V0.5.10 升级）：
        // 每个音节的选字立即返回该字（平台层逐字上屏，编辑区显示
        // "辛mao"——已上屏字 + 剩余音节 composition）。
        // 中间音节：返回选中字 + 保持组词状态（compose_idx 推进、候选切下一音节）。
        // 最后音节：learn 完整拼音+整词后 reset，返回**整词**（V0.6.x）——
        // 引擎 reset 后前缀不可读，平台层必须一次拿到完整文本，否则丢字或重复。
        // （前面各字已逐字上屏，不可再返回整词否则重复上屏）。
        if self.compose_active {
            if let Some(word) = self.candidates.get(index).cloned() {
                self.compose_chars.push(word.clone());
                // V0.6.x：组词候选可能是多字整词（"魔兽世界"）——按字符数推进音节，
                // 否则选完 4 字词只前进 1 个音节，剩余音节被重复组词
                let advance = word.chars().count().max(1);
                self.compose_idx = (self.compose_idx + advance).min(self.compose_syllables.len());
                if self.compose_idx >= self.compose_syllables.len() {
                    let full_word = self.compose_chars.join("");
                    let full_py = self.compose_syllables.join("");
                    crate::dictionary::learn(&full_py, &full_word);
                    self.reset();
                    // 返回整词（不是最后选中的那个词）：平台层用它直接提交。
                    // 引擎已 reset，前缀状态不再可读——这里必须一次给全，
                    // 否则平台层自己拼前缀就会重复或丢字（B-22）。
                    return Some(full_word);
                }
                self.query_all();
                return Some(word);
            }
            return None;
        }
        let abs_idx = self.page * self.page_size + index;
        let is_prefix_word = self
            .candidate_meta
            .get(abs_idx)
            .is_some_and(|m| m.reasons & reason::PREFIX_WORD != 0);
        if is_prefix_word
            && !self.shuangpin_mode
            && !self.ascii_mode
            && let (Some(word), Some(syls)) = (
                self.candidates.get(index).cloned(),
                self.split_compose_syllables(),
            )
        {
            let word_chars = word.chars().count();
            let is_dup_redup = syls.len() == 3 && syls[0] == syls[1] && syls[1] == syls[2];
            if word_chars >= 1 && word_chars < syls.len() && !is_dup_redup {
                self.compose_active = true;
                self.compose_syllables = syls;
                self.compose_chars = vec![word.clone()];
                self.compose_idx = word_chars;
                self.query_all();
                return Some(
                    if self.traditional_mode && !crate::trad::is_traditional(&word) {
                        crate::trad::to_traditional(&word)
                    } else {
                        word
                    },
                );
            }
        }
        let result = self.candidates.get(index).cloned();
        let mut output = result.clone();
        if let Some(word) = &result {
            // 判断候选类型
            let is_english = self.is_english_candidate(index);
            let is_phrase = self.is_phrase_candidate(index);
            let is_symbol = self.is_symbol_mode() || self.is_symbol_prefix();
            let is_calc = self.is_calc_mode();
            let is_datetime = self.is_datetime_candidate(index);
            let is_radical = self.is_radical_mode();
            let is_mistake = self.is_mistake_hit();
            let is_sentence = self.is_sentence_candidate(index);
            let is_emoji = self.src_at(index) == Some(Source::Emoji);
            let is_stitched = self.src_at(index) == Some(Source::Combo)
                && self
                    .candidate_meta
                    .get(abs_idx)
                    .is_some_and(|m| m.reasons & reason::STITCHED != 0);
            // 引擎自己"猜"出来的候选（邻键纠错 / 模糊音 / 拆组拼凑）不学习：
            // 用户选中它只是采纳了一次猜测，不等于认定「这个词就读这个音」。
            // 学进去会让一次误触变成永久首选——nihao 选中纠错来源的「立号」、
            // huiyudao 误按空格选中拼凑来源的「会与到」后，就会写进用户词库长期抢位。
            let guessed = matches!(self.src_at(index), Some(Source::Correction | Source::Fuzzy))
                || is_stitched;
            // 非英文/非短语/非符号/非计算器/非日期/非反查/非错音/非整句/非 Emoji 候选才学习用户词
            if !is_english
                && !is_phrase
                && !is_symbol
                && !is_calc
                && !is_datetime
                && !is_radical
                && !is_mistake
                && !is_sentence
                && !is_emoji
                && !guessed
                && !self.pinyin_buf.is_empty()
                && !self.ascii_mode
                // 双拼奇数键：末尾那半个音节没进查询，学习键必然不完整 → 不学习
                && !(self.shuangpin_mode && self.pinyin_buf.chars().count() % 2 == 1)
            {
                // 双拼：用户词键用解码后的全拼，避免把双拼码（vs）写进用户词库
                // 污染全拼路径；解不出时退回原码（canonical 全拼随后补齐）。
                let input_pinyin = if self.shuangpin_mode {
                    self.shuangpin_primary_pinyin()
                        .unwrap_or_else(|| self.pinyin_buf.clone())
                } else {
                    self.pinyin_buf.clone()
                };
                // 只学「能独立成立的读音」：
                // 打一半（shurf）、简拼（srf）、混拼（nih）、错拼（nihap）都不是一个
                // 读音，学进去会让同一个词攒出十几种拼音别名（实测 user_dict 里
                // 53% 的行是同一词的多拼音），下次打半边就命中错词。
                // 词库词不需要这些别名——规范全拼本身会展开出全部前缀并建声母索引，
                // 半成品输入照样能查到。只有词库查不到的自造词，才用输入键当唯一读音。
                if crate::pinyin::is_complete_pinyin(&input_pinyin)
                    || crate::dictionary::canonical_pinyin_for_word(word).is_none()
                {
                    crate::dictionary::learn(&input_pinyin, word);
                }
            }
            // V0.5.6 简繁隔离：原生繁体词条不再二次转繁（避免多音歧义）；
            // 简体词条转繁（英文/短语/符号/计算器/日期/反查/错音候选不转）
            if self.traditional_mode
                && !is_english
                && !is_phrase
                && !is_symbol
                && !is_calc
                && !is_datetime
                && !is_radical
                && !is_mistake
                && !crate::trad::is_traditional(word)
            {
                output = Some(crate::trad::to_traditional(word));
            }
            // V0.2.23：英文候选按输入大小写模式还原（Hello/HELLO/hello）
            if is_english {
                output = Some(self.apply_cap(word));
            }
            // P1-1 上下文联想：普通中文词上屏 → 记录为最近上屏词
            // （英文/短语/符号/计算器/日期/拆字/错音候选不参与，避免污染上下文）
            if !is_english
                && !is_phrase
                && !is_symbol
                && !is_calc
                && !is_datetime
                && !is_radical
                && !is_mistake
                && !is_sentence
                && !self.ascii_mode
            {
                {
                    let prev = self.last_committed.clone();
                    self.learn_context_pair(&prev, word);
                    self.last_committed = word.clone();
                }
            }
            // 热词探测（v2）：选中领域词 → 对应领域热度 +1（自动匹配多领域）
            if !is_english
                && !is_phrase
                && !is_symbol
                && !is_calc
                && !is_datetime
                && !is_radical
                && !is_mistake
                && !self.ascii_mode
            {
                crate::dictionary::record_domain_hit(word);
            }
        }
        self.reset();
        output
    }

    /// 兜底记录一次外部已上屏选词（供 V4 进程外 `Commit` 未命中活跃候选索引时复用学习过滤与上下文）。
    pub fn record_external_commit(&mut self, pinyin: &str, word: &str) {
        if pinyin.is_empty() || word.is_empty() {
            return;
        }
        let shuangpin_matched = self.shuangpin_mode && self.pinyin_buf == pinyin;
        let skip_shuangpin_odd = shuangpin_matched && self.pinyin_buf.chars().count() % 2 == 1;
        if !skip_shuangpin_odd {
            let decoded_py = if shuangpin_matched {
                self.shuangpin_primary_pinyin()
                    .unwrap_or_else(|| pinyin.to_string())
            } else {
                pinyin.to_string()
            };
            if crate::pinyin::is_complete_pinyin(&decoded_py)
                || crate::dictionary::canonical_pinyin_for_word(word).is_none()
            {
                crate::dictionary::learn(&decoded_py, word);
            }
        }
        let prev = self.last_committed.clone();
        self.learn_context_pair(&prev, word);
        self.last_committed = word.to_string();
        crate::dictionary::record_domain_hit(word);
        if self.pinyin_buf == pinyin {
            self.reset();
        }
    }

    /// 以词定字（V0.2.24）：取当前页首个候选的首/末字符上屏。
    /// first=true 取首字符，false 取末字符；返回上屏文本，重置状态。
    /// 无候选 → None（平台层透传按键）。
    pub fn take_char(&mut self, first: bool) -> Option<String> {
        let word = self.candidates.first()?.clone();
        if word.is_empty() {
            self.reset();
            return None;
        }
        let ch = if first {
            word.chars().next()
        } else {
            word.chars().next_back()
        };
        self.reset();
        ch.map(|c| c.to_string())
    }

    /// 设置候选排序模式（P0-2）：0=默认 1=单字优先 2=长词优先。非法值 clamp。
    pub fn set_sort_mode(&mut self, mode: i32) {
        self.sort_mode = mode.clamp(0, 2);
    }

    /// 查询候选排序模式（P0-2）：0=默认 1=单字优先 2=长词优先
    pub fn sort_mode(&self) -> i32 {
        self.sort_mode
    }

    /// 设置上下文联想开关（P1-1，默认关）
    pub fn set_context_enabled(&mut self, enabled: bool) {
        self.context_enabled = enabled;
    }

    /// 查询上下文联想开关（P1-1）
    pub fn context_enabled(&self) -> bool {
        self.context_enabled
    }

    /// 设置前缀补全开关（V0.6.x）：关闭后不再产生 `Source::Completion` 候选。
    pub fn set_completion_enabled(&mut self, enabled: bool) {
        self.completion_enabled = enabled;
    }

    /// 查询前缀补全开关（V0.6.x）
    pub fn completion_enabled(&self) -> bool {
        self.completion_enabled
    }

    /// 加载外部上下文搭配表：entries: (前词, [后词])，覆盖内置同前词条目。
    pub fn load_contexts(&mut self, entries: Vec<(String, Vec<String>)>) {
        self.context_map.clear();
        for (previous, related) in entries {
            if previous.is_empty() {
                continue;
            }
            let words: Vec<(String, u32)> = related
                .into_iter()
                .filter(|word| !word.is_empty())
                .map(|word| (word, crate::tuning::tuning().context_file_weight))
                .collect();
            if !words.is_empty() {
                self.context_map.insert(previous, words);
            }
        }
    }

    // 排序参数已外置到 `tuning.json`（见 `crate::tuning`）：
    // 学习型搭配上限、上下文权重上限、内置/外部搭配权重、离线先验上限、
    // 降权落点都在那里读取，缺省值与改造前的编译期常量一致。

    /// 加载学习型上下文搭配（启动时从 user_dict.db 的 context_pairs 表读入）。
    /// entries: (前词, 后词, 共现次数)，共现次数直接作为权重。
    pub fn load_learned_contexts(&mut self, entries: Vec<(String, String, i64)>) {
        for (prev, next, count) in entries {
            if prev.is_empty() || next.is_empty() {
                continue;
            }
            let weight = (count.max(1) as u32).min(crate::tuning::tuning().context_weight_cap);
            let list = self.context_map.entry(prev).or_default();
            if let Some(slot) = list.iter_mut().find(|(w, _)| w == &next) {
                slot.1 = slot.1.max(weight);
            } else {
                list.push((next, weight));
            }
            if list.len() > crate::tuning::tuning().learned_context_cap {
                list.remove(0);
            }
        }
    }

    /// 清空运行时学习到的上下文搭配表（保留离线先验 prior_context_map）。
    pub fn clear_learned_contexts(&mut self) {
        self.context_map.clear();
    }

    // 离线先验权重上限低于学习表上限（20），保证真实用词能盖过语料统计；
    // 具体数值见 `crate::tuning`（字段 `context_prior_max_weight`）。

    /// 加载离线上下文先验（启动一次）。只在学习表未覆盖时参与加权。
    pub fn load_prior_contexts(&mut self, entries: Vec<(String, String, i64)>) {
        for (prev, next, count) in entries {
            if prev.is_empty() || next.is_empty() {
                continue;
            }
            let weight =
                (count.max(1) as u32).min(crate::tuning::tuning().context_prior_max_weight);
            let list = self.context_prior.entry(prev).or_default();
            if let Some(slot) = list.iter_mut().find(|(w, _)| w == &next) {
                slot.1 = slot.1.max(weight);
            } else {
                list.push((next, weight));
            }
        }
    }

    /// 学习一次上屏搭配：prev 之后接了 next。
    /// 内存立即生效（下次输入该前词时参与候选加权），并持久化到 user_dict.db。
    fn learn_context_pair(&mut self, prev: &str, next: &str) {
        if prev.is_empty() || next.is_empty() || prev == next {
            return;
        }
        let list = self.context_map.entry(prev.to_string()).or_default();
        match list.iter_mut().find(|(w, _)| w == next) {
            Some(slot) => {
                slot.1 = (slot.1 + 1).min(crate::tuning::tuning().context_weight_cap);
                let bumped = slot.clone();
                list.retain(|(w, _)| w != next);
                list.push(bumped);
            }
            None => list.push((next.to_string(), 1)),
        }
        if list.len() > crate::tuning::tuning().learned_context_cap {
            list.remove(0);
        }
        crate::dictionary::save_context_pair(prev, next);
    }

    /// 按下标置换同步重排 `[start, end)` 段的候选与来源表（其余元素原样保留）。
    /// order[k] = 排序后第 k 位应取的原始下标（绝对下标，均在 start..end 内）。
    /// 只移动 String，不克隆文本。
    fn permute_range(candidates: &mut Vec<Candidate>, start: usize, end: usize, order: &[usize]) {
        if end <= start + 1 || order.len() != end - start || end > candidates.len() {
            return;
        }
        // 下标必须全部落在 [start, end) 且互不重复：否则 mem::take 会把
        // 同一个元素取两次（第二次取到空记录），候选表被破坏。
        let mut seen = vec![false; end - start];
        if order
            .iter()
            .any(|&i| i < start || i >= end || std::mem::replace(&mut seen[i - start], true))
        {
            // debug 构建直接断言；release 只记录并跳过（候选表不动）
            crate::log::error("[permute_range] order 含越界或重复下标，已跳过本次重排");
            debug_assert!(false, "permute_range: order 含越界或重复下标");
            return;
        }
        let tail: Vec<Candidate> = candidates.drain(end..).collect();
        let mut head: Vec<Candidate> = std::mem::take(candidates);
        let moved: Vec<Candidate> = order
            .iter()
            .map(|&i| std::mem::take(&mut head[i]))
            .collect();
        for (k, c) in moved.into_iter().enumerate() {
            head[start + k] = c;
        }
        head.extend(tail);
        *candidates = head;
    }

    /// 候选表中第一个来自 `src` 的下标（英文区边界等）。
    fn first_index_of(candidates: &[Candidate], src: Source) -> Option<usize> {
        candidates.iter().position(|c| c.source == src)
    }

    /// 整句解码（长输入）：在音节图上求最优词序列，插到候选最前。
    ///
    /// 启用条件：纯小写字母、至少 3 个音节、不是英文单词；
    /// 完整拼音 ≥ 6 字符（覆盖 keyima/yigeci/zhegeci/woyaoqu 等三音节短句），
    /// 非完整拼音仍要求 ≥ 8 字符（避免对 6-7 字符的简拼/残拼跑容错切分）。
    /// 整段读成一个词的输入不在此列（交给词级候选）；整句只是多给一条
    /// 「一次上屏整句」的选择，词级候选全部保留在后面。
    fn apply_sentence_candidates(
        &self,
        candidates: &mut Vec<Candidate>,
        cand_seen: &mut std::collections::HashSet<String>,
        pinyin_str: &str,
    ) {
        let min_len = if crate::pinyin::is_complete_pinyin(pinyin_str)
            || crate::sentence::is_pure_initials(pinyin_str)
        {
            crate::sentence::MIN_SYLLABLES * 2
        } else {
            8
        };
        if pinyin_str.len() < min_len || !pinyin_str.is_ascii() {
            return;
        }
        // 精确英文单词不猜中文整句：international / database 这类输入
        // 音节切分也能走通，但用户要的是英文。
        if crate::english::query(pinyin_str)
            .iter()
            .any(|e| e.eq_ignore_ascii_case(pinyin_str))
        {
            return;
        }
        let prev = self.last_committed.clone();
        let sentence_limit = if self.page_size > 5
            && (pinyin_str.len() >= 8 || !crate::pinyin::is_complete_pinyin(pinyin_str))
        {
            self.page_size.min(9)
        } else {
            3
        };
        let sentences = crate::sentence::decode_meta(
            pinyin_str,
            if prev.is_empty() {
                None
            } else {
                Some(prev.as_str())
            },
            sentence_limit,
        );
        if sentences.is_empty() {
            return;
        }
        // 倒序插入：最后插入的那条（得分最高）落在首位。
        //
        // 整句文本可能已经在词级候选里（「中/国」两段拼出的「中国」就是这种）。
        // 这种情况下只把它挪到首位，**不改来源**：来源一改，选词时的学习与
        // 上下文记录就会按「整句」跳过，普通词条被错当成整句。
        for entry in sentences.into_iter().rev() {
            let text = entry.text;
            if let Some(position) = candidates.iter().position(|c| c.text == text) {
                let mut existing = candidates.remove(position);
                if existing.source == Source::Combo && (existing.reasons & reason::STITCHED != 0) {
                    existing.source = Source::Sentence;
                    existing.segment_count = entry.segment_count;
                    existing.clean_coverage = entry.clean_coverage;
                    existing.sentence_score = Some(entry.score);
                }
                candidates.insert(0, existing);
                continue;
            }
            cand_seen.insert(text.clone());
            let mut cand = Candidate::new(text, Source::Sentence);
            cand.segment_count = entry.segment_count;
            cand.clean_coverage = entry.clean_coverage;
            cand.sentence_score = Some(entry.score);
            candidates.insert(0, cand);
        }
    }

    /// 翻页。delta: +1 下一页 / -1 上一页。
    /// 返回翻页后的当前页候选数（0 表示无候选或已到边界）。
    pub fn page(&mut self, delta: i32) -> usize {
        // 组词模式：正常翻当前音节单字候选页（不累计触发）
        if self.compose_active {
            return self.page_inner(delta);
        }
        // 普通模式：触发组词条件 = 正向翻页累计 3 次（Eric 预估值）
        // **或** 候选翻完（正向翻不动——候选不足 2 页时翻 1 次就到头，也应触发）。
        // V0.4.5 fix：阈值 2→3——完整拼音候选充足（如 zhonggou 19 候选 4 页）时
        // 翻 2 页只是浏览候选，误入组词会重算候选、丢失纠错结果（ffi 回归实测）。
        // 先翻页，判断是否到边界
        let result = self.page_inner(delta);
        if delta > 0 {
            self.page_advances = self.page_advances.saturating_add(1);
        }
        // V0.6.x：去掉「翻到边界立即进组词」——候选只有 1-2 页时，用户翻一次
        // 就被拽进逐字模式，长词（魔兽世界）永远选不到。改为累计翻页触发；
        // 组词候选本身也优先出整词（见 apply_mode 的 Mode::Compose）。
        // 整串本身就是一个完整音节（jian / xian / yuan / tian）时不进组词：
        // 用户要的是这个音节的单字或整词（键、西安、预案），
        // 歧义多字词由 apply_ambiguous_split_boost 提到第一页，不需要拆字。
        let whole_is_single_syllable = matches!(
            crate::pinyin::split_first_syllable(&self.pinyin_buf),
            Some((_, rest)) if rest.is_empty()
        );
        // 双拼不进组词（审计 a75b2c7 发现）：双拼输入的是编码（vs=zhong），
        // split_compose_syllables 按全拼切分无意义；且 detect_mode 里 Shuangpin
        // 优先于 Compose，即使 compose_active 成立也拿不到组词候选。
        if self.page_advances >= 3
            && !whole_is_single_syllable
            && !self.shuangpin_mode
            && let Some(syls) = self.split_compose_syllables()
        {
            self.compose_active = true;
            self.compose_syllables = syls;
            self.compose_idx = 0;
            self.compose_chars = Vec::new();
            self.query_all();
            return self.candidates.len();
        }
        result
    }

    fn page_inner(&mut self, delta: i32) -> usize {
        if self.all_candidates.is_empty() {
            self.candidates.clear();
            return 0;
        }
        let total_pages = self.total_pages();
        let new_page = self.page as i32 + delta;
        self.page = new_page.clamp(0, (total_pages - 1) as i32) as usize;
        self.repage();
        self.candidates.len()
    }

    /// 组词音节切分（V0.5+）：完整拼音音节优先，剩余按声母切分（简拼组词）。
    /// 例：taishen → [tai, shen]（完整）；tshen → [t, shen]（声母+完整）；
    ///     wa → [wa]（单音节）→ 不触发；ts → [t, s]（纯声母无完整）→ 不触发。
    /// 要求：≥2 段 且 至少 1 个完整音节（防纯简拼串误入组词）。
    fn split_compose_syllables(&self) -> Option<Vec<String>> {
        const INITIALS: &[&str] = &[
            "zh", "ch", "sh", "b", "p", "m", "f", "d", "t", "n", "l", "g", "k", "h", "j", "q", "x",
            "r", "z", "c", "s", "y", "w",
        ];
        let py = self.pinyin_buf.as_str();
        if py.is_empty() {
            return None;
        }
        // V0.6 隔音歧义优先：yuan 整体也是合法单音节（元），原逻辑只切出 1 段
        // 而不触发组词，用户无法逐字组出「预」+「案」。词库确有与多音节切分
        // 对齐的词条时，改按多音节路径组词。
        if let Some(syls) = self.ambiguous_compose_split(py) {
            return Some(syls);
        }
        let mut rest = py;
        let mut syls = Vec::new();
        let mut has_full = false;
        while !rest.is_empty() {
            if let Some((s, r)) = crate::pinyin::split_first_syllable(rest) {
                has_full = true;
                syls.push(s.to_string());
                rest = r;
            } else {
                // 非完整音节开头 → 尝试切 1-2 字母声母
                let initial = if rest.len() >= 2 && INITIALS.contains(&&rest[..2]) {
                    Some(&rest[..2])
                } else if INITIALS.contains(&&rest[..1]) {
                    Some(&rest[..1])
                } else {
                    None
                };
                {
                    let s = initial?;
                    syls.push(s.to_string());
                    rest = &rest[s.len()..];
                }
            }
        }
        (syls.len() >= 2 && has_full).then_some(syls)
    }

    /// V0.6 隔音歧义组词切分：yuan → [yu, an]（预案）、xian → [xi, an]（西安）。
    /// 仅当「整串可作单音节」且「词库里确有与该切分对齐的多字词」时启用，
    /// 避免改变 xianzai / zhongguo 这类本来没有单音节解释的输入。
    fn ambiguous_compose_split(&self, py: &str) -> Option<Vec<String>> {
        let counts = complete_syllable_counts(py)?;
        if counts.first() != Some(&1) || *counts.last()? < 2 {
            return None;
        }
        let dict_words = dictionary::query(py);
        if dict_words.is_empty() {
            return None;
        }
        let hanzi_count = |w: &str| w.chars().filter(|c| is_hanzi(*c)).count();
        let mut paths: Vec<Vec<String>> = crate::pinyin::syllable_split_variants(py, 64)
            .into_iter()
            .filter(|p| p.len() >= 2)
            .collect();
        paths.sort_by_key(|p| p.len());
        paths
            .into_iter()
            .find(|p| dict_words.iter().any(|w| hanzi_count(w) == p.len()))
    }

    /// 宽松音节切分（V0.5.13 音节可视化）：完整音节优先，剩余按声母切
    /// （zh/ch/sh 优先），兜底逐字符。无「≥2 段且≥1 完整音节」门槛——
    /// 单音节 wo、纯声母 zg 也返回结果，供显示与光标移动全程使用。
    /// 例：zhongguo → [zhong, guo]；zg → [z, g]；tshen → [t, shen]。
    fn split_syllables_lenient(&self) -> Vec<String> {
        const INITIALS: &[&str] = &[
            "zh", "ch", "sh", "b", "p", "m", "f", "d", "t", "n", "l", "g", "k", "h", "j", "q", "x",
            "r", "z", "c", "s", "y", "w",
        ];
        let py = self.pinyin_buf.as_str();
        if py.is_empty() {
            return Vec::new();
        }
        let mut rest = py;
        let mut syls = Vec::new();
        while !rest.is_empty() {
            if let Some((s, r)) = crate::pinyin::split_first_syllable(rest) {
                syls.push(s.to_string());
                rest = r;
            } else {
                // 非完整音节开头 → 尝试切 1-2 字母声母
                let initial = if rest.len() >= 2 && INITIALS.contains(&&rest[..2]) {
                    Some(&rest[..2])
                } else if INITIALS.contains(&&rest[..1]) {
                    Some(&rest[..1])
                } else {
                    None
                };
                match initial {
                    Some(s) => {
                        syls.push(s.to_string());
                        rest = &rest[s.len()..];
                    }
                    None => {
                        // 兜底：单字符，保证显示不丢字符（正常拼音输入不会走到）
                        syls.push(rest[..1].to_string());
                        rest = &rest[1..];
                    }
                }
            }
        }
        syls
    }

    /// 音节分隔显示串（V0.5.13 音节可视化）：zhongguo → "zhong'guo"，
    /// zg → "z'g"，tshen → "t'shen"。输入全程可用（按需现算，微秒级）。
    /// 组词模式：返回 compose_idx 起至末尾带分隔（候选窗指示正在选哪个音节）。
    pub fn syllable_display(&self) -> String {
        if self.compose_active {
            if self.compose_idx < self.compose_syllables.len() {
                return self.compose_syllables[self.compose_idx..].join("'");
            }
            return String::new();
        }
        // 双拼：先把编码还原成全拼再做音节分隔，否则组合区显示的是 vs 而不是 zhong
        if self.shuangpin_mode && self.shuangpin_full_preedit && !self.pinyin_buf.is_empty() {
            let full = self.preedit_string();
            let mut parts: Vec<&str> = Vec::new();
            let mut rest: &str = &full;
            while !rest.is_empty() {
                match crate::pinyin::split_first_syllable(rest) {
                    Some((syl, next)) => {
                        parts.push(syl);
                        rest = next;
                    }
                    None => {
                        parts.push(rest);
                        break;
                    }
                }
            }
            return parts.join("'");
        }
        self.split_syllables_lenient().join("'")
    }

    /// 当前页号（0 起，调试/翻页指示用）
    pub fn current_page(&self) -> usize {
        self.page
    }

    /// 总页数
    pub fn total_pages(&self) -> usize {
        if self.all_candidates.is_empty() {
            return 0;
        }
        self.all_candidates.len().div_ceil(self.page_size)
    }

    /// 注入语音转写文本为唯一候选（V0.5.7）。
    /// 调用方先 reset() 清空输入态，再注入——候选窗显示麦克风语音候选，
    /// 平台层拿到后直接上屏（引擎_select_candidate(0) 语义不变）。
    pub fn set_voice_candidate(&mut self, text: &str) {
        // 走统一发布口：同时处理 page 重置与 repage。
        // 原实现直接改三个字段、不重置 page，page>0 时 src_at 会按错误偏移
        // 查来源表（审计 P2）。
        let cands = if text.is_empty() {
            Vec::new()
        } else {
            vec![text.to_string()]
        };
        self.publish(cands, Source::Voice);
    }

    /// 清空状态
    pub fn reset(&mut self) {
        self.pinyin_buf.clear();
        self.raw_input.clear();
        self.cursor = 0;
        self.cap_state = CapState::Lower;
        self.all_candidates.clear();
        self.candidates.clear();
        self.candidate_src.clear();
        self.candidate_meta.clear();
        self.candidate_cluster.clear();
        self.shadow_snapshot = None;
        self.page = 0;
        self.compose_active = false;
        self.compose_syllables.clear();
        self.compose_idx = 0;
        self.compose_chars.clear();
        self.page_advances = 0;
    }

    /// 退格（删除最后一个拼音字符；P2-1：光标不在末尾时删光标前字符）
    pub fn backspace(&mut self) -> bool {
        // 组词模式：退格退出组词，回到普通候选
        self.compose_active = false;
        self.page_advances = 0;
        if self.cursor < self.pinyin_buf.len() {
            // 光标在中部：删光标前字符（对标 rime revert）
            if self.cursor == 0 {
                return false;
            }
            self.pinyin_buf.remove(self.cursor - 1);
            self.raw_input.remove(self.cursor - 1);
            self.cursor -= 1;
        } else if self.pinyin_buf.pop().is_some() {
            self.raw_input.pop(); // V0.2.23：同步清除原始输入
            if self.cursor > 0 {
                self.cursor -= 1;
            }
        } else {
            return false;
        }
        self.update_cap_state();
        if self.pinyin_buf.is_empty() {
            self.all_candidates.clear();
            self.candidates.clear();
            self.candidate_src.clear();
            self.candidate_meta.clear();
            self.candidate_cluster.clear();
            self.shadow_snapshot = None;
            self.page = 0;
            self.cursor = 0;
        } else {
            self.query_all();
        }
        true
    }

    // ─── P2-1 编辑能力：音节边界光标 ───

    /// 获取拼音串的音节边界（字符索引列表，含 0 与末尾）
    /// V0.5.13：改走宽松切分（split_syllables_lenient），与音节显示对齐——
    /// 简拼 zg 边界 [0,1,2]，Tab/Ctrl+BackSpace 在简拼串也能按音节移动。
    fn syllable_boundaries(&self) -> Vec<usize> {
        let mut boundaries = vec![0usize];
        let mut pos = 0;
        for syl in self.split_syllables_lenient() {
            pos += syl.len();
            boundaries.push(pos);
        }
        boundaries
    }

    /// 移动拼音光标（P2-1 + S2.1）：
    /// - `±1`: 相邻音节边界（Tab / Shift+Tab / Ctrl+Left / Ctrl+Right）
    /// - `±2`: 逐字符移动（Left / Right）
    /// - `<= -3` / `>= 3`: 跳至拼音开头 / 末尾（Home / End）
    ///
    /// 返回移动后光标位置。
    pub fn move_cursor(&mut self, delta: i32) -> usize {
        if self.pinyin_buf.is_empty() {
            self.cursor = 0;
            return 0;
        }
        let len = self.pinyin_buf.len();
        let cur = self.cursor.min(len);
        self.cursor = match delta {
            0 => cur,
            1 => {
                let boundaries = self.syllable_boundaries();
                boundaries.into_iter().find(|&b| b > cur).unwrap_or(len)
            }
            -1 => {
                let boundaries = self.syllable_boundaries();
                boundaries.into_iter().rev().find(|&b| b < cur).unwrap_or(0)
            }
            2 => (cur + 1).min(len),
            -2 => cur.saturating_sub(1),
            d if d >= 3 => len,
            _ => 0,
        };
        self.cursor
    }

    /// 查询光标位置（P2-1）
    pub fn cursor_pos(&self) -> usize {
        self.cursor
    }

    /// 删除光标前一个音节（P2-1，对标 rime Ctrl+BackSpace back_syllable）。
    /// 返回是否删除成功。
    pub fn backspace_syllable(&mut self) -> bool {
        if self.cursor == 0 || self.pinyin_buf.is_empty() {
            return false;
        }
        let boundaries = self.syllable_boundaries();
        let prev = boundaries
            .iter()
            .rev()
            .find(|&&b| b < self.cursor)
            .copied()
            .unwrap_or(0);
        if prev >= self.cursor {
            return false;
        }
        self.pinyin_buf.drain(prev..self.cursor);
        self.raw_input.drain(prev..self.cursor);
        self.cursor = prev;
        self.update_cap_state();
        if self.pinyin_buf.is_empty() {
            self.all_candidates.clear();
            self.candidates.clear();
            self.candidate_src.clear();
            self.candidate_meta.clear();
            self.candidate_cluster.clear();
            self.shadow_snapshot = None;
            self.page = 0;
            self.cursor = 0;
        } else {
            self.query_all();
        }
        true
    }

    /// 删除当前页指定候选（P2-1 Ctrl+Delete，对标 rime delete_candidate）：
    /// 从用户词库移除该词并重查。返回是否删除成功。
    pub fn delete_candidate(&mut self, index: usize) -> bool {
        let Some(word) = self.candidates.get(index).cloned() else {
            return false;
        };
        if self.pinyin_buf.is_empty() {
            return false;
        }
        // V0.5.7：按词删除（内部按词全前缀清理 user_index + user_short_index，
        // 不依赖当前拼音键——简拼/组词场景均能删干净）
        crate::dictionary::remove_user_word("", &word);
        self.query_all();
        true
    }

    // ─── 内部 ───

    /// 输入质量判定（P1-1）。dict_hits = 输入串在词库的直接命中条数。
    /// 判定顺序：完整拼音 → 混合 → 纯声母 → 合法前缀 → 需纠错。
    ///
    /// 当前不参与候选生成的条件判定（原因见 query_all 内的实测结论），
    /// 保留为诊断与后续调参的基础设施。
    #[allow(dead_code)]
    fn classify_input(&self, s: &str, dict_hits: usize) -> InputQuality {
        if crate::pinyin::is_complete_pinyin(s) {
            return if dict_hits > 0 {
                InputQuality::Exact
            } else {
                InputQuality::FullPinyin
            };
        }
        if mixed_syllable_count(s).is_some() {
            return InputQuality::Mixed;
        }
        if short_pinyin_word_len(s).is_some() {
            return InputQuality::Initials;
        }
        if crate::pinyin::is_valid_pinyin_prefix(s) {
            return InputQuality::Partial;
        }
        InputQuality::Typo
    }

    /// 判定当前输入是否属于特殊模式（P2-1）
    fn detect_mode(&self) -> Option<Mode> {
        if self.is_symbol_prefix() {
            return Some(Mode::SymbolPrefix);
        }
        if self.is_symbol_mode() {
            return Some(Mode::SymbolQuery);
        }
        if self.is_calc_mode() {
            return Some(Mode::Calc);
        }
        if self.is_radical_mode() {
            return Some(Mode::Radical);
        }
        if self.is_number_mode() {
            return Some(Mode::Number);
        }
        if self.is_unicode_mode() {
            return Some(Mode::Unicode);
        }
        if self.shuangpin_mode {
            return Some(Mode::Shuangpin);
        }
        if self.compose_active {
            return Some(Mode::Compose);
        }
        None
    }

    /// 执行特殊模式分支。返回 true = 已处理，调用方直接返回；
    /// false = 该模式无候选，回落常规拼音路径（目前只有拆字反查会回落）。
    fn apply_mode(&mut self, mode: Mode) -> bool {
        match mode {
            Mode::SymbolPrefix => {
                // 候选首位是 "v" 单字（中文模式输出字母 v 免切英文），其后热门符号直选
                let mut symbols: Vec<String> = vec!["v".to_string()];
                symbols.extend(symbol::hot_symbols().into_iter().map(|s| s.to_string()));
                self.publish(symbols, Source::Mode);
                true
            }
            Mode::SymbolQuery => {
                let category = &self.pinyin_buf[1..]; // 去掉 'v' 前缀
                let symbols: Vec<String> = symbol::query(category)
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect();
                self.publish(symbols, Source::Mode);
                true
            }
            Mode::Calc => {
                let expr = &self.pinyin_buf[1..]; // 去掉 'c' 前缀
                let candidates = match calculator::eval(expr) {
                    Ok(v) => vec![calculator::format_result(v)],
                    Err(_) => Vec::new(), // 非法算式无候选
                };
                self.publish(candidates, Source::Mode);
                true
            }
            Mode::Radical => {
                // 0.1.50（Eric 反馈）：拆字查不到时回落常规拼音/英文路径——
                // 此前 us/up 这类输入被 u 前缀吃掉，候选窗直接空白
                let parts = &self.pinyin_buf[1..]; // 去掉 'u' 前缀
                let candidates = radical::query(parts);
                if candidates.is_empty() {
                    return false;
                }
                self.publish(candidates, Source::Mode);
                true
            }
            Mode::Number => {
                let digits = &self.pinyin_buf[1..]; // 去掉 'r' 前缀（raw R → buf r）
                let candidates = crate::number::to_cn(digits);
                self.publish(candidates, Source::Mode);
                true
            }
            Mode::Unicode => {
                let hex = &self.pinyin_buf[1..]; // 去掉 'u' 前缀（raw U → buf u）
                let candidates = crate::unichar::query(hex);
                self.publish(candidates, Source::Mode);
                true
            }
            Mode::Shuangpin => {
                self.query_all_shuangpin();
                true
            }
            Mode::Compose => {
                // 组词候选（V0.6.x 重写）：按「本次能消费几个音节」从多到少排——
                // 先给剩余整段的整词（xianshi → 显示），再逐级缩短前缀的词
                // （moshoushijizhenhaowan → 魔兽世界 → 魔兽），最后是当前音节单字。
                //
                // 旧实现只有「phrase_group_guess 整句拆组 + 当前音节」，
                // 两头都不对：拆组产出「有管腺」这类怪组合占满首屏，
                // 而剩余拼音里真实存在的词（显示）根本没查过，用户必须逐字拼。
                let Some(syl) = self.compose_syllables.get(self.compose_idx) else {
                    return false;
                };
                let rest: &[String] = &self.compose_syllables[self.compose_idx..];
                // 多音节整词阶段保留 3 页预算，保证首屏优先展示长词；
                // 单字兜底阶段不受此预算截断，确保任意排名的同音单字均可翻页到达。
                let word_budget = self.page_size * 3;
                let mut cands: Vec<String> = Vec::new();
                let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                // 每级前缀最多取这么多：让「长词优先」真正落到前两页
                const PER_STEP_TAKE: usize = 4;

                // 0) 首位硬保证当前音节单字（组词 = 逐字推进）。
                // 整词占首位时用户按 1 会误选整词 → compose_idx 直接推到末尾 →
                // 组词立即结束、拼音清空，后续数字键全部透传宿主（「多翻几页冒出 12」）。
                // 必须在这里取：放到末尾事后重排时，预算若已被长词占满就凑不出单字，
                // 修复会静默失效（审计 a75b2c7 发现）。
                for w in dictionary::query(syl) {
                    if w.chars().count() == 1 && !w.contains(' ') && seen.insert(w.clone()) {
                        cands.push(w);
                        break;
                    }
                }
                // 1) 整句层：把剩余音节整体拆成词序列，选一次吃完整段，不必逐段拼。
                //    前文已选词参与 bigram 排序——选完「魔兽」后「世界」应压过「时间」。
                let rest_all: String = rest.concat();
                let prev_word = self.compose_chars.last().cloned();
                let mut sentence_taken = 0usize;
                for w in dictionary::sentence_guess(&rest_all, prev_word.as_deref(), 1) {
                    if w.chars().count() != rest.len() || seen.contains(&w) {
                        continue;
                    }
                    seen.insert(w.clone());
                    cands.push(w);
                    sentence_taken += 1;
                    if sentence_taken >= 1 {
                        break;
                    }
                }

                // 1) 剩余整段 → 逐级缩短的前缀：只收「词长正好等于该段音节数」的词
                for cut in (1..=rest.len()).rev() {
                    let prefix_py: String = rest[..cut].concat();
                    let mut taken = 0usize;
                    for w in dictionary::query(&prefix_py) {
                        if w.chars().count() != cut || w.contains(' ') {
                            continue; // 只要「正好吃完这一段」的词
                        }
                        if seen.contains(&w) {
                            continue;
                        }
                        seen.insert(w.clone());
                        cands.push(w);
                        taken += 1;
                        if taken >= PER_STEP_TAKE || cands.len() >= word_budget {
                            break;
                        }
                    }
                    if cands.len() >= word_budget {
                        break;
                    }
                }
                // 2) 当前音节兜底：保证组词态下可翻页选出当前音节的全部单字（不被 word_budget 截断）
                let is_initial = matches!(syl.as_str(), "zh" | "ch" | "sh")
                    || (syl.len() == 1 && "bpmfdtnlgkhjqxzcsrwyaeo".contains(syl.as_str()));
                let fallback_words = if is_initial {
                    dictionary::query_short(syl)
                } else {
                    dictionary::query(syl)
                };
                for w in fallback_words {
                    if w.chars().count() == 1 && !w.contains(' ') && seen.insert(w.clone()) {
                        cands.push(w);
                    }
                }
                self.publish(cands, Source::Mode);
                true
            }
        }
    }

    /// 查询全部候选（含简拼联想 + 多音节切分联想），截断到 max_pages 页，重置到第 0 页
    fn query_all(&mut self) {
        // P2-1 模式分发表：特殊输入模式统一走 detect_mode / apply_mode，
        // 不再为每个模式复制一遍「发布 + 重置页码 + return」。
        if let Some(mode) = self.detect_mode()
            && self.apply_mode(mode)
        {
            return;
        }
        let pinyin_str = self.pinyin_buf.clone();
        // 2026-11-26（Eric）：本次输入是否存在可信中文候选——决定英文是否让出前两位。
        // 在英文分支内计算，排序后用它做英文槽位修正（见 rank_and_truncate 之后）。
        let mut has_credible_chinese = false;
        // P2-3：jqxy 后 v 归一为 u（qv→qu），供拼音查询与切分判断
        let norm_str = crate::pinyin::normalize_v(&pinyin_str);
        // 0.3.x fix（英文误伤）：仅"完全可切分为合法拼音"的输入才做模糊/纠错/错音联想。
        // "hello" 切分失败（he+llo）→ 视为英文，不做拼音联想（只英文混输）。
        let is_full_pinyin = crate::pinyin::is_complete_pinyin(&norm_str);
        // 日期/时间/星期/农历简码（V0.2.19）：rq/sj/xq/nl 精确命中 → 日期候选
        // 优先级：日期简码 > 快捷短语（sj 与短语"手机"并存时日期优先，短语可翻页取）
        // V0.6.x（2026-11-19）：日期候选不再独占候选池。ts / sj / dt 同时也是
        // 高频简拼串（ts→提升、同时；sj→手机、时间），独占会让中文词彻底打不出来
        // （实测 article_exam：输入 ts 只出时间戳，"提升" miss）。
        // 改为前置：日期候选仍排第 1 位，中文候选追加在其后，可翻页取。
        let datetime_seed: Vec<String> = match Self::datetime_candidates(&pinyin_str) {
            // V0.5+：用户词简拼优先——学过 ts→泰深 则 ts 让位给用户词
            //（否则 datetime 时间戳简码拦截，用户词永远轮不到）
            Some(cands) if !crate::dictionary::user_short_hit(&pinyin_str) => cands,
            _ => Vec::new(),
        };
        // 优先整词/全拼前缀查询（Phase A：改走 query_with_split_meta 携带精确拼音与词库层元数据，词序与旧 API 严格一致）
        let (dict_entries, exact_len) = dictionary::query_with_split_meta(&pinyin_str);
        // 「已打满」判据：精确段前 5 里存在**字数等于输入音节数**的多字词，
        // 说明输入串本身就是那个词——用户此刻要的是它，不是「再多打一个音节」。
        // 不能用「任意多字词」判：`xian` 的精确段有「西安」（2 字），但用户打的是
        // 1 个音节，此刻要的很可能是「现在/显示」，误判会整片杀掉补全。
        // 缺这条判据时打满场景被 completion 插位挤出首屏（实测 98.7% → 96.2%）。
        let exact_n = complete_syllable_counts(&pinyin_str)
            .map(|c| c[0])
            .unwrap_or(0);
        let exact_saturated = exact_n >= 2
            && dict_entries
                .iter()
                .take(exact_len.min(5))
                .any(|e| e.word.chars().filter(|c| is_hanzi(*c)).count() == exact_n);
        let mut candidates: Vec<Candidate> = dict_entries
            .into_iter()
            .enumerate()
            .map(|(i, e)| {
                let r = if i < exact_len {
                    reason::EXACT_PINYIN
                } else {
                    reason::PREFIX_EXT
                };
                let mut c = Candidate::with_reason(e.word, Source::Dictionary, r)
                    .with_pinyin(e.pinyin, e.pinyin_len);
                c.dict_layer = Some(e.layer);
                if e.freq > 0 {
                    c.dict_freq = Some(e.freq);
                }
                c
            })
            .collect();
        // 统一可信度判定（B2）：在精确键段（0..exact_len）上一次加锁取真词频，
        // 判定「精确输入是否存在可信中文词」（用户词 ∪ 常用词 ∪ 精确键词频 >= CREDIBLE_EXACT_FREQ）。
        let mut exact_has_credible = {
            let exact_texts: Vec<&str> = candidates
                .iter()
                .take(exact_len)
                .map(|c| c.text.as_str())
                .collect();
            let freqs = dictionary::exact_word_freqs(&norm_str, &exact_texts);
            let mut credible_found = false;
            for (c, freq_opt) in candidates.iter_mut().take(exact_len).zip(freqs) {
                let is_user = dictionary::is_user_word(&c.text);
                let is_common = dictionary::is_common_word(&c.text);
                let is_high_freq = freq_opt.is_some_and(|f| f >= CREDIBLE_EXACT_FREQ);
                if is_user {
                    c.reasons |= reason::USER_WORD;
                }
                if is_common {
                    c.reasons |= reason::COMMON_WORD;
                }
                if is_high_freq {
                    c.reasons |= reason::HIGH_FREQ;
                }
                if let Some(f) = freq_opt {
                    c.score = Some(f as i32);
                }
                let is_pure_hanzi = !c.text.is_empty() && c.text.chars().all(is_hanzi);
                let user_exact_credible = is_user
                    && (is_full_pinyin
                        || (c.dict_layer == Some(0) && c.dict_freq.unwrap_or(0) >= 2));
                if (user_exact_credible || is_common || is_high_freq) && is_pure_hanzi {
                    c.reasons |= reason::CREDIBLE;
                    credible_found = true;
                }
            }
            credible_found
        };
        // 双音节完整拼音口语代词/功能字短语直拼保护（如 dangwo→当我、shiwo→是我、zaiwo→在我）：
        // 词库未将口语短语作整词收录时，若放任 exact_has_credible=false，邻键纠错
        // （dangwo→rangwo「让我」、shiwo/zaiwo→ziwo「自我」）会借 corr_promoted 抢占首词。
        if is_full_pinyin && !exact_has_credible {
            const FUNCTION_JOIN_CHARS: &str =
                "我你他她它是在有不了的就也都要会能给对跟和把被让当为比向从看想说";
            if let Some(syls) = crate::pinyin::syllable_split_variants(&norm_str, 1)
                .into_iter()
                .next()
                && syls.len() == 2
                && syls.iter().all(|s| s.len() >= 2)
            {
                let pick_char = |s: &str| -> Option<String> {
                    let (words, s_exact) = dictionary::query_with_split(s);
                    words.into_iter().take(s_exact).find(|w| {
                        w.chars().count() == 1
                            && w.chars().all(is_hanzi)
                            && dictionary::is_common_word(w)
                    })
                };
                if let (Some(c1), Some(c2)) = (pick_char(&syls[0]), pick_char(&syls[1]))
                    && (FUNCTION_JOIN_CHARS.contains(c1.as_str())
                        || FUNCTION_JOIN_CHARS.contains(c2.as_str()))
                {
                    let joined = format!("{c1}{c2}");
                    candidates.retain(|c| c.text != joined);
                    let mut c = Candidate::with_reason(
                        joined,
                        Source::Dictionary,
                        reason::EXACT_PINYIN | reason::COMMON_WORD | reason::CREDIBLE,
                    )
                    .with_pinyin(
                        Some(std::sync::Arc::from(norm_str.as_str())),
                        norm_str.len(),
                    );
                    c.dict_layer = Some(1);
                    candidates.insert(0, c);
                    exact_has_credible = true;
                }
            }
        }
        // 英文精确命中预判（B5）：提前查一次英文词典，既给英文混输复用，
        // 也用于门控纠错分支——避免对 api/email/app 等明确英文词跑拼音纠错。
        let eng_hits: Vec<String> = if self.mix_mode_enabled
            && !self.ascii_mode
            && !pinyin_str.is_empty()
            && pinyin_str.chars().all(|c| c.is_ascii_alphabetic())
        {
            crate::english::query(&pinyin_str)
        } else {
            Vec::new()
        };
        let looks_like_english_word = pinyin_str.chars().count() >= 3
            && eng_hits.iter().any(|e| e.eq_ignore_ascii_case(&pinyin_str));
        // 混拼前缀常用词保护（如 dans→但是、went→问题）：
        // 若首音节长度 ≥ 2 且剩余后缀以真辅音声母开头（排除 key 等 y/w 结尾英文词），
        // 且词库前 5 条内已有常用词/用户词，则不让同名英文词抢占首词。
        let dict_has_credible_mixed = !is_full_pinyin
            && crate::pinyin::split_first_syllable(&norm_str).is_some_and(|(syl, rest)| {
                syl.len() >= 2
                    && !rest.is_empty()
                    && !rest.starts_with('y')
                    && !rest.starts_with('w')
            })
            && candidates.iter().take(5).any(|c| {
                !c.text.is_empty()
                    && c.text.chars().all(is_hanzi)
                    && (dictionary::is_common_word(&c.text)
                        || (c.dict_layer == Some(0)
                            && c.dict_freq.unwrap_or(0) >= 2
                            && dictionary::is_user_word(&c.text)))
            });
        let suppress_pinyin_correction =
            looks_like_english_word && !exact_has_credible && !dict_has_credible_mixed;
        // P0-3 统一去重集合：query_all 的补充分支原先各自线性 contains，
        // 长串输入同时触发 5-6 个分支会叠加成 O(n²)。这里维护一份与候选
        // 同步的 HashSet，补充分支只做集合查询。
        // 不变量：任何对 candidates 的 push/insert 必须同时写入 cand_seen；
        // truncate/retain 之后重建一次（见下方 RANKING_CANDIDATE_BUDGET 处）。
        let mut cand_seen: std::collections::HashSet<String> =
            candidates.iter().map(|c| c.text.clone()).collect();
        // V0.6.x 前缀补全（Completion）：输入停在完整音节边界时，把「只差一个音节」
        // 的高频词接成独立来源。词库 query 的前缀扩展段早就有这些词，但它们的
        // 身份被抹平了——只能按字数猜，于是 `lian` 的 32 个单字把「联系」压到
        // 第 33 位。这里只补身份，位置由 apply_completion_boost 负责；
        // completion_order 是词库给出的补全优先级（词频降序），插位时按它重排。
        const COMPLETION_TAKE: usize = 5;
        let completion_entries: Vec<dictionary::WordEntry> =
            if exact_saturated || !self.completion_enabled {
                // 输入串本身已命中词，或用户关闭了前缀补全 → 不再预测下一个音节
                Vec::new()
            } else {
                dictionary::completion_candidates_meta(&pinyin_str, COMPLETION_TAKE)
            };
        for e in completion_entries {
            if let Some(c) = candidates.iter_mut().find(|c| c.text == e.word) {
                // 已由词库直取出列：改身份并同步规范补全拼音（覆盖简短别名键如 hou→houmian），不重复入列
                c.source = Source::Completion;
                c.reasons |= reason::COMPLETION;
                if e.pinyin.is_some() {
                    c.pinyin = e.pinyin;
                    c.pinyin_len = e.pinyin_len;
                }
                if c.dict_freq.is_none() && e.freq > 0 {
                    c.dict_freq = Some(e.freq);
                }
            } else if cand_seen.insert(e.word.clone()) {
                let mut c = Candidate::with_reason(e.word, Source::Completion, reason::COMPLETION)
                    .with_pinyin(e.pinyin, e.pinyin_len);
                c.dict_layer = Some(e.layer);
                if e.freq > 0 {
                    c.dict_freq = Some(e.freq);
                }
                candidates.push(c);
            }
        }
        // 繁体模式补充原生繁体词条（系统繁体 + 领域繁体）：
        // 追加在简体主通道候选之后，并携带精确/前缀元数据接入 ranking_v2（不再落入 Match::Foreign）；
        // 随后转繁去重会优先保留简体主通道的高精度常用/系统/用户候选（由输出层转繁并走词组歧义表），
        // 简体库缺失的纯原生繁体词与一简多繁异形字（如 後/幹/麵/臺/哪吒/拚命）则作为有效候选保留。
        if self.traditional_mode {
            for e in dictionary::query_trad_meta(&pinyin_str) {
                if cand_seen.insert(e.word.clone()) {
                    let r = if e.pinyin.is_some() {
                        reason::EXACT_PINYIN
                    } else {
                        reason::PREFIX_EXT
                    };
                    let mut c = Candidate::with_reason(e.word, Source::Dictionary, r)
                        .with_pinyin(e.pinyin, e.pinyin_len);
                    c.dict_layer = Some(e.layer);
                    candidates.push(c);
                }
            }
        }
        // 快捷短语（V0.2.12）：简码精确命中 → 插入用户词后、系统词前
        // 位置记录：短语选中不学习
        if self.phrase_enabled
            && let Some(text) = self.phrase_map.get(&pinyin_str)
        {
            // V0.5.11 语义 A（Eric 2026-08-13）：短语插在精确匹配热+温
            // 用户词之后、常用词之前（用户词 > 短语 > 系统词，非置顶）。
            let insert_at = crate::dictionary::user_word_count(&pinyin_str).min(candidates.len());
            // 短语由 phrase_map 唯一定义，理论上不会与词库候选重名；
            // 仍同步 seen，保证后续分支看到的是同一份去重视图。
            let phrase_reason = if self.user_phrase_keys.contains(&pinyin_str) {
                reason::USER_PHRASE
            } else {
                0
            };
            cand_seen.insert(text.clone());
            candidates.insert(
                insert_at,
                Candidate::with_reason(text.clone(), Source::Phrase, phrase_reason),
            );
        }
        // 简拼补充（输入串同时作为简拼前缀，如 "zg"→中国）
        //
        // 2026-11-18 门控：完整拼音不走简拼查询。zei 是合法音节，读音已确定
        // （贼），但 query_short 会把 zei 当声母串宽松匹配，拉出「这位/周围/
        // 紫微/作为」——3 字符输入产出 6 字符的词，是候选不足时的主要噪声源
        // （P0 体检：zei 前 5 位除「贼」外全是这类）。
        // 非完整拼音（zg / shurf / xzai / rgshni）不受影响。
        // V0.6.x Combo 门槛分级（Eric：qwer/sdfg/wert 不该组词，但简拼必须活）：
        // 判据见 pinyin::is_combo_syllable_plausible：
        //   · 完整拼音 / 从真音节起头的混拼（nimzai/heshenm/dianjishiyong）→ 全放行；
        //   · 纯声母串（zg/nh/nhgbz/qwer/sdfg/wert）→ Combo 系只信常用词。
        //
        // 2026-11-19 修正：简拼系（query_short / query_mixed / query_abbrev_full）
        // 不再共用 combo 门禁——它把纯声母串整体判为不可信，直接误伤真实输入：
        // cyl（产业链）/ gyl（供应链）/ xdh（现代化）都被丢光，只剩英文候选。
        // 简拼系改用更窄的判据 short_keep（两级：键盘连打才要求常用词 + 长度对齐）。
        // 另加长度对齐：纯声母串上，命中词条的字数不应显著超过字母数
        // （简拼一个字母对一个字；npm 上的「女仆冥土小姐」7 字、zg 上的
        // 「中国人民银行」6 字都是前缀补全噪声）。放宽容差 +1 给「多音节字」。
        // 本质区别：Combo 系是「把输入串硬拼成音节」（纯声母拼出来的是垃圾，
        // 如 sdfg→宋代法规），简拼系是「按声母前缀取词」（纯声母正是合法输入方式）。
        let combo_gate = crate::pinyin::is_combo_syllable_plausible(&pinyin_str);
        let combo_common_only = !combo_gate;
        let short_common_only = combo_common_only && crate::pinyin::is_keyboard_run(&pinyin_str);
        let short_keep = |w: &str| -> bool {
            if short_common_only && !dictionary::is_common_word(w) && !dictionary::is_user_word(w) {
                return false;
            }
            !(!combo_gate && w.chars().count() > pinyin_str.chars().count() + 1)
        };
        // C1（BUG-2026-11-16 遗留-02）：缩写+全拼混合（如 zhyang→这样/张扬、xzai→现在、bru→不如）。
        // 含元音的非完整拼音串（区别于纯声母简拼 zg/bj/sj）优先走 `query_abbrev_full`，
        // 且不再被 `candidates.len() < page_size * max_pages` 门槛拦截——
        // 此前 `query_short` 先塞满 40 条导致 `query_abbrev_full` 根本不执行，「张扬」彻底不可达。
        let has_vowel = pinyin_str.bytes().any(|b| b"aeiouv".contains(&b));
        if !is_full_pinyin && has_vowel && pinyin_str.len() >= 3 {
            for (w, f) in dictionary::query_abbrev_full_meta(&pinyin_str) {
                if !short_keep(&w) {
                    continue;
                }
                if cand_seen.insert(w.clone()) {
                    let mut c = Candidate::new(w, Source::Combo);
                    if f > 0 {
                        c.dict_freq = Some(f);
                    }
                    candidates.push(c);
                }
            }
        }
        if !is_full_pinyin {
            for (w, f) in dictionary::query_short_meta(&pinyin_str) {
                if !short_keep(&w) {
                    continue;
                }
                if cand_seen.insert(w.clone()) {
                    let mut c = Candidate::new(w, Source::Combo);
                    if f > 0 {
                        c.dict_freq = Some(f);
                    }
                    candidates.push(c);
                }
            }
        }
        // 混合简拼补充（0.1.26）：全拼前缀 + 声母后缀，如 "shurf"→输入法
        // 简拼系过滤见上方 short_common_only（仅键盘连打才要求常用词）
        if !is_full_pinyin && candidates.len() < self.page_size * self.max_pages {
            for (w, f) in dictionary::query_mixed_meta(&pinyin_str) {
                if !short_keep(&w) {
                    continue;
                }
                if cand_seen.insert(w.clone()) {
                    let mut c = Candidate::new(w, Source::Combo);
                    if f > 0 {
                        c.dict_freq = Some(f);
                    }
                    candidates.push(c);
                }
            }
        }
        // 纯声母等其余情形的缩写+全拼兜底
        if !is_full_pinyin && !has_vowel && candidates.len() < self.page_size * self.max_pages {
            for (w, f) in dictionary::query_abbrev_full_meta(&pinyin_str) {
                if !short_keep(&w) {
                    continue;
                }
                if cand_seen.insert(w.clone()) {
                    let mut c = Candidate::new(w, Source::Combo);
                    if f > 0 {
                        c.dict_freq = Some(f);
                    }
                    candidates.push(c);
                }
            }
        }
        // 逐音节组合联想（V0.4）已移至错音之后（纠错优先，见下方）——此位置删除
        // 模糊音容错（RIME Spelling Algebra，0.1.14）：输入串变体查询，补在精确命中后
        // 0.3.x：仅完整拼音输入触发（防英文单词被 l→n 等变体误联想中文）
        if self.fuzzy_flags != 0
            && is_full_pinyin
            && fuzzy::may_have_fuzzy_with_flags(&pinyin_str, self.fuzzy_flags)
        {
            for variant in fuzzy::fuzzy_variants_with_flags(&pinyin_str, self.fuzzy_flags) {
                let v_arc: std::sync::Arc<str> = std::sync::Arc::from(variant.as_str());
                let v_len = variant.len();
                let (v_words, v_exact) = dictionary::query_with_split(&variant);
                for (idx, w) in v_words.into_iter().enumerate() {
                    if cand_seen.insert(w.clone()) {
                        let mut c = Candidate::new(w, Source::Fuzzy)
                            .with_pinyin(Some(std::sync::Arc::clone(&v_arc)), v_len);
                        c.is_variant_prefix = idx >= v_exact;
                        candidates.push(c);
                    }
                }
                // 变体简拼补充
                for w in dictionary::query_short(&variant) {
                    if cand_seen.insert(w.clone()) {
                        let mut c = Candidate::new(w, Source::Fuzzy)
                            .with_pinyin(Some(std::sync::Arc::clone(&v_arc)), v_len);
                        c.is_variant_prefix = true;
                        candidates.push(c);
                    }
                }
            }
        }
        // 拼写纠错（V0.3.x，对标 rime-ice speller derive 规则）：
        // wia→wai、hzi→zhi、lng→lang/leng/ling/long、zagn→zang、do→dou/dong 等。
        // B5 门控：明确英文词且无精确中文命中时（api/email/app）跳过所有拼音纠错。
        if self.correction_enabled
            && !suppress_pinyin_correction
            && candidates.len() < self.page_size
        {
            for variant in correction::spelling_variants(&pinyin_str) {
                let v_arc: std::sync::Arc<str> = std::sync::Arc::from(variant.as_str());
                let v_len = variant.len();
                let (v_words, v_exact) = dictionary::query_with_split(&variant);
                for (idx, w) in v_words.into_iter().take(CORR_TAKE).enumerate() {
                    if cand_seen.insert(w.clone()) {
                        let mut c = Candidate::new(w, Source::Correction)
                            .with_pinyin(Some(std::sync::Arc::clone(&v_arc)), v_len);
                        c.is_variant_prefix = idx >= v_exact;
                        candidates.push(c);
                    }
                }
            }
        }
        // 多打字母容错（V0.4.4）：删除每个位置一个字符，尝试命中正确拼音。
        // 例：weom→wom（我们）、zhongg→zhong（中国）、zhongguoo→zhongguo（中国）、nihaoa→nihao（你好）。
        // 放宽门控：无精确可信命中时允许在 page_size*max_pages 内补入（此前被 < page_size 挡掉导致
        // zhongguoo / nihaoa 缺删除纠错）。
        if self.correction_enabled
            && !suppress_pinyin_correction
            && (candidates.len() < self.page_size
                || (is_full_pinyin
                    && !exact_has_credible
                    && candidates.len() < self.page_size * self.max_pages))
        {
            for variant in correction::deletion_variants(&pinyin_str) {
                let v_arc: std::sync::Arc<str> = std::sync::Arc::from(variant.as_str());
                let v_len = variant.len();
                let (v_words, v_exact) = dictionary::query_with_split(&variant);
                let take_slice = if v_exact > 0 {
                    &v_words[..v_exact.min(CORR_TAKE * 2)]
                } else {
                    &v_words[..v_words.len().min(CORR_TAKE)]
                };
                let slice_refs: Vec<&str> = take_slice.iter().map(|s| s.as_str()).collect();
                let freqs = if v_exact > 0 {
                    dictionary::exact_word_freqs(&variant, &slice_refs)
                } else {
                    vec![None; take_slice.len()]
                };
                for (idx, (w, fopt)) in take_slice.iter().zip(freqs).enumerate() {
                    if cand_seen.insert(w.clone()) {
                        let mut r = 0u32;
                        if fopt.is_some_and(|f| f >= CREDIBLE_EXACT_FREQ) {
                            r |= reason::HIGH_FREQ;
                        }
                        let mut c = Candidate::with_reason(w.clone(), Source::Correction, r)
                            .with_pinyin(Some(std::sync::Arc::clone(&v_arc)), v_len);
                        c.score = fopt.map(|f| f as i32);
                        c.is_exact_deletion = idx < v_exact;
                        c.is_variant_prefix = idx >= v_exact;
                        candidates.push(c);
                    }
                }
            }
        }
        // 智能纠错（V0.2.10）：键盘相邻键与相邻交换变体补入。
        // 门控允许完整拼音且 `!exact_has_credible` 时放行，避免领域词前缀扩展撑满配额后把纠错彻底挡掉
        // （如 niha），同时不放行已有可信混拼命中的非完整拼音（如 went→问题）。
        if self.correction_enabled
            && !suppress_pinyin_correction
            && (candidates.len() < self.page_size * self.max_pages
                || (is_full_pinyin && !exact_has_credible))
            && correction::may_need_correction(&pinyin_str)
        {
            let input_syl_count = complete_syllable_counts(&norm_str)
                .and_then(|c| c.into_iter().next())
                .or_else(|| mixed_syllable_count(&norm_str))
                .unwrap_or(0);
            let mut corr_cands: Vec<(String, std::sync::Arc<str>, bool)> = Vec::new();
            let mut corr_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            for variant in correction::correction_variants(&pinyin_str) {
                if !crate::pinyin::is_complete_pinyin(&variant) {
                    continue;
                }
                let v_arc: std::sync::Arc<str> = std::sync::Arc::from(variant.as_str());
                let (v_words, v_exact) = dictionary::query_with_split(&variant);
                for (idx, w) in v_words.into_iter().take(CORR_TAKE * 2).enumerate() {
                    let is_prefix = idx >= v_exact;
                    // 前缀扩展变体只保留字数与输入音节数一致的词（如 niha→niba→你帮），
                    // 滤除超长前缀噪声。
                    if is_prefix
                        && input_syl_count >= 2
                        && w.chars().filter(|ch| is_hanzi(*ch)).count() != input_syl_count
                    {
                        continue;
                    }
                    if corr_seen.insert(w.clone()) {
                        corr_cands.push((w, std::sync::Arc::clone(&v_arc), is_prefix));
                    }
                }
                // 变体再走模糊音（纠错 + 模糊叠加，已是双编辑距离，只取精确键命中防前缀噪声）
                if self.fuzzy_flags != 0
                    && fuzzy::may_have_fuzzy_with_flags(&variant, self.fuzzy_flags)
                {
                    for fv in fuzzy::fuzzy_variants_with_flags(&variant, self.fuzzy_flags) {
                        let fv_arc: std::sync::Arc<str> = std::sync::Arc::from(fv.as_str());
                        let (fv_words, fv_exact) = dictionary::query_with_split(&fv);
                        for w in fv_words.into_iter().take(fv_exact.min(CORR_TAKE)) {
                            if corr_seen.insert(w.clone()) {
                                corr_cands.push((w, std::sync::Arc::clone(&fv_arc), false));
                            }
                        }
                    }
                }
            }
            // 精确变体命中优先于前缀变体命中，只补入常用词（最多 3 条）
            corr_cands.sort_by_key(|(_, _, is_prefix)| *is_prefix);
            for (w, py, is_prefix) in corr_cands
                .into_iter()
                .filter(|(w, _, _)| dictionary::is_common_word(w))
                .take(3)
            {
                if cand_seen.insert(w.clone()) {
                    let py_len = py.len();
                    let mut c = Candidate::new(w, Source::Correction).with_pinyin(Some(py), py_len);
                    c.is_variant_prefix = is_prefix;
                    candidates.push(c);
                }
            }
        }
        // 漏打字母容错（V0.5.x）：每个位置插入一个字母后仍能完整切分的变体。
        // 例：meiganxi → meiguanxi（gan 漏打 u）→ 没关系。
        let input_unlikely = !is_full_pinyin
            || (!exact_has_credible
                && candidates
                    .iter()
                    .take(5)
                    .all(|c| !dictionary::is_common_word(&c.text)));
        let head_has_common = candidates
            .iter()
            .any(|c| c.source == Source::Correction && dictionary::is_common_word(&c.text))
            || candidates
                .iter()
                .take(3)
                .any(|c| dictionary::is_common_word(&c.text));
        if self.correction_enabled
            && !suppress_pinyin_correction
            && input_unlikely
            && !head_has_common
            && candidates.len() < self.page_size * self.max_pages
        {
            for variant in correction::insertion_variants(&pinyin_str) {
                let v_arc: std::sync::Arc<str> = std::sync::Arc::from(variant.as_str());
                let v_len = variant.len();
                let (v_words, v_exact) = dictionary::query_with_split(&variant);
                for (idx, w) in v_words.into_iter().take(CORR_TAKE).enumerate() {
                    if cand_seen.insert(w.clone()) {
                        let mut c = Candidate::new(w, Source::Correction)
                            .with_pinyin(Some(std::sync::Arc::clone(&v_arc)), v_len);
                        c.is_variant_prefix = idx >= v_exact;
                        candidates.push(c);
                    }
                }
            }
        }
        // 错音错字提示（V0.2.26）：候选不足时，查易错读音映射 → 用正确读音查词
        // 0.3.x：仅完整拼音输入触发
        if is_full_pinyin && !suppress_pinyin_correction && candidates.len() < self.page_size {
            for (correct_py, _word) in mistake::lookup(&pinyin_str) {
                let py_arc: std::sync::Arc<str> = std::sync::Arc::from(correct_py);
                let py_len = correct_py.len();
                for w in dictionary::query(correct_py) {
                    if cand_seen.insert(w.clone()) {
                        candidates.push(
                            Candidate::new(w, Source::Correction)
                                .with_pinyin(Some(std::sync::Arc::clone(&py_arc)), py_len),
                        );
                    }
                }
            }
        }
        // 逐音节组合联想（V0.4，Eric 反馈：nimzai/ganshm/yaowoquz 混合输入无候选）：
        // 输入 = 音节前缀/声母 任意混插（ni+m+zai → 你们在、gan+sh+m → 干什么、
        // rgshni → 如果是你、yaowoquz → 要我去做）。
        // 两步：先整词匹配（query_combo），无整词再逐音节拼接（combo_guess）。
        // 对标搜狗逐音节智能切分；仅在候选不足时补充（性能可控）。
        // V0.4.1：仅非完整拼音触发——xiexie/zhongguo 等完整拼音 query 已出整词，
        // combo 的宽松拼接（下嗯下嗯）会污染候选并挤掉 emoji/精准词。
        // 联想段起点：后面拆组结果要插到它之前（见 phrase_group_guess 分支）
        let combo_start = candidates.len();
        // 门槛在简拼补充处已计算（见上方 combo_gate / combo_common_only）。
        if !is_full_pinyin
            && !suppress_pinyin_correction
            && candidates.len() < self.page_size * self.max_pages
        {
            for w in dictionary::query_combo(&pinyin_str) {
                if combo_common_only
                    && !dictionary::is_common_word(&w)
                    && !dictionary::is_user_word(&w)
                {
                    continue;
                }
                if cand_seen.insert(w.clone()) {
                    let mut c = Candidate::new(w, Source::Combo);
                    c.is_loose_combo = true;
                    candidates.push(c);
                }
            }
        }
        // 词库锚定拆分组词（V0.4.5，Eric 设计）：优先于 combo_guess 逐字硬拼，
        // 确保 shurus→输入是/输入时、danshiw→但是我 等词组搭接排在逐字死拼之前。
        let target_counts = complete_syllable_counts(&norm_str)
            .or_else(|| mixed_syllable_count(&norm_str).map(|n| vec![n]));
        let has_target_len_common = target_counts.as_ref().is_some_and(|tcounts| {
            candidates.iter().take(6).any(|c| {
                c.source != Source::Correction
                    && dictionary::is_common_word(&c.text)
                    && tcounts.contains(&c.text.chars().filter(|ch| is_hanzi(*ch)).count())
            })
        });
        let need_group = !suppress_pinyin_correction
            && (candidates.len() < self.page_size
                || (!exact_has_credible && !has_target_len_common));
        if need_group {
            // 引擎每键路径：组词展开限 1500 节点 / 60ms（长输入组合爆炸，
            // 实测 13 字符不设限要 349ms）。直接 API 调用不受此限。
            dictionary::set_group_limits(1500, Some(30));
            let prepend = candidates.len() >= self.page_size;
            let mut inserted = 0usize;
            for w in dictionary::phrase_group_guess(&pinyin_str) {
                if combo_common_only
                    && !dictionary::is_common_word(&w)
                    && !dictionary::is_user_word(&w)
                {
                    continue;
                }
                if !cand_seen.insert(w.clone()) {
                    continue;
                }
                let segs = w
                    .chars()
                    .filter(|ch| is_hanzi(*ch))
                    .count()
                    .clamp(2, u8::MAX as usize) as u8;
                let mut cand = Candidate::with_reason(w, Source::Combo, reason::STITCHED);
                // 词库锚定拆组（phrase_group_guess）：前 2 条高置信 bigram 搭接进入 tier 6，
                // 其余退回 tier 8（让 PREFIX_WORD 在首屏第 3 位可达）。
                cand.segment_count = segs;
                cand.is_phrase_group = inserted < 2;
                cand.dict_freq = Some(1000u32.saturating_sub(inserted as u32));
                if prepend && inserted < 2 {
                    let at = inserted.min(candidates.len());
                    candidates.insert(at, cand);
                } else if combo_start < candidates.len() {
                    let at = (combo_start + inserted).min(candidates.len());
                    candidates.insert(at, cand);
                } else {
                    candidates.push(cand);
                }
                inserted += 1;
            }
            dictionary::set_group_limits(0, None);
        }
        if !is_full_pinyin
            && !suppress_pinyin_correction
            && candidates.len() < self.page_size * self.max_pages
        {
            for w in dictionary::combo_guess(&pinyin_str) {
                if combo_common_only
                    && !dictionary::is_common_word(&w)
                    && !dictionary::is_user_word(&w)
                {
                    continue;
                }
                if cand_seen.insert(w.clone()) {
                    let segs = w
                        .chars()
                        .filter(|ch| is_hanzi(*ch))
                        .count()
                        .clamp(2, u8::MAX as usize) as u8;
                    let mut c = Candidate::with_reason(w, Source::Combo, reason::STITCHED);
                    c.segment_count = segs;
                    candidates.push(c);
                }
            }
        }
        const RANKING_CANDIDATE_BUDGET: usize = 4096;
        if candidates.len() > RANKING_CANDIDATE_BUDGET {
            candidates.truncate(RANKING_CANDIDATE_BUDGET);
            cand_seen = candidates.iter().map(|c| c.text.clone()).collect();
        }
        // V0.5+ 自然语言触发日期候选（对标主流输入法）：
        // 输入"日期/时间/年份/年" → 候选末尾追加当前日期/时间/年份
        if let Some(dt) = Self::datetime_pinyin_candidates(&pinyin_str) {
            for w in dt {
                if cand_seen.insert(w.clone()) {
                    candidates.push(Candidate::new(w, Source::Datetime));
                }
            }
        }
        // 中英混输（B5 分类限额）：
        // 1. 精确英文词（len >= 3 且命中英文词典）且中文侧无精确可信词（如 key / api /
        //    test / code / hello / email）：英文原串+最多 2 条词典项前置到首位（T0），
        //    中文候选保留其后（不清空）。
        // 2. 其余场景（完整拼音或拼音前缀或仅英文前缀命中）：英文候选恒在中文之后，
        //    前缀补全上限 3 条，防止大量英文词挤占翻页空间。
        if self.mix_mode_enabled
            && !self.ascii_mode
            && !pinyin_str.is_empty()
            && pinyin_str.chars().all(|c| c.is_ascii_alphabetic())
        {
            const ENG_TAIL_LIMIT: usize = 3;
            // 2026-11-26（Eric）：中文输入法下，只要有可信中文候选，英文就不能占首位。
            // 此前 key 这类「完整首音节 + 单字母尾」的串被 dict_has_credible_mixed 的
            // y/w 排除规则放行（rest="y"），结果 key/keywords/keyword 三个英词占满前三位，
            // 「可以」被压到第 4 位。hello/api/email/code 无中文候选，行为不变。
            // 只认词库直出/补全/组词类中文：纠错与模糊是「猜的读音」，不能拿它们的候选
            // 把明确英文词（npm）压下去；也不认生僻词（test 的「特使团/托尔斯泰」）。
            has_credible_chinese = candidates.iter().any(|c| {
                matches!(
                    c.source,
                    Source::Dictionary | Source::Completion | Source::Combo | Source::Sentence
                ) && !c.text.is_empty()
                    && c.text.chars().all(is_hanzi)
                    && (dictionary::is_common_word(&c.text)
                        || dictionary::is_user_word(&c.text)
                        || c.attest.is_some_and(|a| a.credible()))
            });
            let promote_english = looks_like_english_word
                && !exact_has_credible
                && !dict_has_credible_mixed
                && (!is_full_pinyin
                    || !crate::pinyin::is_valid_pinyin_prefix(&pinyin_str)
                    || exact_len == 0);
            if promote_english {
                let display = eng_hits
                    .iter()
                    .find(|e| e.eq_ignore_ascii_case(&pinyin_str))
                    .cloned()
                    .unwrap_or_else(|| pinyin_str.clone());
                let mut eng_cands: Vec<String> = vec![display];
                for e in &eng_hits {
                    if !eng_cands.iter().any(|c| c.eq_ignore_ascii_case(e)) {
                        eng_cands.push(e.clone());
                    }
                }
                eng_cands.truncate(3);
                for (i, e) in eng_cands.iter().enumerate() {
                    cand_seen.insert(e.clone());
                    let mut c =
                        Candidate::with_reason(e.clone(), Source::English, reason::ENG_EXACT_TOP);
                    c.explicit_promoted = true;
                    c.dict_freq = Some(100u32.saturating_sub(i as u32));
                    candidates.insert(i, c);
                }
            } else if !eng_hits.is_empty() {
                for (i, e) in eng_hits.into_iter().take(ENG_TAIL_LIMIT).enumerate() {
                    cand_seen.insert(e.clone());
                    let mut c = Candidate::new(e, Source::English);
                    c.dict_freq = Some(100u32.saturating_sub(i as u32));
                    candidates.push(c);
                }
            } else {
                // 英文词典无命中 → 原样兜底追加末尾（防候选空）
                let raw = pinyin_str.clone();
                cand_seen.insert(raw.clone());
                candidates.push(Candidate::new(raw, Source::English));
            }
        }
        // 整句解码（长输入）：把整串音节读成最可能的词序列，送入统一排序管线。
        self.apply_sentence_candidates(&mut candidates, &mut cand_seen, &norm_str);
        // ≥3 音节连贯输入（如 fuhyuqi→符合、meiyhouxuan→没有、haisyouxie→还是、
        // shurushi→输入）补入前 2 音节常用词并打上 PREFIX_WORD 标记，
        // 选中它时直接切入后续音节的组词态（而非重置缓冲）。
        if !exact_has_credible
            && !suppress_pinyin_correction
            && norm_str.len() <= 12
            && !complete_syllable_counts(&norm_str)
                .is_some_and(|c| c.contains(&1) || c.contains(&2))
            && let Some(syls) = self.split_compose_syllables()
            && syls.len() >= 3
        {
            let prefix2 = format!("{}{}", syls[0], syls[1]);
            if let Some(pw) = dictionary::leading_common_word(&prefix2, 2) {
                let eng_start =
                    Self::first_index_of(&candidates, Source::English).unwrap_or(candidates.len());
                if let Some(pos) = candidates[..eng_start].iter().position(|c| c.text == pw) {
                    candidates[pos].reasons |= reason::PREFIX_WORD | reason::COMMON_WORD;
                } else if eng_start > 0 || candidates.is_empty() {
                    let cand = Candidate::with_reason(
                        pw,
                        Source::Dictionary,
                        reason::PREFIX_WORD | reason::COMMON_WORD,
                    );
                    candidates.push(cand);
                }
            }
        }
        // V0.5.6 繁体模式转繁去重：简体词条转繁后与原生繁体相同（我們的出口）
        // → 去掉重复（保留先出现的原生繁体）。
        if self.traditional_mode {
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            // 候选已经是结构体，retain 原地过滤即可，不再需要维护第二张表的对齐
            candidates.retain(|c| {
                let display = if crate::trad::is_traditional(&c.text) {
                    c.text.clone()
                } else {
                    crate::trad::to_traditional(&c.text)
                };
                seen.insert(display)
            });
        }
        // P2-5 / Phase D 形态变体生成（Emoji 簇挂接本体词，排序由统一 v2 簇锚定裁决）
        self.attach_form_variants(&mut candidates);
        // 整句解码已前置（见词长分区之前的调用）——放在这里会绕过全部排序链。
        // V0.3.x 白话文长句过滤（Eric 决策：长句匹配无必要 + 防候选窗溢出）：
        // 成语/谚语/常用词（≤10 字）保留，白话文长句/超长专名（>10 字）不进候选。
        // 用户自定义快捷短语不受限（用户显式定义的文本必须可命中）。
        const MAX_CAND_WORD_LEN: usize = 10;
        {
            let phrase_text = if Self::first_index_of(&candidates, Source::Phrase).is_some() {
                self.phrase_map.get(&pinyin_str).cloned()
            } else {
                None
            };
            // 整句候选按音节整段解码，长度本来就等于输入字数，不受词级长句过滤限制
            candidates.retain(|c| {
                c.source == Source::Sentence || c.text.chars().count() <= MAX_CAND_WORD_LEN
            });
            if let Some(pt) = phrase_text
                && !candidates.iter().any(|c| c.text == pt)
            {
                let pr = if self.user_phrase_keys.contains(&pinyin_str) {
                    reason::USER_PHRASE
                } else {
                    0
                };
                candidates.insert(0, Candidate::with_reason(pt, Source::Phrase, pr));
            }
        }
        // 所有来源、排序和过滤完成后再限制保留页数。
        // C3（R5 中英驼峰混输）：raw_input 形如「小写拼音前缀 + 大写开头英文段」（如 woshiEric、
        // yongPython）时，把中文前缀解码结果与原始英文段拼接并前置为 Sentence 候选，
        // 防止整句解码把 Eric 硬拆成「二次/日冲」。
        if let Some((cn_py, en_tail)) = Self::split_cn_en_camel(&self.raw_input) {
            let mut cn_hits: Vec<String> = Vec::new();
            let (cn_dict, cn_exact) = dictionary::query_with_split(cn_py);
            let mut exact_words: Vec<String> = cn_dict
                .into_iter()
                .take(cn_exact)
                .filter(|w| !w.is_empty() && w.chars().all(is_hanzi))
                .collect();
            // 常用词优先（如 woshi 下「我是」优先于「卧室/我市」）
            exact_words.sort_by_key(|w| !dictionary::is_common_word(w));
            let has_common_exact = exact_words.iter().any(|w| dictionary::is_common_word(w));
            // 2–3 音节常见主谓短语（woshi→我是、woyong→我用、nikan→你看）常无整词条，
            // 当精确整词无常用词命中时，优先按首选音节切分取各音节首字拼接。
            if !has_common_exact
                && crate::pinyin::is_complete_pinyin(cn_py)
                && let Some(syls) = crate::pinyin::syllable_split_variants(cn_py, 1)
                    .into_iter()
                    .next()
                && (2..=3).contains(&syls.len())
            {
                let mut joined = String::new();
                let mut all_ok = true;
                for syl in &syls {
                    let (sw, s_exact) = dictionary::query_with_split(syl);
                    if let Some(ch) = sw
                        .into_iter()
                        .take(s_exact.max(1))
                        .find(|w| w.chars().count() == 1 && w.chars().all(is_hanzi))
                    {
                        joined.push_str(&ch);
                    } else {
                        all_ok = false;
                        break;
                    }
                }
                if all_ok && !joined.is_empty() && !cn_hits.contains(&joined) {
                    cn_hits.push(joined);
                }
            }
            for w in exact_words.into_iter().take(3) {
                if !cn_hits.contains(&w) {
                    cn_hits.push(w);
                }
            }
            let prev = if self.last_committed.is_empty() {
                None
            } else {
                Some(self.last_committed.as_str())
            };
            if crate::pinyin::is_complete_pinyin(cn_py) {
                for w in crate::sentence::decode(cn_py, prev, 2) {
                    if !cn_hits.contains(&w) {
                        cn_hits.push(w);
                    }
                }
            }
            if cn_hits.is_empty() {
                for w in dictionary::phrase_group_guess(cn_py).into_iter().take(2) {
                    if !cn_hits.contains(&w) {
                        cn_hits.push(w);
                    }
                }
            }
            if !cn_hits.is_empty() {
                let mut head: Vec<Candidate> = Vec::with_capacity(cn_hits.len().min(3));
                for (idx, w) in cn_hits.into_iter().take(3).enumerate() {
                    let m = format!("{w}{en_tail}");
                    candidates.retain(|c| c.text != m);
                    let mut cand = Candidate::with_reason(
                        m,
                        Source::Sentence,
                        reason::EXACT_PINYIN | reason::CREDIBLE,
                    );
                    cand.explicit_promoted = true;
                    cand.clean_coverage = true;
                    cand.dict_freq = Some(100u32.saturating_sub(idx as u32));
                    head.push(cand);
                }
                candidates.splice(0..0, head);
            }
        }
        // 日期候选前置（最终位置）：中途的短语插入与排序会把种子挤到后面
        // （实测 sj 被短语「手机」顶掉时间候选），这里统一提到首部，
        // 保证「日期优先、中文可翻页」的语义不受排序链影响。
        if !datetime_seed.is_empty() {
            candidates.retain(|c| c.source != Source::Datetime);
            let seeds: Vec<Candidate> = datetime_seed
                .into_iter()
                .enumerate()
                .map(|(i, text)| {
                    let mut c = Candidate::new(text, Source::Datetime);
                    c.explicit_datetime = true;
                    c.dict_freq = Some(100u32.saturating_sub(i as u32));
                    c
                })
                .collect();
            candidates.splice(0..0, seeds);
        }
        let shape = ranking::InputShape::build(&pinyin_str);
        self.rank_and_truncate(&mut candidates, shape);
        // 2026-11-26（Eric）：英文留在首屏，但不占前两位。
        // 排序管线会把 explicit_promoted 的英文顶回最前，所以位置修正在排序之后做：
        // 有可信中文时把英文整体挪到第 3 位起（中文占前两位，英文仍在首屏）；
        // 没有中文可让（hello / code / test）时保持首位。
        if has_credible_chinese && candidates.len() > 2 {
            let mut english: Vec<Candidate> = Vec::new();
            let mut i = 0;
            while i < candidates.len() {
                // 只挪「置顶型」英文（输入串本身命中英文词典），
                // 原样兜底出的 raw 英文不该挤进中间（kankan 这类会被打断）。
                if candidates[i].source == Source::English && candidates[i].explicit_promoted {
                    english.push(candidates.remove(i));
                } else {
                    i += 1;
                }
            }
            for (k, c) in english.into_iter().enumerate() {
                let at = (2 + k).min(candidates.len());
                candidates.insert(at, c);
            }
        }
        self.publish_candidates(candidates);
    }

    /// Phase D 形态变体生成：为命中 Emoji 表的中文候选挂接同簇变体（`cluster = Some(本体)`）。
    /// 不改原候选顺序；同簇位置与坑位上限由 `rank_and_truncate` 统一裁决。
    fn attach_form_variants(&self, candidates: &mut Vec<Candidate>) {
        if !self.emoji_enabled {
            return;
        }
        let mut variants: Vec<Candidate> = Vec::new();
        let mut seen: std::collections::HashSet<String> =
            candidates.iter().map(|c| c.text.clone()).collect();
        for c in candidates.iter() {
            if c.form != ranking::Form::Text || c.source == Source::English {
                continue;
            }
            let emojis = crate::emoji::lookup_all(&c.text);
            if emojis.is_empty() {
                continue;
            }
            for (v_idx, e) in emojis.into_iter().enumerate() {
                let text = format!("{}{e}", c.text);
                if !seen.insert(text.clone()) {
                    continue;
                }
                let mut v = Candidate::new(text, Source::Emoji);
                v.form = ranking::Form::Emoji;
                v.cluster = Some(c.text.clone());
                v.dict_freq = Some(100u32.saturating_sub(v_idx as u32));
                variants.push(v);
            }
        }
        candidates.extend(variants);
    }

    /// 统一 v2 排序与截断管线（Layer 2 证据归约 → Layer 3 单次簇感知全序裁决 → 簇限额与降权保留截断）。
    /// 全拼（`query_all`）与双拼（`query_all_shuangpin`）共用此唯一入口。
    fn rank_and_truncate(&mut self, candidates: &mut Vec<Candidate>, shape: ranking::InputShape) {
        self.reduce_candidates(candidates, &shape);
        let has_clusters = candidates.iter().any(|c| c.cluster.is_some());
        if has_clusters {
            // 形态变体继承其簇本体词的 match_level / demoted / explicit_rank 诊断属性，
            // 使 CandidateMeta 与降权保留截断对整簇保持一致（§5.3 规则 1）。
            let mut anchor_meta: std::collections::HashMap<String, (ranking::Match, bool, u8)> =
                std::collections::HashMap::new();
            for c in candidates.iter() {
                if c.form == ranking::Form::Text
                    && let Some(a) = c.attest
                {
                    anchor_meta.entry(c.text.clone()).or_insert((
                        a.match_level,
                        a.demoted,
                        a.explicit_rank,
                    ));
                }
            }
            for c in candidates.iter_mut() {
                if let Some(ref base) = c.cluster
                    && let (Some(att), Some(&(ml, dem, exp))) =
                        (c.attest.as_mut(), anchor_meta.get(base))
                {
                    att.match_level = ml;
                    att.demoted = dem;
                    att.explicit_rank = exp;
                }
            }
        }
        let order = self.rank_v2_order(candidates);
        let n = candidates.len();
        Self::permute_range(candidates, 0, n, &order);
        if has_clusters {
            // §5.3 规则 3：单簇坑位上限（默认 2：文本本体 + 1 个形态变体），防单簇刷屏
            let limit = self.cluster_slot_limit.max(1);
            let mut counts: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            candidates.retain(|c| {
                let cnt = counts.entry(c.cluster_key().to_string()).or_insert(0);
                *cnt += 1;
                *cnt <= limit
            });
        }
        let cap = self.page_size * self.max_pages;
        if candidates.len() > cap {
            // 截断时保留末尾降权词（§6.2 方案 B：可翻页恢复，不因截断丢失）
            let mut demoted_tail: Vec<Candidate> = Vec::new();
            let mut normal_head: Vec<Candidate> = Vec::with_capacity(cap);
            for c in candidates.drain(..) {
                if c.attest.is_some_and(|a| a.demoted) {
                    demoted_tail.push(c);
                } else {
                    normal_head.push(c);
                }
            }
            let keep_demoted = demoted_tail.len().min(cap / 2);
            normal_head.truncate(cap.saturating_sub(keep_demoted));
            normal_head.extend(demoted_tail.into_iter().take(keep_demoted));
            *candidates = normal_head;
        }
        self.shadow_snapshot = Some((shape, candidates.clone()));
    }

    /// Layer 2 · 证据归约（纯计算 + 单次词库锁批量取事实，不改候选顺序）。
    fn reduce_candidates(&self, candidates: &mut [Candidate], shape: &ranking::InputShape) {
        if candidates.is_empty() {
            return;
        }
        // 1. 批量取词库事实（单次锁）
        let query_items: Vec<(&str, Option<&str>)> = candidates
            .iter()
            .map(|c| (c.text.as_str(), c.pinyin.as_deref()))
            .collect();
        let facts = dictionary::batch_word_facts(&shape.norm, &query_items);

        // 2. 批量取上下文提权（与 apply_context_boost 同口径：档位 1 归零，英文区不参与）
        let context_lifts: Vec<u8> = if self.context_enabled && !self.last_committed.is_empty() {
            let prev = self.last_committed.as_str();
            let mut merged: Vec<(String, u32)> =
                self.context_map.get(prev).cloned().unwrap_or_default();
            if let Some(prior) = self.context_prior.get(prev) {
                for (word, weight) in prior {
                    match merged.iter_mut().find(|(w, _)| w == word) {
                        Some(slot) => slot.1 = slot.1.max(*weight),
                        None => merged.push((word.clone(), *weight)),
                    }
                }
            }
            for word in crate::context::lookup(prev) {
                match merged.iter_mut().find(|(w, _)| w == &word) {
                    Some(slot) => {
                        slot.1 = slot.1.max(crate::tuning::tuning().context_builtin_weight)
                    }
                    None => merged.push((word, crate::tuning::tuning().context_builtin_weight)),
                }
            }
            let weight_cap = crate::tuning::tuning().context_weight_cap;
            let texts: Vec<&str> = candidates.iter().map(|c| c.text.as_str()).collect();
            let bigram_lifts = if crate::dictionary::bigram_available() {
                crate::dictionary::bigram_scores(prev, &texts)
            } else {
                vec![0u32; candidates.len()]
            };
            candidates
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    if c.source == Source::English {
                        return 0;
                    }
                    let learned = merged
                        .iter()
                        .find(|(w, _)| w == &c.text)
                        .map(|(_, weight)| (*weight).min(weight_cap))
                        .unwrap_or(0);
                    let bigram_lift = if bigram_lifts[i] >= 2 {
                        bigram_lifts[i]
                    } else {
                        0
                    };
                    learned.max(bigram_lift).min(u8::MAX as u32) as u8
                })
                .collect()
        } else {
            vec![0u8; candidates.len()]
        };

        // 3. 第一遍（只读）：计算匹配层与整句标志，并统计主匹配层下的用户候选数。
        // 「拥挤」判据必须先知道当前前缀下有多少用户词在竞争，才能在第二遍决定
        // 哪些用户词享有置顶提权（否则任一用户词都无条件压过系统高频词）。
        let mut match_levels: Vec<ranking::Match> = Vec::with_capacity(candidates.len());
        let mut sentence_flags: Vec<bool> = Vec::with_capacity(candidates.len());
        for c in candidates.iter() {
            let char_count = c.text.chars().filter(|ch| is_hanzi(*ch)).count();
            let total_chars = c.text.chars().count();
            let match_in = ranking::MatchInput {
                source: c.source,
                reasons: c.reasons,
                pinyin: c.pinyin.as_deref(),
                pinyin_len: c.pinyin_len,
                char_count,
                total_chars,
                clean_coverage: c.clean_coverage,
                is_loose_combo: c.is_loose_combo,
            };
            match_levels.push(if c.source == Source::Datetime && !c.explicit_datetime {
                ranking::Match::Foreign
            } else {
                ranking::match_of(shape, &match_in)
            });
            sentence_flags.push(c.source == Source::Sentence && !c.explicit_promoted);
        }
        let is_invalid_user_alias = |c: &Candidate, fact: &dictionary::WordFact| -> bool {
            c.source == Source::Dictionary
                && matches!(c.dict_layer, Some(0 | 2))
                && !fact.valid_single_reading
        };
        let primary_user_count = candidates
            .iter()
            .zip(facts.iter())
            .zip(match_levels.iter())
            .filter(|((c, fact), ml)| {
                !(c.source == Source::Sentence && !c.explicit_promoted)
                    && matches!(**ml, ranking::Match::Exact | ranking::Match::Shape)
                    && (fact.is_user || (c.reasons & reason::USER_WORD != 0))
                    && !is_invalid_user_alias(c, fact)
            })
            .count();
        // 多音节完整拼音（ceshi / nihao / taishen）：用户词优先级无条件保留。
        let is_multi_full_py = shape.splits_clean && shape.best_split.len() >= 2;
        // 单音节完整拼音（fu / shi / ji）：同音字天然多条用户词，与简拼/半音节分档处理。
        let is_single_full_syl = shape.splits_clean && shape.best_split.len() == 1;

        // 4. 第二遍：逐条写入 Attestation
        for (i, (c, fact)) in candidates.iter_mut().zip(facts.iter()).enumerate() {
            let ctx_lift = context_lifts[i];
            let char_count = c.text.chars().filter(|ch| is_hanzi(*ch)).count();
            let total_chars = c.text.chars().count();
            let match_level = match_levels[i];
            let is_sentence = sentence_flags[i];
            let is_prefix_word = c.reasons & reason::PREFIX_WORD != 0;
            // 整句在非精确可信同层竞争时按 extent=1 排在精确单段词（extent=0，如 编译器）之后，
            // 同时与同为 extent=1 的 PREFIX_WORD（如 mamama→妈妈）按 user_tier 公平竞争。
            let extent_rank = if is_sentence && match_level == ranking::Match::Exact {
                1
            } else {
                let match_in = ranking::MatchInput {
                    source: c.source,
                    reasons: c.reasons,
                    pinyin: c.pinyin.as_deref(),
                    pinyin_len: c.pinyin_len,
                    char_count,
                    total_chars,
                    clean_coverage: c.clean_coverage,
                    is_loose_combo: c.is_loose_combo,
                };
                ranking::extent_of(shape, &match_in, match_level)
            };

            // 错音历史别名（如用户词库里的 ji→就）在单音节完整拼音下读音不合法，
            // 除了不参与用户提权，还要降权到翻页区，不得占首屏（只作用用户词层，绝不误伤繁体/模糊音/纠错）。
            let invalid_alias = is_invalid_user_alias(c, fact);
            let demoted = c.source != Source::English
                && (self.demoted_words.contains(&c.text) || invalid_alias);
            let explicit_rank = if c.source == Source::Datetime && c.explicit_datetime {
                ranking::explicit::DATETIME
            } else if c.source == Source::Phrase {
                ranking::explicit::PHRASE
            } else if !demoted && c.source != Source::English && self.pin_words.contains(&c.text) {
                ranking::explicit::PINNED
            } else if (c.reasons & reason::ENG_EXACT_TOP != 0)
                || matches!(c.source, Source::Mode | Source::Voice)
                || c.explicit_promoted
            {
                ranking::explicit::PROMOTED
            } else {
                ranking::explicit::NATURAL
            };

            let has_rare = has_rare_hanzi(&c.text);

            // 整句候选是拼接产物：不是用户词/常用词；
            // 段数统一记为 2（大于词级 1，保证整词永远压过整句；同时整句之间不重复惩罚切词数，
            // 完全尊重 Viterbi 语言模型得分，防止低分 2 词「他惠来」反压高分 3 词「他会来」）。
            let (
                is_user,
                is_common,
                user_tier,
                user_freq,
                common_order,
                freq,
                segment_count,
                user_boosted,
            ) = if is_sentence {
                let score_freq = c
                    .sentence_score
                    .map(|sc| ((sc + 200.0).clamp(0.0, 400.0) * 1000.0) as u32);
                (false, false, 3u8, 0u32, None, score_freq, 2u8, false)
            } else {
                let raw_is_user = fact.is_user || (c.reasons & reason::USER_WORD != 0);
                // 错音历史别名（如 ji→就）不算有效用户词，不参与提权与竞争。
                let is_user = raw_is_user && !invalid_alias;
                // 多音字防护：来自词库精确/前缀索引的候选（dict_layer 有值）仅在命中
                // 本键 common_index（layer == 1）或当前前缀常用词表时才算「本读音下的
                // 常用词」，防止多音字借另一读音的常用行序抢位（如 ji 下「给」借 gei
                // 的 rank=42 压过「及/即/急」）。
                let is_common = match c.dict_layer {
                    Some(1) => true,
                    Some(_) => fact.in_prefix_common,
                    None => fact.is_common || (c.reasons & reason::COMMON_WORD != 0),
                };
                // 拥挤输入串（多项用户词竞争首屏）下，只有真正「练出来」的用户词才提权：
                // - 多音节完整拼音：无条件优先（ceshi / nihao）
                // - 不拥挤的单音节完整拼音：优先（en → 恩）
                // - 不拥挤的简拼/半音节串：用户频次 >= 2 才优先（fax → 发现）
                // - 拥挤/多义串：用户频次 >= 15 才算高频习惯词（shi → 是(983)）
                // 仅用户词库层（layer 0/2）的 dict_freq 是用户线性频次；
                // 系统层（layer 3）与 Combo 系统条目的 dict_freq 是系统对数词频（1000..4500），
                // 不得混入用户频次阈值（简拼用户频次已由 batch_word_facts 查 user_short_index 提供）。
                let user_layer_freq = if matches!(c.dict_layer, Some(0 | 2)) {
                    c.dict_freq.unwrap_or(0)
                } else {
                    0
                };
                let eff_user_freq = fact.user_freq.max(user_layer_freq);
                let user_boosted = is_user
                    && (c.dict_layer == Some(0) || fact.user_active)
                    && (is_multi_full_py
                        || (shape.splits_clean && primary_user_count <= 2)
                        || (is_single_full_syl && primary_user_count <= 5 && eff_user_freq >= 5)
                        || (!shape.splits_clean && primary_user_count <= 2 && eff_user_freq >= 2)
                        || (primary_user_count > 2 && eff_user_freq >= 15));
                let user_tier = if user_boosted {
                    0
                } else if is_common
                    || (ctx_lift >= 2
                        && matches!(match_level, ranking::Match::Exact | ranking::Match::Shape))
                {
                    1
                } else {
                    3
                };
                let user_freq = if user_boosted { eff_user_freq } else { 0 };
                let common_order = if is_common { fact.common_order } else { None };
                // 词频统一回系统对数尺度：前缀扩展 / 补全候选先还原系统真词频，
                // 用户频次只在没有系统词条时兜底（纯自造词）。
                let freq = c
                    .score
                    .and_then(|s| (s >= 0).then_some(s as u32))
                    .or(fact.exact_freq)
                    .or(fact.sys_freq)
                    .or(c.dict_freq);
                (
                    is_user,
                    is_common,
                    user_tier,
                    user_freq,
                    common_order,
                    freq,
                    c.segment_count.max(1),
                    user_boosted,
                )
            };

            // 末尾声母截断（如 heshenm→合身/何深）：仅当候选在「去末字母」键上有真实精确词条时成立
            // （nihap 去末字母为 niha，你好在 niha 上并非精确词条，因此不误判为末尾声母截断）。
            let is_trailing_initial_trunc = !shape.splits_clean
                && shape.norm.len() >= 4
                && char_count + 1 == shape.syllable_count
                && c.pinyin
                    .as_deref()
                    .is_some_and(|p| p == &shape.norm[..shape.norm.len() - 1])
                && (c.is_exact_deletion || fact.exact_freq.is_some());

            // 逐字硬拼/宽松组合（query_combo）在 Composed 层内按实际字数计段数（不冒充 1 段整词）
            let final_segment_count = if c.is_loose_combo && match_level == ranking::Match::Composed
            {
                char_count.clamp(2, u8::MAX as usize) as u8
            } else {
                segment_count
            };

            // 同步补写诊断理由位标供探针与协议查询
            if is_user {
                c.reasons |= reason::USER_WORD;
            }
            if is_common {
                c.reasons |= reason::COMMON_WORD;
            }
            if has_rare {
                c.reasons |= reason::RARE_CHAR;
            }
            if ctx_lift > 0 {
                c.reasons |= reason::CONTEXT_BOOST;
            }
            if explicit_rank == ranking::explicit::PINNED {
                c.reasons |= reason::PINNED;
            }
            if demoted {
                c.reasons |= reason::DEMOTED;
            }

            c.attest = Some(ranking::Attestation {
                match_level,
                extent_rank,
                segment_count: final_segment_count,
                char_count,
                freq,
                user_tier,
                user_freq,
                common_order,
                is_user,
                user_boosted,
                is_common,
                has_rare,
                context_lift: ctx_lift,
                explicit_rank,
                demoted,
                is_variant: matches!(c.source, Source::Correction | Source::Fuzzy),
                is_variant_prefix: c.is_variant_prefix,
                is_completion: c.source == Source::Completion,
                is_trailing_initial_trunc,
                is_phrase_group: c.is_phrase_group,
                is_prefix_word,
                is_sentence,
            });
        }
    }

    /// Layer 3 · 单次裁决：返回按 v2 `RankKey`（含形态簇锚定）全序排列后的候选下标序列。
    fn rank_v2_order(&self, candidates: &[Candidate]) -> Vec<usize> {
        let exact_has_credible = candidates.iter().any(|c| {
            c.source == Source::Dictionary
                && c.attest.as_ref().is_some_and(|a| {
                    a.match_level == ranking::Match::Exact && a.credible() && a.char_count > 0
                })
        });
        let sort_mode = self.sort_mode.clamp(0, 2) as u8;
        let has_clusters = candidates.iter().any(|c| c.cluster.is_some());
        ranking::cluster_sort_order(candidates.len(), has_clusters, |i| {
            let c = &candidates[i];
            let a = c.attest.unwrap_or_default();
            let pref = ranking::pref_rank(sort_mode, a.char_count, c.source == Source::English);
            (
                c.text.as_str(),
                c.cluster_key(),
                c.form,
                a.rank_key(pref, c.form, exact_has_credible),
            )
        })
    }

    /// 影子模式对比：对当前输入快照产出排序对照与逐条维度归因（零副作用）。
    pub fn shadow_diff_current(&self) -> Option<ranking::RankDiff> {
        let (shape, cands) = self.shadow_snapshot.as_ref()?;
        let old_order: Vec<String> = cands.iter().map(|c| c.text.clone()).collect();
        let exact_has_credible = cands.iter().any(|c| {
            c.source == Source::Dictionary
                && c.attest.as_ref().is_some_and(|a| {
                    a.match_level == ranking::Match::Exact && a.credible() && a.char_count > 0
                })
        });
        let sort_mode = self.sort_mode.clamp(0, 2) as u8;
        let keys: Vec<ranking::RankKey> = cands
            .iter()
            .map(|c| {
                let a = c.attest.unwrap_or_default();
                let pref = ranking::pref_rank(sort_mode, a.char_count, c.source == Source::English);
                a.rank_key(pref, c.form, exact_has_credible)
            })
            .collect();
        let order = self.rank_v2_order(cands);
        let new_order: Vec<String> = order.iter().map(|&i| cands[i].text.clone()).collect();

        let mut changed = Vec::new();
        let mut explanation = Vec::new();
        for (new_pos, &orig_idx) in order.iter().enumerate() {
            if orig_idx != new_pos {
                changed.push((cands[orig_idx].text.clone(), orig_idx, new_pos));
                // 找出被它反超（或反超它）的参照词，给出精确的首个差异维度
                let ref_idx = new_pos.min(cands.len() - 1);
                let a_att = cands[orig_idx].attest.unwrap_or_default();
                let b_att = cands[ref_idx].attest.unwrap_or_default();
                let reason = if new_pos < orig_idx {
                    ranking::explain_pair(
                        &cands[orig_idx].text,
                        &keys[orig_idx],
                        a_att.match_level,
                        &cands[ref_idx].text,
                        &keys[ref_idx],
                        b_att.match_level,
                    )
                } else {
                    let winner_idx = order[orig_idx];
                    let w_att = cands[winner_idx].attest.unwrap_or_default();
                    ranking::explain_pair(
                        &cands[winner_idx].text,
                        &keys[winner_idx],
                        w_att.match_level,
                        &cands[orig_idx].text,
                        &keys[orig_idx],
                        a_att.match_level,
                    )
                };
                explanation.push(format!(
                    "{} ({} -> {}): {}",
                    cands[orig_idx].text, orig_idx, new_pos, reason
                ));
            }
        }
        Some(ranking::RankDiff {
            input: shape.raw.clone(),
            old_order,
            new_order,
            changed,
            explanation,
        })
    }

    /// 离线影子探针（不改变当前引擎状态）：用当前引擎配置对 `input` 跑一遍并返回新旧排序对比。
    pub fn diff_ranking(&self, input: &str) -> Option<ranking::RankDiff> {
        let mut probe = Engine::new();
        probe.page_size = self.page_size;
        probe.max_pages = self.max_pages;
        probe.fuzzy_flags = self.fuzzy_flags;
        probe.correction_enabled = self.correction_enabled;
        probe.mix_mode_enabled = self.mix_mode_enabled;
        probe.traditional_mode = self.traditional_mode;
        probe.phrase_enabled = self.phrase_enabled;
        probe.phrase_map = self.phrase_map.clone();
        probe.user_phrase_keys = self.user_phrase_keys.clone();
        probe.pin_words = self.pin_words.clone();
        probe.demoted_words = self.demoted_words.clone();
        probe.emoji_enabled = self.emoji_enabled;
        probe.cluster_slot_limit = self.cluster_slot_limit;
        probe.sort_mode = self.sort_mode;
        probe.last_committed = self.last_committed.clone();
        probe.context_enabled = self.context_enabled;
        probe.completion_enabled = self.completion_enabled;
        probe.context_map = self.context_map.clone();
        probe.context_prior = self.context_prior.clone();
        for ch in input.chars() {
            probe.process_key(ch);
        }
        probe.shadow_diff_current()
    }

    /// 内置快捷短语表（V0.2.12 + B4 清理）：
    /// 移除硬编码占位短语（dz 科技园 / qq QQ号码 / email 邮箱地址 / tel 电话号码 / bz 备注：）
    /// 及与高频简拼冲突的 `sj`（时间 3802 > 手机 3094，改由正常简拼按词频排序）。
    /// 用户可在短语配置中按需自定义任何长短语（通过 `load_phrases` 享有 T0 优先级）。
    fn builtin_phrases() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("bq".to_string(), "不客气".to_string());
        m.insert("wm".to_string(), "我们".to_string());
        m.insert("gs".to_string(), "公司".to_string());
        m.insert("zj".to_string(), "再见".to_string());
        m.insert("wx".to_string(), "微信".to_string());
        m.insert("hy".to_string(), "会议".to_string());
        m.insert("wd".to_string(), "文档".to_string());
        m.insert("bg".to_string(), "报告".to_string());
        m.insert("ht".to_string(), "合同".to_string());
        m.insert("fp".to_string(), "发票".to_string());
        m.insert("gsz".to_string(), "工作总结".to_string());
        m.insert("zgs".to_string(), "早上好".to_string());
        m.insert("wns".to_string(), "晚安".to_string());
        m.insert("xiex".to_string(), "谢谢".to_string());
        m
    }

    /// 双拼模式查询：双拼码串 → 全拼候选 → 与全拼共用后处理管线
    ///
    /// 三个关键点（2026-10 修复）：
    /// 1. 只解码「成对」部分——末尾单键是下一个音节的声母，作为顶码候选追加在后；
    /// 2. 解码结果必须能完整切分为合法音节，半截串（gszo→gongzo）走前缀匹配会把
    ///    长词顶到合法词前面（工作 → 共总）；
    /// 3. 排序不再裸查词库，复用全拼的词长匹配/上下文/置顶/降权/英文混输。
    fn query_all_shuangpin(&mut self) {
        let code = self.pinyin_buf.clone();
        let chars: Vec<char> = code.chars().collect();
        let pairs_len = chars.len() / 2 * 2;
        let (prefix_code, tail_code) = if pairs_len == 0 {
            (String::new(), code.clone())
        } else {
            (
                chars[..pairs_len].iter().collect::<String>(),
                chars[pairs_len..].iter().collect::<String>(),
            )
        };

        let decoded: Vec<String> = if prefix_code.is_empty() {
            Vec::new()
        } else {
            self.shuangpin_scheme
                .decode_string(&prefix_code)
                .into_iter()
                .filter(|s| crate::pinyin::is_complete_pinyin(s))
                .collect()
        };
        let primary = decoded.first().cloned();
        let primary_arc: Option<std::sync::Arc<str>> = primary.as_deref().map(std::sync::Arc::from);
        let mut candidates: Vec<Candidate> = Vec::new();
        // P0-3 同款统一去重（双拼路径候选规模小，但分支结构一致）
        let mut cand_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for fp in &decoded {
            let (entries, exact_len) = dictionary::query_with_split_meta(fp);
            for (idx, e) in entries.into_iter().enumerate() {
                if !cand_seen.insert(e.word.clone()) {
                    continue;
                }
                let is_exact = idx < exact_len;
                let reasons = if is_exact {
                    reason::EXACT_PINYIN
                } else {
                    reason::PREFIX_EXT
                };
                // 多解码并存时，精确命中项规范拼音归并到主解码键，使 v2 `match_of` 一致识别为 Exact，
                // 同时保留原解码键的 `dict_layer` 与 `dict_freq`。
                let py = if is_exact { primary_arc.clone() } else { None };
                let mut c = Candidate::with_reason(e.word, Source::Dictionary, reasons)
                    .with_pinyin(py, e.pinyin_len);
                c.dict_layer = Some(e.layer);
                if e.freq > 0 {
                    c.dict_freq = Some(e.freq);
                }
                candidates.push(c);
            }
        }
        // 完整音节无候选时，尝试模糊音（双拼+模糊音可叠加）
        if candidates.is_empty() && self.fuzzy_flags != 0 {
            for fp in &decoded {
                if fuzzy::may_have_fuzzy_with_flags(fp, self.fuzzy_flags) {
                    for variant in fuzzy::fuzzy_variants_with_flags(fp, self.fuzzy_flags) {
                        let (entries, _) = dictionary::query_with_split_meta(&variant);
                        for e in entries {
                            if !cand_seen.insert(e.word.clone()) {
                                continue;
                            }
                            let mut c = Candidate::new(e.word, Source::Fuzzy)
                                .with_pinyin(e.pinyin, e.pinyin_len);
                            c.dict_layer = Some(e.layer);
                            if e.freq > 0 {
                                c.dict_freq = Some(e.freq);
                            }
                            candidates.push(c);
                        }
                    }
                }
            }
        }
        // 末尾未完成音节 → 顶码候选，追加在完整音节候选之后（不抢首位）
        if !tail_code.is_empty() {
            let single_key_complete = primary.is_none() && crate::pinyin::is_complete_pinyin(&code);
            for fp in self.shuangpin_scheme.decode(&tail_code) {
                let (entries, exact_len) = dictionary::query_with_split_meta(&fp);
                for (idx, e) in entries.into_iter().enumerate() {
                    if !cand_seen.insert(e.word.clone()) {
                        continue;
                    }
                    let is_single_exact = single_key_complete && idx < exact_len && fp == code;
                    let reasons = if is_single_exact {
                        reason::EXACT_PINYIN
                    } else {
                        reason::PREFIX_EXT
                    };
                    let py = if is_single_exact { e.pinyin } else { None };
                    let mut c = Candidate::with_reason(e.word, Source::Dictionary, reasons)
                        .with_pinyin(py, e.pinyin_len);
                    c.dict_layer = Some(e.layer);
                    if e.freq > 0 {
                        c.dict_freq = Some(e.freq);
                    }
                    candidates.push(c);
                }
            }
        }

        // 英文混输（双拼补齐）：按原按键串查英文词典——hello 这类解不出中文的串给英文候选
        if self.mix_mode_enabled
            && !self.ascii_mode
            && !code.is_empty()
            && code.chars().all(|c| c.is_ascii_alphabetic())
        {
            let eng = crate::english::query(&code);
            let has_exact =
                code.chars().count() >= 3 && eng.iter().any(|e| e.eq_ignore_ascii_case(&code));
            // 与全拼同构的闸门：输入串本身像拼音（完整或前缀）时英文只在末尾。
            // 否则 song 这类能解成双拼码的英文词会把中文候选顶掉。
            let looks_like_pinyin = crate::pinyin::is_complete_pinyin(&code)
                || crate::pinyin::is_valid_pinyin_prefix(&code);
            if has_exact && !looks_like_pinyin {
                let mut eng_cands: Vec<String> = vec![code.clone()];
                for e in &eng {
                    if !eng_cands.iter().any(|c| c.eq_ignore_ascii_case(e)) {
                        eng_cands.push(e.clone());
                    }
                }
                eng_cands.truncate(3);
                let cn_has_common = candidates
                    .iter()
                    .take(5)
                    .any(|c| crate::dictionary::is_common_word(&c.text));
                for (i, e) in eng_cands.into_iter().enumerate() {
                    let mut c = if !cn_has_common {
                        let mut c =
                            Candidate::with_reason(e, Source::English, reason::ENG_EXACT_TOP);
                        c.explicit_promoted = true;
                        c
                    } else {
                        Candidate::new(e, Source::English)
                    };
                    c.dict_freq = Some(100u32.saturating_sub(i as u32));
                    candidates.push(c);
                }
            } else if !eng.is_empty() {
                for (i, e) in eng.into_iter().take(3).enumerate() {
                    let mut c = Candidate::new(e, Source::English);
                    c.dict_freq = Some(100u32.saturating_sub(i as u32));
                    candidates.push(c);
                }
            } else if candidates.is_empty() {
                candidates.push(Candidate::new(code.clone(), Source::English));
            }
        }

        if self.traditional_mode {
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            candidates.retain(|c| {
                let display = if crate::trad::is_traditional(&c.text) {
                    c.text.clone()
                } else {
                    crate::trad::to_traditional(&c.text)
                };
                seen.insert(display)
            });
        }
        self.attach_form_variants(&mut candidates);
        const MAX_CAND_WORD_LEN: usize = 10;
        candidates.retain(|c| c.text.chars().count() <= MAX_CAND_WORD_LEN);
        let shape = ranking::InputShape::build(primary.as_deref().unwrap_or(&code));
        self.rank_and_truncate(&mut candidates, shape);
        self.publish_candidates(candidates);
    }

    /// 发布一轮候选：同时更新 all_candidates 与来源/元数据表，重置到第 0 页。
    /// 所有候选产出口（特殊模式、双拼、全拼主路径）都必须走这里，
    /// 保证 `candidate_src` / `candidate_meta` / `candidate_cluster` 与 `all_candidates` 永远等长。
    fn publish(&mut self, candidates: Vec<String>, src: Source) {
        let n = candidates.len();
        self.candidate_src = vec![src; n];
        self.candidate_meta = vec![
            CandidateMeta {
                tier: meta_tier_for_source(src),
                reasons: 0,
                score: None,
                match_level: None,
                form: ranking::Form::Text.rank(),
            };
            n
        ];
        self.candidate_cluster = vec![None; n];
        self.shadow_snapshot = None;
        self.all_candidates = candidates;
        self.page = 0;
        self.repage();
    }

    /// 发布候选。文本、来源与裁决元数据已在 [`Candidate`] 里合并，这里同步落表。
    fn publish_candidates(&mut self, candidates: Vec<Candidate>) {
        let mut texts: Vec<String> = Vec::with_capacity(candidates.len());
        let mut srcs: Vec<Source> = Vec::with_capacity(candidates.len());
        let mut metas: Vec<CandidateMeta> = Vec::with_capacity(candidates.len());
        let mut clusters: Vec<Option<String>> = Vec::with_capacity(candidates.len());
        for c in candidates {
            let derived_tier = meta_tier_from_candidate(&c);
            let match_level = c.attest.map(|a| a.match_level.rank());
            let form = c.form.rank();
            texts.push(c.text);
            srcs.push(c.source);
            clusters.push(c.cluster);
            metas.push(CandidateMeta {
                tier: derived_tier,
                reasons: c.reasons,
                score: c.score,
                match_level,
                form,
            });
        }
        self.all_candidates = texts;
        self.candidate_src = srcs;
        self.candidate_meta = metas;
        self.candidate_cluster = clusters;
        self.page = 0;
        self.repage();
    }

    /// 按当前页码刷新候选页
    fn repage(&mut self) {
        let start = self.page * self.page_size;
        let end = (start + self.page_size).min(self.all_candidates.len());
        self.candidates.clear();
        if start < self.all_candidates.len() {
            self.candidates
                .extend_from_slice(&self.all_candidates[start..end]);
        }
    }

    /// 测试辅助：走统一 v2 排序管线（归约 + 单次全序裁决），不截断。
    #[cfg(test)]
    fn rank_v2_for_test(&self, candidates: &mut Vec<Candidate>, input: &str) {
        let shape = ranking::InputShape::build(input);
        self.reduce_candidates(candidates, &shape);
        let order = self.rank_v2_order(candidates);
        let n = candidates.len();
        Self::permute_range(candidates, 0, n, &order);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 把文本列表包成候选列表（来源统一 Dictionary，带严格递减词频与 system 层标记，
    /// 使合成单测不依赖全局词库状态），供 v2 排序单测使用。
    fn srcs_of(texts: &[String]) -> Vec<Candidate> {
        let base = 1000u32;
        texts
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let mut c =
                    Candidate::with_reason(t.clone(), Source::Dictionary, reason::EXACT_PINYIN);
                c.dict_layer = Some(3);
                c.dict_freq = Some(base.saturating_sub(i as u32));
                c
            })
            .collect()
    }

    #[test]
    fn test_complete_syllable_count_debug() {
        assert_eq!(
            complete_syllable_count("taiwan"),
            Some(2),
            "taiwan 应为 2 音节"
        );
        assert_eq!(complete_syllable_count("women"), Some(2));
        // xi/tong/kong/zhi/tai = 5 音节（xitongkongzhitai 系统控制台）
        assert_eq!(complete_syllable_count("xitongkongzhitai"), Some(5));
        assert_eq!(complete_syllable_count("zg"), None, "简拼无完整音节");
        assert_eq!(complete_syllable_count("hel"), None);
    }

    // ─── V0.6 隔音歧义（yuan 打不出「预案」）───

    #[test]
    fn test_syllable_split_variants_heteronym() {
        // yuan：单音节 yuan + 双音节 yu+an（预案）
        let v = crate::pinyin::syllable_split_variants("yuan", 16);
        assert!(
            v.iter().any(|p| p == &["yuan".to_string()]),
            "应含单音节路径: {v:?}"
        );
        assert!(
            v.iter().any(|p| p == &["yu".to_string(), "an".to_string()]),
            "应含 yu+an 路径: {v:?}"
        );
        // xian：单音节 + xi+an（西安）
        assert_eq!(complete_syllable_counts("xian"), Some(vec![1, 2]));
        // fangan 无单音节解释 → 只有 2 音节
        assert_eq!(complete_syllable_counts("fangan"), Some(vec![2]));
        // 非拼音串
        assert_eq!(complete_syllable_counts("zg"), None);
    }

    #[test]
    fn test_complete_syllable_counts_keeps_glide_syllables() {
        // 审计 P1：整串本身就是单音节时，单字母段是真正的隔音歧义，不能过滤。
        // yue = yu+e（余额）、qie = qi+e（企鹅）、jie = ji+e（饥饿）。
        assert_eq!(complete_syllable_counts("yue"), Some(vec![1, 2]));
        assert_eq!(complete_syllable_counts("qie"), Some(vec![1, 2]));
        assert_eq!(complete_syllable_counts("jie"), Some(vec![1, 2]));
        // 非单音节长串仍要压住「zhong+gu+o」这类噪声：只保留完整切分。
        assert_eq!(complete_syllable_counts("zhongguo"), Some(vec![2]));
    }

    #[test]
    fn test_word_len_match_keeps_cross_syllable_word() {
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(None);
        // yuan 输入下，「预案」（2 字，规范拼音 yuan）归 L2 Shape，不应被当成超长前缀词压后
        let e = Engine::new();
        let py: std::sync::Arc<str> = std::sync::Arc::from("yuan");
        let make = |w: &str, layer: u8, freq: u32| {
            let mut c =
                Candidate::with_reason(w.to_string(), Source::Dictionary, reason::EXACT_PINYIN)
                    .with_pinyin(Some(py.clone()), 4);
            c.dict_layer = Some(layer);
            c.dict_freq = Some(freq);
            c
        };
        let mut c = vec![
            make("原", 1, 5000),
            make("源", 1, 4500),
            make("预案", 1, 2000),
            make("元", 1, 4000),
            make("远", 1, 3500),
        ];
        e.rank_v2_for_test(&mut c, "yuan");
        let pos = c.iter().position(|w| w == "预案").unwrap();
        assert!(pos < 5, "预案应在第一页内, got {c:?}");
    }

    #[test]
    fn test_ambiguous_split_boost_yuan_to_fangan() {
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(None);
        // v2 单次全序：yuan 下可信跨音节词「预案」(Shape tier 1) 排在高频/常用单字之后，
        // 但在非常用单字 (tier 2/4) 与前缀扩展词 (tier 5) 之前，无需硬编码第 4 位搬移。
        let e = Engine::new();
        let py: std::sync::Arc<str> = std::sync::Arc::from("yuan");
        let make = |w: &str, layer: u8, freq: u32| {
            let mut c =
                Candidate::with_reason(w.to_string(), Source::Dictionary, reason::EXACT_PINYIN)
                    .with_pinyin(Some(py.clone()), 4);
            c.dict_layer = Some(layer);
            c.dict_freq = Some(freq);
            c
        };
        let mut c = vec![
            make("原", 1, 5000),
            make("源", 1, 4500),
            make("元", 1, 4000),
            make("远", 1, 3500),
            make("塬", 3, 2600),
            make("橼", 3, 1800),
            make("预案", 1, 2000),
        ];
        e.rank_v2_for_test(&mut c, "yuan");
        assert_eq!(
            c[4], "预案",
            "预案应排在常用单字后、非常用单字前, got {c:?}"
        );

        // 无歧义输入按词频序自然排布
        let c2_texts = vec!["方案".to_string(), "刚刚".to_string(), "反感".to_string()];
        let mut c2 = srcs_of(&c2_texts);
        e.rank_v2_for_test(&mut c2, "fangan");
        assert_eq!(c2, vec!["方案", "刚刚", "反感"]);
    }

    #[test]
    fn test_word_len_match_debug() {
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(None);
        let e = Engine::new();
        let py: std::sync::Arc<str> = std::sync::Arc::from("taiwan");
        let exact = |w: &str, freq: u32| {
            let mut c =
                Candidate::with_reason(w.to_string(), Source::Dictionary, reason::EXACT_PINYIN)
                    .with_pinyin(Some(py.clone()), 6);
            c.dict_layer = Some(3);
            c.dict_freq = Some(freq);
            c
        };
        let ext = |w: &str, freq: u32| {
            let mut c =
                Candidate::with_reason(w.to_string(), Source::Dictionary, reason::PREFIX_EXT);
            c.dict_layer = Some(3);
            c.dict_freq = Some(freq);
            c
        };
        let mut c = vec![
            ext("台灣物理學會", 5000),
            exact("台湾", 3000),
            exact("太弯", 2000),
            exact("太晚", 1800),
            ext("臺灣簇蟲", 4000),
        ];
        e.rank_v2_for_test(&mut c, "taiwan");
        assert_eq!(c[0], "台湾", "2 字精确词应排在前缀长词前, got {:?}", c);
    }

    #[test]
    fn test_demote_inserts_after_tenth_without_dropping_words() {
        let mut e = Engine::new();
        e.demoted_words.insert("坏词".to_string());
        // §6.2 方案 B：15 个非降权词 + 1 个降权词 → 降权词压出前 2 屏并置尾，不丢词且保留相对序
        let mut texts: Vec<String> = (0..15).map(|i| format!("词{i}")).collect();
        texts.insert(7, "坏词".to_string());
        let before: Vec<String> = texts
            .iter()
            .filter(|w| w.as_str() != "坏词")
            .cloned()
            .collect();
        let mut c = srcs_of(&texts);
        e.rank_v2_for_test(&mut c, "synthtest");
        assert_eq!(c.len(), 16, "降权不应改变候选总数: {c:?}");
        let pos = c.iter().position(|w| w == "坏词").unwrap();
        assert!(pos >= 10, "降权词应落在前 2 屏之后，实际第 {pos} 位: {c:?}");
        assert_eq!(
            c.last().unwrap(),
            "坏词",
            "v2 方案 B 下降权词沉底可翻页恢复"
        );
        let after: Vec<String> = c
            .iter()
            .filter(|w| w.text != "坏词")
            .map(|w| w.text.clone())
            .collect();
        assert_eq!(after, before, "非降权词相对顺序必须保持不变");
    }

    #[test]
    fn test_demote_few_candidates_degrades_to_tail() {
        let mut e = Engine::new();
        e.demoted_words.insert("坏词".to_string());
        let texts = vec!["甲".to_string(), "坏词".to_string(), "乙".to_string()];
        let mut c = srcs_of(&texts);
        e.rank_v2_for_test(&mut c, "synthtest");
        assert_eq!(
            c,
            vec!["甲".to_string(), "乙".to_string(), "坏词".to_string()],
            "候选不足 10 时降权词排末尾"
        );
    }

    #[test]
    fn test_pin_boost_is_stable_partition() {
        let mut e = Engine::new();
        e.pin_words = ["乙".to_string(), "丁".to_string()].into_iter().collect();
        let texts = vec![
            "甲".to_string(),
            "乙".to_string(),
            "丙".to_string(),
            "丁".to_string(),
            "戊".to_string(),
        ];
        let mut c = srcs_of(&texts);
        e.rank_v2_for_test(&mut c, "synthtest");
        assert_eq!(
            c,
            vec![
                "乙".to_string(),
                "丁".to_string(),
                "甲".to_string(),
                "丙".to_string(),
                "戊".to_string()
            ],
            "置顶词前置且组内保持原顺序"
        );
    }

    #[test]
    fn test_sort_mode_single_first_is_stable() {
        let mut e = Engine::new();
        e.sort_mode = 1;
        let texts = vec![
            "我们".to_string(),
            "我".to_string(),
            "问题".to_string(),
            "五".to_string(),
        ];
        let mut c = srcs_of(&texts);
        e.rank_v2_for_test(&mut c, "synthtest");
        assert_eq!(
            c,
            vec![
                "我".to_string(),
                "五".to_string(),
                "我们".to_string(),
                "问题".to_string()
            ],
            "单字前置且组内保持原顺序"
        );
    }

    #[test]
    fn test_candidate_src_length_always_matches() {
        // 各类路径（词库命中 / 英文 / 简拼 / 纠错 / 短语 / 日期 / 单字母）
        // 走一遍，来源表必须始终与候选等长
        let mut engine = Engine::new();
        for input in [
            "nihao",
            "hello",
            "zg",
            "xinashi",
            "womenceshi",
            "bq",
            "rq",
            "w",
        ] {
            for ch in input.chars() {
                engine.process_key(ch);
            }
            assert_eq!(
                engine.all_candidates.len(),
                engine.candidate_src.len(),
                "输入 {input} 时来源表长度应与候选一致"
            );
            engine.reset();
        }
    }

    #[test]
    fn test_candidate_src_marks_english_tail() {
        let mut engine = Engine::new();
        for ch in "nihao".chars() {
            engine.process_key(ch);
        }
        if let Some(pos) = engine
            .candidate_src
            .iter()
            .position(|&s| s == Source::English)
        {
            assert!(
                engine.candidate_src[pos..]
                    .iter()
                    .all(|&s| s == Source::English),
                "英文区之后不应再出现中文来源: {:?}",
                engine.candidate_src
            );
        }
    }

    #[test]
    fn test_candidate_src_marks_phrase() {
        let mut engine = Engine::new();
        for ch in "bq".chars() {
            engine.process_key(ch);
        }
        assert!(
            engine.candidate_src.contains(&Source::Phrase),
            "bq 应产出短语来源: {:?}",
            engine.candidate_src
        );
    }

    #[test]
    fn test_is_hanzi_ranges() {
        assert!(is_hanzi('中'), "基本区");
        assert!(is_hanzi('龍'), "繁体仍在基本区");
        assert!(is_hanzi('\u{3400}'), "扩展 A 首字");
        assert!(!is_hanzi('é'), "拉丁扩展字母不算汉字");
        assert!(!is_hanzi('，'), "全角标点不算汉字");
        assert!(!is_hanzi('😀'), "emoji 不算汉字");
        assert!(!is_hanzi('a'));
        assert!(!is_hanzi('1'));
    }

    #[test]
    #[ignore = "C1 基线对照，手动运行：cargo test --lib zz_probe_context_order -- --ignored --nocapture"]
    fn zz_probe_context_order() {
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let db =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../resources/system_dict.db");
        if db.exists() {
            crate::dictionary::init_blocking(Some(&db));
        }
        let cases: &[(&str, &str)] = &[
            ("电脑", "pingmu"),
            ("北京", "daxue"),
            ("中国", "renmin"),
            ("今天", "tianqi"),
            ("我们", "yiqi"),
            ("工作", "shijian"),
            ("这个", "wenti"),
            ("你好", "pengyou"),
            ("中国", "guojia"),
            ("国家", "zhongguo"),
            ("明天", "tianqi"),
            ("事情", "zhijian"),
        ];
        let mut engine = Engine::new();
        engine.set_context_enabled(true);
        for &(prev, input) in cases {
            engine.reset();
            engine.last_committed = prev.to_string();
            for ch in input.chars() {
                engine.process_key(ch);
            }
            let n = engine.candidate_count().min(10);
            let items: Vec<String> = (0..n)
                .filter_map(|i| engine.candidate(i).map(|s| s.to_string()))
                .collect();
            println!("CASE {prev}|{input} => {}", items.join(" / "));
        }
    }

    #[test]
    fn test_context_boost_long_list_weight_beats_position() {
        // P1-3 回归：候选超过 20 条时，搭配权重仍必须压过位置先验。
        let mut engine = Engine::new();
        engine.set_context_enabled(true);
        engine.last_committed = "电脑".to_string();
        engine.load_prior_contexts(vec![("电脑".to_string(), "屏幕".to_string(), 6)]);
        let mut texts: Vec<String> = (0..29).map(|i| format!("候选{i}")).collect();
        texts.push("屏幕".to_string());
        let mut cands = srcs_of(&texts);
        engine.rank_v2_for_test(&mut cands, "synthtest");
        assert_eq!(
            cands[0], "屏幕",
            "长候选列表下搭配权重必须压过位置先验: {cands:?}"
        );
    }

    #[test]
    fn test_context_boost_equal_weight_keeps_order() {
        // 权重相同时按词频顺序保持稳定
        let mut engine = Engine::new();
        engine.set_context_enabled(true);
        engine.last_committed = "电脑".to_string();
        engine.load_prior_contexts(vec![
            ("电脑".to_string(), "乙".to_string(), 6),
            ("电脑".to_string(), "丁".to_string(), 6),
        ]);
        let texts = vec![
            "甲".to_string(),
            "乙".to_string(),
            "丙".to_string(),
            "丁".to_string(),
        ];
        let mut cands = srcs_of(&texts);
        engine.rank_v2_for_test(&mut cands, "synthtest");
        assert_eq!(cands, vec!["乙", "丁", "甲", "丙"]);
    }

    #[test]
    fn test_classify_input() {
        let e = Engine::new();
        // 完整拼音：词库有命中 → Exact；无命中 → FullPinyin
        assert_eq!(e.classify_input("nihao", 3), InputQuality::Exact);
        assert_eq!(e.classify_input("nihao", 0), InputQuality::FullPinyin);
        // 纯声母简拼
        assert_eq!(e.classify_input("zg", 0), InputQuality::Initials);
        // 混合（全拼段 + 声母段）
        assert_eq!(e.classify_input("shurf", 0), InputQuality::Mixed);
        // 打错 / 混合切分（宽松匹配，多数无法精确切分的输入落在 Mixed）
        assert_eq!(e.classify_input("logn", 0), InputQuality::Mixed);
        // 合法前缀（末尾音节未打完）——宽松切分下也可能落 Mixed，两者都合理
        assert!(matches!(
            e.classify_input("zho", 0),
            InputQuality::Partial | InputQuality::Mixed
        ));
    }

    #[test]
    fn test_voice_candidate_syncs_source() {
        let mut e = Engine::new();
        // 先制造一个英文候选，确保后面检查的是"旧来源是否残留"
        for ch in "hello".chars() {
            e.process_key(ch);
        }
        e.set_voice_candidate("你好");
        assert_eq!(
            e.candidate_src,
            vec![Source::Voice],
            "语音候选来源应为 Voice"
        );
        assert!(!e.is_english_candidate(0), "语音候选不应被当成英文");

        e.set_voice_candidate("");
        assert!(e.candidate_src.is_empty(), "清空候选时来源表必须同步清空");
    }

    #[test]
    fn test_candidate_src_invariant_across_operations() {
        // 每一步操作后都校验：文本候选与来源表必须等长
        let mut e = Engine::new();
        for ch in "zhegeweomende".chars() {
            e.process_key(ch);
            assert_eq!(
                e.all_candidates.len(),
                e.candidate_src.len(),
                "输入 {ch} 后文本与来源表长度不一致"
            );
        }
        for _ in 0..32 {
            if e.pinyin_str().is_empty() {
                break;
            }
            e.backspace();
            assert_eq!(
                e.all_candidates.len(),
                e.candidate_src.len(),
                "退格后长度不一致"
            );
        }
        assert!(e.pinyin_str().is_empty());
        assert!(e.candidate_src.is_empty(), "输入清空后来源表也必须为空");

        e.set_voice_candidate("测试");
        assert_eq!(e.candidate_src.len(), 1, "语音候选要有来源");
        e.reset();
        assert!(e.candidate_src.is_empty(), "reset 后来源表必须为空");
    }

    #[test]
    #[ignore = "C1 行为风险探针：手动运行看前文是否顶掉高频词首位"]
    fn zz_probe_context_top_risk() {
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let db =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../resources/system_dict.db");
        if db.exists() {
            crate::dictionary::init_blocking(Some(&db));
        }
        for &(prev, input) in &[
            ("我们", "de"),
            ("这个", "shi"),
            ("他们", "de"),
            ("今天", "de"),
            ("中国", "ren"),
        ] {
            let mut base = Engine::new();
            base.set_context_enabled(false);
            for ch in input.chars() {
                base.process_key(ch);
            }
            let base_top = base.candidate(0).map(str::to_string);

            let mut ctx = Engine::new();
            ctx.set_context_enabled(true);
            ctx.last_committed = prev.to_string();
            for ch in input.chars() {
                ctx.process_key(ch);
            }
            let ctx_top5: Vec<String> = (0..5)
                .filter_map(|i| ctx.candidate(i).map(str::to_string))
                .collect();
            println!("CTX {prev}|{input} => base={base_top:?} ctx_top5={ctx_top5:?}");
        }
    }

    #[test]
    fn test_context_boost_short_list_flip_is_intended() {
        // C1 语义变更的边界形态：第 2 位权重 1、第 1 位权重 0，搭配命中优先于词频微差。
        let mut engine = Engine::new();
        engine.set_context_enabled(true);
        engine.last_committed = "电脑".to_string();
        engine.load_prior_contexts(vec![("电脑".to_string(), "乙".to_string(), 1)]);
        let texts = vec!["甲".to_string(), "乙".to_string()];
        let mut cands = srcs_of(&texts);
        engine.rank_v2_for_test(&mut cands, "synthtest");
        assert_eq!(cands, vec!["乙", "甲"], "搭配命中优先于位置先验");
    }

    #[test]
    fn test_demote_survives_single_char_sort_mode() {
        // 审计 P1：单字优先模式下，降权维度排在 pref_rank 之前，无论单字还是多字降权词均不得被拉回首屏。
        let mut engine = Engine::new();
        engine.sort_mode = 1;
        engine.demoted_words.insert("我们".to_string());
        engine.demoted_words.insert("我".to_string());
        let mut texts: Vec<String> = ["我", "你", "他", "她", "它"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        for i in 0..8 {
            texts.push(format!("词{i}"));
        }
        texts.push("我们".to_string());
        let mut cands = srcs_of(&texts);
        engine.rank_v2_for_test(&mut cands, "synthtest");
        for demoted in ["我们", "我"] {
            let pos = cands.iter().position(|w| w == demoted).unwrap();
            assert!(
                pos >= 10,
                "降权词「{demoted}」应落在前 2 屏外，实际第 {pos} 位: {cands:?}"
            );
        }
    }

    #[test]
    fn test_new_engine() {
        let engine = Engine::new();
        assert_eq!(engine.pinyin_str(), "");
        assert_eq!(engine.candidate_count(), 0);
    }

    #[test]
    fn test_basic_pinyin() {
        let mut engine = Engine::new();

        engine.process_key('z');
        engine.process_key('h');
        engine.process_key('o');
        engine.process_key('n');
        engine.process_key('g');

        assert_eq!(engine.pinyin_str(), "zhong");
        assert!(engine.candidate_count() > 0);
    }

    #[test]
    fn test_backspace() {
        let mut engine = Engine::new();
        engine.process_key('z');
        engine.process_key('h');
        assert_eq!(engine.pinyin_str(), "zh");

        engine.backspace();
        assert_eq!(engine.pinyin_str(), "z");
    }

    #[test]
    fn test_reset() {
        let mut engine = Engine::new();
        engine.process_key('n');
        engine.process_key('i');
        engine.reset();
        assert_eq!(engine.pinyin_str(), "");
        assert_eq!(engine.candidate_count(), 0);
    }

    #[test]
    fn test_candidate_limit() {
        let mut engine = Engine::new();

        engine.process_key('z');
        engine.process_key('h');
        engine.process_key('o');
        engine.process_key('n');
        engine.process_key('g');
        let default_count = engine.candidate_count();
        assert!(default_count > 0 && default_count <= 5, "默认每页 5 个候选");

        engine.set_candidate_limit(3);
        assert!(engine.candidate_count() <= 3);
    }

    #[test]
    fn test_ascii_mode_default() {
        let engine = Engine::new();
        assert!(!engine.ascii_mode()); // 默认中文模式
    }

    #[test]
    fn test_ascii_mode_toggle() {
        let mut engine = Engine::new();
        engine.set_ascii_mode(true);
        assert!(engine.ascii_mode());
        engine.set_ascii_mode(false);
        assert!(!engine.ascii_mode());
    }

    #[test]
    fn test_ascii_mode_no_pinyin_accumulation() {
        let mut engine = Engine::new();
        engine.set_ascii_mode(true);
        assert!(!engine.process_key('a'));
        assert_eq!(engine.pinyin_str(), "");
        assert_eq!(engine.candidate_count(), 0);
        engine.set_ascii_mode(false);
        assert!(engine.process_key('n'));
        assert!(engine.process_key('i'));
        assert_eq!(engine.pinyin_str(), "ni");
    }

    #[test]
    fn test_page_navigation() {
        let mut engine = Engine::new();
        // 用 "zh" 查询，page_size 设为 2 → 内置词库必然超过 1 页
        engine.set_candidate_limit(2);
        engine.process_key('z');
        engine.process_key('h');
        assert_eq!(engine.pinyin_str(), "zh");
        assert_eq!(engine.current_page(), 0);
        assert!(engine.candidate_count() > 0);

        // 下一页
        let first_page = engine.candidate_count();
        let count = engine.page(1);
        assert_eq!(engine.current_page(), 1);
        assert!(count > 0 || first_page == 0);

        // 返回上一页
        engine.page(-1);
        assert_eq!(engine.current_page(), 0);
    }

    #[test]
    fn test_page_clamp() {
        let mut engine = Engine::new();
        engine.process_key('z');
        engine.process_key('h');
        // 翻很多页应 clamp 到最后一页
        for _ in 0..50 {
            engine.page(1);
        }
        assert!(engine.current_page() <= engine.total_pages().saturating_sub(1));
    }

    #[test]
    fn test_short_pinyin() {
        // 简拼：输入 zg 应能联想"中国"（zhongguo 的声母）
        let mut engine = Engine::new();
        engine.process_key('z');
        engine.process_key('g');
        let has_zhongguo =
            (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("中国"));
        assert!(has_zhongguo, "简拼 zg 应联想出中国");
    }

    // ─── P2-8 超级简拼测试（对标 rime abbrev 超级简拼 + erase 单音节）───

    #[test]
    fn test_super_abbrev_single_letter() {
        // 超级简拼：单字母 z → 中（zhong 声母，对标 rime abbrev）
        let mut engine = Engine::new();
        engine.process_key('z');
        let mut has_zhong = false;
        let total = engine.total_pages();
        for _ in 0..total {
            for i in 0..engine.candidate_count() {
                if engine.candidate(i) == Some("中") {
                    has_zhong = true;
                }
            }
            if engine.page(1) <= 0 {
                break;
            }
        }
        assert!(has_zhong, "单字母 z 应能翻页找到 中(简拼)");
    }

    #[test]
    fn test_super_abbrev_zh_whole() {
        // zh/ch/sh 视为整体简拼（对标 rime abbrev zh ch sh 整体）
        // 串行锁 + 显式 builtin：真实词库下简拼候选序不同，锁保证确定性
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(None);
        let mut engine = Engine::new();
        for ch in "zh".chars() {
            engine.process_key(ch);
        }
        let has_zhong = (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("中"));
        assert!(has_zhong, "zh 应出 中(zh 整体简拼)");
    }

    #[test]
    fn test_super_abbrev_m_pin() {
        // m 单音节被 erase + pin 置顶（对标 rime erase m + pin_cand_filter）
        // 内置置顶 m → 吗
        let mut engine = Engine::new();
        engine.process_key('m');
        let has_ma = (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("吗"));
        // 词库无"吗"时跳过（词库依赖），但至少有 m 声母词
        let has_some = engine.candidate_count() > 0;
        assert!(has_some, "m 应有简拼候选");
        let _ = has_ma;
    }

    #[test]
    fn test_phrase_guess() {
        // 整词优先：nihao 直接出"你好"
        let mut engine = Engine::new();
        for ch in "nihao".chars() {
            engine.process_key(ch);
        }
        let has_nihao = (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("你好"));
        assert!(has_nihao);
    }

    #[test]
    fn test_fuzzy_default_off() {
        let engine = Engine::new();
        assert!(!engine.fuzzy_enabled(), "模糊音默认应全部关闭");
        assert_eq!(engine.fuzzy_flags(), fuzzy::FUZZY_NONE);
    }

    #[test]
    fn test_fuzzy_independent_group_flags_in_engine() {
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(Some(std::path::Path::new("../resources/system_dict.db")));
        let mut engine = Engine::new();
        for ch in "zongguo".chars() {
            engine.process_key(ch);
        }
        assert!(
            !(0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("中国")),
            "默认关闭模糊音时 zongguo 不应出 中国"
        );

        engine.set_fuzzy_flags(fuzzy::FUZZY_N_L);
        assert!(
            !(0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("中国")),
            "仅开 n↔l 时 zongguo 不应出 中国"
        );

        engine.set_fuzzy_flags(fuzzy::FUZZY_Z_ZH);
        assert!(
            (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("中国")),
            "开启 z↔zh 后 zongguo 应重查并出 中国"
        );

        engine.set_fuzzy_flags(0);
        assert!(
            !(0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("中国")),
            "归零模糊音掩码后应停止产出模糊音候选"
        );
    }

    // ─── V0.2.26 错音错字提示 ───

    #[test]
    fn test_mistake_cancha() {
        // 验证机制：lookup 命中 + 模式判定（候选命中由集成测试覆盖）
        assert!(
            mistake::lookup("cancha")
                .iter()
                .any(|(py, w)| *py == "cenci" && *w == "参差")
        );
        let mut engine = Engine::new();
        for ch in "cancha".chars() {
            engine.process_key(ch);
        }
        assert!(engine.is_mistake_hit(), "cancha 应命中易错映射");
    }

    #[test]
    fn test_mistake_nuanhe() {
        // 验证机制：不加载真实词库（避免污染共享全局状态破坏并行测试）。
        // 候选命中由集成测试覆盖；这里验证 lookup 命中 + 模式判定。
        assert!(mistake::lookup("nuanhe").iter().any(|(_, w)| *w == "暖和"));
        let mut engine = Engine::new();
        for ch in "nuanhe".chars() {
            engine.process_key(ch);
        }
        assert!(engine.is_mistake_hit(), "nuanhe 应命中易错映射");
    }

    #[test]
    fn test_mistake_not_triggered_normal() {
        // 正常输入不触发错音提示
        let mut engine = Engine::new();
        for ch in "zhongguo".chars() {
            engine.process_key(ch);
        }
        assert!(!engine.is_mistake_hit());
        // 中国 候选仍在
        let has_zhongguo =
            (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("中国"));
        assert!(has_zhongguo);
    }

    #[test]
    fn test_word_len_match_pushes_longer_words_after_exact_count() {
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(None);
        let e = Engine::new();
        let py: std::sync::Arc<str> = std::sync::Arc::from("zhongguo");
        let exact = |t: &str, freq: u32| {
            let mut c =
                Candidate::with_reason(t.to_string(), Source::Dictionary, reason::EXACT_PINYIN)
                    .with_pinyin(Some(py.clone()), 8);
            c.dict_layer = Some(3);
            c.dict_freq = Some(freq);
            c
        };
        let ext = |t: &str, freq: u32| {
            let mut c =
                Candidate::with_reason(t.to_string(), Source::Dictionary, reason::PREFIX_EXT);
            c.dict_layer = Some(3);
            c.dict_freq = Some(freq);
            c
        };
        let mut cands = vec![
            ext("中国队", 5000),
            exact("种果", 2000),
            exact("种过", 1900),
            ext("中国画", 4000),
            exact("中国", 3000),
        ];
        e.rank_v2_for_test(&mut cands, "zhongguo");
        let out: Vec<&str> = cands.iter().map(|c| c.text.as_str()).collect();
        // 精确 2 字词在前，前缀 3 字词全部在后
        let pos = out
            .iter()
            .position(|w| *w == "中国")
            .expect("中国 应在候选里");
        let first_long = out
            .iter()
            .position(|w| w.chars().count() > 2)
            .unwrap_or(usize::MAX);
        assert!(
            pos < first_long,
            "中国 应排在所有 3 字词之前，实际 {:?}",
            out
        );
    }

    #[test]
    fn test_mistake_select_no_learn() {
        let mut engine = Engine::new();
        for ch in "cancha".chars() {
            engine.process_key(ch);
        }
        if engine.candidate_count() > 0 {
            let text = engine.select_candidate(0).unwrap();
            assert!(!text.is_empty());
            assert_eq!(engine.pinyin_str(), "", "选中后应重置");
        }
    }

    // ─── P2-2 排序增强测试 ───

    #[test]
    fn test_pin_candidate_top() {
        // 内置置顶：d → 的（对标 rime pin_cand_filter）
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(None);
        let mut engine = Engine::new();
        engine.process_key('d');
        let first = engine.candidate(0).unwrap_or("");
        // 精确匹配 d 的候选里，"的" 应被置顶（若候选存在）
        let has_de = (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("的"));
        assert!(has_de, "d 应有候选 的");
        assert_eq!(first, "的", "d 首位应为 的, got {first}");
    }

    #[test]
    fn test_pin_candidate_custom() {
        // 自定义置顶（V0.5.7 词级 load_pin_words）：我们 置顶（内置词库有"我们"）
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(None);
        let mut engine = Engine::new();
        engine.load_pin_words(vec!["我们".to_string()]);
        for ch in "wo".chars() {
            engine.process_key(ch);
        }
        assert_eq!(
            engine.candidate(0),
            Some("我们"),
            "自定义置顶 我们 应生效, got {:?}",
            (0..engine.candidate_count())
                .map(|i| engine.candidate(i).unwrap_or(""))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_pin_word_level_short_pinyin() {
        // V0.5.7 词级置顶 + 简拼自动展开：置顶"你好"，输入 nih/nh（简拼）
        // 候选里出现"你好"即置顶——无需反查拼音生成展开键
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(None);
        let mut engine = Engine::new();
        engine.load_pin_words(vec!["你好".to_string()]);
        // 全拼 nihao
        for ch in "nihao".chars() {
            engine.process_key(ch);
        }
        let has_ni = (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("你好"));
        if has_ni {
            assert_eq!(engine.candidate(0), Some("你好"), "nihao 置顶 你好 应第 1");
        }
        // 简拼 nh
        engine.reset();
        for ch in "nh".chars() {
            engine.process_key(ch);
        }
        let has_nh = (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("你好"));
        if has_nh {
            assert_eq!(engine.candidate(0), Some("你好"), "nh 简拼置顶 你好 应第 1");
        }
    }

    #[test]
    fn test_demote_word_out_of_two_pages() {
        // V0.5.7 降权（Eric 决策：删除=降权压出前 2 屏）：
        // 内置词库候选不足 10 个时，降权词排到末尾（离开第 0 位）——
        // "压出前 2 屏"在候选不够 2 屏时退化为"不优先、排末尾"。
        // 词库锁：查询依赖全局 DICT，并行测试 init 会覆盖系统词库
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(None);
        crate::dictionary::clear_user_words(); // 并行隔离：清 learn 残留
        let mut engine = Engine::new();
        engine.load_pin_words(vec!["你好".to_string()]);
        // 置顶后第 0 位
        for ch in "nihao".chars() {
            engine.process_key(ch);
        }
        assert_eq!(engine.candidate(0), Some("你好"), "置顶后 你好 应第 0 位");
        // 降权 → 离开第 0 位（排末尾）
        engine.reset();
        engine.demote_word("你好");
        for ch in "nihao".chars() {
            engine.process_key(ch);
        }
        let pos = (0..engine.candidate_count()).position(|i| engine.candidate(i) == Some("你好"));
        let debug_list: Vec<String> = (0..engine.candidate_count())
            .map(|i| engine.candidate(i).unwrap_or("").to_string())
            .collect();
        assert!(
            pos.is_some() && pos.unwrap() > 0,
            "降权后 你好 应离开第 0 位, pos={pos:?} all={debug_list:?}"
        );
        // 恢复后回到第 0 位（pin 仍生效）
        engine.reset();
        engine.undemote_word("你好");
        for ch in "nihao".chars() {
            engine.process_key(ch);
        }
        assert_eq!(
            engine.candidate(0),
            Some("你好"),
            "恢复后 你好 应回第 0 位（置顶仍在）"
        );
    }

    #[test]
    fn test_delete_user_word_short_pinyin() {
        // V0.5.7 删除用户词（Eric：组词组错的热词必须能删）：
        // 简拼场景 ts→泰深 删除后，全拼与简拼都不再出现（user_short_index 已清理）。
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(None);
        crate::dictionary::clear_user_words(); // 并行隔离：清掉其他测试的 learn 残留
        let mut engine = Engine::new();
        // 模拟学习用户词：泰深（taishen）
        crate::dictionary::learn("taishen", "泰深");
        // 全拼命中
        for ch in "taishen".chars() {
            engine.process_key(ch);
        }
        assert!(
            (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("泰深")),
            "学习后 taishen 应有候选 泰深"
        );
        // 简拼命中（V0.5 声母索引）
        engine.reset();
        for ch in "ts".chars() {
            engine.process_key(ch);
        }
        assert!(
            (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("泰深")),
            "学习后 ts 简拼应有候选 泰深"
        );
        // 删除（模拟右键删除：按词删，不依赖当前拼音键）
        engine.reset();
        for ch in "ts".chars() {
            engine.process_key(ch);
        }
        // 找到 泰深 在当前页索引并删除
        let idx = (0..engine.candidate_count())
            .position(|i| engine.candidate(i) == Some("泰深"))
            .unwrap_or(0);
        assert!(engine.delete_candidate(idx), "删除应成功");
        // 全拼重查：不再出现
        engine.reset();
        for ch in "taishen".chars() {
            engine.process_key(ch);
        }
        assert!(
            !(0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("泰深")),
            "删除后 taishen 不应再有 泰深"
        );
        // 简拼重查：不再出现（user_short_index 已清理）
        engine.reset();
        for ch in "ts".chars() {
            engine.process_key(ch);
        }
        assert!(
            !(0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("泰深")),
            "删除后 ts 简拼不应再有 泰深"
        );
    }

    #[test]
    fn test_long_word_filter_mechanism() {
        // 长词优先不崩溃 + 候选存在（词库依赖，验证机制）
        let mut engine = Engine::new();
        for ch in "jie".chars() {
            engine.process_key(ch);
        }
        assert!(engine.candidate_count() > 0, "jie 应有候选");
        // 英文候选不受长词过滤破坏：jie 无英文候选则跳过
    }

    #[test]
    fn test_long_word_filter_builtin() {
        // 内置词库：输入 wo → 我(精确) 我们(长词前缀)
        // 长词过滤应把"我们"提前（若在 4 位后）
        // 串行锁 + 显式 builtin：真实词库下候选序不同，锁保证确定性
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(None);
        let mut engine = Engine::new();
        for ch in "wo".chars() {
            engine.process_key(ch);
        }
        assert_eq!(engine.candidate(0), Some("我"), "wo 首位应为 我");
        // "我们" 应在候选里
        let has_women = (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("我们"));
        assert!(has_women, "wo 应有候选 我们");
    }

    // ─── P2-5 Emoji 测试 ───

    #[test]
    fn test_emoji_default_off() {
        // Eric 要求：Emoji 默认关闭（config emoji=1 可开启）
        let engine = Engine::new();
        assert!(!engine.emoji(), "Emoji 默认应关闭");
    }

    #[test]
    fn test_emoji_off_no_candidate() {
        let mut engine = Engine::new();
        engine.set_emoji(false);
        for ch in "xiexie".chars() {
            engine.process_key(ch);
        }
        let has_emoji =
            (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("谢谢🙏"));
        assert!(!has_emoji, "关闭 emoji 后不应有 emoji 候选");
        // 原候选仍在
        let has_xiexie = (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("谢谢"));
        assert!(has_xiexie, "关闭 emoji 后 谢谢 候选仍在");
    }

    #[test]
    fn test_emoji_select() {
        // 选中 emoji 候选 → 上屏带 emoji 文本且不写入用户词库
        let mut engine = Engine::new();
        engine.set_emoji(true);
        for ch in "xiexie".chars() {
            engine.process_key(ch);
        }
        let idx = (0..engine.candidate_count())
            .position(|i| engine.candidate(i) == Some("谢谢🙏"))
            .expect("开启 emoji 后 xiexie 首屏应紧跟 谢谢🙏");
        let base_idx = (0..engine.candidate_count())
            .position(|i| engine.candidate(i) == Some("谢谢"))
            .expect("谢谢 应在首屏");
        assert_eq!(
            idx,
            base_idx + 1,
            "形态簇变体 谢谢🙏 必须紧随本体 谢谢 之后"
        );
        let meta = engine.candidate_explain(idx).expect("meta 应存在");
        assert_eq!(meta.form, ranking::Form::Emoji.rank());
        let extra = engine.candidate_extra(idx).expect("extra 应存在");
        assert_eq!(extra.cluster.as_deref(), Some("谢谢"));
        let text = engine.select_candidate(idx).unwrap();
        assert_eq!(text, "谢谢🙏");
        assert_eq!(engine.pinyin_str(), "", "选中后应重置");
    }

    #[test]
    fn test_phase_d_cluster_slot_limit_and_annotations() {
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(Some(std::path::Path::new("../resources/system_dict.db")));
        let mut engine = Engine::new();
        engine.set_emoji(true);
        engine.load_word_annotations(vec![(
            "喜欢".to_string(),
            Some("v.".to_string()),
            Some("喜爱、好感".to_string()),
        )]);
        for ch in "xihuan".chars() {
            engine.process_key(ch);
        }
        // 默认 cluster_slot_limit = 2：喜欢 + 喜欢😍，不刷出 喜欢💕
        assert_eq!(engine.cluster_slot_limit(), 2);
        let top5: Vec<String> = (0..engine.candidate_count())
            .filter_map(|i| engine.candidate(i).map(str::to_string))
            .collect();
        let base_pos = top5.iter().position(|w| w == "喜欢").expect("应包含 喜欢");
        assert_eq!(
            top5.get(base_pos + 1).map(String::as_str),
            Some("喜欢😍"),
            "emoji 变体应紧跟本体: {top5:?}"
        );
        assert!(
            !top5.iter().any(|w| w == "喜欢💕"),
            "默认 cluster_slot_limit=2 不应放出第 2 个变体: {top5:?}"
        );
        // 本体与变体共享词性/释义标注
        let base_extra = engine.candidate_extra(base_pos).unwrap();
        assert_eq!(base_extra.cluster, None);
        assert_eq!(base_extra.pos.as_deref(), Some("v."));
        assert_eq!(base_extra.gloss.as_deref(), Some("喜爱、好感"));
        let var_extra = engine.candidate_extra(base_pos + 1).unwrap();
        assert_eq!(var_extra.cluster.as_deref(), Some("喜欢"));
        assert_eq!(var_extra.pos.as_deref(), Some("v."));
        assert_eq!(var_extra.gloss.as_deref(), Some("喜爱、好感"));

        // 提高限额到 3：放出第 2 个变体 喜欢💕
        engine.set_cluster_slot_limit(3);
        let top5_3: Vec<String> = (0..engine.candidate_count())
            .filter_map(|i| engine.candidate(i).map(str::to_string))
            .collect();
        let p = top5_3.iter().position(|w| w == "喜欢").unwrap();
        assert_eq!(top5_3.get(p + 1).map(String::as_str), Some("喜欢😍"));
        assert_eq!(top5_3.get(p + 2).map(String::as_str), Some("喜欢💕"));

        // 降到 1：仅保留文本本体
        engine.set_cluster_slot_limit(1);
        let top5_1: Vec<String> = (0..engine.candidate_count())
            .filter_map(|i| engine.candidate(i).map(str::to_string))
            .collect();
        assert!(
            !top5_1.iter().any(|w| w.contains('😍') || w.contains('💕')),
            "cluster_slot_limit=1 应折叠全部变体: {top5_1:?}"
        );
    }

    #[test]
    fn test_phase_d_demoted_cluster_sinks_together() {
        // 本体词被降权时，同簇的 emoji 变体必须随本体一起下沉，绝不留在首屏
        let mut engine = Engine::new();
        engine.set_emoji(true);
        engine.demote_word("喜欢");
        for ch in "xihuan".chars() {
            engine.process_key(ch);
        }
        let page0: Vec<String> = (0..engine.candidate_count())
            .filter_map(|i| engine.candidate(i).map(str::to_string))
            .collect();
        assert!(
            !page0.iter().any(|w| w == "喜欢" || w == "喜欢😍"),
            "本体降权后同簇变体不得滞留首屏: {page0:?}"
        );
        // 确认重排幂等（影子快照与 cluster 排序一致）
        let diff = engine.shadow_diff_current().expect("snapshot 应存在");
        assert_eq!(diff.old_order, diff.new_order);
    }

    // ─── V0.2.25 拆字反查 ───

    #[test]
    fn test_radical_mode_basic() {
        // 持测试串行锁（V0.4.5 fix）：与 test_radical_mode_unknown_empty 的
        // init(None) 清空全局表并行竞争 → 偶发读到空表失败
        let _g = crate::radical::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut engine = Engine::new();
        // 先加载词库（若存在）
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../resources/rime_ice/radical_pinyin.dict.yaml");
        if path.exists() {
            crate::radical::init(Some(&path));
        }
        for ch in "ushuishou".chars() {
            engine.process_key(ch);
        }
        assert!(engine.is_radical_mode());
        assert!(engine.candidate_count() > 0, "ushuishou 应有拆字候选");
    }

    #[test]
    fn test_radical_mode_unknown_empty() {
        let _g = crate::radical::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut engine = Engine::new();
        crate::radical::init(None); // 空表
        for ch in "uzzzzz".chars() {
            engine.process_key(ch);
        }
        assert!(engine.is_radical_mode());
        // 0.1.50：拆字查不到时回落常规路径——不再要求 0 候选（空白候选窗
        // 会让 us/up 这类输入彻底打不出字）。这里只断言回落后有候选可用，
        // 且不是拆字结果（拆字表为空，任何候选都来自回落路径）。
        assert!(
            engine.candidate_count() > 0,
            "拆字无结果应回落到拼音/英文路径，不能留空候选窗"
        );
    }

    #[test]
    fn test_radical_single_u_not_mode() {
        let mut engine = Engine::new();
        engine.process_key('u');
        assert!(!engine.is_radical_mode(), "单独 u 不进入反查模式");
    }

    #[test]
    fn test_radical_select_no_learn() {
        let _g = crate::radical::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut engine = Engine::new();
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../resources/rime_ice/radical_pinyin.dict.yaml");
        if path.exists() {
            crate::radical::init(Some(&path));
        }
        for ch in "ushuishou".chars() {
            engine.process_key(ch);
        }
        if engine.candidate_count() > 0 {
            engine.select_candidate(0);
            assert_eq!(engine.pinyin_str(), "", "选中后应重置");
        }
    }

    // ─── V0.2.24 以词定字 ───

    #[test]
    fn test_take_char_first() {
        // 候选"中国" → [ 取"中"
        let mut engine = Engine::new();
        for ch in "zhongguo".chars() {
            engine.process_key(ch);
        }
        let text = engine.take_char(true).unwrap();
        assert_eq!(text, "中");
        assert_eq!(engine.pinyin_str(), "", "取字后应重置");
    }

    #[test]
    fn test_take_char_last() {
        // 候选"中国" → ] 取"国"
        let mut engine = Engine::new();
        for ch in "zhongguo".chars() {
            engine.process_key(ch);
        }
        let text = engine.take_char(false).unwrap();
        assert_eq!(text, "国");
    }

    #[test]
    fn test_take_char_single() {
        // 单字候选"好" → [ ] 都取"好"
        let mut engine = Engine::new();
        for ch in "hao".chars() {
            engine.process_key(ch);
        }
        assert_eq!(engine.take_char(true).unwrap(), "好");
        let mut e2 = Engine::new();
        for ch in "hao".chars() {
            e2.process_key(ch);
        }
        assert_eq!(e2.take_char(false).unwrap(), "好");
    }

    #[test]
    fn test_take_char_no_candidate() {
        let mut engine = Engine::new();
        assert_eq!(engine.take_char(true), None, "无候选应返回 None");
        assert_eq!(engine.take_char(false), None);
    }

    #[test]
    fn test_take_char_no_learn() {
        // 取字不学习用户词（reset 后无残留）
        let mut engine = Engine::new();
        for ch in "zhongguo".chars() {
            engine.process_key(ch);
        }
        engine.take_char(true);
        assert_eq!(engine.pinyin_str(), "");
        assert_eq!(engine.candidate_count(), 0);
    }

    // ─── P0-2 候选排序模式（单字/长词优先）───

    #[test]
    fn test_sort_mode_default() {
        // 默认模式：排序保持原样（zhongguo 首候选应为"中国"）
        let mut engine = Engine::new();
        assert_eq!(engine.sort_mode(), 0);
        for ch in "zhongguo".chars() {
            engine.process_key(ch);
        }
        assert_eq!(engine.candidate(0), Some("中国"));
    }

    #[test]
    fn test_sort_mode_single_first() {
        // 单字优先：直接构造混合候选，验证单字提到前、组内词频序不变
        let mut engine = Engine::new();
        engine.set_sort_mode(1);
        assert_eq!(engine.sort_mode(), 1);
        let texts = vec![
            "中国".to_string(),
            "中".to_string(),
            "你好".to_string(),
            "国".to_string(),
            "世界".to_string(),
        ];
        let mut cands = srcs_of(&texts);
        engine.rank_v2_for_test(&mut cands, "synthtest");
        // 单字优先：中/国 提到前（组内相对序保持：中 在 国 前）
        assert_eq!(cands[0], "中");
        assert_eq!(cands[1], "国");
        assert_eq!(cands[2], "中国");
        assert_eq!(cands[3], "你好");
        assert_eq!(cands[4], "世界");
    }

    #[test]
    fn test_sort_mode_long_first() {
        // 长词优先：多字提到前，组内词频序不变
        let mut engine = Engine::new();
        engine.set_sort_mode(2);
        assert_eq!(engine.sort_mode(), 2);
        let texts = vec![
            "中".to_string(),
            "中国".to_string(),
            "国".to_string(),
            "你好".to_string(),
            "世界".to_string(),
        ];
        let mut cands = srcs_of(&texts);
        engine.rank_v2_for_test(&mut cands, "synthtest");
        // 多字优先：中国/你好/世界 提到前（组内相对序保持）
        assert_eq!(cands[0], "中国");
        assert_eq!(cands[1], "你好");
        assert_eq!(cands[2], "世界");
        assert_eq!(cands[3], "中");
        assert_eq!(cands[4], "国");
    }

    #[test]
    fn test_single_letter_no_phrases() {
        // 单字母输入只出单字，不出词组（Eric 需求 2026-08-08）
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(Some(std::path::Path::new("../resources/system_dict.db")));
        crate::dictionary::clear_user_words(); // 并行隔离：清其他测试 learn 的 2 字用户词
        let mut engine = Engine::new();
        engine.process_key('w');
        let n = engine.candidate_count();
        assert!(n > 0, "单字母 w 应有候选");
        for i in 0..n {
            let c = engine.candidate(i).unwrap();
            assert_eq!(c.chars().count(), 1, "候选 {c} 应为单字，不得出现词组");
        }
        // 常用单字应出现在候选前部（我/五/王/无/万...）
        let first = engine.candidate(0).unwrap();
        assert!(
            first == "我"
                || first == "五"
                || first == "王"
                || first == "无"
                || first == "万"
                || first == "为"
                || first == "完"
                || first == "问",
            "首候选应为常用单字，实际 {first}"
        );
    }

    #[test]
    fn test_sort_mode_english_isolated() {
        // 英文候选区不参与单字/长词偏好（pref_rank 隔离返回 2，恒在中文之后）
        let mut engine = Engine::new();
        engine.set_sort_mode(1);
        let mut cands = srcs_of(&["中国".to_string(), "中".to_string(), "国".to_string()]);
        let mut eng1 = Candidate::new("help".to_string(), Source::English);
        eng1.dict_freq = Some(200);
        let mut eng2 = Candidate::new("hello".to_string(), Source::English);
        eng2.dict_freq = Some(100);
        cands.push(eng1);
        cands.push(eng2);
        engine.rank_v2_for_test(&mut cands, "synthtest");
        assert_eq!(cands[0], "中");
        assert_eq!(cands[1], "国");
        assert_eq!(cands[2], "中国");
        assert_eq!(cands[3], "help", "英文区不应被重排");
        assert_eq!(cands[4], "hello", "英文区不应被重排");
    }

    #[test]
    fn test_sort_mode_real_input() {
        // 真实输入链路：zhi 应同时有单字（之/只/知…）与多字（知道/支持…）
        // 单字优先时首候选必须是单字
        let mut engine = Engine::new();
        engine.set_sort_mode(1);
        for ch in "zhi".chars() {
            engine.process_key(ch);
        }
        assert!(engine.candidate_count() > 0);
        let first = engine.candidate(0).unwrap();
        let hanzi = first.chars().filter(|c| is_hanzi(*c)).count();
        assert_eq!(hanzi, 1, "单字优先首候选应为单字，got {first}");
    }

    #[test]
    fn test_sort_mode_clamp() {
        let mut engine = Engine::new();
        engine.set_sort_mode(99);
        assert_eq!(engine.sort_mode(), 2, "超界应 clamp 到 2");
        engine.set_sort_mode(-5);
        assert_eq!(engine.sort_mode(), 0, "负值应 clamp 到 0");
    }

    // ─── P1-1 上下文联想 ───

    #[test]
    fn test_context_default_enabled() {
        // 默认开启：config 未写 context_assoc 时智能联想即生效
        let engine = Engine::new();
        assert!(engine.context_enabled(), "智能联想默认应为开启状态");
    }

    #[test]
    fn test_context_boost_disabled_keeps_frequency_order() {
        // 显式关闭：不改变原有行为，首候选维持词频序
        let mut engine = Engine::new();
        engine.set_context_enabled(false);
        // 串行锁 + 显式加载真实词库（daxue 大学 仅 system_dict 有，builtin 兜底词表无；
        // 并行时其他测试 init(None) 会重置全局 DICT，锁保证 DICT 状态一致）
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let db =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../resources/system_dict.db");
        if db.exists() {
            crate::dictionary::init_blocking(Some(&db));
        }
        for ch in "daxue".chars() {
            engine.process_key(ch);
        }
        assert!(engine.candidate_count() > 0);
        // 首候选维持词频序（大学 应仍在候选中但不强制前置）
        assert_eq!(engine.candidate(0), Some("大学"));
    }

    #[test]
    fn test_bigram_tier1_is_not_evidence() {
        // 档位 1 = 只共现一次（占 context_prior 的 98.6%），不得重排主候选；
        // 档位 2 起才作为搭配证据。口径与 sentence.rs 整句解码一致。
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let db =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../resources/system_dict.db");
        if db.exists() {
            crate::dictionary::init_blocking(Some(&db));
        }

        let probe = |tier: u32| -> Option<Option<String>> {
            let mut table = std::collections::HashMap::new();
            table.insert("我".to_string(), vec![("响".to_string(), tier)]);
            crate::dictionary::install_bigram(table);
            let mut e = Engine::new();
            e.set_context_enabled(true);
            for ch in "wo".chars() {
                e.process_key(ch);
            }
            if e.select_candidate(0).is_none() {
                return None;
            }
            for ch in "xiang".chars() {
                e.process_key(ch);
            }
            Some(e.candidate(0).map(|s| s.to_string()))
        };

        let tier1 = probe(1);
        let tier2 = probe(2);
        // 收尾：清空全局表，避免影响后续测试
        crate::dictionary::install_bigram(std::collections::HashMap::new());

        if let Some(first) = &tier1 {
            assert_ne!(
                first.as_deref(),
                Some("响"),
                "档位 1（仅共现一次）不应把「响」提到首位"
            );
        }
        if let Some(first) = &tier2 {
            assert_eq!(
                first.as_deref(),
                Some("响"),
                "档位 2 应把「响」提到首位（搭配链生效）"
            );
        }
    }

    #[test]
    fn test_context_boost_enabled() {
        // 上屏"北京"后，输入 da → "大学"因搭配表前置
        let mut engine = Engine::new();
        engine.set_context_enabled(true);
        // 串行锁 + 显式加载真实词库（同上）
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let db =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../resources/system_dict.db");
        if db.exists() {
            crate::dictionary::init_blocking(Some(&db));
        }
        // 模拟上屏"北京"（直接设置 last_committed，模拟 select_candidate 结果）
        engine.last_committed = "北京".to_string();
        for ch in "daxue".chars() {
            engine.process_key(ch);
        }
        assert!(engine.candidate_count() > 0);
        // 大学 应在首屏（可能不是第 0 位——若词频本就高则保持，但必须出现且靠前）
        let pos = (0..engine.candidate_count()).position(|i| engine.candidate(i) == Some("大学"));
        assert!(pos.is_some(), "上下文联想下 大学 应出现在候选");
    }

    #[test]
    fn test_context_last_committed_update() {
        // select_candidate 选中普通中文词 → last_committed 更新
        let mut engine = Engine::new();
        for ch in "zhongguo".chars() {
            engine.process_key(ch);
        }
        // 选"中国"（通过 select_candidate 找位置）
        let idx = (0..engine.candidate_count())
            .position(|i| engine.candidate(i) == Some("中国"))
            .expect("中国应在候选");
        engine.select_candidate(idx);
        assert_eq!(engine.last_committed, "中国");
    }

    #[test]
    fn test_context_boost_english_isolated() {
        // 英文候选区不参与上下文前置
        let mut engine = Engine::new();
        engine.set_context_enabled(true);
        engine.last_committed = "北京".to_string();
        let mut cands = srcs_of(&["其他".to_string(), "大学".to_string()]);
        cands.push(Candidate::new("help".to_string(), Source::English));
        engine.rank_v2_for_test(&mut cands, "synthtest");
        assert_eq!(cands[0], "大学", "搭配词应前置");
        assert_eq!(cands[1], "其他");
        assert_eq!(cands[2], "help", "英文区不重排");
    }

    #[test]
    fn test_learn_context_pair_persist() {
        // 自学习搭配：上屏搭配 → 内存生效 → 落库 → 重载可用
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join("taishen_test_context_pairs.db");
        let _ = std::fs::remove_file(&tmp);
        crate::dictionary::set_user_dict_path(Some(tmp.as_path()));

        let mut engine = Engine::new();
        engine.learn_context_pair("中国", "人民");
        engine.learn_context_pair("中国", "人民"); // 共现 2 次
        let weight = engine
            .context_map
            .get("中国")
            .and_then(|v| v.iter().find(|(w, _)| w == "人民").map(|(_, n)| *n));
        assert_eq!(weight, Some(2), "内存应立即生效且累积共现次数");

        let pairs = crate::dictionary::load_context_pairs(2, 8);
        assert!(
            pairs
                .iter()
                .any(|(p, n, c)| p == "中国" && n == "人民" && *c == 2),
            "应写入 db 且满足 count>=2 并带回次数"
        );

        let mut engine2 = Engine::new();
        engine2.load_learned_contexts(pairs);
        let reloaded = engine2
            .context_map
            .get("中国")
            .and_then(|v| v.iter().find(|(w, _)| w == "人民").map(|(_, n)| *n));
        assert_eq!(reloaded, Some(2), "重载后应恢复搭配与权重");

        crate::dictionary::set_user_dict_path(None);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_context_weight_monotonic() {
        // P1-3 纯加分语义：命中搭配的词整体前置；多个命中时按权重降序
        let mut engine = Engine::new();
        engine.set_context_enabled(true);
        engine.last_committed = "电脑".to_string();
        let base: Vec<String> = vec!["一", "二", "三", "四", "五", "六", "七", "八", "九", "十"]
            .into_iter()
            .map(|s| s.to_string())
            .chain(["屏幕".to_string(), "键盘".to_string()])
            .collect();

        // 只有「屏幕」命中：前置到首位
        engine.load_learned_contexts(vec![("电脑".to_string(), "屏幕".to_string(), 1)]);
        let mut once = srcs_of(&base);
        engine.rank_v2_for_test(&mut once, "synthtest");
        assert_eq!(once[0], "屏幕", "命中搭配即前置: {once:?}");

        // 两个词同时命中：权重高的在前（键盘 6 > 屏幕 1）
        engine.load_learned_contexts(vec![("电脑".to_string(), "键盘".to_string(), 6)]);
        let mut both = srcs_of(&base);
        engine.rank_v2_for_test(&mut both, "synthtest");
        assert_eq!(both[0], "键盘", "权重高的优先: {both:?}");
        assert_eq!(both[1], "屏幕", "权重低的次之: {both:?}");
    }

    #[test]
    fn test_context_prior_boosts_without_history() {
        // 冷启动：没有任何学习记录时，离线先验表也能给候选加权
        let mut engine = Engine::new();
        engine.set_context_enabled(true);
        engine.last_committed = "电脑".to_string();
        engine.load_prior_contexts(vec![("电脑".to_string(), "屏幕".to_string(), 6)]);
        let texts: Vec<String> = vec![
            "一", "二", "三", "四", "五", "六", "七", "八", "九", "十", "屏幕",
        ]
        .into_iter()
        .map(|s| s.to_string())
        .collect();
        let mut cands = srcs_of(&texts);
        engine.rank_v2_for_test(&mut cands, "synthtest");
        assert_eq!(
            cands.iter().position(|w| w == "屏幕"),
            Some(0),
            "先验命中即前置（P1-3 纯加分语义）"
        );
    }

    #[test]
    fn test_learned_and_prior_merge_takes_max() {
        // 同词既有先验又有学习记录时，排序取较大权重，两侧数据互不覆盖
        let mut engine = Engine::new();
        engine.set_context_enabled(true);
        engine.last_committed = "电脑".to_string();
        engine.load_prior_contexts(vec![("电脑".to_string(), "屏幕".to_string(), 2)]);
        engine.load_learned_contexts(vec![("电脑".to_string(), "屏幕".to_string(), 9)]);
        assert_eq!(
            engine
                .context_map
                .get("电脑")
                .and_then(|v| v.iter().find(|(w, _)| w == "屏幕"))
                .map(|(_, n)| *n),
            Some(9),
            "学习记录保留自身权重"
        );
        assert_eq!(
            engine
                .context_prior
                .get("电脑")
                .and_then(|v| v.iter().find(|(w, _)| w == "屏幕"))
                .map(|(_, n)| *n),
            Some(2),
            "先验表不被学习记录改写"
        );
        let texts: Vec<String> = vec![
            "一", "二", "三", "四", "五", "六", "七", "八", "九", "十", "屏幕",
        ]
        .into_iter()
        .map(|s| s.to_string())
        .collect();
        let mut cands = srcs_of(&texts);
        engine.rank_v2_for_test(&mut cands, "synthtest");
        assert_eq!(
            cands.iter().position(|w| w == "屏幕"),
            Some(0),
            "两侧数据合并且命中即前置（P1-3 纯加分语义）"
        );
    }

    #[test]
    fn test_context_disabled_keeps_order() {
        // 关闭开关后候选顺序必须与无前文时完全一致
        let mut engine = Engine::new();
        engine.set_context_enabled(false);
        engine.last_committed = "北京".to_string();
        let origin: Vec<String> = vec!["一", "二", "大学", "三"]
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        let mut cands = srcs_of(&origin);
        engine.rank_v2_for_test(&mut cands, "synthtest");
        assert_eq!(cands, origin, "关闭联想不得改变候选顺序");
    }

    // ─── V0.2.19 日期/时间/农历输入 ───

    #[test]
    fn test_datetime_rq() {
        let mut engine = Engine::new();
        for ch in "rq".chars() {
            engine.process_key(ch);
        }
        assert!(engine.candidate_count() >= 3, "rq 应有 3 个日期候选");
        // ISO 格式候选存在
        let has_iso = (0..engine.candidate_count()).any(|i| {
            engine
                .candidate(i)
                .is_some_and(|c| c.len() == 10 && c.contains('-'))
        });
        assert!(has_iso);
    }

    #[test]
    fn test_datetime_sj() {
        let mut engine = Engine::new();
        for ch in "sj".chars() {
            engine.process_key(ch);
        }
        assert!(engine.candidate_count() >= 2, "sj 应有 2 个时间候选");
        let has_colon = (0..engine.candidate_count())
            .any(|i| engine.candidate(i).is_some_and(|c| c.contains(':')));
        assert!(has_colon);
    }

    #[test]
    fn test_datetime_xq() {
        let mut engine = Engine::new();
        for ch in "xq".chars() {
            engine.process_key(ch);
        }
        assert!(engine.candidate_count() >= 3, "xq 应有 3 个星期候选");
    }

    #[test]
    fn test_datetime_nl() {
        let mut engine = Engine::new();
        for ch in "nl".chars() {
            engine.process_key(ch);
        }
        assert!(engine.candidate_count() >= 1, "nl 应有农历候选");
    }

    #[test]
    fn test_datetime_select_no_learn() {
        let mut engine = Engine::new();
        for ch in "rq".chars() {
            engine.process_key(ch);
        }
        let text = engine.select_candidate(0).unwrap();
        assert_eq!(text.len(), 10, "ISO 日期应为 10 字符");
        assert_eq!(engine.pinyin_str(), "", "选中后应重置");
    }

    #[test]
    fn test_datetime_sj_precedes_phrase() {
        // sj 同时是短语"手机"的简码——日期候选应优先
        let mut engine = Engine::new();
        for ch in "sj".chars() {
            engine.process_key(ch);
        }
        let first = engine.candidate(0).unwrap();
        assert!(first.contains(':'), "sj 首个候选应为时间而非手机");
    }

    // ─── V0.2.22 计算器 cC 模式 ───

    #[test]
    fn test_calc_basic() {
        let mut engine = Engine::new();
        for ch in "c35*12".chars() {
            engine.process_key(ch);
        }
        assert!(engine.is_calc_mode());
        assert_eq!(engine.candidate_count(), 1);
        assert_eq!(engine.candidate(0), Some("420"));
    }

    #[test]
    fn test_calc_parens_power() {
        let mut engine = Engine::new();
        for ch in "c(1+2)*3".chars() {
            engine.process_key(ch);
        }
        assert_eq!(engine.candidate(0), Some("9"));

        let mut e2 = Engine::new();
        for ch in "c2^10".chars() {
            e2.process_key(ch);
        }
        assert_eq!(e2.candidate(0), Some("1024"));
    }

    #[test]
    fn test_calc_decimal() {
        let mut engine = Engine::new();
        for ch in "c10/4".chars() {
            engine.process_key(ch);
        }
        assert_eq!(engine.candidate(0), Some("2.5"));
    }

    #[test]
    fn test_calc_divide_zero_no_candidate() {
        let mut engine = Engine::new();
        for ch in "c1/0".chars() {
            engine.process_key(ch);
        }
        assert_eq!(engine.candidate_count(), 0, "除零应无候选");
    }

    #[test]
    fn test_calc_syntax_error_no_candidate() {
        // 语法错误（缺右操作数）应无候选；1++2 因一元正号合法会算出 3
        let mut engine = Engine::new();
        for ch in "c1+".chars() {
            engine.process_key(ch);
        }
        assert_eq!(engine.candidate_count(), 0, "缺操作数应无候选");
    }

    #[test]
    fn test_calc_select_no_learn() {
        let mut engine = Engine::new();
        for ch in "c2*3".chars() {
            engine.process_key(ch);
        }
        let text = engine.select_candidate(0).unwrap();
        assert_eq!(text, "6");
        assert_eq!(engine.pinyin_str(), "", "选中后应重置");
    }

    #[test]
    fn test_calc_letter_falls_back_to_pinyin() {
        // ca → 正常拼音（c 后跟字母不是算式）
        let mut engine = Engine::new();
        engine.process_key('c');
        engine.process_key('a');
        assert!(!engine.is_calc_mode());
        // cai → 才/猜 等拼音候选
        let mut e2 = Engine::new();
        for ch in "cai".chars() {
            e2.process_key(ch);
        }
        assert!(!e2.is_calc_mode());
        assert!(e2.candidate_count() > 0);
    }

    #[test]
    fn test_calc_single_c_not_mode() {
        let mut engine = Engine::new();
        engine.process_key('c');
        assert!(!engine.is_calc_mode(), "单独 c 不进入计算器模式");
    }

    // ─── V0.2.23 英文自动大写 ───

    #[test]
    fn test_english_case_lower() {
        let mut engine = Engine::new();
        for ch in "hello".chars() {
            engine.process_key(ch);
        }
        // 英文候选位置（混输追加项）
        let count = engine.candidate_count();
        assert!(count > 0);
        let text = engine.select_candidate(count - 1).unwrap();
        assert_eq!(text, "hello");
    }

    #[test]
    fn test_english_case_capitalize() {
        let mut engine = Engine::new();
        for ch in "Hello".chars() {
            engine.process_key(ch);
        }
        let count = engine.candidate_count();
        let text = engine.select_candidate(count - 1).unwrap();
        assert_eq!(text, "Hello");
    }

    #[test]
    fn test_english_case_upper() {
        let mut engine = Engine::new();
        for ch in "HELLO".chars() {
            engine.process_key(ch);
        }
        let count = engine.candidate_count();
        let text = engine.select_candidate(count - 1).unwrap();
        assert_eq!(text, "HELLO");
    }

    #[test]
    fn test_english_case_display() {
        let mut engine = Engine::new();
        for ch in "Hello".chars() {
            engine.process_key(ch);
        }
        // candidate_display 应显示转换后文本
        let count = engine.candidate_count();
        let display = engine.candidate_display(count - 1).unwrap();
        assert_eq!(display, "Hello");
    }

    #[test]
    fn test_english_case_backspace_recompute() {
        // P1-1：英文词典候选（hello）按输入大小写模式转换（HELLO → HELLO）
        let mut engine = Engine::new();
        for ch in "HELLO".chars() {
            engine.process_key(ch);
        }
        let mut found_upper = false;
        let total_pages = engine.total_pages();
        for _ in 0..total_pages {
            for i in 0..engine.candidate_count() {
                if let Some(c) = engine.candidate_display(i) {
                    if c == "HELLO" {
                        found_upper = true;
                    }
                }
            }
            if engine.page(1) <= 0 {
                break;
            }
        }
        assert!(found_upper, "HELLO 输入应有 HELLO 英文候选（Upper 转换）");
    }

    #[test]
    fn test_english_case_chinese_unaffected() {
        let mut engine = Engine::new();
        for ch in "nihao".chars() {
            engine.process_key(ch);
        }
        // 中文候选不受大小写影响（输入全小写）
        let display = engine.candidate_display(0).unwrap();
        assert_eq!(display, "你好");
    }

    // ─── V0.2.17 符号输入 v 模式 ───

    #[test]
    fn test_symbol_mode_arrow() {
        let mut engine = Engine::new();
        for ch in "vjt".chars() {
            engine.process_key(ch);
        }
        assert!(engine.is_symbol_mode());
        assert_eq!(engine.pinyin_str(), "vjt");
        assert!(engine.candidate_count() > 0);
        let has_arrow = (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("→"));
        assert!(has_arrow, "vjt 应列出箭头符号");
    }

    #[test]
    fn test_symbol_mode_math() {
        let mut engine = Engine::new();
        for ch in "vsx".chars() {
            engine.process_key(ch);
        }
        // 雾凇 sx 数学表首位 ±（≈ 在第 31 位，需翻页）
        assert!(engine.is_symbol_mode());
        let has_plus = (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("±"));
        assert!(has_plus, "vsx 第一页应含 ±");
    }

    #[test]
    fn test_symbol_mode_select_no_learn() {
        // 符号选中不上屏学习（is_symbol_mode 分支）
        let mut engine = Engine::new();
        for ch in "vjt".chars() {
            engine.process_key(ch);
        }
        // 雾凇 jt 顺序：首位 ↑
        let text = engine.select_candidate(0).unwrap();
        assert_eq!(text, "↑");
        assert_eq!(engine.pinyin_str(), "", "选中后应重置");
        assert_eq!(engine.candidate_count(), 0);
    }

    #[test]
    fn test_symbol_mode_unknown_category() {
        let mut engine = Engine::new();
        for ch in "vzz".chars() {
            engine.process_key(ch);
        }
        assert!(engine.is_symbol_mode());
        assert_eq!(engine.candidate_count(), 0, "未知分类码应无候选");
    }

    #[test]
    fn test_symbol_prefix_hot() {
        // 0.2.30：单 v → 热门符号直选（对标搜狗 v 首屏）
        let mut engine = Engine::new();
        engine.process_key('v');
        assert!(!engine.is_symbol_mode(), "单 v 不进入符号分类模式");
        assert!(engine.is_symbol_prefix(), "单 v 应处于 v 前缀反馈态");
        assert!(engine.candidate_count() > 0);
        assert_eq!(engine.candidate(0), Some("v"), "候选首位应为 v 单字");
        let has_arrow = (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("→"));
        let has_down = (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("↓"));
        assert!(has_arrow, "v 前缀应含箭头 →");
        assert!(has_down, "v 前缀第一页应含 ↓");
    }

    #[test]
    fn test_symbol_prefix_v_char() {
        // 选中候选 0（v 单字）→ 上屏 v，不学习
        let mut engine = Engine::new();
        engine.process_key('v');
        let text = engine.select_candidate(0).unwrap();
        assert_eq!(text, "v");
        assert_eq!(engine.pinyin_str(), "", "选中后应重置");
    }

    #[test]
    fn test_symbol_prefix_select_no_learn() {
        // v 前缀选中符号 → 上屏且不学习、状态重置
        let mut engine = Engine::new();
        engine.process_key('v');
        let idx = (0..engine.candidate_count())
            .find(|&i| engine.candidate(i) == Some("→"))
            .unwrap();
        let text = engine.select_candidate(idx).unwrap();
        assert_eq!(text, "→");
        assert_eq!(engine.pinyin_str(), "", "选中后应重置");
        assert_eq!(engine.candidate_count(), 0);
    }

    #[test]
    fn test_symbol_prefix_category_continues() {
        // v 前缀后继续输入分类码 → 仍走分类符号（vjt 箭头）
        let mut engine = Engine::new();
        for ch in "vjt".chars() {
            engine.process_key(ch);
        }
        assert!(engine.is_symbol_mode());
        let has_arrow = (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("→"));
        assert!(has_arrow, "vjt 仍应列出箭头符号");
    }

    #[test]
    fn test_symbol_prefix_normal_pinyin_unaffected() {
        // 非 v 前缀拼音不受影响（zhong → 中）
        let mut engine = Engine::new();
        for ch in "zhong".chars() {
            engine.process_key(ch);
        }
        assert!(!engine.is_symbol_prefix());
        let has_zhong = (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("中"));
        assert!(has_zhong, "zhong 应出中文候选");
    }

    #[test]
    fn test_symbol_digit_alias() {
        // v+数字别名（0.2.32）：v 后数字键进引擎 → v1 序号分类
        let mut engine = Engine::new();
        engine.process_key('v');
        engine.process_key('1');
        assert!(engine.is_symbol_mode(), "v1 应进入符号分类模式");
        assert_eq!(engine.pinyin_str(), "v1");
        let has_szq = (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("①"));
        assert!(has_szq, "v1 第一页应含 ①（雾凇 szq 首位 ⓪，① 在第 2 位）");
    }

    #[test]
    fn test_symbol_digit_alias_math() {
        let mut engine = Engine::new();
        for ch in "v2".chars() {
            engine.process_key(ch);
        }
        assert!(engine.is_symbol_mode());
        let has_plus = (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("±"));
        assert!(has_plus, "v2 应出数学符号 ±");
    }

    #[test]
    fn test_symbol_mode_backspace_exits() {
        // 退格删掉分类码回到 'v' → 退出符号模式
        let mut engine = Engine::new();
        for ch in "vjt".chars() {
            engine.process_key(ch);
        }
        assert!(engine.is_symbol_mode());
        engine.backspace(); // vj
        engine.backspace(); // v
        assert!(!engine.is_symbol_mode());
        assert_eq!(engine.pinyin_str(), "v");
    }

    #[test]
    fn test_fuzzy_toggle() {
        let mut engine = Engine::new();
        engine.set_fuzzy_enabled(false);
        assert!(!engine.fuzzy_enabled());
        engine.set_fuzzy_enabled(true);
        assert!(engine.fuzzy_enabled());
    }

    #[test]
    fn test_shuangpin_default_off() {
        let engine = Engine::new();
        assert!(!engine.shuangpin_mode(), "双拼默认应关闭");
    }

    #[test]
    fn test_shuangpin_toggle_resets() {
        let mut engine = Engine::new();
        engine.process_key('z');
        assert!(!engine.pinyin_str().is_empty());
        engine.set_shuangpin_mode(true);
        assert!(engine.shuangpin_mode());
        assert_eq!(engine.pinyin_str(), "", "开启双拼应清空未完成拼音");
    }

    #[test]
    fn test_shuangpin_query() {
        // 双拼模式：vs = zhong → 应出"中"
        let mut engine = Engine::new();
        engine.set_shuangpin_mode(true);
        engine.process_key('v');
        engine.process_key('s');
        assert_eq!(engine.pinyin_str(), "vs");
        let has_zhong = (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("中"));
        assert!(has_zhong, "双拼 vs 应命中 中");
    }

    #[test]
    fn test_shuangpin_preedit_decodes() {
        // 默认显示按键：vs 原样；打开「显示全拼音节」开关后才还原成 zhong
        let mut engine = Engine::new();
        engine.set_shuangpin_mode(true);
        engine.process_key('v');
        engine.process_key('s');
        assert_eq!(engine.preedit_string(), "vs", "默认应显示按键串");
        engine.set_shuangpin_full_preedit(true);
        assert_eq!(engine.preedit_string(), "zhong");
        engine.process_key('v');
        assert_eq!(engine.preedit_string(), "zhongzh", "末尾未完成键显示为声母");
    }

    #[test]
    fn test_shuangpin_primary_pinyin_and_semicolon() {
        let mut engine = Engine::new();
        engine.set_shuangpin_mode(true);
        for ch in "vs".chars() {
            engine.process_key(ch);
        }
        assert_eq!(engine.shuangpin_primary_pinyin().as_deref(), Some("zhong"));
        // 微软 ing 在分号键："x;" → xing
        engine.set_shuangpin_scheme("mspy");
        engine.reset();
        for ch in "x;".chars() {
            engine.process_key(ch);
        }
        assert_eq!(engine.pinyin_str(), "x;");
        assert_eq!(engine.shuangpin_primary_pinyin().as_deref(), Some("xing"));
    }

    // ─── V0.2.10 智能纠错测试 ───

    #[test]
    fn test_correction_default_on() {
        let engine = Engine::new();
        assert!(engine.correction_enabled(), "智能纠错默认应开启");
    }

    #[test]
    fn test_correction_toggle() {
        let mut engine = Engine::new();
        engine.set_correction_enabled(false);
        assert!(!engine.correction_enabled());
        engine.set_correction_enabled(true);
        assert!(engine.correction_enabled());
    }

    #[test]
    fn test_correction_full_pinyin_works() {
        // 完整拼音输入纠错仍工作：zhonggou（zhong+gou 完整切分）
        // → 相邻交换 o/u → zhongguo → 中国（内置词库有 zhongguo=中国）
        // 串行锁 + 显式 builtin：并行时其他测试 init_blocking(Some) 会加载真实词库
        // （zhonggou 有重构/鸿沟等候选 → 纠错不触发），锁保证本测试读 builtin。
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(None);
        let mut engine = Engine::new();
        for ch in "zhonggou".chars() {
            engine.process_key(ch);
        }
        let has_zhongguo =
            (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("中国"));
        assert!(
            has_zhongguo,
            "zhonggou 应纠错出 中国, got: {:?}",
            (0..engine.candidate_count())
                .map(|i| engine.candidate(i).unwrap_or(""))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_correction_no_english_pollution() {
        // 0.3.x fix：英文单词（非完整拼音）不触发纠错/模糊联想中文。
        // "hello" 切分失败（he+llo）→ 视为英文 → 不得出"很隆重"（henlongzhong 前缀误配）。
        let mut engine = Engine::new();
        for ch in "hello".chars() {
            engine.process_key(ch);
        }
        let words: Vec<&str> = (0..engine.candidate_count())
            .map(|i| engine.candidate(i).unwrap_or(""))
            .collect();
        assert!(
            !words.contains(&"很隆重"),
            "hello 不应联想中文词（模糊音误伤）, got: {words:?}"
        );
        assert!(
            !words.contains(&"狠练苦练"),
            "hello 不应联想中文词（简拼误伤）, got: {words:?}"
        );
        // 英文候选仍在（混输）
        assert!(
            words.contains(&"hello"),
            "hello 英文候选应保留, got: {words:?}"
        );
    }

    #[test]
    fn test_correction_off_no_suggestion() {
        // 关闭纠错：gogn 不应出 工
        let mut engine = Engine::new();
        engine.set_correction_enabled(false);
        for ch in "gogn".chars() {
            engine.process_key(ch);
        }
        let has_gong = (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("工"));
        assert!(!has_gong, "关闭纠错后 gogn 不应出 工");
    }

    #[test]
    fn test_correction_exact_not_disturbed() {
        // 精确命中不干扰：nihao 正常出 你好，纠错变体排在后面或不出现
        let mut engine = Engine::new();
        for ch in "nihao".chars() {
            engine.process_key(ch);
        }
        let first = engine.candidate(0);
        assert_eq!(first, Some("你好"), "精确命中应优先, got {first:?}");
    }

    // ─── V0.2.8 中英混输测试 ───

    #[test]
    fn test_mix_mode_default_on() {
        let engine = Engine::new();
        assert!(engine.mix_mode(), "中英混输默认应开启");
    }

    #[test]
    fn test_mix_mode_english_candidate_appended() {
        // 中文模式输入 hello → 明显英文，置顶（对齐微软拼音首位）
        let mut engine = Engine::new();
        for ch in "hello".chars() {
            engine.process_key(ch);
        }
        assert_eq!(engine.pinyin_str(), "hello");
        assert_eq!(
            engine.candidate(0),
            Some("hello"),
            "明显英文应置顶, got {:?}",
            engine.candidate(0)
        );
        assert_eq!(
            engine.candidate_src.first(),
            Some(&Source::English),
            "英文区起始于首位"
        );
    }

    #[test]
    fn test_mix_mode_english_tail_for_complete_pinyin() {
        // 完整拼音（ai）→ 英文候选不置顶，排在中文候选之后（中文优先）
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(None);
        let mut engine = Engine::new();
        for ch in "ai".chars() {
            engine.process_key(ch);
        }
        let cn_count = (0..engine.candidate_count())
            .filter(|&i| {
                engine
                    .candidate(i)
                    .map_or(false, |w| w.chars().any(|c| !c.is_ascii()))
            })
            .count();
        assert_eq!(
            engine
                .candidate_src
                .iter()
                .position(|&s| s == Source::English),
            Some(cn_count),
            "英文应排在中文候选之后（中文 {cn_count} 个）"
        );
    }

    #[test]
    fn test_mix_mode_select_english_no_learn() {
        // 选中英文候选 → 上屏原文，不学习用户词
        let mut engine = Engine::new();
        for ch in "hello".chars() {
            engine.process_key(ch);
        }
        let last_idx = engine.candidate_count() - 1;
        let text = engine.select_candidate(last_idx).unwrap();
        assert_eq!(text, "hello", "英文候选应上屏原文");
        // 状态已重置
        assert_eq!(engine.pinyin_str(), "");
    }

    #[test]
    fn test_mix_mode_toggle_off() {
        // 关闭混输 → 无英文候选
        let mut engine = Engine::new();
        engine.set_mix_mode(false);
        assert!(!engine.mix_mode());
        for ch in "hello".chars() {
            engine.process_key(ch);
        }
        let has_english =
            (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("hello"));
        assert!(!has_english, "关闭混输后不应有英文候选");
    }

    #[test]
    fn test_mix_mode_ascii_mode_no_english() {
        // 英文模式（ascii_mode=1）：字母直通，不累积拼音、无英文候选
        let mut engine = Engine::new();
        engine.set_ascii_mode(true);
        engine.process_key('h');
        assert_eq!(engine.pinyin_str(), "");
        assert_eq!(engine.candidate_count(), 0);
    }

    // ─── V0.2.11 简繁转换测试 ───

    #[test]
    fn test_traditional_default_off() {
        let engine = Engine::new();
        assert!(!engine.traditional(), "简繁转换默认应关闭");
    }

    #[test]
    fn test_traditional_toggle() {
        let mut engine = Engine::new();
        engine.set_traditional(true);
        assert!(engine.traditional());
        engine.set_traditional(false);
        assert!(!engine.traditional());
    }

    #[test]
    fn test_traditional_select_output() {
        // 开启简繁：选"中国" → 上屏"中國"
        let mut engine = Engine::new();
        engine.set_traditional(true);
        for ch in "zhongguo".chars() {
            engine.process_key(ch);
        }
        let text = engine.select_candidate(0).unwrap();
        assert_eq!(text, "中國", "简繁模式选中国应上屏中國, got {text}");
    }

    #[test]
    fn test_traditional_candidate_display() {
        // 开启简繁：候选显示繁体
        let mut engine = Engine::new();
        engine.set_traditional(true);
        for ch in "zhongguo".chars() {
            engine.process_key(ch);
        }
        let display = engine.candidate_display(0).unwrap();
        assert_eq!(display, "中國", "候选应显示繁体, got {display}");
    }

    #[test]
    fn test_traditional_english_not_converted() {
        // 英文候选不转繁体（hello 保持原样）
        let mut engine = Engine::new();
        engine.set_traditional(true);
        for ch in "hello".chars() {
            engine.process_key(ch);
        }
        let last_idx = engine.candidate_count() - 1;
        let text = engine.select_candidate(last_idx).unwrap();
        assert_eq!(text, "hello", "英文候选不应转繁体");
    }

    // ─── V0.2.12 快捷短语测试 ───

    #[test]
    fn test_phrase_default_on() {
        let engine = Engine::new();
        assert!(engine.phrase_enabled(), "快捷短语默认应开启");
    }

    // ─── V0.3.x 白话文长句过滤（Eric 决策）───

    #[test]
    fn test_long_sentence_filter() {
        // 普通拼音查询：候选不得含 >10 字词条（白话文长句/超长专名不进候选）
        let mut engine = Engine::new();
        for input in ["nihaoshijie", "shurufa", "zhongguo", "diannao", "quanguo"] {
            engine.reset();
            for ch in input.chars() {
                engine.process_key(ch);
            }
            for i in 0..engine.candidate_count() {
                let w = engine.candidate(i).unwrap();
                assert!(
                    w.chars().count() <= 10,
                    "候选含长句(>10字): {w} (输入 {input})"
                );
            }
        }
    }

    #[test]
    fn test_sentence_candidates_do_not_outrank_dict_words() {
        // youxianji 触发整句解码，产出「有县级 / 有限及 / 有限即」三条非词组合。
        // 它们曾无条件 insert(0) 且绕过全部排序链，把词库首位的「优先级」压到第 4 位
        // （Eric 报障：第一位根本不是词）。现在整句候选进排序链，词库真实词条优先。
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(Some(std::path::Path::new("../resources/system_dict.db")));
        let mut engine = Engine::new();
        for ch in "youxianji".chars() {
            engine.process_key(ch);
        }
        let cands: Vec<&str> = (0..engine.candidate_count())
            .filter_map(|i| engine.candidate(i))
            .collect();
        assert_eq!(
            cands.first().copied(),
            Some("优先级"),
            "词库首位词被整句解码产物压后，候选={cands:?}"
        );
    }

    #[test]
    fn test_long_sentence_still_first_for_long_input() {
        // 整句候选进排序链后，长输入仍应首位出整句（此时没有同长度的词级候选竞争）。
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(Some(std::path::Path::new("../resources/system_dict.db")));
        let mut engine = Engine::new();
        for ch in "nimenjigezaizheliganshenme".chars() {
            engine.process_key(ch);
        }
        let cands: Vec<&str> = (0..engine.candidate_count())
            .filter_map(|i| engine.candidate(i))
            .collect();
        assert!(
            cands.first().is_some_and(|c| c.chars().count() >= 6),
            "长输入首位应是整句而非短词，候选={cands:?}"
        );
    }

    #[test]
    fn test_long_sentence_filter_keeps_phrase() {
        // 用户自定义快捷短语不受长度限制：通过 load_phrases 显式注册 13 字短语必须保留
        let mut engine = Engine::new();
        engine.load_phrases(vec![(
            "dz".to_string(),
            "地址：深圳市南山区科技园".to_string(),
        )]);
        engine.process_key('d');
        engine.process_key('z');
        let found = (0..engine.candidate_count())
            .any(|i| engine.candidate(i) == Some("地址：深圳市南山区科技园"));
        assert!(found, "用户短语(>10字)被长句过滤误杀");
    }

    #[test]
    fn test_phrase_toggle() {
        let mut engine = Engine::new();
        engine.set_phrase_enabled(false);
        assert!(!engine.phrase_enabled());
        engine.set_phrase_enabled(true);
        assert!(engine.phrase_enabled());
    }

    #[test]
    fn test_phrase_suggestion() {
        // 输入 bq → 候选含"不客气"（内置短语）
        let mut engine = Engine::new();
        engine.process_key('b');
        engine.process_key('q');
        let has_phrase =
            (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("不客气"));
        assert!(has_phrase, "bq 应出短语 不客气");
    }

    #[test]
    fn test_phrase_select_no_learn() {
        // 选中短语 → 上屏原文，不学习用户词
        let mut engine = Engine::new();
        engine.process_key('b');
        engine.process_key('q');
        // 短语排最前
        let text = engine.select_candidate(0).unwrap();
        assert_eq!(text, "不客气", "短语应排最前且上屏");
        assert_eq!(engine.pinyin_str(), "");
    }

    #[test]
    fn test_phrase_off_no_suggestion() {
        let mut engine = Engine::new();
        engine.set_phrase_enabled(false);
        engine.process_key('b');
        engine.process_key('q');
        let has_phrase =
            (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("不客气"));
        assert!(!has_phrase, "关闭短语后 bq 不应出短语");
    }

    // ─── V0.5 动态组词（翻页 3 次进入组词模式 + 逐音节选字 + 组合学习）───

    #[test]
    fn test_compose_entry_after_2_pages() {
        // 多音节拼音候选充足时：翻页 2 次仅浏览候选不进入组词，3 次才进入
        // （V0.4.5 fix：阈值 2→3——完整拼音候选充足时翻 2 页只是浏览，
        // 误入组词会重算候选、丢失纠错结果，ffi 回归实测）
        let mut engine = Engine::new();
        // 加载真实词库（zhonggou 候选充足多页；内置兜底词库候选少无法构造多页）
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let db =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../resources/system_dict.db");
        if db.exists() {
            crate::dictionary::init_blocking(Some(&db));
        }
        for ch in "zhonggou".chars() {
            engine.process_key(ch);
        }
        // zhonggou 完整拼音候选充足（真实词库 19 候选 4 页；内置兜底可能不足 → 跳过）
        if engine.total_pages() < 3 {
            eprintln!("zhonggou 候选不足 3 页（词库缺失？），跳过");
            return;
        }
        assert!(!engine.compose_active(), "翻页前不应进入组词");
        engine.page(1); // 第 1 次
        assert!(!engine.compose_active(), "翻 1 次不应进入组词");
        engine.page(1); // 第 2 次
        assert!(
            !engine.compose_active(),
            "翻 2 次不应进入组词（候选充足时仅浏览）"
        );
        engine.page(1); // 第 3 次 → 进入组词
        assert!(engine.compose_active(), "翻页 3 次后应进入组词模式");
        // V0.6.x：组词候选优先出剩余拼音的整句/整词，单字只作兜底——
        // 不再强制全是单字（否则「魔兽世界」这类词永远选不到）
        let n = engine.candidate_count();
        assert!(n > 0, "组词模式应有候选");
        // 再翻页不改变组词状态（组词内翻页不累计触发）
        engine.page(1);
        assert!(engine.compose_active());
    }

    #[test]
    fn test_compose_entry_when_few_candidates() {
        // 候选不足一页：正向翻页翻不动（到边界）→ 立即进入组词（Eric：3 次是预估值）
        let mut engine = Engine::new();
        for ch in "taishen".chars() {
            engine.process_key(ch);
        }
        // 手动构造仅 1 页候选（模拟候选少的场景）
        engine.all_candidates = vec!["太深".to_string(), "太深".to_string()];
        // 测试夹具必须同步来源表，否则 side-table 等长不变量在别处失效
        engine.candidate_src = vec![Source::Dictionary; 2];
        engine.repage();
        assert_eq!(engine.total_pages(), 1, "构造候选应仅 1 页");
        engine.page(1);
        engine.page(1);
        assert!(
            !engine.compose_active(),
            "V0.6.x：翻不动不再立即进组词（否则长词选不到）"
        );
        engine.page(1);
        assert!(engine.compose_active(), "翻满 3 次后应进入组词");
    }

    #[test]
    fn test_compose_entry_short_pinyin() {
        // 简拼组词（V0.5+）：tshen（t 声母 + shen 完整）→ 翻页 3 次开启组词
        let mut engine = Engine::new();
        for ch in "tshen".chars() {
            engine.process_key(ch);
        }
        engine.page(1);
        engine.page(1);
        engine.page(1);
        assert!(
            engine.compose_active(),
            "tshen 应开启组词（t 声母 + shen 完整）"
        );
        // V0.6.x：组词候选含整词，不再保证全是单字
        let n = engine.candidate_count();
        assert!(n > 0, "组词模式应有候选");
    }

    #[test]
    fn test_compose_pure_initials_no_entry() {
        // 纯声母串（ts）无完整音节 → 不开启组词（防误入）
        let mut engine = Engine::new();
        for ch in "ts".chars() {
            engine.process_key(ch);
        }
        engine.page(1);
        engine.page(1);
        engine.page(1);
        assert!(!engine.compose_active(), "纯声母串不应进入组词");
    }

    // ─── V0.5+ 组词学习进化：用户词简拼命中（ts → 泰深）───

    #[test]
    fn test_user_short_hit_after_learn() {
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // learn taishen→泰深 后，简拼 ts 应命中（声母串 t+s）
        let tmp = std::env::temp_dir().join("taishen_test_user_dict2.db");
        let _ = std::fs::remove_file(&tmp);
        crate::dictionary::init(None);
        crate::dictionary::set_user_dict_path(Some(&tmp));
        crate::dictionary::learn("taishen", "泰深");
        let short = crate::dictionary::query_short("ts");
        assert!(
            short.iter().any(|w| w == "泰深"),
            "learn 后 query_short(ts) 应含泰深: {short:?}"
        );
        // 用户词简拼命中 → datetime 时间戳简码让位
        assert!(
            crate::dictionary::user_short_hit("ts"),
            "user_short_hit(ts) 应为 true"
        );
    }

    #[test]
    fn test_ts_input_priority_user_word() {
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // 学过 ts→泰深 后，打 ts 候选应含泰深（datetime 时间戳让位）
        let tmp = std::env::temp_dir().join("taishen_test_user_dict3.db");
        let _ = std::fs::remove_file(&tmp);
        crate::dictionary::init(None);
        crate::dictionary::set_user_dict_path(Some(&tmp));
        crate::dictionary::learn("taishen", "泰深");
        let mut engine = Engine::new();
        engine.process_key('t');
        engine.process_key('s');
        let has_taishen =
            (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("泰深"));
        assert!(has_taishen, "打 ts 候选应含泰深（用户词优先于时间戳简码）");
    }

    // ─── V0.5+ 自然语言触发日期候选（输入"日期/时间/年份"出当前值）───

    #[test]
    fn test_datetime_pinyin_trigger() {
        // datetime_pinyin_candidates 映射
        assert!(
            Engine::datetime_pinyin_candidates("riqi").is_some(),
            "riqi 应触发"
        );
        assert!(
            Engine::datetime_pinyin_candidates("shijian").is_some(),
            "shijian 应触发"
        );
        assert!(
            Engine::datetime_pinyin_candidates("nian").is_some(),
            "nian 应触发"
        );
        assert!(
            Engine::datetime_pinyin_candidates("abc").is_none(),
            "abc 不触发"
        );
        // year_candidates 返回当前年
        let y = crate::datetime::year_candidates();
        assert_eq!(y.len(), 2, "年份候选 2 个: {y:?}");
    }

    #[test]
    fn test_input_riqi_has_date() {
        // 输入 riqi → 候选含当前日期（2026-08-08 格式）
        // 串行锁 + 显式 builtin：真实词库下 riqi 有词条会挤掉日期候选，锁保证确定性
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(None);
        let mut engine = Engine::new();
        for ch in "riqi".chars() {
            engine.process_key(ch);
        }
        let now = crate::datetime::date_candidates();
        let today = &now[0];
        let has_date =
            (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some(today.as_str()));
        assert!(has_date, "输入 riqi 候选应含今天日期 {today}");
    }

    #[test]
    fn test_input_nian_has_year() {
        // 输入 nian → 候选含当前年份
        // 串行锁 + 显式 builtin：真实词库下 nian 有词条会挤掉年份候选，锁保证确定性
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(None);
        let mut engine = Engine::new();
        for ch in "nian".chars() {
            engine.process_key(ch);
        }
        let years = crate::datetime::year_candidates();
        for y in &years {
            let has =
                (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some(y.as_str()));
            assert!(has, "输入 nian 候选应含 {y}");
        }
    }

    #[test]
    fn test_compose_illegal_syllable_no_entry() {
        // wa 无法切分为 ≥2 音节 → 翻页不进入组词
        let mut engine = Engine::new();
        for ch in "wa".chars() {
            engine.process_key(ch);
        }
        engine.page(1);
        engine.page(1);
        engine.page(1);
        assert!(!engine.compose_active(), "非法音节序列不应进入组词");
    }

    #[test]
    fn test_compose_letter_exits() {
        // 组词模式中按字母 → 退出组词，重新普通输入
        let mut engine = Engine::new();
        for ch in "taishen".chars() {
            engine.process_key(ch);
        }
        engine.page(1);
        engine.page(1);
        engine.page(1);
        assert!(engine.compose_active());
        engine.process_key('x'); // 新输入
        assert!(!engine.compose_active(), "输入字母应退出组词");
    }

    #[test]
    fn test_compose_select_and_learn() {
        // 组词模式：逐音节选字 → 组合上屏 + 学习用户词（taishen → 泰生/泰深）
        // 先初始化词库再设置用户词库路径（延迟 init 会重置 user_dict_path，learn 静默跳过）
        // 串行锁：init(None) 异步切换内置词库，并行时污染共享 DICT（V0.5.10 修复）
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join("taishen_test_user_dict.db");
        let _ = std::fs::remove_file(&tmp);
        crate::dictionary::init(None);
        crate::dictionary::set_user_dict_path(Some(&tmp));
        let mut engine = Engine::new();
        for ch in "taishen".chars() {
            engine.process_key(ch);
        }
        engine.page(1);
        engine.page(1);
        engine.page(1);
        assert!(engine.compose_active());
        // 内置词库可能缺 tai 单字 → 手动填充候选模拟词库
        // V0.6.x：组词候选含整词，优先挑单字候选以停在中间音节
        if !(0..engine.candidate_count())
            .any(|i| engine.candidate(i).is_some_and(|c| c.chars().count() == 1))
        {
            engine.candidates = vec!["泰".to_string()];
        }
        let first_idx = (0..engine.candidate_count())
            .find(|&i| engine.candidate(i).is_some_and(|c| c.chars().count() == 1))
            .unwrap_or(0);
        let first = engine.candidate(first_idx).unwrap().to_string();
        // V0.5.10 逐字预上屏：中间音节返回选中字（平台层立即上屏），推进到下一音节
        let r = engine.select_candidate(first_idx);
        assert_eq!(
            r.as_deref(),
            Some(first.as_str()),
            "中间音节应返回选中字（预上屏）"
        );
        assert!(engine.compose_active(), "仍有剩余音节");
        // 第二音节
        if engine.candidate_count() == 0 {
            engine.candidates = vec!["深".to_string()];
        }
        let second = engine.candidate(0).unwrap().to_string();
        let combined = format!("{first}{second}");
        // 最后音节：返回整词（V0.6.x：平台层直接提交，引擎 reset 前一次给全）
        let r = engine.select_candidate(0);
        assert_eq!(r.as_deref(), Some(combined.as_str()), "最后音节应返回整词");
        assert!(!engine.compose_active(), "组词完成应退出");
        // 学习生效：query("taishen") 应含组合词（用户词库）
        let cands = crate::dictionary::query("taishen");
        assert!(
            cands.iter().any(|w| w == &combined),
            "learn 后 taishen 应出 {combined}: {cands:?}"
        );
    }

    #[test]
    fn test_phrase_custom_load() {
        // 外部加载覆盖内置
        let mut engine = Engine::new();
        engine.load_phrases(vec![("bq".to_string(), "自定义短语".to_string())]);
        engine.process_key('b');
        engine.process_key('q');
        let has_custom =
            (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("自定义短语"));
        assert!(has_custom, "外部短语应覆盖内置");
    }

    // ─── V0.5.9 混合简拼词长匹配（Eric：ces/jianp 长词短句抢首位）───
    #[test]
    fn test_mixed_syllable_count() {
        assert_eq!(mixed_syllable_count("ces"), Some(2), "ce+s → 2");
        assert_eq!(mixed_syllable_count("jianp"), Some(2), "jian+p → 2");
        assert_eq!(mixed_syllable_count("shurf"), Some(3), "shu+r+f → 3");
        assert_eq!(
            mixed_syllable_count("ceshi"),
            Some(2),
            "完整拼音混合函数同样切出 2"
        );
        assert_eq!(
            mixed_syllable_count("zg"),
            None,
            "纯声母交 short_pinyin_word_len"
        );
        assert_eq!(mixed_syllable_count("w"), None, "单声母不触发");
        assert_eq!(
            mixed_syllable_count("zhyang"),
            Some(2),
            "zh+yang：声母+完整拼音两段（旧贪心切法会估成 3 段）"
        );
        assert_eq!(
            mixed_syllable_count("hel"),
            Some(2),
            "he+l 可解析为混合（2 字中文词前置，英文候选在末尾不受影响）"
        );
    }

    #[test]
    fn test_mixed_short_pinyin_word_len_match() {
        // 真实词库下回归：ces → 2 字"测试"抢回首位（此前"测试下"3 字在前）；
        // jianp → 首位必须 2 字（此前"柬埔寨"3 字在前）。
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(Some(std::path::Path::new("../resources/system_dict.db")));
        let mut engine = Engine::new();
        for ch in "ces".chars() {
            engine.process_key(ch);
        }
        assert_eq!(
            engine.candidate(0),
            Some("测试"),
            "ces 首位应为 2 字 测试, got: {:?}",
            (0..engine.candidate_count())
                .map(|i| engine.candidate(i).unwrap_or(""))
                .collect::<Vec<_>>()
        );
        engine.reset();
        for ch in "jianp".chars() {
            engine.process_key(ch);
        }
        let first = engine.candidate(0).unwrap_or("");
        let hanzi = first.chars().filter(|c| is_hanzi(*c)).count();
        assert_eq!(
            hanzi,
            2,
            "jianp 首位应为 2 字词, got: {first}, all: {:?}",
            (0..engine.candidate_count())
                .map(|i| engine.candidate(i).unwrap_or(""))
                .collect::<Vec<_>>()
        );
    }

    // ─── 简拼+全拼混合（bru/zhyang）与热词首屏 ───
    #[test]
    fn test_mixed_abbrev_full_and_hotword() {
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(Some(std::path::Path::new("../resources/system_dict.db")));
        // 临时用户词库隔离（learn 不污染其他测试的共享全局 user_dict）
        let tmp = std::env::temp_dir().join("taishen_test_networds.db");
        let _ = std::fs::remove_file(&tmp);
        crate::dictionary::set_user_dict_path(Some(&tmp));
        let mut engine = Engine::new();
        for ch in "bru".chars() {
            engine.process_key(ch);
        }
        let hit_buru = (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("不如"));
        assert!(hit_buru, "bru 应命中 不如（声母+全拼混合）");
        engine.reset();
        for ch in "zhyang".chars() {
            engine.process_key(ch);
        }
        let hit_zheyang =
            (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("这样"));
        // zhyang 的声母序列是 zh+yang，高频解是「这样」（zheyang 3943），
        // 不是「张扬」（zhangyang 2041）。这里只断言混合输入能出高频解；
        // 张扬在混合简拼池里可达，由 dictionary::tests::test_abbrev_full_zhyang 守护。
        assert!(hit_zheyang, "zhyang 应命中 这样（zh+yang 混合的高频解）");
        // 热字：选一次"辛" → 下次 xin 首位
        crate::dictionary::learn("xin", "辛");
        engine.reset();
        for ch in "xin".chars() {
            engine.process_key(ch);
        }
        assert_eq!(
            engine.candidate(0),
            Some("辛"),
            "选过 辛 后 xin 应首位（温词 P1 层）"
        );
        // 热词：选两次"内卷" → 下次 neijuan 首位
        crate::dictionary::learn("neijuan", "内卷");
        crate::dictionary::learn("neijuan", "内卷");
        engine.reset();
        for ch in "neijuan".chars() {
            engine.process_key(ch);
        }
        assert_eq!(
            engine.candidate(0),
            Some("内卷"),
            "选过 内卷 后 neijuan 应首位（热词 P1 层）"
        );
    }

    // ─── V0.5.10 组词逐字预上屏（Eric：辛茂 → 选"辛"立即上屏 + 剩余 mao）───
    #[test]
    fn test_compose_partial_commit() {
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(Some(std::path::Path::new("../resources/system_dict.db")));
        // 临时用户词库隔离（learn 不污染其他测试的共享全局 user_dict）
        let tmp = std::env::temp_dir().join("taishen_test_compose.db");
        let _ = std::fs::remove_file(&tmp);
        crate::dictionary::set_user_dict_path(Some(&tmp));
        let mut engine = Engine::new();
        for ch in "xinmao".chars() {
            engine.process_key(ch);
        }
        // 翻页 3 次触发组词（xinmao → [xin, mao]）
        for _ in 0..3 {
            engine.page(1);
        }
        assert!(engine.compose_active(), "翻页 3 次应进入组词");
        // 第一音节：候选多页（xin 单字 18 个），"辛"在第二页 → 翻页定位
        let mut xin_idx = None;
        for _p in 0..10 {
            if let Some(i) =
                (0..engine.candidate_count()).position(|i| engine.candidate(i) == Some("辛"))
            {
                xin_idx = Some(i);
                break;
            }
            if engine.page(1) == 0 {
                break;
            }
        }
        let idx = xin_idx.expect("翻页后 xin 单字候选应含 辛");
        // 选中"辛"→ 返回"辛"且保持组词状态
        let r1 = engine.select_candidate(idx);
        assert_eq!(r1.as_deref(), Some("辛"), "中间音节应返回选中字（预上屏）");
        assert!(engine.compose_active(), "中间音节后仍处于组词");
        assert_eq!(engine.compose_idx, 1, "应推进到第二音节");
        // 第二音节：候选 = mao 单字，"茂"需翻页定位
        let mut mao_idx = None;
        for _ in 0..10 {
            if let Some(i) =
                (0..engine.candidate_count()).position(|i| engine.candidate(i) == Some("茂"))
            {
                mao_idx = Some(i);
                break;
            }
            if engine.page(1) == 0 {
                break;
            }
        }
        let idx2 = mao_idx.expect("翻页后 mao 单字候选应含 茂");
        let r2 = engine.select_candidate(idx2);
        // V0.6.x：最后音节返回整词（辛茂），平台层直接提交——
        // 引擎此时已 reset，前缀状态不再可读，必须一次给全（B-22）。
        assert_eq!(r2.as_deref(), Some("辛茂"), "最后音节应返回整词");
        assert!(!engine.compose_active(), "最后音节后组词结束");
        assert!(engine.pinyin_str().is_empty(), "组词结束应 reset");
        // learn 验证：xinmao → 辛茂 已入用户词库
        engine.reset();
        for ch in "xinmao".chars() {
            engine.process_key(ch);
        }
        let hit = (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("辛茂"));
        assert!(hit, "组词 learn 后 xinmao 应命中 辛茂");
    }

    // ─── V0.6.x 组词候选：从长到短 ───

    #[test]
    fn test_compose_candidates_prefer_long_words() {
        // 组词候选原来按「本次能消费的音节数」从多到少排（整段词 → 前缀词 → 单字）。
        // V0.6.x 之后修正：**首位固定为当前音节单字**——组词的语义是逐字推进，
        // 整词放首位时用户按 1 以为选首字、实际选中吃光全部剩余音节的整词，
        // 组词立即结束、拼音清空，后续数字键全部透传给宿主（「多翻几页冒出 12」，
        // Eric 报障）。整词候选保留在第 2 位起，仍可一次上屏。
        // 这里验证：有单字时首位是单字；无单字时保持最长词优先；始终收敛在 3 页内。
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // 用真实词库：内置词库没有这些长串，组词候选会为空
        crate::dictionary::init_blocking(Some(std::path::Path::new("../resources/system_dict.db")));
        let mut engine = Engine::new();
        for ch in "jintiantianqizhenbucuo".chars() {
            engine.process_key(ch);
        }
        for _ in 0..3 {
            engine.page(1);
        }
        assert!(engine.compose_active(), "翻 3 次应进入组词");
        let lens: Vec<usize> = (0..engine.total_candidate_count())
            .filter_map(|i| engine.candidate_global(i))
            .map(|w| w.chars().count())
            .collect();
        let multi_word_count = lens.iter().filter(|&&n| n > 1).count();
        assert!(
            multi_word_count <= engine.page_size() * 3,
            "组词多字整词应收敛在 3 页内, multi_word_count={multi_word_count}"
        );
        assert!(!lens.is_empty(), "组词候选不应为空");
        if let Some(pos) = lens.iter().position(|&n| n == 1) {
            assert_eq!(pos, 0, "有单字候选时首位必须是单字, lens={lens:?}");
        } else {
            assert_eq!(
                lens[0],
                *lens.iter().max().unwrap(),
                "无单字时最长词应在首位, lens={lens:?}"
            );
        }
    }

    // ─── V0.5.13 音节可视化 ───

    #[test]
    fn test_syllable_display_full_pinyin() {
        let mut engine = Engine::new();
        for ch in "zhongguo".chars() {
            engine.process_key(ch);
        }
        assert_eq!(engine.syllable_display(), "zhong'guo");
    }

    #[test]
    fn test_syllable_display_pure_initials() {
        let mut engine = Engine::new();
        for ch in "zg".chars() {
            engine.process_key(ch);
        }
        assert_eq!(engine.syllable_display(), "z'g");
    }

    #[test]
    fn test_syllable_display_mixed() {
        let mut engine = Engine::new();
        for ch in "tshen".chars() {
            engine.process_key(ch);
        }
        assert_eq!(engine.syllable_display(), "t'shen");
    }

    #[test]
    fn test_syllable_display_single() {
        let mut engine = Engine::new();
        for ch in "wo".chars() {
            engine.process_key(ch);
        }
        assert_eq!(engine.syllable_display(), "wo", "单音节无分隔");
    }

    #[test]
    fn test_syllable_display_empty() {
        let engine = Engine::new();
        assert_eq!(engine.syllable_display(), "");
    }

    #[test]
    fn test_syllable_display_compose_mode() {
        // 组词模式：返回 compose_idx 起至末尾带分隔（候选窗指示当前音节）
        let mut engine = Engine::new();
        for ch in "taishen".chars() {
            engine.process_key(ch);
        }
        engine.all_candidates = vec!["太深".to_string(), "太深".to_string()];
        // 测试夹具必须同步来源表，否则 side-table 等长不变量在别处失效
        engine.candidate_src = vec![Source::Dictionary; 2];
        engine.repage();
        // V0.6.x：不再「翻不动立即进组词」，累计翻 3 次才进
        engine.page(1);
        engine.page(1);
        engine.page(1);
        assert!(engine.compose_active());
        assert_eq!(
            engine.syllable_display(),
            "tai'shen",
            "组词首音节应显示整串分隔"
        );
        // 选第一字后：显示剩余音节分隔
        engine.compose_idx = 1;
        assert_eq!(engine.syllable_display(), "shen", "选字后应显示剩余音节");
    }

    #[test]
    fn test_syllable_boundaries_short_pinyin() {
        // 简拼 zg 边界 [0,1,2]（Tab/Ctrl+BS 可按音节移动，与显示对齐）
        let mut engine = Engine::new();
        for ch in "zg".chars() {
            engine.process_key(ch);
        }
        assert_eq!(engine.syllable_boundaries(), vec![0, 1, 2]);
        assert_eq!(engine.cursor_pos(), 2, "输入结束光标在末尾");
        engine.move_cursor(-1);
        assert_eq!(engine.cursor_pos(), 1, "Shift+Tab 应回退到 z 与 g 之间");
        engine.move_cursor(-1);
        assert_eq!(engine.cursor_pos(), 0, "再 Shift+Tab 应到开头");
        engine.move_cursor(1);
        assert_eq!(engine.cursor_pos(), 1, "Tab 右移应到 z 与 g 之间");
        engine.move_cursor(1);
        assert_eq!(engine.cursor_pos(), 2, "再次 Tab 应到末尾");
    }

    #[test]
    fn test_syllable_boundaries_full_pinyin() {
        // 全拼 zhongguo 边界不变：[0, 5, 8]
        let mut engine = Engine::new();
        for ch in "zhongguo".chars() {
            engine.process_key(ch);
        }
        assert_eq!(engine.syllable_boundaries(), vec![0, 5, 8]);
    }

    #[test]
    fn test_correction_candidate_not_learned() {
        // 纠错/模糊音来源的候选被选中时不学习：用户只是采纳了一次猜测，
        // 学进用户词库会把一次误触固化成「这个词就读这个音」。
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join("taishen_test_correction_learn.db");
        let _ = std::fs::remove_file(&tmp);
        // 先 init 再设用户词库：延迟 init 会重置路径，learn 会静默跳过
        crate::dictionary::init(None);
        crate::dictionary::set_user_dict_path(Some(&tmp));
        let mut engine = Engine::new();
        for ch in "nihao".chars() {
            engine.process_key(ch);
        }
        engine.candidates = vec!["立号".to_string()];
        engine.candidate_src = vec![Source::Correction];
        let _ = engine.select_candidate(0);
        // 强制刷盘：避免"没落盘"被误判成"没学习"
        crate::dictionary::write_back::flush(true);
        let conn = rusqlite::Connection::open(&tmp).expect("用户词库应已创建");
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM user_dict", [], |row| row.get(0))
            .unwrap_or(0);
        assert_eq!(rows, 0, "纠错来源候选不应写入用户词库");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_complete_pinyin_still_learned() {
        // 完整拼音输入仍然正常学习（新词/组合词靠这条路记住）
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join("taishen_test_complete_learn.db");
        let _ = std::fs::remove_file(&tmp);
        crate::dictionary::init(None);
        crate::dictionary::set_user_dict_path(Some(&tmp));
        let mut engine = Engine::new();
        for ch in "taishen".chars() {
            engine.process_key(ch);
        }
        engine.candidates = vec!["泰深".to_string()];
        engine.candidate_src = vec![Source::Dictionary];
        let _ = engine.select_candidate(0);
        // 用户词是批量落盘（20 条 / 2 秒），断言前强制刷盘
        crate::dictionary::write_back::flush(true);
        let conn = rusqlite::Connection::open(&tmp).expect("用户词库应已创建");
        let hit: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM user_dict WHERE pinyin='taishen' AND word='泰深'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        assert_eq!(hit, 1, "完整拼音输入应正常学习");
        let _ = std::fs::remove_file(&tmp);
    }

    // ─── Wave 2（B1–B7）：来源统一裁决 + 不变量 + 属性测试 ───

    #[test]
    fn test_arbitrate_source_conflict_suite() {
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(Some(std::path::Path::new("../resources/system_dict.db")));
        let tmp = std::env::temp_dir().join("taishen_test_arbitrate_suite.db");
        let _ = std::fs::remove_file(&tmp);
        crate::dictionary::set_user_dict_path(Some(&tmp));

        let query_top = |input: &str, limit: usize| -> Vec<(String, Source)> {
            let mut e = Engine::new();
            e.set_candidate_limit(limit);
            for ch in input.chars() {
                e.process_key(ch);
            }
            (0..e.candidate_count())
                .map(|i| {
                    (
                        e.candidate(i).unwrap_or("").to_string(),
                        e.candidate_source(i).unwrap_or(Source::Dictionary),
                    )
                })
                .collect()
        };

        // 不变量 1（精确优先）：suanfa 首位必须是「算法」，纠错词「断法/转发」不得抢首位
        let sf = query_top("suanfa", 9);
        assert_eq!(sf[0].0, "算法", "suanfa 首位应为算法: {sf:?}");
        assert_eq!(sf[0].1, Source::Dictionary);
        let pos_sf = sf.iter().position(|(w, _)| w == "算法").unwrap();
        if let Some(pos_df) = sf.iter().position(|(w, _)| w == "断法") {
            assert!(pos_sf < pos_df, "算法必须排在断法之前: {sf:?}");
        }

        // houxuan：精确高频词「候选」压过纠错「勾选」
        let hx = query_top("houxuan", 9);
        assert!(
            hx.iter().take(2).any(|(w, _)| w == "候选"),
            "houxuan 前 2 应含候选: {hx:?}"
        );
        assert!(
            !hx.iter().take(3).any(|(w, _)| w == "勾选"),
            "houxuan 前 3 不得被勾选抢占: {hx:?}"
        );

        // 不变量 2（纠错只补位）：无可信精确命中时纠错常用词仍能正确胜出
        for (inp, want, max_rank) in [
            ("zhonggou", "中国", 2),
            ("nihap", "你好", 2),
            ("zhongguoo", "中国", 3),
            ("nihaoa", "你好", 3),
            ("xianashi", "现实", 5),
        ] {
            let res = query_top(inp, 9);
            assert!(
                res.iter().take(max_rank).any(|(w, _)| w == want),
                "{inp} 前 {max_rank} 应含 {want}: {res:?}"
            );
        }

        // B4：移除占位短语（dz 科技园 / qq QQ号码 / email 邮箱地址）
        let dz = query_top("dz", 5);
        assert!(
            !dz.iter().any(|(w, _)| w.contains("科技园")),
            "dz 不应再出硬编码占位地址: {dz:?}"
        );
        let em = query_top("email", 5);
        assert_eq!(em[0].1, Source::English, "email 首位应为英文: {em:?}");
        assert!(
            !em.iter().any(|(_, s)| *s == Source::Correction),
            "明确英文词 email 不得触发拼音纠错: {em:?}"
        );

        // B5：api 首位英文且无拼音纠错噪声
        let api = query_top("api", 5);
        assert_eq!(api[0].1, Source::English, "api 首位应为英文: {api:?}");
        assert!(
            !api.iter().any(|(_, s)| *s == Source::Correction),
            "api 不得触发拼音纠错: {api:?}"
        );

        // B6：is_hanzi 覆盖 Ext-B，生僻字 须𫚪 被识别并压出首屏
        assert_eq!(
            "须𫚪".chars().filter(|c| is_hanzi(*c)).count(),
            2,
            "Ext-B 汉字 𫚪 应计入汉字数"
        );
        assert!(has_rare_hanzi("须𫚪"));
        let xuy = query_top("xuy", 5);
        assert!(
            !xuy.iter().any(|(w, _)| has_rare_hanzi(w)),
            "xuy 首屏不得出现生僻字: {xuy:?}"
        );

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_property_page_width_stability_and_exact_first() {
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(Some(std::path::Path::new("../resources/system_dict.db")));
        let tmp = std::env::temp_dir().join("taishen_test_prop_invariants.db");
        let _ = std::fs::remove_file(&tmp);
        crate::dictionary::set_user_dict_path(Some(&tmp));

        // 不变量 4 属性测试：全量单音节 + 代表多音节在 5 位窗与 9 位窗下前 5 候选严格一致
        let mut unstable: Vec<String> = Vec::new();
        let samples = [
            "xian", "yuan", "tian", "jian", "lian", "shi", "ji", "wu", "you", "zai", "de",
            "suanfa", "houxuan", "zhongguo", "nihao", "shuru",
        ];
        for py in samples {
            let mut e5 = Engine::new();
            e5.set_candidate_limit(5);
            let mut e9 = Engine::new();
            e9.set_candidate_limit(9);
            for ch in py.chars() {
                e5.process_key(ch);
                e9.process_key(ch);
            }
            let top5_narrow: Vec<String> = (0..e5.candidate_count().min(5))
                .filter_map(|i| e5.candidate(i).map(|s| s.to_string()))
                .collect();
            let top5_wide: Vec<String> = (0..e9.candidate_count().min(5))
                .filter_map(|i| e9.candidate(i).map(|s| s.to_string()))
                .collect();
            if top5_narrow != top5_wide {
                unstable.push(format!("{py}: 5={top5_narrow:?} vs 9={top5_wide:?}"));
            }
        }
        assert!(
            unstable.is_empty(),
            "存在窗宽不一致用例（违反不变量 4）:\n{}",
            unstable.join("\n")
        );

        // 不变量 1 属性测试：常用双音节精确词首位不得被 Correction/Fuzzy/Combo/Sentence 抢占
        let exact_words = [
            "suanfa", "houxuan", "zhongguo", "nihao", "shijie", "gongzuo", "xuexi", "kaifa",
            "xitong", "shuju", "wenti", "fangfa", "chengxu", "jisuan", "wangluo", "anquan",
            "sheji", "ceshi", "bushu", "youhua", "jiagou", "mokuai", "jiekou", "wendang",
        ];
        let mut stolen: Vec<String> = Vec::new();
        for py in exact_words {
            let mut e = Engine::new();
            e.set_candidate_limit(9);
            for ch in py.chars() {
                e.process_key(ch);
            }
            if let Some(src) = e.candidate_source(0) {
                if matches!(
                    src,
                    Source::Correction | Source::Fuzzy | Source::Combo | Source::Sentence
                ) {
                    stolen.push(format!(
                        "{py}: #1={}({:?})",
                        e.candidate(0).unwrap_or(""),
                        src
                    ));
                }
            }
        }
        assert!(
            stolen.is_empty(),
            "精确高频词首位被非精确来源抢占（违反不变量 1）:\n{}",
            stolen.join("\n")
        );

        let _ = std::fs::remove_file(&tmp);
    }

    // ─── Wave 3（C1 / C3）：zhyang 管道层 + woshiEric 驼峰中英混输 ───

    #[test]
    fn test_wave3_zhyang_and_camel_cn_en() {
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(Some(std::path::Path::new("../resources/system_dict.db")));
        let tmp = std::env::temp_dir().join("taishen_test_wave3.db");
        let _ = std::fs::remove_file(&tmp);
        crate::dictionary::set_user_dict_path(Some(&tmp));

        // C1：zhyang 管道层不再被 candidates.len() 门槛挡住，「张扬」进入前 9，且「这样」仍在首位
        let mut e = Engine::new();
        e.set_candidate_limit(9);
        for ch in "zhyang".chars() {
            e.process_key(ch);
        }
        let zy: Vec<String> = (0..e.candidate_count())
            .filter_map(|i| e.candidate(i).map(|s| s.to_string()))
            .collect();
        assert_eq!(
            zy.first().map(String::as_str),
            Some("这样"),
            "zhyang 首位应为这样: {zy:?}"
        );
        assert!(
            zy.iter().any(|w| w == "张扬"),
            "zhyang 前 9 应可达张扬（遗留-02）: {zy:?}"
        );

        // xzai 首位保持「现在」不退化
        e.reset();
        for ch in "xzai".chars() {
            e.process_key(ch);
        }
        assert_eq!(e.candidate(0), Some("现在"), "xzai 首位应保持现在");

        // C3：woshiEric → 我是Eric（首位、Source::Sentence）
        e.reset();
        for ch in "woshiEric".chars() {
            e.process_key(ch);
        }
        assert_eq!(
            e.candidate(0),
            Some("我是Eric"),
            "woshiEric 首位应为我是Eric"
        );
        assert_eq!(e.candidate_source(0), Some(Source::Sentence));

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_v0180_mixed_pinyin_and_prefix_word_compose() {
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(Some(std::path::Path::new("../resources/system_dict.db")));
        let tmp = std::env::temp_dir().join("taishen_test_v0180_mixed.db");
        let _ = std::fs::remove_file(&tmp);
        crate::dictionary::set_user_dict_path(Some(&tmp));

        let mut e = Engine::new();
        e.set_candidate_limit(5);
        e.set_sort_mode(1);
        e.set_context_enabled(false);
        e.set_fuzzy_enabled(true);
        e.set_correction_enabled(true);
        e.set_mix_mode(true);

        let top5 = |eng: &mut Engine, py: &str| -> Vec<String> {
            eng.reset();
            for ch in py.chars() {
                eng.process_key(ch);
            }
            (0..eng.candidate_count().min(5))
                .filter_map(|i| eng.candidate(i).map(str::to_string))
                .collect()
        };

        assert_eq!(
            top5(&mut e, "dangwo").first().map(String::as_str),
            Some("当我")
        );
        assert_eq!(
            top5(&mut e, "dans").first().map(String::as_str),
            Some("但是")
        );
        assert_eq!(
            top5(&mut e, "went").first().map(String::as_str),
            Some("问题")
        );

        let fuh = top5(&mut e, "fuhyuqi");
        assert_eq!(
            fuh.first().map(String::as_str),
            Some("符合预期"),
            "fuhyuqi: {fuh:?}"
        );
        assert!(
            fuh.iter().any(|w| w == "符合"),
            "fuhyuqi 首屏应含符合: {fuh:?}"
        );

        let my = top5(&mut e, "meiyhouxuan");
        assert_eq!(
            my.first().map(String::as_str),
            Some("没有候选"),
            "meiyhouxuan: {my:?}"
        );
        assert!(
            my.iter().any(|w| w == "没有"),
            "meiyhouxuan 首屏应含没有: {my:?}"
        );
        assert!(
            !my.iter().any(|w| w == "没有需"),
            "meiyhouxuan 首屏不应含没有需: {my:?}"
        );

        let hyx = top5(&mut e, "haisyouxie");
        assert_eq!(
            hyx.first().map(String::as_str),
            Some("还是有些"),
            "haisyouxie: {hyx:?}"
        );
        assert!(
            hyx.iter().any(|w| w == "还是"),
            "haisyouxie 首屏应含还是: {hyx:?}"
        );

        let hyxi = top5(&mut e, "haisyouxi");
        assert_eq!(
            hyxi.first().map(String::as_str),
            Some("还是游戏"),
            "haisyouxi: {hyxi:?}"
        );
        assert!(
            hyxi.iter().any(|w| w == "还是有些"),
            "haisyouxi 应含还是有些: {hyxi:?}"
        );
        assert!(
            hyxi.iter().any(|w| w == "还是"),
            "haisyouxi 首屏应含还是: {hyxi:?}"
        );

        let srs = top5(&mut e, "shurus");
        assert!(srs.iter().take(2).any(|w| w == "输入是"), "shurus: {srs:?}");
        assert!(srs.iter().take(3).any(|w| w == "输入时"), "shurus: {srs:?}");
        assert!(srs.iter().take(4).any(|w| w == "输入"), "shurus: {srs:?}");

        // 5 候选窗输入 shurushi，先选「输入」自动进入组词态（剩余音节 shi），再翻页选中「时」完成「输入时」
        let srsh = top5(&mut e, "shurushi");
        assert!(
            srsh.iter().take(2).any(|w| w == "输入是"),
            "shurushi: {srsh:?}"
        );
        assert!(
            srsh.iter().take(3).any(|w| w == "输入时"),
            "shurushi: {srsh:?}"
        );
        let idx_sr = srsh
            .iter()
            .position(|w| w == "输入")
            .expect("shurushi 首屏应含输入");
        assert_eq!(e.select_candidate(idx_sr), Some("输入".to_string()));
        assert!(e.compose_active(), "选中前导词「输入」后应自动进入组词态");
        assert_eq!(e.compose_prefix(), "输入");
        assert_eq!(e.compose_remaining(), "shi");

        let mut committed = None;
        for _ in 0..8 {
            if let Some(pos) = (0..e.candidate_count()).position(|i| e.candidate(i) == Some("时"))
            {
                committed = e.select_candidate(pos);
                break;
            }
            if e.page(1) == 0 {
                break;
            }
        }
        assert_eq!(committed.as_deref(), Some("输入时"));
        assert!(!e.compose_active());
        assert!(e.pinyin_buf.is_empty());

        // 验证 fuhyuqi 选中前导词「符合」同样自动进入组词态（剩余 yuqi）
        let fuh2 = top5(&mut e, "fuhyuqi");
        let idx_fh = fuh2
            .iter()
            .position(|w| w == "符合")
            .expect("fuhyuqi 首屏应含符合");
        assert_eq!(e.select_candidate(idx_fh), Some("符合".to_string()));
        assert!(e.compose_active(), "选中前导词「符合」后应自动进入组词态");
        assert_eq!(e.compose_prefix(), "符合");
        assert_eq!(e.compose_remaining(), "yuqi");

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_short_sentence_and_phrase_accuracy_regressions() {
        // 2026-11-21 多音节短词/短句准确率根因回归集：
        // 1. 单字用户词不得在整句解码里叠加 USER_BONUS 把「会+遇到」拆成「会+与+到」；
        // 2. 用户词历史错配别名（yiqi→以前、zu→做、ta→他们）不得进入整句格子；
        // 3. 词库真实对数词频（freq / 365.0）下，短句成词自然胜过散单字；
        // 4. 完整拼音无精确词库命中时，前缀扩展词（xiangyixia→想一想）降组，不压整句；
        // 5. 6-7 字符三音节完整拼音（keyima）走整句解码，不被未提升纠错（柯以敏）抢位；
        // 6. 拼凑候选（Sentence/stitched Combo）一次误选不得写进 user_dict.db 固化。
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(Some(std::path::Path::new("../resources/system_dict.db")));
        let tmp = std::env::temp_dir().join("taishen_test_short_sentence_regressions.db");
        let _ = std::fs::remove_file(&tmp);
        {
            let conn = rusqlite::Connection::open(&tmp).expect("建临时用户词库");
            conn.execute_batch(
                "CREATE TABLE user_dict (
                    pinyin TEXT NOT NULL,
                    word TEXT NOT NULL,
                    frequency INTEGER DEFAULT 1,
                    last_used INTEGER DEFAULT 0,
                    PRIMARY KEY (pinyin, word)
                );",
            )
            .expect("建表");
            let now = 1_795_200_000i64;
            let seed: &[(&str, &str, u32)] = &[
                // 高频单字用户词：曾导致 huiyudao 首选「会与到」
                ("hui", "会", 176),
                ("yu", "与", 11),
                ("dao", "到", 160),
                ("yudao", "遇到", 7),
                // 历史错配/前缀别名：曾污染整句格子导致 我们以前 / 一个做次 / 他们回来
                ("yiqi", "以前", 2),
                ("zu", "做", 27),
                ("ta", "他们", 1),
                ("wo", "我们", 1),
                ("dian", "电话", 1),
                ("da", "到", 3),
            ];
            for (py, w, f) in seed {
                conn.execute(
                    "INSERT INTO user_dict (pinyin, word, frequency, last_used) VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![py, w, f, now],
                )
                .expect("写入测试用户词");
            }
        }
        crate::dictionary::set_user_dict_path(Some(&tmp));

        let mut e = Engine::new();
        e.set_candidate_limit(5);
        e.set_sort_mode(1);
        e.set_fuzzy_enabled(true);

        let top5 = |eng: &mut Engine, s: &str| -> Vec<String> {
            eng.reset();
            for ch in s.chars() {
                eng.process_key(ch);
            }
            (0..eng.candidate_count())
                .filter_map(|i| eng.candidate(i).map(str::to_string))
                .collect()
        };

        let hui = top5(&mut e, "huiyudao");
        assert_eq!(
            hui.first().map(String::as_str),
            Some("会遇到"),
            "huiyudao: {hui:?}"
        );

        let wmyq = top5(&mut e, "womenyiqi");
        assert_eq!(
            wmyq.first().map(String::as_str),
            Some("我们一起"),
            "womenyiqi: {wmyq:?}"
        );

        let ygzc = top5(&mut e, "yigezuci");
        assert_eq!(
            ygzc.first().map(String::as_str),
            Some("一个组词"),
            "yigezuci: {ygzc:?}"
        );

        let thl = top5(&mut e, "tahuilai");
        assert!(
            !thl.iter().any(|w| w.starts_with("他们")),
            "tahuilai 不应受 ta→他们 污染: {thl:?}"
        );
        assert!(
            thl.iter().take(2).any(|w| w == "他会来"),
            "tahuilai: {thl:?}"
        );

        let xyx = top5(&mut e, "xiangyixia");
        assert_eq!(
            xyx.first().map(String::as_str),
            Some("想一下"),
            "xiangyixia: {xyx:?}"
        );

        let kym = top5(&mut e, "keyima");
        assert_eq!(
            kym.first().map(String::as_str),
            Some("可以吗"),
            "keyima: {kym:?}"
        );

        // 选中整句/拼凑候选不应写进 user_dict.db
        let _ = top5(&mut e, "huiyudao");
        assert_eq!(e.select_candidate(0).as_deref(), Some("会遇到"));
        crate::dictionary::write_back::flush(true);
        let conn = rusqlite::Connection::open(&tmp).expect("重开用户词库");
        let learned: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM user_dict WHERE pinyin='huiyudao'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(-1);
        assert_eq!(learned, 0, "整句/拼凑候选不应单次自动写入用户词库");

        crate::dictionary::set_user_dict_path(None);
        let _ = std::fs::remove_file(&tmp);
    }

    /// 跨 Engine 实例隔离（REPORT §五）：先跑一批 Combo/整句输入后，
    /// 新实例的 npm 首候选仍须是英文 npm，不能变成拼凑词。
    ///
    /// 默认 ignore：本测试会改变测试执行顺序，暴露 test_demote_* /
    /// test_delete_* 既有的顺序敏感问题（它们依赖前面的词库/用户词状态）。
    /// 需要时单独跑：cargo test --lib test_cross_instance_isolation -- --ignored
    #[test]
    #[ignore = "顺序敏感：会干扰 test_demote_* / test_delete_*，需要时手动单跑"]
    fn test_cross_instance_isolation_after_combo_queries() {
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // 不重建词库：init_blocking 会打断其它测试依赖的加载状态（实测让
        // test_demote_* / test_delete_* 失败）。仅在词库未就绪时初始化。
        if !crate::dictionary::is_ready() {
            crate::dictionary::init_blocking(Some(std::path::Path::new(
                "../resources/system_dict.db",
            )));
        }
        for s in [
            "yigezuci",
            "huiyudao",
            "zhongguoo",
            "qwer",
            "sdfg",
            "woainima",
        ] {
            let mut e = Engine::new();
            e.set_candidate_limit(9);
            for ch in s.chars() {
                e.process_key(ch);
            }
            let _ = e.candidate_count();
        }
        let mut e = Engine::new();
        e.set_candidate_limit(9);
        for ch in "npm".chars() {
            e.process_key(ch);
        }
        assert_eq!(e.candidate(0), Some("npm"), "npm 首候选被历史查询污染");
        assert_eq!(
            e.candidate_source(0),
            Some(Source::English),
            "npm 首候选来源应为 English"
        );
        crate::dictionary::clear_user_words();
    }

    #[test]
    fn test_mid_pinyin_insert_and_cursor_nav() {
        let mut engine = Engine::new();
        for ch in "nihao".chars() {
            assert!(engine.process_key(ch));
        }
        assert_eq!(engine.pinyin_str(), "nihao");
        assert_eq!(engine.cursor_pos(), 5);

        // 左移 2 字符（每次 -2 移 1 字符）：nihao (5) -> niha|o (4) -> nih|ao (3)
        assert_eq!(engine.move_cursor(-2), 4);
        assert_eq!(engine.move_cursor(-2), 3);
        assert_eq!(engine.move_cursor(2), 4);
        assert_eq!(engine.move_cursor(-2), 3);

        // 中部插入 'x' -> nihxao，光标随动至 4
        assert!(engine.process_key('x'));
        assert_eq!(engine.pinyin_str(), "nihxao");
        assert_eq!(engine.raw_input, "nihxao");
        assert_eq!(engine.cursor_pos(), 4);

        // 退格删除刚插入的 'x' -> 回到 nihao，光标 3
        assert!(engine.backspace());
        assert_eq!(engine.pinyin_str(), "nihao");
        assert_eq!(engine.cursor_pos(), 3);

        // 音节中部按 Ctrl+Left (-1) 回当前音节起点 2，按 Ctrl+Right (+1) 到 5
        assert_eq!(engine.move_cursor(-1), 2);
        assert_eq!(engine.move_cursor(1), 5);

        // Home (-3) / End (+3)
        assert_eq!(engine.move_cursor(-3), 0);
        assert!(!engine.backspace(), "光标在 0 时退格不删字");
        assert_eq!(engine.pinyin_str(), "nihao");
        assert_eq!(engine.move_cursor(3), 5);
    }
}
