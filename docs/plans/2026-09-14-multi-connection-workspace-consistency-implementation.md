# 多连接工作区一致性修复 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
>
> 执行适配：若执行环境没有上述技能，按本文依赖顺序逐任务实施即可，不安装或假定存在额外技能。本文新增接口及测试名是拟定设计，不代表已实现。每项任务先补行为回归测试并确认失败原因，再实现和定向验证；仅在用户要求提交时创建 Git commit。本计划不要求子代理。

**Goal:** 修复多连接下旧连接的表格/SQL 不可用、Console 重复及导航卡住、执行目标选择范围错误、新 Console 默认目标不遵循焦点上下文的问题，并保证保存恢复后行为一致。

**Architecture:** 沿用 Action → App::update → Command → Runtime。全局文档及 EditorWorkspace 是唯一可写文档来源，SessionRegistry 是目标会话状态来源；Explorer 和活动 Tab 只表达交互上下文。请求携带明确的目标、会话身份及文档/请求版本，持久化从当前全局文档生成，不再参与运行时文档所有权切换。

**Tech Stack:** Rust 2024 / Rust 1.94.0、Tokio、Ratatui/Crossterm、Modalkit、Serde/TOML、现有数据库适配器、SQLite 临时文件集成测试。无需新增生产依赖。

---

## 1. 实施基线与范围

分析时间：2026-09-14。本文行号为分析时位置，实施时按符号定位。

相关历史计划：

- `docs/plans/2026-09-12-multi-connection-consoles.md`
- `docs/plans/2026-09-12-multi-connection-session-completion.md`

以上文档包含尚未完全落地或已变化的基线描述。本计划以当前代码为准，尤其覆盖旧计划“新建目标始终优先 Explorer 选中项”的规则：**新规则优先使用打开面板前的实际焦点上下文。**

已确认的代码事实：

| 位置 | 当前行为 | 后果 |
| --- | --- | --- |
| `src/app.rs:874 snapshot_active_workspace` | 克隆全部 Tab/Console，同时 `take(self.editor)` | 旧 Tab 保留但编辑器会话被移走 |
| `src/app.rs:910 append_workspace` | 追加文档，按新 profile 取回编辑器缓存 | 旧 profile 的编辑器会话没有随共享 Tab 保留 |
| `src/app.rs:827 visible_console_records` | 拼接共享列表与 workspace 快照 | 同 UUID 多次出现 |
| `src/model/sql_editor_list.rs:100 move_selection` | 按 UUID 的第一次出现位置移动 | 相邻重复 ID 导致向下移动原地循环 |
| `src/ui/mod.rs:5914` | 使用全局 connection 判断 Console 在线 | 其他在线目标被显示为未连接 |
| `src/app.rs:20196 load_active_relation_with_page` | 全局 connection 与表格 profile 不同则直接返回 | 无请求、保持 Empty、显示 No relation data |
| `src/app.rs:15601 run_active_sql` | 缺失编辑器会话被转换为空 SQL | 无可执行 scope |
| `src/app.rs:9465 OpenTargetSelector` | 当前 profile 候选、旧式全局切换 | editor 入口不能跨连接选择 |
| `src/app.rs:9511 OpenConsoleTargetSelector` | 已有全 profile 候选及按 Console 绑定路径 | 新旧入口语义不一致 |
| `src/app.rs:13729 default_console_target` | 无条件优先 Explorer 选择 | 右侧 Tab 上新建时目标错误 |
| `src/app.rs:1632 workspace_snapshot` | 非活动 profile 从旧快照取文档和 SQL | 共享 Tab 最新修改可能未进入保存结果 |

已存在且应复用：`SessionRegistry`、`ExecutionTarget`、Runtime 的 `HashMap<ConnectionKey, ActiveConnection>`、`PendingExecution` 集合、现有事务退出流程。不要重新实现平行连接管理器。

实施文档创建时未运行测试，以下测试命令都是执行阶段要求，不代表已通过。

## 2. 必须成立的行为契约

### 2.1 文档与所有权

1. 一个 Console UUID 对应一个文档；同名不同 UUID 是合法的不同文档。
2. `App.tabs` 仅表示全局已打开 Tab，顺序不因后台连接事件而变化。
3. 当前运行进程只有一个 `EditorWorkspace`，连接切换不移动、替换或重建它。
4. 文本、光标、选区、撤销历史、输出编辑器均保留；关闭/重开遵循已有文档生命周期。
5. 文档记录是名称、绑定、事务模式的权威来源；若保留 ConsoleTab 的派生副本，只能通过集中方法同步，不允许散落直接写字段。
6. 所有 Console 操作从同一个全局集合定位，不能出现列表可见但打开/删除/重命名找不到的记录。
7. 新建、激活及绑定 Console 不要求先联网；表格加载与显式执行才按需连接。

### 2.2 连接与请求

1. SQL 使用该 Console 的完整 ExecutionTarget，表格使用对象所属目标，不能使用全局当前连接兜底。
2. 同 profile 不同 database/schema 初版继续按完整目标隔离会话，避免改动驱动池共享策略。
3. 已在线目标复用；同目标连接中 single-flight，多目标可独立连接。
4. 打开 B 不取消 A 的查询/表格加载，不修改 A 的绑定，不清空 A 的结果。
5. 后台连接、加载成功或失败只更新其所属状态；是否导航由用户发起请求时的意图决定。
6. 重连、断开、超时、删除 profile 按目标或 profile 精确处理；旧 generation 响应不能覆盖新状态。
7. 实际事务会话必须固定；不能把活动事务自动迁移到新 Session。
8. 缺少可用会话、连接失败、目录未就绪必须有明确状态，不能伪装为成功的空表。

### 2.3 默认目标与面板

打开 consoles 时捕获 `origin_context`。按 a 时使用这个来源；列表选中、搜索、重命名不会修改来源。

| 来源 | 默认目标 |
| --- | --- |
| Explorer profile | 该 profile 最近有效目标，否则配置默认目标 |
| Explorer database | 该 database 内的有效最近目标，否则按驱动规则规范化默认 schema |
| Explorer schema、表、列、索引等子节点 | 沿目录父级得到 profile/database/schema |
| SQL editor、该 Console 的 Results/Output | 该 Console 的当前绑定 |
| Relation 的 Data/DDL | 该 Relation 对象所属目标 |
| Dashboard / 有明确 profile 的 History 上下文 | 该 profile 最近有效目标，否则默认目标 |
| RedisBrowser | 该 Tab 的 profile 和逻辑 database，schema=None |
| 无明确目标的分组、全局 History、未绑定 Tab | 最近有效访问目标；仍没有则未绑定 |

若来源目标已删除或失效：先寻找相同 profile 内仍合法的目标，再回退最近有效访问目标；若 profile 已不存在则直接进入后者。不要按名称排序随意绑定到未访问连接。

“最近访问”由显式 Explorer 选择/Tab 激活、显式绑定及发起数据库操作更新。背景刷新和异步成功事件不更新最近访问；快捷键上下文在变更 focus/active_tab 前解析。现有 `recent_targets` 可继续复用，但要区分用户行为与后台事件来源。

### 2.4 持久化

运行时全局集合是唯一来源。保存需覆盖所有打开/关闭 Console 的最新名称、目标、SQL 和 Tab 顺序，不依赖 active_profile。恢复不自动连接，不把历史在线状态或事务恢复成可用会话。

## 3. 设计边界

### 3.1 最小结构调整

优先保留当前 `Vec<ConsoleRecord>`，通过集中查找/插入/更新入口保证 UUID 唯一；本次不强制迁移为 HashMap，避免不必要的全库改动。`tabs` 保持 Vec，保证顺序。只有恢复边界允许接收按 profile 存储的旧文档数据，导入后即归一化到全局集合。

移除运行期 `workspaces`、`workspace_editors` 的文档快照职责。若仍需按 profile 记忆最近 Tab/focus，用只含 ID/焦点的小型导航状态；不能保存另一套 Console、SQL 和 EditorWorkspace。

`ConnectionState` 可暂时保留为显示投影。禁止数据库命令和权限/能力判断用它代替请求目标；server、mutation capabilities、owner context 的读取必须匹配实际会话身份。

### 3.2 建议的集中接口

以下是职责契约，最终类型名按周围风格调整，不要求机械引入全部封装：

| 接口 | 输入/输出职责 |
| --- | --- |
| `console_record(id)` / 集中更新方法 | 唯一文档记录查找及派生 Tab 字段同步 |
| `interaction_target(context)` | 纯解析来源上下文，不请求网络，不修改绑定 |
| `target_for_tab(tab_id)` | 从 SQL/Relation/Redis/其他有归属的 Tab 提取目标 |
| `connected_session(target)` | 只返回目标匹配且 Connected 的会话 |
| `ensure_target_session(target, intent)` | 复用、合并等待或发出一次 Connect |
| `resume_waiters(identity)` | 按注册的目标/版本恢复有效请求，不读取 active_tab |
| `open_console_target_selector(console_id)` | 唯一选择器入口 |

`get_by_identity().is_some()` 不是“在线”判定，因为注册表还包含 Connecting/Failed attempt。状态显示、执行可用性、过期事件接收使用不同的明确条件。

### 3.3 等待者与版本

复用现有 PendingExecution 保存的 SQL/文档 revision/事务快照。为表格待连接请求保存 tab_id、tab_generation、目标、加载类型、页请求、筛选快照及请求 ID；保存在 App 层即可，不必为了等待连接向驱动请求伪造 identity。

连接成功后，先验证 Tab 仍存在、目标/请求版本未变化，再创建携带真实 identity 的 RelationRequest。若新 Session 尚无该对象的目录承认记录，先走现有目录解析/对账路径；不能跳过 Runtime 的 known_relations 校验来让预览通过。

一个目标可有多个等待者；关闭一个 Tab 只删除其等待者。SQL 重绑时若待执行尚未结束，复用现有取消/等待语义，不让旧 SQL 在新目标执行。凭据弹窗若有全局单槽，也必须保持 attempt 归属并按已有串行交互机制排队。

### 3.4 持久化格式决策

当前 `WORKSPACE_VERSION=5` 且已含顶层 consoles/tabs。首选继续使用 v5 的全局字段作为新写入来源，profile 段仅兼容读取；先验证 loader/validator 已支持该语义。

特别检查 Dashboard 持久化项目前没有 profile_id：若移到全局 tabs 无法保持归属，必须补齐所属字段并定义旧 profile 段继承规则。若这导致旧格式语义无法无歧义兼容，再升级为 v6，并补 v5→v6 转换；不要为了省版本号丢失归属或改变旧字段含义。

同 UUID 重复输入：只对可证明是同文档的镜像记录归一化，优先明确的全局权威记录；同优先级冲突且无 revision 证据时报告迁移冲突并保留源文件，不用空 SQL 或任意一项覆盖。不同 UUID 同名记录全部保留。运行时一旦恢复完成，不再保留可参与列表或保存的旧快照。

## 4. 任务依赖与执行方式

```text
T01 基线与故障测试
  → T02 全局文档所有权
  → T03 保存恢复归一化
  → T04 Console 面板一致性
  → T05 目标 Session 与等待者隔离
  → T06 Relation 路由与加载状态
  → T07 SQL/事务/元数据执行归属
  → T08 统一跨连接目标选择器
  → T09 焦点来源与默认目标
  → T10 真实多连接集成与文档收尾
```

推荐顺序执行，多个任务共同修改 `src/app.rs`，不建议在同一文件上并行实施。T02/T03 是一个交付里程碑：仅修 UI 而仍从旧快照保存不能发布。每个任务内按照“新增用例 → 定向运行 → 单个实现步骤 → 定向重跑 → diff 检查”拆成小步，批量用例逐个补充，不一次改完再测试。

## T01：固定基线与四类故障回归

**Files**
- Modify: `tests/global_workspace.rs`
- Modify: `tests/relation_tabs.rs`
- Modify: `tests/execution_target.rs`
- Modify: 本计划的执行记录章节

**步骤**
1. 记录当前 HEAD、工作区已有修改、Rust 工具链和数据库测试环境是否可用。
2. 运行已有定向基线，记录已有失败，不修改测试以绕过旧模型问题：
   `cargo test --locked --test global_workspace --test connection_switch --test relation_tabs --test execution_target --test workspace_persistence`
3. 在 `tests/global_workspace.rs` 复用现有 memory_profile/server/connect helpers，加入下述完整的文档会话回归用例。
4. 再独立添加 UUID 唯一、跨连接表预览会发出 A 请求、editor 常用入口包含 B 目标、右侧 B 焦点新建绑定 B 四个行为用例。构造两个不同 profile UUID，不能仅改变名称。
5. 单独运行每个新增测试，确认失败来自目标故障，而不是编译错误或测试前置条件缺失。

```rust
#[test]
fn opening_second_profile_keeps_first_editor_session_available() {
    let first = memory_profile("first");
    let second = memory_profile("second");
    let first_id = first.id;
    let second_id = second.id;
    let mut app = App::new(vec![first, second]);

    connect(&mut app, first_id, "first");
    app.update(Action::ReplaceEditor("SELECT 'first'".into()));
    let first_console = app.active_console().id;

    connect(&mut app, second_id, "second");

    assert!(app.tabs.iter().any(|tab| tab.id() == first_console));
    assert_eq!(app.editor_text(first_console).unwrap(), "SELECT 'first'");
}
```

运行：`cargo test --locked --test global_workspace opening_second_profile_keeps_first_editor_session_available -- --exact`

**验收**：新增故障用例准确捕获旧行为；已有测试中要求“切换全局连接、清空旧 workspace”的断言单列为需更新的契约，不当作新需求的正确行为。

**建议提交主题**：`test(workspace): reproduce multi-connection state regressions`。失败测试与后续修复在同一可交付变更中合并。

## T02：统一全局文档和编辑器所有权

**Files**
- Modify: `src/app.rs` — snapshot/append/install/activate workspace、Console CRUD、has_active_workspace
- Modify: `src/model/workspace.rs` — ConnectionWorkspace 的职责
- Modify: `src/model/tab.rs` — 文档与运行态字段同步边界
- Modify: `src/editor/mod.rs` — 编辑器会话保留及一次性恢复入口
- Test: `tests/global_workspace.rs`, `tests/editor_projection.rs`, `tests/workspace_tabs.rs`

**步骤**
1. 补 A→B→A→B 后文本、revision、光标和 undo/redo 保留测试，使用现有编辑器操作 API。
2. 补未连接 Console 打开/激活/编辑不发送 Connect 的用例；补连接成功不改写其他 Console 目标的用例。
3. 把恢复导入与用户连接导航拆开：一次性导入旧 workspace，日常连接只更新会话或导航状态。
4. 删除运行期 `take(self.editor)` 和整体 install/clone 文档的调用路径。DDL/output 会话与 SQL 会话一起保留。
5. 收敛 Console 插入/更新/删除方法，确保记录 UUID 唯一，Tab 副本同步在一个位置完成。
6. 去除 `active_workspace_profile` 对离线文档 CRUD 的门禁；profile 导航状态不再决定文档是否存在。
7. 检查 RequestConnect、ConnectionSucceeded 不再把先前活动 Console 当作 pending_target_console 重新绑定。保留原有显式“新建文档”的业务入口，连接后台完成不能新建重复文档。
8. 运行定向测试并审查所有保留的 ConnectionWorkspace 使用位置；临时兼容结构仅供 T03 恢复流程使用。

运行：`cargo test --locked --test global_workspace --test editor_projection --test workspace_tabs`

**验收**：旧 Tab、编辑器会话、文档记录一一对应；无连接切换触发编辑器搬迁；新旧 Console 的 SQL 都能即时读取。T01 文档回归转绿。

**建议提交主题**：`fix(workspace): retain global document and editor ownership`

## T03：保存恢复只使用全局权威状态

**Files**
- Modify: `src/app.rs` — workspace_snapshot、restore_workspace、persisted_workspace_from_parts
- Modify: `src/persistence/workspace.rs` — 全局格式、迁移与校验
- Test: `tests/global_workspace.rs`, `tests/workspace_persistence.rs`, `tests/workspace_tabs.rs`, `tests/profile_lifecycle.rs`

**步骤**
1. 补“打开 B 后直接修改 A 的共享 Console，保存恢复得到 A 最新 SQL”的测试；不要先切换 profile 来触发旧快照同步。
2. 补关闭 Console 的文本/重命名保存、跨 profile 重绑保存、未绑定与失效目标保存、全局 Tab 顺序与活动 Tab 保存测试。
3. 补 v5 profile 段导入 fixtures，覆盖同名不同 UUID、多处相同 UUID、Dashboard profile 归属。
4. 按第 3.4 节完成格式可表达性验证；保持 v5 或明确升级 v6，将选择理由写入执行记录。
5. 改 workspace_snapshot 为遍历全局记录/Tab/editor；不再从旧 workspace.sql 读取，不因 active_profile 排除其他文档。
6. 缺失编辑器会话时阻止本次文档被空文本覆盖并报告保存错误；沿用现有保存失败状态机，不能静默 unwrap_or_default。
7. 恢复后归一化 UUID、Tab 引用和活动 Tab，保持当前格式校验；移除运行态重复快照集合或改为明确的一次性输入。
8. 在临时目录执行 save→load→restore→save，比较语义快照及 SQL 文件内容，确认第二次保存幂等。

运行：`cargo test --locked --test global_workspace --test workspace_persistence --test workspace_tabs --test profile_lifecycle`

**验收**：任意 profile 的最新编辑不会丢失；关闭状态与顺序正确；不存在一个 UUID 多次写出；重启没有自动连接或虚假在线状态。

**建议提交主题**：`fix(persistence): serialize canonical global workspace state`

## T04：修复 Console 列表、操作与状态展示

**Files**
- Modify: `src/app.rs` — visible_console_records/ids、Console manager actions
- Modify: `src/model/sql_editor_list.rs`
- Modify: `src/ui/mod.rs` — consoles 面板渲染
- Test: `tests/global_workspace.rs`, `tests/ui_render.rs`，及上述模型/App 内现有单元测试

**步骤**
1. 补两连接反复打开后可见 UUID 唯一的测试；用不同 UUID 的同名 console 验证不会误去重。
2. 补从首项按 Down/j 遍历所有项、Up/k 回退、重命名重排后维持同 UUID 选择的用例。
3. visible_console_records 只投影全局记录；删除与旧 workspace 的 chain。若迁移中暂需兼容，去重仅放在归一化边界。
4. 打开、关闭、重命名、删除、搜索全部使用全局记录访问方法；删除后不能从快照恢复出“幽灵”记录。
5. 按完整 target 的 SessionStatus 展示已连接/连接中/失败/未连接，目标有效性和文档 OPEN/CLOSED 独立显示。
6. 固定名称和状态列的 cell 宽度，补齐名称空白，保证最小间隔；选中滚动跟随，适配 Unicode 与窄终端。
7. 更新旧的名称排序测试与现有 open-first 行为之间不一致的断言：保留当前产品排序规则，统一测试；本次不额外更改排序策略。

运行：`cargo test --locked --test global_workspace --test ui_render`

运行：`cargo test --locked --lib sql_editor_list`

运行：`cargo test --locked --lib console_manager`

**验收**：始终只有一行选中；方向键可以走过整张列表；A/B 同时在线均显示已连接；截图中的名称与 OPEN 黏连消失。

**建议提交主题**：`fix(consoles): use unique records and target-scoped status`

## T05：统一目标 Session 解析与待连接请求

**Files**
- Modify: `src/model/session.rs`
- Modify: `src/app.rs` — request_connection_target_inner、prepare_active_console_target、连接成功/失败及 pending 状态
- Modify: `src/action.rs` — 仅在缺少意图或请求身份时扩展事件
- Modify: `src/runtime/connections.rs`, `src/runtime.rs` — 定向安装/退休资源
- Test: `tests/connection_switch.rs`, `tests/profile_runtime.rs`，及 SessionRegistry/ConnectionAttempts 单元测试

**步骤**
1. 补 A/B 不同目标同时连接、B 先成功/A 后成功、A 成功/B 失败的乱序用例。
2. 补同目标两个 Console 等待只发送一次 Connect、激活在线 Console 不 force_reconnect 的用例。
3. 集中 connected_session/ensure_target_session，沿用现有 request/attempt 管理，不创建第二套 session Map。
4. 将全局 pending_target/pending_editor_target_switch 从正确性依据降为兼容投影或移除；请求意图按 attempt/目标记录。
5. 将执行等待者按文档/请求关联到 session attempt；成功和失败只唤醒该 identity 对应目标的等待者。
6. 区分显式 profile 导航与后台执行连线：后台成功不能改变 active_tab、Explorer selection 或其他 Console 绑定。
7. 检查凭据请求、取消、失败回调均携带原 attempt 归属；关闭等待中的单个 Tab 不取消其他等待者所需的连接。
8. 显式重连成功定向退休旧 identity 并清理其资源；失败保留原在线会话。活动事务仍固定在原身份，禁止无条件替换其资源。
9. 删除打开其他连接时的全局查询/表格事务门禁，保留真正修改同一事务资源的限制。

运行：`cargo test --locked --test connection_switch --test profile_runtime`

运行：`cargo test --locked --lib model::session`

运行：`cargo test --locked --lib runtime::connections`

**验收**：不同连接可以独立进入 Connecting/Connected/Failed；成功次序不影响归属；普通激活不产生额外网络连接。

**建议提交主题**：`fix(session): scope connection intents and waiters by target`

## T06：表格预览、DDL 和分页按对象目标路由

**Files**
- Modify: `src/app.rs` — load_active_relation_with_page、relation_page、relation request acceptance、取消路径
- Modify: `src/model/relation.rs` — 加载/等待状态及请求上下文
- Modify: `src/runtime.rs` — load_relation、目录对象校验/对账路径
- Modify: `src/ui/relation.rs`
- Test: `tests/relation_tabs.rs`, `tests/relation_runtime.rs`, `tests/catalog_reducer.rs`

**步骤**
1. 补全局显示 B 时打开 A 表，发出的 LoadRelationPreview.connection 属于 A，relation/profile/database 一致的用例。
2. 补同 profile 两个 database 的同名表用例，防止仅校验 profile_id 就误路由。
3. 从 Relation descriptor/qualified name 解析目标，按驱动规范化；替换 database_command_identity 的全局依赖。
4. 未连接时登记表格等待者和显式等待状态；成功后校验 Tab/request 版本，再发真实请求。
5. 串接目录对象 readiness/known_relations 的正确身份映射。执行会话与 catalog_sessions 可以不同，不能直接用旧目录会话的承认记录冒充新会话。
6. Preview、DDL、刷新、排序/筛选、分页共用目标解析和请求构造，避免只修初次打开。
7. cancel_relation_requests_for_connection/profile 先判断请求归属，再修改 pending 状态；当前实现先 cancel 再过滤的副作用也需消除。
8. 关闭 Tab、再次分页、重连旧 generation 的迟到事件只按请求身份处理，不覆盖新结果。
9. UI 区分连接中、目录解析中、加载中、失败、成功零行；保持旧数据可见时附加明确状态。

运行：`cargo test --locked --test relation_tabs --test relation_runtime --test catalog_reducer`

**验收**：A/B 表格交替及并行加载都可用；打开 B 不取消 A；成功零行和未发送请求有不同状态。

**建议提交主题**：`fix(relations): route preview and ddl through owning sessions`

## T07：收敛 SQL、事务及关联元数据的执行归属

**Files**
- Modify: `src/app.rs` — run_active_sql、run_console_sql_on_session、validate_draft、dispatch_draft、事务/补全/能力解析
- Modify: `src/runtime.rs`, `src/runtime/transaction.rs`
- Modify: `src/model/session.rs` — 会话级能力/owner context（必要时）
- Modify: `src/sql/diagnostics.rs` — 若诊断 key 的目录版本仍跨 profile 共用
- Test: `tests/sql_execution.rs`, `tests/transaction_reducer.rs`, `tests/quit_transaction_review.rs`, `tests/sql_completion.rs`, `tests/sql_diagnostics.rs`, `tests/catalog_editor_reducer.rs`

**步骤**
1. 补 A/B Console 同时 SELECT、乱序完成后各自结果正确的测试；A 手动事务中 B 仍能执行。
2. run_active_sql 缺会话不再吞为默认空文本；显式区分空 SQL 与不存在的 editor session。
3. 将按 Session 执行与旧全局回退路径合并，ExecutionDraft 永远使用该文档目标/identity/方言。
4. 校验 pending SQL 的目标和文档/绑定版本；连接等待期间编辑、取消、关闭、重绑均不能执行过期快照。
5. commit/rollback、关系编辑保存及退出检查按真实事务持有 identity 路由；断开 A 只审核 A 的资源，应用退出仍审核全部。
6. 对 active_profile/database_command_identity/connection.server/能力字段的调用按功能建立核对清单：UI 显示可保留投影，SQL 方言、只读权限、目录修改、补全、诊断、Dashboard 刷新必须使用所属目标。
7. 目录缓存失效使用其 profile/会话 epoch；B 的后台刷新不能把 A 的 completion/diagnostics 当作新目录，也不能让 A 请求错误使用 B 的能力。
8. 对配置修改、删除 profile 和失效回调只退休对应资源，保留其他文档、编辑器与结果。

运行：`cargo test --locked --test sql_execution --test transaction_reducer --test quit_transaction_review --test sql_completion --test sql_diagnostics --test catalog_editor_reducer --test profile_lifecycle`

**验收**：SQL 和表格修改不会依赖最后打开的连接；事务不会跨目标漂移；未涉及的连接查询不受影响。

**建议提交主题**：`fix(execution): resolve sql and transaction context per document`

## T08：统一跨连接 execution target 选择器

**Files**
- Modify: `src/action.rs`
- Modify: `src/app.rs` — OpenTargetSelector/OpenConsoleTargetSelector/ConfirmTargetSelector、候选生成与绑定
- Modify: `src/model/workspace.rs` — TargetSelector overlay 状态
- Modify: `src/ui/mod.rs`, `src/input/keymap.rs`
- Modify: `src/editor/mod.rs` — 如需调整 EditorEffect 映射
- Test: `tests/execution_target.rs`, `tests/commands.rs`, `tests/mouse.rs`, `tests/ui_render.rs`

**步骤**
1. 分别通过 editor effect、快捷键、已有鼠标/命令入口打开选择器，断言都包含 A/B 的合法目标并保存同一个 console_id。
2. 统一所有入口到 open_console_target_selector(console_id)，删除 console_id=None 的全局切换确认分支。
3. 首屏按连接分组展示所有可用 profile，支持组合搜索 profile/database/schema；当前目标预选，最近目标优先展示但不改变绑定。
4. 数据库/schema 采用按需目录加载；未连接 profile 仍可看默认目标，需要展开目录时显式确保该 profile 的目录会话。
5. 异步候选更新保存目标身份而非裸 index；用户关闭/重新打开选择器时，旧加载结果不能改写新选择器状态。空候选时显示原因并禁止无效确认。
6. 确认只更新指定 Console，重置该 Console 的派生结果/补全状态并持久化，保留 SQL 文本及 undo；不立即连接且不切换其他 Tab。
7. 正在查询、待执行或有未结束事务的该 Console 遵循现有局部限制；无关 Console 的活动不阻塞。
8. 候选根据驱动规范化 MySQL/MariaDB schema、SQLite main、Redis schema=None；不把 Redis 混入只适用于 SQL schema 的步骤。

运行：`cargo test --locked --test execution_target --test commands --test mouse --test ui_render`

**验收**：用户实际使用的 editor 入口可以选其他连接；不同入口行为一致；后台目录返回不会导致选中项跳到另一个目标。

**建议提交主题**：`feat(consoles): unify cross-connection target selection`

## T09：新 Console 继承弹窗来源上下文

**Files**
- Modify: `src/model/execution_target.rs` — 上下文目标解析与回退
- Modify: `src/model/sql_editor_list.rs` — origin_context
- Modify: `src/app.rs` — OpenSqlEditorList、create_and_activate_sql_editor、default_console_target、remember_target、焦点/Tab 操作
- Modify: `src/model/tab.rs` — 统一 Tab 目标提取（如需要）
- Test: `tests/execution_target.rs`, `tests/global_workspace.rs`, `tests/mouse.rs`, `tests/workspace_tabs.rs`

**步骤**
1. 按第 2.3 节建立表驱动用例，每行独立断言目标 profile/database/schema。
2. 实现纯上下文解析：Explorer 子节点沿父级查找，不假定每种对象的 native_path 形状相同。
3. 在打开 consoles 之前捕获 focus、来源 Tab/Explorer 节点及当时解析目标；点击面板入口也应保留原始焦点。
4. a 创建时先解析已捕获来源的有效目标，不能使用列表当前选中 Console。捕获来源后发生 profile 删除/范围变更要重新验证合法性。
5. 直接新建快捷键不经过弹窗时，使用当前上下文；两条路径共用同一创建函数，并在设置新 active_tab/focus 前确定目标。
6. MRU 只在显式访问/绑定/操作更新，移除后台 ConnectionSucceeded 的无条件 remember_target；同 profile 内回退先于跨 profile MRU。
7. 补“Explorer 留在 A、焦点在 B Results、新建得到 B”、“焦点在 A 的列节点、新建得到 A schema”、“面板移动到 C 再按 a 仍继承来源 B”用例。
8. 补所有目标都无效/没有 profile 时创建未绑定文档，不发 Connect。

运行：`cargo test --locked --test execution_target --test global_workspace --test mouse --test workspace_tabs`

运行：`cargo test --locked --lib default_target`

**验收**：创建目标与打开面板前的工作位置一致；后台连接完成、旧 Explorer 选择和面板导航不会污染新建目标。

**建议提交主题**：`fix(consoles): inherit new targets from interaction context`

## T10：真实多连接集成、完整检查及使用说明

**Files**
- Create: `tests/multi_connection_workspace.rs`
- Modify: `docs/architecture.md`
- Modify: `README.md`（仅相关 Console/目标操作说明）
- Modify: 本计划执行记录

**步骤**
1. 使用两个独立 SQLite 临时文件，创建同名表 `routing_probe`，分别写入唯一值 A/B；不能使用内存数据库来代替会被多次连接的同一个测试实例。
2. 复用现有 Runtime harness 驱动 Action/Command/事件循环，测试两连接 SQL 与 Relation preview 都返回正确标记，连接回调交错时也不串线。
3. 测试 A 查询期间操作 B、断开 A 后 B 继续可用、A 重连后旧 generation 结果被忽略；采用事件/屏障控制顺序，不用 sleep 维持竞态。
4. 测试持久化 round-trip：两连接文档、最新 SQL、重绑、关闭状态及 Tab 顺序恢复一致。
5. 执行与 CI 对齐的检查：
   - `cargo +1.94.0 fmt --all -- --check`
   - `cargo +1.94.0 clippy --all-targets --all-features -- -D warnings`
   - `cargo +1.94.0 test --all-targets --all-features`
6. 若更改驱动路由/事务相关代码，在配置了现有数据库测试环境时运行 PostgreSQL/MySQL/SQL Server 集成组，参照 `.github/workflows/ci.yml`；缺少环境的测试记录为未验证，不能将条件跳过记为真实数据库通过。
7. 手动执行第 5 节场景，记录真实连接类型、终端尺寸及结果。至少包含一种支持 database/schema 的服务端数据库；SQLite 不能证明 schema 隔离。
8. 更新架构文档和用户说明，明确全局文档、离线创建/绑定、执行时连接、焦点默认目标、关闭与断开的区别。
9. 检查 diff，不纳入其他工作计划或用户改动；记录尚未覆盖的数据库环境与既有失败。

运行：`cargo test --locked --test multi_connection_workspace -- --nocapture`

**验收**：四个用户现象全部消失；定向和完整检查通过；已有环境限制明确记录；不会以旧测试数量替代端到端场景覆盖。

**建议提交主题**：`test(workspace): cover concurrent connections end to end`

## 5. 人工验收脚本

| 编号 | 操作 | 预期 |
| --- | --- | --- |
| M01 | A 打开表格并写 SQL，打开 B，再点 A 表格和 Console | A 预览/查询成功、SQL/撤销历史保留 |
| M02 | A/B 各自运行查询，交替 Tab | 结果回到各自 Tab，目标未变化 |
| M03 | A→B→A 重复三轮，打开 consoles，Down/j 遍历再 Up/k 返回 | ID 唯一、一次只有一行选中、没有卡住 |
| M04 | A/B 均在线，查看 consoles，断开 A | 起初都已连接，随后只有 A 变为未连接 |
| M05 | 从 SQL editor 切 execution target 到另一 profile/database/schema | 仅该 Console 绑定改变，SQL 保留，执行命中目标 |
| M06 | Explorer 停在 A，焦点在 B editor/Results，打开 consoles 按 a | 新 Console 绑定 B |
| M07 | 焦点在 A 的表/列/索引节点，打开 consoles 按 a | 新 Console 绑定 A 的对应 database/schema |
| M08 | 从 B 打开 consoles，列表选择 C 再按 a | 新 Console 仍绑定来源 B |
| M09 | 两连接中修改旧 Console 文本，关闭一个 Tab，保存退出重启 | 最新文本、目标、关闭状态、Tab 顺序不丢失，无虚假在线 |
| M10 | A 手动事务未结束，B 查询/切自己目标 | B 可独立操作，A 事务固定且仍受原退出规则保护 |
| M11 | 同 profile 两个 database/schema 各有同名表 | 预览和 SQL 不串库、不串 schema |
| M12 | B 连接失败或加载失败，继续 A；恢复 B | A 始终可用，B 错误明确且可重试 |

## 6. 交付里程碑

- **M1：文档一致性（T01–T04）**：旧 SQL 会话不消失、列表唯一、最新 SQL 可保存恢复。
- **M2：运行一致性（T05–T07）**：表格/SQL/事务按目标独立工作，异步事件不串线。
- **M3：交互一致性（T08–T09）**：跨连接选择器与焦点默认目标完成。
- **M4：可交付（T10）**：端到端、完整检查、人工验收与说明完成。

所有里程碑完成后再视为本次用户问题已解决。执行中优先保证各阶段可验证，不把 SessionRegistry 或 Runtime 已有多连接 Map 的存在当作多连接行为已正确的证据。

## 7. 执行记录（实施时填写）

- 基线 commit / 工作区状态：待记录。
- 工具链及可用数据库环境：待记录。
- 原有定向测试失败：待记录。
- T01 新增用例的失败证据：待记录。
- 持久化保持 v5 / 升级 v6 的决策及依据：待记录。
- T02–T09 定向验证结果：待记录。
- 完整检查与数据库集成结果：待记录。
- M01–M12 人工验收结果：待记录。
