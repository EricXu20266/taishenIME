# 泰深输入法基线报告 v0.1.90

- 采集时间：2026-11-24 20:46:32
- commit：`26cf0b0`（工作区无未提交改动）
- 词库版本：`2026.11.24.1`

## 1. 构建产物与依赖（V4 打薄基准）

| 产物 | 体积 | SHA256(16) | 依赖命中 |
|---|---:|---|---|
| `platform/windows/out/taishen_ime.dll` | 0.67 MB | `fb4892a4bcafaa1e` | 无 |
| `platform/windows/out/taishen_ime_x86.dll` | 0.58 MB | `2ccea03bfa629c3d` | 无 |
| `platform/windows/out/taishen_server.exe` | 4.76 MB | `997543e6b7db21d4` | d2d1.dll, DWrite.dll, ws2_32.dll |
| `engine/target/release/taishen_engine.dll` | 4.12 MB | `59a9672f76b3e61f` | ws2_32.dll |

## 2. V4 收口状态

- 普通宿主渲染默认：bool use_server_render = true;
- 普通宿主引擎默认：bool use_server_engine = true;
- IPC 协议版本：constexpr uint32_t kMinSupportedIpcVersion = 2;; constexpr uint32_t kRenderIpcVersion = 3;
- ime.dll 仍链接引擎静态库：否
- ime.dll 仍链接 d2d1/dwrite：0 处

## 3. 代码规模

| 口径 | 文件数 | 行数 |
|---|---:|---:|
| engine/src (*.rs) | 30 | 38214 |
| engine/** (*.rs，含 examples/tests) | 132 | 85031 |
| platform/windows/src (cpp+h) | 44 | 18159 |
| platform/windows/** (cpp+h，含 test) | 100 | 29380 |

Rust 单元测试函数：**446**

## 4. 词库

| 文件 | 体积 | 词条 |
|---|---:|---|
| `system_dict.db` | 32.56 MB | system_dict=371735, system_dict_trad=18669 |
| `domains.db` | 25.62 MB | domain_words=176620, domain_words_trad=19872 |
| `common.db` | 0.07 MB | common_words=2619 |
| `bigram.db` | 23.42 MB | context_prior=385992 |
| `system_dict.db.bin` | 74234584 B | （预编译索引） |
| `bigram.db.bin` | 8432104 B | （预编译索引） |

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
| `nihao` | 5 | 2865 | 1186 |
| `nihao` | 9 | 3474 | 1126 |
| `keyima` | 5 | 8189 | 2705 |
| `keyima` | 9 | 4500 | 1171 |
| `shurushi` | 5 | 9693 | 2282 |
| `shurushi` | 9 | 10378 | 1982 |
| `womenyiqi` | 5 | 18798 | 9702 |
| `womenyiqi` | 9 | 12833 | 3866 |
| `yigezuci` | 5 | 11828 | 2090 |
| `yigezuci` | 9 | 11220 | 2073 |
| `jintiantianqi` | 5 | 41346 | 7489 |
| `jintiantianqi` | 9 | 35312 | 5940 |
| `zhongguorenmin` | 5 | 54106 | 21216 |
| `zhongguorenmin` | 9 | 39012 | 7363 |

峰值私有内存：**40.25 MB**，峰值工作集 73.44 MB

## 7. 确定性

两次独立进程候选输出哈希：`1ff3a12df8570a58` / `1ff3a12df8570a58` → **一致**

## 8. 单元测试

- ok: passed 462 / failed 0 / ignored 8

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