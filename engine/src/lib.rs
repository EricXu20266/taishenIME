pub mod dictionary;
pub mod ffi;
pub mod pinyin;

/// 引擎状态
pub struct Engine {
    /// 当前累积的拼音串（如 "zhong"）
    pinyin_buf: String,
    /// 当前候选词列表
    candidates: Vec<String>,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            pinyin_buf: String::new(),
            candidates: Vec::new(),
        }
    }

    /// 处理一个按键，返回是否产生了新的候选词
    pub fn process_key(&mut self, ch: char) -> bool {
        if ch.is_ascii_alphabetic() {
            self.pinyin_buf.push(ch.to_ascii_lowercase());
            self.candidates = dictionary::query(&self.pinyin_buf);
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
                self.candidates = dictionary::query(&self.pinyin_buf);
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
}
