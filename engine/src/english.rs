/// 英文词典模块（P1-1，对标 rime melt_eng + cn_en）
///
/// 中英混输升级：输入串作为英文前缀，匹配内置英文单词/中英混合词，
/// 追加为候选（恒在末尾，不压拼音候选）。词条按频率降序。
///
/// 数据结构：词条 = (小写编码, 显示词, 频率)。编码与显示词分离——
/// 普通英文词编码=显示（hello→hello）；中英混合词编码=小写拼音式
/// 写法、显示保留原样（ai→AI、app→App、iphone→iPhone）。
/// 频率 300~1 递减（越靠前越常用）。

/// 英文单词 + 中英混合词表（按使用频率近似降序）
static ENGLISH_WORDS: &[(&str, &str, u32)] = &[
    // ── top 常用英文单词（code = display）──
    ("the", "the", 300),
    ("be", "be", 299),
    ("to", "to", 298),
    ("of", "of", 297),
    ("and", "and", 296),
    ("a", "a", 295),
    ("in", "in", 294),
    ("that", "that", 293),
    ("have", "have", 292),
    ("i", "i", 291),
    ("it", "it", 290),
    ("for", "for", 289),
    ("not", "not", 288),
    ("on", "on", 287),
    ("with", "with", 286),
    ("he", "he", 285),
    ("as", "as", 284),
    ("you", "you", 283),
    ("do", "do", 282),
    ("at", "at", 281),
    ("this", "this", 280),
    ("but", "but", 279),
    ("his", "his", 278),
    ("by", "by", 277),
    ("from", "from", 276),
    ("they", "they", 275),
    ("we", "we", 274),
    ("say", "say", 273),
    ("her", "her", 272),
    ("she", "she", 271),
    ("or", "or", 270),
    ("an", "an", 269),
    ("will", "will", 268),
    ("my", "my", 267),
    ("one", "one", 266),
    ("all", "all", 265),
    ("would", "would", 264),
    ("there", "there", 263),
    ("their", "their", 262),
    ("what", "what", 261),
    ("so", "so", 260),
    ("up", "up", 259),
    ("out", "out", 258),
    ("if", "if", 257),
    ("about", "about", 256),
    ("who", "who", 255),
    ("get", "get", 254),
    ("which", "which", 253),
    ("go", "go", 252),
    ("me", "me", 251),
    ("when", "when", 250),
    ("make", "make", 249),
    ("can", "can", 248),
    ("like", "like", 247),
    ("time", "time", 246),
    ("no", "no", 245),
    ("just", "just", 244),
    ("him", "him", 243),
    ("know", "know", 242),
    ("take", "take", 241),
    ("people", "people", 240),
    ("into", "into", 239),
    ("year", "year", 238),
    ("your", "your", 237),
    ("good", "good", 236),
    ("some", "some", 235),
    ("could", "could", 234),
    ("them", "them", 233),
    ("see", "see", 232),
    ("other", "other", 231),
    ("than", "than", 230),
    ("then", "then", 229),
    ("now", "now", 228),
    ("look", "look", 227),
    ("only", "only", 226),
    ("come", "come", 225),
    ("its", "its", 224),
    ("over", "over", 223),
    ("think", "think", 222),
    ("also", "also", 221),
    ("back", "back", 220),
    ("after", "after", 219),
    ("use", "use", 218),
    ("two", "two", 217),
    ("how", "how", 216),
    ("our", "our", 215),
    ("work", "work", 214),
    ("first", "first", 213),
    ("well", "well", 212),
    ("way", "way", 211),
    ("even", "even", 210),
    ("new", "new", 209),
    ("want", "want", 208),
    ("because", "because", 207),
    ("any", "any", 206),
    ("these", "these", 205),
    ("give", "give", 204),
    ("day", "day", 203),
    ("most", "most", 202),
    ("us", "us", 201),
    ("right", "right", 200),
    ("thing", "thing", 199),
    ("where", "where", 198),
    ("why", "why", 197),
    ("name", "name", 196),
    ("need", "need", 195),
    ("life", "life", 194),
    ("hand", "hand", 193),
    ("part", "part", 192),
    ("place", "place", 191),
    ("world", "world", 190),
    ("home", "home", 189),
    ("show", "show", 188),
    ("word", "word", 187),
    ("read", "read", 186),
    ("end", "end", 185),
    ("open", "open", 184),
    ("own", "own", 183),
    ("found", "found", 182),
    ("ask", "ask", 181),
    ("help", "help", 180),
    ("start", "start", 179),
    ("turn", "turn", 178),
    ("real", "real", 177),
    ("both", "both", 176),
    ("again", "again", 175),
    ("leave", "leave", 174),
    ("move", "move", 173),
    ("change", "change", 172),
    ("play", "play", 171),
    ("point", "point", 170),
    ("small", "small", 169),
    ("large", "large", 168),
    ("still", "still", 167),
    ("found", "found", 166),
    ("never", "never", 165),
    ("next", "next", 164),
    ("school", "school", 163),
    ("house", "house", 162),
    ("family", "family", 161),
    ("friend", "friend", 160),
    ("number", "number", 159),
    ("water", "water", 158),
    ("food", "food", 157),
    ("money", "money", 156),
    ("power", "power", 155),
    ("week", "week", 154),
    ("month", "month", 153),
    ("book", "book", 152),
    ("phone", "phone", 151),
    ("email", "email", 150),
    ("file", "file", 149),
    ("data", "data", 148),
    ("hello", "hello", 147),
    ("welcome", "welcome", 146),
    ("please", "please", 145),
    ("thanks", "thanks", 144),
    ("sorry", "sorry", 143),
    ("good", "good", 142),
    ("morning", "morning", 141),
    ("afternoon", "afternoon", 140),
    ("evening", "evening", 139),
    ("today", "today", 138),
    ("tomorrow", "tomorrow", 137),
    ("yesterday", "yesterday", 136),
    ("friend", "friend", 135),
    ("happy", "happy", 134),
    ("love", "love", 133),
    ("great", "great", 132),
    ("nice", "nice", 131),
    ("cool", "cool", 130),
    ("awesome", "awesome", 129),
    ("best", "best", 128),
    ("chat", "chat", 127),
    ("learn", "learn", 126),
    ("share", "share", 125),
    ("study", "study", 124),
    ("write", "write", 123),
    ("video", "video", 122),
    ("music", "music", 121),
    ("image", "image", 120),
    ("photo", "photo", 119),
    ("phone", "phone", 118),
    ("code", "code", 117),
    ("system", "system", 145),
    ("computer", "computer", 144),
    ("software", "software", 144),
    ("hardware", "hardware", 143),
    ("network", "network", 142),
    ("internet", "internet", 141),
    ("website", "website", 140),
    ("page", "page", 139),
    ("window", "window", 138),
    ("screen", "screen", 137),
    ("keyboard", "keyboard", 136),
    ("mouse", "mouse", 135),
    ("browser", "browser", 134),
    ("search", "search", 133),
    ("download", "download", 132),
    ("upload", "upload", 131),
    ("update", "update", 130),
    ("version", "version", 129),
    ("setting", "setting", 128),
    ("option", "option", 127),
    ("server", "server", 126),
    ("client", "client", 125),
    ("database", "database", 124),
    ("table", "table", 123),
    ("field", "field", 122),
    ("value", "value", 121),
    ("string", "string", 120),
    ("number", "number", 119),
    ("array", "array", 118),
    ("object", "object", 117),
    ("function", "function", 116),
    ("method", "method", 115),
    ("class", "class", 114),
    ("interface", "interface", 113),
    ("module", "module", 112),
    ("package", "package", 111),
    ("library", "library", 110),
    ("framework", "framework", 109),
    ("project", "project", 108),
    ("company", "company", 107),
    ("team", "team", 106),
    ("meeting", "meeting", 105),
    ("report", "report", 104),
    ("document", "document", 103),
    ("presentation", "presentation", 102),
    ("plan", "plan", 101),
    ("goal", "goal", 100),
    ("time", "time", 99),
    // ── 中英混合词（code 小写，display 保留原样，对标 rime cn_en）──
    ("ai", "AI", 98),
    ("ok", "OK", 97),
    ("app", "App", 96),
    ("iphone", "iPhone", 95),
    ("ipad", "iPad", 94),
    ("wifi", "WiFi", 93),
    ("wechat", "WeChat", 92),
    ("qq", "QQ", 91),
    ("word", "Word", 90),
    ("excel", "Excel", 89),
    ("ppt", "PPT", 88),
    ("pdf", "PDF", 87),
    ("gps", "GPS", 86),
    ("gdp", "GDP", 85),
    ("cpu", "CPU", 84),
    ("gpu", "GPU", 83),
    ("api", "API", 82),
    ("url", "URL", 81),
    ("html", "HTML", 80),
    ("css", "CSS", 79),
    ("http", "HTTP", 78),
    ("https", "HTTPS", 77),
    ("it", "IT", 76),
    ("ceo", "CEO", 75),
    ("cto", "CTO", 74),
    ("cfo", "CFO", 73),
    ("mvp", "MVP", 72),
    ("kpi", "KPI", 71),
    ("roi", "ROI", 70),
    ("saas", "SaaS", 69),
    ("sql", "SQL", 68),
    ("json", "JSON", 67),
    ("xml", "XML", 66),
    ("github", "GitHub", 65),
    ("git", "Git", 64),
    ("docker", "Docker", 63),
    ("linux", "Linux", 62),
    ("windows", "Windows", 61),
    ("macos", "macOS", 60),
    ("android", "Android", 59),
    ("ios", "iOS", 58),
    ("chrome", "Chrome", 57),
    ("firefox", "Firefox", 56),
    ("safari", "Safari", 55),
    ("edge", "Edge", 54),
    ("python", "Python", 53),
    ("java", "Java", 52),
    ("javascript", "JavaScript", 51),
    ("typescript", "TypeScript", 50),
    ("rust", "Rust", 49),
    ("golang", "Go", 48),
    ("react", "React", 47),
    ("vue", "Vue", 46),
    ("angular", "Angular", 45),
    ("node", "Node", 44),
    ("tensorflow", "TensorFlow", 43),
    ("pytorch", "PyTorch", 42),
    ("chatgpt", "ChatGPT", 41),
    ("openai", "OpenAI", 40),
    ("google", "Google", 39),
    ("facebook", "Facebook", 38),
    ("twitter", "Twitter", 37),
    ("youtube", "YouTube", 36),
    ("instagram", "Instagram", 35),
    ("tiktok", "TikTok", 34),
    ("amazon", "Amazon", 33),
    ("microsoft", "Microsoft", 32),
    ("apple", "Apple", 31),
    ("huawei", "Huawei", 30),
    ("xiaomi", "Xiaomi", 29),
    ("tencent", "Tencent", 28),
    ("alibaba", "Alibaba", 27),
    ("baidu", "Baidu", 26),
    ("deepseek", "DeepSeek", 25),
    ("qq", "QQ", 24),
    ("weibo", "微博", 23),
    ("zhihu", "知乎", 22),
    ("douyin", "抖音", 21),
    ("bilibili", "B站", 20),
    ("github", "GitHub", 19),
    ("vscode", "VS Code", 18),
    ("id", "ID", 17),
    ("ip", "IP", 16),
    ("usb", "USB", 15),
    ("pc", "PC", 14),
    ("mac", "Mac", 13),
    ("win", "Win", 12),
    ("app", "APP", 11),
    ("bug", "Bug", 10),
    ("todo", "TODO", 9),
    ("fix", "Fix", 8),
    ("test", "Test", 7),
    ("dev", "Dev", 6),
    ("pro", "Pro", 5),
    ("max", "Max", 4),
    ("mini", "Mini", 3),
    ("lite", "Lite", 2),
    ("beta", "Beta", 1),
];

/// 英文前缀查询：返回显示词列表（按频率降序，最多 8 个）。
/// 空前缀/无匹配返回空。
pub fn query(prefix: &str) -> Vec<String> {
    let p = prefix.to_lowercase();
    if p.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<(&str, u32)> = ENGLISH_WORDS
        .iter()
        .filter(|(code, _, _)| code.starts_with(&p))
        .map(|(_, display, freq)| (*display, *freq))
        .collect();
    hits.sort_by(|a, b| b.1.cmp(&a.1));
    hits.into_iter()
        .take(8)
        .map(|(d, _)| d.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_hello() {
        let r = query("hel");
        assert!(
            r.iter().any(|w| w == "hello"),
            "hel 应命中 hello, got {r:?}"
        );
    }

    #[test]
    fn test_query_world() {
        let r = query("wor");
        assert!(
            r.iter().any(|w| w == "world"),
            "wor 应命中 world, got {r:?}"
        );
    }

    #[test]
    fn test_query_mixed_word() {
        // 混合词：ai → AI
        let r = query("ai");
        assert!(r.iter().any(|w| w == "AI"), "ai 应命中 AI, got {r:?}");
    }

    #[test]
    fn test_query_app() {
        let r = query("app");
        assert!(r.iter().any(|w| w == "App"), "app 应命中 App, got {r:?}");
    }

    #[test]
    fn test_query_limit() {
        // 短前缀命中多，截断 8
        let r = query("a");
        assert!(r.len() <= 8, "应截断 8 个, got {}", r.len());
    }

    #[test]
    fn test_query_empty() {
        assert!(query("").is_empty());
        assert!(query("zzzz").is_empty());
    }

    #[test]
    fn test_query_case_insensitive() {
        let r1 = query("Hello");
        let r2 = query("hello");
        assert!(r1.iter().any(|w| w == "hello"));
        assert_eq!(r1, r2, "大小写不应影响结果");
    }

    #[test]
    fn test_freq_desc() {
        // the > and（频率序）
        let r = query("t");
        let the_pos = r.iter().position(|w| w == "the").unwrap_or(99);
        let r2 = query("a");
        let and_pos = r2.iter().position(|w| w == "and").unwrap_or(99);
        assert!(the_pos < 5, "the 应靠前, got {r:?}");
        assert!(and_pos < 5, "and 应靠前, got {r2:?}");
    }
}
