/// 词库模块 — 拼音到汉字映射
///
/// 第一期 MVP：从 SQLite 系统词库加载 + 内置最小词库降级。
/// 支持：
///   - 全拼前缀查询（query）
///   - 简拼声母查询（query_short，如 zg→中国）
///   - 多音节切分联想（phrase_guess，如 nihaoshijie→你好世界）
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::pinyin;

/// 词库 — 拼音前缀 → 候选词列表（按词频降序）
pub struct Dictionary {
    /// 全拼前缀索引：prefix → [(word, frequency, pinyin_len)]
    index: HashMap<String, Vec<(String, u32, usize)>>,
    /// 简拼声母索引：initial_prefix → [(word, frequency, pinyin_len)]
    short_index: HashMap<String, Vec<(String, u32, usize)>>,
    /// 用户词库索引（V0.2.2）：prefix → [(word, frequency, pinyin_len)]，查询时插队系统词
    user_index: HashMap<String, Vec<(String, u32, usize)>>,
    /// 完整拼音索引（0.1.26 混合简拼用）：pinyin → [(word, frequency)]
    full_index: BTreeMap<String, Vec<(String, u32)>>,
    /// 用户词库文件路径（learn 写回用）
    user_dict_path: Option<PathBuf>,
}

/// 前缀候选排序：精确拼音匹配优先（pinyin 长度 == 前缀长度），同组按词频降序
/// （0.1.26：修复单字被词组淹没——如输入 wo 先出"我"而非"我们"）
fn sort_by_exact_then_freq(entries: &mut [(String, u32, usize)], key_len: usize) {
    entries.sort_by(|a, b| {
        let a_exact = (a.2 == key_len) as u8;
        let b_exact = (b.2 == key_len) as u8;
        b_exact.cmp(&a_exact).then(b.1.cmp(&a.1))
    });
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

        let mut index: HashMap<String, Vec<(String, u32, usize)>> = HashMap::new();
        let mut short_index: HashMap<String, Vec<(String, u32, usize)>> = HashMap::new();
        let mut full_index: BTreeMap<String, Vec<(String, u32)>> = BTreeMap::new();

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
            // 完整拼音索引（混合简拼用）
            full_index
                .entry(pinyin_str.clone())
                .or_default()
                .push((word.clone(), frequency));
            // 为每个可能的前缀建立全拼索引
            for i in 1..=pinyin_str.len() {
                let prefix = &pinyin_str[..i];
                index
                    .entry(prefix.to_string())
                    .or_default()
                    .push((word.clone(), frequency, pinyin_str.len()));
            }
            // 建立简拼声母索引（zh→z, ch→c, sh→s，零声母取首字母）
            let short = pinyin::to_initial_string(&pinyin_str);
            if !short.is_empty() {
                for i in 1..=short.len() {
                    let prefix = &short[..i];
                    short_index
                        .entry(prefix.to_string())
                        .or_default()
                        .push((word.clone(), frequency, pinyin_str.len()));
                }
            }
        }

        // 每个前缀的候选按"精确拼音优先 + 词频降序"排序
        for (prefix, entries) in index.iter_mut() {
            sort_by_exact_then_freq(entries, prefix.len());
        }
        for (prefix, entries) in short_index.iter_mut() {
            sort_by_exact_then_freq(entries, prefix.len());
        }
        // 完整拼音索引按词频降序
        for words in full_index.values_mut() {
            words.sort_by(|a, b| b.1.cmp(&a.1));
        }

        Ok(Self {
            index,
            short_index,
            user_index: HashMap::new(),
            user_dict_path: None,
            full_index,
        })
    }

    /// 从内置词库构建（降级回退）
    fn from_builtin() -> Self {
        let mut index: HashMap<String, Vec<(String, u32, usize)>> = HashMap::new();
        let mut short_index: HashMap<String, Vec<(String, u32, usize)>> = HashMap::new();
        let mut full_index: BTreeMap<String, Vec<(String, u32)>> = BTreeMap::new();

        for entry in builtin_entries() {
            // 完整拼音索引（混合简拼用）
            full_index
                .entry(entry.pinyin.clone())
                .or_default()
                .push((entry.word.clone(), entry.frequency));
            for i in 1..=entry.pinyin.len() {
                let prefix = &entry.pinyin[..i];
                index
                    .entry(prefix.to_string())
                    .or_default()
                    .push((entry.word.clone(), entry.frequency, entry.pinyin.len()));
            }
            let short = pinyin::to_initial_string(&entry.pinyin);
            if !short.is_empty() {
                for i in 1..=short.len() {
                    let prefix = &short[..i];
                    short_index
                        .entry(prefix.to_string())
                        .or_default()
                        .push((entry.word.clone(), entry.frequency, entry.pinyin.len()));
                }
            }
        }

        for (prefix, entries) in index.iter_mut() {
            sort_by_exact_then_freq(entries, prefix.len());
        }
        for (prefix, entries) in short_index.iter_mut() {
            sort_by_exact_then_freq(entries, prefix.len());
        }
        for words in full_index.values_mut() {
            words.sort_by(|a, b| b.1.cmp(&a.1));
        }

        Self {
            index,
            short_index,
            user_index: HashMap::new(),
            user_dict_path: None,
            full_index,
        }
    }

    /// 加载用户词库（V0.2.2）：从独立 SQLite 文件读入 user_index。
    /// 文件不存在/损坏时静默置空（首次运行正常路径）。
    pub fn load_user_dict(&mut self, path: &Path) {
        self.user_dict_path = Some(path.to_path_buf());
        match Connection::open(path) {
            Ok(conn) => {
                // 建表（首次运行自动创建）
                let _ = conn.execute_batch(
                    "CREATE TABLE IF NOT EXISTS user_dict (
                        pinyin TEXT NOT NULL,
                        word TEXT NOT NULL,
                        frequency INTEGER DEFAULT 1,
                        last_used INTEGER DEFAULT 0,
                        PRIMARY KEY (pinyin, word)
                    );
                    CREATE INDEX IF NOT EXISTS idx_user_pinyin ON user_dict(pinyin);",
                );
                let mut stmt = match conn.prepare(
                    "SELECT pinyin, word, frequency FROM user_dict",
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        crate::log::error(&format!("用户词库查询失败: {e}"));
                        return;
                    }
                };
                let rows = stmt.query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, u32>(2)?,
                    ))
                });
                if let Ok(rows) = rows {
                    for row in rows.flatten() {
                        self.add_user_entry(row.0, row.1, row.2);
                    }
                    crate::log::info(&format!(
                        "用户词库加载成功: {} 条",
                        self.user_index.values().map(|v| v.len()).sum::<usize>()
                    ));
                }
            }
            Err(e) => {
                crate::log::error(&format!("用户词库打开失败（降级为空）: {e}"));
            }
        }
    }

    /// 学习用户词（V0.2.2）：内存 + 磁盘写回，词频 +1。
    /// 未启用用户词库（未设置路径）时静默跳过。磁盘失败静默降级（不阻塞输入）。
    pub fn learn_user_word(&mut self, pinyin_str: &str, word: &str) {
        if pinyin_str.is_empty() || word.is_empty() {
            return;
        }
        // 未设置路径 = 未启用用户词库，不学习
        let Some(path) = self.user_dict_path.clone() else {
            return;
        };
        // 内存：频率 +1
        self.add_user_entry(pinyin_str.to_string(), word.to_string(), 1);
        // 磁盘：INSERT OR REPLACE 累加频率
        if let Ok(conn) = Connection::open(&path) {
                let _ = conn.execute_batch(
                    "CREATE TABLE IF NOT EXISTS user_dict (
                        pinyin TEXT NOT NULL,
                        word TEXT NOT NULL,
                        frequency INTEGER DEFAULT 1,
                        last_used INTEGER DEFAULT 0,
                        PRIMARY KEY (pinyin, word)
                    );",
                );
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let res = conn.execute(
                    "INSERT INTO user_dict (pinyin, word, frequency, last_used)
                     VALUES (?1, ?2, 1, ?3)
                     ON CONFLICT(pinyin, word)
                     DO UPDATE SET frequency = frequency + 1, last_used = ?3",
                    rusqlite::params![pinyin_str, word, now],
                );
                if let Err(e) = res {
                    crate::log::error(&format!("用户词库写入失败: {e}"));
                }
            }
    }

    /// 向内存 user_index 添加词条（前缀展开，与系统词库同构）
    fn add_user_entry(&mut self, pinyin_str: String, word: String, frequency: u32) {
        for i in 1..=pinyin_str.len() {
            let prefix = &pinyin_str[..i];
            let entries = self.user_index.entry(prefix.to_string()).or_default();
            if let Some(existing) = entries.iter_mut().find(|(w, _, _)| *w == word) {
                existing.1 = existing.1.saturating_add(frequency);
            } else {
                entries.push((word.clone(), frequency, pinyin_str.len()));
            }
        }
        // 每前缀按"精确拼音优先 + 词频降序"排序
        for (prefix, entries) in self.user_index.iter_mut() {
            sort_by_exact_then_freq(entries, prefix.len());
        }
    }

    /// 用户词条合并进系统候选：用户词插队（去重，位置提前）
    /// 由 query() 内部调用——user_index 的 key 是拼音前缀（add_user_entry 已展开），
    /// 与查询前缀同构，直接按 key 匹配即可。

    /// 全拼前缀查询候选词（系统词 + 用户词插队）
    /// 排序：精确拼音匹配优先（pinyin == 前缀），同组按词频降序（0.1.26）
    pub fn query(&self, pinyin_prefix: &str) -> Vec<String> {
        let key = pinyin_prefix.to_lowercase();
        let mut result: Vec<String> = Vec::new();
        // 用户词插队：精确拼音优先 + 词频序（学过的词优先）
        if let Some(user_entries) = self.user_index.get(&key) {
            for (w, _, _) in user_entries {
                if !result.contains(w) {
                    result.push(w.clone());
                }
            }
        }
        // 系统词（跳过已在用户词中出现的）
        if let Some(entries) = self.index.get(&key) {
            for (w, _, _) in entries {
                if !result.contains(w) {
                    result.push(w.clone());
                }
            }
        }
        result
    }

    /// 简拼声母前缀查询候选词（如 "zg" → 中国）
    pub fn query_short(&self, prefix: &str) -> Vec<String> {
        let key = prefix.to_lowercase();
        match self.short_index.get(&key) {
            Some(entries) => entries.iter().map(|(w, _, _)| w.clone()).collect(),
            None => Vec::new(),
        }
    }

    /// 混合简拼查询（0.1.26）：输入串 = 完整音节前缀 + 声母后缀
    /// 例：shurf = shu(输) + r(入·声母) + f(法·声母) → 输入法（shurufa）
    /// 枚举切分点：prefix 必须能完整切分为音节（防 s/sh 过宽前缀），
    /// 匹配词拼音以 prefix 开头且剩余拼音的声母串以 suffix 开头。
    pub fn query_mixed(&self, input: &str) -> Vec<String> {
        let input = input.to_lowercase();
        let mut result: Vec<String> = Vec::new();
        if input.len() < 3 || input.len() > 8 {
            return result;
        }
        for i in 1..input.len() {
            let prefix = &input[..i];
            let suffix = &input[i..];
            if suffix.is_empty() {
                continue;
            }
            // 前缀必须能被完整切分为音节（避免 s/sh 等过宽前缀）
            let syllables = split_into_syllables(prefix);
            if syllables.is_empty() || syllables.join("") != prefix {
                continue;
            }
            // BTreeMap range：只遍历以 prefix 开头的完整拼音
            for (pinyin, words) in self.full_index.range(prefix.to_string()..) {
                if !pinyin.starts_with(prefix) {
                    break;
                }
                if pinyin.len() == prefix.len() {
                    continue; // 完整拼音已由 query() 覆盖
                }
                let rest = &pinyin[prefix.len()..];
                let rest_initials = crate::pinyin::to_initial_string(rest);
                if !rest_initials.is_empty() && rest_initials.starts_with(suffix) {
                    for (w, _) in words {
                        if !result.contains(w) {
                            result.push(w.clone());
                        }
                    }
                }
            }
        }
        result
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

/// 设置用户词库路径（V0.2.2）。NULL/空 = 禁用用户词库。
/// 需在 init 之后调用（Dictionary 实例已存在）。
pub fn set_user_dict_path(path: Option<&Path>) {
    let mut dict = DICT.lock().unwrap_or_else(|e| e.into_inner());
    match dict.as_mut() {
        Some(d) => match path {
            Some(p) => d.load_user_dict(p),
            None => {
                d.user_dict_path = None;
                d.user_index.clear();
                crate::log::info("用户词库已禁用");
            }
        },
        None => crate::log::error("用户词库路径设置失败：词库未初始化"),
    }
}

/// 学习用户词（V0.2.2）：内存 + 磁盘写回。词库未初始化时静默跳过。
pub fn learn(pinyin_str: &str, word: &str) {
    let mut dict = DICT.lock().unwrap_or_else(|e| e.into_inner());
    match dict.as_mut() {
        Some(d) => d.learn_user_word(pinyin_str, word),
        None => {
            // 词库未初始化——先初始化再学
            drop(dict);
            init(None);
            DICT.lock().unwrap().as_mut().unwrap().learn_user_word(pinyin_str, word);
        }
    }
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

/// 混合简拼查询（0.1.26，自动触发延迟初始化）
pub fn query_mixed(input: &str) -> Vec<String> {
    let dict = DICT.lock().unwrap_or_else(|e| e.into_inner());
    match dict.as_ref() {
        Some(d) => d.query_mixed(input),
        None => {
            drop(dict);
            init(None);
            DICT.lock().unwrap().as_ref().unwrap().query_mixed(input)
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

    // ─── 0.1.26 精确拼音优先测试（单字不被词组淹没）───

    #[test]
    fn test_exact_pinyin_priority_builtin() {
        // 内置词库：wo=我(680), women=我们(450)。精确拼音优先 → wo 先出"我"
        init(None);
        let results = query("wo");
        assert_eq!(results.first().map(String::as_str), Some("我"),
            "内置词库 wo 应优先出单字 我, got: {:?}",
            results.iter().take(5).collect::<Vec<_>>());
    }

    #[test]
    fn test_exact_pinyin_priority_sqlite() {
        // 真实词库：wo 前缀下"我们"(3908) 词频高于"我"，但精确优先应让"我"排第一
        let db_path = Path::new("../../resources/system_dict.db");
        if db_path.exists() {
            init(Some(db_path));
            let results = query("wo");
            assert_eq!(results.first().map(String::as_str), Some("我"),
                "SQLite 词库 wo 应优先出单字 我, got: {:?}",
                results.iter().take(5).collect::<Vec<_>>());
        }
    }

    #[test]
    fn test_exact_pinyin_priority_long_word_still_available() {
        // 精确优先不丢长词：women 仍应在前缀候选里（排在"我"之后）
        init(None);
        let results = query("wo");
        assert!(results.iter().any(|w| w == "我们"),
            "长词 我们 不应丢失, got: {:?}", results.iter().take(10).collect::<Vec<_>>());
    }

    // ─── 0.1.26 混合简拼测试（shurf → 输入法）───

    #[test]
    fn test_mixed_shurf_inputfa() {
        // 真实词库：shurf = shu + rf → shurufa（输入法）
        let db_path = Path::new("../../resources/system_dict.db");
        if db_path.exists() {
            init(Some(db_path));
            let results = query_mixed("shurf");
            assert!(results.iter().any(|w| w == "输入法"),
                "shurf 应联想出 输入法, got: {:?}", results);
        }
    }

    #[test]
    fn test_mixed_full_prefix_still_matches() {
        // shuruf（完整拼音前缀）混合匹配也应出输入法
        let db_path = Path::new("../../resources/system_dict.db");
        if db_path.exists() {
            init(Some(db_path));
            let results = query_mixed("shuruf");
            assert!(results.iter().any(|w| w == "输入法"),
                "shuruf 应联想出 输入法, got: {:?}", results);
        }
    }

    #[test]
    fn test_mixed_too_short_no_result() {
        // 输入过短（<3）不触发混合简拼
        init(None);
        assert!(query_mixed("wo").is_empty(), "wo 不应走混合简拼");
    }

    #[test]
    fn test_mixed_builtin_available() {
        // 内置词库：nihaosj？无此词。验证混合简拼对内置词库不崩溃
        init(None);
        let _ = query_mixed("zhongg"); // zhong + g → 中国(zhongguo)
        // 内置词库有 zhongguo → 应出 中国
        let results = query_mixed("zhongg");
        assert!(results.iter().any(|w| w == "中国"),
            "zhongg 应联想出 中国(内置词库), got: {:?}", results);
    }
}
