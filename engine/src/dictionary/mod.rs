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
use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::pinyin;

/// 词库 — 拼音前缀 → 候选词列表（按词频降序）
/// 0.2.29：支持 serde 序列化 → 部署期预编译 .bin（跳过 SQLite 全量重建索引）。
/// user_index/user_dict_path 为运行时状态（skip，不参与 .bin 持久化）。
#[derive(Serialize, Deserialize)]
pub struct Dictionary {
    /// 全拼前缀索引：prefix → [(word, frequency, pinyin_len)]
    index: HashMap<String, Vec<(String, u32, usize)>>,
    /// 简拼声母索引：initial_prefix → [(word, frequency, pinyin_len)]
    short_index: HashMap<String, Vec<(String, u32, usize)>>,
    /// 常用词索引（V0.2.30）：prefix → [(word, rank, pinyin_len)]，rank 越小越优先。
    /// 命中即出候选，不受 system_dict 词频分档影响；运行时从 common_dict.txt 读，
    /// 不参与 .bin 持久化（用户改词表下次启动生效，旧 .bin 兼容）。
    #[serde(skip)]
    common_index: HashMap<String, Vec<(String, u32, usize)>>,
    /// 常用词声母索引（V0.2.30）：简拼前缀 → [(word, rank, pinyin_len)]
    #[serde(skip)]
    common_short_index: HashMap<String, Vec<(String, u32, usize)>>,
    /// 用户词库索引（V0.2.2）：prefix → [(word, frequency, last_used, pinyin_len)]。
    /// V0.2.30 热度学习：查询时按 热词(7天内≥3次) > 温词 分档插队系统词。
    #[serde(skip)]
    user_index: HashMap<String, Vec<(String, u32, i64, usize)>>,
    /// 完整拼音索引（0.1.26 混合简拼用）：pinyin → [(word, frequency)]
    full_index: BTreeMap<String, Vec<(String, u32)>>,
    /// 用户词库文件路径（learn 写回用）
    #[serde(skip)]
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

/// 用户词前缀排序（V0.2.30，4 元组版）：精确拼音优先 + 词频降序
fn sort_by_exact_then_freq_user(entries: &mut [(String, u32, i64, usize)], key_len: usize) {
    entries.sort_by(|a, b| {
        let a_exact = (a.3 == key_len) as u8;
        let b_exact = (b.3 == key_len) as u8;
        b_exact.cmp(&a_exact).then(b.1.cmp(&a.1))
    });
}

// ─── V0.2.30 常用词层 + 用户词热度学习 ───

/// 热词判定阈值：7 天内累计使用 ≥ HOT_THRESHOLD 次 → 热词（压过常用词）
const HOT_THRESHOLD: u32 = 3;
/// 热词时间窗口（秒）：7 天
const HOT_WINDOW_SECS: i64 = 7 * 24 * 3600;

/// 热词判定（V0.2.30）：近期（7 天窗口内）使用 ≥ 3 次的用户词。
/// 用途：区分"偶然打过一次"（温词，只在系统词前插队）与
/// "稳定偏好"（热词，压过常用词），避免误学污染首位。
fn is_hot(frequency: u32, last_used: i64, now: i64) -> bool {
    frequency >= HOT_THRESHOLD && now - last_used <= HOT_WINDOW_SECS
}

/// 当前 Unix 时间戳（秒）
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 加载常用词表（V0.2.30）：从 <词库目录>/common_dict.txt 解析构建
/// common_index / common_short_index。文件不存在 → 内置兜底词表。
/// 不参与 .bin 持久化（serde skip），运行期每次加载时构建，
/// 用户修改 common_dict.txt 下次启动生效。
fn load_common(dict: &mut Dictionary, dir: Option<&Path>) {
    let entries: Vec<(String, String)> = match dir {
        Some(d) => match std::fs::read_to_string(d.join("common_dict.txt")) {
            Ok(s) => parse_common_file(&s),
            Err(_) => builtin_common(),
        },
        None => builtin_common(),
    };
    dict.common_index.clear();
    dict.common_short_index.clear();
    for (rank, (pinyin_str, word)) in entries.iter().enumerate() {
        // 全拼前缀索引（与系统词库同构）
        for i in 1..=pinyin_str.len() {
            let prefix = &pinyin_str[..i];
            dict.common_index
                .entry(prefix.to_string())
                .or_default()
                .push((word.clone(), rank as u32, pinyin_str.len()));
        }
        // 声母索引（简拼 hd → 好的）
        let short = crate::pinyin::to_initial_string(pinyin_str);
        if !short.is_empty() {
            for i in 1..=short.len() {
                let prefix = &short[..i];
                dict.common_short_index
                    .entry(prefix.to_string())
                    .or_default()
                    .push((word.clone(), rank as u32, pinyin_str.len()));
            }
        }
    }
    // 每前缀按 rank（词表行序）升序——行序即优先级
    for entries in dict.common_index.values_mut() {
        entries.sort_by(|a, b| a.1.cmp(&b.1));
    }
    for entries in dict.common_short_index.values_mut() {
        entries.sort_by(|a, b| a.1.cmp(&b.1));
    }
    crate::log::info(&format!(
        "常用词表加载: {} 条 ({} 前缀 / {} 简拼前缀)",
        entries.len(),
        dict.common_index.len(),
        dict.common_short_index.len()
    ));
}

/// 解析 common_dict.txt：pinyin<TAB>word，行首 # 注释，空行忽略。
/// 兼容首行 UTF-8 BOM（Windows 记事本保存）。
fn parse_common_file(content: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in content.lines() {
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split('\t');
        let py = it.next().unwrap_or("").trim().to_lowercase();
        let w = it.next().unwrap_or("").trim();
        if !py.is_empty() && !w.is_empty() {
            out.push((py, w.to_string()));
        }
    }
    out
}

/// 内置常用词兜底表（V0.2.30）：common_dict.txt 缺失时降级。
/// 覆盖 Eric 指定的高频词 + 口语高频。
fn builtin_common() -> Vec<(String, String)> {
    vec![
        ("wo".into(), "我".into()),
        ("ni".into(), "你".into()),
        ("ta".into(), "他".into()),
        ("hao".into(), "好".into()),
        ("haode".into(), "好的".into()),
        ("zhege".into(), "这个".into()),
        ("name".into(), "那么".into()),
        ("en".into(), "嗯".into()),
        ("mei".into(), "没".into()),
        ("shi".into(), "是".into()),
        ("bu".into(), "不".into()),
        ("le".into(), "了".into()),
        ("zai".into(), "在".into()),
        ("you".into(), "有".into()),
        ("he".into(), "和".into()),
        ("jiu".into(), "就".into()),
        ("dou".into(), "都".into()),
        ("ye".into(), "也".into()),
        ("hen".into(), "很".into()),
        ("shuo".into(), "说".into()),
        ("de".into(), "的".into()),
        ("women".into(), "我们".into()),
        ("nimen".into(), "你们".into()),
        ("tamen".into(), "他们".into()),
        ("keyi".into(), "可以".into()),
        ("meiyou".into(), "没有".into()),
        ("shenme".into(), "什么".into()),
        ("zenme".into(), "怎么".into()),
        ("danshi".into(), "但是".into()),
        ("yinwei".into(), "因为".into()),
        ("suoyi".into(), "所以".into()),
        ("zhidao".into(), "知道".into()),
        ("xiexie".into(), "谢谢".into()),
    ]
}

/// 每个前缀索引最大词条数（V0.2.16 词库扩容 5.8 万 → 62 万后：
/// 单字母前缀可命中数万词条，query() 去重 O(n²) 卡死。
/// 候选页最多显示 max_pages(8) × page_size(9) = 72 个，截断 100 足够，
/// 且精确匹配词排在截断前列，不影响"精确拼音优先"语义。）
const MAX_PREFIX_ENTRIES: usize = 100;

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
            let (pinyin_str, word, frequency) = row.map_err(|e| format!("解析词条失败: {e}"))?;
            // 完整拼音索引（混合简拼用）
            full_index
                .entry(pinyin_str.clone())
                .or_default()
                .push((word.clone(), frequency));
            // 为每个可能的前缀建立全拼索引
            for i in 1..=pinyin_str.len() {
                let prefix = &pinyin_str[..i];
                index.entry(prefix.to_string()).or_default().push((
                    word.clone(),
                    frequency,
                    pinyin_str.len(),
                ));
            }
            // 建立简拼声母索引（zh→z, ch→c, sh→s，零声母取首字母）
            let short = pinyin::to_initial_string(&pinyin_str);
            if !short.is_empty() {
                for i in 1..=short.len() {
                    let prefix = &short[..i];
                    short_index.entry(prefix.to_string()).or_default().push((
                        word.clone(),
                        frequency,
                        pinyin_str.len(),
                    ));
                }
            }
        }

        // 每个前缀的候选按"精确拼音优先 + 词频降序"排序
        // V0.2.16：排序后截断到 MAX_PREFIX_ENTRIES——词库扩容后
        // 单字母前缀命中数万词条，不截断则 query() 去重 O(n²) 卡死
        for (prefix, entries) in index.iter_mut() {
            sort_by_exact_then_freq(entries, prefix.len());
            entries.truncate(MAX_PREFIX_ENTRIES);
        }
        for (prefix, entries) in short_index.iter_mut() {
            sort_by_exact_then_freq(entries, prefix.len());
            entries.truncate(MAX_PREFIX_ENTRIES);
        }
        // 完整拼音索引按词频降序 + 截断（混合简拼遍历用，每个完整拼音最多 100 词）
        for words in full_index.values_mut() {
            words.sort_by(|a, b| b.1.cmp(&a.1));
            words.truncate(MAX_PREFIX_ENTRIES);
        }

        let mut dict = Self {
            index,
            short_index,
            common_index: HashMap::new(),
            common_short_index: HashMap::new(),
            user_index: HashMap::new(),
            user_dict_path: None,
            full_index,
        };
        // V0.2.30：加载常用词表（common_dict.txt，与词库同目录；无文件用内置兜底）
        load_common(&mut dict, path.parent());
        Ok(dict)
    }

    /// 从预编译索引 .bin 加载（0.2.29）：bincode 反序列化，跳过 SQLite 全量重建。
    /// 加载耗时 ~1s（vs SQLite 6-7s），是切换输入法不卡的关键。
    fn from_bin(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("读取索引失败: {e}"))?;
        let mut d: Dictionary =
            bincode::deserialize(&bytes).map_err(|e| format!("反序列化失败: {e}"))?;
        // V0.2.30：.bin 不含 common 层（serde skip），运行期补加载常用词表
        load_common(&mut d, path.parent());
        crate::log::info(&format!(
            "预编译索引加载成功: {} 前缀 ({} 简拼前缀) {} 字节",
            d.index.len(),
            d.short_index.len(),
            bytes.len()
        ));
        Ok(d)
    }

    /// 序列化系统索引为 .bin（0.2.29 部署/首次缓存用）。
    /// 仅序列化 index/short_index/full_index（user_index 等运行时状态跳过）。
    fn to_bin(&self) -> Result<Vec<u8>, String> {
        bincode::serialize(self).map_err(|e| format!("序列化失败: {e}"))
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
                index.entry(prefix.to_string()).or_default().push((
                    entry.word.clone(),
                    entry.frequency,
                    entry.pinyin.len(),
                ));
            }
            let short = pinyin::to_initial_string(&entry.pinyin);
            if !short.is_empty() {
                for i in 1..=short.len() {
                    let prefix = &short[..i];
                    short_index.entry(prefix.to_string()).or_default().push((
                        entry.word.clone(),
                        entry.frequency,
                        entry.pinyin.len(),
                    ));
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

        let mut dict = Self {
            index,
            short_index,
            common_index: HashMap::new(),
            common_short_index: HashMap::new(),
            user_index: HashMap::new(),
            user_dict_path: None,
            full_index,
        };
        // V0.2.30：内置词库场景用内置常用词兜底表
        load_common(&mut dict, None);
        dict
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
                let mut stmt = match conn
                    .prepare("SELECT pinyin, word, frequency, last_used FROM user_dict")
                {
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
                        row.get::<_, i64>(3)?,
                    ))
                });
                if let Ok(rows) = rows {
                    for row in rows.flatten() {
                        self.add_user_entry(row.0, row.1, row.2, row.3);
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
        // 内存：频率 +1（V0.2.30 记录 last_used 供热度判定）
        let now = unix_now();
        self.add_user_entry(pinyin_str.to_string(), word.to_string(), 1, now);
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
    /// V0.2.30：记录 last_used（热度判定用），重复学习时频率累加 + last_used 取最新
    fn add_user_entry(&mut self, pinyin_str: String, word: String, frequency: u32, last_used: i64) {
        for i in 1..=pinyin_str.len() {
            let prefix = &pinyin_str[..i];
            let entries = self.user_index.entry(prefix.to_string()).or_default();
            if let Some(existing) = entries.iter_mut().find(|(w, _, _, _)| *w == word) {
                existing.1 = existing.1.saturating_add(frequency);
                existing.2 = existing.2.max(last_used);
            } else {
                entries.push((word.clone(), frequency, last_used, pinyin_str.len()));
            }
        }
        // 每前缀按"精确拼音优先 + 词频降序"排序
        for (prefix, entries) in self.user_index.iter_mut() {
            sort_by_exact_then_freq_user(entries, prefix.len());
        }
    }

    /// 删除用户词条（P2-1 Ctrl+Delete）：从 user_index 所有相关前缀移除该词。
    /// 磁盘同步删除（降权优先：若频率 >1 则 -1，否则删除词条）。
    pub fn remove_user_entry(&mut self, pinyin_str: &str, word: &str) {
        // 内存：所有相关前缀移除该词（或降频）
        for i in 1..=pinyin_str.len() {
            let prefix = &pinyin_str[..i];
            if let Some(entries) = self.user_index.get_mut(prefix) {
                if let Some(pos) = entries.iter().position(|(w, _, _, _)| w == word) {
                    if entries[pos].1 > 1 {
                        entries[pos].1 -= 1;
                    } else {
                        entries.remove(pos);
                    }
                }
            }
        }
        // 磁盘同步删除（静默失败——不阻塞输入）
        if let Some(path) = self.user_dict_path.clone() {
            if let Ok(conn) = Connection::open(&path) {
                let _ = conn.execute(
                    "DELETE FROM user_dict WHERE pinyin=? AND word=?",
                    rusqlite::params![pinyin_str, word],
                );
            }
        }
        crate::log::info(&format!("用户词删除: {word} ({pinyin_str})"));
    }

    /// 用户词条合并进系统候选：用户词插队（去重，位置提前）
    /// 由 query() 内部调用——user_index 的 key 是拼音前缀（add_user_entry 已展开），
    /// 与查询前缀同构，直接按 key 匹配即可。

    /// 全拼前缀查询候选词（系统词 + 用户词插队 + 常用词层）
    /// 排序（V0.2.30 升级）：精确匹配优先是**全局**规则，
    /// 层内再按「热用户词 > 常用词 > 温用户词 > 系统词」——
    /// 热用户词（7 天内 ≥3 次）压过常用词，温用户词（打过但不够热）只在常用词后插队。
    pub fn query(&self, pinyin_prefix: &str) -> Vec<String> {
        // P2-3：jqxy 后 v 归一为 u（qv→qu），兼容 ü 输入
        let key = crate::pinyin::normalize_v(&pinyin_prefix.to_lowercase());
        let key_len = key.len();
        let mut result: Vec<String> = Vec::new();
        let now = unix_now();

        // 辅助：去重追加（系统/常用词：3 元组）
        let push_entries3 = |result: &mut Vec<String>, entries: &[&(String, u32, usize)]| {
            for (w, _, _) in entries {
                if !result.contains(w) {
                    result.push(w.clone());
                }
            }
        };
        // 辅助：去重追加（用户词：4 元组）
        let push_entries4 = |result: &mut Vec<String>, entries: &[&(String, u32, i64, usize)]| {
            for (w, _, _, _) in entries {
                if !result.contains(w) {
                    result.push(w.clone());
                }
            }
        };

        // 第一层：精确匹配（pinyin == key）——热用户词 > 常用词 > 温用户词 > 系统词
        if let Some(user_entries) = self.user_index.get(&key) {
            let hot: Vec<&(String, u32, i64, usize)> = user_entries
                .iter()
                .filter(|e| e.3 == key_len && is_hot(e.1, e.2, now))
                .collect();
            push_entries4(&mut result, &hot);
        }
        if let Some(common_entries) = self.common_index.get(&key) {
            let exact: Vec<&(String, u32, usize)> =
                common_entries.iter().filter(|e| e.2 == key_len).collect();
            push_entries3(&mut result, &exact);
        }
        if let Some(user_entries) = self.user_index.get(&key) {
            let warm: Vec<&(String, u32, i64, usize)> = user_entries
                .iter()
                .filter(|e| e.3 == key_len && !is_hot(e.1, e.2, now))
                .collect();
            push_entries4(&mut result, &warm);
        }
        if let Some(entries) = self.index.get(&key) {
            let exact: Vec<&(String, u32, usize)> =
                entries.iter().filter(|e| e.2 == key_len).collect();
            push_entries3(&mut result, &exact);
        }
        // 第二层：前缀扩展（pinyin 长于 key）——热用户词 > 常用词 > 温用户词 > 系统词
        if let Some(user_entries) = self.user_index.get(&key) {
            let hot: Vec<&(String, u32, i64, usize)> = user_entries
                .iter()
                .filter(|e| e.3 != key_len && is_hot(e.1, e.2, now))
                .collect();
            push_entries4(&mut result, &hot);
        }
        if let Some(common_entries) = self.common_index.get(&key) {
            let rest: Vec<&(String, u32, usize)> =
                common_entries.iter().filter(|e| e.2 != key_len).collect();
            push_entries3(&mut result, &rest);
        }
        if let Some(user_entries) = self.user_index.get(&key) {
            let warm: Vec<&(String, u32, i64, usize)> = user_entries
                .iter()
                .filter(|e| e.3 != key_len && !is_hot(e.1, e.2, now))
                .collect();
            push_entries4(&mut result, &warm);
        }
        if let Some(entries) = self.index.get(&key) {
            let rest: Vec<&(String, u32, usize)> =
                entries.iter().filter(|e| e.2 != key_len).collect();
            push_entries3(&mut result, &rest);
        }
        result
    }

    /// 简拼声母前缀查询候选词（如 "zg" → 中国）
    /// V0.2.30：常用词声母命中优先（hd → 好的 在系统简拼候选前）
    pub fn query_short(&self, prefix: &str) -> Vec<String> {
        // P2-3：v 归一（简拼中 qv→qu 等）
        let key = crate::pinyin::normalize_v(&prefix.to_lowercase());
        let mut result: Vec<String> = Vec::new();
        if let Some(common) = self.common_short_index.get(&key) {
            for (w, _, _) in common {
                if !result.contains(w) {
                    result.push(w.clone());
                }
            }
        }
        if let Some(entries) = self.short_index.get(&key) {
            for (w, _, _) in entries {
                if !result.contains(w) {
                    result.push(w.clone());
                }
            }
        }
        result
    }

    /// 混合简拼查询（0.1.26）：输入串 = 完整音节前缀 + 声母后缀
    /// 例：shurf = shu(输) + r(入·声母) + f(法·声母) → 输入法（shurufa）
    /// 枚举切分点：prefix 必须能完整切分为音节（防 s/sh 过宽前缀），
    /// 匹配词拼音以 prefix 开头且剩余拼音的声母串以 suffix 开头。
    pub fn query_mixed(&self, input: &str) -> Vec<String> {
        // P2-3：v 归一（qv→qu 等）
        let input = crate::pinyin::normalize_v(&input.to_lowercase());
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
/// 已加载的系统词库路径（幂等判断用）。
/// TSF 每次切换输入法都会 Deactivate→Activate→engine_init，
/// 全量重载 62 万词条 + 重建前缀索引是切换卡顿根因（0.3.x 修复）。
static DICT_PATH: Mutex<Option<String>> = Mutex::new(None);
/// 已加载的用户词库路径（同上，避免每次激活重复读 SQLite）。
static USER_DICT_PATH: Mutex<Option<String>> = Mutex::new(None);
/// 大词库就绪标志（0.3.x 异步加载）：内置兜底=false，大词库换入=true。
/// 平台层可查询（engine_dict_ready）显示"加载中"状态。
static DICT_READY: AtomicBool = AtomicBool::new(false);

/// 大词库是否已就绪（异步加载完成）。未就绪时查询使用内置兜底词库。
pub fn is_ready() -> bool {
    DICT_READY.load(Ordering::SeqCst)
}

/// 尝试从给定路径加载词库，失败则回退到内置词库。
/// 幂等：词库已加载且路径一致 → 直接返回，不重建索引。
/// 0.3.x fix（切换卡顿）：改为异步——立即用内置词库兜底（查询不阻塞），
/// 后台线程加载大词库，完成后自动换入。ActivateEx 不再同步阻塞 1.6s。
pub fn init(dict_path: Option<&Path>) {
    let path_str = dict_path.map(|p| p.to_string_lossy().into_owned());
    // 幂等检查：词库已加载（内置或大词库）且路径相同 → 跳过
    // （Activate/Deactivate 反复切换不重载、不重复启动后台线程）
    {
        let loaded = DICT.lock().unwrap_or_else(|e| e.into_inner()).is_some();
        let cur = DICT_PATH.lock().unwrap_or_else(|e| e.into_inner());
        if loaded && *cur == path_str {
            return;
        }
    }

    // 路径变化 → 立即切到内置词库兜底（瞬间可用，查询永远不阻塞），
    // 后台线程随后加载新路径的大词库。
    {
        let mut dict = DICT.lock().unwrap_or_else(|e| e.into_inner());
        *dict = Some(Dictionary::from_builtin());
        DICT_READY.store(false, Ordering::SeqCst);
    }
    *DICT_PATH.lock().unwrap_or_else(|e| e.into_inner()) = path_str.clone();

    // 有词库路径 → 后台线程加载大词库（加载完自动换入，UI 不卡）
    if path_str.is_some() {
        std::thread::spawn(move || load_dict_async(path_str));
    } else {
        crate::log::info("词库：无路径，使用内置词库");
    }
}

/// 后台线程：加载大词库（.bin 优先，无缓存则 SQLite 全量并写缓存）。
/// 加载期间查询使用内置兜底词库；完成后一次性换入（Mutex 保护，不阻塞查询）。
fn load_dict_async(path_str: Option<String>) {
    let loaded: Option<Dictionary> = match path_str.as_deref() {
        Some(path) => {
            let path = std::path::Path::new(path);
            // 优先预编译索引 .bin（dict_path + ".bin"，如 system_dict.db.bin）
            let bin_path = std::path::PathBuf::from(format!("{}.bin", path.display()));
            if let Ok(d) = Dictionary::from_bin(&bin_path) {
                Some(d)
            } else if let Ok(d) = Dictionary::from_sqlite(path) {
                // 无 .bin 缓存 → SQLite 全量加载，成功后写 .bin 供下次秒加载
                crate::log::info(&format!(
                    "词库 SQLite 加载成功: {} 前缀 ({} 简拼前缀)，写缓存",
                    d.index.len(),
                    d.short_index.len()
                ));
                match d.to_bin() {
                    Ok(bytes) => {
                        if let Err(e) = std::fs::write(&bin_path, bytes) {
                            crate::log::error(&format!("索引缓存写盘失败: {e}"));
                        } else {
                            crate::log::info(&format!("预编译索引已缓存: {}", bin_path.display()));
                        }
                    }
                    Err(e) => crate::log::error(&format!("索引序列化失败: {e}")),
                }
                Some(d)
            } else {
                crate::log::error(&format!("词库加载失败: {}，保持内置词库", path.display()));
                None
            }
        }
        None => None,
    };
    if let Some(d) = loaded {
        let mut dict = DICT.lock().unwrap_or_else(|e| e.into_inner());
        *dict = Some(d);
        DICT_READY.store(true, Ordering::SeqCst);
        crate::log::info("大词库加载完成，已切换（异步后台）");
    }
}

/// 同步加载（测试/部署工具用）：等待词库加载完成再返回。
/// 运行时路径请用 init()（异步）。
pub fn init_blocking(dict_path: Option<&Path>) {
    let path_str = dict_path.map(|p| p.to_string_lossy().into_owned());
    // 幂等检查：词库已加载且路径相同 → 跳过（Activate/Deactivate 反复切换不重载）
    {
        let loaded = DICT.lock().unwrap_or_else(|e| e.into_inner()).is_some();
        let cur = DICT_PATH.lock().unwrap_or_else(|e| e.into_inner());
        if loaded && *cur == path_str {
            return;
        }
    }

    let mut dict = DICT.lock().unwrap_or_else(|e| e.into_inner());

    if let Some(path) = dict_path {
        // 0.2.29：优先加载预编译索引 .bin（dict_path + ".bin"，如 system_dict.db.bin）
        // 部署期/首次缓存生成，运行时秒加载（vs SQLite 全量重建 6-7s）
        let bin_path = PathBuf::from(format!("{}.bin", path.display()));
        if let Ok(d) = Dictionary::from_bin(&bin_path) {
            *dict = Some(d);
            DICT_READY.store(true, Ordering::SeqCst);
            *DICT_PATH.lock().unwrap_or_else(|e| e.into_inner()) = path_str;
            return;
        }
        // 无 .bin 缓存 → SQLite 全量加载，成功后写 .bin 供下次秒加载
        if let Ok(d) = Dictionary::from_sqlite(path) {
            let count = d.index.len();
            crate::log::info(&format!(
                "词库加载成功: {} 前缀 ({} 简拼前缀)",
                count,
                d.short_index.len()
            ));
            // 写 .bin 缓存（失败不阻断——下次再试）
            match d.to_bin() {
                Ok(bytes) => {
                    if let Err(e) = std::fs::write(&bin_path, bytes) {
                        crate::log::error(&format!("索引缓存写盘失败: {e}"));
                    } else {
                        crate::log::info(&format!("预编译索引已缓存: {}", bin_path.display()));
                    }
                }
                Err(e) => crate::log::error(&format!("索引序列化失败: {e}")),
            }
            *dict = Some(d);
            DICT_READY.store(true, Ordering::SeqCst);
            *DICT_PATH.lock().unwrap_or_else(|e| e.into_inner()) = path_str;
            return;
        }
        // SQLite 加载失败——回退到内置词库
        crate::log::error(&format!("词库加载失败: {}，降级内置词库", path.display()));
    }

    // 回退到内置词库
    *dict = Some(Dictionary::from_builtin());
    DICT_READY.store(false, Ordering::SeqCst);
    *DICT_PATH.lock().unwrap_or_else(|e| e.into_inner()) = path_str;
    crate::log::info("词库降级：使用内置词库");
}

/// 预编译索引构建（0.2.29 部署工具）：从 SQLite 词库构建 .bin 索引文件。
/// 部署期调用一次，运行时 engine_init 直接加载 .bin（秒开）。
pub fn build_index(dict_path: &Path, out_bin: &Path) -> Result<(), String> {
    let d = Dictionary::from_sqlite(dict_path)?;
    let bytes = d.to_bin()?;
    std::fs::write(out_bin, bytes).map_err(|e| format!("写盘失败: {e}"))?;
    crate::log::info(&format!("预编译索引构建完成: {}", out_bin.display()));
    Ok(())
}

/// 设置用户词库路径（V0.2.2）。NULL/空 = 禁用用户词库。
/// 需在 init 之后调用（Dictionary 实例已存在）。
/// 幂等：路径未变化且词库已加载 → 跳过（避免切换输入法重复读 SQLite）。
pub fn set_user_dict_path(path: Option<&Path>) {
    let path_str = path.map(|p| p.to_string_lossy().into_owned());
    // 幂等检查：词库已加载且用户词库路径未变 → 跳过
    {
        let loaded = DICT.lock().unwrap_or_else(|e| e.into_inner()).is_some();
        let cur = USER_DICT_PATH.lock().unwrap_or_else(|e| e.into_inner());
        if loaded && *cur == path_str {
            return;
        }
    }

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
    *USER_DICT_PATH.lock().unwrap_or_else(|e| e.into_inner()) = path_str;
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
            DICT.lock()
                .unwrap()
                .as_mut()
                .unwrap()
                .learn_user_word(pinyin_str, word);
        }
    }
}

/// 删除用户词（P2-1 Ctrl+Delete）：从内存 user_index + 磁盘移除。
pub fn remove_user_word(pinyin_str: &str, word: &str) {
    let mut dict = DICT.lock().unwrap_or_else(|e| e.into_inner());
    match dict.as_mut() {
        Some(d) => d.remove_user_entry(pinyin_str, word),
        None => {}
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
            DICT.lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .phrase_guess(pinyin_str)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_fallback() {
        init_blocking(None);
        let results = query("zhong");
        assert!(results.iter().any(|w| w == "中"));
        assert!(results.iter().any(|w| w == "中国"));
    }

    #[test]
    fn test_query_empty() {
        init_blocking(None);
        let results = query("zzz");
        assert!(results.is_empty());
    }

    #[test]
    fn test_sqlite_load() {
        let db_path = Path::new("../../resources/system_dict.db");
        if db_path.exists() {
            init_blocking(Some(db_path));
            let results = query("zhong");
            assert!(results.iter().any(|w| w == "中"));
            assert!(results.len() >= 2);
        }
    }

    #[test]
    fn test_sqlite_short() {
        let db_path = Path::new("../../resources/system_dict.db");
        if db_path.exists() {
            init_blocking(Some(db_path));
            // 简拼：zg → 中国（真实 4060 条词库）
            let results = query_short("zg");
            assert!(
                results.iter().any(|w| w == "中国"),
                "SQLite 词库简拼 zg 应命中中国, got {:?}",
                results
            );
        }
    }

    #[test]
    fn test_short_query() {
        init_blocking(None);
        // 中国 → 简拼 zg
        let results = query_short("zg");
        assert!(results.iter().any(|w| w == "中国"));
    }

    #[test]
    fn test_phrase_guess_builtin() {
        init_blocking(None);
        // 你好世界：nihao 有词条，shijie 可能没有 → 不保证成功，仅验证不崩溃
        let _ = phrase_guess("nihaoshijie");
    }

    // ─── 0.1.26 精确拼音优先测试（单字不被词组淹没）───

    #[test]
    fn test_exact_pinyin_priority_builtin() {
        // 内置词库：wo=我(680), women=我们(450)。精确拼音优先 → wo 先出"我"
        init_blocking(None);
        let results = query("wo");
        assert_eq!(
            results.first().map(String::as_str),
            Some("我"),
            "内置词库 wo 应优先出单字 我, got: {:?}",
            results.iter().take(5).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_exact_pinyin_priority_sqlite() {
        // 真实词库：wo 前缀下"我们"(3908) 词频高于"我"，但精确优先应让"我"排第一
        let db_path = Path::new("../../resources/system_dict.db");
        if db_path.exists() {
            init_blocking(Some(db_path));
            let results = query("wo");
            assert_eq!(
                results.first().map(String::as_str),
                Some("我"),
                "SQLite 词库 wo 应优先出单字 我, got: {:?}",
                results.iter().take(5).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn test_exact_pinyin_priority_long_word_still_available() {
        // 精确优先不丢长词：women 仍应在前缀候选里（排在"我"之后）
        init_blocking(None);
        let results = query("wo");
        assert!(
            results.iter().any(|w| w == "我们"),
            "长词 我们 不应丢失, got: {:?}",
            results.iter().take(10).collect::<Vec<_>>()
        );
    }

    // ─── 0.1.26 混合简拼测试（shurf → 输入法）───

    #[test]
    fn test_mixed_shurf_inputfa() {
        // 真实词库：shurf = shu + rf → shurufa（输入法）
        let db_path = Path::new("../../resources/system_dict.db");
        if db_path.exists() {
            init_blocking(Some(db_path));
            let results = query_mixed("shurf");
            assert!(
                results.iter().any(|w| w == "输入法"),
                "shurf 应联想出 输入法, got: {:?}",
                results
            );
        }
    }

    #[test]
    fn test_mixed_full_prefix_still_matches() {
        // shuruf（完整拼音前缀）混合匹配也应出输入法
        let db_path = Path::new("../../resources/system_dict.db");
        if db_path.exists() {
            init_blocking(Some(db_path));
            let results = query_mixed("shuruf");
            assert!(
                results.iter().any(|w| w == "输入法"),
                "shuruf 应联想出 输入法, got: {:?}",
                results
            );
        }
    }

    #[test]
    fn test_mixed_too_short_no_result() {
        // 输入过短（<3）不触发混合简拼
        init_blocking(None);
        assert!(query_mixed("wo").is_empty(), "wo 不应走混合简拼");
    }

    #[test]
    fn test_mixed_builtin_available() {
        // 内置词库：nihaosj？无此词。验证混合简拼对内置词库不崩溃
        init_blocking(None);
        let _ = query_mixed("zhongg"); // zhong + g → 中国(zhongguo)
        // 内置词库有 zhongguo → 应出 中国
        let results = query_mixed("zhongg");
        assert!(
            results.iter().any(|w| w == "中国"),
            "zhongg 应联想出 中国(内置词库), got: {:?}",
            results
        );
    }

    // ─── P2-3 ü 兼容测试 ───

    #[test]
    fn test_normalize_v() {
        assert_eq!(crate::pinyin::normalize_v("qv"), "qu");
        assert_eq!(crate::pinyin::normalize_v("jv"), "ju");
        assert_eq!(crate::pinyin::normalize_v("xvqu"), "xuqu");
        assert_eq!(crate::pinyin::normalize_v("nv"), "nv"); // n 后不转（nü 保留）
        assert_eq!(crate::pinyin::normalize_v("vip"), "vip"); // 开头 v 不转
    }

    #[test]
    fn test_query_v_normalized() {
        // 真实词库：qv → qu → 去/取/曲 应命中（词库按 u 注音）
        let db_path = Path::new("../../resources/system_dict.db");
        if db_path.exists() {
            init_blocking(Some(db_path));
            let results = query("qv");
            assert!(
                results.iter().any(|w| w == "去"),
                "qv 应归一为 qu 命中 去, got: {:?}",
                results.iter().take(5).collect::<Vec<_>>()
            );
        }
    }

    // ─── 0.1.26 修复 v2：用户词不压过精确单字 ───

    #[test]
    fn test_user_word_not_shadow_exact() {
        // 用户学习词组"我们"(women, 非精确) 后，打 wo 单字"我"(精确) 仍应优先
        let mut dict = Dictionary::from_builtin();
        dict.add_user_entry("women".to_string(), "我们".to_string(), 99, unix_now());
        let results = dict.query("wo");
        assert_eq!(
            results.first().map(String::as_str),
            Some("我"),
            "用户词 我们(99) 不应压过精确单字 我, got: {:?}",
            results.iter().take(5).collect::<Vec<_>>()
        );
        // 用户词仍在候选（第二层，不丢失）
        assert!(
            results.iter().any(|w| w == "我们"),
            "用户词 我们 应仍在候选, got: {:?}",
            results.iter().take(10).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_user_exact_still_prioritized() {
        // 用户学精确词"我"（频率高于系统）→ 用户 exact 仍应优先（同层插队保留）
        let mut dict = Dictionary::from_builtin();
        dict.add_user_entry("wo".to_string(), "我".to_string(), 1000, unix_now());
        let results = dict.query("wo");
        assert_eq!(
            results.first().map(String::as_str),
            Some("我"),
            "用户精确词 我 应优先, got: {:?}",
            results.iter().take(5).collect::<Vec<_>>()
        );
    }

    // ─── V0.2.30 常用词层 + 用户词热度学习测试 ───

    /// 构造内置词库 + 真实 common_dict.txt（覆盖内置兜底）
    fn dict_with_common() -> Dictionary {
        let mut d = Dictionary::from_builtin();
        let res_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("resources");
        load_common(&mut d, Some(&res_dir));
        d
    }

    #[test]
    fn test_common_en_first() {
        // 核心痛点：en → 「嗯」必须首位（此前被「奀」压住）
        let d = dict_with_common();
        let r = d.query("en");
        assert_eq!(
            r.first().map(String::as_str),
            Some("嗯"),
            "en 首位应为 嗯, got: {:?}",
            r.iter().take(6).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_common_wo_first() {
        let d = dict_with_common();
        let r = d.query("wo");
        assert_eq!(r.first().map(String::as_str), Some("我"));
    }

    #[test]
    fn test_common_haode_first() {
        let d = dict_with_common();
        let r = d.query("haode");
        assert_eq!(r.first().map(String::as_str), Some("好的"));
    }

    #[test]
    fn test_common_zhege_name_mei() {
        let d = dict_with_common();
        assert_eq!(d.query("zhege").first().map(String::as_str), Some("这个"));
        assert_eq!(d.query("name").first().map(String::as_str), Some("那么"));
        assert_eq!(d.query("mei").first().map(String::as_str), Some("没"));
    }

    #[test]
    fn test_common_short_hd() {
        // 简拼 hd → 「好的」应在系统简拼候选前
        let d = dict_with_common();
        let r = d.query_short("hd");
        assert_eq!(
            r.first().map(String::as_str),
            Some("好的"),
            "简拼 hd 首位应为 好的, got: {:?}",
            r.iter().take(5).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_user_warm_not_hot() {
        // 学一次「恩」→ 温词，en 首位仍「嗯」，「恩」只插在常用词后
        let mut d = dict_with_common();
        d.add_user_entry("en".to_string(), "恩".to_string(), 1, unix_now());
        let r = d.query("en");
        assert_eq!(
            r.first().map(String::as_str),
            Some("嗯"),
            "温词不应压过常用词"
        );
        let pos = r.iter().position(|w| w == "恩").expect("温词 恩 应在候选");
        assert!(
            pos == 1,
            "恩 应紧跟 嗯 后, got {:?}",
            r.iter().take(4).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_user_hot_override() {
        // 7 天内 3 次 → 热词，en 首位变「恩」
        let mut d = dict_with_common();
        d.add_user_entry("en".to_string(), "恩".to_string(), 3, unix_now());
        let r = d.query("en");
        assert_eq!(
            r.first().map(String::as_str),
            Some("恩"),
            "热词应压过常用词"
        );
        assert_eq!(r.get(1).map(String::as_str), Some("嗯"));
    }

    #[test]
    fn test_user_cool_down() {
        // 3 次但 8 天前 → 超窗口降温，en 首位回「嗯」
        let mut d = dict_with_common();
        let old = unix_now() - 8 * 24 * 3600;
        d.add_user_entry("en".to_string(), "恩".to_string(), 3, old);
        let r = d.query("en");
        assert_eq!(r.first().map(String::as_str), Some("嗯"), "超窗口应降温");
    }

    #[test]
    fn test_parse_common_file_real() {
        let res_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("resources");
        let content = std::fs::read_to_string(res_dir.join("common_dict.txt"))
            .expect("common_dict.txt 应存在");
        let entries = parse_common_file(&content);
        assert!(
            entries.len() > 100,
            "常用词表应 >100 条, got {}",
            entries.len()
        );
        assert!(entries.iter().any(|(p, w)| p == "en" && w == "嗯"));
        assert!(entries.iter().any(|(p, w)| p == "wo" && w == "我"));
    }

    #[test]
    fn test_bin_compat_common_loaded() {
        // 旧 .bin（无 common 字段）应能反序列化 + 运行期补加载常用词
        let bin = Path::new("../../resources/system_dict.db.bin");
        if bin.exists() {
            let d = Dictionary::from_bin(bin).expect("旧 .bin 应可反序列化");
            assert!(!d.common_index.is_empty(), "common 索引应已加载");
            let r = d.query("en");
            assert_eq!(
                r.first().map(String::as_str),
                Some("嗯"),
                "en 首位应是 嗯 (真实词库), got {:?}",
                r.iter().take(4).collect::<Vec<_>>()
            );
        }
    }
}
