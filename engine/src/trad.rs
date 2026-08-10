/// 简繁转换模块 — 简体 → 繁体（V0.2.11）
///
/// 输入拼音时可按需输出繁体（港台用户/书面写作）。开启后候选显示繁体。
/// 转换是输出层转换：词库/用户词库仍存简体，select_candidate 返回前转繁体。
///
/// 策略：词组优先（处理单字映射歧义，如 系统→系統 vs 关系→關係），
/// 剩余逐字查表（trad_full 1313 常用字简→繁）；无映射字（简繁同形）保持原样。

/// 常用简→繁词组映射（处理单字歧义，如 头发→頭髮 vs 发现→發現）
fn build_trad_phrases() -> Vec<(&'static str, &'static str)> {
    vec![
        ("头发", "頭髮"),
        ("发现", "發現"),
        ("出现", "出現"),
        ("发展", "發展"),
        ("出发", "出發"),
        ("发生", "發生"),
        ("开发", "開發"),
        ("发车", "發車"),
        ("发送", "發送"),
        ("发票", "發票"),
        ("发布", "發佈"),
        ("发财", "發財"),
        ("发明", "發明"),
        ("发表", "發表"),
        ("发动", "發動"),
        ("发射", "發射"),
        ("头发", "頭髮"),
        ("理发", "理髮"),
        ("洗发", "洗髮"),
        ("干洗", "乾洗"),
        ("干净", "乾淨"),
        ("干部", "幹部"),
        ("干活", "幹活"),
        ("干杯", "乾杯"),
        ("干什么", "幹什麼"),
        ("微软", "微軟"),
        ("软件", "軟體"),
        ("硬件", "硬體"),
        ("网络", "網路"),
        ("信息", "資訊"),
        ("程序", "程式"),
        ("激光", "雷射"),
        ("硬盘", "硬碟"),
        ("屏幕", "螢幕"),
        ("鼠标", "滑鼠"),
        ("打印机", "印表機"),
        ("数据库", "資料庫"),
        ("服务器", "伺服器"),
        ("中国", "中國"),
        ("台湾", "臺灣"),
        ("项目", "項目"),
        ("用户", "用戶"),
        ("支持", "支持"),
        ("标准", "標準"),
        ("样式", "樣式"),
        ("内容", "內容"),
        ("下载", "下載"),
        ("上传", "上傳"),
        ("网站", "網站"),
        ("网页", "網頁"),
        ("链接", "連結"),
        ("浏览器", "瀏覽器"),
        ("数字", "數字"),
        ("文档", "文件"),
        ("文件", "檔案"),
        ("桌面", "桌面"),
        ("图标", "圖示"),
        ("窗口", "視窗"),
        ("菜单", "選單"),
        ("设置", "設定"),
        ("配置", "組態"),
        ("版本", "版本"),
        ("更新", "更新"),
        ("升级", "升級"),
        ("重新", "重新"),
        ("启动", "啟動"),
        ("运行", "執行"),
        ("打开", "開啟"),
        ("关闭", "關閉"),
        ("保存", "儲存"),
        ("删除", "刪除"),
        ("新建", "新增"),
        ("复制", "複製"),
        ("粘贴", "貼上"),
        ("剪切", "剪下"),
        ("撤销", "復原"),
        ("重做", "重做"),
        ("查找", "搜尋"),
        ("替换", "取代"),
        ("帮助", "說明"),
        ("关于", "關於"),
        ("退出", "結束"),
        ("登录", "登入"),
        ("注册", "註冊"),
        ("密码", "密碼"),
        ("账号", "帳號"),
        ("搜索", "搜尋"),
        ("音乐", "音樂"),
        ("电影", "電影"),
        ("视频", "影片"),
        ("图片", "圖片"),
        ("照片", "照片"),
        ("聊天", "聊天"),
        ("朋友", "朋友"),
        ("工作", "工作"),
        ("学习", "學習"),
        ("生活", "生活"),
        ("手机", "手機"),
        ("电脑", "電腦"),
        ("键盘", "鍵盤"),
        ("鼠标", "滑鼠"),
        ("屏幕", "螢幕"),
        // V0.5.6 多音字歧义词组（逐字转换会错，词组优先纠正）：
        // 系(系统/关系)、制(控制/制造)、后(以后/皇后/后面)、干(干/乾/幹)、里/台/面
        ("系统", "系統"),
        ("关系", "關係"),
        ("联系", "聯繫"),
        ("体系", "體系"),
        ("系列", "系列"),
        ("控制", "控制"),
        ("制造", "製造"),
        ("制度", "制度"),
        ("以后", "以後"),
        ("皇后", "皇后"),
        ("后面", "後面"),
        ("后面", "後面"),
        ("里面", "裡面"),
        ("干部", "幹部"),
        ("干活", "幹活"),
        ("干净", "乾淨"),
        ("干枯", "乾枯"),
        ("台湾", "臺灣"),
        ("台球", "撞球"),
        ("台风", "颱風"),
        ("面子", "面子"),
        ("面条", "麵條"),
        ("对面", "對面"),
        ("里面", "裡面"),
        ("公里", "公里"),
        ("公里", "公里"),
    ]
}

/// 简体文本 → 繁体（词组优先 + 逐字查表；无映射保持原样）
/// V0.5.6：逐字表升级为 trad_full（GB2312 一级 1313 常用字简→繁），
/// 覆盖 测/试/们/系 等此前缺失的常用字（我们→我們、测试→測試）。
/// 多音字歧义由词组表（build_trad_phrases）优先处理（系统→系統、关系→關係）。
pub fn to_traditional(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let phrases = build_trad_phrases();
    let map = crate::trad_full::trad_map();

    // 词组匹配：贪心最长匹配
    let mut result = String::with_capacity(text.len() * 2);
    let mut i = 0;
    let chars: Vec<char> = text.chars().collect();
    while i < chars.len() {
        // 尝试词组（最长 4 字）
        let mut matched = false;
        for len in (2..=4).rev() {
            if i + len > chars.len() {
                continue;
            }
            let word: String = chars[i..i + len].iter().collect();
            if let Some(trad) = phrases.iter().find(|(s, _)| *s == word) {
                result.push_str(trad.1);
                i += len;
                matched = true;
                break;
            }
        }
        if matched {
            continue;
        }
        // 单字查表
        let ch = chars[i];
        match map.get(&ch) {
            Some(t) => result.push(*t),
            None => result.push(ch),
        }
        i += 1;
    }
    result
}

/// 判断文本是否含繁体独有字（V0.5.6 简繁隔离用）：
/// 原生繁体词条（系統控制臺）识别后不再走 to_traditional 二次转换，
/// 避免多音字歧义（系统→繫統、控制→控製）。
pub fn is_traditional(text: &str) -> bool {
    text.chars()
        .any(|c| crate::trad_simp::simp_map().contains_key(&c))
}

/// 繁体文本 → 简体（V0.5.3：词库加载层简繁归一化）。
/// 逐字查表（trad_simp 繁→简表，覆盖词库实际出现的 1698 个繁体字）。
/// 繁→简方向多对一收敛（髮/發→发、乾/乹→干），无需词组匹配；
/// 简体/简繁同形/生僻字保持原样。用途：词库混入繁体词条时统一转简体，
/// 保证简体模式候选不出现繁体。
pub fn to_simplified(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let map = crate::trad_simp::simp_map();
    let mut result = String::with_capacity(text.len());
    for ch in text.chars() {
        match map.get(&ch) {
            Some(s) => result.push(*s),
            None => result.push(ch),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_char() {
        assert_eq!(to_traditional("中国"), "中國");
        assert_eq!(to_traditional("学习"), "學習");
        assert_eq!(to_traditional("你好"), "你好"); // 简繁同形
    }

    #[test]
    fn test_phrase_disambiguation() {
        // 头发 → 頭髮（单字"发"映射是 發，词组覆盖为 髮）
        assert_eq!(to_traditional("头发"), "頭髮");
        assert_eq!(to_traditional("发现"), "發現");
    }

    #[test]
    fn test_no_mapping_unchanged() {
        assert_eq!(to_traditional("abc"), "abc");
        assert_eq!(to_traditional(""), "");
    }

    #[test]
    fn test_mixed() {
        assert_eq!(to_traditional("中国软件"), "中國軟體");
    }

    #[test]
    fn test_to_simplified() {
        assert_eq!(to_simplified("中國"), "中国");
        assert_eq!(to_simplified("學習"), "学习");
        assert_eq!(to_simplified("我們"), "我们");
        assert_eq!(to_simplified("側視"), "侧视");
        assert_eq!(to_simplified("我們的出口"), "我们的出口");
        assert_eq!(to_simplified("我們的奇蹟"), "我们的奇迹");
        assert_eq!(to_simplified("你好"), "你好"); // 简繁同形
        assert_eq!(to_simplified("abc"), "abc");
        assert_eq!(to_simplified(""), "");
        // 简体输入保持原样
        assert_eq!(to_simplified("中国软件"), "中国软件");
    }

    #[test]
    fn test_simplified_roundtrip() {
        // 简→繁→简 回环
        assert_eq!(
            to_simplified(&to_traditional("中华人民共和国")),
            "中华人民共和国"
        );
        assert_eq!(to_simplified(&to_traditional("台湾地区")), "台湾地区");
    }

    #[test]
    fn test_qian_not_mapped_to_gan() {
        // 乾 是简繁同形多音字：qián 音（乾隆/乾坤/乾县）在简体词库中大量出现，
        // 若逐字映射 乾→干 会把它们错误转成 干隆/干坤/干县。
        assert_eq!(to_simplified("乾隆"), "乾隆");
        assert_eq!(to_simplified("乾坤"), "乾坤");
        assert_eq!(to_simplified("乾县"), "乾县");
        assert_eq!(to_simplified("承乾宫"), "承乾宫");
        // 其他繁体字仍正常转换
        assert_eq!(to_simplified("質"), "质");
        assert_eq!(to_simplified("我們的"), "我们的");
    }
}
