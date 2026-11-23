# 泰深输入法基线报告 v0.1.90

- 采集时间：2026-11-23 21:12:57
- commit：`58759b7`（工作区有未提交改动）
- 词库版本：`2026.11.21.8`

## 1. 构建产物与依赖（V4 打薄基准）

| 产物 | 体积 | SHA256(16) | 依赖命中 |
|---|---:|---|---|
| `platform/windows/out/taishen_ime.dll` | 7.47 MB | `7565ee9316b4cf52` | d2d1.dll, DWrite.dll, ws2_32.dll, reqwest, hyper, h2- |
| `platform/windows/out/taishen_ime_x86.dll` | 6.19 MB | `5d16dd68b89a2323` | d2d1.dll, DWrite.dll, ws2_32.dll, reqwest, hyper, h2- |
| `platform/windows/out/taishen_server.exe` | 4.4 MB | `d879e9ca452b51da` | d2d1.dll, DWrite.dll |
| `engine/target/release/taishen_engine.dll` | 6.66 MB | `5a693630a27692fd` | ws2_32.dll, reqwest, hyper, h2- |

## 2. V4 收口状态

- 普通宿主渲染默认：bool use_server_render = false;
- 普通宿主引擎默认：bool use_server_engine = false;
- IPC 协议版本：constexpr uint32_t kMinSupportedIpcVersion = 2;; constexpr uint32_t kRenderIpcVersion = 3;
- ime.dll 仍链接引擎静态库：是
- ime.dll 仍链接 d2d1/dwrite：4 处

## 3. 代码规模

| 口径 | 文件数 | 行数 |
|---|---:|---:|
| engine/src (*.rs) | 29 | 36052 |
| engine/** (*.rs，含 examples/tests) | 117 | 71258 |
| platform/windows/src (cpp+h) | 34 | 18190 |
| platform/windows/** (cpp+h，含 test) | 88 | 28342 |

Rust 单元测试函数：**425**

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
| `nihao` | 5 | 4029 | 1860 |
| `nihao` | 9 | 4342 | 1775 |
| `keyima` | 5 | 6172 | 2790 |
| `keyima` | 9 | 4509 | 1039 |
| `shurushi` | 5 | 8877 | 2029 |
| `shurushi` | 9 | 10328 | 2110 |
| `womenyiqi` | 5 | 17146 | 7669 |
| `womenyiqi` | 9 | 11090 | 3706 |
| `yigezuci` | 5 | 12864 | 2613 |
| `yigezuci` | 9 | 10172 | 2066 |
| `jintiantianqi` | 5 | 37357 | 5103 |
| `jintiantianqi` | 9 | 29546 | 5166 |
| `zhongguorenmin` | 5 | 45916 | 21886 |
| `zhongguorenmin` | 9 | 33568 | 6270 |

峰值私有内存：**39.69 MB**，峰值工作集 74.86 MB

bigram 加载：79315 前词 / 385992 对，**40 ms**

## 7. 确定性

两次独立进程候选输出哈希：`4e569da1a19ab3b4` / `4e569da1a19ab3b4` → **一致**

## 8. 单元测试

- ok: passed 439 / failed 0 / ignored 8

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