# Relation Mutation Capabilities and SQL Preview Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **当前 OpenCode 执行约定：** 本轮交付实施计划。后续按任务顺序实施；若执行环境没有上述技能，直接使用现有工具逐项执行并记录验证结果，不调用不存在的技能。任务有共享模型依赖，默认由单个执行者顺序完成。

**Goal:** 修复 MariaDB 无主键表在 Relation Data 中新增行无法提交的问题，并统一操作能力、变更计划及 SQL 预览，使界面承诺与实际驱动执行一致。

**Architecture:** 先完成 MariaDB 的最小闭环修复，再将新增能力、插入结果获取策略、已有行定位能力分开建模。草稿规范化为唯一的变更计划，预览和参数化执行消费同一份计划；数据库专有类型绑定、行版本和事务行为继续由驱动处理。

**Tech Stack:** Rust 2024 / Rust 1.94、SQLx 0.9、Tokio、Tiberius 0.12、Ratatui；MariaDB 10.5+，真实主验收环境使用项目 MariaDB 11.4 fixture。

---

## 1. 证据、基线与实施边界

以下行号来自分析时工作区，执行时以函数名和实际代码为准。

| 已确认事实 | 位置 |
| --- | --- |
| 仅 PostgreSQL 被允许无主键插入，且 `is_postgres` 直接等于该判断 | `src/app.rs:22410–22415`，`relation_save` |
| 相同限制分别出现在前置检查和请求构造 | `src/app.rs:22469`、`:22548` |
| MariaDB 已通过参数化 `INSERT ... RETURNING` 返回行 | `src/db/mysql.rs:2879–2944` |
| MariaDB Catalog 最低版本已是 10.5 | `src/db/mysql.rs:3890–3900` |
| 预览固定双引号、固定 `DEFAULT VALUES`、按点拆分展示名称 | `src/model/relation_review.rs:46`、`:117–127` |
| 两个预览调用点都传入 `tab.title()` | `src/app.rs:14961`、`:22625` |
| 插入入口直接创建草稿，提交阶段才报能力限制 | `src/app.rs:23087` |
| 旧 `EditableRelationCapability` 是整表二元开关，未接入当前 App 编辑链路 | `src/db/mutation.rs:43–148`；当前搜索仅发现测试调用 |
| MySQL 插入回读仅取第一主键 | `src/db/mysql.rs:2950–2983` |
| SQLite 插入回读使用 `rowid = last_insert_rowid()` | `src/db/sqlite.rs:3339–3346` |
| SQL Server 已使用 OUTPUT，但空列插入分支语法顺序有误 | `src/db/mssql.rs:2853–2905` |
| MariaDB RETURNING 库测试依赖环境变量，不配置时跳过 | `src/db/mysql.rs:4089–4106` |
| 数据库 CI 的 MariaDB 步骤目前只列外部测试目标 | `.github/workflows/ci.yml:167–168` |

实施前记录 `git rev-parse HEAD`、`git status --short`。现有未跟踪的 `docs/plans/2026-09-17-*.md` 和 `.git-opencode-tasks/` 为既有工作，保留。旧计划中的历史测试声明不能作为本次验收结果。

### 交付分期

- **M1：用户问题闭环。** Task 1–3、Task 10 中 MariaDB CI 步骤。可独立发布的修复切片。
- **M2：公共设计收敛。** Task 4–7，解决入口/提交分裂、列映射、预览/执行分裂。
- **M3：跨驱动收敛。** Task 8–11，验证并启用 SQLite/SQL Server 的无主键插入能力，修正 MySQL 可靠回查。
- 每个里程碑都必须具备对应测试证据；M1 完成不代表 M2/M3 完成。

### 本计划采用的产品语义

1. 无主键不妨碍创建新行。
2. 编辑/移除未提交的新行属于本地草稿操作，不需要数据库行定位。
3. 对已存在的数据库行，当前网格继续要求完整主键定位；唯一键和全行定位不在此次新增支持中。
4. 插入结果必须来自服务器返回或可靠回查，不能把草稿值当作数据库最终值。
5. 同一批次包含不支持的操作时，在首条写入前拒绝整个批次，保留全部草稿。
6. 插入后不能可靠回读的 MySQL 请求在写入前拒绝，并明确说明回读限制。未来若实现“仅返回影响行数、提交后刷新”，须单独扩展结果模型；本次不加入未接通的 `RefreshRequired` 枚举分支。
7. 原生 RETURNING/OUTPUT 对触发器的可见性由数据库决定，不笼统保证能看到所有 AFTER-trigger 的最终改写；需要另行刷新时必须明确标注并测试。

## 2. 目标模型与职责

### 2.1 操作级能力

在 `src/db/mutation.rs` 中定义操作能力，替换旧的整表 Editable/ReadOnly 作为生产决策来源。建议最小结构：

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationAvailability<T> {
    Available(T),
    Unavailable(EditDisabledReason),
    MetadataRequired,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InsertResultStrategy {
    Returning,
    Output,
    LookupByPrimaryKey,
    LookupBySqliteRowId { alias: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelationMutationCapabilities {
    pub insert: OperationAvailability<InsertResultStrategy>,
    pub update: OperationAvailability<Vec<usize>>,
    pub delete: OperationAvailability<Vec<usize>>,
}
```

这里的主键索引是元数据列序，不是网格列序。可新增类型包装以避免两种索引混用。`LookupByPrimaryKey` 还需对具体草稿验证键是否能在插入后确定；它不是“表有主键就保证可回读”。

职责分配：

- 驱动声明插入结果策略及引擎专属关系事实。
- 公共纯函数将驱动事实、表元数据和运行状态合并为操作能力。
- App 的创建、粘贴、已有行编辑/删除、预览、提交读取同一能力结果。
- 后端在执行边界验证请求、目标、定位列及结果策略，不能只信任 UI 判断。
- 未加载元数据是 `MetadataRequired`，不是 `MissingPrimaryKey`。不把未知状态缓存为不支持。

列级能力单独处理：生成列/不可写列禁止显式写入；可省略它们并由数据库赋值的 INSERT 仍应允许。不要因为一列 identity/generated 就把整张表判为只读。

### 2.2 唯一的变更计划

新增模块：

- `src/db/relation_plan.rs`：由草稿产生有序 `RelationMutationRequest`、统一列映射、批次预检。
- `src/db/relation_sql.rs`：对规范 mutation 生成结构化语句、参数引用、返回列和必要的后续回读步骤。
- `src/model/relation_review.rs`：仅将计划渲染为预览并计算摘要，不再重新解释草稿生成另一套 DML。

数据流：

```text
RelationEditSession + canonical relation + metadata + capability facts
    → RelationMutationPlan
        → dialect statement templates + parameter references
            → SQL review rendering
            → driver parameter binding / execution / decoding
```

实施约束：

- statement template 用结构化的文本片段和参数节点，禁止对字符串执行 `replace("?", value)` 一类替换。
- `InputValue::Null`、`Default`、未提供字段保持不同语义。
- 行版本、原值比较、执行顺序、返回列顺序都是计划的一部分。
- 数据库名/schema/object 从规范路径按驱动规则提取，不通过 title，不对对象名执行 `split('.')`，不把 PostgreSQL OID/SQL Server object_id 当成名称段。
- 预览优先显示方言正确的内联值；不支持无损内联的类型显示参数占位符和类型/值信息，不生成貌似可直接执行却语义错误的字面量。
- 预览中的字符串转义必须考虑 MariaDB/MySQL 的反斜杠模式。未知 session mode 时采用不依赖该模式的编码表达式或保留参数表示，不猜测转义规则。
- 执行永远采用参数绑定。
- 执行阶段才确定的 last_insert_id 等值在预览标为动态参数；不伪造值。执行前必须校验的元数据查询与用户 DML 在展示上有明确标识。

## 3. 详细任务

### Task 1：建立 MariaDB 上层拦截的失败回归

**Files**
- Modify/Test: `src/app.rs` 的测试模块。
- Reference: `src/app.rs::relation_save_allows_postgres_insert_without_primary_key`。

**步骤**
1. 提取现有无主键插入 fixture 中可复用的连接、表身份、DDL 和网格构造代码，保留原 PostgreSQL 测试。
2. 新增 `relation_save_allows_mariadb_insert_without_primary_key`：MariaDb profile，规范路径 `[database, database, table]`；两列均为 text，无主键；创建草稿并显式提供 `'1'`、`'2'`。
3. 通过 `Action::RelationCommit` 进入真实 reducer，断言得到 `Command::RelationMutation`、空主键和准确的列/值映射，而非仅调用纯函数。
4. 添加 `relation_save_rejects_keyless_existing_mutations_before_insert`：混合新行与已有行更新/删除，断言零写入 command、草稿仍在。
5. 运行以下命令，记录第一条在修复前确实失败于未发出 mutation。

```sh
cargo test --lib relation_save_allows_mariadb_insert_without_primary_key -- --nocapture
cargo test --lib relation_save_rejects_keyless_existing_mutations_before_insert -- --nocapture
```

第二条约束测试可在当前实现上通过；不要求所有新测试必须红灯。

### Task 2：修复 MariaDB 放行与 PostgreSQL 行版本耦合

**Files**
- Modify/Test: `src/app.rs::relation_save`。

**步骤**
1. 单独取得当前连接对应的 `database_kind`。
2. 最小修复逻辑使用以下独立判断：

```rust
let is_postgres = database_kind == Some(crate::profile::DatabaseKind::Postgres);
let allow_keyless_insert = matches!(
    database_kind,
    Some(crate::profile::DatabaseKind::Postgres | crate::profile::DatabaseKind::MariaDb)
);
```

3. 两处无主键检查消费同一结果；保留混合批次的前置拒绝。
4. 新增 `relation_save_mariadb_delete_does_not_require_postgres_version`：有主键的 MariaDB 行、`version=None`，应构造删除 command。
5. 新增/保留 PostgreSQL 缺失行版本的删除拒绝回归；确保允许 MariaDB 不会削弱 PostgreSQL 规则。
6. 执行测试：

```sh
cargo test --lib relation_save -- --nocapture
```

**验收**：MariaDB 无主键 INSERT 可进入驱动；MariaDB 删除不要求 xmin；PostgreSQL 行版本规则保持正确。

本步骤的类型判断在 Task 4 改为能力来源，不在更多 App 入口复制白名单。

### Task 3：真实 MariaDB worker、提交/回滚与方言预览闭环

**Files**
- Modify/Test: `src/db/mysql.rs` 现有 `#[cfg(test)]` 测试模块。
- Modify/Test: `src/model/relation_review.rs`。
- Modify/Test: `src/app.rs` 两个 preview 调用点。
- Reference: `src/runtime/transaction.rs`、`docker-compose.mariadb.yml`。

**步骤**
1. 复用 crate 内 adapter/backend/worker 访问方式，不为测试扩大生产 API 可见性。
2. 创建 UUID 后缀的 InnoDB 测试表，DDL 与用户案例相同；测试统一使用 `mariadb_relation_grid_` 前缀。
3. 通过真实事务 worker 连续发送两次相同值的 InsertRow，断言每次返回两列 Text、没有错误回查或去重。
4. Commit 后用独立连接确认两行持久化；另开事务插入后 Rollback，确认计数不增加。
5. 增加空列默认行、显式 NULL、默认值、带引号的列名和实际返回列序案例；为批次回滚另建带 CHECK/UNIQUE 约束的 fixture。
6. 先为 review 引入明确的方言及结构化目标参数，修复反引号、空列语法和目标名称；Task 6–7 再迁移到共享计划，避免两个长期实现。
7. 新增 `mariadb_review_` 前缀的 reducer/渲染回归：两个预览入口都得到正确 schema/database 目标；普通文本 `'1'`、`'2'` 保持可读；无需开启 ANSI_QUOTES。
8. 所有 worker 等待使用有界 timeout；清理唯一创建的表，不清理用户表、不删除 fixture volume；失败时保留首个错误。

**本地运行**（若复用已有测试实例，直接使用其 URL）：

```sh
docker compose -f docker-compose.mariadb.yml up -d --wait
LAZYDB_REQUIRE_DATABASE_TESTS=1 LAZYDB_TEST_MARIADB_URL='mariadb://lazydb:lazydb_password@127.0.0.1:3307/lazydb_test' cargo test --lib mariadb_relation_grid_ -- --nocapture --test-threads=1
cargo test --lib mariadb_review_ -- --nocapture
cargo test --lib relation_review -- --nocapture
```

**验收**：用户原始问题的 reducer、driver、worker、commit/rollback、预览各层都有证据。不能以直接执行手写 INSERT 的冒烟测试代替 mutation 测试。

### Task 4：实现操作能力模型与元数据加载状态

**Files**
- Modify/Test: `src/db/mutation.rs`。
- Modify: `src/db/mod.rs`、`src/db/mysql.rs`、`src/db/postgres.rs`、`src/db/sqlite.rs`、`src/db/mssql.rs`。
- Modify: `src/model/relation.rs`、`src/runtime.rs`、`src/app.rs`。

**步骤**
1. 实现第 2.1 节的最小能力模型及具体禁用原因：缺主键、缺少元数据、驱动未实现、无法可靠回读、只读连接、非实时快照等。
2. 驱动用统一接口产出关系写入事实：MariaDB/PG 原生 Returning，SQL Server Output，SQLite 可用 rowid 别名/表形态，MySQL 完整主键与自增列信息。
3. 新增 `RelationMutationFacts` 并随当前关系元数据加载结果传递；在 `RelationTab` 保存 transient facts，绑定已有 connection/profile/scope/tab generation 身份。该状态不进入 workspace 持久化。
4. 元数据 facts 查询由 Runtime 调用对应 adapter，在同一次逻辑元数据加载中完成；使用现有 relation request 失效检查，切换连接、表重命名、DDL 刷新时失效。
5. 公共函数计算能力；表无主键时只使 update/delete 不可用，不能覆盖原生 RETURNING 的 insert。
6. 将 `relation_save` 的临时类型白名单替换为该能力结果；PostgreSQL 行版本规则继续独立存在，或用独立版本策略表达。
7. 迁移旧 `editable_capability` 的测试与调用；确认没有使用者后移除旧二元模型，避免并存两个决策源。
8. 增加 `relation_mutation_capabilities_` 表驱动测试，覆盖所有引擎声明、无主键、元数据未就绪、生成列、已有主键和只读状态。

```sh
cargo test --lib relation_mutation_capabilities_ -- --nocapture
cargo test --lib relation_save -- --nocapture
cargo test --test relation_runtime --test relation_tabs
```

**验收**：未知元数据不会被当作无主键；能力不跨连接/代际复用；只有驱动事实被验证后的操作才标为可用。

### Task 5：统一编辑入口行为与草稿体验

**Files**
- Modify/Test: `src/app.rs` 的 `relation_insert_row`、`relation_paste`、`relation_edit_cell`、`relation_delete_range`、元数据完成处理。
- Modify/Test: `src/model/relation.rs`、`src/model/relation_edit.rs`。
- Modify: `src/ui/relation.rs`、`src/help.rs`；仅同步已有快捷键提示与能力说明。

**步骤**
1. 新增/粘贴使用 insert 能力；已有行编辑和删除分别使用 update/delete 能力。
2. 未加载元数据时复用现有 DDL 加载链路，保存一个带 tab/request generation 的待执行编辑意图；同一请求只加载一次。
3. 元数据加载成功后仅在意图仍匹配当前 tab/代际时重放一次；失败保留现有草稿，显示原因。
4. InsertDraft 的本地改单元格和删除不走已有行定位检查。
5. insert 已提交为 Clean 后按已有行能力处理；允许继续新增另一行。
6. 生成列只限制显式赋值；可复制的普通列进入新草稿，生成列默认省略；不可写列必须提供明确 UI 原因。
7. 增加 `relation_edit_capability_` 测试，覆盖新增、粘贴、混合选区删除、元数据加载重放、取消/切换 tab、提交后的无主键行。

```sh
cargo test --lib relation_edit_capability_ -- --nocapture
cargo test --test relation_tabs --test quit_transaction_review
```

**验收**：支持的插入可正常操作；不支持的已有行修改在入口告知；提交阶段仍进行整批校验。

### Task 6：抽取规范化 mutation 计划，统一列映射和顺序

**Files**
- Create/Test: `src/db/relation_plan.rs`。
- Modify: `src/db/mod.rs`、`src/db/mutation.rs`。
- Modify/Test: `src/app.rs::relation_save`、`relation_mutation_result`。

**步骤**
1. 将 `relation_save` 中从 edit rows 到 requests 的转换抽为纯函数，输入显式包含规范身份、metadata、结果列名和能力。
2. 构造双向列映射，所有读取草稿值都用结果索引，所有 mutation 列号都用元数据索引。修正目前 update 分支混用两种索引的风险。
3. Insert 的 `(metadata_column, value)` 成对排序，保留值关联；这也满足 SQL Server 对索引唯一且有序的校验。
4. 在生成第一条 command 前校验整批：列存在、索引唯一、必需主键列完整、能力可用、输入可绑定。
5. 明确定义执行顺序，并使 review 消费同一个顺序。保持当前正常操作顺序，避免无需求地重排 delete/insert。
6. 同一行多个主键字段被修改时，后续 mutation locator 必须依据前一条服务器返回结果更新；若不能静态确定，以显式结果依赖表示，不能继续使用旧主键发送后续请求。
7. 结果行先按返回元数据顺序解码，再映射回网格列序；不再直接假定 returned row 与 grid row 同序。
8. 新增 `relation_plan_` 测试：反转列序、稀疏 insert、复合键更新、原值 NULL、删除快照、不同表同名字段、混合批次失败零请求。

```sh
cargo test --lib relation_plan_ -- --nocapture
cargo test --lib relation_save -- --nocapture
cargo test --lib relation_insert_success -- --nocapture
```

**验收**：网格列序不同于元数据列序时也写入/回显正确；不能依赖简单 fixture 掩盖索引错误。

### Task 7：统一方言 SQL 编译、预览和实际执行

**Files**
- Create/Test: `src/db/relation_sql.rs`。
- Modify: `src/db/mod.rs`、`src/db/relation_plan.rs`。
- Modify/Test: `src/model/relation_review.rs`、`src/app.rs` 两个 review 入口。
- Modify/Test: `src/db/mysql.rs`、`src/db/postgres.rs`、`src/db/sqlite.rs`、`src/db/mssql.rs` 的 mutation SQL 构造。

**步骤**
1. 实现规范对象引用：MySQL/MariaDB `database.table`，PG/SQLite `schema.table`，SQL Server 使用经过验证的数据库/schema/object；标识符逐段引用。
2. 先迁移 INSERT：占位符、NULL/DEFAULT/省略列、RETURNING/OUTPUT、回查模板共享同一编译结果。驱动保留 native binding 与 decode。
3. 方言处理空列 INSERT：PG/SQLite `DEFAULT VALUES`，MariaDB/MySQL `() VALUES ()`，SQL Server `OUTPUT ... DEFAULT VALUES`。
4. 再迁移 UPDATE/DELETE：保留原值比较、NULL-safe predicates、PostgreSQL xmin、影响行数检查和已有事务边界；不为了预览简化执行条件。
5. 预览 renderer 基于参数节点生成文本；Text、Boolean、Bytes、时间类型各有规则；不可无损表达时展示参数和类型，不走 `clipboard_text()` 通用字符串兜底。
6. 删除旧 preview 中的 `predicate()`、按点拆分名称和 `WHERE 1 = 1` 回退。无法编译的操作显示阻断原因，确认界面不能继续提交。
7. 两个 preview 入口与实际 save 调用同一计划构造；事务中展示已经提交给 worker 的计划快照，而非重新从变化的草稿生成。
8. 新增 `relation_sql_` 测试：名称含点/反引号/双引号/方括号、空列插入、文本反斜杠/单引号、字节值、动态回查参数、主键更新顺序、NULL 比较。
9. 对每个驱动验证模板 SQL 与参数顺序，不仅做预览字符串快照；保留真实数据库测试作为语法与类型行为依据。

```sh
cargo test --lib relation_sql_ -- --nocapture
cargo test --lib relation_review -- --nocapture
cargo test --test quit_transaction_review --test transaction_reducer
LAZYDB_REQUIRE_DATABASE_TESTS=1 LAZYDB_TEST_MARIADB_URL='mariadb://lazydb:lazydb_password@127.0.0.1:3307/lazydb_test' cargo test --lib mariadb_relation_grid_ -- --nocapture --test-threads=1
```

**验收**：预览与执行允许参数展示形式不同，但目标、列、条件、操作顺序及结果获取方式一致。

### Task 8：验证并启用 SQLite、SQL Server 无主键插入

**Files**
- Modify/Test: `src/db/sqlite.rs`、`src/db/mssql.rs`。
- Modify/Test: `src/app.rs` 能力及提交测试。
- Modify/Test: `tests/sqlserver_transactions.rs`。
- Reference: `tests/sqlite_transactions.rs`。

**SQLite 步骤**
1. 利用权威表元数据判断 rowid 表、WITHOUT ROWID、虚拟表和可用 rowid 别名；不要从网格列名猜表形态。
2. 在 `rowid`、`_rowid_`、`oid` 中选择未被用户列遮蔽的真实隐藏别名；没有可靠路径时返回具体禁用原因。
3. WITHOUT ROWID 表使用完整且可确定的主键回查策略；不错误执行 last_insert_rowid 查询。该表必有主键，但依然要覆盖插入默认键值无法确定的情形。
4. 明确禁止未验证的虚拟表写入能力；普通 rowid 表通过同一连接插入并回读。
5. 添加 `sqlite_relation_grid_` 测试：无主键重复行、用户 `rowid` 列、全部别名遮蔽、WITHOUT ROWID、默认值、回滚。

**SQL Server 步骤**
1. 修复空列 INSERT 的 OUTPUT 位置，并使用明确返回列序。
2. 验证 reordered insert 列被规范化，重复列被拒绝；OUTPUT 返回必须恰好一行。
3. 覆盖无主键普通表、默认行、identity/computed/rowversion 列、省略生成列、提交和回滚。
4. 对启用触发器的表，验证直接 OUTPUT 的限制；需要采用 OUTPUT INTO 时为其设计类型匹配的结果容器并测试。未实现可靠路径的表在能力中给出具体原因，不宣称普遍支持。
5. 对 INSTEAD OF/AFTER trigger 的结果语义按真实行为断言，不把 OUTPUT 结果当成触发器后最终快照。
6. App 的无主键 insert 表驱动用例加入已通过验证的 SQLite/SQL Server 表形态。

```sh
cargo test --lib sqlite_relation_grid_ -- --nocapture
cargo test --test sqlite_transactions
cargo test --test sqlserver_transactions -- --nocapture --test-threads=1
```

SQL Server 命令要求已配置 `LAZYDB_TEST_SQLSERVER_URL`；若本地不可用，使用数据库 CI 的真实服务验证，记录本地跳过而非通过。

### Task 9：修复 MySQL 插入的可靠键回查

**Files**
- Modify/Test: `src/db/mysql.rs`。
- Modify/Test: `src/db/mutation.rs`、`src/db/relation_plan.rs`、`src/db/relation_sql.rs`。

**步骤**
1. 根据全部主键列构造 SELECT predicate，不再只取 `.first()`。
2. 回查前验证每个键值来源：用户显式值、已确认自增列的 insert result；未知默认表达式/触发器可能改写的键不能直接猜测。
3. 显式 NULL 和 0 按实际 session SQL mode、自增元数据与 insert result 处理，不查询自增列 `IS NULL`；0 与 NO_AUTO_VALUE_ON_ZERO 需要真实测试。
4. 对无法确定回读身份的请求，在执行 INSERT 前返回能力/计划错误；不要 INSERT 成功后才发现无主键。
5. 使用显式返回列序；回查要求恰好一行，不用 `fetch_one` 默默掩盖多个匹配结果。
6. 添加 `mysql_relation_grid_` crate 内集成测试：复合键首列重复、显式键、自增省略/NULL/0、默认键不确定、无主键前置拒绝。
7. MariaDB 始终保留原生 RETURNING 路径，不退回 MySQL 回查。

```sh
cargo test --lib mysql_relation_grid_ -- --nocapture --test-threads=1
cargo test --test mysql_adapter -- --nocapture --test-threads=1
```

为新的 MySQL 集成测试采用与 MariaDB 相同的强制环境检查规则；数据库 CI 设置 `LAZYDB_REQUIRE_DATABASE_TESTS=1` 和 MySQL URL 后运行，不能用 MariaDB 实例证明 MySQL 行为。

### Task 10：补齐强制数据库 CI 与事务回归

**Files**
- Modify: `.github/workflows/ci.yml`。
- Modify/Test: `src/runtime/transaction.rs`、`src/app.rs`，仅在现有测试未覆盖新路径时添加。
- Reference/Test: `tests/relation_runtime.rs`、`tests/postgres_relation_mutations.rs`、`tests/sqlserver_transactions.rs`。

**步骤**
1. 在 databases job 的 MariaDB 步骤旁新增库测试步骤，复用该 job 的 URL：

```sh
timeout 10m env LAZYDB_REQUIRE_DATABASE_TESTS=1 cargo test --locked --lib mariadb_relation_grid_ -- --nocapture --test-threads=1
timeout 10m env LAZYDB_REQUIRE_DATABASE_TESTS=1 cargo test --locked --lib mariadb_insert_returning -- --nocapture --test-threads=1
```

2. 加入 MySQL 新库测试的强制步骤：

```sh
timeout 10m env LAZYDB_REQUIRE_DATABASE_TESTS=1 cargo test --locked --lib mysql_relation_grid_ -- --nocapture --test-threads=1
```

3. 在有对应服务的数据库 job 中显式运行 `postgres_relation_mutations`，保留并扩展 SQL Server transaction 测试。
4. 检查每条过滤命令实际运行非零测试；缺 URL 必须失败。Linux CI 的 `timeout` 不要求 macOS 本地直接照搬。
5. 回归：失败清空后续写队列、整批 rollback、草稿快照恢复、commit 失败/结果未知、失效 connection generation 不接受结果。
6. 混合 unsupported 批次验证没有任何 mutation 被发送；数据库约束失败验证先前成功的 INSERT 也被回滚。

```sh
cargo test --lib relation_mutation_failure -- --nocapture
cargo test --lib failed_relation_mutation_clears_remaining_writes_and_restores_full_snapshot -- --nocapture
cargo test --test relation_runtime --test transaction_reducer --test quit_transaction_review
```

**验收**：新的真实数据库回归在 CI 不依赖“开发者恰好设置了 URL”。

### Task 11：文档、最终检查与交付

**Files**
- Modify: `docs/database-capabilities.md`。
- Modify: `docs/mariadb-test-database.md`。
- Modify: `src/help.rs`，与已实现行为一致。

**步骤**
1. 单独增加 Relation Data 操作矩阵，列明 insert/update/delete、回读策略及表形态限制，不与 Catalog DDL 能力混写。
2. 记录原始案例的操作与预期预览，说明 NULL 和省略字段的不同语义。
3. 更新 MariaDB 测试文档：库测试命令、强制 URL 检查、worker 验证目标。
4. 按当前 CI 的真实 Rust 门禁运行一次最终检查：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
git diff --check
```

5. 真实数据库测试分开记录：MariaDB、MySQL、PG、SQL Server 的版本、URL 是否配置、命令、实际测试数及结果；不输出真实连接密码。无外部数据库的通用 test 通过不替代数据库 CI。
6. 人工 TUI 验收聚焦原始流程：打开表 → 新增 → 输入 1/2 → review → commit → refresh → 数据一致；再验证空行默认值、重复行、无主键已有行的编辑提示。
7. 收集最终 diff 和测试记录。提交按可独立验收的里程碑组织，不把无关未跟踪文件纳入提交；只有执行任务明确包含提交时才创建 Git commit。

## 4. 最终验收矩阵

| 场景 | 预期 |
| --- | --- |
| 用户提供的 MariaDB 两列 TEXT 无主键表 | 新增提交成功，回显和刷新均为实际数据库值 |
| MariaDB 两条完全相同的新行 | 两条都插入，返回各自本次结果，不依赖全列回查 |
| 无主键新增草稿本地修改/删除 | 可操作，无数据库定位需求 |
| 无主键已有行更新/删除 | 入口明确提示；提交层整批拒绝 |
| MariaDB 有主键删除、无 xmin | 正常构造删除；不走 PostgreSQL 行版本门槛 |
| PostgreSQL 删除缺少行版本 | 继续拒绝并要求刷新 |
| 普通 rowid SQLite 表无主键插入 | 同连接准确回读并提交 |
| SQLite rowid 名遮蔽/WITHOUT ROWID | 使用验证过的替代策略或执行前具体拒绝 |
| SQL Server 无主键/default-only 插入 | OUTPUT 语法正确，返回列序准确 |
| MySQL 复合主键首列重复 | 按完整键回查，返回本次行 |
| MySQL 无可靠插入回读身份 | INSERT 前拒绝，不执行后猜测身份 |
| 生成列和默认值 | 可省略生成列；显式 NULL 与默认值不混淆 |
| 网格列序与 metadata 列序不同 | 参数绑定、回显均准确 |
| 特殊对象名 | 逐段正确引用，title 不参与目标定位 |
| review 与执行 | 同一规范计划，目标/条件/顺序/返回策略一致 |
| 批次中途约束错误 | 回滚整批，停止后续 mutation，恢复草稿 |
| 连接切换或元数据过期 | 不重放旧编辑意图，不接受旧 mutation 结果 |
| CI 缺少必须的数据库配置 | 明确失败，不静默跳过 |

## 5. 依赖与建议提交边界

```text
Task 1 → Task 2 → Task 3 → M1（并完成 Task 10 的 MariaDB CI 步骤）
                     ↓
                  Task 4 → Task 5 → Task 6 → Task 7 → M2
                                               ↓
                                            Task 8 → Task 9 → Task 10 → Task 11 → M3
```

建议逻辑提交名：

1. `fix(relation): allow MariaDB keyless inserts and correct SQL review`
2. `refactor(relation): centralize operation-level mutation capabilities`
3. `refactor(relation): share mutation plans between review and execution`
4. `fix(db): validate cross-driver inserted-row retrieval`
5. `test(relation): enforce live mutation coverage in database CI`

阶段完成条件以实际验收矩阵为准，不以提交数量为准。测试通过后只因新改动、失败或明确未解决问题才扩展/重复测试。

## 6. 参考

- MariaDB INSERT RETURNING：https://mariadb.com/docs/server/reference/sql-statements/data-manipulation/inserting-loading-data/insertreturning
- MariaDB 标识符：https://mariadb.com/docs/server/reference/sql-structure/sql-language-structure/identifier-names
- 既有设计：`docs/plans/2026-08-27-relation-data-editing-design.md`
- 既有 MariaDB 修复背景：`docs/plans/2026-09-17-mariadb-insert-returning.md`
- 真实 CI：`.github/workflows/ci.yml`

执行 Task 7–9 时，应进一步核对 SQL Server OUTPUT/trigger 和 SQLite rowid 的官方文档与目标版本；当前计划已识别这些边界，不能仅凭 MariaDB 的 RETURNING 行为类推其他数据库。
