# 泰深输入法 — 需求看板

> 新需求先记账再开发。每项需求标注：状态、涉及 Root、预计工时、验证标准。
> **当前活跃排期（2026-11-23，基线 `v0.1.90`）**：[V4 打薄与工程强化排期（Wave 0–Wave 5，29 项原子任务）](#v4-打薄与工程强化排期2026-11-23基线-v0190) · 统一验收与自测标准见 [AUDIT-CHECKLIST.md](AUDIT-CHECKLIST.md)。按评估方案 v3.0 / 实施计划书 v2.0，本轮取消工时估算，推进节奏由基线门禁控制；旧未启待办（`V0.7`/`V0.8`/英文输入体验）顺延至第一档 V4 打薄收口后再排。

## V0.1 MVP（第一期：能打字就行）

| # | 需求 | Root | 状态 | 工时 |
|---|------|------|------|------|
| 0.1.0 | 项目初始化（Git/CMake/Cargo/Biome） | — | ✅ 完成 | — |
| 0.1.1 | Rust 引擎骨架：Engine 状态机 + 拼音模块 + 词库模块 + FFI | #2 #3 #4 | ✅ 骨架已有（10 测试） | — |
| 0.1.2 | Windows TSF DLL 骨架（COM 注册/导出） | #3 | ✅ 完成（CLSID/类工厂/注册表注册，regsvr32 验证） | 4h |
| 0.1.3 | 系统词库构建（SQLite，203 条词库 + 构建工具） | #1 | ✅ 完成 | — |
| 0.1.4 | 词库加载通道（SQLite→引擎内存 + 内置降级） | #1 | ✅ 完成 | — |
| 0.1.5 | TSF KeyEvent 捕获与 FFI 对接 | #3 | ✅ 完成（OnKeyDown→engine_bridge→FFI，COM 实例化验证） | 6h |
| 0.1.6 | Direct2D 候选窗口渲染（一期 Windows 原生） | #8 | ✅ 完成（CCandidateWindow D2D/DWrite 渲染，冒烟测试通过） | 8h |
| 0.1.7 | 选词上屏（候选→TSF 文本提交） | #3 #8 | ✅ 完成（TSF 组合 Start/Update/Commit，DllGetClassObject 验证） | 4h |
| 0.1.8 | 基础配置系统（候选数、词库路径） | #7 | ✅ 完成（config.ini 解析 + engine_set_candidate_count + 词库路径） | 2h |
| 0.1.9 | 中英文切换 | #4 | ✅ 完成（Ctrl+Space 切换 + ascii_mode + 英文直通上屏） | 2h |
| 0.1.10 | FFI panic 守卫 + 日志 | #9 #10 | ✅ 完成（catch_unwind 守卫 + 锁中毒恢复 + 文件日志） | 3h |
| 0.1.11 | 安装器（NSIS/MSI 基础版） | #12 | ✅ 完成（install/uninstall.ps1 零依赖 + NSIS 蓝图） | 4h |
| 0.1.12 | 集成测试 + 性能基准 | #11 | ✅ 完成（FFI 集成测试完整链路 + 性能基准单键 2.37µs） | 4h |

## Bug 报告（2026-08-04 安装版实测反馈）

| # | 问题 | 根因 | 状态 | 优先级 |
|---|------|------|------|--------|
| B-10 | 设置对话框中文标签乱码 | settings.rc 编辑写回时丢失 UTF-8 BOM。rc.exe 对无 BOM 文件不认 code_page(65001)，中文按系统 GBK 解析 → 乱码（文件头注释已声明「编码：UTF-8 with BOM」） | ✅ 修复（补回 BOM + 验证 exe 资源为 UTF-8 字节序列） | P1 |

## Bug 报告（2026-08-05 实测反馈：候选逻辑 + 按键 + 渲染 10 项）

| # | 问题 | 根因 | 状态 | 优先级 |
|---|------|------|------|--------|
| B-11 | 方向键↓后候选窗口错乱（一列半/字符拉长）且无法恢复 | 多行模式 Draw 用固定 96px 列宽，CalculateSize 用实际文本宽 → 错位裁切 | ✅ 修复（三处统一等宽列布局） | P0 |
| B-12 | 中文下无法打标点；Ctrl+R/Ctrl+V 等系统快捷键失效 | 字母分支不检查 Ctrl/Alt 修饰键全部吞掉；逗号句号被当翻页键 | ✅ 修复（Ctrl/Alt 透传；标点无条件全角化，有候选先上屏默认候选） | P0 |
| B-13 | 候选 5 个后有"6"、右下角 xx/xxx 计数 | 翻页指示 "1/6" 绘制 + 窗口宽度预留 | ✅ 修复（删除翻页指示） | P2 |
| B-14 | 选词窗口和工具栏背景想去掉 | FillRoundedRect 背景填充 | ✅ 修复（去背景仅留边框/按钮） | P2 |
| B-15 | 工具栏设置点击 Notepad++ 崩溃 0xC0000005 | TSF 回调线程嵌套模态消息循环与宿主冲突 | ✅ 修复（独立线程弹设置窗口）待实测 | P0 |
| B-16 | hello/welcome 等英文词在中文模式无候选 | welcome 不在英文词表；词表偏小 | ✅ 修复（扩充词表） | P1 |
| B-17 | 单字母 l/n/m 出"一万一/一丈红"等非常用字 | **词库数据**：build_dict.py pypinyin 批量注音 zip 错位，22.4 万三字词注音退化单字母，exact 匹配顶到最前 | ✅ 修复（修注音逻辑 + 重建词库，单字母拼音 224046→50） | P0 |
| B-18 | shh/xzai/zhz 无候选 | 超级简拼 zh/ch/sh 未保留（社会→sh 非 shh）；无"声母缩写+全拼后缀"匹配 | ✅ 修复（to_initial_full + short_index_full + query_abbrev_full/suffix_index） | P0 |
| B-19 | 中文打拼音后 Shift 切英文，无法空格上词 | set_ascii_mode 切换时 reset 清空拼音 | ✅ 修复（切换保留拼音，空格可上词；进程恢复显式清空） | P0 |
| B-20 | Shift+字母无法输入大写 | 字母分支不检查 Shift，Shift+A 被当拼音 a 累积 | ✅ 修复（Shift+字母输出大写，有候选先上屏） | P1 |
| B-21 | 错误纠正不智能（只有按键相邻容错） | 雾凇有 40+ 条拼写 derive 规则（wia→wai、hzi→zhi、后鼻音错位等），我们的 correction.rs 只有 logn→long 类键位容错 | ✅ 修复（spelling_variants 全量对齐 rime derive 规则 + 不受 is_full_pinyin 限制，英文零误伤）<br>✅ 2026-11-15 二轮（实测仍不生效）：补 insertion_variants（漏打字母）+ 纠错候选豁免词长分区；nihap→你好、keyyi→可以、meiganxi→没关系 均第 1 | P1 |
| B-22 | 组词时已选字从组合区消失，重新选字后又出现；翻页选后续字看不到已选内容 | 组合串拼装散落 6 处，状态变化（翻页/退格穿透）与候选窗拼音行只写剩余音节，丢掉 m_composeAccum | 🔧 2026-11-16 修复（BuildCompositionText + SyncComposition 统一入口，退格不再穿透，候选窗显示已选前缀），待实机验证 | P1 |

**Bug 报告（2026-08-02 实测反馈）**

| # | 问题 | 根因（初判） | 状态 | 优先级 |
|---|------|--------------|------|--------|
| B-1 | 候选窗口不显示（看不到选词界面） | WS_EX_LAYERED + D2D HwndRenderTarget 渲染不兼容 → 去掉 layered + 不透明背景 | ✅ 0.1.15 修复 | P0 |
| B-2 | 退格键不能正常删除 | 双执行 bug 已修；残留问题：engine_get_pinyin_str 返回 len+1（空串=1），>0 判断永远吞退格 | ✅ 0.1.15 修复 | P0 |
| B-3 | 英文直出/无法输入（重启后） | AdviseKeyEventSink 重复注册返回 TF_E_NOLOCK (0x80040201) → 按键不达。Deactivate 未注销 + ActivateEx 未先 Unadvise | ✅ 0.1.15 修复 | P0 |
| B-4 | 候选窗口不跟随光标 | ITfThreadMgrEventSink::OnSetFocus 不触发 → m_pFocusContext null → 降级鼠标位置。ActivateEx 主动 GetFocus | ✅ 0.1.15 修复 | P0 |

## Bug 报告（2026-08-03 安装版实测反馈）

| # | 问题 | 根因 | 状态 | 优先级 |
|---|------|------|------|--------|
| B-5 | 切换输入法卡程序（秒级） | ActivateEx 每次调用 engine_init → 全量重载 62 万词条词库 + 重建前缀索引（release 实测 6-7s）。TSF 切换触发 Deactivate→Activate 每次重载 | ✅ 修复（词库/radical 路径幂等，已加载则跳过） | P0 |
| B-6 | 中文模式标点输出英文（句号等） | 平台层无全角标点映射，标点键无候选时直接透传 | ✅ 修复（MapFullWidthPunct 21 键全角映射，中文模式吞键输出） | P0 |
| B-7 | 工具栏莫名其妙消失 | 显示依赖 SetWinEventHook 前台回调，hook 失效/注册线程无消息泵时状态卡死 | ✅ 修复（OnKeyDown 兜底 EvaluateForeground + 补 WS_EX_NOACTIVATE） | P1 |
| B-8 | 托盘出现两个图标 | TSF DLL 进程内注入，每个激活泰深输入的进程各自 NIM_ADD，无 GUID 不去重 | ✅ 修复（NOTIFYICONDATAW 加固定 guidItem + NIF_GUID，Shell 合并去重） | P1 |
| B-9 | 工具栏按钮悬停光标错误（左右箭头） | 窗口类注册漏设 hCursor（候选窗口有设，工具栏漏了） | ✅ 修复（hCursor=IDC_ARROW + WM_SETCURSOR 按钮 hover 换 IDC_HAND） | P2 |

**MVP 估计总工时**：~43h（约 1 周全职）

## V0.2 第二期（完整输入体验）

| # | 需求 | Root | 状态 | 工时 |
|---|------|------|------|------|
| 0.2.1 | 双拼方案支持 | #2 | ✅ 完成（0.1.14 微软双拼方案 + 配置开关） | 8h |
| 0.2.2 | 用户词库（学习+持久化） | #1 | ✅ 完成（0.1.17 选词自动学习 + SQLite 持久化 + 插队排序 + config 接入） | 6h |
| 0.2.3 | 云输入候选 | #3 | ⚪ 暂不做 | — |
| 0.2.4 | 皮肤/主题系统 | #8 | ✅ 完成（0.1.22 四色可配 theme_bg/text/highlight/dim + HEX 解析） | 8h |
| 0.2.5 | macOS 平台适配 | #3 #8 | ⚪ 暂不做 | — |
| 0.2.6 | 模糊音支持 | #2 | ✅ 完成（0.1.14 RIME 拼写变体 + 配置开关） | 4h |
| 0.2.7 | 自动更新 | #12 | ⬜ 待开始（NSIS 安装包之后） | 6h |
| 0.2.8 | 中英混输（不切换直接输英文候选） | #2 #4 | ✅ 完成（0.1.19 候选末尾英文候选 + 选中上屏不学习） | 6h |
| 0.2.9 | emoji/符号快捷输入 | #2 | ✅ 符号完成于 0.2.17；Emoji 完成（emoji.rs + config emoji=1 开关） | — |
| 0.2.10 | 智能纠错（错键纠正，logn→龙） | #2 | ✅ 完成（0.1.18 键盘相邻键变体 + 交换/替换 + 开关） | 5h |
| 0.2.11 | 简繁转换 | #2 | ✅ 完成（0.1.20 内置简繁映射表 + 词组歧义处理 + 输出转换） | 3h |
| 0.2.12 | 快捷短语/剪贴板 | #7 | ✅ 快捷短语完成（0.1.21 简码→短语 + 外部文件）；剪贴板历史拆分待做 | 4h |
| 0.2.13 | 状态栏/托盘图标（中英状态+菜单） | #8 | ✅ 完成（0.1.23 托盘图标 + tooltip 状态 + 左键/右键菜单切换） | 5h |
| 0.2.14 | 多行候选面板（↓ 展开） | #8 | ✅ 完成（0.1.24 ↓ 展开多行网格 / ↑ 收起 + 行列点击定位） | 5h |
| 0.2.15 | 工具栏（右下角常驻：中英/简繁/双拼/设置按钮，切到泰深显示、切走隐藏、状态即时更新） | #8 | ✅ 完成（0.1.26，搜狗/QQ拼音同款，待实测） | 4h |

## V0.2 补充 — rime-ice 功能对齐（2026-08-03 竞品对比后新增）

> 对比雾凇拼音(rime-ice) 后识别出的功能缺口。按优先级分三档。
> Emoji 输入、双拼方案扩展暂不纳入。

### P0 — 日常输入体验挡路

| # | 需求 | Root | 状态 | 工时 | 说明 |
|---|------|------|------|------|------|
| 0.2.16 | 词库扩容：吸收 rime-ice ext + tencent 词库 | #1 | ✅ 完成（0.1.27 ext 全量 33.9 万 + tencent 3 字词 22.4 万，5.8 万→62 万 + 前缀索引截断优化） | 6h | 5.8 万 → 50 万+。扩展 build_dict.py 导入 ext.dict.yaml + tencent.dict.yaml，含多音字注音和大词库自动注音 |
| 0.2.17 | 符号输入 v 模式（首期：箭头/数学/单位/标点 4 类） | #2 | ✅ 完成（0.1.27 v+分类码 4 类符号候选 + 选词不学习 + 双拼排除） | 4h | rime-ice 有 60+ 分类 2000+ 符号。首期覆盖最常用的 4 类，v + 分类码触发候选。后续可逐步扩展 |

### P1 — 明显落后竞品

| # | 需求 | Root | 状态 | 工时 | 说明 |
|---|------|------|------|------|------|
| 0.2.18 | 候选窗口行内预编辑（inline_preedit） | #8 | ✅ 完成（0.1.28 inline_preedit 开关默认开，拼音写组合候选窗不重复，布局联动） | 6h | 拼音串显示在光标位置而非候选窗内。Weasel 默认开启，搜狗/QQ拼音标配。TSF 通过 ITfContext::GetTextExt 获取光标坐标，ITfInsertAtSelection 渲染 |
| 0.2.19 | 日期/时间/农历输入 | #2 | ✅ 完成（0.1.28 rq/sj/xq/nl 简码，日期 3 格式 + 农历 1900-2100 查表，sj 与短语并存日期优先） | 4h | rq→日期、sj→时间、xq→星期、nl→农历。至少 3 种日期格式 + 中文日期。农历可引用 rime-ice 的 lunar.db |
| 0.2.20 | 深色模式跟随系统 | #8 | ✅ 完成（0.1.28 注册表 AppsUseLightTheme + WM_SETTINGCHANGE 实时切换，候选窗+工具栏；用户显式配置优先） | 3h | Windows 10 1809+ 支持。检测注册表/API，自动切换候选窗+工具栏主题色 |
| 0.2.21 | 候选窗字体/字号可配 | #8 #7 | ✅ 完成（0.1.28 font_face/font_size，布局随字号缩放） | 2h | config.ini 新增 font_face/font_size，候选窗 DWrite 读取配置 |
| 0.2.22 | 计算器（cC + 算式 → 候选） | #2 | ✅ 完成（0.1.28 c+算式四则/括号/幂/取模，非法算式无候选） | 3h | 四则运算 + 常用函数（sqrt/pow/sin/cos 等）。引擎侧新增 calculator 模块 |
| 0.2.23 | 英文自动大写（Hello/HELLO） | #2 | ✅ 完成（0.1.28 大小写模式驱动候选，退格重算） | 2h | 输入首字母大写→候选首字母大写；前 2 字母大写→候选全大写。在 FFI 候选回调层做转换 |

### P2 — 差异化竞争力

| # | 需求 | Root | 状态 | 工时 | 说明 |
|---|------|------|------|------|------|
| 0.2.24 | 以词定字（[ ] 键取首尾字） | #2 #8 | ✅ 完成（0.1.29 [ 取首字 ] 取末字 + FFI + TSF 映射，取字不学习） | 3h | [ 取当前候选第一个字，] 取最后一个字。rime-ice 标配，对生僻字输入很实用 |
| 0.2.25 | 拆字反查（uU + 部件拼音 → 生僻字） | #2 | ✅ 完成（0.1.29 u+部件拼音反查，radical_pinyin 13.2 万词库 + FFI 加载） | 4h | 依赖 radical_pinyin 词库。输入 uU + 部件拼音（如 uUshuishou→𣲗），候选显示对应汉字+拼音 |
| 0.2.26 | 错音错字提示 | #2 | ✅ 完成（0.1.29 易错读音映射表 + 正确读音补词，精选 20+ 条高频易错词） | 3h | 输入错误拼音/字形时在候选 comment 提示正确读音/写法。依赖 others.dict.yaml 词库 |

**V0.2 补充估计总工时**：~40h

## 体验优化排期（2026-08-03 rime-ice 对标）

> 对标 rime-ice（雾凇拼音）切换/标点/加载机制后的排期。
> 现状差距：①切换键 Ctrl+Space（主流已统一 Shift）②标点无复选/配对 ③词库运行期建索引（首次 6-7s）。

| # | 需求 | rime-ice 做法 | 泰深现状 | 状态 | 优先级 |
|---|------|--------------|---------|------|--------|
| 0.2.26 | 中英切换改 Shift（tap 检测：快速按下松开切换，Shift+字母/符号不误切） | ascii_composer Shift_L: commit_code | Ctrl+Space（保留备选） | ✅ 完成 | P0 |
| 0.2.27 | 标点表对齐 rime half_shape：`@#%\|~` 保留半角、`_`→——、`\`→顿号、数字分隔符 `,.:` | half_shape 预设 | 全角化过度（@＃％ 也全角了） | ✅ 完成（数字分隔符降级——拼音场景无数字流） | P1 |
| 0.2.28 | 标点复选候选（`<` →《〈«‹）+ 配对引号（`'` `"` 自动开闭） | punctuator 多映射/pair | 无 | ✅ 完成 | P2 |
| 0.2.29 | 词库部署期预编译索引落盘，运行时秒加载（根治首次 6-7s） | 部署期编译 .bin，mmap 加载 | 运行期建索引（已幂等，首次仍慢） | ✅ 完成（bincode 预编译 .bin：首次 6.9s→2.7s，部署 .bin 后秒开） | P1 |
| 0.2.30 | 符号模式 v 前缀即时反馈（单 v → v 单字 + 热门符号直选，符号表全量对齐雾凇） | 搜狗按 v 候选栏立即出符号 | 单 v 只出英文候选（value/version…），无符号提示 | ✅ 完成（v 前缀 v+热门符号；符号表 27→183 分类 3585 符号全量对齐 rime-ice symbols_v.yaml） | P2 |
| 0.2.32 | v+数字分类别名（v1-v9，对标 QQ 拼音） | QQ v1-v9 固定数字分类 | 无数字分类入口 | ✅ 完成（v 前缀数字键进引擎；v1 序号/v2 数学/v3 标点/v4 箭头/v5 单位货币/v6 希腊/v7 特殊/v8 拼音注音/v9 部首笔画，动态合并已有分类） | P2 |
| 0.2.33 | 设置页「符号」速查页 | 搜狗/QQ 设置内符号指南 | 无 | ✅ 完成（设置窗第 5 页：v+数字/分类码/短码 三卡片速查，关联 docs/reference/symbol-quickref.md） | P2 |

## V0.3 第三期（AI 特色功能 — 竞品对标 + 泰深差异化）

> 竞品（搜狗/讯飞/百度）已全部上车 AI（帮写/翻译/续写/搜索）。泰深差异化定位：
> **"输入即思考"** —— 不只打字，让输入法成为 AI 入口。
> 技术底座：本地 DeepSeek API（已配置）+ 泰深已有 LLM 链路。

| # | 需求 | 竞品对标 | 泰深特色 | 工时 |
|---|------|---------|---------|------|
| 0.3.1 | AI 帮写（输入意图→续写/润色/改写） | 搜狗汪仔/讯飞 AI 键 | 深度结合拼音上下文，长句续写 | 12h |
| 0.3.2 | 智能候选重排（词频+语境+AI 热度融合排序） | 搜狗打字模型 | 融合 AI 语义打分 | 10h |
| 0.3.3 | 输入即搜索（选中拼音→AI 搜索直达） | 搜狗 AI 搜索 | 搜索结果直接上屏 | 8h |
| 0.3.4 | 翻译（中英/中日互译，回车上屏） | 搜狗/QQ 翻译 | 不跳出输入框直接翻译 | 6h |
| 0.3.5 | 剪贴板 AI 摘要/重写 | 搜狗剪贴板 | 粘贴内容一键润色 | 6h |
| 0.3.6 | 个性化人设（用户画像→风格化输出） | 搜狗 AI 帮写风格 | 泰深记忆系统驱动 | 8h |

## 技术债务

| # | 问题 | Root | 优先级 |
|---|------|------|--------|
| TD-1 | ffi.rs 中 unwrap() 跨 FFI 边界 → 改为 Result 错误码 | #10 | ✅ 0.1.10 已解决（catch_unwind + 锁中毒恢复） |
| TD-2 | 词库硬编码 → 改为 SQLite 外部加载 | #1 | ✅ 0.1.3/0.1.4 已解决 |
| TD-3 | 无日志 → 引入 tracing crate | #9 | ✅ 0.1.10 轻量文件日志（tracing 二期） |
| TD-4 | 构建工具链：本机唯一 MSVC 为 cl 19.0（ScopeCppSDK，不支持 /utf-8，与 Rust 1.97 lib 混链崩溃）。workaround：cl 14.0 编译 + rust-lld 链接 + 源文件 BOM + /GS-。**待找到 cl 14.5x 工具链后移除 workaround**（旧 DLL 为 14.51 链接器构建，系统某处应有 VS2022） | #3 | ✅ 0.1.16 已解决（定位 VS18 BuildTools MSVC 14.51.36231，移除 /GS- 与 rust-lld，CMake 统一构建 + 冒烟测试入 CMake） |

## 选词逻辑优化（2026-08-03 新增）

> 现状痛点：62 万词条词频为分档值（1369–9999，无低频），大量生僻字与常用字同频竞争，
> 排序不稳定——实测 `en` 候选首位是「奀」而非「嗯」。

| # | 需求 | Root | 状态 | 工时 | 说明 |
|---|------|------|------|------|------|
| 0.2.30 | 常用词库分层（选词优先级重排） | #1 | ✅ 完成（0.1.31 常用词表 common_dict.txt + 用户词热/温两档学习：7 天内 ≥3 次压过常用词，超窗口降温） | 4h | 新增常用词表 common_dict.txt（人工维护高频口语/书面词），加载为独立 common 层，查询排序升级为「常用词 > 用户词 > 系统词」，根治同频生僻字压住常用字 |

## 应用级配置（2026-08-04 新增 — 对标 rime-ice weasel app_options）

> 现状：P2-6 `app_ascii` 仅支持「命中进程名→强制英文」单向配置，且引擎 ascii_mode 为全局单例，
> 所有程序共享一个中英状态——切到终端是中文、切回聊天又变英文，体验割裂。
> 对标雾凇 weasel.yaml `app_options`（per-app 初始状态 + per-window 记忆）。

| # | 需求 | Root | 状态 | 工时 | 说明 |
|---|------|------|------|------|------|
| 0.2.32 | 应用级配置 app_options（升级 P2-6） | #7 #3 #4 | ✅ 完成（0.1.33 app_ascii 语义修正 + app_cn/app_inline 新增，设置对话框高级页 2 新输入框） | 3h | app_ascii 升级为 app_options 配置段：按进程名配置 ascii_mode（双向：true 默认英文 / false 默认中文）+ inline_preedit 按程序覆盖。语义从「强制锁定」改为「初始状态」，用户手动切换后不被弹回。配置示例见 [modules/app-options/SPEC.md](modules/app-options/SPEC.md) |
| 0.2.33 | per-app 状态记忆（中英状态按进程隔离） | #4 #3 | ✅ 完成（0.1.33 app_state 模块：焦点切换应用记忆，Shift/托盘/工具栏统一走 AppStateSetAscii） | 4h | 每个进程独立记忆中英状态：切到 cod.exe 自动英文、切回微信保持中文。TSF 层按前台进程维护状态表，切换前台时应用该进程记忆状态；引擎侧提供按上下文读写 ascii_mode 的 FFI（或由平台层缓存，引擎保持单例） |

## 应用级配置后续（2026-08-04 竞品调研后规划）

> 调研结论（[reference/RESEARCH_2026-08-04-app-options.md](reference/RESEARCH_2026-08-04-app-options.md)）：
> 泰深组合已覆盖主流三家之长（per-window 记忆 + 应用级配置 + declarative 配置），
> 差距在①图形化入口 ②出厂程序兼容表（搜狗 changelog 一半是程序兼容修复，泰深空表）③vim_mode。

| # | 需求 | Root | 状态 | 工时 | 说明 |
|---|------|------|------|------|------|
| 0.2.34 | 应用设置图形化管理（结构化"程序→行为"UI） | #7 #8 | ✅ 完成（V0.3.4 设置对话框现代化重构：左侧导航+右侧面板，应用级配置结构化列表，进程名+行为下拉+行内预编辑复选） | 4h | 设置对话框高级页升级：从"3 个进程名文本输入框"改为结构化列表（每行：进程名 + 行为下拉[跟随全局/默认英文/默认中文] + 行内预编辑复选），对标搜狗「应用设置」。增删改行，保存写回 app_ascii/app_cn/app_inline |
| 0.2.35 | 出厂程序兼容表（内置推荐配置） | #7 #4 | ✅ 完成（0.1.34 app_state 内置终端/编辑器默认英文表，用户配置叠加生效） | 3h | 出厂自带程序级默认配置：终端类（cmd.exe/powershell.exe/pwsh.exe/wt.exe/WindowsTerminal.exe/conhost.exe/mintty.exe）+ nvim-qt.exe → 默认英文。用户未显式配置时生效，用户配置覆盖。对标搜狗内置兼容数据库 |
| 0.2.36 | vim_mode（Esc/Ctrl+C/Ctrl+[ 切 ASCII） | #4 #3 | ✅ 完成（0.1.34 app_vim 配置 + OnTestKeyDown 透传 + OnKeyUp 切英文） | 3h | 对标雾凇 weasel app_options vim_mode（nvim-qt 场景）。app_inline 同级的 per-app 选项：命中进程时 Esc/<C-c>/<C-[> 切换 ascii_mode 状态 |

## 设置图形化（2026-08-03 新增）

> 现状痛点：工具栏「设置」按钮用 ShellExecuteW 打开 config.ini 文本文件，普通用户看不懂 key=value。
> 目标：原生 Win32 设置对话框可视化编辑全部配置项，保存写回 config.ini（热加载自动生效）。

| # | 需求 | Root | 状态 | 工时 | 说明 |
|---|------|------|------|------|------|
| 0.2.31 | 设置 UI 窗口（替代文本编辑 config.ini） | #7 #8 | ✅ 完成（settings_dialog：4 Tab 页 基础/输入/外观/高级 + 配色子对话框，SaveConfig 写回，test_settings_dialog 预览 exe） | 6h | 详见 [modules/settings-ui/SPEC.md](modules/settings-ui/SPEC.md)。工具栏设置按钮 → ShowSettingsDialog。全部 20+ 配置项可视化：开关复选、双拼方案/主题下拉、数值校验（1-20/12-32/0-40）、主题 10 色 ChooseColorW 调色、恢复默认、打开配置文件兜底 |

## V0.3 UI 全面重构 — 自研窗体系统（2026-08-04 新增）

> 目标：彻底摆脱 Win32 标准控件与 .rc 资源，自研窗体/控件系统，全部 UI 组件统一渲染引擎。
> 现状痛点：三套 UI 技术并存——候选窗（Direct2D 自绘）+ 工具栏（GDI 自绘）+ 设置对话框（Win32 对话框资源，.rc BOM 乱码坑）。
> 方案（用户确认：全量一步到位 + 现代化重设计）：Direct2D/DirectWrite 统一渲染 → 控件库 → 三组件全部迁移 → 设置界面左侧导航+右侧面板卡片式重设计，深浅主题跟随系统。
> 架构详见 [modules/ui-framework/SPEC.md](modules/ui-framework/SPEC.md)。

| # | 需求 | Root | 状态 | 工时 | 说明 |
|---|------|------|------|------|------|
| 0.3.0 | 窗体系统底座（ui_window + 控件基类 + 主题 token + 布局） | #8 | ✅ 完成（ui_theme/ui_render/ui_control/ui_layout/ui_window 五层 + test_ui_framework 冒烟） | 8h | D2D/DWrite 渲染层统一（从候选窗抽取公共层）；无边框透明置顶窗口基类（WS_POPUP+LAYERED+NOACTIVATE）封装消息分发；UITheme token（深浅两套：颜色/圆角/间距/字号，跟随系统）；简单流式布局。候选窗渲染迁移为框架首个验证者 |
| 0.3.1 | 控件库（Label/Button/CheckBox/ComboBox/Edit/Tab） | #8 | ✅ 完成（8 控件 + 弹出层命中/全局按下/滚轮机制 + test_ui_controls 六步冒烟） | 12h | 自绘控件全实现：命中检测/悬停/按下/禁用态；键盘焦点与 Tab 序；编辑框 IME 组合输入（复用 TSF 组合经验）；下拉框自绘弹出列表；标签页。颜色选择器（自绘色板替代 ChooseColorW） |
| 0.3.2 | 候选窗口迁移到新框架 | #8 | ✅ 完成（CCandidateWindow → UIWindow+CandidatePanel，接口零改动，布局算法原样搬运，31KB→16KB） | 4h | CCandidateWindow 重构为 ui_window + 控件树，视觉回归（圆角/高亮/多行/翻页指示/鼠标点击/滚轮）不变 |
| 0.3.3 | 工具栏迁移到新框架 | #8 | ✅ 完成（CBannerWindow GDI → UIWindow+ToolbarPanel D2D，单例/前台跟踪/命令保留） | 3h | CBannerWindow GDI → D2D 统一渲染，按钮悬停/按下/高亮态走控件库，主题跟随系统 |
| 0.3.4 | 设置对话框现代化重构 | #8 #7 | ✅ 完成（CSettingsWindow：左侧导航+右侧面板+自绘标题栏，20+ 配置项全保留，应用级配置结构化列表，.rc 资源移除） | 12h | .rc 资源 → 代码构建 UI 树（消灭 BOM 坑）；左侧导航 + 右侧内容面板；卡片式分组；20+ 配置项全部保留；深浅主题；自绘配色器 |
| 0.3.5 | 全组件验证 + 装机实测 | #8 | ✅ 完成（2026-08-09 装机实测通过） | 3h | 候选窗/工具栏/设置三组件回归 + 深浅主题切换 + 引擎功能不受影响（cargo test + 全测试 exe）+ 安装版真机验证 |
| 0.3.6 | 极简扁平视觉现代化（微信输入法式） | #8 | ✅ 完成（84abda1，0.1.36） | 10h | 候选窗不透明圆角卡片 + 胶囊高亮 + 拼音弱化；设置页卡片分组 + 左侧导航左边界条选中态 + checkbox 升级 toggle 开关 + 窗口 640×480。详见 [modules/ui-framework/SPEC-modern-minimal.md](modules/ui-framework/SPEC-modern-minimal.md) |

**V0.3 估计总工时**：~42h（约 1 周全职）

## V0.4.2+ 竞品对标开发（2026-08-08 调研后新增）

> 来源：五大输入法（搜狗/QQ/微信/谷歌/微软）功能对标调研，详见 [reference/RESEARCH_2026-08-08-ime-benchmark.md](reference/RESEARCH_2026-08-08-ime-benchmark.md)

| # | 需求 | Root | 状态 | 说明 |
|---|------|------|------|------|
| P0-1 | 以词定字（[ 取词首字 / ] 取词尾字） | #1 #3 | ✅ 已有（V0.2.24，调研复核确认无需开发） | take_char + FFI + 平台层按键 |
| P0-2 | 候选排序切换（0默认/1单字优先/2长词优先） | #1 #3 | ✅ 完成（543c26d） | 引擎稳定分区 + FFI + config 全链路 |
| P0-3 | 错音提示纠错库 | #1 | ✅ 已有（V0.2.26，mistake.rs） | 内置易错读音映射表 |
| P0-2-UI | 设置窗口加入候选排序下拉框 | #7 #8 | ✅ 完成（本轮，与 P1-1/P1-3 入口一起） | 输入页新增：候选排序下拉 + 上下文联想开关 + 专业词库编辑框，Load/Save 全接线 |
| P1-1 | 智能联想（前文参与候选排序） | #1 | ✅ 完成（f6178e7）；2026-10-28 加权改造 + 默认开 | last_committed + context.rs 搭配表 + context_pairs 共现次数，权重即前移位数（上限 20），内置/学习表合并取大 |
| P1-2 | 人名输入模式（;R 触发） | #1 | ❌ 不做（Eric 决策 2026-08-08） | 跳过，低使用频率 |
| P2-11 | 逐键提示（候选伴随键入实时显示） | #1 #3 | ✅ 已有（核实 2026-08-08：process_key 每键 query_all + UpdateCandidateWindow 实时刷新，天然满足） | 无需开发 |
| P1-3 | 专业词库分类（对标微软/搜狗分类词库） | #1 | ✅ 完成（本轮） | domain_index + domain_short_index，txt 每行"词 拼音"，config domain_dicts 逗号分隔多文件，resources/domains/computer.txt 示例 |

## V0.4.3 全屏/多屏场景候选窗口定位优化（2026-08-08 调研后新增）

> 来源：全屏场景处理调研（泰深 vs Weasel vs 微软拼音），详见记忆库《输入法全屏场景处理调研》
> 核心结论：独占全屏候选窗不可见是平台硬限制（微软官方确认），输入法可控的是——多显示器定位、DPI 坐标对齐、定位兜底分级

| # | 需求 | Root | 状态 | 说明 |
|---|------|------|------|------|
| P0-A | 多显示器定位修正 | #8 | ✅ 完成（candidate_window.cpp MonitorFromPoint + MONITOR_DEFAULTTONEAREST，副屏 clamp） | PositionWindow 边界检查用 SM_CXSCREEN（主屏）→ 改 MonitorFromPoint + GetMonitorInfo(rcWork)，副屏 clamp 正确 |
| P1-B | 坐标单位对齐（DPI unaware 宿主） | #3 #8 | ✅ 完成（dpi_util.cpp V0.4.3-P1B：GetWindowDpiAwarenessContext 检测宿主感知，unaware 按 GetDpiForSystem()/96 换算） | GetTextExt 返回坐标单位取决于宿主 DPI 感知：DPI-unaware 应用返回 96-DPI 逻辑像素，与 GetCursorPos 物理像素混用导致缩放≠100% 时偏移。检测宿主感知模式 + 换算 |
| P2-C | 定位兜底分级（含全屏居中条） | #8 | ⬜ 远期 | GetTextExt→GetCursorPos→屏幕底部居中全屏条（对标 weasel FullScreenLayout，自适应字号）。本期不实施，SPEC 记录 |

## V0.4.4 纠错增强 — 多打字母容错（2026-08-08）

> 来源：用户实测 weom→wom（我们），多打一个字母时无法通过现有纠正（按键相邻替换/交换 + 拼写 derive 规则）覆盖
> 方案：deletion_variants 生成删除一个字符的拼音变体，候选不足时触发，不受 is_full_pinyin 限制（不完整拼音也可能多打字母）

| # | 需求 | Root | 状态 | 说明 |
|---|------|------|------|------|
| 0.4.4 | deletion_variants 多打字母容错 | #2 | ✅ 完成（ee6ec55） | correction.rs 新增 deletion_variants 函数，lib.rs query 在候选不足时触发。验证：250/250 测试通过 |

## V0.4.5 纠错增强 — 词库锚定拆分组词（2026-08-08）

> 来源：用户实测反馈 weom→wom 后提出——**不能一刀切按音节分词，应以词库词组为锚**。
> 用户输出非单字时，目标必是词库中已有的词组（单词/短句/成语/谚语），应拆分组词匹配词库词组。
> 方案：递归把输入串切成若干段，每段必须命中词库真实词组（full_index 完整拼音匹配，允许段内 1 个错误：多打/打错/换序/漏打），组合输出。对标搜狗逐音节智能切分，但锚点是词库词组而非音节。

| # | 需求 | Root | 状态 | 说明 |
|---|------|------|------|------|
| 0.4.5 | 词库锚定拆分组词（phrase_group_guess） | #2 | ✅ 完成（V0.5.4：phrase_group_guess 词库锚定拆分，候选不足一页触发；删拼接怪词 phrase_guess） | dictionary 新增 phrase_group_guess：递归切段→每段查 full_index 命中词组（容错变体：deletion/correction/spelling）→组合。lib.rs 长串（>6 字符）候选不足时触发。防英文污染：段须命中词库词组 |

## V0.5 语音输入（2026-08-09 新增）

> **双路径策略**：输入法独立管理 Whisper 引擎与模型，不依赖泰深。
> - **优先路径**：检测到泰深已安装且 whisper-server 在运行 → 直连复用（零额外资源占用）
> - **自管路径**：未检测到泰深 → 输入法自行下载引擎 + 模型 + 启动 whisper-server（完全自给）
>
> 参考实现：泰深 `electron/ipc/whisper.ts`（模型/CLI 下载+校验）、`electron/services/whisper-server-manager.ts`（server 生命周期）、
> `src/renderer/components/chat/VoiceInputButton.tsx`（VAD+录音+转写管道）、`src/renderer/components/SettingsPage.tsx`（设置 UI 布局）。
>
> 输入法端需实现：WASAPI 音频采集 → VAD 分段 → WAV 编码 → HTTP POST whisper-server → 候选注入。
> 架构详见 [modules/voice-input/SPEC.md](modules/voice-input/SPEC.md)。

| # | 需求 | Root | 状态 | 工时 | 说明 |
|---|------|------|------|------|------|
| 0.5.0 | 语音输入 SPEC 文档 | #2 #3 #7 #8 | ✅ 完成（2026-08-10 入库 docs/modules/voice-input/SPEC.md，双路径架构 + 配置项 + 存储布局） | 1h | 完整需求规格 + 接口设计 + 双路径数据流。写入 docs/modules/voice-input/SPEC.md |
| 0.5.1 | 音频采集模块（WASAPI 16kHz PCM） | #3 | ✅ 完成（2026-08-17 audio_capture.cpp：共享模式 16kHz mono 16bit，MMCSS 线程，静音帧处理） | 8h | C++ 平台层 AudioCapture：WASAPI IAudioClient → IAudioCaptureClient，f32 归一化回调，Stop 可靠释放 |
| 0.5.2 | VAD 语音活动检测（Rust engine 侧） | #2 | ✅ 完成（2026-08-17 voice.rs：RMS 滑窗 + 状态机，8 项单测全过） | 4h | engine/voice.rs：滑窗 RMS 能量检测，可配阈值/最短语音/静音超时。逻辑对齐泰深 TS 版 VoiceActivityDetector |
| 0.5.3 | 泰深检测模块（优先路径） | #3 | ✅ 完成（2026-08-17 engine_detect_taishen + VoiceManager::DetectTaishen） | 2h | 文件检查 ~/.taishen/bin/whisper-server.exe + TCP 端口探测（500ms 超时）→ 直连/未启动/不可用三态 |
| 0.5.4 | Whisper 转写对接（HTTP → whisper-server） | #2 #3 | ✅ 完成（2026-08-17 voice.rs transcribe：reqwest blocking multipart，120s 超时，语言参数强制传） | 3h | engine_voice_transcribe：WAV → POST /inference → {text}。始终传 language=zh 防中文被当英文翻译（SPEC 10.9） |
| 0.5.5 | Whisper 引擎 + 模型下载管理（自管路径） | #3 #7 #12 | ⬜ 待开始（预留扩展点，VoiceManager 自管路径先直连需手动启动） | 8h | 下载 whisper-server + ggml 模型到 %LOCALAPPDATA%\TaishenIME\whisper\。CDN + 校验参照泰深 whisper.ts |
| 0.5.6 | whisper-server 生命周期管理（自管路径） | #3 | ⬜ 待开始（预留扩展点） | 4h | 启动/停止/健康检查/自动重启。参照泰深 whisper-server-manager.ts |
| 0.5.7 | 语音候选注入（Rust FFI + 候选窗口展示） | #2 #3 #8 | ✅ 完成（2026-08-17 engine_voice_result + set_voice_candidate + UpdateCandidateWindow pinyin 空条件放宽） | 6h | engine_voice_result 注入语音文本为唯一候选；候选窗 pinyin 空但候选非空时显示（对齐 0.2.28 复选标点）；用户按键链自动上屏 |
| 0.5.8 | 工具栏语音按钮 | #8 | ✅ 完成（2026-08-17 banner_window 4→5 按钮，麦克风按钮激活态高亮） | 2h | 工具栏新增语音按钮（中/英·简繁·双拼·麦克风·设置），点击 Start/Stop VoiceManager，录音中高亮 |
| 0.5.9 | 设置页「语音」标签 | #7 #8 | ✅ 完成（2026-08-17 settings_window 5→6 导航页） | 6h | 设置窗第 6 页「语音」：总开关 + 引擎状态（泰深直连/独立运行）+ 模型大小 + 引擎风格 + 识别语言 + VAD 参数。config.ini voice_* 键读写 |
| 0.5.10 | 端到端集成测试 + 装机实测 | #11 #12 | ⬜ 待开始（test_voice 冒烟 5 项过；真机麦克风实测待 Eric） | 3h | test_voice：WAV 编码/VAD 状态机/泰深检测/候选注入/config 往返全过。真机四种场景验证待做 |

**V0.5 估计总工时**：~47h

### 双路径架构

```
启动语音输入
    │
    ├─ 检测 ~/.taishen/bin/whisper-server.exe 存在
    │   └─ 是 → 检测 127.0.0.1:9080 连通
    │          ├─ 通 → 【优先路径】直连泰深 whisper-server（零资源占用）
    │          └─ 不通 → 可能泰深未启动 → 尝试自启 whisper-server 或走自管路径
    │
    └─ 否 → 【自管路径】
             ├─ %LOCALAPPDATA%\TaishenIME\whisper\ 检查引擎+模型
             ├─ 缺 → 弹窗引导下载（CDN 源）
             ├─ 有 → 启动自带 whisper-server
             └─ HTTP POST 127.0.0.1:{port}/inference
```

### 自管路径存储布局

```
%LOCALAPPDATA%\TaishenIME\whisper\
├── bin\
│   ├── whisper-server.exe         # 常驻 HTTP 推理服务
│   └── ... (ggml.dll, SDL2.dll 等依赖)
├── models\
│   ├── ggml-tiny.bin              # ~78MB
│   ├── ggml-base.bin              # ~148MB
│   ├── ggml-small.bin             # ~488MB
│   ├── ggml-medium.bin            # ~1.5GB
│   └── ggml-large-v3-turbo.bin    # ~1.5GB（推荐）
└── downloads\                     # 临时下载文件
```

### 配置项新增（config.ini）

```ini
[voice]
enabled = false                   # 语音输入总开关
engine = whisper                  # 引擎：whisper（后续可扩展）
model_size = large-v3-turbo      # 模型大小
server_port = 9080                # whisper-server 端口
language = zh                     # 识别语言
vad_threshold = 0.02              # VAD 能量阈值
vad_silence_timeout_sec = 1.8    # 静音超时（秒）
vad_min_speech_sec = 0.8         # 最短语音时长（秒）
engine_flavor = cpu               # 引擎风格：cpu / cuda / blas
taishen_detected = false          # 运行时检测结果（只读，不写回）
```

## V0.6 IMM32 兼容层（2026-08-09 新增 — 老游戏/老应用适配）

> 来源：LOL 输入法根因实锤（2026-08-09 调研）——LOL 聊天框用 Scaleform GFxIME（IMM32 协议）+ `Game\DATA\Menu\IMEConfig.xml` 输入法白名单，泰深纯 TSF 不在白名单 → LOL 从不激活（实测进程未加载 DLL）。微软拼音被官方适配、搜狗/QQ 有 IMM32 身份+被收录所以能用。
> 对照：小狼毫 weasel 双实现（TSF weasel.dll + IMM32 weasel.ime），但 Win10 默认不加载 weasel.ime 需 ImmInstallIME 手动注册；weasel 官方承认 LOL 是特例（issue #305）。
> 决策：Eric 选定 L2 方案——IMM32 兼容层（2026-08-09）。
> 架构详见 [modules/imm32-layer/SPEC.md](modules/imm32-layer/SPEC.md)。

| # | 需求 | Root | 状态 | 工时 | 说明 |
|---|------|------|------|------|------|
| 0.6.1 | IMM32 DLL 骨架 + 注册（Keyboard Layouts KLID + ImeInquire 能力声明） | #3 | ✅ 完成（taishen_ime_imm32.ime，23 导出，E0C00804 注册 + 冲突检测） | 4h | 独立 DLL taishen_ime_imm32.ime（仿 weasel.ime），复用 Rust engine staticlib + engine_bridge。KLID E0C00804（厂商自定义区间），DllRegisterServer 含冲突检测 |
| 0.6.2 | IMM32 输入链（ImeProcessKey→引擎→组合串 WM_IME_COMPOSITION + ImeConversionList 候选 + ImeToAsciiEx 上屏） | #3 | ✅ 完成（复用 tsf_keyevent HandleKeyDown，行为与 TSF 一致） | 8h | 与 TSF 共享引擎状态（ascii/候选/词库）。验证：test_imm32_load 全通过（组合/候选/上屏/END） |
| 0.6.3 | IMM32 候选窗（复用 UIWindow + CandidatePanel 自绘） | #8 | ✅ 完成（复用 CCandidateWindow + UI 框架源码） | 4h | 非白名单应用自绘候选；LOL 白名单命中走 GFxIME 游戏内渲染 |
| 0.6.4 | 集成验证（LOL IMEConfig.xml 注册 + 32 位 IME DLL 构建 + 老游戏回归） | #3 #11 | ⏳ 待实测（DLL 构建 ✅，LOL 真机验证待 Eric） | 4h | test_imm32_load 全通过；LOL 实测需用户配合（IMEConfig.xml 注册泰深条目） |
| 0.6.5 | 安装器集成（KLID 注册 + 卸载清理 + 32/64 双包） | #12 | ✅ 完成（install/uninstall.ps1 集成，regsvr32 注册 + 验证） | 2h | install.ps1 复制 .ime + 提权注册 E0C00804 + 验证；uninstall.ps1 注销 |

**V0.6 估计总工时**：~22h（实际 ~14h）

## V0.6.9 TSF 候选窗兜底 —— 魔兽世界（2026-11-13）

> 来源：Eric 实测——`wow.exe` 聊天框里打不出可见候选。日志实证：`EnsureCandidateUiElement: begun id=0 show=0`，
> 随后 `UpdateCandidateWindow` 被 `m_candidateUiSuppressed` 提前 return，自绘候选窗也被 Hide；而 `CommitComposition` 成功。
> 即「引擎在跑、字能上屏，但屏幕上什么都不显示」。
> 对照：青简（`E:\AllinDeepSeek\qingjian`）根本没有 `ITfUIElementMgr` 实现，候选窗始终由独立
> `qingjian-server.exe` 自绘（`WS_EX_LAYERED|TOOLWINDOW|TOPMOST|NOACTIVATE`），不把 UI 生死交给宿主。

| # | 需求 | Root | 状态 | 说明 |
|---|------|------|------|------|
| 0.6.9a | 宿主 `show=FALSE` 时区分「宿主自绘」与「宿主不管」 | #8 | ✅ 完成 | 新增 `m_uiElementDataRead`：`GetCount/GetSelection/GetString/GetPageIndex/GetCurrentPage` 被调用即置位；`BeginUIElement` 前清零。宿主没读数据 → 我们自绘兜底 |
| 0.6.9b | 按进程显式覆盖 | #7 | ✅ 完成 | `app_self_draw` / `app_no_self_draw`（逗号分隔进程名），优先级高于自动判定；暂只支持 config.ini 手改 |
| 0.6.9c | 魔兽世界真机验证 | #8 #11 | ⏳ 待实测 | 需替换安装目录 DLL + 重启 explorer/ctfmon；判定：游戏聊天框输入 `nihao` 应出现自绘候选窗，日志出现 `self-draw fallback` |

**边界**：本次不触碰 IMM32 兼容层（`E0C00804` 注册、`enable-imm32.ps1`、`install.nsi` 历史清理段）——
问题在 TSF 通道，且该段有登录循环历史事故。

**已知限制**：独占全屏下任何候选窗都不可见（平台硬限制）；游戏内 `inline_preedit=1` 易丢字（weasel #983 实测），建议关。

## V0.6.10 长句解码与组词/渲染结构修正（2026-11-15 ~ 11-16）

> 来源：Eric 实测长句候选错位、组词选不动、切输入法卡顿。
> 两天内 6 个结构性改动：组词候选重写、整句解码、组合串单一拼装、bigram 主排序、
> 渲染层分段属性、候选 UIElement 归属判定。

| # | 需求 | Root | 状态 | 说明 |
|---|------|------|------|------|
| 0.6.10a | 组词候选重写（从长到短 + 剩余拼音整词） | #2 | ✅ 完成 | 按「本次能消费几个音节」从多到少：剩余整段整词 → 逐级缩短前缀 → 当前音节单字；`phrase_group_guess` 退出组词模式；收敛到 3 页 |
| 0.6.10j | 组词首位改当前音节单字 | #2 | ✅ 完成 | 0.6.10a 把整词放首位，用户按 1 误选整词 → `compose_idx` 到头、组词结束、拼音清空，后续数字透传宿主（出现「12」）。现首位固定单字（逐字推进），整词保留第 2 位起 |
| 0.6.10b | 组词整句层 | #2 | ✅ 完成 | `sentence_guess` 把剩余音节整体转换，前文已选词参与 bigram；零错 + 常用词过半两道门槛；只取 1 条 |
| 0.6.10c | 组合串单一拼装入口 | #8 | ✅ 完成 | TSF `BuildCompositionText` + `SyncComposition`；已选前缀归属引擎（`compose_prefix`），消除双份状态 |
| 0.6.10d | IMM32 组词对齐 TSF | #3 #8 | ✅ 完成 | 组合串 = 已选前缀 + 剩余音节；中间音节不落文档、最后音节整词提交；退格先回退已选词 |
| 0.6.10e | 有状态 FFI 单次读取 | #3 | ✅ 完成 | `ReadEngineActionUtf8` 固定缓冲单次调用；select/undo/take_char 不再被读两遍（读两遍 = 选两次音节） |
| 0.6.10f | 组合串分段显示属性 | #8 | ✅ 完成 | 已选汉字无下划线、剩余拼音虚线下划线；按宽字符位置切分 |
| 0.6.10g | 纠错插入变体 + 词长豁免 | #2 | ✅ 完成 | 漏打字母容错（`nihap→你好`）；纠错来源的常用词不参与词长分组 |
| 0.6.10h | bigram 档位表接入主排序 | #2 | ✅ 完成 | 与学习表/先验/内置表四源取 max；修复「装了 bigram 主排序无感」 |
| 0.6.10i | 长句整句解码 | #2 | ✅ 完成 | `sentence.rs` 词图 Viterbi + 束搜索，15 例首位命中 12；`Source::Sentence` 不学习、不写上下文 |
| 0.6.10j | 整句审计修复 | #2 | ✅ 完成 | 换库/删词清格子缓存；同文本保留原来源；容错每段只算一次；切分深度前置截断 |
| 0.6.10k | bigram 加载幂等 + 首键延迟 + `.bin` 缓存 | #11 | ✅ 完成 | 175 次重复加载（累计 284s）→ 同进程幂等；2760ms → 103ms |
| 0.6.10l | 候选 UIElement 全局语义 + 拼音行同源 | #8 | ✅ 完成 | 无条件注册；按「宿主是否读过候选数据」决定自绘兜底；拼音行与组合串同源 |
| 0.6.10m | 单字候选不再被常用词表打乱 | #2 | ✅ 完成 | `shua → 刷` 回到首位；单字保持词库词频序 |
| 0.6.10n | 打包链补漏 | #12 | ✅ 完成 | NSIS/PS1 补 `bigram.db`、`.bin` 缓存失效、32 位 IMM32 |
| 0.6.10o | 组词/长句实机验证 | #8 #11 | ⏳ 待实测 | 组词翻页、退格、选后续字；长句整句上屏 |

**文档同步**：ARCHITECT v3.1（24 模块 / 81 FFI / 呈现层结构）、business-flow v3（27 阶段装配）、
composition / presentation / context-assoc 三份 SPEC 修订，新增 `modules/sentence-decode/SPEC.md`。

## Bug 报告（2026-08-09 实测反馈：候选排序 3 项）

> 根因全部定位（真实词库实测复现），待 Eric 确认修复方案后动手。

| # | 问题 | 根因（已实锤） | 状态 | 优先级 |
|---|------|--------------|------|--------|
| B-22 | 简体模式下繁体字被候选（women 出「我們的出口」、ceshi 出「側視」） | **词库数据混入繁体**：system_dict.db 38.1 万词中 16,858 条含繁体独有字，domains.db 16.9 万词中 19,955 条（维基繁体词条直接入库），另含 GBK 乱码残留（紝鐨剉 类 mojibake）。查询层无繁→简归一化，trad.rs 只做输出层简→繁（繁体模式）。 | ✅ 修复（V0.5.4 加载层简繁归一化 + 去重；V0.5.5 分层 pick 固化） | P1 |
| B-23 | 过度联想：women 第 1 位是「我们是冠军」而非「我们」；womenceshi 全出「我们测是」拼接怪词 | ① `phrase_guess`（多音节切分联想）在 candidates 为空时**无条件触发**，把音节 top 单字笛卡尔积拼接（wo→我、men→们、ce→测、shi→是 → 我们测是），而 combo_guess/phrase_group_guess 有 `!is_full_pinyin` 条件——条件不一致是 bug；② 领域词热度前置（sport 领域「我们是冠军」womenshiguanjun 前缀命中）+ `apply_long_word_filter` 把 3-4 字短语提前，压过 2 字双字词「我们」；③ 词频排序不约束「词长 = 输入音节数」。 | ✅ 修复（V0.5.4 删 phrase_guess + 词长匹配；V0.5.5 分层——P3「我们」> P4「我们是冠军」） | P1 |
| B-24 | 常用字词优先级异常（短句/领域词抢常用词位） | 与 B-23 同根：domain 前缀扩展 + domain_boost + long_word_filter 三级叠加，把常用双字词挤下前 5 位。`wo` 正常（我/握/窝/卧/我国），`women`/`ceshi`/`xihuan` 异常。 | ✅ 修复（V0.5.5 重构为 P1→P4 分层 pick 根治：领域词永远在系统词后，高频专名进 P2 common） | P1 |

## V0.5.7 候选右键管理 — 置顶/降权/删除 + 简拼自动展开（2026-08-10）

> Eric 决策：砍掉雾凇「降频到第 4 位」中间档。保留词级置顶（简拼自动展开）+ 降权（压出前 2 屏）+ 删除（彻底移除用户词）。

| # | 需求 | Root | 状态 | 工时 | 说明 |
|---|------|------|------|------|------|
| 0.5.7a | 候选窗右键菜单：置顶/取消置顶、降权/恢复、删除该词 | #2 #8 | ✅ 完成 | 4h | TSF + IMM32 双接线。UIWindow WM_RBUTTONDOWN → UIControl::OnRightClick → CandidatePanel 命中 → TrackPopupMenu 原生菜单。引擎 pin_words + demoted_words 词集合同步持久化到 user_dict.db |
| 0.5.7b | 置顶词级简拼自动展开（pin_map 编码级 → pin_words 词级） | #2 | ✅ 完成 | 2h | 置顶「你好」后 nihao/nih/nh 任意输入方式候选里出现即置顶。apply_pin_boost 收尾阶段把 pin_words 集合里的词提到候选最前，不依赖编码反查 |
| 0.5.7c | 删除按词全量清理（user_index + user_short_index + 磁盘三索引同步） | #1 | ✅ 完成 | 1h | 修复原 engine_delete_candidate 只按当前拼音串删 user_index 的 bug（简拼场景删不干净、user_short_index 残留）；改为全前缀遍历清理 + 磁盘按词 DELETE 所有行 |
| 0.5.7d | 降权：命中词压出前 2 屏（第 11 位之后） | #2 | ✅ 完成 | 1h | apply_demote 收尾阶段把 demoted_words 的词移到 rest 之后（候选充足时自然压到 >10 位，不足时排末尾）。可恢复（undemote） |

### 涉及文件

| 层 | 文件 | 改动 |
|----|------|------|
| 引擎 | `engine/src/lib.rs` | pin_map→pin_words+demoted_words HashSet；apply_pin_boost + apply_demote 两段收尾；pin/unpin/demote/undemote/delete 方法；13 个新测试 |
| 引擎 | `engine/src/ffi.rs` | 6 个新导出（pin_word/unpin_word/demote_word/undemote_word/is_pinned/is_demoted） |
| 引擎 | `engine/src/dictionary/mod.rs` | user_dict.db 新增 pin_words/demoted_words 两张表 + 加载/保存/删除函数；remove_user_entry 按词全量清理 user_index + user_short_index |
| 平台 | `platform/windows/include/ui_control.h` | UIControl 新增 OnRightClick 虚方法 |
| 平台 | `platform/windows/include/ui_window.h` | —（HandleMessage 已处理 WM_RBUTTONDOWN） |
| 平台 | `platform/windows/include/candidate_window.h` | CCandidateWindow 新增 RightClickCallback + SetRightClickCallback |
| 平台 | `platform/windows/include/engine_bridge.h` | 6 个新 FFI 声明 |
| 平台 | `platform/windows/src/ui_window.cpp` | HandleMessage 增加 WM_RBUTTONDOWN → DispatchMouse 右键分发 |
| 平台 | `platform/windows/src/candidate_window.cpp` | CandidatePanel::OnRightClick + 回调 |
| 平台 | `platform/windows/src/tsf_module.cpp` | CTextService::OnCandidateRightClicked → TrackPopupMenu（3 项 + 分隔线）→ FFI 调用 → RefreshState |
| 平台 | `platform/windows/src/imm32/imm32_ime.cpp` | IMM32 候选窗同步注册右键回调（TSF 同款菜单） |
| 文档 | `docs/modules/candidate-rightclick/SPEC.md` | 完整 SPEC（需求 + 方案 + 实施计划） |
| 文档 | `docs/reference/RESEARCH_2026-08-10-rime-ice-ranking-compare.md` | rime-ice 排序机制竞品对比 |

## V0.7 输入足迹与自我复盘（2026-08-17 新增 — 取代原 V0.3 AI 特色功能；2026-11-16 修订）

> **方向调整（Eric 决策 2026-08-17）**：不做「输入中 AI」（原 0.3.1~0.3.6 全部作废：AI 帮写/智能候选重排/输入即搜索/翻译/剪贴板 AI 摘要/个性化人设）。
> 改为**事后总结**：输入法记录全部上屏文本，用户主动或定时发起 AI 总结——
> ① 输入内容提炼（时间段内）② 词库更新总结（输入内容 vs 词库，未录入词/短句 → 建议录入用户组词或热词提权）。
>
> **分工**：AI 总结绑定泰深（启动泰深后发任务），输入法侧负责「记录 + 词库对比 + 写入通道」。
> 泰深侧需求清单见 [modules/input-history/SPEC.md](modules/input-history/SPEC.md) 第六节，在 taishen-pisdk 项目实现。

> **⚠️ 2026-11-16 技术修订**（数据契约见 [SPEC.md](modules/input-history/SPEC.md)；详细设计稿本地保留，暂不入库）
>
> **数据架构**：`retention_days 30` → `raw_retention_days 90` + L1/L2 分层 + 按月分库。
> 原因：30 天保留与年度报告（需 365 天）直接冲突。
>
> **AI 位置**：只在事后，输入阶段零介入。技术原因：实时检查过不了 30ms 按键预算；候选窗空间放不下多行结果；TSF 无法给应用的系统右键菜单加项。
>
> **隐私升级为 P0**：输入法进程无网络（可被防火墙验证）、密码框零记录（写入前丢弃）、内容正则过滤、应用白名单。
>
> **⚠️ 价值未验证**：第 0 步先做 CLI，用真实输入跑 3 天，确认复盘输出有实际参考价值后再投入后续开发。

| # | 需求 | Root | 状态 | 工时 | 说明 |
|---|------|------|------|------|------|
| 0.7.0 | **价值验证 CLI**（先做这个） | #3 | ⬜ 待开始 | 6h | `input_journal.exe --date` → 当天 Markdown + prompt 模板。不做 UI/隐私/分层。**判据：真实输入跑 3 天，确认复盘输出有实际参考价值** |
| 0.7.1 | 输入历史记录（按月分库 + 分层 compaction） | #3 #1 | ⬜ 待开始 | 14h | `input_history_YYYY_MM.db` 按月分库（删旧=删文件）；跨天 compaction 压出 L1 日聚合 + L2 段落归档；字段 ts/app/type/text/pinyin/source |
| 0.7.1b | 隐私三闸（P0，与 0.7.1 同期） | #3 | ⬜ 待开始 | 12h | ① 输入法进程无网络（静态检查）② 密码框零记录（TSF input scope + UIA + ES_PASSWORD 三保险，pinyin 一并过滤）③ 内容过滤（token/私钥/身份证/银行卡）+ 应用白名单 |
| 0.7.2 | 历史记录读取接口（泰深侧查询） | #1 | ⬜ 待开始 | 4h | SQLite 直接读 + 文档约定表结构；类型过滤 |
| 0.7.3 | 词库对比分析 + 词库整理 | #1 #2 | ⬜ 待开始 | 10h | `input_analyze.exe`：n-gram 切分 → 过滤 → 四层比对 → JSON。任务③：user_dict 误学词识别 |
| 0.7.4 | 推荐词写入通道 | #1 | ⬜ 待开始 | 6h | `pending_import` 表 + `ime_import --file`；learn 自动 / pin 人工确认 |
| 0.7.4b | 词库溯源（白箱 L3） | #1 | ⬜ 待开始 | 4h | user_dict 加 source_app / first_seen_ts / learn_reason / evidence；设置页可查「这个词哪来的」 |
| 0.7.5 | 配置项 + 设置页入口 + 实测占用预估 | #7 #8 | ⬜ 待开始 | 7h | config [history]：raw_retention_days=90 / archive_retention_days=365 / daily_compact_hour；设置页显示按实际 7 天平均算的预估占用 |
| 0.7.6 | 端到端验证 | #11 #12 | ⬜ 待开始 | 4h | 记录 → compaction → analyze → AI 复盘 → 推荐词写回 → 导入生效，全链路真机 |

**V0.7 估计总工时**：~67h（输入法侧，原 36h 已含分层/隐私/溯源增量）

## V0.8 用户词库膨胀抑制（2026-08-17 新增 — 延伸自 V0.7 复盘）

> **背景**：user_dict.db 等记录用户输入的词库**不应无限增长**——选词学习使词条数与词频持续累积，
> 当前 2288 条，长期可到数万条，影响加载速度与排序稳定性。
> AI 复盘任务③（噪音删除）是**被动**抑制器（用户/AI 驱动），本需求做**主动**治理。
> 对标：搜狗/微软拼音「越用越准、不用就降」的词频衰减机制。

| # | 需求 | Root | 状态 | 工时 | 说明 |
|---|------|------|------|------|------|
| 0.8.1 | 词频衰减（time-decay） | #1 #2 | ⬜ 待开始 | 6h | 老词条按时间降权：N 天未使用词频逐步衰减（如每 30 天 ×0.5），衰减到阈值以下进入淘汰候选。user_dict 原「不做」清单的"词频衰减"转正 |
| 0.8.2 | 词库容量上限 max_entries | #1 | ⬜ 待开始 | 4h | 词条数超阈值（默认可配，如 10000）淘汰最不常用（frequency 低 + last_used 久），保留高频常用 |
| 0.8.3 | 定期整理（启动/每日惰性检查） | #1 #3 | ⬜ 待开始 | 3h | 启动或每日首次激活执行清理；淘汰候选集提示用户确认（可恢复，避免误删）；与 AI 复盘任务③联动 |

**V0.8 估计总工时**：~13h

## V0.9 索引加载性能（2026-10-27 新增 — 对标主流输入法后的性能改进项）

> **背景**：release 实测引擎 init → 词库就绪约 1.2s，其中 bincode 反序列化占约 83%。
> 两个缓存文件合计 327MB（系统 170MB + 领域 165MB），切换输入法后立刻打字，
> 就绪前只有内置 33 条常用词可用。设计见 [modules/index-perf/SPEC.md](modules/index-perf/SPEC.md)。
>
> 历史误读已纠正：此前记录的"5.8 秒"是 debug 构建测试进程的数字，不是产品实际表现。

| # | 需求 | Root | 状态 | 工时 | 说明 |
|---|------|------|------|------|------|
| 0.9.1 | .bin 格式版本头 + 旧缓存自动重建 | #1 #11 | ✅ 完成 | 2h | v2 头（magic/version/分片数）；v1 判失效回退 SQLite 并重写，不阻塞启动 |
| 0.9.2 | ShardedMap 分片容器包装 index | #1 | ✅ 完成 | 4h | 按 key 哈希分片，get/len/values_mut/push/iter_mut；查询语义不变 |
| 0.9.3 | build_index 写分片段 + from_bin 并行加载 | #1 #11 | ✅ 完成 | 4h | 系统 1000ms → 588ms（read 67 + meta 404 + shards 98） |
| 0.9.4 | 领域 .bin 同步分片 | #1 | ✅ 完成 | 3h | domain_index 112MB 分片，813ms → 385ms |
| 0.9.5 | 回归验证 | #11 | ✅ 完成 | 1h | cargo test 319 项全绿 + select_protocol 协议测试 + bench 前后对比 |

**实测结果（release，本机）**：引擎 init → 两大词库就绪并合并 1200ms → 700ms。
剩余瓶颈是 meta（系统 404ms / 领域 218ms，除主索引外的次级索引单线程反序列化），
后续若需 ≤400ms 可把 short_index / suffix_index / trad_index 一并分片。

**V0.9 估计总工时**：~21h

---

## 待办 — 英文输入体验（2026-11-19 记录，待排期）

**设计意图**：中文输入状态下直接敲英文单词，输入法应响应——允许英文上屏，并且做联想（自动补全 + 拼写纠错），用户不必把单词敲全。

**当前问题**：最近几个版本实测发现，英文单词的上屏**数量与优先级都太高**。

**怀疑方向**：自动补全与纠错两个来源产生的英文候选过多，挤占了候选窗的正常位置。需要在候选装配阶段区分「英文原串」「英文补全」「英文纠错」三类的数量与位次，而不是笼统按一个 English 来源处理。

**关联已有实现**：
- 0.2.8 中英混输：候选末尾追加英文候选（`mix_mode`）
- 0.2.23 英文自动大写（候选首字母大小写跟随输入）
- 英文候选区边界：`english_candidate_pos` 之后的区段不参与中文分区排序
- 英文词表：`resources/english_words.txt`

**状态**：mark，等当前词库校正与候选质量收尾后处理。

**验收建议**：用文章模式考试（`article_exam`）的 `mixed` / `typo` 段量化——英文候选占比、目标英文词位次、英文候选挤掉中文候选的条数，都可以直接测出来。

---

## V4 打薄与工程强化排期（2026-11-23，基线 v0.1.90）

> **依据文档**：[评估方案 v3.0](商业化对标与工程强化评估方案-2026-11-23.md) · [实施计划书 v2.0](商业化升级与工程强化实施计划书-2026-11-23.md) · [交付审计清单](AUDIT-CHECKLIST.md) · [基线 v0.1.90](baseline/v0.1.90/baseline.md) · [ARCHITECT-V4.md](ARCHITECT-V4.md)
> **排期原则**：第一档 **V4 打薄**为当前唯一最高优先级目标；取消工时估算，全部节奏由基线门禁与审计清单控制；V4 收口前不新增依赖双轨并存的特性。

### 依据文档与交付契约

| 铁律 / 契约 | 核心内容 |
|:---|:---|
| **1. V4 免重启兼容契约** | 主协议版本冻结在 `kRenderIpcVersion = 3`，服务端兼容 `[2, 3]`；新增字段一律走可选 JSON 键与能力位增量演进，保障覆盖升级后「旧 DLL + 新 Server」免重启可用。 |
| **2. 基线不劣化门禁** | 每完成一个工作包运行 `python tools/baseline/collect_baseline.py --out docs/baseline/<版本> --with-tests` 并对比 `v0.1.90` 四项硬指标（产物依赖、考试通过数 ≥ 279/292、确定性哈希一致、峰值私有内存 ≤ 42 MB），任一劣化即停下修。 |
| **3. 小步闭环与开关保护** | 涉及架构切换的动作（`use_server_*` 切默认、`bigram` 换格式、整句改评分）必须保留 `config.ini` 或编译期开关，直至宿主矩阵或盲测门禁通过。 |
| **4. 交付自测与独立审计** | 每个工作包完成后，必须严格遵循 [AUDIT-CHECKLIST.md](AUDIT-CHECKLIST.md) 五条拒收铁律、通用前置 `P1–P6` 及对应工作包专有判据逐行执行命令验证，并按 §十模板输出交付说明；单工作包独立 commit，确保 `P6` 改动范围清晰。 |

### 代码核查前置补正（基于 `db9775b` 实测）

1. **`collect_baseline.py --diff` 当前为空桩**：`tools/baseline/collect_baseline.py:397-403` 目前只打印占位文本且恒返回 0；`S3.1` 须先补齐四项硬指标（`binaries[].deps` / `exam.total.pass` / `determinism.identical` / `latency.peak_private_mb`）自动比对与回归非零退出码。
2. **`V1` 迁入 `taishen_server` 存在隐性依赖与调用耦合**：`settings_window.cpp` 引入 `<wincrypt.h>`（需链接 `crypt32`）、`audio_capture.cpp` 引入 WASAPI/MMCSS（需核补 `oleaut32`/`avrt`）、`banner_window.cpp` 依赖 `app_state.cpp`；且 `tsf_module.cpp`（L1158/1414/1419/1424/1430/1501）直接调用 `CBannerWindow::Instance()` 前台跟踪与开关接口，`V1.1–V1.3` 须补齐链接库并经 IPC 同步工具栏开关与前台状态。
3. **IMM32 通道尚未接入 Server IPC**：`platform/windows/src/imm32/imm32_ime.cpp` 与 `CMakeLists.txt:210-224` 目前仍直调 `engine_*` 与本地 `CCandidateWindow`，未引入 `ipc_render_client.cpp`；将 `V3` 拆为 `V3.1`（TSF DLL 摘链接）与 `V3.2`（IMM32 迁入 Server IPC 并摘链接，对应 `V3-7`），防止 IMM32 阻塞 TSF 主线收口。
4. **32 位宿主连 64 位 Server 通道已就绪**：`platform/windows/include/render_ipc.h` 已全面采用定宽类型，且 `CMakeLists.txt:409-416` 已存在 `test_render_ipc_x86` 跨位数冒烟测试；无需另建 32 位 server，纳入 `V2.3` / `V3.1` 门禁即可。
5. **`S2` 拼音行光标线需与 V4 服务端渲染协同**：`use_server_render = true` 后候选窗由 `taishen_server.exe` 绘制，当前 `render_ipc.h` 尚无光标偏移字段；`S2.2` 须在冻结 `v=3` 契约下追加可选 JSON 键 `"cursor"`（旧 DLL 缺省兼容）。

---

### 六波次排期总表（Wave 0 – Wave 5，共 29 项原子任务）

#### Wave 0 · 门禁先行与运行时止血（`S3.1–S3.2` + `S1.1–S1.5`）

> 零架构切换风险，优先立起自动门禁与崩溃取证能力，避免 `V1` 把控制台 `stderr` 污染和并发隐患带入常驻 `taishen_server.exe`。

| 编号 | 工作包 | 任务 | Root | 前置 | 状态 | 关键文件与审计条目（[AUDIT-CHECKLIST](AUDIT-CHECKLIST.md)） |
|:---|:---|:---|:---|:---|:---|:---|
| `S3.1` | `S3` | 补全 `collect_baseline.py --diff` 四项硬指标（`deps` / `exam` / `determinism` / `peak_private_mb`）自动比对与回归非零退出码 | `#11` | 无 | ✅ 完成（`5d6262e`） | `tools/baseline/collect_baseline.py` · 审计：`P5`, `S3-3` |
| `S3.2` | `S3` | 新建 `.github/workflows/ci-verify.yml`（`cargo fmt --check`、`clippy -D warnings`、`cargo test --release`、`i686` check、基线门禁、`check-structure`；C++ 行数硬阻断随 `V4.2` 启用） | `#11 #12` | `S3.1` | ✅ 完成（`5d6262e`；`S3-4` PR 变红待推送后实测） | `.github/workflows/ci-verify.yml`, `tools/check-structure.ps1` · 审计：`S3-1`–`S3-5` |
| `S1.1` | `S1` | `engine/src/log.rs:79` `eprint!` 改为仅 `cfg!(test)` 或 `TAISHEN_STDERR_LOG=1` 开启；`:40` 修本地时区 | `#9 #10` | 无 | ✅ 完成（`aa27d31`） | `engine/src/log.rs`（含 `lm.rs` 日志收敛）· 审计：`S1-1`, `S1-2` |
| `S1.2` | `S1` | `platform/windows/src/debug_log.cpp` 加 `std::mutex`、`FILE_SHARE_DELETE`、5 MB 轮转 | `#9 #10` | 无 | ✅ 完成（`aa27d31`） | `platform/windows/src/debug_log.cpp` · 审计：`S1-3` |
| `S1.3` | `S1` | `engine/src/dictionary/write_back.rs` 抽 `open_user_db()`（WAL + `synchronous=NORMAL` + `busy_timeout=2000`），替换 `dictionary/mod.rs` 7 处裸连（2285/6284/6318/6337/6359/6378/6739）并补并发写单测 | `#1 #10` | 无 | ✅ 完成（`aa27d31`，含 L2493 共 8 处全收口） | `engine/src/dictionary/write_back.rs`, `engine/src/dictionary/mod.rs` · 审计：`S1-4`, `S1-5` |
| `S1.4` | `S1` | 确定性收口：`dictionary/mod.rs:2951/3097` 排序比较器追加 `.then_with(\|\| a.0.cmp(&b.0))`，全仓排查 `HashMap::into_iter` 候选路径 | `#2 #10` | 无 | ✅ 完成（`aa27d31`） | `engine/src/dictionary/mod.rs` · 审计：`P4`, `S1-6` |
| `S1.5` | `S1` | 编译加固与崩溃转储：CMake 补 `/guard:cf`、`/CETCOMPAT`（x64）、`/Zi`；`engine/Cargo.toml` 补 `[profile.release] lto="thin"`、`codegen-units=1`；`taishen_server.cpp:953-967` 接 `MiniDumpWriteDump` | `#6 #10 #12` | 无 | ✅ 完成（`aa27d31`） | `platform/windows/CMakeLists.txt`, `engine/Cargo.toml`, `platform/windows/src/taishen_server.cpp` · 审计：`S1-7`, `S1-8` |

**本波次门禁**：`P1–P6` + `S1-1`–`S1-8` + `S3-1`–`S3-5`（基线快照：[v0.1.90-wave0](baseline/v0.1.90-wave0/baseline.md)）。

#### Wave 1 · 第一档启动 `V1` 补 Server 能力 + 第二档 `S2` 手感四项（`V1.1–V1.3` + `S2.1–S2.3`）

> 保持 `ime.dll` 默认进程内路径不变，两套 UI 并存，零架构回退风险；同步完成输入手感四项缺陷修复。

| 编号 | 工作包 | 任务 | Root | 前置 | 状态 | 关键文件与审计条目（[AUDIT-CHECKLIST](AUDIT-CHECKLIST.md)） |
|:---|:---|:---|:---|:---|:---|:---|
| `V1.1` | `V1` | `platform/windows/CMakeLists.txt` 的 `taishen_server` 目标追加 `settings_window`/`banner_window`/`audio_capture`/`voice_manager`/`app_state` 与 8 个 `ui_*.cpp` 及所需库（`crypt32`/`oleaut32` 等），`ime.dll` 不动 | `#3 #8` | Wave 0 | ✅ 完成（`9f18522`；`V1-4`/`V1-5`/`V1-7` 待实机复核） | `platform/windows/CMakeLists.txt` · 审计：`V1-1`, `V1-2`, `V1-3`, `V1-6` |
| `V1.2` | `V1` | `render_ipc.h` + `taishen_server.cpp` + `ipc_render_client.cpp` 在 v3 契约下新增可选消息（`open_settings`/`toggle_voice`/工具栏前台与开关同步） | `#3 #8` | `V1.1` | ✅ 完成（`9f18522`；`V1-4`/`V1-5` 待实机复核） | `platform/windows/include/render_ipc.h`, `src/taishen_server.cpp`, `src/ipc_render_client.cpp` · 审计：`V1-4`, `V1-5` |
| `V1.3` | `V1` | `tsf_module.cpp` 语言栏「属性设置…」及语音/工具栏入口改走管道消息，server 不可达时回退本地（保留降级），验保存后即时生效 | `#3 #7 #8` | `V1.2` | ✅ 完成（`9f18522`；`V1-4`/`V1-5`/`V1-7` 待实机复核） | `platform/windows/src/tsf_module.cpp` · 审计：`V1-4`, `V1-5`, `V1-7` |
| `S2.1` | `S2` | 拼音中部插入与方向键接管：`engine/src/lib.rs:1141-1143` `push` 改 `insert(self.cursor, ch)` 同步 `raw_input`；`tsf_keyevent.cpp` 有拼音时接管 `VK_LEFT`/`VK_RIGHT`/`VK_HOME`/`VK_END`（`Ctrl+Left/Right` 复用已有 `engine_move_cursor`，不重复建设） | `#2 #3` | Wave 0 | ✅ 完成（`623d1c5`；`S2-1` 待实机复核） | `engine/src/lib.rs`, `platform/windows/src/tsf_keyevent.cpp` · 审计：`S2-1`, `S2-2`, `S2-3`, `S2-8` |
| `S2.2` | `S2` | 候选窗光标竖线 + 页码 + 默认翻页键：`render_ipc.h` 增可选 `"cursor"` 字段；`candidate_window.cpp` 画 1.5px 光标竖线与 `‹ N/M ›` 可点翻页；`config_reader.h:89` 改 `page_minus_equal = true` | `#3 #7 #8` | `S2.1` | ✅ 完成（`623d1c5`；`S2-4`/`S2-6` 待实机复核） | `render_ipc.h`, `src/candidate_window.cpp`, `include/config_reader.h` · 审计：`S2-4`, `S2-6`, `S2-7` |
| `S2.3` | `S2` | 数字后半角标点：`tsf_module.cpp` 数字透传分支置 `g_lastCommitEndsWithDigit = true`、非数字键复位；`tsf_keyevent.cpp` 放宽 `:` 直通条件（`3.14` / `1,000` / `14:30` 输出半角） | `#3` | Wave 0 | ✅ 完成（`623d1c5`；`S2-5` 待实机复核） | `platform/windows/src/tsf_module.cpp`, `src/tsf_keyevent.cpp` · 审计：`S2-5` |

**本波次门禁**：`P1–P6` + `V1-1`–`V1-7` + `S2-1`–`S2-8`（基线快照：[v0.1.90-wave1](baseline/v0.1.90-wave1/baseline.md)）。`V1` 迁入 `voice_manager`/`settings_window` 使 `taishen_server.exe` 体积由 `4.40 MB` 增至 `6.91 MB`（引入既有引擎依赖 `reqwest`/`hyper`/`h2-`/`ws2_32.dll`），已同步排入 `V3.1` 替换 `voice.rs` 的 `reqwest` 以满足 `V3-8 < 6 MB`。

#### Wave 2 · 第一档切换 `V2` 默认切 Server + 全宿主灰度（`V2.1–V2.3`）

> 将非容器宿主默认路径切换至 `taishen_server.exe`，保留 `config.ini` 逃生开关完成全宿主矩阵灰度。

| 编号 | 工作包 | 任务 | Root | 前置 | 状态 | 关键文件与审计条目（[AUDIT-CHECKLIST](AUDIT-CHECKLIST.md)） |
|:---|:---|:---|:---|:---|:---|:---|
| `V2.1` | `V2` | `ipc_render_client.cpp` 5s 重连黑洞改指数短探测（`50/150/500 ms`），保障 server 自拉起后快速就绪 | `#3 #10` | `V1.3` | ✅ 完成（`8dc4e2a`） | `platform/windows/src/ipc_render_client.cpp` · 审计：`V2-3` |
| `V2.2` | `V2` | `config_reader.h:197/199` 切默认 `use_server_render = true`、`use_server_engine = true`，保留 `config.ini` `use_server_engine=0` 逃生开关与配置说明 | `#3 #7` | `V2.1` | ✅ 完成（`8dc4e2a`） | `platform/windows/include/config_reader.h` · 审计：`V2-1`, `V2-2` |
| `V2.3` | `V2` | 全宿主矩阵灰度验收（Edge / Win11 记事本 / VS Code / Office / 微信 / 32 位游戏 + `test_render_ipc_x86` / 任务栏搜索 AppContainer），记录首键延迟样本、100 字连续输入与免重启升级实测 | `#3 #11` | `V2.2`, `S2.2`, `S2.3` | ⏳ 待人工灰度实测（x64/x86 IPC 冒烟与基线已过；`V2-4`/`V2-5`/`V2-7` 宿主矩阵待真机逐项签收后再进 `V3`） | 宿主矩阵实测记录 · 审计：`V2-4`–`V2-7` |

**本波次门禁**：`P1–P6` + `V2-1`–`V2-7`（基线快照：[v0.1.90-wave2](baseline/v0.1.90-wave2/baseline.md)）。

#### Wave 3 · 第一档收口 `V3` 摘链接 + `V4` 拆 `tsf_module`（`V3.1–V3.2` + `V4.1–V4.2`）— 第一档里程碑

> 完成 V4 进程边界核心目标：`ime.dll` 与 `ime_imm32` 彻底摘除引擎/D2D/网络栈依赖，并拆分 `tsf_module.cpp`。

| 编号 | 工作包 | 任务 | Root | 前置 | 状态 | 关键文件与审计条目（[AUDIT-CHECKLIST](AUDIT-CHECKLIST.md)） |
|:---|:---|:---|:---|:---|:---|:---|
| `V3.1` | `V3` | TSF DLL（`taishen_ime` / `taishen_ime_x86`）纯减法摘链接：从 `CMakeLists.txt` 移除 `taishen_engine`/`d2d1`/`dwrite` 及 18 个 UI/语音/引擎源文件，清理 `tsf_module.cpp` 本地 UI 直调；并将 `engine/src/voice.rs` 的 `reqwest` 替换为轻量 `std::net::TcpStream` HTTP/1.1 以满足 `V3-8 < 6 MB`；重构建后验依赖清零、`ime.dll < 1.2 MB`、`server.exe < 6 MB` | `#3 #8 #12` | `V2.3`, `S1.1–S1.5` | ✅ 完成（`6ba6b2a`；`V3-6` 宿主矩阵待真机复核） | `platform/windows/CMakeLists.txt`, `src/tsf_module.cpp`, `engine/src/voice.rs` · 审计：`V3-1`–`V3-6`, `V3-8` |
| `V3.2` | `V3` | IMM32 通道迁入 Server IPC 并摘链接：`imm32_ime.cpp` 接入 `ipc_render_client.cpp` 转发按键/渲染，随后从 `taishen_ime_imm32`（x64/x86）移除 `taishen_engine` 与 `d2d1`/`dwrite` | `#3 #8` | `V3.1` | ✅ 完成（`6ba6b2a`；x64/x86 IMM32 IPC 冒烟已过，`V3-6` 宿主待真机复核） | `platform/windows/src/imm32/imm32_ime.cpp`, `CMakeLists.txt` · 审计：`V3-1`, `V3-2`, `V3-5`, `V3-7` |
| `V4.1` | `V4` | 物理删除 `tsf_module.cpp` 废弃托盘代码（L1508–2160）与空桩调用（L3018–3020、L3260–3262），迁移核验 `ReloadConfigIfChanged` 热加载链完整 | `#3 #8` | `V3.1` | ✅ 完成（`0d32842`；`V4-5` 热加载调用链已代码核验，真机切换窗口复核待人工签收） | `platform/windows/src/tsf_module.cpp` · 审计：`V4-4`, `V4-5` |
| `V4.2` | `V4` | 纯移动拆分 `tsf_module.cpp`（4,493 行）→ `tsf_text_service` / `tsf_key_sink` / `tsf_edit_session` / `tsf_langbar` / `tsf_focus_caret`（5 文件均不超限），`dumpbin /exports` 校验前后导出表一致，并将 `check-structure.ps1` 行数检查升为硬阻断 | `#3 #12` | `V4.1` | ✅ 完成（`0d32842`；`V4-6` 宿主矩阵待真机复核，非 TSF 历史 3 文件由 `check-structure.ps1` 棘轮锁死） | `platform/windows/src/tsf_*.cpp`, `CMakeLists.txt`, `tools/check-structure.ps1` · 审计：`V4-1`–`V4-3`, `V4-6`, `S3-5` |

**本波次门禁**：`P1–P6` + `V3-1`–`V3-8` + `V4-1`–`V4-6` + `S3-5`（`V3` 基线快照：[v0.1.90-v3](baseline/v0.1.90-v3/baseline.md)；`V4` 基线快照：[v0.1.90-v4](baseline/v0.1.90-v4/baseline.md)）。

#### Wave 4 · 第二档收尾 `S3.3` + 第三档质量与易用 `Q1` / `Q2`（5 项）

| 编号 | 工作包 | 任务 | Root | 前置 | 状态 | 关键文件与审计条目（[AUDIT-CHECKLIST](AUDIT-CHECKLIST.md)） |
|:---|:---|:---|:---|:---|:---|:---|
| `S3.3` | `S3` | 试卷清洗与词库回源：按 [CASE-AUDIT.md](modules/exam/CASE-AUDIT.md) 处置失效用例；词库缺口回 `taishen-dict/curate/` 重建后经 `sync_to_ime.py` 单向同步（严禁直改 `resources/`） | `#1 #2 #11` | `S3.2` | ✅ 完成（`b7d90d0`；`taishen-dict` `651889f` 局部重建 `common.db` 并经 `sync_to_ime.py` 同步，exam `284/289 (98.3%)`、`L4-D 11/11`） | `engine/tests/exam-cases.tsv`（按审计处置）, `taishen-dict/curate/` · 审计：`P3`, `S3-6` |
| `Q1.1` | `Q1` | `engine/src/sentence.rs:61` `LN_RAW_TOTAL` 改二元条件概率 + 显式边缺失平滑回退（保留编译期开关 `TS_SENTENCE_LEGACY`） | `#2` | `S3.3` | ✅ 完成（`4581311`；二元条件概率 `ln P(w|prev)` + 显式 Katz 退避与 `bigram.db` 平滑，保留 `sentence_legacy` 编译期与 `TS_SENTENCE_LEGACY` 运行期开关，`exam` `284/289 (98.3%)`、`probe_sentence` `14/15 + 4/4`） | `engine/src/sentence.rs` · 审计：`P3`, `Q1` |
| `Q1.2` | `Q1` | 高冲突同音字双轨束搜索（在/再、的/得/地）+ 放宽 `sentence.rs:105` 纯声母伪段通道 + 建 500 句盲测集（首轮 Top-1 ≥ 55%，严禁针对盲测句硬编码） | `#2 #11` | `Q1.1` | ✅ 完成（`e823b43`；高冲突同音簇双轨束搜索 + `de→地` 主读豁免 + O(1) 纯声母词图通道 + 500 句盲测集首轮 Top-1 `315/500 (63.0%)` / Top-3 `402/500 (80.4%)`、防作弊 0 命中，`exam` `284/289 (98.3%)`） | `engine/src/sentence.rs`, 盲测集 · 审计：`P3`, `Q1` |
| `Q2.1` | `Q2` | 模糊音重构（`engine/src/fuzzy.rs` + `config_reader`）：9 组独立开关且默认关，子串替换改音节边界替换 | `#2 #7` | `S3.2` | ✅ 完成（`704b55a`；9 组独立开关默认全关 + 音节边界替换，`fuzzy` 单测 11/11、`test_config_reader` 与全量 `cargo test` 通过，`exam` `284/289`） | `engine/src/fuzzy.rs`, `include/config_reader.h` · 审计：`Q2` |
| `Q2.2` | `Q2` | 易用性补齐：竖排候选、迁移预设、设置页「词库与数据」页、修复「恢复默认」保留 `app_*` 列表、进程拾取器（`EnumWindows`） | `#7 #8` | `V3.1` | ✅ 完成（`1036df9` + `26cf0b0`；竖排候选布局与命中、4 套迁移预设、设置页第 9 栏「词库与数据」导出/导入/清空 FFI、修复「恢复默认」保留 `app_*` 列表、纯 Win32 进程拾取器，并顺带修复 `load_bigram_bin` 启动 8 MB 瞬态内存峰值；`check-structure`/双架构构建/`test_config_reader`/`test_q22_usability`/`test_render_ipc` x64&x86/全量 `cargo test` 全过，桌面 GUI 视觉与交互待人工复核） | `src/candidate_window.cpp`, `src/settings_window.cpp`, `src/config_reader.cpp` · 审计：`P1`–`P6`, `Q2` |

**本波次门禁**：`P1–P6` + `S3-6` + `Q1` / `Q2`（`S3.3` 基线快照：[v0.1.90-s3.3](baseline/v0.1.90-s3.3/baseline.md)；`Q2.1` 基线快照：[v0.1.90-q2.1](baseline/v0.1.90-q2.1/baseline.md)；`Q1.1` 基线快照：[v0.1.90-q1.1](baseline/v0.1.90-q1.1/baseline.md)；`Q1.2` 基线快照：[v0.1.90-q1.2](baseline/v0.1.90-q1.2/baseline.md)；`Q2.2` 基线快照：[v0.1.90-q2.2](baseline/v0.1.90-q2.2/baseline.md)）。

#### Wave 5 · 第三档平台契约与性能深水 `Q3` / `Q4`（4 项）

| 编号 | 工作包 | 任务 | Root | 前置 | 状态 | 关键文件与审计条目（[AUDIT-CHECKLIST](AUDIT-CHECKLIST.md)） |
|:---|:---|:---|:---|:---|:---|:---|
| `Q3.1` | `Q3` | TSF 契约补全：`ITfCompartment`、`ITfInputScope`（密码框自动切英文）、`ActivateEx dwFlags`、UIA Provider、`ITfFnReconversion` | `#3 #6` | `V4.2` | ✅ 完成（`8664780`；双向 `ITfCompartment`、`ITfInputScope`+`ES_PASSWORD` 自动英文、`ActivateEx dwFlags`、Server 动态 UIA Provider、`ITfFnReconversion`+`engine_reconvert` FFI 与可选 `v=3` IPC；`check-structure`/双架构重建/`test_q31_contracts` 等 12 项平台测/`cargo test` 464 全过，真机 `im-select`/密码框/讲述人/Word 再转换待人工复核） | `platform/windows/src/tsf_*.cpp`, `candidate_uia.cpp` · 审计：`Q3` |
| `Q3.2` | `Q3` | 安装与交付升级：迁 `%ProgramFiles%`、正式签名、`uiAccess="true"`、下线自签 Root CA、修 `install/install.nsi:381/384` `VersionMajor`/`VersionMinor` 硬编码从 `/DVERSION` 拆解 | `#6 #12` | `V4.2` | ⬜ 待开始 | `install/install.nsi`, `.github/workflows/windows-build.yml` · 审计：`Q3` |
| `Q4.1` | `Q4` | Server 内 `candidate_window.cpp` `IDWriteTextLayout` 缓存（消除每键 50–126 次布局创建销毁） | `#8 #11` | `V4.2` | ⬜ 待开始 | `platform/windows/src/candidate_window.cpp` · 审计：`Q4` |
| `Q4.2` | `Q4` | 尾延迟与启动削峰：`zhongguorenmin`（`zhongg` 过渡态）峰值 19.73 ms → < 10 ms；`radical_pinyin.dict.yaml` 改懒加载；按需评估 `TSBGRAM3` mmap（峰值私有内存 ≤ 42 MB） | `#1 #2 #11` | `V4.2` | ⬜ 待开始 | `engine/src/dictionary/`, `engine/src/lib.rs` · 审计：`Q4` |

**本波次门禁**：`P1–P6` + `Q3` / `Q4`（`Q3.1` 基线快照：[v0.1.90-q3.1](baseline/v0.1.90-q3.1/baseline.md)）。

---

### 每波次基线复测与 Release Gate 终验

1. **基线采集与对比**：
   ```powershell
   cargo build --release --examples --manifest-path engine/Cargo.toml
   python tools/baseline/collect_baseline.py --out docs/baseline/v<版本> --with-tests
   python tools/baseline/collect_baseline.py --diff docs/baseline/v0.1.90/baseline.json
   ```
2. **Release Gate 终验 7 条**（详见 [实施计划书 v2.0 §七](商业化升级与工程强化实施计划书-2026-11-23.md#七release-gate) 与 [AUDIT-CHECKLIST.md §十–§十一](AUDIT-CHECKLIST.md)）：
   - 依赖洁净：`ime.dll` / `ime_x86.dll`（含 IMM32 产物）无 `taishen_engine`、`d2d1`、`DWrite`、`reqwest`
   - 基线不劣化：非缺陷组 ≥ 279/292；确定性哈希一致；峰值私有内存 ≤ 42 MB
   - 自动化全绿：`fmt` + `clippy -D warnings` + `cargo test` + `check-structure` 硬阻断
   - 宿主矩阵：Edge / 记事本 / VS Code / Office / 微信 / 32 位游戏 / 任务栏搜索 全过
   - 零宿主污染：控制台宿主加载 DLL 时 `stderr` 为 0 字节
   - 安装洁净：覆盖升级免重启、卸载无残留
   - 文档同步：`DEV-TRACKER.md`、`CHANGELOG`、`docs/baseline/<版本>/` 及审计交付说明同步更新
