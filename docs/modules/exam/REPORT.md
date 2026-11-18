# 考试工具升级交付报告

> 日期：2026-11-18
> 范围：引擎来源 API + `exam.rs` 断言扩展 + 58 场景用例
> 纪律遵守：未改候选生成逻辑；未删现有用例；未为了让用例通过而放宽断言；未引入新依赖。

---

## 一、交付物

| 文件 | 变更 |
|---|---|
| `engine/src/lib.rs` | 新增 `Source::as_str()`（稳定来源名）与 `Engine::candidate_source_name(index)` |
| `engine/src/ffi.rs` | 新增 `engine_get_candidate_source_name(index, buf, len)`；旧的 `engine_get_candidate_source` 未动 |
| `engine/examples/exam.rs` | 9 类断言、分级统计、已知缺陷分离、失败明细带来源、`--dump` 校准、`--no-completion` / `--traditional` / `completion=off` / `traditional=on` |
| `engine/tests/exam-cases.tsv` | 306 条用例，覆盖 DESIGN 的 58 个场景；旧 44 条断言全部保留（仅重新归类） |
| `docs/modules/exam/SPEC.md` | 断言语法与分级口径 |

运行命令：

```bash
cargo run --release --manifest-path engine/Cargo.toml --example exam
```

---

## 二、通过率矩阵

词库：`resources/system_dict.db`（362,966 词条）；候选窗宽 9；前缀补全默认开启。

| 级别 | 通过/总数 | 通过率 | 说明 |
|---|---|---|---|
| L1 全拼 | 85/95 | 89.5% | 单字、双字词、长句、叠词、儿化 |
| L2 简拼 | 48/63 | 76.2% | 首屏、2-5 字简拼、不灌满 |
| L3 混拼 | 5/13 | 38.5% | **最低**：混拼几乎全靠 Combo，成句质量差 |
| L4 纠错与联想 | 50/75 | 66.7% | 含 15 条已知缺陷 |
| └ L4 不含已知缺陷 | 38/60 | 63.3% | **问题集中区** |
| L5 边界 | 52/60 | 86.7% | 退化输入全过，姓名/新词有缺口 |
| **总计（不含已知缺陷）** | **228/291** | **78.4%** | |
| 已知缺陷 | 12/15 | 80.0% | FAIL 不计入总计与退出码 |

旧用例回归：原 44 条断言全部通过（可换分组，未删未放宽）。
编译：`cargo build --release --manifest-path engine/Cargo.toml --example exam` 零 error。

---

## 三、L4 FAIL 明细（本次最核心的产出）

计入总计的 L4 FAIL 共 22 条，分四类。

### 3.1 纠错抢占（6 条）

| 输入 | 应然 | 实际 |
|---|---|---|
| `niha` | 你好 在前 | **#7 你好**；#1 你帮(Correction) |
| `nihaoa` | 你好 在前 | **#8 你好**；#1 鸟氨酸(Correction) |
| `zhongguoo` | 中国 在前 | **#8 中国**；#1-7 全 Combo/Sentence：中国哦/中国噢/中国喔/中姑婆/中国奥/中过驳/种姑婆 |
| `xianashi` | 现实 可达 | 前 9 无「现实」；下那是/下那时/腺癌是… 全 Combo |

结论：错拼时「纠错」与「组词」的相对优先级颠倒——多打一个字母（`zhongguoo`）不是先纠回「中国」，
而是被 Combo 拿去做「中国 + 语气字」的组词。这是「错误拼音强行组词」的直接证据。

### 3.2 模糊音抢位（6 条）

| 输入 | 实际 |
|---|---|
| `nihao` | #1 你好(Dictionary)，**#2-5 全是 Fuzzy**：利好/理好/立号/立好 |
| `zei` | **#2 这(Fuzzy)**，把「贼骨头/贼船」挤到 #3/#4 |
| `fo` | **#4 候 / #5 后 / #8 红 / #9 厚（全 Fuzzy）** |
| `lisi` | **#1 历史(Fuzzy)**，李四退到 #2 |
| `zhangsan` | #2 长山 / #3 掌上（Fuzzy）；**「张三」前 9 不可达**（#1 长三、#6 张三丰） |

结论：正常全拼已精确命中词库时，模糊音仍进前 5，属确定性抢占，不是偶发。

### 3.3 无意义串强行组词（4 条）

| 输入 | 候选总数 | 实际 |
|---|---|---|
| `qwer` | 9 | **9 条全 Combo**：气味儿/去问让/去问日/去问软/去位让/去位日/去位软/其问让/其问日 |
| `sdfg` | 9 | 前 5 全 Combo：宋代法规/是都发个/是都发给/是都发刚/是都法个 |
| `wert` | 9 | #2-5 Combo：瓦尔塔/瓦尔特/瓦尔托/韦尔塔 |

对照：`zxcvb` / `asdfg` / `ghjkl` / `uiop` 只出英文原串，未组词——说明门槛存在，
但「键盘邻键连打」这类串（qwer/sdfg）仍然漏过。**组词池噪声与「96% 是垃圾」的判断一致。**

### 3.4 英文词被中文压过（6 条）

| 输入 | 实际 |
|---|---|
| `key` | **#1 可以(Dictionary)**，key 在 #2 |
| `npm` | 完整用例中 **#1 女仆冥土小姐(Combo)**，npm 在 #2（单独跑时 npm 为 #1，见第五节） |
| `api` | 前 9 无 api；#1 排(Correction)、#2 支付宝(Correction) |

`test` / `code` / `app` / `hello` / `world` / `json` / `git` 通过，说明英文识别基本生效，
问题集中在「短英文串恰好是有效拼音前缀」（key→ke-y、api→a-pi）。

---

## 四、其他级别的典型 FAIL

### L3 混拼（5/13，38.5%）

| 输入 | 应然 | 实际 |
|---|---|---|
| `woainima` | 我们爱你吗 / 我爱你们 | 前 5 全 Combo：我爱你妈/我爱你吗/我爱你马/我爱你码/我爱昵吗 |
| `nihaoshj` | 你好世界 | 前 5 全 Combo：你好杀及/你好杀即/你好奥是/你好是就/你好是进 |
| `nhshijie` | 你好世界 | 前 5 全 Combo：你哈世界/你哈使节/你蛤世界/你蛤使节/嗯哈世界 |
| `moshoushijzhenhaow` | 成句且无垃圾段 | 前 5 全 Combo：没收世纪真好玩/没收世纪镇海奥… |
| `erty` | 不成句则不出词 | #3-5 Combo：儿童用/耳他有/耳他也 |

唯一亮点：`dianjishiyong` → #1 点击使用（优于 #4 电机使用），场景 29 通过。

### L2 简拼（48/63，76.2%）

- `zg` 中国 #2（#1 这个）；`bj` 北京 **#6**（本机/标记/编辑/不仅/被拒在前）。
- `zgrm`（中国人民）**前 9 全是 Combo 垃圾**：在个让没/在个让们/在个让吗…
- `sj` 只出 Datetime（`22:35` / `22:35:14`），「时间/世界/手机」全部不可达，候选总数仅 2。
- `zzz` 9 条全 Combo；`nhgbz` 只有英文原串，无中文候选。

### L1 全拼（86/96，89.6%）

- `zhonghuarenmingongheguo` #2/#3 是 Combo「中华人民共和过/果」——长词纠错后仍混入垃圾。
- `gaoxiaoxuexiao` 无「高等学校」，首位是 Combo「高校学校」。
- `huar` 无「花儿」（前 9 全是以 hua 开头的双字词）；`wanr` 玩儿 #6。
- `kankan` #3 砍砍、`changchang` #2 尝尝（次常用同音词，弱缺陷）。
- `mamama` 无「妈妈」精确项（前 9 是妈妈吗/妈妈码/吗妈妈…，末位是英文原串）；`bababa` 无「爸爸」。
- `e` 首位「鄂」，「饿」在 #7（零声母 e 的常用字排序不合理）。

### L5 边界（57/66，86.4%）

- 退化输入全过：空输入 count=0、30+ 音节长句成句（Sentence）、重复按键、26 个单字母均有候选。
- 姓名：`lisi` 李四 #2 通过；`zhangsan` 张三不可达；`wangwei` 王伟 #6；`liuyang` 刘洋不可达。
- 网络词：`xswl` → 笑死我了 #1 通过；`yyds` / `nsdd` 无对应词（全 Combo）。
- 术语：`suanfa` 算法被 #1 断法(Correction)、#2 转发(Correction) 压到 #3。
- 窗宽：`xian` 在 page_size 5 与 9 下第 5 位不同（西安 vs 现在），`@page_stable=5` FAIL。

---

## 五、测量环境效应（重要，影响结果解读）

1. **用例顺序会影响候选组成**。`npm` 单独跑：

   ```
   npm  count=1  #1 npm(English)
   ```

   在完整 306 条用例中：

   ```
   npm  #1 女仆冥土小姐(Combo)  #2 npm(English)
   ```

   已用两份小用例复现：仅含 `npm` 时通过；先跑若干 Combo 输入再跑 `npm` 时失败。
   引擎存在跨 `Engine` 实例的全局状态（疑似候选/切分缓存），属引擎缺陷。
   当前用例顺序固定，同一份文件的通过率可复现（两次全量运行的 FAIL 集合一致，仅 Datetime 文本随时钟变化）。

2. **候选窗宽影响候选组成**。`xian` page_size=9 时第 5 位「西安」，page_size=5 时第 5 位「现在」。
   候选生成未与页宽解耦，`@page_stable` 已将其固化为回归用例。

3. `sj` 的 Datetime 候选带实时时间戳，失败明细文本每次不同，判定不受影响。

---

## 六、断言语法最终版

| 写法 | 语义 |
|---|---|
| `期望词` | 该词在前 N |
| `!词1,词2` | 这些词不得出现在前 N |
| `>词A>词B>词C` | 顺序：A 在 B 前、B 在 C 前 |
| `@src=Dictionary` | 前 N 全部来自该来源（至少 1 个） |
| `@has_src=Correction` | 前 N 至少一个来自该来源 |
| `@no_src=Correction,Combo` | 前 N 不得有这些来源 |
| `@multi=0` / `@multi>=1` / `@multi<=2` | 前 N 多字词数量约束 |
| `@count<=N` / `@count>=N` | 候选总数约束 |
| `@page_stable=N` | 窗宽 N 与主窗口前 N 一致 |

备注开关：`completion=off`（关闭前缀补全）、`traditional=on`（繁体输出）。
来源名：Dictionary / Phrase / Datetime / English / Emoji / Correction / Combo / Fuzzy / Mode / Voice / Sentence / Completion。

覆盖统计：6 类任务书要求的新断言各有 ≥3 条用例；`@page_stable` 为额外补充。

---

## 七、后续建议

按修复收益排序。

1. **纠错/模糊音不应抢精确命中的位**（L4-B，6 条）
   正常全拼已有 Dictionary 命中时，Fuzzy 应整体排在 Dictionary 之后，或仅在前 N 不足时补位。
   影响：`nihao` `fo` `zei` `lisi` `zhangsan` 的直接观感。

2. **纠错优先于组词**（L4-A，4 条）
   `zhongguoo` 应先纠回「中国」，而不是生成「中国哦/中国噢」。
   建议：整串音节+尾部冗余时，Correction 排在 Combo/Sentence 之前。

3. **Combo 生成门槛**（L4-C + L3，9 条）
   对「无完整音节边界」的串（qwer/sdfg/wert/zzz）与随机混拼（woainima/nihaoshj）禁用或严格限制 Combo。
   这类候选占满前 9，是「组词池 96% 垃圾」的直接来源。

4. **简拼二次排序**（L2，6 条）
   `zg`/`bj`/`zgrm` 的首位被高频短词抢占；`sj` 被 Datetime 完全占据。
   建议：完整音节简拼（如 `sj` 可切 shi-jian）优先于纯声母串；Datetime 简码在无「时间语境」时降权。

5. **补全灌满**（L4-D 已知缺陷，3 条）
   `zei`/`women`/`ji` 候选数恒为 9 且含补全/多字词。`exact_saturated` 已存在，但未覆盖这些输入。
   建议：精确命中饱和后截断补全，不补满窗口。

6. **候选与页宽解耦**（L5-C，1 条）
   `xian` 的 page_size 5/9 结果不同，需查分页/截断时序。

7. **跨用例全局状态**（引擎缺陷，非用例问题）
   `npm` 的顺序效应说明查询缓存跨 Engine 实例泄漏，建议排查 `dictionary` 层的全局缓存与 `completion` 排序缓存。

8. **词库覆盖（非排序问题）**
   `huar` 无「花儿」；`mamama`/`bababa` 无「妈妈/爸爸」精确项；`liuyang` 无「刘洋」。
   这类应回到 taishen-dict 侧补词，不要在引擎里做特例。

9. **用例侧待校准项（弱断言，优先级低）**
   `e→饿`（#7）、`kankan !砍砍`（#3）、`changchang !尝尝`（#2）、`mamama 妈妈`（无精确项）。
   这些是「次常用字/词的排位偏好」，可在引擎修完后重新评估阈值，不属于本轮缺陷。

---

## 八、复现

```bash
# 全量
cargo run --release --manifest-path engine/Cargo.toml --example exam

# 查看单个输入的候选与来源
cargo run --release --manifest-path engine/Cargo.toml --example exam -- --dump zhongguoo qwer nihao

# 关闭补全对照
cargo run --release --manifest-path engine/Cargo.toml --example exam -- --no-completion
```
