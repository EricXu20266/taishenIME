# SPEC: 设置 UI 窗口（替代文本编辑器打开 config.ini）

> 对应 ARCHITECT.md Root #7「配置系统」
> 关联 DEV-TRACKER: 设置图形化（工具栏设置按钮不再打开记事本）
> 前置依赖: config-system（config_reader 已存在，20+ 配置项 + 2s 热加载）

---

## 一、需求

工具栏「设置」按钮当前用 `ShellExecuteW` 打开 config.ini 文本文件，普通用户看不懂 key=value。
改为弹出原生 Win32 设置对话框，可视化编辑全部配置项，保存后写回 config.ini（热加载自动生效）。

**目标**：用户不改代码、不碰文本文件，即可完成输入法全部行为与外观配置。

## 二、界面设计

### 2.1 主对话框 IDD_SETTINGS（约 430×330 DLU）

四个 Tab 页（SysTabControl32）分组配置项，底部按钮行：

| 按钮 | 行为 |
|------|------|
| 确定 | 校验 → SaveConfig 写回 config.ini → EndDialog |
| 取消 | 丢弃修改，EndDialog(IDCANCEL) |
| 恢复默认 | 重置 ImeConfig 默认值并刷新控件（不立即写盘） |
| 打开配置文件 | ShellExecuteW 打开 config.ini（高级用户兜底） |

### 2.2 Tab 页内容

**页 0 基础**
- 候选词数量（1-20，默认 9）→ `candidate_count`
- 候选窗字体 → `font_face`
- 正文字号（12-32，默认 16）→ `font_size`
- 行内预编辑（拼音写组合）→ `inline_preedit`
- 候选标签格式（%d. / ① / %s、）→ `label_format`

**页 1 输入**
- 模糊音 → `fuzzy`
- 智能纠错 → `correction`
- 中英混输 → `mix_mode`
- 简繁转换 → `traditional`
- 双拼模式 → `shuangpin`
- 双拼方案（微软/小鹤/搜狗/自然码/紫光/加加）→ `shuangpin_scheme`
- 快捷短语 → `phrase`
- 英文标点透传 → `ascii_punct`
- Emoji 候选 → `emoji`

**页 2 外观**
- 主题模式（深色/浅色预设）→ 写 10 项 `theme_*` 键
- 自定义配色… → 子对话框 IDD_THEME_COLORS
- 窗口圆角（1-16，默认 4）→ `corner_radius`
- 高亮圆角（1-16，默认 3）→ `hilite_corner_radius`
- 内边距（0-20，默认 8）→ `padding`
- 候选间距（0-40，默认 14）→ `candidate_spacing`

**页 3 高级**
- 应用级英文模式进程（逗号分隔，如 cod.exe,cmd.exe）→ `app_ascii`
- 系统词库路径 → `dict_path`
- 用户词库路径 → `user_dict_path`
- 自定义短语路径 → `phrase_path`

### 2.3 配色子对话框 IDD_THEME_COLORS

10 项主题色，每项一行：`[颜色按钮(当前色)] [HEX 文本] [选择…]`。
选择… 调 `ChooseColorW`（COMMDLG），保存 HEX 到 ImeConfig.theme。

| 行 | 配置键 |
|----|--------|
| 背景 | theme_bg |
| 正文 | theme_text |
| 序号 | theme_label |
| 注释 | theme_comment |
| 边框 | theme_border |
| 选中背景 | theme_highlight（兼容旧键） |
| 选中文字 | theme_highlight_text |
| 选中序号 | theme_highlight_label |
| 页码/次要 | theme_dim |
| 选中标记 | theme_mark |

## 三、接口设计

### 3.1 config_reader 新增写回

```cpp
// config_reader.h
/// 将配置写回 DLL 同目录 config.ini（覆盖写，保留注释头）。
/// 返回是否成功（文件打开失败 → false）。
bool SaveConfig(const std::wstring& dllDir, const ImeConfig& cfg);
```

- 全部键显式写出（含默认值），注释头说明「此文件由设置窗口生成，可直接编辑」
- UTF-8 编码（与 LoadConfig 读取一致）
- 保存后 tsf_module 的 2s 轮询热加载自动检测 mtime → ApplyConfig，无需重启

### 3.2 设置对话框

```cpp
// settings_dialog.h
namespace taishen {
/// 弹出设置对话框（模态，自带消息循环）。
/// @param parent 父窗口（工具栏 HWND），可空
/// @param dllDir DLL 目录（尾分隔符），用于定位 config.ini
void ShowSettingsDialog(HWND parent, const std::wstring& dllDir);
}
```

- 资源从 `g_hModule`（DLL 模块句柄）加载
- 对话框过程内部：LoadConfig → 填充控件 → Tab 切换显隐 → 保存时收集 → SaveConfig
- 主题子对话框与主对话框共享一个 context 结构（保存编辑中的 ImeConfig）

## 四、涉及文件

| 文件 | 操作 |
|------|------|
| `platform/windows/include/resource.h` | 新增：控件/对话框 ID |
| `platform/windows/settings.rc` | 新增：IDD_SETTINGS + IDD_THEME_COLORS 模板 |
| `platform/windows/include/settings_dialog.h` | 新增：ShowSettingsDialog 声明 |
| `platform/windows/src/settings_dialog.cpp` | 新增：对话框过程 + 校验 + 收集 + 配色 |
| `platform/windows/include/config_reader.h` | 修改：+ SaveConfig 声明 |
| `platform/windows/src/config_reader.cpp` | 修改：+ SaveConfig 实现（含主题预设写入） |
| `platform/windows/src/banner_window.cpp` | 修改：Settings 分支 → ShowSettingsDialog |
| `platform/windows/CMakeLists.txt` | 修改：+ settings_dialog.cpp + settings.rc + comdlg32 |
| `docs/modules/config-system/SPEC.md` | 后续补充 SaveConfig（本次仅新增文档） |

## 五、校验规则

| 控件 | 规则 | 违规提示 |
|------|------|----------|
| 候选数 | 1-20 整数 | 「候选数量需在 1-20 之间」 |
| 字号 | 12-32 | 「字号需在 12-32 之间」 |
| 圆角 | 1-16 | 「圆角需在 1-16 之间」 |
| 内边距 | 0-20 | 「内边距需在 0-20 之间」 |
| 间距 | 0-40 | 「候选间距需在 0-40 之间」 |
| 标签格式 | 非空 | 「标签格式不能为空」 |

## 六、风险与依赖

- **线程模型**：ShowSettingsDialog 在工具栏 UI 线程调用（有消息循环），DialogBox 自带循环，无跨线程问题
- **热加载联动**：保存写盘后 tsf_module 2s 轮询自动 ApplyConfig；引擎 FFI 状态（双拼/简繁）由 ApplyConfig 同步
- **rc 中文**：settings.rc 保存为 UTF-8 with BOM，MSVC rc.exe 可正确编译中文字符串
- **测试目标**：`test_banner_window` 依赖 banner_window.cpp → 需同步链接 settings_dialog.cpp + comdlg32
