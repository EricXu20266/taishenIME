//! 部署工具：为系统词库 SQLite 预生成 .bin 预编译索引（V0.5.13 P1）。
//!
//! 用法：cargo run --release --bin build_index -- <dict.db> <out.bin>
//! 打包脚本 package.ps1 在产物收集前调用，保证安装包自带 .bin（首启秒开，
//! 用户机器首次激活不再走 SQLite 全量重建 6-7s）。
//!
//! 幂等：每次运行全量重建（system_dict.db 变更后由 package.ps1 按 mtime 判断触发）。

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("用法: build_index <dict.db> <out.bin>");
        std::process::exit(2);
    }
    let dict = std::path::Path::new(&args[1]);
    let out = std::path::Path::new(&args[2]);
    match taishen_engine::dictionary::build_index(dict, out) {
        Ok(()) => {
            let size = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
            println!(
                "[OK] {} -> {} ({} MB)",
                dict.display(),
                out.display(),
                size / 1024 / 1024
            );
        }
        Err(e) => {
            eprintln!("[ERROR] build_index 失败: {e}");
            std::process::exit(1);
        }
    }
}
