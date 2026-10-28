# 改进批次验收表（2026-10-28）

> 执行依据：本地改进方案 v1（仅存于本机，不入版本库）
> 执行范围：排期中的 P0 全部 + P1 两项，另两项给出不实施理由

## 一、已完成并验证

| 批次 | 内容 | 主要改动 | 验证证据 | commit |
|------|------|---------|---------|--------|
| A | 智能联想改加权前移 + 默认开启 | `engine/src/lib.rs`、`engine/src/dictionary/mod.rs`、`config_reader.*`、`settings_window.cpp`、`config.ini.example` | `cargo build` 通过；`cargo test --lib -- context` 11 passed；新增权重单调/关闭保序/默认开启 3 个测试 | `58788f0` |
| B | 用户数据批量落盘 | 新增 `engine/src/dictionary/write_back.rs`；`learn_user_word`/`save_context_pair` 改投队列；`engine_flush_user_writes`；TSF 定时器与 IMM32 取消选中时落盘 | `cargo test --lib -- write_back` 3 passed；C++ 全量构建 EXIT=0 | `58788f0` |
| C | `.bin` v3：meta 分段并行 | `to_bin` 拆 5 段、`read_meta_segments`、`shell()`；保留 v2 读取分支 | `cargo test --lib -- bin` 3 passed（含旧 v2 文件读取）；`test_bin_v3_meta_roundtrip` 1 passed | `58788f0` |
| D | 离线上下文先验表 | `load_context_prior`、`Engine::load_prior_contexts`、`engine_set_context_prior_path`、TSF/IMM32 激活时加载 | `test_context_prior_boosts_without_history`、`test_learned_and_prior_merge_takes_max` 通过；生成脚本冒烟：348,536 词表 → 6,230 对（0.4 MB），档位 1-6 分布正常 | `898c926` |
| F | 候选来源反查 | `Dictionary::user_words` + `word_source` + `engine_get_candidate_source` | `test_word_source_marks_user_word` 通过；C++ 构建 EXIT=0 | `7746527` |

全量回归：`cargo test -- --test-threads=1` → **330 passed / 0 failed**（lib）+ 1 passed（集成）。

## 二、结论为不实施（附理由）

### E 拼音多路径切分

方案设想：`split_first_syllable` 单路径 → 枚举全部合法切分，仅用于拆词路径。

实现后验证不成立，**已回滚**，两个理由：

1. `combo_syl_rec` 本来就是多路径——它按 1..6 长度逐段递归，只要「合法音节前缀」就分支，不是单路径。
2. `query_combo` 的首段 range 已覆盖全部可命中词：`combo_match` 要求输入以词首音节的前缀开头；词首音节必然是最长合法首段的**前缀**（否则该词首音节本身构成更长的合法切分，与「最长」矛盾）。于是词拼音以最长首段开头，落在 `range(最长首段..下一字符)` 区间内。加回退分支后无法构造出命中差异，属无效代码。

### G IMM32 候选数据接口 + 全屏降级

缺口真实：`imm32_ime.cpp` 从不填 `INPUTCONTEXT::hCandInfo`，也不处理 `IMC_*`，宿主拿不到候选；全平台无全屏检测。

但改动落在 `ImeToAsciiEx` 的输入上下文共享内存块：字节量算错、`ImmLockIMCC`/`ImmUnlockIMCC` 不配对，会让宿主进程崩溃（联动所有应用）。五条验收判据全部需要真实安装 + 宿主应用实测，无人值守下风险高于收益。

处置：落设计稿 `docs/modules/imm32-candidate-data/SPEC.md`（含结构布局、通知消息、ui_mode 取值、全屏检测 API、四条验收判据、回滚路径），待在场专项实施。

## 三、待验收项（需你回来点一遍）

| 项 | 怎么做 | 预期 |
|----|--------|------|
| 智能联想体感 | 部署新 DLL，打「北京」后输入 `da` | 「大学」在首屏；设置页关闭后候选顺序与旧版一致 |
| 启动耗时 | 看 `engine.log` 的 `from_bin 分段` 行 | meta 段耗时明显下降（v3 拆 5 段并行） |
| 上屏不卡 | 连续选词 100 次，观察 `user_dict.db` | 写入事务数从 100 降到 ≤8；进程强杀最多丢 20 条 |
| 新装联想可用 | 删 `user_dict.db` 后输入常见搭配 | 内置表命中，无需先学 |
| 候选来源 | 调 `engine_get_candidate_source(i)` | 1=用户词 2=专业词库 3=系统词 |
| 先验表（可选） | 用 `taishen-dict/tools/build_context_prior.py` 跑真实语料，产物放 DLL 同目录 | 语料规模足够时新装即有联想 |

## 四、V2 格式迁移说明

`.bin` 升到 v3 后，现有 v2 文件仍可读（保留兼容分支）。下次部署重建索引时自动写 v3，无需手动删文件。

## 五、未做项（方案内的后续）

| 项 | 原因 |
|----|------|
| 字符串池化 + 索引去重（P0-2 步骤 2/3） | 4-5 天，触及全部查询返回路径，回归面大 |
| P2 整句输入 | 依赖 P0-1 体感跑稳 + 真实 bigram 表 |
| 全词库 mmap / 云候选 / 8KB 位图 | 方案明确不做，理由已记录 |

## 六、产物清单

| 文件 | 说明 |
|------|------|
| `engine/src/dictionary/write_back.rs` | 批量写缓冲 |
| `engine/src/dictionary/mod.rs` | v3 序列化、先验表加载、来源反查 |
| `engine/src/lib.rs` | 加权联想 + 先验合并 |
| `engine/src/ffi.rs` | 新增 4 个导出：flush / prior / source /（context 相关已有） |
| `docs/modules/context-assoc/SPEC.md` | 更新为加权模型 + 先验表 |
| `docs/modules/imm32-candidate-data/SPEC.md` | 新增设计稿 |
| `taishen-dict/tools/build_context_prior.py` | 先验表生成脚本（另一仓库，未提交） |
