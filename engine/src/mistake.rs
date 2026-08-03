/// 错音错字提示（V0.2.26）
///
/// 内置易错读音映射表：错音 → [(正确拼音, 正确词)]。
/// 输入错音时正常查询结果少 → 追加正确词候选，用户看到正确写法即自我纠正。
///
/// 选词原则（拼音输入法不区分声调，声调错不在范围内）：
/// - 只收录**声母/韵母拼写**易错的真实高频词
/// - 错音必须与正确拼写不同
/// - 单字多音不收录（场景由词级处理）

/// 易错读音映射表（错音 → [(正确拼音, 正确词)]，可扩展）
const MISTAKE_TABLE: &[(&str, &[(&str, &str)])] = &[
    // 参差 cēn cī（误 can cha / chan cha）
    ("cancha", &[("cenci", "参差")]),
    ("canchaci", &[("cenci", "参差")]),
    ("chancha", &[("cenci", "参差")]),
    // 角色 jué sè（误 jiao se）
    ("jiaose", &[("juese", "角色")]),
    ("jiaoseban", &[("jueseban", "角色扮演")]),
    // 主角 jué（误 jiao）
    ("zhujiao", &[("zhujue", "主角")]),
    // 暖和 nuǎn huo（误 nuan he）
    ("nuanhe", &[("nuanhuo", "暖和")]),
    // 暴露 bào lù（误 po lu）
    ("polu", &[("baolu", "暴露")]),
    // 果脯 guǒ fǔ（误 guo pu）
    ("guopu", &[("guofu", "果脯")]),
    // 标识 biāo zhì（误 biao shi）
    ("biaoshi", &[("biaozhi", "标识")]),
    // 发酵 fā jiào（误 fa xiao）
    ("faxiao", &[("fajiao", "发酵")]),
    // 给予 jǐ yǔ（误 gei yu）
    ("geiyu", &[("jiyu", "给予")]),
    ("geiyile", &[("jiyile", "给予了")]),
    // 龟裂 jūn liè（误 gui lie）
    ("guilie", &[("junlie", "龟裂")]),
    // 埋怨 mán yuàn（误 mai yuan）
    ("maiyuan", &[("manyuan", "埋怨")]),
    // 模样 mú yàng（误 mo yang）
    ("moyang", &[("muyang", "模样")]),
    // 血泊 xuè pō（误 xue bo）
    ("xuebo", &[("xuepo", "血泊")]),
    // 睥睨 pì nì（误 bi ni）
    ("bini", &[("pini", "睥睨")]),
    // 铜臭 tóng xiù（误 tong chou）
    ("tongchou", &[("tongxiu", "铜臭")]),
    // 威吓 wēi hè（误 wei xia）
    ("weixia", &[("weihe", "威吓")]),
    // 择菜 zhái cài（误 ze cai）
    ("zecai", &[("zhaicai", "择菜")]),
    // 慰藉 wèi jiè（误 wei ji）
    ("weiji", &[("weijie", "慰藉")]),
    // 关卡 guān qiǎ（误 guan ka）
    ("guanka", &[("guanqia", "关卡")]),
    // 力能扛鼎 gāng（误 kang ding）
    ("kangding", &[("gangding", "扛鼎")]),
    // 悄然 qiǎo（误 qiao 声调外；qiao 本身对，不收录）
];

/// 查询易错读音映射：返回 [(正确拼音, 正确词)]，未命中空
pub fn lookup(wrong_pinyin: &str) -> Vec<(&'static str, &'static str)> {
    MISTAKE_TABLE
        .iter()
        .find(|(wrong, _)| *wrong == wrong_pinyin)
        .map(|(_, entries)| entries.to_vec())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_cancha() {
        let r = lookup("cancha");
        assert!(r.iter().any(|(py, w)| *py == "cenci" && *w == "参差"));
    }

    #[test]
    fn test_lookup_nuanhe() {
        let r = lookup("nuanhe");
        assert!(r.iter().any(|(_, w)| *w == "暖和"));
    }

    #[test]
    fn test_lookup_zhujiao() {
        let r = lookup("zhujiao");
        assert!(r.iter().any(|(py, _)| *py == "zhujue"));
    }

    #[test]
    fn test_lookup_miss() {
        assert!(lookup("zhongguo").is_empty());
        assert!(lookup("").is_empty());
        assert!(lookup("xxxx").is_empty());
    }

    #[test]
    fn test_table_no_duplicate_keys() {
        // 检查无重复错音键（防表错误）
        let mut seen = std::collections::HashSet::new();
        for (wrong, _) in MISTAKE_TABLE {
            assert!(seen.insert(*wrong), "重复错音键: {wrong}");
        }
    }

    #[test]
    fn test_table_wrong_differs_from_correct() {
        // 错音必须与正确拼写不同（声调差异不算——拼音输入法不区分声调）
        for (wrong, entries) in MISTAKE_TABLE {
            for &(correct, _) in entries.iter() {
                assert_ne!(*wrong, correct, "错音=正确音无意义: {wrong}");
            }
        }
    }
}
