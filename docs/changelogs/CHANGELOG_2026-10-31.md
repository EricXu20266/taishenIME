# CHANGELOG 2026-10-31

## 引擎审计优化（A / B / C / D 四批）

> 依据 `docs/modules/ENGINE-AUDIT-20261031.md`。每批独立提交、独立验收。
> 引擎 lib 测试从 341 项增至 358 项，全部通过。

### A1 统一去重集合（P0-3）

- `query_all` 与 `query_all_shuangpin` 各维护一份与候选同步的 `cand_seen`（HashSet）
- 13 处线性 `contains` 改为集合查询，长串输入不再叠加成 O(n²)
- 截断到排序预算后重建 seen，保持与原 `contains` 语义等价

### A2 原地稳定分区（P0-2）

- `apply_pin_boost` / `apply_sort_mode` / `apply_word_len_match` 改稳定排序，不再建新 Vec
- `apply_demote` 改原地分区 + `rotate_left`，**顺带修正原实现丢词与尾部重复**：
  非降权词多于 11 条时，原代码会覆盖掉中间若干词并在尾部留下重复项
- `apply_context_boost` 改下标排序 + `permute_range`（此处排序键仍为 `lift - i`，C1 单独改为权重降序）
- 五段后处理不再克隆候选文本

### A3 排序参数外置（§4.9）

- 新增 `engine/src/tuning.rs`：6 个参数，越界钳制 + 缺省回退 + 不热更新
- 读取优先级：`TAISHEN_TUNING` > 可执行文件同目录 > `%APPDATA%/taishen-ime`
- 新增 5 个单测（默认值 / 覆盖 / 越界 / 缺失 / 非法类型）
- 文档：`docs/modules/ENGINE-TUNING.md` + `tuning.example.json`

### B1 候选来源 side-table（P0-1）

- 新增 `Source` 枚举与 `Engine.candidate_src`，与 `all_candidates` 严格等长
- 新增 `publish` / `publish_with_src`，成为候选产出的唯一发布口
- 删除 `english_candidate_pos` / `phrase_candidate_pos` / `datetime_candidate_pos`
- Emoji 插入不再手动修正位置字段；`is_english_candidate` 等改查来源表
- 五个 `apply_*` 增加来源表参数，`permute_range` 同步重排文本与来源

### B2 模式分发表（P2-1）

- 新增 `Mode` 枚举与 `detect_mode` / `apply_mode`，8 个平铺 if 分支压成一个 match
- 拆字反查无候选返回 false 回落常规路径，语义与原实现一致
- 新增一种输入模式 = 一个变体 + 一行判定 + 一个分支

### B3 汉字判定区间化（P2-3）

- 新增 `is_hanzi`，只认 `4E00-9FFF` / `3400-4DBF` / `F900-FAFF`
- 全角标点、拉丁扩展字母（é/ü）、emoji 不再计入「汉字数」

### C1 上下文权重纯加分（P1-3）

- 排序键从 `(lift - i)` 改为按权重降序、同权重保持原位置
- 原实现里位置项会压过权重：第 30 位命中权重 20 的搭配词排不过首位权重 5 的词
- 更新 3 个基于旧语义的测试；新增长列表权重生效、等权重保序两个用例
- 12 组「前文 + 输入」基线探针改前 / 改后输出逐字节一致

### C2 输入质量分类（P1-1，结论：条件未启用）

- 新增 `InputQuality` 与 `classify_input`（完整拼音 / 混合 / 纯声母 / 前缀 / 需纠错）
- 尝试用质量替代 spelling / deletion 的「候选数量」条件，真实词库实测回归：
  - `xinashi`（Exact，词库命中冷僻词「希腊式」）丢失「显示」
  - `logn` / `nihap` / `rung`（Mixed）丢失「龙」等正确纠错
- 结论：质量判据保留为诊断基础设施，触发条件维持原实现，依据写进代码注释
- 32 例候选探针输出与改动前逐字节一致

### D1 词库路径收敛（P2-2）

- 合并 `DICT_PATH` / `USER_DICT_PATH` / `DOMAIN_DICT_PATH` / `PENDING_DOMAIN` 为一个 `DictPaths`
- 保留 `DICT_READY` / `DICT_GENERATION` 两个 Atomic（无锁高频读，不塞进 Mutex）
- 显式约定锁顺序 **DICT → PATHS**，与领域加载线程一致
- `pending_domain` 先 take 再判断，避免在 if let 块内持有路径锁
- 新增并发 init 两次的测试

### 未落地

- **P1-2 统一打分**：按报告 §4.5.2 的启动条件（三条全不满足）不启动，
  方案留档在审计报告内，条件满足时再立项

### 验证

- `cargo build` 通过；`cargo test -- --test-threads=1` 全绿（358 lib + 集成，1 ignored）
- 32 例候选探针、12 组上下文探针逐条对比
- `rustfmt --check`：改动文件无新增格式差异（剩余差异为既有代码）
