# 完成报告 — 引擎内存治理（P1–P5）与长输入延迟优化（2026-11-22）

> 引擎版本：`taishen_engine v0.1.86` · 提交区间：`94faf69..51f2c29`（共 6 个本地提交，工作区干净，尚未 `push`）。
> 常驻私有内存：`947.6–950.2 MB` → **`39.8 MB`**（**-95.8%**）；峰值提交：`1,139.4–1,140.0 MB` → **`44.1 MB`**（**-96.1%**）；常驻工作集：`921.8–924.5 MB` → **`74.8 MB`**（**-91.9%**）。
> 长输入单键峰值（`zhongguorenmin limit=5`）：`100.08 ms`（P5 后 `101.01 ms`）→ **`20.79 ms`**（**-79.4%**）；其余长输入单键全部降至 **`1.2–8.1 ms`**。
> 考试通过率：`main_exam` **`278/292 (95.2%) → 279/292 (95.5%)`**（唯一变化为 `heshenm → #1 和什么` 由超时未出恢复通过）；三组长句 `36/90`、`31/90`、`0/90` **0 差异**。

---

## 一、目标与方案裁决

本轮围绕两大引擎瓶颈连续推进：**引擎内存治理（目标从 1 GB 级降至 100–300 MB，兼顾 32 位宿主 2 GB VA 上限与 Windows 覆盖安装安全）** 与 **长输入单键延迟优化（解决此前已排除 8 个嫌疑但仍达 ~97–101 ms 的 `zhongguorenmin` 峰值）**。

### 1. 内存治理方案评估与评审定案

独立评估报告已落盘 [EVAL-20261122.md](../modules/dict-memory/EVAL-20261122.md)，对比六类路线并修正原设计稿与原 `mapped.rs` 的关键隐患后，评审定案为：
- **弃用 8 分片哈希路由**：避免 `word_pool` 重复存储、保留全局字典序以支持 `range_from` / `range_prefix` 单段扫描、消除 `DefaultHasher` 跨版本及 32/64 位差异；改用**单表全局字典序 + 257 槽首字节目录 + 共享集中词池**。
- **双底层统一只读视图**：抽象 `Backing::{Mmap, Owned}`，保证 `TSDICT06` / `TSDOM003` / `TSLM0001` 文件映射与内存构建同路径零语义漂移；系统繁体 `trad_index` 与领域繁体 `domain_trad_index` 拆源只读查询，消除运行期写表。
- **P2 稀疏前缀微基准裁决（`sparse_prefix_bench`）**：若跳过前缀表改扫 `full_index` 还原，实测中高频前缀成本显著高于定点读取，评审确认不将方案 f 作为主线，保留完整前缀扁平容器并按 **P1 → P2 → P3 → P4 → P5** 全链路收口。
- **Windows mmap 安装锁保护**：所有 mmap 映射 `.bin` 文件统一以 `FILE_SHARE_READ | FILE_SHARE_DELETE`（禁止 `FILE_SHARE_WRITE`）打开；安装脚本升级前先将旧 `.bin` 改名为唯一 `.old.<ts>` 并登记重启清理；运行时写缓存走「同目录临时文件 → 原子 `rename`」。

---

## 二、内存治理全周期进展与结果（P1–P5）

### 1. 阶段演进实测对比（`TS_MEM_REPORT=1` + `GetProcessMemoryInfo` 20ms 活体轮询）

数据来源：[p1-baseline-before.txt](../../tmp/p1-baseline-before.txt)、[p1-after.txt](../../tmp/p1-after.txt)、[p2-after.txt](../../tmp/p2-after.txt)、[p3-after.txt](../../tmp/p3-after.txt)、[p4-after.txt](../../tmp/p4-after.txt)、[p5-after.txt](../../tmp/p5-after.txt) 及 Track 2 后复测。

| 阶段 | Commit | 覆盖对象与容器格式 | `.bin` 体积（系统 / 领域 / LM） | `SteadyPriv`（MB） | `PeakCommit`（MB） | `SteadyWS`（MB） | `PeakWS`（MB） |
|---|---|---|---:|---:|---:|---:|---:|
| **基线** | `94faf69` | 全量 bincode 堆反序列化（8 分片） | 171.1 MB / 86.3 MB / — | 947.6–950.2 | 1139.4–1140.0 | 921.8–924.5 | 1114.1–1114.8 |
| **P1** | `55955e1` | 主索引 `index` → `TSDICT04` mmap + 安装锁保护 | 115.5 MB / 86.3 MB / — | 636.2–637.9 | 683.0–685.6 | 699.7–701.3 | 741.2–743.3 |
| **P2** | `71ef8f6` | `full_index` + `suffix_index` → `TSDICT05` mmap | 97.6 MB / 86.3 MB / — | 501.3–502.7 | 547.7–548.9 | 540.8–542.1 | 576.6–577.7 |
| **P3** | `a0320aa` | 系统 6 表全部切入 `TSDICT06` + 拆分领域繁体 | **70.8 MB** / 86.3 MB / — | 356.9–358.4 | 498.9–499.2 | 363.0–363.1 | 489.1–489.5 |
| **P4** | `94d7702` | 领域 6 表全部切入 `TSDOM003` v3 mmap | 70.8 MB / **35.7 MB** / — | 125.3 | 166.5 | 146.0–146.1 | 166.7 |
| **P5** | `d51dbbe` | LM（52.3 万词）切入 `TSLM0001` v1 `lm_cache.bin` | 70.8 MB / 35.7 MB / **25.8 MB** | **39.8–40.0** | **44.2** | **74.8–75.0** | **74.8–75.0** |
| **Track 2 后复测** | `51f2c29` | 叠加 Trie 拼音切分与延迟优化后复测 | 同上 | **39.8** | **44.1** | **74.8** | **74.9** |

> 各只读容器实测精确体积：`resources/system_dict.db.bin` = `74,234,584 B`（70.80 MB）、`resources/domains/domains.db.bin` = `37,474,912 B`（35.74 MB）、`resources/lm/lm_cache.bin` = `27,004,352 B`（25.75 MB），三容器合计 **132.3 MB** 且跨进程共享物理页。`TS_MEM_REPORT=1` 下 `system_dict`、`domains`、`lm` 堆索引估算全部为 **0 MB**。

### 2. 三套零拷贝容器结构（[mapped.rs](../../engine/src/dictionary/mapped.rs) & [lm.rs](../../engine/src/lm.rs)）

| 容器 | 魔数 / 版本 | 纳入表 | 核心布局与热路径零分配抽象 |
|---|---|---|---|
| **系统词库** | `TSDICT06` v6 | `index` / `full` / `suffix` / `short` / `short_full` / `trad` | 单份共享 `SEC_WORD_POOL` + 定长 8B 条目（`word_id(24b) \| pinyin_len(8b) \| freq(32b)`）；后缀 12B 条目共享前缀池；提供 `find_word_freq`、`iter_with_pinyin_len`、`iter_word_byte_len_range`、`LazyRangeFromIter` |
| **领域词库** | `TSDOM003` v3 | `dom_index` / `dom_full` / `dom_short` / `dom_exact` / `dom_trad` / `word_domain` | 复用相同词池与 `KeyTableLayout` / `KeyEntryLayout`；新增 `MappedU32Map`（`word_domain`）与 `SEC_DOM_META`；`iter_without_pinyin_len` 预过滤 |
| **语言模型** | `TSLM0001` v1 | `SEC_LM_META`（2132B） / `SEC_LM_SEG_A` / `SEC_LM_SEG_B` / `SEC_LM_W2I` | 新增 `MappedU32ListMap`（`stride=4`）+ `U32ListView::to_small_array::<4>` 栈上 `SmallIds`（最多 4 ID 零堆分配）；原地读取 `seg_a`/`seg_b` 量化参数与二分查边；缓存缺失/过期/损坏时从 `lm_data.bin` + `lm_words.json` 原子重建并立即转 mmap |

---

## 三、长输入延迟采样定因与无损优化（Track 2，`51f2c29`）

### 1. CPU 采样定位（1000 Hz `dbghelp.dll` PDB 堆栈采样，2,477 样本）

使用带 PDB 符号的 `lat_probe.exe` 进行 3 轮 1000 Hz 挂起采样 + `lat_stage` 分项探针验证，定位此前未能解释的 ~97–101 ms 瓶颈根因：
- **87.4% 引擎 CPU 汇聚在同一条公共漏斗**：
  - `Engine::query_all` → `phrase_group_guess` → `group_rec`：**59.5%**（在 `set_group_limits(1500, Some(30))` 下频繁撞满 30 ms 硬时限）；
  - `sentence::decode_meta` → `char_graph` → `abbrev_span_candidates`：**15.3%**（`lat_stage` / `TS_SENTENCE_PROFILE=1` 显示 `zhongguor` 首键冷 `span_cache` 达 65.7 ms）；
  - `query_combo`：**12.6%**。
- **三大叶节点根因**：
  1. `HashSet` SipHash（约占总样本 25–27%）：`is_valid_syllable` / `is_valid_pinyin_prefix` 在全小写 411 音节上反复跑 SipHash；
  2. `O(n²)` 音节切分回溯（约 18.7%）：`split_into_syllables` 每步循环调 `split_first_syllable` 重算整段 DP，且 `combo_rec` 对单音节再调 `to_initial_full` 重算切分；
  3. 频繁堆分配（约 22% `RtlAllocateHeap` + `memset` + `RtlFreeHeap`）：每次 `combo_match` 都分配 `Vec<String>` 与每个音节 `String`，同时 `abbrev_span_candidates` / `query_combo` / 3 轮 `group_rec` 迭代加深未按首字母缩窄或复查同一段候选。

### 2. 四项数学等价的无损优化（[pinyin/mod.rs](../../engine/src/pinyin/mod.rs) & [dictionary/mod.rs](../../engine/src/dictionary/mod.rs)）

1. **26 路平坦 ASCII `SyllableTrie`**：以 ~28 KB（约 500 节点）L1 缓存静态 Trie 替换 `syllable_set()` / `syllable_prefix_set()` 的 `HashSet`；单趟 Trie 前推重写 `can_segment_completely`、`split_first_syllable`、`is_complete_pinyin`（且防非 ASCII 切片越界）；新增 `O(n)` 零堆分配 `split_syllables_into`（写入栈 `[&str; 16]` + `u64` 可达位图）与 `split_syllables_slices`。
2. **无损字节子序列预过滤 + 零分配 `combo_match`**：`combo_prefilter` 新增首字节相等 + 长度下界 + 有序字节子序列校验（~3 ns 拒绝绝大多数未命中键）；`combo_match` 走 `split_syllables_into` + `combo_rec_slices`，移除已被前缀枚举包含的 `to_initial_full` 分配分支。
3. **惰性扫描与首字母窄区间**：`abbrev_span_candidates` 切为 `range_from_lazy`（`combo_match` 命中后才构造 `EntriesView`），`abbrev_span_candidates` 与 `query_combo` 的 `common_full_index` / `user_full_index` 均通过 `first_char_range` + `btree_str_range` 只扫对应首字母区间。
4. **`group_rec` 单次调用 `GroupCaches`**：提取 `compute_stage_a_picked` / `compute_stage_b_variant_words`，以 `Rc` 共享阶段 A（`(seg, at_end, at_start)`）与阶段 B（`seg` 变体词）检索结果，消除 3 轮迭代加深（`max_err = 0/1/2`）与递归子树间的重复扫库。

### 3. 延迟对比实测（3 轮干净 `lat_probe` 中位数：[p5-lat-retest.txt](../../tmp/p5-lat-retest.txt) → [lat-opt-retest.txt](../../tmp/lat-opt-retest.txt)）

| 输入用例 | 窗口宽 | 优化前 Peak（ms） | 优化后 Peak（ms） | 降幅 | 优化前后 Total（ms） |
|---|---:|---:|---:|---:|---:|
| `zhongguorenmin` | 5 | 101.01 | **20.79** | **-79.4%**（4.9×） | 198.40 → **39.50** |
| `zhongguorenmin` | 9 | 40.56 | **5.10** | **-87.4%**（8.0×） | 190.35 → **25.84** |
| `jintiantianqi` | 5 | 50.18 | **5.31** | **-89.4%**（9.5×） | 257.36 → **34.59** |
| `jintiantianqi` | 9 | 39.12 | **4.83** | **-87.7%**（8.1×） | 228.02 → **27.56** |
| `womenyiqi` | 5 | 54.17 | **8.12** | **-85.0%**（6.7×） | 130.83 → **17.01** |
| `womenyiqi` | 9 | 38.03 | **3.52** | **-90.7%**（10.8×） | 101.42 → **9.45** |
| `yigezuci` | 5 | 43.90 | **2.31** | **-94.7%**（19.0×） | 102.68 → **10.32** |
| `shurushi` | 5 | 26.33 | **1.35** | **-94.9%**（19.5×） | 76.83 → **6.74** |
| `keyima` | 5 | 14.24 | **2.91** | **-79.6%**（4.9×） | 20.59 → **5.56** |
| `nihao` | 5 | 2.18 | **1.18** | **-45.9%**（1.8×） | 4.02 → **2.62** |

- **`lat_stage` 分项对比**（[lat-opt-stage.txt](../../tmp/lat-opt-stage.txt)）：`zhongg` `37.7ms → 15.6ms`、`zhongguor` `27.1ms → 5.0ms`、`womeny` `15.3ms → 3.1ms`、`jintia` `13.2ms → 4.3ms`；其余长输入前缀每键均在 `1.1–4.3 ms`。

---

## 四、质量回归与全量校验清单

### 1. 四组 `exam` 回归结果（[lat-opt-exams.txt](../../tmp/lat-opt-exams.txt) vs [p1-after-exams.txt](../../tmp/p1-after-exams.txt)）

| 套件 | 基线（P1–P5） | 优化后（`51f2c29`） | 差异说明 |
|---|---:|---:|---|
| `main_exam` | 278/292 (95.2%) | **279/292 (95.5%)** | 唯一变化：`[L1-B 全拼双字词] heshenm 和什么 → 范围 2` 由 FAIL 转为 PASS（原 30 ms 时限截断了 Pass 2 `shenm→shenme` 变体，提速后在 1500 节点预算内命中 `#1 和什么(Combo)`）；已知缺陷维持 `9/15 (60.0%)` |
| `tmp/lb-全拼.tsv` | 36/90 (40.0%) | **36/90 (40.0%)** | **0 差异** |
| `tmp/lb-虚词简.tsv` | 31/90 (34.4%) | **31/90 (34.4%)** | **0 差异** |
| `tmp/lb-全简.tsv` | 0/90 (0.0%) | **0/90 (0.0%)** | **0 差异**（不含既有随机哈希 tie-break 导致的 `实际:` 展示行） |

### 2. 自动化校验矩阵（提交前均已运行并通过）

| 检查命令 | 最新结果（`51f2c29` 前后） | 说明 |
|---|---|---|
| `cargo run --release --example mapped_check` | **全部 0 不一致** | 系统 6 表（107 万主索引 + 5 表）+ 领域 6 表（38.3 万主索引 + 5 表）+ LM 523,334 词与 13,801 对 bigram（P5 验证） |
| `cargo test --manifest-path engine/Cargo.toml --release -- --test-threads=1` | **lib 416 passed, 0 failed, 5 ignored** + **全部 9 个集成测试通过** | 新增 Trie 等价、`combo_match` 参考比对、`TSLM0001` 往返/失效与原子替换测试；`repro_issues` 由 `15.19s → 0.20s`、`first_char_verify` 由 `6.61s → 1.21s` |
| `cargo check --manifest-path engine/Cargo.toml --release --target i686-pc-windows-msvc` | **通过** | 保证 32 位宿主静态库正常编译（注：`i686` 上各容器映射约 132 MB，远低于 2 GB 地址红线） |
| `pwsh -NoProfile -File tools/check-structure.ps1` | **`[PASS]`** | 模块边界、资源清单与常用词表一致性全部通过 |
| 新增代码/文档/提交信息审计 | **通过** | 已检查 `git diff` 新增行，无任何外部输入法产品名或外部语料/词库来源描述 |

### 3. 本地提交记录（`94faf69` 之后新增 6 个 commit）

| # | Commit | 主题 | 主要涉及文件 |
|---|---|---|---|
| 1 | `55955e1` | P1：主索引切入 `TSDICT04` mmap + 安装锁保护 | [EVAL-20261122.md](../modules/dict-memory/EVAL-20261122.md)、`mapped.rs`、`dictionary/mod.rs`、`mapped_check.rs`、`install/*`、`package.ps1` |
| 2 | `71ef8f6` | P2：`full_index` 与 `suffix_index` 切入 `TSDICT05` mmap + 稀疏前缀微基准 | `mapped.rs`、`dictionary/mod.rs`、`mapped_check.rs`、`sparse_prefix_bench.rs`、`package.ps1` |
| 3 | `a0320aa` | P3：`short`/`short_full`/`trad` 切入 `TSDICT06` 并拆分领域繁体 | `mapped.rs`、`dictionary/mod.rs`、`mapped_check.rs`、`package.ps1` |
| 4 | `94d7702` | P4：领域六表全部切入 `TSDOM003` v3 mmap 容器 | `mapped.rs`、`dictionary/mod.rs`、`mapped_check.rs` |
| 5 | `d51dbbe` | P5：语言模型切入 `TSLM0001` v1 `lm_cache.bin` mmap 容器与锁保护 | `mapped.rs`、`dictionary/mod.rs`、`lm.rs`、`mapped_check.rs`、`install/*` |
| 6 | `51f2c29` | Track 2：拼音 Trie 切分、零分配 `combo_match` 与 `GroupCaches` | [pinyin/mod.rs](../../engine/src/pinyin/mod.rs)、[dictionary/mod.rs](../../engine/src/dictionary/mod.rs) |

> 本地保留基准产出（已 `.gitignore`，未入库）：`resources/system_dict.db.bin`、`resources/domains/domains.db.bin`、`resources/lm/lm_cache.bin`，以及 `tmp/p1-*` ~ `tmp/p5-*`、`tmp/lat-opt-*`、`tmp/lb-*.tsv`。

---

## 五、关键工程经验与注意事项

1. **Windows 进程内 mmap 的热升级契约**：已 mmap 文件必须用 `FILE_SHARE_READ | FILE_SHARE_DELETE` 打开，否则外部原地覆盖会触发页错误或安装失败；运行时更新与 NSIS/PowerShell 安装器覆盖前都必须先将旧文件改名为唯一 `.old.<ts>`。
2. **`GetProcessMemoryInfo` 稳态采样陷阱**：短生命周期 CLI 在退出末尾阶段会释放页表与工作集，切忌进程结束后读取或直接取末样本；应在进程存活期以 10–20 ms 轮询并过滤活跃工作集后取倒数第二帧，才能测准真实稳态 `PrivateUsage` 与 `WorkingSetSize`。
3. **`phrase_group_guess` 时间上限与节点预算交互**：此前 `heshenm` 在 `set_group_limits(1500, Some(30))` 内受 30 ms 超时提前截断；大幅提速后 DFS 会在 2–3 ms 内跑完整个 1500 节点预算，使原本因超时丢弃的合规路径得以返回。后续任何长输入候选微调需同时兼顾节点预算与时间上限。
4. **`tmp/lb-全简.tsv` 的既有随机性**：`abbrev_span_candidates` 内部 `best: HashMap<String, (u32, u8)>` 在同词频且同先验分并列时受 Rust 随机 Hash 种子影响，因此全简测试的 `实际:` 展示行在连续两次同二进制运行间本身即有 tie-break 差异，对比时以总通过数及非 `实际:` 行判定为准。

---

## 六、遗留与下一步交接

1. **构建打包与本机部署（高优）**：本轮所有验证均在 `engine/` 库、单测与示例程序侧完成，**尚未重新运行 [package.ps1](../../package.ps1) 编译 64/32 位 DLL/`taishen_server.exe` 并部署到 `%LOCALAPPDATA%\TaishenIME`**；另请注意 [package.ps1](../../package.ps1) 与 NSIS 当前主要打包 `system_dict.db(.bin)` / `domains.db` / `common.db` / `bigram.db`，如需随安装包携带 `resources/lm/` 与预生成 `lm_cache.bin`，需在收集段补充。
2. **远程同步**：P1–P5 与 Track 2 共 6 个提交（以及此前积累的本地提交）均在本地 `master`，尚未 `git push`。
3. **可选延伸项**：
   - `bigram.db`（当前约 8 MB，加载 40 ms，估算占用仍在剩余 39.8 MB 私有内存中占比较大）暂保持现有 `.db.bin` 结构，如需进一步压缩底盘也可复用 `MappedU32Map`/平坦结构；
   - `zhongguorenmin limit=5` 峰值当前已降至 `20.79 ms`（其中首字 `zhongg` 阶段占 `15.6 ms`），若要压至 `< 10 ms`，可针对当前已大幅收敛后的 `zhongg` 热路径重跑一次 `dbghelp` 采样。
