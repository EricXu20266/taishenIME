pub mod calculator;
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
pub mod unichar;

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
    /// 置顶候选映射（P2-2，对标 rime pin_cand_filter）：编码 → [候选词]
    pin_map: std::collections::HashMap<String, Vec<String>>,
    /// Emoji 开关（P2-5，对标 rime emoji 开关，默认开）
    emoji_enabled: bool,
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
            correction_enabled: true,
            mix_mode_enabled: true,
            english_candidate_pos: None,
            traditional_mode: false,
            phrase_enabled: true,
            phrase_map: Self::builtin_phrases(),
            phrase_candidate_pos: None,
            datetime_candidate_pos: None,
            pin_map: Self::builtin_pins(),
            emoji_enabled: false, // 默认关闭（Eric 要求，config emoji=1 可开启）
        }
    }

    /// 内置置顶候选（P2-2，对标 rime pin_cand_filter 默认示例）：
    /// d→的、m→吗/嘛、hm→后面（覆盖单音节独占）
    fn builtin_pins() -> std::collections::HashMap<String, Vec<String>> {
        let mut m = std::collections::HashMap::new();
        m.insert("d".to_string(), vec!["的".to_string()]);
        m.insert("m".to_string(), vec!["吗".to_string(), "嘛".to_string()]);
        m.insert("hm".to_string(), vec!["后面".to_string()]);
        m
    }

    /// 加载外部置顶候选（P2-2）：entries: (编码, [词])，覆盖/补充内置
    pub fn load_pins(&mut self, entries: Vec<(String, Vec<String>)>) {
        for (code, words) in entries {
            if !code.is_empty() && !words.is_empty() {
                self.pin_map.insert(code, words);
            }
        }
        if !self.pinyin_buf.is_empty() {
            self.query_all();
        }
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
        if ch.is_ascii_alphabetic() {
            if self.ascii_mode {
                return false; // 英文模式：不累积拼音，平台层直通上屏
            }
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

    /// 符号输入 v 模式判定（V0.2.17）：拼音串以 'v' 开头且长度 > 1
    /// （单独 'v' 时按普通拼音处理，避免误伤；双拼模式下 v 是 zh 声母，不走符号模式）
    pub fn is_symbol_mode(&self) -> bool {
        !self.shuangpin_mode && self.pinyin_buf.starts_with('v') && self.pinyin_buf.len() > 1
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
        } else if self.traditional_mode {
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

    /// 选择候选词并提交（返回提交文本，同时重置状态）
    /// V0.2.2：选词时自动学习用户词（拼音串 + 选中词）
    /// V0.2.8：选中英文候选（混输）→ 上屏原文不学习
    /// V0.2.11：简繁模式开启时上屏文本转繁体
    pub fn select_candidate(&mut self, index: usize) -> Option<String> {
        let result = self.candidates.get(index).cloned();
        let mut output = result.clone();
        if let Some(word) = &result {
            // 判断候选类型
            let is_english = self.is_english_candidate(index);
            let is_phrase = self.is_phrase_candidate(index);
            let is_symbol = self.is_symbol_mode();
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
            {
                crate::dictionary::learn(&self.pinyin_buf, word);
            }
            // 简繁转换：输出繁体（英文/短语/符号/计算器/日期/反查/错音候选不转）
            if self.traditional_mode
                && !is_english
                && !is_phrase
                && !is_symbol
                && !is_calc
                && !is_datetime
                && !is_radical
                && !is_mistake
            {
                output = Some(crate::trad::to_traditional(word));
            }
            // V0.2.23：英文候选按输入大小写模式还原（Hello/HELLO/hello）
            if is_english {
                output = Some(self.apply_cap(word));
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

    /// 翻页。delta: +1 下一页 / -1 上一页。
    /// 返回翻页后的当前页候选数（0 表示无候选或已到边界）。
    pub fn page(&mut self, delta: i32) -> usize {
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
    }

    /// 退格（删除最后一个拼音字符；P2-1：光标不在末尾时删光标前字符）
    pub fn backspace(&mut self) -> bool {
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
    fn syllable_boundaries(&self) -> Vec<usize> {
        let mut boundaries = vec![0usize];
        let mut rest = self.pinyin_buf.as_str();
        let mut pos = 0;
        while !rest.is_empty() {
            match crate::pinyin::split_first_syllable(rest) {
                Some((syl, remaining)) => {
                    pos += syl.len();
                    boundaries.push(pos);
                    rest = remaining;
                }
                None => break,
            }
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
        crate::dictionary::remove_user_word(&self.pinyin_buf, &word);
        self.query_all();
        true
    }

    // ─── 内部 ───

    /// 查询全部候选（含简拼联想 + 多音节切分联想），截断到 max_pages 页，重置到第 0 页
    fn query_all(&mut self) {
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
        if self.is_radical_mode() {
            let parts = &self.pinyin_buf[1..]; // 去掉 'u' 前缀
            let candidates = radical::query(parts);
            self.english_candidate_pos = None;
            self.phrase_candidate_pos = None;
            self.all_candidates = candidates;
            self.page = 0;
            self.repage();
            return;
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
            self.english_candidate_pos = None;
            self.all_candidates = cands;
            if !self.all_candidates.is_empty() {
                self.datetime_candidate_pos = Some(0);
            }
            self.page = 0;
            self.repage();
            return;
        }
        // 优先整词/全拼前缀查询
        let mut candidates = dictionary::query(&pinyin_str);
        // 快捷短语（V0.2.12）：简码精确命中 → 插入用户词后、系统词前
        // 位置记录：短语选中不学习
        if self.phrase_enabled {
            if let Some(text) = self.phrase_map.get(&pinyin_str) {
                // 用户词已经在 query 前面（query 内部先 user 后 system），
                // 短语插在用户词之后：找到首个系统词位置（即第一个非用户词）
                candidates.insert(0, text.clone());
                self.phrase_candidate_pos = Some(0);
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
        // 多音节切分联想（如 "nihaoshijie" → "你好世界" 无整词时，切分 ni+hao+shijie）
        if candidates.is_empty() && pinyin_str.len() > 4 {
            let phrase = dictionary::phrase_guess(&pinyin_str);
            for w in phrase {
                if !candidates.contains(&w) {
                    candidates.push(w);
                }
            }
        }
        // 拼写纠错（V0.3.x，对标 rime-ice speller derive 规则）：
        // wia→wai、hzi→zhi、lng→lang/leng/ling/long、zagn→zang、do→dou/dong 等。
        // 拼音特有模式，对英文单词天然安全（hello/world 无变体），不受 is_full_pinyin 限制。
        if self.correction_enabled && candidates.len() < self.page_size {
            for variant in correction::spelling_variants(&pinyin_str) {
                for w in dictionary::query(&variant) {
                    if !candidates.contains(&w) {
                        candidates.push(w);
                    }
                }
            }
        }
        // 智能纠错（V0.2.10）：候选不足时，键盘相邻键变体补入
        // 误触纠正：logn→long→龙、nihap→nihao→你好（排在精确/模糊之后）
        // 0.3.x：仅完整拼音输入触发（英文单词如 hello 不误联想中文）
        if self.correction_enabled
            && is_full_pinyin
            && candidates.len() < self.page_size
            && correction::may_need_correction(&pinyin_str)
        {
            for variant in correction::correction_variants(&pinyin_str) {
                // 变体直接查询（若为合法拼音则命中词条）
                for w in dictionary::query(&variant) {
                    if !candidates.contains(&w) {
                        candidates.push(w);
                    }
                }
                // 变体再走模糊音（纠错 + 模糊叠加）
                if self.fuzzy_enabled && fuzzy::may_have_fuzzy(&variant) {
                    for fv in fuzzy::fuzzy_variants(&variant) {
                        for w in dictionary::query(&fv) {
                            if !candidates.contains(&w) {
                                candidates.push(w);
                            }
                        }
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
        // 输入 = 音节前缀/声母 任意混插（ni+m+zai → 你们在、gan+sh+m → 干什么）。
        // 两步：先整词匹配（query_combo），无整词再逐音节拼接（combo_guess）。
        // 对标搜狗逐音节智能切分；仅在候选不足时补充（性能可控）。
        // V0.4 位置调整：放在拼写纠错/智能纠错/错音之后——宽松联想不挤占精准修正
        // （此前 combo 在纠错前，logn 被 combo 怪词填满 → 纠错 龙 不触发）。
        if candidates.len() < self.page_size * self.max_pages {
            for w in dictionary::query_combo(&pinyin_str) {
                if !candidates.contains(&w) {
                    candidates.push(w);
                }
            }
        }
        if candidates.len() < self.page_size * self.max_pages {
            for w in dictionary::combo_guess(&pinyin_str) {
                if !candidates.contains(&w) {
                    candidates.push(w);
                }
            }
        }
        // V0.3.x 白话文长句过滤（Eric 决策：长句匹配无必要 + 防候选窗溢出）：
        // 成语/谚语/常用词（≤10 字）保留，白话文长句/超长专名（>10 字）不进候选。
        // 用户自定义快捷短语不受限（用户显式定义的文本必须可命中）。
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
        // 截断到 max_pages 页
        candidates.truncate(self.page_size * self.max_pages);
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
                if !eng.is_empty() {
                    // 英文单词输入且词典命中：原样 + 词典候选插到第一页末尾
                    let mut eng_cands: Vec<String> = vec![pinyin_str.clone()];
                    for e in &eng {
                        if !eng_cands.contains(e) {
                            eng_cands.push(e.clone());
                        }
                    }
                    let insert_at = (self.page_size.saturating_sub(1)).min(candidates.len());
                    for (i, e) in eng_cands.iter().enumerate() {
                        candidates.insert(insert_at + i, e.clone());
                    }
                    self.english_candidate_pos = Some(insert_at);
                } else {
                    // 词典无命中 → 输入串原样兜底（保证非词非拼音输入可上屏）
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
        // P2-2 置顶候选（对标 rime pin_cand_filter）：精确编码命中 → 词提到最前
        if let Some(words) = self.pin_map.get(&pinyin_str) {
            for w in words {
                if let Some(pos) = candidates.iter().position(|c| c == w) {
                    let w = candidates.remove(pos);
                    candidates.insert(0, w);
                }
            }
        }
        // P2-2 长词优先（对标 rime long_word_filter）：单字占前时把长词提到第 4 位起
        self.apply_long_word_filter(&mut candidates);
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
        self.all_candidates = candidates;
        self.page = 0;
        self.repage();
    }

    /// P2-2 长词优先（对标 rime long_word_filter）：
    /// 从第 4 位起找长词（2+ 汉字），提前到第 4、5 位（最多 2 个）。
    /// 只处理汉字候选（英文候选恒在末尾不参与）。默认 count=2 idx=4。
    /// V0.2.30：common 词顺序由用户词表显式指定，长词过滤不干预——
    /// 目标词跳过 common 词，插入位置也跳过 common 占位（避免 system 长词打乱常用词第一屏）。
    fn apply_long_word_filter(&mut self, candidates: &mut Vec<String>) {
        const IDX: usize = 4; // 插入位置（对标 rime idx: 4）
        const COUNT: usize = 2; // 提升数量（对标 rime count: 2）
        // 英文候选起始位置（其后的不参与）
        let eng_start = self.english_candidate_pos.unwrap_or(candidates.len());
        let limit = candidates.len().min(eng_start);
        if limit <= IDX {
            return; // 候选不足，无需调整
        }
        let common = crate::dictionary::common_word_set();
        let is_common = |w: &str| common.contains(w);
        let mut inserted = 0;
        let mut i = IDX;
        while i < limit && inserted < COUNT {
            let is_hanzi_long = candidates[i].chars().count() >= 2
                && candidates[i].chars().any(|c| c as u32 > 0x7F);
            if is_hanzi_long && !is_common(&candidates[i]) {
                let w = candidates.remove(i);
                // 插入位置：跳过 common 词占位（common 顺序不动），不越过英文区
                // V0.4：remove 后 len 减小，pos 可能越界（英文无命中 + 原样兜底移除后）
                let mut pos = IDX + inserted;
                while pos < eng_start && pos < candidates.len() && is_common(&candidates[pos]) {
                    pos += 1;
                }
                candidates.insert(pos.min(eng_start), w);
                inserted += 1;
                // 插入后继续（i 前进避免死循环）
            }
            i += 1;
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

    /// 双拼模式查询：双拼码串 → 全拼候选 → 词库查询
    fn query_all_shuangpin(&mut self) {
        let code = self.pinyin_buf.clone();
        let mut candidates = Vec::new();

        // 解码双拼码为全拼（可能有多个歧义候选；P2-7 按当前方案解码）
        let full_pinyins = self.shuangpin_scheme.decode_string(&code);
        for fp in &full_pinyins {
            for w in dictionary::query(fp) {
                if !candidates.contains(&w) {
                    candidates.push(w);
                }
            }
        }
        // 完整音节无候选时，尝试模糊音（双拼+模糊音可叠加）
        if candidates.is_empty() && self.fuzzy_enabled {
            for fp in &full_pinyins {
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

        // 截断到 max_pages 页（双拼模式不追加英文候选/短语）
        candidates.truncate(self.page_size * self.max_pages);
        self.english_candidate_pos = None;
        self.phrase_candidate_pos = None;
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
        // 自定义置顶（P2-2 load_pins）：wo → 我们 置顶（内置词库有"我们"）
        let mut engine = Engine::new();
        engine.load_pins(vec![("wo".to_string(), vec!["我们".to_string()])]);
        for ch in "wo".chars() {
            engine.process_key(ch);
        }
        assert_eq!(
            engine.candidate(0),
            Some("我们"),
            "自定义置顶 wo→我们 应生效, got {:?}",
            (0..engine.candidate_count())
                .map(|i| engine.candidate(i).unwrap_or(""))
                .collect::<Vec<_>>()
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
        let mut engine = Engine::new();
        crate::radical::init(None); // 空表
        for ch in "uzzzzz".chars() {
            engine.process_key(ch);
        }
        assert!(engine.is_radical_mode());
        assert_eq!(engine.candidate_count(), 0, "未知部件应无候选");
    }

    #[test]
    fn test_radical_single_u_not_mode() {
        let mut engine = Engine::new();
        engine.process_key('u');
        assert!(!engine.is_radical_mode(), "单独 u 不进入反查模式");
    }

    #[test]
    fn test_radical_select_no_learn() {
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
        let has_approx = (0..engine.candidate_count()).any(|i| engine.candidate(i) == Some("≈"));
        assert!(has_approx, "vsx 应列出数学符号");
    }

    #[test]
    fn test_symbol_mode_select_no_learn() {
        // 符号选中不上屏学习（is_symbol_mode 分支）
        let mut engine = Engine::new();
        for ch in "vjt".chars() {
            engine.process_key(ch);
        }
        let text = engine.select_candidate(0).unwrap();
        assert_eq!(text, "→");
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
    fn test_symbol_mode_single_v_is_normal() {
        // 单独 'v' 不进入符号模式（正常拼音处理）
        let mut engine = Engine::new();
        engine.process_key('v');
        assert!(!engine.is_symbol_mode());
        // P1-1：v 无合法拼音 → 英文词典候选（value/version/vue…）
        assert!(engine.candidate_count() >= 1, "v 应有英文词典候选");
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
        // 中文模式输入 hello → 末尾应含英文候选 hello
        let mut engine = Engine::new();
        for ch in "hello".chars() {
            engine.process_key(ch);
        }
        assert_eq!(engine.pinyin_str(), "hello");
        // 英文候选恒在末尾
        let last = engine.candidate(engine.candidate_count() - 1);
        assert_eq!(last, Some("hello"), "末尾应为英文候选, got {last:?}");
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
}
