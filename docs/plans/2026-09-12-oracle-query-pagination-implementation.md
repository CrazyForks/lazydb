# Oracle 查询分页与执行诊断 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 若执行环境未提供该技能，按以下任务顺序实施、验证并记录。此计划不授权提交、推送或启用子代理；这些操作须获得用户明确授权。

**Goal:** 修复 Oracle 编辑器 SELECT 被包装为错误方言 SQL 导致的 ORA-00933，同时覆盖派生筛选、分页计数、尾部注释和实际执行 SQL 诊断。

**Architecture:** 在公共 SQL 派生查询层集中生成 Oracle 子查询和分页语法，复用现有 Oracle 单语句准备函数。保留编辑器原文与执行草稿，在 Oracle 驱动错误边界补充实际提交文本；以纯 SQL 生成测试、编辑器命令测试和必须真实连接的 Oracle 集成测试分层验收。

**Tech Stack:** Rust 2024 / Rust 1.94、sqlparser 0.62、oracle 0.6.3、Tokio、现有 ExecutionDraft / PageRequest / DatabaseDiagnostic。

---

## 0. 已确认事实、假设与范围

### 已确认的代码路径

1. `src/sql/dialect.rs:20` 已把 Oracle 映射到 `SqlDialect::Oracle`。
2. `src/app.rs:13405` 将可派生 SELECT 转为 `Command::RunQueryPage`；手动模式在 `13367` 使用 `bounded_query()`。
3. `src/runtime.rs:2309、2364` 生成并执行 `query.page_sql`；末页解析总数时执行 `count_sql`。
4. `src/sql/derived_result.rs:90` 仅特判 SQL Server；Oracle 落入包含 `AS __lazydb_page ... LIMIT` 的分支。
5. 同文件 `55、81` 的筛选源包装也硬编码 `AS`；计数别名同样需要修复。
6. `src/db/oracle.rs:432` 已接入 `prepare_oracle_statement()`，本次无需重新实现分号扫描器。
7. `src/app.rs:17724` 失败日志展示 `last.draft.sql`，不能由此确认驱动收到的文本。
8. `src/db/oracle.rs:257` 的表预览已经使用 `OFFSET ... FETCH NEXT`。
9. `tests/oracle_adapter.rs` 现有查询测试绕过公共分页生成器，且缺少配置时直接返回。
10. `docs/database-capabilities.md` 尚未声明 Oracle 分页的最低版本。

行号仅用于定位；实施时重新检查最新源码和工作区。之前的 `2026-09-12-oracle-sql-execution-implementation.md` 是终止符专项计划，不能代替本计划，也不覆盖它的既有内容。

### 版本边界

- 本计划的分页生成方案以 **Oracle 12c+** 为前提，与现有表预览语法一致。
- 现场数据库版本与截图二进制对应提交仍未确认；ORA-00933 本身不能用于推断版本。
- 在现有客户端或直接适配器路径执行 `SELECT version FROM product_component_version WHERE product LIKE 'Oracle Database%'`，不要用尚未修复的编辑器分页路径取得版本。
- 若现场为 11g，记录该阻塞并先确认兼容范围：11g 需要独立的 ROWNUM 分页设计，涉及辅助列处理、偏移、排序、计数与表预览；不得把本计划交付宣称为现场故障已解决。
- 缺少现场版本不阻止纯代码修复和测试，但阻止现场验收完成。

### 本次范围

- Oracle 普通查询首页、翻页、末页计数和派生筛选/排序。
- Oracle 源 SQL 的终止符准备、尾部注释换行隔离。
- Oracle 执行失败时的提交文本诊断。
- 既有其他方言的回归、默认 feature 和无默认 feature 构建。

不扩展 Oracle PL/SQL 解析器、事务生命周期、11g 支持或公共 Agent API。特别是 Oracle 手动事务的裸 BEGIN 问题独立存在；本次只验证手动路径生成的 SQL，不以此宣称真实手动事务已修复。

## 1. 行为契约

默认页大小 500，多取 1 行判断下一页。Oracle 包装使用以字母开头的内部别名，不使用表别名 AS。

输入：

```sql
SELECT * FROM MFGSUPPORT.ACCESSORY_BINDING;
```

首页目标：

```sql
SELECT * FROM (
SELECT * FROM MFGSUPPORT.ACCESSORY_BINDING
) lazydb_page OFFSET 0 ROWS FETCH NEXT 501 ROWS ONLY
```

计数目标：

```sql
SELECT COUNT(*) FROM (
SELECT * FROM MFGSUPPORT.ACCESSORY_BINDING
) lazydb_count
```

筛选与排序目标（页大小 10，偏移 20）：

```sql
SELECT * FROM (
SELECT * FROM (
SELECT id FROM sample_data
) lazydb_result WHERE id > 1
) lazydb_page ORDER BY id DESC OFFSET 20 ROWS FETCH NEXT 11 ROWS ONLY
```

契约细节：

- 仅移除合法的客户端终止符；字符串、引号、hint、注释和用户原文保真。
- Oracle 每层源 SQL 与右括号间必须有换行，避免尾部 `--` 吞掉生成语法。
- 不自动添加不存在的稳定排序；无 ORDER BY 的分页不保证跨次执行顺序，文档明确说明。
- 原查询已有 FETCH/OFFSET 时保留在内层；外层页和 COUNT 针对原查询结果，而非移除用户限额后的全集。
- COUNT 针对完整筛选结果，不带自动生成的页偏移和 lookahead 限额。
- 继续拒绝多语句、DML、锁定查询等不能安全派生的来源；解析失败不能直接包装绕过风险校验。
- Oracle 替代引用等语法若仍不被 sqlparser 支持，保持原有不可派生行为；词法准备成功不等于解析器支持。
- 重名列、序列伪列和复杂 Oracle 查询有派生视图/row limiting 限制；本次不承诺任意合法 SELECT 均可包装，也不自动改列名改变结果契约。

## 2. Task 1：建立可重复的分页缺陷回归

**Files**
- Modify: `src/sql/derived_result.rs` 的模块内测试。
- Inspect: `src/model/pagination.rs`。

**步骤**
1. 执行 `git status --short` 和 `git diff`，识别现有用户改动；记录测试前提交及现场版本验证状态。
2. 增加以下具体测试，全部使用 `SqlDialect::Oracle`：
   - `oracle_first_page_uses_offset_fetch`：截图形式的 SQL，预期第 1 节首页文本。
   - `oracle_page_and_count_use_valid_aliases`：`PageRequest::at(PageSize::Ten, 20)`，预期 OFFSET 20 / FETCH NEXT 11，COUNT 无页限制。
   - `oracle_filtered_page_and_count_share_filter`：筛选 `id > 1`、排序 `id DESC`，校验每层别名与 COUNT 的筛选条件。
   - `oracle_derived_query_matches_first_derived_page`：两个派生入口的默认首页输出一致。
3. 执行 `cargo test --lib sql::derived_result::tests::oracle_`。
4. 记录失败应是 Oracle SQL 不匹配，实际文本含通用 LIMIT/AS；不是环境或编译错误。

**验收**：至少首页和筛选回归可以稳定暴露当前缺陷，不需要 Oracle 原生库或数据库连接。

## 3. Task 2：集中实现 Oracle 子查询与分页生成

**Files**
- Modify: `src/sql/derived_result.rs`：`build_derived_query`、`build_derived_paginated_query`、`wrap_paginated_source`，新增私有子查询包装函数。

**步骤**
1. 新增小型内部函数，复用已有 `derived_alias()`，统一处理每层子查询。建议实现：

```rust
fn derived_source(source: &str, dialect: SqlDialect, name: &str) -> String {
    match dialect {
        SqlDialect::Oracle => {
            format!("(\n{source}\n) {}", name.trim_start_matches('_'))
        }
        _ => format!("({source}) AS {}", derived_alias(dialect, name)),
    }
}
```

此函数仅接收内部固定名称 `__lazydb_result`、`__lazydb_page`、`__lazydb_count`，不是公共的任意标识符引用 API。

2. 两个派生入口均用 `derived_source(..., "__lazydb_result")` 构造 `SELECT * FROM ...`，不在调用方拼接 AS。
3. 在 `wrap_paginated_source()` 增加明确 Oracle 分支，复用现有 `lookahead_limit()` 和 offset：

```rust
if dialect == SqlDialect::Oracle {
    let page_source = derived_source(source, dialect, "__lazydb_page");
    let count_source = derived_source(source, dialect, "__lazydb_count");
    let order_by = order_by
        .map(|clause| format!(" ORDER BY {clause}"))
        .unwrap_or_default();
    return PaginatedSql {
        page_sql: format!(
            "SELECT * FROM {page_source}{order_by} OFFSET {offset} ROWS FETCH NEXT {limit} ROWS ONLY"
        ),
        count_sql: format!("SELECT COUNT(*) FROM {count_source}"),
    };
}
```

4. SQL Server 仍保留缺省排序 `(SELECT NULL)`；其他方言保持原有 SQL 输出。不要把 Oracle 放进 SQL Server 分支后再替换字符串。
5. 执行 `cargo test --lib sql::derived_result`，确认新增与既有 SQLite/Postgres/SQL Server 测试通过。

**验收**：所有 Oracle 自动生成层均没有通用 LIMIT、不合法表别名 AS 或未引用的下划线起始别名；不对用户 SQL 中合法的列别名 AS 做全局替换。

## 4. Task 3：处理终止符与注释边界

**Files**
- Modify: `src/sql/derived_result.rs`：`parse_source()` 与测试。
- Reuse / Inspect: `src/sql/oracle.rs`：`prepare_oracle_statement()`。
- Inspect: `src/sql/risk.rs`。

**步骤**
1. 添加表驱动回归：无分号、末尾分号、`; -- tail`、`; /* tail */`、无分号但尾部行注释、CRLF/中文注释、`SELECT ';' AS value FROM dual;`。
2. 增加拒绝用例：两条语句、重复分号、未闭合字符串、多语句夹注释、FOR UPDATE、非查询语句。
3. 运行 `cargo test --lib sql::derived_result::tests::oracle_`，记录终止符残留相关失败。
4. `parse_source()` 先为 Oracle 获取 `prepare_oracle_statement(source)` 的 Cow 视图，将准备错误映射到 `DerivedQueryError`；在该视图上进行 AST 单查询和 locks 校验。
5. 风险校验保持完整单语句只读要求；保留原始输入的多语句事实，不用首条 SQL 的风险替代全体。确认已有 `classify_sql` 的 Oracle 准备逻辑后选择一致输入，避免重复扫描逻辑实现。
6. Oracle 返回准备后的文本，非 Oracle 保持既有尾部分号处理；不得通过 AST `to_string()` 重写用户查询。
7. 验证外层右括号前换行可隔离所有尾部行注释；字符串内分号没有被删除，注释内字节未被重排。
8. 执行 `cargo test --lib sql::derived_result` 和 `cargo test --lib sql::oracle`。

**验收**：截图查询和带尾部注释的同类查询可生成合法提交文本；没有扩大语法解析、风险授权或 PL/SQL 支持范围。

## 5. Task 4：验证编辑器与派生执行入口

**Files**
- Modify: `tests/sql_execution.rs`。
- Inspect: `src/app.rs`：`dispatch_draft()`、分页/筛选命令生成。
- Inspect: `src/runtime.rs`：`run_query_page()`、派生查询执行。
- Inspect: `src/runtime/transaction.rs`：手动分页执行。

**步骤**
1. 按现有 `connected_app()` 模式新增 Oracle 测试 fixture，构造 Oracle profile 和 `Action::ConnectionSucceeded`，使用虚拟测试主机；不建立真实连接。
2. 添加 `oracle_editor_dispatches_page_with_original_sql`：执行 `SELECT 1 AS id FROM dual;`，断言生成 `RunQueryPage`、方言为 Oracle、source_sql 与草稿仍含原分号、默认第一页正确。
3. 用该命令的真实参数调用 `build_paginated_query()`，断言输出使用 Oracle 分页；连接编辑器方言传递与生成器，而非仅测试硬编码 Oracle 输入。
4. 添加 Oracle 翻页和末页测试，复用现有分页状态 fixture：确认 next offset、`resolve_total` 与 SQL 生成一致。
5. 添加手动模式命令级测试，验证 `ManualExecute.sql` 使用同一 Oracle 生成器；仅验证命令，不连接/验证事务生命周期。
6. 核对筛选、排序调用 `build_derived_paginated_query()`，不得在 Runtime 或 Adapter 再复制一套分页字符串修补。
7. 执行 `cargo test --test sql_execution`。

**验收**：Oracle 方言从编辑器一直传递到生成器；首页、后续分页、派生结果不发生方言丢失，原文契约不变。

## 6. Task 5：在驱动边界补充真实执行 SQL 诊断

**Files**
- Modify: `src/db/oracle.rs`：`execute_pool_with_budget()` 的 prepare/build/query/execute 原生错误映射及模块内测试。
- Reuse / Inspect: `src/db/mod.rs`：`DatabaseDiagnostic.context`、`DatabaseError::output_message()`。
- Inspect: `src/db/transaction.rs`、`src/runtime.rs` 的错误转文本路径。
- Test: `tests/sql_execution.rs` 的失败输出契约，按需要扩展。

**实现选择**

复用既有 `DatabaseDiagnostic.context`，在 Oracle 执行入口给原生 SQL 执行错误追加 `Executed SQL:` 与准备后的最终文本。避免为本次诊断新增公共错误字段或修改全部 Action 结构；若 context 已存在，则保留并追加。

**步骤**
1. 增加私有错误装饰函数测试：保留原 code/message/已有 context，新增上下文带最终 SQL，控制字符由 `sanitize_terminal_text()` 清理。
2. 在 `execute_pool_with_budget()` 中保留准备后的 `sql`，将 statement build、query、非 query execute 的原生错误通过该装饰函数映射。
3. 不把连接锁失败、本地 SQL 准备失败标注成“已执行 SQL”；不为本次增加自动重试。build 失败的标签说明这是提交给驱动的文本，并不表示语句已经成功执行。
4. 保留 Oracle 原生错误码映射；不伪造 `DatabaseErrorPosition::Internal` 的偏移来显示 SQL。
5. 确认分页与 COUNT 错误通过 `output_message()` 输出 context，COUNT 失败必须显示 count_sql 而非 page_sql。
6. 核对手动路径的 TransactionError 转换是否保留 output_message；若丢失，仅修复与该信息传递相关的转换，不改事务行为。
7. 增加日志回归：原始 SQL 仍为执行草稿，错误正文另外含提交文本；只在既有用户错误输出中展示，不新增持久化或后台 SQL 追踪。
8. 执行 `cargo test --lib db::oracle`、`cargo test --lib db::tests`、`cargo test --test sql_execution --test sql_diagnostics`。

**验收**：从错误日志能区分原查询与最终 page/count SQL；Oracle 错误 code 和已有诊断不丢失。

## 7. Task 6：真实 Oracle 集成验收

**Files**
- Modify: `tests/oracle_adapter.rs`。

**测试入口**

新增 `#[tokio::test]` + `#[ignore = "requires a configured Oracle 12c+ test database"]` 的 `oracle_query_pagination_required`。显式运行时，对三个环境变量使用 `expect`；DPI-1047、连接失败、查询失败必须让测试失败，不允许静默返回。

**步骤**
1. 使用 `LAZYDB_TEST_ORACLE_URL`、`LAZYDB_TEST_ORACLE_USER`、`LAZYDB_TEST_ORACLE_PASSWORD` 建立连接；配置由环境安全注入，测试输出不打印值。
2. 通过直接适配器查询记录数据库版本，并校验本次分页验收环境满足 12c+；不要把版本号或数据库产品补丁差异转换成跳过。
3. 使用无需创建对象的 25 行稳定数据源。为兼顾 GenericDialect 解析能力，Rust 中将 `1..=25` 映射为 `SELECT CAST({id} AS NUMBER(10,0)) AS id FROM dual`，以 ` UNION ALL ` 连接，再在末尾添加 ` ORDER BY id`。
4. 用 `build_paginated_query()` 生成 `PageSize::Ten` 的三个页面：offset 0 返回 1–11、offset 10 返回 11–21、offset 20 返回 21–25。驱动保留 lookahead 行；通过 `ResultPagination::from_page()` 校验 visible_rows 10/10/5、has_next true/true/false。
5. 执行 count_sql，预期 25；最后一页 offset 为 20。适配器层类型断言按现有 CellValue/Oracle NUMBER 契约书写。
6. 执行筛选 `id > 5`、排序 `id DESC` 的派生页面，预期首页 25–15 共 11 行，COUNT 为 20。
7. 增加单行有/无分号、尾部行/块注释、字符串分号和已有 `FETCH FIRST 3 ROWS ONLY` 的用例；已有 FETCH 的 COUNT 预期 3。
8. 单独覆盖解析器支持的 CTE、UNION、ORDER BY 查询，校验生成后的 SQL 在真实 Oracle 上成功；若某查询不满足现有 parser 能力，明确记录限制，不宣称所有复杂 SELECT 已支持。
9. 构造只读的错误查询，验证错误 context 中包含最终提交 SQL、code 为真实 ORA code；不依赖完整英文错误消息。
10. 运行以下命令并保留成功/失败和服务器版本记录：

```bash
cargo test --test oracle_adapter oracle_query_pagination_required -- --ignored --exact --nocapture
```

11. 在实际 TUI 中执行截图查询、切换页大小、下一页、末页、筛选排序；没有 ORDER BY 的业务查询只验证成功及列结构，不断言跨页顺序稳定。

**验收**：真实 Oracle 通过公共生成器产生的查询成功，分页/COUNT 数值正确；缺少环境时记录“未验收”，而不是以默认 ignored 测试显示通过替代。

## 8. Task 7：跨方言回归、文档和交付

**Files**
- Modify: `docs/database-capabilities.md`。
- Update: 本计划的执行记录。

**步骤**
1. 为分页生成器补齐明确的 MySQL、Postgres、SQLite、SQL Server 对照，验证既有 LIMIT/SQL Server 排序与别名策略。
2. 运行聚焦测试（已通过且代码未变的同一测试不必重复）：

```bash
cargo test --lib sql::derived_result
cargo test --lib sql::oracle
cargo test --test sql_execution --test sql_scope --test sql_risk --test sql_diagnostics
```

3. 执行格式、默认/无默认 feature 构建验证：

```bash
cargo fmt --all --check
cargo check --all-targets
cargo check --all-targets --no-default-features
cargo test --no-default-features --lib sql::derived_result
```

4. 因修改公共 SQL 层及执行错误路径，运行一次完整回归与 lint：

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

5. 更新能力文档：明确自动 SQL 分页和表预览使用 Oracle 12c+ 语法；无稳定 ORDER BY 的分页顺序限制；复杂派生查询限制；真实验证环境与未验证版本分开写。不要把这一功能版本要求写成已实现全局连接版本门禁。
6. 执行 `git diff --check`，审查最终 diff，确认没有无关代码、密钥和旧计划覆盖。
7. 记录各任务改动、实际命令、通过/失败/跳过情况、真实数据库版本与手工验收结果。

**交付说明必须区分**：纯生成器回归通过、编辑器命令测试通过、真实 Oracle 验收通过或未执行；不得把三者合并为“Oracle 全部正常”。

## 9. 依赖与完成标准

```text
Task 1 缺陷回归 → Task 2 方言生成 → Task 3 源 SQL 边界
                                      ↓
                            Task 4 编辑器链路回归
                                      ↓
                            Task 5 执行 SQL 诊断
                                      ↓
                            Task 6 真实 Oracle 验收
                                      ↓
                            Task 7 回归与交付
```

- [ ] Oracle 首页、后续分页、派生筛选和 COUNT 均生成正确方言。
- [ ] 字符串、分号、尾部注释与原文契约符合第 1 节。
- [ ] 页面 lookahead、visible_rows、has_next 和总数正确。
- [ ] Oracle 失败日志包含原始查询与实际提交文本，保留 ORA code。
- [ ] 其他方言、默认 feature、无默认 feature 的相关检查通过。
- [ ] 现场 Oracle 版本已核实，截图业务查询在修复构建中成功。
- [ ] 真实集成测试显式执行成功，未通过环境条件静默跳过。
- [ ] 已知 parser/复杂查询限制和独立手动事务问题如实记录。

只有用户明确要求提交时再按内聚变更提交；本计划不执行版本发布。

## 10. 执行记录

- 2026-09-12：完成源码复核与实施计划编写；仅新增此计划文件。未修改应用代码、未运行 Rust 测试、未连接 Oracle；现场版本和修复效果待实施时验证。
