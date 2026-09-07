# PostgreSQL Row-Version Delete Implementation Plan

> Execution guidance: Implement task-by-task with failing regression tests before production changes. This document is a plan, not authorization to modify business data, commit, or push. If available, use the executing-plans workflow; otherwise follow the checkpoints below directly.

**Goal:** 修复 PostgreSQL 全类型表删除失败，使用主键与读取时的行版本保护删除，并完整保留分页、编辑草稿和事务原子性。

**Architecture:** 仅在 PostgreSQL 支持的普通表关系预览中获取隐藏的 xmin，通过关系专用元数据传入编辑行和删除请求；删除不再比较业务列旧值。INSERT/UPDATE 返回最新行版本，但 UPDATE 保留现有单元格旧值并发语义。类型赋值与比较能力分别校验，未知类型明确拒绝，不回退到不安全 SQL。

**Tech Stack:** Rust 2024, SQLx 0.9, PostgreSQL 12+, Tokio, 现有关系预览与事务 worker，环境变量控制的 PostgreSQL 集成测试。

---

## 1. 事实、范围与设计约束

### 已确认事实

- 只读检查目标为 lssc-uat / moss_biz / test_schema.all_types_test，服务端 PostgreSQL 17.6。
- 表为普通非分区表，24 个业务列，主键 id bigint。实施前仍需检查 pg_inherits，不能仅凭 relkind=r 判断没有继承关系。
- 复杂列为 duration interval、metadata json、attributes jsonb、tags text[]、ip_address inet、location point；exact_num 为 numeric(12,2)。
- postgres_delete_sql 当前生成主键条件及全部旧值条件，本表每行 25 个参数。
- interval 解码为 CellValue::Text，bind_cell 将其绑定为 text，postgres_placeholder 未提供 interval 转换。
- 只读 SELECT 已复现 interval=text、json=json、point=point 运算符错误；interval、text[]、inet 的简单显式转换比较已通过。这不是完整往返验证。
- 当前 numeric typmod 测试通过，INSERT/UPDATE 已使用 postgres_placeholder，UPDATE 已使用 RETURNING *。不要重复实施旧计划中已经完成的工作。

### 本计划与旧计划的关系

- 本计划替代 docs/plans/2026-09-07-postgres-grid-writeback-implementation.md 中 PostgreSQL 删除保留全列旧值比较的设计。
- 不覆盖或删除旧计划；其中 decimal 精度、固定白名单、事务原子性、错误脱敏等约束继续有效。
- 本次只创建计划文件，不修改应用源码或实际表结构。

### 首期支持边界

- 新删除策略限 PostgreSQL 普通表，且不是分区表、分区叶子、继承父表或继承子表；要求有效完整主键。
- 视图、物化视图、外部表及上述复杂关系继续可读。若原来可写，本次明确限制其网格删除并记录行为变化；绝不能偷偷退回仅主键删除。
- 不改变 SQLite/MySQL/SQL Server 的 SQL、绑定方式和并发语义；共享结构变化只做必要适配。
- 不改变任意 SQL 控制台查询、MCP 查询结果、CSV 导出格式、ResultSet 的通用序列化形状。
- 不持久化 xmin，不将其用作业务主键或时间顺序。它是 32 位事务标识，不是永久唯一版本号，也不是同一事务内每次更新都不同的计数器。
- 版本只在当前在线预览/编辑上下文内有效。重连、工作区恢复、连接代次变化和结构失效后必须重新加载；长期未刷新的草稿不承诺跨 XID 回卷的冲突检测，文档明确这一限制。
- 不引入 xmin 之外的哈希、ctid 定位、长期事务快照、后台锁行或自动重试。
- 不把 UPDATE 改成整行版本锁。本期不顺便解决复合主键多列连续修改等已有问题，但必须保证新增版本元数据不恶化它们。
- 所有数据库写入测试只能使用明确配置的可丢弃测试库，禁止使用 lssc-uat 或 moss-test 作为 fixture 写入目标。

## 2. 目标数据流和 SQL 合约

```text
PostgresAdapter::preview_relation
  -> RelationPreview（业务结果 + 独立、等长的行版本元数据）
  -> 当前 RelationRequest / ConnectionIdentity 校验
  -> RelationEditSession / EditableRow（行 ID + 业务值 + 版本）
  -> DeleteRowMutation（行 ID + 主键 locator + 版本）
  -> PostgreSQL 同连接事务 worker
  -> 主键 + xmin DELETE
  -> 每行恰好影响一行 / 整批失败回滚
```

预览的逻辑 SQL：

```sql
SELECT "all_types_test".*, "all_types_test".xmin::text AS "__lazydb_row_version"
FROM "test_schema"."all_types_test"
LIMIT 501 OFFSET 0;
```

别名不是公共协议；解码必须按已知附加位置识别隐藏列，而不是按名称删除列。真实表允许存在同名业务列。避免引入会破坏已有 WHERE/ORDER BY 中表名引用的新表别名。

目标删除 SQL：

```sql
DELETE FROM "test_schema"."all_types_test"
WHERE "id" = $1::bigint
  AND xmin = $2::xid;
```

目标写入返回形式：

```sql
UPDATE "test_schema"."all_types_test"
SET "short_text" = $1::varchar
WHERE "id" IS NOT DISTINCT FROM $2::bigint
  AND "short_text" IS NOT DISTINCT FROM $3::varchar
RETURNING *, xmin::text AS "__lazydb_row_version";
```

推荐最小共享结构变化，具体命名在实施时统一确认：

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RowVersion {
    PostgresXmin(u32),
}
```

- RelationPreview 增加关系专用版本侧数据，推荐 Option<Vec<RowVersion>>；None 表示未提供该能力，Some 必须与保留业务行等长，空表为 Some(vec![])。
- EditableRow 增加 Option<RowVersion>；原有 from_rows 用于无版本适配器，增加有长度校验的带版本构造入口。
- DeleteRowMutation 增加 EditableRowId 与 Option<RowVersion>，保留 original 供其他数据库和历史流程使用；PostgreSQL 删除不再绑定 original。
- MutationResult::Updated/Inserted 增加 Option<RowVersion>。这两处字段扩展是实际跨提交版本传递所必需，不是未来扩展设计。
- 不给所有数据库增加一套通用版本能力注册框架，不修改 CellValue 为数据库专有枚举。

## Task 1: 建立真实回归 Fixture 与基线

**Files:**
- Create: tests/postgres_relation_mutations.rs
- Reference: tests/postgres_adapter.rs
- Test: src/db/postgres.rs 的现有 tests 模块

1. 运行 cargo test --lib postgres_placeholder，记录现有通过结果，不把它当作删除回归覆盖。
2. 建立唯一 schema 名的 fixture，业务列名与上述 24 列表一致；使用固定、自造数据，不能复制真实业务行。
3. 至少创建四行，复杂列均有非 NULL 样例；另补 NULL 样例。通过真实 Catalog/DDL 和 preview_relation 得到元数据与旧行，不手工伪造适配器结果。
4. 编写 delete_two_all_types_rows 回归，经真实事务后端提交两个删除；在修复前记录 interval/text 错误，确保测试不会被 numeric 或空值提前掩盖。
5. 清理只属于此 fixture 的 schema，包括测试失败路径；避免在 Drop 中启动无法等待的异步清理。
6. 测试遵守 LAZYDB_TEST_POSTGRES_URL；未设置时明确输出 skip 原因，不输出 URL，不自动连接用户配置。

**Run:** cargo test --test postgres_relation_mutations delete_two_all_types_rows -- --nocapture

**Expected before fix:** 在可丢弃 PostgreSQL 环境中稳定失败，数据库原始行保留；没有环境则明确 skipped，不能声称复现通过。

## Task 2: 引入关系专用版本元数据与稳定行身份

**Files:**
- Modify: src/db/mod.rs（RelationPreview）
- Modify: src/db/mutation.rs（RowVersion、DeleteRowMutation、MutationResult）
- Modify: src/model/relation_edit.rs（EditableRow 与构造、历史）
- Adapt: src/db/postgres.rs, src/db/mysql.rs, src/db/sqlite.rs, src/db/mssql.rs
- Adapt/test: src/app.rs, tests/sqlserver_transactions.rs, tests/relation_runtime.rs, tests/relation_tabs.rs

1. 添加失败测试：带版本构造保持行顺序和 ID；长度不等报明确错误，禁止 zip 静默截断。
2. 添加失败测试：undo/redo、discard_changes 和事务 snapshot clone 保留原行版本；新增/粘贴行没有版本，yank 只复制业务值。
3. 添加上述最小类型字段，其他适配器显式返回 None，现有无版本构造继续服务其他数据库。
4. 将 delete 请求的 UI 身份改为每行的 EditableRowId；删除成功/失败状态更新不再通过 original 全行相等搜索。
5. 修复删除 locator 在 src/app.rs 中没有使用 result_indexes 的列映射不一致；更新与删除统一按元数据索引映射业务列。
6. 逐个修复构造点编译错误，不添加序列化默认值来掩盖遗漏；这些字段不属于已持久化数据。

**Run:** cargo test --lib model::relation_edit

**Run:** cargo check --all-targets

**Acceptance:** 版本随行身份而不是屏幕行号移动；业务列结构不变，其他适配器仍编译。

## Task 3: 预览读取与分页版本对齐

**Files:**
- Modify/test: src/db/postgres.rs（verify_relation、preview_relation、受控结果解码）
- Modify: src/app.rs（RelationSnapshot::Preview 安装路径，当前约 src/app.rs:15621）
- Reference: src/model/relation.rs（RelationRequest、OwnedSnapshot、provenance）
- Test: tests/postgres_relation_mutations.rs, tests/relation_runtime.rs, tests/relation_tabs.rs

1. 先测试能力探测：普通独立表可获得版本；分区父/子、继承父/子和视图不执行 xmin 投影。
2. 在已有关系身份检查附近读取 relkind、relispartition，并检查 pg_inherits 的父、子关系；不要依赖已经被归一化为 table 的 native_kind 字符串。
3. 支持的表在原预览 SELECT 末尾附加 xmin::text；非支持关系保持原预览。不要单独按主键查版本。
4. 按固定末列位置解码版本为 u32。格式不合法或列数不符应失败，不能将损坏元数据当成 None。
5. 业务 columns 和 rows 显式排除最后的内部列；空表仍保留完整业务列 metadata。
6. 先按 page.size 对原始数据库行截断，再同时提取业务值和版本；测试 0、1、500、501 行边界。
7. resolve_total 的 COUNT 查询继续使用无内部投影的业务查询；保留 WHERE、ORDER BY、offset、lookahead 和实际 SQL 日志语义。
8. 安装预览时使用现有请求新鲜度检查，将对应版本传入编辑会话。过期响应不能覆盖新页版本。
9. 测试业务列真的名叫 __lazydb_row_version 时仍显示完整，没有按名字误删；有表名限定的过滤条件继续有效。
10. 测试断线/连接代次变化/工作区恢复后的旧快照不能产生有效版本删除请求。

**Run:** cargo test --test relation_runtime

**Run:** cargo test --test relation_tabs

**Run:** cargo test --test postgres_relation_mutations preview -- --nocapture

**Acceptance:** 24 列仍显示、复制、导出为 24 列；一页 N 行严格对应 N 个版本，隐藏元数据不改变列宽和排序索引。

## Task 4: 改造 PostgreSQL 删除 SQL 与提交预检

**Files:**
- Modify/test: src/db/postgres.rs（postgres_delete_sql、DeleteRows 分支）
- Modify/test: src/app.rs（保存队列构造与预检）
- Modify if needed: src/db/mutation.rs（操作限制原因）
- Test: tests/postgres_relation_mutations.rs

1. 先写 SQL 单测：bigint 主键 + xmin 恰好两个参数；复合主键为 K+1 个参数；SQL 不含 duration、metadata、location 或其他非键列。
2. 添加 malformed 请求测试：空主键、顺序错误、重复/越界列、参数数量错误、缺少/错误种类版本，必须在 DML 前失败。
3. 保留原 original 快照供 UI/其他适配器使用，PostgreSQL 不再根据其列数或可绑定性限制删除。
4. 所有主键列必须与 metadata.primary_key 完全一致；类型转换沿用受控白名单，不接受原始 native_type 字符串拼接。
5. 版本绑定选择 text + $n::xid，u32 先格式化为十进制；不要将 u32 强转 i32，覆盖大于 i32::MAX 的编号。
6. 首期仍逐行执行 DELETE，不增加批量 SQL；每行必须影响恰好一行。零行报告冲突类别，提示可能已修改、删除或不可见，不能谎称已证明是外部更新。
7. 大于一行是 locator/完整性错误而非普通并发冲突，终止队列并回滚。
8. 保存开始前扫描整批 PostgreSQL 删除：版本/能力不满足则不启动任何先行 UPDATE/INSERT；后端再次验证防止内部请求绕过。
9. 不为不支持的关系回退旧全行 SQL，也不无声回退仅主键 SQL。缺版本时提示刷新数据，不把已有草稿自动丢弃。

**Run:** cargo test --lib delete_sql

**Run:** cargo test --test postgres_relation_mutations delete -- --nocapture

**Acceptance:** 原始两行全类型删除回归通过；并发修改 json/point 等任意非主键列后删除被拒绝；真实数据库修改发生在 fixture 内。

## Task 5: INSERT/UPDATE 后返回并保存最新版本

**Files:**
- Modify/test: src/db/postgres.rs（INSERT/UPDATE RETURNING 与解码）
- Modify/test: src/app.rs（relation_mutation_result）
- Modify/test: src/model/relation_edit.rs（mark_inserted、提交及历史状态）
- Test: tests/postgres_relation_mutations.rs

1. 先写失败测试：更新后提交，再删除同一行成功；插入后提交，再删除新行成功。
2. 支持版本的表在 INSERT/UPDATE RETURNING * 后附加 xmin::text，复用预览的受控解码方法，业务结果不包含内部列。
3. MutationResult 将版本带回 app，与返回业务行同时替换原始/当前行状态；本地编辑不提前改变版本。
4. 保持 UPDATE WHERE 为主键 + 当前单元格旧值，不加入 xmin 条件。不能以此任务为由收紧不同列并发编辑语义。
5. UPDATE 使用能验证返回数量的读取方式，零行冲突，多行完整性错误；避免 fetch_optional 悄悄接受多行。
6. 测试同一行两个普通单元格顺序更新，只保留最后返回版本；测试两次独立提交之间版本同步。
7. 覆盖普通主键更改、DEFAULT、BEFORE 触发器更改返回值。检查现有 mutation undo/redo 路径，任何返回行替换都必须同步版本。
8. 不假定同一事务每次更新 xmin 都不同；测试断言围绕后续操作正确性，而不是版本必定递增。
9. 明确 AFTER 触发器、跨行触发器再次更新可能使客户端行数据过期。版本不匹配时拒绝删除并刷新，不在提交前重新取 xmin 来绕过冲突；不声称解决所有触发器快照同步问题。

**Run:** cargo test --test postgres_relation_mutations returning -- --nocapture

**Run:** cargo test --lib relation_mutation

**Acceptance:** 用户自己的上一笔提交不会导致下一笔删除错误使用旧版本；外部提交仍能被检测。

## Task 6: 类型赋值与比较能力分离

**Files:**
- Modify/test: src/db/postgres.rs（postgres_placeholder、INSERT/UPDATE 与 locator 预检）
- Test: tests/postgres_relation_mutations.rs

1. 给现有 helper 增加可失败的受控转换结果；以小型匹配表/枚举区分 assignment 与 equality，不创建全局类型注册器。
2. 保持 numeric 不经 f64，typmod 归一化不把比较参数强制舍入到列精度；严格解析限定的内建类型名和修饰符。
3. 拒绝未知类型、畸形类型文本及不在白名单的数组/自定义类型；不要把 enum/domain/引用类型名误归一化为内建类型。
4. 为 interval、text[]（以及实际 decoder 已支持并有测试的文本数组类型）、inet/cidr 增加固定转换；仅在适配器往返测试通过后声明支持。
5. interval 覆盖负数、月/日/微秒及不同符号分量；数组覆盖空数组、SQL NULL、NULL 元素、文本 NULL、引号、反斜杠、分隔符；网络覆盖 IPv4/IPv6 和前缀。
6. typed NULL 与 DEFAULT 保持不同路径；DEFAULT 不占参数编号，NULL 使用明确目标转换。
7. json/point 可以分别测试赋值能力，但当前 UPDATE 旧值比较不具备支持条件时，在 SQL 前返回 UnsupportedComparison；其他支持列仍能更新，删除不受其影响。
8. 不转换 json 为 jsonb 来伪造等价语义，不把 point 的 ~= 当通用 =，不对所有业务列做 ::text。
9. 由于 UPDATE 仍按单元格旧值比较，明确记录其复杂类型编辑限制；解锁 json/point 更新是后续独立的并发语义决策。

**Run:** cargo test --lib postgres_placeholder

**Run:** cargo test --test postgres_relation_mutations type_roundtrip -- --nocapture

**Acceptance:** 删除与展示类型解耦，受支持的赋值通过真实往返；不支持的位置在提交前给出明确原因。

## Task 7: 证明原子性、回滚与草稿恢复

**Files:**
- Modify/test: src/app.rs（relation_mutation_result、relation_transaction_finished）
- Test/modify only if needed: src/runtime/transaction.rs
- Test: src/model/relation_edit.rs, tests/postgres_relation_mutations.rs

1. 两个连接测试：A 预览两行，B 更新第二行的 json 或 point 并提交，A 删除第一行后第二行匹配失败。
2. 经正常 worker/backend 路径回滚，用独立连接验证第一行仍存在，第二行保留 B 的修改。
3. 扩展为两次单元格更新 + 两行删除，后一个操作失败后，本批所有写入不持久化。
4. app 层模拟成功更新后失败，检查 pending_save 清空、不发后续 DML/COMMIT，收到回滚成功后恢复完整 transaction_snapshot。
5. snapshot 必须包含旧版本、行 ID、待编辑值、删除标记和历史；不要将成功过但已回滚的 RETURNING 版本留在草稿中。
6. 回滚失败/断线保留快照并进入明确失败或 OutcomeUnknown 状态，不能宣告成功，也不能盲目重试。
7. 回滚成功后的同一旧快照若仍与外部修改冲突，应继续冲突；刷新/重新编辑必须是可见操作，不能偷偷更新令牌。
8. 测试 metadata loading 延迟保存、过期 mutation 回调与切换连接，不让旧请求改变新会话。

**Run:** cargo test --lib relation_mutation

**Run:** cargo test --lib runtime::transaction

**Run:** cargo test --test postgres_relation_mutations atomic -- --nocapture

**Acceptance:** 数据库测试证明原子性，app 测试证明队列及草稿，worker 测试证明同连接串行。不能用其中一层代替全部。

## Task 8: 结构化诊断与用户反馈

**Files:**
- Inspect/modify: src/db/transaction.rs, src/runtime/transaction.rs
- Modify: src/db/postgres.rs, src/action.rs, src/command.rs, src/app.rs
- Inspect/modify if it owns the bridge: src/runtime.rs
- Test: src/app.rs, src/runtime/transaction.rs, tests/postgres_relation_mutations.rs

1. 先追踪 TransactionError 以及 RelationMutation 结果转换的全部消费者，确认当前字符串化的实际边界，再选最小实现。
2. 优先引入 relation-mutation 专用错误详情：category、可选 SQLSTATE、operation、qualified relation、可确定的安全列/类型上下文。
3. 不为此修改所有事务控制操作的公共错误接口；其他适配器可使用通用类别，不伪造 PostgreSQL SQLSTATE。
4. PostgreSQL 错误在 to_string 前保留 SQLSTATE，不解析英文错误消息判断类别。
5. 只有零行匹配的并发/可见性失败标为 Conflict；类型、限制、约束错误保留草稿并显示具体原因。
6. 执行失败即时通知；回滚确认后显示“本次提交未保存，草稿已恢复”。失败或未知结果明确提示，不显示已回滚。
7. 不记录 URL、密码、绑定值或未清理的服务端 DETAIL；多列语句不能猜测唯一失败列。
8. 添加错误分类、字段缺失、终端文本清理、回滚确认时序测试。

**Run:** cargo test --lib relation_mutation

**Run:** cargo test --lib runtime::transaction

**Acceptance:** 用户能够区分并发冲突、类型不支持、输入/约束失败以及未知提交结果。

## Task 9: 最终回归、文档与发布前检查

**Files:**
- Test: tests/postgres_relation_mutations.rs, tests/postgres_adapter.rs
- Test: tests/relation_runtime.rs, tests/relation_tabs.rs, tests/connection_switch.rs, tests/workspace_tabs.rs
- Update: 本计划的执行记录；如需要修改用户文档，先定位现有关系编辑文档，沿用其组织结构，不另造未使用的入口。

1. 运行 cargo fmt --check。
2. 运行 cargo test --lib。
3. 在明确配置测试库后运行 cargo test --test postgres_relation_mutations -- --nocapture。
4. 运行 cargo test --test postgres_adapter -- --nocapture；区分未配置数据库导致的跳过与真实通过。
5. 运行 cargo test；记录环境依赖和已有失败，不修无关问题。
6. 运行 cargo clippy --all-targets -- -D warnings。
7. 运行 git diff --check，审查全部差异，确认未提交凭据、未改其他适配器行为、未弱化主键/版本校验。
8. 在 PostgreSQL 12 和 17.6/可用当前版本执行核心集成矩阵；无法提供的版本明确列为未验证。
9. 手动 TUI 验证：24 列展示、分页/排序/过滤、删两行、更新后删、插入后删、并发失败草稿恢复、刷新后重新操作。
10. 记录性能合约：此表每行 DELETE 绑定 2 个参数，而非 25 个；一次预览不为每行追加数据库请求。无需为此新增基准框架。
11. 文档列出普通表边界、xmin 的短期会话限制、UPDATE 仍保留单元格语义以及 json/point 更新限制。
12. 仅经用户新授权后再 commit/push；本计划不包含自动提交步骤。

## 3. 总体验收矩阵

| 领域 | 必须覆盖 |
| --- | --- |
| 原问题 | 24 列全类型表，两行复杂值非 NULL，真实适配器删除并提交 |
| 版本读取 | 空表、分页 500/501、排序、过滤、重复内部别名、同查询原子获取 |
| 关系限制 | 普通独立表、分区父/子、继承父/子、视图、无主键 |
| 行身份 | 重排、删除批次、多行相同非键值、列映射、过期响应 |
| 版本生命周期 | undo/redo、粘贴新行、更新后删、插入后删、主键变更、回滚恢复 |
| 并发 | 修改普通列/json/point、外部删除、主键复用、值改回、权限不可见 |
| 事务 | 前项成功后项失败、整批 rollback、rollback 失败、未知结果 |
| 类型 | decimal 精度、interval 符号/微秒、数组转义/NULL、网络前缀、typed NULL、DEFAULT |
| 隔离 | 控制台/MCP/导出无内部字段，其他适配器行为不变 |
| 安全 | 固定类型转换、无值日志、不按名称剥离列、不使用用户数据库写 fixture |

## 4. 实施顺序和评审关口

1. Tasks 1-3：锁定回归并建立可靠版本传递。关口 A：隐藏列和分页不改变业务结果。
2. Tasks 4-5：实现主键 + xmin 删除及写后版本同步。关口 B：删两行、更新后删、插入后删均通过。
3. Task 7：优先证明原子性与草稿恢复。关口 C：后项失败不发生部分提交。
4. Task 6：独立完善类型赋值和比较边界，避免其延期阻塞核心删除验证。
5. Task 8：完成错误分类和回滚反馈。关口 D：类型错误不再伪装成并发冲突。
6. Task 9：全量回归和支持边界文档，完成后才可建议发布。

不要并行修改 postgres.rs、app.rs 或共享 mutation 类型。可在类型合约稳定后并行设计独立测试，但同一文件只由一个执行者修改。

## 5. 执行记录模板

| 任务 | 状态 | 实际命令/数据库版本 | 结果/跳过原因 | 剩余风险 |
| --- | --- | --- | --- | --- |
| Task 1 | pending | | | |
| Task 2 | pending | | | |
| Task 3 | pending | | | |
| Task 4 | pending | | | |
| Task 5 | pending | | | |
| Task 6 | pending | | | |
| Task 7 | pending | | | |
| Task 8 | pending | | | |
| Task 9 | pending | | | |
