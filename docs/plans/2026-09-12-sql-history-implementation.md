# SQL History Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行环境若没有上述技能，按本文任务、依赖、测试与验收门槛逐项执行；不要假定未提供的技能可用。

**Goal:** 增加跨 Console、跨连接、可持久化的 SQL History 面板，准确展示执行时间、耗时、数据库、行数、执行结果和事务结果，支持完整 SQL 复制与鼠标拖选复制。

**Architecture:** 执行入口创建不可变上下文，数据库适配器和事务 Worker 在真实执行边界产生结构化生命周期事件；独立 HistoryRecorder 将事件幂等写入 SQLite，App 只消费历史查询与变更通知。History 是单例工作区标签，数据不归标签所有；查询结果区继续保留原有 generation 防过期机制。执行状态、结果确定性和事务结果分别建模。

**Tech Stack:** Rust 2024、Tokio、SQLx SQLite、Serde、Chrono、UUID、Ratatui、Crossterm，以及项目现有 EditorWorkspace、TextDetail、ClipboardPayload、OSC52。

---

## 0. 范围、前提与执行约定

- 基于 2026-09-12 当前代码，定位用符号和文件路径，行号仅作参考。
- 本文件是实施计划；编写计划时未执行实现测试，文中命令及结果均为实施阶段要求。
- 现有未跟踪的其他计划文件属于工作区已有内容，不纳入本功能提交。
- 实施前建议创建专用 feature 分支或 worktree；每个任务完成一个可编译的提交，提交范围只包含该任务文件。
- 单任务内按照“行为测试 → 确认失败 → 最小实现 → 定向测试 → 提交”执行。每个编号步骤再按测试用例逐个推进，单次动作控制在约 2–5 分钟；驱动接入按数据库分提交。
- 不把整项功能的实现代码预写进计划；下文的数据定义是冻结的语义契约，内部 API 可按编译反馈调整，但不得改变未知状态、记录粒度和事务语义。
- 实施中若现有多连接 Console 计划已合入，以新的连接管理 API 接入，History 不依赖单一活动连接。

### 0.1 最终覆盖范围

“所有 SQL”定义为 LazyDB 应用层实际向数据库驱动提交的 SQL/事务命令，包括 Console、Relation、Catalog、Agent，以及内部元数据和监控查询。驱动握手协议、驱动内部不可见 SQL、数据库存储过程内部语句、其他客户端 SQL 不属于可观测范围。

记录与默认展示分开：完整模式记录用户及内部执行；默认列表显示用户相关操作，内部执行可通过来源筛选查看。历史库自己的 SQL 不进入记录链路。

计划覆盖所有来源后才可标记“完整覆盖”。第一阶段 Console 可用不等于整个功能完成。

### 0.2 不变量

1. 已接受的每次执行都有独立 `execution_id`；关联 UI 的 `(tab_id, generation)` 不是主键。
2. 实际 SQL 未提交时，不能显示“执行失败且影响 0 行”；可记录为准备失败/未执行，`started_at` 为空。
3. 同一 SQL 成功与事务回滚可以同时成立。
4. `affected_rows = NULL` 表示未知或不适用；`0` 表示驱动确认的零行。
5. 取消意图不证明 SQL 停止，超时不证明事务回滚。
6. 记录完成不依赖标签是否存在、当前连接是否匹配。
7. 原始 SQL、实际 SQL、连接名称、database/schema 保存执行时快照。
8. 不通过重新拆分并执行 SQL 改变现有批处理语义。
9. 复制完整原文；终端展示继续使用现有文本清理与投影，不能直接输出 SQL 中的终端控制序列。
10. 不默认删除旧历史。容量/天数清理必须由显式配置启用。

## 1. 当前接入地图

| 位置 | 当前行为 | 实施动作 |
| --- | --- | --- |
| `src/sql/execution.rs::ExecutionDraft` | SQL、执行目标、事务与文档快照 | 创建执行上下文的主要输入 |
| `src/runtime.rs::run_query` | 直接执行，错误转字符串 | 统一生命周期与结构化完成信息 |
| `src/runtime.rs::manual_execute` | 通过事务 Worker 执行 | 上下文随 TransactionRequest 传递 |
| `src/runtime.rs::run_query_page` | COUNT + 重建分页 SQL + 页面查询 | 父操作与真实 SQL 子记录 |
| `src/runtime.rs::run_derived_query` | 派生查询独立入口 | 标记来源和父操作 |
| `src/db/query.rs::QueryOutcomeAccumulator` | 客户端 execution/fetch 和截断统计 | 提取摘要，不保留结果内容 |
| `src/db/transaction.rs::TransactionError` | `pub String`，含 relation 特殊桥接编码 | 迁移为结构化错误，保留现有诊断显示语义 |
| `src/runtime/transaction.rs` | begin、execute、commit、rollback、cancel、强制关闭 | 发出执行与事务事实事件 |
| `src/model/transaction.rs::transition` | 当前事务状态机 | 历史投影遵循同样事实，不依赖当前 UI 状态 |
| `src/app.rs::QueryFinished/QueryFailed` 分支 | generation/连接不匹配即忽略 | 保留 UI 丢弃；历史由独立记录器处理 |
| `src/model/tab.rs::WorkspaceTab` | Sql、Relation、Dashboard | 新增 History 分支 |
| `src/model/text_detail.rs`、`src/ui/text_selection.rs` | 文本详情、源位置投影 | 复用 SQL 详情和选区复制 |
| `src/input/mouse.rs::map_mouse` | Editor/TextDetail 专用拖选分支 | 增加 HistoryPreview 只读来源 |
| `src/agent/service.rs` | 不经过 TUI Runtime | 使用共享 HistoryRecorder 接口 |
| `src/persistence/paths.rs::AppPaths` | 独立目录与文件路径 | 新增历史库路径 |

## 2. 冻结的数据与事件契约

### 2.1 记录层次

- **Operation**：一次用户操作，例如执行 SQL、翻到末页、提交事务或修改一行。
- **Execution**：一次实际提交给驱动的 SQL/事务命令。Operation 可以拥有多个 Execution。
- **Statement projection**：对一个批次做静态解析后的语句展示，只有驱动能够可靠关联时才填逐句结果。
- **Transaction**：跨多次 Execution 的事务事实与最终结果。

父操作只聚合展示，不同时作为一条真实 SQL 参与执行次数、影响行数统计。准备失败保留在操作记录中，实际 SQL 列表可显示“未执行”，但不计入已提交 SQL 数量。

### 2.2 类型语义

新增 `src/model/sql_history.rs`，冻结以下枚举的含义；Serde 使用 snake_case，时间统一 UTC 存储、本地时区显示。

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryExecutionStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
    Interrupted,
    NotExecuted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryResultCertainty {
    Confirmed,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryTransactionOutcome {
    NotApplicable,
    Pending,
    AutoCommitted,
    Committed,
    RolledBack,
    RolledBackToSavepoint,
    Unknown,
}
```

取消中由 `cancel_requested_at` 派生，不立即改为终态。本地超时但服务端结果无法确认时显示“超时 / 结果未知”。确认无执行效果或读操作不需要提交的情形可显示“不适用”；手动事务 SELECT 仍展示其所属事务归属。

### 2.3 持久化表

使用版本化建表 SQL：`src/persistence/sql_history/schema_v1.sql`，通过现有 SQLx 运行时查询 API 执行，无需为此引入编译期数据库校验配置。

| 表 | 必需列 |
| --- | --- |
| `history_sessions` | session_id、process_id、instance_token、started_at、last_seen_at、closed_at、lifecycle |
| `history_operations` | operation_id、session_id、source、owner_id、requested_at、finished_at、original_sql、status、preparation_error |
| `history_executions` | execution_id、operation_id、session_id、sequence、revision、profile_id、connection_generation、connection_name、database_kind、database_name、schema_name、query_generation、transaction_id、transaction_sequence、requested_at、started_at、finished_at、elapsed_us、first_event_us、fetch_us、status、certainty、cancel_requested_at、stop_reason、affected_rows、returned_rows、fetched_rows、truncated、error_category、error_code、error_message、transaction_override |
| `history_sql` | execution_id、actual_sql、sql_preview、parameter_metadata |
| `history_transactions` | transaction_id、session_id、owner_kind、owner_id、profile_id、connection_generation、generation、started_at、resolved_at、outcome、resolution_reason、revision |
| `history_transaction_events` | event_id、transaction_id、sequence、execution_id、kind、savepoint_name、boundary_sequence、occurred_at |

约束与索引：

- execution_id、operation_id、transaction_id、event_id 均为 UUID；主键保证幂等。
- 主列表排序键使用 `(requested_at DESC, execution_id DESC)`，未执行时间为空不会破坏分页。
- 索引覆盖 `(profile_id, requested_at, execution_id)`、`(status, requested_at, execution_id)`、`(transaction_id, transaction_sequence)`、`(operation_id, sequence)`。
- 外键不级联删除连接相关历史；profile_id 是快照引用，不依赖 profiles 文件中的记录继续存在。
- 数据库 INTEGER 是有符号 64 位，驱动 u64 行数需受检转换；超界值用无损十进制文本存储策略，禁止强制转换溢出。
- 不持久化结果集内容。参数化 SQL 保存模板及类型/参数数目；只有已有业务策略允许时才保存值，不能臆造一条内联 SQL 冒充真实执行文本。
- 不需要永久保存全部 Started/Finished 原始事件。执行表保留当前投影，事务边界事件单独保存以支持保存点与追溯。

### 2.4 事件与排序

最小事件集合：OperationAccepted、ExecutionQueued、ExecutionStarted、CancellationRequested、ExecutionFinished、TransactionStarted、TransactionBoundaryObserved、TransactionResolved、SessionClosed。

事件携带 event_id、session_id、execution_id/transaction_id、单调 revision。生产者是执行监督器或 Worker，不是 UI。数据库写入采用条件更新，重复事件幂等；已确认终态不可被迟到取消意图覆盖。已提交/回滚的事务解析与迟到执行完成事件独立关联，因此后到的 SQL 记录仍能显示最终事务结果。

确认提交/回滚来自 Worker/适配器，不来自 App 的按钮事件。ClearOutcome 只影响操作界面，不发历史改写事件。

## 3. 实施任务

### Task 1：建立历史领域模型与测试夹具

**依赖：** 无。

**文件：** 新增 `src/model/sql_history.rs`、`tests/sql_history_model.rs`；修改 `src/model/mod.rs`。

1. 新增快照夹具，测试成功与回滚共存、未知行数与零行不同、时区转换不改变存储时间。
2. 执行 `cargo test --test sql_history_model`，预期因模型缺失而失败。
3. 实现上述枚举、ExecutionContext、ExecutionSummary、HistoryFilter、HistoryCursor、事务关联键；Instant 仅存在于运行时，不可序列化。
4. 增加纯状态投影测试：重复完成事件、迟到取消、未知提交、ClearOutcome 不变更历史；实现事件 reducer。
5. 再运行同一测试，预期全部通过；提交 `feat(history): add execution history domain model`。

**验收：** 历史模型不依赖 Ratatui、SQLx 和当前活动连接。

### Task 2：实现 SQLite 存储、迁移与分页

**依赖：** Task 1。

**文件：** 新增 `src/persistence/sql_history.rs`、`src/persistence/sql_history/schema_v1.sql`、`tests/sql_history_store.rs`；修改 `src/persistence/mod.rs`、`src/persistence/paths.rs`。

1. 用 tempfile 建库测试首次创建、二次打开、schema 版本验证、完整 SQL 往返和同时间戳分页。
2. 执行 `cargo test --test sql_history_store`，确认失败。
3. 增加 `AppPaths::history_file()` → `state_dir/sql-history.sqlite3`，同步目录识别的 `has_lazydb_data`。
4. 实现版本化 schema、WAL、外键、短事务和 busy timeout；先实现插入、完成更新、事务解析、列表与详情查询。
5. 使用 `(requested_at, execution_id)` 游标；参数绑定实现组合筛选，文字搜索转义 LIKE 通配符以实现字面子串语义。
6. 实现条件 revision 更新与事务事件幂等；补充 finish-before-transaction-resolution 和反向顺序测试。
7. 再运行存储测试及 `cargo test --test persistence`；提交 `feat(history): persist history with cursor pagination`。

**验收：** 列表查询不读取整条 actual_sql；连接改名或删除不会改写历史快照；新版本数据库不能被旧实现破坏性重建。

### Task 3：实现共享 Recorder、队列与可靠性边界

**依赖：** Task 2。

**文件：** 新增 `src/history.rs`、`tests/sql_history_recorder.rs`；修改 `src/lib.rs`、`src/runtime.rs`。

1. 测试多个生产者、重复事件、写锁暂时占用、写入失败后重试、flush 与关闭。
2. 执行 `cargo test --test sql_history_recorder`，确认失败。
3. 实现可克隆 HistoryRecorder、无 I/O 的测试 sink、进程内单写队列和持久化确认。
4. 队列有界；异步执行任务允许等待队列容量，App/UI 不等待磁盘。活动摘要缓存有上限；背压时 UI 仍可交互。
5. 区分“已接收”“已持久化”。正常路径先持久化开始记录再提交驱动；完成记录等待落盘后发布持久化确认。该等待计入记录开销，不计入 SQL 执行耗时。
6. 持久化不可用时进入明确的降级记录模式，保留有界重试缓存并通知；不得声称仍能完整保存所有历史。缓存满时汇总无法保存的数量，避免静默丢失或无限内存。
7. 测试通过后提交 `feat(history): add shared recorder and persistence queue`。

**验收：** SQL 执行结果不被历史库写入失败改写；没有使用 Drop 中的异步写入作为唯一收尾保障。

### Task 4：保留结构化错误与停止原因

**依赖：** Task 1。

**文件：** 修改 `src/db/mod.rs`、`src/db/transaction.rs`、`src/db/postgres.rs`、`src/db/mysql.rs`、`src/db/sqlite.rs`、`src/db/mssql.rs`、`src/db/oracle.rs`、`src/runtime/transaction.rs`、`src/runtime.rs`；新增 `tests/sql_history_errors.rs`。

1. 编写错误转换测试，覆盖 SQL 错误码、连接丢失、本地超时、用户取消、确认丢失和 relation diagnostic。
2. 执行 `cargo test --test sql_history_errors`，确认失败。
3. 把 TransactionError 从 tuple string 迁移为结构化错误，提供 message、code、category、certainty、relation diagnostic；逐驱动替换构造和 `.0` 访问。
4. 保持现有 Output 的可读错误格式和 relation 诊断的字段控制；避免为了 History 回填原先刻意不输出的服务端值。
5. 增加 Timeout 分类及独立 StopReason；优先使用驱动错误类型/错误码，不能仅用英文字符串 contains 判定。无法区分 timeout/cancel 的数据库代码结合本地停止原因，否则保留一般取消/未知。
6. 执行 `cargo test --test sql_history_errors --test transaction_reducer --test relation_runtime` 及 `cargo check --all-targets`；提交 `refactor(db): preserve structured execution failures`。

**验收：** 错误进入 UI 前仍可被 Recorder 完整分类；Oracle 条件编译和现有 Fake Backend 全部适配。

### Task 5：打通 Console 自动执行的真实记录边界

**依赖：** Tasks 3、4。

**文件：** 新增 `src/db/execution_observer.rs`、`tests/sql_history_runtime.rs`；修改 `src/db/mod.rs`、`src/runtime.rs`、`src/sql/execution.rs`、`src/action.rs`、`src/app.rs` 和五个驱动文件。

1. 用 SQLite 测试 SELECT、零行 UPDATE、语法失败、执行时数据库快照和完整 SQL。
2. 执行 `cargo test --test sql_history_runtime`，确认失败。
3. App 派发不可变上下文；Runtime 创建 operation/execution ID，并在业务执行边界安装显式 observer 上下文。
4. Runtime 只记录操作生命周期；适配器记录实际 SQL，避免同一 SQL 两次计数。跨 spawn_blocking 的 Oracle 路径显式传递 observer，不依赖 Tokio task-local 隐式继承。
5. 在驱动调用前后产生开始/完成，失败也由外层 Instant 记录总耗时；从 QueryOutcome 提取首事件/fetch/行数摘要。
6. SELECT 驱动返回的“处理行数”不自动认定为 DML 影响行数；单语句可结合分类和驱动结果，批次不能可靠归属的保留原始统计与未知。
7. 测试延迟完成后关闭 Console、切换连接、提高 generation：结果区拒绝旧结果，History 仍完成。
8. 同时运行 `cargo test --test sql_history_runtime --test sql_execution --test connection_switch`；提交 `feat(history): record console executions independently of tabs`。

**验收：** 不从 `OutputEntry.message` 反解析 SQL；准备失败无实际开始时间；驱动层不会递归记录历史库查询。

### Task 6：接入手动事务与提交/回滚回填

**依赖：** Task 5。

**文件：** 修改 `src/db/transaction.rs`、`src/runtime/transaction.rs`、`src/runtime.rs`、`src/model/sql_history.rs`、`src/persistence/sql_history.rs`；新增 `tests/sql_history_transactions.rs`。

1. 用事务 Fake 和 SQLite 编写 UPDATE→COMMIT、UPDATE→ROLLBACK、语句失败→ROLLBACK、COMMIT 拒绝后再次操作的测试。
2. 执行 `cargo test --test sql_history_transactions`，确认失败。
3. 给 TransactionRequest 增加执行上下文与 transaction ID；事务内维护单调 execution sequence。
4. Worker 的实际 begin/commit/rollback 各产生命令记录；按钮触发的命令保存规范化文本及来源，用户输入命令同时保留原文。
5. 事务解析更新事务表，普通语句通过关联读取最终结果；不用逐行无条件改写所有 SQL 状态。
6. 提交被明确拒绝保留 Pending；提交确认丢失标 Unknown；shutdown 回滚成功记录 RolledBack。
7. 测试先收到事务解析、后收到 SQL 完成也正确；测试 ClearOutcome 不触碰历史。
8. 执行 `cargo test --test sql_history_transactions --test sqlite_transactions --test quit_transaction_review`；提交 `feat(history): track transaction outcomes across executions`。

### Task 7：取消、超时与执行监督器

**依赖：** Task 6。

**文件：** 新增 `src/runtime/execution.rs`、`tests/sql_history_cancellation.rs`；修改 `src/runtime.rs`、`src/runtime/transaction.rs`、`src/action.rs`、`src/app.rs`。

1. 用可控 oneshot/Fake 驱动测试取消先到、成功先到、取消失败、手动取消回滚成功、ack 丢失。
2. 执行 `cargo test --test sql_history_cancellation`，确认失败。
3. 引入监督器持有 execution ID 和最终化责任；底层 JoinHandle abort 不能让记录停留 Running。
4. 取消先记意图，再走已有驱动取消/清理流程；不能确认数据库停止时记录 Interrupted/Unknown，而不是 Cancelled/Confirmed。
5. 数据库超时分类沿用 Task 4；本地 query timeout 为可选配置、默认关闭。启用时走同一个停止和事务清理协议，不能只包 timeout 后丢弃 Future。
6. 事务取消保留 `CancelledAndRolledBack` 与 Quarantine 差异；结果未知与事务未知分别表达。
7. 测试未提供服务端取消能力的适配器仍能准确显示“结果未知”；通过后提交 `fix(history): finalize cancelled and timed out executions accurately`。

**验收：** 同一 execution 只有一个权威最终结果，迟到取消不会覆盖已确认成功。

### Task 8：崩溃恢复、多实例与退出刷盘

**依赖：** Task 7。

**文件：** 修改 `src/history.rs`、`src/persistence/sql_history.rs`、`src/runtime.rs`；新增 `tests/sql_history_recovery.rs`。

1. 测试两个 Recorder 共享同一库，一个退出不影响另一个 Running；测试 PID 重用/过期心跳不构成存活证明。
2. 执行 `cargo test --test sql_history_recovery`，确认失败。
3. 会话存储 instance token 和进程身份；正常退出写 SessionClosed。启动恢复仅最终化有证据已结束的旧会话。
4. 心跳超时只标记失联/待确认，不直接把仍可能运行的 SQL 判为中断；平台无法可靠判定旧进程时保留未知状态说明。
5. 先完成事务退出流程，再等待 Recorder flush，最后关闭存储；刷新超时保留未完成证据供下次恢复。
6. 不推断崩溃后的事务一定回滚；已持久化的确认提交结果保持不变。
7. 执行恢复测试和 `cargo test --test workspace_persistence --test quit_transaction_review`；提交 `feat(history): recover interrupted sessions without cross-instance corruption`。

### Task 9：新增 History 标签与纯 UI 状态

**依赖：** Tasks 2、5。

**文件：** 新增 `src/model/history_tab.rs`、`src/app/history.rs`、`tests/sql_history_app.rs`；修改 `src/model/mod.rs`、`src/model/tab.rs`、`src/model/workspace.rs`、`src/app.rs`、`src/action.rs`、`src/runtime.rs`、`src/persistence/workspace.rs`。

1. 测试单例打开、关闭后重开、无活动连接仍可打开、筛选变化丢弃旧查询回调、选择稳定。
2. 执行 `cargo test --test sql_history_app`，确认失败。
3. 新增 WorkspaceTab::History，补齐 id/title/kind、所有枚举匹配、关闭/排序/激活路径。
4. HistoryTab 只保留列表摘要页、选中 execution ID、过滤器、分页游标、request generation、详情缓存和内部焦点。
5. 新增 OpenSqlHistory、LoadSqlHistory、SqlHistoryLoaded、LoadSqlHistoryDetail、SqlHistoryDetailLoaded 等 Action/Command；I/O 仅在 Runtime。
6. 历史使用全宽主内容布局与局部 List/Preview 焦点，不把它硬塞为 Console 的 ResultView，也不新增全局第四个 Focus。
7. 持久化 History 标签位置、过滤器和选中 ID，不保存历史正文或缓存；恢复时 ID 不存在则选择首条。
8. 执行 `cargo test --test sql_history_app --test workspace_tabs --test workspace_persistence`；提交 `feat(history): add singleton history workspace tab`。

### Task 10：列表、筛选、详情与响应式布局

**依赖：** Task 9。

**文件：** 新增 `src/ui/sql_history.rs`、`tests/sql_history_ui.rs`；修改 `src/ui/mod.rs`、`src/ui/layout.rs`、`src/model/history_tab.rs`。

1. 用 Ratatui TestBackend 测试空态、加载、读取失败、窄屏、宽屏、不同事务状态以及超长 SQL。
2. 执行 `cargo test --test sql_history_ui`，确认失败。
3. 宽屏显示时间、耗时、数据库、行数、执行状态、事务结果、SQL；窄屏保留时间/状态/耗时/SQL，详情展示完整字段。
4. 默认最新在前，SQL 单行预览预计算；多行正文仅选中时加载，列表渲染限于可见行。
5. 增加连接、数据库、时间、执行状态、事务结果、来源筛选；文本搜索 debounce 约 200ms，并使用 request generation 丢弃旧结果。
6. 详情明确首事件时间不是服务端精确执行时间；总耗时不包含 History 落盘与 UI 渲染，等待时间独立展示。
7. 顶部跟随模式更新时维持选择锚点；用户浏览旧页时只提示新增数量。
8. 内进程变更订阅触发去抖刷新；跨进程变化仅在 History 可见时轻量检查数据库变化版本，不全表轮询。
9. 执行 `cargo test --test sql_history_ui --test ui_render`；提交 `feat(history): render searchable history list and SQL details`。

### Task 11：完整复制、拖选与键盘导航

**依赖：** Task 10。

**文件：** 修改 `src/ui/sql_history.rs`、`src/ui/text_selection.rs`、`src/ui/text_detail.rs`、`src/model/text_detail.rs`、`src/input/mouse.rs`、`src/input/keymap.rs`、`src/config.rs`、`src/help.rs`、`src/action.rs`、`src/app/history.rs`；新增 `tests/sql_history_interaction.rs`。

1. 测试复制长 SQL 不截断、鼠标跨行拖选、中文/Tab/水平滚动、切换记录后旧选区失效。
2. 执行 `cargo test --test sql_history_interaction`，确认失败。
3. 列表复制使用完整原始 SQL，详情提供复制实际 SQL；按钮说明区分两者。拖选基于源文本范围，不附带边框、行号。
4. 复用 TextDetail 显示完整文本，并能返回 History；预览增加 HistoryPreview 来源，所有手势携带 session ID/revision。
5. SQL 预览保持原始换行、默认水平滚动；不默认格式化，避免选区与原文无法映射。
6. 接入现有 ClipboardPayload、arboard/OSC52 路径与现有鼠标松开复制习惯；OSC52 超限明确失败，不截断。
7. 增加 j/k、分页、gg/G、/ 搜索、Tab 切换列表/预览、Enter 详情；全局打开键和局部复制键经过冲突检查后加入可配置绑定与帮助。
8. “放入新 Console”加载历史 SQL 和目标快照，连接失效时保留文本并提示目标不可用，不自动执行。
9. 执行 `cargo test --test sql_history_interaction --test mouse --test keymap --test editor_projection`；提交 `feat(history): support keyboard navigation and mouse SQL copying`。

**里程碑 A：** Tasks 1–11 完成后，Console 自动/手动历史可用，保存、事务、复制和重启语义可靠。

### Task 12：分页、派生查询与批次投影

**依赖：** Tasks 6、10。

**文件：** 修改 `src/runtime.rs`、`src/runtime/transaction.rs`、`src/db/query.rs`、`src/sql/execution.rs`、`src/model/sql_history.rs`、`src/ui/sql_history.rs`；新增 `tests/sql_history_batches.rs`。

1. 测试翻末页 COUNT+page 两条实际 SQL、COUNT 失败不伪造页面执行、派生查询来源。
2. 测试多语句中途失败、多结果集、SQL Server GO、Oracle PL/SQL 的不可拆分边界。
3. 执行 `cargo test --test sql_history_batches`，确认失败。
4. 每次用户操作生成 parent operation；每个实际驱动调用生成唯一子 Execution，原文和实际 SQL 分离。
5. 批次仅对可靠观测到的逐句事件填充统计；没有证据时展示“批次失败，逐句结果未提供”，不按结果集索引猜测语句。
6. 查询预算截断在 UI 显示返回/实际获取数量差异，不将预算截断当执行失败；分页总耗时覆盖 COUNT 和 page，单条耗时各自保留。
7. 执行 `cargo test --test sql_history_batches --test sql_batch --test sql_scope --test sql_execution`；提交 `feat(history): group paginated and batch executions without changing SQL semantics`。

### Task 13：保存点、隐式提交与驱动事务能力

**依赖：** Tasks 6、12。

**文件：** 修改 `src/sql/transaction.rs`、`src/db/transaction.rs`、`src/runtime/transaction.rs`、`src/db/postgres.rs`、`src/db/mysql.rs`、`src/db/sqlite.rs`、`src/db/mssql.rs`、`src/db/oracle.rs`、`src/persistence/sql_history.rs`；扩展 `tests/sql_history_transactions.rs`、`tests/transaction_sql.rs`。

1. 测试 SAVEPOINT 前后多条 SQL、ROLLBACK TO 后再次写入并 COMMIT、同名保存点覆盖和 RELEASE。
2. 测试隐式提交前的事务与 DDL 自身结果分别记录，尤其 DDL 失败但前一个事务已提交的情况。
3. 执行 `cargo test --test sql_history_transactions --test transaction_sql`，确认新增行为测试失败。
4. 记录保存点作用边界；回滚区间设置语句级 transaction_override，最终 COMMIT 不覆盖已回滚区间。
5. 适配器报告确认的边界、嵌套事务深度和隐式提交证据；不能把 SQL Server 内层 COMMIT 当作外层事务最终提交。
6. 只有库/驱动允许可靠推断的自动提交记 AutoCommitted；失败批次可能部分提交时保留混合/未知摘要。
7. Fake 覆盖完整事件矩阵，再按已有连接测试约定执行真实驱动测试；提交 `feat(history): track savepoint and implicit transaction boundaries`。

### Task 14：Relation 编辑与 Catalog SQL 全覆盖

**依赖：** Tasks 12、13。

**文件：** 修改 `src/runtime.rs`、`src/db/mutation.rs`、`src/db/catalog_mutation.rs`、`src/db/catalog_drop.rs`、五个驱动文件和 `src/runtime/transaction.rs`；新增 `tests/sql_history_sources.rs`。

1. 测试单元格更新、批量删除、插入、DDL 创建/修改/删除及失败路径。
2. 执行 `cargo test --test sql_history_sources`，确认失败。
3. 从用户操作创建 operation，上下文传递到 SQL 构造与实际 execute/fetch 边界；每条参数化 SQL 保留模板，不从变更后的行数据反构造 SQL。
4. 标记业务修改、冲突检查、回读和元数据辅助查询；一次编辑可展开所有实际 SQL。
5. Relation 手动事务加入同一个 transaction ID；取消编辑不能删除已经执行的历史。
6. 对所有 execute/query/fetch/call 的应用层驱动提交点建立覆盖清单，逐个确认是否触发 observer；内部直接调用不能因绕过 DatabaseConnection::execute 而漏记。
7. 执行 `cargo test --test sql_history_sources --test relation_runtime --test catalog_mutation --test catalog_drop`；提交 `feat(history): record relation and catalog executions`。

### Task 15：Agent 与内部查询接入、跨进程可见

**依赖：** Tasks 8、14。

**文件：** 修改 `src/agent/service.rs`、`src/agent/cli.rs`、`src/agent/mcp.rs`、`src/agent/catalog.rs`、`src/db/mod.rs`、五个驱动文件、`src/history.rs`；新增 `tests/sql_history_agent.rs`；扩展 `tests/sql_history_sources.rs`。

1. 测试 CLI/MCP 查询和写入落到同一历史库，TUI 可读；多个实例同时写入无丢行。
2. 执行 `cargo test --test sql_history_agent --test sql_history_sources`，确认失败。
3. AgentService 注入共享 Recorder，不构造 TUI App；标记 agent_cli/agent_mcp 来源和当前项目上下文。
4. 短命 CLI 返回前等待该操作 flush；MCP 长会话复用 Recorder；原有 JSON 输出格式保持兼容，历史故障不得混入 stdout 协议。
5. 内部监控、catalog、连接探测/初始化等可观测 SQL 都显式标记 Internal 来源；默认列表过滤，不丢弃记录。
6. 添加覆盖测试确认 HistoryStore I/O 不递归产生 History；TUI 看见 Agent 新记录不依赖重新启动。
7. 执行 `cargo test --test sql_history_agent --test agent_service --test agent_cli --test agent_mcp --test agent_serialization`；提交 `feat(history): include agent and internal SQL executions`。

**里程碑 B：** Tasks 12–15 完成并核对提交点清单后，达到 LazyDB 可观测 SQL 的完整覆盖。

### Task 16：配置、容量控制与性能优化

**依赖：** Tasks 10、15。

**文件：** 修改 `src/config.rs`、`src/persistence/settings.rs`、`src/persistence/sql_history.rs`、`src/history.rs`、`src/model/history_tab.rs`；新增 `tests/sql_history_performance.rs`。

1. 测试旧 settings 无 history 配置仍能加载，默认记录启用、内部查询默认隐藏、默认不清理。
2. 加入 history.enabled、history.include_internal、history.default_show_internal、history.retention_days、history.max_entries；include_internal 默认 true，关闭时界面明确显示覆盖范围。
3. 分页大小默认 100、缓存建议上限 500 条摘要，活动执行单独管理；SQL 正文按选中项少量缓存。这些是初始参数，可用测量调整。
4. 清理仅处理已经终结且超过用户策略的操作；活跃事务与未完成记录不可删除；事务边界需与关联执行一致清理。
5. 构造 100,000 条历史、不同长度正文，测试深分页、状态/连接筛选、SQL 搜索和持续写入时渲染。
6. 执行 `cargo test --release --test sql_history_performance -- --nocapture`。先记录同机基线，目标：100k 记录列表首屏/游标分页 p95 < 100ms，内存随缓存上限而非历史总量增长；普通 CI 不使用易抖动的绝对耗时断言。
7. 若 SQL 子串扫描超过目标，单独增加搜索索引方案并验证中文、标点与子串语义；不未经测量就引入 FTS，也不宣称普通 B-tree 能优化 `%keyword%`。
8. 通过后提交 `perf(history): bound caches and optimize indexed history browsing`。

### Task 17：文档、完整回归与人工验收

**依赖：** Tasks 1–16。

**文件：** 新增 `docs/sql-history.md`；修改 `README.md`、`docs/architecture.md`、`docs/configuration.md`、`docs/keybindings.md`、`docs/database-capabilities.md`、`docs/coding-agent-access.md`、`docs/performance.md`；按需要扩展 `tests/docs.rs`。

1. 文档写明字段口径、来源范围、自动提交/回滚/未知、批次限制、实际 SQL 和原始 SQL、快捷键及存储位置。
2. 完成 Task 14 的提交点清单，逐驱动标明逐句统计、取消确认、保存点和隐式提交能力。
3. 顺序执行以下最终检查，各项通过后不无故重复：

   ```bash
   cargo fmt --all -- --check
   cargo check --all-targets
   cargo test --all-targets
   cargo check --all-targets --no-default-features
   cargo test --no-default-features --test sql_history_model --test sql_history_store --test sql_history_runtime --test sql_history_transactions --test sql_history_interaction
   cargo clippy --all-targets --all-features -- -D warnings
   ```

4. 真实数据库测试沿用 `tests/postgres_adapter.rs`、`tests/mysql_adapter.rs`、`tests/sqlserver_transactions.rs`、`tests/oracle_adapter.rs` 的环境变量与 fixture 约定；实施时先核对当前约定。缺少服务/Oracle 客户端导致的提前 return 必须记为“未验证”，不能算数据库验收通过。
5. 人工验证本地终端和支持 OSC52 的远程终端：打开 History、过滤、拖选中文 SQL、长 SQL 复制、事务回填、关闭重开、并行 Agent 写入。
6. 更新实际执行记录和剩余能力限制；提交 `docs(history): document SQL history and verified capabilities`。

## 4. 依赖与交付门槛

```text
1 → 2 → 3 ─┐
1 → 4 ─────┴→ 5 → 6 → 7 → 8
2 + 5 → 9 → 10 → 11                 里程碑 A（同时要求 6–8）
6 + 10 → 12 → 13 → 14
8 + 14 → 15                          里程碑 B
10 + 15 → 16 → 17                    最终验收
```

按依赖顺序执行；每个提交保持可编译。驱动接入需控制共享错误类型、TransactionRequest 和 observer 契约的变更顺序，避免同时修改同一接口产生大规模冲突。

## 5. 最终验收矩阵

| 场景 | 必须观察到的结果 |
| --- | --- |
| SELECT、UPDATE 零行、SQL 语法错误 | 时间、目标、执行状态、行数口径准确 |
| 查询返回被截断 | 成功 + 截断提示，返回行数不冒充总行数 |
| 手动 UPDATE 后 ROLLBACK | 执行成功 / 已回滚 |
| COMMIT 明确拒绝 | COMMIT 失败，原事务仍 Pending |
| COMMIT ack 丢失 | 事务 Unknown，不显示已回滚 |
| ClearOutcome | 当前界面清理，历史 Unknown 保留 |
| ROLLBACK TO 后 COMMIT | 回滚区间保留 RolledBackToSavepoint |
| 隐式提交 DDL 失败 | 前一事务与 DDL 自身效果分别呈现 |
| SQL Server 内层 COMMIT | 不提前标记整个事务 Committed |
| 取消/成功竞态 | 单个权威结果，不被迟到意图覆盖 |
| 本地超时 | TimedOut，结果确定性和事务结果有依据 |
| 关闭标签、切换连接 | 旧结果不污染 UI，History 仍结束 |
| 批次中途失败 | 不伪造后续语句成功或逐句耗时 |
| 分页 COUNT 失败 | COUNT 失败，page 不显示已执行 |
| 崩溃与两个实例并存 | 不误伤活会话，不推断回滚 |
| Agent 短命 CLI | 退出前正常 flush，TUI 可查看记录 |
| 大 SQL 与中文拖选 | 复制原文准确、无边框/行号/显示截断 |
| 参数化 Relation SQL | 显示真实模板，不冒充内联 SQL |
| 历史库故障 | 有明确降级与保存失败反馈，业务结果不被改写 |
| 100k 历史与持续新增 | 游标稳定、选中项稳定、内存有界 |

## 6. 实施记录模板

每个任务完成后在对应任务末尾补充：

- 状态：待实施 / 进行中 / 完成 / 阻塞。
- 实际提交 ID。
- 执行过的测试命令与结果。
- 真实数据库验证环境与未验证项。
- 与计划偏差及原因。

最终交付包含可运行功能、数据库 schema、迁移逻辑、来源覆盖清单、验证结果和用户文档。
