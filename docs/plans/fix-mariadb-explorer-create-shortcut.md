# MariaDB Explorer Create Implementation Plan

> **执行者：Luna。** 按下述可验收单元顺序实施、审查、提交和合并；当前 Astra 仅负责分析/计划。用户已指定执行分工，不启动子 Agent。

**Goal:** MariaDB Explorer 的 `a` 在数据库/schema、Tables、Views 等对应位置打开正确创建入口，完成预览、执行、刷新与定位；不可用操作有明确反馈。

**Architecture:** 复用 catalog mutation 的能力声明、编辑器、SQL plan、显式目标连接和目录刷新。统一 MySQL/MariaDB 的 DatabaseIsSchema 上下文，保留原始 anchor，所有创建入口遵守同一验证契约。MariaDB schema 与 database 同义，不增加 PostgreSQL 式嵌套 schema。

**Tech Stack:** Rust 1.94、Ratatui/Crossterm、SQLx MySQL adapter、现有 reducer/runtime、MariaDB 11.4 CI service。

---

## 基线与边界

- 用户起点 d7f85bc544ef2c8e4d5a72db93b820821640ea57；调查 HEAD 为 08c95d767c8dd608b9e7f5f0712946d3bb9eccb0。原工作区 main 包含其他任务成果，不能 reset 到旧起点。
- 实施前核对实际 HEAD/diff，按插件安排创建任务分支/工作树，任务命名由 Luna 完成。保留现有未跟踪计划文件与任务目录。
- 分析：`.git/opencode-tasks/ses_f51dac38affeaTSAkymgm2z93R/analysis.md`。本计划不改业务代码。
- 核心支持 Database、Table、View；其他未实现对象保留明确“不支持”反馈。表叶节点的 a 仍表示新增列/约束，不改为新增同级表。
- 非活动 profile 提示先激活，不拓展后台跨 profile 写入。同一活动 profile 的其他合法数据库采用显式 execution target，不修改现有控制台目标。

## 验证要求的来源与级别

- **用户需求验收：** MariaDB Explorer 正确位置按 a 可新建数据库/schema（同义）、表和视图；创建过程可操作、错误可见、结果刷新。用户没有强制指定人工、PTY 或截图验收。
- **项目既有门禁：** `.github/workflows/ci.yml` 的 Rust 1.94 fmt、all-targets/all-features Clippy 与测试；数据库 CI 使用 MariaDB 11.4，并以 `LAZYDB_REQUIRE_DATABASE_TESTS=1` 执行 `mariadb_catalog_mutation` 等既有集成测试目标。新增回归用例放入这些既有测试目标。
- **本计划选定的自动化证据：** keymap/reducer/state/plan 契约回归，以及生成 plan → execute → catalog 验证的新建对象用例。这些是实现验证手段，不宣称用户另行指定了这些命令。
- **补充建议验证：** 人工/PTY 演示、截图及本地额外 MySQL fixture 验证。它们不是新增必需门禁，不得以缺少 PTY 或人工操作无限保持 progress。
- **环境限制处理：** 对人工/PTY 等环境受限检查最多一次有针对性的修复重试，再记录限制并由 Luna 收尾审查决定补充证据。若本地真实数据库环境不可用，明确记录未执行/跳过及 CI 待验证项；不得把跳过当通过，也不要求用户代替执行剩余功能。项目 CI 门禁继续按既有规则判定。

## 单元 1：Table/View 从每种正确位置创建成功

**修改：** `src/db/catalog_mutation.rs`、`src/db/mysql.rs`、`src/model/catalog_editor.rs`、`src/app.rs`、`src/input/keymap.rs`、`src/help.rs`。
**测试：** `tests/object_mutation_contract.rs`、`tests/catalog_editor_state.rs`、`tests/catalog_editor_reducer.rs`、`tests/keymap.rs`。

### 1.1 建立回归测试

在现有测试构造器基础上，使用 MariaDb/MySql 参数矩阵：

| Explorer 位置 | a 预期 | draft/plan |
| --- | --- | --- |
| Database `[db]` | Table/View picker | 选择任一项后生成对应 draft |
| Schema `[db, db]` | Table/View picker | 合法 schema draft 上下文 |
| Tables Group | 直接 Table form | Table plan |
| Views Group | 直接 View form | View plan |
| 同 profile 另一数据库 | 同上 | target 指向所选 db，console target 不变 |

必须通过真实 Keymap 的 a 得到 action，再 app.update，检查 overlay、editor.draft、PlanCatalogMutation command 和生成 plan；不以直接调用 planner 代替入口测试。新增测试名称统一含 `mysql_namespace_create` 或 `mariadb_explorer_create`，方便定向运行。

Run: `cargo test --locked --test catalog_editor_state --test catalog_editor_reducer --test keymap --test object_mutation_contract mysql_namespace_create`

旧代码预期失败：Database 上无 draft、Schema anchor 被 planner 拒绝。不得把编译错误视为业务回归已复现。

### 1.2 统一 namespace 解析

在 `src/db/catalog_mutation.rs` 增加小型 DatabaseIsSchema 创建上下文解析函数/类型，供 editor 与 adapter 共用；不要把 SQL 生成放进 model。

精确规则：
- Catalog Database：路径恰为一个非空 db；规范化 schema id 为 `[db, db]`。
- Catalog Schema：路径恰为两个非空且符合数据库即 schema 身份的分量。
- Group：父 id 必须为合法 Schema，Tables 对应 Table、Views 对应 View。
- 其他 kind、错误长度、跨 profile、分组/对象不匹配返回 typed error。
- 保留 request.anchor，返回用于 draft 默认值、SQL、target、refresh 的规范化上下文。
- 非 MySQL/MariaDB 仍使用既有 namespace 行为。

### 1.3 修复 draft 和 planner

`CatalogEditorState::select_object_type`：DatabaseIsSchema 的 Database 和 Schema 来源均初始化 Table/View draft，使用 editor.database_kind 选择表类型；无法初始化时返回 false，不能留下 Form + None draft 的假成功状态。

`MySqlAdapter::plan_catalog_mutation`：Create 路径接受三种合法 anchor，根据 request.object_type 和 draft 生成 SQL；Edit 路径保持现有逻辑。SQL 标识符继续复用 quote_identifier，SQL target 和 refresh 的 schema 来自规范化上下文。若 draft 的 schema 可编辑，必须验证其与目标一致或显式重新验证新目标，不能静默忽略。

契约断言包括：含反引号/Unicode 名称的正确引用、refresh 为对应 Objects group、selection 为三段式对象 id、group/draft 不匹配拒绝、跨 profile 拒绝。

### 1.4 创建可用性与反馈

将 `selected_catalog_create_options` 内部升级为带原因的解析结果，保留 Option 包装给 help 使用即可。Keymap 对 SQL catalog/group 的 a 分发创建动作，由 reducer 在无有效 selection 时显示解析原因；保留 Profile/ConnectionGroup/Redis 专用分支。

原因至少区分：只读、未连接/连接中、非活动 profile、节点过期、不支持对象、非法/超出 scope 的目标。创建开始和提交均校验 identity、generation、epoch、read_only，不靠一次按键校验保证后续有效。

只对 MySQL/MariaDB 放宽“anchor database 必须等于活动 database”的限制，改为验证所选 execution target。`resolve_catalog_mutation_connection` 已验证 target.is_valid 并按 ConnectionKey 查连接或 connect_target，不新建重复连接机制。

### 1.5 定向验收与提交

Run: `cargo test --locked --test catalog_editor_state --test catalog_editor_reducer --test keymap --test object_mutation_contract`

预期全部通过；同时检查原有其他驱动用例。记录命令/退出码/HEAD/工作区状态。

建议提交：`fix(explorer): resolve mysql namespace creation from selected nodes`。只 stage 本单元相关文件，提交由 Luna 执行。随后继续单元 2，不等待用户 resume。

## 单元 2：创建数据库（schema 同义）完整闭环

**修改：** `src/model/catalog_editor.rs`、`src/ui/catalog_editor.rs`、`src/db/mysql.rs`、`src/app.rs`；如能力分发需要则修改 `src/db/mod.rs`。
**测试：** `tests/catalog_editor_state.rs`、`tests/catalog_editor_reducer.rs`、`tests/object_mutation_contract.rs`、`tests/ui_render.rs`、`tests/explorer_add.rs`。

### 2.1 先写入口与校验回归

MariaDB 活动可写 profile 上 a → ExplorerAdd → Database → name-only 表单 → review command。无需 Owner/Template。断言 Postgres 表单仍要求其原有必填字段。

额外已确认根因：`DatabaseDraft::new` 默认 template0/UTF8/libc/C；`DatabaseDraft::validate` 强制 owner、template、encoding。这会在进入 adapter 前阻断 MariaDB，必须一起处理。

Run: `cargo test --locked --test catalog_editor_state --test catalog_editor_reducer --test object_mutation_contract mariadb_database_create`

### 2.2 方言感知数据库表单

在既有 DatabaseDraft 上增加适当的创建方言/字段能力，或使用项目已有同类机制；统一字段显示、Tab/jk 导航、mouse focus、校验。MariaDB/MySQL 本修复先支持名称，明确显示 database/schema 同义；不提供 PostgreSQL Owner/Template/Locale 等字段，也不悄悄丢弃可编辑输入。

全链路校验必须使用相同方言：app review 调用 draft.validate 和 adapter planner 都能接受 name-only MySQL 数据库 draft。保持 Postgres 创建/编辑字段与校验。若新增模型字段，更新实际受影响的构造器和测试，不进行无关重构。

### 2.3 能力与 SQL 规划

MySqlAdapter profile_create 增加 Available Database。新增 Profile anchor 的 Database create 分支，验证 profile identity、mode、object type、draft、无 baseline，生成 `CREATE DATABASE <quoted-name>`。

- execution target 使用 request.current_database 中已存在且合法的数据库；不能连接新数据库，也不硬编码需要管理权限的 mysql 库。
- Autocommit，refresh 为 Databases，selection 为新 Database id `[name]`。
- 缺 current_database 时返回可见错误，不能伪造有效计划。
- Database/Schema 节点仍创建其内的 Table/View，不在 Database 内再建独立 schema。

### 2.4 连接菜单能力一致性

`explorer_add_options` 根据 profile_create 和连接可用性组合 Database/User/Role 的 availability。`open_profile_catalog_create` 再次检查对应能力，防止菜单打开后状态变化绕过。Connection/ConnectionGroup 仍可用；MySQL/MariaDB User/Role 没有规划器时应禁用并给原因；Postgres 已有能力保持。

### 2.5 刷新/定位与定向验收

模拟 CatalogMutationSucceeded 和后续目录页响应：overlay 关闭、Databases invalidation/request、新对象定位；失败显示错误并保留可修正表单；过期响应不得改变当前 UI。

Run: `cargo test --locked --test catalog_editor_state --test catalog_editor_reducer --test object_mutation_contract --test explorer_add --test ui_render`

建议提交：`feat(mysql): create databases through explorer catalog editor`。

## 单元 3：真实 MariaDB 与整体回归验收

**修改测试：** `tests/mariadb_catalog_mutation.rs`；需要辅助代码时使用 `tests/support/mod.rs` 既有 URL 约定。
**文档：** `docs/mariadb-test-database.md` 增加 Explorer 创建操作、权限与 schema 同义说明。
**按证据修改：** `src/runtime.rs`、`src/app.rs` 的跨库执行/刷新仅在新测试暴露缺陷时最小修复。

### 3.1 集成测试

扩展测试必须通过 DatabaseConnection.plan_catalog_mutation/execute_catalog_mutation，而非原始 execute 直接创建被测对象：
1. 使用有 CREATE DATABASE 权限账号连接已存在数据库，生成唯一测试 db 名。
2. plan/create 数据库，读取 catalog page 验证 Database id。
3. 用 Database、Schema、Group 三种锚点分别创建表/视图（名称互不冲突）；验证生成 SQL 与实际对象及目标数据库一致。
4. 读取 Objects 页确认 Table/View id，并验证视图查询结果。
5. 验证重复名失败传回错误；记录恢复/清理结果，清理唯一测试对象/数据库。

权限负例可用现有受限账号增加独立用例，但不能拿权限不足账号执行整个成功闭环。避免 panic 后跳过清理，采用现有清理模式或捕获执行结果后统一清理。

### 3.2 真实环境定向检查

参考 `docs/mariadb-test-database.md` 与 `docker-compose.mariadb.yml`。检查已有 fixture 再决定启动，不重置卷。成功测试账号需全局 CREATE DATABASE 权限，普通 lazydb fixture 账号不保证具备。

Run（先按环境设置 LAZYDB_TEST_MARIADB_URL，输出记录不得包含密码）：

`env LAZYDB_REQUIRE_DATABASE_TESTS=1 cargo test --locked --test mariadb_catalog_mutation -- --nocapture --test-threads=1`

预期测试真正执行并通过。LAZYDB_REQUIRE_DATABASE_TESTS=1 确保 URL 缺失不会伪装 pass。CI `.github/workflows/ci.yml:167-168` 已包含该 test target 与 MariaDB 11.4/root 配置，无需为本修复另建测试工作流。

### 3.3 一次完整检查

功能齐备后按现有 Rust CI 执行：

```
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

记录缺数据库 URL 的集成跳过，不冒称所有数据库已验收。针对共享 MySQL planner 的真实回归，可在 MySQL fixture 可用时运行 `cargo test --locked --test mysql_adapter -- --nocapture --test-threads=1`；这不能替代 MariaDB 新增创建链路测试。

本计划以 MariaDB plan/execute/目录验证及 deterministic 按键/reducer 测试作为自动化业务证据；真实数据库可由本地 fixture 或项目既有 CI 执行，不额外要求必须在本地手工验收。PTY 为补充：若可用，手动/自动操作 a → 表单 → preview → apply → Explorer 定位，确认不同数据库时 SQL console target 不变。PTY 环境问题至多一次针对性修复重试，随后由 Luna 审查决定记录限制或补充其他证据，不无限循环。

### 3.4 收尾

更新 validation.md，注明各命令、退出结果、HEAD/未提交 diff 和环境；相关代码/环境未变化不重跑已完成检查。完整 diff 审查重点：跨 profile/epoch 防护、数据库/schema 同义、无空 draft、无静默校验拒绝、UI 字段真实可执行、scope 内创建后可刷新、Postgres 兼容性。

建议提交：`test(mariadb): cover explorer catalog creation round trips`。Luna 完成后按工作流提交/合并 main；只写当阶段用户明确指定的新回执，不编造 token，不修改插件 state/checkpoint。

## 完成标准

- 活动 MariaDB profile 可创建 Database；Database/Schema/Tables/Views 对应入口可创建 Table/View。
- 所选数据库与 SQL console 当前数据库不同也不会静默无响应，创建 target 正确且控制台目标保留。
- 禁用/未支持操作显示明确原因，菜单/help/reducer 能力一致。
- SQL 预览与实际 DDL 一致；创建后刷新并定位；权限/重名错误可见，过期结果无副作用。
- MySQL 共享语义和现有其他驱动回归通过；真实 MariaDB 验收与环境限制如实记录。
