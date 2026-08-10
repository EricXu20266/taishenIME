# CHANGELOG 2026-08-10

## 候选词全量体检与优化（V0.5.7 候选质量）

### 体检方法
- 新增候选全量体检工具 `engine/tests/full_candidate_audit.rs`（`#[ignore]` 显式运行）：
  走完整 FFI 链路 + 真实词库（与部署 DLL 同代码），15,702 条用例（单字 3050 + 双字词 4953 + 三字词 7583 + common 表 558 + 简拼/混拼 22），判定标准 = 前 9 候选首屏（`candidate_count=9` 部署默认），实时落盘未命中明细（崩溃不丢结果）。
- 测试集生成器 `tmp/gen_audit_cases.py`：按拼音分组取词频 topN（单字 top9 / 双字 top3 / 三字 top3）+ 绝对词频下限，保证"期望首屏"是有意义的判定。

### 体检结果
- **总命中率 99.84%**（15,702 用例，25 未命中）
- 未命中归因三类：
  1. **简繁转换 bug（引擎）**：`trad_simp.rs` 无条件映射「乾→干」，词库 69 个含「乾」词条全部被破坏（乾隆→干隆、乾坤→干坤、乾陵→干陵）——实际均为 qián 音简体规范字，gān 音只出现在繁体表（不走 to_simplified）
  2. **common 表单字词频脱节（词表）**：`ji` 组选了 级/记/机/计/基（词频 3358/3256/3229），真实 top5 是 及/即/既/极/急（3951/3817/3557/3519/3402），15 个 char 未命中全由此产生
  3. **合理现象**：word3 专名（伯利安/印第安/承乾宫等）组内排名 1 但被力学/马良等真实高频词挤出——符合预期，不修

### 修复
- **引擎**：`trad_simp.rs` 删除「乾→干」逐字映射（简繁同形多音字，qián 音在简体中为规范字），新增回归测试 `test_qian_not_mapped_to_gan`（trad.rs）。验证：qianlong→乾隆、qiankun→乾坤、qianling→乾陵、niuzhuanqiankun→扭转乾坤 全部回首位。
- **词表**：`common_dict.txt` 12 组单字按真实词频重排（shi/ji/jing/mo/qi/xi/shen/yan/lan/wu/qing/jian），各组词频 top9 全覆盖；重建 `common.db`（583 条）与 `system_dict.db.bin`。
- **测试对齐**：`first_char_verify.rs` 首屏判定 前 5 → 前 9 + 显式 `engine_set_candidate_count(9)`（原测试用引擎默认 5，与部署 9 不一致导致误判）。

### 验证
- char 区 3,050 用例：2985/3000（99.5%）→ **3000/3000（100%）**
- 引擎 283 单元测试 + ffi_integration + first_char_verify 全绿
- biome check + cargo fmt 通过

### 已知问题（未修，独立记录）
- 引擎在 word2/word3 区间**连续查询 3000+ 次后偶发崩溃**（累积型内存损坏，哈希布局相关，无 panic 日志，小段跑可规避）——真实部署（逐次选词上屏）不触发，本次体检用分段（≤300 条）绕过；后续单独排查。
