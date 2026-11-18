# Combo 音节合法性门槛

> 日期：2026-11-18
> 范围：引擎 Combo 候选生成前置门槛（不改生成算法、不改词库、不改用例、不放宽断言、不加依赖）
> 依据：`docs/modules/exam/REPORT.md` 3.3 节（无意义串强行组词 4 条 FAIL）与第五节（用例顺序效应）

---

## 一、问题

宽松匹配（段可以是单声母、半截音节）让键盘连打串也能凑出词库里的真实词条：

| 输入 | 改前实际 |
|---|---|
| `qwer` | 9 条全 Combo：气味儿/去问让/去问日/去问软/去位让/去位日/去位软/其问让/其问日 |
| `sdfg` | 前 5 全 Combo：宋代法规/是都发个/是都发给/是都发刚/是都法个 |
| `wert` | #2-5 Combo：瓦尔塔/瓦尔特/瓦尔托/韦尔塔 |

`qwer` 被解释成 q + w + e + r（四个单字母），`sdfg` 是 s + d + f + g。
这类串按音节解释时并不从真实音节起头，不是拼音输入，组出来的词必然是噪声。

## 二、改动

| 文件 | 变更 |
|---|---|
| `engine/src/pinyin/mod.rs` | 新增 `is_combo_syllable_plausible(input) -> bool` 门槛 + 1 个单测 |
| `engine/src/lib.rs` | 6 个 Combo 消费点接入门槛（简拼补充 / 混合简拼 / 缩写+全拼 / query_combo / combo_guess / phrase_group_guess） |

行数：`lib.rs` +29，`pinyin/mod.rs` +45（含注释与单测），未删改任何既有生成逻辑。

## 三、门槛判据

判据分两级。原因是纯声母串（`nh`→你好、`zg`→中国）本身是合法简拼，
一刀关掉会打断输入法底线行为；但它们与 `sdfg` 结构同构，只能靠词库质量分开。

**第一级 · 拼音可信（`combo_gate = true`）**

满足任一即放行全部 Combo 结果：

1. 整串可切分为完整音节序列 —— `zhongguo`、`zhongguoo`（错拼但仍可切）、`dianjishiyong`、`huar`；
2. 首段是长度 ≥ 2 的完整音节 —— `nimzai`(ni+m+zai)、`heshenm`(he+shen+m)、`ganshm`(gan+sh+m)、`yaowoquz`(yao+wo+qu+z)、`zhegew`(zhe+ge+w)；
3. 声母起头但**音节为主导**：完整音节覆盖的字母数 > 声母段字母数 —— `bru`(b+ru，2>1)、`zhyang`(zh+yang，4>2)、`shurf`(shu+r+f，3>2)。

第 3 条是 `cargo test` 反逼出来的：一开始只认「首段完整音节」，把 `bru`/`zhyang`
这类「声母+全拼」的混合简拼误判成纯声母串，`test_mixed_abbrev_full_and_hotword`
直接红。加上「音节字母数 > 声母字母数」后，`qwer`(q+w+er，2 不 > 2)、
`wert`(w+er+t，2 不 > 2) 仍被拦，混拼保住。
`a/e/o` 这类单字母音节计入声母侧，否则 `qwer` 的 `e` 会把键盘串洗白。

含数字/符号等非小写字母的输入不放行，交给英文/混输路径。

**第二级 · 纯声母串（`combo_common_only = true`）**

不关路径，只保留常用词表命中或用户学过的词（`is_common_word` / `is_user_word`）。
`nh`→你好 留下，`sdfg`→宋代法规、`wert`→瓦尔塔、`qwer`→气味儿 全部滤掉。

用户词必须显式放行——`ts`→泰深 是 learn 出来的词，不在 common 表里；
漏了这一条 `test_delete_user_word_short_pinyin` 与 `test_ts_input_priority_user_word` 会红。

判据不做长度判断——`nh`(2) 与 `zgrm`(4) 不会因长度被区别对待。

## 四、前后对比

词库 `resources/system_dict.db`（362,966 词条），候选窗宽 9，前缀补全默认开启，
用例顺序固定（同一份 `exam-cases.tsv`）。

| 级别 | 改前 | 改后 | 变化 |
|---|---|---|---|
| L1 全拼 | 85/95（89.5%） | 85/95（89.5%） | 0 |
| L2 简拼 | 48/63（76.2%） | 51/63（81.0%） | **+3** |
| L3 混拼 | 5/13（38.5%） | 5/13（38.5%） | 0 |
| L4 纠错与联想 | 54/75（72.0%） | 59/75（78.7%） | **+5** |
| └ L4 不含已知缺陷 | 42/60（70.0%） | 47/60（78.3%） | **+5** |
| L5 边界 | 52/60（86.7%） | 52/60（86.7%） | 0 |
| **总计（不含已知缺陷）** | **232/291（79.7%）** | **240/291（82.5%）** | **+8** |
| 已知缺陷 | 12/15 | 12/15 | 0 |

## 五、验收结果

| 验收项 | 判据 | 结果 |
|---|---|---|
| `qwer` 不出 Combo | `--dump qwer` | ✅ `count=1 #1 qwer(English)` |
| `sdfg` 不出 Combo | `--dump sdfg` | ✅ `count=1 #1 sdfg(English)` |
| `wert` 不出 Combo | `--dump wert` | ✅ 仅 3 条 Correction(儿童系) + `wert(English)`，无 Combo |
| L4-C 相应条目转 PASS | 全量用例 | ✅ 4 条不再出现在失败明细 |
| `dianjishiyong` 不倒退 | `--dump` | ✅ `#1 点击使用(Dictionary)`，与改前一致 |
| 总通过率不降 | 全量用例 | ✅ 232 → 240 |
| 简拼基本盘不退化 | `--dump nh zg bru zhyang` | ✅ `nh #1 你好`、`zg #2 中国`、`bru #1 比如/#2 不如`、`zhyang #1 这样` |
| `cargo test` 无新增失败 | `--test-threads=1` | ✅ 388 passed / 0 failed（见第六节） |

## 六、副作用与取舍

改后出现的可见变化，逐条列明：

| 现象 | 原因 | 判断 |
|---|---|---|
| `bj`→北京 掉出候选 | 「北京」不在常用词表（common 2732 条），纯声母路径的 Combo 被 common 过滤 | 已知取舍。简拼正解应走简拼索引而非 Combo；留待下一轮把「高频系统词」并入白名单 |
| `zgrm`→中国人民 / `rmzj`→燃眉之急 / `nhgbz` 不出中文 | 同上，长简拼词组不在常用词表 | 这三条的正解断言原本就 FAIL；`@no_src=Combo` 断言转 PASS，净收益为正 |
| `nh`→你好 保住 #1 | 你好在常用词表，第二级过滤放行 | 简拼底线未被破坏 |
| `zg`→中国 仍 #2 | `这个`(common) 先入列，与改前一致 | 未恶化 |
| `bru`/`zhyang`/`shurf` 行为不变 | 第 3 条判据（音节为主导）放行 | 混合简拼未退化 |
| `ts`→泰深 简拼仍命中 | 第二级过滤放行 `is_user_word` | 用户词未被误伤 |
| `wert` 仍 `#1 儿童(Correction)` | 纠错路径，不属于 Combo | 本轮范围外（REPORT 3.1 纠错抢占） |
| `woshiEric` 仍出 Combo | 归一化后 `woshieric` 首段 `wo` 是真音节 → 第一级放行 | 本轮范围外（L5-A 中英混输） |

`cargo test` 结果：

```
lib       : 384 passed; 0 failed; 5 ignored（131.6s）
集成测试   : 全部 0 failed（含 ffi_integration / candidate_fix / repro_issues 等 9 个文件）
```

## 七、本轮未做

| 类别 | 案例 | 不做的原因 |
|---|---|---|
| 音节合法但组词垃圾 | `woainima`、`nihaoshj`、`nhshijie`、`moshoushijzhenhaow` | 首段是完整音节（wo / ni / mo），门槛按设计放行；再往下筛就是语义质量评分，属下一轮 |
| 错拼纠错抢占 | `zhongguoo`、`nihaoa`、`xianashi` | 纠错与组词的相对优先级问题，与 Combo 门槛正交 |
| 英文词被中文压过 | `key`、`api`、`app` | 拼音/英文判定与排序问题 |
| 简拼长词正解缺失 | `zgrm`、`rmzj`、`nhgbz` | 属常用词白名单覆盖问题，改白名单会动词库口径 |

## 八、验证命令

```bash
cargo build --release --manifest-path engine/Cargo.toml --example exam
cargo run --release --manifest-path engine/Cargo.toml --example exam
cargo run --release --manifest-path engine/Cargo.toml --example exam -- --dump qwer sdfg wert dianjishiyong nh zg
cargo test --manifest-path engine/Cargo.toml -- --test-threads=1
```

`exam` 退出码 1 属预期——它表示仍有 FAIL 用例，不代表构建失败。

`cargo fmt --check`：本次改动的两个文件（`engine/src/lib.rs` 改动区、`engine/src/pinyin/mod.rs`）无格式差异。
仍报差异的两处属既有未格式化代码，与本次无关：`engine/src/ffi.rs:311`、`engine/src/lib.rs:3206`。

Biome 未跑：本次只改 Rust，`biome.json` 仅覆盖根目录 JSON/JS/TS，对 `engine/` 无检查口径。

