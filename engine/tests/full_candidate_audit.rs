//! 候选全量体检（V0.6 候选优化回归资产）— 走完整 FFI 链路 + 真实词库。
//! 输入: tmp/audit_cases.tsv（type<TAB>input<TAB>expect，由 tmp/gen_audit_cases.py 生成）
//! 判定: 首屏 = 前 9 候选（candidate_count=9，与部署默认一致）
//! 输出: tmp/audit_report.txt（统计）+ tmp/audit_fails.tsv（未命中明细，实时落盘）
//!
//! 运行（#[ignore] 需显式指定）:
//!   cargo test --release --test full_candidate_audit -- --ignored --nocapture
//! 调试参数: AUDIT_OFFSET / AUDIT_LIMIT 控制起始/条数（分段跑，防引擎隐藏 bug 崩溃丢结果）。
//! 已知：word2/word3 区间连续查询 3000+ 条后引擎偶发崩溃（累积型，哈希布局相关），
//!       分段（≤300 条）跑可规避；崩溃不丢已落盘结果。

use std::ffi::CStr;
use std::io::Write;
use std::os::raw::c_char;

use taishen_engine::ffi::*;

/// 从 FFI 缓冲区读取 C 字符串
unsafe fn read_cstr(buf: &[u8]) -> String {
    unsafe {
        CStr::from_ptr(buf.as_ptr() as *const c_char)
            .to_string_lossy()
            .into_owned()
    }
}

/// 输入拼音，返回前 9 候选（首屏）
unsafe fn type_and_top9(py: &str) -> Vec<String> {
    engine_reset();
    for ch in py.chars() {
        engine_process_key(ch as i32);
    }
    let n = engine_get_candidate_count();
    let take = n.min(9);
    let mut out = Vec::with_capacity(take as usize);
    for i in 0..take {
        let mut b = [0u8; 64];
        engine_get_candidate(i, b.as_mut_ptr() as *mut c_char, b.len() as i32);
        out.push(read_cstr(&b));
    }
    out
}

#[test]
#[ignore = "长跑回归体检：显式运行，见文件头说明"]
fn full_candidate_audit() {
    unsafe {
        // ── 真实词库初始化（与部署目录同源 resources/system_dict.db）──
        let dict = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("resources/system_dict.db");
        let dict_c = std::ffi::CString::new(dict.to_string_lossy().as_bytes()).unwrap();
        assert_eq!(engine_init(dict_c.as_ptr()), 0, "engine_init 应成功");
        engine_set_candidate_count(9);
        let mut waited_ms = 0;
        while engine_dict_ready() == 0 && waited_ms < 20_000 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            waited_ms += 100;
        }
        assert_eq!(engine_dict_ready(), 1, "真实词库应就绪");
        // 等待 domains 后台加载完成（异步线程持 DICT 锁 2-3s），
        // 避免查询线程与加载线程并发竞争触发非确定性崩溃（panic 穿 FFI → abort）。
        std::thread::sleep(std::time::Duration::from_millis(4000));
        println!("词库就绪（含 domains 加载窗口等待）");

        // ── 读取用例 ──
        let cases_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tmp/audit_cases.tsv");
        let content = std::fs::read_to_string(&cases_path).expect("audit_cases.tsv 应存在");
        let mut cases: Vec<(String, String, String)> = Vec::new();
        for line in content.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() == 3 {
                cases.push((
                    parts[0].to_string(),
                    parts[1].to_string(),
                    parts[2].to_string(),
                ));
            }
        }
        // 调试：AUDIT_LIMIT 控制条数，AUDIT_OFFSET 控制起始位置
        let offset: usize = std::env::var("AUDIT_OFFSET")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let limit: usize = std::env::var("AUDIT_LIMIT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(usize::MAX);
        if offset > 0 {
            cases.drain(..offset.min(cases.len()));
            println!("[调试] AUDIT_OFFSET={offset}，从第 {} 条开始", offset + 1);
        }
        if limit < cases.len() {
            cases.truncate(limit);
            println!("[调试] AUDIT_LIMIT={limit}，本次共跑 {} 条", cases.len());
        }
        println!("用例总数: {}", cases.len());

        // ── 逐条体检（实时落盘：崩溃不丢已验结果）──
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let progress_path = root.join("tmp/audit_progress.txt");
        let fail_path = root.join("tmp/audit_fails.tsv");
        std::fs::write(&progress_path, "started").ok();

        let mut by_type: std::collections::HashMap<String, (usize, usize)> =
            std::collections::HashMap::new();
        let mut fail_file = std::fs::File::create(&fail_path).expect("创建 fails 文件失败");
        let mut fail_count = 0usize;

        for (i, (ty, input, expect)) in cases.iter().enumerate() {
            let top9 = type_and_top9(input);
            let hit = top9.iter().any(|c| c == expect);
            let e = by_type.entry(ty.clone()).or_insert((0, 0));
            e.1 += 1;
            if hit {
                e.0 += 1;
            } else {
                fail_count += 1;
                let line = format!(
                    "{ty}\t{input}\t{expect}\t{}\n",
                    top9.iter().cloned().collect::<Vec<_>>().join("/")
                );
                fail_file.write_all(line.as_bytes()).ok();
            }
            if i % 100 == 0 {
                std::fs::write(
                    &progress_path,
                    format!("{i}/{} last_input={}", cases.len(), input),
                )
                .ok();
            }
        }
        fail_file.flush().ok();
        std::fs::write(&progress_path, "done").ok();

        // ── 报告 ──
        let mut report = String::new();
        report.push_str(&format!(
            "候选全量体检报告\n首屏标准: 前 9 候选\n用例总数: {}\n\n",
            cases.len()
        ));

        report.push_str("═══ 各维度命中率 ═══\n");
        let mut total_pass = 0usize;
        let mut total_all = 0usize;
        let mut types: Vec<_> = by_type.keys().cloned().collect();
        types.sort();
        for ty in &types {
            let (pass, all) = by_type[ty];
            total_pass += pass;
            total_all += all;
            let rate = if all > 0 {
                pass as f64 * 100.0 / all as f64
            } else {
                0.0
            };
            report.push_str(&format!("{ty:<8} {pass:>6}/{all:<6} {:>5.1}%\n", rate));
        }
        if total_all > 0 {
            let rate = total_pass as f64 * 100.0 / total_all as f64;
            report.push_str(&format!(
                "────────────────\n总命中   {total_pass:>6}/{total_all:<6} {:>5.1}%\n\n",
                rate
            ));
        }
        report.push_str(&format!(
            "未命中总数: {fail_count}（明细见 tmp/audit_fails.tsv）\n"
        ));

        let report_path = root.join("tmp/audit_report.txt");
        let mut f = std::fs::File::create(&report_path).expect("创建报告失败");
        f.write_all(report.as_bytes()).expect("写报告失败");
        println!("报告已写入: {}", report_path.display());

        // 摘要到 stdout
        println!("═══ 摘要 ═══");
        for ty in &types {
            let (pass, all) = by_type[ty];
            let rate = if all > 0 {
                pass as f64 * 100.0 / all as f64
            } else {
                0.0
            };
            println!("{ty:<8} {pass:>6}/{all:<6} {:>5.1}%", rate);
        }
        println!("总命中   {total_pass}/{total_all}");
        println!("未命中总数: {fail_count}");

        engine_destroy();
    }
}
