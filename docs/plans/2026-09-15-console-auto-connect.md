# Console 自动连接与完整工作区展示 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行环境若未提供该技能，按下述任务顺序逐项实施、验证并记录结果；本计划不依赖并行代理。

**Goal:** 新建或启动恢复当前 Console 时自动连接其执行目标，并在离线、连接中和失败状态下始终展示完整 Console。

**Architecture:** 保持 App 状态更新 → Command → Runtime 异步执行 → Action 回传的现有架构，复用 `prepare_active_console_target()` 和 `SessionRegistry::request()`。Console 文档渲染与网络连接状态解耦；启动只准备当前恢复的 Console，后台 Console 在激活时连接。使用一个可校验的最新激活意图处理跨目标连接中的切换，不引入新的连接管理服务。

**Tech Stack:** Rust 1.94 / edition 2024、Ratatui 0.30、Tokio、现有 Workspace 持久化与 SessionRegistry、Cargo 集成测试。

---

## 1. 行为契约

| 场景 | 预期 |
| --- | --- |
| 新建 Console，目标有效 | 立即显示并聚焦编辑器，发起目标连接，保存文档 |
| 普通启动，恢复的活动标签为 Console | 首帧展示保存的 SQL，异步准备该 Console 的完整 execution_target |
| 显式 URL/profile 启动 | 显式目标优先，保留现有显式启动语义；不得被恢复标签的隐式连接覆盖 |
| 恢复多个后台 Console | 仅恢复文档，不遍历所有目标建连；激活时连接 |
| 当前标签为 Relation / Dashboard / RedisBrowser | 本次不新增其启动自动加载规则；保留已有标签激活逻辑 |
| 无标签、无连接 | 保留欢迎页 |
| 有 Console、无连接 | 展示编辑器、结果标签、结果区和 Output，可编辑 SQL |
| 无目标 / 目标失效 | 展示 Console，提示选择目标；不发无效连接请求 |
| 同一目标已连接 / 连接中 | 复用 Session / 连接尝试 |
| 连接失败 | 保留文档，显示目标失败信息，提供显式重试；不按 Tick 重试 |
| 主动断开 | 保留离线 Console，清除受影响的延迟激活意图；不立即回连 |
| 后续主动激活离线 Console | 允许重新准备连接 |
| SQL 执行 / 手动事务 | 建连成功本身不执行 SQL，不恢复服务端事务；沿用现有执行和事务约束 |

状态与身份必须使用完整 `ExecutionTarget { profile_id, database, schema }`；不通过同 profile、Explorer 当前选中项或全局 Connected 推断当前 Console 已就绪。

## 2. 已确认的代码位置

- `src/app.rs:14806`：`create_and_activate_sql_editor_named()` 创建后仅返回持久化命令。
- `src/app.rs:14959`：`activate_sql_editor()` 已调用 `prepare_active_console_target()`。
- `src/app.rs:14976`：Console 准备连接、事务约束及跨目标 pending 阻塞。
- `src/app.rs:15684`：`request_connection_target_inner()` 复用 Session 或发出 Connect。
- `src/app.rs:10994` 附近：连接成功依靠 `pending_editor_target_switch` 决定是否激活工作区。
- `src/app.rs:11454` 附近：失败处理与 `target_error` 归属。
- `src/app.rs:2173`：`restore_workspace()` 恢复文档和活动标签。
- `src/runtime.rs:5723`、`:5788`：恢复工作区及首帧前启动动作派发。
- `src/runtime.rs:6033`：两个启动入口当前仅处理显式 selected。
- `src/runtime.rs:6417`：selected 来自 URL/profile，并非恢复工作区目标。
- `src/ui/mod.rs:738`、`:958`、`:1084`：全局无连接导致 Console 与相关交互隐藏。
- `src/ui/mod.rs:3404`：编辑器目标状态，目前主要依赖全局 connection。
- `src/model/session.rs:91`：已有目标级连接请求去重。
- `tests/console_manager_input.rs:41`：明确断言新建 Console 仅产生持久化命令，需要更新。

行号为计划编写时参考。实施前按符号定位，并检查工作区已有修改。现有其他计划文件应保留。

## 3. 实施顺序

依次完成任务 1 → 2 → 3 → 4 → 5 → 6 → 7。任务之间涉及 App 和 UI 的共享状态，建议串行实施。每个任务先执行能暴露实际缺陷的行为测试，再完成实现与定向验证。

### Task 1：解除 Console 渲染对全局在线状态的依赖

**Files:**
- Modify: `src/ui/mod.rs`，`DisconnectedWorkspace::for_app()`。
- Test: `tests/ui_render.rs`。

**Step 1 — 添加行为回归测试。**

创建有 Console 的离线 App，写入非空 SQL，渲染 120×36 与 80×24。断言 SQL、SQL EDITOR、DATA、OUTPUT 可见，欢迎页文字不存在；断言 editor_viewport 与 Focus::Editor 命中区域存在。恢复场景通过 WorkspaceSnapshot 建立，不仅手工修改 connection.status。

保留已有 `disconnected_workspace_without_profiles_renders_first_run_empty_state` 和 `disconnected_workspace_with_profiles_prompts_for_connection`，它们描述的是无标签场景，仍应通过。

**Step 2 — 运行新增测试，确认旧逻辑隐藏编辑器。**

```bash
cargo +1.94.0 test --test ui_render console_workspace -- --nocapture
```

新增测试统一使用 `console_workspace_` 前缀；预期修改前失败于 SQL/编辑器可见性断言，不能仅因夹具错误失败。

**Step 3 — 修改统一空状态判断。**

在 `DisconnectedWorkspace::for_app()` 开头加入：

```rust
if app.active_console_opt().is_some() {
    return None;
}
```

复用原有正文和 editor_rendered 的调用，不分别新增相互独立的布尔条件。验证无目标 Console 也能渲染；如 renderer 存在目标假设，使用已有 Option 处理补齐，不伪造在线状态。

**Step 4 — 验证整个 UI 测试目标。**

```bash
cargo +1.94.0 test --test ui_render
```

预期全部通过，包括窄屏、TooSmall、光标、视口、鼠标选择和分隔条相关测试。

**Checkpoint:** `fix(ui): keep offline console workspace visible`。

### Task 2：新建 Console 自动准备自身执行目标

**Files:**
- Modify: `src/app.rs`，`create_and_activate_sql_editor_named()`。
- Test: `tests/console_manager_input.rs`、`tests/workspace_tabs.rs`，必要时更新 `src/app.rs` 内受行为变化影响的测试。

**Step 1 — 更新旧契约并补充目标归属测试。**

将 `console_manager_empty_startup_creates_offline_console` 改为新建即请求连接的命名与断言：一个 Console、正确目标、Editor 焦点、关闭管理器、一个匹配目标的 Connect、保留 PersistWorkspace。Connect 异步完成前 `connection.profile_id` 可以仍为空，应检查 pending_target/status，而不是提前要求 Connected。

补充：继承已有 Console 的 database/schema；从 Explorer 来源创建；目标 Session 已连接时不增加 Connect；相同目标 connecting 时不增加 Connect；重复名称不建连也不创建文档；无有效目标不发 Connect。

**Step 2 — 执行回归。**

```bash
cargo +1.94.0 test --test console_manager_input
```

预期旧实现缺少 Connect 导致新契约失败。

**Step 3 — 接入已有准备函数。**

创建函数尾部替换为：

```rust
self.create_sql_editor_named(name, origin_target);
self.active_tab = self.tabs.len().saturating_sub(1);
self.focus = Focus::Editor;
let mut commands = self.prepare_active_console_target();
commands.push(self.persist_workspace_command());
commands
```

不修改低层 `create_sql_editor_named()` 的职责，避免恢复或其他仅创建文档的路径隐式执行 I/O。检查连接复用分支中的 `activate_profile_workspace()` 不会替换新标签或重置焦点；使用 UUID 断言新标签仍为活动标签。

**Step 4 — 验证创建、标签和已有命名测试。**

```bash
cargo +1.94.0 test --test console_manager_input --test workspace_tabs
cargo +1.94.0 test --lib console
```

预期通过；只更新真实改变的“离线创建”预期，不批量放宽命令断言。

**Checkpoint:** `fix(console): prepare target connection on creation`。

### Task 3：统一启动动作并连接恢复的当前 Console

**Files:**
- Modify: `src/action.rs`、`src/app.rs`、`src/runtime.rs`。
- Test: `tests/startup_profiles.rs`、`tests/workspace_tabs.rs`、`src/runtime.rs` 的测试模块。

**Step 1 — 添加启动决策测试。**

覆盖：无显式参数且活动 Console 有目标；恢复多个 profile 和多个后台 Console；显式 profile 优先；直接 URL 的现有测试；无工作区；活动非 Console；无目标；已删除或不支持的 profile；保存的非默认 database/schema。

同时断言：恢复 SQL/Console UUID 不变，只产生活动目标的 Connect，不产生 SQL 执行命令，不创建额外默认 Console。对显式目标不同于恢复 Console 的情况，验证现有显式工作区选择语义，确保不会隐式改绑该恢复文档。

**Step 2 — 增加一个内部 Action。**

建议命名 `PrepareActiveConsole`，由 `App::update()` 调用 `prepare_active_console_target()`；不改变焦点，不关闭 overlay，不执行 SQL。此次选择 Console 专用 Action，避免扩大 Relation/RedisBrowser 的启动自动加载范围。

在 runtime 提取共享动作选择函数，逻辑如下：

```rust
fn startup_action(app: &App, selected: Option<Uuid>) -> Option<Action> {
    if let Some(profile_id) = selected {
        return Some(Action::RequestProfileConnect { profile_id });
    }
    app.active_console_opt()
        .map(|_| Action::PrepareActiveConsole)
}
```

目标合法性由 App 已有连接准备流程负责；无目标时返回空命令。`apply_startup_action()` 与实际 Runtime 入口调用同一函数，前者更新 App，后者经 `apply_action()` 派发 Command。

**Step 3 — 保持恢复为纯状态重建。**

不在 `restore_workspace()`、render 或 Tick 里发 Connect。使用 run_tui 现有首帧前位置派发，确保 Runtime 和工作区存储已就绪。普通恢复可保留当前启动 Explorer 焦点规则；新建 Console 则仍聚焦 Editor。连接成功不得异步抢焦点。

**Step 4 — 运行启动与恢复测试。**

```bash
cargo +1.94.0 test --test startup_profiles --test workspace_tabs
cargo +1.94.0 test --lib startup
```

预期启动决策和实际返回命令都有覆盖，避免只检查 Connecting 状态而遗漏命令派发。

**Checkpoint:** `fix(startup): connect the restored active console target`。

### Task 4：处理连接中的最新 Console 激活意图

**Files:**
- Modify: `src/app.rs`，准备函数、成功/失败/取消/失效处理、关闭标签与断开相关入口。
- Test: `tests/connection_switch.rs`、`tests/workspace_tabs.rs`。

**Step 1 — 建立可控 Action 序列测试。**

先复用现有测试夹具，以手动回传 ConnectionSucceeded/ConnectionFailed 的方式控制顺序，不靠 sleep。至少覆盖：

1. A connecting → 激活 B → A success → B 产生一次 Connect。
2. A connecting → B → C → A success，只准备仍然活动的 C。
3. A connecting → B → A → A success，不再准备 B。
4. A connecting → B → A failure，仍可准备 B；不重试失败的 A。
5. 等待中的 B 关闭、改绑目标、profile 删除或切换到非 Console，意图失效。
6. 主动断开/取消后迟到的 A 结果不触发回连。
7. 相同目标的两个 Console 不重复连接，错误按目标可见。
8. B 已存在 Connected Session，A 完成后直接复用 B。

**Step 2 — 添加一个最新待准备意图。**

推荐 App 私有字段 `deferred_console_activation: Option<DeferredConsoleActivation>`，内容至少为 `console_id`、完整 `target` 和正在等待的 `ConnectionIdentity`。它与现有 `pending_editor_target_switch` 职责不同：前者表示下一步激活请求，后者仍标识当前连接结果的归属。

不同目标连接中时，用最新请求替换待准备意图，返回空连接命令；不再仅提示“等待”后永久丢弃请求。相同目标继续依赖 SessionRegistry 去重，不重置同一次连接尝试的开始时间。

**Step 3 — 完成事件中一次性消费意图。**

只有当前被接受的连接完成事件与等待 identity 匹配，才检查意图。消费前同时验证：活动 Console UUID、目标仍相等、profile/target 有效、用户未断开/取消、事务约束仍满足。

先 take 意图，再调用准备函数；没有待处理意图时不无条件重新准备活动 Console。这样失败保持稳定，不形成“失败 → prepare → 再失败”的无限循环。后台/过期结果不得推进意图。

**Step 4 — 防止完成事件抢回活动目标。**

审查 ConnectionSucceeded 中 `should_activate_workspace` 和 `should_project_connection`，以及 Session 已连接的快速路径。旧 A 的完成可以登记 Session 和更新 A 的 Explorer 状态，但不能恢复 A 标签、覆盖当前编辑器目标或清除属于新 generation 的 pending。

如现有完成处理会先抢回标签，使用明确的当前请求归属校验修正；不要通过事后强制切回标签掩盖错误。

**Step 5 — 清理失效意图并验证。**

在关闭目标标签、切换到非 Console、目标改绑、删除 profile、主动 disconnect/cancel、退出/重启路径清理相应意图。连接失败仍允许消费用户之前明确提出的其他目标激活意图。

```bash
cargo +1.94.0 test --test connection_switch --test workspace_tabs
cargo +1.94.0 test --lib session
```

预期所有时序测试通过，重复或迟到响应不影响当前上下文。

**Checkpoint:** `fix(console): preserve latest activation during connection changes`。

### Task 5：统一目标状态与可操作的失败反馈

**Files:**
- Modify: `src/app.rs`、`src/ui/mod.rs`、`src/action.rs`（重试 Action）。
- Modify: `src/commands.rs` 或命令模块中实际注册 Console 操作的文件，按现有注册模式提供重试命令。
- Test: `tests/ui_render.rs`、`tests/connection_switch.rs`。

**Step 1 — 为状态归属添加测试。**

两个不同 execution_target：全局 connection 指向 A，活动 Console 为 B，分别令 B Connecting/Connected/Failed/Absent。断言 B 的显示依赖 B Session；同 profile 不同 database/schema 也必须隔离。

**Step 2 — 提取活动 Console 目标展示投影。**

以 Console.execution_target、profiles 合法性、SessionRegistry 和匹配当前目标的 tab.target_error 为输入，输出无目标、目标不可用、离线、连接中、就绪、失败的状态。UI 不启动连接。

状态优先级：无目标/目标无效 → 当前目标连接中 → 当前目标已连接 → 当前目标失败 → 离线。重试中不继续展示旧 TARGET ERROR；连接成功清理匹配目标的过时错误。保留现有 READY/CONNECTING/OFFLINE 等术语，复用已有 ActivityIndicator。

不要将纯 UI 的 READY 当作新的执行授权；现有执行路径的目标/identity/事务检查仍应通过回归测试。

**Step 3 — 增加失败详情及显式重试。**

提供 `RetryActiveConsoleConnection` 内部 Action，重新准备当前 Console 的准确目标。通过现有命令注册系统暴露“Retry console connection”，在 Console 失败提示中给出该命令名称；如现有目标菜单已有等价操作则复用，不新增冲突快捷键。

失败信息放在 Console 自身的状态/结果区域中，保留编辑器与现有结果，不依赖短暂 Toast 才能找到错误。对终端文本使用现有清理方法，不展示凭据。连续重试 Connecting 目标只复用同一次尝试；重试成功不自动重放 SQL。

**Step 4 — 验证交互与目标隔离。**

```bash
cargo +1.94.0 test --test ui_render --test connection_switch
```

检查 80 列终端状态可读、长错误可截断或按现有结果区方式查看、离线编辑/选择/复制正常。若命令注册修改影响命令测试，执行对应测试目标。

**Checkpoint:** `fix(console): show target session status and retry failures`。

### Task 6：补充真实 Runtime 与持久化闭环回归

**Files:**
- Test: `tests/connection_switch.rs`、`tests/startup_profiles.rs`、`tests/workspace_tabs.rs`。
- Reference: `src/runtime.rs`、`src/model/transaction.rs`。

**Step 1 — 使用已有 Runtime 夹具与 SQLite 临时文件。**

验证：创建 Console → Connect dispatch → 收到成功 Action → 对应 Session Connected；保存非空 SQL → 新 App 恢复 → 启动动作派发 → 同一 Console UUID/SQL/target → Connected。

测试隔离 workspace 文件和 profile registry，不使用开发者真实数据库或原有 workspace。

**Step 2 — 验证失败与事务边界。**

用可控连接失败夹具验证失败后 SQL 不丢失，显式重试为新 generation，迟到旧事件被丢弃。保存 Manual 模式的 Console 后恢复应为 Manual/Idle，不自动 BEGIN、COMMIT、ROLLBACK 或执行保存的 SQL。

**Step 3 — 验证退出/断开和后台恢复。**

主动断开后后续 Tick/渲染不产生 Connect；恢复多个 Console 只为当前目标建连；切换后台标签才发起其目标连接。已有自动执行/分页等命令只在原有明确操作条件下产生。

**Step 4 — 运行相关集成测试集合。**

```bash
cargo +1.94.0 test --test console_manager_input --test startup_profiles --test workspace_tabs --test connection_switch --test ui_render
```

预期全部通过。测试记录明确区分 mock Action 验证和真实 SQLite Runtime 验证。

**Checkpoint:** `test(console): cover auto-connect restore and lifecycle boundaries`。

### Task 7：全量检查与人工验收

**Files:**
- Update: 本计划，记录实际实现、检查结果及必要偏差。
- Modify: 现有 Console/工作区用户文档的对应章节（实施时定位实际页面，说明活动恢复与后台按需连接）。

**Step 1 — 对齐 CI 执行检查。**

依据 `.github/workflows/ci.yml`：

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

预期格式、Clippy、测试全部通过。如果本机缺少工具链或驱动依赖，记录具体阻塞；定向测试通过不能描述为全量 CI 通过。数据库服务测试按项目现有环境运行，不要求为本改动新增全部驱动的基础设施。

**Step 2 — 人工重现截图场景。**

1. 无活动连接，在 Explorer 对选中 profile 新建 Console。
2. 确认 Console 立即完整显示，状态从 CONNECTING 转为 READY。
3. 输入 SQL，不执行，退出并普通重启。
4. 确认同一 Console、SQL、database/schema 被恢复并自动连接。
5. 模拟失败，确认完整 Console、错误详情与重试操作可用。
6. 快速创建/切换不同目标，确认最终目标生效且焦点不被旧响应抢走。
7. 主动断开，确认 Console 保留且不会立即回连。
8. 关闭所有标签，确认欢迎页正常；在 80×24 下复测主要状态。

**Step 3 — 检查改动与文档。**

```bash
git diff --check
git status --short
git diff --stat
```

确认计划之外的已有文件未被覆盖。文档需明确：连接成功不自动执行 SQL；恢复当前 Console 自动连接，后台按需连接；失败可显式重试。

**Checkpoint:** `docs(console): document active console auto-connect behavior`。

## 4. 最终完成标准

- [ ] 用户提出的新建与重启两个场景均自动请求正确目标。
- [ ] 完整 Console 不受全局 disconnected 条件遮挡。
- [ ] 连接中可编辑 SQL；失败后可定位错误并显式重试。
- [ ] 相同目标无重复连接；不同目标按 profile/database/schema 隔离。
- [ ] 快速切换只推进最新有效意图，迟到事件不抢回标签与焦点。
- [ ] 主动断开、取消、关闭和删除操作不会触发意外回连。
- [ ] 无目标与无标签场景各有正确展示。
- [ ] 恢复 SQL、UUID、目标与事务模式，连接成功不执行 SQL。
- [ ] 定向测试、Runtime 闭环、CI 检查和人工验收结果已记录。

## 5. 实施记录

当前状态：已在 `task/console-auto-connect` worktree 中完成任务 1—7 的实现与逐项复核。全量检查通过；其中一条既有 UI 国际化测试已改为同时接受中文和英文等价文案。

提交建议仅作为每个逻辑任务的检查点；实施时按用户的提交要求操作，精确暂存本任务涉及的文件。
