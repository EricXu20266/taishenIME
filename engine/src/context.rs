/// 上下文联想（P1-1，对标搜狗/微软前文关联候选）
///
/// 维护"前词 → 后词"搭配表：上屏"北京"后输入 da，因"大学"在
/// context_map["北京"] 中 → 候选前置。
///
/// 内置精选高频搭配；外部可通过 load 覆盖/补充（同前词条目覆盖）。
/// 只收录真实高频搭配（人工精选 + 可扩展），避免误前置。
use std::collections::HashMap;

/// 内置上下文搭配表（前词 → 后词列表，可扩展）
fn builtin() -> HashMap<String, Vec<String>> {
    let mut m = HashMap::new();
    // 地名/机构 + 常见后缀
    m.insert(
        "北京".to_string(),
        vec![
            "大学".to_string(),
            "市".to_string(),
            "时间".to_string(),
            "人".to_string(),
            "欢迎你".to_string(),
        ],
    );
    m.insert(
        "中国".to_string(),
        vec![
            "人民".to_string(),
            "特色".to_string(),
            "梦".to_string(),
            "制造".to_string(),
            "电信".to_string(),
        ],
    );
    m.insert(
        "上海".to_string(),
        vec![
            "市".to_string(),
            "人".to_string(),
            "交通".to_string(),
            "大学".to_string(),
        ],
    );
    m.insert(
        "深圳".to_string(),
        vec!["市".to_string(), "科技园".to_string(), "机场".to_string()],
    );
    // 时间 + 常见搭配
    m.insert(
        "今天".to_string(),
        vec![
            "天气".to_string(),
            "晚上".to_string(),
            "早上".to_string(),
            "上班".to_string(),
            "开会".to_string(),
        ],
    );
    m.insert(
        "明天".to_string(),
        vec![
            "早上".to_string(),
            "晚上".to_string(),
            "开会".to_string(),
            "天气".to_string(),
        ],
    );
    m.insert(
        "晚上".to_string(),
        vec![
            "好".to_string(),
            "吃饭".to_string(),
            "开会".to_string(),
            "睡觉".to_string(),
        ],
    );
    // 代词 + 高频动词/助词
    m.insert(
        "我们".to_string(),
        vec![
            "的".to_string(),
            "要".to_string(),
            "可以".to_string(),
            "一起".to_string(),
            "公司".to_string(),
        ],
    );
    m.insert(
        "你们".to_string(),
        vec![
            "好".to_string(),
            "的".to_string(),
            "要".to_string(),
            "公司".to_string(),
        ],
    );
    m.insert(
        "他们".to_string(),
        vec![
            "的".to_string(),
            "要".to_string(),
            "公司".to_string(),
            "已经".to_string(),
        ],
    );
    // 学习/工作场景
    m.insert(
        "学习".to_string(),
        vec![
            "能力".to_string(),
            "成绩".to_string(),
            "方法".to_string(),
            "英语".to_string(),
            "进步".to_string(),
        ],
    );
    m.insert(
        "工作".to_string(),
        vec![
            "经验".to_string(),
            "内容".to_string(),
            "环境".to_string(),
            "计划".to_string(),
            "报告".to_string(),
        ],
    );
    m.insert(
        "项目".to_string(),
        vec![
            "管理".to_string(),
            "进度".to_string(),
            "计划".to_string(),
            "需求".to_string(),
            "文档".to_string(),
        ],
    );
    m
}

/// 查询前词的后词搭配列表（无则空）
pub fn lookup(prev: &str) -> Vec<String> {
    builtin().get(prev).map(|v| v.clone()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_lookup_hit() {
        let v = lookup("北京");
        assert!(v.contains(&"大学".to_string()), "北京 应含 大学 搭配");
        assert!(v.contains(&"市".to_string()));
    }

    #[test]
    fn test_context_lookup_miss() {
        let v = lookup("不存在的词xyz");
        assert!(v.is_empty(), "未知前词应返回空");
    }
}
