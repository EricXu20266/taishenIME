# 泰深输入法 (taishenIME)

> 纯自研中文拼音输入法 · Rust 核心引擎 + C++ Windows TSF 平台层

[![Rust](https://img.shields.io/badge/Rust-1.97+-orange.svg)](https://www.rust-lang.org)
[![C++](https://img.shields.io/badge/C++-17-blue.svg)](https://isocpp.org)
[![Version](https://img.shields.io/badge/version-0.1.74-blue.svg)]()
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

泰深输入法是一个从零自研的中文拼音输入法：Rust 实现核心引擎（音节切分、词库查询、候选排序、纠错联想、中英混输），C++ 对接 Windows TSF 与 IMM32 双平台层，Direct2D 自绘全部界面（候选窗 / 工具栏 / 设置窗体 / 托盘）。

个人项目，AI 辅助开发。词库由独立管线 [taishen-dict](https://github.com/EricXu20266/taishen-dict) 构建。

---

## 功能特性

### 输入方式

- **全拼 / 简拼 / 混合拼写**：`zg` → 中国、`rug` → 如果（ru+g）、`xinashi` → 显示（相邻键纠错）
- **双拼**：微软 / 搜狗 / 小鹤 / 自然码四套原生方案，键位与对应输入法一致；
  紫光、加加目前映射到微软键位，键位并不完全一致，不建议使用
- **中英混输**：中文模式下直接打英文候选。判定规则是「中文侧有没有常用词」——
  `rug` → 如果 / rug、`key` → 可以 / key，中文留首位、英文紧随其后；
  `hello` / `python` 这类中文侧只有冷僻切分时，英文独占首位
- **拆字反查**：`u` + 部件拼音 → 汉字候选；查不到时回落常规拼音路径，不留空白候选
- **词库锚定拆分组词**：长串中间音节容错（`phrase_group_guess`）
- **联想补全**：非完整拼音时逐段递归拼接（≤4 段，音节/声母混插）

### 选词与排序

- **候选排序切换**：默认（词频+长词过滤）/ 单字优先 / 长词优先
- **智能联想**：上屏"北京"后输入 `da` → "大学"前置（按前文搭配与用词习惯）
- **上下文联想**：固定前后词搭配表 + 用户习惯调整
- **专业词库领域热度重排**：选中领域词热度 +1，查询按热度加权
- **常用词分层**：2034 条常用词表 + 用户词热/温两档学习（7 天内 ≥2 次压过常用词）
- **置顶 / 降权**：`d` → 的、`m` → 吗/嘛；也可手动把某词压出前两屏
- **词长匹配分区**：输入 N 个音节，N 字词稳定排在前面，不做过度的长词联想
- **以词定字**：`[` 取候选首字、`]` 取尾字

### 纠错与容错

- **键盘相邻键容错**：`logn` → long、`nihap` → 你好
- **拼写规则纠错**：`wia` → wai、`hzi` → zhi、`lng` → long（rime-ice derive 规则）
- **多打字母容错**：删除一位变体（`weom` → wom、`zhongg` → zhong）
- **纠错结果只前置常用词**：`xinashi` → 显示 / 现实；冷僻变体（`nihao` 的 jihao → 极好）不抢正常输入的首位
- **模糊音**：平翘舌 / 前后鼻音 / n-l / f-h 等
- **错音提示**：易错读音映射表（`jiaose` → 提示"角色"正确写法）

### 按键与快捷键

设置页「按键」页可改，全部持久化到 `config.ini`，保存后下一次按键即生效。

| 功能 | 可选键位 | 默认 |
|------|---------|------|
| 中/英文模式切换 | `Ctrl + 空格`、单按 `Shift`（≤300ms） | 两个都开 |
| 全/半角切换 | `无`、`Shift + 空格` | `Shift + 空格` |
| 中/英文标点切换 | `无`、`Ctrl + 句点` | `Ctrl + 句点` |
| 简/繁切换 | `无`、`Ctrl + Shift + A~Z` | `Ctrl + Shift + F` |
| 翻页附加键 | 逗号/句号、主键盘减号/等号、左右方括号（以词定字）、小键盘 +/−（多选；`PgUp`/`PgDn` 固定生效） | 默认仅 `PgUp`/`PgDn`（附加键可按需勾选） |

- 页内有「恢复按键默认设置」，只重置按键，不动其他配置
- 全角模式下提交的 ASCII 字符转全角（中文原样）

### 特殊输入模式

- **V 模式**：计算器（`v85+7*31`）、Unicode（`U+`）、数字金额大写（`R+`）
- **日期时间**：`rq` / `sj` / `xq` / `nl` → 日期 / 时间 / 星期 / 农历候选
- **符号体系**：183 分类 3585 符号
  - **v 前缀即时反馈**：按 `v` 立即列出 `[v 单字]` + 热门符号，v 单字可上屏字母免切英文
  - **v1-v9 分类别名**：序号 / 数学 / 标点 / 箭头 / 单位货币 / 希腊 / 特殊 / 拼音注音 / 部首笔画
  - **分类码输入**：`vbd` 标点、`vjt` 箭头、`vsx` 数学；**快捷短码**：`vdui` → ✓、`vpi` → π
- **配对符号成对上屏**：《》（）「」等开符号自动补闭符号 + 光标居中
- **快捷短语**：`bq` → 不客气，支持外部自定义短语文件
- **Emoji 候选**：可选开关（默认关）

### 系统集成

- **语言栏按钮**（对齐微软拼音）：TSF 任务栏「中/英」状态按钮，左键切中英，右键出快速菜单（含简繁/双拼/标点/显示工具栏/属性设置）
- **浮动工具栏**：中/简/全拼/麦/设置/隐藏六个按钮，固定在屏幕右下角；开关写进配置，跨程序、重启都保持
- **应用级配置**：按进程独立中英状态（终端/编辑器默认英文、微信保持中文）、行内预编辑策略、候选窗自绘策略、vim 模式透传
- **IMM32 兼容层**：极少数陈旧应用（完全不认 TSF）的兜底通道。游戏内聊天通常经 `msctfime.ime` 桥走 TSF 通道，**安装后即可直接输入，不需要任何额外步骤**；确需时才运行 `tools\enable-imm32.ps1`
- **专业词库自动加载**：全量扫描 `domains/` 目录，零配置加载，**39 领域 17.2 万词**
- **词库自动学习**：选词即学，7 天热度衰减
- **词库秒加载**：部署期预编译索引（170MB，分片 + meta 段并行读取，秒开）
- **配置热加载**：改 `config.ini` 保存，下一次按键生效，不用重启
- **语音输入**（可选）：whisper 引擎，工具栏麦克风按钮；优先复用泰深的 whisper-server，否则本机独立运行

### 界面

- **极简扁平视觉**：候选窗圆角卡片 + 胶囊高亮 + 窗口圆角裁剪
- **自研窗体系统**：Direct2D/DirectWrite 统一渲染（无 Win32 标准控件、无 .rc 资源）
- 控件库：Label / Button / CheckBox / Edit / ComboBox / Tab / ColorSwatch / ScrollBar（弹出层浮最上层不被裁剪）
- 深浅主题跟随系统 + 10 色配色器自定义
- **设置窗体 7 页**：基础 / 输入 / 外观 / 高级 / 符号 / 语音 / 按键
- 候选窗：多行展开、翻页、滚轮、鼠标选词、长词完整显示、多显示器定位 + DPI 适配

---

## 架构

```
┌─────────────────────────────────┐
│     C++ 平台层（TSF + IMM32）     │
│  按键拦截 → 拼音串 → 候选上屏     │
│  自绘候选窗 / 工具栏 / 设置 / 托盘 │
├─────────────────────────────────┤
│       Rust 核心引擎 (FFI)        │
│  音节切分 · 词库查询 · 候选排序    │
│  纠错 · 联想 · 中英混输 · 特殊模式 │
├─────────────────────────────────┤
│       SQLite 词库（三层）         │
│   system_dict.db  (37.3 万)      │
│   domains.db      (17.2 万)      │
│   common.db       (2034 条)      │
└─────────────────────────────────┘
```

| 层 | 技术 | 说明 |
|---|------|------|
| 核心引擎 | Rust (cdylib + staticlib) | 平台无关的拼音处理逻辑，FFI 导出 |
| Windows 平台 | C++17 + TSF / IMM32 | TSF 主路径 + 老应用兼容层 |
| 界面渲染 | Direct2D / DirectWrite | 候选窗 / 工具栏 / 设置窗体全自绘 |
| 词库 | SQLite × 3 + 预编译索引 | [taishen-dict](https://github.com/EricXu20266/taishen-dict) 独立构建，部署期编译 .bin |
| 代码质量 | rustfmt + clippy + Biome | 格式化 + Lint |

## 词库职责边界

**本仓库不构建、不管理词库**——词库构建全部走 [taishen-dict](https://github.com/EricXu20266/taishen-dict) 独立管线，本仓库只消费。

```
taishen-dict（构建）：改 curate/ 源 → python pipeline.py（构建+校验+版本清单）
                    → python tools/sync_to_ime.py（hash 对账同步）
taishenIME（消费）：  resources/ 下的 system_dict.db / domains.db / common.db + VERSION.json
```

- 词库版本：`resources/VERSION.json`（当前 `2026.11.17.8`），同步时自动更新
- 打包校验：`package.ps1` 只校验词库存在与版本，不构建词库
- 词库缺失时：运行 taishen-dict 的 `python tools/sync_to_ime.py`
- **词库分层**：采集候选按三档入库——A 术语进对应领域（热域 `domain_heat=1`，高优先）、B 专名（人名/地名/机构/事件）进 `proper_noun` 域（非热域 `heat=0`，低优先但可上屏）、C 碎片丢弃

## 配置文件

`config.ini` 与 DLL 同目录（安装后即 `%LOCALAPPDATA%\TaishenIME\config.ini`），改完保存，下一次按键自动生效。

```ini
candidate_count=5          # 候选词数量（1-20）
sort_mode=0                # 0=默认 1=单字优先 2=长词优先
context_assoc=1            # 智能联想
fuzzy=1                    # 模糊音
correction=1               # 智能纠错
mix_mode=1                 # 中英混输

key_toggle_ascii=ctrl_space,shift   # 中英切换键
key_toggle_fullwidth=shift_space    # 全/半角切换
key_toggle_punct=ctrl_period        # 中英标点切换
key_toggle_traditional=ctrl_shift_f # 简繁切换
key_page_extra=minus_equal,brackets # 翻页附加键

toolbar=0                  # 右下角浮动工具栏

app_ascii=cod.exe,cmd.exe  # 这些程序首次进入默认英文
app_cn=notepad.exe         # 这些程序首次进入默认中文
app_vim=nvim-qt.exe        # vim 模式（Esc/Ctrl+C 切英文并透传）

voice_enabled=0            # 语音输入总开关
```

设置页可改全部常用项；「打开配置文件」按钮直接跳转编辑。

> **架构与应急回退说明**：V4 打薄后，`taishen_ime.dll` / `taishen_ime_imm32.ime` 仅保留轻量平台壳，词库查询与候选窗渲染统一由 `taishen_server.exe` 承载。旧版配置项 `use_server_engine` / `use_server_render` 已废弃（仅保留解析兼容，若在 `config.ini` 中置为 `0` 将记录警告日志且不会回退至进程内引擎）；如遇宿主兼容异常需应急回退，请**降级安装旧版本**。

## 快速开始

### 环境要求

- Windows 10/11（x64）
- Rust 1.97+（`rustup`），并安装 32 位目标
- CMake 3.26+
- Visual Studio 2022 Build Tools（MSVC C++ 工作负载 + Windows SDK）
- Python 3（打包脚本校验词库用）
- NSIS 3（打安装包用）

```powershell
rustup target add i686-pc-windows-msvc
```

### 获取词库

词库是构建产物，不进版本库。二选一：

```powershell
# A. 下载现成产物（推荐，与 CI 使用同一份）
curl.exe -L -o dict.zip https://github.com/EricXu20266/taishen-dict/releases/download/dict-2026.11.21.8/taishen-dict-asset.zip
Expand-Archive dict.zip -DestinationPath resources -Force

# B. 从词库仓库自行构建（构建步骤见该仓库 README）
git clone https://github.com/EricXu20266/taishen-dict
```

解压后 `resources/` 下应有 `system_dict.db`、`VERSION.json`、`common.db`、`bigram.db`，缺任一项 `package.ps1` 会直接报错。

### 构建引擎

```powershell
cargo build --release --manifest-path engine/Cargo.toml
cargo build --release --target i686-pc-windows-msvc --manifest-path engine/Cargo.toml
```

### 构建平台层

```powershell
# 需要先加载 MSVC 环境（vcvars64.bat），或用 VS 开发者命令提示符
cmake -S platform/windows -B platform/windows/build -G "NMake Makefiles"
cmake --build platform/windows/build
```

### 打包与安装

```powershell
# 一键：校验词库 → 引擎 release(64+32) → CMake(64+32) → NSIS
.\package.ps1

# 跳过签名（默认跳过；正式发布由 CI SignPath 签名并启用 -UiAccess）
.\package.ps1 -SkipSign
```

`package.ps1` 会自己定位 `vcvars64.bat` / `vcvars32.bat`，无需事先打开开发者命令行。

产物 `install/TaishenIME-Setup-<版本>.exe`，默认安装到 `%ProgramFiles%\TaishenIME`（需管理员权限写入 `%ProgramFiles%` 与 TSF 注册项），用户配置与用户词库保存在 `%APPDATA%\taishen-ime\`（`config.ini`、`user_dict.db`）。

### 不自己编译

每次推送都会由 GitHub Actions 构建出完整产物：

- Actions → `windows-build` → 选择运行 → 下载 Artifacts `taishenime-windows`
- 或直接从 [Releases](https://github.com/EricXu20266/taishenIME/releases) 下载安装包

CI 流程见 [.github/workflows/windows-build.yml](.github/workflows/windows-build.yml)，与本地的 `package.ps1` 使用同一套构建命令。

### 关于代码签名

本地构建的产物默认**不带签名**且 `uiAccess="false"`，Windows 会提示「未知发布者」。

官方 GitHub Release 安装包由 [SignPath.io](https://about.signpath.io) 提供受信任的代码签名：

- `taishen_ime.dll` / `taishen_ime_x86.dll`、IMM32 兼容层、候选窗服务 `taishen_server.exe` 及安装包均使用正式代码签名
- `taishen_server.exe` 安装于受保护目录 `%ProgramFiles%\TaishenIME`，并原生嵌入 `uiAccess="true"` 清单（支持任务栏搜索框与提权窗口候选窗置顶），无需导入任何自签根证书（升级安装或卸载时会自动清理历史开发版自签根证书残留）。


安装后在 设置 → 时间和语言 → 语言 → 中文 → 键盘 中添加「泰深输入法」，建议注销一次再登录。

**老游戏 / 老应用（IMM32 兼容层）**：文件随安装包一起安装，但**默认不注册**——把 `.ime` 注册进系统级 Keyboard Layouts 会让登录界面（winlogon）也加载它。

绝大多数游戏（含魔兽世界等）的聊天框经 `msctfime.ime` 桥接走 TSF 通道，安装后直接就能输入，不需要任何额外步骤。

只有完全不认 TSF、只走 IMM32 的老应用，才需要以管理员身份运行：

```
%ProgramFiles%\TaishenIME\tools\enable-imm32.ps1
```

关闭兼容层：`tools\disable-imm32.ps1`。0.1.69 及更早版本留下的自动注册，会在升级时由安装器清除。

## 项目结构

```
taishenIME/
├── engine/                     # Rust 核心引擎
│   ├── src/
│   │   ├── lib.rs              # Engine 状态机（选词/排序/联想/混输/纠错入口）
│   │   ├── dictionary/         # 词库查询（系统/常用/用户/专业）+ 多索引
│   │   ├── pinyin/             # 音节表 + 切分
│   │   ├── correction.rs       # 相邻键纠错 + 拼写规则 + 删字变体
│   │   ├── fuzzy.rs            # 模糊音
│   │   ├── english.rs          # 中英混输英文词典
│   │   ├── radical.rs          # 拆字反查（u 模式）
│   │   ├── symbol.rs           # 符号表 + v 模式
│   │   ├── context.rs          # 上下文联想
│   │   ├── mistake.rs          # 错音提示
│   │   ├── shuangpin.rs        # 双拼 6 方案
│   │   ├── trad*.rs            # 简繁转换（含原生繁体词库）
│   │   ├── emoji.rs / unichar.rs / number.rs / calculator.rs / datetime.rs
│   │   ├── voice.rs            # 语音（VAD + whisper 客户端）
│   │   └── ffi.rs              # C ABI 导出
│   └── examples/
│       ├── probe_candidates.rs # 候选行为探针（诊断用）
│       ├── check_first.rs      # 第一屏候选检查
│       └── bench_index.rs
├── platform/windows/           # Windows 平台层
│   ├── include/                # 头文件
│   └── src/
│       ├── tsf_module.cpp      # TSF 主模块（激活/托盘/热点配置/语言栏）
│       ├── tsf_keyevent.cpp    # 按键决策（键位绑定读配置）
│       ├── tsf_composition.cpp # 组合与提交（含全角化）
│       ├── candidate_window.cpp / banner_window.cpp / langbar_item.cpp
│       ├── settings_window.cpp # 7 页设置窗体
│       ├── config_reader.cpp   # config.ini 读写
│       ├── app_state.cpp       # 应用级状态记忆
│       ├── voice_manager.cpp / audio_capture.cpp
│       ├── imm32/              # IMM32 兼容层
│       └── ui_*.cpp            # 自研控件库
├── resources/                  # 词库产物（gitignore，由 taishen-dict 同步）
├── install/                    # install.nsi / install.ps1
├── package.ps1                 # 一键打包
├── docs/                       # 设计文档、需求跟踪、Bug 记录
└── taishenIME.md               # L2 宪法（开发约定）
```

## 文档

- [架构设计](docs/ARCHITECT.md)
- [开发需求跟踪](docs/DEV-TRACKER.md)
- [核心数据流](docs/business-flow.md)
- [项目宪法](taishenIME.md) — 开发流程、Git 策略、工具链约定
- [词库构建管线](https://github.com/EricXu20266/taishen-dict) — 独立词库项目
- [词库采集方法论](https://github.com/EricXu20266/taishen-dict/blob/master/docs/COLLECTION.md) — 三通道采集（维基分类 API / dumps 离线 / RSS 新闻）与三档分级，含实测数据与踩坑

## 设计参考

本项目的功能设计参考了以下开源输入法，仅借鉴公开的键位布局、拼写规则与配置思路，未复制其代码：

- [RIME / librime](https://github.com/rime/librime) — 双拼键位（微软 / 搜狗 / 小鹤 / 自然码）的权威对照、拼写变体（derive）规则、符号表组织方式
- [rime-ice 雾凇拼音](https://github.com/iDvel/rime-ice) — 拼写纠错 derive 规则、拆字词库与符号表
- [Weasel 小狼毫](https://github.com/rime/weasel) — 应用级配置（app_options）、行内预编辑、IMM32 兼容层的设计思路

感谢这些项目的作者与贡献者。

## 词库致谢

词库由独立项目 **[taishen-dict](https://github.com/EricXu20266/taishen-dict)** 构建与管理，与输入法 App 解耦独立迭代。

系统词库基于 [jieba](https://github.com/fxsjy/jieba)（MIT License）词典 + [pypinyin](https://github.com/mozillazg/python-pinyin)（MIT License）注音；专业分类词库基于中文维基百科（CC BY-SA 4.0）分类词条提取 + [THUOCL](https://github.com/thunlp/THUOCL) 清华开放中文词库（MIT License）补充。感谢所有开源数据与工具的贡献者。

## Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org).

- Committers and reviewers: [EricXu20266](https://github.com/EricXu20266)
- Approvers: [EricXu20266](https://github.com/EricXu20266)

Privacy policy: this program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.

输入内容、用户词库、学习记录全部保存在本机，不上传任何服务器。语音输入仅在用户主动触发时使用麦克风，且不做云端上传。安装器写入的内容：TSF 注册表项、`%ProgramFiles%\TaishenIME` 程序目录（含启用 `uiAccess="true"` 的候选窗服务，用于任务栏搜索及提权窗口置顶）、`%APPDATA%\taishen-ime` 用户配置与用户词库目录。卸载方式：控制面板「应用」或安装目录下的卸载程序，会移除注册表项与程序文件（保留用户词库与配置）。

## License

MIT

