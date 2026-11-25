# 泰深输入法基线报告 v0.1.90

- 采集时间：2026-11-25 10:43:08
- commit：`b1f0204`（工作区有未提交改动）
- 词库版本：`2026.11.24.1`

## 1. 构建产物与依赖（V4 打薄基准）

| 产物 | 体积 | SHA256(16) | 依赖命中 |
|---|---:|---|---|
| `platform/windows/out/taishen_ime.dll` | 0.71 MB | `466b4c2f14d05bb7` | 无 |
| `platform/windows/out/taishen_ime_x86.dll` | 0.61 MB | `e4af7bedc528ced4` | 无 |
| `platform/windows/out/taishen_server.exe` | 4.83 MB | `279d172c21f28aad` | d2d1.dll, DWrite.dll, ws2_32.dll |
| `engine/target/release/taishen_engine.dll` | 4.15 MB | `61176f2f4dc1266f` | ws2_32.dll |

## 2. V4 收口状态

- 普通宿主渲染默认：bool use_server_render = true;
- 普通宿主引擎默认：bool use_server_engine = true;
- IPC 协议版本：constexpr uint32_t kMinSupportedIpcVersion = 2;; constexpr uint32_t kRenderIpcVersion = 3;
- ime.dll 仍链接引擎静态库：否
- ime.dll 仍链接 d2d1/dwrite：0 处

## 3. 代码规模

| 口径 | 文件数 | 行数 |
|---|---:|---:|
| engine/src (*.rs) | 30 | 39733 |
| engine/** (*.rs，含 examples/tests) | 132 | 86550 |
| platform/windows/src (cpp+h) | 49 | 20627 |
| platform/windows/** (cpp+h，含 test) | 109 | 33734 |

Rust 单元测试函数：**457**

## 4. 词库

| 文件 | 体积 | 词条 |
|---|---:|---|
| `system_dict.db` | 32.56 MB | system_dict=371735, system_dict_trad=18669 |
| `domains.db` | 25.62 MB | domain_words=176620, domain_words_trad=19872 |
| `common.db` | 0.07 MB | common_words=2619 |
| `bigram.db` | 23.42 MB | context_prior=385992 |
| `system_dict.db.bin` | 74234584 B | （预编译索引） |
| `bigram.db.bin` | 4751352 B | （预编译索引） |

## 5. 考试成绩

| 分级 | 通过 | 占比 |
|---|---:|---:|
| L1 | 94/96 | 97.9% |
| L2 | 60/60 | 100.0% |
| L3 | 13/13 | 100.0% |
| L4 | 69/71 | 97.2% |
| L5 | 59/60 | 98.3% |
| **总计** | **284/289** | **98.3%** |

已知缺陷组：11/11（100.0%）

exam 退出码：1（0=全通过，1=有 FAIL）

## 6. 性能与内存

| 输入 | limit | 总耗时(us) | 单键峰值(us) |
|---|---:|---:|---:|
| `nihao` | 5 | 3337 | 1315 |
| `nihao` | 9 | 3200 | 1149 |
| `keyima` | 5 | 5651 | 2068 |
| `keyima` | 9 | 4745 | 1436 |
| `shurushi` | 5 | 7593 | 1960 |
| `shurushi` | 9 | 8608 | 2619 |
| `womenyiqi` | 5 | 13090 | 4851 |
| `womenyiqi` | 9 | 8530 | 1928 |
| `yigezuci` | 5 | 9667 | 1919 |
| `yigezuci` | 9 | 9006 | 2113 |
| `jintiantianqi` | 5 | 28436 | 3893 |
| `jintiantianqi` | 9 | 23959 | 4015 |
| `zhongguorenmin` | 5 | 28113 | 5445 |
| `zhongguorenmin` | 9 | 28728 | 4419 |

峰值私有内存：**11.12 MB**，峰值工作集 45.44 MB

## 7. 确定性

两次独立进程候选输出哈希：`1ff3a12df8570a58` / `1ff3a12df8570a58` → **一致**

## 8. 单元测试

- ok: passed 473 / failed 0 / ignored 8

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