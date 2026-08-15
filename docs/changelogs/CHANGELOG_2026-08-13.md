# CHANGELOG 2026-08-13

## V0.5.13 音节可视化（贴近微软）+ 部署期 .bin 预生成

> 基于 2026-08-13 竞品审计结论（言泉 Cassotis 对比）：音节可视化是核心待办，
> 引擎切分能力现成、前台未接；加载优化已基本到位，仅补部署期预生成。

### 音节可视化（输入全程显示音节分隔）
- **引擎** `engine/src/lib.rs`：
  - 新增 `split_syllables_lenient()` 宽松切分：完整音节优先 → 声母（zh/ch/sh 优先）→ 逐字符兜底，
    无「≥2 段且≥1 完整音节」门槛（单音节 wo、纯声母 zg 也返回），供显示与光标移动全程使用
  - 新增 `syllable_display()`：非组词返回全串带 `'` 分隔（zhongguo→zhong'guo、zg→z'g、tshen→t'shen）；
    组词模式返回 compose_idx 起至末尾带分隔（候选窗指示当前音节）
  - `syllable_boundaries()` 改走宽松切分：简拼串（zg）Tab/Ctrl+BackSpace 也能按音节移动，与显示对齐
- **FFI** `engine/src/ffi.rs` + `platform/windows/include/engine_bridge.h`：新增 `engine_syllable_display`
- **前台**：
  - `tsf_module.cpp`：候选窗拼音区统一用 `engine_syllable_display`（去掉「组词: 」前缀）；
    非组词 composition 也显示音节分隔（zhongguo→zhong'guo，贴近微软）
  - `imm32_ime.cpp`：候选窗拼音区同步音节分隔（LOL 场景一致）
- **Tab/Shift+Tab 音节移动**：tsf_keyevent.cpp 本无组词约束（全程可用），引擎侧 boundaries 对齐后简拼也可跳

### 部署期 .bin 预生成（P1）
- 新增 `engine/src/bin/build_index.rs`：命令行工具调 `dictionary::build_index` 生成 .bin
- `package.ps1` 集成：`resources/system_dict.db.bin` 缺失或早于 .db → 打包时 `cargo run --release --bin build_index` 预生成，
  保证安装包自带 .bin（用户首启秒开，免 SQLite 全量重建 6-7s）

### 验证
- 引擎 12 项 syllable 相关测试全过（全拼/简拼/混拼/单音节/空串/组词模式/边界移动）
- 全量 lib 测试 301/302 通过（唯一失败 `test_delete_user_word_short_pinyin` 为并行隔离 flaky——该测试不持 TEST_DICT_LOCK，
  与 learn 测试并行竞争全局用户词库；单线程复验通过）
- cargo fmt + clippy（新代码无新增 warning）；biome check 通过
- CMake Release 全量编译通过（taishen_ime.dll + taishen_ime_imm32.ime + 全部冒烟测试目标）
- 冒烟：test_tsf_load PASSED / test_config_reader PASSED / test_imm32_load PASSED
  （test_ascii_mode STEP4 与 test_ui_framework STEP4 为既有环境/设计问题，与本次改动无关）

## V0.5.14 用户词持久化修复（并行会话提交，一并记录）

- 核心：`load_dict_async` 大词库换入时迁移用户词状态（user_index/user_short_index/user_full_index/user_dict_path），
  修复每次 DLL 更新/进程重启后历史组词从内存消失（磁盘仍在）
- `engine_init` 幂等：Engine 已存在不重建，置顶/降权二次激活不再丢
- `set_user_dict_path(None)` 清空三个用户索引（原只清 user_index）
- 新增 `clear_user_words()` 测试隔离 API；用户词相关测试加并行隔离（TEST_DICT_LOCK + 清理）
- IMM32 层补 ImmInstallIME 注册（install.ps1）

### V0.5.14 组词拼音显示修复（编辑区剩余音节带分隔）

- `tsf_module.cpp`：
  - 编辑区 composition 组词分支从 `engine_compose_info`（只返回**当前音节**）改为 `engine_syllable_display`
    （剩余全部音节带分隔）——修复「组词时只显示当前候选字拼音、不显示剩余拼音」：
    进入组词显示 `deng'nihui'lzhih`，选"等"后编辑区显示 `nihui'lzhih`（与候选窗一致）
  - committed 分支（键盘选字后 Start 剩余音节）`compose_remaining` → `engine_syllable_display`（带分隔）
  - `OnCandidateClicked` 鼠标点选路径同样统一（原 `engine_compose_info`）
- `imm32_ime.cpp`：新增 `GetDisplayPinyinWide`（组词时组合窗显示剩余音节带分隔，非组词保持原样缩小改动面），
  `ImeToAsciiEx` 的 `SyncComposition` 改用
- 验证：CMake Release 两 DLL 编译通过（taishen_ime.dll + taishen_ime_imm32.ime）；commit 74672ab
- 引擎测试未重跑全量——大库测试 debug 慢为既有已知问题（见下方已知问题）

### 已知问题（沿用，未修）
- `test_ascii_mode` STEP4：Ctrl+Space 切换模式不清 pinyin_buf，STEP2 残留拼音使「英文模式不累积」断言失败——
  引擎设计（切换不丢输入）与测试预期冲突，属既有问题
- 全量测试中 dictionary 大库用例（加载 178MB .bin）debug 模式单测极慢（每例 1-2 分钟），并行跑可接受
