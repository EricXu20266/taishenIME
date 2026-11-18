# 模糊音不抢占精确命中（L4-B 修复）

> 日期：2026-11-18
> 范围：`engine/src/lib.rs` 候选排序
> 约束遵守：只调顺序，未删 Fuzzy 候选、未关闭模糊音、未改用例与断言、未引入依赖。

---

## 一、根因

Fuzzy 候选在**生成阶段**的插入位置是对的——`query_all` 的模糊音分支在精确词库命中之后才 push。
问题出在后面的 `apply_word_len_match`（词长匹配分区），它的排序键是：

```
(group, exact_len_rank, user_rank, common_rank, combo_like)
```

这个键里没有来源信任度：Fuzzy 与 Dictionary 同权，于是被另外几个维度抬高：

| 输入 | 抬高维度 | 结果 |
|---|---|---|
| `nihao` | 字数对齐 | 「利好/理好」2 字落 exact 组；「你好坏」3 字落 longer 组 → Fuzzy 排在精确长词前 |
| `lisi` | 常用词加权 | 「历史」在常用词表、「李四」不在 → Fuzzy 直接第 1 |
| `zhangsan` | 字数 + 常用词 | 「长山/掌上」2 字、常用词加权 → 压在「张三丰/长三角」前 |
| `zei` | 字数分组 | 「这」单字与「贼」同组，词库词频序之外没有来源维度 → 第 2 |
| `fo` | 字数分组 | 候/后 单字与 佛/否/缶 同组 → 第 4/5 |

结论：不是 Fuzzy 生成位置错，是排序键缺「猜出来的候选必须让位于精确命中」这一维。

---

## 二、改动

文件：`engine/src/lib.rs`

1. **新增 `Engine::apply_fuzzy_demote`**
   - 触发条件：`fuzzy_enabled` 且候选中存在 `Source::Dictionary`。
   - 作用：在英文区之前对中文候选做稳定分区，Fuzzy 段整体后置。
   - 无 `Dictionary` 时不干预——这类输入下 Fuzzy 是唯一有意义的候选，保持原序。
   - `pin_words` 中的词不降级（用户显式置顶是更强的意图）。

2. **`apply_word_len_match` 排序键增加最高维 `fuzzy_rank`**
   ```rust
   (fuzzy_rank, group, exact_len_rank, user_rank, common_rank, combo_like)
   ```
   有精确命中时 Fuzzy 一律排 1、其余排 0。必须放在第一维：先分区后词长排序没用，
   稳定分区只是保持相对序，词长分组仍会把 2 字 Fuzzy 重新排到 3 字精确词前面。

3. **排序链尾部兜底**
   在单字母分区之后、`apply_demote` 之前再调用一次 `apply_fuzzy_demote`，
   防止 `apply_sort_mode`（单字/长词优先）和单字母分区把 Fuzzy 抬回前排。
   放在 `apply_demote` 之前——用户显式降权是更具体的意图，必须最后生效。

未改：候选生成、Fuzzy 变体计算、`select_candidate` 的「猜测候选不学习」逻辑。

---

## 三、通过率前后对比

命令：`cargo run --release --manifest-path engine/Cargo.toml --example exam`

| 级别 | 修复前 | 修复后 | 变化 |
|---|---|---|---|
| L1 全拼 | 85/95（89.5%） | 85/95（89.5%） | — |
| L2 简拼 | 48/63（76.2%） | 48/63（76.2%） | — |
| L3 混拼 | 5/13（38.5%） | 5/13（38.5%） | — |
| L4 纠错与联想 | 50/75（66.7%） | 54/75（72.0%） | +4 |
| └ L4 不含已知缺陷 | 38/60（63.3%） | 42/60（70.0%） | +4 |
| L5 边界 | 52/60（86.7%） | 52/60（86.7%） | — |
| **总计（不含已知缺陷）** | **228/291（78.4%）** | **232/291（79.7%）** | **+4** |
| 已知缺陷 | 12/15（80.0%） | 12/15（80.0%） | — |

L4-B「纠错不抢占」FAIL：**6 → 2**。

### 候选前后对照（page_size=9）

| 输入 | 修复前 | 修复后 |
|---|---|---|
| `nihao` | 你好, **利好/理好/立号/立好**(F), 你好坏 | 你好, 你好坏, 利好/理好/立号(F) |
| `zei` | 贼, **这**(F), 贼骨头, 贼船 | 贼, 贼骨头, 贼船, 贼心不死 |
| `fo` | 佛, 否, 缶, **候**(F), **后**(F) | 佛, 否, 缶, 否则, 否定 |
| `lisi` | **历史**(F), 李四, 李斯 | 李四, 李斯, 里斯 |
| `zhangsan` | 长三, **长山**(F), **掌上**(F) | 长三, 张三丰, 长三角, 张三爷, 张三李四 |

---

## 四、剩余 2 条 FAIL 及理由

L4-B 仍有 2 条 FAIL，均不是排序能解决的：

1. **`nihao @no_src=Fuzzy` 范围 5**
   修复后 #1 你好、#2 你好坏，非 Fuzzy 候选只有这 2 条。
   page_size=9 时纠错分支因候选已满而不再补入（`--top 30` 才出现 8 条 Correction）。
   要让前 5 无 Fuzzy，只能改候选生成（补入纠错/补全）或剔除 Fuzzy 候选——
   前者超出「只改排序」约束，后者违反「不删 Fuzzy 候选」。
   该项属候选生成/页宽耦合问题（REPORT §五·2 已记录），排序侧无解。

2. **`zhangsan 张三` 范围 5**
   `resources/system_dict.db` 中无「张三」词条（只有 张三丰 / 张三爷 / 张三李四 / 张三笑）。
   属词库覆盖缺口，须在 taishen-dict 源补词后经 `sync_to_ime.py` 同步，引擎侧不做特例。

---

## 五、验证

```bash
cargo build --release --manifest-path engine/Cargo.toml --example exam   # 零 error
cargo run --release --manifest-path engine/Cargo.toml --example exam     # 232/291
cargo test --manifest-path engine/Cargo.toml -- --test-threads=1
```

回归结论：无新增失败，L1/L2/L3/L5 与已知缺陷组完全不变。
