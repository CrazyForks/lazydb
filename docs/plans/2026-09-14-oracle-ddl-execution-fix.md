# Oracle DDL 执行修复 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行环境若没有该技能，则按本文任务顺序实施，逐项记录验证结果。提交代码前使用 @git-commit 技能。

**Goal:** 修复 Oracle 普通 CREATE / ALTER 被误判为不支持的 PL/SQL 的问题，使表、视图、序列的创建和编辑能够通过共享执行链路，并修正建表默认值与非空约束的顺序。

**Architecture:** 在 `src/sql/oracle.rs` 内以有限的语句前缀分类替代 CREATE / ALTER 一刀切拦截，普通 SQL 与匿名块继续使用各自的终止符处理。Oracle Adapter 仍通过共享预处理执行目录变更；使用生成计划到执行准备的契约测试保证能力声明、SQL 生成和执行准备一致。

**Tech Stack:** Rust 2024、MSRV 1.94、现有 Oracle Scanner、oracle 0.6.3（driver-oracle feature）、Cargo 单元/集成测试、可配置的真实 Oracle 测试环境。

---

## 1. 代码事实与复现基线

行号是编写计划时的位置，实施时以符号定位。

| 位置 | 已确认事实 |
| --- | --- |
| `src/sql/oracle.rs:32–42` | `prepare_oracle_statement` 将首关键字为 CREATE / ALTER 的全部 SQL 返回 `UnsupportedProgram` |
| `src/sql/oracle.rs:12–24` | 错误显示文本与用户截图完全一致 |
| `src/sql/oracle.rs:228` | 匿名块已有独立预处理，不能用“全部 PL/SQL 不支持”描述当前能力 |
| `src/sql/oracle.rs:297` | `code_end` 当前主要按空白、分号和引号分界，紧邻关键字的注释需特别测试 |
| `src/db/oracle.rs:128–159` | 创建表生成普通 CREATE TABLE；列定义先追加 NOT NULL，再追加 DEFAULT |
| `src/db/oracle.rs:63–79` | UI 能力声明包含表、视图、序列的创建和编辑 |
| `src/db/oracle.rs:353–365` | 目录变更逐条调用 `execute_pool_with_budget` |
| `src/db/oracle.rs:933–953` | 先准备 SQL，成功后才构建并执行驱动 statement |
| `src/sql/scope.rs:193–195` | Oracle 编辑器语句范围使用同一文件中的边界扫描逻辑 |
| `tests/object_mutation_contract.rs:167–328` | 已有 Oracle 表重命名、视图替换、序列编辑的 SQL 断言，可扩充为可准备性断言 |
| `tests/oracle_adapter.rs` | 现有真实连接测试依赖环境变量；缺少变量或部分测试遇到 DPI-1047 会直接返回 |
| `.github/workflows/ci.yml:81–83` | CI 使用 Rust 1.94.0，执行 fmt、全 feature clippy 和全 targets 测试 |

已经通过独立编译当前预处理模块复现：CREATE TABLE / VIEW / SEQUENCE 和 ALTER TABLE / SEQUENCE 均失败；SELECT 和简单 BEGIN 块通过。尚未进行真实数据库建表验证。

## 2. 设计决定

1. 普通 SQL：保留原文，仅处理合法的客户端终止符；继续拒绝一次提交多条语句。
2. 匿名块：保持现有 BEGIN / DECLARE 路径及块末尾分号语义。
3. 程序单元定义：精确识别尚未支持的 CREATE PROCEDURE / FUNCTION / PACKAGE / TRIGGER / TYPE BODY，返回具体类型的本地错误。
4. TYPE specification 与 TYPE BODY 分开分类。仅为普通类型声明放行时，必须有分号处理和真实语法验证；不将 TYPE BODY 当普通 DDL。
5. CREATE 前缀按语法顺序处理可选 OR REPLACE、可选 EDITIONABLE / NONEDITIONABLE；采用 token 边界匹配，不扫描全文中的单词。
6. ALTER PROCEDURE / FUNCTION / PACKAGE / TYPE 的编译语句属于不含程序体的普通 SQL，不能沿用对象名黑名单拒绝。
7. 分类器是客户端执行形态识别器，不承担完整 Oracle 语法验证；未支持的特殊形态通过明确测试决定处理，不扩展成通用解析器。
8. 预处理错误保持 `ErrorCategory::Sql` 与 `oracle_sql_prepare_error`，错误消息说明拒绝的程序单元；驱动返回的 ORA 错误沿现有路径展示。
9. 目录表单、控制台共用修复。新增契约测试直接调用公开模块 `lazydb::sql::oracle`，不增加测试专用公共 API。

## 3. 任务与依赖

执行顺序：任务 1 → 任务 2 → 任务 3 → 任务 4 → 任务 5 → 任务 6。每个步骤单独执行并记录结果；任务 2、3 完成后形成可评审的修复提交。

### Task 1：建立普通 DDL 与程序单元回归矩阵

**Files:**
- Modify/Test: `src/sql/oracle.rs` 的 `#[cfg(test)] mod tests`

**Step 1：增加截图对应的失败测试。**

```rust
#[test]
fn accepts_create_table_from_catalog_editor() {
    let sql = "CREATE TABLE \"MFGSUPPORT\".\"tt1\" (\"id\" varchar(20) NOT NULL)";
    assert_eq!(prepare_oracle_statement(sql).unwrap(), sql);
}
```

**Step 2：运行并记录预期失败。**

Run: `cargo test --lib sql::oracle::tests::accepts_create_table_from_catalog_editor`

Expected: 在 `unwrap()` 处因 `UnsupportedProgram` 失败。

**Step 3：增加表驱动测试。**

```rust
#[test]
fn prepares_ordinary_oracle_ddl() {
    for sql in [
        "CREATE TABLE t (id NUMBER)",
        "CREATE VIEW v AS SELECT 1 AS id FROM dual",
        "CREATE OR REPLACE VIEW v AS SELECT 2 AS id FROM dual",
        "CREATE SEQUENCE s START WITH 1 INCREMENT BY 1 CACHE 20",
        "CREATE INDEX i ON t (id)",
        "ALTER TABLE t RENAME TO t2",
        "ALTER SEQUENCE s INCREMENT BY 2",
        "ALTER PROCEDURE p COMPILE",
    ] {
        assert_eq!(prepare_oracle_statement(sql).unwrap(), sql);
        assert_eq!(prepare_oracle_statement(&format!("{sql};")).unwrap(), sql);
    }
}
```

**Step 4：增加分类边界与保留行为用例。**

| 场景 | 输入代表 | 预期 |
| --- | --- | --- |
| 前置注释、大小写 | `/* ddl */ create table t (id NUMBER);` | 放行且仅移除终止分号 |
| 紧邻注释 | `CREATE/* comment */TABLE t (id NUMBER);` | 放行 |
| 修饰符 | `CREATE OR REPLACE EDITIONABLE VIEW v AS SELECT 1 AS id FROM dual;` | 放行 |
| 非 editionable 程序 | `CREATE OR REPLACE NONEDITIONABLE PROCEDURE p AS BEGIN NULL; END;` | 精确拒绝 PROCEDURE |
| 程序族 | PROCEDURE、FUNCTION、PACKAGE、PACKAGE BODY、TRIGGER、TYPE BODY | 分别识别，含内部多个分号也不误报普通多语句 |
| 类型声明 | `CREATE TYPE t AS OBJECT (id NUMBER);` | 按设计决定验证普通声明的终止符处理 |
| 单词伪装 | 表名、列名或默认值包含 `BEGIN` / `PROCEDURE` | 普通 DDL 不误判 |
| 引用内容 | 字符串、q quote、双引号标识符包含分号 | 引用内分号保持原样 |
| 尾部注释 | `CREATE TABLE t (id NUMBER); -- tail` | 保留注释 |
| 多语句 | `CREATE TABLE t (id NUMBER); DROP TABLE t;` | MultipleStatements |
| 分号后引用 token | `CREATE TABLE t (id NUMBER); 'extra'` | MultipleStatements |
| 客户端 slash | 普通 SQL 后合法独立 `/`；裸 `/` | 前者按现有规则去除，后者拒绝 |
| 匿名块 | 原有简单块、嵌套 IF / LOOP、DECLARE 块 | 末尾 `END;` 保留 |

Note: 当前普通路径会忽略终止分号后的 `Quoted` token。放行 DDL 时同步补上这个单语句边界漏洞，而不是仅测试第二条语句以裸关键字开头的情况。

**Step 5：运行模块测试，确认新增失败集中在待修复行为。**

Run: `cargo test --lib sql::oracle::tests`

Expected: 原有支持行为通过；新增普通 DDL、精确错误和相邻注释相关断言按当前缺陷失败。

### Task 2：实现前缀分类与准确错误

**Files:**
- Modify: `src/sql/oracle.rs` 的错误类型、Display、Scanner、`prepare_oracle_statement`
- Review: `src/sql/derived_result.rs`、`src/sql/scope.rs`（共享调用点）

**Step 1：定义私有执行形态分类与程序类型。**

建议 `OracleStatementKind` 表达 Ordinary / AnonymousBlock / UnsupportedProgram，程序类型明确区分 Procedure、Function、Package、PackageBody、Trigger、TypeBody。错误若携带该类型，按 Rust 可见性要求设置类型可见性；实现前查找所有 `UnsupportedProgram` 构造和匹配点。

**Step 2：实现有限前缀读取。**

- 从开头读取有效 token，跳过行注释、块注释和空白。
- BEGIN / DECLARE 直接分入已有匿名块路径。
- CREATE 后按顺序读取可选修饰符，再识别对象类型和可选 BODY。
- 非程序单元定义进入 Ordinary；ALTER 不再被首关键字拒绝。
- 扫描错误使用 Result 传播，避免将不完整引用或注释悄悄吞成“没有关键字”。
- 关键字与 `/*`、`--` 紧邻时仍能正确分界；扫描始终推进到合法 UTF-8 边界。

**Step 3：替换首关键字 CREATE / ALTER 拒绝分支。**

普通路径继续使用当前 Scanner 的引用处理、单语句检查、尾部分号和 slash 处理。将终止分号之后的 `Token::Quoted(_)` 与 `Token::Code(_)` 一样拒绝；注释仍可保留。

**Step 4：调整 Display。**

程序类型对应错误示例：`Oracle execution does not yet support CREATE PACKAGE BODY statements`。普通 DDL 不再返回这个错误；SQL*Plus 客户端命令错误沿用独立类别。本轮先验证已有 slash 行为，完整 SQL*Plus 命令目录另行设计。

**Step 5：执行测试和共享扫描回归。**

Run:

```bash
cargo test --lib sql::oracle::tests
cargo test --test sql_scope
cargo test --test sql_risk
cargo test --lib sql::derived_result
```

Expected: 新旧测试全部通过；最后一个命令如果匹配到 0 个测试，必须记录为无匹配测试，再定位实际派生查询测试执行，不能记作覆盖完成。

**Step 6：提交可独立评审的分类修复。**

建议提交：`fix(oracle): allow ordinary DDL through SQL preparation`

### Task 3：修复 DEFAULT / NOT NULL 生成顺序

**Files:**
- Modify: `src/db/oracle.rs` 的 `plan_catalog_mutation` Table 分支
- Modify/Test: `tests/object_mutation_contract.rs`

**Step 1：添加 Oracle 建表计划测试。**

参考现有 `mysql_table_create_plan_uses_backtick_quoting` 的请求构造，使用 Oracle 的 `["SERVICE", "APP"]` Schema anchor 和 `ObjectGroup::Tables`，建立 id NUMBER、默认值 1、nullable=false 的草稿。

精确断言：

```rust
assert_eq!(
    plan.statements(),
    &["CREATE TABLE \"APP\".\"t\" (\"id\" NUMBER DEFAULT 1 NOT NULL)"]
);
```

**Step 2：运行该测试确认当前输出顺序错误。**

Run: `cargo test --test object_mutation_contract oracle_table_create`

Expected: 修复前因 NOT NULL 位于 DEFAULT 前而失败。

**Step 3：将列生成顺序改为类型、默认值、非空约束。**

替换现有两个追加分支为：

```rust
if !column.default_expression.value().trim().is_empty() {
    sql.push_str(" DEFAULT ");
    sql.push_str(column.default_expression.value().trim());
}
if !column.nullable {
    sql.push_str(" NOT NULL");
}
```

**Step 4：补足四种字段组合。**

覆盖默认值为空/非空 × nullable=true/false，验证标识符引用、默认表达式原文和 Removed 列过滤。至少一例用包含分号的字符串默认值，随后调用预处理，确保表达式不被切断。

**Step 5：运行测试后提交。**

Run: `cargo test --test object_mutation_contract oracle_`

Expected: 全部 Oracle 契约测试通过。

建议提交：`fix(oracle): render column defaults before nullability constraints`

### Task 4：补齐生成计划到执行准备的契约

**Files:**
- Modify/Test: `tests/object_mutation_contract.rs`

**Step 1：添加复用断言函数。**

```rust
fn assert_oracle_plan_prepares(
    plan: &lazydb::db::catalog_mutation::CatalogMutationPlan,
) {
    for statement in plan.statements() {
        let prepared = lazydb::sql::oracle::prepare_oracle_statement(statement)
            .expect("advertised Oracle mutation must pass SQL preparation");
        assert_eq!(prepared.as_ref(), statement.as_str());
    }
}
```

该原文相等断言用于生成器产生的不含客户端终止符的 SQL；用户输入终止符归一化由模块测试覆盖。

**Step 2：扩充现有三个编辑计划测试。**

在表重命名、视图替换、序列属性编辑的精确 SQL 断言后调用辅助函数。

**Step 3：新增三个创建计划契约。**

分别覆盖表、视图、序列；保留针对 SQL 内容和 execution mode 的断言，并调用辅助函数。表创建复用任务 3 的 fixture。验证每个计划采用 `CatalogMutationExecutionMode::Autocommit`。

**Step 4：检查控制台范围与风险分类。**

若现有 `tests/sql_scope.rs` / `tests/sql_risk.rs` 没有 Oracle 普通 DDL 用例，则添加 CREATE TABLE 的范围选择、两条普通 DDL 的边界、DDL 风险保持非 ReadOnly 的断言。仅在测试暴露具体问题时修改对应实现。

**Step 5：执行 targeted tests。**

```bash
cargo test --test object_mutation_contract oracle_
cargo test --test sql_scope
cargo test --test sql_risk
cargo test --no-default-features --test object_mutation_contract oracle_
```

Expected: 全部通过；不依赖 Oracle 客户端即可执行纯计划与预处理契约。

建议提交：`test(oracle): verify catalog plans pass SQL preparation`

### Task 5：真实 Oracle 与 TUI 验收

**Files:**
- Modify/Test: `tests/oracle_adapter.rs`

**Step 1：新增显式运行的 DDL 生命周期测试。**

建议命名 `oracle_catalog_ddl_round_trip`，使用 `#[ignore = "requires a configured Oracle DDL test schema"]`。显式运行后，连接变量缺失或 DPI-1047 应失败并解释环境问题，避免假通过。

复用环境变量：`LAZYDB_TEST_ORACLE_URL`、`LAZYDB_TEST_ORACLE_USER`、`LAZYDB_TEST_ORACLE_PASSWORD`；需要执行目标 schema 时增加 `LAZYDB_TEST_ORACLE_SCHEMA`，默认使用当前用户对应的 schema，并保留引用标识符大小写。服务名从连接/目录探测结果获取，不沿用已有 probe 测试中 supportdb 的硬编码假设。

**Step 2：实现建表和编辑生命周期。**

- 用短 UUID 后缀命名测试对象，控制名字长度以兼容较老 Oracle 标识符限制。
- 通过实际 catalog mutation 计划创建含 DEFAULT 与 NOT NULL 的表。
- 查询目录或数据字典验证表和列存在。
- 插入省略默认列的记录，查询验证默认值确实生效。
- 重命名表，验证新名称存在、旧名称不存在。
- 通过同一 catalog mutation 路径创建/替换视图、创建/修改序列，并验证结果。

**Step 3：实现清理和失败诊断。**

执行结果保存后再统一清理已创建对象，最终汇总原始失败和清理失败；不要在清理之前用 unwrap 提前退出。Oracle DDL 自动提交，因此清理使用显式 DROP，不能依靠 ROLLBACK。

**Step 4：运行真实集成测试。**

Run: `cargo test --test oracle_adapter oracle_catalog_ddl_round_trip -- --ignored --exact --nocapture`

Expected: 测试实际连接、完成 DDL 与数据验证、清理成功。无可用环境时记录阻塞原因，验收状态标为“未验证”。

**Step 5：执行 TUI 手工验收。**

1. 在 Oracle schema 的 Tables 分组创建截图同类表，使用唯一测试名。
2. Review SQL 显示正确的 CREATE TABLE；按 Enter 执行成功。
3. 新对象出现在目录中，可以加载结构和预览数据。
4. 设置默认值且选择非空，再次创建并确认生成顺序和默认值行为。
5. 在 SQL 控制台执行普通 CREATE / ALTER，验证共享修复生效。
6. 提交未支持的程序定义，验证展示具体类型的客户端错误。
7. 提交一个普通语法错误，验证展示 Oracle 驱动错误而非 UnsupportedProgram。
8. 清理本轮创建的测试对象。

建议提交：`test(oracle): cover catalog DDL lifecycle against Oracle`

### Task 6：最终检查与交付记录

**Files:**
- Update: 本计划末尾实施记录
- Review: 本轮修改文件的 diff

**Step 1：执行与 CI 一致的检查。**

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
git diff --check
```

Expected: 全部通过。数据库相关测试的环境跳过不能算作真实 Oracle 验证，Task 5 单独记录结果。

**Step 2：核对交付条件。**

- [ ] 截图同类 CREATE TABLE 通过预处理，并经真实 Oracle 验证。
- [ ] 表、视图、序列的 3 类创建和 3 类编辑计划均通过准备契约。
- [ ] DEFAULT + NOT NULL 组合语法及实际默认值行为正确。
- [ ] CREATE 程序单元仍准确拒绝，ALTER 编译语句不被误判。
- [ ] 引用、注释、分号和现有匿名块回归测试通过。
- [ ] Oracle 普通 DDL 的范围选择与风险分类没有回退。
- [ ] TUI 执行成功后目录刷新、结构加载和预览正常。
- [ ] 错误消息区分本地准备失败与 Oracle 执行失败。
- [ ] fmt、clippy、全量 Rust 测试和 diff 检查通过。

**Step 3：记录实际结果并交付。**

记录修改摘要、执行命令、测试计数、跳过或阻塞原因、真实 Oracle 版本与 TUI 验收结果。全部通过后按需进入发布流程，版本号与发布说明由正式发布任务处理。

## 4. 实施记录

- 当前状态：计划已编写，尚未实施生产代码修改。
- 已验证：当前源码中的 CREATE / ALTER 误拦截可本地稳定复现。
- 待验证：代码修复、回归测试、真实 Oracle、TUI、CI 等价检查。
