# 泰深输入法 (taishenIME)

> 纯自研中文拼音输入法 · Rust 核心引擎 + C++ Windows TSF 平台层

[![Rust](https://img.shields.io/badge/Rust-1.97+-orange.svg)](https://www.rust-lang.org)
[![C++](https://img.shields.io/badge/C++-17-blue.svg)](https://isocpp.org)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

泰深输入法是一个从零自研的中文拼音输入法：Rust 实现核心引擎（音节切分、词库查询、候选排序、纠错联想、特殊模式），C++ 对接 Windows TSF (Text Services Framework) 平台层，Direct2D 自绘全部界面（V0.3.6 极简扁平视觉）。个人项目，AI 辅助开发。

---

## 功能特性

### 输入方式
- **全拼 / 简拼 / 混合拼写**：`zg` → 中国、`nimzai` → 你们在、`yaowoquz` → 要我去做（逐音节智能切分）
- **双拼**：6 种方案（微软/小鹤/搜狗/自然码/紫光/加加），双拼下 V 模式可用
- **词库锚定拆分组词**：长串中间音节容错（`phrase_group_guess`），提高整句命中
- **联想补全**：非完整拼音时逐段递归拼接（≤4 段，音节/声母混插）
- **中英混输**：中文模式下英文词典候选追加，`hel` 原样进首屏

### 选词与排序
- **以词定字**：`[` 取候选首字、`]` 取尾字
- **候选排序切换**：默认（词频+长词过滤）/ 单字优先 / 长词优先
- **上下文联想**：上屏"北京"后输入 `da` → "大学"前置
- **专业词库领域热度重排**：选中领域词热度 +1，查询按热度加权，越用越贴合你的领域
- **常用词分层**：5 万常用词表 + 用户词热/温两档学习（7 天内 ≥2 次压过常用词）
- **置顶候选**：`d` → 的、`m` → 吗/嘛

### 纠错与容错
- **智能纠错**：键盘相邻键容错（`logn` → long）+ 末字符规则纠错（`wia` → wai）
- **多打字母容错**：删除一位变体（`weom` → wom）
- **模糊音**：平翘舌 / 前后鼻音 / n-l / f-h 等
- **错音提示**：易错读音映射表（`jiaose` → 提示"角色"正确写法）

### 特殊输入模式
- **U 模式拆字**：`uhspn` → 朩、`uniuniuniu` → 犇（笔画/拆分/部首混合）
- **V 模式**：计算器（`v85+7*31`）、日期/时间/星期/农历（`rq`/`sj`/`xq`/`nl`）、数字金额大写（`R+`）、Unicode（`U+`）
- **符号体系**（183 分类 3585 符号）：
  - **v 前缀即时反馈**：按 `v` 立即列出 `[v 单字]` + 热门符号（→ ← ↑ ↓ ≈ ≠ ≤ ≥ ± × ÷ ℃ …），v 单字可上屏字母免切英文
  - **v1-v9 分类别名**：序号/数学/标点/箭头/单位货币/希腊/特殊/拼音注音/部首笔画
  - **分类码输入**：`vbd` 标点、`vjt` 箭头、`vsx` 数学、`vxm` 星座… 全量 183 分类
  - **快捷短码**：`vdui` → ✓、`vpi` → π、`vno` → の 等高频直达
- **配对符号成对上屏**：《》（）「」等开符号自动补闭符号 + 光标居中
- **快捷短语**：`bq` → 不客气，支持外部自定义短语文件

### 系统集成
- **应用级配置**：按进程独立中英状态（终端/编辑器默认英文、微信保持中文）、vim 模式透传（Esc/Ctrl+C）
- **专业词库 v2 自动加载**：全量扫描 `domains/` 目录零配置加载，9 领域（计算机/数学/物理/化学/生物/地理/天文/气象/成语），词条取自中文维基百科
- **词库自动学习**：选词即学，越用越懂你；7 天热度衰减
- **词库秒加载**：部署期预编译索引（bincode），首次启动无 6-7s 卡顿
- **诊断日志开关**：设置页基础 tab 可开关（默认关）

### 界面
- **V0.3.6 极简扁平视觉**：候选窗圆角卡片 + 胶囊高亮 + 窗口圆角裁剪
- **自研窗体系统**：Direct2D/DirectWrite 统一渲染（无 Win32 标准控件、无 .rc 资源）
- 控件库：Label/Button/CheckBox/Edit/ComboBox/Tab/ColorSwatch/ScrollBar（窗口弹出层机制：色板/下拉浮最上层不被裁剪）
- 深浅主题跟随系统 + 10 色配色器自定义（弹出位置自动适配窗口边缘）
- 现代化设置窗体：5 页导航（基础/输入/外观/高级/符号），配置全量可视化
- 候选窗：多行展开、翻页、滚轮、鼠标选词、单行紧凑列长词完整显示、多显示器定位 + DPI 适配

---

## 架构

```
┌─────────────────────────────────┐
│        C++ TSF 平台层            │
│   按键拦截 → 拼音串 → 候选上屏    │
│   自绘候选窗/工具栏/设置窗体      │
├─────────────────────────────────┤
│       Rust 核心引擎 (FFI)        │
│   音节切分 · 词库查询 · 状态机    │
│   候选排序 · 纠错 · 联想 · 特殊模式│
├─────────────────────────────────┤
│        SQLite 系统词库           │
│   （jieba 词典 + pypinyin 注音）  │
└─────────────────────────────────┘
```

| 层 | 技术 | 说明 |
|---|------|------|
| 核心引擎 | Rust (cdylib) | 平台无关的拼音处理逻辑，FFI 导出 |
| Windows 平台 | C++17 + TSF | 系统输入法框架对接 |
| 界面渲染 | Direct2D/DirectWrite | 候选窗 / 工具栏 / 设置窗体全自绘 |
| 词库 | SQLite + 预编译索引 | [taishen-dict](https://github.com/EricXu20266/taishen-dict) 独立构建（jieba MIT + Wikipedia CC BY-SA），部署期编译 .bin 秒加载 |
| 代码质量 | Biome + rustfmt + clippy | 格式化 + Lint |

## 快速开始

### 环境要求

- Windows 10+
- Rust 1.97+ (`rustup`)
- CMake 3.26+
- Visual Studio Build Tools (MSVC)

### 构建引擎

```bash
cd engine
cargo build --release
```

### 构建平台层

```bash
cd platform/windows
cmake -B build_vs18 -G "NMake Makefiles"
cmake --build build_vs18
```

### 安装

```bash
# 复制产物到安装目录并注册 TSF
powershell -File install/install_latest.ps1
```

安装后在 设置 → 时间和语言 → 语言 → 中文 → 键盘 中添加「泰深输入法」。

## 项目结构

```
taishenIME/
├── engine/                 # Rust 核心引擎
│   └── src/
│       ├── lib.rs          # Engine 状态机（选词/排序/联想/上下文）
│       ├── dictionary/     # 词库查询（系统/常用/用户/专业分类）
│       ├── pinyin/         # 音节表 + 切分
│       ├── symbol.rs       # 符号表（183 分类 3585 符号 + v 模式）
│       ├── correction.rs   # 智能纠错
│       ├── fuzzy.rs        # 模糊音
│       ├── context.rs      # 上下文联想
│       ├── mistake.rs      # 错音提示
│       ├── radical.rs      # U 模式拆字
│       ├── calculator.rs / datetime.rs / number.rs  # V 模式
│       └── ...
├── platform/
│   └── windows/            # Windows TSF 实现 + 自绘 UI
├── resources/              # 词库文件（由 taishen-dict 独立项目构建产出）
│   ├── system_dict.db      # SQLite 系统词库（gitignore，发布时复制）
│   ├── domains/            # 专业词库分类（computer.txt 等，引擎运行时自动加载）
│   └── common_dict.txt     # 手工维护超高频常用词表
├── install/                # 安装脚本
├── docs/                   # 设计文档（SPEC/调研/需求看板）
└── taishenIME.md           # L2 宪法（开发环境与工作流）
```

## 文档

- [架构设计](docs/ARCHITECT.md)
- [开发需求跟踪](docs/DEV-TRACKER.md)
- [核心数据流](docs/business-flow.md)
- [竞品调研](docs/reference/RESEARCH_2026-08-08-ime-benchmark.md) — 五大输入法功能调研
- [项目宪法](taishenIME.md) — 开发环境、Git 策略、工具链约定
- [词库构建管线](https://github.com/EricXu20266/taishen-dict) — 独立词库项目（jieba + Wikipedia，MIT + CC BY-SA）

## 词库致谢

词库由独立项目 **[taishen-dict](https://github.com/EricXu20266/taishen-dict)** 构建与管理，与输入法 App 解耦独立迭代。

系统词库基于 [jieba](https://github.com/fxsjy/jieba)（MIT License）词典 + [pypinyin](https://github.com/mozillazg/python-pinyin)（MIT License）注音；专业分类词库基于中文维基百科（CC BY-SA 4.0）分类词条提取。感谢所有开源数据与工具的贡献者——清晰许可的数据源让本项目得以专注在引擎与平台层。

## License

MIT
