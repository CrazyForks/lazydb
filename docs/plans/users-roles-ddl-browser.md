# Users & Roles DDL Implementation Plan

> **执行者：Luna。** 按下列端到端单元持续实施、验证与审查；Astra 仅负责分析/计划。本任务用户协议优先于技能默认的委派、另开会话或等待 resume 流程。不要启动子 Agent。

**Goal:** 在关系型连接第一层末尾展示 Users & Roles，支持用户/角色列表与独立、无子 tab 的只读 DDL 浏览，完整复用现有 SQL 高亮、滚动条和 Vim 交互。

**Architecture:** 引入独立 principal 身份、列表/DDL 读取契约和 PrincipalDdl workspace tab；树节点挂在连接下，不伪装成 Schema/Relation。所有编辑器渲染和输入仍通过现有 ReadOnly session 与 ReadOnlySqlEditor，驱动负责原生作用域、类型判别和 SQL 生成。

**Tech Stack:** Rust 1.94、Tokio、Ratatui、modalkit、sqlx、tiberius、可选 Oracle driver；沿用既有 reducer/action/command/runtime 与 workspace serde 持久化。

---

## 0. 执行约束、基线和交付

- 原空间 `/Users/yelog/workspace/tui/lazydb`，目标 `main`，指定起点 `303058ce01b77955379d7e2bf0bdebb3493388d1`。任务名称和分支由计划完成后 Luna/插件自动命名，本计划不创建或指定最终分支。
- 分析：`.git/opencode-tasks/ses_f47b39e22ffeeu8nbNWh1JdCNv/analysis.md`；验证日志同目录 `validation.md`；恢复优先读插件 checkpoint（本次尚不存在）。不改 state.json，不复用 analyze 回执。
- 计划阶段实际工作区 main ahead 5，并有他人未提交的 `src/app.rs`、`tests/mariadb_catalog.rs` 及其他计划文档；不要 stash、reset、提交或覆盖这些文件。实现开始时用插件安排的隔离 worktree，核对 HEAD 与起点；本计划文件应带入任务分支。
- 起点之后的已见 diff 主要是 catalog editor 修改单元格高亮。合并前针对 app/tab/UI 最新变更作增量审查；本计划中的符号比行号更可靠。
- 所有下述测试命令是**待执行要求**，不是已通过结果。定向测试验证单元，全量检查在功能齐备后一次执行；代码/环境无相关变化不重复。
- 编译错误、普通实现取舍自行解决。人工/PTY 环境问题最多一次有针对性的修复重试，再记录限制并交由收尾审查。普通未完成项不作为 blocked。

## 1. 固定产品决策

### 需求与验证分级

- **用户直接需求（功能验收）**：连接第一层末尾分组、用户/角色的图标与颜色区别、正确列表、Enter 后准确命名的 DDL-only tab、与已有 DDL 一致的高亮/滚动条/Vim 操作。以实现行为和自动化测试验证，不额外要求用户人工签字。
- **项目已有强制门禁**：CI 中的 fmt、clippy、全量 Rust 测试及项目既有数据库 job；沿用项目规则。下文新增 principal 回归测试纳入对应测试集，不能将缺数据库时的 skip 写为已验证。
- **工程实现与回归标准**：请求隔离、身份去重、错误/partial、持久化兼容等是本方案为融入已有应用选择的技术要求，不冒充用户新增需求；允许 Luna 在不损害功能和兼容性的前提下调整具体实现。
- **补充建议验证**：人工 PTY、不同终端/字体矩阵、超出既有 CI 服务范围的额外服务器版本与权限组合。它们不是新强制门禁。环境不可用时记录具体缺口，最多一次针对性修复重试，由 Luna 收尾审查决定是否补证；不得仅因这些补充检查缺失无限 progress。
- 下文真实数据库命令是获得驱动证据的建议执行方式；项目已有 CI 覆盖项仍按 CI 门禁执行。未能本地运行时明确转交 CI 或记录限制，不能宣称真实集成通过；无需阻止继续实施其他可验收单元。

1. 普通 explorer 视图中，每个关系型连接的最后一个第一层子项是 `Users & Roles`，同级的 databases/加载/分页行排在它之前。展开后直接列 users 和 roles，用户在前，每类按稳定名称排序。
2. 分组按需加载，不在连接展开时逐账号读取 DDL。不同 profile、SQL Server database、MySQL host 必须保持独立身份。
3. SQLite 分组可展开，显示 `SQLite does not support users or roles`，不发账号 SQL；Redis 不显示分组。
4. Enter user/role 打开或激活唯一 tab；标题 `{driver icon} {display name}@{connection name}`。正文无 DATA/DDL selector，无 DDL/RELATION DDL 标题，无 relation data 区域；保留必要 loading/error/source/position 状态。
5. DDL 只读；沿用现有 DDL 的 Vim、搜索、复制、鼠标和滚动条。不新增 principal CRUD UI。
6. “当前数据库”使用连接绑定目标。PG cluster roles、MySQL/MariaDB server accounts 按原生作用域显示；SQL Server 展示 database users/roles，不混入 server logins；Oracle 使用当前 service/container。
7. 列表和 DDL 支持完整/部分可见/不支持/失败的区分。DDL 展示可读对象定义、直接 membership 和可见显式 grants，不声称无损备份认证信息或所有继承权限。

### 图标/颜色（集中在 IconSet 和主题映射中维护）

| 对象 | NerdFont | Unicode | ASCII | 颜色 |
| --- | --- | --- | --- | --- |
| group | MD_ACCOUNT_GROUP | ♟ | UR | theme.accent / 默认 #63E6D8 |
| user | MD_ACCOUNT | ● | US | theme.action / 默认 #65A7FF |
| role | MD_SHIELD_ACCOUNT | ◇ | RL | theme.syntax_column / 默认 #C792EA |

确认 nerd-font-symbols 0.3 的实际常量导出；名称不同使用该 crate 等价 glyph，不新增依赖。color=never 使用 Reset。tab 使用既有 database()/database_color()，不使用 principal 图标。

## 2. 类型与状态契约

新增 `src/db/principal.rs`，以下是实现所需字段约束，不要求逐字沿用类型名：

| 类型 | 必须保存的内容/不变量 |
| --- | --- |
| PrincipalKind | User 或 Role；原生无法完全判别时通过 metadata 表明限制，不以 locked 一刀切 |
| PrincipalScope | Cluster、Server 或 Database(name)；Oracle 绑定服务/container；不得由 active tab 临时推断 |
| PrincipalId | profile UUID + scope + driver-native identity；PG OID、SQL Server principal_id 与名称验证；MySQL/MariaDB user 与 host 独立字段 |
| PrincipalEntry | id、display_name、kind、原生分类/内置标志、必要判别说明 |
| PrincipalPage | entries、next cursor、完整性状态；每页唯一身份、稳定排序、cursor 绑定 scope |
| PrincipalDdl | SQL text、NativeCatalog/AdapterGenerated provenance、完整性说明；不把 permission error 当空成功 |
| PrincipalListRequest | connection identity/generation、刷新 epoch、request_id、scope、cursor、page limit |
| PrincipalDdlRequest | 上述 attribution + principal id + tab UUID/generation |

分页默认使用既有 catalog 页大小约定，SQL 侧 limit + lookahead；无法 server-side 分页的来源采用有界读取并明确 truncated 状态，不能无限 fetch_all 再静默截断。排序 key 至少为 kind/name/native identity，不能仅用非唯一 name。

`src/model/principal.rs` 保存列表/详情状态。建议局部定义 `PrincipalLoad<T, R>` 的 Empty / Loading(request, previous) / Ready(snapshot) / Failed(message, previous) / Cancelled(previous)，不借用包含 RelationRequest 的 RelationLoad，也不先全仓泛型化。

响应接纳规则：仅 pending request 与收到的 request 完全相同、connection 未 retired、owner 仍存在时更新；DDL 另外验证 tab generation。所有刷新先改变 epoch/request，再派发，旧成功和旧失败都不能覆盖。

## 单元 1：PostgreSQL 完整浏览闭环

**验收目标：** 连接真实 PG 后，group→用户/角色→DDL tab 全链路可用；测试不允许用静态示例 DDL 代替 driver 读取。

**Files**
- Create: `src/db/principal.rs`, `src/model/principal.rs`, `src/ui/principal.rs`。
- Modify: `src/db/mod.rs`, `src/db/postgres.rs`, `src/model/mod.rs`, `src/model/explorer.rs`, `src/model/workspace.rs`, `src/model/explorer_actions.rs`, `src/model/tab.rs`, `src/action.rs`, `src/app.rs`, `src/runtime.rs`。
- Modify: `src/ui/mod.rs`, `src/ui/icons.rs`, `src/ui/read_only_sql.rs`（仅必要共享接线）, `src/ui/relation.rs`, `src/input/keymap.rs`, `src/input/mouse.rs`, `src/help.rs`。
- 基础 exhaustive match 接线: `src/persistence/workspace.rs`, `src/model/navigation.rs` 及编译器指出的现有 match；本单元不能通过丢弃新 tab 绕过编译。
- Create tests: `tests/principal_contract.rs`, `tests/principal_tabs.rs`, `tests/principal_runtime.rs`。
- Extend tests: `tests/explorer_state.rs`, `tests/catalog_reducer.rs`, `tests/postgres_adapter.rs`, `tests/ui_render.rs`。

### 1.1 先写用户可观察的回归测试

每一条测试先确认在缺少功能时失败，再实施相关部分；避免只复制 getter 实现。

- `principal_group_is_last_direct_child`：建立两数据库和根分页状态的 profile，断言 group depth=profile.depth+1 且最后；展开数据库不改变 group 归属。
- `enter_principal_opens_single_read_only_tab`：注入含 login/no-login 的列表响应，对同一 leaf 连续 Enter，断言只有一个 tab、focus Results、只派发 DDL 请求而非 Preview/RelationChildren。
- `principal_ddl_rejects_retired_and_closed_tab_results`：发出 request 后 reconnect/close，再注入旧 success 与 failure，断言其他 tab 和 session 不受影响。
- `principal_view_has_no_relation_chrome`：TestBackend 内容及 hit regions 不含 DATA/DDL selector、RELATION DDL、data grid；driver icon 与准确标题存在。

Run: `cargo test --locked --test principal_contract --test principal_tabs --test principal_runtime --test explorer_state --test catalog_reducer`。
Expected：首次因未实现契约或断言失败；实现后全部通过，不以临时 ignore 通过。

### 1.2 数据库实现

1. 定义上述 typed identity/page/request；分别验证 profile、scope、page limit 和 cursor ownership。
2. `DatabaseConnection` 增加 list_principals / principal_ddl dispatch。PG 实现真读取；其他驱动此时仅提供明确临时未实现返回，记录在剩余单元中，最终不能留下 placeholder。
3. PG 列表查 `pg_roles`，用 rolcanlogin 分类；按 User/Role、名称 COLLATE "C"、OID 排序；不要走 `pg_authid` 或历史 `Database/__role__` 编码。
4. 详情以 OID 和名称重新验证对象，读取角色属性、shared comment 与 memberships；使用 pg_shdescription / shobj_description 对 shared object 的正确语义。
5. 拼接 quoted CREATE ROLE、LOGIN/NOLOGIN 及已支持属性、COMMENT、GRANT memberships；PG 版本相关字段从现有 server/version 能力判断。省略未知密码使用 SQL 注释，不生成 PASSWORD NULL。
6. SQL 查询使用绑定参数；DDL identifier/literal 使用驱动现有 quote。测试含双引号、单引号、Unicode 名称及不存在的 OID。

### 1.3 Explorer 实现

1. 新 group/leaf node 与 owner id；profile 存独立 PrincipalListState。不要改 ObjectGroup 的 schema-parent 不变量。
2. 在 `ExplorerTreeState::append_profile` 的 roots/state rows 之后追加 group；Redis 提前返回，SQLite 设置本地 unsupported 状态。
3. 覆盖 selected name、profile_id、node_exists、visible_parent、expand/collapse、projection、visible search、选择恢复、scroll 和 load-more。加载/失败/空状态沿用现有 Status row 交互。
4. `src/model/workspace.rs` 的兼容 VisibleCatalogNode projection 传递新图标类型与 expandability；普通列表和搜索渲染用同一映射。
5. Enter group 展开并只加载一次，Enter leaf 发打开请求；principal 节点创建/编辑/删除 action 返回 NotApplicable，不能误调用数据库 mutation。

### 1.4 Workspace 与 runtime 实现

1. 增加 `WorkspaceTab::PrincipalDdl` 与 TabKind；tab owns editor UUID、identity、generation、request sequence、snapshot state。
2. `src/action.rs` 同时定义列表/DDL action 与 command（仓库 Command 在这里，不在不存在的 src/command.rs）。
3. App 使用绑定 target 的 session 准备流程，再发请求；runtime 通过匹配 identity + target 的连接执行，不能盲用当前 active connection。
4. runtime 注册独立 task map，重复 request 不重复 spawn；完成/取消清理；关闭/reconnect/profile remove 取消对应任务。
5. 成功打开/更新 `editor.open_read_only` session；重复 Enter 激活旧 tab；失败保留前 snapshot 并显示 error；对象不存在显示 unavailable，不重建同名伪对象。
6. 完成所有 TabKind/WorkspaceTab 基础分支：id/title/profile、焦点、关闭、渲染、保存、quit；不得转成 SQL console 或丢入 relation transaction。

### 1.5 共享只读编辑器接线

1. 从 App 当前 DDL/readonly session accessor 提供 tab-bound session/profile/dialect；扩展 `active_read_only_session_id`、`mouse_session_focus`、snapshot 和 viewport 访问。
2. principal UI 用独立 `principal_ddl_layout`，无 selector，高度只扣边框和可选状态行；render/runtime 同一个 layout helper。
3. 调用 `ReadOnlySqlEditor`，不复制 editor_line_spans、Vim parser 或 scrollbar 逻辑。旧 relation renderer 布局保持。
4. keymap/mouse 中 relation DDL-only 条件扩展到新只读 tab；数据网格判断不扩展。检查 Vim prefix、search prompt、yank、Ctrl-w 与 Help，不被 relation view-toggle 抢键。
5. UI assertions 覆盖三种 IconMode + plain theme。校验窄窗无多余一行空白，mouse target 与 viewport 一致。

Run: `cargo test --locked --test principal_tabs --test principal_runtime --test ui_render --test relation_tabs --test relation_runtime`。
Run: `cargo test --locked --lib principal`，新增必要 input/editor 单测后用其实际模块过滤器执行。
Run（有隔离 PG URL）: `cargo test --locked --test postgres_adapter principal -- --nocapture --test-threads=1`。
Expected：PG fixture 包含一个 login role 与一个 no-login role，查询/DDL/普通可见权限路径均实际执行；缺 URL 记录 skipped，不算真实 PG 验收完成。

**Checkpoint:** group、真实 PG 数据和 DDL tab 可用，标明 adapter 集成证据。提交本单元实际相关文件，建议 message `feat(principals): browse PostgreSQL users and roles`；不要 git add -A 纳入他人文件。提交由 Luna 使用 @git-commit 完成。

## 单元 2：持久化、切连接与刷新闭环

**Files**
- Modify: `src/app.rs`, `src/model/principal.rs`, `src/model/tab.rs`, `src/model/navigation.rs`, `src/model/execution_target.rs`, `src/model/session.rs`（必要时）, `src/persistence/workspace.rs`, `src/runtime.rs`。
- Tests: `tests/principal_tabs.rs`, `tests/principal_runtime.rs`, `tests/workspace_persistence.rs`, `tests/workspace_tabs.rs`, `tests/global_workspace.rs`, `tests/connection_switch.rs`。

1. 写测试：两个 profile 同名 role 不合并；旧连接响应不能填入新 target；关闭后重开拥有新 session；profile rename 更新标题但不改变身份。
2. PersistedTab 添加 principal descriptor/UUID 与必要 viewport 字段；不保存 SQL text、pending tasks、connection generation。新 variant 保持旧 workspace fixtures 可读，按现有 version/migration 约定处理，而非无理由更改所有版本。
3. 更新 normalize、tab_id、snapshot/restore、active tab 与 profile ownership；恢复时重新校验并懒加载，缺 profile/对象用既有恢复失败模式明确处理。
4. 刷新列表后 reconcile selection：保留仍存在 principal；删除则回 group/相邻。刷新 DDL 保留旧 viewport，正文变短则 clamp。
5. PG 现有 role mutation 成功后 invalidate 对应 profile principal group/DDL；外部 console role DDL 通过既有 catalog change hook 能识别则接入，未识别时至少提供显式 refresh，不新增 SQL parser 大重构。
6. 同步 navigation back、tab close/reopen、quit；新 tab 没有 transaction、dirty SQL 和执行动作。

Run: `cargo test --locked --test principal_tabs --test principal_runtime --test workspace_persistence --test workspace_tabs --test global_workspace --test connection_switch`。
Expected：新旧 workspace round trip、切连接及 stale 响应隔离均通过。
**Checkpoint/commit:** `feat(principals): preserve preview lifecycle and workspace state`。

## 单元 3：MySQL 与 MariaDB 真实浏览闭环

**Files**
- Modify: `src/db/mysql.rs`, `src/db/principal.rs`, `src/db/mod.rs`。
- Tests: `tests/mysql_adapter.rs`, `tests/mariadb_principals.rs`, `tests/principal_contract.rs`。

1. 写同 user 不同 host、locked 普通用户、角色、无权限和 quoted account fixtures；保留现有 `mariadb_does_not_advertise_postgresql_role_or_schema_owner_mutations` 回归。
2. 在同一 MySqlAdapter 内依据 self.kind 分别 dispatch 原生 principal metadata；MariaDB roles_mapping/is_role 与 MySQL role_edges 不能混用。
3. MySQL 不以 account_locked 单独判 role。用 role_edges/可用版本的 role metadata 确认角色，无法区分未使用 role 的情况标明原生限制；不能为通过 UI 测试编造全部角色完整性。
4. 账号 identity 保存 user/host，display 用 quoted account；详情 lookup 与 quote 分开，不 split('@') 还原。list stable page key 含 host。
5. 用户定义优先 SHOW CREATE USER 加 SHOW GRANTS；角色按方言 CREATE ROLE 与直接 grants；保留必要原生选项/管理员授权语义，不把有效继承权限平铺。认证定义不进入日志/持久化，对省略部分诚实标注。
6. 权限失败若 fallback 当前用户，则 page=partial、有解释；完整 list error 与空列表区分。
7. 真实 MySQL8.4 与 MariaDB11.4 两次独立跑 fixture；created fixtures 唯一命名、测试收尾清理，不使用生产连接。

Run: `cargo test --locked --test principal_contract --test mariadb_principals`。
Run（配置隔离 URL）: `env LAZYDB_REQUIRE_DATABASE_TESTS=1 cargo test --locked --test mysql_adapter principal -- --nocapture --test-threads=1`。
Run（配置隔离 MariaDB URL）: `env LAZYDB_REQUIRE_DATABASE_TESTS=1 cargo test --locked --test mariadb_principals -- --nocapture --test-threads=1`。
集成测试须显式支持 REQUIRE_DATABASE_TESTS，缺 URL 时不能 quiet pass；不在命令日志打印含凭据 URL。
**Checkpoint/commit:** 两驱动均完成 group→native list→DDL，不以共用 adapter 编译通过替代 MariaDB 证据；`feat(principals): support MySQL and MariaDB accounts`。

## 单元 4：SQL Server 数据库用户/角色闭环

**Files**
- Modify: `src/db/mssql.rs`, `src/db/principal.rs`。
- Tests: `tests/sqlserver_adapter.rs`, `tests/principal_contract.rs`, `tests/principal_runtime.rs`。

1. 先写两个 database 同名 principal 的 scope test；SQL/Windows/external users 和 database role fixture 不把 server login 混入。
2. 通过 pool_for_database/既有 target 会话查 sys.database_principals，按 native type 分类；sys.database_role_members 和 sys.database_permissions 获取直接关系与授权。
3. 详情验证 database+principal_id+name；quote identifiers 用方括号 escaping；生成 CREATE USER 的原生 authentication 形式、CREATE ROLE owner、membership 和支持的显式 GRANT/DENY/REVOKE 语义。
4. 内置角色/用户显示系统定义注释，不生成会失败的假 CREATE。未知权限 class 标明未导出，不能丢失后声称完整。
5. 实测低权限 metadata visibility，以 partial 标识；SQL Server 只显示当前绑定数据库的 principal。

Run（配置隔离 SQL Server URL）: `env LAZYDB_REQUIRE_DATABASE_TESTS=1 cargo test --locked --test sqlserver_adapter principal -- --nocapture --test-threads=1`。
Run: `cargo test --locked --test principal_runtime --test principal_contract`。
**Checkpoint/commit:** `feat(principals): support SQL Server database principals`。

## 单元 5：Oracle、SQLite 和驱动边界闭环

**Files**
- Modify: `src/db/oracle.rs`, `src/db/principal.rs`, `src/db/mod.rs`, `src/model/explorer.rs`。
- Tests: 扩展实际 Oracle adapter 测试文件（先定位现有测试），`tests/sqlite_adapter.rs`, `tests/explorer_state.rs`, `tests/principal_contract.rs`；若没有适合 Oracle principal 测试则 Create `tests/oracle_principals.rs`。

1. Oracle 列表按当前 service/container 读取可见 users/roles；DBA views 权限失败时仅退到确实可读的目录，ALL_USERS 不等于完整 role list。
2. 详情优先 DBMS_METADATA.GET_DDL(USER/ROLE)，按现有 Oracle adapter blocking/runtime 与 CLOB 处理模式实现；可见直接授权补充相应 metadata。native metadata 权限失败只标详情失败，不能清空已有树。
3. 覆盖 ordinary account、role、系统对象、跨容器/服务限制与 metadata permission error；使用真实服务器验证 SQL，不在没有服务器时声明 Oracle 集成通过。
4. SQLite group 明确 unsupported，第一次展开立即进入本地状态、没有数据库 command；重复展开与刷新不生成错误重试风暴。
5. Redis 无 group；driver-oracle feature 禁用时保持已有 unsupported driver 行为。

Run: `cargo test --locked --test explorer_state --test sqlite_adapter --test principal_contract`。
Run: `cargo check --locked --no-default-features`（该单元首次验证 feature 边界）。
Oracle test 使用现有项目 URL/运行器约定；若新增上述 test target，Run: `cargo test --locked --all-features --test oracle_principals -- --nocapture --test-threads=1`，无服务器必须记录 skip/环境限制。
**Checkpoint/commit:** `feat(principals): support Oracle and unsupported database states`。

## 单元 6：交互回归、文档与发布前检查

**Files**
- Tests: `tests/ui_render.rs`, `tests/principal_tabs.rs`, `tests/principal_runtime.rs`, `tests/relation_tabs.rs`, `tests/relation_runtime.rs`；`src/input/keymap.rs`、`src/input/mouse.rs` 与 editor 已有单测模块。
- Docs: 在 `README.md` 和实际已有快捷键/数据库能力文档对应位置描述新分组、只读行为和 scope/partial 限制；不新建无关联文档站结构。

### 6.1 行为验收矩阵

- 三 icon modes × default/plain theme：group/user/role glyph 与颜色，tab 是驱动 icon；control chars sanitize，Unicode cell width，长标题溢出。
- 高亮：相同 SQL 在 relation DDL 与 principal DDL 的 token kind 一致；方言来自 tab profile。
- Vim：hjkl、gg/G、Ctrl-d/u、/、n/N、v/V、y；只读修改被拒绝；search prompt 优先接键；Ctrl-w 转 explorer；没有 data-view toggle/transaction 快捷键提示。
- 鼠标：点击聚焦、wheel、横滚、scrollbar track/thumb、拖选与复制；resize 和 status row 出现/消失后 geometry 正确。
- 生命周期：first load/refresh/cancel/error/retry，关 tab/reconnect/换数据库后晚到响应，profile rename/remove，restore 与旧 workspace。
- 旧 relation DATA/DDL、Redis preview、SQL output 仍工作。尤其共享只读 accessor 不把所有 Results 当 principal DDL。

Run: `cargo test --locked --test principal_tabs --test principal_runtime --test ui_render --test relation_tabs --test relation_runtime --test workspace_persistence`。
必要真实终端检查为补充：使用仓库已有 TUI 启动方式与隔离测试 profile 验证字体/拖动，不因缺 PTY 无限重试。

### 6.2 项目强制全量检查（一次收尾）

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

逐条记录 exit code；全量测试 exit 0 仍应列出缺数据库 URL 的 skipped adapter 测试。CI 其他 distribution/windows/macOS packaging job 若代码未涉及无需为本单元手工重复，合并时依赖项目正常 CI。

### 6.3 Luna 收尾审查与提交

1. 对照需求矩阵逐项检查最终 diff；重点查伪造 Relation、裸 active connection、吞权限错误、role locked 判别、persisted SQL、data toolbar/hit region 残留。
2. 对照新 main 相关 diff 做冲突/语义审查；只对实际冲突处理导致的相关代码变化补跑定向检查，必要时再跑受影响全量检查。
3. 最终 validation.md 写命令、退出、HEAD/工作区、环境、skip/PTY 限制与剩余风险。不得将计划中的 Expected 写成执行结果。
4. Luna 完成纠偏、提交与插件约定的合并；不切回 Astra 审查，不要求用户逐轮 resume。

## 完成条件

- 所有关系型驱动获得正确 group 行为；支持用户/角色的驱动真实读取，SQLite 正确解释 unsupported，Redis 不出现该分组。
- 可打开精准命名的独立只读 DDL tab，语法/Vim/滚动/鼠标与现有 DDL 共用实现；没有 DATA/DDL 子 tab 与 relation data 内容。
- 请求隔离、错误/partial、身份、持久化和原有 tab 回归通过；数据库/字体环境无法验证的部分明确列证据缺口并经 Luna 审查，不以 fake success 掩盖。
- 工作流/任务分支命名在计划完成后交由 Luna/插件；当前文档描述功能，不替代最终任务名称。
