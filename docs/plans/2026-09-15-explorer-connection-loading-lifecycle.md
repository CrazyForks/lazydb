# Explorer 多连接目录加载生命周期实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行环境若没有该技能，则按本文任务顺序实施并逐项验证；不要假定技能可用。

**Goal:** 修复快速打开多个连接时 Explorer 永久 SYNCING、展开后空白及 o/Enter 无法恢复的问题，使后台连接可以独立完成目录加载。

**Architecture:** 复用 SessionRegistry、ConnectionIdentity、CatalogRequestKey 和按 profile 保存的 Explorer 状态。所有目录请求显式传递归属，连接节点通过幂等入口确保目录可用；数据回写与前台工作区激活分离，展示状态由有效会话及实际目录任务推导。Redis 复用生命周期规则，保留独立的数据发现实现。

**Tech Stack:** Rust 2024 / Rust 1.94、Tokio、Ratatui、现有 Action → App::update → Command → Runtime 事件流、cargo test。

---

## 0. 范围、约束和执行方式

- 每个任务按“补充行为回归测试 → 确认失败原因 → 实现 → 定向验证 → 审查 diff”的顺序完成。
- 任务中的新测试名为建议名称；复用现有 fixture，不复制完整 App 初始化体系。
- 本文引用的行号来自计划编写时的源码，实施时以函数、事件名称定位。
- 本次不改变数据库适配器的查询语义、不增加新的通用任务调度框架。
- 保留多连接能力、现有预加载范围、合法空目录缓存和 SQL 事务退出约束。
- 不使用禁用切换、防抖、每次展开重连等方式规避竞态。
- 不在后台异步路径中通过临时修改 selected/active_profile 来给请求传参。
- 改动涉及的 pending、epoch、request ID 都是运行时状态，不写入持久化工作区。
- 任务完成后可按建议提交边界提交；提交需遵循实际执行时的用户指令。

## 1. 已确认的代码依据

| 问题 | 位置 | 实施要求 |
|---|---|---|
| Databases 不携带 profile | `src/db/catalog.rs:731` | 保留领域 target，内部调用显式传 connection |
| 根请求优先从光标推断归属 | `src/app.rs:start_catalog_request`，17445 | 移除内部 UI fallback |
| 连接完成后根目录加载不传归属 | `src/app.rs:ConnectionSucceeded`，11176 | 使用该成功事件的 identity |
| 根分页继续依赖推断 | `src/app.rs:accept_catalog_page`，18069 | 继承 page.key.connection |
| o/Enter 对 Online/Syncing 仅切换展开 | `src/app.rs:toggle_explorer_selected`，18808 | 展开时确保目录已加载或正在加载 |
| 已展开直接返回 | `src/app.rs:expand_explorer_selected`，19747 | 显式展开仍可修复未加载状态 |
| 加载判定使用全局连接 | `src/app.rs:target_needs_load`，19864 | 显式传入所属 profile/connection |
| 切换及复用 session 直接写 Online | `src/app.rs:request_connection_target_inner`，15523 | 由统一状态推导替代 |
| 后台投影改写 active_profile | `src/model/workspace.rs:rebuild_projection`，937 | 分离树更新与当前上下文投影 |
| Redis 后台结果被过滤 | `src/app.rs:RedisDatabasesLoaded/Failed`，12730 | 使用所属 session 和请求匹配 |
| Redis retry 使用全局 generation | `src/app.rs:expand_explorer_selected`，19778 | 从目标节点所属 session 获取 |
| 没有 load_state 就没有占位 | `src/model/explorer.rs:append_state_rows`，2251 | 保证展开初始化及明确状态行 |
| SQL 目录页没有统一外层超时 | `src/runtime.rs:load_catalog_page`，2179 | 给每个有效请求确定的收尾路径 |
| Redis discovery 没有整体超时 | `src/runtime.rs:discover_redis_databases`，1067 | 整体发现需要超时失败回调 |

## 2. 状态和行为契约

### 2.1 请求归属

- 用户输入入口：从操作节点获取 profile，再解析该 profile 的有效 catalog session。
- 异步入口：从事件/request key 继承 connection，不能读取当前光标决定归属。
- 有 native profile 的 CatalogTarget 必须与 connection.profile_id 一致。
- 接收 SQL 页必须同时匹配有效 catalog session、当前 epoch 和 pending 完整 request key。
- 接收 Redis 发现必须同时匹配有效 catalog session 和 pending discovery key。
- 每个 profile 只有一个选定的 Explorer catalog session；同 profile 的不同 execution target 不可无条件抢占其目录所有权。
- 更换 catalog session 时，先清理或恢复旧 pending，再安装新 owner；旧响应不影响新请求。

### 2.2 展开行为

| 状态 | o/Enter 从折叠到展开 / 显式 expand | 折叠 |
|---|---|---|
| Offline / 连接失败 | 记录展开意图并连接 | 只更新展开意图 |
| Linking | 复用现有尝试，不重复连接 | 清理展开意图，完成后不强制展开 |
| NotLoaded | 发起一次加载 | 保留后台加载 |
| 有效 Loading | 复用请求 | 保留后台加载 |
| Loaded，包括空目录 | 使用缓存 | 不清缓存 |
| Failed / Stale | 显式展开允许重试；保留可用旧数据 | 不发请求 |
| PermissionDenied | 展示原因，用户显式展开/重试可重试 | 不自动循环重试 |

`o/Enter` 保持切换语义：已展开时首先折叠；下一次展开时执行 ensure。显式 expand 对已展开但 NotLoaded/Failed 的节点也执行 ensure。单纯光标移动不发起加载。

### 2.3 展示约束

- SYNCING 代表该 profile 确实存在有效目录加载任务。
- Online 代表会话在线，不等同于目录非空。
- 已展开的连接必须有内容、Loading、Empty 或可重试错误中的一种可解释展示；Linking 阶段可沿用连接行提示。
- 部分目录失败时保留成功加载的数据；有其他任务时仍可 Syncing，全部结束后回 Online 并保留局部错误行。
- 子级自动请求必须先登记，再重新计算 Syncing，避免父页结束时错误地提前显示 Online。

## Task 1：修复 SQL 请求和续页的显式归属

**Files**
- Modify: `src/app.rs`：start_catalog_request、ConnectionSucceeded、accept_catalog_page、target_for_node/owner 的调用入口、commands_for_catalog_targets 等全部调用者。
- Test: `tests/connection_switch.rs`、`tests/catalog_reducer.rs`。

**Step 1 — 添加竞态回归测试**

在现有双 memory profile fixture 上分别构造 A→B、B→A 完成顺序。真实设置 Explorer 选中项为 B，再送入成功事件；捕获 update 返回的 Command，不直接向 catalog 插入条目来代替加载。

建议测试：
- `connection_catalog_request_belongs_to_completed_profile`
- `background_database_continuation_keeps_original_connection`
- `catalog_target_profile_mismatch_does_not_create_pending_request`

断言 A、B 各自的根请求 connection、scope、epoch 正确；B 未连接时 A 仍能发出请求；分页时移动选中项不改变 connection；不匹配 target 不污染 pending。

**Step 2 — 验证测试命中问题**

Run: `cargo test --test connection_switch connection_catalog_request_belongs_to_completed_profile`

Expected: 修改前由于请求缺失或归属错误失败，而不是 fixture 或编译错误。

**Step 3 — 调整核心接口并迁移调用者**

目标接口：

```rust
fn start_catalog_request(
    &mut self,
    connection: ConnectionIdentity,
    target: CatalogTarget,
    cursor: Option<crate::db::catalog::CatalogCursor>,
    intent: CatalogRequestIntent,
) -> Vec<Command>
```

1. 从 connection.profile_id 定位 state 和 scope，验证 session 为 Connected。
2. target.profile_id() 为 Some 时验证等于 connection.profile_id。
3. 用户入口通过一个只读 helper 获取 profile 的 catalog identity；优先使用有效 catalog_sessions，不用无序遍历任意挑选 session。
4. 初次 ConnectionSucceeded 在没有 catalog owner 时显式安装 owner；后台 execution session 成功不能无条件替换已有 owner。
5. 根请求使用成功事件 identity；续页、schema/group/object 预加载使用父页 identity。
6. 刷新、状态行重试、LoadMore、mutation reconciliation 等调用者传入自身业务对象的 identity。
7. 保留现有 pending 去重和完整 key 校验，不通过放宽校验接受错误归属的结果。

**Step 4 — 验证**

Run: `cargo test --test connection_switch --test catalog_reducer`

Expected: 全部通过；复用已有 session、不同 target 并发测试继续通过。

**建议提交：** `fix(explorer): bind catalog requests to explicit sessions`

## Task 2：统一展开和已有连接的按需加载

**Files**
- Modify: `src/app.rs`：toggle_explorer_selected、expand_explorer_selected、target_needs_load、request_connection_target_inner。
- Test: `tests/connection_switch.rs`、`tests/catalog_reducer.rs`、`tests/explorer_state.rs`。

**Step 1 — 补充恢复行为测试**

- `expanding_connected_unloaded_profile_starts_catalog_request`
- `explicit_expand_repairs_already_expanded_unloaded_profile`
- `repeated_expand_reuses_pending_catalog_request`
- `loaded_empty_profile_is_not_reloaded_on_expand`
- `background_profile_load_check_uses_its_own_state`
- `collapse_during_connect_clears_expand_after_connect`

覆盖 Failed、Stale、PermissionDenied 显式重试，保持缓存和分页语义。

**Step 2 — 定向运行新增测试，确认原实现失败**

Run: `cargo test --test connection_switch expanding_connected_unloaded_profile_starts_catalog_request`

**Step 3 — 引入 ensure 入口**

增加 `ensure_profile_catalog(profile_id, intent)`，通过有效 session 与根 load_state 决策；SQL 分支调用 Task 1 的显式请求函数，Redis 分支在 Task 5 接入。

把“改变 expanded 集合”和“确保资源可用”分开：toggle 折叠不加载，toggle 展开及 explicit expand 调 ensure；已有连接激活也调 ensure。连接尚在 Linking 时去重并维护 expand_after_connect。target_needs_load 改为显式 owner/profile 参数，不能读全局 active connection。

不要把 catalog.is_empty() 当成未初始化的判断；根 Loaded 空结果是合法缓存。Loading 的恢复依赖请求退休/超时明确收尾，不能靠固定时间猜测任务失效后重复发请求。

**Step 4 — 验证**

Run: `cargo test --test connection_switch --test catalog_reducer --test explorer_state`

**建议提交：** `fix(explorer): ensure catalog availability when expanding profiles`

## Task 3：收敛连接状态、目录状态及请求退休

**Files**
- Modify: `src/model/explorer.rs`：ExplorerProfileState、recover_pending_requests、advance_catalog_epoch 周边状态管理。
- Modify: `src/app.rs`：连接成功、连接切换、accept/fail_catalog_page、clear_profile_catalog、retire_profile_connections。
- Test: `tests/catalog_reducer.rs`、`tests/connection_switch.rs`。

**Step 1 — 增加生命周期测试**

- `syncing_requires_pending_catalog_work`
- `catalog_followup_requests_keep_profile_syncing`
- `partial_catalog_failure_preserves_successful_children`
- `catalog_session_replacement_retires_old_pending_requests`
- `old_catalog_failure_cannot_clear_new_request`

对同一 profile 更换 generation、同 generation refresh 替换、disconnect、profile 删除/编辑后的旧响应分别验证。

**Step 2 — 确认失败**

Run: `cargo test --test catalog_reducer catalog_followup_requests_keep_profile_syncing`

**Step 3 — 增加统一状态重算入口**

1. 保留 ExplorerConnectionStatus 作为 UI 投影，统一从 session 的建连/在线状态、SQL pending 和 Redis discovery pending 推导。
2. 删除“旧 active profile 直接 Online”“catalog.is_empty() 就 Syncing”等散落赋值。
3. request 登记、成功、失败、后继任务登记、取消/退休完成后重新推导状态。
4. session 替换时用 recover_pending_requests 的既有逻辑恢复状态，有快照转 Stale，无快照转可重新加载状态；先移除旧 owner，再装新 owner。
5. 用明确的初始化/owner 变化判断推进 epoch，空目录不重复推进。
6. 旧 key 回调仅忽略；不得清除同 owner 的新 pending。

**Step 4 — 验证**

Run: `cargo test --test catalog_reducer --test connection_switch`

**建议提交：** `fix(explorer): derive syncing state from catalog lifecycle`

## Task 4：后台回写与当前工作区投影解耦

**Files**
- Modify: `src/app.rs`：ConnectionSucceeded、accept_catalog_page、fail_catalog_page、CatalogPageLoaded 后处理。
- Modify: `src/model/workspace.rs`：rebuild_projection、completion_index 和 active_profile 的更新入口。
- Test: `tests/connection_switch.rs`、`tests/catalog_reducer.rs`、`tests/lsp_catalog.rs`。

**Step 1 — 添加乱序完成测试**

- `background_catalog_page_does_not_change_active_workspace`
- `background_connection_success_does_not_activate_old_navigation`
- `background_catalog_failure_does_not_replace_active_projection`
- `background_catalog_page_does_not_replace_active_completion_index`

记录 B 的 connection、active_workspace_profile、active tab、focus、Explorer selected 和补全样本；让 A 完成连接/加载/失败，断言 A 树发生正确变化而 B 上下文保持。节点插入允许为了保持选中项可见而必要调整滚动数值，不固定断言 scroll 永远不变。

**Step 2 — 定向验证失败**

Run: `cargo test --test connection_switch background_catalog_page_does_not_change_active_workspace`

**Step 3 — 分离更新路径**

1. normalized 树始终按响应 profile 更新。
2. rebuild_projection 拆分为不改变 active_profile 的树/可见性 reconciliation，以及显式前台 profile 投影。
3. 只有前台激活入口可以更改 active_profile；不再通过后台结果“顺手”切换。
4. completion_index 更新基于实际前台 consumer 的 execution target；切换回 A 时从缓存重建 A 的投影。
5. accept_catalog_page 返回明确是否接受的结果，例如 `Option<Vec<Command>>`，避免旧页被拒绝后外层仍触发 diagnostics、pending recovery 或“同步完成”通知。
6. catalog_sync_pending、pending_parent_recoveries 和 pending_catalog_selection 如需跨异步保存，携带 connection/owner；尤其 Databases key 不能单独区分 profile。
7. 后台连接 target 从已接受的 SessionState 获取，不回退为 profile 默认 target，保留非默认数据库的准确语义。

**Step 4 — 验证**

Run: `cargo test --test connection_switch --test catalog_reducer --test lsp_catalog`

**建议提交：** `fix(explorer): isolate background catalog updates from active context`

## Task 5：Redis 发现请求身份、后台回写和重试

**Files**
- Modify: `src/model/explorer.rs`：Redis discovery key/pending/load state。
- Modify: `src/action.rs`：DiscoverRedisDatabases 相关 Command 与 Loaded/Failed Action。
- Modify: `src/app.rs`：发现启动、回写、刷新及状态行重试。
- Modify: `src/runtime.rs`：discover_redis_databases 和 dispatch。
- Test: `tests/redis_explorer.rs`、`tests/redis_discovery.rs`、`tests/connection_switch.rs`。

**Step 1 — 添加行为测试**

- `background_redis_discovery_updates_own_profile`
- `redis_retry_uses_selected_profile_session`
- `older_redis_discovery_cannot_overwrite_refresh`
- `redis_discovery_failure_finishes_syncing`
- `redis_discovery_completion_after_disconnect_is_ignored`

覆盖 B 为 SQL、A 为 Redis 的混合情况；覆盖同 generation 两次刷新响应乱序。

**Step 2 — 运行新增测试确认失败**

Run: `cargo test --test redis_explorer background_redis_discovery_updates_own_profile`

**Step 3 — 添加最小请求身份**

建议放在 `src/model/explorer.rs` 的类型：

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RedisDiscoveryRequestKey {
    pub connection: crate::identity::ConnectionIdentity,
    pub catalog_epoch: u64,
    pub request_id: u64,
}
```

1. 复用 profile 的 request ID 分配器；保存 pending discovery key 和加载状态。
2. Command 和成功/失败 Action 都携带同一个 key，Runtime 原样传回。
3. 接收时检查所属有效 catalog session 和完整 pending key，移除全局 connection 过滤。
4. ensure_profile_catalog 的 Redis 分支执行 single-flight；显式 refresh 可替换旧请求，旧结果由 key 拒绝。
5. o/Enter、explicit expand、Retry、refresh 统一路由 Redis 发现，不能落入 SQL LoadCatalogPage。
6. 部分成功 discovery 保留数据库列表与 warnings；失败有旧数据时保留旧数据并标注 stale。
7. disconnect、profile 删除、session 替换清理 discovery pending 并推进/失效 owner。

**Step 4 — 验证**

Run: `cargo test --test redis_explorer --test redis_discovery --test connection_switch`

**建议提交：** `fix(redis): scope explorer discovery lifecycle to request identity`

## Task 6：加载、失败和空目录的可解释展示

**Files**
- Modify: `src/model/explorer.rs`：append_profile、append_state_rows。
- Modify: `src/model/workspace.rs`：状态行 label/metadata。
- Modify: `src/ui/mod.rs`：仅在状态图标或文本消费接口需要调整时修改。
- Test: `tests/explorer_state.rs`、`tests/redis_explorer.rs`。

**Step 1 — 添加可见树测试**

- SQL 和 Redis expanded+Loading 显示 Loading 子行。
- Loaded 空目录显示 Empty 子行，不显示 Retry，不重新请求。
- Failed 显示错误/Retry 行，保留连接在线语义。
- Stale 保留已有子节点并显示重试提示。
- collapsed 隐藏子行但 pending 继续有效。

**Step 2 — 定向运行**

Run: `cargo test --test explorer_state --test redis_explorer`

Expected: 修改前新增 Redis loading/empty 用例失败。

**Step 3 — 接入统一状态投影**

SQL 沿用现有 StatusRowKind；Redis 从 discovery load state 生成相同语义的 Loading/Retry/Stale/Empty 行。NotLoaded 不伪装为永久 Loading：加载动作由 ensure 创建真实 pending 后再展示。同步状态变化不自动展开用户已折叠的节点。

**Step 4 — 重跑定向测试并审查图标/状态一致性**

Run: `cargo test --test explorer_state --test redis_explorer`

**建议提交：** `fix(explorer): expose loading and retry states for all profiles`

## Task 7：目录任务超时与收尾保障

**Files**
- Modify: `src/runtime.rs`：load_catalog_page、discover_redis_databases、相关测试模块。
- Test: `src/runtime.rs` 内部 Tokio 单元测试；必要时在 `tests/catalog_reducer.rs` 增加超时事件接收测试。

**Step 1 — 建立可控挂起测试**

使用 Tokio paused time 和一个永不完成的 future 验证 deadline，不访问真实慢数据库，不使用 sleep 模拟概率时序。将超时包装提取成小型内部函数或使用现有测试注入点；不为测试引入通用调度框架。

建议测试名统一 `explorer_request_timeout_` 前缀，覆盖 SQL 和 Redis 返回原 key 的失败事件，以及旧超时事件不能清除替换请求。

**Step 2 — 设置明确的运行时期限**

1. 先检查已有 timeout 配置/常量；有合适机制则复用。
2. 若无统一目录期限，新增内部 `EXPLORER_REQUEST_TIMEOUT`，初始建议 30 秒，涵盖单个 SQL 页或一次 Redis discovery；不在本任务扩展用户配置格式。
3. 超时转换为现有失败 Action，带原始 key 和清楚的 timeout message，不将其误标为权限或认证错误。
4. 有效请求的 missing profile/database 等提前返回路径必须发终态；已退休请求可忽略，前提是 App 已清理 pending。
5. 保留 runtime 的 latest request 检查，并仅在完整 key 相等时清理该请求记录，不能删除新请求的记录。
6. future 超时是 UI 等待期限，不承诺强制终止 Oracle 等阻塞后端工作；旧响应仍由 identity/key 防护。

**Step 3 — 验证**

Run: `cargo test --lib explorer_request_timeout_`

Run: `cargo test --test catalog_reducer --test redis_explorer`

Expected: 时间推进到期限后只有正确失败结果，pending 被终结，显式重试能够重新发起。

**建议提交：** `fix(runtime): bound explorer metadata requests with timeouts`

## Task 8：集成回归及最终验收

**Files**
- Test: `tests/connection_switch.rs`、`tests/catalog_reducer.rs`、`tests/redis_explorer.rs`、`tests/explorer_state.rs`。
- Review: 本计划涉及的 Action/Command 所有构造点和所有目录请求调用者。

**Step 1 — 确认事件顺序矩阵**

| 场景 | 必须断言 |
|---|---|
| 打开 A 后立即打开 B，A 先成功 | A 加载 A，B 加载 B，最新导航仍是 B |
| B 先成功，A 后成功 | A 在后台更新，不抢 B 上下文 |
| A 根页加载中切换 B | A 响应和续页仍归 A |
| A 同步中折叠再展开 | 无重复有效请求，最终可见 |
| A 已在线但 NotLoaded | 展开补发一次加载 |
| A 合法空目录 | 明确 Empty，不重复初始化 |
| A 失败/超时 | Syncing 结束，能重试 |
| A 断开/重连后旧结果晚到 | 不覆盖新数据，不清除新 pending |
| 同 profile 两个 execution target | catalog owner 稳定，execution target 不串用 |
| SQL/Redis 混合并发 | 各自后台完成，Retry 路由正确 |
| A 后台返回、B 正在补全/搜索 | B 数据归属不受污染 |

**Step 2 — 运行定向集成测试**

```bash
cargo test --test connection_switch --test catalog_reducer --test explorer_state --test redis_explorer --test redis_discovery --test lsp_catalog
```

Expected: 全部通过；测试失败不能通过删除原有多连接断言来绕过。

**Step 3 — 完成代码检查**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
git diff --check
```

按顺序执行；fmt 失败先运行 formatter 再检查；行为测试失败先定位修复。记录需要外部数据库或 Oracle 环境的跳过/阻塞项，区分测试通过与未执行。若默认 driver-oracle 确实无法构建，可用 `--no-default-features` 验证公共逻辑，并明确完整默认构建尚待环境验证。

**Step 4 — 手动验收**

使用可用测试连接，在 Explorer 连续打开两个 SQL 连接及一个 Redis 连接；反复改变完成顺序、移动光标、折叠展开。观察各自 Loading→内容/Empty/Retry，后台完成不改变当前工作区。至少覆盖一个失败连接以及失败后重试。无真实连接环境时保留为待执行，不以静态分析代替手动验证结论。

**Step 5 — 最终审查**

- start_catalog_request 及异步续页不存在 selected/active_profile fallback。
- Redis discovery 接收和重试不存在全局 generation 拼接。
- ConnectionSucceeded 不根据 catalog.is_empty() 猜测初始化状态。
- 所有 Syncing 都能追溯到有效任务；所有有效任务都有完成、失败、超时或退休路径。
- 拒绝的过期结果不触发前台补全、恢复和成功通知。
- 每个 profile 的缓存、expanded、错误状态独立。
- 只包含本计划范围内文件的预期修改。

## 3. 依赖顺序与交付标准

顺序：Task 1 → Task 2 → Task 3 → Task 4 → Task 5 → Task 6 → Task 7 → Task 8。

这些任务共享 App reducer 和 Explorer 状态，建议顺序实施以减少接口迁移冲突。每个阶段保留小范围可审查 diff，完成对应定向验证后再进入下一阶段。

最终交付：

1. SQL 与 Redis 根目录请求及回调均显式按 session/request 归属。
2. 快速切换时所有有效连接可在后台完成目录加载。
3. 已展开未加载状态可以通过正常展开操作恢复。
4. 合法空、加载中、失败、stale 均有明确展示。
5. 后台响应不会抢占当前工作区或污染其派生数据。
6. 请求失效、超时和重连不会留下永久 Loading/Syncing。
7. 事件顺序回归测试及适用的格式、lint、测试检查通过。

## 4. 计划状态

- 当前交付为实施计划，尚未修改业务代码。
- 文中的新测试、helper 和 request key 类型尚待实施。
- 完成实施后在最终交付说明中列明实际执行的测试命令、结果和手动验证情况。
