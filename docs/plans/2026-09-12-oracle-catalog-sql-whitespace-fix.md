# Oracle Catalog SQL Whitespace Fix Implementation Plan

> **For Claude:** 按任务顺序逐项实施；若执行环境提供 `superpowers:executing-plans`，使用该技能。保留已有工作区修改；只有用户明确授权后才提交代码或使用子代理。

**Goal:** 修复 Oracle Explorer 展开 Tables / Views / Sequences 时因 SQL 单词粘连导致的加载失败，并以 SQL 回归测试及真实 Oracle 分页验证验收。

**Architecture:** 在 Oracle Adapter 内提取一个私有对象列表 SQL 构造函数，使用保留换行的模板，继续使用已有的字段映射、参数绑定、单查询计数、keyset 分页及 CatalogPage 校验。查询错误复用现有 SQL diagnostic；Oracle 11g 兼容性作为明确版本后才启用的条件分支。

**Tech Stack:** Rust 2024 / MSRV 1.94、oracle 0.6.3、Tokio spawn_blocking、现有 CatalogRequest / CatalogPage 协议。

---

## 1. 事实、范围与约束

### 已确认事实

- 截图中 Schema 分组已有对象数量，但展开 Tables 后失败；错误文本在 `ORA-0090…` 处被截断，不能据此认定完整错误码。
- `src/db/oracle.rs::oracle_group_page` 使用独立的 COUNT 查询，续行前保留了空格。
- `src/db/oracle.rs::oracle_object_page` 的 SQL 模板约位于 796–812 行，使用没有尾部空格的反斜杠续行。
- Rust 会移除反斜杠后的换行及下一行缩进，实际 SQL 会出现 `all_tablesWHERE`、`:2ORDER`、`cLEFT JOIN`、`1ORDER BY` 等错误片段。
- Tables、Views、Sequences 共用该对象页加载函数；三个分组的字典字段映射当前正确。
- 现有单元测试验证字段映射和内存分页，没有覆盖实际 SQL 构造；集成测试缺少凭据或遇到 DPI-1047 会提前返回。

### 实施范围

必改文件：

1. `src/db/oracle.rs`：SQL helper、调用替换、查询诊断、针对性单元测试。
2. `tests/oracle_adapter.rs`：专门的目录分页集成验收。

默认不需要修改共享 Catalog 协议、App reducer 或 UI。只有实测发现与本缺陷直接相关的问题才追加修改，并说明原因。

保持以下行为：

- schema / cursor 使用绑定参数，数据库标识符来自固定映射。
- 首页两个位置绑定；续页三个位置绑定；不能把重复 owner 占位符合并后仍沿用旧的绑定列表。
- count 不受 cursor 筛选影响；count 与页面由同一条 SQL 获取。
- 页面只取 `page_size + 1` 个名称，由 `finalize_oracle_object_names` 决定裁剪及下一页游标。
- 空页面通过 LEFT JOIN 返回计数占位行，`None` 名称不构建 CatalogEntry。
- SQL 排序与游标过滤继续使用 `NLSSORT(..., 'NLS_SORT=BINARY')`。

本计划是已有 `2026-09-12-oracle-explorer-counts-pagination-implementation.md` 的针对性缺陷修复，不重新实施其已完成的计数、映射或 reducer 改动，也不覆盖该文档。

## 2. Task 1：确认基线与真实验收条件

**Files:** 检查 `src/db/oracle.rs`、`tests/oracle_adapter.rs`、`Cargo.toml`。

1. 执行 `git status --short`，记录已有修改及未跟踪文档。
2. 按符号确认 `oracle_object_page`、`oracle_object_group`、`oracle_error_with_query` 的当前实现；行号仅作定位提示。
3. 执行当前 Oracle 单元测试，记录基线：

   ```bash
   cargo test --features driver-oracle --lib db::oracle
   ```

   预期：现有纯逻辑测试通过；此时通过不代表生成的 SQL 正确。

4. 检查测试凭据是否存在，只报告变量是否配置，不打印值：`LAZYDB_TEST_ORACLE_URL`、`LAZYDB_TEST_ORACLE_USER`、`LAZYDB_TEST_ORACLE_PASSWORD`。
5. 如有可用连接，通过现有 probe 或只读查询确认 Oracle 服务端版本，并记录完整错误码。可用版本查询：

   ```sql
   SELECT version
   FROM product_component_version
   WHERE product LIKE 'Oracle Database%'
   ```

6. 没有真实 Oracle 环境时继续本地修复；将数据库验收标记为未完成，不因环境不足阻塞 SQL 单元回归。

**完成标准：** 已区分确定的字符串缺陷、未知服务端版本和真实数据库验证条件。

## 3. Task 2：提取 SQL 构造点并建立可失败的回归测试

**Files:** 修改 `src/db/oracle.rs`，测试放入现有 `#[cfg(all(test, feature = "driver-oracle"))]` 对应测试模块（以当前实际 cfg 为准）。

### Step 1：机械提取现有 SQL，暂不修复模板

增加私有 helper：

```rust
#[cfg(feature = "driver-oracle")]
fn oracle_object_page_sql(
    description: OracleObjectGroup,
    has_cursor: bool,
    limit: usize,
) -> String
```

把当前 `cursor_predicate` 和 `format!` 原样移入 helper，使测试使用与运行时完全相同的 SQL。`oracle_object_page` 保留游标解码和绑定列表，并改为：

```rust
let cursor = oracle_object_cursor(request)?;
let limit = request.page_size.saturating_add(1);
let sql = oracle_object_page_sql(description, cursor.is_some(), limit);
```

### Step 2：增加 SQL 词法边界回归测试

新增测试 `oracle_object_page_sql_preserves_token_boundaries`，遍历三个分组和 `has_cursor = false / true` 六种组合。

对 helper 结果执行 `split_whitespace().collect::<Vec<_>>().join(" ")`，只归一化空白，不修复缺失的分隔符。验证关键边界：

```rust
let normalized = sql.split_whitespace().collect::<Vec<_>>().join(" ");
assert!(normalized.contains(&format!(
    "FROM {} WHERE {} = :2",
    description.dictionary, description.owner_column,
)));
assert!(normalized.contains("FROM object_count c LEFT JOIN page_rows p"));
assert!(normalized.contains("ON 1 = 1 ORDER BY"));
```

首页额外验证 `= :2 ORDER BY`。续页验证存在完整的 `NLSSORT(:3, 'NLS_SORT=BINARY')` 游标比较，并在其后有独立的 ORDER BY。两类查询都验证字典对应的名称列及 `FETCH FIRST 11 ROWS ONLY`。

这是针对已知 SQL 词法缺陷的测试，不把整条模板复制成快照；真实语法及绑定行为交给 Task 5 的 Oracle 实测。

### Step 3：确认测试能够暴露原始缺陷

```bash
cargo test --features driver-oracle --lib db::oracle::tests::oracle_object_page_sql_preserves_token_boundaries
```

预期：FAIL，失败原因是上述 SQL 单词边界缺失。若测试未失败，先检查是否测试了实际运行时 helper、或在提取阶段已经意外修复。

## 4. Task 3：替换为保留换行的 SQL 模板

**Files:** `src/db/oracle.rs::oracle_object_page_sql`。

使用以下完整 helper 实现修复：

```rust
#[cfg(feature = "driver-oracle")]
fn oracle_object_page_sql(
    description: OracleObjectGroup,
    has_cursor: bool,
    limit: usize,
) -> String {
    let cursor_predicate = if has_cursor {
        format!(
            " AND NLSSORT({name}, 'NLS_SORT=BINARY') > NLSSORT(:3, 'NLS_SORT=BINARY')",
            name = description.name_column,
        )
    } else {
        String::new()
    };

    format!(
        r#"
WITH object_count AS (
    SELECT COUNT(*) AS total_count
    FROM {dictionary}
    WHERE {owner_column} = :1
), page_rows AS (
    SELECT {name_column} AS object_name
    FROM {dictionary}
    WHERE {owner_column} = :2{cursor_predicate}
    ORDER BY NLSSORT({name_column}, 'NLS_SORT=BINARY')
    FETCH FIRST {limit} ROWS ONLY
)
SELECT c.total_count, p.object_name
FROM object_count c
LEFT JOIN page_rows p ON 1 = 1
ORDER BY NLSSORT(p.object_name, 'NLS_SORT=BINARY')
"#,
        dictionary = description.dictionary,
        owner_column = description.owner_column,
        name_column = description.name_column,
    )
}
```

`limit` 继续来自经过验证的请求；helper 不接收用户 SQL 标识符或参数值。

执行：

```bash
cargo test --features driver-oracle --lib db::oracle
```

预期：新增回归测试及现有映射、游标、分页边界测试全部通过。

**完成标准：** 运行时六种对象查询组合均经过修复后的 helper；没有新增依赖、版本状态或多余的 SQL 后处理。

## 5. Task 4：复用现有查询诊断

**Files:** `src/db/oracle.rs::oracle_object_page`。

1. 查询调用保持现有绑定顺序，并替换错误转换：

   ```rust
   let rows = match cursor {
       Some((_, cursor_name)) => connection.query(&sql, &[owner, owner, &cursor_name]),
       None => connection.query(&sql, &[owner, owner]),
   }
   .map_err(|error| oracle_error_with_query(error, &sql))?;
   ```

2. 行迭代阶段可能延迟报告数据库执行错误，将 `let row = row.map_err(oracle_error)?;` 改为：

   ```rust
   let row = row.map_err(|error| oracle_error_with_query(error, &sql))?;
   ```

3. 保留字段解码和 CatalogPage 验证的现有错误处理，不把所有内部错误统一包装为 SQL 执行失败。
4. 检查 diagnostic 使用的是带占位符的 SQL，不插入密码、连接 URL 或绑定参数值。
5. 复用现有 helper，不为这一薄包装新增重复单元测试。执行最终 Oracle 单元测试时一并验证编译；真实错误展示若仍被窄窗口截断，记录为后续可观测性问题，不在本次扩展 UI。

**完成标准：** 对象查询及拉取错误保留原错误分类/错误码，并附加经过现有终端清理的 SQL context。不能声称仅增加 diagnostic 就已让 UI 展示完整错误。

## 6. Task 5：真实 Oracle 目录分页验收

**Files:** `tests/oracle_adapter.rs`。

### Step 1：新增明确的集成验收入口

新增异步测试 `oracle_catalog_sql_pages_all_object_groups`，使用 `#[tokio::test]`，并添加 `#[ignore = "requires Oracle credentials, Instant Client, and a stable catalog fixture"]`。测试按以下步骤实现：

1. 读取现有三个测试环境变量；此专用验收测试被显式运行时，缺变量必须明确失败，不静默 return。
2. 使用现有 `import_connection_url` 和 `DatabaseConnection::connect` 模式；DPI-1047 同样失败并说明客户端不可用。
3. 不在错误中输出 URL、用户名、密码或原始配置内容。
4. 复用现有 discovery / catalog 请求方式确定目标 schema，不硬编码截图中的服务名或 schema。
5. 独立加载 Groups，记录三个分组的 `CatalogCount::Exact`。

### Step 2：遍历三个对象组

对 Tables、Views、Sequences 分别执行：

- 首页 `page_size = 2`、`cursor = None`。
- 每次接收页面后调用 `validate_for(&request)`。
- 验证 `entries.len() <= 2`、分组对应 kind、parent/schema 正确。
- 每页 `total_count` 必须是 Exact，且在稳定 fixture 中等于该分组的初始 count。
- 用集合记录对象身份，重复插入即失败。
- 有下一页时更新 `request_id` 和 cursor；记录 cursor，重复 cursor 立即失败，防止测试无限循环。
- 对最终全部对象数量和初始 count 做一致性断言。
- 同时与预置 fixture 清单或同口径独立字典查询得到的名称集合比较，不只与应用自己的计数比较。
- 清理连接。测试不创建、删除或修改业务对象。

### Step 3：使用稳定 fixture 验证边界

fixture 由专用测试环境预置，不在截图连接上自动建删对象：

| 场景（P=2） | 验收点 |
| --- | --- |
| N=0 | 空 entries、Exact(0)、无 next_cursor |
| N=1 | 一页完整返回 |
| N=2 | 恰好一页，无 next_cursor |
| N=3 | 第一页有 cursor，第二页完整结束 |
| N>=5 | 至少三页，无重复、无遗漏 |
| 名称带点号、空格、大小写或非 ASCII | 名称不被拆分，游标可继续 |

并非每个环境都具备所有边界 fixture；实际验收报告逐项列出已覆盖及缺失项，不能用纯内存分页测试替代 SQL 语义验收。

### Step 4：显式执行

```bash
cargo test --features driver-oracle --test oracle_adapter oracle_catalog_sql_pages_all_object_groups -- --ignored --exact --nocapture
```

预期：确实连接成功并遍历三个分组；缺少环境或客户端为明确失败。默认测试套件保持忽略该外部依赖验收测试。

### Step 5：TUI 验收

启动修复后的程序，对原连接执行刷新：

1. 展开 Schema，确认三个分组数量可见。
2. 展开 Tables，确认出现对象列表，原 Retry 错误不再出现。
3. 对 Views 和 Sequences 重复操作。
4. 加载下一页并刷新分组，确认分页和计数没有回归。
5. 记录实际服务端版本和完整错误（若仍失败），不要把第一次代码修复成功等同于数据库验收完成。

## 7. 条件分支：服务端为 Oracle 11g 时

**触发条件：** 明确确认目标是 11g，且产品需要支持该版本。版本未知时先完成已确认的空白缺陷修复，不按截图猜测版本。

`FETCH FIRST` 不适用于 11g。需要兼容时，将同一个 SQL helper 的 `page_rows` 改为：

```sql
page_rows AS (
    SELECT object_name
    FROM (
        SELECT {name_column} AS object_name
        FROM {dictionary}
        WHERE {owner_column} = :2{cursor_predicate}
        ORDER BY NLSSORT({name_column}, 'NLS_SORT=BINARY')
    )
    WHERE ROWNUM <= {limit}
)
```

- cursor 筛选和排序在内层，ROWNUM 限制在外层；禁止把 ROWNUM 与 ORDER BY 放在同一层。
- 保留 count CTE、LEFT JOIN 和最终排序。
- 调整 SQL 边界测试的限行断言，并在 11g 与一个已支持的新版本上运行 Task 5。
- 对此简单目录查询优先采用共同兼容的 SQL，不引入版本缓存或“失败后换语法重试”的隐藏状态。
- `OracleAdapter::preview_relation` 也使用 OFFSET/FETCH；若要宣称整体支持 11g，必须另行评估数据预览及查询分页。未完成前只能声明目录查询兼容，不能声明完整 11g 支持。

## 8. Task 6：最终检查与交付

完成所有代码修改后运行一次定向验证，避免没有新修改时反复跑大范围测试：

```bash
cargo test --features driver-oracle --lib db::oracle
cargo test --test catalog_contract
cargo fmt --all -- --check
cargo check --all-targets --features driver-oracle
cargo check --all-targets --no-default-features
git diff --check
git diff --stat
git status --short
```

预期：Oracle 定向测试、Catalog 契约测试和两种 feature 构建通过。若现有文件存在无关格式问题，记录并区分，不批量重写无关文件。

真实 Oracle 验收使用 Task 5 的显式命令单独报告；普通测试绿色不代表数据库验收成功。

### 完成标准

- 六种对象 SQL 组合不再发生 token 粘连。
- 绑定列表保持正确，表/视图/序列字段映射正确。
- 单次页面请求依然单查询、带全组计数、最多获取 P+1 个真实名称。
- 计数占位空行和 keyset 分页机制保持正确。
- 查询/拉取错误携带 SQL context，且不包含连接凭据。
- 默认 Oracle feature 和无 Oracle feature 均可编译。
- 实际 Oracle 版本、执行测试、覆盖边界及未验证项目有明确记录。
- 没有覆盖已有计划或用户修改，没有未经授权的 commit / push。

### 交付报告模板

1. 修改文件与关键修复。
2. 原始失败测试及修复后结果。
3. 本地测试/构建命令与结果。
4. Oracle 实测版本、三个分组及分页验收结果。
5. 未验证内容及原因；是否触发 11g 条件分支。

## 9. 参考依据

- Rust 字符串续行语义：https://doc.rust-lang.org/reference/expressions/literal-expr.html#string-continuation-escapes
- Oracle ROWNUM 与先排序后限行：https://docs.oracle.com/en/database/oracle/oracle-database/19/sqlrf/ROWNUM-Pseudocolumn.html
- Oracle SELECT / row_limiting_clause：https://docs.oracle.com/en/database/oracle/oracle-database/19/sqlrf/SELECT.html
