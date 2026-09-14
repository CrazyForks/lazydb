# Omni 候选、单选与行布局优化 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行环境若没有该技能，按本文任务顺序实施并记录验证结果。任务依赖顺序执行即可，无需子代理。

**Goal:** Omni 只列出表、视图、物化视图等可打开的 relation 对象，始终单选，并以右对齐的结构化元信息显示连接图标、连接名与命名空间。

**Architecture:** 在共享 Catalog 搜索请求中显式指定对象范围，各适配器在结果限额之前过滤；Omni 统一构造本地和远程候选，按稳定对象 ID 合并、去重和协调选择。连接位置采用结构化模型，UI 负责图标、显示宽度预算、截断和右对齐，继续复用现有 OmniLayout、Theme、IconSet。

**Tech Stack:** Rust 2024 / MSRV 1.94，Ratatui 0.30.2，unicode-width 0.2，SQLx / Tiberius，现有 Cargo 集成测试与 TestBackend。

---

## 1. 已确认的代码事实

行号仅供参考，执行时按符号定位并检查工作区最新内容。

| 位置 | 事实 |
| --- | --- |
| `src/app.rs::refresh_omni_items` | 本地 catalog 候选已经通过 `is_relation()` 过滤，副标题为连接 / 数据库 / schema |
| `src/app.rs::apply_omni_search_page` | 子对象使用父 relation 的 ID，但保留子对象名称和类型，导致重复 ID |
| `src/model/omni.rs::set_items` | 直接赋值，没有去重 |
| `src/model/omni.rs::move_selection` | 按 ID 找第一个可见位置，重复 ID 会影响移动 |
| `src/ui/omni.rs::render` | 按 ID 高亮，标题后只拼两个空格，没有填满预留宽度 |
| `src/ui/omni.rs::truncate_cells` | 按字符显示宽度截断，不补齐、不加省略标记 |
| `src/db/catalog.rs::CatalogSearchRequest` | 有 query、scope、limit，没有对象类型范围；最大返回 100 项 |
| `src/db/postgres.rs::search_catalog_snapshot` | SQL 获取 limit+1 后截断，再加载命中子对象的元信息 |
| `src/db/mysql.rs::search_catalog_snapshot` | 候选获取后会构造 relation cache、补充子对象元信息 |
| `src/db/sqlite.rs::search_catalog_snapshot` | 通过 search_schema 收集候选，再排序、去重、截断 |
| `src/db/mssql.rs::search_catalog` | 多库候选排序、截断后加载子对象详情 |
| `src/db/mod.rs::search_catalog` | 支持上述四种适配器；MariaDB 复用 MySQL；Oracle/Redis 当前没有此搜索实现 |
| `src/ui/icons.rs` | 已有 database、database_color、catalog 和三种图标模式 |

相关已有计划：`docs/plans/2026-09-14-omni-render-animation-fix-implementation.md`。本计划使用现有 `OmniLayout` 作为绘制、滚动及鼠标区域的共同来源。

当前工作区包含 SQL History 等已暂存修改和其他未跟踪计划。实施时检查 `git diff` 与 `git diff --cached`，提交前逐块审阅，避免把已有修改纳入本任务提交。

## 2. 固定行为契约

1. Omni catalog 候选允许 `Table / View / MaterializedView`，与当前 `is_relation()` 和 `OpenRelation` 一致。连接、命令、Console、最近位置仍是各自独立类别。
2. 列、索引、主键、外键、约束、触发器不成为 Omni 候选，也不转换成父表候选；搜索只命中列名时不会因此返回父表。
3. 共享协议区分 `AllObjects` 和 `RelationsOnly`。Omni 使用后者；其他现有调用者显式使用前者。
4. 类型过滤发生在排名、limit、truncated 计算之前；RelationsOnly 路径不加载子对象详情。
5. 任一 OmniState 的候选 ID 唯一；空列表无选中，有可见项时至多一个选中，键盘与鼠标操作同一个稳定 ID。
6. 异步页更新保留仍可见的选中 ID，否则按现有回落规则选择首项；继续校验连接身份、session、generation，拒绝过期结果。
7. 同一 relation 的 ID、标题、kind 和 OpenRelation action 指向同一对象。
8. 行结构：`选择标记 + 对象图标 + 标题 + 弹性间隔 + 连接图标/连接名/命名空间 + [open]`。
9. 右端落在 `layout.results.right() - 1` 的内容单元，不能写入边框。至少保留两格左右间距。
10. 路径不重复对象名；空 database/schema 不生成空分隔段。数据库和 schema 同名时保留其语义层级，不以名称相同为由随意去重。
11. 标题和元信息按终端显示单元计宽；ASCII 模式使用 `...`，其他模式使用 `…`；预算不足以容纳省略标记时只显示可容纳的字符。
12. 先保留标题和可辨认的连接名，再保留 schema、database；窄窗口逐级省略位置段，必要时隐藏右侧元信息。`[open]` 也须纳入预算。
13. 图标由 UI 根据 IconSet 生成，不进入可搜索文本；连接图标紧邻连接名。未知或已删除连接显示明确文本回退，不猜测数据库类型。

## 3. 实施任务

每个 Step 是一个独立检查点；较大的适配器步骤按文件分别完成。红灯测试与对应实现一起组成可通过的提交。

### Task 1：复现重复选中与错误候选

**Files:**
- Modify/Test: `tests/omni_search.rs`
- Modify/Test: `tests/omni_navigation.rs`
- Reference: `tests/omni_providers.rs`, `src/db/catalog.rs`

**Step 1 — 准备真实数据夹具。**
复用 connected_app，构造一个表、一个视图、表下的 Column / Index / PrimaryKey，使用 CatalogEntry 的合法构造函数和 ancestors。通过 `Action::CatalogSearchSucceeded` 注入当前 session/generation 的页，避免直接伪造最终 UI 行来替代数据链路测试。

**Step 2 — 增加失败用例。**
- `omni_search_excludes_relation_children`：输入匹配表名的 query，页包含表和其子对象，最终 catalog 候选只剩 relation，action 和 ID 一致。
- `omni_navigation_visits_each_relation_once`：连续移动选择，依次到达每个 relation，无重复 ID 导致的停滞。
- 同名对象位于不同 schema/profile 时仍是不同候选，不得按 title 去重。

**Step 3 — 确认失败来自目标问题。**
Run: `cargo test --test omni_search --test omni_navigation -- --nocapture`
Expected: 新增排除子对象/身份断言失败；已有异步身份测试正常。

### Task 2：修正 Omni 对象身份与单选不变量

**Files:**
- Modify: `src/app.rs::refresh_omni_items`, `apply_omni_search_page`
- Modify/Test: `src/model/omni.rs::set_items`
- Test: `tests/omni_search.rs`, `tests/omni_navigation.rs`

**Step 1 — 过滤远程结果。**
在构造 item 前检查 `hit.entry.kind.is_relation()`，不通过则跳过。直接使用 `hit.entry.id`，删除 relation_id 兜底。保留 session/generation 及调用链已有连接身份检查。

**Step 2 — 统一入口去重。**
`set_items` 使用 HashSet 保留同一 ID 的第一个元素，保持输入顺序，再调用 `reconcile_selection(true)`。约定：来源合并在 app 层解决优先级，set_items 只负责最后一道唯一性保障。

**Step 3 — 添加状态级必要回归。**
覆盖重复 ID 输入、选中项仍存在、选中项被移除、全部项被 query 过滤、相同标题但不同 ID。保留稳定 ID，不改成纯索引状态。

**Step 4 — 运行定向验证。**
Run: `cargo test --lib model::omni -- --nocapture`
Run: `cargo test --test omni_search --test omni_navigation -- --nocapture`
Expected: Task 1 及状态测试全部通过。

**提交边界：** `fix(omni): enforce relation identity and unique selection`

### Task 3：为共享搜索协议增加对象范围

**Files:**
- Modify: `src/db/catalog.rs`, `src/app.rs`, `src/agent/catalog.rs`
- Modify/Test: `tests/catalog_contract.rs`, `tests/omni_search.rs`
- Update request literals: `tests/sqlite_adapter.rs`, `tests/postgres_adapter.rs`, `tests/mysql_adapter.rs`, `tests/sqlserver_adapter.rs`, `tests/postgres_relation_mutations.rs`

**Step 1 — 定义类型。**
在 CatalogSearchRequest 附近新增：

```rust
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CatalogSearchObjectScope {
    #[default]
    AllObjects,
    RelationsOnly,
}

impl CatalogSearchObjectScope {
    pub const fn includes(self, kind: CatalogKind) -> bool {
        match self {
            Self::AllObjects => true,
            Self::RelationsOnly => kind.is_relation(),
        }
    }
}
```

给请求增加 `pub object_scope: CatalogSearchObjectScope`，与现有数据库/schema `scope` 区分。无需将 UI owner 传入适配器，也无需新增用户配置。

**Step 2 — 更新所有构造点。**
Omni 设置 RelationsOnly；Agent 以及原有全对象搜索测试设置 AllObjects。用编译器确认所有 struct literal 均更新；检查 runtime 是否完整转发请求，而不是重新组装丢失字段。

**Step 3 — 增加行为断言。**
Omni 请求测试断言 object_scope；Agent/catalog 原有列、索引等搜索测试继续验证全对象语义。新增范围契约用例覆盖 relation 与子对象的区别。

**Step 4 — 编译与验证。**
Run: `cargo check --all-targets --all-features`
Run: `cargo test --test catalog_contract --test omni_search --test agent_catalog -- --nocapture`
Expected: 构造点完整更新，原调用者行为测试通过。协议与 Task 4 适配器实现组成完整交付检查点。

### Task 4：在各适配器限额之前应用范围

**Files:**
- Modify: `src/db/postgres.rs::SEARCH_CATALOG_SQL`, `search_catalog_snapshot`
- Modify: `src/db/mysql.rs::CATALOG_SEARCH_CANDIDATES_SQL`, `search_catalog_snapshot`
- Modify: `src/db/sqlite.rs::search_catalog_snapshot`, `search_schema`
- Modify: `src/db/mssql.rs::format_search_candidates`, `search_catalog`
- Test: `tests/postgres_adapter.rs`, `tests/mysql_adapter.rs`, `tests/sqlite_adapter.rs`, `tests/sqlserver_adapter.rs`

**Step 1 — 加入适配器回归夹具。**
在现有测试库创建匹配同一前缀的表、视图和大量列/索引/约束。小 limit（例如 2）验证只按 relation 计数；超过 100 个子对象的夹具验证默认上限不吞掉表。分别断言 AllObjects 与 RelationsOnly，以及无 relation 命中时 `hits=[]`、`truncated=false`。

**Step 2 — PostgreSQL。**
为 SQL 增加绑定参数表达 RelationsOnly；在候选筛选、排序和 LIMIT 前限制 kind 为 table/view/materialized_view，并让非 relation 的 UNION 分支在 RelationsOnly 下不产出候选。保留现有搜索规范化与 catalog scope。参数绑定顺序与 SQL 同步；relation_ids 为空时自然跳过 children hydration。

**Step 3 — MySQL / MariaDB。**
在候选 SQL 的固定谓词或绑定参数中增加对象范围，只允许 table/view；过滤位于 relation cache 构建之前。动态 SQL 只使用程序生成的固定范围片段，不拼接外部输入。继续复用 MySqlAdapter 覆盖 MariaDB。

**Step 4 — SQLite。**
将范围传给 search_schema；RelationsOnly 不收集 database/schema 为结果，不查询列、索引、外键等子对象。仍构造 relation 所需祖先节点。过滤后的关系候选参与现有 rank、dedup、limit+1 流程。

**Step 5 — SQL Server。**
format_search_candidates 接收范围，在候选生成处排除 schema/子对象/其他对象；外层数据库命中仅在 AllObjects 下加入。保留跨库范围筛选和排序，确保 truncate 之前类型正确；RelationsOnly 不调用 load_search_children。

**Step 6 — 运行数据库测试。**
Run: `cargo test --test sqlite_adapter -- --nocapture`
Run: `cargo test --test postgres_adapter -- --nocapture --test-threads=1`
Run: `cargo test --test mysql_adapter -- --nocapture --test-threads=1`
Run: `cargo test --test sqlserver_adapter -- --nocapture --test-threads=1`

服务测试使用 `.github/workflows/ci.yml` 约定的 `LAZYDB_TEST_POSTGRES_URL`、`LAZYDB_TEST_MYSQL_URL`、`LAZYDB_TEST_SQLSERVER_URL`。MariaDB 使用其测试 URL 替换 MySQL URL，并设置 `LAZYDB_TEST_MYSQL_FUNCTIONAL_INDEX=0`。执行者须检查用例实际运行情况，缺少环境导致跳过不能记为适配器验证通过。

Expected: 全对象旧用例与 relation-only 新用例通过，限额及 truncated 只反映目标范围。必要时在开发测试库检查 query log，确认 RelationsOnly 没有 children hydration 查询。

**提交边界：** `feat(catalog): support relation-only search scope`（Task 3–4）。

### Task 5：统一候选构造、元信息和异步合并

**Files:**
- Modify: `src/model/omni.rs::OmniItem`, `searchable_fields`
- Modify: `src/app.rs::refresh_omni_items`, `apply_omni_search_page`
- Test: `tests/omni_providers.rs`, `tests/omni_search.rs`

**Step 1 — 增加位置模型。**
新增 `OmniLocation { profile_id: Uuid, database: Option<String>, schema: Option<String> }`，OmniItem 增加 `location: Option<OmniLocation>`，new 默认 None。沿用 context.profile_id，不新增第二套独立选择身份。subtitle 继续承载命令类别等普通描述；有 location 的对象使用结构化位置展示。

**Step 2 — 抽取 relation 构造助手。**
在 app 的 Omni 辅助函数区域定义统一入口，输入 CatalogEntry 与连接展示信息，输出 Option<OmniItem>。助手验证 is_relation，统一 title/kind/id/action/context/location。refresh 与异步页都调用该助手。

**Step 3 — 保持搜索文本完整。**
keywords 保存无图标的连接名称、database、schema、完整限定对象路径及现有 ancestors 关键词。不能因为 UI 路径缩短而丢失搜索字段，也不能依赖终端宽度决定匹配结果。profile/name 更新时通过既有 refresh 重建这些字段。

**Step 4 — 其他候选的位置。**
Console 使用 execution_target 填充 location；连接候选左侧继续使用 Connection(kind) 图标，右侧显示类型/访问属性，避免重复显示连接名。最近位置只有能可靠取得目标时才填充 location；未绑定 Console 使用现有 unbound 文本。

**Step 5 — 明确异步合并算法。**
刷新远程结果时保留非 Catalog 候选，从 normalized catalog 重新构造当前本地 relation 候选，然后合并当前页。按 CatalogId 去重，远程条目更新同 ID 本地对象字段；不同 profile 的本地对象保留。不要累积上一代远程搜索结果，当前 CatalogSearchPage 按整次查询结果处理，而非增量分页。

**Step 6 — 验证来源一致性。**
测试同一个对象来自本地和远程时仅一行，元信息一致；新 query 不遗留旧远程项；过期页无影响；profile scope 继续正确；以连接名/命名空间搜索仍能匹配已知本地对象。远程跨连接搜索能力按当前 active connection 协议执行。

Run: `cargo test --test omni_providers --test omni_search --test omni_navigation -- --nocapture`
Expected: 合并不重复、选中稳定、本地/远程字段一致。

**提交边界：** `refactor(omni): unify result metadata and provider merging`

### Task 6：实现右对齐布局及连接图标

**Files:**
- Modify/Test: `src/ui/omni.rs`
- Reference: `src/ui/icons.rs`, `src/ui/theme.rs`
- Test: `tests/ui_render.rs`

**Step 1 — 提取可测试行布局函数。**
render 保留数据获取和 HitRegion 注册；辅助函数负责生成一行的 spans。所有预算以 layout.results.width 为准；title、连接名称及命名空间先 sanitize，再计算 cell_width。不要将数据库类型查找和副标题字符串解析混在宽度算法中。

**Step 2 — 构建右侧 spans。**
通过 location.profile_id 解析 profile，生成 database(kind) 图标、一个空格、连接名和非空路径段；普通 subtitle 同样归入右侧。元信息使用 theme.muted，连接图标复用 database_color；选中背景覆盖图标、文本和弹性间隔。[open] 保持 theme.success，并计入右侧总宽度。

**Step 3 — 实施确定性的预算策略。**
先扣除 marker、对象图标和一个空格；正常窗口右侧目标上限为剩余空间的 40%，左侧优先保留至多 24 格的标题最低预算。标题较短时可把空余借给右侧。若完整路径放不下，依次缩短 database、schema，再截断连接名；若连最小连接标识及两格间隔都放不下，隐藏位置段。所有边界使用 saturating_sub。

**Step 4 — 用实际宽度填充。**
分别完成左右截断，再重新计算最终宽度：`gap = width.saturating_sub(left_width + right_width)`。右侧非空时 gap 至少为 2；不足时重新执行降级。将 gap 空格插入两侧之间；右侧为空时不强制两格间距。截断 helper 为省略标记预留宽度，宽字符不可切半，零宽字符不能造成越界。

**Step 5 — 保持交互区域一致。**
每行 HitRegion 使用 layout.results 的 x/y/width，索引为 start+offset；整行可点击，包括右侧元信息和空白间隔。滚动窗口依旧按唯一选中 ID 找位置。

**Step 6 — 加入 Buffer 行为测试。**
用 TestBackend 实际绘制并检查 cell 坐标和 style，而不是只 contains 字符串：
- 不同标题长度的右侧最后一个可见 cell 横坐标相同。
- 只有一行具有选择 marker 和 selection 背景，选中空白也连续着色。
- PostgreSQL、MySQL、SQLite 连接图标位于连接名前；分别覆盖 NerdFont / Unicode / Ascii。
- 长中文名、窄终端、长 ASCII 图标、不带元信息、带 [open]、失效连接回退均不覆盖边框。
- 滚动后点击右侧区域，选中并打开该行对应对象。

Run: `cargo test --lib ui::omni -- --nocapture`
Run: `cargo test --test ui_render omni -- --nocapture`
Run: `cargo test --test omni_input --test omni_navigation -- --nocapture`
Expected: 对齐坐标、单选背景、图标和点击目标全部正确。

**提交边界：** `fix(omni): right-align metadata and show connection icons`

### Task 7：集成检查与最终验收

**Files:**
- Verify: 所有上述修改文件。
- Record: 本计划末尾执行记录。

**Step 1 — 完整 Omni 回归。**
Run: `cargo test --test omni_flows --test omni_input --test omni_navigation --test omni_providers --test omni_resume --test omni_search`
Expected: 命令、连接筛选、Console、多步交互、挂起恢复和搜索通过。

**Step 2 — 与 CI 一致的最终检查。**
Run: `cargo +1.94.0 fmt --all -- --check`
Run: `cargo +1.94.0 clippy --all-targets --all-features -- -D warnings`
Run: `cargo +1.94.0 test --all-targets --all-features`
Expected: 全部通过。涉及测试环境的适配器按 Task 4 单独记录真实执行结果。通过后仅在新变更或新失败出现时重复检查。

**Step 3 — 手工复核截图场景。**
在有 sys_user 及其列/约束的测试库打开 Omni，搜索 sys_user，验证只有表/视图、上下移动无重复高亮、右侧边缘一致、连接前有正确图标。调整终端尺寸，切换图标模式；验证同名跨 schema 对象可辨识、打开目标正确。

**Step 4 — 审查最终差异与交付记录。**
确认搜索协议的所有构造点已更新、旧消费者测试通过、当前变更没有被误加入提交。记录实际命令、通过/失败/跳过、数据库环境和手工验收结果。

## 4. 依赖关系与完成标准

顺序：`Task 1 → Task 2 → Task 3 → Task 4 → Task 5 → Task 6 → Task 7`。

完成标准：
- [ ] 本地与远程 catalog 结果都只有 relation。
- [ ] 类型范围在 limit 前生效，子对象不占结果预算。
- [ ] 候选 ID 唯一，键盘/鼠标/异步刷新均保持单选。
- [ ] 同一对象来源切换不改变展示格式或身份。
- [ ] 元信息右对齐，连接前有数据库类型图标。
- [ ] 长文本、窄窗口、中文和三种图标模式不溢出。
- [ ] Agent 等全对象消费者的现有能力通过回归。
- [ ] 定向测试、真实数据库测试及最终 CI 检查有执行记录。

## 5. 执行记录

计划已完成；实现与测试待执行。每个任务完成后记录修改、验证命令、结果与偏差说明。
