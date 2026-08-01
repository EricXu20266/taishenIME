# SPEC: 配置系统（Root #7）— MVP 基础配置

> 对应 ARCHITECT.md Root #7「配置系统 — 不改代码怎么改行为」
> 关联 DEV-TRACKER: 0.1.8 基础配置系统（候选数、词库路径）

---

## 一、需求

MVP 阶段提供两个可配置项，不改代码即可调整输入法行为：

| 配置项 | key | 默认值 | 说明 |
|--------|-----|--------|------|
| 候选词数量 | candidate_count | 9 | 候选窗口最多显示几个候选词 |
| 系统词库路径 | dict_path | 空（内置词库） | SQLite 词库文件路径（相对 DLL 目录或绝对路径） |

**合约**：
- 配置文件：`config.ini`（key=value 每行，`#` 注释），放在 DLL 同目录（调试期）——MVP 用极简格式，不引入 YAML 解析依赖
- 平台层（C++）负责读取配置，通过 FFI 传给引擎：`engine_init(dict_path)` + 新增 `engine_set_candidate_count(n)`
- 引擎层（Rust）保存 candidate_limit，查询后截断候选列表
- 配置读取失败 → 全部回退默认值，不影响输入法加载

**不做**：
- YAML 完整分层配置（默认→安装→用户）——二期
- 热加载（改完即时生效）——二期
- 皮肤/主题、双拼、模糊音等配置——二期
- 用户配置覆盖 `%APPDATA%`——二期

## 二、配置格式

```
# 泰深输入法配置
candidate_count=9
dict_path=system_dict.db
```

解析规则（平台层 C++ 实现，约 60 行）：
- 逐行读取，跳过空行和 `#` 开头行
- 按 `=` 分割为 key/value，trim 空白
- 未知 key 忽略（向前兼容）
- 非法值（非数字）忽略，用默认值

## 三、接口变更

### Rust 引擎（engine/src/lib.rs + ffi.rs）

```rust
// Engine 新增字段
pub struct Engine {
    pinyin_buf: String,
    candidates: Vec<String>,
    candidate_limit: usize,  // 新增，默认 9
}

// 设置候选数上限（截断候选列表）
pub fn set_candidate_limit(&mut self, limit: usize);

// FFI 新增
#[unsafe(no_mangle)]
pub extern "C" fn engine_set_candidate_count(count: i32) -> i32;
//   - 调用 Engine::set_candidate_limit
//   - 返回 0 成功 / -1 引擎未初始化
```

引擎在 `process_key` / `backspace` 查询后截断：`candidates.truncate(candidate_limit)`。

### 平台层（platform/windows）

新增 `config_reader.h/.cpp`：
```cpp
namespace taishen {
struct ImeConfig {
    int candidate_count = 9;
    std::wstring dict_path;   // 空 = 内置词库
};
// 读取 DLL 同目录 config.ini，失败回退默认
ImeConfig LoadConfig(const std::wstring& dllDir);
}
```

`CTextService::ActivateEx` 修改：
```cpp
// 读取配置
taishen::ImeConfig cfg = taishen::LoadConfig(GetDllDir());
// 初始化引擎：传词库路径
engine_init(cfg.dict_path.empty() ? nullptr : W2U(cfg.dict_path).c_str());
// 设置候选数
engine_set_candidate_count(cfg.candidate_count);
```

`RefreshState` 硬编码 `i < 16` 改为引擎返回的真实数量（引擎已截断，无需再限制）。

## 四、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | 引擎：Engine 加 candidate_limit + set_candidate_limit + 查询截断 | engine/src/lib.rs | cargo test |
| 2 | FFI：engine_set_candidate_count | engine/src/ffi.rs | cargo test |
| 3 | 引擎测试：候选数上限生效 | engine/src/lib.rs | cargo test |
| 4 | 平台：config_reader.h/.cpp 解析 | include/config_reader.h + src/config_reader.cpp | 编译 |
| 5 | ActivateEx 集成：读配置 → engine_init + set_count | src/tsf_module.cpp | 编译 |
| 6 | RefreshState 去硬编码 | src/tsf_module.cpp | 编译 |
| 7 | CMake 接入 + 构建 + 回归 | CMakeLists.txt | 全链路 |

## 五、风险与依赖

- **config.ini 缺失**：默认 candidate_count=9 + 内置词库，输入法正常加载
- **词库路径解析**：相对路径以 DLL 目录为基准解析（GetDllDir 已有模式）
- **依赖 0.1.5**：engine_init 已接受 dict_path 参数（NULL=内置降级），本次直接使用
