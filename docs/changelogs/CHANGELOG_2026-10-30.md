# CHANGELOG 2026-10-30

## 0.1.67

### 修复：设置页保存后不生效（配置热加载失效）

- `tsf_module.cpp` `StartConfigWatch()`：`m_configPath` 与基准 mtime 改为无条件初始化。
  0.1.67 停用托盘图标后托盘窗口不再创建（`m_trayHwnd` 恒 `nullptr`），旧写法在
  提前 return 处就退出 → 路径永远为空 → `ReloadConfigIfChanged()` 恒返回 →
  保存设置后必须重启宿主应用。2s 轮询仍在有托盘窗口时启动，无窗口时由按键兜底
- `ApplyConfig()` 增加所选翻页附加键日志（`pageKeys minus_equal=/numpad=/...`）
- `imm32/imm32_ime.cpp` `ImeSelect()`：补 `SetKeyBindings()`，IMM32 进程（老应用的
  `taishen_ime_imm32.ime` 路径）此前没有配置应用点，按键设置在老应用里恒为默认值

### 测试

- `test_config_reader.cpp`：新增 `key_page_extra=minus_equal,numpad` 解析往返断言
- `test_ascii_mode.exe` STEP1–STEP10 全通过；CMake Release 全目标编译通过

### 修复：热词被同前缀旧词压住（别名/全拼合并丢精确判定）

- `dictionary/mod.rs` `add_user_entry()`：同词在同一前缀键下合并时，「完整拼音长度」
  改为取更短值（此前保留首次插入值）。同一词既有规范全拼学习（huihua→会话）又有
  简拼别名学习（huih→会话）时，别名键下长度仍是 6 → `query` 的精确层判定
  `e.3 == key_len` 失效 → 热用户词掉进前缀扩展层，被精确层里更旧的同前缀词压住。
  真实数据：会话（今天 134 次）被 72 天前的绘画（22 次）压到第 2 位；直接/之间同样

### 测试

- 新增 `engine/tests/user_word_alias_exact.rs`：先学全拼、再学别名、并放入同前缀
  竞争词，断言热词回到首位（修复前失败，修复后通过）
- 真实用户词库快照：25 个高频词首位命中 25/25（修复前 23/25）

### 已知

- 候选只有一页时翻页键无视觉反馈（候选窗已移除页码指示），不是配置未生效

## 0.1.68（未发布）

### 修复：双拼四个方案里三个键位表错位（默认方案打不出字）

- `shuangpin.rs`：微软/搜狗/自然码三张韵母表按权威键位重写——
  微软 = 搜狗 = 自然码布局微调 `ing→;`、`ü→Y`；自然码 `ing/uai→Y`、`un→P`、`in→N`、
  `ie→X`、`ia/ua→W`、`ou→B`、`ü→V`
- 此前微软按搜狗式顺手布局误置 8 个韵母（ia/ua→X、ie→P、in→B、ing→K、un→Y、
  uai→K、ü→V），按微软习惯打「家/新/花/文化」候选窗直接空白；自然码错 6 处、搜狗错 9 处
- 小鹤表原本与 RIME `double_pinyin_flypy` 逐条一致，未改
- 搜狗表与微软表合并为同一份常量（二者键位本来就完全相同）
- 补上三处反向键位遗漏（此前按标准打法打不出字）：微软/搜狗缺 `z` 键（ei → 被/北/美/飞），
  自然码缺 `z` 键（ei），小鹤 `k` 键缺 `uai`（快/块/怪/坏）
- 微软 `v` 键兼作 üe（`lv`→略、`xv`→学）；搜狗共用该表，多一个无害别名
- 微软 `y` 键不再兼 ing（ing 只在 `;`），消除 `ly` 的 ling/lv 歧义

### 修复：双拼解码与输入链路

- `process_key`：双拼模式下 `;` 作为编码键进引擎（微软/搜狗的 ing 键）
- `tsf_keyevent.cpp`：`ShouldEatKey` / `HandleKeyDown` 在双拼模式下把无 Shift 的 `;`
  送进引擎，Shift+; 仍出「：」；IMM32 走同一个 `HandleKeyDown`，自动生效
- `query_all_shuangpin` 重写：只解码成对部分（末尾单键作为下一音节声母单独顶码）；
  解码串必须能完整切分为合法音节——此前半截串（gszo→gongzo）走前缀匹配，把
  「共总/公奏议」顶到「工作」前面
- 补齐双拼缺失的后处理管线：词长匹配、上下文联想、置顶/降权、简繁去重、emoji、
  英文混输（`hello` 此前出「何亮工」，现在 hello 置顶）

### 修复：双拼学习污染用户词库

- `select_candidate` 学习键改为解码后的全拼（vs→zhong）。此前把双拼码直接写进用户
  词库（`vs→中`），切回全拼后该键仍然命中
- 新增 `Engine::shuangpin_primary_pinyin()` 供学习/诊断复用

### 改进：组合区显示全拼音节

- 新增 `Engine::preedit_string()` + FFI `engine_get_preedit_str`：双拼模式把编码还原为
  全拼（vs→zhong，末尾未完成键显示声母 zhongzh）；TSF / IMM32 组合区改用它

### 修复：工具栏「全拼/双拼」与设置页开关不联动

- `banner_window.cpp` 工具栏双拼按钮、`tsf_module.cpp` 托盘菜单切换：切换后写回
  `config.ini`（`shuangpin=`）。此前只改本进程引擎，设置页读到的仍是磁盘旧值；
  一旦设置页保存或任何配置重载触发，双拼状态会被 config 冲回
- 简繁按钮（`ToggleTraditional`）仍是临时状态、不写回 config，如需一致再改

### 文档与文案

- README 与设置页双拼表述改为「微软/搜狗/小鹤/自然码四套原生方案，键位与对应输入法一致；
  紫光、加加按微软键位兼容，键位并不完全一致」；下拉项标注「(兼容)」

### 测试

- 新增三条表结构测试：全部音节 encode→decode 往返、任意两键最多解出一个合法音节、
  正向/反向键位表一一对应——这三条抓出了上面三处漏键
- 新增 `probe_shuangpin*.rs` 四个探针：真实 37 万词库，34 常用词 × 4 方案 ×
  官方键位表 / 引擎自带表两套编码
- 小鹤首选 31/34（余 3 项 shuangpin/lv/an 全拼同样顺序，属词库排序）
- 微软标准键位：家 jw、别 bx、新 xn、快 ky、论 lp、花 hw、文化 wfhw 全部出字
  （修复前 4 项候选窗空白）
- 学习键：`user_word_count("vs")=0`、`("zhong")=1`（修复前 vs 有 1 条）
- `cargo test` 全量 336 passed；CMake Release 全目标编译通过
