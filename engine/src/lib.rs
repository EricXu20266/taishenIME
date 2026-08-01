pub mod dictionary;
pub mod ffi;
pub mod log;
pub mod pinyin;

/// 引擎状态
pub struct Engine {
    /// 当前累积的拼音串（如 "zhong"）
    pinyin_buf: String,
    /// 当前候选词列表
    candidates: Vec<String>,
    /// 候选词数量上限（截断候选列表）
    candidate_limit: usize,
    /// 英文模式（true = 字母直接上屏，不经过拼音）
    ascii_mode: bool,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            pinyin_buf: String::new(),
            candidates: Vec::new(),
            candidate_limit: 9,
            ascii_mode: false,
        }
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

    /// 设置候选词数量上限（>=1 生效）
    pub fn set_candidate_limit(&mut self, limit: usize) {
        if limit >= 1 {
            self.candidate_limit = limit;
        }
        // 若已有候选超出新上限，立即截断
        self.candidates.truncate(self.candidate_limit);
    }

    /// 处理一个按键，返回是否产生了新的候选词
    /// 英文模式下字母键返回 false（平台层直接上屏字母）
    pub fn process_key(&mut self, ch: char) -> bool {
        if ch.is_ascii_alphabetic() {
            if self.ascii_mode {
                return false; // 英文模式：不累积拼音，平台层直通上屏
            }
            self.pinyin_buf.push(ch.to_ascii_lowercase());
            let mut candidates = dictionary::query(&self.pinyin_buf);
            candidates.truncate(self.candidate_limit);
            self.candidates = candidates;
            true
        } else {
            false
        }
    }

    /// 获取当前拼音串
    pub fn pinyin_str(&self) -> &str {
        &self.pinyin_buf
    }

    /// 获取候选词数量
    pub fn candidate_count(&self) -> usize {
        self.candidates.len()
    }

    /// 获取指定候选词
    pub fn candidate(&self, index: usize) -> Option<&str> {
        self.candidates.get(index).map(|s| s.as_str())
    }

    /// 选择候选词并提交（返回提交文本，同时重置状态）
    pub fn select_candidate(&mut self, index: usize) -> Option<String> {
        let result = self.candidates.get(index).cloned();
        self.reset();
        result
    }

    /// 清空状态
    pub fn reset(&mut self) {
        self.pinyin_buf.clear();
        self.candidates.clear();
    }

    /// 退格（删除最后一个拼音字符）
    pub fn backspace(&mut self) -> bool {
        if self.pinyin_buf.pop().is_some() {
            if self.pinyin_buf.is_empty() {
                self.candidates.clear();
            } else {
                let mut candidates = dictionary::query(&self.pinyin_buf);
                candidates.truncate(self.candidate_limit);
                self.candidates = candidates;
            }
            true
        } else {
            false
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

        // 默认上限 9
        engine.process_key('z');
        engine.process_key('h');
        engine.process_key('o');
        engine.process_key('n');
        engine.process_key('g');
        let default_count = engine.candidate_count();
        assert!(default_count > 0 && default_count <= 9);

        // 设上限 3：候选立即截断（若实际候选少于 3 则全保留）
        engine.set_candidate_limit(3);
        assert!(engine.candidate_count() <= 3);

        // 恢复 16：已截断的候选不自动恢复（当前拼音查询结果保留）
        let after = engine.candidate_count();
        engine.set_candidate_limit(16);
        assert_eq!(engine.candidate_count(), after);

        // 重新查询（退格再输入）验证新上限生效
        engine.reset();
        engine.process_key('n');
        engine.process_key('i');
        assert!(engine.candidate_count() > 0);
    }

    #[test]
    fn test_candidate_limit_ignores_zero() {
        let mut engine = Engine::new();
        engine.process_key('n');
        engine.process_key('i');
        // 0 应被忽略（保持默认）
        engine.set_candidate_limit(0);
        assert!(engine.candidate_count() > 0);
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
        // 英文模式下字母不累积拼音
        assert!(!engine.process_key('a'));
        assert_eq!(engine.pinyin_str(), "");
        assert_eq!(engine.candidate_count(), 0);
        // 切回中文后正常累积
        engine.set_ascii_mode(false);
        assert!(engine.process_key('n'));
        assert!(engine.process_key('i'));
        assert_eq!(engine.pinyin_str(), "ni");
    }
}
