# 多连接 Console 与离线编辑 Implementation Plan

> 执行说明：按任务依赖顺序实施，每个任务先补行为测试、确认旧行为失败，再实现并运行相关检查。仅在用户明确要求时提交 Git。本文中的新增类型、动作名和测试名是拟定接口，不表示已经实现。

**Goal:** 支持多个数据库连接和跨连接 Tabs 同时工作，使所有 Console 可独立创建、绑定、关闭、删除和离线持久化，并在执行时按需连接。

**Architecture:** 保留 Action → App::update → Command → Runtime 边界。将按 profile 切换的工作区转换为全局文档工作区；将单活动连接转换为按 ExecutionTarget 索引的会话集合。Console 文档绑定、Explorer 选择和运行时连接状态彼此独立，所有异步操作按对象及会话身份路由。

**Tech Stack:** Rust 2024、Tokio、Ratatui/Crossterm、Modalkit、现有 DatabaseConnection 驱动抽象、Serde/TOML、现有 workspace SQL 文件存储。无需新增生产依赖。

---

## 1. 范围与已确认的代码基础

- `src/runtime.rs`：`Runtime.connection` 是单个 `Option<ActiveConnection>`；`ConnectionAttemptTracker` 是单个 latest/cancelled；`active_database_for_target()` 同时匹配身份和完整目标。
- `src/model/workspace.rs`：`ConnectionState` 保存单连接状态；`ConnectionWorkspace` 持有 tabs、ConsoleRecord 和 SQL 文本。
- `src/app.rs`：`activate_profile_workspace()`、`take_active_workspace()`、`install_workspace()` 整体切换工作区，并重新创建 EditorWorkspace。
- `src/app.rs`：`prepare_active_console_target()` 激活 Console 时请求连接；`run_active_sql()` 首先要求全局活动连接。
- `src/model/tab.rs`：ConsoleTab、ConsoleRecord 已有 execution_target；WorkspaceTab 包含 SQL、Relation、Dashboard。
- `src/model/execution_target.rs`：ExecutionTarget 已实现 Eq/Hash，包含 profile_id/database/schema，并提供配置校验及应用方法。
- `src/identity.rs`：ConnectionIdentity 已包含 profile_id 和 generation，并被大量数据库请求复用。
- `src/persistence/workspace.rs`：当前格式版本为 4，按 profile 存储 tabs 和 consoles；PersistedConsole 已支持 open 和 target。
- `docs/architecture.md`：明确记载单活动池和激活 Console 触发连接的现有契约，需要同步更新。

实施前运行相关基线测试并记录既有失败；不要将环境缺失或既有失败归因于本次改动。

## 2. 必须保持的行为契约

### 2.1 文档、目标、连接

1. 一个全局 Tab 栏同时容纳多个 profile 的 SQL、Relation 和 Dashboard。
2. Explorer 的选择只影响导航和新建默认目标，不改变已有文档绑定。
3. 打开、激活、编辑、保存和重新绑定 Console 不发送 Connect 命令。
4. 连接成功或失败不切换活动 Tab，也不重建 EditorWorkspace。
5. 关闭文档 Tab 不断开连接；断开连接不关闭或删除文档。
6. 无 profile、无连接、无 Tab 都是合法状态。
7. 表格和 Dashboard 的目标固定；跨目标切换仅适用于 Console。

### 2.2 默认挂载

按以下顺序选择第一个有效候选：

1. Explorer 当前选择节点所属的 profile；其 database/schema 节点提供更具体目标。
2. 最近实际使用过且仍存在的 profile，优先复用仍合法的最近目标。
3. 至少存在一个在线会话的 profile，按 Explorer 稳定排列选第一个。
4. 无候选则创建 target=None 的未绑定 Console。

Explorer 选择是导航上下文，不要求按下新建快捷键时键盘焦点仍在左侧；但分组、空节点不能沿用一个隐藏的历史 profile 来伪装当前选择。第一、二级可以离线。实际使用指激活已绑定 Tab、显式绑定或发起数据库操作；后台定时刷新、回调和纯方向键浏览不更新 MRU。

### 2.3 命名与显示

- Console 持久化名称不含 `@profile`；标签渲染时拼接。
- SQL、Relation、Dashboard 都显示连接名；未绑定显示 `@未绑定`，失效绑定显示可辨识的缺失目标。
- 同名连接显示分组限定名，仍冲突则使用短 UUID。
- 窄终端优先截断文档名，目标栏提供完整连接/database/schema。
- Console 管理器是全局列表，分别显示文档打开状态和目标连接状态，并支持组合搜索。

### 2.4 生命周期和事务

- Close：保存并关闭 Tab，保留文档；Delete：确认后删除文档和 SQL 文件。
- 首项不再特殊；连接建立不自动生成 default Console。
- 查询运行中、待连接执行中重新绑定需先取消或等待，不隐式将任务搬到新目标。
- 活跃手动事务提交/回滚后才能重绑、关闭或断开；复用现有事务退出选择，不另建平行确认体系。
- 断开 profile 处理它全部会话及相关事务，不触及其他 profile。
- 删除 profile 保留 SQL 文档及失效目标引用；重命名只更新显示；配置实质变更只退休该 profile 相关会话。
- 应用退出检查所有 Tab 的事务，不能仅检查活动 Tab/活动连接。

## 3. 数据与请求设计

### 3.1 最小必要结构调整

| 层 | 推荐结构 | 说明 |
| --- | --- | --- |
| 文档 | 全局 ConsoleRecord 集合 | ID、名称、目标、事务模式和打开状态的权威来源 |
| Tab | 全局 Vec<WorkspaceTab> | 只维护打开页面及运行/展示状态；按 ID 关联文档 |
| 编辑器 | 一个 EditorWorkspace | 按 Console UUID 管理会话，不随连接切换重建 |
| 会话状态 | 按 ExecutionTarget 索引 | 每个目标独立 Connecting/Connected/Failed/Offline |
| Runtime 会话 | HashMap<ExecutionTarget, ActiveConnection> | 初版完整目标隔离，不跨 schema 猜测驱动共享安全性 |
| 连接尝试 | 按目标索引的 attempt | 同目标 single-flight，不同目标独立连接 |
| 待执行 | 按 Console ID 索引 | 每个 Console 最多一个待执行请求，多个 Console 可等待同一会话 |
| MRU | 最近有效 profile/target | 足够满足默认规则，不额外引入复杂排名系统 |

优先保留 ConnectionIdentity 的现有形状。generation 使用应用级单调分配器，给每个会话尝试分配不同值，保证同 profile 的多个目标不会出现相同身份；状态及请求是否有效由会话注册表判定，而非全局 latest。generation 不代表持久化身份。

ConsoleRecord 和 ConsoleTab 的重复字段应在文档化过程中消除或集中到唯一访问接口；不能继续依赖切换工作区时批量同步。数据库运行结果仍属于 ConsoleTab，不能作为文档持久化内容。

### 3.2 延迟执行契约

新建 PendingExecution 数据保存：请求 UUID、Console UUID、目标快照、绑定修订号、文档修订号、SQL 范围及文本快照、方言、事务模式。最终 ExecutionDraft 仍复用现有类型，在获得有效 ConnectionIdentity 后构造。

推荐状态转换：

```text
Idle → AwaitingTarget（未绑定时）→ AwaitingConnection → AwaitingConfirmation → Running → Idle
                                       ↓                    ↓           ↓
                                  Failed/Cancelled      Cancelled    Completed/Failed/Cancelled
```

执行前先做本地空 SQL、目标范围及权限预检查；最终确认沿用现有 ExecutionDraft 流程。连接等待中修改文本不改写 SQL 快照；编辑后的文本不被执行，也不被覆盖。旧 SQL 错误定位只有在文档修订仍匹配时才投影到当前编辑器。

连接成功后按请求 ID 和 Console ID 继续，禁止递归调用依赖 active_tab 的 run_active_sql。关闭/删除/取消或重绑使 pending 请求失效。连接失败不保留自动执行队列，重试必须是新的用户执行请求。SQL 已提交后即使网络错误也不自动重放。

### 3.3 多目标与缓存

初版以完整 ExecutionTarget 为会话键，复用 apply_to_profile 和现有连接构造路径；不要用共享连接上的 USE/search_path 修改来实现跨 Console 切换。真实驱动验证后才考虑更粗粒度池复用。

元数据请求保留 ConnectionIdentity、catalog epoch、target、request id 校验。profile 内相同目录对象可共享确定的元数据，但请求时效、能力、owner context 和正在加载状态必须能区分会话。CatalogId 继续表示对象身份，不为解决请求路由而随意改写。

## 4. 任务清单与依赖

```text
T01 基线与契约
 ├─ T02 全局文档 → T03 v5 持久化 → T04 离线文档生命周期
 └─ T05 多会话注册表
T02 + T05 → T06 App 路由 → T07 元数据/Relation/Dashboard → T08 事务与断开
T04 + T06 + T08 → T09 Console 重绑 → T10 延迟执行
T04 + T06 → T11 默认目标与 MRU
T07 + T09 + T11 → T12 Tab/Console 管理交互
T03 + T07 + T08 + T10 + T12 → T13 集成验收与文档
```

以下任务是可评审的工作单元，不要求一次性修改所有文件。每个任务内部按“补行为测试 → 确认失败 → 最小实现 → 定向验证”推进。共享 src/app.rs/src/runtime.rs 的任务顺序执行，避免互相覆盖。

### T01：建立基线和替换契约清单

**文件：** `tests/connection_switch.rs`、`tests/workspace_tabs.rs`、`tests/workspace_persistence.rs`、`tests/app_flow.rs`、`docs/architecture.md`。

1. 阅读现有测试，列出“只有一个活动连接”“切换连接隐藏旧 Tabs”“default 不可关闭”“激活自动连接”的断言。
2. 运行基线：`cargo test --test connection_switch --test workspace_tabs --test workspace_persistence --test app_flow`。
3. 记录待替换断言与原本仍需要保留的隔离、失败回滚、旧事件拒绝规则；不能直接整文件删测试。
4. 确定新回归测试入口：`tests/multi_connection.rs`、`tests/offline_consoles.rs`。

**完成标准：** 后续每种旧语义都能对应一项新行为测试；已知环境问题有记录。

### T02：全局 Console 文档与 Tabs

**修改：** `src/model/tab.rs`、`src/model/workspace.rs`、`src/app.rs`、`src/editor/mod.rs`。

**测试：** `tests/workspace_tabs.rs`、`src/editor/tests.rs`。

1. 补测试：两个 profile 的 Console 可同时存在；切换 Tab 保留 SQL、光标和撤销记录。
2. 将 sql_editors 和 tabs 变为全局工作区，移除 active_workspace_profile 对文档可访问性的限制。
3. 删除连接切换路径中的 take/install/cache 整体搬迁；连接事件不再更换 EditorWorkspace。
4. 明确 ConsoleRecord 为文档信息权威来源，改造名称、目标和事务模式访问；关闭 Tab 后保留文档所需 SQL。
5. Relation 和 Dashboard 增加/保留显式归属入口，避免依赖当前 profile 补全目标。
6. 统一处理 active_tab=None/空列表，替换在无 Tab 路径上的下标和 expect 假设。
7. 运行 `cargo test --test workspace_tabs` 与 `cargo test --lib editor::tests`。

**完成标准：** 连接焦点变化不影响文档集合和编辑历史；全局 Tab 顺序由用户导航操作决定。

### T03：v5 持久化与兼容迁移

**修改：** `src/persistence/workspace.rs`、`src/app.rs`、`src/runtime.rs`。

**测试：** `tests/workspace_persistence.rs`、`tests/persistence.rs`。

1. 用现有 fixture 构造多 profile 的 v4 工作区，包含同名 Console、关闭 Console、Relation、Dashboard、空 SQL 和缺失 profile。
2. 新格式包含 version、全局 consoles、tabs、active_tab、最近使用目标；Relation/Dashboard 必须显式保存 profile 归属。
3. 将 v4 每个 profile 工作区按稳定顺序扁平化，保留 Console UUID 和 SQL 文件引用；缺少 target 时从原容器补齐。
4. 原 active_profile 的 active_tab 优先恢复；失效活动 ID 回退至首个打开 Tab；没有打开项则 None。
5. 所有旧 default 当普通文档迁移；缺失 profile 不删除文档；重复 Console ID 或缺失 SQL 文件走显式错误/恢复反馈，不静默丢弃。
6. 保留此前支持的旧格式读取链，使其先规范化再迁移到 v5。
7. 保存 v4 原始 manifest 的迁移备份，新 manifest 完整写入后再替换；首次迁移不重命名或删除 SQL 文件。
8. 检查保存队列的 revision 合并和退出 flush；模拟写失败，确保旧 manifest/SQL 可恢复。
9. 运行 `cargo test --test workspace_persistence --test persistence`。

**完成标准：** v4→v5→重启往返不丢文档、内容或归属；迁移可重复且失败可恢复；不持久化在线状态/事务句柄。

### T04：移除 default 并开放离线生命周期

**修改：** `src/app.rs`、`src/model/tab.rs`、`src/runtime.rs`、`src/ui/mod.rs`。

**测试：** `tests/offline_consoles.rs`（新增）、`tests/workspace_tabs.rs`、`tests/startup_profiles.rs`。

1. 补测试：有配置但零连接、完全无配置时都能创建/打开 Console，返回命令不含 Connect。
2. 删除 is_default_console、首项排序、禁止重命名/关闭/删除和自动补建的分支。
3. 移除 prepare_active_console_target 在激活文档时的联网副作用。
4. 恢复工作区只恢复文档；普通启动不因恢复活动目标连接。显式 CLI 指定连接的启动意图仍可连接，并以测试区分。
5. Close 保存但不删除；Delete 沿用确认后删除；允许最后一项关闭和删除。
6. 文本变化、绑定、重命名和打开状态变化都独立触发持久化；退出仍等待保存确认。
7. 运行 `cargo test --test offline_consoles --test workspace_tabs --test startup_profiles`。

**完成标准：** 离线创建→编辑→关闭→重启→打开内容不变，整个文档流程没有数据库调用。

### T05：多目标会话与连接 single-flight

**新增：** `src/runtime/connections.rs`（会话注册表和连接尝试辅助类型）。

**修改：** `src/runtime.rs`、`src/model/workspace.rs`、`src/command.rs`、`src/action.rs`；必要时 `src/db/mod.rs` 的目标连接入口。

**测试：** `tests/multi_connection.rs`（新增）、`tests/profile_runtime.rs`、`tests/connection_switch.rs`。

1. 测试 A/B 连接结果乱序、同目标同时请求、同 profile 不同 target 同时连接。
2. 使用 ExecutionTarget 为键保存会话及 attempt；全局 generation 分配后绑定到具体目标。
3. lookup 必须匹配目标、ConnectionIdentity 和 profile revision；替换 active_database/active_database_for_target 单槽查询。
4. 同目标重复请求复用一个连接尝试；不同目标彼此独立，失败不退休其他会话。
5. 连接时只短暂持有注册表锁，释放锁后解析凭据和联网，完成后再次验证 attempt/revision。
6. 过期连接成功回调关闭本次新建资源，不关闭当前有效会话。
7. 若两个目标同时需要交互凭据，提示按队列串行呈现，响应绑定原 attempt；缓存复用遵守现有凭据策略。
8. shutdown 遍历全部会话和任务进行关闭。
9. 运行 `cargo test --test multi_connection --test profile_runtime --test connection_switch`。

**完成标准：** A/B 互不替换、同目标不重复拨号、取消/过期结果无资源泄漏或串联影响。

### T06：App 操作路由与异步结果隔离

**修改：** `src/app.rs`、`src/action.rs`、`src/command.rs`、`src/model/workspace.rs`、`src/sql/execution.rs`。

**测试：** `tests/multi_connection.rs`、`tests/sql_execution.rs`、`tests/app_flow.rs`。

1. 补测试：A 执行中切到 B 并执行；B 先返回，A 后返回；两个结果归位且焦点不变。
2. 用 console_target/session_for_target、relation_target、dashboard_target 等对象级接口替换 database_command_identity 的全局含义。
3. Query/分页/取消按 Tab 和 generation 定位；接收后台结果不依赖活动 Tab。
4. 把连接 pending、全局 has_running_query 等阻塞缩小到目标会话/Console/真实共享资源。
5. 将连接状态、错误、计时及事务操作计时中相关全局字段改为按目标或 Tab 保存。
6. 连接成功只更新对应会话、Explorer 状态及等待请求，不激活文档、不改其绑定。
7. 运行 `cargo test --test multi_connection --test sql_execution --test app_flow`。

**完成标准：** 前台焦点和后台执行生命周期完全解耦；旧 generation 结果拒绝规则继续成立。

### T07：Explorer、Relation、Dashboard、补全诊断的多连接适配

**修改：** `src/app.rs`、`src/runtime.rs`、`src/model/workspace.rs`、`src/model/explorer.rs`、`src/model/relation.rs`、`src/model/dashboard.rs`、`src/model/database_selector.rs`、`src/ui/dashboard.rs`。

**测试：** `tests/catalog_reducer.rs`、`tests/relation_runtime.rs`、`tests/relation_tabs.rs`、`tests/sql_completion.rs`、`tests/sql_diagnostics.rs`、`tests/multi_connection.rs`。

1. 测试 A/B 元数据和表加载同时进行，另一目标连接/断开不清空当前有效结果。
2. Explorer 每个 profile 聚合状态；目标栏显示精确目标会话状态，避免一个目标在线掩盖另一个目标连接失败。
3. 目录 epoch、owner context、capabilities、搜索请求和取消定位到所属连接；不得继续用单 catalog_search_task 取消无关 profile 请求。
4. Relation 查询、DDL、分页、变更保存及身份恢复使用 Relation 自身目标和原有 scope 快照。
5. Dashboard 显式绑定目标；自动刷新仅用于其在线会话，断开后暂停，手动刷新才可请求重连。
6. 方言从 Console 绑定 profile 获取；未绑定用 Generic，离线语法解析不联网。
7. 补全使用匹配目标的缓存或关键字补全；诊断 key 包含目标/绑定修订及目录版本。切换目标取消旧请求，不能误报离线目录未加载为对象不存在。
8. 运行 `cargo test --test catalog_reducer --test relation_runtime --test relation_tabs --test sql_completion --test sql_diagnostics --test multi_connection`。

**完成标准：** 无隐藏自动重连；非 Console 功能同样支持并发和正确归属。

### T08：事务、断开、配置变更和退出

**修改：** `src/app.rs`、`src/runtime.rs`、`src/runtime/transaction.rs`、`src/model/transaction.rs`、`src/model/workspace.rs`。

**测试：** `tests/transaction_reducer.rs`、`tests/sqlite_transactions.rs`、`tests/quit_transaction_review.rs`、`tests/profile_lifecycle.rs`、`tests/multi_connection.rs`。

1. 测试 A 手动事务运行时 B 正常操作；断开 B 不提交、回滚或取消 A。
2. deferred intent 收集范围改为相关 Console/会话；全局退出才聚合全部事务。
3. 断开 profile 取消它的待连接请求、运行请求、目录任务和 worker，并关闭全部目标会话。
4. 取消单个 Console 等待不得取消其他等待者共享的连接尝试；无等待者且无显式连接意图时才可取消尝试。
5. profile 重命名不重连；影响认证/地址等连接配置变化退休相关会话；scope 变化沿用现有校验和失效规则。
6. 删除 profile 不调用删除 Console 文档逻辑；保留失效目标和 SQL，供重新绑定。
7. 活跃事务连接丢失使用既有失败状态机，不自动重连后继续原事务。
8. 运行 `cargo test --test transaction_reducer --test sqlite_transactions --test quit_transaction_review --test profile_lifecycle --test multi_connection`。

**完成标准：** 事务资源不跨目标迁移；所有 destructive/退出动作保留现有用户选择语义且作用域正确。

### T09：Console 显式重新绑定

**修改：** `src/action.rs`、`src/app.rs`、`src/model/database_selector.rs`、`src/model/tab.rs`、`src/model/workspace.rs`。

**测试：** `tests/execution_target.rs`、`tests/offline_consoles.rs`、`tests/transaction_reducer.rs`。

1. 增加携带 console_id 的目标选择/绑定动作，不从提交弹层时的 active_tab 推断来源。
2. 选择器列出当前工作区可见 profile，包含离线 profile；目标合法性使用 ExecutionTarget::is_valid 和现有 CatalogScope。
3. 离线选择默认 database/schema；允许在配置范围内输入目标，在线目录只作为候选来源，不作为绑定前置条件。
4. 提交绑定立即更新文档、递增 binding revision 并保存，不等待网络成功。
5. 保留文本、光标和 undo；清除旧结果网格、分页及补全/诊断，保留带执行来源的 Output。
6. 输出新增重绑记录；正在执行或活跃事务走 T08 的等待/取消/事务退出规则。
7. 运行 `cargo test --test execution_target --test offline_consoles --test transaction_reducer`。

**完成标准：** A→B→离线编辑→重启仍绑定 B；B 无法连接不回滚已明确选择的文档绑定。

### T10：执行时连接与不可变待执行请求

**新增：** `src/model/pending_execution.rs`，并在 `src/model/mod.rs` 导出。

**修改：** `src/app.rs`、`src/action.rs`、`src/command.rs`、`src/runtime.rs`、`src/sql/execution.rs`。

**测试：** `tests/offline_consoles.rs`、`tests/multi_connection.rs`、`tests/sql_execution.rs`、`tests/credential_resolution.rs`。

1. 补行为测试：首次执行→一次连接→自动执行一次；连接期间切换 Tab/修改 SQL/重复执行/取消/关闭分别验证。
2. run_active_sql 先定位文档、解析 scope、捕获 SQL 快照，不以全局在线状态提前拒绝。
3. 未绑定时保存本次意图并打开目标选择器；取消则结束请求，确定目标后继续。
4. 目标在线直接生成 ExecutionDraft；离线提交/加入连接尝试，并记录 WaitingForConnection 状态。
5. 收到连接完成事件后，按目标和请求 ID 查找等待者，再检查文档存在、绑定 revision、profile revision 和取消状态。
6. 将快照转换为 ExecutionDraft，沿用事务分类、确认策略、只读策略和 dispatch_draft，按原 Console 派发。
7. 多个待确认请求按队列显示，弹层响应绑定请求 ID，不覆盖其他模态操作；用户取消仅取消对应请求。
8. 连接失败、凭据取消清理请求并写入原 Console Output；不会在下次手动连接时悄悄重放。
9. SQL 提交后的网络错误不自动重试；诊断位置若修订不匹配仅展示于输出。
10. 运行 `cargo test --test offline_consoles --test multi_connection --test sql_execution --test credential_resolution`。

**完成标准：** 延迟执行准确一次、固定目标和 SQL；连接/确认异步过程不会串到当前活动 Tab。

### T11：默认绑定与最近使用目标

**修改：** `src/app.rs`、`src/model/workspace.rs`、`src/model/explorer.rs`、`src/persistence/workspace.rs`。

**测试：** `tests/offline_consoles.rs`、`tests/explorer_state.rs`、`tests/workspace_persistence.rs`。

1. 对第 2.2 节的优先级做表驱动测试，覆盖连接子节点、分组、无 profile、已删除 MRU、离线 MRU、多在线顺序。
2. 集中实现一个默认目标解析器，新建入口全部调用同一函数。
3. 对具体 database/schema 节点提取范围，其他对象取其所属合法目标；不能越过 CatalogScope。
4. 更新 MRU 的语义动作集中处理，排除异步回调和后台刷新。
5. MRU 保存和恢复后重新校验；配置不再存在时跳过，不阻塞新建。
6. 运行 `cargo test --test offline_consoles --test explorer_state --test workspace_persistence`。

**完成标准：** 任意新建入口得到相同、可预测目标，且不触发连接。

### T12：Tab 标签、目标入口和全局 Console 管理器

**修改：** `src/ui/mod.rs`、`src/ui/layout.rs`、`src/input/keymap.rs`、`src/input/mouse.rs`、`src/help.rs`、`src/model/tab.rs`、`src/app.rs`。

**测试：** `tests/ui_render.rs`、`tests/keymap.rs`、`tests/mouse.rs`、`tests/workspace_tabs.rs`。

1. 用共享 label helper 渲染所有 Tab 的名称和连接后缀，真实名称保持不变。
2. 测试 Unicode 宽度、长名称、同名 profile、缺失 profile 和窄终端；布局和鼠标 hitbox 使用同一最终宽度。
3. Space+s 保持 Console 管理器入口；按名称/连接/database/schema 搜索所有打开和关闭文档。
4. 排序建议为当前 Console、其他已打开、已关闭，各组名称和 UUID 稳定排序；移除 default 优先级。
5. 推荐将 Space+d 扩展为当前 Console 的执行目标选择器（profile→database/schema），保留现有 database 快速选择能力；更新键位帮助和鼠标入口。
6. 显示连接状态与文档状态两列；结果/Output 的来源与目标栏一致，不只依赖颜色。
7. 空工作区提供新建 Console、打开管理器提示；从空工作区调用两入口可正常工作。
8. 运行 `cargo test --test ui_render --test keymap --test mouse --test workspace_tabs`。

**完成标准：** 用户无需执行 SQL 就能辨识、切换和管理跨连接文档；键盘与鼠标行为一致。

### T13：集成回归、驱动验证与文档交付

**修改：** `tests/multi_connection.rs`、`tests/offline_consoles.rs`、`tests/connection_switch.rs`、`tests/workspace_persistence.rs`、`docs/architecture.md`、`docs/keybindings.md`、`README.md`；相关文档校验 `tests/docs.rs`。

1. SQLite 使用两个临时数据库文件和不同 marker，验证并发结果归属与断开隔离；不使用两个独立 :memory: 连接假装同一数据库。
2. Tokio 通道/受控完成顺序测试所有 race，不依赖 sleep 猜测时序。
3. 对可用的 PostgreSQL/MySQL/SQL Server/Oracle 环境验证同 profile 多 database/schema、事务隔离和能力/目录归属；按现有 adapter 测试入口执行，缺环境明确列出未验证项。
4. 将旧 connection_switch 中单连接契约测试改为多连接语义，保留“失败不损坏旧资源”和“迟到事件无效”的核心断言。
5. 更新架构中的单池和激活联网说明、快捷键、关闭/删除差异、未绑定和断开行为。
6. 执行最终检查：

```bash
cargo fmt --all -- --check
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo test
```

7. 若默认 Oracle 特性受本机环境影响，记录原因并补跑 `cargo test --no-default-features`；不能将该结果表述为默认特性全部通过。
8. 手工按第 5 节验收矩阵逐项操作，记录实际结果及驱动验证范围。

**完成标准：** 核心离线、并发、事务和迁移场景全部通过；文档和 UI 帮助反映最终行为。

## 5. 发布前验收矩阵

| 编号 | 场景 | 预期 |
| --- | --- | --- |
| A01 | A/B 同时连接，任意切换 Explorer/Tab | 两连接保留，Tab 不消失 |
| A02 | A 长查询，B 连接和执行，A 最后返回 | 结果各归原 Console，焦点不跳 |
| A03 | 同 profile 的不同 database/schema Console | 查询目标和事务不互相改变 |
| A04 | 同目标多个 Console 同时首次执行 | 一次连接尝试，各 SQL 只提交一次 |
| A05 | A 连接失败或取消，B 在线 | B 不受影响，A SQL 保留 |
| A06 | 零在线连接新建、编辑、自动保存、重启 | 文档完整恢复，数据库调用为零 |
| A07 | 无配置创建未绑定 Console，首次执行 | 先选择目标，取消不执行 |
| A08 | 等待连接时切 Tab、修改 SQL | 执行原 SQL 快照，归属原 Console |
| A09 | 等待连接时取消/关闭/删除 | 迟到成功不执行已取消 SQL |
| A10 | A→B 重绑但 B 离线 | ID/SQL/undo 保留，立即持久化，未连接 |
| A11 | A 活跃事务时重绑/关闭/断开 | 先走事务退出选择；其他 profile 不受阻 |
| A12 | 关闭/删除最后一个 Console | 空工作区合法，不补建 default |
| A13 | profile 重命名、删除、配置更新 | 标签更新；删除保留失效文档；更新不影响其他 profile |
| A14 | Space+s 搜索连接名，打开离线已关闭文档 | 可找到并打开，无网络操作 |
| A15 | v4 多 profile 迁移、写失败、再启动 | 内容/ID/归属保留，失败可恢复 |
| A16 | A 断开后 B 的目录/Relation/Dashboard 返回 | B 正常更新；A 迟到结果被拒绝 |
| A17 | 同名连接及中文长标签、鼠标选择 | 后缀可辨识，宽度和点击位置正确 |
| A18 | 所有 profile 下多个活跃事务退出 | 全量事务审查后刷盘并关闭全部资源 |

## 6. 里程碑与交付方式

- **M1：文档独立** — T01～T04。v5 迁移、空工作区、离线生命周期成立。
- **M2：运行时独立** — T05～T08。多目标并发、异步路由、目录及事务隔离成立。
- **M3：用户流程完整** — T09～T12。重绑、首次执行、默认目标、标签和管理器完整。
- **M4：可交付** — T13 和全部验收矩阵通过。

执行时建议每完成一个任务评审一次 diff 和相关测试结果，再推进依赖任务。提交由用户另行授权，不自动提交或发布。本文是实施计划，未表示功能代码或测试已完成。
