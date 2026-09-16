# Redis Browser 离线焦点自动连接 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行约束以自动任务指令为准：本阶段只生成计划；插件安排后续阶段、命名任务分支和管理 worktree，不启动子 Agent。若后续环境未提供上述技能，按本计划逐项执行，不虚构技能调用。

**Goal:** 启动后所有连接离线时，将焦点从 Explorer 移入恢复的 Redis Browser tab，即自动连接该 tab 的 Redis 目标并加载 keys 树。

**Architecture:** 在现有 Console 焦点准备 helper 中增加 Redis Browser 分派，并将 Redis 专用焦点入口接入该 helper。复用 `ensure_redis_browser_loaded`、会话连接去重、连接成功回调和 SCAN 批次更新流程。保持按用户激活惰性连接及当前扫描状态策略。

**Tech Stack:** Rust 2024 / Rust 1.94、App Action/Command 状态机、Redis SCAN、Cargo 集成测试；新增回归测试不依赖真实 Redis 服务。

---

## 输入、范围与执行顺序

- 分析依据：同目录 `analysis.md`，已在 plan 阶段读取。
- 原工作空间：`/Users/yelog/workspace/tui/lazydb`。
- 起点：`714eadc269b96b440950f5fe53d5942061665d5b`；目标分支：`main`。
- 任务分支待插件调用 Luna 命名，当前为空正常；实施在插件指定的工作空间进行。
- 预计修改：`src/app.rs`、`tests/redis_loading_lifecycle.rs`。
- 只读回归参照：`tests/redis_browser_tabs.rs`、`tests/workspace_tabs.rs`、`src/input/mouse.rs`、`.github/workflows/ci.yml`。
- 依赖顺序：任务 1 → 任务 2 → 任务 3 → 任务 4。每一步单独完成和复核，测试失败时先定位失败原因。
- 行号对应分析基线，实施时按符号定位；当前 plan 阶段不改业务或测试代码。

## 任务 1：建立离线恢复焦点进入的失败回归

**Files**
- Modify/Test：`tests/redis_loading_lifecycle.rs`。
- Reference：`tests/workspace_tabs.rs:108-281` 的 WorkspaceSnapshot fixture 和 Console 焦点测试。

### 步骤 1：新增恢复 fixture

在测试文件添加所需 imports，提供 `restored_redis_for_focus_test(database: u32)`，返回 `(App, ExecutionTarget, Uuid)`。fixture 使用真实 `restore_workspace`，而非手工伪造 Connected 状态。建议代码：

```rust
fn restored_redis_for_focus_test(database: u32) -> (App, ExecutionTarget, Uuid) {
    use lazydb::persistence::workspace::{
        PersistedProfileWorkspace, PersistedTab, WorkspaceSnapshot,
    };
    use lazydb::model::workspace::Focus;

    let profile = lazydb::profile::import_connection_url(
        "redis://localhost:6379/0", Some("restored-cache"),
    ).unwrap().profile;
    let target = ExecutionTarget {
        profile_id: profile.id,
        database: database.to_string(),
        schema: None,
    };
    let tab_id = Uuid::new_v4();
    let snapshot = WorkspaceSnapshot {
        active_profile: Some(profile.id),
        profiles: vec![PersistedProfileWorkspace {
            profile_id: profile.id,
            active_tab: Some(tab_id),
            consoles: Vec::new(),
            tabs: vec![PersistedTab::RedisBrowser {
                tab_id,
                profile_id: profile.id,
                database,
                pattern: b"user:*".to_vec(),
            }],
        }],
        active_console: Uuid::nil(),
        consoles: Vec::new(),
        tabs: Vec::new(),
        sql: Vec::new(),
        recent_targets: Vec::new(),
    };
    let mut app = App::new(vec![profile]);
    app.restore_workspace(snapshot, None);
    app.focus = Focus::Explorer;
    (app, target, tab_id)
}
```

复核：fixture 恢复的活动 tab ID 与返回值一致、连接 Disconnected、pending_target 为空、keyspace 未加载。不要用硬编码 tab 下标；每次取 `app.active_tab` 或按 ID 查找。

### 步骤 2：添加焦点入口测试

新增 `restored_redis_focus_entries_request_connection_once`，对每个 case 建立全新 fixture，遍历数据库 0 和 3，分别验证：

| Action | 预期内部 pane |
| --- | --- |
| FocusNext | Keys |
| FocusPrevious | Preview |
| Focus(Results) | 保留 fixture 的 Keys |
| RedisFocusPane(Keys) | Keys |
| RedisFocusPane(Preview) | Preview |

每次执行 `app.update(action)` 后，断言：

- `app.focus == Focus::Results`，内部 pane 符合表格。
- 返回命令中恰好一个 `Command::Connect`，profile_id 与 target 完整相等（包含 database 和 schema）。
- `app.connection.pending_target == Some(target)`，对应 Explorer profile 的 status 为 Linking。
- 没有 `Command::ScanRedisKeys`；原 tab ID、pattern 保持不变。

对命令按变体筛选，而不是断言整个命令 Vec 只能有一个元素，避免把持久化/元数据等合法附带命令误判为失败。

### 步骤 3：运行并记录预期失败

```bash
cargo test --test redis_loading_lifecycle restored_redis_focus_entries_request_connection_once -- --exact
```

预期：当前业务代码下 Connect 数量为 0，测试失败。若是编译失败，修正 fixture/API 使用后再确认行为失败；不可把编译失败作为缺陷复现。

## 任务 2：接入统一的焦点进入准备流程

**Files**
- Modify：`src/app.rs:15135`，`prepare_console_after_focus_change`。
- Modify：同文件 `Action::FocusNext`、`FocusPrevious`、`Focus`、`RedisFocusPane` 和旧 helper 调用点。
- Test：任务 1 新增测试。

### 步骤 1：扩展并重命名 helper

使用 `prepare_active_tab_after_focus_change` 名称，替换原 helper。建议实现主体：

```rust
fn prepare_active_tab_after_focus_change(&mut self, previous_focus: Focus) -> Vec<Command> {
    if previous_focus != Focus::Explorer
        || !matches!(self.focus, Focus::Editor | Focus::Results)
    {
        return Vec::new();
    }
    if self.active_console_opt().is_some() {
        return self.prepare_active_console_target();
    }
    if self.focus == Focus::Results
        && matches!(self.tabs.get(self.active_tab), Some(WorkspaceTab::RedisBrowser(_)))
    {
        return self.ensure_redis_browser_loaded(self.active_tab, false);
    }
    Vec::new()
}
```

复核：Console 分支原有事务与 pending 逻辑仍由 `prepare_active_console_target` 负责；Redis 走 `false` 自动加载策略；其他 tab 返回空命令。

### 步骤 2：接入键盘焦点循环

- 在 `Action::FocusNext`、`Action::FocusPrevious` 开始改变焦点前保存 `let previous_focus = self.focus;`，删除分支下方重复的同名声明。
- Redis 的 Explorer → Keys / Preview 专用分支设置 focus 后，将原 `return Vec::new()` 改为 `return self.prepare_active_tab_after_focus_change(previous_focus);`。
- Redis 已处于 Results 时的 Keys ↔ Preview ↔ Explorer 内部循环保持原有返回行为。
- 通用循环结尾调用新 helper。

注意：调用 helper 前结束对 tab 的可变借用，不在调用后继续使用该引用；按 Rust NLL 规则保持局部修改即可。

### 步骤 3：接入直接聚焦与鼠标共用入口

- `Action::Focus` 改为调用新 helper。
- `Action::RedisFocusPane` 保存 previous_focus，设置 Results 和内部 pane 后返回新 helper 的命令。
- 旧 helper 其余调用点（基线 `9514` 的鼠标文本光标、`13682` 的 GridSelect）改为新名称。
- 全文复核旧符号已无引用；`src/input/mouse.rs` 的 Action 映射继续复用，不新增鼠标层连接逻辑。

### 步骤 4：运行焦点回归

```bash
cargo test --test redis_loading_lifecycle restored_redis_focus_entries_request_connection_once -- --exact
cargo test --test redis_browser_tabs redis_browser_focus_cycles_explorer_keys_preview -- --exact
cargo test --test workspace_tabs focus
```

预期：新用例通过，Redis pane 循环与 Console 聚焦测试通过。此时不更改 `open_redis_browser`、`request_connection_target_inner` 或 SCAN 实现。

## 任务 3：验证连接成功后自动加载及幂等边界

**Files**
- Modify/Test：`tests/redis_loading_lifecycle.rs`。
- Reference：`src/app.rs` 的 `ConnectionSucceeded`、`ConnectionFailed`、`RedisKeysLoaded` 分支和现有生命周期测试。

### 步骤 1：添加成功链路测试

新增 `restored_redis_focus_connects_then_populates_key_tree`：

1. 使用 database=3 的恢复 fixture，通过 FocusPrevious 进入 Preview。
2. 从实际 Connect 命令提取 generation，不使用固定 generation。
3. 构造 `Action::ConnectionSucceeded`：profile_id 为 fixture 目标，generation 为上一步值，server.kind=Redis、version="7.2"、database="3"、current_user=None，mutation_capabilities=Default::default()。
4. 从回调命令取出唯一 `ScanRedisKeys(request)`；断言 identity.owner_id=原 tab_id、identity.target 是 db3、pattern 是 `user:*`、position 为 Start。
5. 用该 identity 回送 `Action::RedisKeysLoaded(KeyScanBatch { identity, keys: vec![b"user:1".to_vec()], next: ScanPosition::Complete })`。
6. 断言原 tab 仍活动，Keys 树包含 `KeyTreeNodeId::Key(b"user:1".to_vec())`，keys 中出现该 key，状态 Complete；Explorer 已不处于 Offline/Linking/Failed，Preview 焦点保持。

不要要求回调命令 Vec 仅包含 Scan，也不要 mock 掉 App::update；这是整个业务状态转换的回归。

### 步骤 2：增加连接与扫描幂等测试

新增 `restored_redis_focus_reentry_deduplicates_loading`：首次聚焦取 Connect 后，Focus(Explorer) 再 FocusNext 多次，累计 Connect 仍只有一次，且成功回调之前无 Scan。随后按步骤 1 注入成功事件，确认恰好一个 Scan；在 Loading 时重复聚焦和 Keys/Preview 内部切换，确认没有新增 Connect/Scan。

### 步骤 3：增加完成与失败边界测试

- `restored_redis_focus_preserves_completed_scan`：分别用空批次和非空批次结束扫描，验证 CompleteEmpty / Complete 状态再次聚焦不重扫，不清空树或移动已有选择。
- `restored_redis_focus_connection_failure_does_not_scan`：从真实 Connect 捕获 generation，用现有 `Action::ConnectionFailed` 的字段构造连接失败事件；断言没有 Scan、keys 树仍未加载、连接错误进入既有失败反馈。再次主动聚焦是否重试按现有 request 语义处理，不添加自动后台重试。
- 若需覆盖 Scan 失败，使用发出的 scan identity 注入 RedisKeysFailed，再入焦点不自动重试，验证 `explicit_open=false` 的约束。

### 步骤 4：补足普通标签激活的离线对照

新增 `restored_redis_tab_activation_requests_connection`，用相同恢复 fixture 执行 `ActivateTab(app.active_tab)`，断言正确 Connect。现有 NextTab/PreviousTab 用例继续保留，用于验证标签激活路径没有回退。无需给所有既有测试重复加入同样断言。

### 步骤 5：运行定向测试

```bash
cargo test --test redis_loading_lifecycle --test redis_browser_tabs --test workspace_tabs
```

预期：全部通过。分析阶段原 Redis 两个测试文件共 27 项通过；本次新增数量以实际测试统计为准。若成功回调、目标或幂等用例暴露已有未预料的状态问题，先定位并记录；只修复实现此需求所必需的问题，再重跑受影响用例。

## 任务 4：复核、项目检查与交付

**Files**
- Review：`src/app.rs`、`tests/redis_loading_lifecycle.rs`。
- Reference：`.github/workflows/ci.yml:81-83`。

### 步骤 1：审查差异

```bash
git diff --check
git diff --stat
git diff -- src/app.rs tests/redis_loading_lifecycle.rs
```

复核清单：

- 所有已列出的合法焦点入口都能产生准备命令，命令被调用者返回给 runtime。
- Redis Profile/database 始终来自 tab.target，不来自 Explorer 当前选择。
- FocusPrevious / RedisFocusPane(Preview) 的内部焦点不会被重置成 Keys。
- 新 helper 仅在 Explorer → 主内容区域时触发；启动 restore 和普通绘制没有增加连接。
- 连接及 SCAN 的 generation、owner_id、去重由现有机制维护，没有平行实现。
- 自动加载仍传 false，CompleteEmpty、Loading、Failed、Paused 的策略保留。
- 已连接成功后结果写入原 tab；重复聚焦不会创建新 tab。
- 工作区没有无关修改；未修改 state.json、工作流状态或 worktree 生命周期文件。

### 步骤 2：执行 Rust 检查

先对实际修改文件按项目格式处理，检查 formatter 是否产生无关历史格式差异；不要批量接纳无关变化。随后执行 CI 对齐命令：

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

预期：退出码均为 0，无新增 lint，全部测试通过；环境跳过的数据库测试据实记录。若工具链或外部依赖缺失，记录准确命令和阻塞，不宣称通过。全量测试用于此共享 App 焦点逻辑变更的最终 CI 对齐检查，通过后无需反复重复运行。

### 步骤 3：用户路径验收（有可用 Redis 时）

1. 打开 Redis db0 Browser，退出并重新启动以恢复 workspace，确认初始连接离线。
2. 从 Explorer 通过正向焦点切换进入 Keys，观察对应 profile 建连后 keys 自动出现。
3. 再次离线恢复，通过反向焦点切换进入 Preview，确认同样加载 keys 且 Preview 保持焦点。
4. 用鼠标聚焦内容区域验证一致行为；在 Keys/Preview 间重复切换不重载已完成树。

该冒烟测试不要求新部署 Redis；无服务时以状态机回归验证并明确未做真实服务验收。

### 步骤 4：阶段交付与提交边界

汇总修改文件、根因修复点、测试命令/结果以及未执行项目。后续插件如安排提交阶段，将这一个局部修复和相应回归作为一个逻辑提交，建议消息：`fix(redis): connect restored browser tabs on focus`。当前 plan 阶段不提交、不建分支、不执行本计划中的实现步骤；后续各阶段回执使用插件当次提供的 token。

## 最终验收标准

1. 完全离线恢复的 Redis Browser tab，通过五种焦点入口均请求正确 profile/database 的一次连接。
2. Explorer 正常展示连接中与连接成功状态，无需用户先手工展开连接。
3. ConnectionSucceeded 自动发起正确 tab/pattern 的 SCAN；合法结果自动形成当前 tab keys 树。
4. 恢复 tab 的 ID、数据库、pattern 以及进入的 Keys/Preview pane 保持正确。
5. 连接中和扫描中反复聚焦不重复请求；空数据库扫描完成后保持 CompleteEmpty；失败状态遵循既有重试策略。
6. Console 聚焦自动连接、Redis 原 pane 循环和标签切换测试全部通过。
7. 变更局限于焦点准备与业务回归测试，相关定向测试及项目检查结果已记录。

## Plan 阶段记录

- 已读取 analysis.md，并调用 writing-plans 技能。
- 已检查测试 fixture 结构、SCAN batch 示例及项目 CI 验证命令。
- 未改业务或测试代码，未执行实现阶段命令，未修改 state.json，未启动子 Agent。
- 无阻塞；完整计划直接保存于用户指定路径，由插件继续安排实施。
