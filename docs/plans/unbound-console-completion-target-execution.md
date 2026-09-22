# Unbound Console Completion and Execution — Implementation Plan

**执行人：** Luna。Astra 仅完成本计划；不启动子 Agent，不实施业务代码，不提交或合并。用户指定任务目录优先于通用 skill 的 docs/plans 路径；无需额外执行方式选择。

**Goal:** 未绑定 Console 提供通用 SQL 补全，执行时选择目标后自动连接并继续原 SQL。

**Architecture:** 补全复用 Generic 方言和空 catalog；目标选择器持有一次性执行意图。冻结 Console、文档 revision、光标和选区，选择目标后用目标方言解析，再复用 session/PendingExecution/ExecutionDraft 链路。

**Tech Stack:** Rust 2024、现有 App action/command reducer、modalkit editor、Tokio runtime、SQLite 测试。

---

## 基线、限制与交接

- 根目录 `/Users/yelog/workspace/tui/lazydb`，目标分支 main，起点 `5f8c149a947364eac09eb761a8141a93adcba4a9`。
- plan 阶段重新执行 `git status --short; git rev-parse HEAD`，退出 0，状态为空且 HEAD 未变。分析仍有效，无需重新通读历史。
- checkpoint.json 仍不存在；不创建或修改 checkpoint.json/state.json。
- 本计划及验证记录仅写当前任务目录。本次正式 plan token 为 `056993ae-28ac-4785-9fc3-9b99d02e9f6a`；先写 change-scope.json，最后写同 token 的 plan 回执，不复用 analyze 回执。
- 工作流名称、任务分支与 worktree 由 Luna 在接手后按插件规则命名/创建。新 worktree 从指定基线开始；当前无未提交业务文件需要迁移。若接手时状态变化，先比较并明确依赖，不清空、不 stash 原工作区。
- 每个单元完成定向验收后继续下一个，不等待用户 resume；全量检查通过后由 Luna 审查、提交合并。提交遵循实际工作流授权，本文不替用户立即执行 Git 操作。

## 已确定的行为

1. unbound Insert 模式可自动/显式补全并接受 SELECT/FROM 等通用候选，不借用全局连接的元数据或专属 builtin。
2. RunActiveSql 保留发起时光标/选区；RunAllSql 保留全缓冲语义。
3. 未绑定执行打开现有 selector；正常关系型目标选定后自动续接。已有 session 不重连，离线目标连接成功再执行。
4. 普通打开 selector 仍只绑定；Esc、None、请求过期和连接失败不执行。没有关系型 profile 时允许显示既有 None 项并退出。
5. 不新增配置、快捷键、数据库适配器或持久化格式；既有执行确认、事务和只读规则仍适用。

## 验收来源与门禁分级

- **用户需求（必须满足）：** unbound 基础补全可用；执行时打开 target selector；选择后按需连接并自动执行。前述原 SQL/Console 归属、取消不执行与保留既有确认行为是实现正确性要求。
- **项目既有 Rust 门禁：** `.github/workflows/ci.yml` 的 Rust 1.94.0 fmt、全目标/全特性 clippy（warnings 为错误）和 test。最终命令见文末。其他平台/外部数据库矩阵仍由原 CI 负责，不把尚未执行的矩阵写成通过。
- **本计划选定的自动验证：** 各单元 action/command 回归、editor 范围回归及本地 SQLite Runtime 用例，用来证明上述功能。它们是实现测试方案，不宣称是用户新增的人工要求。测试内使用有界事件等待。
- **补充建议：** PTY/截图目视检查、`git diff --check` 是补充证据/代码卫生检查，非用户新增强制人工门禁。PTY 无法运行时最多一次有针对性的环境修复重试，记录限制，由 Luna 收尾决定是否补证，不无限延续 progress。

## 预计修改范围

change-scope.json 列出本计划全部预计仓库文件，均为修改现有文件，无新增/删除/重命名业务文件，无未提交依赖。任务目录中的计划/验证/回执是工作流产物，不随业务提交，不列为仓库业务变更。

`src/app.rs`、`src/editor/mod.rs`、`src/editor/tests.rs`、`src/model/pending_execution.rs`、`src/model/workspace.rs`、`src/help.rs`、`src/ui/mod.rs`、`tests/sql_completion.rs`、`tests/console_lazy_connection.rs`、`tests/mouse.rs`、`tests/ui_render.rs`。

`tests/keymap.rs`、`tests/connection_switch.rs`、`tests/workspace_tabs.rs` 的现有 TargetSelector 匹配已使用 `..`，仅读取与回归，不预计修改；`tests/app_flow.rs`、`tests/sql_execution.rs` 同样仅作参考/回归。若实施发现范围需要变化，由 Luna 先更新范围清单并记录理由，不预先扩大到整个 src/tests。

## 单元一：未绑定编辑的完整补全闭环

**Modify:** `src/app.rs`：completion_key、completion_request_is_current、set_completion_request。

**Test:** `tests/sql_completion.rs`，复用现有 App-level 输入/补全 fixture（如 offline_console_keeps_local_completion_without_catalog_objects_or_requests）。

### 步骤

1. 添加回归测试：新建 unbound，进入 Insert 输入 `SEL`，从返回命令取得 ScheduleCompletion key，发送 CompletionDue，断言存在 SELECT，接受后文本正确。另以 `SELECT 1 FR` 验证 FROM。不要只调用 CompletionExplicit，否则抓不到原调度 bug。
2. 定向运行新增用例，预期基线缺少 ScheduleCompletion/候选而失败；记录实际结果，不依赖真实 debounce 时间。
3. 去掉 completion_key 中 target 与 connection 同时为空的早退。保留 Insert、非空文本和 Console 检查。
4. 三处 completion 身份计算共用一致规则：target=None 时 connection=None；有 target 时保留现有兼容行为，避免扩大修改范围。继续使用现有 target、revision、cursor、catalog generation 判定，空索引/Generic 路径无需重写。
5. 添加显式补全、无 Connect/LoadCatalogPage、其他 profile 的缓存对象不泄漏、绑定/解绑使旧 key 失效的断言。候选应排除数据库专属函数，但允许 Generic 的通用函数/类型。
6. 执行该测试文件，确认原有有目标补全回归通过。

**命令：**

```sh
cargo +1.94.0 test --test sql_completion unbound
cargo +1.94.0 test --test sql_completion
```

**完成标准：** 自动调度→候选→接受的实际 action 链通过；无元数据请求和对象泄漏。建议提交单元名 `fix(completion): enable generic suggestions in unbound consoles`，由 Luna 按最终提交策略处理。

## 单元二：选择目标到 SQL 执行的闭环

**Modify:**
- `src/editor/mod.rs`：提取 current_scope 的光标/选区捕获逻辑。
- `src/model/pending_execution.rs`：新增仅内存使用的选择阶段意图结构，现有 PendingExecution 继续表示已选目标、等待连接的具体 SQL。
- `src/model/workspace.rs`：TargetSelector 增加可选执行意图。
- `src/app.rs`：打开/确认 selector、run_active_sql、共享执行 helper、方言按目标查询。
- `src/help.rs`：现有 TargetSelector fixture 是完整构造，补充意图 None。
- `src/ui/mod.rs`：现有 TargetSelector 为完整解构，增加 `..`；布局保持现有实现。
- `tests/mouse.rs`：三处现有 TargetSelector 构造补充 execution=None。
- `tests/ui_render.rs`：现有 TargetSelector 构造补充 execution=None。

**Test:** `tests/console_lazy_connection.rs`、`src/editor/tests.rs`；编译定位其他完整构造处并做必要适配。

### 步骤 A：冻结请求并延后方言解析

1. 从 editor.current_scope 提取 `scope_input(id)` 一类方法，返回 `(cursor_byte, Option<ScopeSelection>)`，保留现有 CharWise/LineWise/BlockWise 的范围算法。current_scope 改为调用它再调用 sql::resolve_scope；避免复制约 60 行选区提取代码。
2. 在 pending_execution 模块定义 `TargetSelectionExecution`，字段明确为：console_id、document_revision、full_buffer、cursor、selection，以及 transaction_generation/mode/state。selection 使用现有 ScopeSelection，无需创造新 SQL 范围模型。源 SQL 暂不复制；确认时按 console_id 读取且验证 revision，连接开始后由 PendingExecution 冻结具体 SQL。
3. 为冻结光标/反向选区/行选区/块选区补充 editor 回归，确保原 current_scope 行为不变。捕获后光标移动不改变已捕获的执行输入。
4. 未绑定执行：空白文档先沿用 No SQL scope 警告；有内容则捕获请求，打开 selector 并存入意图。不要用 Generic current_statement 是否成功作为目标方言有效 SQL 的唯一门槛；最终 scope 用选定目标方言解析。显式空白选区最终仍不得执行。

**确认目标后的 scope 算法：**

```text
verify original console exists and revision/transaction snapshot still match
verify original console is still unbound and chosen target is valid relational target
dialect = SqlDialect::for_database_kind(chosen_profile.kind)
text = editor.text(intent.console_id)
if intent.full_buffer:
    scope = FullBuffer + Contiguous(0..text.len()) + original text
else:
    scope = sql::resolve_scope(text, intent.cursor, intent.selection, dialect)
if no meaningful scope: notify and consume intent without executing
bind original console to target; verify target actually equals requested target
submit scope for that console and target through shared execution entry
```

捕获范围后不需要依赖确认时的活动光标/选区；revision 改变则取消。FullBuffer 和选区的确切字节内容应保持原逻辑，不能将选区升级为整个缓冲区。

### 步骤 B：意图的归属与一次性消费

1. TargetSelector 新增 `execution: Option<TargetSelectionExecution>`。普通 OpenConsoleTargetSelector 的 execution=None；执行触发构造 Some。所有创建点显式初始化。
2. ConfirmTargetSelector 从 `overlay.take()` 同时取得 candidate 和意图。None candidate 或无效索引直接消费退出。普通绑定路径保留；有意图路径校验归属并续接。
3. Esc/CancelTargetSelector 直接丢弃 overlay，执行意图自然消失。不能另用全局布尔标记，也不把 selector 阶段意图提前插入 pending_executions。
4. 选中后绑定可能因 running/transaction 状态拒绝；只有校验通过且实际 target 已绑定成功才执行，不能仅看 bind_console_target 返回空 Vec 判断成败。

### 步骤 C：共用连接与执行入口

1. 把 run_active_sql 中“已经取得目标和 scope 之后”的流程提取为按 `console_id, target, scope, dialect` 接收参数的私有 helper。普通有目标执行与 selector 续接共用。
2. helper 中从原 Console 读取 revision/事务状态/query status，而非盲用 active_console。已 connected session 调用 run_console_sql_on_session；未连接插入现有 PendingExecution 再请求连接。
3. 保留重复请求抑制、完整 target 的 session 选择及 generation 校验。拒绝启动连接的分支不能留下永远等待的 pending；已有同目标 connecting session 应可等待，而非重复 Connect。
4. 保留 ConnectionSucceeded 的 pending 消费和 revision/事务校验；选定 profile 的真实 dialect 写入 pending，不使用 unbound Generic。
5. 检查 dispatch_transaction_sql、dispatch_manual_sql 中按 active_console 读取的部分：当以明确 console_id 续跑时，相关读取/修改也必须按该 ID 定位。只修该执行路径，不重构无关连接管理。
6. 普通 selector 确认后仍只持久化绑定；本功能不能让所有绑定动作自动执行。

### 步骤 D：端到端验收

在 tests/console_lazy_connection.rs 保留原“手动绑定再执行”用例，增加：

- `unbound_run_selects_connects_and_executes_once`：RunActiveSql 打开 selector，无旧未绑定警告；选择 SQLite target 发一次 Connect；ConnectionSucceeded 后产生匹配 SQL、console_id 与 target 的一次查询。
- `unbound_run_reuses_connected_session`：已有目标 session 时不发 Connect，直接执行。
- `unbound_run_all_preserves_full_buffer`：多语句请求保留全文，经原确认策略后下发，不能误当当前语句。
- `ordinary_target_selection_does_not_execute`：显式打开 selector 后仅绑定，无 Connect/SQL 命令。

```sh
cargo +1.94.0 test --lib current_scope
cargo +1.94.0 test --test console_lazy_connection
```

复核 selector 字段适配：`cargo +1.94.0 test --test mouse --test ui_render target_selector`，预期编译通过且命中用例通过；若过滤器未命中，用现有测试真实名称定向运行，不将 0 tests 当作行为通过。UI/输入/help 剩余回归由最终 all-targets 验证覆盖。

**完成标准：** 用户只触发一次执行并确认一个 target，即可到达原执行/确认链路；未连接时自动连接成功后续跑。建议提交单元名 `feat(console): resume unbound SQL after target selection`。

## 单元三：异常生命周期与实际结果验证

**Modify/Test:** `tests/console_lazy_connection.rs`、`tests/sql_completion.rs`；运行时集成优先放 `tests/console_lazy_connection.rs`，参考 `tests/app_flow.rs` 的 Runtime/事件驱动 fixture；仅修前两单元相关代码。

### 有意义的边界矩阵

| 场景 | 预期 |
| --- | --- |
| Cancel/Esc 或 None 后普通绑定 | 没有 SQL、没有残留执行意图 |
| 无 profile / 仅 Redis | selector 可退出，不发 SQL/错误数据库连接 |
| selector 打开后文档修改、关闭、被另一路重绑 | 原请求失效，不执行新文本或新目标 |
| selector 打开后光标/活动 tab 改变 | 使用原捕获范围和原 Console；不会执行活动 tab 的 SQL |
| 连接期间文档修改/关闭/target 变化 | ConnectionSucceeded 不执行已过期请求 |
| 重复触发执行、重复/延迟连接事件 | 不重复 Connect/查询，不串 generation/target |
| 连接失败后用户重试 | 旧 pending 已清理，能够产生新有效连接请求 |
| 目标方言与 Generic 的分句不同 | 按目标方言解析原光标位置；不使用 Generic 预切片 |
| DML/DDL 或原策略要求确认 | 出现现有 ExecutionConfirm；取消不发 SQL |
| 事务语句与混合事务控制 | 仍使用既有分类/拒绝/事务状态机，归属原 Console |

1. 添加上述关键 action-level 用例，优先关注请求归属、恰好一次、失败重试，不为每个字段写镜像测试。
2. 增加一个真实本地 SQLite Runtime 用例：unbound `SELECT 1 AS value` → selector → Connect → 实际连接事件 → 查询完成，断言原 Console 结果值为 1。复用现有 Runtime 调度/有界事件等待方式；不要无限等待或手工连接外部数据库。
3. 运行单元定向测试，必要时回归 `tests/sql_execution.rs`。定位异常后修复再重跑受影响范围，不反复跑整个项目。

```sh
cargo +1.94.0 test --test console_lazy_connection
cargo +1.94.0 test --test sql_execution
```

**完成标准：** 取消/失败/过期请求无副作用，可重试；本地实际 SQL 结果可见。运行时故障与业务断言失败分别记录，不能把模拟 Action 用例当作真实数据库结果。

## 最终验证与 Luna 收尾

功能齐备后一次执行项目 Rust CI 要求：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

补充代码卫生检查：`git diff --check`，预期退出 0。

预期全部退出 0；实际结果写 validation.md，记录命令、代码版本/diff、环境与相关文件。新增修复后只重跑受影响检查，最终需要的全量证据须对应最终代码。外部数据库和平台 CI 不能冒充本地已验证。

补充 PTY 场景为进入 unbound 输入 SELECT/FROM、执行选择 SQLite、观察结果，以及取消 selector。用户未强制 PTY，环境受限最多一次针对性修复重试，记录限制由 Luna 收尾审查决定是否补证，不保持 progress 无限重试。

Luna 审查重点：自动而非只有显式补全；无全局元数据泄漏；普通绑定无隐式执行；原选区/Console/目标方言准确；pending 恰好一次及失败可重试；没有新增持久化状态；改动只限任务范围。后续阶段回执使用届时用户/插件指定的新 token 和文件，不覆盖 analyze 回执。
