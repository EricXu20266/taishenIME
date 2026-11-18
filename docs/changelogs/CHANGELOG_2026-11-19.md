# CHANGELOG 2026-11-19

## Combo 门槛不再误伤简拼查询

### 现象

真实文章考试 `short` 段 15 个简拼词只有 3 条进前 3，9 条完全 miss：

| 输入 | 期望 | 修复前候选 |
|------|------|-----------|
| `cyl` | 产业链 | `cylinder(English)` 唯一 |
| `gyl` | 供应链 | `gyl(English)` 唯一 |
| `xdh` | 现代化 | `xdh(English)` 唯一 |

### 根因

`1d573bc` 引入的 `combo_common_only` 同时接在 Combo 系（`query_combo` / `combo_guess` / `phrase_group_guess`）与简拼系（`query_short` / `query_mixed` / `query_abbrev_full`）上。

纯声母串 `cyl` 触发门槛 → 命中词条必须在 2732 条常用词表内 → 「产业链」（频次 2024）、「供应链」（1804）不在表内 → 整段丢弃。

两类查询语义不同：Combo 系是把输入串**硬拼成音节**（纯声母拼出来是垃圾），简拼系是按**声母前缀取词**（纯声母正是合法输入）。

### 改动

- `engine/src/pinyin/mod.rs`：新增 `is_keyboard_run`（同键重复 / 同行左右 / 同列上下，连续段 ≥3）+ 单测
- `engine/src/lib.rs`：简拼系三处过滤体换成 `short_keep`
  - 键盘连打才要求常用词（`sdfg` / `qwer` / `zzz` / `qaz`）
  - 纯声母串上，命中词条字数不得超过输入字母数 +1（挡 `npm`→女仆冥土小姐、`zg`→中国人民银行）
- `combo_common_only` 原样保留，继续把守 Combo 系

### 效果

| 指标 | 修复前 | 修复后 |
|------|-------|-------|
| 考试总计 | 244/291 | **245/291** |
| L4 | 59/75 | 60/75 |
| 文章 short 段 | ok 3 / miss 9 | ok 6 / miss 3 |

`cyl` 出产业链（#2）、`gyl` 出供应链（#1）、`xdh` 出现代化（#1）；`qwer`/`sdfg`/`asdfg`/`wert` 仍无 Combo。

详见 `docs/modules/exam/FIX-COMBO-SHORT.md`。
