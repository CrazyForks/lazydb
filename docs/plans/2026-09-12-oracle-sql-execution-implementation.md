# Oracle SQL 执行兼容性 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行环境若未提供该技能，则直接按本计划逐任务实施、验证并报告，不假定技能可用。提交代码或启用子代理前须取得用户明确授权。

**Goal:** 修复 Oracle SQL Editor 将普通 SQL 末尾分号提交 OCI 导致的错误，并保证 Oracle 引号、PL/SQL 边界、风险分类和错误信息的一致性。

**Architecture:** 在 SQL 层提供独立于 Oracle 原生库的词法扫描和单语句准备能力，编辑器继续保留原文范围，Oracle 适配器在驱动调用前统一准备 SQL。分阶段交付普通 SQL 修复与程序块兼容；边界识别不承担完整语法校验，风险识别继续保守处理解析器不支持的语法。

**Tech Stack:** Rust 2024 / Rust 1.94、Tokio、oracle 0.6.3、sqlparser 0.62、现有 scope / transaction / risk / execution 模块、Rust 单元与集成测试。

---

## 0. 已确认事实与实施约束

- `src/sql/scope.rs`：`scan_statements()` 的范围有意包含终止分号；`resolve_scope()` 返回原文。
- `src/app.rs`：执行草稿接收 `scope.sql`；这是历史、确认和执行上下文的一部分。
- `src/runtime.rs`：普通查询进入 `DatabaseConnection::execute_with_budget()`。
- `src/db/mod.rs`：Oracle 路由进入 `OracleAdapter::execute_pool_with_budget()`。
- `src/db/oracle.rs`：目前把 SQL 原样传给 `connection.statement(&sql).build()`。
- `src/sql/dialect.rs`：Oracle 映射到 `Generic`，没有独立 Oracle 分支。
- `src/sql/transaction.rs`：事务分类复用语句扫描；`BEGIN`、`END` 有事务命令含义，不能与完整 PL/SQL 块混淆。
- `src/sql/risk.rs`：使用 sqlparser，解析失败返回 `Unknown`；不能因识别为一个块就判定只读。
- `tests/oracle_adapter.rs`：现有值查询不含末尾分号；没有环境变量时跳过，部分测试遇到 DPI-1047 也跳过。
- 默认 feature 已包含 `driver-oracle`，还需验证 `--no-default-features`。
- 计划编写时 `src/app.rs` 已有未提交修改。实施前检查最新 diff，在现有修改基础上操作。
- 本计划未连接业务 Oracle；分号是代码支持的直接原因判断，现场对照验证仍待执行。

### 范围

本次交付：单语句终止符处理、Oracle 词法与范围识别、PL/SQL 保真、调用入口接入、分类一致性、结构化 Oracle 错误码和回归验证。

单次 API 执行只允许一个执行单元。多语句选区或全缓冲区在任何数据库执行前明确拒绝，不执行第一条后再报错。

SQL*Plus 的 `SET`、`SPOOL`、`PROMPT`、`@` 等脚本命令不在本次支持范围；不新增脚本执行器，不改变自动提交、连接所有权、取消和事务生命周期。

## 1. 行为契约

| 输入 | 编辑器范围 / 驱动行为 |
| --- | --- |
| `SELECT 1 FROM dual` | 一个单元，原样提交 |
| `SELECT 1 FROM dual;` | 范围含分号，提交时删除该终止符 |
| `SELECT 1 FROM dual; -- tail` | 选区原文不变；删除代码终止符，保留注释与换行 |
| `SELECT ';' FROM dual;` | 保留字符串分号，移除最外层终止符 |
| `SELECT q'[it's;ok]' FROM dual;` | 替代引用是一个完整 token |
| 带双引号标识符、hint、CRLF、中文注释 | 保留内容与正确 UTF-8 字节范围 |
| `BEGIN NULL; END;` | 第二阶段作为一个 PL/SQL 单元，保留 `END;` |
| `DECLARE ... BEGIN ... END;` | 第二阶段保留声明区与块内所有分号 |
| 程序定义 / 包 / 触发器 | 第二阶段按明确支持矩阵识别，不按内部分号拆分 |
| 一个执行单元后独占一行的 `/` | 删除客户端分隔符；不把它解释成重执行上一条命令 |
| 字符串或注释中的 `/`，表达式中的除号 | 保留 |
| 单独 `/`、重复 `/`、只有空白或注释 | 明确本地错误，无 OCI 调用 |
| 两个独立执行单元 | 明确多语句错误，无 OCI 调用 |
| 未闭合引用、注释、无法确定边界的程序结构 | 本地错误，不猜测、截断或补齐 |

普通 SQL 重复终止符如 `SELECT 1 FROM dual;;` 按无效输入拒绝，不用 `trim_end_matches` 静默清洗。

### 数据流

```text
编辑器原文 → scope 原文范围 → ExecutionDraft / 确认与风险策略
                                        ↓
                       OracleAdapter 执行入口
                                        ↓
                  prepare_oracle_statement（恰好一个单元）
                                        ↓
                       OCI prepare / execute / query
```

Oracle 扫描能力由 scope 和 prepare 共用。分类可使用准备后的只读视图，但日志、草稿、历史、复制仍持有原文；不要让 UI 预处理成为唯一防线。

## 2. 阶段一：修复普通 SQL 提交

### Task 1：建立 Oracle 普通 SQL 扫描与准备模块

**Files**
- Create: `src/sql/oracle.rs`（实现和模块内单元测试）
- Modify: `src/sql/mod.rs`（模块注册，优先 crate 内可见）

**建议内部接口**

```rust
pub(crate) fn prepare_oracle_statement(
    sql: &str,
) -> Result<std::borrow::Cow<'_, str>, OracleSqlError>;
```

`OracleSqlError` 至少区分空输入、多执行单元、不支持的客户端指令、不完整输入、暂不支持的程序边界。复用现有 TextRange 表示字节范围；扫描结果记录正文范围和可删除的客户端终止符范围。没有删除操作时返回借用，有删除操作时一次构造字符串。

**步骤**
1. 添加模块和函数的可编译骨架，新增表驱动测试：无分号、分号、尾部注释、字符串分号、双引号、hint、重复分号和空输入。
2. 执行 `cargo test --lib sql::oracle`，确认行为断言失败；不是因环境或无关编译错误失败。
3. 实现 Oracle 专用词法状态：普通字符、单引号（双写转义）、双引号、行注释、块注释、`q` / `Q` 与 `nq` / `NQ` 替代引用。成对和自定义分隔符都须覆盖。
4. 明确 Oracle 不沿用 Generic 的 `#` 注释、美元引用和反斜杠转义规则。扫描器只产生边界与删除范围，不重排 token。
5. 实现普通 SQL 的单元计数、终止符删除与尾部注释保留，识别单元后的独占行 `/`。所有输入预检结束后才允许返回可执行文本。
6. 第一阶段遇到程序型输入或内嵌 PL/SQL 的 `WITH FUNCTION/PROCEDURE` 时返回明确暂不支持错误，不能套用普通 SQL 清理；第二阶段再放开。
7. 添加 UTF-8、CRLF、引号未闭合、非配对引用分隔符、除法和多语句测试。
8. 再次执行同一测试命令；预期全部通过且不需要 Oracle 原生客户端。

**验收**：普通 SQL 仅删除合法客户端终止符，任何字符串、注释、hint 内容不变；多语句和不完整输入不产生可执行结果。

### Task 2：在 Oracle 适配器统一接入

**Files**
- Modify: `src/db/oracle.rs`（`execute_pool_with_budget`、本地准备错误映射）
- Modify: `tests/oracle_adapter.rs`

**步骤**
1. 为 `SELECT 1 FROM dual`、带分号、尾部注释、字符串分号添加 Oracle 集成用例；断言列和值一致。
2. 在已配置的测试 Oracle 上运行 `cargo test --test oracle_adapter oracle_ -- --nocapture`，记录分号用例修复前的真实错误。没有环境则标注待验证，不将跳过视为复现。
3. 在 feature 启用分支、获取连接锁和 `spawn_blocking` 前调用准备函数，然后把准备后的拥有所有权的文本传入阻塞闭包。
4. 本地错误映射到有稳定 code 的 DatabaseError；保持现有驱动未启用错误行为。
5. 在模块测试中验证错误映射，并确认 Agent 与事务后端通过同一执行入口；不另设复制的分号清洗代码。
6. 执行 `cargo test --lib sql::oracle` 和配置好的 Oracle 集成测试。

**验收**：带分号查询成功；多语句在 OCI 之前拒绝；正常查询的预算与结果类型保持一致。

**阶段一交付门槛**：Task 1–2 通过后可单独交付普通 SQL 修复，但必须明确此时的程序块输入仍受限制，不能宣称 PL/SQL 已支持。

## 3. 阶段二：Oracle 范围与程序块兼容

### Task 3：引入独立 Oracle 方言并接入 scope

**Files**
- Modify: `src/sql/dialect.rs`
- Modify: `src/sql/scope.rs`
- Modify: `src/sql/oracle.rs`
- Modify: `tests/sql_dialect_mapping.rs`
- Modify: `tests/sql_scope.rs`
- 根据编译错误检查其他 `SqlDialect` 穷尽匹配，逐处保留原有行为。

**步骤**
1. 添加 Oracle 映射断言与 scope 用例：字符串含分号、`q` 引用含单引号、前后多条普通 SQL、光标位于正文/分号/注释/空白。
2. 运行 `cargo test --test sql_dialect_mapping --test sql_scope`，确认新增断言失败。
3. 增加 `SqlDialect::Oracle`，`for_database_kind` 返回它。`parser_dialect` 初期明确复用 GenericDialect，而不是假定 sqlparser 0.62 存在完整 OracleDialect。
4. Oracle 的 scope 边界委托共用扫描器；保留公开的 TextRange 与原文契约。扫描错误时不能返回部分程序块作为“当前语句”。
5. 运行 `cargo check --all-targets`，按实际穷尽匹配逐处处理；尤其核对格式化、补全、分页与风险模块。不得将 Oracle 默认落入 MySQL LIMIT 路径。
6. 执行 `cargo test --test sql_dialect_mapping --test sql_scope --test sql_format --test sql_completion`。

**验收**：Oracle 词法正确；其他方言现有范围测试通过；视觉选择仍保持精确选区文本。

### Task 4：补齐 PL/SQL 执行单元边界

**Files**
- Modify: `src/sql/oracle.rs`
- Modify: `tests/sql_scope.rs`
- Modify: `tests/oracle_adapter.rs`

**边界支持矩阵**
- 匿名 `BEGIN` / `DECLARE` 块，包括标签、嵌套块、EXCEPTION。
- `IF ... END IF`、`LOOP ... END LOOP`、CASE 表达式与 CASE 语句。
- CREATE [OR REPLACE] [EDITIONABLE | NONEDITIONABLE] PROCEDURE / FUNCTION。
- PACKAGE / PACKAGE BODY、普通与复合 TRIGGER、TYPE / TYPE BODY；普通对象类型定义与程序体必须区分，不能把所有 CREATE TYPE 一律当程序块。
- `WITH FUNCTION/PROCEDURE` 的 SQL 执行单元：若本次无法可靠识别，应明确拒绝并列入限制，不能误切或判只读。

**步骤**
1. 将上述每一类做成独立测试 fixture，断言单元数量、原文范围、准备后的字节内容；包括程序块后接第二条 SQL 的反例。
2. 分别运行 `cargo test --lib sql::oracle` 和 `cargo test --test sql_scope`，记录尚不支持项。
3. 为词法 token 增加程序头识别和结构栈；显式区分 `END`、`END IF`、`END LOOP`、`END CASE`。不要用简单 BEGIN/END 计数模拟 PL/SQL。
4. 区分声明区的局部子程序结束与外层结束；处理命名 END、包规范、包体和复合触发器。保留必需的最终分号。
5. 将独占行 `/` 作为客户端边界，绝不作为 OCI 内容或重复执行命令；块未闭合时即使遇到 `/` 也不能执行截断块。
6. 对不能判定的结构返回明确错误；不补 END，不追加分号，不绕过风险策略。普通 SQL 的准备行为继续通过阶段一测试。
7. 添加 `BEGIN NULL; END;` 和 `DECLARE n NUMBER := 1; BEGIN n := n + 1; END;` 的真实 Oracle 测试，及带末尾 `/` 的等价用例。
8. 程序对象 DDL 在专用测试 schema 中使用唯一命名并清理；测试前确认对象创建权限。不要在业务 schema 建测试对象。

**验收**：支持矩阵中的每一项都有成功/反例测试；未完成项必须明确拒绝并记录，不能通过“把剩余缓冲区当一个块”隐式放行。

### Task 5：打通事务分类、风险分类与编辑器执行

**Files**
- Modify: `src/sql/transaction.rs`
- Modify: `src/sql/risk.rs`
- Inspect / 按需修改: `src/sql/execution.rs`、`src/app.rs`
- Modify: `tests/transaction_sql.rs`
- Modify: `tests/sql_risk.rs`
- Modify: `tests/sql_execution.rs`
- Inspect / 按需补充: `tests/agent_service.rs`

**步骤**
1. 增加完整 PL/SQL 不被识别为事务 Begin/Commit、普通 COMMIT/ROLLBACK 分类保持正确的测试。
2. 增加普通只读 SELECT 带终止符或 `/` 后分类一致的测试；完整未知程序块应为 Unknown，不能因只看到内部 SELECT 就变为 ReadOnly。
3. Oracle 分类复用单元识别与终止符视图；sqlparser 不支持的完整程序单元保持 Unknown。多单元始终保持多语句事实，不用第一条的风险替代全体。
4. 核对 ExecutionDraft 的混合事务判断和确认流程。完整块应作为一个数据执行单元处理，不能误走事务控制分发。
5. 添加 editor scope → ExecutionDraft 的回归：原文不变、确认文本不变、选区与当前语句选择正确。若要修改 app.rs，先阅读用户最新 diff。
6. 核对 Agent authorize_query/authorize_write：普通只读查询不受终止符影响；未知 PL/SQL 不因本次功能而绕过现有授权。
7. 运行 `cargo test --test transaction_sql --test sql_risk --test sql_execution --test agent_service`。

**验收**：程序边界与事务/风险分类一致，SQL Editor 可以沿既有确认路径提交完整 PL/SQL；不改变 Unknown 的既有权限策略。

## 4. 阶段三：错误诊断与完整验收

### Task 6：保留 Oracle 原生错误码

**Files**
- Modify: `src/db/oracle.rs`
- Inspect / 按需修改: `src/db/mod.rs`（现有 DatabaseDiagnostic 契约）
- Modify: `tests/oracle_adapter.rs`
- Inspect: `tests/sql_diagnostics.rs`

**步骤**
1. 实施时查阅 oracle 0.6.3 的实际错误类型/API，确认原生错误码、偏移信息是否可用及偏移单位，不凭记忆调用 API。
2. 将原生 Oracle 错误与锁失败、任务失败等本地错误分开映射；同步检查提前 `.to_string()` 的调用点，避免丢失结构化数据。
3. 数据库错误 code 使用稳定 `ORA-xxxxx`；DPI、配置和本地准备错误保留各自稳定 code。保留终端文本清理。
4. 有可靠偏移 API 时才能接入诊断，且须处理提交文本删除字符后的原文映射；没有则本次只交付错误码，不伪造位置。
5. 添加映射测试及真实 Oracle 语法错误测试，断言错误码而非依赖完整英文错误文案。
6. 不对 ORA-00933 自动重试，也不对所有该错误附加“删除分号即可”的错误结论。

**验收**：错误保留原生 code；真正的语法错误仍正常显示；无额外数据库执行。

### Task 7：验证、文档与交付

**Files**
- Update: 本计划的执行记录与已确认限制
- 按需更新现有 Oracle 用户文档，实施时先定位文档；不为此改版本或创建发布标签。

**步骤与命令**
1. 运行聚焦测试：

```bash
cargo test --lib sql::oracle
cargo test --test sql_dialect_mapping --test sql_scope --test transaction_sql --test sql_risk --test sql_execution
```

2. 验证格式与两种构建配置：

```bash
cargo fmt --all --check
cargo check --all-targets
cargo check --all-targets --no-default-features
cargo test --no-default-features --lib sql::oracle
```

3. 由于新增方言分支涉及公共 SQL 模块，运行一次完整 Rust 测试和 lint：

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

4. 在外部安全配置 `LAZYDB_TEST_ORACLE_URL`、`LAZYDB_TEST_ORACLE_USER`、`LAZYDB_TEST_ORACLE_PASSWORD`，原生库可加载后运行：

```bash
cargo test --test oracle_adapter -- --nocapture
```

真实验证应增加显式启用的 ignored 集成测试入口：在显式执行时，缺少环境或 DPI-1047 必须失败而不是跳过。建议新增 `oracle_execution_compatibility_required` 测试，然后运行：

```bash
cargo test --test oracle_adapter oracle_execution_compatibility_required -- --ignored --exact --nocapture
```

5. 手工 TUI 验收：
   - `SELECT 1 FROM dual;` 当前语句执行成功。
   - 带尾部注释和替代引用查询成功。
   - 选中完整匿名块执行成功，正文及 END 分号保持原样。
   - 块后有第二条 SQL 时，当前语句只选中所在单元；全选执行在数据库前拒绝。
   - SQL 历史、复制和确认内容仍为用户原文。
   - 真实语法错误显示 ORA code。
6. 检查 `git diff --check` 与最终 diff；报告测试通过、跳过、未运行各自数量或具体项目。既有失败与本次回归区分记录，不能以跳过测试宣称 Oracle 已验收。

## 5. 依赖顺序与完成标准

```text
Task 1 → Task 2 → 普通 SQL 修复可交付
   ↓
Task 3 → Task 4 → Task 5 → Oracle 程序块兼容可交付
                              ↓
                          Task 6 → Task 7
```

每个任务按“添加有意义的失败用例 → 最小实现 → 聚焦验证 → 更新记录”推进。只有用户要求提交时，才按以上阶段拆分提交；本计划不授权自动 commit/push 或子代理执行。

**最终完成标准**
- 截图对应的带分号 SELECT 在真实 Oracle 上执行成功。
- 普通 SQL、PL/SQL 终止符和客户端 `/` 的行为符合契约。
- 多语句输入在执行前拒绝，不出现部分执行。
- 编辑器定位、确认、风险识别、事务识别没有互相冲突。
- 非 Oracle 方言与无 Oracle feature 构建通过回归。
- 真实 Oracle 验证可证明没有被环境条件静默跳过。

## 6. 已发现但需独立处理的问题

`OracleTransactionBackend::begin()` 当前提交裸 `BEGIN`，Oracle 中它不是完整匿名块；自动提交、共享连接、取消及 force_close 也需要独立核对。它们不解释截图的 AUTO SELECT 分号问题，本次不能通过修改终止符顺带改变事务语义。若手动事务验收受阻，应单独报告并制定事务生命周期修复计划，不把手动事务模式列为本次已验证能力。

## 7. 执行记录

- 2026-09-12：完成代码核查与实施计划编写；未修改应用代码，未执行测试，未连接 Oracle。
