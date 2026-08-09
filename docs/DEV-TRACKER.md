# 泰深输入法 — 需求看板

> 新需求先记账再开发。每项需求标注：状态、涉及 Root、预计工时、验证标准。

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
| B-21 | 错误纠正不智能（只有按键相邻容错） | 雾凇有 40+ 条拼写 derive 规则（wia→wai、hzi→zhi、后鼻音错位等），我们的 correction.rs 只有 logn→long 类键位容错 | ✅ 修复（spelling_variants 全量对齐 rime derive 规则 + 不受 is_full_pinyin 限制，英文零误伤） | P1 |

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
| 0.2.9 | emoji/符号快捷输入 | #2 | ⚪ 符号完成于 0.2.17；Emoji 暂不纳入 | — |
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
| 0.2.27 | 标点表对齐 rime half_shape：`@#%|~` 保留半角、`_`→——、`\`→顿号、数字分隔符 `,.:` | half_shape 预设 | 全角化过度（@＃％ 也全角了） | ✅ 完成（数字分隔符降级——拼音场景无数字流） | P1 |
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
| 0.2.34 | 应用设置图形化管理（结构化"程序→行为"UI） | #7 #8 | ⬜ 待开始 | 4h | 设置对话框高级页升级：从"3 个进程名文本输入框"改为结构化列表（每行：进程名 + 行为下拉[跟随全局/默认英文/默认中文] + 行内预编辑复选），对标搜狗「应用设置」。增删改行，保存写回 app_ascii/app_cn/app_inline |
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
| P1-1 | 上下文联想（前文参与候选排序） | #1 | ✅ 完成（f6178e7） | last_committed + context.rs 搭配表，config context_assoc |
| P1-2 | 人名输入模式（;R 触发） | #1 | ❌ 不做（Eric 决策 2026-08-08） | 跳过，低使用频率 |
| P2-11 | 逐键提示（候选伴随键入实时显示） | #1 #3 | ✅ 已有（核实 2026-08-08：process_key 每键 query_all + UpdateCandidateWindow 实时刷新，天然满足） | 无需开发 |
| P1-3 | 专业词库分类（对标微软/搜狗分类词库） | #1 | ✅ 完成（本轮） | domain_index + domain_short_index，txt 每行"词 拼音"，config domain_dicts 逗号分隔多文件，resources/domains/computer.txt 示例 |

## V0.4.3 全屏/多屏场景候选窗口定位优化（2026-08-08 调研后新增）

> 来源：全屏场景处理调研（泰深 vs Weasel vs 微软拼音），详见记忆库《输入法全屏场景处理调研》
> 核心结论：独占全屏候选窗不可见是平台硬限制（微软官方确认），输入法可控的是——多显示器定位、DPI 坐标对齐、定位兜底分级

| # | 需求 | Root | 状态 | 说明 |
|---|------|------|------|------|
| P0-A | 多显示器定位修正 | #8 | ⬜ 待开发 | PositionWindow 边界检查用 SM_CXSCREEN（主屏）→ 改 MonitorFromPoint + GetMonitorInfo(rcWork)，副屏 clamp 正确 |
| P1-B | 坐标单位对齐（DPI unaware 宿主） | #3 #8 | ⬜ 待开发 | GetTextExt 返回坐标单位取决于宿主 DPI 感知：DPI-unaware 应用返回 96-DPI 逻辑像素，与 GetCursorPos 物理像素混用导致缩放≠100% 时偏移。检测宿主感知模式 + 换算 |
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
| 0.4.5 | 词库锚定拆分组词（phrase_group_guess） | #2 | ⬜ 待开发 | dictionary 新增 phrase_group_guess：递归切段→每段查 full_index 命中词组（容错变体：deletion/correction/spelling）→组合。lib.rs 长串（>6 字符）候选不足时触发。防英文污染：段须命中词库词组 |

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
| 0.5.0 | 语音输入 SPEC 文档 | #2 #3 #7 #8 | ⬜ 待开始 | 1h | 完整需求规格 + 接口设计 + 双路径数据流。写入 docs/modules/voice-input/SPEC.md |
| 0.5.1 | 音频采集模块（WASAPI 16kHz PCM） | #3 | ⬜ 待开始 | 8h | C++ 平台层新增 AudioCapture 类：WASAPI IAudioClient → IAudioCaptureClient，16kHz 单声道 16bit PCM。启动/停止/缓冲区回调。与 VAD 对接 |
| 0.5.2 | VAD 语音活动检测（Rust engine 侧） | #2 | ⬜ 待开始 | 4h | engine 新增 voice.rs 模块：滑窗 RMS 能量检测，可配阈值、最短语音时长、静音超时。FFI 暴露 vad_process_frame / vad_flush。逻辑对齐泰深 TS 版 VoiceActivityDetector |
| 0.5.3 | 泰深检测模块（优先路径） | #3 | ⬜ 待开始 | 2h | 启动时检测：① `~/.taishen/bin/whisper-server.exe` 存在？② 127.0.0.1:9080 可连通？→ 是则直连，跳过引擎下载和 server 启动。进程不存在或端口不通 → 走自管路径 |
| 0.5.4 | Whisper 转写对接（HTTP → whisper-server） | #2 #3 | ⬜ 待开始 | 3h | Rust 端 reqwest：音频段 WAV 编码 → POST 127.0.0.1:{port}/inference → 返回 {text}。超时 30s。server 不可用时自动尝试启动（自管路径）或提示（优先路径泰深未运行） |
| 0.5.5 | Whisper 引擎 + 模型下载管理（自管路径） | #3 #7 #12 | ⬜ 待开始 | 8h | 输入法独立管理引擎和模型：下载 whisper-server.exe + ggml 模型到 `%LOCALAPPDATA%\TaishenIME\whisper\`。CDN 源 + 校验逻辑参照泰深 whisper.ts。进度条 + 模型大小选择。GPU/CUDA 检测决定推荐引擎风格 |
| 0.5.6 | whisper-server 生命周期管理（自管路径） | #3 | ⬜ 待开始 | 4h | 启动/停止/健康检查/自动重启/崩溃循环检测。参照泰深 whisper-server-manager.ts。输入法激活时启动，退出时优雅关闭 |
| 0.5.7 | 语音候选注入（Rust FFI + 候选窗口展示） | #2 #3 #8 | ⬜ 待开始 | 6h | 新增 FFI：engine_voice_result(text) 注入转写文本为语音候选。候选窗口区分语音模式（麦克风图标+流式更新）。说话间逐段上屏（类比 Whisper 模式 onTranscript） |
| 0.5.8 | 工具栏语音按钮 | #8 | ⬜ 待开始 | 2h | 工具栏新增麦克风按钮（Mic/MicOff 图标，Unicode 字符或 D2D 绘制）。点击切换开/关，状态同步到引擎。关闭时发 engine_voice_stop |
| 0.5.9 | 设置页「语音」标签 | #7 #8 | ⬜ 待开始 | 6h | 设置窗新增第 6 个导航页「语音」（kNavNames 扩容至 6，m_navItems[6]）。① 总开关（启用语音输入）② 引擎状态区：显示「泰深已连接（直连模式）」或「本机独立运行」，标注 whisper-server 路径 ③ 模型管理区：模型大小下拉 + 下载按钮/进度 + 已安装标记 ④ 引擎风格选择（CPU/CUDA/BLAS）+ GPU 检测按钮 + CUDA 运行时状态 ⑤ 语言选择（auto/zh）⑥ VAD 阈值滑块。参考泰深 SettingsPage.tsx L2349-2615 |
| 0.5.10 | 端到端集成测试 + 装机实测 | #11 #12 | ⬜ 待开始 | 3h | 四种场景验证：① 有泰深（直连模式）② 无泰深（自管完整流程）③ 安静短句 ④ 嘈杂长句。安装版真机验证 |

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

## Bug 报告（2026-08-09 实测反馈：候选排序 3 项）

> 根因全部定位（真实词库实测复现），待 Eric 确认修复方案后动手。

| # | 问题 | 根因（已实锤） | 状态 | 优先级 |
|---|------|--------------|------|--------|
| B-22 | 简体模式下繁体字被候选（women 出「我們的出口」、ceshi 出「側視」） | **词库数据混入繁体**：system_dict.db 38.1 万词中 16,858 条含繁体独有字，domains.db 16.9 万词中 19,955 条（维基繁体词条直接入库），另含 GBK 乱码残留（紝鐨剉 类 mojibake）。查询层无繁→简归一化，trad.rs 只做输出层简→繁（繁体模式）。 | ⏳ 方案待确认 | P1 |
| B-23 | 过度联想：women 第 1 位是「我们是冠军」而非「我们」；womenceshi 全出「我们测是」拼接怪词 | ① `phrase_guess`（多音节切分联想）在 candidates 为空时**无条件触发**，把音节 top 单字笛卡尔积拼接（wo→我、men→们、ce→测、shi→是 → 我们测是），而 combo_guess/phrase_group_guess 有 `!is_full_pinyin` 条件——条件不一致是 bug；② 领域词热度前置（sport 领域「我们是冠军」womenshiguanjun 前缀命中）+ `apply_long_word_filter` 把 3-4 字短语提前，压过 2 字双字词「我们」；③ 词频排序不约束「词长 = 输入音节数」。 | ⏳ 方案待确认 | P1 |
| B-24 | 常用字词优先级异常（短句/领域词抢常用词位） | 与 B-23 同根：domain 前缀扩展 + domain_boost + long_word_filter 三级叠加，把常用双字词挤下前 5 位。`wo` 正常（我/握/窝/卧/我国），`women`/`ceshi`/`xihuan` 异常。 | ⏳ 方案待确认 | P1 |
```
