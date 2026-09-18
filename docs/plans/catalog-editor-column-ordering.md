# Table Editor Column Order Implementation Plan

> **执行者：Luna。** 按端到端验收单元连续实施；实现、审查、纠偏、提交合并均由 Luna 完成。Astra 只负责分析和计划。不启动子 Agent，不要求用户逐项 resume。

**Goal:** 让 Columns 的 COMMENT 占满剩余宽度，支持 A 在上方新增、J/K 调序，并将可支持的目标列序真正保存到数据库。

**Architecture:** `TableDraft.columns` 的有效列序列是唯一目标顺序，保留每列原始身份与 ordinal；独立的插入/调序动作贯通 keymap、reducer、dirty summary 和 SQL Review。MySQL/MariaDB 原生 FIRST/AFTER 保存，SQLite 复用保真重建；各适配器对不能保存的布局在规划阶段明确返回错误，禁止静默丢顺序。

**Tech Stack:** Rust 2024 / Rust 1.94、Crossterm 0.29、Ratatui 0.30.2、SQLx、现有 CatalogMutationPlan/Runtime、TestBackend 与数据库适配器测试。

---

## 0. 基线、产物及执行约定

- 工作空间：`/Users/yelog/workspace/tui/lazydb`；目标 main；计划核验 HEAD：`84c5df482680bdaad719afdf1e7719e3b02cb78f`。
- 任务目录：`/Users/yelog/workspace/tui/lazydb/.git/opencode-tasks/ses_f4c6465b5ffeoj3Cp3MDjtlw2C`。
- 分析文件：任务目录 `analysis.md`；本计划使用新文件名 `table-editor-column-order`，避免覆盖上一任务的 `table-editor-columns` 计划。
- 任务名和工作分支由后续流程自动命名；本阶段不创建 worktree、不改业务代码、不提交。
- 截至 plan 开始 checkpoint 不存在。实现/恢复时读取当时 checkpoint、实际 diff 和本计划，不照搬旧日志 next；不修改 state/checkpoint，不覆盖历史回执，只写当阶段 activeReceipt。
- 五个已有未跟踪计划文档均为其他任务产物，保留且不带入提交。本文件是本任务新增计划。
- 分析阶段基线测试 `cargo +1.94.0 test --test catalog_editor_state` 为 63 passed；这是旧代码证据，不得冒充实现后通过。
- 每单元按“建立必要回归 → 实现 → 定向验证 → 记录结果 → 下一单元”推进。业务闭环完成后可按工作流授权形成原子提交；没有授权时保留 diff，不擅自提交或合并。
- 下文命令的预期均是将来验收要求，不是已执行结果。发生新修改或失败时才重跑相关检查，不反复全量 check/clippy/test。

## 1. 产品契约与数据库能力矩阵

### 1.1 交互

| 输入/情形 | 结果 |
|---|---|
| Columns a | 在当前可见行下方打开新列详情，确认才插入 |
| Columns A | 在当前可见行上方打开新列详情，确认才插入 |
| 空列表 a/A | 插入位置为 0 |
| Columns J/K | 当前有效列下移/上移一个有效列位置；选择跟随 row_id |
| Removed 当前行 J/K | 无操作；有效列调序跳过 Removed 行 |
| 首尾 J/K | 无操作，不环绕、不改变焦点 |
| 小写 j/k、箭头 | 原有移动选择/焦点行为 |
| 详情打开 | 所有编辑仍针对 session，禁止父列表调序 |
| A/J/K + NONE 或 SHIFT | 都识别为大写快捷键 |
| CTRL/ALT 等修饰 A/J/K | 不触发新增或调序 |
| General/详情文本 A/J/K | 正常文本输入 |
| busy / SQL Preview | 保持既有拦截，不修改草稿 |
| 调序后恢复原序 | 没有其他变更时 dirty 消失 |

Removed 行占位保持在 Vec 中。有效列调序交换相邻有效行的整个 ColumnDraft，Removed 槽位不动；恢复删除后，该列恢复在当前可见位置。此定义既保留删除恢复上下文，又不把待删除对象的顺序作为数据库变更。

### 1.2 数据库保存能力

| 模式/后端 | 本任务实现及验收 |
|---|---|
| 所有当前支持 CREATE TABLE 的后端 | 按草稿有效列序生成 CREATE；A/J/K 的最终顺序保存 |
| MySQL/MariaDB EDIT | 新增位置、已有列调序、删除和改名组合用原生 ALTER 保存；保留既有列定义属性 |
| SQLite EDIT | 新增及纯调序触发已有重建；按列名复制数据并保存最终顺序 |
| PostgreSQL EDIT | 可保存原有存活列原序 + 末尾新增列；不能保存的中间插入/既有列调序在 Review 返回具体错误并保留草稿 |
| SQL Server/Oracle EDIT | 当前表编辑仅支持表改名；包含本次列新增/顺序变动的请求明确拒绝，不能只改名后成功 |

本矩阵是技术能力边界，不是宣称实现了所有数据库的任意物理重排。不要用本地偏好代替数据库顺序，也不在本任务引入 PostgreSQL/SQL Server/Oracle 通用表重建系统。最终交付必须披露受限后端。

## 2. 单元一：目标列序的交互、dirty 和布局闭环

**修改文件**
- `src/action.rs`：新增 above / reorder 动作。
- `src/input/keymap.rs::map_table_editor`：大写快捷键及修饰键作用域，catalog pending 清理。
- `src/app.rs`：Catalog Editor reducer 分支（基线约 7781–7800）。
- `src/model/catalog_editor.rs`：TableDraft、TableChangeSummary、插入/调序/顺序比较 helpers。
- `src/ui/catalog_editor.rs`：table_column_widths、summary、table_shortcut_hints。
- `tests/catalog_editor_state.rs`、`tests/catalog_editor_reducer.rs`、`tests/ui_render.rs`；keymap 文件现有局部测试。

### 步骤 2.1：建立有意义的模型回归

复用当前三列 fixture，原始顺序 `id,name,email`；保存其 row_id、existing_name、ordinal_position。

新增测试聚焦生命周期，不逐个镜像 enum：

1. 选中 name，A 打开 session 的 insert_at=1；确认 `age` 后为 `id,age,name,email`，取消则仍为原序。首行/空列表使用同一参数化用例。
2. 选中 name，J 后 `id,email,name` 且仍选中 name；K 返回原序。首尾无操作，身份/内容/状态完全保持。
3. Removed 中间槽位、Added 行、无效 selection、详情打开、零/一列；调序跳过 Removed，详情打开时无操作。
4. 纯调序 summary dirty，恢复顺序后 clean；单纯改列名不算 order changed，删除造成的位置收缩不算 order changed。
5. Review reducer 发出的 PlanCatalogMutation 持有目标 Vec 序列；不是只检测 UI。

运行 `cargo +1.94.0 test --test catalog_editor_state` 和新增 reducer 的精确过滤器；先确认失败来自行为缺失，不来自无效 fixture。

### 步骤 2.2：实现模型与独立动作

建议动作：`CatalogEditorAddTableColumnAbove`、`CatalogEditorReorderTableColumn(isize)`。

- 不改变 `CatalogEditorMoveTableColumn` / `move_column` 的“移动选择”语义。
- 抽取 `begin_add_column_at(insert_at)`；above 为 selected.min(len)，below 为非空时 min(selected,last)+1，空时 0。继续使用 `TableColumnEditTarget::New { insert_at }`，沿用 max ordinal+1 和确认/取消流程。
- `reorder_selected_column(delta)` 只接受方向、寻找同方向相邻非 Removed 列并交换整个结构；边界或 session 存在时 false，无副作用；成功更新 selected_column，保持 Columns 焦点。
- `column_order_changed` 纳入 TableChangeSummary::is_dirty，并在 UI 摘要显示明确的 `column order changed`。
- 对比顺序时使用 existing_name；保持原始 ordinal_position 和 row_id。不要给每次移动重编号。

建议提炼两种语义，不能混用：

```text
existing_order_changed =
  baseline_existing_names.filter(name survives)
    != draft.active_columns.filter(existing).map(existing_name)

append_layout_matches =
  baseline_existing_names.filter(name survives).map(Existing)
    + draft.active_columns.filter(added).map(Added(row_id))
    == draft.active_columns.map(identity)
```

第一种用于已有列相对重排和 dirty；第二种用于 PostgreSQL 是否能通过 DROP/ADD 达成目标。Added/Removed 已经使 summary dirty，不依赖 order flag 才保护退出。

### 步骤 2.3：输入和 reducer 接线

- 在 Columns 作用域识别 A/J/K，允许 NONE 或 SHIFT。先处理焦点与详情，再做大小写快捷键分发，不能把 General/详情文本抢走。
- 现有前置 guard 拒绝 SHIFT，须精确调整；不要全局清除 modifiers 或把 CTRL+J 当 J。
- reducer 调用新的模型方法，沿用 finish_edit_group，清除过期错误/计划的方式与周边字段变更保持一致；检查 Form/非 busy 的直接 Action 边界。
- dd 的 pending 在 A/J/K 操作后失效。测试 `d,J,d` 不删除，文本上下文大写保持输入。
- a/按钮 Add Column 保持原下方新增语义；不增加额外确认步骤。

### 步骤 2.4：COMMENT 分配真实剩余宽度

基线 `table_column_widths:2123` 的问题是四列均 clamp 到 40、没有补余量；不能只换 Constraint::Fill，因为正文已经按旧 comment_width 截断。

算法：

1. 沿用 sanitize_terminal_text 与 unicode-width，扫描全部草稿行测量前三列，保留内容理想值/最小值/40 上限。
2. budget = viewport.saturating_sub(序号槽+四个分隔符)。本单元保留现有序号槽策略，不扩展为通用表格重构。
3. 正常尺寸先预留 COMMENT 最低宽度，再确定性收缩前三列，直到前三列 + COMMENT minimum <= budget。
4. COMMENT = budget - 前三列之和，取消 COMMENT 的 40 最大值。
5. 若 budget 小于正常 minimum 总和，按固定优先级降级为可用宽度，确保总和 <= budget；0 宽不 panic。
6. constraints 和 truncate_cells 消费同一最终宽度；表头/正文分隔符位置一致。不要改 Added/Removed 背景、hit region 或焦点标记规则。

测试实际 Buffer 的右侧 COMMENT 内容而非只测返回数字：宽窗口中 40 字符后的文字可见，短 comment 的列仍延伸到表格右边，前三列没有被拉伸。覆盖 106×34、80×24、56×16 及零/极窄 helper；Unicode/控制字符使用已有投影。

### 步骤 2.5：提示及单元验收

- Columns hints 包含 a add below、A add above、J/K reorder；保留 j/k、dd、r、Esc。
- 沿用多行 hints，验证实际 footer 高度变化后列表、动作、命中区域不重叠，56×16 仍能看见选中行。

```sh
cargo +1.94.0 test --test catalog_editor_state
cargo +1.94.0 test --test catalog_editor_reducer
cargo +1.94.0 test --lib input::keymap
cargo +1.94.0 test --test ui_render table_editor
cargo +1.94.0 test --test ui_render table_column_details
```

预期：相关测试实际执行且全部通过。可形成 `feat(catalog): support column insertion and reordering`，但本单元仅证明草稿闭环，不能宣称数据库保存已完成。

## 3. 单元二：SQLite 真实保存与 CREATE 顺序闭环

**修改文件**
- `src/db/sqlite.rs::sqlite_table_requires_rebuild`、必要的 create 分支。
- `src/db/postgres.rs`、`src/db/mysql.rs`、`src/db/mssql.rs`、`src/db/oracle.rs` 的 CREATE 分支：仅在存在有效列过滤缺口时修改。
- `tests/sqlite_adapter.rs`、`tests/object_mutation_contract.rs`、`tests/catalog_mutation.rs`。

### 步骤 3.1：建立本地 SQLite round-trip

用现有内存 SQLite adapter fixture 建表 `items(id INTEGER PRIMARY KEY, name TEXT, score INTEGER)`，插入区分明显的数据，例如 `(7,'Ada',42)`。载入真实 baseline，从草稿重排为 `score,id,name`，计划、执行、重新载入。

断言：
- 纯重排生成 rebuild 而不是 NoChanges；重建 CREATE 列序准确。
- `PRAGMA table_info` / definition 列序为 score,id,name。
- 显式查询 id/name/score 值不变，SELECT * 结果与新顺序匹配。
- 再次以载入结果建草稿为 clean。
- 新增 nullable/default 列到首位/中间后值和顺序正确。

### 步骤 3.2：触发重建并验证保真

- sqlite_table_requires_rebuild 复用 shared existing-order 比较，不用数字 ordinal 相等判定。
- 沿用 sqlite_rebuild_create_sql 中的目标 Vec 顺序和 `(existing_name,new_name)` 映射；保持 INSERT target-list 与 SELECT source-list 成对生成。
- 复用已有事务、索引/触发器恢复和 snapshot，不再创造第二套重建 executor。
- 扩展实际执行测试覆盖普通索引/触发器、约束、表选项及失败回滚。重排同时改名若触及已有依赖重写缺口，必须在 Review 阶段明确拒绝该组合或完整处理所需引用，不能执行一半才假成功。

### 步骤 3.3：CREATE 顺序契约

检查每个 adapter 的 CREATE TABLE 使用草稿有效序列、排除 Removed。用同一目标 `b,new,a` 验证各方言生成顺序，不通过把字符串按逗号 split 的方式断言列定义（类型和表达式可含逗号）。现有 parser 或明确 quoted identifier 的位置足够。

```sh
cargo +1.94.0 test --test sqlite_adapter
cargo +1.94.0 test --test object_mutation_contract
cargo +1.94.0 test --test catalog_mutation
```

若 SQLite fixture 放在内部模块，运行该模块具体测试过滤器并记录非零执行数。此单元形成可独立验收的真实数据库保存证据。可提交 `fix(sqlite): persist table column order through rebuilds`。

## 4. 单元三：MySQL/MariaDB 保真新增与顺序保存闭环

**修改/新增文件**
- 新增 `src/db/mysql/table_mutation.rs`：snapshot、原始列片段提取、目标序列 diff 和 SQL 规划 helper，避免继续扩张 mysql.rs。
- `src/db/mysql.rs`：声明子模块，接入 definition loading 和 table edit planning。
- `tests/object_mutation_contract.rs`：无服务的 planner 契约。
- `tests/mysql_adapter.rs`、`tests/mariadb_catalog_mutation.rs`：真实 round-trip。
- 参考但尽量不修改：`src/runtime.rs:2554–2663`，其中已有 baseline_fingerprint 的执行前重新载入比较。

### 步骤 4.1：确定 snapshot 表示并建立解析回归

选定适配器私有版本化 snapshot，沿用 SQLite 将可序列化快照承载于 TableDefinition.baseline_fingerprint 的既有模式，避免给公共 TableDefinition 增加字段后修改所有 fixture。

建议私有结构：

```text
MySqlTableSnapshotV1 {
  database, schema, name,
  columns: Vec<{name, definition_sql}>,
  structural_sql: String
}
fingerprint prefix = "mysql-table-v1:"
```

- definition_sql 保留完整原生列定义，包括引用过的列名；structural_sql 是稳定的结构快照，用于发现索引/约束等外部 DDL 改变。
- SHOW CREATE TABLE 的表级 AUTO_INCREMENT 下一计数随普通 INSERT 改变，不能原样纳入结构指纹导致无关数据写入判 stale。仅在顶层表选项语境规范化该计数，不删除列上的 AUTO_INCREMENT，也不修改字符串中的同名词。snapshot 序列化必须确定性，不使用无序 Map。
- definition loader 仍保留现有 ColumnDefinition UI 元数据；补充 SHOW CREATE TABLE 原始结构来源。

解析 helper 用词法状态跟踪引号、反引号转义、注释、括号深度，只在表列列表顶层逗号分割。不得全局 split(',')。对无法识别的输入返回具体错误，不返回不完整快照。

必要 fixture：DECIMAL(10,2)、ENUM('a,b','c')、字符串转义、名称含反引号、CURRENT_TIMESTAMP(6)、ON UPDATE、生成表达式含括号/逗号、VIRTUAL/STORED/PERSISTENT、COLLATE、AUTO_INCREMENT、INVISIBLE、CHECK 和带名字的约束、MySQL versioned comment。只需要提取列片段，未知片段不应被误当作列或静默丢弃。

将框架无服务回归先写在新模块 `#[cfg(test)]`。实现期间查具体 SQL 语法时使用官方当前文档；不要凭 PostgreSQL formatter 猜 MySQL 语法。

### 步骤 4.2：载入与运行时 stale 链路

- 在现有 MySQL definition loading 的同一连接上读取 SHOW CREATE TABLE，使用已有 quote_identifier 正确转义 schema/name。
- 构建新 snapshot fingerprint；验证取出的列名与信息表列名一一对应，避免使用错表/不完整定义。
- 计划携带 Some(baseline.baseline_fingerprint)；Runtime 已在执行前重新载入比较，复用它而不是再做无效的第二轮实现。
- 更改现有仅改名计划的 None fingerprint 为实际 baseline；旧无 snapshot 的 fixture 若需要修改列则显式失败，测试改用真实格式 fixture。纯 rename 的兼容路径可保留，但必须保持校验语义清楚。
- 普通行插入改变自增计数不导致结构 stale；新增外部列/修改定义导致 stale。数据并发与 DDL 竞争窗口按现有执行架构处理，不声称新增了原子 DDL 锁。

### 步骤 4.3：建立目标序列 planner 测试

覆盖至少以下表驱动情形：

| baseline | target | 期望 |
|---|---|---|
| a,b,c | c,a,b | 对 c 的定位含 FIRST，结果无属性损失 |
| a,b,c | a,c,b | b 或 c 的最小必要移动，AFTER 引用存在的前驱 |
| a,b | n,a,b | ADD n FIRST |
| a,b | a,n,b | ADD n AFTER a |
| a,b | a,n,m,b | 新增动作按目标顺序，m AFTER n |
| a,b,c | c,a（b removed） | DROP b 与重排共同保存 |
| a,b | renamed_b,a | CHANGE 或等效改名后定位，引用新的名称 |
| a,b | a,b | 无其他变更时 NoChanges |
| a,b | 新表名且 b,a | 同一计划同时保存表名和列序 |

为每个计划模拟执行动作后的名称序列并比较 target，避免测试只断言 SQL 包含 FIRST。

### 步骤 4.4：实现 ALTER 规划

1. draft.validate，核对 object/baseline 身份、schema 和快照。
2. 识别 Removed、Added、Existing；按 existing_name 匹配 baseline，改名不当成新增。
3. 保留存活原列的模拟顺序；先安排删除，再从目标有效 Vec 左到右处理。
4. 新列生成 MySQL 方言完整 definition，并在必要时附 FIRST/AFTER。
5. 已有列位置变更且属性不变时直接复用 snapshot 完整 definition，生成 MODIFY COLUMN；名称变化生成 CHANGE COLUMN old-name new-definition，保留其余属性。
6. 已有列字段被编辑时只替换被编辑的语义片段，保留未知原生片段；无法保证保真的组合在规划阶段明确拒绝。不要把简化 ColumnDefinition 无条件 stringify，不能将默认字面量误作表达式或抹掉 ON UPDATE。
7. 对未知或暂不支持的 table owner/schema/index/constraint 等变化返回具体 InvalidDraft，避免“列顺序成功”掩盖其他草稿字段被丢弃。不要为此扩展通用对象编辑系统。
8. 尽可能在一次 ALTER TABLE 中组合列动作和 RENAME TO；使用现有 Autocommit 模式，不把多条 DDL 声称为可回滚事务。
9. DROP COLUMN 沿用既有 destructive 标识和 Review 流程；纯顺序变更不擅自增加新的审批 UI。
10. 计划 refresh_targets 包含表对象列表及 RelationChildren，设置现有 CatalogMutationImpact 的 owning_relation_id、旧/新身份，复用 App 成功分支的缓存失效链路。

generated 列相互依赖等原生服务器限制允许在已知情况下提前报清楚错误；若服务拒绝 DDL，保留原始错误与草稿，不回退为尾部新增。任意名称交换/循环改名不是本次必要新增能力，无法支持时规划阶段拒绝，不制造无效依赖顺序。

### 步骤 4.5：真实服务 round-trip

MySQL 和 MariaDB 都使用各自测试 URL，不以一个后端通过代替另一个。

测试建带以下特性的表：主键自增、文本默认值、时间默认表达式及 ON UPDATE、带 COLLATE/comment 的列、生成列、普通索引。选取服务器版本确实支持的特性，不假定两者语法完全一致。

从真实 loader 获取 baseline → A/目标草稿插入新列 → J/K/模型调序 → Review 计划 → execute → 重新读取 definition / SHOW CREATE / SELECT。

必须断言：目标名称序列、原始数据按名称保留、默认/自增/生成语义与索引保持、重新打开 clean、表改名+列序共同生效；对不支持的属性组合验证明确失败且没有部分成功提示。

```sh
cargo +1.94.0 test --lib db::mysql::table_mutation
cargo +1.94.0 test --test object_mutation_contract
cargo +1.94.0 test --test mysql_adapter
cargo +1.94.0 test --test mariadb_catalog_mutation
```

服务 URL 缺失而测试 return 必须记录为未执行数据库检查。无服务 planner/snapshot 测试仍要完成；不能据此宣称真实 MySQL/MariaDB round-trip 通过。可形成 `feat(mysql): persist table column positions without losing definitions`。

## 5. 单元四：能力边界、刷新与组合场景闭环

**修改文件**
- `src/db/postgres.rs::plan_table_mutation`。
- `src/db/mssql.rs::plan_catalog_mutation`、`src/db/oracle.rs::plan_edit`。
- `src/model/catalog_editor.rs` / `src/db/catalog_mutation.rs`：只在需要时集中能力判定；避免到处复制数据库 match。
- `src/ui/catalog_editor.rs`：必要的明确能力说明，复用现有错误呈现。
- `src/app.rs`：仅当新 impact 不能触发正确刷新时补齐。
- `tests/catalog_mutation.rs`、`tests/object_mutation_contract.rs`、`tests/catalog_editor_reducer.rs`、`tests/postgres_adapter.rs`。

### 步骤 5.1：PostgreSQL 布局可达性

- CREATE 无需限制。
- EDIT 在生成任何 SQL 之前比较 append_layout_matches；若不一致返回 InvalidDraft，说明 PostgreSQL 当前表编辑不支持定位/重排已有表列，保留草稿供调整。
- 接受删除后原列保序、末尾新增、末尾新增之间调整顺序；ADD 按目标新增序列生成。
- 测试：`a,b -> b,a` 拒绝；`a,b -> a,n,b` 拒绝；`a,b -> a,b,n,m` 接受；`a,b,c -> a,c,n` 接受；表改名+非法布局整单拒绝。
- 错误可修复：用户把新列移动到末尾后 Review 成功；不关闭表单，不提前执行 rename。

### 步骤 5.2：rename-only 后端防遗漏

- SQL Server/Oracle EDIT 显式对比 baseline 和 draft 的结构变化；顺序、新增或其他当前未支持的列变更在生成 rename 计划前拒绝。
- 纯表改名仍按现有测试通过。CREATE 使用单元二确定的顺序契约。
- 若 UI 需提示能力，只使用表单已有 hint/error/说明区域，写明当前支持范围；不要禁用一切 J/K，末尾新增调序/CREATE 仍有合法用途。

### 步骤 5.3：执行成功刷新与失败保留

以 `CatalogMutationSucceeded/Failed` 的既有 reducer 测试方式模拟：成功的表结构修改刷新 RelationChildren、失效旧关系元数据，已有 relation tab 后续 reload 按数据库顺序显示；失败保留草稿及目标列序。

关键的是复用现有 CatalogMutationImpact，不能新建本地列顺序缓存造成两份真相。新旧 object identity 相同时也必须失效结构元数据，不只在 rename 时刷新。

### 步骤 5.4：联合场景

`进入 Columns → A 新增 → 确认 → J/K 调整 → 查看 order dirty → Review → 提交 → 重新载入`，在 SQLite 和 MySQL/MariaDB 支持路径中目标序列一致；不支持路径中错误明确且没有任何数据库执行。

同时核对长 COMMENT、Added/Removed 背景、选择跟随、多行 hints、小终端滚动、dd pending 和文本输入。用少量联合断言串联现有测试，不建设第二套测试框架。

```sh
cargo +1.94.0 test --test catalog_mutation
cargo +1.94.0 test --test object_mutation_contract
cargo +1.94.0 test --test catalog_editor_reducer
cargo +1.94.0 test --test postgres_adapter
```

可形成 `fix(catalog): reject unsavable column layouts and refresh metadata`。到此才进行完整功能收尾。

## 6. 最终验证、审查与交接

### 6.1 验证来源和等级

1. **用户必需验收**：剩余宽度、A 上方新增、J/K 调序、提交保存新增和列序。数据库支持/限制按第 1.2 节如实呈现。
2. **项目强制 Rust 门禁**：`.github/workflows/ci.yml:81–83`。
3. **计划选定的自动化证据**：有真实数据的 SQLite round-trip、MySQL/MariaDB planner/snapshot 和可用服务 round-trip、模型/reducer/TestBackend 回归。服务配置缺失必须区分“未执行”与“通过”。
4. **补充检查**：人工、PTY、截图。用户未强制此类验证；环境受限最多一次针对性修复重试，再由 Luna 收尾审查决定补充证据或记录限制，不无限 progress。

完整实现后执行一次：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

遵循仓库 CI 的其他既有平台/数据库检查，不因为本计划只列 Rust 门禁就删减项目要求。发生失败后修复并重跑受影响检查；没有相关代码/环境变化时复用当轮有效结果，不重复全量验证。

每条命令追加到任务 `validation.md`：时间、命令、退出码、用例数量/跳过、HEAD、相关文件与 diff 状态、环境。提交前后的相同内容可说明代码对应关系，不伪造新运行。

### 6.2 Luna 审查清单

- COMMENT 的真实截断宽度与 constraint 相同，40 以后的内容可显示，窄尺寸总宽不超预算。
- 大写键支持 SHIFT，文本输入不被抢键；新动作与原移动选择动作分离。
- Vec 目标顺序唯一，row_id/existing_name/original ordinal 保持，详情 index 不因调序失效。
- order dirty 与 append 可达性分开，纯恢复/删除偏移/改名不误判。
- MySQL snapshot 保真，指纹不被数据自增计数干扰，Runtime stale guard 实际携带指纹。
- MODIFY/CHANGE 不丢 DEFAULT/ON UPDATE/COLLATE/generated storage/不可见属性；DDL 顺序中的 AFTER 引用真实存在。
- SQLite 按名字复制，不因位置重排错配数据；重建属性和依赖保持或明确拒绝。
- PostgreSQL/SQL Server/Oracle 不能保存的布局没有被 NoChanges、只改名或尾部追加掩盖。
- 成功刷新列元数据，失败保留草稿；后台原有 busy、read-only、preview 约束仍有效。
- 测试“通过”没有包含未配置服务的静默 return；最终摘要披露后端边界。
- diff 不带入其他任务计划、state/checkpoint、生成文件或凭据。

### 6.3 完成交接

Luna 按当时工作流授权提交/合并，不切回 Astra 审查。最终摘要包含四项需求完成状态、数据库矩阵、真实验证结果及环境限制、提交标识。

本 plan 阶段只交付计划和指定 activeReceipt；下一阶段由插件命名并安排 Luna 实施，不询问用户执行模式，不启动子 Agent。
