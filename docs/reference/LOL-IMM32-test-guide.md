# LOL 实测指引 — IMM32 兼容层验证（V0.6）

> 状态：DLL 构建与单元测试全部通过（test_imm32_load），真机实测待 Eric。
> 目标：验证 LOL（英雄联盟）可切换泰深输入法并输入中文。

## 前置（必做）

安装 IMM32 兼容层（需要管理员，写 HKLM Keyboard Layouts）：

```powershell
# 1. 构建（如果 out/ 还没有 .ime）
cd E:\AllinDeepSeek\taishenIME\platform\windows
.\build_imm32.cmd          # 产出 out\taishen_ime_imm32.ime

# 2. 注册（提权弹窗，点击允许）
regsvr32 /s out\taishen_ime_imm32.ime

# 3. 验证注册
reg query "HKLM\SYSTEM\CurrentControlSet\Control\Keyboard Layouts\E0C00804"
# 期望：Layout File / IME File = 完整路径，Layout Text = 泰深拼音
```

> ⚠️ PathGuard 不允许 PowerShell 直接操作 HKLM PSDrive，用 `reg query` 命令行验证。
> 完整安装走 `install\install.ps1`（复制 .ime + 提权注册 + 验证，一步到位）。

## LOL IMEConfig.xml 白名单注册（关键）

LOL 聊天框走 Scaleform GFxIME，只对 `IMEConfig.xml` 白名单内输入法激活并渲染候选。
**即使 IMM32 已注册，也需要把泰深加入 LOL 白名单**（否则可能只能输入、候选由 IME 自绘或不可见）。

1. 找到 LOL 安装目录（国服腾讯版）：`<LOL安装目录>\Game\DATA\Menu\IMEConfig.xml`
   - 例如 `D:\英雄联盟\Game\DATA\Menu\IMEConfig.xml`（WeGame 版在 `WeGame Apps\英雄联盟\...`）
2. 用记事本打开，在 `<ChineseSimplified>` 节点下添加：

```xml
<IME>
    <imeName>泰深拼音</imeName>
    <displayName>泰深拼音输入法</displayName>
    <Tag>GFxIME_Ch_Simp_Taishen_1_0</Tag>
</IME>
```

3. 保存。重启 LOL。

> 参考：白名单现有条目格式（微软拼音示例）：
> `<imeName>微软拼音</imeName>` / `<displayName>MSPinyin IME 3.0</displayName>` / `<Tag>GFxIME_Ch_Simp_MSPinyin_3_0</Tag>`
> displayName 需与输入法在系统中的显示名匹配（LOL 用它匹配当前激活输入法）。

## 实测步骤

1. 启动 LOL，进入对局（或训练模式）——**无边框全屏模式**（你之前用的模式）
2. 按 Enter 打开聊天框
3. Win+Space 或 Alt+Shift 切换输入法到「泰深拼音」
4. 输入拼音测试：组合串显示、候选选择（空格/数字）、上屏、退格
5. 诊断：如果候选框不显示，检查
   - 本机 `%LOCALAPPDATA%\TaishenIME\ime_debug.log`（debug_log 开关在 config.ini）
   - LOL 是否真的加载了 .ime：任务管理器 → LOL 进程 → 加载的模块里搜 `taishen_ime_imm32`
   - IMEConfig.xml 是否生效（重启过 LOL）

## 回退

```powershell
regsvr32 /s /u out\taishen_ime_imm32.ime   # 注销 IMM32
# 或 install\uninstall.ps1 完整卸载
```

## 已知边界（第一版）

- 配对符号光标居中（TSF 有，IMM32 第一版未做——成对输出但光标在末尾）
- Shift tap 中英切换（TSF 有，IMM32 用 Ctrl+Space 切换）
- 32 位 IME DLL 未构建（老游戏若为 32 位进程需补 x86 构建）
- 多行候选展开（↓/↑）IMM32 不响应
