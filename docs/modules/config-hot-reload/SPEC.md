# SPEC: 配置热加载（对标 rime-ice 重新部署）

> 关联 DEV-TRACKER: V0.2 补充 · 配置热加载
> 对标雾凇拼音「重新部署」机制——改配置不重启输入法即生效

---

## 一、需求

config.ini 修改后自动重新加载生效，无需重启输入法/重新激活。

**对标雾凇**：雾凇改配置 → 托盘「重新部署」→ 生效。泰深更进一步：**自动监听**，
保存 config.ini 即生效（秒级）。

**用户价值**：调候选数/主题/开关不用反复重启输入法，所见即所得。

**合约**：
- **监听 config.ini**（DLL 同目录），mtime 变化 → 重载
- 轮询间隔 2s（轻量，避免高频磁盘 IO）
- **仅重载 config.ini 的配置项**（候选数/开关/主题/字体/行内预编辑）
- **不重载词库**（62 万条重建索引 1-2s，且有并发风险——词库更新走 install 流程重启）
- 重载不打断正在进行的输入（拼音清空？——开关变化可能触发 engine reset，可接受）

**不做**：
- 词库/短语文件/拆字词库热加载——体积大/加载慢，后续
- 监听失败降级（文件被占用/删除时静默跳过，下次轮询恢复）
- 多实例并发写 config（单用户单实例场景）

## 二、数据模型

```
无新增存储——复用 ImeConfig + LoadConfig
```

### 变更检测

```
mtime 轮询：SetTimer 2s → GetFileAttributesEx 取 mtime → 与上次比较 → 不同则重载
首次加载记录基准 mtime
```

## 三、接口

### 平台层（tsf_module.cpp）

```cpp
// 新增
void StartConfigWatch();   // ActivateEx 启动 2s 轮询
void StopConfigWatch();    // Deactivate 停止
void ReloadConfigIfChanged();  // 定时器回调：mtime 变化 → 重载

// 重载流程（抽取 ActivateEx 的配置应用部分为公共函数）
void ApplyConfig(const ImeConfig& cfg, const std::wstring& dllDir);
```

### 应用到

```
引擎：engine_set_candidate_count / fuzzy / correction / mix_mode / traditional / shuangpin / phrase_enabled / phrase_path
候选窗：SetTheme（含跟随系统判断）/ SetFont / SetInlinePreedit
工具栏：主题模式（浅/深）
```

### 定时器

```
WM_TIMER (id=CONFIG_WATCH_TIMER) → ReloadConfigIfChanged
```

## 四、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | 抽取 ApplyConfig（ActivateEx 复用） | tsf_module.cpp | 编译 |
| 2 | mtime 轮询 + WM_TIMER | tsf_module.cpp | 编译 |
| 3 | Start/Stop watch 生命周期 | tsf_module.cpp | 编译 |
| 4 | 全链路验证（改 config.ini 生效） | — | 手动验证 |

## 五、测试用例

- 修改 candidate_count=3 → 2s 内候选数变 3
- 修改 theme_bg → 候选窗变色
- 修改 fuzzy=0 → 模糊音关闭
- 未修改 config → 无重载（无副作用）
- 删除 config.ini → 静默跳过，恢复后继续
