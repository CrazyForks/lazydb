# Relation Preview Loading Lifecycle Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. 若执行环境未提供该技能，按本文任务顺序、验证命令和检查点执行；不要假定技能存在。

**Goal:** 恢复或首次激活 Relation 标签页时，自动准备正确的目标会话、验证必要的对象身份并加载 DATA/DDL；刷新能重试完整流程，加载不依赖访问其他 Console 或无关目录加载。

**Architecture:** 使用统一标签页激活入口和按 Relation 标签页持有的瞬态加载意图，将会话准备、对象解析、查询执行组成事件驱动流程。会话按精确 ExecutionTarget 复用，异步完成按请求所有者续接；Explorer 的全局目录加载不再作为 Relation 预览的前置屏障。保留现有 RelationLoad 快照与请求代际校验，并增加可见的准备状态。

**Tech Stack:** Rust 1.94 / edition 2024、Tokio、Ratatui、App Action/Command reducer、SessionRegistry、现有数据库适配器和 Rust 集成测试。

---

## 1. 背景、已确认问题与验证边界

### 已确认代码路径

| 问题 | 位置（编写计划时） | 行为 |
|---|---|---|
| 恢复为空壳 | `src/app.rs:2376`、`src/model/relation.rs:321` | 只恢复 descriptor/view，DATA/DDL 为 Empty |
| 激活只准备 Console | `src/app.rs:5677–5723,14835` | Relation 无目标会话准备 |
| 无会话静默退出 | `src/app.rs:21962–21979` | 找不到精确会话时不请求连接，不登记续接 |
| Schema 参与目标 | `src/app.rs:22397`、`src/model/session.rs:115` | 同 Profile、同 database、不同 schema 不一定匹配 |
| 刷新无法修复前置条件 | `src/app.rs:21842,21938` | refresh 判断在会话与目录检查之后 |
| 目录依赖过大 | `src/app.rs:22126` | Profile 下任意 pending catalog request 都可能阻塞 |
| 目录完成续接不对称 | `src/app.rs:18128,11644` | 成功重试当前表，失败无续接 |
| 解析依赖活动连接 | `src/app.rs:17780,17822,11660` | 发起和回填都依赖全局活动连接 |
| UI 混淆未加载与无数据 | `src/ui/relation.rs:583` | Empty 渲染 No relation data |

用户现场的两个 Console target 尚未读取；“console_1 补齐 test_schema 会话”应先用可控 reducer 测试复现，不当作已经取得的现场日志事实。

### 工作范围

- 修复 Relation DATA/DDL 的激活、恢复、刷新、分页、筛选后的公共加载流程。
- 统一已有激活入口的后置处理，保留各标签页原有焦点、位置历史和初始化语义。
- 不修改工作区持久化格式；等待状态、请求身份和查询结果均为瞬态。
- 不做按 database 合并不同 schema 会话的优化。此改动涉及 search_path、事务与执行语义，应另行设计。
- 不把这次工作扩展为所有数据库对象、Redis、Dashboard 的全新加载框架。

## 2. 必须保持的不变量

1. Relation 查询只使用由该表 descriptor 和 profile 解析出的合法目标；解析失败不能回退到任意当前连接。
2. 目标一致的 Connected 会话直接复用；Connecting attempt 只登记等待；仅 Started 发出 Connect。
3. 标签激活不等价于强制重连。显式更改 Console 执行目标的现有重连语义保留。
4. 每个标签页最多保留一个待推进的视图加载意图；用户改视图、筛选或页码时，新意图替换旧意图。
5. 已发出的 DATA/DDL 请求仍分别受各自 RelationLoad 和 request identity 校验。
6. 仅恢复标签页不批量预加载；首次激活才建立加载意图。已激活后切到别的标签页，允许原任务完成并回填原页。
7. 连接/解析完成不能抢回焦点、恢复另一个工作区或改变当前 Console 的目标。
8. 关闭标签页、目标变化、断开连接、Profile 删除/范围变化后，旧意图不能复活。
9. 有未提交编辑或活动事务时，沿用现有刷新保护，不清除编辑、不替换事务会话。
10. 全局目录成功/失败不能成为重复自动重试查询错误的触发器。
11. UI 的 Loading 只用于实际查询；会话连接与对象解析有独立可见状态；零行只属于成功结果。

## 3. 状态与接口设计

### 3.1 数据归属

在 `src/model/relation.rs` 增加准备状态及 pending intent，并由 `RelationTab` 持有。优先使用已有 `next_request_id` 分配单调递增的意图/请求序号，不增加另一套无必要的全局 ID。

意图字段契约：

| 字段 | 类型/来源 | 用途 |
|---|---|---|
| tab_id | Uuid | 完成事件定位原页 |
| tab_generation | RelationTab.generation | 拒绝关闭、重绑前的旧事件 |
| intent_id | 单调 u64 | 拒绝已被新操作替换的意图 |
| target | ExecutionTarget | 精确 database/schema 会话 |
| view | RelationView | DATA 或 DDL |
| options | RelationPreviewOptions | 捕获提交时的筛选/排序 |
| page | PageRequest | 捕获请求页，等待结束不重新读取 UI 页码 |
| reason | Activate / Refresh / Page / Query | 定义刷新重置、错误重试等差异 |
| connection | Option<ConnectionIdentity> | 绑定实际连接代际 |

准备状态建议采用单一 enum，例如 `Idle`、`WaitingForSession`、`ResolvingIdentity`、`Failed`；等待和失败携带意图或其所有者键。不要同时维护多套可相互矛盾的 loading 布尔值和重复意图副本。

`RelationLoad<T>` 继续承担 Empty / Loading / Ready / Failed / Cancelled 及 previous snapshot，不把未发查询的准备状态伪装成 `RelationLoad::Loading`。

### 3.2 推进规则

```text
激活/刷新/改查询/分页
  → 编辑与事务保护、scope/target 校验
  → 捕获或合并加载意图
  → 精确目标会话
      Connected → 继续
      Connecting → WaitingForSession
      Missing → 启动一次连接，WaitingForSession
      Failed → 显示错误；显式 r 可以新建 attempt
  → 对象身份
      当前会话下已验证 → 继续
      恢复身份或失效身份 → 定向解析，ResolvingIdentity
      缺失/解析失败 → Failed
  → 发出 LoadRelationPreview / LoadRelationDdl
  → 清除准备中的意图，将查询交给既有 RelationLoad
  → Ready / Failed
```

加载函数拆为两层：

- 面向活动 UI 的薄包装：把 `active_tab` 转为稳定 tab_id，捕获操作参数。
- 按 tab_id/intent 推进的核心：不读取活动页来决定所有者，不切焦点，不在 render/tick 中轮询。

会话准备必须返回显式结果（Ready / Waiting / Started / Failed），不能继续使用“commands.is_empty() 表示准备完成”。

### 3.3 解析请求所有权

现有 Explorer 解析使用 `pending_identity_requests`、catalog epoch 和活动连接，不直接套用到后台 Relation。

新增 Relation 专用的强类型解析请求/完成事件，复用适配器 `resolve_relation_identity_with_scope`：请求携带上述 owner、connection、relation、scope；Runtime 通过已绑定会话执行，App 按所有者身份接受结果。Explorer 解析仍保留自身 epoch/选择恢复语义。

这样避免全局 request_id 在不同 Profile 之间混用，也避免为了读取已恢复表而强行向缺少父节点的 Explorer catalog 注入条目。

恢复身份验证至少绑定 `ConnectionIdentity + relation key`；重连、catalog mutation 或 descriptor rebind 后失效。解析 None 是明确的对象缺失状态，不自动按相同名称绑定另一张新表；若现有适配器支持安全身份重绑，复用其已有规则与编辑保护。

## 4. 分阶段实施任务

以下命令在仓库根目录执行。每项按“编写行为测试 → 确认预期失败 → 实现 → 定向测试通过 → 检查 diff”推进；逻辑阶段完成后可形成独立提交，不在测试失败状态建立交付检查点。

### Task 1：建立可控的 PostgreSQL 恢复回归场景

**Files**
- Create: `tests/relation_loading_lifecycle.rs`
- Reference: `tests/workspace_tabs.rs`、`tests/execution_target.rs`、`tests/relation_runtime.rs`

**步骤**
1. 从现有 workspace fixture 提取本测试文件内的小型构造器：Postgres profile、恢复 Relation descriptor、两个不同 schema 的 Console、ConnectionSucceeded 事件。
2. 固定 profile/database 为测试值，默认 schema 为 public，表 schema 为 test_schema；模拟 public 会话已连接。
3. 增加 `restored_postgres_relation_prepares_its_schema_session`：激活表必须发出 test_schema 目标的 Connect 或复用该目标已有会话，不能空返回并停在 Empty。
4. 增加 `refresh_retries_missing_relation_session`：首次准备失败后 r 应发起新的目标准备；不能借访问其他 Console 完成。
5. 增加已连接目标复用测试：Connected identity 不变、Connect 数量为零。
6. 运行 `cargo test --test relation_loading_lifecycle -- --nocapture`，确认旧代码因缺少 Connect/等待状态而失败，而不是 fixture 错误。

**验收**：测试无需真实数据库，能准确重现目标会话缺失这一主缺口。此阶段红测与 Task 2/3 的修复作为同一首批交付。

### Task 2：定义 Relation 加载意图与准备状态

**Files**
- Modify: `src/model/relation.rs`
- Test: `tests/relation_tabs.rs`、`tests/relation_loading_lifecycle.rs`

**步骤**
1. 增加第 3 节定义的 intent、reason、preparation 类型；只在一个位置保存完整意图。
2. 给 `RelationTab` 增加瞬态准备状态及身份验证标记，更新 new/with_descriptor/restored 初始化。
3. 定义同一意图去重规则：同 target/view/options/page 的重复等待合并；显式改变参数替换并分配新 intent_id。
4. 覆盖 restored 初始化、重复刷新合并、DATA→DDL 替换、rebind 失效的行为测试。
5. 运行 `cargo test --test relation_tabs`。
6. 运行 `cargo test --test workspace_persistence`，确认不需要持久化格式迁移。

**验收**：准备状态不会创建假的数据库请求；旧 intent 身份可明确判断为过期。

### Task 3：按目标确保会话并续接原标签页

**Files**
- Modify: `src/app.rs`（relation_execution_target、加载函数、连接成功/失败处理）
- Modify: `src/model/session.rs`（仅在需要明确分类返回值时调整）
- Modify: `src/action.rs`（如现有连接命令需要区分请求用途，在现有结构上增加类型化用途）
- Test: `tests/relation_loading_lifecycle.rs`、`tests/execution_target.rs`、`tests/global_workspace.rs`

**步骤**
1. 将精确 Relation target 解析失败变为显式失败，不使用当前 connection.target 兜底。
2. 抽出不投影活动工作区的会话确保函数，复用 `SessionRegistry::request`。检查 Started/Existing：Existing Connecting 不再次发出 Connect。
3. 避免直接复用 `request_connection_target_for_editor_target`，该函数明确强制重连；也不要原样调用具有 activate_profile_workspace 副作用的连接入口。
4. 用 typed connection purpose 或等效的 session-owner 记录区分 Relation resource connection 与用户主动工作区连接，复用现有 Command::Connect 和 Runtime 连接能力。
5. 会话成功先安装 registry，再收集等待该 target/identity 的 Relation 所有者，逐一推进；不受 should_activate_workspace 或 active_tab 限制。
6. 会话失败只结束匹配 attempt 的等待意图，错误显示在原表；晚到的旧失败不能覆盖新 attempt。
7. 保留已有工作区连接投影、Console 显式目标切换与事务保护。
8. 增加两张表共用同 target 时只产生一个 Connect、切走后成功不抢焦点、多个 target 完成乱序的测试。
9. 运行 `cargo test --test relation_loading_lifecycle --test execution_target --test global_workspace`。

**验收**：Task 1 主回归通过；不访问 console_1 也能完成准备。资源连接不会覆盖全局 pending 导航或强制恢复工作区。

**建议提交**：`fix(relation): prepare target sessions for restored previews`

### Task 4：集中标签页激活后置处理

**Files**
- Modify: `src/app.rs`（NextTab、PreviousTab、ActivateTab、open_relation_descriptor、open_catalog_relation、open_location_tab、activate_context_tab、close_tab、工作区激活/恢复入口）
- Test: `tests/workspace_tabs.rs`、`tests/omni_navigation.rs`、`tests/global_workspace.rs`、`tests/relation_loading_lifecycle.rs`

**步骤**
1. 列出所有 active_tab 赋值位置，区分真正激活与临时内部上下文切换，避免后者产生额外网络请求。
2. 增加统一的真实激活后置入口；Relation 调用加载协调器，Console 调用已有目标准备，Redis/Dashboard 调用原有行为。
3. 将 Next/Previous/ActivateTab、从 Explorer 打开、导航跳转和关闭后选择邻页接入该入口，保持各自焦点和位置历史规则。
4. 工作区恢复只为最终激活的表创建意图；不要为所有恢复表请求连接。
5. 用参数化测试比较鼠标语义 Action、下一页、上一页、Explorer 重开已有表、导航、关闭邻页的 Relation 请求行为。
6. 运行 `cargo test --test workspace_tabs --test omni_navigation --test global_workspace --test relation_loading_lifecycle`。

**验收**：每个真实激活入口都有相同的目标准备保证；重复激活 Ready 表不重新查询。

**建议提交**：`refactor(tabs): centralize activation resource preparation`

### Task 5：定向解析身份，解除 Explorer 全局加载屏障

**Files**
- Modify: `src/model/relation.rs`（解析请求及验证身份）
- Modify: `src/action.rs`（Relation 专属解析命令和完成事件）
- Modify: `src/runtime.rs`（复用 resolve_relation_identity_with_scope）
- Modify: `src/app.rs`（解析推进、事件回填、移除全局 readiness 阻塞）
- Test: `tests/relation_loading_lifecycle.rs`、`tests/relation_runtime.rs`、`tests/workspace_tabs.rs`

**步骤**
1. 给恢复身份及 stale identity 加定向解析请求，携带 target/connection/tab_generation/intent_id。
2. Runtime 使用精确绑定的目标会话执行解析，返回 entry / None / error；取消或身份失效时也必须结束匹配等待。
3. App 接受解析结果时验证会话仍 Connected、请求与现存意图一致、scope 仍允许、relation 仍一致。
4. 成功只更新相应 Relation 的 descriptor/验证标记；有待提交编辑时沿用 rebind 保护；不为了预览强制加载 Explorer 的整条父树。
5. 删除 Relation 对 `relation_catalog_readiness == Loading` 的依赖；清理仅为此存在的 enum/helper。
6. 删除 accept_catalog_page 对活动 Relation 的无差别查询重试；目录事件仅在确有对象失效影响时通知协调器。
7. 改写现有 `restored_relation_waits_for_catalog_before_loading`、DDL 和 later page 测试：预期改为定向身份就绪后加载、无关分页不阻塞。保留对象身份验证意图，不直接删掉这些回归覆盖。
8. 增加无关 schema 请求永久 pending、目录最后一页失败、对象解析失败、对象不存在、Profile A/B request_id 相同的测试。
9. 运行 `cargo test --test relation_loading_lifecycle --test relation_runtime --test workspace_tabs`。

**验收**：表的加载只等待它自己的会话和身份；全局目录失败不再造成 Empty 悬挂；解析错误不会自动形成无限重试。

**建议提交**：`fix(relation): resolve preview identities independently of explorer loading`

### Task 6：统一刷新、查询参数与异步失效处理

**Files**
- Modify: `src/app.rs`（refresh_active_relation、load_active_relation_with_page、筛选提交、分页、cancel/close/disconnect、catalog mutation）
- Modify: `src/model/relation.rs`
- Modify: `src/runtime.rs`（需要的取消与目标匹配）
- Test: `tests/relation_loading_lifecycle.rs`、`tests/relation_runtime.rs`、`tests/relation_tabs.rs`

**步骤**
1. 把刷新、分页、筛选提交接入同一意图入口；refresh 捕获第一页，Page/Query 捕获各自参数。
2. Waiting 中重复 r 合并；Loading 中重复同一刷新不形成 cancel/restart 风暴。不同 options/page 替换旧请求时执行现有取消协议。
3. Ready 表的显式刷新重新查询；连接/解析失败由 r 重试完整流程；单纯切换标签页不自动反复重试失败查询。
4. DATA 与 DDL 的失败重试策略统一，移除当前自动加载对 Failed 的不一致判断。
5. 在关闭、断开、Profile 删除、scope 收窄、descriptor rebind、catalog mutation 中使相应意图失效；共享会话不要因单一标签关闭而断开。
6. 校验 relation_result_is_current 对目标会话存活的判断，不仅检查 registry 中存在，还检查状态、代际和请求归属。
7. 增加旧成功晚到、旧失败晚到、等待中换视图/改筛选、关闭后解析完成、重连后旧结果、scope 变化及活动事务保护测试。
8. 运行 `cargo test --test relation_loading_lifecycle --test relation_runtime --test relation_tabs`。

**验收**：刷新能推进所有可重试阶段；查询条件不因异步等待丢失；旧请求无法污染新状态。

**建议提交**：`fix(relation): preserve load intents across retries and invalidate stale work`

### Task 7：呈现明确等待状态与可诊断原因

**Files**
- Modify: `src/ui/relation.rs`
- Modify: `src/help.rs`（刷新说明如需更新）
- Modify: `src/app.rs`（结构化 tracing）
- Test: `tests/ui_render.rs`、`tests/keymap.rs`

**步骤**
1. 统一 DATA/DDL 状态投影，先显示对应视图的 preparation，再显示 RelationLoad 查询状态。
2. Empty 文案改为 Not loaded；等待会话显示 Connecting to database/schema；解析显示 Resolving relation；错误显示实际原因和 r 重试。
3. 保留旧快照时继续显示旧数据与状态提示；不要因新请求失败清空已有数据。
4. 成功零行明确呈现 0 rows，不使用 Not loaded 或 No relation data。
5. 使用现有 tracing 记录阶段转换、tab_id、intent_id、target、connection generation、跳过原因；不记录凭据或 SQL 参数值，不按渲染帧重复输出。
6. 增加 DATA/DDL 等待、准备失败、旧快照刷新失败、零行成功的 Ratatui 渲染测试；验证 r 在错误状态下仍映射 RefreshActiveRelation。
7. 运行 `cargo test --test ui_render relation` 与 `cargo test --test keymap relation`。

**验收**：截图场景在连接未就绪时有可理解状态；所有已发起意图最终到成功、失败或取消之一。

**建议提交**：`fix(ui): distinguish relation preparation from empty query results`

### Task 8：集成验证与最终交付

**Files**
- Test: `tests/relation_loading_lifecycle.rs`、`tests/relation_runtime.rs`、`tests/postgres_adapter.rs`
- Modify: 本计划，记录实际验证结果与偏差

**步骤**
1. 运行定向组合：

```bash
cargo test --test relation_loading_lifecycle --test relation_tabs --test relation_runtime --test workspace_tabs --test workspace_persistence --test execution_target --test global_workspace --test omni_navigation
```

2. 在前述测试通过后运行与 CI 一致的检查：

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

预期：格式无差异、clippy 零警告、测试全通过。缺失工具链或外部依赖需如实记录为环境阻塞，不能报告为通过。

3. 有本地测试 PostgreSQL 时，设置测试专用 `LAZYDB_TEST_POSTGRES_URL`，运行：

```bash
cargo test --locked --test postgres_adapter -- --nocapture --test-threads=1
```

确认输出实际执行数据库测试，而不是因未设置 URL 提前跳过。

4. 在测试库创建 public/test_schema 两个目标，用两个 Console 和一张有记录的表进行 TUI 验收；不使用截图中的远程业务库作为自动化 fixture。
5. 分别验证：重启恢复且 Console 默认 public → 首次打开 test_schema 表；只恢复表且当前活动页即为表；跨数据库；DATA/DDL；目录慢/失败；连接失败后 r；连接过程中切走再回来；返回已缓存表。
6. 最后检查 `git diff --check`、`git diff --stat` 和 `git status --short`，确认改动符合任务范围。

**验收**：首次激活无需绕行 Console；已有连接切换零额外 Connect；相同待处理加载零重复请求；无旧事件串页；全量检查通过或清晰记录环境阻塞。

## 5. 回归矩阵

| 场景 | 预期 | 主要覆盖 |
|---|---|---|
| 同库不同 Schema | 确保表的精确会话并自动加载 | reducer |
| 不同 database | 不回退到默认库查询 | reducer/runtime |
| target 已 Connected | 复用 identity，零 Connect | reducer |
| target 已 Connecting | 共用 attempt，只等待一次 | reducer |
| 两表共享 target | 一个 Connect，各自正确加载 | reducer |
| 首次恢复未激活表 | 无批量 Connect/查询 | workspace |
| 等待中切走 | 完成回填原页，不抢焦点 | reducer/runtime |
| 目录永不完成/失败 | 不阻挡已验证表 | reducer |
| 原生身份失效/对象不存在 | 定向验证、明确失败 | reducer/adapter |
| 刷新期间保留快照 | 原数据可见，错误可见 | UI |
| 连接失败后 r | 一个新 attempt，成功后续接 | reducer |
| 快速重复 r | 不发生连接/查询风暴 | reducer |
| DATA/DDL 切换 | 意图按视图正确归属 | reducer/UI |
| 页码/筛选改变 | 只应用最新意图参数 | reducer |
| 关闭/重连/scope 改变 | 旧结果被拒绝 | runtime/reducer |
| 有编辑/事务 | 保护仍有效 | relation/transaction 现有测试 |
| 成功零行 | 0 rows，与未加载区分 | UI |
| SQLite/MySQL/SQL Server | 遵守各自 target 解析语义 | 现有 adapter/target 测试 |

## 6. 实施顺序与检查点

依赖顺序：Task 1 → Task 2 → Task 3 → Task 4 → Task 5 → Task 6 → Task 7 → Task 8。

- 检查点 A（Task 3）：目标会话主缺口修复，首个 PostgreSQL reducer 回归通过。
- 检查点 B（Task 5）：统一激活、后台续接和定向身份解析闭环，彻底解除目录屏障。
- 检查点 C（Task 7）：刷新/失效/UI 语义闭环。
- 检查点 D（Task 8）：全量检查与真实 PostgreSQL 手工验收完成。

核心任务共享 App reducer、RelationTab 与 Action/Command 协议，建议按顺序实现。每阶段完成后检查已改函数调用点，不通过整文件大搬迁来混合本次行为修复。

## 7. 完成定义

- 用户给出的 console → all_types_test 路径，首次访问即可自行完成初始化。
- r 在缺会话、解析失败、查询失败时有可观察、可恢复的行为。
- 无关目录加载不影响当前表格预览就绪。
- 切换 console_1 不再成为任何 Relation 初始化的隐含前置条件。
- 新状态不改变持久化格式，既有编辑/事务和快照隔离回归通过。
- 最终交付记录修改文件、测试命令及实际结果，区分模拟测试和真实数据库验证。
