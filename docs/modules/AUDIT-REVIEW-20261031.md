# 引擎层重构复审（commit 10f0584..HEAD）

> 范围：`10f0584..HEAD`（10 个提交，引擎层审计优化落地）——`engine/src/lib.rs`、`engine/src/dictionary/mod.rs`、`engine/src/tuning.rs`
> 方法：diff 通读 + 当前文件逐点核对 + 用 `git show 10f0584:engine/src/lib.rs` 取旧实现做行为对照；纯只读，未执行 `cargo build/test/install`
> 日期：2026-10-31

## 1. 总体结论

结构性改造（side-table、原地分区、模式分发表、路径收敛）落地质量良好：**候选/来源表的成对性没有残余破口，锁顺序无反向获取，`permute_range` / `apply_demote` 的边界都守住了**，未发现数据损坏或稳定复现的 panic 路径。风险集中在两处**行为语义**：上下文加权排序键的改动面比文档描述的大（短列表也会翻转首选词），以及 `apply_demote` 之后仍执行 `apply_sort_mode` 导致降权在「单字优先」下部分失效。建议按 P1 补两条断言测试后合入，其余 P2 可随后续迭代处理。

## 2. 问题清单

- **[P1] engine/src/lib.rs:1308-1314 — 上下文加权新排序键的翻转面被低估，且无自动化回归保护** — 旧键 `(lift - i, lift, i)` 与新键 `(lift↓, i↑)`（`order.sort_by(|a,b| lifts[b].cmp(&lifts[a]).then(a.cmp(&b)))`）**并非只在长列表下不同**：对任意 `i_a < i_b` 且 `0 < lift_b - lift_a ≤ i_b - i_a` 的对，旧实现判 a 在前（或判平），新实现判 b 在前。即「第 2 位权重 1、第 1 位权重 0」这种短列表形态就会换首选词，而首选词就是空格上屏的内容。作者自己的探针 `zz_probe_context_top_risk`（lib.rs:3051，`#[ignore]`）选的正是 `我们|de`、`这个|shi`、`今天|de` 这三种最易翻转的形态，但只 `println!` 不断言；新语义的自动化覆盖只有 `test_context_weight_monotonic`（lib.rs:2988）、`test_context_boost_long_list_weight_beats_position`（2946）、`test_context_boost_equal_weight_keeps_order`（2965）三例合成列表，均不涉及真实词库首选率 — **证据**：`git show 10f0584:engine/src/lib.rs` 中 `apply_context_boost` 的 `scored` 为 `(lift - i as i64, lift, i)` 且 `sort_by(|a,b| b.0.cmp(&a.0).then(a.2.cmp(&b.2)))`；`docs/changelogs/CHANGELOG_2026-10-31.md` C1「12 组探针改前/改后逐字节一致」未落盘基线，无法复核 — **建议**：①把 `zz_probe_context_top_risk` 的 5 组用例转成断言（首位高频词在权重 ≤2 的搭配命中下不被顶掉）；②对 `docs/` 落一份改前候选快照作为基线，避免「探针一致」只存在于提交说明里。

- **[P1] engine/src/lib.rs:1367-1372 + 2294/2318 — `apply_demote` 之后仍执行 `apply_sort_mode`，降权在「单字优先」下被部分撤销** — 降权把命中词稳定分区到尾部后再 `rotate_left` 到索引 `demote_position`（默认 10）；紧接着 `apply_sort_mode`（sort_mode=1 单字优先）对 `[0, limit)` 整体重排，把单字候选提到最前。若降权词是多字词而单字候选只有 k 条（k < 10），降权词会从第 11 位被提到第 k+1 位——例如候选里只有 3 个单字 + 降权词「我们」时，「我们」回到第 4 位，进入首屏，「压出前 2 屏」的语义失效 — **证据**：`apply_demote` 的 `insert_at = demote_position.min(rest_len)`（1366）与 `apply_sort_mode` 的 `order.sort_by_key(|i| !single)`（1396-1399）作用于同一区间；执行顺序见 2294 → 2318 — **建议**：把 `apply_demote` 移到最后（Emoji 插入之后），或在 `apply_sort_mode` 里跳过 `Source` 为降权词的候选；并补一条 `sort_mode=1 + 降权` 的断言测试。

- **[P2] engine/src/lib.rs:1212-1228 — `permute_range` 校验失败时静默 no-op，且 debug 与 release 行为分叉** — 参数不合法（`order` 非 `[start,end)` 的排列）时 debug 构建走 `debug_assert!(false)` 直接 panic，release 构建直接 `return`，排序静默不生效且**无日志**。当前 5 个调用点（1287/1325/1357/1395/2441）都生成完整排列，属不可达分支；但它是「排序静默失效」与「测试环境崩溃」两种截然不同的表现 — **证据**：校验失败分支只有 `debug_assert!(false, ...)`+`return`，没有 `crate::log::error` — **建议**：release 分支补一条 `log::error`（与 `publish_with_src` 的降级日志对齐），或返回 `bool` 让调用方感知。

- **[P2] engine/src/lib.rs:1371 + 2687-2694 — release 下的长度兜底在管线末端，`apply_demote` 的 rotate 是唯一「失配即 panic」的点** — `publish_with_src` 在出口做 `srcs.resize` 兜底，但 `apply_demote` 的 `srcs[insert_at..].rotate_left(rest_len - insert_at)`（1371）发生在兜底之前：若上游某处漏 push 来源导致 `srcs.len() < candidates.len()`，`permute_range` 会因 `end > srcs.len()` 提前 return（两边都不动，安全），但随后的 `rotate_left(mid)` 会因 `mid > len` 直接 panic（release 同样 panic，rotate 的内建越界检查）。当前唯一入口纪律是「不漏 push」，无运行时防御 — **证据**：`permute_range` 的守卫在 1214-1216，`apply_demote` 的 rotate 在同一函数内 1367-1372，兜底在 2687-2694 — **建议**：`apply_demote` 入口加 `if candidates.len() != srcs.len() { return; }`（或 `debug_assert_eq!` + release 静默跳过），把不变量修复点前移到最靠近风险处。

- **[P2] engine/src/lib.rs:1346-1372 — 降权集合含英文候选时，「英文恒在末尾」与英文区边界被破坏** — `apply_demote` 按 `candidates.iter()` 全列表处理（注释亦如此声明），不排除 `Source::English`。若用户对某个英文候选执行降权（`engine_demote_word` 只校验非空，ffi.rs:402-413），该英文项会被搬到索引 10，英文块被切成两段；随后 `apply_sort_mode` / 之后任何轮次的 `first_index_of(Source::English)` 会把边界判在第 10 位，后续重排范围被截短 — **证据**：`apply_demote` 无 `srcs` 过滤；`apply_sort_mode`（1386）与 `apply_word_len_match`（2418）都用 `first_index_of(English)` 做右边界 — **建议**：`apply_demote` / `apply_pin_boost` 的分区谓词加 `&& srcs[i] != Source::English`，或在 `demote_word` 里拒绝英文候选。

- **[P2] engine/src/lib.rs:1580-1591 — `set_voice_candidate` 不重置 `page`、不走 `publish`，来源表按全局偏移查表会失配** — `candidate_src` 是**全局**表（`src_at` 用 `self.page * self.page_size + index`），而 `set_voice_candidate` 只把 `candidates` 置为 1 条、`candidate_src` 置为 1 条，没有 `self.page = 0` 也没有 `repage()`。若调用方在 `page > 0` 时注入（ffi `engine_set_voice_candidate` 的契约是先 `reset()`，属调用方责任），`src_at(0)` 取到 `None`，语音候选的来源判定全部落空 — **证据**：`src_at` 定义 lib.rs:909-913；`set_voice_candidate` 无 page/repage 处理，而同类的 `reset`（1602）、`backspace`（1636）、`backspace_syllable`（1710）都显式 `self.page = 0` — **建议**：`set_voice_candidate` 内补 `self.page = 0;` 并改走 `publish(vec![text], Source::Voice)`，让「发布口唯一」这条纪律不在这个函数上破例。

- **[P2] engine/src/lib.rs:1849 — `Mode::Compose` 直接下标 `self.compose_syllables[self.compose_idx]`，无 debug_assert 兜底** — `compose_idx < compose_syllables.len()` 这个不变量目前只由 `page()`（1423-1425，`split_compose_syllables()` 保证 ≥2 段）与 `select_candidate`（973-978，越界即 reset）共同维持，`detect_mode` 只判 `compose_active`。一旦将来新增「置 `compose_active = true` 但未填 syllables」的入口，就是 release 下的越界 panic（这正是本次审计要盯的「debug_assert 失效处仍有直接下标」形态） — **证据**：`apply_mode` 的 `Mode::Compose` 分支无 `.get()` 兜底，而同类函数 `compose_current_syllable`（865-869）用的是 `.get(...).unwrap_or_default()` — **建议**：改成 `.get(self.compose_idx)`，取不到时 `return false` 回落常规路径（与 `Mode::Radical` 的回落语义一致）。

- **[P2] engine/src/lib.rs:4927、5390 — 测试夹具直接给 `all_candidates` 赋值，破坏 side-table 不变量** — `engine.all_candidates = vec!["太深".to_string(), "太深".to_string()];` 之后 `candidate_src` 仍为旧长度/空，`repage()` 会把 `candidates` 填成 2 条而来源表对不上；当前因 `src_at` 用 `.get()`（返回 `None`）不 panic，但这类夹具会让「来源表等长」的 debug 断言在其它用例里偶发失效，也让后来者难以判断不变量到底由谁保证 — **证据**：`test_compose_entry_when_few_candidates`（4920-4932）、`test_syllable_display_compose_mode`（5384-5400） — **建议**：抽一个 `#[cfg(test)] fn set_all_candidates(&mut self, v: Vec<String>)` 同步写两份，或至少在这两处同时赋 `candidate_src`。

- **[P2] engine/src/tuning.rs:60-63 — `context_weight_cap` 允许配 0，会让上下文加权整体静默失效且无告警** — `min` 为 0，`parse` 只对「越界」告警，0 在区间内 → 不告警。而 `apply_context_boost` 在 `lifts.iter().all(|l| l == 0)` 时直接 return（1305-1307），表现出来就是「智能联想开了但没反应」，排查成本高（配置在 `%APPDATA%`，远离代码） — **证据**：`FieldSpec{ key:"context_weight_cap", min:0, max:100, default:20 }`；`learn_context_pair`（1189）与 lift 计算（1299）都以它为上限 — **建议**：`min` 提到 1；或在 `Tuning::load_from_disk` 里对「关键参数被配成 0/默认值偏离」打一条 `log::info`，让「为什么候选顺序变了」可从日志回答。

- **[P2] docs/changelogs/CHANGELOG_2026-10-31.md（A2 节末条）— 文档与实现矛盾，会误导后续维护** — A2 写「`apply_context_boost` 改下标排序 + `permute_range`，**排序键仍为 `lift - i`**」，而 C1 明确把排序键改成了权重降序 + 原位置；两节同属本批提交，读者无法判断当前实现是哪个 — **建议**：把 A2 那条改为「排序键此处不变，C1 再改」，或直接删除该半句。

## 3. 确认无问题

**A（候选来源表同步完整性）** — `query_all` / `query_all_shuangpin` 内 46 处 `candidates` 变更与 46 处 `cand_src` 变更**逐点成对**，无一遗漏：`query_all` 的 1890 初始化（`vec![Dictionary; len]`）、繁体外挂（1911/1912 与 1914/1915）、短语（1931/1932）、简拼/混合/缩写/模糊（1939-1979 各 5 组）、纠错三段（1997/2011/2087）、错音（2099）、组合联想与锚定插队（2115/2123/2158/2161）、预算截断（2179/2180，且 2183 重建 `cand_seen` 保持与原 `contains` 语义等价）、日期（2190）、英文四条分支（2244/2251/2260/2267/2274）、Emoji（2335/2336，边界改由来源表查，等价于旧 `*p += emo.len()`）、长句过滤 drain+回填（2353-2361）与短句回插（2364/2365）、单字母两分区（2377/2385）、末段 truncate（2391/2392）；`query_all_shuangpin` 结构同构（2519-2667）。清空分支（`set_voice_candidate` 1581-1589、`reset` 1599-1601、`backspace` 1633-1635、`backspace_syllable` 1707-1709）三者同清；`repage()`（2705）只动页窗口不动 `candidate_src`，与 `src_at` 的**全局偏移**语义（909-913）自洽，不构成失配。`publish`（2674-2679）与 `publish_with_src`（2682-2698）是唯一写入口，`all_candidates` 无裸赋值（除上述两条测试夹具，已列 P2）。

**B（`permute_range`）** — 参数校验完备：`end <= start + 1`（含 `end < start` 的 usize 下溢，先于 `end - start` 求值）、`order.len() != end - start`、`end > candidates.len()`、`end > srcs.len()` 四道；越界/重复检查用 `i < start || i >= end ||` 短路，保证 `seen[i - start]` 不会在 `i < start` 时被求值（无下溢 panic）。**校验失败时文本与来源表同时不动**（第 1227 行 return 在 `drain` 之前），不会出现「只动一边」的不一致；`moved_src` 先整体 collect 再写回，`moved_text` 用 `mem::take`，重复下标不会取到两次（且重复已被前置拒绝）。5 个调用点的 `order` 均为 `[start,end)` 的完整排列。

**C（锁顺序）** — 无反向获取。`paths()` 只在三类位置出现：①纯语句临时值（3285、3296、3298、3357、3405、3433、3463、3472、3955、3990 等），守卫在分号处释放；②先 `DICT.lock()` 后取 `paths()`（3293→3296、3399→3405、3462→3463、3972→3990），符合注释约定的 **DICT → PATHS**；③唯一的 `if let Some(p) = paths().user.clone()`（3577，`user_dict_db_path`）会把守卫延长到块结束，但块内只有 `return`，不二次加锁，当前无死锁。`init`/`init_blocking`/`set_user_dict_path` 的幂等检查已把 `loaded` 落成 `bool`（3284、3455、3527），DICT 守卫在判 `paths()` 前就释放——这是旧代码（`DICT_PATH` 守卫变量被延长）的改进点。领域线程（3332→3343）同样是 DICT→PATHS。

**D（`apply_demote` 边界）** — 0 条/1 条候选：`candidates.len() <= 1` 提前返回（1347）；全部命中降权：`rest_len = 0` → `insert_at = 0` → `insert_at < rest_len` 为假，不 rotate，纯分区结果稳定；无命中：`candidates.iter().any(...)` 提前返回（1351）；`rest_len >= demote_position` 时 `insert_at = demote_position`，`rotate_left(rest_len - insert_at)` 的 `mid` 恒 `< len = candidates.len() - insert_at`（因 `rest_len <= n`），**参数安全**；`tuning` 的 `demote_position` 下限是 1（tuning.rs:90），不会出现「降权词反被转到最前」的旋转。含英文区时：rest 段内的英文仍在 demote 段之后（最终序为 `R[0..insert_at] | D | R[insert_at..rest_len]`），「英文恒在末尾」不被这条 rotate 打破（英文块被切开的是另一条路径，见 P2 第 5 条）。旧实现的「非降权词 > 11 条时覆盖丢词 + 尾部重复」确实已消除（新旧实现对比：旧代码用 `rest`/`demoted` 两个新 Vec 按 `i` 写回）。

**E（release 不变量）** — `debug_assert` 失效处未发现新的越界访问：`permute_range` 的四处索引都有「`end <= len` + `order` 是排列」前置守卫（1212-1228）；`apply_pin_boost`(1331/1337)、`apply_sort_mode`(1397)、`apply_word_len_match`(2443)、`apply_context_boost`(1297/1313) 的 `candidates[i]` 全部 `i < limit <= len`；`page_inner` 的 `total_pages - 1` 被 `all_candidates.is_empty()` 早退保护（1435-1441）；`move_cursor` 的 `boundaries.len() - 1` 恒 ≥ 1（`vec![0]` 打底，1650）；`src_at` 用 `.get()`；模式分支的 `&self.pinyin_buf[1..]` 均由 `is_symbol_mode`/`is_radical_mode`/`is_number_mode`/`is_unicode_mode`（805-836）先判 `len() > 1` 且首字节为 ASCII，切片边界安全。两处「直接下标」已在 P2 列出（`compose_syllables[idx]`、`permute_range`/`apply_demote` 的长度依赖）。`publish_with_src` 的 release 兜底（2687-2694）本身正确（`resize` 双向补齐/截断 + 错误日志）。

**F（`apply_context_boost` 行为差异）** — 差异形态见 P1 第 1 条：`(lift - i)` → `(lift↓, i↑)` 的等价区间只有「所有 lift 相同」与「后位权重高出前位超过下标距离」两段，中间的 `0 < Δlift ≤ Δi` 全部翻转。测试覆盖面上，新语义有 3 条合成用例锁定（`test_context_weight_monotonic` 断言「第 11 位权重 1 的搭配词前置到首位」——这条在旧实现下会失败，说明测试是随语义一起改的，属有意变更而非回归掩盖），但**真实词库首选率无断言级回归**（2 个探针 `zz_probe_context_order`/`zz_probe_context_top_risk` 均 `#[ignore]` 只打印），这正是建议补测的部分。

## 4. 未覆盖

- 按约束未执行 `cargo build` / `cargo test` / `cargo install`：所有结论来自静态阅读，**未验证 358 项测试实际全绿**，也未验证 debug 构建下 `permute_range`/`publish_with_src` 断言是否被触发（这一点只能靠运行确认）。
- 未在真实词库上跑候选探针：P1 第 1 条（首选率变化）、`need_group`/`english` 相关分支的实际候选形态都缺少实测数据；`engine/examples/bench_query.rs` 未运行。
- `engine/src/ffi.rs` 只核了 `engine_pin_word` / `engine_demote_word` / `engine_set_voice_candidate` 三个入口的调用时序，未通审平台层（C++/TSF）如何使用 `candidate_display` 与 `select_candidate` 的返回值。
- `engine/src/dictionary/mod.rs` 的 124 行 diff 中，锁/全局相关的部分逐行核对，其余（若涉及查询层）未逐行比对；`dictionary::query` 的分层逻辑属上轮审计范围，本轮未复核。
- 繁体分支的一处语义替换未做逐字节验证：`traditional` 分支由 `!candidates.contains(w)` 换成 `cand_seen.insert(w)`（语义等价性已论证：`cand_seen` 初值等于 `candidates` 快照、后续同步增长），但未做逐字节候选对比。

---

## 5. 修复状态（2026-10-31 当日闭环）

| 编号 | 状态 | 处理方式 |
|------|------|----------|
| P1-1 上下文排序翻转面 | ✅ 已补断言 | 新增 `test_context_boost_short_list_flip_is_intended`（第 2 位权重 1 即可前置）；真实词库 5 组探针首位未变 |
| P1-2 降权被单字分区撤销 | ✅ 已修 | `apply_demote` 移到 `apply_sort_mode` 之后（query_all 与双拼两条路径），补 `test_demote_survives_single_char_sort_mode` |
| P2 `permute_range` 静默 no-op | ✅ 已修 | release 分支补 `log::error`，与 `publish_with_src` 的降级日志对齐 |
| P2 `apply_demote` 长度依赖 | ✅ 已修 | 入口增加长度校验，失配跳过并记日志（避免 rotate 越界 panic） |
| P2 英文候选可被降权 | ✅ 已修 | 降权谓词排除 `Source::English`，英文块不再被搬动，仍居末尾 |
| P2 `set_voice_candidate` | ✅ 已修 | 改走 `publish`，统一处理 page 重置与 repage |
| P2 `Mode::Compose` 直接下标 | ✅ 已修 | 改 `.get()` + 越界回落常规路径（与 Radical 回落语义一致） |
| P2 测试夹具破坏不变量 | ✅ 已修 | 两处 `all_candidates` 裸赋值同步写 `candidate_src` |
| P2 `context_weight_cap` 可为 0 | ✅ 已修 | 下限 0 改 1；文档区间同步 |
| P2 changelog 描述矛盾 | ✅ 已修 | A2 条目注明「排序键此处不变，C1 再改」 |

验证：`cargo test --lib -- --test-threads=1` → 362 项通过 / 0 失败 / 2 ignored（探针）。
