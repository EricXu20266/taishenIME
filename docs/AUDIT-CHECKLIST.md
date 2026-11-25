# 交付审计清单（开发自测 / 独立审计 共用）

> **基线**：`docs/baseline/v0.1.90/baseline.json`（commit `c43606b` 时采集）
> **适用**：V1–V4（V4 打薄）、S1–S3（稳定与手感）、Q1–Q4（质量生态）
> **用法**：开发 agent 每完成一个工作包，按对应表逐行自测，把**命令与输出**贴进交付说明；审计按同一张表复核，状态只认命令输出。

---

## 〇、五条拒收铁律

1. **判据必须带命令**。写不出读取命令的判据一律视为无效判据（「看起来正常」「应该没问题」不算）。
2. **不许改验证标准代替改对象**。修改 `exam-cases.tsv` 断言、放宽阈值、跳过用例、删测试——直接拒收。
3. **不许用旧产物当本次证据**。日志、截图、baseline 必须是本次跑出来的；引用历史记录须注明时间且不得作为唯一证据。
4. **编译通过不等于做对**。每个工作包的需求点必须逐条对应到命令输出。
5. **不许直接改 `resources/` 下的词库**。任何词库调整必须回 `taishen-dict` 改 `curate/`，重建后 `sync_to_ime.py` 同步。

---

## 一、通用前置（每个工作包都要过）

| # | 判据 | 命令 | 通过条件 |
|:--|:---|:---|:---|
| P1 | 编译通过 | `cargo build --release --manifest-path engine/Cargo.toml` | exit 0 |
| P2 | 单元测试全绿 | `cargo test --release --manifest-path engine/Cargo.toml -- --test-threads=1` | failed = 0，passed ≥ 439 |
| P3 | 考试成绩不劣化 | `engine\target\release\examples\exam.exe` | 非缺陷组 ≥ 279/292 |
| P4 | 确定性 | 同上跑两次，比对候选行哈希 | 两次一致 |
| P5 | 基线快照 | `python tools/baseline/collect_baseline.py --out docs/baseline/<版本> --with-tests` | 生成 baseline.json/md |
| P6 | 改动范围 | `git diff --stat <基线commit>..HEAD` | 与工作包声明一致，无越界文件 |

P3/P4 可用 P5 一次跑完（采集器内含）。**P5 的产物必须提交**，放在 `docs/baseline/<版本>/`。

---

## 二、V1 · 补 server 能力

**目标**：`server.exe` 具备设置窗/工具栏/UI 控件/语音能力，`ime.dll` 不变。

| # | 判据 | 命令 | 通过条件 |
|:--|:---|:---|:---|
| V1-1 | server 源文件已补齐 | `Select-String -Path platform/windows/CMakeLists.txt -Pattern "settings_window\|banner_window\|voice_manager\|ui_combobox"` | 命中且在 `add_executable(taishen_server ...)` 列表内 |
| V1-2 | ime 源文件未动 | `git diff <基线commit> -- platform/windows/CMakeLists.txt` | 只新增 server 侧，未删 ime 侧 |
| V1-3 | ime.dll 体积不变 | `Get-Item platform/windows/out/taishen_ime.dll` | 与基线 7.47 MB 差 < 0.05 MB |
| V1-4 | 设置窗可从语言栏打开 | 人工：右键语言栏 → 属性设置 | 窗口弹出、可保存、保存后立即生效 |
| V1-5 | 设置窗不卡宿主 UI | 人工：打开设置窗同时在其他窗口打字 | 宿主输入无卡顿 |
| V1-6 | server 体积记录 | `collect_baseline.py` 的 `binaries` 段 | server 体积变化写入交付说明 |
| V1-7 | 降级可用 | 杀掉 server 后点属性设置 | ime 本地打开设置窗（保留降级） |

---

## 三、V2 · 切默认

**目标**：`use_server_render/engine` 默认 true，所有非容器宿主走 server。

| # | 判据 | 命令 | 通过条件 |
|:--|:---|:---|:---|
| V2-1 | 默认值已改 | `Select-String -Path platform/windows/include/config_reader.h -Pattern "use_server_render\|use_server_engine"` | 两处均为 `true` |
| V2-2 | 配置废弃兼容与回退说明 | 同上 + `README.md` / `config-system/SPEC.md` | `use_server_engine` 已在 V4 `THIN_DLL` 下废弃（置 `0` 写警告日志，保留解析兼容）；应急回退说明为**降级安装旧版本** |
| V2-3 | 重连探测已优化 | `Select-String -Path platform/windows/src/ipc_render_client.cpp -Pattern "50\|150\|500"` | 指数短探测已落地 |
| V2-4 | 宿主矩阵 | 人工，逐项：Edge / Win11 记事本 / VS Code / Office / 微信 / 32 位游戏 / 任务栏搜索 | 每项：首键出候选、连打 50 字、切窗保组合、点击候选、翻页 |
| V2-5 | 无丢键/重复上屏 | 人工：每宿主连续输入 100 字 | 字数与内容一致 |
| V2-6 | 考试不劣化 | `exam.exe` | ≥ 279/292 |
| V2-7 | 端到端延迟样本 | 人工：录屏或 QPC 计时 | 记录首键出候选耗时，写入交付说明 |

V2-4/V2-5 是**人工判据**，必须附宿主版本与测试时间；不接受「已测试」四个字。

---

## 四、V3 · 摘链接（最关键）

**目标**：`ime.dll` 与引擎、D2D/DWrite、UI、语音彻底脱钩。

| # | 判据 | 命令 | 通过条件 |
|:--|:---|:---|:---|
| V3-1 | ime 不再链引擎 | `Select-String -Path platform/windows/CMakeLists.txt -Pattern "target_link_libraries\(taishen_ime(_imm32)? PRIVATE taishen_engine"` | 0 命中 |
| V3-2 | ime 不再链 D2D | `Select-String -Path platform/windows/CMakeLists.txt -Pattern "^\s*(d2d1\|dwrite)\s*$"` | 0 命中（taishen_ime / imm32 目标内） |
| V3-3 | 依赖二进制清零 | `collect_baseline.py` → `binaries[taishen_ime.dll].deps` | 不含 `d2d1.dll`、`DWrite.dll`、`reqwest`、`hyper`、`h2-` |
| V3-4 | 体积达标 | 同上 `binaries[].mb` | ime.dll < 1.2 MB；ime_x86.dll 同步下降 |
| V3-5 | 无漏搬符号 | `cargo build` + CMake 全量构建 | 无未定义符号 |
| V3-6 | 宿主矩阵重跑 | 同 V2-4 | 全部通过 |
| V3-7 | IMM32 同步打薄 | 同上 V3-1/V3-2（含 imm32 目标） | 0 命中 |
| V3-8 | server 未超预算 | `binaries[taishen_server.exe].mb` | < 6 MB |

**审计重点**：V3-3 必须来自**重新构建后的产物**。若 `platform/windows/out/` 里的 dll 是旧文件，判据无效——先确认 `mtime` 晚于本次 commit。

---

## 五、V4 · 拆 tsf_module

| # | 判据 | 命令 | 通过条件 |
|:--|:---|:---|:---|
| V4-1 | 拆分为 5 文件 | `Get-ChildItem platform/windows/src/tsf_*.cpp` | `tsf_text_service` / `tsf_key_sink` / `tsf_edit_session` / `tsf_langbar` / `tsf_focus_caret` 均存在 |
| V4-2 | 单文件不超限 | `pwsh -File tools/check-structure.ps1` | 无 > 1,200 行 C++ |
| V4-3 | 导出表不变 | `dumpbin /exports platform/windows/out/taishen_ime.dll`（改前改后各一份） | 导出符号集合一致 |
| V4-4 | 托盘死代码已删 | `Select-String -Path platform/windows/src/*.cpp -Pattern "g_trayInstances\|WM_TRAYICON"` | 0 命中 |
| V4-5 | 配置热加载未丢 | 人工：改 `config.ini` 后切换窗口 | 新配置生效 |
| V4-6 | 宿主矩阵 | 同 V2-4 | 全部通过 |

V4-3 是拆分的安全网：导出表变了说明拆错了。

---

## 六、S1 · 运行时止血

| # | 判据 | 命令 | 通过条件 |
|:--|:---|:---|:---|
| S1-1 | stderr 零输出 | `Select-String -Path engine/src/log.rs -Pattern "eprint!"` | 0 命中，或被 `cfg!(test)` / 环境变量包裹 |
| S1-2 | 控制台宿主无污染 | 人工：`pwsh` 内加载 DLL 打一段字，重定向 stderr 到文件 | 文件 0 字节 |
| S1-3 | 日志有锁与轮转 | `Select-String -Path platform/windows/src/debug_log.cpp -Pattern "mutex\|FILE_SHARE_DELETE"` | 均命中 |
| S1-4 | SQLite 裸连收口 | `Select-String -Path engine/src/dictionary/mod.rs -Pattern "Connection::open"` | user_dict 相关 7 处改为 `open_user_db()` |
| S1-5 | 并发写无 BUSY | 新增/已有单元测试 | 零 `SQLITE_BUSY` |
| S1-6 | 排序兜底字典序 | `Select-String -Path engine/src/dictionary/mod.rs -Pattern "a\.0\.cmp\(&b\.0\)"` | 命中 |
| S1-7 | 编译加固 | `dumpbin /loadconfig platform/windows/out/taishen_server.exe` | 含 `GuardCF`；x64 含 CET |
| S1-8 | Minidump 生效 | 人工：触发测试崩溃 | 生成 `.dmp`，WinDbg 可还原栈 |

---

## 七、S2 · 手感四项

| # | 判据 | 命令 | 通过条件 |
|:--|:---|:---|:---|
| S2-1 | 中部插入生效 | 人工：输 `nihao` → `←` ×2 → 打 `x` | `x` 插在第 3 位，光标随动 |
| S2-2 | 插入用 cursor | `Select-String -Path engine/src/lib.rs -Pattern "insert\(self\.cursor"` | 命中 |
| S2-3 | 方向键接管 | `Select-String -Path platform/windows/src/tsf_keyevent.cpp -Pattern "VK_LEFT"` | 有拼音时 `eaten=true` |
| S2-4 | 光标可见 | 人工：按 `←` | 候选窗拼音行出现竖线 |
| S2-5 | 数字后标点 | 人工：中文模式直敲 `3.14` / `1,000` / `14:30` | 全部半角 |
| S2-6 | 页码 | 人工：翻页 | 可见 `1/N` |
| S2-7 | 默认翻页键 | `Select-String -Path platform/windows/include/config_reader.h -Pattern "page_minus_equal"` | `= true` |
| S2-8 | 未重复建设 | `git diff` 检查 | 未新增 `move_cursor_char` 等已有能力 |

---

## 八、S3 · 评测门禁

| # | 判据 | 命令 | 通过条件 |
|:--|:---|:---|:---|
| S3-1 | CI 文件存在 | `Test-Path .github/workflows/ci-verify.yml` | True |
| S3-2 | CI 含测试步骤 | `Select-String -Path .github/workflows/ci-verify.yml -Pattern "cargo test\|clippy\|fmt"` | 均命中 |
| S3-3 | CI 含基线门禁 | 同上，含 `exam` 或 `collect_baseline` | 命中 |
| S3-4 | 失败能变红 | 人工：故意改坏一条用例后推 PR | CI 变红 |
| S3-5 | 结构检查硬阻断 | `Select-String -Path tools/check-structure.ps1 -Pattern "exit 1"` | 命中 |
| S3-6 | 词库未直改 | `git diff --stat <基线commit> -- resources/` | 无改动（除 *.bin 重生成） |

---

## 九、Q1–Q4（简化）

| 工作包 | 核心判据 | 通过条件 |
|:---|:---|:---|
| Q1 整句 | 新增 500 句盲测集 + 分阶段 Top-1 | 第一轮 ≥ 55%；exam 不劣化；**盲测集不得有任何针对句子的硬编码**（`git diff` 检查词库与规则中是否出现盲测句原文） |
| Q2 易用性 | `fuzzy_variants` 单测 | `"zhang"` 不含 `zhhang`/`zhangg`；`"fan"` 不含 `fal` |
| Q3 契约交付 | `im-select` 读写 + 密码框 | 状态互通；密码框自动英文；提权进程可显示候选 |
| Q4 性能 | `zhongguorenmin` 峰值 | < 10 ms；峰值私有内存不升 |

---

## 十、交付说明模板（开发 agent 必填）

```
工作包：V3 摘链接
commit：<hash>
改动文件：<git diff --stat 输出>
P1-P6：<各命令 exit code / 关键输出>
V3-1..V3-8：<逐条命令与结果>
人工项：<宿主版本 + 测试时间 + 现象>
未覆盖项：<明确列出，不得留空>
```

---

## 十一、审计签署

| 项 | 结论 |
|:---|:---|
| 五条铁律 | 通过 / 违反第 N 条 |
| 通用前置 P1–P6 | 通过 / 卡在第 N 项 |
| 工作包专有判据 | 通过 / 第 N 项不达标 |
| 证据来源合法性 | 合法 / 存在旧产物或改标准 |
| 结论 | 可合并 / 退回 |

审计不通过时，退回理由必须指向具体判据编号与命令输出，不接受「质量不够」这类无索引结论。
