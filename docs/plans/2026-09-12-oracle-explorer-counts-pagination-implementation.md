# Oracle Explorer Counts and Pagination Implementation Plan

> **For Claude:** 按任务顺序执行本计划；若执行环境提供 `superpowers:executing-plans`，使用该技能逐项实施。仅在用户明确授权后提交代码或使用子代理。

**Goal:** Oracle Explorer 在展开 schema 后准确显示 Tables / Views / Sequences 的对象数量，支持完整分页，并在刷新对象列表后同步数量。

**Architecture:** 在 Oracle Catalog 适配层从与对象列表一致的数据字典查询准确计数，复用 `CatalogGroupSummary → CatalogGroupState → metadata` 的展示链路。修复 Oracle Objects 的 keyset 分页，并在 App 接受有效对象页时同步已知总数；保留现有请求身份、epoch 和页面校验机制。

**Tech Stack:** Rust 2024（MSRV 1.94）、oracle 0.6.3、Tokio spawn_blocking、现有 Catalog 协议、ratatui TestBackend。

---

## 1. 范围与设计约定

### 已确认的问题

- `src/db/oracle.rs:598`：`oracle_group_page` 固定返回三个分组，每个 `object_count` 都是 `Unknown`。
- `src/db/oracle.rs:185`：普通 CatalogPage 的 `total_count` 固定为 `Unknown`。
- `src/db/oracle.rs:634,851–858`：查询多取一条，却在 finalize 前截断；游标在 SQL 限条数后才于 Rust 过滤。
- `src/db/oracle.rs:650–664`：Sequences 错用 `owner` / `table_name`。
- `src/app.rs:14036`：`accept_catalog_page` 只从 Groups 响应更新分组计数。
- `src/model/workspace.rs:1520`：Unknown 返回 None；`src/ui/mod.rs:2423` 只显示非空 metadata。这一展示规则正确。
- `tests/oracle_adapter.rs` 现有集成测试只验证三个分组及首批表非空，缺少数量与分页断言。

行号是编写计划时的定位提示；实施前通过符号确认当前代码。保留工作区已有修改及其他计划文档。

### 计数含义

| 分组 | 字典 | owner 列 | 名称列 |
| --- | --- | --- | --- |
| Tables | all_tables | owner | table_name |
| Views | all_views | owner | view_name |
| Sequences | all_sequences | sequence_owner | sequence_name |

- 计数是当前账号可见、属于目标 schema 的对象数，不是表行数。
- schema 使用请求中的真实名称，绑定传参，不统一转大写，不拼接为 SQL 字面量。
- `group_summaries[i].object_count` 是该分组对象数。
- Groups 页的 `total_count` 是分组总数 3；Objects 页的 `total_count` 是当前分组对象总数。
- `Exact(0)` 显示 0；Unknown 不等于 0；错误沿用现有加载失败 / 重试流程。
- 不通过已加载子节点数推断总量；不从 `NUM_ROWS` 等统计信息估算。
- 不新增后台计数任务、缓存、公共枚举或生产依赖。
- 本次分页重点是 Groups 和 Objects；不顺带重构 RelationChildren 的异构排序与分页。

### 计数与并发 DDL

列表和计数采用同一条 Objects SQL 获取，使二者来自同一个查询快照。建议使用“计数 CTE + 有界页面 CTE + LEFT JOIN”的结构；不要简单执行两条独立 SQL 后假定结果一致。

跨页的多个请求不承诺固定快照。列表浏览期间发生 DDL 时，计数可随新请求变化；刷新恢复当前列表。保持现有 `CatalogPage::validate_for` 校验，不为并发变化放宽协议。

## 2. Task 1：统一 Oracle 分组描述与游标约定

**修改：** `src/db/oracle.rs` 的私有 helper 与内联单元测试。

1. 查看 `oracle_catalog_entries`、`load_catalog_page`、`oracle_group_page` 及 Catalog 游标协议；确认其他 CatalogTarget 路径不受影响。
2. 增加最小私有分组描述：group、稳定 group_key、字典名、owner 列、name 列；只由固定 match 返回，不接受用户 SQL 标识符。
3. 用同一描述生成列表和分组计数查询，消除 Sequences 的错误字段及两套筛选口径。
4. 明确 Objects 同一 schema、同一类型下名称唯一，因此采用 `(name, name)` 游标；组内 SQL 只需按 name 比较。游标解码需校验两个分量一致。
5. SQL 名称排序和过滤都显式使用二进制比较，避免会话 NLS 排序与 Rust 游标推进校验不同。实施时核对 Oracle 支持版本中的 `NLSSORT(..., 'NLS_SORT=BINARY')` 行为及客户端 Unicode 编码。
6. 为特殊名称、包含点号的名称、无效游标增加测试，确认游标不再依赖 `native_path.join(".")`。

**验证命令：**

```bash
cargo test --lib db::oracle
```

预期：无需 Oracle 服务即可运行的 helper 测试通过；新增测试先确认能暴露错误映射或游标不一致，再修改实现。

## 3. Task 2：实现准确分组计数和 Groups 分页

**修改：** `src/db/oracle.rs::oracle_group_page`、`OracleConnection::load_catalog_page`（以实际 impl 类型名为准）。

1. Groups 请求在 `spawn_blocking` 获取连接后直接进入分组加载函数，避免先调用只返回空列表的 entries 分支。
2. 分组函数接收连接与请求，校验 schema 路径，取出 owner。
3. 一次 UNION ALL 查询获取三组计数，查询口径如下：

```sql
SELECT 'tables' AS group_key, COUNT(*) AS object_count
FROM all_tables WHERE owner = :1
UNION ALL
SELECT 'views' AS group_key, COUNT(*) AS object_count
FROM all_views WHERE owner = :1
UNION ALL
SELECT 'sequences' AS group_key, COUNT(*) AS object_count
FROM all_sequences WHERE sequence_owner = :1
```

4. 按当前驱动绑定规则绑定重复的 owner 占位符；核对 oracle 0.6.3 文档后确定调用，不假设 SQL 与 PL/SQL 绑定规则相同。
5. 非负计数经检查转换为 u64，返回 `CatalogCount::Exact`；转换失败不得默认为 0。
6. 用固定顺序 Tables → Views → Sequences 展示。Groups 游标采用稳定递增 key，例如 `01_tables`、`02_views`、`03_sequences`，避免展示顺序与字符串顺序冲突。
7. 三个 summary 在内存按游标过滤，然后使用 `finalize_keyset_page`；返回 `CatalogPage::groups(..., Exact(3), next_cursor)`。
8. 保留数量为零的分组。缺失结果行、未知 group_key 或查询失败按内部/数据库错误处理，不捏造计数。

**测试：**

- 三个不同计数、全部为零。
- `page_size=1` 连续三页，分组顺序稳定，最后一页无游标。
- `page_size=2` 两页、`page_size>=3` 单页。
- 每页 `total_count == Exact(3)`，每个对象数量独立正确。
- 页面能通过 `validate_for`。

运行 `cargo test --lib db::oracle`，预期所有纯页面构造测试通过。

## 4. Task 3：Objects 正确分页并返回一致的总数

**修改：** `src/db/oracle.rs::load_catalog_page`、`oracle_catalog_entries` 的 Objects 分支及新增私有 Objects page helper。

1. 将 Objects 从通用 entries helper 分离为私有页面加载函数，使计数、额外一条和游标由同一层负责；保留已有 CatalogEntry 构造逻辑或提取最小 helper 复用。
2. 先写边界测试：N=0、N<P、N=P、N=P+1、N>2P；P 为页大小。
3. 生成同一条 SQL 的 count / page 结构。以 Tables 的有游标页为例：

```sql
WITH object_count AS (
    SELECT COUNT(*) AS total_count
    FROM all_tables
    WHERE owner = :1
), page_rows AS (
    SELECT table_name AS object_name
    FROM all_tables
    WHERE owner = :1
      AND NLSSORT(table_name, 'NLS_SORT=BINARY')
          > NLSSORT(:2, 'NLS_SORT=BINARY')
    ORDER BY NLSSORT(table_name, 'NLS_SORT=BINARY')
    FETCH FIRST 11 ROWS ONLY
)
SELECT c.total_count, p.object_name
FROM object_count c
LEFT JOIN page_rows p ON 1 = 1
ORDER BY NLSSORT(p.object_name, 'NLS_SORT=BINARY')
```

4. 示例中的 11 替换为经过请求验证的 `page_size + 1`。首页省略游标谓词；Views / Sequences 使用 Task 1 固定映射。计数部分始终不应用游标条件。
5. LEFT JOIN 保证空分组或游标之后无记录时仍返回计数。读取 `object_name: Option<String>`；None 是计数占位行，不构建 CatalogEntry。
6. 保留 P+1 个真实对象给 `finalize_keyset_page`，由其裁剪并生成 `(name, name)` 游标；删除 Objects 的 Rust 后置游标过滤与提前 take。
7. 返回 `CatalogPage::new(request, entries, Exact(total), next_cursor)`，经现有页面验证。
8. 首次请求前往空组应得到 Exact(0)、空 entries、无 cursor；恰好 P 个对象不生成下一页；P+1 个对象必须生成下一页。
9. 与真实 Oracle 验证 SQL 语法、绑定和排序，不以字符串断言代替数据库行为验证。

**验证：** `cargo test --lib db::oracle`，以及 Task 6 的真实数据库集成测试。

## 5. Task 4：有效 Objects 响应同步分组数量

**修改：** `src/app.rs::accept_catalog_page`。
**测试：** `tests/catalog_reducer.rs`。

1. 复用现有请求/响应 fixture，先增加 Objects 页 `Exact(N)` 能更新所属分组的失败测试。
2. 在页面校验及树变更成功后、提交 `next_explorer` 前更新所属 `CatalogGroupState.count`；这一更新与节点变更一起生效。
3. 只处理 `CatalogTarget::Objects`；Group 的名称、顺序、相邻分组和其他 schema 不应受到影响。
4. 合并规则：新 Exact 始终采用（包括比旧值小）；新 AtLeast 可更新旧 Unknown/AtLeast，旧 AtLeast 与新值取较大下界；旧 Exact 不被 AtLeast 降级；新 Unknown 保留原值。
5. 不用计数覆盖 `CatalogGroupState.completeness`，除非先确认其所有消费者语义；本任务只改变 count，列表分页状态仍由 load_states 管理。
6. 以下响应不能改变计数：旧连接 generation、旧 epoch、被替换 request_id、无效页面、加载失败。
7. 验证刷新首批对象后计数能从 10 降为 8，也能从 8 升为 12；只刷新单一分组时不要求额外请求 Groups。

**验证命令：**

```bash
cargo test --test catalog_reducer
```

预期：新增计数同步测试及已有分页、刷新、错误恢复测试全部通过。此处为共享 reducer 改动，需要保留非 Oracle fixture 验证。

## 6. Task 5：验证现有 Explorer 展示链路

**测试：** `tests/ui_render.rs`，必要时 `tests/explorer_state.rs`。
**生产代码预期：** 不需要修改 `src/ui/mod.rs` 或 `src/model/workspace.rs`。

1. 复用已有 `explorer_find_keeps_group_counts_and_column_metadata_visible` 附近的分组 fixture。
2. 增加普通 Explorer 分组行检查：Exact(128) → `Tables  128`；Exact(0) → `Tables  0`；AtLeast(128) → `Tables  128+`；Unknown 无数字。
3. 检查折叠、展开、选中后计数均可见；固定祖先行继续使用相同 metadata。
4. 复用已有 find 测试保证搜索展示不退化，不复制大量等价快照。
5. 仅当测试暴露真实渲染缺陷才调整 UI；不增加 Oracle 专属展示规则。

```bash
cargo test --test ui_render explorer_
cargo test --test explorer_state
```

预期：普通树、find、选择及展开行为的相关测试通过。

## 7. Task 6：真实 Oracle 验收

**测试：** `tests/oracle_adapter.rs`。

1. 扩展现有环境变量驱动的测试，检查三个 summary 均为 Exact，并检查 Groups 页 total_count 为 Exact(3)。
2. 使用专用测试 schema 的稳定 fixture：至少 5 个表、2 个视图、2 个序列，另备空 schema 或空对象类型；包含合法的特殊名称及非 ASCII 名称。fixture 由测试环境预置，不在截图连接上自动建删对象。
3. 采用 `page_size=2` 遍历全部 Objects 页，用集合验证无重复，用预置清单及同口径独立查询验证无遗漏；检查每页总数和最后一页游标。
4. Groups 使用 `page_size=1` 验证三个分组完整遍历。
5. 验证表、视图、序列均正常列举；在支持的 NLS 配置下检查排序稳定。
6. 通过可控 fixture 变更验证刷新计数增长、减少；共享环境无法变更时，以 reducer 测试覆盖并记录真实刷新未验收。
7. 记录分组查询次数为 1、每次 Objects 页面查询次数为 1；大 schema 使用小页大小，确认仅传输最多 P+1 个名称，不全量加载对象。
8. 在大 schema 记录当前方案的查询耗时；字典精确计数成本按实测评估，不提前引入缓存或异步状态。

```bash
cargo test --features driver-oracle --test oracle_adapter oracle_ -- --nocapture
```

环境使用现有 `LAZYDB_TEST_ORACLE_URL`、`LAZYDB_TEST_ORACLE_USER`、`LAZYDB_TEST_ORACLE_PASSWORD`，不在命令或日志中输出值。Oracle Instant Client 必须可用。

**重要：** 现有测试会在缺少环境变量或 DPI-1047 时提前返回；测试进程绿色不能证明数据库验收完成。实施报告必须说明实际连接成功、实际执行了哪些用例，未连接时标记未验证。

## 8. 最终检查与交付

在各任务定向测试通过后，执行一次共享契约与构建检查：

```bash
cargo test --test catalog_contract --test catalog_reducer --test explorer_state
cargo fmt --all -- --check
cargo check --all-targets --features driver-oracle
cargo check --all-targets --no-default-features
git diff --check
git status --short
git diff --stat
```

若 formatter / 构建发现既有问题，先区分本次引入与原有问题，避免无关文件批量修改。若需要了解 Oracle API / SQL 兼容性，实施前查阅当前文档并用目标版本验证。

### 完成标准

- schema 分组加载完成后，即使 Tables 未展开也能显示准确数量。
- 空分组显示 0；未知及错误不伪装成 0。
- Tables、Views、Sequences 都能完整分页，计数与列表口径一致。
- 单独刷新对象分组后更新数量，陈旧响应不污染状态。
- 小页面 Groups 分页符合协议。
- 无数据库环境的 reducer / UI / 契约测试通过；真实 Oracle 验收状态明确。
- 默认 feature 与无 Oracle feature 均能编译。

### 交付说明

列出修改文件、测试命令及结果、Oracle 实测环境与未验证项。计划文档和功能修改不自动提交；用户要求提交时，再按实际 diff 拆分为 Oracle Catalog 修复与 reducer/UI 覆盖等内聚提交。
