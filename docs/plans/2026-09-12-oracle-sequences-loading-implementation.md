# Oracle Sequences Loading Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. 若环境未提供该技能，按本文任务顺序执行；提交与子代理使用仍需用户明确授权。

**Goal:** 修复 Oracle Explorer 展开 Sequences 时的 ORA-00904，并保证 Tables、Views、Sequences 的对象列表能完整、稳定地分页加载。

**Architecture:** 在 Oracle 适配层统一数据字典字段映射，将 Objects 请求交由专用私有页面加载路径处理。数据库负责按游标过滤及有界读取，现有 finalize_keyset_page 负责保留页大小、生成下一页游标；继续复用 CatalogEntry、CatalogPage 及 Explorer 的错误/重试流程。

**Tech Stack:** Rust 2024 / MSRV 1.94、oracle 0.6.3、Tokio spawn_blocking、现有 Catalog keyset 协议。

---

## 背景与边界

已确认的源码问题（行号仅供定位，执行时以符号为准）：

1. `src/db/oracle.rs::oracle_catalog_entries`，约 648–667 行：Sequences 使用 `all_sequences`，却查询 `table_name` 并按 `owner` 过滤；应为 `sequence_name`、`sequence_owner`。
2. 同函数约 851–858 行：SQL FETCH 后才在 Rust 过滤游标，且在返回前 `.take(request.page_size)` 丢弃额外一条。
3. `OracleAdapter::load_catalog_page`，约 210–215 行：finalize 时已无额外记录，且游标 tie-breaker 使用 `native_path.join(".")`，与内存过滤的 `(name, name)` 不一致。
4. `tests/oracle_adapter.rs` 的基本目录测试只实际加载 Tables，缺少 Sequences 和多页覆盖。

参考：Oracle 官方 https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/ALL_SEQUENCES.html 。

相关计划：`docs/plans/2026-09-12-oracle-explorer-counts-pagination-implementation.md`。该计划范围更大，包含精确计数、Groups 分页和 reducer 数量同步。本计划只交付错误字段及 Objects 分页修复，继续返回 `CatalogCount::Unknown`。若相关计划已实施，复用其分组描述、Objects 加载器及测试，禁止新建第二套同义实现。

保持现有 Oracle 版本兼容基线；继续使用代码已有的 FETCH FIRST 语法。本次不引入依赖或修改公共 Catalog 协议，不改动 RelationChildren 的异构分页。

## Task 1：确认实施基线与建立回归用例

**查看：** `src/db/oracle.rs`、`src/db/catalog.rs`、`tests/oracle_adapter.rs`、上述关联计划。

**修改：** `tests/oracle_adapter.rs`。

1. 执行 `git status --short`、`git diff -- src/db/oracle.rs tests/oracle_adapter.rs`，确认是否已有并行或未提交实现；保留现有工作。
2. 确认 `load_catalog_page → oracle_catalog_entries → CatalogEntry → finalize_keyset_page` 当前路径，以及 `CatalogPage::validate_for` 的排序、页大小、请求身份约束。
3. 增加独立集成用例 `oracle_sequences_catalog_loads_when_configured`，按现有连接方式发现 database/schema，并直接请求 `ObjectGroup::Sequences`；避免依赖“数据库至少有一个表”。
4. 用例断言查询成功；非空结果逐项检查 Sequence 类型、父 Schema、qualified_name 与 native_path，空结果也是合法成功。
5. 使用现有 `LAZYDB_TEST_ORACLE_URL`、`LAZYDB_TEST_ORACLE_USER`、`LAZYDB_TEST_ORACLE_PASSWORD`，不输出变量值。
6. 在配置完整的 Oracle 环境运行失败用例：应因 ORA-00904 失败。没有数据库时记录“未完成真实失败复现”，不以测试提前返回作为复现成功。

```bash
cargo test --features driver-oracle --test oracle_adapter oracle_sequences_catalog_loads_when_configured -- --nocapture
```

**完成标准：** 回归测试请求真正的 Sequences 分组；已有集成测试继续保留。

## Task 2：统一字段映射并修复 SQL

**修改：** `src/db/oracle.rs` 私有分组描述、Objects 查询分支及内联测试模块。

1. 将当前分散的 view/column match 合并成一个私有映射函数，例如 `oracle_object_columns(group) -> Option<(&'static str, &'static str, &'static str)>`；tuple 顺序固定为 view、name_column、owner_column。
2. 若关联计数计划已引入命名字段的描述结构，直接使用该结构。
3. 所有动态标识符来自固定 match，Schema 继续绑定参数，不统一转大写。

映射函数的目标实现：

```rust
fn oracle_object_columns(
    group: ObjectGroup,
) -> Option<(&'static str, &'static str, &'static str)> {
    match group {
        ObjectGroup::Tables => Some(("all_tables", "table_name", "owner")),
        ObjectGroup::Views => Some(("all_views", "view_name", "owner")),
        ObjectGroup::Sequences => {
            Some(("all_sequences", "sequence_name", "sequence_owner"))
        }
        _ => None,
    }
}
```

4. 实际查询改为 `SELECT {name_column} FROM {view} WHERE {owner_column} = :1 ORDER BY {name_column} FETCH FIRST {limit} ROWS ONLY`。
5. 增加简洁的表驱动字段契约测试，覆盖三个受支持分组。此测试保护外部字典字段契约，但真实 SQL 是否有效仍由 Task 1/5 集成测试验证。
6. 运行离线测试及 Task 1 的集成测试；真实环境中 ORA-00904 应消失。

```bash
cargo test --features driver-oracle --lib db::oracle
cargo test --features driver-oracle --test oracle_adapter oracle_sequences_catalog_loads_when_configured -- --nocapture
```

**完成标准：** 两处错误字段一起修复，Tables/Views 映射保持正确；保留 ALL_SEQUENCES 的可见对象语义。

## Task 3：隔离 Objects 页面加载并修复 keyset 分页

**修改：** `src/db/oracle.rs::OracleAdapter::load_catalog_page`、`oracle_catalog_entries`，新增私有 Objects 加载器及必要的页面构造 helper。

**复用：** `src/db/catalog.rs::finalize_keyset_page`、`CatalogCursor::from_keyset/keyset_parts`、`CatalogPage::new`。

1. 在持有连接锁的 spawn_blocking 闭包中，将 `CatalogTarget::Objects` 直接分派到私有 Objects 页面加载器；其余 target 继续使用原路径。
2. 将 Objects 的查询、名称解码和 Entry 构造迁入该加载器，移除不可达的旧 Objects 分支，避免双份实现。保留 Sequence 的 `CatalogEntry::object(..., false)` 和 Table/View 的 `CatalogEntry::relation(..., true)` 语义。
3. 同一 Schema、同一分组内名称唯一，Objects 游标统一采用 `(name, name)`。解码续页游标后检查两个分量相等，不等时返回明确的无效请求错误；不将非 Objects 的游标切换到这个规则。
4. 首页仅绑定 owner；续页绑定 owner 和 last_name。查询条件必须在 FETCH FIRST 之前执行。
5. 排序与游标过滤采用相同的显式二进制排序表达式，避免 NLS 会话设置改变分页顺序。实施时通过官方文档及实机核实 NLSSORT 在支持版本、字符集中的行为，并确认与 Rust 字符串游标推进校验一致；若出现不一致，先统一 SQL/游标表示再交付，不放宽协议校验。

续页 SQL 模板（以 Sequences、page_size=10 为例）：

```sql
SELECT sequence_name
FROM all_sequences
WHERE sequence_owner = :1
  AND NLSSORT(sequence_name, 'NLS_SORT=BINARY')
      > NLSSORT(:2, 'NLS_SORT=BINARY')
ORDER BY NLSSORT(sequence_name, 'NLS_SORT=BINARY')
FETCH FIRST 11 ROWS ONLY
```

6. 首页使用相同 ORDER BY，省略续页谓词。`limit` 来自已验证请求的 `page_size + 1`；核对 oracle 0.6.3 的位置绑定规则后实现两个分支。
7. 最多将 P+1 条名称构造成 entries；取消 Objects 的 Rust 后置游标过滤及 `.take(P)`。
8. 调用 `finalize_keyset_page(&mut entries, P, name, name)`，两个闭包均返回 `qualified_name.object.clone()`。
9. 返回 `CatalogPage::new(request, entries, CatalogCount::Unknown, next_cursor)`，继续由现有校验器验证，不修改共享 finalize 或放宽页契约。
10. 从 Objects 路径移除 `native_path.join(".")` 的游标生成依赖；native_path 本身继续保存原始标识，不拆解名称中的点号。

**完成标准：** 额外一条只在 finalize 被裁剪；每次查询最多读取 P+1 条；后续请求读取游标之后的对象；其他 target 行为不受影响。

## Task 4：验证分页边界与游标契约

**修改：** `src/db/oracle.rs` 内联单元测试模块。

1. 为实际用于 Objects 路径的页面构造 helper 编写测试，不重复测试一份脱离生产路径的算法。
2. 输入构造应模拟数据库返回至多 P+1 个名称；对比以下输出：

| 输入记录数 | 返回 entries | next_cursor | completeness |
| --- | --- | --- | --- |
| 0 | 0 | None | Complete |
| P-1 | P-1 | None | Complete |
| P | P | None | Complete |
| P+1 | P | 第 P 个名称的 `(name, name)` | Partial |

3. 检查 P=1、包含点号、大小写混合、非 ASCII 名称仍原样构造 ID；从生成游标解码应得到预期两个分量。
4. 检查不一致的游标分量被拒绝，续页输入为空时可正常结束。
5. 对每个生成页面执行现有 `validate_for`，验证请求身份、父节点、页大小、游标推进及 completeness。
6. 运行以下测试；新增边界用例应先证明旧实现丢弃 lookahead 的问题，再验证修复。

```bash
cargo test --features driver-oracle --lib db::oracle
cargo test --test catalog_contract
cargo test --test catalog_reducer
```

**完成标准：** 分页边界和协议测试通过；明确这些离线测试不证明 Oracle SQL、绑定或 NLS 行为正确。

## Task 5：真实 Oracle 全路径验收

**修改：** `tests/oracle_adapter.rs`，新增 `oracle_objects_paginate_when_configured`，补充集成测试执行标记。

1. 使用专用测试 Schema 预置稳定 fixture：每类至少 5 个对象，包含合法的带点号、混合大小写及非 ASCII 名称；另有空对象分组/Schema用于空列表验收。fixture 由测试环境准备，不在截图中的业务 Schema 自动创建或删除对象。
2. 用已确认正确的三个字典查询独立获取期望名称清单；每次测试期间保持 fixture 稳定。直接查询使用 owner 绑定，不读业务表数据、不调用 sequence.NEXTVAL。
3. 用 `page_size=2` 遍历 Tables/Views/Sequences。每页验证 `validate_for`，累计 ID 无重复，游标严格推进，最终集合与独立查询及 fixture 清单一致。
4. 增加最大迭代次数保护，若游标不前进或循环，应直接报告测试失败而非永久运行。
5. 覆盖 N=0、N=P、N=P+1、N>2P；使用不同页大小和预置对象数量实现，不要求修改生产对象。
6. 返回条目全部属于目标 Schema；具备跨 Schema 授权的专用环境中验证第二 Schema 的同名对象不会混入。无此环境则记录隔离实机覆盖不足。
7. 在默认及可用的非二进制 NLS 会话设置下运行名称排序分页用例，证明显式二进制排序有效。设置只作用于专用测试连接。
8. 测试环境变量缺失时允许沿用可选测试习惯，但输出不含凭据的明确 SKIP 原因；已显式配置运行时，连接失败及 DPI-1047 必须使本次新增验收用例失败。
9. 在连接成功且断言完成后输出无敏感信息的执行标记，避免将提前返回误报为 Oracle 验收成功。

```bash
cargo test --features driver-oracle --test oracle_adapter oracle_ -- --nocapture
```

**完成标准：** 已确认真实连接成功、三个分组查询成功，多页无遗漏无重复；环境未就绪时列出未验收项目，不宣称 SQL 修复已通过实机验证。

## Task 6：编译检查、人工验收与交付

**检查文件：** `src/db/oracle.rs`、`tests/oracle_adapter.rs`。

按顺序执行最小必要检查；同一代码版本已通过的测试无需重复运行：

```bash
cargo fmt --all -- --check
cargo check --features driver-oracle
cargo check --no-default-features
git diff --check
git diff --stat
git diff -- src/db/oracle.rs tests/oracle_adapter.rs
```

1. 格式检查若涉及既有文件差异，先区分本次与已有改动，仅修复本次引入的问题。
2. 人工启动应用，连接目标 Oracle，展开 Sequences：应显示序列节点或正确空状态；检查 Tables、Views 正常加载。
3. 使用刷新/Retry 路径验证成功响应能替换失败状态；对象超过页大小时 Load more 能继续直到结束。无需为本问题增加新的 UI 状态。
4. 若目标数据库或数据量无法满足多页人工验收，使用专用 fixture 的集成结果补充并注明。
5. 交付说明包含：字段修复、Objects 分页修复、实际执行的检查、实际执行/跳过的 Oracle 用例、支持版本/NLS 的验证范围。
6. 不自动 bump 版本、生成发布记录或提交。若用户另行授权提交，可按两个逻辑单元组织：`fix(oracle): correct sequence catalog columns`、`fix(oracle): repair object catalog keyset pagination`，各自包含相关回归测试。

## 最终验收清单

- [ ] Sequences 使用 SEQUENCE_OWNER 和 SEQUENCE_NAME，真实查询无 ORA-00904。
- [ ] 空列表成功，非空列表的名称、父 Schema 和 Sequence 类型正确。
- [ ] Tables/Views/Sequences 均可按数据库游标有界分页。
- [ ] P+1 lookahead 保留到 finalize，边界页和最后一页判断正确。
- [ ] Objects 游标生成、解码、SQL 过滤及排序一致，特殊名称不被改写。
- [ ] 三类对象多页结果与稳定 fixture/独立字典查询一致，无重复遗漏。
- [ ] Oracle feature 启用/禁用均可编译，相关契约及 reducer 测试通过。
- [ ] 真实 Oracle 验收状态明确，未执行项目不标记为通过。
