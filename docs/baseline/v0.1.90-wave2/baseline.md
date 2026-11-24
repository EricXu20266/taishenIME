# 泰深输入法基线报告 v0.1.90

- 采集时间：2026-11-24 10:03:33
- commit：`8dc4e2a`（工作区无未提交改动）
- 词库版本：`2026.11.21.8`

## 1. 构建产物与依赖（V4 打薄基准）

| 产物 | 体积 | SHA256(16) | 依赖命中 |
|---|---:|---|---|
| `platform/windows/out/taishen_ime.dll` | 7.18 MB | `9be7780547162ed2` | d2d1.dll, DWrite.dll, ws2_32.dll, reqwest, hyper, h2- |
| `platform/windows/out/taishen_ime_x86.dll` | 6.05 MB | `f3dba95de307a7c9` | d2d1.dll, DWrite.dll, ws2_32.dll, reqwest, hyper, h2- |
| `platform/windows/out/taishen_server.exe` | 6.91 MB | `b15d6fb24fed0815` | d2d1.dll, DWrite.dll, ws2_32.dll, reqwest, hyper, h2- |
| `engine/target/release/taishen_engine.dll` | 6.32 MB | `817109265138da6e` | ws2_32.dll, reqwest, hyper, h2- |

## 2. V4 收口状态

- 普通宿主渲染默认：bool use_server_render = true;
- 普通宿主引擎默认：bool use_server_engine = true;
- IPC 协议版本：constexpr uint32_t kMinSupportedIpcVersion = 2;; constexpr uint32_t kRenderIpcVersion = 3;
- ime.dll 仍链接引擎静态库：是
- ime.dll 仍链接 d2d1/dwrite：4 处

## 3. 代码规模

| 口径 | 文件数 | 行数 |
|---|---:|---:|
| engine/src (*.rs) | 29 | 36399 |
| engine/** (*.rs，含 examples/tests) | 129 | 82820 |
| platform/windows/src (cpp+h) | 34 | 18894 |
| platform/windows/** (cpp+h，含 test) | 88 | 29547 |

Rust 单元测试函数：**430**

## 4. 词库

| 文件 | 体积 | 词条 |
|---|---:|---|
| `system_dict.db` | 32.56 MB | system_dict=371735, system_dict_trad=18669 |
| `domains.db` | 25.62 MB | domain_words=176620, domain_words_trad=19872 |
| `common.db` | 0.07 MB | common_words=2618 |
| `bigram.db` | 23.42 MB | context_prior=385992 |
| `system_dict.db.bin` | 74234584 B | （预编译索引） |
| `bigram.db.bin` | 8432104 B | （预编译索引） |

## 5. 考试成绩

| 分级 | 通过 | 占比 |
|---|---:|---:|
| L1 | 92/96 | 95.8% |
| L2 | 59/63 | 93.7% |
| L3 | 13/13 | 100.0% |
| L4 | 66/75 | 88.0% |
| L5 | 58/60 | 96.7% |
| **总计** | **279/292** | **95.5%** |

已知缺陷组：9/15（60.0%）

exam 退出码：1（0=全通过，1=有 FAIL）

## 6. 性能与内存

| 输入 | limit | 总耗时(us) | 单键峰值(us) |
|---|---:|---:|---:|
| `nihao` | 5 | 3869 | 1856 |
| `nihao` | 9 | 5469 | 2193 |
| `keyima` | 5 | 7262 | 3462 |
| `keyima` | 9 | 6120 | 1870 |
| `shurushi` | 5 | 10578 | 2123 |
| `shurushi` | 9 | 11253 | 2150 |
| `womenyiqi` | 5 | 19052 | 8530 |
| `womenyiqi` | 9 | 11720 | 4649 |
| `yigezuci` | 5 | 13507 | 3018 |
| `yigezuci` | 9 | 10787 | 2352 |
| `jintiantianqi` | 5 | 44332 | 6630 |
| `jintiantianqi` | 9 | 32299 | 4722 |
| `zhongguorenmin` | 5 | 52944 | 23944 |
| `zhongguorenmin` | 9 | 35539 | 5742 |

峰值私有内存：**39.99 MB**，峰值工作集 75.32 MB

## 7. 确定性

两次独立进程候选输出哈希：`4e569da1a19ab3b4` / `4e569da1a19ab3b4` → **一致**

## 8. 单元测试

- ok: passed 444 / failed 0 / ignored 8

退出码：0

## 9. 未覆盖项（本基线不采集，需人工/另跑）

- CTest 平台层测试（需先构建 `platform/windows/build` 的 `test_*` 目标）
- 端到端 TSF→IPC→引擎→D2D 延迟（需活体宿主 + QPC 埋点）
- 真实用户词库回归（`exam --user-db`，仅人工入口）
- 多宿主兼容矩阵（Edge/VS Code/Office/微信/IMM32 游戏）

## 10. 复测方法

```
cargo build --release --examples --manifest-path engine/Cargo.toml
python tools/baseline/collect_baseline.py --out docs/baseline/v<新版本>
```

对比方式：`python tools/baseline/collect_baseline.py --diff <旧 baseline.json>`