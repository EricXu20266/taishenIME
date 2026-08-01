# SPEC: 可靠性（Root #10）— FFI panic 守卫 + 轻量日志

> 对应 ARCHITECT.md Root #10「可靠性 — 错了能回退」+ Root #9「可观测性」
> 关联 DEV-TRACKER: 0.1.10 FFI panic 守卫 + 日志
> 关联技术债务: TD-1（ffi.rs unwrap 跨 FFI 边界）+ TD-3（无日志）

---

## 一、需求

Rust 引擎通过 C FFI 暴露给 C++ 平台层。Rust 的 panic 跨 FFI 边界传播是 **UB（未定义行为）**——C++ 侧没有 unwinding 支持，进程直接崩溃。

当前 ffi.rs 隐患：
```rust
let mut engine = ENGINE.lock().unwrap();  // 锁中毒 → panic → 跨 FFI 崩溃
```

**合约**：
- 所有 extern "C" 函数体用 `std::panic::catch_unwind` 包裹，panic 时返回安全错误码（-1 或 0）
- Mutex 锁中毒（poisoned）用 `unwrap_or_else(PoisonError::into_inner)` 恢复，不 panic
- 新增零依赖轻量日志模块（写文件 + eprintln），记录引擎生命周期事件与错误
- 日志文件：`%APPDATA%/taishen-ime/logs/engine.log`（与 ARCHITECT 数据分库约定一致）
- 日志级别：INFO（初始化/模式切换/词库加载）/ ERROR（FFI 错误/panic 捕获）

**不做**：
- tracing crate 完整接入——二期（依赖重，MVP 用零依赖文件日志）
- 日志滚动保留 7 天——二期
- Windows EventLog 对接——二期

## 二、接口设计

### panic 守卫宏（ffi.rs）

```rust
/// 包裹 FFI 函数体，panic 时返回 fallback 错误码
macro_rules! ffi_guard {
    ($fallback:expr, $body:block) => {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| $body))
            .unwrap_or_else(|_| {
                crate::log::error("FFI panic 已捕获");
                $fallback
            })
    };
}
```

每个 extern "C" 函数改为：
```rust
#[unsafe(no_mangle)]
pub extern "C" fn engine_process_key(ch: i32) -> i32 {
    ffi_guard!(-1, {
        // 原逻辑，lock 用 poisoned 恢复
    })
}
```

### 日志模块（engine/src/log.rs）

```rust
pub fn init();            // 初始化（创建日志目录 + 打开文件）
pub fn info(msg: &str);   // INFO 级
pub fn error(msg: &str);  // ERROR 级
```

实现：`std::fs::OpenOptions::append` + `std::env::var("APPDATA")`，每行带时间戳。

### Mutex 访问模式

```rust
// 锁中毒恢复（不 panic）
let mut engine = ENGINE.lock().unwrap_or_else(|e| e.into_inner());
```

## 三、日志内容

| 事件 | 级别 | 内容 |
|------|------|------|
| 引擎初始化 | INFO | engine_init(dict_path=...) 成功/失败 |
| 词库加载 | INFO | 从 SQLite 加载 N 条 / 降级内置词库 |
| 候选数设置 | INFO | candidate_count=N |
| 模式切换 | INFO | ascii_mode=true/false |
| FFI panic | ERROR | 捕获位置（函数名） |
| 引擎销毁 | INFO | engine_destroy |

## 四、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | 新建 log.rs 日志模块 | engine/src/log.rs | cargo build |
| 2 | ffi.rs 加 ffi_guard 宏 + 包裹全部函数 | engine/src/ffi.rs | cargo test |
| 3 | 锁中毒恢复（unwrap_or_else） | engine/src/ffi.rs | cargo test |
| 4 | 日志接入：init/info/error 调用点 | engine/src/ffi.rs + dictionary/mod.rs | cargo test |
| 5 | 测试：panic 捕获不崩溃 | engine/src/ffi.rs | cargo test |

## 五、风险与依赖

- **catch_unwind 限制**：仅捕获 unwind 型 panic；`panic = abort` 配置下无效——Cargo.toml 不设 abort，保持默认 unwind
- **日志写入性能**：每次按键不打日志（只在状态变化/错误时打），避免热路径开销
- **依赖 0.1.8**：日志目录复用 %APPDATA%/taishen-ime 约定
