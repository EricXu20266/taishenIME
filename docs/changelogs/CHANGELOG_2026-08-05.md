# Changelog — 2026-08-05

## V0.3.x 候选逻辑重构 + 平台层修复（Eric 实测 10 项问题）

### 词库数据修复（问题 7 根因）
- **修复 build_dict.py tencent 词库注音 bug**：pypinyin 对 list 输入返回扁平列表，
  旧实现 `zip(chunk, py_list)` 错位 + `p[0]` 取首字符 → 22.4 万三字词（36%）注音退化为
  单字母（"一万一"→"l"），exact 匹配把它们顶到单字母候选最前（输入 l 出"一万一/一丈红"）。
  改为按词逐字切分扁平结果，每词取 n=len(word) 个元素。重建 system_dict.db（单字母拼音 224046 → 50）。
- 重建 system_dict.db.bin 预编译索引（62 万词 + 扩展索引 335MB）。

### 引擎候选逻辑（对标 rime-ice 雾凇）
- **超级简拼 zh/ch/sh 整体**（问题 8）：新增 `to_initial_full()`（zh/ch/sh 保留两字符），
  新增 `short_index_full` 完整声母索引——"社会"shehui → shh、"正在"zhengzai → zhz 可命中。
  query_short 合并 compact + full 统一按词频降序（避免"资格"顶掉"中国"）。
- **缩写+全拼混合查询**（问题 8）：新增 `query_abbrev_full()` + `suffix_index` 后缀索引——
  "xzai" = x(声母) + zai(全拼) → 现在。与 query_mixed（全拼前缀+声母后缀，shurf→输入法）互补。
- **旧 .bin 兼容**：from_bin 检测扩展索引为空时从 full_index 重建（免全量 SQLite）。
- **英文词表扩充**（问题 6）：补 welcome/please/thanks/chat/share 等常用词。
- **common_dict.txt 补充**：zhong 中/种/重/众 + l/n/m 单字母首屏字（zhong 首屏"中"排第一）。

### 平台层（C++）
- **Ctrl/Alt 组合键透传**（问题 2）：ShouldEatKey/HandleKeyDown 字母分支检查 Ctrl/Alt——
  Ctrl+C/V/R 等系统快捷键不再被吞。
- **中文标点放开候选限制**（问题 2）：标点键无论有无候选都全角化；
  有候选时先上屏默认候选再接标点（微软拼音行为，zhong + ，→ "中，"）；
  逗号/句号从翻页键移除（保留 PgUp/PgDn 和 +/-）。
- **Shift+字母输出大写**（问题 10）：中文模式 Shift+字母 → 大写上屏（有候选先上屏默认候选）。
- **Shift 切换保留拼音**（问题 9）：引擎 `set_ascii_mode` 不再 reset；shift tap 切换后
  候选窗口继续显示，空格可将已打词上屏。进程记忆恢复（AppStateApply）显式 reset 防残留。
- **候选窗口多行布局修复**（问题 1）：↓ 展开后每列等宽（max 项宽），
  CalculateSize/Draw/CandidateAt 三处一致——修复"一列半"（列宽 96px 与文本宽错位裁切）。
- **候选窗口去翻页指示**（问题 3）：删除 "1/3" 计数绘制。
- **候选窗口/工具栏去背景**（问题 4）：去掉 FillRoundedRect 背景填充，仅保留边框/按钮。
- **设置窗口独立线程**（问题 5）：工具栏设置按钮改为独立线程弹窗——
  避免在宿主进程（Notepad++）UI 线程嵌套模态消息循环导致 0xC0000005 崩溃。

### 测试
- 引擎 218 单测全过 + ffi 集成测试过。
- platform 全部 smoke 测试过（test_ascii_mode STEP7 断言更新：Ctrl+C 现在透传）。

## V0.3.x 拼写纠错（对标 rime-ice speller derive 规则）

- **新增 `spelling_variants()` 拼写纠错**（问题 11：错误纠正不智能的根因——原实现只有按键相邻容错）：
  - zh/ch/sh 声母错位：hzi→zhi、zih→zhi
  - 韵母写反：wia→wai、wie→wei、jei→jie、oa→ao、uo→ou
  - 后鼻音错位：ang→nag/agn、eng→neg/egn、ing→nig/ign、ong→nog/ogn
  - 复合韵母错位：iao→ioa/oia、ui↔iu、iang→aing/inag、ua→au、uai→aui、uan→aun、ue→eu、uang→aung/uagn/unag/augn、iong→inog/oing/iogn/oign
  - 尾韵特殊：do→dou/dong、lon→long、ten→teng、lng→lang/leng/ling/long
- **拼写纠错不受 is_full_pinyin 限制**：模式是拼音特有的，对英文单词天然安全（hello/world/welcome 实测零变体零误伤）。
- 实测：wia→外、hzi/zih→只/知、lng→狼/浪/郎、zagn→藏/脏、do→都、ten→腾、lon→龙。
- 测试：新增 8 个拼写纠错单测（226 全过）；ffi 集成测试断言更新（logn→long→龙 是正确纠正，非英文误伤）。
