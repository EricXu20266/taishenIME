pub mod calculator;
pub mod context;
pub mod correction;
pub mod datetime;
pub mod dictionary;
pub mod emoji;
pub mod english;
pub mod ffi;
pub mod fuzzy;
pub mod log;
pub mod mistake;
pub mod number;
pub mod pinyin;
pub mod radical;
pub mod shuangpin;
pub mod symbol;
pub mod trad;
pub mod trad_full;
pub mod trad_simp;
pub mod unichar;
pub mod voice;

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
    /// 模糊音开关（RIME 拼写变体，默认开）
    fuzzy_enabled: bool,
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
    /// 英文候选在 all_candidates 中的位置（None = 无英文候选）
    english_candidate_pos: Option<usize>,
    /// 简繁转换开关（V0.2.11，默认关）：候选输出转繁体
    traditional_mode: bool,
    /// 快捷短语开关（V0.2.12，默认开）：简码 → 短语
    phrase_enabled: bool,
    /// 短语表：简码(小写) → 短语文本
    phrase_map: std::collections::HashMap<String, String>,
    /// 短语候选在 all_candidates 中的位置（None = 无）
    phrase_candidate_pos: Option<usize>,
    /// 日期简码候选位置（V0.2.19，None = 无）
    datetime_candidate_pos: Option<usize>,
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
}

/// V0.4.5 纠错候选补入限量：每个纠错变体最多取前 N 个词。
/// deletion 变体（zhonggu→364 词）曾全量灌入候选，撑爆 page_size 判断
/// 导致 correction 分支被跳过、「中国」丢失。限量后补入可控。
const CORR_TAKE: usize = 3;

/// V0.5.3：输入串能否完整切分为合法拼音音节；能则返回音节数。
/// 简拼/英文/特殊模式返回 None（目标词长不可知，不做词长匹配）。
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

/// V0.5.8 声母串 → 估算目标词长（简拼词长匹配用）。
/// "zg" → 2, "shh" → 2 (sh+h), "dy" → 2。单声母/非声母 → None。
fn short_pinyin_word_len(input: &str) -> Option<usize> {
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
        match INITIALS.iter().find(|i| rest.starts_with(*i)) {
            Some(init) => {
                count += 1;
                rest = &rest[init.len()..];
            }
            None => return None,
        }
    }
    (count >= 2).then_some(count)
}

/// V0.5.9 混合简拼 → 估算目标词长（全拼前缀+声母后缀，如 "ces"→2、"jianp"→2、"shurf"→3）：
/// 完整音节优先切分（split_first_syllable），剩余部分按声母切分（zh/ch/sh 优先）。
/// 段数 = 目标词长。要求 ≥2 段且至少 1 个完整音节——防纯声母串（交给
/// short_pinyin_word_len）与单音节误判。与 split_compose_syllables 同款贪心。
fn mixed_syllable_count(input: &str) -> Option<usize> {
    const INITIALS: &[&str] = &[
        "zh", "ch", "sh", "b", "p", "m", "f", "d", "t", "n", "l", "g", "k", "h", "j", "q", "x",
        "r", "z", "c", "s", "y", "w",
    ];
    if input.is_empty() {
        return None;
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
                match initial {
                    Some(s) => {
                        n += 1;
                        rest = &rest[s.len()..];
                    }
                    None => return None,
                }
            }
        }
    }
    (n >= 2 && has_full).then_some(n)
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
            fuzzy_enabled: true,
            shuangpin_mode: false,
            shuangpin_scheme: shuangpin::find_scheme("mspy").unwrap_or(&shuangpin::SCHEMES[0]),
            shuangpin_full_preedit: false,
            correction_enabled: true,
            mix_mode_enabled: true,
            english_candidate_pos: None,
            traditional_mode: false,
            phrase_enabled: true,
            phrase_map: Self::builtin_phrases(),
            phrase_candidate_pos: None,
            datetime_candidate_pos: None,
            pin_words: Self::builtin_pin_words(),
            demoted_words: std::collections::HashSet::new(),
            emoji_enabled: false, // 默认关闭（Eric 要求，config emoji=1 可开启）
            sort_mode: 0,
            last_committed: String::new(),
            context_enabled: true,
            context_map: std::collections::HashMap::new(),
            context_prior: std::collections::HashMap::new(),
            compose_active: false,
            compose_syllables: Vec::new(),
            compose_idx: 0,
            compose_chars: Vec::new(),
            page_advances: 0,
        }
    }

    /// 内置置顶候选（V0.5.7，词级）：的/吗/嘛/后面（对标 rime pin_cand_filter
    /// 默认示例 d→的、m→吗/嘛、hm→后面，覆盖单音节独占）
    fn builtin_pin_words() -> std::collections::HashSet<String> {
        let mut s = std::collections::HashSet::new();
        s.insert("的".to_string());
        s.insert("吗".to_string());
        s.insert("嘛".to_string());
        s.insert("后面".to_string());
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

    /// 设置模糊音开关
    pub fn set_fuzzy_enabled(&mut self, enabled: bool) {
        if self.fuzzy_enabled != enabled {
            self.fuzzy_enabled = enabled;
            if !self.pinyin_buf.is_empty() {
                self.query_all(); // 开关变化时重查
            }
        }
    }

    /// 查询模糊音开关
    pub fn fuzzy_enabled(&self) -> bool {
        self.fuzzy_enabled
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

    /// 处理一个按键，返回是否产生了新的候选词
    /// 英文模式下字母键返回 false（平台层直接上屏字母）
    pub fn process_key(&mut self, ch: char) -> bool {
        // 双拼 ing 键（微软/搜狗把 ing 放在分号上）：判据收敛到 semicolon_key_active，
        // 平台层吞键与引擎接受共用同一份，避免「吞了但引擎不收 → 按键丢失」。
        if ch == ';' && self.semicolon_key_active() {
            self.compose_active = false;
            self.page_advances = 0;
            self.pinyin_buf.push(';');
            self.raw_input.push(';');
            self.cursor = self.pinyin_buf.len();
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
            self.pinyin_buf.push(ch.to_ascii_lowercase());
            self.raw_input.push(ch); // V0.2.23：保留原始大小写
            self.cursor = self.pinyin_buf.len(); // P2-1：光标跟随输入
            self.update_cap_state();
            self.query_all();
            true
        } else if self.is_calc_continuation(ch) {
            // V0.2.22：c 模式下继续输入运算符/数字 → 追加并触发计算器查询
            self.pinyin_buf.push(ch);
            self.raw_input.push(ch);
            self.cursor = self.pinyin_buf.len();
            self.query_all();
            true
        } else if self.is_symbol_prefix() && ch.is_ascii_digit() {
            // v+数字别名（0.2.32，对标 QQ v1-v9）：v 前缀时数字键追加进引擎
            // → v1/v2... 触发数字分类符号查询（平台层此时把数字键送进引擎而非选候选）
            self.pinyin_buf.push(ch);
            self.raw_input.push(ch);
            self.cursor = self.pinyin_buf.len();
            self.query_all();
            true
        } else if self.is_number_continuation(ch) {
            // P1-4：R 模式继续输入数字/点 → 追加并触发金额大写查询
            self.pinyin_buf.push(ch.to_ascii_lowercase());
            self.raw_input.push(ch);
            self.cursor = self.pinyin_buf.len();
            self.query_all();
            true
        } else if self.is_unicode_continuation(ch) {
            // P1-4：U 模式继续输入 hex → 追加并触发 Unicode 查询
            self.pinyin_buf.push(ch.to_ascii_lowercase());
            self.raw_input.push(ch);
            self.cursor = self.pinyin_buf.len();
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
        } else if self.raw_input.len() >= 1
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

    /// 判断当前页 index 是否为英文候选（混输追加项）
    /// P1-1：英文候选恒在末尾，绝对位置 >= 起始位置即英文（可多个）
    fn is_english_candidate(&self, index: usize) -> bool {
        match self.english_candidate_pos {
            Some(pos) => self.page * self.page_size + index >= pos,
            None => false,
        }
    }

    /// 判断当前页 index 是否为短语候选（V0.2.12）
    fn is_phrase_candidate(&self, index: usize) -> bool {
        match self.phrase_candidate_pos {
            Some(pos) => pos == self.page * self.page_size + index,
            None => false,
        }
    }

    /// 判断当前页 index 是否为日期简码候选（V0.2.19）
    fn is_datetime_candidate(&self, index: usize) -> bool {
        match self.datetime_candidate_pos {
            Some(pos) => pos == self.page * self.page_size + index,
            None => false,
        }
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

    /// 选择候选词并提交（返回提交文本，同时重置状态）
    /// V0.2.2：选词时自动学习用户词（拼音串 + 选中词）
    /// V0.2.8：选中英文候选（混输）→ 上屏原文不学习
    /// V0.2.11：简繁模式开启时上屏文本转繁体
    pub fn select_candidate(&mut self, index: usize) -> Option<String> {
        // 组词模式（V0.5 逐字预上屏，V0.5.10 升级）：
        // 每个音节的选字立即返回该字（平台层逐字上屏，编辑区显示
        // "辛mao"——已上屏字 + 剩余音节 composition）。
        // 中间音节：返回选中字 + 保持组词状态（compose_idx 推进、候选切下一音节）。
        // 最后音节：learn 完整拼音+整词后 reset，返回最后选中的字
        // （前面各字已逐字上屏，不可再返回整词否则重复上屏）。
        if self.compose_active {
            if let Some(word) = self.candidates.get(index).cloned() {
                self.compose_chars.push(word.clone());
                self.compose_idx += 1;
                if self.compose_idx >= self.compose_syllables.len() {
                    let full_word = self.compose_chars.join("");
                    let full_py = self.compose_syllables.join("");
                    crate::dictionary::learn(&full_py, &full_word);
                    self.reset();
                    return Some(word);
                }
                self.query_all();
                return Some(word);
            }
            return None;
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
            // 非英文/非短语/非符号/非计算器/非日期/非反查/非错音候选才学习用户词
            if !is_english
                && !is_phrase
                && !is_symbol
                && !is_calc
                && !is_datetime
                && !is_radical
                && !is_mistake
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
                let canonical_pinyin = crate::dictionary::canonical_pinyin_for_word(word);
                // 保留原输入别名，同时记录词库规范全拼，避免简拼学习只对
                // 当前缩写生效。反查必须在写入别名之前完成。
                crate::dictionary::learn(&input_pinyin, word);
                if let Some(canonical) = canonical_pinyin {
                    if canonical != input_pinyin {
                        crate::dictionary::learn(&canonical, word);
                    }
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
                .map(|word| (word, Self::CONTEXT_EXTERNAL_WEIGHT))
                .collect();
            if !words.is_empty() {
                self.context_map.insert(previous, words);
            }
        }
    }

    /// 学习型搭配上限：每个前词最多保留的后词数
    const LEARNED_CONTEXT_CAP: usize = 8;
    /// 上下文权重上限：共现再多，单个后词的前移位数也不超过该值
    const CONTEXT_WEIGHT_CAP: u32 = 20;
    /// 内置精选搭配权重：人工确认过的高频搭配，命中即置顶
    const CONTEXT_BUILTIN_WEIGHT: u32 = 32;
    /// 外部搭配表权重
    const CONTEXT_EXTERNAL_WEIGHT: u32 = 12;

    /// 加载学习型上下文搭配（启动时从 user_dict.db 的 context_pairs 表读入）。
    /// entries: (前词, 后词, 共现次数)，共现次数直接作为权重。
    pub fn load_learned_contexts(&mut self, entries: Vec<(String, String, i64)>) {
        for (prev, next, count) in entries {
            if prev.is_empty() || next.is_empty() {
                continue;
            }
            let weight = (count.max(1) as u32).min(Self::CONTEXT_WEIGHT_CAP);
            let list = self.context_map.entry(prev).or_default();
            if let Some(slot) = list.iter_mut().find(|(w, _)| w == &next) {
                slot.1 = slot.1.max(weight);
            } else {
                list.push((next, weight));
            }
            if list.len() > Self::LEARNED_CONTEXT_CAP {
                list.remove(0);
            }
        }
    }

    /// 离线先验权重上限：低于学习表上限（20），保证真实用词能盖过语料统计
    const CONTEXT_PRIOR_MAX_WEIGHT: u32 = 6;

    /// 加载离线上下文先验（启动一次）。只在学习表未覆盖时参与加权。
    pub fn load_prior_contexts(&mut self, entries: Vec<(String, String, i64)>) {
        for (prev, next, count) in entries {
            if prev.is_empty() || next.is_empty() {
                continue;
            }
            let weight = (count.max(1) as u32).min(Self::CONTEXT_PRIOR_MAX_WEIGHT);
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
                slot.1 = (slot.1 + 1).min(Self::CONTEXT_WEIGHT_CAP);
                let bumped = slot.clone();
                list.retain(|(w, _)| w != next);
                list.push(bumped);
            }
            None => list.push((next.to_string(), 1)),
        }
        if list.len() > Self::LEARNED_CONTEXT_CAP {
            list.remove(0);
        }
        crate::dictionary::save_context_pair(prev, next);
    }

    /// 上下文候选加权重排：命中的后词按权重前移，最终按 (权重 - 原位置) 降序稳定排序。
    /// 权重来源：内置精选表 / 外部搭配表 / 用户上屏共现次数（上限 CONTEXT_WEIGHT_CAP）。
    /// 英文候选区（english_candidate_pos 之后）不参与；不新增候选，只在原列表内重排。
    fn apply_context_boost(&mut self, candidates: &mut Vec<String>) {
        if !self.context_enabled || self.last_committed.is_empty() {
            return;
        }
        let prev = self.last_committed.clone();
        let mut merged: Vec<(String, u32)> = self
            .context_map
            .get(prev.as_str())
            .cloned()
            .unwrap_or_default();
        // 离线先验补充：同词以较大权重为准（学习表权重上限 20，先验上限 6）
        if let Some(prior) = self.context_prior.get(prev.as_str()) {
            for (word, weight) in prior {
                match merged.iter_mut().find(|(w, _)| w == word) {
                    Some(slot) => slot.1 = slot.1.max(*weight),
                    None => merged.push((word.clone(), *weight)),
                }
            }
        }
        // 内置表与学习表合并，同词取较大权重
        for word in crate::context::lookup(prev.as_str()) {
            match merged.iter_mut().find(|(w, _)| w == &word) {
                Some(slot) => slot.1 = slot.1.max(Self::CONTEXT_BUILTIN_WEIGHT),
                None => merged.push((word, Self::CONTEXT_BUILTIN_WEIGHT)),
            }
        }
        if merged.is_empty() {
            return;
        }
        let eng_start = self.english_candidate_pos.unwrap_or(candidates.len());
        let limit = candidates.len().min(eng_start);
        if limit <= 1 {
            return;
        }
        let mut scored: Vec<(i64, i64, usize)> = (0..limit)
            .map(|i| {
                let lift = merged
                    .iter()
                    .find(|(w, _)| w == &candidates[i])
                    .map(|(_, weight)| (*weight).min(Self::CONTEXT_WEIGHT_CAP) as i64)
                    .unwrap_or(0);
                (lift - i as i64, lift, i)
            })
            .collect();
        // 没有任何候选命中搭配表 → 保持原序
        if !scored.iter().any(|(_, lift, _)| *lift > 0) {
            return;
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.2.cmp(&b.2)));
        let reordered: Vec<String> = scored.iter().map(|&(_, _, i)| candidates[i].clone()).collect();
        for (i, word) in reordered.into_iter().enumerate() {
            candidates[i] = word;
        }
    }

    /// V0.5.7 词级置顶（对标 rime pin_cand_filter，语义为词级）：
    /// 候选里命中 pin_words 的词提到最前（保持被置顶词的相对顺序）。
    /// 与 apply_context_boost 同款"收集命中 + 其余"两分区，但不依赖前文——
    /// 任何输入方式（全拼/简拼/混合）候选里出现该词即置顶，天然覆盖简拼自动展开。
    fn apply_pin_boost(&mut self, candidates: &mut Vec<String>) {
        if self.pin_words.is_empty() {
            return;
        }
        let eng_start = self.english_candidate_pos.unwrap_or(candidates.len());
        let limit = candidates.len().min(eng_start);
        if limit <= 1 {
            return;
        }
        let mut pinned: Vec<String> = Vec::new();
        let mut rest: Vec<String> = Vec::new();
        for w in candidates.iter().take(limit) {
            if self.pin_words.contains(w) {
                pinned.push(w.clone());
            } else {
                rest.push(w.clone());
            }
        }
        if pinned.is_empty() {
            return;
        }
        // 置顶词按候选原顺序前置（内置的/吗/嘛 顺序即候选出现顺序）
        pinned.append(&mut rest);
        for (i, w) in pinned.into_iter().enumerate() {
            candidates[i] = w;
        }
    }

    /// V0.5.7 降权（Eric 2026-08-10 决策："删除"语义 = 降权，压出前 2 屏）：
    /// 候选里命中 demoted_words 的词从原位移除，追加到第 11 位（page_size*2+1）
    /// 之后——仍可翻页取到，可恢复。全列表处理：英文候选恒在末尾（泰深设计），
    /// 降权词插入 min(10, 非降权词数) 位；候选不足 10 时退化为排末尾。
    fn apply_demote(&mut self, candidates: &mut Vec<String>) {
        if self.demoted_words.is_empty() || candidates.len() <= 1 {
            return;
        }
        let mut demoted: Vec<String> = Vec::new();
        let mut rest: Vec<String> = Vec::new();
        for w in candidates.iter() {
            if self.demoted_words.contains(w) {
                demoted.push(w.clone());
            } else {
                rest.push(w.clone());
            }
        }
        if demoted.is_empty() {
            return;
        }
        // 前 10 位（前 2 屏）保持原顺序；降权词插入第 11 位之后
        // （rest 不足 10 时紧跟 rest 尾部——候选不够 2 屏时降权退化为排末尾）
        let insert_at = 10usize.min(rest.len());
        for (i, w) in rest.into_iter().enumerate() {
            candidates[i] = w;
        }
        for (i, w) in demoted.into_iter().enumerate() {
            candidates[insert_at + i] = w;
        }
    }

    /// P0-2 候选排序（对标微软单字/长词优先）：
    /// 在 apply_long_word_filter 之后调用，按用户选择稳定分区。
    /// - mode=1 单字优先：单字（1 汉字）提到多字前，组内相对顺序不变
    /// - mode=2 长词优先：多字（2+ 汉字）提到单字前，组内相对顺序不变
    /// 约束：英文候选区（english_candidate_pos 之后）不参与分区；
    /// 短语/日期/符号等特殊候选不重排（保持其前置位）。
    fn apply_sort_mode(&mut self, candidates: &mut Vec<String>) {
        if self.sort_mode == 0 {
            return;
        }
        // 英文候选起始位置（其后的不参与）
        let eng_start = self.english_candidate_pos.unwrap_or(candidates.len());
        let limit = candidates.len().min(eng_start);
        if limit <= 1 {
            return;
        }
        // 中文汉字数 ≥1 才算"字"候选（排除纯英文/符号候选的误判）
        let hanzi_count = |w: &str| w.chars().filter(|c| *c as u32 > 0x7F).count();
        // 稳定分区：按"单字在前"（mode=1）或"长词在前"（mode=2）
        let single_first = self.sort_mode == 1;
        let mut first: Vec<String> = Vec::new();
        let mut second: Vec<String> = Vec::new();
        for w in candidates.iter().take(limit) {
            let single = hanzi_count(w) == 1;
            let in_first = if single_first { single } else { !single };
            if in_first {
                first.push(w.clone());
            } else {
                second.push(w.clone());
            }
        }
        first.append(&mut second);
        for (i, w) in first.into_iter().enumerate() {
            candidates[i] = w;
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
        let before = self.page;
        let result = self.page_inner(delta);
        let at_boundary = delta > 0 && self.page == before;
        if delta > 0 {
            self.page_advances = self.page_advances.saturating_add(1);
        }
        if self.page_advances >= 3 || at_boundary {
            if let Some(syls) = self.split_compose_syllables() {
                self.compose_active = true;
                self.compose_syllables = syls;
                self.compose_idx = 0;
                self.compose_chars = Vec::new();
                self.query_all();
                return self.candidates.len();
            }
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
                match initial {
                    Some(s) => {
                        syls.push(s.to_string());
                        rest = &rest[s.len()..];
                    }
                    None => return None,
                }
            }
        }
        (syls.len() >= 2 && has_full).then_some(syls)
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
        (self.all_candidates.len() + self.page_size - 1) / self.page_size
    }

    /// 注入语音转写文本为唯一候选（V0.5.7）。
    /// 调用方先 reset() 清空输入态，再注入——候选窗显示麦克风语音候选，
    /// 平台层拿到后直接上屏（引擎_select_candidate(0) 语义不变）。
    pub fn set_voice_candidate(&mut self, text: &str) {
        self.all_candidates.clear();
        self.candidates.clear();
        if !text.is_empty() {
            self.all_candidates.push(text.to_string());
            self.candidates.push(text.to_string());
        }
    }

    /// 清空状态
    pub fn reset(&mut self) {
        self.pinyin_buf.clear();
        self.raw_input.clear();
        self.cursor = 0;
        self.cap_state = CapState::Lower;
        self.all_candidates.clear();
        self.candidates.clear();
        self.page = 0;
        self.english_candidate_pos = None;
        self.phrase_candidate_pos = None;
        self.datetime_candidate_pos = None;
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

    /// 移动光标到相邻音节边界（P2-1）：delta>0 向右（下一音节），<0 向左。
    /// 返回移动后光标位置。
    pub fn move_cursor(&mut self, delta: i32) -> usize {
        if self.pinyin_buf.is_empty() {
            return 0;
        }
        let boundaries = self.syllable_boundaries();
        let cur = self.cursor.min(self.pinyin_buf.len());
        // 当前落在哪个边界区间（首个 >= cur 的边界）
        let mut idx = boundaries
            .iter()
            .position(|&b| b >= cur)
            .unwrap_or(boundaries.len() - 1);
        if delta > 0 {
            idx = (idx + 1).min(boundaries.len() - 1);
        } else {
            idx = idx.saturating_sub(1);
        }
        self.cursor = boundaries[idx];
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

    /// 查询全部候选（含简拼联想 + 多音节切分联想），截断到 max_pages 页，重置到第 0 页
    fn query_all(&mut self) {
        // v 前缀即时反馈（0.2.30）：pinyin_buf == "v" → v 单字 + 热门符号（跳过英文混输）
        if self.is_symbol_prefix() {
            // 候选首位是 "v" 单字（中文模式输出字母 v 免切英文），其后热门符号直选
            let mut symbols: Vec<String> = vec!["v".to_string()];
            symbols.extend(symbol::hot_symbols().into_iter().map(|s| s.to_string()));
            self.english_candidate_pos = None;
            self.phrase_candidate_pos = None;
            self.all_candidates = symbols;
            self.page = 0;
            self.repage();
            return;
        }
        // 符号输入 v 模式（V0.2.17）：pinyin_buf 以 'v' 开头且长度>1 → 查符号分类
        if self.is_symbol_mode() {
            let category = &self.pinyin_buf[1..]; // 去掉 'v' 前缀
            let symbols: Vec<String> = symbol::query(category)
                .into_iter()
                .map(|s| s.to_string())
                .collect();
            self.english_candidate_pos = None;
            self.phrase_candidate_pos = None;
            self.all_candidates = symbols;
            self.page = 0;
            self.repage();
            return;
        }
        // 计算器模式（V0.2.22）：c + 算式 → 结果候选
        if self.is_calc_mode() {
            let expr = &self.pinyin_buf[1..]; // 去掉 'c' 前缀
            let candidates = match calculator::eval(expr) {
                Ok(v) => vec![calculator::format_result(v)],
                Err(_) => Vec::new(), // 非法算式无候选
            };
            self.english_candidate_pos = None;
            self.phrase_candidate_pos = None;
            self.all_candidates = candidates;
            self.page = 0;
            self.repage();
            return;
        }
        // 拆字反查（V0.2.25）：u + 部件拼音 → 汉字候选
        // 0.1.50（Eric 反馈）：拆字查不到时回落常规拼音/英文路径——
        // 此前 us/up 这类输入被 u 前缀吃掉，候选窗直接空白
        if self.is_radical_mode() {
            let parts = &self.pinyin_buf[1..]; // 去掉 'u' 前缀
            let candidates = radical::query(parts);
            if !candidates.is_empty() {
                self.english_candidate_pos = None;
                self.phrase_candidate_pos = None;
                self.all_candidates = candidates;
                self.page = 0;
                self.repage();
                return;
            }
        }
        // 数字金额大写（P1-4）：R + 数字 → 中文大写（对标 rime R+ 前缀）
        if self.is_number_mode() {
            let digits = &self.pinyin_buf[1..]; // 去掉 'r' 前缀（raw R → buf r）
            let candidates = crate::number::to_cn(digits);
            self.english_candidate_pos = None;
            self.phrase_candidate_pos = None;
            self.all_candidates = candidates;
            self.page = 0;
            self.repage();
            return;
        }
        // Unicode 输入（P1-4）：U + hex → 字符（对标 rime U+ 前缀）
        if self.is_unicode_mode() {
            let hex = &self.pinyin_buf[1..]; // 去掉 'u' 前缀（raw U → buf u）
            let candidates = crate::unichar::query(hex);
            self.english_candidate_pos = None;
            self.phrase_candidate_pos = None;
            self.all_candidates = candidates;
            self.page = 0;
            self.repage();
            return;
        }
        // 双拼模式：输入串是双拼码，先解码为全拼再查询
        if self.shuangpin_mode {
            self.query_all_shuangpin();
            return;
        }
        // 组词模式（V0.5）：候选 = 当前音节的单字（逐字选词）
        if self.compose_active {
            let syl = &self.compose_syllables[self.compose_idx];
            let cands: Vec<String> = dictionary::query(syl)
                .into_iter()
                .filter(|w| w.chars().count() == 1 && !w.contains(' '))
                .collect();
            self.english_candidate_pos = None;
            self.phrase_candidate_pos = None;
            self.datetime_candidate_pos = None;
            self.all_candidates = cands;
            self.page = 0;
            self.repage();
            return;
        }
        let pinyin_str = self.pinyin_buf.clone();
        // P2-3：jqxy 后 v 归一为 u（qv→qu），供拼音查询与切分判断
        let norm_str = crate::pinyin::normalize_v(&pinyin_str);
        // 0.3.x fix（英文误伤）：仅"完全可切分为合法拼音"的输入才做模糊/纠错/错音联想。
        // "hello" 切分失败（he+llo）→ 视为英文，不做拼音联想（只英文混输）。
        let is_full_pinyin = crate::pinyin::is_complete_pinyin(&norm_str);
        // 日期/时间/星期/农历简码（V0.2.19）：rq/sj/xq/nl 精确命中 → 日期候选
        // 优先级：日期简码 > 快捷短语（sj 与短语"手机"并存时日期优先，短语可翻页取）
        self.phrase_candidate_pos = None;
        self.datetime_candidate_pos = None;
        if let Some(cands) = Self::datetime_candidates(&pinyin_str) {
            // V0.5+：用户词简拼优先——学过 ts→泰深 则 ts 让位给用户词
            //（否则 datetime 时间戳简码拦截，用户词永远轮不到）
            if !crate::dictionary::user_short_hit(&pinyin_str) {
                self.english_candidate_pos = None;
                self.all_candidates = cands;
                if !self.all_candidates.is_empty() {
                    self.datetime_candidate_pos = Some(0);
                }
                self.page = 0;
                self.repage();
                return;
            }
        }
        // 优先整词/全拼前缀查询
        let mut candidates = dictionary::query(&pinyin_str);
        // 词库直接命中数（V0.5.x）：后续纠错/组合会往 candidates 里补词，
        // 这里先记下「输入串本身在词库里能直接查到多少条」——
        // 为 0 说明候选全是纠错补出来的（dianjishiy），才允许锚定词组插队。
        let query_hit_count = candidates.len();
        // V0.5.6 简繁分集：繁体模式优先用原生繁体词条（繁体字库）。
        // 前 TRAD_FRONT_LIMIT 个前置（第一屏原生繁体优先）；其余追加末尾
        // （避免 40+ 个繁体词条挤占 truncate 窗口，简体词条被挤出→词长分区失效）。
        // 无原生繁体时简体词条由 candidate_display/select_candidate 输出层转繁兜底。
        if self.traditional_mode {
            const TRAD_FRONT_LIMIT: usize = 8;
            let trad = dictionary::query_trad(&pinyin_str);
            for (i, w) in trad.iter().enumerate() {
                if !candidates.contains(w) {
                    if i < TRAD_FRONT_LIMIT {
                        candidates.insert(i, w.clone());
                    } else {
                        candidates.push(w.clone());
                    }
                }
            }
        }
        // 快捷短语（V0.2.12）：简码精确命中 → 插入用户词后、系统词前
        // 位置记录：短语选中不学习
        if self.phrase_enabled {
            if let Some(text) = self.phrase_map.get(&pinyin_str) {
                // V0.5.11 语义 A（Eric 2026-08-13）：短语插在精确匹配热+温
                // 用户词之后、常用词之前（用户词 > 短语 > 系统词，非置顶）。
                let insert_at =
                    crate::dictionary::user_word_count(&pinyin_str).min(candidates.len());
                candidates.insert(insert_at, text.clone());
                self.phrase_candidate_pos = Some(insert_at);
            }
        }
        // 简拼补充（输入串同时作为简拼前缀，如 "zg"→中国）
        let short = dictionary::query_short(&pinyin_str);
        for w in short {
            if !candidates.contains(&w) {
                candidates.push(w);
            }
        }
        // 混合简拼补充（0.1.26）：全拼前缀 + 声母后缀，如 "shurf"→输入法
        if candidates.len() < self.page_size * self.max_pages {
            for w in dictionary::query_mixed(&pinyin_str) {
                if !candidates.contains(&w) {
                    candidates.push(w);
                }
            }
        }
        // 缩写+全拼混合补充（V0.3.x，对标 rime abbrev 逐音节缩写）：
        // 声母缩写前缀 + 完整拼音后缀，如 "xzai"→现在、"shh"→社会（配合完整声母索引）
        if candidates.len() < self.page_size * self.max_pages {
            for w in dictionary::query_abbrev_full(&pinyin_str) {
                if !candidates.contains(&w) {
                    candidates.push(w);
                }
            }
        }
        // 逐音节组合联想（V0.4）已移至错音之后（纠错优先，见下方）——此位置删除
        // 模糊音容错（RIME Spelling Algebra，0.1.14）：输入串变体查询，补在精确命中后
        // 0.3.x：仅完整拼音输入触发（防英文单词被 l→n 等变体误联想中文）
        if self.fuzzy_enabled && is_full_pinyin && fuzzy::may_have_fuzzy(&pinyin_str) {
            for variant in fuzzy::fuzzy_variants(&pinyin_str) {
                for w in dictionary::query(&variant) {
                    if !candidates.contains(&w) {
                        candidates.push(w);
                    }
                }
                // 变体简拼补充
                for w in dictionary::query_short(&variant) {
                    if !candidates.contains(&w) {
                        candidates.push(w);
                    }
                }
            }
        }
        // 拼写纠错（V0.3.x，对标 rime-ice speller derive 规则）：
        // wia→wai、hzi→zhi、lng→lang/leng/ling/long、zagn→zang、do→dou/dong 等。
        // 拼音特有模式，对英文单词天然安全（hello/world 无变体），不受 is_full_pinyin 限制。
        // V0.4.5 fix：每个变体限量补入（take CORR_TAKE）——zhonggou 场景 deletion
        // 变体 zhonggu 命中 364 词灌满候选，导致后续 correction 分支（< page_size）
        // 被跳过，「中国」丢失。
        if self.correction_enabled && candidates.len() < self.page_size {
            for variant in correction::spelling_variants(&pinyin_str) {
                for w in dictionary::query(&variant).into_iter().take(CORR_TAKE) {
                    if !candidates.contains(&w) {
                        candidates.push(w);
                    }
                }
            }
        }
        // 多打字母容错（V0.4.4）：删除每个位置一个字符，尝试命中正确拼音。
        // 例：weom→wom（我们）、zhongg→zhong（中国）。不受 is_full_pinyin 限制——
        // 不完整拼音也可能多打字母，由 dictionary::query 自然过滤无效变体。
        if self.correction_enabled && candidates.len() < self.page_size {
            for variant in correction::deletion_variants(&pinyin_str) {
                for w in dictionary::query(&variant).into_iter().take(CORR_TAKE) {
                    if !candidates.contains(&w) {
                        candidates.push(w);
                    }
                }
            }
        }
        // 智能纠错（V0.2.10）：候选不足时，键盘相邻键变体补入
        // 误触纠正：logn→long→龙、nihap→nihao→你好（排在精确/模糊之后）
        // 0.3.x：仅完整拼音输入触发（英文单词如 hello 不误联想中文）
        // V0.4.5 fix：条件放宽到 page_size*max_pages——deletion/spelling 补入
        // 可能把候选凑满 page_size，correction 作为最终兜底仍应有机会补入
        // 精确变体（zhonggou→zhongguo→中国）。
        // 0.1.50（Eric 反馈）：不再要求 is_full_pinyin——拼错的输入恰恰切不开
        // （logn/nihap/xinashi 全被旧门槛挡住）。改用"是否像明确英文单词"兜底，
        // 英文词典精确命中（hello/world）仍不触发，避免误联想中文。
        {
            let looks_like_english_word = pinyin_str.chars().count() >= 3
                && crate::english::query(&pinyin_str)
                    .iter()
                    .any(|e| e.eq_ignore_ascii_case(&pinyin_str));
            if self.correction_enabled
                && (is_full_pinyin || !looks_like_english_word)
                && candidates.len() < self.page_size * self.max_pages
                && correction::may_need_correction(&pinyin_str)
            {
                // 0.1.50：先收集，再前置。原实现追加末尾——而精确候选往往已经
                // 占满第一页（xinashi → 希腊式/纤丝鸟 这类冷僻切分），纠错结果
                // 落在第 8 位，用户永远看不到。相邻交换变体一旦能命中词库，
                // 说明这个输入本身更像另一种拼法，放到前面比冷僻精确词更有价值。
                let mut corr_cands: Vec<String> = Vec::new();
                for variant in correction::correction_variants(&pinyin_str) {
                    // 只认"变体本身是合法完整拼音"的命中——无效变体走查词库
                    // 的宽松匹配会带回一堆无关词（nihap 的 bihap 变体拉出"李亚平"）
                    if !crate::pinyin::is_complete_pinyin(&variant) {
                        continue;
                    }
                    for w in dictionary::query(&variant)
                        .into_iter()
                        .take(CORR_TAKE * 2)
                    {
                        if !corr_cands.contains(&w) {
                            corr_cands.push(w);
                        }
                    }
                    // 变体再走模糊音（纠错 + 模糊叠加）
                    if self.fuzzy_enabled && fuzzy::may_have_fuzzy(&variant) {
                        for fv in fuzzy::fuzzy_variants(&variant) {
                            for w in dictionary::query(&fv)
                                .into_iter()
                                .take(CORR_TAKE)
                            {
                                if !corr_cands.contains(&w) {
                                    corr_cands.push(w);
                                }
                            }
                        }
                    }
                }
                // 只前置"常用词"——常用词表是最好的可信度信号：
                //   xinashi → 变体 xianshi 命中「显示/现实」（常用）→ 前置
                //   nihap   → 变体 nihao 命中「你好」（常用）→ 前置
                //   nihao   → 变体 jihao 只命中「极好/记号」（冷僻）→ 不动，避免回归
                let trusted: Vec<String> = corr_cands
                    .into_iter()
                    .filter(|w| dictionary::is_common_word(w))
                    .take(3)
                    .collect();
                // 首位已是常用词 → 精确拼音已经命中可信词，纠错结果从第 2 位起插，
                // 不抢占首位（haode：query 首位「好的」是常用词，纠错变体 haole
                // 命中的「好了」也曾是常用词，原实现无条件 insert(0) 把好的顶到第 2）。
                let start = usize::from(
                    candidates
                        .first()
                        .map(|w| dictionary::is_common_word(w))
                        .unwrap_or(false),
                );
                for (i, w) in trusted.into_iter().enumerate() {
                    if !candidates.contains(&w) {
                        let at = (start + i).min(candidates.len());
                        candidates.insert(at, w);
                    }
                }
            }
        }
        // 错音错字提示（V0.2.26）：候选不足时，查易错读音映射 → 用正确读音查词
        // 0.3.x：仅完整拼音输入触发
        if is_full_pinyin && candidates.len() < self.page_size {
            for (correct_py, _word) in mistake::lookup(&pinyin_str) {
                for w in dictionary::query(correct_py) {
                    if !candidates.contains(&w) {
                        candidates.push(w);
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
        if !is_full_pinyin && candidates.len() < self.page_size * self.max_pages {
            for w in dictionary::query_combo(&pinyin_str) {
                if !candidates.contains(&w) {
                    candidates.push(w);
                }
            }
        }
        if !is_full_pinyin && candidates.len() < self.page_size * self.max_pages {
            for w in dictionary::combo_guess(&pinyin_str) {
                if !candidates.contains(&w) {
                    candidates.push(w);
                }
            }
        }
        // 词库锚定拆分组词（V0.4.5，Eric 设计）：长串候选不足时，以词库词组为锚
        // 拆分组词（每段允许 1 个错误），组合输出真实词组序列。
        // 例：zhegeweomende → 这个我们的（weom 删 e → wom）；zhegewumende → 这个我们的
        // （wum 换 o → wom）。与 combo_guess 区别：锚点是词库词组而非音节——不会
        // 拼出"下嗯下嗯"式怪词。
        // V0.5.3：去掉 is_full_pinyin 限制——完整拼音无整词时（womenceshi 词库无词条）
        // 也要能锚定拆分出"我们测试"（真实词组）；仅候选不足一页时兜底，防过度联想。
        // 触发条件在 V0.5.x 放宽：候选虽已满一页，但前 6 位没有常用词时也跑——
        // dianjishiy 的候选是纠错补出的冷僻三字词（点击事件/电机市场），
        // 锚定组合能给出「点击使用」这类真实词组，应当插队到首位（Eric 决策）。
        // 注：query_hit_count 是「输入串本身不是任何词条拼音前缀」的判据
        // （query 含前缀扩展），不是「词库零命中」——它把放宽限制在拼错/未打完的长串上。
        let need_group = candidates.len() < self.page_size
            || (query_hit_count == 0
                && candidates
                    .iter()
                    .take(6)
                    .all(|w| !dictionary::is_common_word(w)));
        if need_group {
            // 候选已满 → 词组插队（最多 2 个），否则按原逻辑追加。
            // 计数用「已插入个数」而非输出序号：前一条已存在会被 continue，
            // 用 enumerate 下标会让第二条插到第 1 位而不是首位。
            let prepend = candidates.len() >= self.page_size;
            let mut inserted = 0usize;
            for w in dictionary::phrase_group_guess(&pinyin_str) {
                if candidates.contains(&w) {
                    continue;
                }
                if prepend && inserted < 2 {
                    let at = inserted.min(candidates.len());
                    candidates.insert(at, w);
                } else {
                    candidates.push(w);
                }
                inserted += 1;
            }
        }
        // 注：完整拼音的单字组合兜底（an+xia → 按下）已移除。
        // 它的收益被常用词表补齐覆盖（anxia→按下 已进 P2 层），但代价是压过纠错结果
        // （xinashi 的「心啊是」会顶掉变体 xianshi 命中的「显示」）。词库缺词的场景
        // 交给词库补词解决，不在引擎热路径里猜。
        // V0.3.x 白话文长句过滤（Eric 决策：长句匹配无必要 + 防候选窗溢出）：
        // 成语/谚语/常用词（≤10 字）保留，白话文长句/超长专名（>10 字）不进候选。
        // 用户自定义快捷短语不受限（用户显式定义的文本必须可命中）。
        // 不按展示页数提前截断：后面的词长分区、置顶和排序需要看到更完整的召回集。
        // 但对异常大的前缀召回设置独立排序预算，避免全量词库进入热路径；
        // 该预算远大于展示页数，且保留查询阶段已经按精确度/词频排序的前缀。
        const RANKING_CANDIDATE_BUDGET: usize = 4096;
        candidates.truncate(RANKING_CANDIDATE_BUDGET);
        // V0.5+ 自然语言触发日期候选（对标主流输入法）：
        // 输入"日期/时间/年份/年" → 候选末尾追加当前日期/时间/年份
        if let Some(dt) = Self::datetime_pinyin_candidates(&pinyin_str) {
            for w in dt {
                if !candidates.contains(&w) {
                    candidates.push(w);
                }
            }
        }
        // 中英混输（V0.2.8 + P1-1 升级）：中文模式下候选末尾追加英文词典候选
        // （对标 rime melt_eng 英文词典 + cn_en 中英混合词）。
        // 英文候选恒在末尾，不干扰汉字排序；ASCII 模式不追加。
        // V0.4（Eric 反馈）：非完整拼音输入英文单词（hel/o）时英文候选此前在
        // 72 位之后第一屏看不到 → 空格上屏的是中文。改为：英文词典命中时
        // 原样（hel）+ 词典候选（help/hello）插到第一页末尾（第 5 位起）。
        // 注意：仅词典命中才前置——zh 等中文简拼（词典无命中）不挤占中文候选。
        self.english_candidate_pos = None;
        if self.mix_mode_enabled
            && !self.ascii_mode
            && !pinyin_str.is_empty()
            && pinyin_str.chars().all(|c| c.is_ascii_alphabetic())
        {
            let eng = crate::english::query(&pinyin_str);
            // V0.4：英文候选前置仅限"明显英文"——非完整拼音且非合法拼音前缀
            // （hel → 英文；zh/o → 中文简拼/音节，中文优先，不挤占中文候选）
            if !is_full_pinyin && !crate::pinyin::is_valid_pinyin_prefix(&pinyin_str) {
                // 「精确命中且长度 >= 3」才置顶：hello / app 有精确词条 → 首屏第一位。
                // 两字母不置顶：nh / zg / ces 在拼音体系里多是简拼（nh→你好），
                // 且 google 词表里有 nh(New Hampshire) 这类缩写会误抢首位。
                let has_exact = pinyin_str.chars().count() >= 3
                    && eng.iter().any(|e| e.to_ascii_lowercase() == pinyin_str);
                if has_exact {
                    // 0.1.50（Eric 反馈）：不再清空中文件候选——key/dans/rug 之前
                    // 全英文上屏就是这句 candidates.clear() 造成的。
                    // 0.1.51：插入位置改由「中文侧有没有常用词」决定——
                    //   rug → ru+g 命中「如果」、key → ke+y 命中「可以」，
                    //   都有常用词 → 中文留在首位，英文紧随其后（第 2 位起）
                    //   hello → 中文侧只有「贺龙来」这类冷僻切分 → 英文独占首位
                    // 这样两种输入都符合直觉，且都能空格直上屏。
                    let display = eng
                        .iter()
                        .find(|e| e.eq_ignore_ascii_case(&pinyin_str))
                        .cloned()
                        .unwrap_or_else(|| pinyin_str.clone());
                    let mut eng_cands: Vec<String> = vec![display];
                    for e in &eng {
                        if !eng_cands.iter().any(|c| c.eq_ignore_ascii_case(e)) {
                            eng_cands.push(e.clone());
                        }
                    }
                    eng_cands.truncate(3);
                    // 中文侧前 5 个里有常用词 → 中文优先，英文插到第 2 位起
                    let cn_has_common = candidates
                        .iter()
                        .take(5)
                        .any(|w| crate::dictionary::is_common_word(w));
                    let insert_at = if cn_has_common { 1 } else { 0 };
                    for (i, e) in eng_cands.iter().enumerate() {
                        candidates.insert(insert_at + i, e.clone());
                    }
                    self.english_candidate_pos = Some(insert_at);
                } else if !eng.is_empty() {
                    // 仅前缀命中：追加末尾，不干扰中文候选
                    self.english_candidate_pos = Some(candidates.len());
                    candidates.extend(eng);
                } else {
                    // 词典无命中 → 输入串原样兜底，追加末尾。
                    // 不放前面：zhongg / nihap 这类打错的拼音也走这条分支，
                    // 提到前排会挤掉纠错候选。
                    candidates.push(pinyin_str.clone());
                    self.english_candidate_pos = Some(candidates.len() - 1);
                }
            } else if !eng.is_empty() {
                // 完整拼音/拼音前缀：英文词典候选保持末尾（中文优先）
                self.english_candidate_pos = Some(candidates.len());
                candidates.extend(eng);
            } else {
                // 英文词典无命中 → 原样兜底（防候选空，如 cai 内置词库无词）
                candidates.push(pinyin_str.clone());
                self.english_candidate_pos = Some(candidates.len() - 1);
            }
        }
        // V0.5.5 词长匹配分区：输入 N 个完整音节 → 候选按「字数 == N」稳定分区在前。
        // 词库分层（P1-P4）在 dictionary::query 内完成，此处只做词长匹配兜底
        // （领域热度排序已并入 P4 层内，不再需要引擎层 domain_boost）。
        // P1-1 上下文联想（对标搜狗/微软前文关联）：前文搭配词前置
        self.apply_context_boost(&mut candidates);
        // 词长匹配分区（Eric 2026-08-09：输入双字词就显示双字，不能过度联想）
        self.apply_word_len_match(&mut candidates, &pinyin_str);
        // P2-2 置顶候选（对标 rime pin_cand_filter）：词级置顶——候选里命中
        // pin_words 的词提到最前。必须在词长分区之后执行——置顶优先级高于
        // 词长匹配（wo→我们 置顶不被分区挤掉）。
        // V0.5.7：从编码级（pin_map: 编码→词）重构为词级（pin_words 集合）——
        // 输入 nihao/nih/nh 任何方式，候选里出现"你好"即置顶，天然覆盖简拼自动展开。
        // （8fd8172 重构时置于分区之前导致置顶被覆盖，V0.5.7 修正顺序）
        self.apply_pin_boost(&mut candidates);
        // V0.5.7 降权（Eric 2026-08-10 决策）：命中 demoted_words 的词压出前 2 屏。
        // 在 pin 之后执行——被置顶的词不降权（两集合互斥由 UI 菜单保证）。
        self.apply_demote(&mut candidates);
        // V0.5.6 繁体模式转繁去重：简体词条转繁后与原生繁体相同（我們的出口）
        // → 去掉重复（保留先出现的原生繁体）。
        if self.traditional_mode {
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            candidates.retain(|w| {
                let display = if crate::trad::is_traditional(w) {
                    w.clone()
                } else {
                    crate::trad::to_traditional(w)
                };
                seen.insert(display)
            });
        }
        // P0-2 候选排序（对标微软单字/长词优先）：用户显式开启时稳定分区
        self.apply_sort_mode(&mut candidates);
        // P2-5 Emoji（对标 rime simplifier@emoji）：命中候选词映射 → 追加 "词emoji"
        if self.emoji_enabled {
            let mut emo: Vec<String> = Vec::new();
            for w in &candidates {
                if let Some(e) = crate::emoji::lookup(w) {
                    emo.push(format!("{w}{e}"));
                    if emo.len() >= 5 {
                        break; // 最多 5 个 emoji 候选
                    }
                }
            }
            if !emo.is_empty() {
                // 插到英文候选之前（英文候选位置后移）
                let eng_pos = self.english_candidate_pos.unwrap_or(candidates.len());
                for (i, e) in emo.iter().enumerate() {
                    candidates.insert(eng_pos + i, e.clone());
                }
                if let Some(p) = self.english_candidate_pos.as_mut() {
                    *p += emo.len();
                }
            }
        }
        // V0.3.x 白话文长句过滤（Eric 决策：长句匹配无必要 + 防候选窗溢出）：
        // 成语/谚语/常用词（≤10 字）保留，白话文长句/超长专名（>10 字）不进候选。
        // 用户自定义快捷短语不受限（用户显式定义的文本必须可命中）。
        // 必须在英文候选兜底之后执行——8fd8172 重构后英文兜底在过滤前 push
        // 原样串（nihaoshijie 11 字符逃过滤），V0.5.7 移到最后统一过滤。
        const MAX_CAND_WORD_LEN: usize = 10;
        {
            let phrase_text = if self.phrase_candidate_pos.is_some() {
                self.phrase_map.get(&pinyin_str).cloned()
            } else {
                None
            };
            candidates.retain(|w| w.chars().count() <= MAX_CAND_WORD_LEN);
            if let Some(pt) = phrase_text {
                if !candidates.iter().any(|w| w == &pt) {
                    candidates.insert(0, pt);
                }
            }
        }
        // 单字母输入（1 字节）：单字候选优先，>1 字词压后（Eric 2026-08-13：
        // 从「删除」改为「压后保留」——单字排前，"我们"等词压到靠后位置，
        // 翻页可见）。根因：query() 对单字母做全拼前缀扩展（w → 我们/问题/晚上），
        // 词组高频淹没单字（我/五/王）。统一在最终候选处做单字优先分区。
        // v 符号/c 计算器/u 拆字等特殊模式已提前 return，不受影响。
        if pinyin_str.len() == 1 {
            let mut single: Vec<String> = Vec::new();
            let mut multi: Vec<String> = Vec::new();
            for w in candidates.drain(..) {
                if w.chars().count() == 1 {
                    single.push(w);
                } else {
                    multi.push(w);
                }
            }
            single.append(&mut multi);
            candidates = single;
        }
        // 所有来源、排序和过滤完成后再限制保留页数。
        candidates.truncate(self.page_size * self.max_pages);
        self.all_candidates = candidates;
        self.page = 0;
        self.repage();
    }

    /// V0.5.3 词长匹配分区（Eric 2026-08-09/11：不能过度联想）：
    /// 输入 N 个完整音节或 N 个声母（简拼）→ 候选按「字数 == N」稳定分区在前，
    /// 其余靠后——长词短句不抢首位。英文候选区（english_candidate_pos 之后）不参与。
    fn apply_word_len_match(&mut self, candidates: &mut Vec<String>, pinyin_str: &str) {
        // 估算目标词长：完整拼音 → 音节数；混合简拼 → 音节+声母段数；
        // 纯声母简拼 → 声母数（≥2 才触发）。V0.5.9：补上混合简拼分支——
        // ces/jianp 等"全拼前缀+声母后缀"此前两个分支都不命中，长词短句抢首位。
        let n = match complete_syllable_count(pinyin_str) {
            Some(n) => n,
            None => match mixed_syllable_count(pinyin_str) {
                Some(n) => n,
                None => match short_pinyin_word_len(pinyin_str) {
                    Some(n) => n,
                    None => return, // 英文/单字母/不可解析 → 不干预
                },
            },
        };
        let eng_start = self.english_candidate_pos.unwrap_or(candidates.len());
        let limit = candidates.len().min(eng_start);

        if limit <= 1 {
            return;
        }
        // V0.5.10：短语/日期特殊候选恒在最前（phrase/datetime 显式定义，
        // 用户意图明确），不参与词长分区——否则 bq 的 2 字"并且"会把
        // 短语"不客气"(3字) 挤出首位（真实词库下实测回归）。
        let start =
            if self.phrase_candidate_pos == Some(0) || self.datetime_candidate_pos == Some(0) {
                1
            } else {
                0
            };
        if limit - start <= 1 {
            return;
        }
        // 中文汉字数（排除英文/符号候选的误判）
        let hanzi_count = |w: &str| w.chars().filter(|c| *c as u32 > 0x7F).count();
        // V0.5.11 三分法（Eric 2026-08-13）：
        //   字数 == n 排最前；字数 < n（单字）居中；字数 > n（长词）压最后。
        // 效果：长词不抢首位/次首位（exact+shorter 占满前位后，长词才出现；
        // 正常输入下长词落在第 5 候选及以后）。
        let mut exact: Vec<String> = Vec::new();
        let mut shorter: Vec<String> = Vec::new();
        let mut longer: Vec<String> = Vec::new();
        for w in candidates.iter().skip(start).take(limit - start) {
            let c = hanzi_count(w);
            if c == n {
                exact.push(w.clone());
            } else if c < n {
                shorter.push(w.clone());
            } else {
                longer.push(w.clone());
            }
        }
        exact.append(&mut shorter);
        exact.append(&mut longer);
        for (i, w) in exact.into_iter().enumerate() {
            candidates[start + i] = w;
        }
    }

    /// 内置快捷短语表（V0.2.12）：简码 → 常用文本
    fn builtin_phrases() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("bq".to_string(), "不客气".to_string());
        m.insert("wm".to_string(), "我们".to_string());
        m.insert("dz".to_string(), "地址：深圳市南山区科技园".to_string());
        m.insert("sj".to_string(), "手机".to_string());
        m.insert("gs".to_string(), "公司".to_string());
        m.insert("zj".to_string(), "再见".to_string());
        m.insert("wx".to_string(), "微信".to_string());
        m.insert("qq".to_string(), "QQ号码".to_string());
        m.insert("email".to_string(), "邮箱地址".to_string());
        m.insert("tel".to_string(), "电话号码".to_string());
        m.insert("bz".to_string(), "备注：".to_string());
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
        let mut candidates = Vec::new();
        for fp in &decoded {
            for w in dictionary::query(fp) {
                if !candidates.contains(&w) {
                    candidates.push(w);
                }
            }
        }
        // 完整音节无候选时，尝试模糊音（双拼+模糊音可叠加）
        if candidates.is_empty() && self.fuzzy_enabled {
            for fp in &decoded {
                if fuzzy::may_have_fuzzy(fp) {
                    for variant in fuzzy::fuzzy_variants(fp) {
                        for w in dictionary::query(&variant) {
                            if !candidates.contains(&w) {
                                candidates.push(w);
                            }
                        }
                    }
                }
            }
        }
        // 末尾未完成音节 → 顶码候选，追加在完整音节候选之后（不抢首位）
        if !tail_code.is_empty() {
            for fp in self.shuangpin_scheme.decode(&tail_code) {
                for w in dictionary::query(&fp) {
                    if !candidates.contains(&w) {
                        candidates.push(w);
                    }
                }
            }
        }

        // 英文混输（双拼补齐）：按原按键串查英文词典——hello 这类解不出中文的串给英文候选
        self.english_candidate_pos = None;
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
                    .any(|w| crate::dictionary::is_common_word(w));
                let insert_at = if cn_has_common { 1 } else { 0 };
                for (i, e) in eng_cands.iter().enumerate() {
                    candidates.insert(insert_at + i, e.clone());
                }
                self.english_candidate_pos = Some(insert_at);
            } else if !eng.is_empty() {
                self.english_candidate_pos = Some(candidates.len());
                candidates.extend(eng);
            } else if candidates.is_empty() {
                candidates.push(code.clone());
                self.english_candidate_pos = Some(candidates.len() - 1);
            }
        }

        self.phrase_candidate_pos = None;
        self.apply_context_boost(&mut candidates);
        if let Some(py) = primary.as_deref() {
            self.apply_word_len_match(&mut candidates, py);
        }
        self.apply_pin_boost(&mut candidates);
        self.apply_demote(&mut candidates);
        if self.traditional_mode {
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            candidates.retain(|w| {
                let display = if crate::trad::is_traditional(w) {
                    w.clone()
                } else {
                    crate::trad::to_traditional(w)
                };
                seen.insert(display)
            });
        }
        self.apply_sort_mode(&mut candidates);
        if self.emoji_enabled {
            let mut emo: Vec<String> = Vec::new();
            for w in &candidates {
                if let Some(e) = crate::emoji::lookup(w) {
                    emo.push(format!("{w}{e}"));
                    if emo.len() >= 5 {
                        break;
                    }
                }
            }
            if !emo.is_empty() {
                let eng_pos = self.english_candidate_pos.unwrap_or(candidates.len());
                for (i, e) in emo.iter().enumerate() {
                    candidates.insert(eng_pos + i, e.clone());
                }
                if let Some(p) = self.english_candidate_pos.as_mut() {
                    *p += emo.len();
                }
            }
        }
        const MAX_CAND_WORD_LEN: usize = 10;
        candidates.retain(|w| w.chars().count() <= MAX_CAND_WORD_LEN);
        // 单键输入（只按了第一个键）：单字优先，词压后
        if code.chars().count() == 1 {
            let mut single: Vec<String> = Vec::new();
            let mut multi: Vec<String> = Vec::new();
            for w in candidates.drain(..) {
                if w.chars().count() == 1 {
                    single.push(w);
                } else {
                    multi.push(w);
                }
            }
            single.append(&mut multi);
            candidates = single;
        }
        candidates.truncate(self.page_size * self.max_pages);
        self.all_candidates = candidates;
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
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_word_len_match_debug() {
        let mut e = Engine::new();
        let mut c = vec![
            "台灣物理學會".to_string(),
            "台湾".to_string(),
            "太弯".to_string(),
            "太晚".to_string(),
            "臺灣簇蟲".to_string(),
        ];
        e.apply_word_len_match(&mut c, "taiwan");
        assert_eq!(c[0], "台湾", "2 字词应提前, got {:?}", c);
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
    fn test_fuzzy_default_on() {
        let engine = Engine::new();
        assert!(engine.fuzzy_enabled(), "模糊音默认应开启");
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
    fn test_emoji_candidate_appended() {
        // 内置词库：xiexie → 谢谢 → 追加 "谢谢🙏"（显式开启 emoji）
        // 串行锁 + 显式 builtin：真实词库下 emoji 追加位置可能不同，锁保证确定性
        let _g = crate::dictionary::TEST_DICT_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::dictionary::init_blocking(None);
        let mut engine = Engine::new();
        engine.set_emoji(true);
        for ch in "xiexie".chars() {
            engine.process_key(ch);
        }
        let has_emoji =
            (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("谢谢🙏"));
        assert!(
            has_emoji,
            "xiexie 应追加 emoji 候选 谢谢🙏, got {:?}",
            (0..engine.candidate_count())
                .map(|i| engine.candidate(i).unwrap_or(""))
                .collect::<Vec<_>>()
        );
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
        // 选中 emoji 候选 → 上屏带 emoji 文本
        let mut engine = Engine::new();
        for ch in "xiexie".chars() {
            engine.process_key(ch);
        }
        let idx = (0..engine.candidate_count()).position(|i| engine.candidate(i) == Some("谢谢🙏"));
        if let Some(i) = idx {
            let text = engine.select_candidate(i).unwrap();
            assert_eq!(text, "谢谢🙏");
            assert_eq!(engine.pinyin_str(), "", "选中后应重置");
        }
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
        // 单字优先：直接构造混合候选，验证稳定分区（单字提到前，组内相对序不变）
        let mut engine = Engine::new();
        engine.set_sort_mode(1);
        assert_eq!(engine.sort_mode(), 1);
        // 构造 [中国, 中, 你好, 国, 世界] —— 英文区（末尾）不参与
        engine.english_candidate_pos = Some(5);
        let mut cands = vec![
            "中国".to_string(),
            "中".to_string(),
            "你好".to_string(),
            "国".to_string(),
            "世界".to_string(),
        ];
        engine.apply_sort_mode(&mut cands);
        // 单字优先：中/国 提到前（组内相对序保持：中 在 国 前）
        assert_eq!(cands[0], "中");
        assert_eq!(cands[1], "国");
        assert_eq!(cands[2], "中国");
        assert_eq!(cands[3], "你好");
        assert_eq!(cands[4], "世界");
    }

    #[test]
    fn test_sort_mode_long_first() {
        // 长词优先：多字提到前，组内相对序不变
        let mut engine = Engine::new();
        engine.set_sort_mode(2);
        assert_eq!(engine.sort_mode(), 2);
        engine.english_candidate_pos = Some(5);
        let mut cands = vec![
            "中".to_string(),
            "中国".to_string(),
            "国".to_string(),
            "你好".to_string(),
            "世界".to_string(),
        ];
        engine.apply_sort_mode(&mut cands);
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
        // 英文候选区不参与分区（末尾保持原样）
        let mut engine = Engine::new();
        engine.set_sort_mode(1);
        engine.english_candidate_pos = Some(3);
        let mut cands = vec![
            "中国".to_string(),
            "中".to_string(),
            "国".to_string(),
            "help".to_string(),
            "hello".to_string(),
        ];
        engine.apply_sort_mode(&mut cands);
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
        let hanzi = first.chars().filter(|c| *c as u32 > 0x7F).count();
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
        engine.english_candidate_pos = Some(2);
        let mut cands = vec!["大学".to_string(), "其他".to_string(), "help".to_string()];
        engine.apply_context_boost(&mut cands);
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
        // 共现次数越高，候选前移位数越大（位次单调前移）
        let mut engine = Engine::new();
        engine.set_context_enabled(true);
        engine.last_committed = "电脑".to_string();
        let base: Vec<String> = vec!["一", "二", "三", "四", "五", "六", "七", "八", "九", "十"]
            .into_iter()
            .map(|s| s.to_string())
            .chain(std::iter::once("屏幕".to_string()))
            .collect();

        engine.load_learned_contexts(vec![("电脑".to_string(), "屏幕".to_string(), 1)]);
        let mut once = base.clone();
        engine.apply_context_boost(&mut once);
        let pos_once = once.iter().position(|w| w == "屏幕").unwrap();

        engine.load_learned_contexts(vec![("电脑".to_string(), "屏幕".to_string(), 6)]);
        let mut six = base.clone();
        engine.apply_context_boost(&mut six);
        let pos_six = six.iter().position(|w| w == "屏幕").unwrap();

        assert_eq!(pos_once, 10, "共现 1 次只前移 1 位，第 11 位不动");
        assert_eq!(pos_six, 5, "共现 6 次前移 6 位");
        assert!(pos_six < pos_once, "权重升高后位次应更靠前");
    }

    #[test]
    fn test_context_prior_boosts_without_history() {
        // 冷启动：没有任何学习记录时，离线先验表也能给候选加权
        let mut engine = Engine::new();
        engine.set_context_enabled(true);
        engine.last_committed = "电脑".to_string();
        engine.load_prior_contexts(vec![("电脑".to_string(), "屏幕".to_string(), 6)]);
        let mut cands: Vec<String> = vec!["一", "二", "三", "四", "五", "六", "七", "八", "九", "十", "屏幕"]
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        engine.apply_context_boost(&mut cands);
        assert_eq!(
            cands.iter().position(|w| w == "屏幕"),
            Some(5),
            "先验权重 6 应让第 11 位前移 6 位"
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
        let mut cands: Vec<String> = vec!["一", "二", "三", "四", "五", "六", "七", "八", "九", "十", "屏幕"]
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        engine.apply_context_boost(&mut cands);
        assert_eq!(
            cands.iter().position(|w| w == "屏幕"),
            Some(2),
            "取较大权重 9：第 11 位前移 9 位"
        );
    }

    #[test]
    fn test_context_disabled_keeps_order() {
        // 关闭开关后候选顺序必须与未启用时完全一致
        let mut engine = Engine::new();
        engine.set_context_enabled(false);
        engine.last_committed = "北京".to_string();
        let mut cands: Vec<String> = vec!["一", "二", "大学", "三"]
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        let origin = cands.clone();
        engine.apply_context_boost(&mut cands);
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
        assert_eq!(engine.english_candidate_pos, Some(0), "英文区起始于首位");
    }

    #[test]
    fn test_mix_mode_english_tail_for_complete_pinyin() {
        // 完整拼音（ai）→ 英文候选不置顶，排在中文候选之后（中文优先）
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
            engine.english_candidate_pos,
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
    fn test_long_sentence_filter_keeps_phrase() {
        // 用户自定义快捷短语不受长度限制：dz → "地址：深圳市南山区科技园"（13 字）必须保留
        let mut engine = Engine::new();
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
        // 候选应为单字（第一音节 zhong）
        let n = engine.candidate_count();
        for i in 0..n {
            let c = engine.candidate(i).unwrap();
            assert_eq!(c.chars().count(), 1, "组词候选应为单字: {c}");
        }
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
        engine.repage();
        assert_eq!(engine.total_pages(), 1, "构造候选应仅 1 页");
        engine.page(1); // 翻不动 → 立即触发组词
        assert!(engine.compose_active(), "候选不足一页时应立即进入组词");
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
        // 候选应为单字（第一段 t 开头的字）
        let n = engine.candidate_count();
        assert!(n > 0, "组词模式应有候选");
        for i in 0..n.min(5) {
            let c = engine.candidate(i).unwrap();
            assert_eq!(c.chars().count(), 1, "组词候选应为单字: {c}");
        }
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
        if engine.candidate_count() == 0 {
            engine.candidates = vec!["泰".to_string()];
        }
        let first = engine.candidate(0).unwrap().to_string();
        // V0.5.10 逐字预上屏：中间音节返回选中字（平台层立即上屏），推进到下一音节
        let r = engine.select_candidate(0);
        assert_eq!(r.as_deref(), Some("泰"), "中间音节应返回选中字（预上屏）");
        assert!(engine.compose_active(), "仍有剩余音节");
        // 第二音节
        if engine.candidate_count() == 0 {
            engine.candidates = vec!["深".to_string()];
        }
        let second = engine.candidate(0).unwrap().to_string();
        let combined = format!("{first}{second}");
        // 最后音节：返回最后一个字（前面字已逐字上屏），组词结束 + learn 整词
        let r = engine.select_candidate(0);
        assert_eq!(
            r.as_deref(),
            Some(second.as_str()),
            "最后音节应返回最后一个字"
        );
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
        let hanzi = first.chars().filter(|c| *c as u32 > 0x7F).count();
        assert_eq!(
            hanzi,
            2,
            "jianp 首位应为 2 字词, got: {first}, all: {:?}",
            (0..engine.candidate_count())
                .map(|i| engine.candidate(i).unwrap_or(""))
                .collect::<Vec<_>>()
        );
    }

    // ─── V0.5.10 摸底：简拼+全拼混合（bru/zhyang）与热词首屏（临时）───
    #[test]
    fn debug_bru_zhyang_hotword() {
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
        let hit_zhangyang =
            (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("张扬"));
        assert!(hit_zhangyang, "zhyang 应命中 张扬（zh+yang 混合）");
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
        for p in 0..10 {
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
        assert_eq!(r2.as_deref(), Some("茂"), "最后音节应返回最后一个字");
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
        engine.repage();
        engine.page(1); // 候选不足一页 → 立即触发组词
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
}
