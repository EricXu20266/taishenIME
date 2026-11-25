/// 拆字反查模块（V0.2.25）— u + 部件拼音 → 汉字
///
/// 词库：rime-ice radical_pinyin.dict.yaml（13.2 万条）
/// 格式：字\t部件拼音(可含'分隔)\t频率
/// 反查：输入 u + 部件拼音串（去 ' 分隔符）→ 精确匹配词库 key
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 测试串行锁（V0.4.5 fix）：radical 全局状态被多个测试读写——
/// 加载表（init Some）与清空表（init None）并行竞争 → 偶发读到空表失败。
/// 所有 radical 相关测试（radical.rs tests + lib.rs tests）入口持锁串行执行。
#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

type RadicalMap = HashMap<String, Vec<(String, u32)>>;

struct RadicalState {
    path: Option<PathBuf>,
    map: Option<RadicalMap>,
    loaded: bool,
}

/// 拆字反查全局状态（Q4.2 懒加载）：启动只登记路径，首次 `query` 时才读盘解析 13.2 万行 YAML。
static RADICAL_STATE: Mutex<RadicalState> = Mutex::new(RadicalState {
    path: None,
    map: None,
    loaded: false,
});

/// 登记 radical_pinyin.dict.yaml 路径（懒加载）。`None` 清空词库；同路径重复调用幂等。
pub fn init(path: Option<&Path>) {
    let new_path = path.map(Path::to_path_buf);
    let mut state = RADICAL_STATE.lock().unwrap_or_else(|e| e.into_inner());
    if state.path == new_path {
        return;
    }
    state.path = new_path.clone();
    state.map = None;
    state.loaded = new_path.is_none();
    if new_path.is_some() {
        crate::log::info("radical 词库路径已登记（懒加载，待首查触发）");
    }
}

fn ensure_loaded() {
    let target_path = {
        let state = RADICAL_STATE.lock().unwrap_or_else(|e| e.into_inner());
        if state.loaded {
            return;
        }
        state.path.clone()
    };

    let t0 = std::time::Instant::now();
    let mut map: RadicalMap = HashMap::new();
    if let Some(ref path) = target_path {
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
            for entries in map.values_mut() {
                entries.sort_by_key(|e| std::cmp::Reverse(e.1));
            }
            crate::log::info(&format!(
                "radical 词库懒加载完成: {} 条（{}ms）",
                map.len(),
                t0.elapsed().as_millis()
            ));
        } else {
            crate::log::error("radical 词库读取失败（回退空表）");
        }
    }

    let mut state = RADICAL_STATE.lock().unwrap_or_else(|e| e.into_inner());
    if state.path == target_path && !state.loaded {
        state.map = Some(map);
        state.loaded = true;
    }
}

#[cfg(test)]
fn is_loaded() -> bool {
    let state = RADICAL_STATE.lock().unwrap_or_else(|e| e.into_inner());
    state.map.is_some()
}

#[cfg(test)]
fn is_resolved() -> bool {
    let state = RADICAL_STATE.lock().unwrap_or_else(|e| e.into_inner());
    state.loaded
}

/// 反查：部件拼音串（去 ' 分隔符）→ 候选字（按频率降序，截断 200）
pub fn query(parts: &str) -> Vec<String> {
    ensure_loaded();
    let key = parts.replace('\'', "");
    let state = RADICAL_STATE.lock().unwrap_or_else(|e| e.into_inner());
    match state.map.as_ref() {
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

    #[test]
    fn test_lazy_init_defers_disk_load_until_first_query() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = test_dict_path();
        if !path.exists() {
            return;
        }
        // 先清空状态，排除同进程前序测试残留
        init(None);
        assert!(!is_loaded());

        // 启动仅登记路径，不得立即读盘解析
        init(Some(&path));
        assert!(!is_loaded(), "init 应懒加载，未查询前不得解析词库");
        assert!(!is_resolved(), "init 后在首次 query 前 loaded 应为 false");

        // 首次 query 触发懒加载并给出候选
        let hits = query("shuishou");
        assert!(
            !hits.is_empty(),
            "首次 query 应触发加载并返回 shuishou 候选"
        );
        assert!(is_loaded());
        assert!(is_resolved());

        // 同路径重复 init 保持已加载状态（幂等不重载/不卸载）
        init(Some(&path));
        assert!(is_loaded());
        assert!(is_resolved());

        // 传 None 清空表且无候选
        init(None);
        assert!(!is_loaded());
        assert!(is_resolved());
        assert!(query("shuishou").is_empty());

        // 缺失路径在首查后也标记 loaded=true，避免反复撞盘
        let missing = path.with_file_name("missing_radical_dict_never_exists.yaml");
        init(Some(&missing));
        assert!(!is_resolved());
        assert!(query("shuishou").is_empty());
        assert!(is_resolved());
        assert!(is_loaded());
    }
}
