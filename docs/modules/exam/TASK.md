# 考试工具升级任务书

> 给新会话的完整作业指导。读完直接开工，不需要回来确认。
> 工作目录：`E:/AllinDeepSeek/taishenIME`

---

## 0. 背景与已完成的前置

**已完成的（不要重做）**：
- 引擎新增 `completion_enabled` 开关（`lib.rs` / `ffi.rs` / 配置 / 设置页已贯通）
- 探针 `probe_candidates` 支持 `--no-completion` 参数

**你要做的**：升级考试工具，让它能检出「垃圾词组」「纠错抢占」「补全侵占」这类问题。

**设计文档**（必读，含 58 场景清单）：`docs/modules/exam/DESIGN.md`

---

## 1. 现状

`engine/examples/exam.rs`（292 行）当前能力：
- 读 TSV：`拼音<TAB>期望词<TAB>最大排名<TAB>备注`
- `[分组名]` 标记分组
- 跑引擎取前 N 候选，断言期望词排名
- 输出分组通过率 + 失败明细

**三个缺口**：
1. 只能正向断言「某词在第几位」，无法断言「不该出现的词」
2. 引擎不暴露候选来源，无法验证「纠错不抢占」「不该组词却组了」
3. 用例扁平无分级，答不了「简拼质量如何」

现有用例 `engine/tests/exam-cases.tsv`（86 行，9 个分组）——**必须保持全部通过，不能改坏**。

---

## 2. 任务一：引擎暴露候选来源

### 2.1 现状

`engine/src/lib.rs` 内部有 `Source` 枚举（约 30-58 行）：

```rust
Dictionary / User / Emoji / Correction / Combo / Fuzzy / Mode / Voice / Sentence / Completion
```

但对外只有：
```rust
pub fn candidate(&self, index: usize) -> Option<&str>   // 当前页内索引
pub fn candidate_global(&self, index: usize) -> Option<&str>  // 跨页全局索引
```

### 2.2 要加的 API

在 `lib.rs` 的 `candidate()` 附近加：

```rust
/// 候选来源名（考试/诊断用）：Dictionary/User/Emoji/Correction/Combo/Fuzzy/Mode/Voice/Sentence/Completion
/// 索引语义与 candidate() 一致（当前页内）
pub fn candidate_source_name(&self, index: usize) -> Option<&'static str>
```

实现要点：
- 对齐 `candidate(index)` 的索引语义（不是 global）
- `Source` 每个变体映射为一个稳定字符串，**字符串内容一旦定下不要改**（用例文件要按它写断言）
- 顺带在 `ffi.rs` 加一个可选导出 `engine_get_candidate_source_name(int index, char* buf, int len)`，供平台层调试

### 2.3 参考

`ffi.rs` 已有 `engine_get_candidate_source`（返回 1=用户词 2=专业词库 3=系统词），粒度太粗，**不要改它**（平台层在用），新加的是另一个函数。

---

## 3. 任务二：exam.rs 扩展断言

### 3.1 断言语法

第 2 列用前缀区分类型，一行一断言：

| 第 2 列写法 | 含义 | 实现要点 |
|---|---|---|
| `期望词` | 正向：该词在 max_rank 内 | 现有行为 |
| `!词1,词2` | 反向：这些词不得出现在前 max_rank | 逗号分隔 |
| `>词A>词B` | 顺序：A 必须排在 B 之前 | 任一缺失即 FAIL |
| `@src=Dictionary` | 前 max_rank 候选**全部**来自该来源 | |
| `@has_src=Correction` | 前 max_rank 内**至少一个**来自该来源 | |
| `@no_src=Correction,Combo` | 前 max_rank 内**不得有**这些来源 | |
| `@multi=0` | 前 max_rank 内多字词数量 = 0 | 字数按汉字计 |
| `@multi>=1` | 前 max_rank 内多字词数量 ≥ 1 | |
| `@count<=N` / `@count>=N` | 候选总数约束 | 用 candidate_count() |

解析规则：第 2 列以 `!` / `>` / `@` 开头按新语法，否则按现有正向断言。**保证旧用例零改动可跑**。

### 3.2 输出改造

失败明细要带来源标记，例如：

```
FAIL  zei    @no_src=Fuzzy 1-3
      实际: #1 贼(Dictionary)  #2 这(Fuzzy)  #3 贼骨头(Dictionary)
```

分级统计按 section 名首段聚合（`L1-xxx` → L1）：

```
L1 全拼      通过 18/20  90.0%
L2 简拼      通过 7/9    77.8%
L3 混拼      通过 5/6    83.3%
L4 纠错      通过 6/13   46.2%
L5 边界      通过 14/15  93.3%
总计 50/63 (79.4%)
```

### 3.3 已知缺陷标记

L4 里有些断言现在必然 FAIL（如「单字不应有补全词」，关闭开关才能通过）。加一个标记：

```
[L4-C 已知缺陷]  → section 名含「已知缺陷」的，FAIL 不计入总失败、不影响退出码，单独统计
```

---

## 4. 任务三：重写用例文件

按 `docs/modules/exam/DESIGN.md` 的 58 个场景写用例，落到 `engine/tests/exam-cases.tsv`。

**要求**：
- 保留现有 86 条（可重新归类到 L1/L2 分组，但断言不能放宽）
- 每个场景至少 1 条断言，关键场景 2-3 条
- 目标总量 300-400 条
- 分组命名：`[L1-A 全拼单字]` / `[L4-C 无意义串不强行组词]` / `[L4-D 已知缺陷]` 这种格式

**用例必须自己跑过一遍**，把实际通过率填进报告。

---

## 5. 验收标准

| 项 | 标准 |
|---|---|
| 编译 | `cargo build --release --manifest-path engine/Cargo.toml --example exam` 零 error |
| 旧用例 | 原有 86 条断言全部仍通过（可换分组，不可删断言） |
| 新功能 | 6 类新断言各至少 3 条用例覆盖 |
| 运行 | `cargo run --release --manifest-path engine/Cargo.toml --example exam` 跑通，输出分级统计 |
| 文档 | 更新 `docs/modules/exam/SPEC.md`（新建），写明断言语法与分级口径 |

---

## 6. 纪律

1. **不许改候选生成逻辑**。这次只加 API 和测试工具，不动 Completion/Correction/Combo 的产生规则。
2. **不许为了让用例通过而放宽断言**。FAIL 就如实记录，L4 的 FAIL 是本次要交付的成果之一。
3. **不许删现有用例**。
4. **不许引入新依赖**（保持只有 std + 现有依赖）。
5. 改完跑 `cargo test --manifest-path engine/Cargo.toml -- --test-threads=1`，确认没有回归。

---

## 7. 交付报告

写 `docs/modules/exam/REPORT.md`，包含：
1. 通过率矩阵（L1-L5 各分类）
2. L4 的 FAIL 明细（这是最有价值的部分：「强行组词」「纠错抢占」的实际数量）
3. 断言语法最终版
4. 后续建议（哪些 FAIL 是引擎缺陷、哪些是用例写得不准）

---

## 8. 参考数据（写用例时用得上）

当前实测候选（用于校准期望值）：

```
ji     及 即 既 极 急 几 系 奇 给 级 [继续][集美]        （开启补全）
ji     及 即 既 极 急 几 系 奇 给 级 记 机              （关闭补全）
zei    贼 这(Fuzzy) 贼骨头 贼船 贼心不死 [贼克][贼人]   ← Fuzzy 抢占实证
si     死 四 似 伺 俟 似乎 思想 寺 司 斯 丝 思
fo     佛 否 缶 候(Fuzzy) 后(Fuzzy) 佛系 佛教 红 厚 鸿
chang  常 场 长 厂 唱 长度 场景 尝 肠 畅 昌 偿
women  我们 我们要 我们就 我们也 我们是 我们不 我们有 我们认为
```

注意 `zei` 第 2 位是 Fuzzy，`fo` 的 4-5 位和 8-12 位是 Fuzzy——这两处都是「纠错抢占」的实证，要写进 L4 用例。
