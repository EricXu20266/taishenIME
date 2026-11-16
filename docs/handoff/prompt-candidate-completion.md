# 引擎侧任务 · 前缀补全（Completion）

> 用法：新会话开新对话，把本文件全文作为第一条消息发过去。
> 前置：候选体系五轮词库工程已完成（`bigram-acceptance-*` / `fragment-cleanup-*` / `longword-restore-*`）
> 产出：`Source::Completion` 实现 + 可复现的评分提升

---

## 目标

把「打一半就看得到目标词」的能力补上：

| 指标（`probe_prefix`，1,170 条真实输入留痕） | 现在 | 目标 |
|---|---|---|
| 打满全部音节时前 5 命中率 | 98.7% | **不得退化** |
| **只差一个音节时前 5 命中率** | **4.8%** | **≥ 30%** |
| 只打第一个音节时前 5 命中率 | 2.0% | 尽力，不作硬指标 |
| `check_first` FAIL | 0 | 仍为 0 |

**打满 98.7% 是及格线，不是竞争力。** 真实使用是「打一段看候选，出现想要的词就停」，
用户不会打满。

## 先看清现状：不是「没做前缀匹配」，是「做了但被埋了」

`dictionary::query` 的签名就是 `query(pinyin_prefix: &str)`——**它本来就是前缀查询**。
实测：

```
lian*  共 3,117 条  → 联系 词库第 6、联合 第 7、连续 第 10
tong*  共 1,886 条  → 通常 词库第 8
xian*  共 4,085 条  → 显示 词库第 24
但引擎候选前 5：lian = 脸/连/链/练/凉了    tong = 同/通/统/铜/痛
```

根因在 `apply_word_len_match` 的分组：

```
输入 lian（1 音节）→ counts = [1]
  单字（两/连/脸）      → group 0（字数 == N）
  多字词（联系/联合）    → group 2（字数 > N）
```

前 5 全是 group 0，completion 候选永远进不来。

**所以缺的是「候选的身份」，不是索引。** `query` 返回 `Vec<String>`，
在返回值里把「精确命中」与「前缀扩展」的边界抹掉了，引擎只能按字数猜，一猜就错。

## 设计（对应设计稿 7.6 节）

### 1. 新增候选来源

```rust
enum Source {
    ...
    /// 前缀补全：输入停在完整音节边界，候选拼音以输入开头且更长
    Completion,
}
```

### 2. 分组改为四级

| group | 内容 | 判据 |
|---|---|---|
| 0 | exact | 字数 == N |
| 1 | **completion** | `source == Completion` |
| 2 | shorter | 字数 < N |
| 3 | longer | 字数 > N，非 completion |

### 3. 触发条件（三条同时满足，缺一不可）

1. **输入停在完整音节边界**：`crate::pinyin::is_complete_pinyin(input)` 为真。
   `lian` 是，`lianx` 不是（`lianx` 继续按拼音串正常查，不展开）
2. 候选词拼音以输入开头且**更长**（严格前缀扩展，不等于输入本身）
3. **限额 5 条**，按词频降序

**限额是硬要求**：`lian*` 3,117 条、`xian*` 4,085 条。不限额会把单字全挤掉，
重演 V0.5.3 词长分区当年要解决的「过度联想」问题。

### 4. 排序链的配套改动

`combo_like` 维（`apply_word_len_match` 的第五维）当前把 `Sentence | Combo` 判为 1。
`Completion` **必须判为 0**——它来自词库前缀匹配，是可信的；`Combo`/`Sentence` 是拼出来的串。
这一条搞错，刚修好的「整句产物压过真实词条」会以新形式复发。

---

## 实施步骤（每步都要跑验证，不要一口气改完）

### 步骤 1：dictionary 暴露分段信息

`query` 返回 `Vec<String>`，丢失了分段。改成能区分「精确段」与「前缀扩展段」。

推荐做法：新增 `pub fn query_with_split(pinyin: &str) -> (Vec<String>, usize)`，
第二值 = 精确段长度；原 `query` 保留为薄包装（`query_with_split(p).0`），
避免影响其他调用点。

**验证**：此步不改行为。跑 `probe_prefix`，指标必须与基线**完全一致**
（4.8% / 98.7%）。不一致说明包装写错了。

### 步骤 2：接上 Source::Completion

在 `query_all` 里，对 `query_with_split` 返回的「前缀扩展段」（`index >= exact_len`）
标记 `Source::Completion`，并按词频降序截取前 5 条。

**验证**：跑 `probe_prefix`，看「只差一个音节」是否上升；同时跑 `check_first`
确认 FAIL 仍为 0（若单字被挤掉，会立刻出现 FAIL）。

### 步骤 3：改 group 计算

`apply_word_len_match` 的分组加 completion 分支（见设计第 2 点），
并把 `combo_like` 维里 `Completion` 判为 0。

**验证**：三项指标全跑一遍。

### 步骤 4：调参

三个可调项，逐个试，记录每次指标：

- 限额：3 / 5 / 8 / 不限
- 是否要求「扩展后拼音长度 ≤ 输入 + 一个音节」（更保守）
- 是否要求输入音节数 ≤ 2（长输入不展开）

**判据**：只差一个音节命中率尽量高，同时 `check_first` FAIL 为 0、打满 98.7% 不退化。

### 步骤 5：全量验证与部署

- `cargo test --manifest-path engine/Cargo.toml --release --lib -- --test-threads=1`
  必须全绿（当前基线 382 passed / 5 ignored）
- `probe_candidates` 抽测 `lian` / `tong` / `xian` / `shi`，确认前 5 仍以单字为主、
  completion 词紧随其后
- 全平台产物重建部署（4 个 DLL + `bigram.db` + `common.db`，**别漏 bigram**）
- `verify_build.py` / `verify_resources.py` 双校验

---

## 必须守住的底线（违反任一条即回滚）

1. **打满 98.7% 不得退化**（`probe_prefix` 指标 1 全拼组首位 84.9%/首屏 98.1% 也不得降）
2. **`check_first` FAIL 必须仍为 0**（单字首屏不能被 completion 挤掉）
3. **`probe_context` 120 例 ≥ 95**（搭配表效果不能因新来源受损）
4. **lib 全量测试全绿**（382 passed 基线）

## 测试卷（`docs/handoff/prompt-candidate-completion.md` 配套）

```powershell
# 真实输入留痕（1,170 条，来自 user_dict.db 的混杂输入）
cd E:\AllinDeepSeek\taishenIME\engine
cargo run --release --example probe_prefix -- ..\tmp\prefix_cases.tsv

# 首屏质量（409 音节）
cargo run --release --example check_first

# 搭配表效果（120 例）
cargo run --release --example probe_context -- ..\tmp\context_cases.tsv

# 候选抽样
cargo run --release --example probe_candidates -- lian tong xian shi
```

`tmp/prefix_cases.tsv` 若不存在，按下面重建（从用户词库导出高频多字词）：

```python
import sqlite3, os
p = os.path.expandvars(r'%APPDATA%\taishen-ime\user_dict.db')
c = sqlite3.connect(p)
rows = c.execute("select pinyin, word, frequency from user_dict where length(word)>=2 and frequency>=3 order by frequency desc").fetchall()
with open(r'E:\AllinDeepSeek\taishenIME\tmp\prefix_cases.tsv','w',encoding='utf-8') as f:
    f.write('# word\tuser_pinyin\tfrequency\n')
    for py,w,fr in rows: f.write(f'{w}\t{py}\t{fr}\n')
```

## 约束

1. 只改 `taishenIME` 仓库（引擎 + 平台层），不动词库侧。
2. **文档、代码注释、commit 说明中不得出现任何竞品/其他输入法名称**。
3. 每完成一个步骤就 commit（可回溯），不要攒成一个大提交。
4. 改前先读 `docs/modules/candidate-ranking/DESIGN.md` 的 7.6 节与第五章排序链。
5. 若某一步指标不达标，**先把测量结果记下来再调**，不要凭感觉改参数。

## 交付

- 报告 `docs/modules/candidate-ranking/COMPLETION-IMPL.md`：每步的指标变化、
  最终参数选择与理由、四项底线的验证结果
- 设计稿 7.6 节状态更新为「已实现」，补上实际参数
- 三个 commit：dictionary 分段 / Source::Completion / group 与调参
