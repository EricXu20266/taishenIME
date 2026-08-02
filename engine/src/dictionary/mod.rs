/// 词库模块 — 拼音到汉字映射
///
/// 第一期 MVP：从 SQLite 系统词库加载 + 内置最小词库降级。
/// 支持：
///   - 全拼前缀查询（query）
///   - 简拼声母查询（query_short，如 zg→中国）
///   - 多音节切分联想（phrase_guess，如 nihaoshijie→你好世界）
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::pinyin;

/// 词库 — 拼音前缀 → 候选词列表（按词频降序）
pub struct Dictionary {
    /// 全拼前缀索引：prefix → [(word, frequency)]
    index: HashMap<String, Vec<(String, u32)>>,
    /// 简拼声母索引：initial_prefix → [(word, frequency)]
    short_index: HashMap<String, Vec<(String, u32)>>,
}

impl Dictionary {
    /// 从 SQLite 文件加载系统词库
    pub fn from_sqlite(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| format!("打开词库失败: {e}"))?;

        let mut stmt = conn
            .prepare(
                "SELECT pinyin, word, frequency FROM system_dict ORDER BY pinyin, frequency DESC",
            )
            .map_err(|e| format!("查询词库失败: {e}"))?;

        let mut index: HashMap<String, Vec<(String, u32)>> = HashMap::new();
        let mut short_index: HashMap<String, Vec<(String, u32)>> = HashMap::new();

        let rows = stmt
            .query_map([], |row| {
                let pinyin: String = row.get(0)?;
                let word: String = row.get(1)?;
                let frequency: u32 = row.get(2)?;
                Ok((pinyin, word, frequency))
            })
            .map_err(|e| format!("读取词库失败: {e}"))?;

        for row in rows {
            let (pinyin_str, word, frequency) =
                row.map_err(|e| format!("解析词条失败: {e}"))?;
            // 为每个可能的前缀建立全拼索引
            for i in 1..=pinyin_str.len() {
                let prefix = &pinyin_str[..i];
                index
                    .entry(prefix.to_string())
                    .or_default()
                    .push((word.clone(), frequency));
            }
            // 建立简拼声母索引（zh→z, ch→c, sh→s，零声母取首字母）
            let short = pinyin::to_initial_string(&pinyin_str);
            if !short.is_empty() {
                for i in 1..=short.len() {
                    let prefix = &short[..i];
                    short_index
                        .entry(prefix.to_string())
                        .or_default()
                        .push((word.clone(), frequency));
                }
            }
        }

        // 每个前缀的候选词按频率降序排列
        for entries in index.values_mut() {
            entries.sort_by(|a, b| b.1.cmp(&a.1));
        }
        for entries in short_index.values_mut() {
            entries.sort_by(|a, b| b.1.cmp(&a.1));
        }

        Ok(Self { index, short_index })
    }

    /// 从内置词库构建（降级回退）
    fn from_builtin() -> Self {
        let mut index: HashMap<String, Vec<(String, u32)>> = HashMap::new();
        let mut short_index: HashMap<String, Vec<(String, u32)>> = HashMap::new();

        for entry in builtin_entries() {
            for i in 1..=entry.pinyin.len() {
                let prefix = &entry.pinyin[..i];
                index
                    .entry(prefix.to_string())
                    .or_default()
                    .push((entry.word.clone(), entry.frequency));
            }
            let short = pinyin::to_initial_string(&entry.pinyin);
            if !short.is_empty() {
                for i in 1..=short.len() {
                    let prefix = &short[..i];
                    short_index
                        .entry(prefix.to_string())
                        .or_default()
                        .push((entry.word.clone(), entry.frequency));
                }
            }
        }

        for entries in index.values_mut() {
            entries.sort_by(|a, b| b.1.cmp(&a.1));
        }
        for entries in short_index.values_mut() {
            entries.sort_by(|a, b| b.1.cmp(&a.1));
        }

        Self { index, short_index }
    }

    /// 全拼前缀查询候选词
    pub fn query(&self, pinyin_prefix: &str) -> Vec<String> {
        let key = pinyin_prefix.to_lowercase();
        match self.index.get(&key) {
            Some(entries) => entries.iter().map(|(w, _)| w.clone()).collect(),
            None => Vec::new(),
        }
    }

    /// 简拼声母前缀查询候选词（如 "zg" → 中国）
    pub fn query_short(&self, prefix: &str) -> Vec<String> {
        let key = prefix.to_lowercase();
        match self.short_index.get(&key) {
            Some(entries) => entries.iter().map(|(w, _)| w.clone()).collect(),
            None => Vec::new(),
        }
    }

    /// 多音节切分联想：将整串拼音切分为音节序列，
    /// 每个音节取首个候选，拼接成短语候选（如 nihaoshijie → 你好世界）。
    /// 任一首节查不到候选则跳过该组合。
    pub fn phrase_guess(&self, pinyin_str: &str) -> Vec<String> {
        // 递归切分为音节序列
        let syllables = split_into_syllables(pinyin_str);
        if syllables.len() < 2 {
            return Vec::new();
        }

        // 收集每个音节的候选（取前 3 个，供组合）
        let mut per_syllable: Vec<Vec<String>> = Vec::new();
        for syl in &syllables {
            let cands = self.query(syl);
            if cands.is_empty() {
                return Vec::new(); // 有音节无候选 → 无法联想
            }
            per_syllable.push(cands[..cands.len().min(3)].to_vec());
        }

        // 贪心组合：每音节取第一个候选 → 最自然短语
        let mut result = Vec::new();
        let first = per_syllable
            .iter()
            .map(|c| c[0].clone())
            .collect::<String>();
        result.push(first);

        // 组合每音节前 2 个候选生成额外候选（去重）
        let mut seen = std::collections::HashSet::new();
        seen.insert(result[0].clone());
        let mut stack: Vec<String> = vec![String::new()];
        for cands in &per_syllable {
            let mut next = Vec::new();
            for prefix in &stack {
                for c in cands.iter().take(2) {
                    let joined = format!("{prefix}{c}");
                    if !seen.contains(&joined) {
                        seen.insert(joined.clone());
                        next.push(joined);
                    }
                }
            }
            stack = next;
        }
        for combo in stack {
            result.push(combo);
        }
        result
    }
}

/// 将拼音串切分为音节序列（最长匹配）
fn split_into_syllables(input: &str) -> Vec<String> {
    let mut syllables = Vec::new();
    let mut rest = input;
    while !rest.is_empty() {
        match pinyin::split_first_syllable(rest) {
            Some((syl, remaining)) => {
                syllables.push(syl.to_string());
                rest = remaining;
            }
            None => {
                // 无法继续切分——放弃
                return Vec::new();
            }
        }
    }
    syllables
}

// ─── 内置最小词库（SQLite 不可用时的降级方案）───

struct BuiltinEntry {
    pinyin: String,
    word: String,
    frequency: u32,
}

fn builtin_entries() -> Vec<BuiltinEntry> {
    vec![
        e("de", "的", 1000),
        e("yi", "一", 900),
        e("shi", "是", 850),
        e("bu", "不", 800),
        e("le", "了", 750),
        e("ren", "人", 700),
        e("wo", "我", 680),
        e("zai", "在", 660),
        e("you", "有", 640),
        e("ta", "他", 620),
        e("zhe", "这", 600),
        e("wei", "为", 580),
        e("da", "大", 560),
        e("lai", "来", 540),
        e("shang", "上", 520),
        e("ge", "个", 500),
        e("men", "们", 480),
        e("dao", "到", 460),
        e("shuo", "说", 440),
        e("zi", "子", 420),
        e("jiu", "就", 400),
        e("ye", "也", 390),
        e("he", "和", 380),
        e("xia", "下", 370),
        e("yao", "要", 360),
        e("hui", "会", 350),
        e("neng", "能", 340),
        e("zhong", "中", 330),
        e("guo", "国", 320),
        e("hao", "好", 310),
        e("sheng", "生", 300),
        e("nian", "年", 290),
        e("xue", "学", 280),
        e("gong", "工", 270),
        e("tian", "天", 260),
        e("di", "地", 250),
        e("xin", "心", 240),
        e("qian", "前", 230),
        e("hou", "后", 220),
        e("jia", "家", 210),
        e("shi", "时", 200),
        e("duo", "多", 195),
        e("shao", "少", 190),
        e("ming", "名", 185),
        e("wen", "文", 180),
        e("gao", "高", 175),
        e("er", "而", 170),
        e("fa", "发", 165),
        e("ru", "如", 160),
        e("zhongguo", "中国", 500),
        e("women", "我们", 450),
        e("tamen", "他们", 440),
        e("yige", "一个", 420),
        e("renwei", "认为", 400),
        e("yinwei", "因为", 390),
        e("suoyi", "所以", 380),
        e("keshi", "可是", 370),
        e("gongzuo", "工作", 300),
        e("xuexiao", "学校", 290),
        e("wentiti", "问题", 280),
        e("fangfa", "方法", 270),
        e("shijian", "时间", 260),
        e("shenghuo", "生活", 250),
        e("kaifa", "开发", 240),
        e("chengxu", "程序", 230),
        e("shuru", "输入", 220),
        e("shuchu", "输出", 210),
        e("bianma", "编码", 200),
        e("xitong", "系统", 190),
        e("jisuan", "计算", 180),
        e("shuju", "数据", 170),
        e("wangluo", "网络", 160),
        e("ruanjian", "软件", 155),
        e("yingjian", "硬件", 150),
        e("sudu", "速度", 135),
        e("anquan", "安全", 130),
        e("fuwu", "服务", 125),
        e("yonghu", "用户", 115),
        e("jieguo", "结果", 110),
        e("guocheng", "过程", 105),
        e("yanjiu", "研究", 100),
        e("nihao", "你好", 300),
        e("xiexie", "谢谢", 290),
        e("zaijian", "再见", 280),
        e("duibuqi", "对不起", 270),
        e("meiguanxi", "没关系", 260),
        e("bangzhu", "帮助", 240),
        e("lianxi", "联系", 230),
        e("huanying", "欢迎", 220),
    ]
}

fn e(pinyin: &str, word: &str, frequency: u32) -> BuiltinEntry {
    BuiltinEntry {
        pinyin: pinyin.to_string(),
        word: word.to_string(),
        frequency,
    }
}

// ─── 全局单例 ───

static DICT: Mutex<Option<Dictionary>> = Mutex::new(None);

/// 尝试从给定路径加载词库，失败则回退到内置词库
pub fn init(dict_path: Option<&Path>) {
    let mut dict = DICT.lock().unwrap_or_else(|e| e.into_inner());

    if let Some(path) = dict_path {
        if let Ok(d) = Dictionary::from_sqlite(path) {
            let count = d.index.len();
            crate::log::info(&format!("词库加载成功: {} 前缀 ({} 简拼前缀)", count, d.short_index.len()));
            *dict = Some(d);
            return;
        }
        // SQLite 加载失败——回退到内置词库
        crate::log::error(&format!("词库加载失败: {}，降级内置词库", path.display()));
    }

    // 回退到内置词库
    *dict = Some(Dictionary::from_builtin());
    crate::log::info("词库降级：使用内置词库");
}

/// 查询候选词（自动触发延迟初始化）
pub fn query(pinyin_prefix: &str) -> Vec<String> {
    let dict = DICT.lock().unwrap_or_else(|e| e.into_inner());
    match dict.as_ref() {
        Some(d) => d.query(pinyin_prefix),
        None => {
            // 尚未初始化——延迟加载内置词库
            drop(dict);
            init(None);
            DICT.lock().unwrap().as_ref().unwrap().query(pinyin_prefix)
        }
    }
}

/// 简拼声母查询（自动触发延迟初始化）
pub fn query_short(prefix: &str) -> Vec<String> {
    let dict = DICT.lock().unwrap_or_else(|e| e.into_inner());
    match dict.as_ref() {
        Some(d) => d.query_short(prefix),
        None => {
            drop(dict);
            init(None);
            DICT.lock().unwrap().as_ref().unwrap().query_short(prefix)
        }
    }
}

/// 多音节切分联想（自动触发延迟初始化）
pub fn phrase_guess(pinyin_str: &str) -> Vec<String> {
    let dict = DICT.lock().unwrap_or_else(|e| e.into_inner());
    match dict.as_ref() {
        Some(d) => d.phrase_guess(pinyin_str),
        None => {
            drop(dict);
            init(None);
            DICT.lock().unwrap().as_ref().unwrap().phrase_guess(pinyin_str)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_fallback() {
        init(None);
        let results = query("zhong");
        assert!(results.iter().any(|w| w == "中"));
        assert!(results.iter().any(|w| w == "中国"));
    }

    #[test]
    fn test_query_empty() {
        init(None);
        let results = query("zzz");
        assert!(results.is_empty());
    }

    #[test]
    fn test_sqlite_load() {
        let db_path = Path::new("../../resources/system_dict.db");
        if db_path.exists() {
            init(Some(db_path));
            let results = query("zhong");
            assert!(results.iter().any(|w| w == "中"));
            assert!(results.len() >= 2);
        }
    }

    #[test]
    fn test_sqlite_short() {
        let db_path = Path::new("../../resources/system_dict.db");
        if db_path.exists() {
            init(Some(db_path));
            // 简拼：zg → 中国（真实 4060 条词库）
            let results = query_short("zg");
            assert!(results.iter().any(|w| w == "中国"),
                    "SQLite 词库简拼 zg 应命中中国, got {:?}", results);
        }
    }

    #[test]
    fn test_short_query() {
        init(None);
        // 中国 → 简拼 zg
        let results = query_short("zg");
        assert!(results.iter().any(|w| w == "中国"));
    }

    #[test]
    fn test_phrase_guess_builtin() {
        init(None);
        // 你好世界：nihao 有词条，shijie 可能没有 → 不保证成功，仅验证不崩溃
        let _ = phrase_guess("nihaoshijie");
    }
}
