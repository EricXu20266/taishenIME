pub mod calculator;
pub mod correction;
pub mod datetime;
pub mod dictionary;
pub mod ffi;
pub mod fuzzy;
pub mod log;
pub mod mistake;
pub mod pinyin;
pub mod radical;
pub mod shuangpin;
pub mod symbol;
pub mod trad;

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
    /// 模糊音开关（RIME 拼写变体，默认开）
    fuzzy_enabled: bool,
    /// 双拼模式（RIME 双拼方案，微软双拼，默认关）
    shuangpin_mode: bool,
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
}

impl Engine {
    pub fn new() -> Self {
        Self {
            pinyin_buf: String::new(),
            raw_input: String::new(),
            cap_state: CapState::Lower,
            all_candidates: Vec::new(),
            candidates: Vec::new(),
            page: 0,
            page_size: 5,
            max_pages: 8,
            ascii_mode: false,
            fuzzy_enabled: true,
            shuangpin_mode: false,
            correction_enabled: true,
            mix_mode_enabled: true,
            english_candidate_pos: None,
            traditional_mode: false,
            phrase_enabled: true,
            phrase_map: Self::builtin_phrases(),
            phrase_candidate_pos: None,
            datetime_candidate_pos: None,
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
    pub fn set_ascii_mode(&mut self, enabled: bool) {
        self.ascii_mode = enabled;
        // 切换模式时清空未完成拼音
        self.reset();
    }

    /// 查询英文模式
    pub fn ascii_mode(&self) -> bool {
        self.ascii_mode
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
            self.update_cap_state();
            self.query_all();
            true
        } else if self.is_calc_continuation(ch) {
            // V0.2.22：c 模式下继续输入运算符/数字 → 追加并触发计算器查询
            self.pinyin_buf.push(ch);
            self.raw_input.push(ch);
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
        self.pinyin_buf.starts_with('c')
            && (ch.is_ascii_digit() || "+-*/()%^.".contains(ch))
    }

    /// 重算英文候选大小写模式（V0.2.23）
    /// Hello → Capitalize；HE（前 2 大写/全大写）→ Upper；其余 → Lower
    fn update_cap_state(&mut self) {
        self.cap_state = if self.raw_input.len() >= 2
            && self.raw_input.chars().nth(1).is_some_and(|c| c.is_ascii_uppercase())
        {
            CapState::Upper
        } else if self.raw_input.len() >= 1
            && self.raw_input.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        {
            CapState::Capitalize
        } else {
            CapState::Lower
        };
    }

    /// 按大小写模式转换文本（V0.2.23，仅英文候选用）
    fn apply_cap(&self, word: &str) -> String {
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
    pub fn is_radical_mode(&self) -> bool {
        !self.shuangpin_mode && self.pinyin_buf.starts_with('u') && self.pinyin_buf.len() > 1
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
    fn is_english_candidate(&self, index: usize) -> bool {
        match self.english_candidate_pos {
            Some(pos) => pos == self.page * self.page_size + index,
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
    fn datetime_candidates(code: &str) -> Option<Vec<String>> {
        match code {
            "rq" => Some(datetime::date_candidates()),
            "sj" => Some(datetime::time_candidates()),
            "xq" => Some(datetime::weekday_candidates()),
            "nl" => Some(datetime::lunar_candidates()),
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
            if !is_english && !is_phrase && !is_symbol && !is_calc && !is_datetime && !is_radical && !is_mistake
                && !self.pinyin_buf.is_empty() && !self.ascii_mode
            {
                crate::dictionary::learn(&self.pinyin_buf, word);
            }
            // 简繁转换：输出繁体（英文/短语/符号/计算器/日期/反查/错音候选不转）
            if self.traditional_mode && !is_english && !is_phrase && !is_symbol && !is_calc && !is_datetime && !is_radical && !is_mistake {
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
        self.cap_state = CapState::Lower;
        self.all_candidates.clear();
        self.candidates.clear();
        self.page = 0;
        self.english_candidate_pos = None;
        self.phrase_candidate_pos = None;
        self.datetime_candidate_pos = None;
    }

    /// 退格（删除最后一个拼音字符）
    pub fn backspace(&mut self) -> bool {
        if self.pinyin_buf.pop().is_some() {
            self.raw_input.pop(); // V0.2.23：同步清除原始输入
            self.update_cap_state();
            if self.pinyin_buf.is_empty() {
                self.all_candidates.clear();
                self.candidates.clear();
                self.page = 0;
            } else {
                self.query_all();
            }
            true
        } else {
            false
        }
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
        // 双拼模式：输入串是双拼码，先解码为全拼再查询
        if self.shuangpin_mode {
            self.query_all_shuangpin();
            return;
        }
        let pinyin_str = self.pinyin_buf.clone();
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
        // 模糊音容错（RIME Spelling Algebra，0.1.14）：输入串变体查询，补在精确命中后
        if self.fuzzy_enabled && fuzzy::may_have_fuzzy(&pinyin_str) {
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
        // 智能纠错（V0.2.10）：候选不足时，键盘相邻键变体补入
        // 误触纠正：logn→long→龙、nihap→nihao→你好（排在精确/模糊之后）
        if self.correction_enabled && candidates.len() < self.page_size
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
        if candidates.len() < self.page_size {
            for (correct_py, _word) in mistake::lookup(&pinyin_str) {
                for w in dictionary::query(correct_py) {
                    if !candidates.contains(&w) {
                        candidates.push(w);
                    }
                }
            }
        }
        // 截断到 max_pages 页
        candidates.truncate(self.page_size * self.max_pages);
        // 中英混输（V0.2.8）：中文模式下候选末尾追加英文候选（输入串原样）
        // 不干扰汉字排序；ASCII 模式不追加
        self.english_candidate_pos = None;
        if self.mix_mode_enabled && !self.ascii_mode
            && !pinyin_str.is_empty() && pinyin_str.chars().all(|c| c.is_ascii_alphabetic())
        {
            candidates.push(pinyin_str.clone());
            self.english_candidate_pos = Some(candidates.len() - 1);
        }
        self.all_candidates = candidates;
        self.page = 0;
        self.repage();
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

        // 解码双拼码为全拼（可能有多个歧义候选）
        let full_pinyins = shuangpin::codec::decode_string(&code);
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
        let has_zhongguo = (0..engine.candidate_count())
            .any(|i| engine.candidate(i) == Some("中国"));
        assert!(has_zhongguo, "简拼 zg 应联想出中国");
    }

    #[test]
    fn test_phrase_guess() {
        // 整词优先：nihao 直接出"你好"
        let mut engine = Engine::new();
        for ch in "nihao".chars() {
            engine.process_key(ch);
        }
        let has_nihao = (0..engine.candidate_count())
            .any(|i| engine.candidate(i) == Some("你好"));
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
        assert!(mistake::lookup("cancha").iter().any(|(py, w)| *py == "cenci" && *w == "参差"));
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
        let has_zhongguo = (0..engine.candidate_count())
            .any(|i| engine.candidate(i) == Some("中国"));
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
        let has_iso = (0..engine.candidate_count())
            .any(|i| engine.candidate(i).is_some_and(|c| c.len() == 10 && c.contains('-')));
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
        let mut engine = Engine::new();
        for ch in "HELLO".chars() {
            engine.process_key(ch);
        }
        // 退格到 HE → 仍 Upper（前 2 大写）
        engine.backspace();
        engine.backspace();
        engine.backspace();
        // 英文候选在末尾——翻页遍历（display 含大小写转换）找到 HE
        let mut found = String::new();
        let total_pages = engine.total_pages();
        for _ in 0..total_pages {
            for i in 0..engine.candidate_count() {
                if let Some(c) = engine.candidate_display(i) {
                    if c == "HE" {
                        found = "HE".to_string();
                    }
                }
            }
            if engine.page(1) <= 0 {
                break;
            }
        }
        assert_eq!(found, "HE");
        // 再退格到 H → Capitalize
        let mut engine2 = Engine::new();
        for ch in "Hello".chars() {
            engine2.process_key(ch);
        }
        engine2.backspace();
        engine2.backspace();
        engine2.backspace();
        engine2.backspace();
        // 英文候选在末尾——翻页遍历（display 含大小写转换）找到 H
        let mut found2 = String::new();
        let total_pages2 = engine2.total_pages();
        for _ in 0..total_pages2 {
            for i in 0..engine2.candidate_count() {
                if let Some(c) = engine2.candidate_display(i) {
                    if c == "H" {
                        found2 = "H".to_string();
                    }
                }
            }
            if engine2.page(1) <= 0 {
                break;
            }
        }
        assert_eq!(found2, "H");
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
        let has_arrow = (0..engine.candidate_count())
            .any(|i| engine.candidate(i) == Some("→"));
        assert!(has_arrow, "vjt 应列出箭头符号");
    }

    #[test]
    fn test_symbol_mode_math() {
        let mut engine = Engine::new();
        for ch in "vsx".chars() {
            engine.process_key(ch);
        }
        let has_approx = (0..engine.candidate_count())
            .any(|i| engine.candidate(i) == Some("≈"));
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
        // v 无合法拼音 → 仅英文混输候选（v 本身）
        assert_eq!(engine.candidate_count(), 1);
        assert_eq!(engine.candidate(0), Some("v"));
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
        let has_zhong = (0..engine.candidate_count())
            .any(|i| engine.candidate(i) == Some("中"));
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
    fn test_correction_logn_to_long() {
        // gogn → 相邻交换 → gong → 工（内置词库有 gong=工）
        let mut engine = Engine::new();
        for ch in "gogn".chars() {
            engine.process_key(ch);
        }
        let has_gong = (0..engine.candidate_count())
            .any(|i| engine.candidate(i) == Some("工"));
        assert!(has_gong, "gogn 应纠错出 工, got: {:?}", (0..engine.candidate_count()).map(|i| engine.candidate(i).unwrap_or("")).collect::<Vec<_>>());
    }

    #[test]
    fn test_correction_off_no_suggestion() {
        // 关闭纠错：gogn 不应出 工
        let mut engine = Engine::new();
        engine.set_correction_enabled(false);
        for ch in "gogn".chars() {
            engine.process_key(ch);
        }
        let has_gong = (0..engine.candidate_count())
            .any(|i| engine.candidate(i) == Some("工"));
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
        let has_english = (0..engine.candidate_count())
            .any(|i| engine.candidate(i) == Some("hello"));
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
        let has_phrase = (0..engine.candidate_count())
            .any(|i| engine.candidate(i) == Some("不客气"));
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
        let has_phrase = (0..engine.candidate_count())
            .any(|i| engine.candidate(i) == Some("不客气"));
        assert!(!has_phrase, "关闭短语后 bq 不应出短语");
    }

    #[test]
    fn test_phrase_custom_load() {
        // 外部加载覆盖内置
        let mut engine = Engine::new();
        engine.load_phrases(vec![("bq".to_string(), "自定义短语".to_string())]);
        engine.process_key('b');
        engine.process_key('q');
        let has_custom = (0..engine.candidate_count())
            .any(|i| engine.candidate(i) == Some("自定义短语"));
        assert!(has_custom, "外部短语应覆盖内置");
    }
}
