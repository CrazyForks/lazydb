# SQL History 类型标签实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行环境若未提供该技能，按本文任务顺序逐项执行并记录验证结果。

**Goal:** 在 SQL History 左侧列表显示固定宽度 SQL 类型标签，提高多行历史记录的扫读效率。

**Architecture:** 新增独立于执行风险的纯 SQL 类型分类器，复用项目的方言解析和 SQL Server 分批能力。标签和 SQL 分列绘制，分类结果保存在 overlay 对应的展示缓存中，仅缓存可见记录；原始历史 SQL 仍是复制和详情的数据源。

**Tech Stack:** Rust 1.94、sqlparser 0.62、ratatui 0.30.2、unicode-width、现有 Theme 和 SqlHistoryState。

---

## 1. 已确认的代码入口

- `src/sql/mod.rs`：纯 SQL 服务模块及公开导出。
- `src/sql/risk.rs:31`：`classify_sql()` 是风险分类，不直接作为展示类型来源。它把锁查询视为 DML 风险，并将 EXPLAIN 递归到内部语句。
- `src/ui/sql_history_modal.rs:144`：`render_list()` 负责 SQL 预览、元信息、选中背景、`SqlHistoryRow(index)` 点击区域。
- `src/ui/sql_preview.rs:23`：`lines_without_line_numbers()` 已包含终端文本净化、语法高亮和按显示宽度折行。
- `src/model/sql_history_view.rs:163`：`complete()` 校验请求身份，支持追加去重和首次加载替换。
- `src/app.rs:13899`：`Action::SqlHistoryLoaded` 接入历史页。
- `src/app.rs:16735`：`sql_history_dialect()` 按记录 profile 解析方言，目标缺失时返回 Generic。
- `src/ui/mod.rs`：`UiState`、overlay 渲染入口和点击目标定义。
- `src/ui/theme.rs:45`：主题颜色及无颜色模式。
- `tests/sql_history_interaction.rs`：复制完整 SQL、进入 SQL 详情、打开和关闭 overlay 的现有回归测试。

行号是计划编写时的定位提示，实施时按符号定位。

工作区已有 `src/app.rs`、`src/ui/mod.rs`、`tests/workspace_tabs.rs` 的未提交修改，以及另一份计划文档。实施前查看相关 diff，保留现有工作；若最终创建提交，只暂存本功能涉及的修改块。

## 2. 产品与分类规则

### 2.1 标签集合

| 显示 | 内部枚举 | 规则 |
| --- | --- | --- |
| DQL | Dql | SELECT、VALUES、TABLE、普通查询 CTE、已识别的 SHOW / 描述表结构语句 |
| DML | Dml | INSERT、UPDATE、DELETE、MERGE、含写操作的 CTE |
| DDL | Ddl | CREATE、ALTER、DROP、TRUNCATE 等明确的对象定义语句 |
| DCL | Dcl | GRANT、REVOKE |
| TCL | Tcl | 开始事务、COMMIT、ROLLBACK、SAVEPOINT、RELEASE SAVEPOINT |
| MIX | Mixed | 可成功识别的多个独立语句包含不同类型 |
| SQL | Other | 空文本、仅注释、解析失败、未覆盖的管理命令或过程调用 |

使用标准拼写 DQL。所有可见标签均为三个 ASCII 字符。

### 2.2 明确边界

1. 前导注释、换行和大小写通过 AST 处理，不使用字符串前缀或正则代替解析。
2. `WITH ... SELECT` 为 DQL；主语句为 UPDATE / DELETE 等时为 DML；查询 CTE 内有写操作时归为 DML。
3. `SELECT ... FOR UPDATE` 展示 DQL，原风险服务继续判定锁风险。
4. PostgreSQL / SQL Server 创建新表的 `SELECT INTO` 展示 DDL；若某方言表示变量赋值，则不套用创建表规则，无法确认的结构返回 SQL。
5. 普通 EXPLAIN 展示 DQL；`EXPLAIN ANALYZE` 按被执行语句类型分类。以当前依赖支持的 AST 标志识别 ANALYZE，不匹配字符串中的单词。
6. 多条 SQL：全同类则显示该类，不同已知类型显示 MIX；任意一条无法判定则整个记录显示 SQL，避免给不完整分析结果一个确定分类。
7. SQL Server 先复用 `split_sql_server_batches()` 处理 GO，再解析并聚合所有语句。
8. `CALL`、`EXECUTE`、`PREPARE` 及未明确覆盖的命令先返回 SQL。
9. 类型是历史展示信息，不作为执行权限、只读判断或事务路由依据。

### 2.3 布局与主题

```text
[DQL] SELECT * FROM sys_user
      WHERE enabled = true
      09-15 15:36:10 · moss_biz · success
[DDL] ALTER TABLE sys_user
      ADD COLUMN last_login timestamp
      09-15 15:24:09 · moss_biz · success
```

- 标签列为 `[TYPE] `，固定 6 个字符列，仅记录第一行显示标签。
- SQL 所有续行与元信息从同一 x 坐标开始；原 SQL 自带缩进继续保留。
- 最多 3 行 SQL + 1 行元信息；剩余高度先预留元信息，至少需要 2 行才显示记录。
- 先铺满整条记录背景，再绘制标签和内容，避免标签列或空白尾部出现选中断层。
- 标签使用加粗和彩色前景，不铺抢眼的独立亮色背景。
- DQL→action，DDL→syntax_type，DML→warning，DCL→syntax_parameter，TCL→accent，MIX/SQL→muted。
- 无颜色模式保留方括号和加粗；主题色均为 Reset 时沿用项目反显惯例表达选中。
- 内宽不足 7 列时隐藏标签列，以全部可用宽度显示 SQL；宽高为零时跳过内容绘制。
- 整条记录只有一个点击区域，标签与正文点击结果一致。

## 3. 任务一：实现纯 SQL 类型分类器

**Files:**
- Create: `src/sql/statement_kind.rs`
- Modify: `src/sql/mod.rs`
- Test: `tests/sql_statement_kind.rs`
- Reference: `src/sql/risk.rs`、`src/sql/dialect.rs`、`src/sql/batch.rs`

**步骤：**

1. 新增表驱动测试，先覆盖普通 SELECT、INSERT、ALTER TABLE、GRANT、COMMIT、未知语句以及多语句聚合。
2. 执行 `cargo +1.94.0 test --test sql_statement_kind`，确认失败来自尚未提供的分类接口。
3. 定义公开 `SqlStatementKind`，派生 `Clone, Copy, Debug, Eq, PartialEq`；公开 `classify_statement_kind(sql: &str, dialect: SqlDialect) -> SqlStatementKind`，通过 `src/sql/mod.rs` 导出。
4. 实现私有 AST 语句分类、查询/CTE 遍历和批次聚合函数。复用 `parser_dialect()` 与 SQL Server 分批；使用明确 AST 分支覆盖 DDL，不能调用风险分类器来映射类型。
5. 加入上述边界用例，尤其是 SELECT FOR UPDATE、写 CTE、SELECT INTO、EXPLAIN 和含未知语句的批次。
6. 添加 Postgres、MySql、Oracle、Sqlite、SqlServer、Generic 的基础语句用例；MariaDB 沿用项目的 MySql 方言映射。
7. 执行 `cargo +1.94.0 test --test sql_statement_kind --test sql_risk --test mariadb_sql`。预期新分类测试和既有风险测试均通过。

**代表性测试契约：**

```rust
use lazydb::sql::{SqlDialect, SqlStatementKind, classify_statement_kind};

#[test]
fn locked_select_remains_a_query_type() {
    assert_eq!(
        classify_statement_kind("SELECT * FROM users FOR UPDATE", SqlDialect::Postgres),
        SqlStatementKind::Dql,
    );
}

#[test]
fn mixed_batch_is_not_classified_by_its_first_statement() {
    assert_eq!(
        classify_statement_kind("SELECT 1; DELETE FROM users", SqlDialect::Postgres),
        SqlStatementKind::Mixed,
    );
}
```

**完成标准：** 分类不依赖 UI、App 或数据库连接，未知语法稳定回退 SQL；既有风险规则通过回归。

## 4. 任务二：加入有界展示缓存

**Files:**
- Modify: `src/ui/sql_history_modal.rs`
- Modify: `src/ui/mod.rs`（`UiState` 和 overlay 生命周期）
- Test: `src/ui/sql_history_modal.rs` 的内部测试模块

**设计细化：** 原建议是在加载完成时分类。结合当前 `render_list(&App, &SqlHistoryState, &mut UiState)` 的接口，改用 UiState 中的展示缓存更简单：既能读取每条记录的当前方言，也能避免改动历史加载消息和模型接口。缓存命中时不再解析；它仍然是视图侧派生数据。

**步骤：**

1. 定义 `SqlHistoryPresentationCache`，由 UiState 持有；缓存包含 overlay_id、query_generation 和按 execution_id 索引的条目。
2. 条目保存准确的原始 SQL、SqlDialect、SqlStatementKind；用 SQL 内容和方言核验命中，不只依赖 UUID，不使用存在碰撞歧义的裸哈希作为正确性依据。
3. 新 overlay 或 query_generation 改变时清空缓存。
4. 可见记录第一次绘制时分类；后续相同 SQL 和方言命中已有结果。借用范围限制在查询缓存的方法调用内，避免持有 UiState 子字段可变借用时注册 hit region。
5. 每帧结束仅保留本帧可见记录；overlay 关闭时清理缓存。由此限制长 SQL 副本和分页数据带来的内存增长。
6. 使用私有可注入分类闭包或等价测试入口，验证两次相同查询只分类一次、SQL/方言变化重新分类、状态/耗时变化不重新分类，以及换 overlay / 换 generation / 淘汰记录时失效。
7. 执行 `cargo +1.94.0 test --lib sql_history_modal`，预期缓存行为测试通过。

**完成标准：** 重绘不重复解析未变的可见 SQL；来源 profile 消失导致 Generic 方言时缓存正确失效；关闭面板释放缓存。

## 5. 任务三：实现标签列和记录布局

**Files:**
- Modify: `src/ui/sql_history_modal.rs`（`render_list()` 及局部辅助函数）
- Reference: `src/ui/sql_preview.rs`、`src/ui/theme.rs`
- Test: `src/ui/sql_history_modal.rs` 的内部测试模块

**步骤：**

1. 在内部测试模块使用 ratatui TestBackend 构造含单行、多行及中文 SQL 的列表，断言首行标签、续行 x 坐标和元信息 x 坐标。
2. 抽取类型→三字符标签和类型→Theme 前景色的小型映射；不为该功能新增全套主题配置字段。
3. 统一定义标签列宽常量 6。正常宽度时正文宽度为 `inner.width - 6`，直接传入现有 `lines_without_line_numbers()`；移除原来无标签布局的减 2 魔数。
4. 在每条记录开始时计算剩余高度：小于 2 则停止，否则 SQL 行预算为 `min(3, remaining - 1)`。
5. SQL 预览至少占 1 行；元信息固定 1 行，删除其原有两个前导空格，改由正文 Rect 定位。
6. 背景绘制范围覆盖完整 row_area；首行标签使用左侧 5 列，第 6 列为间距；其余 SQL 行保持标签列空白。
7. 使用实际绘制高度推进 y，并以完整 row_area 注册 `HitTarget::SqlHistoryRow(index)`。
8. 增加 0/1/6/7 列宽度和剩余 0/1/2/3/4 行的边界测试；中文和宽字符继续沿用现有预览显示宽度算法。
9. 增加缓冲区样式断言：选中行标签区和尾部背景一致，无颜色模式仍能辨识选中；移除选中后不残留背景。
10. 执行 `cargo +1.94.0 test --lib sql_history_modal`。预期内容、边界、样式和点击区域测试通过。

**完成标准：** 无重叠、越界或宽度下溢；两行剩余空间可显示一行 SQL 和元信息；每条记录仅首行显示标签。

## 6. 任务四：联调选择、滚动及完整 SQL 行为

**Files:**
- Modify if needed: `src/ui/sql_history_modal.rs`
- Modify if needed: `src/app.rs`（SQL History 选择和滚动分支）
- Test: `tests/sql_history_interaction.rs`
- Test: `src/ui/sql_history_modal.rs` 内部测试模块

**步骤：**

1. 使用超过一屏的混合长度记录检查 ↑/↓、首尾导航和窗口缩放；新增标签导致折行增多后，当前选择应仍在可见范围内。
2. 如现有滚动路径未按实际行高处理，抽取与绘制共用的记录高度/可见范围计算，修正最小范围的偏移逻辑；避免为此新增一套通用列表框架。
3. 验证点击标签、SQL 续行和元信息均选择同一 execution_id；截断记录的区域不能覆盖下一条。
4. 验证搜索替换、分页追加去重、旧 generation 返回、空列表、加载中和加载失败状态下标签不串行或残留。
5. 复用现有复制完整 SQL 和详情测试，补充长 SQL / 前导注释 / 多语句样例，确保剪贴板和详情包含原始 SQL，不包含标签或预览省略内容。
6. 执行 `cargo +1.94.0 test --test sql_history_interaction --test sql_history_model --test sql_history_store --test sql_history_recorder`。
7. 执行 `cargo +1.94.0 test --lib sql_history` 覆盖相关模型与渲染单测。预期异步请求、持久化、复制和展示回归均通过。

**完成标准：** 标签跟随正确记录；重绘、导航和分页不会污染原始 SQL 或历史顺序。

## 7. 任务五：最终检查与视觉验收

**Files:**
- 本功能改动文件
- 本计划文档（记录实际完成项、验证结果和必要偏差）

**步骤：**

1. 执行与 CI 一致的格式检查：`cargo +1.94.0 fmt --all -- --check`。
2. 执行静态检查：`cargo +1.94.0 clippy --all-targets --all-features -- -D warnings`。
3. 执行完整 Rust 检查：`cargo +1.94.0 test --all-targets --all-features`。
4. 若环境缺少工具链、原生库或外部数据库，记录准确阻塞及未验证项；不能把环境失败报告为测试通过。数据库相关覆盖以仓库 CI 配置为准。
5. 运行 `cargo +1.94.0 run --locked`，使用已有历史或本地测试连接检查宽屏左右布局、窄屏上下布局、长 SQL、多行中文、深色/外部主题和无颜色模式；命令行选项按仓库实际 CLI 定义选择。
6. 对照本计划 2.3 的样例确认标签密度和对齐，保存实施后的界面截图用于交付说明。
7. 查看最终 diff，确认所有变更均有对应需求或回归依据；报告变更文件、检查结果及分类约定。

**预期结果：** 格式和 clippy 退出码为 0，Rust 测试通过，视觉验收确认标签清晰、选中连续、正文宽度正确。

## 8. 执行顺序与交付物

执行顺序：分类器 → 展示缓存 → 标签布局 → 交互联调 → 最终验证。

预期主要交付：

1. `src/sql/statement_kind.rs` 和公开分类接口。
2. `src/ui/sql_history_modal.rs` 的缓存、标签及布局改动。
3. `src/ui/mod.rs` 中最小的 UiState 接入。
4. 有语义价值的分类、缓存失效、渲染边界和交互回归测试。
5. 检查结果及界面截图。

如用户要求提交，建议按分类器、历史标签展示、集成回归三个逻辑单元整理提交，并使用 `@git-commit` 技能；提交前逐块检查与工作区已有修改的边界。
