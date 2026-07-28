/// 词库模块 — 拼音到汉字映射
///
/// 第一期 MVP：内置静态词库，前缀查询。
/// 第二期：用户词库持久化 + 词频动态调整。

use std::collections::HashMap;
use std::sync::Mutex;

/// 词条结构
struct Entry {
    /// 拼音（小写，无空格）
    pinyin: String,
    /// 对应汉字词
    word: String,
    /// 词频（越高越靠前）
    frequency: u32,
}

/// 内置基础词库
/// 第一期 MVP：手工精选 ~50 个高频词覆盖常用场景
fn builtin_dict() -> Vec<Entry> {
    vec![
        // 单字高频
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
        // 双字词
        e("zhongguo", "中国", 500),
        e("women", "我们", 450),
        e("tamen", "他们", 440),
        e("zijide", "自己的", 430),
        e("yigeyi", "一个", 420),
        e("renwei", "认为", 400),
        e("yinwei", "因为", 390),
        e("suoyi", "所以", 380),
        e("keshi", "可是", 370),
        e("ruguoke", "如果", 360),
        e("ranhou", "然后", 350),
        e("xianzaizai", "现在", 340),
        e("meiyou", "没有", 330),
        e("shenmehen", "什么", 320),
        e("zenmezha", "怎么", 310),
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
        e("jiamia", "加密", 145),
        e("jiemi", "解密", 140),
        e("suandu", "速度", 135),
        e("anquan", "安全", 130),
        e("fuwu", "服务", 125),
        e("kehudu", "客户", 120),
        e("yonghu", "用户", 115),
        e("jieguo", "结果", 110),
        e("guocheng", "过程", 105),
        e("yanjiu", "研究", 100),
    ]
}

fn e(pinyin: &str, word: &str, frequency: u32) -> Entry {
    Entry {
        pinyin: pinyin.to_string(),
        word: word.to_string(),
        frequency,
    }
}

/// 词库 — 拼音前缀 → 候选词列表（按词频降序）
pub struct Dictionary {
    /// 拼音前缀索引：prefix → [(word, frequency)]
    index: HashMap<String, Vec<(String, u32)>>,
}

impl Dictionary {
    pub fn new() -> Self {
        let mut dict = Self {
            index: HashMap::new(),
        };
        dict.load_builtin();
        dict
    }

    /// 加载内置词库
    fn load_builtin(&mut self) {
        for entry in builtin_dict() {
            // 为每个可能的前缀建立索引
            let pinyin = &entry.pinyin;
            for i in 1..=pinyin.len() {
                let prefix = &pinyin[..i];
                self.index
                    .entry(prefix.to_string())
                    .or_default()
                    .push((entry.word.clone(), entry.frequency));
            }
        }

        // 每个前缀的候选词按频率降序排列
        for entries in self.index.values_mut() {
            entries.sort_by(|a, b| b.1.cmp(&a.1));
        }
    }

    /// 根据拼音前缀查询候选词
    pub fn query(&self, pinyin_prefix: &str) -> Vec<String> {
        let key = pinyin_prefix.to_lowercase();
        match self.index.get(&key) {
            Some(entries) => entries.iter().map(|(w, _)| w.clone()).collect(),
            None => Vec::new(),
        }
    }
}

/// 全局词库查询函数（供 Engine 调用）
static DICT: Mutex<Option<Dictionary>> = Mutex::new(None);

pub fn ensure_initialized() {
    let mut dict = DICT.lock().unwrap();
    if dict.is_none() {
        *dict = Some(Dictionary::new());
    }
}

pub fn query(pinyin_prefix: &str) -> Vec<String> {
    ensure_initialized();
    DICT.lock().unwrap().as_ref().unwrap().query(pinyin_prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_single() {
        ensure_initialized();
        let results = query("zhong");
        assert!(results.contains(&"中".to_string()));
        assert!(results.iter().any(|w| w == "中"));
    }

    #[test]
    fn test_query_empty() {
        ensure_initialized();
        let results = query("zzz");
        assert!(results.is_empty());
    }
}
