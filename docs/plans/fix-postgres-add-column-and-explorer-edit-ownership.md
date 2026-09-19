# Catalog Editor Add Column and Explorer Edit Implementation Plan

> **执行负责人：Luna。** 按端到端验收单元连续实施；实现、审查、纠偏、提交合并均由 Luna 完成。Astra 只负责分析与计划。不启动子 Agent，不要求用户逐项 resume。

**Goal:** 修复两个表结构编辑问题：(1) PostgreSQL 上新增一列提交不再报错；(2) Explorer 中在任意已连接连接的表节点上按 `e` 都能打开结构编辑器，不再要求先为该连接启动 console。

**Architecture:** 问题一：把“新增列的位置”按后端能力对齐——PostgreSQL 无 `ADD COLUMN FIRST/AFTER`，新增列一律追加到末尾，并在提示中显式说明；草稿的 `database_kind` 改为真实连接类型。问题二：把目录编辑的连接所有权从“前台连接”改为“所选 catalog 对象所属 profile 的已连接会话”，供 help 能力判定、`OpenCatalogEdit`、Preview/Apply 与异步结果校验共用；复用 `SessionRegistry`，与既有 `redis_connection_for_target` 同源。

**Tech Stack:** Rust 2024 / Rust 1.94、Crossterm 0.29、Ratatui 0.30、现有 App reducer / Command runtime / SessionRegistry、各数据库适配器、Rust 单元与集成测试。

---

## 0. 基线、产物与执行约定

- 工作空间：`/Users/yelog/workspace/tui/lazydb`；目标 `main`。
- state.json 记录起点 `8ec2584fc0e60f5240b33563c0e3531578128eeb`；计划核验时实际 HEAD 为 `2ee009a77545a2671ffac7c40a37478abbdf984b`（Luna 已合并上一 Redis 任务），`8ec2584` 是其祖先，两个问题均不由该合并引入。
- 任务目录：`.git/opencode-tasks/ses_f4c325064ffeAuRPpGdUDUgMMl`；分析文件为本目录 `analysis.md`；完整计划同步保存到本目录 `plan.md`。
- 任务名、分支和工作树由后续流程/Luna 分配；本阶段不改业务代码、不提交。
- 截至 plan 开始 checkpoint.json 不存在。实现/恢复时读取当时 checkpoint、实际 diff 与本计划，不照搬旧日志 next；不修改 state/checkpoint，不覆盖历史回执，只写当阶段 activeReceipt。
- 工作区有 7 个既有未跟踪 `docs/plans/2026-09-18-*-implementation.md`，均属其他任务，保留且不带入提交。本文件是本任务新增计划。
- 分析阶段基线：`catalog_editor_state` 65 passed、`catalog_editor_reducer` 33 passed、`object_mutation_contract` 26 passed（旧代码证据，不得冒充实现后通过）。
- 按“建立必要回归 → 实现 → 定向验证 → 记录结果 → 下一单元”推进；普通编译错误自行修复。发生新修改或失败时才重跑相关检查，不反复全量 check/clippy/test。无工作流授权时不擅自提交/合并。

### 验证要求的来源与等级

1. **用户需求（必须满足）**：PostgreSQL 新增列提交成功；Explorer 中任意已连接连接的表节点按 `e` 能打开编辑器并完成编辑提交。
2. **项目强制门禁**：`.github/workflows/ci.yml` 的 Rust 1.94 `fmt`、`clippy --all-targets --all-features`、`test --all-targets --all-features`。
3. **本计划选定的自动化证据**：模型级新增列位置回归、PostgreSQL 规划器回归、双连接 reducer/keymap 回归。
4. **补充验证（非必需门禁）**：真实 PostgreSQL 写入、PTY/人工观察。不得访问用户真实 `lssc-uat` 作为 fixture；环境受限时记录未执行。

## 1. 已确定的产品行为与后端能力

### 1.1 新增列位置

| 后端 | 新增列行为 | 依据 |
|---|---|---|
| MySQL / MariaDB | 按所选位置插入，生成 `ADD COLUMN ... FIRST/AFTER` | `src/db/mysql.rs:617-720` |
| SQLite | 按所选位置插入，必要时走保真重建 | `src/db/sqlite.rs:454,500-615` |
| PostgreSQL | **一律追加到末尾**；不支持插入到已有列之前 | 无 `FIRST/AFTER` 语法，本任务不改用整表重建 |
| SQL Server / Oracle | 当前表编辑仅支持改名，列变更仍被拒绝（既有边界，不在本任务扩展） | `src/db/mssql.rs:414-430`、`src/db/oracle.rs:334-352` |

PostgreSQL 上 `A`(上方新增) 与 `a`(下方新增) 都追加到末尾；UI 提示统一表述为“新增到末尾”，避免用户以为会插在中间。J/K 调整已有列顺序在 PostgreSQL 仍会在 Review 阶段被明确拒绝（既有行为，本任务不改），最终摘要中如实披露。

### 1.2 Explorer `e` 连接所有权

`e` 的可用性与执行都改为依据**所选 catalog 对象的 profile**解析其已连接会话（`SessionRegistry` 中该 profile 的 `Connected` 会话，优先 `target.database` 与对象 database 一致者）。没有匹配的已连接会话时视为不可编辑，不退回到无关前台连接。目录编辑打开后，定义加载、Preview、Apply 及异步结果校验都使用该对象所属连接，不要求它是前台连接。

## 2. 单元一：PostgreSQL 新增列闭环

**修改文件**
- `src/profile.rs`：`DatabaseKind` 新增能力方法。
- `src/model/catalog_editor.rs`：`TableDraft::begin_add_column_at`。
- `src/app.rs`：载入表定义时按连接类型构造草稿（约 `7446-7450`）。
- `src/ui/catalog_editor.rs`：`table_shortcut_hints`（约 `2100-2134`）。
- 测试：`tests/catalog_editor_state.rs`、`tests/catalog_mutation.rs`。

### Step 1.1：建立失败/基线回归

在 `tests/catalog_editor_state.rs` 新增模型用例（沿用既有 Table fixture 构造）：

1. `postgres_new_columns_append_to_the_end`：PostgreSQL 草稿选中中间列，分别 `begin_add_column_above()` / `begin_add_column_below()`，填 name/type 后 `confirm_column_details()`；断言新列位于 `columns.last()`，且中间位置的其他列顺序不变。
2. `mysql_new_columns_keep_the_requested_position`：MariaDB 草稿同样操作，断言仍插入到所选行上方/下方（保护既有能力不被单元一误伤）。

在 `tests/catalog_mutation.rs` 新增规划器用例（无服务）：

3. `postgres_table_edit_appends_added_columns`：构造 `events(id,name,score)` baseline，草稿先按 PostgreSQL 追加语义新增 `age`，调用 `PostgresAdapter::plan_catalog_mutation`，断言 `Ok` 且语句含 `ADD COLUMN "age"`。
4. `postgres_table_edit_rejects_reordered_existing_columns`：手工构造已有列相对顺序变化的草稿，断言仍返回 `InvalidDraft`（确认没有放宽真正的乱序拒绝）。

运行：
```bash
cargo +1.94.0 test --test catalog_editor_state postgres_new_columns
cargo +1.94.0 test --test catalog_mutation postgres_table_edit_appends
cargo +1.94.0 test --test catalog_mutation postgres_table_edit_rejects
```
预期：用例 1/3 在实现前因“中间插入”而失败（PostgreSQL 追加断言不成立 / 规划器返回 `InvalidDraft`）；用例 2 通过。记录实际失败输出，不预先宣称已复现。

### Step 1.2：按后端能力对齐新增位置

`src/profile.rs` 的 `impl DatabaseKind` 增加：

```rust
/// Whether a newly added column can be placed at a chosen position.
pub const fn places_new_columns(self) -> bool {
    matches!(self, Self::MySql | Self::MariaDb | Self::Sqlite)
}
```

`src/model/catalog_editor.rs::begin_add_column_at`（约 `2265`）改为：

```rust
fn begin_add_column_at(&mut self, insert_at: usize) {
    if self.column_editor.is_some() {
        return;
    }
    let insert_at = if self.database_kind.places_new_columns() {
        insert_at
    } else {
        self.columns.len()
    };
    let mut column = ColumnDraft::new_added_for_database(self.database_kind);
    column.ordinal_position = self
        .columns
        .iter()
        .map(|column| column.ordinal_position)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    self.column_editor = Some(TableColumnEditSession {
        target: TableColumnEditTarget::New { insert_at },
        draft: column,
        error: None,
    });
    self.focus = TableEditorFocus::ColumnDetails(TableColumnField::Name);
}
```

### Step 1.3：草稿使用真实连接类型

`src/app.rs` 载入表定义处（约 `7446-7450`）把

```rust
let mut draft = crate::model::catalog_editor::TableDraft::from_definition(&table);
```

改为按 `request.connection.profile_id` 找到 profile 的类型：

```rust
let connection_kind = self
    .profiles
    .iter()
    .find(|profile| profile.id == request.connection.profile_id)
    .map(|profile| profile.kind);
let mut draft = match connection_kind {
    Some(kind) => crate::model::catalog_editor::TableDraft::from_definition_for_database(
        &table,
        kind,
    ),
    None => crate::model::catalog_editor::TableDraft::from_definition(&table),
};
```

这同时修正非 PostgreSQL 连接新增列默认类型取自错误后端的问题。

### Step 1.4：提示与能力边界

`src/ui/catalog_editor.rs::table_shortcut_hints` 在 `TableEditorFocus::Columns` 分支依据 `draft.database_kind.places_new_columns()` 选择文案：

```rust
let (add_below, add_above) = if draft.database_kind.places_new_columns() {
    ("add below", "add above")
} else {
    ("add (append)", "add (append)")
};
```

非定位后端不再暗示可插入中间；其余 hints（j/k、J/K、e、dd、r、Esc）保持不变。

### Step 1.5：定向验证

```bash
cargo +1.94.0 test --test catalog_editor_state
cargo +1.94.0 test --test catalog_mutation
cargo +1.94.0 test --test object_mutation_contract
cargo +1.94.0 test --test ui_render table_editor
```
预期：全部通过；PostgreSQL 任意所选位置新增列只生成末尾 `ADD COLUMN`，MySQL/MariaDB 位置能力不变。

### Step 1.6：Luna 单元审查

检查真实 diff：`places_new_columns` 只影响新增位置，不改 J/K 调序与既有列比较；`from_definition_for_database` 未改变已有列读取；未放宽 PostgreSQL 乱序拒绝。建议提交 `fix(catalog): append new columns on postgres`。

## 3. 单元二：Explorer 目录编辑连接所有权

**修改文件**
- `src/app.rs`：新增会话解析辅助；`Action::OpenCatalogEdit`（约 `7114-7236`）；`CatalogObjectDefinitionLoaded/LoadFailed`（约 `7410-7537`）；`CatalogOwnerContextLoaded/Failed`（约 `7088-7112`）；`Action::CatalogEditorPreview`（约 `8446-8551`）；`Action::CatalogEditorApply`（约 `8552-8594`）；`CatalogMutationSucceeded/Failed`（约 `8658-8743`）。
- `src/help.rs`：`catalog_editor_capabilities`（约 `3135-3174`）。
- 测试：新增 `tests/catalog_editor_ownership.rs`；必要时复用 `tests/catalog_editor_reducer.rs` fixture。

### Step 2.1：建立失败回归

新增 `tests/catalog_editor_ownership.rs`，建立两个 profile（A=PostgreSQL 语义、B=Redis 或 SQLite），按真实 reducer 路径 `RequestConnect` + `ConnectionSucceeded`（带各自 `mutation_capabilities`）连接，再连 B 使其成为前台，最后把 A 的表节点设为 Explorer 选中项，并插入 A 的 database/schema/table 目录项。

1. `explorer_edit_maps_when_owner_is_not_foreground`：断言 `Keymap::default().map(Char('e')) == Some(OpenCatalogEdit)`。
2. `open_catalog_edit_dispatches_owner_definition_load`：`app.update(Action::OpenCatalogEdit)` 产生 `Command::LoadCatalogObjectDefinition(request)`，且 `request.connection.profile_id == A`、`request.target` 为 A 的目标数据库。
3. `owner_definition_load_is_accepted_when_not_foreground`：注入 `Action::CatalogObjectDefinitionLoaded`（request 与上一步一致），断言 `catalog_editor` 进入表单且 `draft` 为 Table。
4. `owner_plan_apply_succeeds_when_not_foreground`：构造/注入 `CatalogMutationPlanReady`，调用 `CatalogEditorApply`，断言产生 `Command::ExecuteCatalogMutation`；注入 `CatalogMutationSucceeded`，断言编辑器关闭并刷新 A 的目录。
5. `single_connection_edit_still_works`：单连接前台时上述流程仍通过（防回归）。
6. 断言非法情况仍被拒绝：不属于该 profile 的 id、缺少 A 的已连接会话、陈旧 `catalog_epoch`。

运行：
```bash
cargo +1.94.0 test --test catalog_editor_ownership -- --nocapture
```
预期：实现前用例 1-4 失败（`e` 得到 `None` 或 handler 返回空），用例 5 通过；记录实际失败输出。

### Step 2.2：新增只读会话解析

在 `src/app.rs` 增加（放在 `database_command_identity` 附近，约 `17163`）：

```rust
/// Session that owns catalog edits for `profile_id` (optionally constrained to `database`).
pub(crate) fn catalog_edit_session(
    &self,
    profile_id: Uuid,
    database: Option<&str>,
) -> Option<crate::model::session::SessionState> {
    let connected = |identity: crate::identity::ConnectionIdentity| {
        self.sessions
            .get_by_identity(identity)
            .filter(|session| {
                session.identity.profile_id == profile_id
                    && session.status == crate::model::session::SessionStatus::Connected
            })
    };
    let mut sessions = self
        .sessions
        .iter()
        .filter(|session| {
            session.identity.profile_id == profile_id
                && session.status == crate::model::session::SessionStatus::Connected
        })
        .collect::<Vec<_>>();
    if let Some(database) = database {
        if let Some(session) = sessions
            .iter()
            .find(|session| session.target.database == database)
        {
            return Some((*session).clone());
        }
        return None;
    }
    sessions
        .pop()
        .cloned()
        .or_else(|| {
            self.explorer
                .catalog_sessions
                .get(&profile_id)
                .copied()
                .and_then(connected)
        })
}

/// True when `identity` is the foreground connection or a live Connected session.
pub(crate) fn session_is_connected(&self, identity: crate::identity::ConnectionIdentity) -> bool {
    self.connection.active_identity() == Some(identity)
        || self
            .sessions
            .get_by_identity(identity)
            .is_some_and(|session| {
                session.status == crate::model::session::SessionStatus::Connected
            })
}
```

同时从 `CatalogId` 提取对象 profile 与 database 的小工具（或在调用点内联）：

```rust
fn catalog_object_database(id: &crate::db::catalog::CatalogId) -> Option<String> {
    id.native_path
        .first()
        .filter(|value| value.as_str() != "__role__")
        .cloned()
}
```

### Step 2.3：`e` 可用性与执行改用所属会话

`src/help.rs::catalog_editor_capabilities` 的 `edit` 分支：把

```rust
let capabilities = &app.connection.mutation_capabilities;
matches!(selected, ExplorerNodeId::Catalog(_))
    && capabilities.can_edit(&anchor, entry).unwrap_or(false)
```

改为按节点 profile/database 解析会话能力：

```rust
let edit = matches!(selected, ExplorerNodeId::Catalog(_))
    && app
        .catalog_edit_session(profile_id, database.as_deref())
        .is_some_and(|session| session.mutation_capabilities.can_edit(&anchor, entry).unwrap_or(false));
```
其中 `database` 由 `ExplorerNodeId::Catalog(id)` 的 `catalog_object_database(id)` 得到（`__role__` 视为 None）。

`src/app.rs::Action::OpenCatalogEdit` 的 `ExplorerMutationIntent::Edit(anchor)` 分支改为以对象 profile 解析：

```rust
let profile_id = object.profile_id();
let Some(profile) = self
    .profiles
    .iter()
    .find(|profile| profile.id == profile_id)
    .cloned()
else {
    self.notify_warning("Catalog", "The active connection profile is missing");
    return Vec::new();
};
if profile.read_only {
    self.notify_warning("Catalog", "Catalog editing requires a writable profile");
    return Vec::new();
}
let database = catalog_object_database(object);
let Some(session) = self.catalog_edit_session(profile_id, database.as_deref()) else {
    self.notify_warning(
        "Catalog",
        "The connection for the selected catalog object is not available",
    );
    return Vec::new();
};
let connection = session.identity;
let target = session.target.clone();
// 保留既有 entry / id-kind / can_edit / target.is_valid 校验，改为查 profile_id 的目录；
// 删除 `connection != self.connection.active_identity().unwrap()` 前台限制。
```
`editor.database_kind` 由 `profile.kind` 设置；`LoadCatalogObjectDefinition` 仍使用解析出的 `connection`/`target`。

### Step 2.4：Preview / Apply 使用所属会话

`Action::CatalogEditorPreview`（约 `8518-8542`）：

```rust
let (connection, current_database) = match &anchor {
    CatalogMutationAnchor::Catalog(id) => {
        let database = catalog_object_database(id);
        let Some(session) =
            self.catalog_edit_session(id.profile_id(), database.as_deref())
        else {
            self.notify_warning("Catalog", "The connection for the selected catalog object is not available");
            return Vec::new();
        };
        (session.identity, session.target.database)
    }
    _ => {
        let Some(connection) = self.database_command_identity() else {
            return Vec::new();
        };
        (
            connection,
            self.connection
                .target
                .as_ref()
                .map(|target| target.database.clone())
                .unwrap_or_default(),
        )
    }
};
```
`CatalogMutationRequest::new(connection, ...)` 后 `.with_current_database(current_database)`。Create（非 Catalog anchor）保持原前台逻辑。

`Action::CatalogEditorApply`（约 `8574`）把

```rust
if self.connection.active_identity() != Some(plan.request.connection) {
    return Vec::new();
}
```

改为 `if !self.session_is_connected(plan.request.connection) { return Vec::new(); }`。

### Step 2.5：异步结果按所属会话校验

把以下位置的“前台连接相等”改为“活的已连接会话”：

- `CatalogObjectDefinitionLoaded`（约 `7414`）与 `CatalogObjectDefinitionLoadFailed`（约 `7520`）：`self.database_command_identity() == Some(request.connection)` → `self.session_is_connected(request.connection)`。
- `CatalogOwnerContextLoaded/Failed`（约 `7090/7109`）：同样替换，保证表 owner 选择在非前台连接下可用。
- `CatalogMutationSucceeded`（约 `8679`）：`self.connection.active_identity() == Some(plan.request.connection)` → `self.session_is_connected(plan.request.connection)`。
- `CatalogMutationFailed`（约 `8726`）：`identity_matches` 改为 `self.session_is_connected(plan.request.connection)`。

`catalog_epoch`、`accepts_definition_request`、`plan` 与 `request_id` 匹配、`definition_matches_request` 等既有校验全部保留；只放宽“必须恰好是前台连接”。

### Step 2.6：定向验证

```bash
cargo +1.94.0 test --test catalog_editor_ownership -- --nocapture
cargo +1.94.0 test --test catalog_editor_reducer
cargo +1.94.0 test --test keymap
cargo +1.94.0 test --lib help::tests
```
预期：全部通过；前台为无关连接时 `e` 仍映射并打开 A 的编辑器、加载定义、可提交；单连接与既有 reducer/help 行为不回归。

### Step 2.7：Luna 单元审查

检查：解析是否始终绑定对象 profile（不使用全局 fallback 绕过）；`__role__`/database 级对象的 database 处理；异步结果不会因前台切换被丢弃或误接收；`catalog_epoch`/`request_id`/owner 校验未削弱；未把 create 流程误改成按对象解析。建议提交 `fix(catalog): open explorer edits on the owning connection`。

## 4. 完整验证与收尾

### 4.1 联合场景

1. 打开 PostgreSQL 表编辑器 → 选中中间列 → 新增列 → Review/提交成功，新列出现在末尾；MySQL/MariaDB 新增仍按所选位置。
2. 前台为 Redis/另一 SQL 连接时，Explorer 选中 PostgreSQL 表按 `e` → 编辑器打开并显示结构；编辑一列/新增一列 → Review → Apply 成功；目录刷新。
3. 单连接前台时上述两流程与既有测试一致。

### 4.2 强制门禁（功能齐备后统一执行一次）

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```
每条记录命令、退出码、用例数量/跳过项、HEAD/diff 状态、Rust 版本。失败修复后只重跑受影响检查。数据库条件测试缺失服务时明确记录“未执行”，不冒充通过。

### 4.3 补充证据

- 在可用的一次性 PostgreSQL/MySQL 测试库上验证真实 round-trip（不得使用用户真实 `lssc-uat`）。
- PTY/人工观察用于补充真实终端显示，不作为用户强制门禁；环境受限最多一次针对性修复重试，再由 Luna 收尾审查决定补证或记录限制。

### 4.4 最终验收清单

- [ ] PostgreSQL 任意所选位置新增一列都能提交成功，生成末尾 `ADD COLUMN`；MySQL/MariaDB 位置能力不变。
- [ ] PostgreSQL 已有列乱序仍被明确拒绝，未静默改序。
- [ ] 非前台已连接连接的表节点按 `e` 能映射、打开、加载定义、Preview、Apply 并刷新。
- [ ] 无匹配已连接会话时不回退到无关前台连接；返回可操作提示。
- [ ] 单连接与既有 help/keymap/reducer 测试无回归。
- [ ] 最终代码通过项目检查；受限检查明确记录。Luna 完成 diff 审查与任务流程规定的提交合并，保留其他任务文件。

## 5. 当前计划阶段证据

- 本阶段未修改业务代码，未运行实现测试；仅读取当前 git 状态/HEAD、analysis.md、CONTRIBUTING.md 与相关代码。
- 分析阶段识别并复现了两个根因：`a2a32f9` 引入的 PostgreSQL 位置守卫（中间新增报错）与 `catalog_editor_capabilities`/`OpenCatalogEdit` 的前台连接所有权（`e` 无反应）。
- 下一步由 Luna 在分配工作树中，从单元一的失败回归开始推进，再完成单元二与统一门禁。
