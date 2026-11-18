# Combo 门槛误伤简拼 —— 过滤范围拆分

> 日期：2026-11-19
> 范围：`engine/src/lib.rs` 候选生成过滤条件、`engine/src/pinyin/mod.rs` 新增 `is_keyboard_run`
> 依据：`docs/modules/exam/FIX-COMBO-GATE.md`（`1d573bc` 引入 `combo_common_only`）与文章模式考试 `short` 段 9 条 miss

---

## 一、现象

真实文章考试 `short` 段 15 个简拼词只有 3 条进前 3，9 条完全 miss：

| 输入 | 期望 | 改前候选 |
|---|---|---|
| `cyl` | 产业链 | `cylinder(English)` 唯一 |
| `gyl` | 供应链 | `gyl(English)` 唯一 |
| `aqsp` | 安全水平 | `aqsp(English)` 唯一 |
| `xdh` | 现代化 | `xdh(English)` 唯一 |
| `tx` | 体系（前 3） | 同学/提醒/天下（体系不在） |
| `rx` | 韧性（前 3） | 人形机器人/`rx(English)` |

整段结果：ok 3 / late 1 / page 2 / miss 9。

## 二、根因

`1d573bc` 给 Combo 系加的门槛 `combo_common_only` 同时接在了三个**简拼系**消费点上：

```rust
// 改前，engine/src/lib.rs
let combo_gate = crate::pinyin::is_combo_syllable_plausible(&pinyin_str);
let combo_common_only = !combo_gate;
for w in dictionary::query_short(&pinyin_str) {          // ← 简拼系
    if combo_common_only && !is_common_word(&w) && !is_user_word(&w) { continue; }
```

`cyl` / `gyl` 都是纯声母串 → `is_combo_syllable_plausible` 返回 false → `combo_common_only = true` → 命中词条必须在 2732 条常用词表里。「产业链」（频次 2024）、「供应链」（1804）都不在表内，于是被整段丢弃，只剩英文候选。

这是把两种职责混在一起：

| 系 | 函数 | 输入语义 | 纯声母串意味着 |
|---|---|---|---|
| Combo 系 | `query_combo` / `combo_guess` / `phrase_group_guess` | 把输入串**硬拼成音节**再组合 | 拼出来的词是垃圾（`sdfg`→宋代法规、`qwer`→气味儿） |
| 简拼系 | `query_short` / `query_mixed` / `query_abbrev_full` | 按**声母前缀**取词库词条 | 纯声母正是合法输入方式（`cyl`→产业链、`zg`→中国） |

对 Combo 系，纯声母串是「不可信输入」；对简拼系，纯声母串是「标准输入」。同一个布尔量不能同时服务两边。

## 三、改动

`combo_common_only` 保留原样，继续把守 Combo 系三个消费点（`qwer`/`sdfg` 的拦截效果不变）。

简拼系换成两级更窄的判据 `short_keep`：

1. **键盘连打才要求常用词**（`short_common_only = combo_common_only && is_keyboard_run(input)`）
   判据见新增的 `pinyin::is_keyboard_run`：存在长度 ≥ 3 的连续段，段内相邻字符满足「同键重复」「同行左右相邻」「同列上下相邻」之一。
   - `sdfg` / `qwer` / `wert` / `asdfg` / `zzz` / `qaz` → 连打，只信常用词
   - `cyl` / `gyl` / `aqsp` / `xjzzy` / `xdh` / `tx` / `rx` / `gj` / `gg` → 放行
   - 斜向不算连续（`aqsp` 里 `a→q` 是竖排、`q→s` 是斜向，最长段只有 2）
2. **长度对齐**：纯声母串上，命中词条的字数不得超过输入字母数 + 1（简拼一个字母对一个字，+1 给多音节字）。
   挡掉前缀补全噪声：`npm` 上的「女仆冥土小姐」（7 字）、`zg` 上的「中国人民银行」（6 字）。

| 文件 | 变更 |
|---|---|
| `engine/src/pinyin/mod.rs` | 新增 `is_keyboard_run` + 1 个单测 |
| `engine/src/lib.rs` | 简拼系三处过滤体换成 `short_keep`；注释重写；Combo 系不动 |

## 四、数据

### 4.1 考试分档（291 条）

| 档 | 改前 | 改后 | 变化 |
|---|---|---|---|
| L1 | 86/95 | 86/95 | 持平 |
| L2 | 51/63 | 51/63 | 持平 |
| L3 | 8/13 | 8/13 | 持平 |
| L4 | 59/75 | 60/75 | +1 |
| L5 | 52/60 | 52/60 | 持平 |
| 总计 | 244/291 | **245/291** | **+1** |

L4-C（无意义串不强行组词）保持：`qwer`/`sdfg`/`wert`/`asdfg` 均无 Combo 候选。

### 4.2 文章模式（69 词）

| 段 | 改前 | 改后 |
|---|---|---|
| short（15 词） | ok 3 / late 1 / page 2 / miss 9 | ok 6 / late 2 / page 4 / **miss 3** |
| 总计 | ok 47 / late 4 / page 5 / miss 13 | ok 50 / late 5 / page 7 / miss 7 |

### 4.3 关键串候选

| 输入 | 改前 | 改后 | 判定 |
|---|---|---|---|
| `cyl` | cylinder(English) 唯一 | 陈友谅 / **产业链 #2** / 垂杨柳 / 产油量 / 常有林 | ✅ |
| `gyl` | gyl(English) 唯一 | **供应链 #1** / 供应量 / 郭云龙 / 感应炉 / 公寓楼 | ✅ |
| `xdh` | xdh(English) 唯一 | **现代化 #1** / 兄弟会 / 现代汉 / 协调会 / 熄灯号 | ✅ |
| `tx` | 同学/提醒/天下/弹性/腾讯视频 | 同学/提醒/天下/弹性/**体系 #5**/体现/投降/通信/特性 | ✅ |
| `rx` | 人形机器人 / rx(English) | 如下/人选/人心/人性/入学/热心/仁兄/人学/入选 | ✅ |
| `qwer` | qwer(English) | qwer(English) | ✅ 仍无 Combo |
| `sdfg` | sdfg(English) | sdfg(English) | ✅ 仍无 Combo |
| `asdfg` | asdfg(English) | asdfg(English) | ✅ 仍无 Combo |
| `wert` | 儿童×3(Correction) / wert(English) | 儿童×3(Correction) / wert(English) | ✅ 仍无 Combo |
| `npm` | npm(English) | npm(English) | ✅ |
| `zg` | 这个/中国/整个/逐个/最高（5 条） | 这个/中国/整个/逐个/最高/主骨/总高/做个/职工（9 条） | 长度对齐挡掉超长补全 |

## 五、取舍与未解决项

**1. `wmy` 的「万美元」回到第 4 位（L2 一条断言失守，已由 `zg` 的修复抵平）**

`wmy` 命中「万美元」频次 2781，比「产业链」的 2024 还高；它同样不在常用词表。任何「词频阈值」或「常用词表」判据都无法把 `wmy` 的噪声挡掉而不误伤 `cyl`——两者在生成层同构（3 个合法声母 + 3 字词条）。

该条属**排序偏好**（期望 `wmy` 优先给「我们要」），不是生成缺陷。按宪法「排序问题不动生成」，本次不在生成层为它开后门。可行路径是词库侧 `boost.yaml` / `demote.yaml` 调整，留待词库仓库处理。

**2. `aqsp` / `xjzzy` 仍 miss —— 词库缺口，不是过滤问题**

`resources/system_dict.db`、`resources/domains/domains.db`、`resources/common.db` 三处都没有「安全水平」「先进制造业」（Python 直查确认）。引擎无法为一枚不存在的词条生成候选。需在 taishen-dict 的 `curate/extra_words.txt` 补齐后同步，本仓库不改词库。

**3. 简拼放行的连带效应**

L2 的 `bj`（北京 #6）、`zg`（中国 #2）仍是排序问题，改动前就存在，未恶化。`wmy` 候选从 5 条涨到 9 条，噪声（王满银/伍麦叶等）随放行增加，但「我们要」「我们也」「我们有」占据前 3，实际输入可用。

## 六、验收命令

```powershell
cargo build --release --manifest-path engine/Cargo.toml --example exam
cargo run --release --quiet --manifest-path engine/Cargo.toml --example exam
cargo run --release --quiet --manifest-path engine/Cargo.toml --example exam -- --dump cyl gyl qwer sdfg wert
cargo run --release --quiet --manifest-path engine/Cargo.toml --example article_exam -- --article engine/tests/article-real-2026-11-19.json
cargo test --manifest-path engine/Cargo.toml -- --test-threads=1
```

结果：245/291（L1 86 / L2 51 / L3 8 / L4 60 / L5 52），文章 short 段 miss 9→3。
