# Console 恢复后按焦点延迟连接 Implementation Plan

**Goal:** 普通启动恢复当前 Console 时保持离线，用户主动聚焦该 Console 后才准备其数据库连接。

**Architecture:** 启动入口只处理显式选择的连接。App 的用户焦点入口在从 Explorer 进入 Console 时调用既有 `prepare_active_console_target`，继续复用 SessionRegistry 的目标识别、去重、事务保护及延后激活机制。主动 tab 激活沿用现有准备路径。

**Tech Stack:** Rust 1.94、Action/Command reducer、Tokio runtime、现有 Rust 单元及集成测试。

---

## 上下文和阶段约束

- 分析依据：同目录 `analysis.md`。
- 仓库：`/Users/yelog/workspace/tui/lazydb`。
- 目标分支：`main`；分析基线：`6fc85a946e0c291abb2d02d71fe710bd97768331`。
- 名称、任务分支及 worktree 由插件管理；实施阶段使用插件分配的目录，不自行命名或切换分支。
- 当前只编写计划，不执行下列代码修改、测试或提交，不修改 `state.json`，不启动子 Agent。
- 本次正式 plan 阶段回执：同目录 `plan-44730f62-473c-48e4-a955-90693acdd443.json`；完成计划复核后写入指定 token、stage 和 completed 状态。
- 已按正式阶段要求先读取 `analysis.md` 并调用 `writing-plans` 技能；用户已选择自动工作流继续实施，无需询问执行方式。
- 以下行号来自分析基线；实施时按函数和 action 名定位。

## 明确的行为契约

1. 无显式 startup selected 时，不因恢复当前 Console 而连接。
2. Explorer → 当前 Console 的 Editor 或 Results 是连接触发点；Editor ↔ Results 是同一 Console 内部切换，不重复准备。
3. 有效主动 Console tab 激活同时进入 Editor，并沿已有 tab 路径准备目标。
4. 无效鼠标事件、无目标 Console、非 Console 焦点变化不新增连接。
5. 同目标 Connected/Connecting 复用现有会话；目标比较使用完整 ExecutionTarget。
6. 显式 CLI selected、主动执行 SQL、Console 管理器激活等已有明确使用意图继续有效。
7. 不添加 startup 标记、持久化字段、配置开关或每帧连接扫描，不引入离开焦点即取消连接的机制。

## Task 1：修改启动连接契约

**Files**
- Modify: `src/runtime.rs::startup_action`、`apply_startup_action`、`apply_startup_action_with_runtime`（约 6033–6052）。
- Test: `tests/workspace_tabs.rs`（约 108–151 的旧启动测试）。
- Test: `src/runtime.rs` 的私有函数单元测试。

### Step 1：改写恢复启动回归测试

将 `restored_active_console_can_prepare_its_target_without_explicit_startup_selection` 改名为 `restored_active_console_stays_offline_without_explicit_startup_selection`。

保留现有 snapshot 构造，恢复后设置 `app.focus = Focus::Explorer` 以模拟真实 `run_tui`。保存原始 connection status，调用公开 `apply_startup_action(&mut app, None)`，断言：

- connection status 保持原值，pending generation/target 为空。
- active Console ID、SQL `select 42` 和完整 execution target 保留。
- 焦点仍是 Explorer。

此测试只验证启动，不提前调用焦点 action，以便故障定位清晰。

### Step 2：确认旧实现失败

```sh
cargo +1.94.0 test --test workspace_tabs restored_active_console_stays_offline_without_explicit_startup_selection
```

预期旧实现因 Connecting/pending 状态断言失败；若工具链或编译环境阻塞，报告真实原因，不将环境错误当成回归测试红灯。

### Step 3：移除 startup fallback

私有函数调整为：

```rust
fn startup_action(selected: Option<Uuid>) -> Option<Action> {
    selected.map(|profile_id| Action::RequestProfileConnect { profile_id })
}
```

两个调用方改为 `startup_action(selected)`，保留公开 `apply_startup_action` 接口。保留 `Action::PrepareActiveConsole` 及其 reducer 分支。

### Step 4：直接验证命令选择契约

在 runtime 单元测试中分别断言：

- `startup_action(None)` 返回 None。
- `startup_action(Some(id))` 匹配 `Some(Action::RequestProfileConnect { profile_id })` 且 ID 相同。

测试模块使用唯一名称 `startup_console_connection_tests`，避免依赖公开启动 helper 丢弃返回命令的行为。

### Step 5：验证本任务

```sh
cargo +1.94.0 test --lib startup_console_connection_tests
cargo +1.94.0 test --test workspace_tabs --test startup_profiles
```

预期全部通过。不要将其他测试中合理的显式连接断言改为离线。

## Task 2：键盘聚焦时准备 Console 目标

**Files**
- Modify: `src/app.rs` 的 Focus / FocusNext / FocusPrevious 分支（约 5745–5819）。
- Modify: `src/app.rs` 中 `prepare_active_console_target` 邻近位置。
- Test: `tests/workspace_tabs.rs`。

### Step 1：添加行为测试

按 Task 1 的恢复场景创建独立 App，启动焦点设为 Explorer。测试以下 action，每个 action 使用独立初始状态：

- `Action::Focus(Focus::Editor)`。
- `Action::Focus(Focus::Results)`。
- `Action::FocusNext`。
- `Action::FocusPrevious`。

检查返回命令中恰好有一个 Connect，目标与 Console target 相符，pending target 正确，最终焦点位于 Console。不要只断言 Connecting，以免遗漏命令未传回 runtime 的错误。

若需共享 fixture，提取仅供这些测试使用的恢复 Console helper；不重构整份测试文件。

### Step 2：运行新增测试确认失败

建议名称统一包含 `restored_console_focus`，运行：

```sh
cargo +1.94.0 test --test workspace_tabs restored_console_focus
```

旧焦点分支返回空命令，应在 Connect 断言失败。

### Step 3：加入窄范围辅助函数

在 App impl 中添加：

```rust
fn prepare_console_after_focus_change(&mut self, previous_focus: Focus) -> Vec<Command> {
    if previous_focus == Focus::Explorer
        && matches!(self.focus, Focus::Editor | Focus::Results)
        && self.active_console_opt().is_some()
    {
        self.prepare_active_console_target()
    } else {
        Vec::new()
    }
}
```

三个焦点 action 在修改焦点前保存 previous_focus，完成原有 clear/normalize 后返回该函数的命令。保留 RedisBrowser 特殊循环分支原有行为。不得在 helper 外另行判断全局 connection status 或自行构造 Connect。

### Step 4：添加负向和重复进入验证

- Focus(Explorer) 不连接。
- Editor → Results、Results → Editor 不新增准备命令。
- 连接中返回 Explorer 再进入同目标不产生第二个 Connect。
- 无 target Console 不连接。
- target 指向不存在的 profile 或已失效的数据库配置时，不发 Connect。
- 无 tab 和非 Console tab 的焦点操作不触发 Console Connect。
- 恢复后保持 Explorer，处理 viewport 更新或周期事件不发 Connect。

### Step 5：运行验证

```sh
cargo +1.94.0 test --test workspace_tabs
```

预期新增正反用例及既有 tab 行为通过。

## Task 3：覆盖鼠标聚焦与主动 tab 激活

**Files**
- Modify: `src/app.rs::update` 的 SetEditorMouseCursor（约 9481–9511）、GridSelect（约 13669–13673）、ActivateTab（约 5735–5743）。
- Test: `tests/mouse.rs`。
- Test: `tests/workspace_tabs.rs`。
- Reference: `src/input/mouse.rs:610–633`，`src/app.rs::mouse_session_focus`。

### Step 1：添加鼠标特殊路径回归

复用现有鼠标测试中的 session/revision/position 构造方式。测试应包含：

- 有效编辑区点击实际映射到 SetEditorMouseCursor，从 Explorer 进入 Editor 并返回一个 Connect，光标位置仍正确。
- 有效 Console 输出文本点击进入 Results 并准备相同目标。
- revision 过期或 session 不属于当前 tab 时不连接。
- pane 点击仍走 Focus 路径，无需额外重复准备。

鼠标映射测试不能只停留在 action 类型断言；至少一个用例将 action 交给 App::update 并断言 Connect。

### Step 2：接入鼠标焦点 helper

SetEditorMouseCursor 保存原焦点，仅在现有 session/revision/set_mouse_cursor 成功分支完成后调用 helper 并返回命令；失败继续返回空列表。保留原 Visual mode、补全清理和 Redis preview 状态操作。

GridSelect 保存原焦点，执行原 clear、设置 Results、select_grid 后返回 helper 命令。

为 GridSelect 增加独立 reducer 测试，验证 Explorer → Console Results 的命令和最终网格选择；不要仅用普通 Focus(Results) 测试代替这条路径。

### Step 3：对齐主动 tab 激活语义

增加测试：Explorer 焦点下 ActivateTab 指向 SQL Console（包括已经可见的当前 Console），结果进入 Editor 且准备该目标一次；非法 index 不产生任何准备命令。

有效 ActivateTab 分支将 `normalize_focus()` 替换为 `normalize_focus_after_tab_switch()`，保留已有 `prepare_active_tab()`，不要额外调用焦点 helper。

检查非 Console 激活回归，确保 Relation / RedisBrowser 仍使用已有准备路径。

### Step 4：验证

```sh
cargo +1.94.0 test --test mouse --test workspace_tabs
```

预期鼠标点击与键盘操作语义一致，既有鼠标光标及 tab 测试通过。

## Task 4：验证多目标会话和异步边界

**Files**
- Test: `tests/global_workspace.rs`。
- Test: `tests/connection_switch.rs` 或 `tests/workspace_tabs.rs`，根据已有 fixture 选择最小合适位置。
- Reference: `src/model/session.rs::request`。
- Reference: `src/app.rs::prepare_active_console_target`、`request_connection_target_inner`、DeferredConsoleActivation 成功处理。

### Step 1：检查并复用已有测试设施

先定位现有 ConnectionSucceeded/Failed action 构造及多 target 会话 fixture。不要为这些 reducer 行为启动外部数据库，也不要手工设置部分全局连接字段伪装完整已连接会话。

### Step 2：增加真正覆盖新入口的边界用例

1. Console A 对应目标已 Connected，Explorer → Console：没有新 Connect，目标绑定正确。
2. 同目标处于 Connecting，反复进入：无第二个 Connect。
3. 当前活动连接是 B，恢复 Console 保持 A：进入 Console 后准备 A，不使用 Explorer 选中的 B；若 A 已存在则复用。
4. B 正在连接时进入 A：先不重复/抢发连接，B 成功后现有 deferred 路径继续准备 A。
5. 请求失败后，失败事件自身不自动重连；离开并重新进入才触发新尝试。
6. 手动事务处于非 Idle 时保留既有保护；同目标正常已连接分支不应被新 helper 改写。

以返回的 Connect 数量、完整 target、generation 及必要的错误/状态断言验证实际契约；避免只断言私有 helper 被调用。

### Step 3：运行目标测试

```sh
cargo +1.94.0 test --test workspace_tabs --test startup_profiles --test mouse --test global_workspace --test connection_switch
```

如果现有会话测试已经覆盖底层机制，新增测试只需覆盖焦点入口与该机制的衔接，不复制整套连接生命周期测试。

## Task 5：完整检查和交付

### Step 1：审阅 diff

确认业务变更限定为启动选择及用户焦点/Console 激活路径；没有持久化格式、驱动、凭据、state.json 或非必要重构变更。核对所有新增返回的 Command 均能经 App::update 传到 Runtime::dispatch。

### Step 2：运行仓库检查

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

预期退出码为 0。若格式检查失败，执行 formatter 后重跑相关检查；环境依赖阻塞必须在交付中明确记录，不宣称全套通过。

### Step 3：人工验收（具备测试环境时）

恢复一个远程数据库 Console，启动停留 Explorer：不出现该目标 Linking 或连接凭据提示。仅移动 Explorer 选择不连接；键盘/鼠标进入 Console 才连接。再次进入同目标不重复连接。显式 `--profile` 启动仍连接指定目标。

如果无法执行真实 TUI 验收，明确记录未执行，保留自动化行为测试结果作为主要验证证据。

### Step 4：交付记录

记录修改文件、关键行为、测试命令及实际结果。提交与后续阶段回执遵循插件提供的阶段要求；只有收到对应 token/路径且阶段要求全部完成后，才写该阶段回执。不要使用 analyze 回执作为 plan 或实施回执。

## 完成标准

以下为后续实施验收标准，不表示计划阶段已经修改代码或通过测试：

- 普通恢复启动不发隐式 Console Connect。
- 键盘和鼠标首次进入恢复 Console 均自动准备正确目标。
- 同目标已连接/连接中不重复 Connect。
- 显式启动连接、其他 tab 和现有会话保护没有回归。
- 相关行为测试及仓库检查结果已记录。

## 计划复核记录

- 已对照 `analysis.md` 核对根因、启动入口、四类焦点 action、鼠标专用路径及 tab 激活路径。
- 每项任务列出了具体文件、实施步骤、验证命令和预期结果；覆盖显式启动选择、失效目标、非用户事件、会话去重、跨目标延后激活和事务保护。
- 任务依赖顺序为 Task 1 → Task 2 → Task 3 → Task 4 → Task 5；所有步骤由插件安排的后续实施阶段顺序执行。
- 本阶段仅更新任务目录内的计划文档和完成回执；未修改业务代码、未执行上述测试命令、未修改 state.json、未创建分支/worktree、未启动子 Agent。

当前计划文档已完成；实施由插件的下一阶段执行。
