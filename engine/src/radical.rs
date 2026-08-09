/// 拆字反查模块（V0.2.25）— u + 部件拼音 → 汉字
///
/// 词库：rime-ice radical_pinyin.dict.yaml（13.2 万条）
/// 格式：字\t部件拼音(可含'分隔)\t频率
/// 反查：输入 u + 部件拼音串（去 ' 分隔符）→ 精确匹配词库 key
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

/// 测试串行锁（V0.4.5 fix）：radical 全局表 RADICAL 被多个测试读写——
/// 加载表（init Some）与清空表（init None）并行竞争 → 偶发读到空表失败。
/// 所有 radical 相关测试（radical.rs tests + lib.rs tests）入口持锁串行执行。
#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

/// 反查索引：规范化部件拼音串（去 '）→ [(字, 频率)]
static RADICAL: Mutex<Option<HashMap<String, Vec<(String, u32)>>>> = Mutex::new(None);
/// 已加载的 radical 词库路径（幂等判断：TSF 反复 Activate 不重复读 13.2 万条 yaml）
static RADICAL_PATH: Mutex<Option<String>> = Mutex::new(None);

/// 加载 radical_pinyin.dict.yaml。失败/路径空 → 空表（反查无候选）。
/// 幂等：已加载且路径一致 → 直接返回。
pub fn init(path: Option<&Path>) {
    let path_str = path.map(|p| p.to_string_lossy().into_owned());
    {
        let loaded = RADICAL.lock().unwrap_or_else(|e| e.into_inner()).is_some();
        let cur = RADICAL_PATH.lock().unwrap_or_else(|e| e.into_inner());
        if loaded && *cur == path_str {
            return;
        }
    }
    let mut map: HashMap<String, Vec<(String, u32)>> = HashMap::new();
    if let Some(path) = path {
        if let Ok(content) = std::fs::read_to_string(path) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') || line.starts_with("---") {
                    continue;
                }
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() != 3 {
                    continue;
                }
                let word = parts[0];
                let pys = parts[1].replace('\'', "");
                if word.is_empty()
                    || pys.is_empty()
                    || pys.chars().any(|c| !c.is_ascii_alphabetic())
                {
                    continue;
                }
                let freq: u32 = parts[2].trim().parse().unwrap_or(0);
                map.entry(pys).or_default().push((word.to_string(), freq));
            }
            crate::log::info(&format!("radical 词库加载: {} 条", map.len()));
        } else {
            crate::log::error("radical 词库读取失败（回退空表）");
        }
    }
    // 每个 key 按频率降序
    for entries in map.values_mut() {
        entries.sort_by(|a, b| b.1.cmp(&a.1));
    }
    let mut guard = RADICAL.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(map);
    *RADICAL_PATH.lock().unwrap_or_else(|e| e.into_inner()) = path_str;
}

/// 反查：部件拼音串（去 ' 分隔符）→ 候选字（按频率降序，截断 200）
pub fn query(parts: &str) -> Vec<String> {
    let key = parts.replace('\'', "");
    let guard = RADICAL.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(map) => map
            .get(&key)
            .map(|entries| entries.iter().take(200).map(|(w, _)| w.clone()).collect())
            .unwrap_or_default(),
        None => {
            // 未初始化——尝试内置（空表，无候选）
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dict_path() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../resources/rime_ice/radical_pinyin.dict.yaml")
    }

    #[test]
    fn test_init_and_query() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = test_dict_path();
        if !path.exists() {
            eprintln!("radical_pinyin.dict.yaml 不存在，跳过");
            return;
        }
        init(Some(&path));
        // 水+手 → shuishou（氵扌 相关字）
        let results = query("shuishou");
        assert!(!results.is_empty(), "shuishou 应有拆字候选");
        // 前几个候选应含 氵/扌 部首字
        let sample = results.first().unwrap();
        assert!(!sample.is_empty());
    }

    #[test]
    fn test_query_unknown_empty() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        init(None);
        assert!(query("zzzzz").is_empty());
        assert!(query("").is_empty());
    }

    #[test]
    fn test_query_normalized_separator() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // 输入带 ' 分隔与不带应等价
        let path = test_dict_path();
        if !path.exists() {
            return;
        }
        init(Some(&path));
        let with_sep = query("bai'shao");
        let without_sep = query("baishao");
        assert_eq!(with_sep, without_sep);
    }
}
