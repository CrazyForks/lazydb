# Users / Roles 权限工作区 Implementation Plan

> 执行人：Luna。Astra 仅负责本分析和计划；实施、审查、纠偏、提交与合并均由 Luna 完成。不启动子 Agent。

**Goal:** 将用户/角色工作区升级为默认权限概览与只读 DDL 双视图，完成五种 SQL 引擎的常用授权、撤权、成员管理和主体操作，并在操作后展示数据库的实际状态。

**Architecture:** 保留 `PrincipalId`、现有 principal workspace variant、只读 SQL 编辑器和 Action → App → Command → Runtime → Adapter 架构。引入结构化主体快照、分区覆盖率、方言变更计划，概览及 DDL 使用同一加载批次的元数据；保留旧格式恢复兼容。按 PostgreSQL、MySQL、MariaDB、SQL Server、Oracle 的端到端闭环依次推进，能力只有在对应实现和验证齐备后才开放。

**Tech Stack:** Rust 2024 / Rust 1.94、Ratatui 0.30、Tokio、sqlx、Tiberius、可选 Oracle 驱动，沿用项目 secret、SQL quoting、编辑器和测试设施。

---

## 0. 执行上下文与边界

- 仓库：`/Users/yelog/workspace/tui/lazydb`；目标分支 `main`；起点和计划阶段 HEAD 均为 `dd9246da608700d84958e61479f82e44ea1a1b55`。
- 计划前复查实际 diff：tracked 文件与 index 没有差异。唯一未跟踪文件 `docs/plans/2026-09-20-console-lazy-connection-implementation.md` 是另一任务的文档，本任务不依赖、不复制、不提交。没有需要从原 workspace 带入新 worktree 的业务改动。
- 任务报告目录：`/Users/yelog/workspace/tui/lazydb/.git/opencode-tasks/ses_f42275555ffeqePWeNWyM8Hx2n`。本计划及进度、验证只写这里。业务实现由后续执行阶段进行。
- `checkpoint.json` 当前不存在；不得自行建立或更新插件维护的 checkpoint/state。本轮唯一回执为 `plan-84e12537-6270-49a0-8df8-24fe9d142caf.json`（token `84e12537-6270-49a0-8df8-24fe9d142caf`），在计划、修改范围及验证记录完成后最后写入，不改写历史回执。
- Luna 在计划交接后命名工作流及任务分支，并按当时阶段指令建立工作区。没有授权时不自动提交或合并；本文件给出的是逻辑提交边界，不要求每个微步骤提交。
- 初次续做先读 checkpoint（若出现）及实际 diff，对照下面验收表定位第一个未完成单元，不重读全部历史，也不采用历史日志中的等待 resume 要求。
- 截图附件不可见。产品设计以本计划为准；后续若收到截图，以明确差异调整，不阻塞已知需求。
- 代码定位使用符号及路径；本计划引用的行号只表示起点代码。Codegraph 未返回目标实现时再定向读取，不把模糊搜索结果当作已读源码。

### 0.1 已确认的代码修正

- `Command` 和 `Action` 均在 `src/action.rs`，不存在本任务需要新增的 `src/command.rs`。
- 语义命令在 `src/commands.rs`；overlay 在 `src/model/workspace.rs`。
- 现有公共描述明确为 DDL-only，须更新 `docs/database-capabilities.md:25-42`。
- `docs/ui-dialog-guidelines.md` 是弹层实现契约：内容旁放局部操作；底部稳定位置放 Cancel / Review SQL / Apply；再下一行放非按钮式快捷键帮助；禁用按钮不注册 hit region。

### 0.2 验收和验证的来源分级

| 类别 | 来源及内容 | 执行与限制 |
|---|---|---|
| 用户需求验收 | 默认权限等信息、双视图、当前 DDL、主体展示与授权操作；只在指定目录规划、保持业务/index 不变、交给 Luna 自动继续 | 直接需求必须满足；本阶段不实施业务，不向用户重问执行方式 |
| 项目既有强制门禁 | `.github/workflows/ci.yml` 的 fmt/clippy/all-targets all-features test；`docs/ui-dialog-guidelines.md` 的交互契约；`docs/database-capabilities.md` 的对外开放写入能力需 round-trip 证据 | 按项目要求完成；已有 CI 数据库服务与本机是否具备服务分开记录，不能用跳过代替通过 |
| 本任务实施验证设计 | U1–U7 的定向模型、SQL、reducer/runtime、TestBackend 和真库回归；REQUIRE gate 与新增 CI 测试接入 | 是为需求正确性设计的自动化验证，并非声称用户逐项指定了这些测试；不为每个微步骤重复全量。真库环境缺失保留未验证项，交 Luna 收尾决定补 CI/环境证据或保持相关能力未开放 |
| 补充建议验证 | 人工视觉体验、交互 PTY 演示、额外服务器版本矩阵、大规模性能观察 | 不作为新增强制合并门禁，不要求用户人工签字。环境受限最多一次针对性修复重试，记录限制后继续其他可执行工作 |

Oracle 真库验证是写入能力的集成证据需求，不是要求当前 macOS 必须安装一套 Oracle，也不是用户新增的人工门禁。实现、feature 编译、自动化 fixtures 可继续；没有真库证据时不得写“Oracle round-trip 通过”。高级版本兼容矩阵未列入首版范围，不能无限扩展验证任务。

### 0.3 修改范围声明

同目录 `change-scope.json` 是预计新增/修改的完整业务范围清单，使用精确仓库相对路径。没有预计删除或文件重命名；adapter 内部抽取函数只涉及原文件修改和新文件新增。`src/db/principal.rs`、`src/db/principal_drop.rs`、`src/model/tab.rs` 可按整合需要做最小兼容修正，已计入范围；帮助入口明确为 `src/help.rs`。原有未跟踪 Console 计划不是依赖，未纳入。任务目录内报告/回执不作为实施提交文件列入该业务清单。

若实现确需新增清单外文件，由 Luna 在实际修改前更新任务范围和理由；不得为了方便将范围扩大为整个 `src` 或 `tests`。

## 1. 冻结的用户行为

### 1.1 双视图

- 同一主体仍复用一个 `name@connection` workspace tab，默认 `OVERVIEW`，另一个 selector 为 `DDL`，没有关系表的 DATA 标签。
- 保留内部 `WorkspaceTab::PrincipalDdl` / `PrincipalDdlTab` 命名和持久化 variant，避免全仓重命名。本次将其语义扩为主体工作区，并更新误导性注释。
- Overview：两行以内身份/属性摘要；一行 `Permissions | Member of | Members` 分区；一个过滤输入；主体表格；底部可见性/状态与操作。User 的 Members 为不适用，Role 同时可有两个方向的关系。
- 默认 Permissions，显示目标、权限、来源、可转授标志；细节包含 grantor、native state、精确作用域、说明。过滤仅作用于已加载行，并提示覆盖率，不能伪装服务端全库检索。
- 未实现全量有效权限求值。直接 grants、PUBLIC、所有权/隐式规则、可确定的继承来源分开；未知继承范围明确 partial，不能把“没有直接 grant”写成“无访问权限”。
- PostgreSQL 权限页显式显示当前数据库并允许选择同连接可访问数据库；cluster 角色身份不因数据库变化而变，详情请求的 target 则必须变化。
- DDL 是当前已读取状态的定义及授权，编辑草稿仅出现在 Review SQL。不是历史原始建号语句，也不是密码恢复工具。不可重建部分用说明标明，不能合成会扩大权限的语句。

### 1.2 输入

- `Ctrl-o` 切换 Overview/DDL，先核对既有关系页 selector 和用户配置映射，统一为同一切换动作；若配置覆盖，帮助显示实际按键。
- Overview：`j/k`/方向键移行，`/` 过滤，`Enter` 查看行详情，`r` 刷新。分区可通过 selector 焦点/方向键及鼠标切换。
- Grant/Add member、Revoke/Remove member、Edit principal、Drop 使用可聚焦按钮及已有语义动作入口，首版不额外抢占全局字母快捷键。
- DDL 保留现有只读 Vim、复制、选择和滚动；切换 selector 的按键在 SQL 编辑器分派之前处理。
- 隐藏编辑器不接收 Overview 的键盘/滚轮/拖选，不参与 viewport 同步。弹层打开时由顶层弹层独占输入。
- 新打开和恢复的 tab 均默认 Overview；同一运行期重开已存在 tab 保留当前 view/filter/selection。不新增视图状态持久化需求。

### 1.3 操作范围（必须完成）

| 引擎 | 普通主体 CRUD | 权限 | 成员 |
|---|---|---|---|
| PostgreSQL | 复用现有 role/login role 新建、属性编辑、rename、drop | database/schema/table/view/column/sequence/function/procedure 的直接 GRANT/REVOKE、grant option | 双向展示，加入/移除，admin；版本允许时 inherit/set |
| MySQL 8+ | user/role 创建；user rename/host/password/lock；drop；没有原生 role rename 时不伪造 | global/database/table/column/routine，grant option；未知动态权限原文保留、能力门控 | grant/revoke role，default roles，admin option（原生支持时） |
| MariaDB | user/role 创建；user rename/password；drop；账户锁定依服务器能力 | 同类普通授权，独立语法与 privilege 类型 | role grant/revoke、default role、admin option；遵循 MariaDB 自身限制 |
| SQL Server | database role；普通 FOR LOGIN 或 WITHOUT LOGIN user；名称/default schema/role owner 编辑；drop | database/schema/object/column 的 GRANT/REVOKE/DENY、grant option | ALTER ROLE ADD/DROP MEMBER；角色也能加入角色 |
| Oracle | 普通 user/role 创建；user password/lock/default tablespace/profile、role rename 不开放；drop | 系统及对象/列的 GRANT/REVOKE、适用的 admin/grant option | role grants、撤销、admin/default role（依适用主体） |

高级认证提供者、SQL Server server login 管理、Oracle 特殊认证/所有资源配额、全量有效权限模拟及 Redis ACL 不纳入首版表单；其已返回元数据必须可读，不得静默丢弃。PG default privileges 作为独立来源可展示，首版不编辑默认 ACL；显示不可编辑原因，不混入现有对象授权。

## 2. 共享实现设计

### 2.1 领域与加载

**新增 `src/db/principal_details.rs`，导出于 `src/db/mod.rs`。**

- `PrincipalReadTarget` 包含 PrincipalId 和具体 database（若适用）；不能仅拿 profile 当前默认数据库。
- `MetadataCoverage`：Complete / Partial(reason) / Unavailable(reason) / Unsupported(reason)。`MetadataSection<T>` 持有数据及 coverage。Complete + empty 才表示已证实为空。
- `PrincipalDetails` 持有 authoritative entry、target、非秘密属性、permissions、member_of、members，以及生成的 `PrincipalDdl` 或 DDL 分区错误。可恢复的单分区错误收集到 section，不用一个 `?` 丢掉其他成功结果。
- `PrincipalPermission` 保留 target 原生类别/id、database/schema/object/columns/routine signature、grantor、grantee、权限名、native state、grant option、来源。行 identity 必须包含 grantor/target/options，不能按名称去重丢语义。
- `PrincipalMembership` 保存两个主体身份、admin/inherit/set/default options 及不可见值状态。
- 引擎特有属性用明确类型或键值展示项；不要用可展示的字符串反推执行标识符。
- 原始 SQL 只保留已按引擎脱敏的部分；MySQL/MariaDB 不以共享正则盲拆任意 SQL。

**扩展 `src/model/principal.rs`：**

- `PrincipalView` 默认 Overview；`PrincipalSection` 默认 Permissions；分区各自 filter/selected identity/scroll。
- `PrincipalDetailsRequest` 包含 tab_id/tab_generation/request_id/ConnectionIdentity/PrincipalReadTarget。
- 详情 load 状态沿用 Empty/Loading(previous)/Ready/Failed(previous)/Cancelled(previous) 风格，成功和失败均匹配完整 pending request。
- 选择共享详情任务作为最终唯一主动刷新入口：同一读取批次生成 Overview 数据和 DDL。保留 `DatabaseConnection::principal_ddl` API 兼容现有调用；各 adapter 抽取共同元数据读取/格式化，避免两条实现长期分叉。迁移期其他引擎可沿旧 DDL loader 展示并明确 Overview 未完成，最终去掉这条 UI fallback。
- `generation` 在刷新替换、目标切换、变更完成、重连/失效时推进并取消旧读取；request_id 用 checked_add，禁止溢出复用。
- 不声称所有分区有事务一致性；记录批次及读取时间。DDL 和 Overview 必须至少属于同一批次，不能混用不同版本。

### 2.2 变更

**新增 `src/db/principal_mutation.rs`、`src/model/principal_editor.rs`、`src/ui/principal_editor.rs`。**

- typed operation：GrantPrivilege、RevokePrivilege、DenyPrivilege、GrantMembership、RevokeMembership、SetDefaultRoles；主体 CRUD 的 PG 路径复用已有 CatalogEditor，其余引擎采用专用 enum draft，通过共同 principal 操作服务执行。
- plan 请求绑定 principal identity、connection generation、目标 database、baseline revision 和唯一 request_id。表单构建在 active principal tab 或显式 explorer principal anchor 上，不再读取之后可能移动的 explorer selection。
- 服务器 capability + operation target + 当前上下文共同判断支持性。选项仅出现在支持它的操作里；未知 privilege 名不可直接作为 SQL token。
- Review SQL 使用脱敏显示 SQL；执行使用独立私有 execution payload/Secret 字段。不得调用显示为 `'<REDACTED>'` 的字符串作为真正执行密码。已有 PG 路径也要通过真实执行路径回归这一点。
- 预览后基线变化应重读并重新生成计划；单项撤权精确绑定原授权主体/来源，不将 inherited grant 当作当前主体 direct grant。
- Runtime 再次检查只读 profile、identity、database scope、请求是否仍有效；重复 Apply 在 busy 状态不发第二次命令。
- 批量多语句按步骤记录 Applied / Failed / NotRun / OutcomeUnknown；不假设 DDL 可事务回滚，复用项目现有 mutation progress 类型或等价语义。
- mutation 无论成功、部分成功或结果不确定都使当前详情及 DDL 失效并触发回读；失败若明确没有执行则保留标记了时间的旧快照。取消 UI 不保证取消服务端写入。

### 2.3 模块组织

各引擎逻辑随体量拆入 `src/db/postgres/principal.rs`、`src/db/mysql/principal.rs`、`src/db/mssql/principal.rs`、`src/db/oracle/principal.rs`，父文件声明私有子模块并由同一 adapter impl 暴露方法。新增路径前核对现有模块声明，不机械搬动无关函数。MariaDB 共享 mysql adapter，但在 `src/db/mysql/principal.rs` 中使用明确 kind 分支/独立生成函数及独立 fixtures，禁止用名称相似代替语法验证。

## 3. 单元 U1：PostgreSQL 权限浏览与双视图

**文件**
- 新增：`src/db/principal_details.rs`、`src/db/postgres/principal.rs`、`tests/principal_details.rs`、`tests/postgres_principal_details.rs`。
- 修改：`src/db/mod.rs`、`src/db/postgres.rs`、`src/model/principal.rs`、`src/action.rs`、`src/app.rs`、`src/runtime.rs`、`src/ui/principal.rs`、`src/ui/mod.rs`、`src/input/keymap.rs`、`src/input/mouse.rs`。
- 回归：`tests/principal_tabs.rs`、`tests/principal_contract.rs`。

**步骤**
1. 更新 tab 测试：首次打开显示 Overview selector；同 identity 复用；切 DDL 显示 SQL；不出现 DATA / RELATION DDL。旧测试无 selector 断言明确移除，其余有效断言保留。
2. 新增契约测试：Partial 空集合不等同于“无授权”；role 同时有 member_of 和 members；同 oid 在不同 database 的请求不互相接受；旧 generation 的成功/失败均不覆盖。
3. 运行下述定向测试一次，确认失败来自新契约/尚缺接口而非环境。实现详情领域模型和 reducer，不为每个 getter 写镜像测试。
4. PG loader 校验 profile/oid，读取 pg_roles、双向 pg_auth_members，按服务器版本读取 membership 列；按指定数据库执行 ACL 读取。
5. 使用 pg_database / pg_namespace / pg_class / pg_attribute / pg_proc / pg_default_acl 的 ACL 元数据；显式展开 direct ACL 和默认 ACL，保存 PUBLIC（grantee oid=0）、grantor、grantable、对象类型和函数身份参数。NULL ACL 的默认行为与显式 empty ACL 不混淆。
6. 查询必须限制于当前 target scope，分对象类型分区加载；大结果有界加载/分页，未取完则 Partial。翻页请求同样绑定 generation，不能 UI 过滤后误判已全部读取。
7. 从同批数据生成 PG DDL：真实 role 属性、正确 quoted VALID UNTIL、membership 及其 options、当前范围 direct grants；default ACL 单独说明/语句，不能改写成当前对象授权。
8. 接通 LoadPrincipalDetails/Loaded/Failed/Cancel 的 Action/Command/Runtime；第一次打开加载，重复打开已加载 tab 不多发任务。刷新、新 database、重连产生新请求。
9. 重构 `src/ui/principal.rs` 布局，复用 ReadOnlySqlEditor；selector、摘要、表格及状态共用一次几何计算。权限行详情可复用现有 text detail 外壳。
10. 键盘、鼠标、viewport 同步根据 view 分支；帮助和 header badge 由当前 view 决定，不再对所有 PrincipalDdl variant 固定显示 DDL。
11. 建立 PG 真库 fixture：随机后缀 user、parent role、child role、schema/table/column/sequence/function；授予不同选项，读取并比较结构化详情和 DDL；在退出路径清理。
12. 运行定向回归，写入 validation 的实际命令、版本、退出和是否进入 DB 测试体。

**验证命令**
```sh
cargo +1.94.0 test --locked --test principal_details --test principal_tabs --test principal_contract
LAZYDB_REQUIRE_DATABASE_TESTS=1 cargo +1.94.0 test --locked --test postgres_principal_details -- --nocapture --test-threads=1
```

第二条要求已经设置 `LAZYDB_TEST_POSTGRES_URL`，新测试必须在 REQUIRE=1 且 URL 缺失/连接失败时 fail，不能 return。预期 UI/契约全部通过；真库实际断言 ACL、双向成员和数据库隔离。

**验收**：从 explorer 打开 PG user 和 role，默认看到真实权限，切页能看对应 DDL；部分读取、失败/重试、目标切换、旧响应均有证据。完成后接 U2，不以共享结构完成代替业务闭环。

## 4. 单元 U2：PostgreSQL 授权、撤权、成员及主体管理

**文件**
- 新增：`src/db/principal_mutation.rs`、`src/model/principal_editor.rs`、`src/ui/principal_editor.rs`、`tests/principal_mutation.rs`、`tests/principal_editor.rs`。
- 修改：`src/db/postgres/principal.rs`、`src/db/mod.rs`、`src/model/mod.rs`、`src/model/workspace.rs`、`src/action.rs`、`src/app.rs`、`src/runtime.rs`、`src/commands.rs`、`src/ui/mod.rs`、`src/ui/principal.rs`、`src/input/keymap.rs`、`src/input/mouse.rs`。
- 必要复用修正：`src/model/catalog_editor.rs`、`src/db/catalog_mutation.rs`、`src/db/postgres.rs`。
- 测试：`tests/postgres_principal_mutations.rs`、`tests/principal_drop.rs`。

**步骤**
1. 建立 SQL plan 回归：特殊名称 quoting、column-only grant、routine overload、admin/grant option、revoke option only 与全撤权不同、非法 privilege/target 组合被拒绝。
2. 建立 reducer/runtime 测试：表单打开后 explorer 移动仍操作原主体；read_only 在 runtime 拒绝；预览后 scope/generation 变化拒绝；重复 Apply 只执行一次。
3. 实现 typed operations 和 capabilities；权限选择随 target type 更新，grant option/admin option 不共用一个无语义 bool。
4. 表单实现 target、privileges、options；成员表单使用明确方向（给当前主体加入角色 / 给当前角色加成员）。按项目 dialog contract 排布 Cancel / Review SQL 及帮助。
5. SQL plan 生成 PG 精确目标，membership options 版本门控，默认 RESTRICT；不自动加 CASCADE。
6. 连接现有 PG role 属性编辑和创建/删除入口。active tab 的 Edit 从 tab entry 构建 anchor，不能临时改 explorer 选择来复用旧函数。
7. Runtime 执行 typed plan，成功或部分失败刷新关联 principal list、当前 tab 和可确定受影响的成员/角色 tab；避免全 workspace 重载。rename 以 oid 重绑定 title。
8. 修正现有真库测试提前 return 的验证歧义，增加 REQUIRE gate；密码路径测试验证实际请求使用真实 secret，日志和预览仍脱敏，不能只检查 SQL 字符串。
9. 真库 round-trip：grant SELECT/column UPDATE → 回读 → DDL 精确一致 → revoke → 回读移除；成员 admin 加/改/移除；role create/edit/rename/drop；拒绝过期主体 identity；执行失败保留可修正草稿。
10. 定向验证并记录；逻辑提交边界为 U2 完整闭环，不提交半通的 UI 或空 capabilities。

```sh
cargo +1.94.0 test --locked --test principal_mutation --test principal_editor --test principal_drop --test principal_tabs
LAZYDB_REQUIRE_DATABASE_TESTS=1 cargo +1.94.0 test --locked --test postgres_principal_mutations -- --nocapture --test-threads=1
```

**验收**：界面内完成授权/撤权/成员/已有主体 CRUD，无需手工把 SQL 复制到 Console；操作后 Overview 和 DDL 显示回读状态；失败不谎报成功。

## 5. 单元 U3：MySQL 完整闭环

**文件**
- 新增：`src/db/mysql/principal.rs`、`tests/mysql_principal_details.rs`、`tests/mysql_principal_mutations.rs`。
- 修改：`src/db/mysql.rs`、`src/db/mod.rs`、`src/db/principal_details.rs`、`src/db/principal_mutation.rs`、`src/model/principal_editor.rs`、`src/ui/principal_editor.rs`；按需要接入 `src/app.rs` 主体操作入口。
- 回归：`tests/mysql_adapter.rs`、共享 principal tests。

**步骤**
1. fixtures 覆盖 user@host 含特殊字符、未授予过的 role、global/db/table/column/routine grant、grant option、role edges/default roles、USAGE、未知 dynamic privilege、低权限 SHOW GRANTS。
2. 抽取原 principal_grants 和认证 SQL 清理逻辑；读取普通属性失败不阻止已有授权显示。解码失败报告 coverage，不再静默 if let Ok 丢行。
3. 权限表/SHOW GRANTS 的方言解析保持引号边界，精确区分逗号权限列表、列列表、账户列表、role grant。无法解析的行脱敏后保留为 native detail 并禁止编辑该行。
4. MySQL 没有独立 role 标志：不要把无法确定分类伪称为可靠 Role；保留 native-kind 信息，创建角色成功后不能仅靠临时客户端记忆长期判断类型。
5. 实现 §1.3 MySQL 主体字段和 GRANT/REVOKE/role/default role SQL；version/capability 检查，不把全局动态权限放入通用 token 输入。
6. 实现 host/rename 后新身份重绑定；密码只在执行 payload 中，变更后不读取密码/哈希用于回填。
7. DDL 展示当前可见属性、授权和角色语句；不为了完整 DDL 无约束读取 SHOW CREATE USER 的秘密字段。
8. 真库完成 user/role create → details → 修改普通属性 → details → grant/revoke → details/DDL → drop，并测试只读/错误和准确 host。
9. 能力仅在上述 round-trip 实现通过后开放；运行测试并记录真实 MySQL 版本。

```sh
cargo +1.94.0 test --locked --lib principal
LAZYDB_REQUIRE_DATABASE_TESTS=1 cargo +1.94.0 test --locked --test mysql_principal_details --test mysql_principal_mutations -- --nocapture --test-threads=1
```

**验收**：MySQL 8.4 上 user 与 role 的两视图和常用操作闭环；低权限可见部分正确显示，不因 mysql.user 不可读变成整页失败。

## 6. 单元 U4：MariaDB 独立闭环

**文件**
- 修改：`src/db/mysql/principal.rs`、`src/db/mysql.rs`、共享 principal capabilities/draft，以及 `tests/mariadb_principals.rs`。
- 新增：`tests/mariadb_principal_mutations.rs`。

**步骤**
1. 为 is_role、MariaDB role identity、mysql.roles_mapping、default role、SHOW GRANTS 认证语句及授权人建独立 fixtures；不通过设置 MySQL test URL 冒充全部 MariaDB 语义。
2. 使用现有 authoritative is_role 分类，按 MariaDB 原生形式构造 role 名称；账户仍准确保存 host。
3. 按服务端版本提供 default role 和 lock 能力，禁止发送 MySQL-only 语法；role admin/grant options 使用 MariaDB 语义。
4. 复用共享表单外壳，但选项、SQL 和 coverage 来自 MariaDB capability，不从 MySQL “支持角色”布尔值继承全部能力。
5. 真库完成 user/role 创建、属性修改、grant/revoke、成员/default role、DDL 回读和删除；包含 quoted name 和只读失败。
6. 修复并验证认证信息清理，不让 `IDENTIFIED` 出现在引号名称里时错误截断整条 grant。

```sh
LAZYDB_REQUIRE_DATABASE_TESTS=1 cargo +1.94.0 test --locked --test mariadb_principals --test mariadb_principal_mutations -- --nocapture --test-threads=1
```

使用 `LAZYDB_TEST_MARIADB_URL`；测试体明确检查该变量。**验收**：MariaDB 11.4 的原生角色和用户操作可用；不存在 MySQL 语法兼容假设。

## 7. 单元 U5：SQL Server 正确权限与管理

**文件**
- 新增：`src/db/mssql/principal.rs`、`tests/sqlserver_principal_details.rs`、`tests/sqlserver_principal_mutations.rs`。
- 修改：`src/db/mssql.rs`、共享 principal details/mutation/editor、`src/db/mod.rs`；回归 `tests/sqlserver_adapter.rs`。

**步骤**
1. 先建当前缺陷回归：OBJECT_OR_COLUMN 经 ob.schema_id 取得 schema、列权限包含真实列名、grantee bracket quoting、user/login 名不同、role owner 非 dbo、角色作为其他角色成员。
2. 原读取按 sys.database_principals 与 sys.database_permissions 的 class/major_id/minor_id 建真实目标；join sys.columns 取得列名；保留 GRANT、DENY、GRANT_WITH_GRANT_OPTION 及 REVOKE 列例外。
3. 对未支持 class 保留明细并标能力限制，不 filter_map 静默丢弃。固定角色的隐式权限标系统定义，不假装空权限。
4. 读取实际 owner、authentication type、default schema；login mapping 仅在可见时展示/生成。如果无法读取 mapping，不能假设同名 login。
5. 精确生成权限 DDL，列级表达必须有列标识，不得降级为对象级 GRANT 加注释。内建主体不生成虚假的可重建 CREATE。
6. 实现 database role 与 FOR LOGIN/WITHOUT LOGIN user 的表单、create/edit/drop，以及 database/schema/object/column GRANT/REVOKE/DENY；作用域固定来自 tab database。
7. 实现双向成员变更 ALTER ROLE ADD/DROP MEMBER。SQL Server user/role 同名/重建时用 principal_id 及可用身份属性重验。
8. 真库测试 database scope 切换不串库；列授权不授予整表；DENY 与 column exception 读写；普通 user/role CRUD 及回读。

```sh
LAZYDB_REQUIRE_DATABASE_TESTS=1 cargo +1.94.0 test --locked --test sqlserver_principal_details --test sqlserver_principal_mutations -- --nocapture --test-threads=1
```

使用 `LAZYDB_TEST_SQLSERVER_URL`。**验收**：2022 测试环境的权限语义和 DDL 精确一致，无对象遗漏或列权限扩大；不创建 server logins。

## 8. 单元 U6：Oracle 可见性、授权及主体操作

**文件**
- 新增：`src/db/oracle/principal.rs`、`tests/oracle_principal_details.rs`、`tests/oracle_principal_mutations.rs`。
- 修改：`src/db/oracle.rs`、共享 principal details/mutation/editor、`src/db/mod.rs`；回归 `tests/oracle_adapter.rs`。

**步骤**
1. fixtures 覆盖 DBA/ALL/USER 视图可见范围、字典权限错误与网络/SQL 错误区别、user/role native identity、系统/对象/列权限、admin/default role 和不完整 owner 信息。
2. 依据实际字典列定义写分级读取：DBA_ROLE_PRIVS/DBA_SYS_PRIVS/DBA_TAB_PRIVS/DBA_COL_PRIVS，以及相应可见视图回退。回退只处理确认的权限/可见性错误，不能对任意 Err 降级。
3. USER_* 只能代表当前用户可见范围时，选中其他主体显示 Unavailable/Partial，不执行假设所有视图都有 GRANTEE 的通用 SQL。
4. 使用 spawn_blocking + 现有 connection lock；一次任务收集多个分区，错误分类后继续可独立的分区，不持锁等待 UI。
5. 读取非秘密属性并生成真实的可见 DDL；缺认证的 CREATE USER 不作为可重放 SQL，使用注释解释缺失。系统/对象/列权限精确展示和生成。
6. 完成普通 user/role create、user 属性/password/lock 修改、drop、系统/对象授权及成员操作；保留 Oracle 用户同时是 schema 的含义。
7. DROP USER 默认不附 CASCADE；依赖对象错误正常反馈，不能为“让测试通过”默认删除所有 schema 内容。
8. feature disabled 路径编译并明确 unavailable；真库 REQUIRE 模式缺客户端/URL/权限应失败且记录原因，不能把跳过计为通过。

```sh
cargo +1.94.0 test --locked --no-default-features --test principal_details --test principal_mutation
LAZYDB_REQUIRE_DATABASE_TESTS=1 cargo +1.94.0 test --locked --all-features --test oracle_principal_details --test oracle_principal_mutations -- --nocapture --test-threads=1
```

Oracle 测试 URL/凭据配置沿现有 `tests/oracle_adapter.rs` 的变量约定实现，禁止把秘密写入命令记录。**验收**：driver 两种构建路径有证据；普通 user/role 完整读写；低权限读取不虚报完整。无真库环境时保留明确缺失证据，不能把该行标为真库已验收。

## 9. 单元 U7：跨引擎恢复、文档与最终验证

**文件**
- 修改：`src/persistence/workspace.rs`、`src/app.rs`（仅必要兼容/恢复）、`src/commands.rs`、`src/help.rs`、`docs/database-capabilities.md`、`docs/keybindings.md`、`.github/workflows/ci.yml`。
- 测试：`tests/workspace_persistence.rs`、`tests/workspace_tabs.rs`、`tests/global_workspace.rs`、`tests/principal_tabs.rs`、`tests/principal_editor.rs`；新增 `tests/principal_runtime.rs` 以隔离核心异步/目标测试。

**步骤**
1. 旧 `PersistedTab::PrincipalDdl` JSON fixture 恢复后默认 Overview，重连后加载正确主体；旧配置无需迁移。数据库权限快照、表单密码和未应用 secret 不落盘。
2. 固化 lifecycle：tab close、profile 删除、重连、修改目标、rename、主体被外部 drop/recreate、两次刷新竞态、读取取消、写入结果未知；所有动作目标来源可追踪。
3. 对 TestBackend 120×40、80×24 和极小区域验证 selector/行/按钮/状态；Unicode cell width、plain color 焦点标记、禁用无 hit region、弹层 footer 不因焦点跳动。
4. 验证读取旧快照 offline/stale 标签，重试仅针对相关任务，不因失联不断自动循环查询。
5. 移除迁移期间的 DDL-only UI fallback；所有已支持 principal 引擎统一走详情快照，SQLite 保留明确不支持提示、Redis 保持原行为。
6. 更新数据库能力表：逐项区分 implemented/unsupported/version gated/metadata restricted，删除 DDL-only 说明；更新键盘帮助及实际操作文档。
7. CI 数据库 job 接入新测试文件：PG/MySQL/MariaDB/SQL Server 在已有 URL 下用 REQUIRE=1；PG principal mutation 不再无数据库地“成功”。Oracle 不伪称现有 CI service，记录可运行命令与所需客户端环境。
8. 每个变动单元已经定向通过后，仅此时运行全量 fmt/clippy/test。格式检查失败先修本任务引入的问题，区分基线遗留，不无条件格式化无关文件。
9. Luna 审查 diff 与能力表、数据库权限 SQL、secret 日志、部分失败语义，修复审查发现，按改动范围重跑相关验证。只有最后变更影响全局才重复全量。
10. 输出简短进度/验证摘要，按届时用户授权提交合并；不主动修改 state/checkpoint，不重用历史回执 token。

**最终命令**
```sh
cargo +1.94.0 test --locked --test workspace_persistence --test workspace_tabs --test global_workspace --test principal_tabs --test principal_editor --test principal_runtime
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

项目 CI 另有发行/平台检查，按涉及文件及 CI 运行结果判断；本任务不修改安装器，不能为了权限页要求本机运行 Windows 安装器验证。真实 TUI/PTY 外观检查为补充，TestBackend/输入命中测试为必需；环境受限至多一次有针对性的修复重试，再记录限制交收尾审查，不无限 progress。

## 10. 可验收清单与交接

### 10.1 单元完成前的定向复核（由 Luna 执行）

| 单元 | 必查实现点 | 证据与通过条件 |
|---|---|---|
| U1 | 来源/覆盖率不混淆；数据库切换绑定请求；隐藏 DDL 编辑器不接收输入；DDL 同批数据 | 定向契约/界面测试通过，PG 真库 ACL 与回读结果一致；无证据项如实列出 |
| U2 | typed SQL 与 secret payload 分开；列/routine 目标精确；表单目标不随 explorer 漂移；失败后回读 | SQL/reducer/runtime 回归通过，grant→read→revoke→read 和主体操作 round-trip |
| U3 | user/host 不丢失；SHOW GRANTS 未解析行不丢弃；role 分类不伪造；MySQL 版本能力 | 原文保留/脱敏测试与 MySQL 真库操作证据；错误 UI 能恢复 |
| U4 | MariaDB role 身份、default role/admin 语法独立；认证清理不破坏 quoted names | MariaDB 自身 fixture 和真库证据，不借用 MySQL 通过结果 |
| U5 | schema join、列名、grantee quoting、真实 login/owner；DENY/列例外 | SQL Server 列级测试证明未扩大成整表权限；实际读写和 DDL 对应 |
| U6 | 字典回退仅对权限错误；选中其他用户不冒用 USER_* 完整性；同步驱动正确线程 | feature 开/关证据、字典 fixture、可用真库证据；缺失环境明确记录 |
| U7 | 旧工作区恢复、tab 生命周期、文档能力表与实现一致、CI 测试实际进入测试体 | 最终自动化门禁结果、diff 审查、未验证项清单；无新引入人工阻塞 |

发现普通实现或测试错误直接纠正，按相关代码范围复测；只有缺少外部输入/权限或互斥需求需用户选择才标 blocked。没有真库环境时先完成可验证部分和其他单元，不反复重试同一环境。每单元通过后写简短检查点摘要并继续下一单元。

### 10.2 验收状态

| 单元 | 完成定义 | 当前状态 |
|---|---|---|
| U1 | PG 权限、属性、成员双向 + 双视图 + 正确作用域/请求隔离 | 未实施 |
| U2 | PG 授权撤权/成员/主体 CRUD + 回读/失败/只读闭环 | 未实施 |
| U3 | MySQL 独立完整闭环及真实测试 | 未实施 |
| U4 | MariaDB 原生语义完整闭环及真实测试 | 未实施 |
| U5 | SQL Server 精确列权限/DDL + database principal 管理 | 未实施 |
| U6 | Oracle 分级可见性、普通主体及授权管理，驱动验证 | 未实施 |
| U7 | 恢复/交互/文档/CI/全量验证与 Luna 审查 | 未实施 |

**下一具体动作：** Luna 命名工作流/任务分支并准备执行工作区后，从 U1 的 `tests/principal_tabs.rs` 行为回归及 `PrincipalDetails` 契约开始，完成 PG 用户打开→权限概览→DDL→刷新这个闭环，再继续 U2。普通实现取舍由执行者按本计划自行推进；实现尚未结束不是阻塞，也不需要用户反复 resume。

**验证记录规则：** 每次写 `validation.md` 包含命令、退出结果、代码 commit/diff 状态、相关文件、数据库版本/测试体是否运行、环境限制。本文所有命令都是待执行计划，不代表已经通过。计划阶段仅完成静态核对，未修改业务代码、执行测试或建立 Git 分支。
