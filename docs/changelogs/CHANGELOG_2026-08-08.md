# 变更日志 — 2026-08-08

## V0.4.x 配对符号成对上屏（feat）

**动机**：中文标点中《》（）【】等开符号几乎总是成对出现，单符号上屏需要
手动补闭符号 + 移动光标，低效且容易漏。对齐微软拼音/搜狗/微信的"智能成对"。

**功能**：
- 全角开符号（（《【｛「『〖〈“‘）上屏时自动补闭符号，光标停在开符号后
- 覆盖路径：单值标点（Shift+9 → （））、复选标点（Shift+[ 选「 → 「」）、
  配对引号（' → ‘’ / " → “”）
- 光标居中：提交后编辑会话内 SetSelection 定位，失败静默降级（光标在末尾）
- 半角符号不参与成对（代码场景不受影响）

**配置**：`pair_punct`（默认开，设置页「输入」卡片「配对符号成对上屏」复选框）
- 关闭时退化为原行为：复选单符号、引号交替开闭

**文件**：
- `platform/windows/include/config_reader.h` / `src/config_reader.cpp`：pair_punct 配置
- `platform/windows/include/tsf_keyevent.h` / `src/tsf_keyevent.cpp`：配对表 + ExpandPairPunct
- `platform/windows/src/tsf_module.cpp`：光标定位编辑会话 + 复选/引号成对化 + 开关同步
- `platform/windows/include/settings_window.h` / `src/settings_window.cpp`：开关 UI

## Bug 修复

### 中文模式无组合时打不出标点（第三次修复，根治）

根因：`CommitComposition` 在无活跃组合时直接 `return S_OK`，而按键链路
（ShouldEatKey 吞键 → HandleKeyDown 生成 committed → RunCompositionOp Commit）
在无拼音直接按标点时必然无组合 → 键被吞、标点丢失。前两次修复只改映射表/
吞键逻辑，未触及提交链路。

修复：无组合但有非空文本时先建空组合再提交。文件：`src/tsf_composition.cpp`。

### 复选标点候选窗不显示（书名号/大括号打不出）

根因：复选标点路径传 `pinyin=""` 给候选窗，而 `CCandidateWindow::UpdateState`
判定 `pinyin 为空 || 候选为空 → HIDE`——pinyin 恒为空，候选窗永远不显示。

修复：判定只看候选是否为空（pinyin 空但候选存在时照常显示，渲染层有
hasPinyin 守卫）。文件：`src/candidate_window.cpp`。
