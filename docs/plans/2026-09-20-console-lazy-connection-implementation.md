# Console 按需连接与无连接编辑 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
>
> 执行适配：若环境没有上述技能，按本文依赖顺序逐项实施。文中新增接口与测试名为建议名称，实施时遵循现有命名。每项先建立行为回归，再完成实现与定向验证。提交粒度见各任务末尾，仅在用户要求提交时创建 Git commit。

**Goal:** 复用现有 Console，使用户无需配置或建立数据库连接即可编辑、格式化和保存 SQL；有执行目标时，仅在执行 SQL 或显式连接时建立连接。

**Architecture:** 保留 Action → App::update → Command → Runtime 架构及 Console 的可选 ExecutionTarget，将文档激活、目标绑定和网络连接分开。补全与诊断使用当前 Console 的精确目标/session 上下文，离线时提供本地语言能力；冷连接和热连接执行汇合到同一执行入口。未绑定 Console 使用现有工作区顶层文档字段持久化，并与按 profile 保存的工作区共同恢复。

**Tech Stack:** Rust 2024 / Rust 1.94、Ratatui 0.30.2、Crossterm 0.29、Tokio、现有 SQL formatter/parser/completion engine、WorkspaceStore、SessionRegistry；测试使用现有 reducer fixtures、tempfile 和本地 SQLite。

---

## 0. 基线与实施方式

- 需求来源：<https://github.com/yelog/lazydb/issues/9>。
- 分析基线：2026-09-20，提交 `b7c3c0d`。行号只是定位参考，实施时按符号定位并检查当前差异。
- 核心用户规则：**Console 随时可以编辑，执行时才需要连接。**
- 本文覆盖规划；文中的测试和检查命令是实施阶段要求，不表示已经执行或通过。
- 旧计划 `docs/plans/2026-09-15-console-auto-connect.md` 中关于创建、恢复、激活和焦点进入 Console 即自动连接的规则，由本文替代。
- 衔接 `docs/plans/2026-09-15-sql-editor-target-completion-isolation.md`：继续按完整目标隔离补全；离线状态下不触发冷 catalog 加载，不展示缓存中的数据库对象。
- 每个任务按“新增/调整行为测试 → 运行并确认失败原因 → 实现 → 定向测试”的顺序推进。避免用固定 sleep 验证异步流程；通过 Action、请求身份和 generation 控制事件顺序。

## 1. 冻结产品行为

### 1.1 状态矩阵

| Console 状态 | 编辑 / 格式化 / 保存 | SQL 方言 | 数据库对象补全 | 执行 |
|---|---|---|---|---|
| 未选择连接 | 可用 | Generic | 不提供 | 引导选择或配置连接 |
| 已选择目标，未连接 | 可用 | profile 对应方言 | 不提供，包括旧缓存对象 | 自动连接后继续本次执行 |
| 连接中 | 可用 | profile 对应方言 | 不提供 | 合并本 Console 的重复执行请求 |
| 已连接，元数据未就绪 | 可用 | profile 对应方言 | 按已有元数据逐步提供 | 可执行，不等待整个 catalog 加载 |
| 已连接，元数据就绪 | 可用 | profile 对应方言 | 正常提供 | 直接执行 |
| 连接失败 / 已断开 | 可用 | profile 对应方言 | 不提供 | 再次执行时发起新的连接尝试 |

关键词、内置表达式和已有引擎能从当前文本推导的局部提示保留；数据库表、列、schema、自定义函数等元数据候选只在目标有可用 session 时提供。离线时仍保留语法诊断，依赖数据库对象存在性的诊断暂停。

### 1.2 连接触发规则

1. 新建、打开、恢复、切换 Console、鼠标激活、焦点进入 SQL Editor，以及绑定目标，均不主动建立连接。
2. 已建立的目标 session 可以复用。激活 Console 可以同步其本地上下文，并在已有可用 session 上按需加载元数据。
3. 执行 SQL，或者用户显式执行连接操作，才发起 Console 所需的数据库连接。
4. 连接失败、断开或取消之后，仅切换焦点不会重试。
5. 一个 profile 下其他 database/schema 的 session 在线，不能直接被视为当前完整 ExecutionTarget 在线。
6. Explorer 中用户明确请求连接、展开需要联网的节点，以及其他数据视图的既有显式加载行为，继续按各自操作语义处理。

### 1.3 Console 创建和目标选择

1. 零 profile 的首次启动显示现有初始 Console，并可直接输入 SQL。
2. 新建 Console 继续继承明确的来源目标：当前 Console、Relation 或 Explorer 中选定的有效 SQL 目标；无有效来源时创建 `execution_target = None` 的 Console。
3. 新建 Console 不再以“存在 active_workspace_profile”为必要条件。
4. 现有目标选择器增加 `No connection` 选项，表示清除绑定。用户始终操作同一种 Console；界面使用 `Console`、`No connection`、`Disconnected` 等现有风格的文案。
5. 已有 profile 时，也能通过目标选择器将 Console 设为未绑定。Omni 的新建流程支持选择 `No connection`，不强迫先连接。
6. 无目标时按执行键：有可用 SQL profile 则打开目标选择器；没有可用 SQL profile 则提示创建连接，并提供现有连接管理入口。取消后仍停留在原 Console。
7. **无目标执行不创建待执行请求。** 用户完成选择/配置后，明确提示再次执行；避免首次绑定引入第二种“选择即执行”的隐式语义。有既定目标但未连接的执行，仍是一键自动连接并继续执行。
8. 通用 SQL 足以满足本次无目标编辑；绑定已有 profile 后立即切换相应方言并刷新本地分析。

### 1.4 文档和等待执行的生命周期

- 文档 UUID、文本、名称和打开/关闭状态独立于连接的存在与在线状态。
- 初始 Console 一旦作为正常可编辑文档显示，激活第一个 profile 时不能再被占位清理逻辑删除。
- 新建连接、连接成功、切换 profile、重启恢复均不自动给未绑定 Console 补上目标。
- 目标显式绑定或清除时保留 UUID、文本和编辑位置；继续遵守已有运行中查询和活动事务的目标变更约束。
- 执行等待期间保存不可变请求。文本 revision、目标或事务状态变化，关闭/删除 Console、取消、断开或退出，均使相应请求失效；迟到的连接事件不能执行它。
- 等待期间仅切换焦点/标签不使请求失效；SQL 仍属于发起执行的 Console。需要确认时必须明确展示该 Console 和目标，不能覆盖另一个确认弹窗或其他前台交互。
- 重复执行同一未变更请求不重复排队；共享目标的多个 Console 只建立一次连接，各自的执行意图独立保存。
- 有活动或状态不明的手动事务时，先沿用现有事务恢复/失效规则处理，不把新 session 当作旧事务的延续。

## 2. 已确认的代码入口

| 文件 / 符号 | 现状与本次调整 |
|---|---|
| `src/app.rs::with_profiles` | 零 profile 已创建 target=None 的初始 Console；将其作为正式文档处理 |
| `src/app.rs::has_active_workspace` | 包含 profile 条件；Console 文档操作需要摆脱该条件 |
| `src/app.rs::create_and_activate_sql_editor_named` / `activate_sql_editor` | 调用 prepare 并可能发起连接；改为离线激活 |
| `src/app.rs::prepare_active_console_target` / `prepare_active_tab_after_focus_change` | 激活和焦点变化的自动连接入口；改为本地准备和复用已有 session |
| `src/app.rs::bind_console_target` / `ConfirmTargetSelector` | 绑定会调用连接准备，且存在另一条直接请求连接的 selector 分支；统一为纯绑定 |
| `src/app.rs::remove_placeholder_console` / `activate_profile_workspace` | 初始 Console 可被直接删除；移除其作为可丢弃 placeholder 的语义 |
| `src/ui/mod.rs::WorkspaceEmptyState::for_app` | 当前只对有 target 的 Console 跳过空状态；改成有 Console 即显示编辑器 |
| `src/app.rs::workspace_snapshot` / `normalize_workspace_snapshot` / `restore_workspace` | 未绑定归属受 active profile 影响；混合恢复提前返回；恢复会自动补绑目标 |
| `src/persistence/workspace.rs` | v5 已有顶层 consoles/tabs 和 profile workspaces，可表达混合文档 |
| `src/app.rs::completion_key` | 需要非空连接身份和目标，且回退全局 active_identity；改为目标局部、可离线的请求上下文 |
| `src/app.rs::complete_now` | 离线仍可能读取 profile 缓存，并按依赖请求 catalog；增加统一在线门槛 |
| `src/app.rs::diagnostic_analysis_command` | 总是尝试构建目标 catalog snapshot；离线仅进行不依赖元数据的分析 |
| `src/app.rs::run_active_sql` / `run_console_sql_on_session` | 已有执行时建连、session 复用和正常执行确认路径 |
| `src/app.rs::ConnectionSucceeded` 待执行处理 | 当前构造 draft 后直接 dispatch，未经过正常路径中的分类/确认逻辑；需要汇合 |
| `src/app.rs::dispatch_transaction_sql` | 参数含 tab_id，但仍读取 active_console；后台恢复时需要全程使用发起 Console |
| `src/model/pending_execution.rs::PendingExecution` | 已包含 UUID、目标、revision、scope、事务信息，可扩展连接尝试关联 |

## Task 1：建立目标局部的 Console 上下文与离线激活

**Files:**
- Modify: `src/app.rs`、`src/action.rs`。
- Inspect / modify as needed: `src/model/session.rs`、`src/runtime.rs`、`src/commands.rs`。
- Test: `tests/consoles_lifecycle.rs`、`tests/console_manager_input.rs`、`tests/connection_switch.rs`、`tests/startup_profiles.rs`。

**Steps:**
1. 将 `activating_an_offline_saved_console_starts_its_connection` 改为离线打开的行为测试；保留目标、UUID、文本、Editor 焦点断言，断言没有 `Command::Connect`。
2. 将两个 Console 激活触发 single-flight 的旧用例改成“两次激活均无 Connect”；执行时 single-flight 在 Task 6 覆盖。
3. 添加新建、恢复、Tab 切换、鼠标激活对应公共 Action、焦点往返和连接失败后再次聚焦的用例；断言无连接请求且文档可用。
4. 运行 `cargo +1.94.0 test --test consoles_lifecycle --test console_manager_input`，记录旧自动连接断言与新增用例的失败原因。
5. 提供一个目标局部的只读 session 解析入口：从 Console 的完整目标查询 `SessionRegistry`，校验状态及 identity；旧兼容 connection 投影只有在目标完全匹配且确实 Connected 时才能参与。后续补全、诊断、状态显示、执行共用它。
6. 收敛 `prepare_active_console_target`：仅解析/同步已有 session 和目标上下文；没有有效 session 直接返回，不排 deferred activation，不发 Connect。使用已连接 session 时继续保留必要的 catalog 准备。
7. 检查所有 prepare 调用点和 `Action::PrepareActiveConsole` 的语义；显式连接/重试入口直接走连接请求逻辑，避免误把显式操作变成 no-op。
8. 清理纯焦点自动连接所需的 deferred activation 状态及成功回调；执行等待继续由 PendingExecution 管理。
9. 运行上述四个测试 target，更新确实表达旧策略的测试；对于其他行为失败先排查，不能机械删除 Connect 断言。

**Acceptance:** 打开任意未连接 Console 不产生网络连接；已有 session 可正确复用；显式连接操作仍有效；不同目标的状态不串用。

**Suggested commit:** `refactor(console): separate activation from connection setup`

## Task 2：统一未绑定 Console 的创建、绑定与目标选择

**Files:**
- Modify: `src/app.rs`、`src/model/workspace.rs`、`src/model/omni.rs`、`src/ui/mod.rs`、`src/ui/omni.rs`。
- Modify as needed: `src/action.rs`、`src/commands.rs`、`src/help.rs`。
- Test: `tests/console_manager_input.rs`、`tests/execution_target.rs`、`tests/omni_flows.rs`、`tests/omni_navigation.rs`。

**Steps:**
1. 添加零 profile、已有 profile 但无 active workspace、仅有 Redis profile 三种新建 Console 用例；均应创建可编辑的 SQL Console，无法得到有效 SQL 来源时 target=None。
2. 添加“绑定离线 SQL profile 不连接”“清除绑定保留 UUID/SQL/编辑位置”“运行中/活动事务维持现有约束”用例。
3. 将目标 selector 的选择项建模为明确的 `NoConnection` 与 `Target(ExecutionTarget)`，而不是使用伪 profile 或无效 UUID 表示 UI 选项。名称按实际周边代码命名。
4. 将 Console 目标更新收敛到接收可选目标的单一入口；同步 ConsoleRecord、缓存投影、补全/诊断失效、方言和持久化。普通选择与清除绑定不发起连接。
5. 统一 `ConfirmTargetSelector` 的 Console 分支，去除残留的选择即连接路径；元数据发现所需的显式联网行为与文档绑定分开。
6. 保留新建时明确来源目标的继承，移除没有 active workspace 就无法创建/删除文档的限制。让 Omni 的 PickConnection 支持 `No connection`；没有 profile 时直接命名。
7. 无目标执行使用现有目标选择/连接管理入口，并明确提示完成后再次执行；取消时不留下 PendingExecution。
8. 运行 `cargo +1.94.0 test --test console_manager_input --test execution_target --test omni_flows --test omni_navigation`。

**Acceptance:** 任意启动状态都可以通过现有 Console 入口写 SQL；绑定只改变目标；用户不用创建第二种文档。

**Suggested commit:** `feat(console): support optional targets across console entry points`

## Task 3：修正文档归属、首次连接和持久化恢复

**Files:**
- Modify: `src/app.rs`、`src/persistence/workspace.rs`。
- Inspect / modify as needed: `src/model/console_document.rs`、`src/model/workspace_save.rs`。
- Test: `tests/global_workspace.rs`、`tests/workspace_persistence.rs`、`tests/workspace_tabs.rs`、`tests/consoles_lifecycle.rs`。

**Steps:**
1. 用 TempDir + WorkspaceStore 建立真实 save/load 回归：零 profile 写入 SQL、关闭重开、重启恢复，UUID/SQL/target=None/open 状态保持。
2. 添加混合快照回归：一个未绑定 Console、两个 profile 下的 Console，同时保存与恢复；无论 active_profile 是哪个，所有文档都应出现且每个 UUID 只有一份 SQL。
3. 添加初始 Console 输入 SQL 后新建/连接第一个 profile 的回归；验证原文档仍在且未自动补绑。
4. 添加重启时已有 profiles 但文档 target=None 的回归，验证不被 `selected.map(ExecutionTarget::from_profile)` 自动绑定。
5. 移除初始可编辑 Console 的 placeholder 清理语义，包括对应字段/删除调用；默认初始文档与普通 Console 使用同一保存和关闭规则。
6. 固定归属规则：有目标的文档归其 target.profile_id；无目标的文档归现有顶层 `consoles` / `tabs`。不以 active_workspace_profile 推断未绑定文档的归属。
7. snapshot 同时收集顶层未绑定文档与 profile 文档。normalize 使用 live record 的真实目标去重，不把未绑定文档归入当前 profile；SQL 记录与文档 ID 一一对应。
8. restore 合并读取顶层文档、各 profile 工作区、旧 `Uuid::nil()` 迁移记录，再按 UUID 去重；移除会跳过顶层文档的早返回和无目标自动补绑。
9. 显式绑定/清除目标只迁移文档归属，不改变 SQL 文件 UUID，不生成第二份文档。缓存 workspace 投影同步移除旧归属。
10. v5 字段足以表示这次状态，优先复用。顶层 active_tab 只引用顶层打开的 tab，profile 内活动 tab 仍写各 profile 字段；恢复时优先有效顶层活动项，再选 active profile 的活动项。保持既有顺序能力，不引入额外排序状态。
11. 更新空初始 Console 的“不持久化”特判，使恢复结果与用户保存的正常文档一致；关闭最后一个 tab 后保持无打开 tab，文档仍可从 F6 重开。
12. 验证 v1/v2 legacy 和 v3/v4/v5 文件仍可读取；失效目标不自动换成其他数据库。显式 None 必须原样保存。
13. 运行 `cargo +1.94.0 test --test global_workspace --test workspace_persistence --test workspace_tabs --test consoles_lifecycle`。

**Acceptance:** 首次连接、目标改绑、profile 切换及重启均不丢 SQL、不重复文档、不偷偷给无目标 Console 选择数据库。

**Suggested commit:** `fix(workspace): persist unbound consoles alongside profile workspaces`

## Task 4：离线 SQL 编辑器展示与状态一致性

**Files:**
- Modify: `src/ui/mod.rs`、`src/help.rs`、`src/commands.rs`。
- Inspect / modify as needed: `src/input/mouse.rs`、`src/input/keymap.rs`。
- Test: `tests/ui_render.rs`、`tests/console_manager_input.rs`。

**Steps:**
1. 调整 `disconnected_workspace_without_profiles_renders_first_run_empty_state`：首次启动应看到 SQL Editor；输入文本后渲染可见，并有 editor_viewport 和正确焦点热区。
2. `WorkspaceEmptyState::for_app` 对任何存在的 SQL Console 返回非空工作区，判断不依赖 execution_target。无 tab 时仍显示空状态，并提示现有 F6 新建/打开 Console 入口。
3. Console header、F6 列表、目标选择器共享 Task 1 的目标状态语义：未绑定、Disconnected、Connecting、Connected、Failed。按目标状态显示，不借用 Explorer 当前连接状态。
4. 未绑定目标显示 `No connection`；有 profile 但未连接时继续显示具体目标和方言。沿用现有图标、颜色与窄屏截断规则。
5. 让编辑、格式化、新建和目标选择的 command availability 在离线状态可用；无目标执行保持可调用，以便进入选择引导。
6. 添加 120×36、80×24、极小终端渲染回归；未绑定和失败状态下文本可见、编辑热区有效，取消弹窗后焦点回到原 Console。
7. 运行 `cargo +1.94.0 test --test ui_render --test console_manager_input`。

**Acceptance:** 无连接时用户确实能看到并操作 Console；空状态仅表达没有可展示的 tab。

**Suggested commit:** `fix(ui): render consoles independently of connection state`

## Task 5：离线补全和诊断按能力降级

**Files:**
- Modify: `src/sql/completion.rs`、`src/model/tab.rs`、`src/app.rs`。
- Modify as needed: `src/action.rs`、`src/runtime.rs`、`src/sql/diagnostics.rs`、`src/sql/semantic.rs`。
- Test: `tests/sql_completion.rs`、`tests/sql_diagnostics.rs`、`tests/connection_switch.rs`。

**Steps:**
1. 增加未绑定和离线目标下 `sel` 等关键词自动/显式补全测试；不要用必然要求表名的上下文断言关键词。
2. 注入同 profile 的旧表/列索引，验证离线目标不会展示这些对象；再注入另一个在线 profile，验证不会借用其 session 或候选。
3. 添加离线输入和显式补全不生成 Connect 或 catalog 请求的断言。
4. 将 CompletionScheduleKey 的 connection/target 改为 Option，并纳入 dialect；Console、revision、cursor、完整目标、有效 session identity 共同构成请求上下文。与 CompletionRequest 共用上下文构造，消除全局 active_identity 回退。
5. complete_now 在 Task 1 上下文无有效 session 时传入空 CompletionIndex，且不调度 relation children/catalog 加载；本地引擎继续提供原有语言候选。
6. 有效 session 存在时使用所属 profile 索引及现有 database/schema 范围规则，保留异步 catalog 准备和按需列加载；连接完成不意味着所有元数据已经就绪。
7. diagnostic_analysis_command 在离线时使用明确不可用/未知覆盖状态的 catalog，保留纯语法与局部文本分析。确认空索引不能被解释成“完整 catalog 中不存在该对象”。
8. 断开、失败、改绑时清理数据库候选和语义诊断；重新连接或改绑后重建请求上下文。迟到 CompletionDue、诊断结果和 catalog 刷新在消费前验证上下文，离线弹窗不能被旧对象重新填充。
9. 添加离线→在线→断开、A→B、A→B→A、后台 profile 更新的回归；已连接时元数据候选正常，离线时关键词仍可用。
10. 运行 `cargo +1.94.0 test --test sql_completion --test sql_diagnostics --test connection_switch`；编译共享 SQL 类型的消费方，确认 LSP 原有离线语言能力保持可用。

**Acceptance:** 离线不是禁用编辑器智能能力，而是只暂停数据库元数据能力；无跨目标提示、无输入触发重连。

**Suggested commit:** `feat(sql): retain local assistance while console targets are offline`

## Task 6：统一冷连接与热连接执行路径

**Files:**
- Modify: `src/app.rs`、`src/model/pending_execution.rs`。
- Inspect / modify as needed: `src/runtime/connections.rs`、`src/model/session.rs`、`src/model/confirmation.rs`、`src/action.rs`。
- Test: `tests/sql_execution.rs`、`tests/connection_switch.rs`、`tests/consoles_lifecycle.rs`。

**Steps:**
1. 添加已有目标离线执行测试：只发一个对应目标的 Connect；收到正确 ConnectionSucceeded 后只执行一次原 SQL，目标/session/Console UUID 均一致。
2. 添加对照用例：热连接与冷连接执行普通 SELECT、需要确认的语句、full-buffer、多语句、BEGIN/COMMIT、混合事务控制 SQL，其分类和确认结果一致。
3. 执行入口先解析并验证 scope。空 SQL 不建连；无目标走 Task 2 引导；正在运行的 Console 不排第二次执行；有目标时读取精确 session。
4. 将已连接执行和连接成功后的 PendingExecution 恢复汇合到同一个“按 Console ID 执行已解析 scope”的入口。恢复使用捕获的 scope/SQL，不重新读当前光标或当前活动 Console。
5. 从 ConnectionSucceeded 的待执行分支移除直接 dispatch_draft；经过统一入口中的事务 SQL 分类、已有确认策略和 draft 校验后再 dispatch。
6. 修正 dispatch_transaction_sql 及下游任何把 tab_id 与 active_console 混用的读取，确保用户已切换标签时仍处理原 Console。
7. PendingExecution 关联目标连接尝试的身份/generation 或等价请求凭据。相同目标重复执行合并；失败/取消后清理匹配请求，重新执行产生新请求，迟到旧成功事件不恢复旧 SQL。
8. 连接成功前校验文档 revision、目标、事务 mode/state/generation 和 Console 存续；不一致时取消并给出简短原因。文本编辑不会自动执行新文本。
9. 复用现有执行确认/deferred 机制串行展示多个 Console 的确认请求。若现有机制不能表达已连接待确认的执行，就增加最小的队列状态；不能让循环覆盖 Overlay，也不能自动确认。
10. 测试共享目标两个 Console 的一次 Connect、各自一次执行；测试不同目标并发、前台切走、连接重试、主动断开、关闭文档、改绑、修改 SQL、取消和退出。
11. 验证事务失效路径：连接丢失后已有手动事务不能通过重连被当作正常延续；沿用现有状态机与用户提示。
12. 运行 `cargo +1.94.0 test --test sql_execution --test connection_switch --test consoles_lifecycle`。

**Acceptance:** 用户执行一次即可在连接成功后继续；冷/热路径规则一致；没有重复、过期、错目标或错 Console 执行。

**Suggested commit:** `fix(sql): resume lazy connections through the unified execution path`

## Task 7：全流程回归与本地运行验证

**Files:**
- Create: `tests/console_lazy_connection.rs`。
- Reuse: `tests/app_flow.rs`、`tests/startup_profiles.rs`、现有 Runtime/SQLite fixtures。

**Steps:**
1. 建立一条用户旅程测试：零 profile → 初始 Console 输入 SQL → 格式化 → 保存 → 重启 → 文本可见且 target=None。
2. 接续配置本地 SQLite → 绑定原 Console → 验证不建连、不换 UUID、不丢文本 → 执行 → 实际获得 SELECT 结果。使用 TempDir SQLite 文件和现有 Runtime fixture，不连接用户配置。
3. 接续主动断开 → 编辑/聚焦不重连 → 本地补全可用、对象补全不可用 → 再次执行重新连接。
4. 使用可控连接失败事件覆盖失败后编辑、重试及迟到成功；不要依赖随机不可达公网地址或固定 sleep。
5. 运行 `cargo +1.94.0 test --test console_lazy_connection --test startup_profiles --test app_flow`。
6. 在隔离的测试配置/临时项目中进行 TUI 人工验收：F6 新建/重开、F2 新建、鼠标切换、目标选择/清除、格式化、断开后焦点往返、SQL 执行。确认实际所用格式化/执行按键与项目当前配置一致。
7. 检查已存在 SQL Console 与 Relation/Redis/Dashboard 共存时的焦点行为，确认显式数据库访问仍能正常发起。

**Acceptance:** issue #9 的零配置路径与用户提出的三项策略在同一产品流程中全部成立。

**Suggested commit:** `test(console): cover offline editing and lazy execution end to end`

## Task 8：用户文档与最终质量检查

**Files:**
- Modify: `README.md`、`docs/keybindings.md`、`docs/omni-bar.md`、`docs/architecture.md`。
- Modify: `docs/plans/2026-09-15-console-auto-connect.md`，在头部标明连接触发策略被本文替代。

**Steps:**
1. README 的 Quickstart 增加“无需配置连接即可使用 Console 编写和格式化 SQL”，说明执行时才选择/建立连接。
2. 更新 keybindings 中“选择 Console target 会启动连接”的旧描述，写明绑定不连接；保留并注明现有显式连接入口。
3. Omni 文档说明 `No connection` 与现有 Console 新建路径；架构文档说明文档/目标/session 三者职责和 PendingExecution 的恢复校验。
4. 用状态矩阵检查文案和 help/command descriptions，统一离线、未绑定、连接中、失败的含义。
5. 执行与当前 CI 对齐的检查：

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
git diff --check
```

6. 预期：格式、Clippy、测试和 diff 检查均通过。需要外部数据库的现有测试如按环境条件跳过，记录实际覆盖情况；本功能必须有无需远端数据库的 SQLite 全流程验证。
7. 最终检查差异：旧自动连接回归是否全部按新契约更新，是否仍有 UI 入口绕过统一绑定/执行逻辑，是否有遗留恢复自动补绑或 placeholder 删除路径。

**Suggested commit:** `docs(console): explain offline editing and execution-time connections`

## 3. 实施顺序与里程碑

```text
Task 1 目标局部上下文、离线激活
  → Task 2 创建与绑定
  → Task 3 持久化和恢复
  → Task 4 UI 与入口可用性
  → Task 5 补全和诊断
  → Task 6 执行时连接
  → Task 7 全流程验证
  → Task 8 文档与最终检查
```

- **里程碑 A（Task 1–4）：** 零配置即可进入 Console，保存并重启后内容/目标保持，打开和聚焦不建连。
- **里程碑 B（Task 5–6）：** 离线本地语言能力正常，执行时自动连接，冷/热执行行为一致。
- **里程碑 C（Task 7–8）：** 完整用户旅程通过，文档与新行为一致，CI 同级检查通过。

## 4. 最终验收清单

- [ ] 零 profile 启动能立即看到并编辑 Console，格式化不需要连接。
- [ ] 未绑定 Console 的内容、UUID、名称和打开状态可保存/恢复。
- [ ] 创建第一个连接、连接成功和切换 profile 不删除或自动绑定原 Console。
- [ ] 已配置连接也可以持有未绑定 Console，F6/F2 和目标选择器行为一致。
- [ ] 新建、打开、恢复、切换、聚焦、绑定 Console 均不主动连接。
- [ ] 离线有方言高亮、格式化和本地补全，没有数据库对象补全/存在性误报。
- [ ] 当前 Console 不借用其他目标的连接状态、元数据或异步结果。
- [ ] 已绑定离线目标按一次执行即可连接并继续；重复执行不重复提交。
- [ ] 自动连接后的执行经过同一确认和事务流程，绑定到最初发起操作的 Console。
- [ ] 连接失败可重试；取消、改文、改绑、关闭、断开和退出使等待请求正确失效。
- [ ] 多 Console 共享目标的连接去重和多个确认请求均正确处理。
- [ ] 本地 SQLite 全流程、定向回归及最终检查通过。
