# MariaDB 完整支持实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行环境若没有该技能，按本文任务顺序执行，并逐项记录测试证据；本文本身不授权启动子代理。

**Goal:** 在现有 MySQL 兼容传输基础上，为 MariaDB 建立独立、可验证的服务器能力、数据类型、目录、编辑、SQL 方言与连接支持。

**Architecture:** 继续复用 `MySqlAdapter` 的连接池、执行、事务和公共元数据代码，把产品识别、版本能力、类型转换和对象变更拆成小模块。静态实现能力、服务器能力与当前权限分别建模；只有通过定义读取、操作、重新读取与界面刷新验证的功能才进入可用能力集合。MariaDB 专属语义通过显式配置接入共享 SQL 和 UI 管线。

**Tech Stack:** Rust 1.94、Tokio、SQLx 0.9 MySQL transport、sqlparser 0.62、chrono、ratatui、GitHub Actions、MariaDB/MySQL 服务测试。

---

## 1. 基线与执行规则

审查日期：2026-09-14。当前是代码审查结论，不是实机认证结果。

已确认：

- `src/db/mysql.rs::catalog_capabilities` 只列 Tables、Views、Functions、Procedures、Triggers。
- `catalog_mutation_capabilities` 只声明 table/view；现有 table edit 主要是 rename。
- `src/db/capabilities.rs` 对 MariaDB 设置 `relation_edit=false`，但共享事务后端已有行 mutation。
- `src/sql/dialect.rs` 将 MariaDB 映射为 MySQL。
- `parse_version_triplet` 从开头读取版本，不能正确处理 `5.5.5-10.11.8-MariaDB`。
- `decode_cell` 将 TIME 解码为 `NaiveTime`，不能覆盖负时长与超过 24 小时的合法值。
- CHECK 未进入结构化约束读取；索引读取存在 MariaDB 专用 SQL。
- CI 配置 MariaDB 11.4，并复用 `mysql_adapter` suite。

上次验证命令在 Redis Action 匹配不完整处编译失败；工作区正在变化，不能把该错误视为现在仍然存在。执行任务前重新检查基线。

### 1.1 与已有计划衔接

已有 `docs/plans/2026-09-14-multi-database-object-mutation-implementation.md`：

- Task 12 的 MySQL 公共定义和变更逻辑对应本文 T08–T10、T16。
- Task 13 的 MariaDB 特化由本文 T02–T03、T11–T12 细化。
- Task 14 的索引、约束、主体对应本文 T10、T16。

执行前检查这些任务是否已落地，复用已合并实现和测试；不建立第二套 definition、draft、plan、refresh 协议。数据库/角色表单同时参考 `docs/plans/2026-09-14-catalog-database-role-form-redesign.md`。

### 1.2 每个任务的执行节奏

以下任务是交付单元。一个任务含多个用例时，按一个行为一个循环拆成约 2–5 分钟步骤：

1. 添加一个有业务意义的失败测试或真实服务 fixture。
2. 运行该测试，确认失败原因是目标行为缺失，而不是构建/环境错误。
3. 实现该行为，运行同一测试。
4. 全部行为完成后运行任务指定的回归集合。
5. 更新能力与测试证据，按任务单独提交代码及测试。

文档、纯搬移等低风险变更不写镜像测试；复用现有测试。本文提供精确行为契约和核心回归代码，不把未知的服务器语义当作已确认实现。

每个任务提交前只暂存该任务文件，禁止 `git add .`。提交消息见各任务。建议在可构建的独立分支/worktree实施；不得清理或覆盖当前其他工作的文件。

### 1.3 测试环境约定

- `LAZYDB_TEST_MARIADB_URL`：隔离的 MariaDB 测试数据库，格式 `mariadb://...`。
- `LAZYDB_TEST_MYSQL_URL`：MySQL 8.4 回归服务。
- 新增 `LAZYDB_REQUIRE_DATABASE_TESTS=1`：集成任务缺少所需连接配置时失败。
- fixture 名称使用随机后缀；显式关闭连接并清理 fixture；失败时也尝试清理。
- 涉及创建用户、数据库、终止查询的测试使用专用测试服务与明确权限账户。
- 普通 suite 不因 strict 标志要求所有数据库都存在：只有该 suite 实际依赖的 URL 是必需项。
- 执行前核对 SQLx、sqlparser 与目标 MariaDB 版本官方文档；Context7 不可用时使用官方文档和锁定版本源代码。

## 2. 里程碑与依赖

| 里程碑 | 任务 | 可交付结果 |
| --- | --- | --- |
| M0 验证基础 | T01–T05 | 独立测试、版本/能力识别、TIME 与核心类型无损读取 |
| M1 日常编辑 | T06–T10 | 事务可靠性、网格编辑、表/视图/索引/约束编辑 |
| M2 原生对象 | T11–T12 | Sequence 浏览、搜索、DDL、创建/编辑/删除闭环 |
| M3 原生 SQL | T13–T14 | MariaDB 方言上下文、语句切分、分页与诊断边界 |
| M4 扩展管理 | T15–T18 | 过程/事件、数据库/主体、系统版本表、连接配置 |
| M5 支持认证 | T19–T20 | 性能/故障测试、版本矩阵、文档与最终验收 |

依赖：

```text
T01 → T02 → T03
T01 → T04 → T05
T03 + T05 → T06 → T07
T03 → T08 → T09 → T10
T03 + T08 → T11 → T12
T03 → T13 → T14
T08 + T14 → T15
T08 + T10 → T16
T09 + T14 → T17
T03 + T06 → T18
T03 + T06 → T19
T01…T19 → T20
```

T04、T08、T13 可以在前置条件满足后独立安排，但共享模型变更必须串行合并。完成 M0–M3 可发布“日常开发支持增强”；只有 M4–M5 也完成，才按已认证范围描述原生管理支持。

## 3. 详细任务

### T01：建立可见、严格的 MariaDB 测试入口

**Files:** 新增 `tests/support/mariadb.rs`、`tests/mariadb_adapter.rs`；修改 `tests/mariadb_profile.rs`、`.github/workflows/ci.yml`；共享 suite 如需复用 helper，再修改 `tests/mysql_adapter.rs`。

1. 重新运行 `cargo check --locked`，记录基线；若被并行工作阻断，先选定可构建基线再继续测试，不顺手修改 Redis 功能。
2. 在 helper 中实现 URL 加载：有值返回配置；无值且 strict=true 则报错；普通本地运行无值时输出 `SKIP: LAZYDB_TEST_MARIADB_URL is not set`。
3. 用纯函数测试三条配置分支，避免并发修改进程环境变量。
4. 新增实机 smoke：产品标识、`VERSION()`、空结果列元数据、多结果集、受影响行数、错误码。
5. CI MariaDB step 显式执行 profile、adapter 和已有共享 suite；设置 strict=1。
6. 对照共享 `mysql_adapter` fixture，保留已有目录分页、scope、DDL验证，不复制整个 suite。

**Run:** `cargo test --locked --test mariadb_profile --test mariadb_adapter -- --nocapture --test-threads=1`。

**Expected:** 有服务时断言全部执行；无服务本地明确 skip；strict 模式无 URL 时非零退出。空结果列测试失败则在 T05 补齐，M0 不得带失败结束。

**Commit:** `test(mariadb): add explicit strict integration entrypoints`。

### T02：产品识别与版本归一化

**Files:** 新增 `src/db/mysql/server.rs`；修改 `src/db/mysql.rs`、`tests/mariadb_profile.rs`、`tests/mysql_adapter.rs`。

1. 在现有公开函数上加入可直接运行的回归：

```rust
#[test]
fn mariadb_catalog_accepts_compatibility_prefix() {
    use lazydb::{db::mysql::supports_catalog_version_for_kind, profile::DatabaseKind};
    assert!(supports_catalog_version_for_kind(
        DatabaseKind::MariaDb,
        "5.5.5-10.11.8-MariaDB"
    ));
    assert!(!supports_catalog_version_for_kind(
        DatabaseKind::MySql,
        "5.5.5-10.11.8-MariaDB"
    ));
}
```

2. 增加普通 10.5/10.11/11.4、低于门槛、发行版后缀、空值、畸形值、MySQL 8.4 用例。
3. 将产品判断和版本解析集中到 server 模块；兼容前缀仅按明确 MariaDB 模式处理，不删除任意版本片段。
4. 保存原始版本用于诊断；不能解析时返回明确 unsupported/unknown 能力，不猜测版本。
5. 连接探测区分 profile 所选产品与实际服务器产品。延续现有严格目录边界：产品不匹配时明确指出所选驱动及实际产品，不静默改写持久化 profile。
6. 更新索引分支注释：当前 CI 已使用 `mariadb://`，不再声称固定通过 MySQL URL 测试。

**Run:** `cargo test --locked --test mariadb_profile --test mysql_adapter catalog`。

**Expected:** 前缀回归从失败变通过；MySQL 不接受 MariaDB 版本。

**Commit:** `fix(mariadb): normalize product-specific server versions`。

### T03：连接级能力快照与权限边界

**Files:** 新增 `src/db/mysql/capabilities.rs`；修改 `src/db/mysql/server.rs`、`src/db/mysql.rs`、`src/db/mod.rs`、`src/db/capabilities.rs`、`src/model/session.rs`、`src/runtime.rs`；测试 `tests/object_mutation_contract.rs`、`tests/mariadb_adapter.rs`。

1. 写测试：相同 profile 在不同实际版本上得到不同原生能力；未知版本不显示未验证功能；权限拒绝不变成“服务器不支持”。
2. 建立 server snapshot：实际产品、规范化版本、原始版本和稳定服务器设置。
3. 能力计算输出实现范围与服务器范围的交集；表编辑拆成 rename/columns/options，避免仅用“edit table”概括。
4. 对权限使用运行时结果或有范围的权限快照；禁止因一次连接失败永久关闭整个驱动能力。
5. 连接建立或重连时刷新快照，目录/搜索/索引读取共用；移除不必要的每表 VERSION 查询。
6. `sql_mode`、时区等会话状态归属于物理会话/事务执行上下文，不能假设连接池所有连接一直相同；T13接入编辑器。

**Run:** `cargo test --locked --test object_mutation_contract --test mariadb_adapter -- --nocapture --test-threads=1`。

**Expected:** 已实现的公共功能保留；不存在“支持 sequence 的服务器就自动开放未实现 sequence 编辑”的情况。

**Commit:** `feat(mariadb): derive capabilities from server identity`。

### T04：TIME 无损读取和写回基础

**Files:** 新增 `src/db/mysql/value.rs`、`tests/mariadb_values.rs`；修改 `src/db/mysql.rs`、`src/model/cell_editor.rs`。若选择扩展共享值模型，修改 `src/db/value.rs` 及编译器指出的序列化/渲染消费者。

1. 创建真实 TIME(6) 列并插入 `00:00:00`、`23:59:59.123456`、`24:00:00`、`-01:02:03.123456`、正负最大值。
2. 断言显示、复制和序列化不变号、不模 24 小时、不丢微秒。
3. 优先将 MariaDB/MySQL TIME 统一解码为规范化无损文本，保留列原生类型；从锁定版本 SQLx 的 MySQL 时间类型读取，不通过 NaiveTime 中转。
4. 只有现有文本路径不能满足类型绑定/编辑时，才扩展共享 CellValue 为有符号微秒时长；若扩展，保留旧 snapshot 的反序列化兼容性。
5. 实现 TIME 编辑输入校验及参数绑定，拒绝越界而非截断；普通时钟类型的其他数据库行为保持一致。

**Run:** `cargo test --locked --test mariadb_values time -- --nocapture --test-threads=1`。

**Expected:** 所有合法时长无损往返；越界输入得到明确错误。

**Commit:** `fix(mysql): preserve signed and extended TIME values`。

### T05：核心类型与结果集完整性矩阵

**Files:** 修改 `src/db/mysql/value.rs`、`src/db/mysql.rs`；测试 `tests/mariadb_values.rs`、`tests/mariadb_adapter.rs`。

1. 分别创建 DECIMAL(65,30)、BIGINT UNSIGNED 最大值、BIT(1/64)、SET、ENUM、JSON、BLOB、utf8mb4、DATE/DATETIME(6)/TIMESTAMP(6) fixture。
2. 增加受版本控制的 UUID、INET6 与空间类型 fixture，记录“原生值/文本/字节/明确 unsupported”的目标表示。
3. DECIMAL 使用无损十进制字符串或既有精确值表示，禁止经 f64；二进制保持原始字节；JSON 保留原文及 MariaDB 别名语义。
4. 零日期与非法编码使用明确、可复制的表示策略，不把它们静默变成 NULL。
5. 覆盖 SELECT 0 rows、多结果集中的空集合、混合 DDL/DML/SELECT；如 raw stream 不提供空列元数据，依据锁定 SQLx 接口补齐元数据收集，不额外执行原查询。
6. 将类型支持与写入能力分别列入测试表；不能安全绑定的类型仍只读。

**Run:** `cargo test --locked --test mariadb_values --test mariadb_adapter -- --nocapture --test-threads=1`。

**Expected:** 精度和字节不变，空结果保留列名/类型；未认证类型显式标明。

**Commit:** `test(mariadb): certify value and result-set fidelity`；实际转换修复按类型单独 `fix(mysql): ...`。

### T06：事务、取消、会话状态与 DDL 提交语义

**Files:** 新增 `tests/mariadb_transactions.rs`；修改 `src/db/mysql.rs`、`src/db/transaction.rs`、`src/runtime/transaction.rs`、`src/sql/transaction.rs`，必要时修改 `src/model/transaction.rs`（执行前确认实际模型路径）。

1. 仿照现有 `tests/sqlserver_transactions.rs` 编写 MariaDB 实机事务测试，不复制驱动语法。
2. 验证 BEGIN→INSERT→ROLLBACK、BEGIN→INSERT→COMMIT、同物理连接中的临时表及 session变量。
3. 验证长查询取消后连接状态、取消与查询结束竞态、控制连接权限不足。
4. 验证 DDL 隐式提交：数据库真实结果与事务 UI 状态一致；不展示已经无法执行的 rollback承诺。
5. 覆盖执行中断线和 commit 返回前断线；结果未知时保留 unknown outcome，重新读取前不能按失败自动重放写操作。
6. 对连接池会话状态定义恢复/废弃策略，确保 SET SESSION 不泄漏到不相关工作区。

**Run:** `cargo test --locked --test mariadb_transactions --test transaction_reducer --test transaction_sql -- --nocapture --test-threads=1`。

**Expected:** 事务状态匹配服务器，取消不误杀下一条查询，断线不重复写入。

**Commit:** `fix(mariadb): synchronize transaction and cancellation outcomes`。

### T07：验证并开放数据网格编辑

**Files:** 新增 `tests/mariadb_relation_mutations.rs`；修改 `src/db/mysql.rs`、`src/db/mysql/value.rs`、`src/db/mutation.rs`、`src/db/capabilities.rs`、`src/model/relation_edit.rs`、`src/model/cell_editor.rs`、`src/app.rs`。

1. 从 `tests/postgres_relation_mutations.rs` 参考公共行为：插入、单元格修改、删除、批量失败回滚。
2. 覆盖复合主键、修改主键、自增、NULL/DEFAULT、生成列、并发更新/删除、影响行数为 0/多行。
3. 检查现有 `editable_capability` 的生成列与 `timestamp` 判定；不能让跨驱动规则把 MariaDB 普通 timestamp 列误判成 SQL Server rowversion。
4. 首阶段按列排除自动生成/不可写列；若已有模型仅支持整表只读，先明确原因再扩展列级可写性。
5. 对事务型引擎开放已认证操作；非事务引擎不能沿用整批可回滚语义，首阶段给出明确不支持原因。
6. 确保乐观并发比较使用无损原始值，文本排序规则、NULL、二进制比较有明确策略。
7. 通过实机和 UI gate 测试后才将 MariaDB relation_edit 打开，MySQL 独立按其验证状态处理。

**Run:** `cargo test --locked --test mariadb_relation_mutations --test object_mutation_contract -- --nocapture --test-threads=1`。

**Expected:** 网格入口真实可用，失败不误报成功，并发冲突不覆盖他人数据。

**Commit:** `feat(mariadb): enable verified relation data editing`。

### T08：完整定义快照与公共 mutation 模块

**Files:** 新增或复用 `src/db/mysql/mutation.rs`、`tests/mysql_catalog_mutation.rs`、`tests/mariadb_catalog_mutation.rs`；修改 `src/db/mysql.rs`、`src/db/catalog_mutation.rs`、`src/model/catalog_editor.rs`、`src/db/mod.rs`。

1. 将已有 table/view plan 与 definition 读取移入模块，运行原有契约测试证明行为不变。
2. 定义快照覆盖 engine、charset、collation、comment、columns、generated/default、indexes、constraints、view options。
3. 读取 information_schema 的结构化数据并保存 SHOW CREATE 原文；二者各司其职，不用脆弱的字符串替换编辑 DDL。
4. 测试只改表名/一列时，未修改属性保持；无法理解的原生属性保留并使相关重建操作不可用。
5. 指纹包含计划实际依赖的属性，基线变化时拒绝陈旧计划；不能使用展示文本裁剪结果作指纹。

**Run:** `cargo test --locked --test object_mutation_contract --test mysql_catalog_mutation --test mariadb_catalog_mutation -- --nocapture --test-threads=1`。

**Expected:** 定义读取可支撑后续编辑；模块拆分本身不扩大能力广告。

**Commit:** `refactor(mysql): centralize authoritative mutation definitions`。

### T09：表列、表属性与视图编辑

**Files:** 修改 `src/db/mysql/mutation.rs`、`src/db/catalog_mutation.rs`、`src/model/catalog_editor.rs`、`src/ui/catalog_editor.rs`；测试 `tests/mariadb_catalog_mutation.rs`、`tests/mysql_catalog_mutation.rs`、`tests/catalog_mutation.rs`。

1. 逐项实现列新增/改名/类型/默认值/nullable/删除，每项先添加 create→load→edit→reload测试。
2. 覆盖 auto_increment、生成表达式、列位置、列注释；只生成目标差异的 ALTER，不重建整张表覆盖未知属性。
3. 实现表注释、引擎、字符集/排序规则的明确操作；区分“修改默认字符集”与“转换现有列数据”。
4. 视图修改保留 ALGORITHM、DEFINER、SQL SECURITY、CHECK OPTION；名称变化后的 plan绑定新 identity。
5. 每步执行结果记录到 mutation outcome；部分成功后按实际状态刷新，不继续使用旧 fingerprint。
6. UI 只显示已经实现的字段，name 未变但 column 变化不再返回 NoChanges。

**Run:** `cargo test --locked --test mariadb_catalog_mutation --test mysql_catalog_mutation --test catalog_mutation -- --nocapture --test-threads=1`。

**Expected:** 修改一项后其他定义保持；同名结构编辑生效；隐式提交结果准确。

**Commit:** 按 columns/table-options/views 分别 `feat(mariadb): ...`。

### T10：索引与约束的读取和编辑闭环

**Files:** 修改 `src/db/mysql.rs`、`src/db/mysql/mutation.rs`、`src/db/catalog.rs`、`src/db/catalog_mutation.rs`、`src/model/catalog_editor.rs`、`src/ui/catalog_editor.rs`；测试 `tests/mariadb_adapter.rs`、`tests/mariadb_catalog_mutation.rs`、`tests/mysql_catalog_mutation.rs`。

1. 新增 CHECK 读取，保留约束名和表达式，接入 relation children、搜索、详情与补全。
2. 索引快照覆盖列顺序、前缀长度、方向、类型、唯一性，以及服务器确实支持的可见性/ignored属性。
3. 不将 MySQL functional index 语法直接移植到 MariaDB；按版本验证生成列索引及其他实际支持方式。
4. 实现主键/unique/index/FK/CHECK 创建与删除；变更若需 drop+create，计划明确为多步骤且保持完整定义。
5. FK 覆盖复合列顺序、引用目标、ON UPDATE/DELETE；正确区分 unique constraint 与其底层 index，防止重复删除。
6. 验证子节点刷新、rename后的缓存淘汰、权限不足时旧快照保留为 stale。

**Run:** `cargo test --locked --test mariadb_adapter --test mariadb_catalog_mutation --test mysql_catalog_mutation -- --nocapture --test-threads=1`。

**Expected:** CHECK 不只出现在 DDL 文本中；索引修改不丢前缀或顺序。

**Commit:** 按 metadata/indexes/constraints 分别 `feat(mariadb): ...`。

### T11：Sequence 目录、搜索与 DDL

**Files:** 新增 `src/db/mysql/mariadb.rs`、`tests/mariadb_sequences.rs`；修改 `src/db/mysql.rs`、`src/db/mysql/capabilities.rs`、`src/db/mod.rs`、`src/sql/completion.rs`。

1. 创建 sequence fixture；确认目标版本如何通过目录辨识 sequence，查阅目标版本而不是直接依赖最新版 information_schema.SEQUENCES。
2. 使用现有 CatalogKind::Sequence/ObjectGroup::Sequences；建立稳定 identity、scope规则和 keyset排序。
3. 对 tables 与 sequences做正确分类，防止同一对象双重出现。
4. 接入目录分页、搜索、详情、SHOW CREATE SEQUENCE；序列元数据读取不得调用 NEXT VALUE，避免浏览消耗值。
5. 增加跨数据库scope、同名对象、分页、drop后刷新和补全用例。

**Run:** `cargo test --locked --test mariadb_sequences --test sql_completion -- --nocapture --test-threads=1`。

**Expected:** 序列可浏览/搜索/查看DDL，刷新不改变下一值。

**Commit:** `feat(mariadb): expose sequence catalog and DDL`。

### T12：Sequence 创建、修改和删除

**Files:** 修改 `src/db/mysql/mariadb.rs`、`src/db/mysql/mutation.rs`、`src/db/mysql/capabilities.rs`、`src/model/catalog_editor.rs`、`src/ui/catalog_editor.rs`；测试 `tests/mariadb_sequences.rs`、`tests/mariadb_catalog_mutation.rs`。

1. 接入现有 sequence draft，覆盖 START/INCREMENT/MIN/MAX/CACHE/CYCLE。
2. 分开表示静态参数、当前运行状态和 restart操作，不能编辑 comment 时顺便重置计数器。
3. 实现 create→load→alter→reload→drop；覆盖负increment、边界、cache与非法组合。
4. 读定义与执行操作保留独立权限结果；只在服务器与实现均支持时开放菜单。

**Run:** `cargo test --locked --test mariadb_sequences --test mariadb_catalog_mutation -- --nocapture --test-threads=1`。

**Expected:** 原生参数正确往返，普通编辑不意外消耗/重置值。

**Commit:** `feat(mariadb): support sequence mutations`。

### T13：MariaDB SQL 方言上下文

**Files:** 新增 `src/sql/mariadb.rs`、`tests/mariadb_sql.rs`；修改 `src/sql/dialect.rs`、`src/sql/mod.rs`、`src/sql/builtins.rs`、`src/sql/type_name.rs`、`src/sql/completion.rs`、`src/app.rs`、`src/lsp/server.rs`、`src/lsp/completion.rs`。

1. 增加独立 SqlDialect::MariaDb；先在 MySQL 通用路径复用 tokenizer/quoting，再显式覆盖差异。
2. 不假定 sqlparser 存在可直接使用的 MariaDbDialect；检查锁定版本并封装项目自己的选择/扩展逻辑。
3. 添加 SQL上下文，包含已知服务器版本与当前有效模式；离线时使用明确的默认支持集合。
4. 增加 sequence、RETURNING、UUID/INET6 等已支持版本的关键字/类型/表达式补全。
5. TUI 与 LSP 使用同一个上下文与补全表；替换 app 中手写 MySQL/MariaDB合并映射。
6. SQL_MODE 只取自有效执行会话；无法确定模式时避免确定性误诊，显示上下文未知而不是沿用另一个连接的状态。

**Run:** `cargo test --locked --test mariadb_sql --test sql_dialect_mapping --test sql_completion --test sql_diagnostics`。

**Expected:** MariaDB 有独立语义入口，MySQL补全不被 MariaDB 原生词表污染，TUI/LSP一致。

**Commit:** `feat(sql): add MariaDB dialect context`。

### T14：原生语句切分、只读判定与分页

**Files:** 修改 `src/sql/mariadb.rs`、`src/sql/batch.rs`、`src/sql/execution.rs`、`src/sql/analysis.rs`、`src/sql/diagnostics.rs`、`src/sql/derived_result.rs`、`src/sql/risk.rs`、`src/sql/catalog_change.rs`；测试 `tests/mariadb_sql.rs`、`tests/sql_batch.rs`、`tests/sql_execution.rs`、`tests/sql_risk.rs`、`tests/mariadb_adapter.rs`。

1. 表驱动覆盖 INSERT/DELETE/REPLACE RETURNING、WITH、FOR SYSTEM_TIME、sequence表达式。
2. 对 RETURNING明确标记为写语句结果，禁止当成 SELECT做 count/paging包裹。
3. 实现 DELIMITER 客户端指令处理：仅指令位置改变分隔符，不发送指令给服务器；字符串、注释与过程体内的分号不误切。
4. 增加 SQL_MODE=ANSI_QUOTES/NO_BACKSLASH_ESCAPES边界；ORACLE模式先明确支持集，不能宣称兼容完整 PL/SQL。
5. 未解析成功的复杂合法SQL可原样执行；只读/变更分类采用未知状态，不把“解析失败”当成无副作用。
6. 特别覆盖 SELECT NEXT VALUE FOR：语法是查询也会改变序列状态，不应由自动预览/count重复执行。
7. 目录变更识别覆盖 sequence/event/routine，成功后刷新受影响scope。

**Run:** `cargo test --locked --test mariadb_sql --test sql_batch --test sql_execution --test sql_risk --test mariadb_adapter -- --nocapture --test-threads=1`。

**Expected:** 不误切过程、不包裹写语句、不因分页额外消耗序列值。

**Commit:** `fix(sql): preserve MariaDB native statement semantics`。

### T15：Routine、Trigger 与 Event 管理

**Files:** 修改 `src/db/mysql/mariadb.rs`、`src/db/mysql/mutation.rs`、`src/db/catalog.rs`、`src/db/catalog_mutation.rs`、`src/model/catalog_editor.rs`、`src/ui/catalog_editor.rs`、`src/ui/icons.rs`、`src/sql/completion.rs`；新增 `tests/mariadb_programmable_objects.rs`。

1. 补齐过程/函数/触发器完整原生定义读取及编辑基线。
2. Event 在当前模型无相应kind/group时新增，并更新序列化、树、搜索、图标和穷尽match；老快照仍可读。
3. 实现各对象create/load/edit/reload/drop，保留definer、SQL security、sql_mode、字符集等定义环境。
4. Event保留schedule、时区、状态、on completion；测试fixture默认禁用，避免依靠等待调度触发验证定义。
5. 需要drop+create的操作精确记录部分成功，创建失败后不能把旧对象仍存在当作事实。

**Run:** `cargo test --locked --test mariadb_programmable_objects --test mariadb_sql -- --nocapture --test-threads=1`。

**Expected:** 复合主体完整往返，事件时区和状态保留，失败结果可刷新恢复。

**Commit:** 按 routines/triggers/events分别 `feat(mariadb): ...`。

### T16：数据库、用户和角色

**Files:** 修改 `src/db/mysql/mutation.rs`、`src/db/mysql/mariadb.rs`、`src/db/catalog_mutation.rs`、`src/model/catalog_editor.rs`、`src/ui/catalog_editor.rs`；新增 `tests/mariadb_principals.rs`；测试 `tests/mariadb_catalog_mutation.rs`。

1. 复用已有数据库/主体表单；实现database创建与默认字符集、排序规则编辑，不虚构通用database rename。
2. 明确user identity为user+host，角色按MariaDB原生标识建模；两个同名不同host用户不能合并。
3. 实现用户/角色读取、创建、可变属性、角色授予/撤销与删除，按服务器版本覆盖默认角色语义。
4. 密码沿用现有secret执行路径，不进入可见SQL预览、历史或日志；补充泄漏回归。
5. root创建受限fixture账户，用其测试可见范围与拒绝结果；恢复/清理角色和用户。

**Run:** `cargo test --locked --test mariadb_principals --test mariadb_catalog_mutation -- --nocapture --test-threads=1`。

**Expected:** 命名与权限范围准确，数据库属性不隐式转换已有列，凭据不出现在历史。

**Commit:** 按 databases/principals分别 `feat(mariadb): ...`。

### T17：系统版本表识别与保真编辑

**Files:** 修改 `src/db/mysql/mariadb.rs`、`src/db/mysql/mutation.rs`、`src/db/catalog_mutation.rs`、`src/sql/mariadb.rs`、`src/ui/catalog_editor.rs`；新增 `tests/mariadb_temporal.rs`。

1. 创建system-versioned table，写入并修改数据，验证当前/历史查询。
2. 读取period列、system versioning属性及原生定义，树和详情给出明确标识。
3. 表编辑保留这些属性；未支持的修改返回具体原因，禁止退化为普通表。
4. 为历史预览构造明确的AS OF范围，结果按历史快照只读；普通Data页仍显示当前数据。
5. 测试执行权限、历史删除/保留语义、版本差异，逐项扩展而非一次宣称所有temporal功能可用。

**Run:** `cargo test --locked --test mariadb_temporal --test mariadb_sql -- --nocapture --test-threads=1`。

**Expected:** 修改定义不丢历史语义，历史结果不能网格写回。

**Commit:** `feat(mariadb): preserve system-versioned table semantics`。

### T18：连接与认证配置专项

**Files:** 修改 `src/profile.rs`、`src/model/profile_manager.rs`、`src/ui/profiles.rs`、`src/persistence/profiles.rs`、`src/db/mysql.rs`；新增 `tests/mariadb_connection.rs`；测试 `tests/profile_url.rs`、`tests/profile_compatibility.rs`、`tests/profile_draft.rs`。

1. 先列当前SQLx支持的认证插件和TLS接口，以实机probe确定支持表，不承诺底层驱动没有的认证。
2. 给MySQL/MariaDB连接增加项目当前缺失的CA/客户端证书/私钥路径与Unix socket设置；兼容现有profile反序列化默认值。
3. URL支持明确白名单参数；unknown参数按既有策略报错或保留提示，禁止看似接受却完全忽略。
4. 验证SSL disabled/prefer/required/verify-ca/verify-full、错误主机名、自签名CA、双向TLS。
5. 验证socket与TCP优先级、字符集协商、连接超时与断线重连；认证不支持给出插件名称和原因。
6. 检查配置、日志与URL展示脱敏，测试字段编辑/URL往返/持久化兼容性。

**Run:** `cargo test --locked --test mariadb_connection --test profile_url --test profile_compatibility --test profile_draft -- --nocapture --test-threads=1`。

**Expected:** 连接选项真实传递到底层，证书校验等级准确，旧profile可加载。

**Commit:** `feat(mariadb): complete verified connection options`。

### T19：目录和监控性能、故障恢复

**Files:** 修改 `src/db/mysql.rs`、`src/db/monitor.rs`、`src/runtime.rs`；新增 `tests/mariadb_monitoring.rs`、`tests/mariadb_performance.rs`；测试 `tests/explorer_performance.rs`。

1. 测量基线：目录首屏与翻页查询数、完整catalog装载耗时、每表索引读取的额外probe数。
2. 构造1,000表/多索引fixture，验证只加载请求页，搜索limit发生在服务端。
3. 优化 `SHOW FULL PROCESSLIST` 客户端截断路径：若版本/权限允许，用可限制的原生目录查询；回退路径明确仅客户端有界。
4. 监控计数器缺失、服务器重启、权限不足、采样超时不得伪造0速率或完整可见性。
5. 测试取消目录请求、断线重连、旧epoch响应、外部drop/rename后刷新，不覆盖新连接状态。
6. 性能验收使用查询数量/内存增长/有界行数等稳定指标；运行时间只记录基线与比较，不设依赖CI机器的脆弱毫秒阈值。

**Run:** `cargo test --locked --test mariadb_monitoring --test explorer_performance -- --nocapture --test-threads=1`；大型服务fixture使用 `cargo test --locked --test mariadb_performance -- --ignored --nocapture --test-threads=1`。

**Expected:** 稳态每表读取不重复探测版本，目录/结果/监控资源边界有证据，故障后缓存不会串连接。

**Commit:** `perf(mariadb): bound metadata and monitoring work`。

### T20：版本矩阵、文档和发布验收

**Files:** 修改 `.github/workflows/ci.yml`、`README.md`、`docs/database-capabilities.md`、`docs/architecture.md`、`docs/configuration.md`；新增 `docs/mariadb-support.md`。

1. PR矩阵至少保留MariaDB 11.4和MySQL 8.4；夜间/手动矩阵增加10.5、10.11及实施时选定的最新稳定版。
2. 10.5作为当前声明最低兼容版本验证，不等同于推荐部署版本或官方维护状态；如无法通过，修复或明确调整支持门槛。
3. 固定具体镜像版本/摘要，禁止浮动latest代替已验证版本；新版本进入支持表前记录全部suite结果。
4. 所有MariaDB专属suite接入数据库job；TLS/受限账户/大型fixture用单独专项job。
5. 文档逐项列出支持版本、读/写能力、原生类型表示、SQL_MODE、认证方式、实际验证日期与限制。
6. 修正README旧MariaDB排除声明、CI job名称和“表编辑”等过宽表述；链接到统一能力表。
7. 在可构建且无并行变更的验证基线运行下列最终检查，服务用例按任务范围显式执行。

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

专项服务命令示例（先配置对应URL）：

```bash
env LAZYDB_REQUIRE_DATABASE_TESTS=1 cargo test --locked --test mariadb_profile --test mariadb_adapter --test mariadb_values --test mariadb_transactions --test mariadb_relation_mutations --test mariadb_catalog_mutation --test mariadb_sequences --test mariadb_programmable_objects --test mariadb_principals --test mariadb_temporal --test mariadb_monitoring -- --nocapture --test-threads=1
```

TLS suite单独使用其证书fixture环境，不能因没有证书而在认证矩阵中标成通过。MySQL共享回归使用真实MySQL URL；MariaDB复用共享suite时继续使用 `LAZYDB_TEST_MYSQL_FUNCTIONAL_INDEX=0`，直到对应测试重构为实际产品能力驱动。

**Expected:** 每个认证版本有实际执行证据；skip、blocked、failed与passed分别计数。

**Commit:** `docs(mariadb): publish verified support matrix`，CI变更单独 `ci(mariadb): certify supported server versions`。

## 4. 完成标准与证据模板

每个任务维护以下记录，放在PR或本文执行记录区：

| 字段 | 内容 |
| --- | --- |
| 任务 | T编号、提交SHA |
| 验证基线 | Rust版本、代码SHA、服务镜像摘要 |
| 服务信息 | 实际产品/版本、sql_mode、引擎、字符集；不记录凭据 |
| 执行 | 精确命令与suite |
| 结果 | passed / failed / skipped / blocked及原因 |
| 往返证据 | 操作前定义、变更目标、操作后定义与未变属性断言 |
| UI契约 | 菜单能力、错误反馈、目录和补全刷新 |
| 遗留 | 未支持版本/类型/模式及对应后续任务 |

最终完成必须同时满足：

- 连接识别、版本门槛与能力表一致。
- 已广告编辑操作均有真实数据库往返测试及失败路径验证。
- Sequence/CHECK等对象能在目录、搜索、DDL、操作、刷新间完整流转。
- TIME、DECIMAL、二进制等关键类型无损；特殊类型的展示与可写范围明确。
- MariaDB SQL不因MySQL解析假设被误切、误判或错误分页。
- 事务/DDL/取消/断线后的状态符合服务器真实结果。
- 支持矩阵中的每个版本均实测，缺少环境不计通过。
- MySQL及共享UI/SQL模型回归通过。

## 5. 建议排期

按一名熟悉Rust和数据库协议的工程师连续实施估算，包含测试与评审：

| 阶段 | 估算工作日 | 主要不确定性 |
| --- | --- | --- |
| M0 | 5–8 | SQLx时间/空结果元数据边界 |
| M1 | 12–20 | DDL保真、并发编辑、共享表单完成度 |
| M2 | 3–5 | 不同版本sequence目录来源 |
| M3 | 5–9 | parser扩展、session模式与脚本切分 |
| M4 | 12–20 | routine重建、主体权限、认证插件 |
| M5 | 4–7 | 跨版本差异和故障fixture |

总计约41–69个工作日，不包含上游SQLx/sqlparser缺陷修复等待。先完成M0后依据实测调整后续估算；若已有多数据库mutation计划落地，M1/M4可显著缩短。每个里程碑可独立合并，能力仅随通过验收的任务逐步开放。
