# SPEC: 生命周期（Root #12）— MVP 安装器

> 对应 ARCHITECT.md Root #12「生命周期 — 怎么部署升级回滚」
> 关联 DEV-TRACKER: 0.1.11 安装器（NSIS/MSI 基础版）

---

## 一、需求

将编译产物（taishen_ime.dll + system_dict.db + config.ini）部署到目标目录，注册 TSF COM 组件，提供卸载能力。

**决策**：当前环境无 NSIS（winget 受限无法安装）。MVP 采用**零依赖批处理脚本**作为基础版安装器（立即可用），同时交付 **NSIS 脚本蓝图**（用户安装 NSIS 后构建正式安装器，二期发布用）。

**合约**：
- `install.ps1`：复制产物到安装目录 + regsvr32 注册（注册表校验）+ 生成 config.ini
- `uninstall.ps1`：regsvr32 /u 注销 + 删除安装目录（保留用户数据 %APPDATA%/taishen-ime）
- 安装目录：`%LOCALAPPDATA%\TaishenIME\`（HKCU 注册，无需管理员——与调试期一致）
- 词库随安装复制（system_dict.db），config.ini 指向安装目录词库
- 幂等：重复安装覆盖旧文件，不报错
- 脚本用 PowerShell（.ps1）：批处理 .bat 对 UTF-8 中文注释解析错乱（cmd 按 GBK 解码），PowerShell 原生 UTF-8 无此问题

**不做**：
- MSI 安装包——二期（需 WiX 工具链）
- 自动更新（0.2.7）
- 开始菜单/桌面快捷方式——二期
- 系统级安装（HKLM + 管理员权限）——二期

## 二、文件布局

```
安装目录 %LOCALAPPDATA%\TaishenIME\
├── taishen_ime.dll      # 输入法 DLL
├── system_dict.db       # 系统词库（从 resources/ 复制）
├── config.ini           # 配置（候选数/词库路径）
└── uninstall.bat        # 卸载脚本（复制过去）

构建目录 platform/windows/out\
├── taishen_ime.dll      # 编译产物
└── install.bat          # 安装脚本（构建后生成或手动复制）
```

## 三、脚本设计

### install.ps1（PowerShell，零依赖）

```powershell
# 1. 源文件：构建输出目录（platform\windows\out\）
# 2. 安装目录：%LOCALAPPDATA%\TaishenIME
# 3. 复制 DLL + 词库 + 生成 config.ini
# 4. regsvr32 /s 注册（注册表键校验成功，不用 exit code——GUI 程序等待不可靠）
# 5. 提示成功 + 告知去系统输入法设置启用
```

### uninstall.ps1

```powershell
# 1. regsvr32 /s /u 注销
# 2. 删除安装目录（保留 %APPDATA%/taishen-ime 用户数据）
```

### install.nsi（NSIS 蓝图，二期构建）

```nsis
!include "MUI2.nsh"
Name "泰深输入法"
OutFile "TaishenIME-Setup.exe"
InstallDir "$LOCALAPPDATA\TaishenIME"
# 复制 DLL/词库/config + regsvr32 + 卸载程序
# 卸载时 DllUnregisterServer + 删除目录
```

## 四、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | 编写 install.ps1（PowerShell 脚本） | install/install.ps1 | 实际运行验证 |
| 2 | 编写 uninstall.ps1 | install/uninstall.ps1 | 实际运行验证 |
| 3 | 编写 install.nsi（NSIS 蓝图） | install/install.nsi | 语法检查（无 NSIS 则存稿） |
| 4 | 端到端验证：install → 注册 → 卸载 | — | reg query 验证注册表 |

## 五、风险与依赖

- **NSIS 不可用**：本期只交付脚本蓝图，正式构建待 NSIS 环境就绪（或二期 CI）
- **依赖 0.1.2**：注册表注册由 DLL 的 DllRegisterServer 完成，脚本只调 regsvr32
- **用户数据保留**：卸载不删 %APPDATA%/taishen-ime（用户词库/日志二期）
