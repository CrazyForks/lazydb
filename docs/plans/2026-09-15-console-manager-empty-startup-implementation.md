# Consoles 空启动交互修复 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
>
> 执行适配：若环境没有上述技能，按本文顺序逐项实施。默认在当前会话顺序执行；仅在用户要求提交时创建 Git commit。本文代码为拟实施代码，行号以 2026-09-15 分析时为准，执行时按符号定位。

**Goal:** 让没有活动 SQL Console 的启动状态下，Consoles 面板可以正常新增、关闭、搜索和管理已有记录，并保持新增 Console 无需连接数据库。

**Architecture:** 保留 Keymap → Action → App::update → Command 路径，用一个集中的 Action 分类明确 Console 管理操作不依赖活动 Console，并从旧保护条件中清除其重复枚举。沿用现有创建、工作区、命名校验、事务保护和持久化处理函数，以真实按键与状态转换测试验证修复。

**Tech Stack:** Rust 2024 / Rust 1.94、Crossterm、现有 App reducer 与 Cargo 测试；无需新增依赖。

---

## 1. 已确认的根因与边界

| 位置 | 现状 |
| --- | --- |
| `src/input/keymap.rs:774–786` | Browse 模式把 `a`、`/`、Esc 正确映射为管理面板 Action |
| `src/app.rs:3653–3974` | 无活动 Console 的保护条件对面板操作存在重复且冲突的分类 |
| `src/app.rs:3670–3673` | `NewConsole`、打开面板和按 UUID 激活被直接放行 |
| `src/app.rs:3949–3968` | 新增、取消、搜索、移动等面板 Action 被列为受限操作 |
| `src/app.rs:5421–5426` | 正常创建后关闭面板，但故障路径没有到达这里 |
| `src/app.rs:14798–14835` | 创建过程准备工作区、创建 Tab、聚焦编辑器并生成持久化命令 |
| `src/app.rs:26480–26495` | 已有新增测试先创建活动 Console，遗漏真正的空启动条件 |

根因触发条件是 `active_console_opt().is_none()`；未连接只是截图中的启动背景。Relation、Dashboard、关闭全部 Console 等状态也需要覆盖。

此次采用集中分类加局部修改，不搬迁全部 `App::update()` 分支。这样消除当前重复分类，同时让改动保持易于审查。

## 2. 验收契约

1. 有 profile、未连接、无 Tab：打开面板后按 `a` 创建 `console_1`，关闭面板，焦点进入 Editor。
2. 创建返回工作区持久化命令，不返回连接或 SQL 执行命令；连接状态不因新增而变化。
3. 新 Console 的执行目标沿用 `console_manager_origin_target` 和现有默认目标规则。
4. 空列表按 Esc 关闭；搜索模式按 Esc 先退出搜索，再按 Esc 关闭面板。
5. 空列表移动、激活、重命名、删除不产生 panic、不虚构记录，面板仍可退出。
6. 关闭全部 Console 后，可管理已有关闭记录，并通过现有激活流程重新打开。
7. Relation / Dashboard 作为活动页面时，面板管理行为与 Console 页面一致。
8. SQL 编辑和执行等确实依赖活动 Console 的操作，仍受既有保护。
9. 重命名冲突、空名称、删除确认、未决事务等既有约束继续生效。

## 3. Task 1：增加能稳定失败的空启动回归测试

**Files:**
- Create: `tests/console_manager_input.rs`
- Reference: `tests/omni_input.rs`（Keymap 驱动方式）
- Reference: `tests/startup_profiles.rs`（profile 构造与启动状态）

### Step 1：建立测试夹具

通过 `import_connection_url("postgresql://user@127.0.0.1:1/app", Some("offline"))` 创建 profile，再调用 `App::new(vec![profile])` 和既有启动选中逻辑。只驱动 App，不实例化 Runtime、不执行返回的 Command。

每个空启动测试在开始时明确断言：

```rust
assert!(app.active_console_opt().is_none());
assert!(app.tabs.is_empty());
assert!(app.connection.profile_id.is_none());
```

如构造器当前默认选中行为需要调整，使用 `reveal_startup_profile(None)`，不能先调用 `NewConsole`，也不能构造一个“已连接”状态来绕开问题。

按键驱动统一使用：

```rust
let action = keymap
    .map(KeyEvent::new(code, KeyModifiers::NONE), &app)
    .expect("console manager should map this key");
let commands = app.update(action);
```

### Step 2：编写三个核心行为测试

- `console_manager_empty_startup_creates_offline_console`：打开面板、按 `a`；断言 Tab 数增加、名称为 `console_1`、目标为所选 profile、焦点为 Editor、overlay 为 None、产生 `PersistWorkspace`，并且所有返回命令都是此创建路径预期的持久化命令。
- `console_manager_empty_startup_escape_closes_overlay`：打开面板、按 Esc；断言 overlay 为 None、Tab 仍为空。
- `console_manager_empty_startup_search_can_be_cancelled`：按 `/`、输入字符，断言 Search 模式及查询值；第一次 Esc 回 Browse，第二次 Esc 关闭。

### Step 3：运行红灯测试

```bash
cargo test --test console_manager_input -- --nocapture
```

预期：在当前实现中，分别因没有新增 Tab、overlay 未关闭、未进入 Search 而失败。测试必须快速完成，失败原因不能是网络连接或等待超时。

## 4. Task 2：集中管理 Action 分类并修复保护条件

**Files:**
- Modify: `src/action.rs`（新增 `impl Action` 分类方法）
- Modify: `src/app.rs`（`App::update` 的无活动 Console 保护条件）
- Test: `tests/console_manager_input.rs`

### Step 1：添加单一分类入口

在 `src/action.rs` 为 Action 增加以下方法：

```rust
impl Action {
    /// Console management operates on records and overlays, not an active editor.
    pub(crate) fn is_console_management_action(&self) -> bool {
        matches!(
            self,
            Self::NewConsole
                | Self::NewConsoleNamed(_)
                | Self::OpenSqlEditorList
                | Self::SqlEditorListMove(_)
                | Self::SqlEditorListActivate
                | Self::SqlEditorListCreate
                | Self::SqlEditorListDeleteRequest
                | Self::SqlEditorListDeleteConfirm
                | Self::SqlEditorListDeleteActivate
                | Self::SqlEditorListDeleteCancel
                | Self::SqlEditorListDeleteFocusNext
                | Self::SqlEditorListDeleteFocusPrevious
                | Self::SqlEditorListSearchStart
                | Self::SqlEditorListRenameStart
                | Self::SqlEditorListRenameCommit
                | Self::SqlEditorListInputInsert(_)
                | Self::SqlEditorListInputBackspace
                | Self::SqlEditorListInputDeletePreviousWord
                | Self::SqlEditorListInputDeleteToStart
                | Self::SqlEditorListInputDelete
                | Self::SqlEditorListInputMoveLeft
                | Self::SqlEditorListInputMoveRight
                | Self::SqlEditorListInputMoveHome
                | Self::SqlEditorListInputMoveEnd
                | Self::SqlEditorListInputUndo
                | Self::SqlEditorListInputRedo
                | Self::SqlEditorListCancel
                | Self::ActivateSqlEditor(_)
        )
    }
}
```

### Step 2：接入现有保护条件

在 `self.active_console_opt().is_none()` 之后加入：

```rust
&& !action.is_console_management_action()
```

该例外只针对“缺少活动 Console”的保护条件。保留原有 Action 处理分支及其面板模式、记录存在性和事务检查。

### Step 3：删除重复枚举

在同一个无活动 Console 的复合条件中，从以下三处删除上述方法已覆盖的全部 Action：

1. 顶部直接放行列表。
2. Relation / Dashboard 等非 Console 页面例外列表。
3. 末尾受限操作列表。

逐项审查删除范围，仅清理这个前置条件中的管理 Action 枚举。保留 `match action` 的业务分支，以及 `RequestDeleteActiveConsole` 等真正以活动对象为上下文的操作。

不要为验证枚举完整性再复制一份全量 Action 测试表；后续使用行为测试覆盖分类是否正确。

### Step 4：运行核心回归

```bash
cargo test --test console_manager_input -- --nocapture
cargo test --lib console_manager
```

预期：空启动三个测试通过，既有 Console 管理单元测试通过。若新增产生了意外连接命令，检查是否错误引入激活流程，恢复使用现有 `create_and_activate_sql_editor`。

## 5. Task 3：覆盖空列表、关闭记录与非 Console 页面

**Files:**
- Modify: `tests/console_manager_input.rs`
- Modify: `src/app.rs` 内现有 Console 管理测试模块（需要内部 workspace / relation / transaction 夹具的用例）
- Reference: `src/model/sql_editor_list.rs`

### Step 1：增加空列表容错用例

测试空列表下 `j`、`k`、Enter、`r`、`d` 后仍无 Tab，无新增或删除命令；随后 Esc 能退出。增加无 profile 的空启动新增用例，先明确验证无活动 Console，再断言生成未绑定目标的 Console。

### Step 2：增加关闭记录管理用例

通过现有 NewConsole 和 CloseActiveTab 流程制造“有持久记录、无活动 Tab”，不要删除记录。分别测试：

- 管理面板 Enter 重开相同 UUID 的 Console，SQL 文本保留。
- 面板内对关闭记录重命名，记录名及持久化内容更新。
- 删除确认和取消正常；取消保留记录，确认删除目标记录。

每个用例操作前断言 `active_console_opt().is_none()`。重开已有记录按现有连接策略验证，不把“新增不连接”的契约扩大为所有激活操作均不连接。

### Step 3：增加非 Console 页面用例

复用 `src/app.rs` 测试模块中的 Relation / Dashboard 构造方式，各覆盖一次打开面板并新增，以及取消面板后保留原活动页面。

多 profile 时补充一个上下文测试：在 Explorer 选中第二个 profile 后打开管理器，新增 Console 的 `execution_target.profile_id` 应来自该上下文，而非任意活动连接或字母排序首项。

### Step 4：保护既有边界

补充无活动 Console 时发送 `EditorKey` 和 `RunActiveSql` 的行为断言：不创建 Console、不生成执行命令、不 panic。复用现有命名和事务测试，确认集中分类没有跳过处理函数内的保护。

### Step 5：运行相关测试

```bash
cargo test --test console_manager_input --test startup_profiles --test omni_input
cargo test --lib console_manager
cargo test --lib resolving_transaction_for_manager_delete_restores_delete_confirmation
```

预期：全部通过。若失败涉及原有工作区/事务行为，先确认是否由分类改动引起，再针对实际失败修复；不要通过放宽断言或提前创建活动 Console 消除失败。

## 6. Task 4：完整检查与终端验收

**Files:**
- Reference: `.github/workflows/ci.yml:81–83`
- Update: 本计划的执行记录（实施完成时）

### Step 1：执行格式、静态检查和全量测试

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
git diff --check
```

这次修改的是共享 Action 入口，因此在定向测试通过后执行一次全量 Rust 检查。若本地缺少工具链或依赖，记录具体阻塞；外部数据库测试未实际运行时单独注明，不把跳过记录为覆盖成功。

### Step 2：人工终端验收

使用测试配置启动 `cargo run`，配置至少一个未连接的 profile，并使初始工作区无打开的 Console。不要对真实用户 Console 文件做清理来制造状态。

按以下顺序验收：

1. 启动后检查未连接且无活动 Console。
2. 用现有 Consoles 快捷键（Space → s）打开面板，Esc 关闭。
3. 再次打开，按 `/` 输入搜索，连续两次 Esc 分别退出搜索和面板。
4. 再次打开，按 `a`，检查编辑器出现并可立即输入 SQL。
5. 检查未因为新增而启动连接；输入后关闭所有 Console，再次打开管理器，重开原记录，确认 SQL 保留。
6. 检查原有 Console 名称冲突和删除取消流程。

核心验收不依赖实际数据库在线。已有可用测试数据库时，再验证执行 SQL 仍沿用现有连接与执行流程。

### Step 3：审查和交付

确认 diff 的业务改动集中在 `src/action.rs` 与 `src/app.rs` 的保护条件，测试补齐此前缺失的无活动 Console 状态。

交付记录包含：修复位置、新增行为测试列表、实际运行的命令与结果、终端验收结果及未执行项目。用户要求提交时，建议将核心修复与回归测试作为一个原子提交：

```text
fix(console): allow console management without an active console
```

## 7. 依赖与完成标准

执行顺序：Task 1 红灯复现 → Task 2 分类修复与绿灯 → Task 3 边界回归 → Task 4 完整检查及人工验收。

- [ ] 空启动按键测试在修复前稳定失败。
- [ ] 管理 Action 的无活动 Console 例外只维护一处。
- [ ] 新增、Esc、搜索、关闭记录管理测试通过。
- [ ] 未连接新增只产生预期持久化命令。
- [ ] 目标继承、非 Console 页面、编辑器保护、事务约束验证通过。
- [ ] 格式、Clippy、全量测试结果已记录。
- [ ] 终端能复现并验证修复后的完整交互。

**当前状态：仅完成实施计划；上述修改与检查尚未执行。**
