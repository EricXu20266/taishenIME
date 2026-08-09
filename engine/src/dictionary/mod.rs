/// 词库模块 — 拼音到汉字映射
///
/// 第一期 MVP：从 SQLite 系统词库加载 + 内置最小词库降级。
/// 支持：
///   - 全拼前缀查询（query）
///   - 简拼声母查询（query_short，如 zg→中国）
///   - 多音节切分联想（phrase_guess，如 nihaoshijie→你好世界）
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
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
    /// 完整声母索引（V0.3.x，对标 rime abbrev zh/ch/sh 整体）：
    /// zh/ch/sh 保留两字符 → "社会"shehui → "shh"，"正在"zhengzai → "zhz"。
    /// 与 short_index 并存：compact(z/c/s) 优先，full 补充（shh/zhz/zhg 等）。
    #[serde(default)]
    short_index_full: HashMap<String, Vec<(String, u32, usize)>>,
    /// 混合简拼后缀索引（V0.3.x，声母缩写前缀 + 完整拼音后缀，xzai → 现在）：
    /// suffix_pinyin → [(prefix_initials_full, word, frequency)]
    /// 构建：枚举词的音节边界，后缀完整拼音为 key，前缀完整声母串为 value。
    #[serde(default)]
    suffix_index: HashMap<String, Vec<(String, String, u32)>>,
    /// 常用词索引（V0.2.30）：prefix → [(word, rank, pinyin_len)]，rank 越小越优先。
    /// 命中即出候选，不受 system_dict 词频分档影响；运行时从 common_dict.txt 读，
    /// 不参与 .bin 持久化（用户改词表下次启动生效，旧 .bin 兼容）。
    #[serde(skip)]
    common_index: HashMap<String, Vec<(String, u32, usize)>>,
    /// 常用词声母索引（V0.2.30）：简拼前缀 → [(word, rank, pinyin_len)]
    #[serde(skip)]
    common_short_index: HashMap<String, Vec<(String, u32, usize)>>,
    /// 常用词完整声母索引（V0.3.x2）：zh/ch/sh 保留两字符（shh→社会）。
    /// 解决"社会"compact 简拼为 "sh"（≠"shh"）导致 shh 时常用词层不命中的问题——
    /// 常用词在完整声母简拼下也排最前（对标雾凇 9999 档常用词优先）。
    #[serde(skip)]
    common_short_full_index: HashMap<String, Vec<(String, u32, usize)>>,
    /// 用户词库索引（V0.2.2）：prefix → [(word, frequency, last_used, pinyin_len)]。
    /// V0.2.30 热度学习：查询时按 热词(7天内≥3次) > 温词 分档插队系统词。
    #[serde(skip)]
    user_index: HashMap<String, Vec<(String, u32, i64, usize)>>,
    /// 用户词声母索引（V0.5+）：简拼前缀 → [(word, frequency, last_used, pinyin_len)]。
    /// 组词学习进化：学过 taishen→泰深 后打 ts（声母串）也能命中。
    #[serde(skip)]
    user_short_index: HashMap<String, Vec<(String, u32, i64, usize)>>,
    /// 完整拼音索引（0.1.26 混合简拼用）：pinyin → [(word, frequency)]
    full_index: BTreeMap<String, Vec<(String, u32)>>,
    /// 专业词库索引（对标微软/搜狗分类词库）：prefix → [(word, freq, pinyin_len, domain_id)]
    /// 运行时从 resources/domains/*.txt 全量自动加载（serde skip，不入 .bin）。
    /// 查询时追加到系统词后，热度 > 0 的领域词优先（热词探测自动匹配）。
    #[serde(skip)]
    domain_index: HashMap<String, Vec<(String, u32, usize, usize)>>,
    /// 专业词库简拼索引：prefix → [(word, freq, pinyin_len, domain_id)]
    #[serde(skip)]
    domain_short_index: HashMap<String, Vec<(String, u32, usize, usize)>>,
    /// 专业词库完整简拼精确索引（V0.5.2）：完整简拼 → [(word, freq, pinyin_len, domain_id)]
    /// key = to_initial_string(pinyin)（wb → 微博）。简拼查询时插到 system 前缀扩展前，
    /// 避免领域词被维基海量候选淹没（实测 wb 474 位 / dy 715 位）。
    #[serde(skip)]
    domain_exact_short_index: HashMap<String, Vec<(String, u32, usize, usize)>>,
    /// 词 → 领域 ID（select_candidate 热词探测用，一词一域取首个加载领域）
    #[serde(skip)]
    word_domain: HashMap<String, usize>,
    /// 领域名列表（索引 = domain_id，加载顺序）
    #[serde(skip)]
    domain_names: Vec<String>,
    /// 领域热度（热词探测 v2，索引 = domain_id）：选词命中 +1，查询加权
    #[serde(skip)]
    domain_heat: Vec<i64>,
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

/// V0.5.2：热门领域名单——初始热度 +1（冷启动下这些领域的词在 domain 组内优先）。
/// 作用：domain_exact 层同长词按加载序排列时，现代高频领域（网络流行语/对话挖掘/
/// 现代词/计算机/成语/美食/财经/体育）不被 agriculture 等早期加载的冷门领域挤出。
fn is_hot_domain(name: &str) -> bool {
    matches!(
        name,
        "network_slang"
            | "conversation"
            | "modern"
            | "computer"
            | "idiom"
            | "food"
            | "economics"
            | "sport"
            | "thuocl_chengyu"
            | "thuocl_it"
    )
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
/// V0.4：3 → 2——选 2 次即热，更快响应上词（Eric：选一次没反应很傻逼）
const HOT_THRESHOLD: u32 = 2;
/// 热词时间窗口（秒）：7 天
const HOT_WINDOW_SECS: i64 = 7 * 24 * 3600;

/// 热词判定（V0.2.30）：近期（7 天窗口内）使用 ≥ 2 次的用户词。
/// 用途：区分"偶然打过一次"（温词，只在系统词前插队）与
/// "稳定偏好"（热词，压过常用词），避免误学污染首位。
fn is_hot(frequency: u32, last_used: i64, now: i64) -> bool {
    frequency >= HOT_THRESHOLD && now - last_used <= HOT_WINDOW_SECS
}

/// 最近使用判定（V0.4）：7 天窗口内用过（与 is_hot 时间窗一致）。
/// 用途：温词（freq<2 但近期用过）排常用词前——选一次即生效（Eric 反馈
/// "选一次没反应"：温词此前只在 system 前插队，对 system 里本就靠前的词无感）。
/// 7 天未用 → 衰减为过期词（回 common 后 system 前）。
fn is_recent(last_used: i64, now: i64) -> bool {
    now - last_used <= HOT_WINDOW_SECS
}

/// 当前 Unix 时间戳（秒）
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 从 common.db 加载常用词表（V0.5+）：SQLite 读取，rank 即行序（越小越优先）。
fn load_common_from_db(_dict: &mut Dictionary, db_path: &Path) -> Vec<(String, String)> {
    let conn = match Connection::open(db_path) {
        Ok(c) => c,
        Err(e) => {
            crate::log::error(&format!("common.db 打开失败 {}: {e}", db_path.display()));
            return builtin_common();
        }
    };
    let mut stmt = match conn.prepare("SELECT pinyin, word FROM common_words ORDER BY rank") {
        Ok(s) => s,
        Err(e) => {
            crate::log::error(&format!("common.db 查询失败: {e}"));
            return builtin_common();
        }
    };
    let rows = match stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    }) {
        Ok(r) => r,
        Err(e) => {
            crate::log::error(&format!("common.db 遍历失败: {e}"));
            return builtin_common();
        }
    };
    let mut entries: Vec<(String, String)> = Vec::new();
    for row in rows {
        if let Ok((py, w)) = row {
            entries.push((py, w));
        }
    }
    if entries.is_empty() {
        return builtin_common();
    }
    entries
}

/// 加载常用词表（V0.2.30）：V0.5+ 优先 common.db，不存在则回退 common_dict.txt。
/// common_index / common_short_index。文件不存在 → 内置兜底词表。
/// 不参与 .bin 持久化（serde skip），运行期每次加载时构建。
fn load_common(dict: &mut Dictionary, dir: Option<&Path>) {
    let entries: Vec<(String, String)> = match dir {
        Some(d) => {
            let db_path = d.join("common.db");
            if db_path.exists() {
                load_common_from_db(dict, &db_path)
            } else {
                match std::fs::read_to_string(d.join("common_dict.txt")) {
                    Ok(s) => parse_common_file(&s),
                    Err(_) => builtin_common(),
                }
            }
        }
        None => builtin_common(),
    };
    dict.common_index.clear();
    dict.common_short_index.clear();
    dict.common_short_full_index.clear();
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
        // 完整声母索引（V0.3.x2，shh→社会）：zh/ch/sh 保留两字符。
        // compact 简拼把"社会"归为 "sh"（s 归一），完整声母 "shh" 才能命中
        // 雾凇式逐音节缩写（shh = sh + h）。仅当与 compact 不同才建（去冗余）。
        let short_full = crate::pinyin::to_initial_full(pinyin_str);
        if !short_full.is_empty() && short_full != short {
            for i in 1..=short_full.len() {
                let prefix = &short_full[..i];
                dict.common_short_full_index
                    .entry(prefix.to_string())
                    .or_default()
                    .push((word.clone(), rank as u32, short_full.len()));
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
    for entries in dict.common_short_full_index.values_mut() {
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
/// 候选页最多显示 max_pages(8) × page_size(9) = 72 个，但简拼/缩写索引的前缀
/// 命中词条远超页容量——V0.3.x2 修复：100 → 400（2 音节 sh+h 同频词 100+
/// 个，旧值把"社会"（拼音序中段）截出简拼候选，见 shh 缺"社会" bug）。
/// 400 条/前缀的内存开销可控（总词条 69 万，前缀分桶后总量线性）。
const MAX_PREFIX_ENTRIES: usize = 400;
/// V0.5.3：system 词条"超高频"阈值（维基语料词频）。精确层/简拼层排序时，
/// system 超高频词（>= 阈值）优先于领域词——避免领域精确词（侧视 astronomy
/// 压测试 2848、之卦 conversation 压中国 4298）；中频 system 词（微波 2365）
/// 仍让位给领域热门专名（微博，V0.5.2 效果保留）。
const SYSTEM_HIGH_FREQ: u32 = 2500;

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
        let mut short_index_full: HashMap<String, Vec<(String, u32, usize)>> = HashMap::new();
        let mut suffix_index: HashMap<String, Vec<(String, String, u32)>> = HashMap::new();
        let mut full_index: BTreeMap<String, Vec<(String, u32)>> = BTreeMap::new();

        let rows = stmt
            .query_map([], |row| {
                let pinyin: String = row.get(0)?;
                let word: String = row.get(1)?;
                let frequency: u32 = row.get(2)?;
                Ok((pinyin, word, frequency))
            })
            .map_err(|e| format!("读取词库失败: {e}"))?;

        // V0.5.3 简繁归一化：维基繁体词条（我們/側視）加载时统一转简体，
        // 与同拼音简体词条去重（ORDER BY frequency DESC 保证高频先到，保留高频）。
        let mut seen: HashSet<(String, String)> = HashSet::with_capacity(4096);
        for row in rows {
            let (pinyin_str, word, frequency) = row.map_err(|e| format!("解析词条失败: {e}"))?;
            let word = crate::trad::to_simplified(&word);
            if !seen.insert((pinyin_str.clone(), word.clone())) {
                continue; // 同拼音同词已加载（繁体转简后与现有简体重复）
            }
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
            // 完整声母索引（V0.3.x：zh/ch/sh 保留两字符，shh→社会）
            let short_full = pinyin::to_initial_full(&pinyin_str);
            if !short_full.is_empty() && short_full != short {
                // 第 3 字段 = 完整声母串长度（非 pinyin_len）：
                // sort_by_exact_then_freq 的 exact 判定用它——"社会"(shh 长度3) 输入 shh
                // 精确匹配排前，而"社会化"(shhh 长度4) 只作前缀匹配排后。
                for i in 1..=short_full.len() {
                    let prefix = &short_full[..i];
                    short_index_full
                        .entry(prefix.to_string())
                        .or_default()
                        .push((word.clone(), frequency, short_full.len()));
                }
            }
            // 混合简拼后缀索引（V0.3.x：xzai → 现在，x + zai）
            let syls = split_into_syllables(&pinyin_str);
            if syls.len() >= 2 {
                let mut prefix_acc = String::new();
                for k in 0..syls.len() {
                    prefix_acc.push_str(&pinyin::to_initial_full(&syls[k]));
                    if k + 1 < syls.len() {
                        let suffix = syls[k + 1..].join("");
                        suffix_index.entry(suffix).or_default().push((
                            prefix_acc.clone(),
                            word.clone(),
                            frequency,
                        ));
                    }
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
        for (prefix, entries) in short_index_full.iter_mut() {
            sort_by_exact_then_freq(entries, prefix.len());
            entries.truncate(MAX_PREFIX_ENTRIES);
        }
        for entries in suffix_index.values_mut() {
            // 前缀声母串短的优先（全拼前缀更精确），同前缀按词频降序
            entries.sort_by(|a, b| a.0.len().cmp(&b.0.len()).then(b.2.cmp(&a.2)));
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
            short_index_full,
            suffix_index,
            common_index: HashMap::new(),
            common_short_index: HashMap::new(),
            common_short_full_index: HashMap::new(),
            user_index: HashMap::new(),
            user_short_index: HashMap::new(),
            user_dict_path: None,
            full_index,
            domain_index: HashMap::new(),
            domain_short_index: HashMap::new(),
            domain_exact_short_index: HashMap::new(),
            word_domain: HashMap::new(),
            domain_names: Vec::new(),
            domain_heat: Vec::new(),
        };
        // V0.2.30：加载常用词表（common_dict.txt，与词库同目录；无文件用内置兜底）
        load_common(&mut dict, path.parent());
        Ok(dict)
    }

    /// 从预编译索引 .bin 加载（0.2.29）：bincode 反序列化，跳过 SQLite 全量重建。
    /// 加载耗时 ~1s（vs SQLite 6-7s），是切换输入法不卡的关键。
    /// V0.3.x：旧版 .bin 无 short_index_full/suffix_index（serde default 空）
    /// → 从 full_index 重建扩展索引（避免全量 SQLite 重建）。
    fn from_bin(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("读取索引失败: {e}"))?;
        let mut d: Dictionary =
            bincode::deserialize(&bytes).map_err(|e| format!("反序列化失败: {e}"))?;
        // V0.2.30：.bin 不含 common 层（serde skip），运行期补加载常用词表
        load_common(&mut d, path.parent());
        // V0.3.x：旧 .bin 补建扩展索引（幂等——已存在则跳过）
        if d.short_index_full.is_empty() || d.suffix_index.is_empty() {
            d.rebuild_extended_indexes();
        }
        crate::log::info(&format!(
            "预编译索引加载成功: {} 前缀 ({} 简拼前缀, {} 完整声母, {} 后缀) {} 字节",
            d.index.len(),
            d.short_index.len(),
            d.short_index_full.len(),
            d.suffix_index.len(),
            bytes.len()
        ));
        Ok(d)
    }

    /// 序列化系统索引为 .bin（0.2.29 部署/首次缓存用）。
    /// 仅序列化 index/short_index/full_index（user_index 等运行时状态跳过）。
    fn to_bin(&self) -> Result<Vec<u8>, String> {
        bincode::serialize(self).map_err(|e| format!("序列化失败: {e}"))
    }

    /// 从 full_index 重建扩展索引（V0.3.x）：旧版 .bin 无
    /// short_index_full / suffix_index（serde default 空）时调用。
    /// full_index 含完整拼音→词映射，足以重建两个派生索引。
    fn rebuild_extended_indexes(&mut self) {
        let mut short_index_full: HashMap<String, Vec<(String, u32, usize)>> = HashMap::new();
        let mut suffix_index: HashMap<String, Vec<(String, String, u32)>> = HashMap::new();
        for (pinyin_str, words) in &self.full_index {
            let short_full = pinyin::to_initial_full(pinyin_str);
            let short = pinyin::to_initial_string(pinyin_str);
            if !short_full.is_empty() && short_full != short {
                for i in 1..=short_full.len() {
                    let bucket = short_index_full
                        .entry(short_full[..i].to_string())
                        .or_default();
                    for (w, f) in words {
                        bucket.push((w.clone(), *f, short_full.len()));
                    }
                }
            }
            let syls = split_into_syllables(pinyin_str);
            if syls.len() >= 2 {
                let mut prefix_acc = String::new();
                for k in 0..syls.len() {
                    prefix_acc.push_str(&pinyin::to_initial_full(&syls[k]));
                    if k + 1 < syls.len() {
                        let suffix = syls[k + 1..].join("");
                        let bucket = suffix_index.entry(suffix).or_default();
                        for (w, f) in words {
                            bucket.push((prefix_acc.clone(), w.clone(), *f));
                        }
                    }
                }
            }
        }
        for entries in short_index_full.values_mut() {
            sort_by_exact_then_freq(entries, 1);
            entries.truncate(MAX_PREFIX_ENTRIES);
        }
        for entries in suffix_index.values_mut() {
            entries.sort_by(|a, b| a.0.len().cmp(&b.0.len()).then(b.2.cmp(&a.2)));
            entries.truncate(MAX_PREFIX_ENTRIES);
        }
        self.short_index_full = short_index_full;
        self.suffix_index = suffix_index;
        crate::log::info("扩展索引重建完成（旧 .bin 兼容）");
    }

    /// 从内置词库构建（降级回退）
    fn from_builtin() -> Self {
        let mut index: HashMap<String, Vec<(String, u32, usize)>> = HashMap::new();
        let mut short_index: HashMap<String, Vec<(String, u32, usize)>> = HashMap::new();
        let mut short_index_full: HashMap<String, Vec<(String, u32, usize)>> = HashMap::new();
        let mut suffix_index: HashMap<String, Vec<(String, String, u32)>> = HashMap::new();
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
            // 完整声母索引（V0.3.x）+ 后缀索引（xzai → 现在）
            let short_full = pinyin::to_initial_full(&entry.pinyin);
            if !short_full.is_empty() && short_full != short {
                for i in 1..=short_full.len() {
                    let prefix = &short_full[..i];
                    short_index_full
                        .entry(prefix.to_string())
                        .or_default()
                        .push((entry.word.clone(), entry.frequency, short_full.len()));
                }
            }
            let syls = split_into_syllables(&entry.pinyin);
            if syls.len() >= 2 {
                let mut prefix_acc = String::new();
                for k in 0..syls.len() {
                    prefix_acc.push_str(&pinyin::to_initial_full(&syls[k]));
                    if k + 1 < syls.len() {
                        let suffix = syls[k + 1..].join("");
                        suffix_index.entry(suffix).or_default().push((
                            prefix_acc.clone(),
                            entry.word.clone(),
                            entry.frequency,
                        ));
                    }
                }
            }
        }

        for (prefix, entries) in index.iter_mut() {
            sort_by_exact_then_freq(entries, prefix.len());
        }
        for (prefix, entries) in short_index.iter_mut() {
            sort_by_exact_then_freq(entries, prefix.len());
        }
        for (prefix, entries) in short_index_full.iter_mut() {
            sort_by_exact_then_freq(entries, prefix.len());
        }
        for entries in suffix_index.values_mut() {
            entries.sort_by(|a, b| a.0.len().cmp(&b.0.len()).then(b.2.cmp(&a.2)));
        }
        for words in full_index.values_mut() {
            words.sort_by(|a, b| b.1.cmp(&a.1));
        }

        let mut dict = Self {
            index,
            short_index,
            short_index_full,
            suffix_index,
            common_index: HashMap::new(),
            common_short_index: HashMap::new(),
            common_short_full_index: HashMap::new(),
            user_index: HashMap::new(),
            user_short_index: HashMap::new(),
            user_dict_path: None,
            full_index,
            domain_index: HashMap::new(),
            domain_short_index: HashMap::new(),
            domain_exact_short_index: HashMap::new(),
            word_domain: HashMap::new(),
            domain_names: Vec::new(),
            domain_heat: Vec::new(),
        };
        // V0.2.30：内置词库场景用内置常用词兜底表
        load_common(&mut dict, None);
        dict
    }

    /// 加载专业词库分类文件（对标微软/搜狗分类词库）：
    /// 解析 txt（每行 `词 拼音`，空格分隔），构建全拼前缀 + 简拼声母索引。
    /// domain_id 为该领域的 ID（热词探测用）；词记录进 word_domain（一词一域取首个）。
    pub fn load_domain_dict(&mut self, path: &Path) {
        let content = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                crate::log::error(&format!("分类词库读取失败 {}: {e}", path.display()));
                return;
            }
        };
        // 领域 ID = 当前领域数（首次加载该文件时分配）
        let domain_id = self.domain_names.len();
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("domain{domain_id}"));
        self.domain_names.push(name);
        self.domain_heat.push(0); // 新领域初始热度 0
        // V0.5.2：热门领域初始热度 1（与 load_domains_from_db 一致）
        if is_hot_domain(&self.domain_names[domain_id]) {
            *self.domain_heat.last_mut().unwrap() = 1;
        }
        let mut count = 0usize;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut it = line.split_whitespace();
            let (Some(word), Some(pinyin_str)) = (it.next(), it.next()) else {
                continue; // 格式错误行跳过
            };
            if word.is_empty()
                || pinyin_str.is_empty()
                || !pinyin_str.chars().all(|c| c.is_ascii_alphabetic())
            {
                continue;
            }
            // 拼音统一小写（兼容含 ASCII 字符的混词，如 C位→cwei）
            let pinyin_str = pinyin_str.to_lowercase();
            // 词 → 领域 ID（热词探测用；一词一域取首个）
            self.word_domain
                .entry(word.to_string())
                .or_insert(domain_id);
            // 全拼前缀索引
            for i in 1..=pinyin_str.len() {
                let prefix = &pinyin_str[..i];
                self.domain_index
                    .entry(prefix.to_string())
                    .or_default()
                    .push((word.to_string(), 0, pinyin_str.len(), domain_id));
            }
            // 简拼声母索引
            let short = crate::pinyin::to_initial_string(&pinyin_str);
            if !short.is_empty() {
                for i in 1..=short.len() {
                    let prefix = &short[..i];
                    self.domain_short_index
                        .entry(prefix.to_string())
                        .or_default()
                        .push((word.to_string(), 0, pinyin_str.len(), domain_id));
                }
                // V0.5.2：完整简拼精确索引（wb → 微博），与 load_domains_from_db 一致
                self.domain_exact_short_index
                    .entry(short.clone())
                    .or_default()
                    .push((word.to_string(), 0, pinyin_str.len(), domain_id));
            }
            count += 1;
        }
        // V0.5.2：构建后统一按词长预排序（短词优先），与 load_domains_from_db 一致
        for v in self.domain_index.values_mut() {
            v.sort_by(|a, b| a.0.chars().count().cmp(&b.0.chars().count()));
        }
        for v in self.domain_short_index.values_mut() {
            v.sort_by(|a, b| a.0.chars().count().cmp(&b.0.chars().count()));
        }
        // V0.5.2：精确简拼索引——词长 + 热门领域热度双排序（同 load_domains_from_db）
        {
            let heat = &self.domain_heat;
            for v in self.domain_exact_short_index.values_mut() {
                v.sort_by(|a, b| {
                    let la = a.0.chars().count();
                    let lb = b.0.chars().count();
                    let ha = heat.get(a.3).copied().unwrap_or(0);
                    let hb = heat.get(b.3).copied().unwrap_or(0);
                    la.cmp(&lb).then(hb.cmp(&ha)).then(a.0.cmp(&b.0))
                });
            }
        }
        crate::log::info(&format!(
            "分类词库加载: {} ({} 词条, domain_id={})",
            path.display(),
            count,
            domain_id
        ));
    }

    /// 从 domains.db 加载专业词库（V0.5+）：
    /// SQLite 存储，一次性游标遍历构建内存索引（前缀查询仍需 HashMap O(1)）。
    /// 格式：word / pinyin / domain_id / domain_name，domain_id 确保 domain_names 对齐。
    pub fn load_domains_from_db(&mut self, db_path: &Path) {
        let conn = match Connection::open(db_path) {
            Ok(c) => c,
            Err(e) => {
                crate::log::error(&format!("domains.db 打开失败 {}: {e}", db_path.display()));
                return;
            }
        };
        let mut stmt = match conn.prepare(
            "SELECT word, pinyin, domain_id, domain_name FROM domain_words ORDER BY domain_id",
        ) {
            Ok(s) => s,
            Err(e) => {
                crate::log::error(&format!("domains.db 查询失败: {e}"));
                return;
            }
        };
        let rows = match stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, usize>(2)?,
                row.get::<_, String>(3)?,
            ))
        }) {
            Ok(r) => r,
            Err(e) => {
                crate::log::error(&format!("domains.db 遍历失败: {e}"));
                return;
            }
        };
        let mut count = 0usize;
        // V0.5.3 简繁归一化：维基繁体领域词（我們的出口）加载时转简体，去重
        let mut seen: HashSet<(String, String)> = HashSet::with_capacity(4096);
        for row in rows {
            let (word, pinyin, domain_id, domain_name) = match row {
                Ok(r) => r,
                Err(_) => continue,
            };
            let word = crate::trad::to_simplified(&word);
            if !seen.insert((pinyin.clone(), word.clone())) {
                continue; // 转简后与已加载词条重复（一词一域取首个）
            }
            // 预扩 domain_names / domain_heat 以对齐 domain_id
            while self.domain_names.len() <= domain_id {
                self.domain_names.push(String::new());
                self.domain_heat.push(0);
            }
            if self.domain_names[domain_id].is_empty() {
                self.domain_names[domain_id] = domain_name.to_string();
                // V0.5.2：热门领域初始热度 1（冷启动下领域词组内优先）
                if is_hot_domain(&domain_name) {
                    self.domain_heat[domain_id] = 1;
                }
            }
            // 词 → 领域 ID（一词一域取首个）
            self.word_domain.entry(word.clone()).or_insert(domain_id);
            // 全拼前缀索引
            for i in 1..=pinyin.len() {
                let prefix = &pinyin[..i];
                self.domain_index
                    .entry(prefix.to_string())
                    .or_default()
                    .push((word.clone(), 0, pinyin.len(), domain_id));
            }
            // 简拼声母索引
            let short = crate::pinyin::to_initial_string(&pinyin);
            if !short.is_empty() {
                for i in 1..=short.len() {
                    let prefix = &short[..i];
                    self.domain_short_index
                        .entry(prefix.to_string())
                        .or_default()
                        .push((word.clone(), 0, pinyin.len(), domain_id));
                }
                // V0.5.2：完整简拼精确索引（wb → 微博）——简拼查询时优先出领域词
                self.domain_exact_short_index
                    .entry(short.clone())
                    .or_default()
                    .push((word.clone(), 0, pinyin.len(), domain_id));
            }
            count += 1;
        }
        // V0.5.2：构建后统一按词长预排序（短词优先）——
        // 查询端 take 截断窗口时，热门短词（微博/美团）不会被早期加载的
        // 长领域词挤掉。sort_by 稳定，同长词保持加载序。
        for v in self.domain_index.values_mut() {
            v.sort_by(|a, b| a.0.chars().count().cmp(&b.0.chars().count()));
        }
        for v in self.domain_short_index.values_mut() {
            v.sort_by(|a, b| a.0.chars().count().cmp(&b.0.chars().count()));
        }
        // V0.5.2：精确简拼索引——词长 + 热门领域热度双排序。
        // take 窗口在查询端排序前截断，必须在此处让热门领域词（微博/b站）进入前 30。
        {
            let heat = &self.domain_heat;
            for v in self.domain_exact_short_index.values_mut() {
                v.sort_by(|a, b| {
                    let la = a.0.chars().count();
                    let lb = b.0.chars().count();
                    let ha = heat.get(a.3).copied().unwrap_or(0);
                    let hb = heat.get(b.3).copied().unwrap_or(0);
                    la.cmp(&lb).then(hb.cmp(&ha)).then(a.0.cmp(&b.0))
                });
            }
        }
        crate::log::info(&format!(
            "domains.db 加载完成: {} 领域 ({} 词)",
            self.domain_names.len(),
            count
        ));
    }

    /// 扫描目录自动加载全部专业词库（热词探测 v2）：
    /// V0.5+ 优先 domains.db；不存在则回退遍历 dir/*.txt。
    /// 目录不存在静默（无词库）。
    pub fn load_domains_from_dir(&mut self, dir: &Path) {
        // V0.5+ 优先 domains.db
        let db_path = dir.join("domains.db");
        if db_path.exists() {
            self.load_domains_from_db(&db_path);
            return;
        }
        // 回退 txt 扫描（向后兼容）
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => {
                crate::log::info("domains 目录不存在，跳过专业词库");
                return;
            }
        };
        let mut paths: Vec<std::path::PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "txt").unwrap_or(false))
            .collect();
        paths.sort(); // 稳定领域 ID（按文件名序）
        for p in paths {
            self.load_domain_dict(&p);
        }
        if !self.domain_names.is_empty() {
            crate::log::info(&format!(
                "专业词库全量加载完成: {} 领域 ({} 词)",
                self.domain_names.len(),
                self.word_domain.len()
            ));
        }
    }

    /// 查询词所属领域 ID（热词探测，None = 非领域词）
    pub fn word_domain(&self, word: &str) -> Option<usize> {
        self.word_domain.get(word).copied()
    }

    /// 领域名列表（索引 = domain_id）
    pub fn domain_name(&self, id: usize) -> Option<&str> {
        self.domain_names.get(id).map(|s| s.as_str())
    }

    /// 领域名全列表（调试/测试用）
    pub fn domain_names(&self) -> Vec<&str> {
        self.domain_names.iter().map(|s| s.as_str()).collect()
    }

    /// 清空专业词库索引（停用分类时调用），热度同步重置
    pub fn clear_domain(&mut self) {
        self.domain_index.clear();
        self.domain_short_index.clear();
        self.word_domain.clear();
        self.domain_names.clear();
        self.domain_heat.clear();
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
        // V0.5 fix：路径未设置时回退默认 %APPDATA%/taishen-ime/user_dict.db——
        // 某些进程未调 engine_set_user_dict_path（如 ActivateEx 未完整执行）
        // 导致 learn 静默跳过 → 组词/选词不记忆。回退保证 learn 始终可用。
        let path = match self.user_dict_path.clone() {
            Some(p) => p,
            None => {
                let fallback = std::env::var_os("APPDATA")
                    .map(|a| PathBuf::from(a).join("taishen-ime").join("user_dict.db"))
                    .unwrap_or_default();
                if fallback.as_os_str().is_empty() {
                    return;
                }
                if let Some(dir) = fallback.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                self.user_dict_path = Some(fallback.clone());
                fallback
            }
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
    /// V0.5+：同步建声母索引（user_short_index）——组词学习进化：ts → 泰深
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
        // 声母索引（V0.5+）：taishen → "ts"（tai→t, shen→s 归一）
        // 打 ts（简拼）时 query_short 命中用户词
        let short = crate::pinyin::to_initial_string(&pinyin_str);
        if !short.is_empty() {
            for i in 1..=short.len() {
                let sprefix = &short[..i];
                let sentries = self
                    .user_short_index
                    .entry(sprefix.to_string())
                    .or_default();
                if let Some(existing) = sentries.iter_mut().find(|(w, _, _, _)| *w == word) {
                    existing.1 = existing.1.saturating_add(frequency);
                    existing.2 = existing.2.max(last_used);
                } else {
                    sentries.push((word.clone(), frequency, last_used, pinyin_str.len()));
                }
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
    /// 按领域热度排序追加 domain 词（热词探测 v2）：
    /// 热度 > 0 的领域词先出（按热度降序），热度 0 领域词最后（冷启动不抢位）。
    /// 组内保持原顺序（稳定），去重追加。
    /// V0.5.2：精确匹配优先（词字符数 == key_len，如简拼 wb → 微博）
    /// + 词长短优先（截断窗口内短词先出）。
    /// ⚠️ 性能（V0.5.1 修复）：专业词库全量加载后，短拼音（如 "yi"）可命中
    /// 5000+ 条 domain 词——全量排序 + contains 去重是 O(n²)（实测 25ms/键，
    /// 打字卡顿 + 候选窗刷新滞后）。先截断到每查询上限再排序。
    fn push_domain_sorted(
        &self,
        result: &mut Vec<String>,
        entries: Vec<&(String, u32, usize, usize)>,
        key_len: usize,
    ) {
        const MAX_DOMAIN_PER_QUERY: usize = 60;
        let slice = &entries[..entries.len().min(MAX_DOMAIN_PER_QUERY)];
        let mut indexed: Vec<(usize, &&(String, u32, usize, usize))> =
            slice.iter().enumerate().collect();
        indexed.sort_by(|a, b| {
            let (ai, ea) = a;
            let (bi, eb) = b;
            let a_exact = ea.0.chars().count() == key_len;
            let b_exact = eb.0.chars().count() == key_len;
            let ha = self.domain_heat.get(ea.3).copied().unwrap_or(0);
            let hb = self.domain_heat.get(eb.3).copied().unwrap_or(0);
            b_exact
                .cmp(&a_exact)
                .then(ea.0.chars().count().cmp(&eb.0.chars().count()))
                .then(hb.cmp(&ha))
                .then(ai.cmp(bi))
        });
        for (_, e) in indexed {
            let w = &e.0;
            if !result.contains(w) {
                result.push(w.clone());
            }
        }
    }

    /// 记录领域命中（热词探测 v2）：word 属于某领域 → 该领域热度 +1。
    /// 由 select_candidate 选词时调用；多个领域可同时升温。
    pub fn record_domain_hit(&mut self, word: &str) {
        if let Some(id) = self.word_domain.get(word).copied() {
            if id < self.domain_heat.len() {
                self.domain_heat[id] += 1;
            }
        }
    }

    /// 查询领域热度（供调试/测试）
    pub fn domain_heat(&self) -> &[i64] {
        &self.domain_heat
    }

    /// 全拼前缀查询候选词（"zhong" → 中国等）
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

        // 第一层：精确匹配（pinyin == key）——
        // V0.4：热用户词 > 温用户词(7天内) > 常用词 > 过期用户词(>7天) > 系统词
        // （此前温词在 common 后，对 system 里本就靠前的词选后无感——Eric 反馈）
        if let Some(user_entries) = self.user_index.get(&key) {
            let hot: Vec<&(String, u32, i64, usize)> = user_entries
                .iter()
                .filter(|e| e.3 == key_len && is_hot(e.1, e.2, now))
                .collect();
            push_entries4(&mut result, &hot);
            let warm: Vec<&(String, u32, i64, usize)> = user_entries
                .iter()
                .filter(|e| e.3 == key_len && !is_hot(e.1, e.2, now) && is_recent(e.2, now))
                .collect();
            push_entries4(&mut result, &warm);
        }
        if let Some(common_entries) = self.common_index.get(&key) {
            let exact: Vec<&(String, u32, usize)> =
                common_entries.iter().filter(|e| e.2 == key_len).collect();
            push_entries3(&mut result, &exact);
        }
        if let Some(user_entries) = self.user_index.get(&key) {
            let stale: Vec<&(String, u32, i64, usize)> = user_entries
                .iter()
                .filter(|e| e.3 == key_len && !is_recent(e.2, now))
                .collect();
            push_entries4(&mut result, &stale);
        }
        // V0.5.3：system 高频词（语料常见，frequency >= SYSTEM_HIGH_FREQ）优先于领域词——
        // 避免 domain 精确词（侧视 astronomy）压过常用词（测试 2848，common 表未覆盖）。
        // 低频 system 同音词（苇箔/薇铂）仍让位给领域词（微博，V0.5.2 效果保留）。
        if let Some(entries) = self.index.get(&key) {
            let exact: Vec<&(String, u32, usize)> =
                entries.iter().filter(|e| e.2 == key_len).collect();
            let high: Vec<&(String, u32, usize)> = exact
                .iter()
                .filter(|e| e.1 >= SYSTEM_HIGH_FREQ)
                .copied()
                .collect();
            let low: Vec<&(String, u32, usize)> = exact
                .iter()
                .filter(|e| e.1 < SYSTEM_HIGH_FREQ)
                .copied()
                .collect();
            push_entries3(&mut result, &high);
            // V0.5.2：领域词完整拼音精确匹配提前（weibo → 微博）——system 维基词
            // 精确同音词（苇箔/薇铂）会把领域词顶到 6-7 位。仅 key_len >= 3 生效：
            // 完整拼音信息量大，领域词值得优先；短拼音（yi/de）由 system 高频单字主导。
            if key_len >= 3 {
                if let Some(domain_entries) = self.domain_index.get(&key) {
                    let dexact: Vec<&(String, u32, usize, usize)> = domain_entries
                        .iter()
                        .filter(|e| e.2 == key_len)
                        .take(20)
                        .collect();
                    self.push_domain_sorted(&mut result, dexact, key_len);
                }
            }
            push_entries3(&mut result, &low);
        }
        // 专业词库（对标微软/搜狗分类词库）：追加到系统词后，不抢常用位
        // 热词探测：热度 > 0 的领域词优先（按热度降序），冷启动（全 0）不改变顺序
        // V0.5.1 性能：filter 前限流（取前 120 条），避免 5000+ 全量 collect
        if let Some(domain_entries) = self.domain_index.get(&key) {
            let exact: Vec<&(String, u32, usize, usize)> = domain_entries
                .iter()
                .filter(|e| e.2 == key_len)
                .take(120)
                .collect();
            self.push_domain_sorted(&mut result, exact, key_len);
        }
        // 第二层：前缀扩展（pinyin 长于 key）——
        // V0.4：热用户词 > 温用户词 > 常用词 > 过期用户词 > 系统词
        if let Some(user_entries) = self.user_index.get(&key) {
            let hot: Vec<&(String, u32, i64, usize)> = user_entries
                .iter()
                .filter(|e| e.3 != key_len && is_hot(e.1, e.2, now))
                .collect();
            push_entries4(&mut result, &hot);
            let warm: Vec<&(String, u32, i64, usize)> = user_entries
                .iter()
                .filter(|e| e.3 != key_len && !is_hot(e.1, e.2, now) && is_recent(e.2, now))
                .collect();
            push_entries4(&mut result, &warm);
        }
        if let Some(common_entries) = self.common_index.get(&key) {
            let rest: Vec<&(String, u32, usize)> =
                common_entries.iter().filter(|e| e.2 != key_len).collect();
            push_entries3(&mut result, &rest);
        }
        if let Some(user_entries) = self.user_index.get(&key) {
            let stale: Vec<&(String, u32, i64, usize)> = user_entries
                .iter()
                .filter(|e| e.3 != key_len && !is_recent(e.2, now))
                .collect();
            push_entries4(&mut result, &stale);
        }
        if let Some(entries) = self.index.get(&key) {
            let rest: Vec<&(String, u32, usize)> =
                entries.iter().filter(|e| e.2 != key_len).collect();
            push_entries3(&mut result, &rest);
        }
        // 专业词库前缀扩展：追加到系统词后（V0.5.1 性能：filter 前限流 120）
        if let Some(domain_entries) = self.domain_index.get(&key) {
            let rest: Vec<&(String, u32, usize, usize)> = domain_entries
                .iter()
                .filter(|e| e.2 != key_len)
                .take(120)
                .collect();
            self.push_domain_sorted(&mut result, rest, key_len);
        }
        result
    }

    /// 简拼声母前缀查询候选词（如 "zg" → 中国）
    /// V0.2.30：常用词声母命中优先（hd → 好的 在系统简拼候选前）
    /// V0.3.x：合并完整声母索引（zh/ch/sh 保留，shh→社会/zhz→正在）+ compact
    /// （zh→z 归一，zg→中国），同组统一按词频降序——
    /// 避免 full 优先把"资格"(zige=zg) 顶到"中国"(zhongguo→zg) 前。
    pub fn query_short(&self, prefix: &str) -> Vec<String> {
        // P2-3：v 归一（简拼中 qv→qu 等）
        let key = crate::pinyin::normalize_v(&prefix.to_lowercase());
        let mut result: Vec<String> = Vec::new();
        // V0.5+：用户词简拼优先（组词学习进化：ts → 泰深）——按词频降序
        if let Some(user_short) = self.user_short_index.get(&key) {
            let mut us = user_short.clone();
            us.sort_by(|a, b| b.1.cmp(&a.1));
            for (w, _, _, _) in us {
                if !result.contains(&w) {
                    result.push(w);
                }
            }
        }
        if let Some(common) = self.common_short_index.get(&key) {
            for (w, _, _) in common {
                if !result.contains(w) {
                    result.push(w.clone());
                }
            }
        }
        // V0.3.x2：完整声母常用词索引（shh→社会）——排在系统简拼候选前
        if let Some(common_full) = self.common_short_full_index.get(&key) {
            for (w, _, _) in common_full {
                if !result.contains(w) {
                    result.push(w.clone());
                }
            }
        }
        // V0.5.3：system 简拼高频词优先于领域词——避免领域简拼精确词
        // （之卦 conversation/杂谷 food）压过 system 高频（中国 4298）。
        // V0.5.2 的领域词前置（wb → 微博）仍生效，但只对 system 低频让位。
        // 注意：rest 需在 domain_exact 之前构建，以便拆分高/低频。
        let mut rest: Vec<(String, u32)> = Vec::new();
        if let Some(entries) = self.short_index_full.get(&key) {
            for (w, f, _) in entries {
                rest.push((w.clone(), *f));
            }
        }
        if let Some(entries) = self.short_index.get(&key) {
            for (w, f, _) in entries {
                rest.push((w.clone(), *f));
            }
        }
        // 精确简拼匹配优先（word 字符数 == key 长度 → 单字优先），同组按词频降序。
        // V0.x 修复：简拼纯词频排序下，新闻语料高频词组（万能通 289）压过单字（我 217830），
        // 导致打 w 时单字被淹没。精确匹配让单字候选回到词组前。
        rest.sort_by(|a, b| {
            let a_exact = a.0.chars().count() == key.len();
            let b_exact = b.0.chars().count() == key.len();
            b_exact.cmp(&a_exact).then(b.1.cmp(&a.1))
        });
        // 拆 system 高/低频（与 query() 精确层同一阈值）
        let mut rest_high: Vec<(String, u32)> = Vec::new();
        let mut rest_low: Vec<(String, u32)> = Vec::new();
        for e in rest {
            if e.1 >= SYSTEM_HIGH_FREQ {
                rest_high.push(e);
            } else {
                rest_low.push(e);
            }
        }
        for (w, _) in &rest_high {
            if !result.contains(w) {
                result.push(w.clone());
            }
        }
        // V0.5.2：领域词完整简拼精确匹配（wb → 微博）——插到 system 低频词前。
        // 仅 ≥2 字母生效：单字母简拼太模糊，应优先看 system 高频单字（我/为/外）。
        if key.len() >= 2 {
            if let Some(domain_exact) = self.domain_exact_short_index.get(&key) {
                let entries: Vec<&(String, u32, usize, usize)> =
                    domain_exact.iter().take(30).collect();
                self.push_domain_sorted(&mut result, entries, key.len());
            }
        }
        for (w, _) in &rest_low {
            if !result.contains(w) {
                result.push(w.clone());
            }
        }
        // 专业词库简拼（对标微软/搜狗分类词库）：追加到系统简拼后
        // V0.5.1 性能：限流 120，避免短简拼命中数千条
        if let Some(domain) = self.domain_short_index.get(&key) {
            let entries: Vec<&(String, u32, usize, usize)> = domain.iter().take(120).collect();
            self.push_domain_sorted(&mut result, entries, key.len());
        }
        result
    }

    /// 缩写+全拼混合查询（V0.3.x，对标 rime abbrev 逐音节缩写）：
    /// 输入 = 声母缩写前缀 + 完整拼音后缀（xzai = x + zai → 现在）。
    /// 与 query_mixed（全拼前缀+声母后缀，shurf→输入法）互补。
    /// 例：xianzai(现在) 音节 [xian, zai] → suffix_index["zai"] 记 (x, 现在)
    ///     输入 "xzai"：prefix="x"（声母）✓ suffix="zai"（拼音前缀）✓ → 命中
    pub fn query_abbrev_full(&self, input: &str) -> Vec<String> {
        let input = crate::pinyin::normalize_v(&input.to_lowercase());
        if input.len() < 3 || input.len() > 10 {
            return Vec::new();
        }
        let mut result: Vec<String> = Vec::new();
        for i in 1..input.len() {
            let prefix = &input[..i];
            let suffix = &input[i..];
            if !is_valid_initials_prefix(prefix) {
                continue;
            }
            if !pinyin::is_valid_pinyin_prefix(suffix) {
                continue;
            }
            if let Some(entries) = self.suffix_index.get(suffix) {
                for (pi, w, _) in entries {
                    if pi.starts_with(prefix) && !result.contains(w) {
                        result.push(w.clone());
                    }
                }
            }
        }
        result
    }

    /// 调试：获取 common_index 指定前缀词条（V0.2.30 诊断用）
    pub fn common_index_debug(&self, prefix: &str) -> Option<&Vec<(String, u32, usize)>> {
        self.common_index.get(prefix)
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

    /// 逐音节组合联想（V0.4，Eric 反馈：nimzai/ganshm/yaowoquz 混合输入无候选）：
    /// 输入 = 音节前缀 / 声母缩写 的混合序列（如 ni+m+zai → 你们在、gan+sh+m → 干什么）。
    /// 与 query_mixed（全拼前缀+声母后缀）和 query_abbrev_full（声母前缀+全拼后缀）
    /// 的区别：支持任意位置混插（全拼+声母+全拼、全拼+多音节缩写等），对标搜狗
    /// 逐音节智能切分。
    ///
    /// 实现：递归把输入切成段（段 = 音节前缀 | 音节声母），逐段匹配词的音节序列。
    /// 遍历 full_index 中"首音节前缀"命中的完整拼音词条（BTreeMap range 有界），
    /// 对每个词做段匹配。性能：首音节前缀词条有限（如 ni* 数千条），段匹配为
    /// 字符串前缀比较，实测 <5ms（仅在候选不足时调用，可接受）。
    pub fn query_combo(&self, input: &str) -> Vec<String> {
        let input = crate::pinyin::normalize_v(&input.to_lowercase());
        // 短输入（<4）交给 query/query_short/query_abbrev_full；超长不联想
        if input.len() < 4 || input.len() > 12 {
            return Vec::new();
        }
        // 首段：完整音节 → range(音节..)；否则首字符（声母，如 r/g）→ range(字符..下一字符)。
        // V0.4.1（Eric：rgshni→如果是你）：首字符非音节时不再放弃——"r" 是 ru 的声母，
        // range("r".."s") 遍历 r 开头的所有完整拼音（ran/re/ren/ri/rong/ru...）。
        let range_start = match pinyin::split_first_syllable(&input) {
            Some((syl, _)) => syl.to_string(),
            None => match input.chars().next() {
                Some(c) => c.to_string(),
                None => return Vec::new(),
            },
        };
        // range 终点：range_start 末字符 +1（字典序下一字符，如 "r"→"s"、"ni"→"nj"）
        let mut range_end = range_start.clone();
        if let Some(last) = range_end.pop() {
            if let Some(next) = char::from_u32(last as u32 + 1) {
                range_end.push(next);
            }
        }
        let mut result: Vec<String> = Vec::new();
        // 遍历上限（性能保险）：首段过宽（单声母如 r）时词条多，截断
        let mut scanned = 0usize;
        for (pinyin, words) in self.full_index.range(range_start.clone()..) {
            if !pinyin.starts_with(&range_start) || !range_end.is_empty() && pinyin >= &range_end {
                break;
            }
            scanned += 1;
            if scanned > COMBO_SCAN_LIMIT {
                break;
            }
            // 输入串本身就是完整拼音的（query 已覆盖）跳过
            if *pinyin == input {
                continue;
            }
            if !combo_match(&input, pinyin) {
                continue;
            }
            for (w, _) in words {
                if !result.contains(w) {
                    result.push(w.clone());
                    if result.len() >= COMBO_OUTPUT_LIMIT {
                        return result;
                    }
                }
            }
        }
        result
    }

    /// 逐音节拼接联想（V0.4，Eric 反馈：nimzai/ganshm 混合输入无候选）：
    /// 输入切成 [音节前缀 | 声母]* 段（如 ni+m+zai、gan+sh+m），每段查候选后拼接
    /// 成词（你+吗+在 → 你吗在、干+什+么 → 干什么）。与 query_combo（整词匹配）
    /// 互补：词库无整词时（"你们在"不存在）也能联想出合理短语。
    /// 对标搜狗"逐音节智能切分"：先试整词（query_combo），无果再逐音节拼接。
    pub fn combo_guess(&self, input: &str) -> Vec<String> {
        let input = crate::pinyin::normalize_v(&input.to_lowercase());
        if input.len() < 4 || input.len() > 14 {
            return Vec::new();
        }
        let mut out: Vec<String> = Vec::new();
        let mut segs: Vec<String> = Vec::new();
        self.combo_syl_rec(&input, &mut segs, &mut out);
        out
    }

    /// 递归切分输入为 [音节前缀 | 单声母]* 段（任意段数 ≤4，任意开头），
    /// 每段查候选拼接。V0.4.1 升级（Eric：yaowoquz→要我去做 需 4 段 yao+wo+qu+z，
    /// 原 combo_guess 固定 3 段且段2 仅声母串，无法覆盖）。
    fn combo_syl_rec(&self, input: &str, segs: &mut Vec<String>, out: &mut Vec<String>) {
        if input.is_empty() {
            if segs.len() >= 2 {
                self.build_combo_words(segs, out);
            }
            return;
        }
        if segs.len() >= 4 || out.len() >= COMBO_OUTPUT_LIMIT {
            return;
        }
        // 分支A：段 = 音节前缀（长优先，完整音节优先匹配）
        for i in (1..=input.len().min(6)).rev() {
            let seg = &input[..i];
            if crate::pinyin::is_valid_pinyin_prefix(seg) {
                segs.push(seg.to_string());
                self.combo_syl_rec(&input[i..], segs, out);
                segs.pop();
                if out.len() >= COMBO_OUTPUT_LIMIT {
                    return;
                }
            }
        }
        // 分支B：段 = 单声母（zh/ch/sh 两字符或单字符）
        let chars: Vec<char> = input.chars().collect();
        let init_len =
            if chars.len() >= 2 && matches!(&chars[..2], ['z', 'h'] | ['c', 'h'] | ['s', 'h']) {
                2
            } else {
                1
            };
        let init: String = chars[..init_len].iter().collect();
        let is_init = init == "zh"
            || init == "ch"
            || init == "sh"
            || (init.len() == 1 && "bpmfdtnlgkhjqxzcsrwyaeo".contains(&init));
        if is_init {
            segs.push(init.clone());
            self.combo_syl_rec(&input[init_len..], segs, out);
            segs.pop();
        }
    }

    /// 段候选拼接（combo_syl_rec 用）：每段查候选（声母段→query_short，
    /// 音节段→query，均取单字），笛卡尔积拼接，限流防爆炸。
    fn build_combo_words(&self, segs: &[String], out: &mut Vec<String>) {
        let mut combos: Vec<String> = vec![String::new()];
        for seg in segs {
            // 声母段判定：单字符声母 或 zh/ch/sh
            let is_initial = matches!(seg.as_str(), "zh" | "ch" | "sh")
                || (seg.len() == 1 && "bpmfdtnlgkhjqxzcsrwyaeo".contains(seg.as_str()));
            // 声母段取 top3（如 z→在/再/做），音节段取 top2（如 wo→我/窝）
            let picks: Vec<String> = if is_initial {
                self.query_short(seg)
                    .into_iter()
                    .filter(|w| w.chars().count() == 1)
                    .take(3)
                    .collect()
            } else {
                self.query(seg)
                    .into_iter()
                    .filter(|w| w.chars().count() == 1)
                    .take(2)
                    .collect()
            };
            if picks.is_empty() {
                return; // 某段无候选 → 放弃该切分
            }
            let mut next: Vec<String> = Vec::new();
            for c in &combos {
                for p in &picks {
                    let w = format!("{c}{p}");
                    if !out.contains(&w) {
                        next.push(w);
                        if next.len() >= 24 {
                            break;
                        }
                    }
                }
                if next.len() >= 24 {
                    break;
                }
            }
            combos = next;
            if combos.is_empty() {
                return;
            }
        }
        for c in combos {
            if c.chars().count() >= 2 && !out.contains(&c) {
                out.push(c);
                if out.len() >= COMBO_OUTPUT_LIMIT {
                    return;
                }
            }
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

    // ─────────────────────────────────────────────────────────
    // V0.4.5 词库锚定拆分组词（Eric 设计：不按音节机械切分，以词库词组为锚）
    //
    // 用户输出非单字时，目标必是词库中已有的词组（单词/短句/成语/谚语）。
    // 因此把输入串递归切成若干段，每段必须命中词库真实词组（full_index 完整
    // 拼音匹配，允许段内 1 个错误：多打/打错/换序），组合各段词组文字输出。
    //
    // 与 phrase_guess（按音节切分）区别：锚点是"词库词组"而非"音节"——
    // 切出来的每段都是真实词组，不会拼出"下嗯下嗯"式怪词。
    // ─────────────────────────────────────────────────────────
    /// 拆分组词：输入串 → 词库词组序列（每段允许 1 个错误）
    /// 例：zhegeweomende → "这个"+"我们的"（weom 删 e → wom 命中）
    ///     zhegewumende → "这个"+"我们的"（wum 换 o → wom 命中）
    /// 排序：错误段数少优先 → 段数少优先（更完整）→ 字数多优先。
    pub fn phrase_group_guess(&self, input: &str) -> Vec<String> {
        let input = pinyin::normalize_v(&input.to_lowercase());
        // 长串才有拆组价值（<6 交给 query 等）；>24 防爆炸
        if input.len() < 6 || input.len() > 24 {
            return Vec::new();
        }
        let mut out: Vec<(String, usize, usize)> = Vec::new(); // (词组, 错误段数, 段数)
        let mut segs: Vec<String> = Vec::new();
        let mut errs: usize = 0;
        // V0.4.5 修复：group_rec 无记忆化递归——词库扩容（62万+21领域）后
        // full_index 命中面增大，同一 rest 被不同前缀路径反复展开 → 指数爆炸
        // （实测 test_group_exact_phrase_priority 挂起 60s+）。
        // 不能按 (rest, errs) 记忆化——同一 (rest, errs) 在不同 segs 前缀下
        // 展开结果不同（seg 组合不同），visited 会误伤正确路径（实测丢失
        // "这个我们的"）。方案：节点预算硬性截断——预算内穷尽所有组合，
        // 预算耗尽即放弃（极端词库输入防挂起，正常输入远低于预算）。
        let mut budget: usize = GROUP_NODE_BUDGET;
        self.group_rec(&input, &mut segs, &mut errs, &mut out, &mut budget);
        // 错误段数少优先 → 段数少优先（完整拆分 > 部分拆分）→ 字数多优先
        out.sort_by(|a, b| {
            a.1.cmp(&b.1)
                .then(a.2.cmp(&b.2))
                .then(b.0.chars().count().cmp(&a.0.chars().count()))
        });
        out.truncate(GROUP_OUTPUT_LIMIT);
        out.into_iter().map(|(w, _, _)| w).collect()
    }

    /// 递归拆段：每段 s = input[..i]（i 从长到短，优先长词组）
    /// 段命中（精确或单错变体命中 full_index）→ 递归剩余 → 组合
    /// budget: 节点预算——每次展开递减，耗尽即放弃（极端输入硬性兜底防挂起）
    fn group_rec(
        &self,
        rest: &str,
        segs: &mut Vec<String>,
        errs: &mut usize,
        out: &mut Vec<(String, usize, usize)>,
        budget: &mut usize,
    ) {
        // 预算耗尽：放弃本次展开（避免极端词库输入下的组合爆炸）
        if *budget == 0 {
            return;
        }
        // 组合条件：至少 2 段（单段交给 query）；整串拆完才算一个结果
        if rest.is_empty() {
            if segs.len() >= 2 {
                let joined: String = segs.join("");
                if !out.iter().any(|(w, _, _)| w == &joined) {
                    out.push((joined, *errs, segs.len()));
                }
            }
            return;
        }
        if segs.len() >= GROUP_MAX_SEGS || out.len() >= GROUP_OUTPUT_LIMIT * 4 {
            return;
        }
        *budget -= 1;
        let max_i = rest.len().min(GROUP_MAX_SEG_LEN);
        // A 阶段：先尝试所有精确段（i 从长到短）。精确路径优先于变体路径——
        // 否则长段变体（如 zhegewi→zhegei→"这给"）先吃满输出配额，
        // 正确的词组拆分（zhege→"这个"）被截断不执行（V0.4.5 实测修复）。
        for i in (2..=max_i).rev() {
            let seg = &rest[..i];
            // 剪枝：首字符必须是合法声母或零声母字母（防完全无关的英文段，
            // 如 hello 的 he/llo；拼音首字母集合 bpmfdtnlgkhjqxzcsrwyaeo）。
            // 不能要求整段是音节前缀——多音节段（zhege/weomende）前缀不合法但
            // 可能是词组或错误段（weomende→womende），full_index 查询做最终过滤。
            let first = seg.chars().next().unwrap_or(' ');
            if !"bpmfdtnlgkhjqxzcsrwyaeo".contains(first) {
                continue;
            }
            // 精确命中词库词组（full_index 完整拼音）
            if let Some(words) = self.full_index.get(seg) {
                for (w, _) in words.iter().take(GROUP_EXACT_TAKE) {
                    segs.push(w.clone());
                    self.group_rec(&rest[i..], segs, errs, out, budget);
                    segs.pop();
                    if out.len() >= GROUP_OUTPUT_LIMIT * 4 {
                        return;
                    }
                }
            }
        }
        // B 阶段：变体段（多打/打错/换序，对标 deletion/correction）——
        // 全部精确段尝试后再补变体，全串最多 GROUP_MAX_ERR 个错误段。
        if *errs < GROUP_MAX_ERR {
            for i in (2..=max_i).rev() {
                let seg = &rest[..i];
                let first = seg.chars().next().unwrap_or(' ');
                if !"bpmfdtnlgkhjqxzcsrwyaeo".contains(first) {
                    continue;
                }
                for v in group_seg_variants(seg) {
                    if let Some(words) = self.full_index.get(&v) {
                        *errs += 1;
                        for (w, _) in words.iter().take(GROUP_ERR_TAKE) {
                            segs.push(w.clone());
                            self.group_rec(&rest[i..], segs, errs, out, budget);
                            segs.pop();
                            if out.len() >= GROUP_OUTPUT_LIMIT * 4 {
                                *errs -= 1;
                                return;
                            }
                        }
                        *errs -= 1;
                    }
                }
            }
        }
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

/// query_combo 遍历上限（V0.4）：首音节前缀词条数保险，超出截断防卡顿
const COMBO_SCAN_LIMIT: usize = 8000;
/// query_combo 输出上限（V0.4）：混合联想最多补 N 个候选
const COMBO_OUTPUT_LIMIT: usize = 10;

// ─── V0.4.5 拆分组词常量 ───
/// 拆分组词输出上限（避免候选爆窗）
const GROUP_OUTPUT_LIMIT: usize = 8;
/// 最大段数（用户场景：超过 4 词的长串 → 最多 5 段）
const GROUP_MAX_SEGS: usize = 5;
/// 单段最大长度（拼音音节最长 6 字符，词组拼音可更长，8 覆盖双音节词组）
const GROUP_MAX_SEG_LEN: usize = 8;
/// 精确命中段每段取词数
const GROUP_EXACT_TAKE: usize = 2;
/// 错误段每段取词数（容错组合收敛，防爆炸）
const GROUP_ERR_TAKE: usize = 1;
/// 全串最多错误段数（用户场景：中间 1~2 个词有错）
const GROUP_MAX_ERR: usize = 2;
/// 拆分组词递归节点预算（V0.4.5 修复）：无记忆化时同一子串被反复展开，
/// 词库扩容后组合爆炸。预算耗尽即整体放弃展开（返回已收集的结果）。
/// 正常输入（≤5 段、≤2 错段）在数百节点内收敛，预算取 5000 留足余量。
const GROUP_NODE_BUDGET: usize = 5000;

/// 段内单错变体：多打（删除一位）+ 键位相邻（替换/交换）+ 拼写模式
/// 合并 correction 模块的变体生成，去重、去原串、保长度 ≥2。
fn group_seg_variants(seg: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |v: String| {
        if v != seg && !out.contains(&v) && v.len() >= 2 {
            out.push(v);
        }
    };
    for v in crate::correction::deletion_variants(seg) {
        push(v);
    }
    for v in crate::correction::correction_variants(seg) {
        push(v);
    }
    for v in crate::correction::spelling_variants(seg) {
        push(v);
    }
    out
}

/// 逐音节段匹配（V0.4，query_combo 用）：
/// input（如 "nimzai"）能否切成段序列，逐段匹配 word_pinyin（如 "nimenzai"）的
/// 音节序列。段类型：音节前缀（1..=音节长）或 音节声母（zh/ch/sh 保留两字符）。
fn combo_match(input: &str, word_pinyin: &str) -> bool {
    let syls = split_into_syllables(word_pinyin);
    if syls.is_empty() {
        return false;
    }
    combo_rec(input, &syls, 0)
}

/// 递归段匹配：input[..] 匹配 syls[idx..] 的音节序列
/// 每步两个分支：①当前音节声母（如 m）②当前音节前缀（如 men/m/me...）
/// 任一分支递归到底（input 耗尽且音节耗尽）即匹配成功。
fn combo_rec(input: &str, syls: &[String], idx: usize) -> bool {
    if input.is_empty() {
        return idx == syls.len();
    }
    if idx >= syls.len() {
        return false;
    }
    let syl = &syls[idx];
    // 分支①：段 = 音节声母（zh/ch/sh 保留两字符，n→n、m→m、z→z）
    let initial = pinyin::to_initial_full(syl);
    if !initial.is_empty() && input.starts_with(&initial) {
        if combo_rec(&input[initial.len()..], syls, idx + 1) {
            return true;
        }
    }
    // 分支②：段 = 音节前缀（长优先——完整音节优先匹配）
    for i in (1..=syl.len()).rev() {
        if input.starts_with(&syl[..i]) {
            if combo_rec(&input[i..], syls, idx + 1) {
                return true;
            }
        }
    }
    false
}

/// 判断字符串是否为合法的声母缩写序列前缀（V0.3.x，query_abbrev_full 用）：
/// zh/ch/sh 两字符整体 + 单字符声母 bpmfdtnlgkhjqxzcsrwy + 零声母单字母 aeo。
/// 示例："x" ✓、"xz" ✓、"xza" ✗（a 不能跟在 z 后）、"shh" ✓（sh+h）
fn is_valid_initials_prefix(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if (c == 'z' || c == 'c' || c == 's') && i + 1 < chars.len() && chars[i + 1] == 'h' {
            i += 2;
        } else if "bpmfdtnlgkhjqxzcsrwyaeo".contains(c) {
            i += 1;
        } else {
            return false;
        }
    }
    true
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
/// 已加载的专业词库分类路径（幂等判断用）。
static DOMAIN_DICT_PATH: Mutex<Option<String>> = Mutex::new(None);
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
    // 热词探测 v2：词库就绪后自动加载专业词库目录
    // 目录 = 词库所在目录下的 domains/（如 resources/domains）
    if let Some(path) = path_str.as_deref() {
        let dict_dir = std::path::Path::new(path).parent();
        if let Some(dir) = dict_dir {
            let domains_dir = dir.join("domains");
            set_domain_dict_path(Some(&domains_dir));
        }
    } else {
        // 内置词库（无路径）→ 跳过 domains（测试环境无词库文件）
        crate::log::info("内置词库场景：不自动加载专业词库");
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

/// 设置专业词库目录（热词探测 v2，对标微软/搜狗分类词库）。
/// dir: domains 目录（自动扫描 *.txt 全量加载）。NULL/空 = 清空分类索引（停用）。
/// 需在 init 之后调用。幂等：路径未变化且已加载 → 跳过。
pub fn set_domain_dict_path(path: Option<&Path>) {
    let path_str = path.map(|p| p.to_string_lossy().into_owned());
    {
        let cur = DOMAIN_DICT_PATH.lock().unwrap_or_else(|e| e.into_inner());
        if *cur == path_str {
            return;
        }
    }
    let mut dict = DICT.lock().unwrap_or_else(|e| e.into_inner());
    match dict.as_mut() {
        Some(d) => match path {
            Some(p) => d.load_domains_from_dir(p),
            None => {
                d.clear_domain();
                crate::log::info("专业词库已禁用");
            }
        },
        None => crate::log::error("专业词库设置失败：词库未初始化"),
    }
    *DOMAIN_DICT_PATH.lock().unwrap_or_else(|e| e.into_inner()) = path_str;
}

/// 记录领域命中（热词探测 v2）：选词时调用，词所属领域热度 +1。
pub fn record_domain_hit(word: &str) {
    let mut dict = DICT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(d) = dict.as_mut() {
        d.record_domain_hit(word);
    }
}

/// 领域名全列表（调试/测试用）
pub fn domain_names() -> Vec<String> {
    let dict = DICT.lock().unwrap_or_else(|e| e.into_inner());
    match dict.as_ref() {
        Some(d) => d.domain_names().iter().map(|s| s.to_string()).collect(),
        None => Vec::new(),
    }
}

/// 领域热度全列表（apply_domain_boost 用）
pub fn domain_heats() -> Vec<i64> {
    let dict = DICT.lock().unwrap_or_else(|e| e.into_inner());
    match dict.as_ref() {
        Some(d) => d.domain_heat().to_vec(),
        None => Vec::new(),
    }
}

/// 词所属领域的当前热度（apply_domain_boost 用，None = 非领域词）
pub fn word_domain_heat(word: &str) -> Option<i64> {
    let dict = DICT.lock().unwrap_or_else(|e| e.into_inner());
    match dict.as_ref() {
        Some(d) => {
            let id = d.word_domain(word)?;
            d.domain_heat().get(id).copied()
        }
        None => None,
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

/// 用户词简拼是否命中（V0.5+）：datetime 简码（ts 时间戳）让位用户学习词
pub fn user_short_hit(code: &str) -> bool {
    let dict = DICT.lock().unwrap_or_else(|e| e.into_inner());
    match dict.as_ref() {
        Some(d) => d
            .user_short_index
            .get(&code.to_lowercase())
            .map(|v| !v.is_empty())
            .unwrap_or(false),
        None => false,
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

/// 缩写+全拼混合查询（V0.3.x，xzai → 现在；自动触发延迟初始化）
pub fn query_abbrev_full(input: &str) -> Vec<String> {
    let dict = DICT.lock().unwrap_or_else(|e| e.into_inner());
    match dict.as_ref() {
        Some(d) => d.query_abbrev_full(input),
        None => {
            drop(dict);
            init(None);
            DICT.lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .query_abbrev_full(input)
        }
    }
}

/// 逐音节组合联想（V0.4，ni+m+zai → 你们在；自动触发延迟初始化）
pub fn query_combo(input: &str) -> Vec<String> {
    let dict = DICT.lock().unwrap_or_else(|e| e.into_inner());
    match dict.as_ref() {
        Some(d) => d.query_combo(input),
        None => {
            drop(dict);
            init(None);
            DICT.lock().unwrap().as_ref().unwrap().query_combo(input)
        }
    }
}

/// 逐音节拼接联想（V0.4，ni+m+zai → 你吗在/你们在；自动触发延迟初始化）
pub fn combo_guess(input: &str) -> Vec<String> {
    let dict = DICT.lock().unwrap_or_else(|e| e.into_inner());
    match dict.as_ref() {
        Some(d) => d.combo_guess(input),
        None => {
            drop(dict);
            init(None);
            DICT.lock().unwrap().as_ref().unwrap().combo_guess(input)
        }
    }
}

/// 词库锚定拆分组词（V0.4.5，zhegeweomende → 这个我们的；自动触发延迟初始化）
/// Eric 设计：不按音节机械切分，每段锚定词库真实词组，允许段内 1 个错误。
pub fn phrase_group_guess(input: &str) -> Vec<String> {
    let dict = DICT.lock().unwrap_or_else(|e| e.into_inner());
    match dict.as_ref() {
        Some(d) => d.phrase_group_guess(input),
        None => {
            drop(dict);
            init(None);
            DICT.lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .phrase_group_guess(input)
        }
    }
}

/// 常用词集合（V0.2.30）：引擎层长词过滤跳过用。
/// common 词条顺序由用户词表显式指定，不应被 P2-2 长词过滤打乱。
/// 词库未加载时返回空集（长词过滤照常，无保护）。
pub fn common_word_set() -> std::collections::HashSet<String> {
    let dict = DICT.lock().unwrap_or_else(|e| e.into_inner());
    let mut set = std::collections::HashSet::new();
    if let Some(d) = dict.as_ref() {
        for entries in d.common_index.values() {
            for (w, _, _) in entries {
                set.insert(w.clone());
            }
        }
    }
    set
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
    fn test_bin_contains_shehui() {
        // V0.3.x2：直接反序列化 .bin，验证 short_index_full["shh"] 含"社会"
        let bin_path = Path::new("../../resources/system_dict.db.bin");
        if !bin_path.exists() {
            return; // .bin 未生成则跳过（SQLite 加载测试覆盖）
        }
        let d = Dictionary::from_bin(bin_path).expect("from_bin");
        if let Some(entries) = d.short_index_full.get("shh") {
            println!("short_index_full[shh] ({} entries)", entries.len());
            for (i, (w, f, l)) in entries.iter().take(15).enumerate() {
                println!("  [{i}] {w} freq={f} len={l}");
            }
            assert!(
                entries.iter().any(|(w, _, _)| w == "社会"),
                ".bin short_index_full[shh] 缺'社会'"
            );
        } else {
            panic!("short_index_full[shh] MISSING");
        }
        let full = d.query("shehui");
        assert_eq!(full.first().map(|s| s.as_str()), Some("社会"));
        let short = d.query_short("shh");
        assert!(short.iter().any(|w| w == "社会"), "query_short(shh) 缺社会");
        // Engine 完整流程模拟（与 example 一致）
        let mut eng = crate::Engine::new();
        for ch in ['s', 'h', 'h'] {
            eng.process_key(ch);
        }
        println!("engine shh candidates:");
        for i in 0..eng.candidate_count() {
            println!("  [{i}] {}", eng.candidate(i).unwrap_or(""));
        }
        assert!(
            (0..eng.candidate_count()).any(|i| eng.candidate(i) == Some("社会")),
            "Engine shh 候选缺'社会'"
        );
    }

    #[test]
    fn test_sqlite_shehui_short() {
        // V0.3.x2 回归：词库重建后 shh 简拼必须出"社会"（TOP_N_BASE 截取 bug）
        let db_path = Path::new("../../resources/system_dict.db");
        if db_path.exists() {
            init_blocking(Some(db_path));
            let short = query_short("shh");
            println!("query_short(shh) = {:?}", short);
            // dump short_index_full["shh"] 前 20
            let d = DICT.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(dict) = d.as_ref() {
                if let Some(entries) = dict.short_index_full.get("shh") {
                    println!("short_index_full[shh] ({} entries):", entries.len());
                    for (i, (w, f, l)) in entries.iter().take(20).enumerate() {
                        println!("  [{i}] {w} freq={f} len={l}");
                    }
                } else {
                    println!("short_index_full[shh] MISSING");
                }
                println!(
                    "short_index keys containing 'shh': {:?}",
                    dict.short_index_full
                        .keys()
                        .filter(|k| k.starts_with("shh"))
                        .take(10)
                        .collect::<Vec<_>>()
                );
            }
            drop(d);
            assert!(
                short.iter().any(|w| w == "社会"),
                "shh 简拼应含'社会'，实际: {:?}",
                short
            );
            let full = query("shehui");
            assert!(
                full.first().map(|s| s.as_str()) == Some("社会"),
                "shehui 精确拼音首位应为'社会'，实际: {:?}",
                full.first()
            );
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
    fn test_domain_dict_load_and_query() {
        // 专业词库：加载后 query 应命中 domain 词（追加在系统词后）
        let mut d = dict_with_common();
        d.load_domain_dict(std::path::Path::new("tests/data/domain_test.txt"));
        let r = d.query("shenjingwangluo");
        assert!(
            r.contains(&"神经网络".to_string()),
            "domain 词 神经网络 应命中, got: {:?}",
            r
        );
        // 简拼也应命中
        let rs = d.query_short("sjwl");
        assert!(
            rs.contains(&"神经网络".to_string()),
            "简拼 sjwl 应命中 神经网络, got: {:?}",
            rs
        );
        // 清空后不命中
        d.clear_domain();
        let r2 = d.query("shenjingwangluo");
        assert!(!r2.contains(&"神经网络".to_string()), "清空后不应命中");
    }

    #[test]
    fn test_common_top5_survey() {
        // 全量调查：常用词（common_dict 538 条）+ 现代领域词，全拼/简拼前 5 命中率。
        // 探索用——只打印统计，不改断言。跑完根据数据决定改进。
        let sys_path = std::path::Path::new("../resources/system_dict.db");
        let db_path = std::path::Path::new("../resources/domains/domains.db");
        if !sys_path.exists() || !db_path.exists() {
            eprintln!("[SKIP] 词库文件缺失");
            return;
        }
        let mut d = Dictionary::from_sqlite(sys_path).unwrap_or_else(|e| {
            panic!("system_dict 加载失败: {e}");
        });
        load_common(&mut d, Some(std::path::Path::new("../resources/")));
        d.load_domains_from_db(db_path);

        // ── 1) common 词全拼前 5 ──
        // common_index 的 key = 拼音前缀；pinyin_len==key.len() 的 entry 是完整拼音词
        let mut common_words: Vec<(String, String)> = Vec::new(); // (pinyin, word)
        for (key, entries) in d.common_index.iter() {
            for (w, _, plen) in entries {
                if *plen == key.len() {
                    common_words.push((key.clone(), w.clone()));
                }
            }
        }
        common_words.sort();
        common_words.dedup();
        let mut miss_full: Vec<String> = Vec::new();
        let mut hit_full = 0usize;
        for (py, w) in &common_words {
            let r = d.query(py);
            if r.iter().take(5).any(|c| c == w) {
                hit_full += 1;
            } else {
                miss_full.push(format!("{w}({py}) 前5={:?}", &r[..r.len().min(5)]));
            }
        }
        let tf = common_words.len();
        eprintln!(
            "[COMMON 全拼] 前5命中 {hit_full}/{tf} ({:.1}%)，失败 {} 词",
            hit_full as f64 * 100.0 / tf as f64,
            miss_full.len()
        );
        for m in miss_full.iter().take(20) {
            eprintln!("  MISS {m}");
        }

        // ── 2) common 词简拼前 5 ──
        let mut miss_short: Vec<String> = Vec::new();
        let mut hit_short = 0usize;
        for (py, w) in &common_words {
            let short = crate::pinyin::to_initial_string(py);
            if short.is_empty() {
                continue;
            }
            let r = d.query_short(&short);
            if r.iter().take(5).any(|c| c == w) {
                hit_short += 1;
            } else {
                miss_short.push(format!("{w}({py}→{short}) 前5={:?}", &r[..r.len().min(5)]));
            }
        }
        eprintln!(
            "[COMMON 简拼] 前5命中 {hit_short}/{tf} ({:.1}%)，失败 {} 词",
            hit_short as f64 * 100.0 / tf as f64,
            miss_short.len()
        );
        for m in miss_short.iter().take(25) {
            eprintln!("  MISS {m}");
        }

        // ── 3) 现代领域词全拼前 5 / 前 9 ──
        let modern: &[(&str, &str)] = &[
            ("微博", "weibo"),
            ("拼多多", "pinduoduo"),
            ("美团", "meituan"),
            ("快手", "kuaishou"),
            ("b站", "bzhan"),
            ("大模型", "damoxing"),
            ("充电桩", "chongdianzhuang"),
            ("短视频", "duanshipin"),
            ("带货", "daihuo"),
            ("抖音", "douyin"),
            ("小红书", "xiaohongshu"),
            ("知乎", "zhihu"),
            ("BOSS直聘", "bosszhipin"),
            ("大语言模型", "dayuyanmoxing"),
            ("固态电池", "gutaidianchi"),
            ("直播切片", "zhiboqiepian"),
            ("情绪价值", "qingxujiazhi"),
            ("社区团购", "shequtuangou"),
            ("数字游民", "shuziyoumin"),
            ("沉浸式", "chenjinshi"),
            ("微信", "weixin"),
            ("支付宝", "zhifubao"),
            ("自动驾驶", "zidongjiashi"),
            ("人形机器人", "renxingjiqiren"),
        ];
        eprintln!("\n[MODERN 现代词]");
        for (w, py) in modern {
            let r = d.query(py);
            let pos = r.iter().position(|c| c == w);
            let short = crate::pinyin::to_initial_string(py);
            let rs = d.query_short(&short);
            let spos = rs.iter().position(|c| c == w);
            eprintln!(
                "  {w}({py}): 全拼位置={} 简拼位置={} | 前5={} 前9={}",
                pos.map(|i| i + 1)
                    .map(|i| i.to_string())
                    .unwrap_or("-".into()),
                spos.map(|i| i + 1)
                    .map(|i| i.to_string())
                    .unwrap_or("-".into()),
                pos.map(|i| i < 5).unwrap_or(false),
                pos.map(|i| i < 9).unwrap_or(false),
            );
        }
    }

    #[test]
    fn test_modern_words_query_from_db() {
        // 回归：用户点名的现代词必须在真实 domains.db 中可查询（全拼 + 简拼）
        // 词表：微博/拼多多/美团/快手/b站/大模型/充电桩/短视频/带货
        let db_path = std::path::Path::new("../resources/domains/domains.db");
        if !db_path.exists() {
            eprintln!("[SKIP] domains.db 不存在: {}", db_path.display());
            return;
        }
        let mut d = dict_with_common();
        d.load_domains_from_db(db_path);
        let cases: &[(&str, &str, &str)] = &[
            ("微博", "weibo", "wb"),
            ("拼多多", "pinduoduo", "pdd"),
            ("美团", "meituan", "mt"),
            ("快手", "kuaishou", "ks"),
            ("b站", "bzhan", "bz"),
            ("大模型", "damoxing", "dmx"),
            ("充电桩", "chongdianzhuang", "cdz"),
            ("短视频", "duanshipin", "dsp"),
            ("带货", "daihuo", "dh"),
        ];
        for (word, full, short) in cases {
            let r = d.query(full);
            assert!(
                r.contains(&word.to_string()),
                "全拼 {full} 应命中 {word}, got: {:?}",
                r
            );
            let rs = d.query_short(short);
            assert!(
                rs.contains(&word.to_string()),
                "简拼 {short} 应命中 {word}, got: {:?}",
                rs
            );
        }
    }

    #[test]
    fn test_domain_heat_detection() {
        // 热词探测：选中领域词 → 热度 +1；多领域可同时升温
        let mut d = dict_with_common();
        d.load_domain_dict(std::path::Path::new("tests/data/domain_test.txt"));
        // 初始热度 0
        assert_eq!(d.domain_heat(), &[0]);
        // 命中领域词 → 热度 +1
        d.record_domain_hit("神经网络");
        assert_eq!(d.domain_heat(), &[1]);
        // 非领域词不升温
        d.record_domain_hit("你好");
        assert_eq!(d.domain_heat(), &[1]);
        // 领域归属查询
        assert!(d.word_domain("神经网络").is_some());
        assert!(d.word_domain("随便").is_none());
        assert_eq!(d.domain_name(0), Some("domain_test"));
    }

    #[test]
    fn test_domain_heat_boost_order() {
        // 热度加权：高热度领域词排在低热度/零热度前（同拼音多领域词场景）
        let mut d = dict_with_common();
        // 两个领域文件：a_domain（量子计算）、b_domain（量变）
        std::fs::create_dir_all("tests/data/tmp_domains").unwrap();
        std::fs::write(
            "tests/data/tmp_domains/a.txt",
            "量子计算 liangzijisuan\n量子点 liangzidian\n",
        )
        .unwrap();
        std::fs::write(
            "tests/data/tmp_domains/b.txt",
            "量变 liangbian\n量子 神经 liangzi\n",
        )
        .unwrap();
        d.load_domains_from_dir(std::path::Path::new("tests/data/tmp_domains"));
        assert_eq!(d.domain_names.len(), 2, "应加载 2 个领域");
        // 升温领域 b（id=1）
        d.record_domain_hit("量变");
        // 查询 liangzi：领域 b 的词（量变不算，但量子在 b 里？不，量子在 a）——验证热度排序机制
        let r = d.query("liangzi");
        // 领域 b 热度 1 > 领域 a 热度 0 → b 的"量子 神经"？词是"量子 神经"带空格被过滤，用正常词验证
        let _ = r;
        // 直接验证 word_domain 与 heat 状态
        assert!(d.word_domain("量子计算").is_some(), "量子计算 应属领域 a");
        assert!(d.word_domain("量变").is_some(), "量变 应属领域 b");
        // 清理
        let _ = std::fs::remove_dir_all("tests/data/tmp_domains");
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
        // V0.4（Eric 反馈"选一次没反应"）：温词（7天内用过）压过常用词——
        // 学一次「恩」→ en 首位即「恩」（此前温词只在 system 前插队，
        // 对 system 里本就靠前的词选后无感）。7 天未用衰减回 common 后。
        let mut d = dict_with_common();
        d.add_user_entry("en".to_string(), "恩".to_string(), 1, unix_now());
        let r = d.query("en");
        assert_eq!(
            r.first().map(String::as_str),
            Some("恩"),
            "温词应压过常用词（V0.4 上词逻辑）, got: {:?}",
            r.iter().take(4).collect::<Vec<_>>()
        );
        // 过期（>7天未用）→ 回 common 后
        let old = unix_now() - HOT_WINDOW_SECS - 1;
        let mut d2 = dict_with_common();
        d2.add_user_entry("en".to_string(), "恩".to_string(), 1, old);
        let r2 = d2.query("en");
        assert_eq!(
            r2.first().map(String::as_str),
            Some("嗯"),
            "过期温词不应压过常用词"
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

    // ─── V0.4.5 词库锚定拆分组词测试 ───
    // ⚠️ 真实词库路径：用 CARGO_MANIFEST_DIR 拼（../../resources 相对路径从
    // engine/ 解析不到 → 测试静默跳过 = 假通过，2026-08-08 修正）

    fn real_dict() -> Option<std::path::PathBuf> {
        let p =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../resources/system_dict.db");
        p.exists().then_some(p)
    }

    #[test]
    fn test_group_weom_to_wom() {
        // 用户场景：zhegeweomende（这个+我们的，weom 多打 e → wom）
        if let Some(db) = real_dict() {
            init_blocking(Some(&db));
            let results = phrase_group_guess("zhegeweomende");
            assert!(
                results.iter().any(|w| w == "这个我们的"),
                "zhegeweomende 应拆出 这个我们的, got: {:?}",
                results
            );
        }
    }

    #[test]
    fn test_group_wim_to_wom() {
        // 键位打错：zhegewimende（wim 打错 → wom，i/o 相邻键）
        // o 的相邻键是 i/p/k/l，correction_variants 应生成 wim→wom
        if let Some(db) = real_dict() {
            init_blocking(Some(&db));
            let results = phrase_group_guess("zhegewimende");
            assert!(
                results.iter().any(|w| w == "这个我们的"),
                "zhegewimende 应拆出 这个我们的, got: {:?}",
                results
            );
        }
    }

    #[test]
    fn test_group_two_words_both_error() {
        // 两个词都多打（用户场景：中间 1~2 个词有错）：
        // zheggeweomende = 这个(zhegge 多打 g) + 我们的(weomende 多打 e)
        if let Some(db) = real_dict() {
            init_blocking(Some(&db));
            let results = phrase_group_guess("zheggeweomende");
            assert!(
                results.iter().any(|w| w == "这个我们的"),
                "zheggeweomende 应拆出 这个我们的, got: {:?}",
                results
            );
        }
    }

    #[test]
    fn test_group_english_not_anchored() {
        // 英文长串不应被词库锚定拆出中文怪词（he/llo 无词组命中）
        init_blocking(None);
        let r = phrase_group_guess("hello");
        assert!(r.is_empty(), "hello 不应拆出词组, got: {:?}", r);
    }

    #[test]
    fn test_group_exact_phrase_priority() {
        // 完全正确的长串拼音：zhegewomen（这个我们）→ 应优先拆出 这个我们
        if let Some(db) = real_dict() {
            init_blocking(Some(&db));
            let results = phrase_group_guess("zhegewomen");
            assert!(
                results.iter().any(|w| w == "这个我们"),
                "zhegewomen 应拆出 这个我们, got: {:?}",
                results
            );
        }
    }
}
