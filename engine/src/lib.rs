pub mod correction;
pub mod dictionary;
pub mod ffi;
pub mod fuzzy;
pub mod log;
pub mod pinyin;
pub mod shuangpin;

/// 引擎状态
pub struct Engine {
    /// 当前累积的拼音串（如 "zhongguo"）
    pinyin_buf: String,
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
}

impl Engine {
    pub fn new() -> Self {
        Self {
            pinyin_buf: String::new(),
            all_candidates: Vec::new(),
            candidates: Vec::new(),
            page: 0,
            page_size: 9,
            max_pages: 8,
            ascii_mode: false,
            fuzzy_enabled: true,
            shuangpin_mode: false,
            correction_enabled: true,
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
            self.query_all();
            true
        } else {
            false
        }
    }

    /// 获取当前拼音串
    pub fn pinyin_str(&self) -> &str {
        &self.pinyin_buf
    }

    /// 获取当前页候选词数量
    pub fn candidate_count(&self) -> usize {
        self.candidates.len()
    }

    /// 获取指定候选词（当前页内索引）
    pub fn candidate(&self, index: usize) -> Option<&str> {
        self.candidates.get(index).map(|s| s.as_str())
    }

    /// 选择候选词并提交（返回提交文本，同时重置状态）
    /// V0.2.2：选词时自动学习用户词（拼音串 + 选中词）
    pub fn select_candidate(&mut self, index: usize) -> Option<String> {
        let result = self.candidates.get(index).cloned();
        if let Some(word) = &result {
            // 学习用户词：当前拼音串 + 选中词 → 用户词库（frequency+1）
            if !self.pinyin_buf.is_empty() && !self.ascii_mode {
                crate::dictionary::learn(&self.pinyin_buf, word);
            }
        }
        self.reset();
        result
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
        self.all_candidates.clear();
        self.candidates.clear();
        self.page = 0;
    }

    /// 退格（删除最后一个拼音字符）
    pub fn backspace(&mut self) -> bool {
        if self.pinyin_buf.pop().is_some() {
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
        // 双拼模式：输入串是双拼码，先解码为全拼再查询
        if self.shuangpin_mode {
            self.query_all_shuangpin();
            return;
        }
        let pinyin_str = self.pinyin_buf.clone();
        // 优先整词/全拼前缀查询
        let mut candidates = dictionary::query(&pinyin_str);
        // 简拼补充（输入串同时作为简拼前缀，如 "zg"→中国）
        let short = dictionary::query_short(&pinyin_str);
        for w in short {
            if !candidates.contains(&w) {
                candidates.push(w);
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
        // 截断到 max_pages 页
        candidates.truncate(self.page_size * self.max_pages);
        self.all_candidates = candidates;
        self.page = 0;
        self.repage();
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

        // 截断到 max_pages 页
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
        assert!(default_count > 0 && default_count <= 9);

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
}
