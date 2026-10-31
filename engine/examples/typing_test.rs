//! 打字准确度与性能测试（全拼 + 双拼）。
//!
//! 用法：
//!   cargo run --release --example typing_test
//!   TAISHEN_DICT=<system_dict.db 绝对路径> cargo run --release --example typing_test
//!
//! 输出每例一行（便于 diff 前后版本）：
//!   CASE <mode> <input> <expect> <rank 或 ->
//! 末尾输出汇总：首选命中率、top5 命中率、单键延迟 P50/P95。

use std::path::{Path, PathBuf};
use std::time::Instant;

use taishen_engine::Engine;

struct Case {
    pinyin: &'static str,
    syllables: &'static [&'static str],
    expect: &'static str,
}

const CASES: &[Case] = &[
    Case { pinyin: "nihao", syllables: &["ni", "hao"], expect: "你好" },
    Case { pinyin: "zhongguo", syllables: &["zhong", "guo"], expect: "中国" },
    Case { pinyin: "women", syllables: &["wo", "men"], expect: "我们" },
    Case { pinyin: "shijian", syllables: &["shi", "jian"], expect: "时间" },
    Case { pinyin: "wenti", syllables: &["wen", "ti"], expect: "问题" },
    Case { pinyin: "pengyou", syllables: &["peng", "you"], expect: "朋友" },
    Case { pinyin: "jintian", syllables: &["jin", "tian"], expect: "今天" },
    Case { pinyin: "mingnian", syllables: &["ming", "nian"], expect: "明年" },
    Case { pinyin: "gongzuo", syllables: &["gong", "zuo"], expect: "工作" },
    Case { pinyin: "daxue", syllables: &["da", "xue"], expect: "大学" },
    Case { pinyin: "dianhua", syllables: &["dian", "hua"], expect: "电话" },
    Case { pinyin: "xihuan", syllables: &["xi", "huan"], expect: "喜欢" },
    Case { pinyin: "zhidao", syllables: &["zhi", "dao"], expect: "知道" },
    Case { pinyin: "meiyou", syllables: &["mei", "you"], expect: "没有" },
    Case { pinyin: "suoyi", syllables: &["suo", "yi"], expect: "所以" },
    Case { pinyin: "renmin", syllables: &["ren", "min"], expect: "人民" },
    Case { pinyin: "guojia", syllables: &["guo", "jia"], expect: "国家" },
    Case { pinyin: "tianqi", syllables: &["tian", "qi"], expect: "天气" },
    Case { pinyin: "yisi", syllables: &["yi", "si"], expect: "意思" },
    Case { pinyin: "wenhua", syllables: &["wen", "hua"], expect: "文化" },
    Case { pinyin: "jiaoyu", syllables: &["jiao", "yu"], expect: "教育" },
    Case { pinyin: "jingji", syllables: &["jing", "ji"], expect: "经济" },
    Case { pinyin: "shehui", syllables: &["she", "hui"], expect: "社会" },
    Case { pinyin: "fazhan", syllables: &["fa", "zhan"], expect: "发展" },
    Case { pinyin: "yingxiang", syllables: &["ying", "xiang"], expect: "影响" },
    Case { pinyin: "wenjian", syllables: &["wen", "jian"], expect: "文件" },
    Case { pinyin: "xiangmu", syllables: &["xiang", "mu"], expect: "项目" },
    Case { pinyin: "ceshi", syllables: &["ce", "shi"], expect: "测试" },
    Case { pinyin: "jiancha", syllables: &["jian", "cha"], expect: "检查" },
    Case { pinyin: "hangye", syllables: &["hang", "ye"], expect: "行业" },
];

fn dict_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TAISHEN_DICT") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
        eprintln!("TAISHEN_DICT 指向的文件不存在: {}", p.display());
        return None;
    }
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .join("resources/system_dict.db");
    p.exists().then_some(p)
}

fn p50_p95(mut v: Vec<u64>) -> (u64, u64) {
    if v.is_empty() {
        return (0, 0);
    }
    v.sort_unstable();
    (v[v.len() / 2], v[v.len() * 95 / 100])
}

fn main() {
    let Some(dict) = dict_path() else {
        eprintln!("找不到词库，退出");
        return;
    };
    taishen_engine::dictionary::init(Some(dict.as_path()));
    for _ in 0..600 {
        if taishen_engine::dictionary::is_ready() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    if !taishen_engine::dictionary::is_ready() {
        eprintln!("词库未就绪，退出");
        return;
    }
    let scheme = taishen_engine::shuangpin::find_scheme("mspy").expect("mspy 方案应存在");

    let mut full_lat = Vec::new();
    let mut shuang_lat = Vec::new();
    let mut full_top1 = 0;
    let mut full_top5 = 0;
    let mut shuang_top1 = 0;
    let mut shuang_top5 = 0;
    let mut shuang_encode_fail = 0;

    for mode in ["full", "shuang"] {
        for case in CASES {
            let input = if mode == "full" {
                case.pinyin.to_string()
            } else {
                match case
                    .syllables
                    .iter()
                    .map(|s| scheme.encode(s))
                    .collect::<Option<String>>()
                {
                    Some(code) => code,
                    None => {
                        shuang_encode_fail += 1;
                        println!("CASE\t{mode}\tENCODE_FAIL\t{}\t-", case.expect);
                        continue;
                    }
                }
            };

            let mut engine = Engine::new();
            if mode == "shuang" {
                engine.set_shuangpin_mode(true);
                engine.set_shuangpin_scheme("mspy");
            }
            // 预热一次，排除首查建页开销
            for ch in "a".chars() {
                engine.process_key(ch);
            }
            engine.reset();

            // 每例重复 20 次，取末键耗时中位数（单次抖动太大）
            let mut lat: Vec<u64> = Vec::with_capacity(20);
            let mut top5: Vec<String> = Vec::new();
            for rep in 0..20 {
                engine.reset();
                let mut us = 0;
                for ch in input.chars() {
                    let t = Instant::now();
                    engine.process_key(ch);
                    us = t.elapsed().as_micros() as u64;
                }
                if rep == 0 {
                    top5 = (0..5)
                        .filter_map(|i| engine.candidate(i).map(|s| s.to_string()))
                        .collect();
                }
                lat.push(us);
            }
            lat.sort_unstable();
            let last_us = lat[lat.len() / 2];
            let rank = top5.iter().position(|w| w == case.expect);
            match rank {
                Some(0) => {
                    if mode == "full" {
                        full_top1 += 1;
                        full_top5 += 1;
                    } else {
                        shuang_top1 += 1;
                        shuang_top5 += 1;
                    }
                }
                Some(_) => {
                    if mode == "full" {
                        full_top5 += 1;
                    } else {
                        shuang_top5 += 1;
                    }
                }
                None => {}
            }
            if mode == "full" {
                full_lat.push(last_us);
            } else {
                shuang_lat.push(last_us);
            }
            println!(
                "CASE\t{mode}\t{input}\t{}\t{}",
                case.expect,
                rank.map(|r| r.to_string()).unwrap_or_else(|| "-".to_string())
            );
        }
    }

    let total = CASES.len() as f64;
    let (f50, f95) = p50_p95(full_lat);
    let (s50, s95) = p50_p95(shuang_lat);
    println!(
        "SUM\tfull\ttop1={:.0}%\ttop5={:.0}%\tp50={f50}us\tp95={f95}us",
        full_top1 as f64 / total * 100.0,
        full_top5 as f64 / total * 100.0
    );
    println!(
        "SUM\tshuang\ttop1={:.0}%\ttop5={:.0}%\tp50={s50}us\tp95={s95}us\tencode_fail={shuang_encode_fail}",
        shuang_top1 as f64 / total * 100.0,
        shuang_top5 as f64 / total * 100.0
    );
}
