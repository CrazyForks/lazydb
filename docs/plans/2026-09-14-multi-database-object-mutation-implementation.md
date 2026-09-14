# Multi-Database Object Mutation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 为 LazyDB 所有数据库提供符合引擎语义的对象新增和编辑能力，统一 Explorer 的 `a/e` 入口、能力提示、定义编辑、执行结果和刷新行为。

**Architecture:** 保留现有 Action → Command → Runtime → DatabaseConnection → Adapter 链路，建立唯一的对象操作解析器；公共编辑器管理状态和公共字段，各适配器管理原生定义、字段差异、DDL 规划和执行策略。SQL 引擎共享 Catalog Mutation 协议，Redis 使用同一入口规范下的原生 Key-Value 变更协议；按对象完整闭环逐项开放能力。

**Tech Stack:** Rust 2024 / Rust 1.94、Ratatui、Tokio、SQLx、Tiberius、可选 Oracle 驱动、redis crate；沿用 Cargo integration tests 与 GitHub Actions。

---

## 0. 执行说明与基线

- 本文是实施计划，新增模块、类型和测试名称均为拟议命名，不表示代码已经实现。
- 顶部执行技能若在执行环境不可用，按本文依赖顺序逐项实施，不假定存在额外工具。只有明确选择代理协作时才分派代理。
- 本次规划发现当前工作区有大量已暂存改动，`src/ui/mod.rs` 为 `UU`。先完成现有合并，再从确认后的提交建立实施分支/工作树；不要清理或覆盖这些改动。
- 当前同时存在 Redis preview、keys navigation/filter/delete 计划。任务 19–20 应先对照其最终实现，复用身份、缓存失效、值分页和删除操作协议。
- 行号来自规划时工作树，执行时按符号重新定位。第一步使用代码索引检查目标符号及调用者；索引提示文件过期时读取对应文件。
- 本计划不要求在规划阶段启动数据库或跑业务测试。下文命令是实施时的验证要求，提交也是完成对应任务后的建议提交边界。
- 每个任务的实现步骤允许再按对象/字段拆成 2–5 分钟动作；先建立具体失败用例，再实现，再运行对应测试。不要一次添加所有能力开关。

### 已确认的代码依据

| 位置/符号 | 当前问题 |
| --- | --- |
| `src/app.rs::explorer_add_options` | 连接级 Database/User/Role 限制为 PostgreSQL |
| `src/app.rs::open_profile_catalog_create` | 表单入口重复检查 PostgreSQL |
| `src/app.rs::selected_catalog_create_options` | Oracle 返回 None，其他引擎依赖空能力 |
| `src/app.rs` 的 `Action::OpenCatalogEdit` 分支 | PostgreSQL 限制及对象类型白名单 |
| `src/help.rs::catalog_editor_capabilities` | 编辑统一使用 PostgreSQL 能力 |
| `src/input/keymap.rs::map_explorer` | 下级节点 `a` 不可用时直接返回 None |
| `src/db/catalog_mutation.rs::create_options` | Database 固定只能创建 Schema |
| `src/db/catalog_mutation.rs::DatabaseDefinition/RoleDefinition` | 定义含 PostgreSQL 专属字段 |
| `src/db/mod.rs` 的定义读取、规划、执行分发 | 非 PostgreSQL 未实现 |
| `src/db/mod.rs::resolve_relation_identity` | 非 PostgreSQL 返回 None |
| `src/runtime.rs::execute_catalog_mutation` | 需要扩展多步骤部分完成语义 |

## 1. 交付范围和支持定义

### 1.1 “支持编辑”的统一定义

支持由 `引擎 + 服务器版本 + 对象 + 操作 + 上下文` 决定，而不是一个全局布尔值。

对每项已开放功能必须完成：入口可发现 → 创建或加载完整定义 → 表单字段可编辑性正确 → SQL/命令预览 → 规划与执行 → 成功/失败反馈 → 对象重定位与缓存刷新。

- 表：按操作区分重命名、增加列、类型/默认值/可空性修改、删除列、约束变更、重建。
- 数据库：只展示原生可变属性，不暗示所有数据库都支持重命名。
- 索引/约束：原生 ALTER 与重建式修改分别描述，不标记成无差别编辑。
- 程序对象：源定义编辑，保留引擎属性和批次边界；不保证任意 SQL 能由表单解析回结构化字段。
- 本计划中的 SQL 编辑是对象结构/定义编辑。表格行数据的可写能力仍由现有 relation mutation 模型描述，不与 Catalog edit 布尔值混用。

### 1.2 引擎目标矩阵

| 引擎 | 核心闭环 | 完整覆盖阶段 | 原生边界 |
| --- | --- | --- | --- |
| PostgreSQL | 迁移现有功能、行为回归 | 消除现有类型白名单差异，接入程序对象模型 | 现有已支持能力必须保留 |
| Oracle | 表、视图、序列新增/编辑 | 用户/Schema、角色、索引、约束、函数/过程/触发器及包 | Schema 与用户关联；实例/PDB 创建不等同普通建库 |
| MySQL | Database、表、视图新增/编辑 | 索引、约束、用户/角色、函数/过程/触发器 | DatabaseIsSchema；用户身份包含 host |
| MariaDB | 复用 MySQL 核心闭环 | 独立版本能力、序列及其他原生差异 | 不能直接以 MySQL 版本号推断能力 |
| SQL Server | Database、Schema、表、视图新增/编辑 | 序列、索引、约束、Login/User/Role、函数/过程/触发器 | server/database principal 不同；DDL 批次规则 |
| SQLite | 文件库、表、视图、索引、触发器 | 复杂表结构重建；附加库入口及生命周期 | 无独立 CREATE SCHEMA/用户/角色 |
| Redis | String/Hash/List/Set/ZSet Key 和元素新增/编辑、TTL | Stream 的追加、删除及可支持属性编辑 | 数据库编号不是可 CREATE 的数据库；Stream 条目不能原位改写 |

Oracle 实例/CDB/PDB 的管理作为任务 11 的明确产品决策：普通 Explorer 不伪装为 CREATE DATABASE；如纳入支持，提供独立管理操作、目标和表单后再开放。对原生不存在的操作显示“不适用”，对尚未实现的原生操作显示“暂未实现”，对版本不足显示版本原因。

## 2. 技术决策

1. 不引入插件框架，继续由 `DatabaseConnection` 枚举分发。
2. 新建 `src/model/object_actions.rs`，统一入口策略；该层不连接数据库、不执行 SQL。
3. 扩展现有 `CatalogMutationCapabilities`，提供创建位置规则和细粒度编辑操作。避免保留两套长期独立的能力真相源。
4. 公共定义采用公共字段加类型化引擎扩展；数据库/主体允许引擎专属表单。未知、未加载、空值三个状态不可合并。
5. 拆出中立变更输入，数据库层不再直接依赖 UI `CatalogDraft`。迁移期通过单个转换函数兼容 PostgreSQL。
6. 执行计划保留步骤边界、固定连接要求和原子性。Oracle/MySQL 的隐式提交与 SQL Server 批次、SQLite 重建分别在适配器内处理。
7. 结果区分成功、失败且无已知提交、部分完成、提交结果未知；后两者刷新受影响目录，不自动重放整组语句。
8. 基线指纹只用于执行前乐观检查，不宣称消除检查与执行之间的所有竞争。服务器错误/锁语义仍由适配器处理。
9. Redis 共用动作可发现性、只读判断、身份和结果规范，不强行装进 SQL CatalogObjectDefinition。
10. 所有驱动新增 SQL/版本规则实施时查询官方文档（可用时通过 Context7），将已验证版本记录在能力文档。本文不以未经运行的 SQL 示例替代适配器验证。

### 2.1 公共执行结果契约草案

以下是拟议契约的完整最小形态；接入时复用仓库已有诊断与身份类型，而不是重复定义：

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationCompletion {
    Succeeded,
    Failed,
    PartiallyApplied,
    OutcomeUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MutationProgress {
    pub completed_steps: Vec<usize>,
    pub failed_step: Option<usize>,
    pub completion: MutationCompletion,
}
```

`completed_steps` 表示已确认执行成功的步骤；是否已提交由计划执行策略和 completion 一起解释。事务回滚后不能将其显示成已持久化修改。断连且无法确认提交时必须使用 OutcomeUnknown。

## 3. 依赖与里程碑

```text
T01 基线/范围契约
  → T02 能力模型 → T03 动作解析 → T04 入口接入
  → T05 定义/变更输入 → T06 表单
  → T07 计划执行结果 → T08 刷新/身份
  → T09 PostgreSQL 迁移回归
  → T10 Oracle 核心 → T11 Oracle 扩展
  → T12 MySQL 核心 → T13 MariaDB → T14 MySQL/MariaDB 扩展
  → T15 SQL Server 核心 → T16 SQL Server 扩展
  → T17 SQLite 核心 → T18 SQLite 重建/附加库
  → T19 Redis 协议 → T20 Redis 编辑器
  → T21 程序对象统一覆盖 → T22 CI/文档/最终验收
```

- M1：T01–T09，公共框架落地，PostgreSQL 功能回归通过。
- M2：T10–T11，优先解决当前 Oracle 问题。
- M3：T12–T16，MySQL、MariaDB、SQL Server 新增和编辑闭环。
- M4：T17–T20，SQLite 与 Redis 原生对象编辑闭环。
- M5：T21–T22，扩展对象覆盖及全矩阵验收。
- 各适配器核心任务在 M1 后技术上可独立推进，但共用的 `db/mod.rs`、`app.rs`、`action.rs`、`runtime.rs` 修改必须协调。默认按上述顺序执行。

## 4. 详细任务

### Task 01：确定干净基线和可执行支持矩阵

**Files:** 修改 `docs/database-capabilities.md`；新增 `tests/object_mutation_contract.rs`。

1. 执行 `git status --short`，确认现有合并完成；记录实施起点提交。必要时从该提交创建独立 worktree。
2. 对照既有 Redis 计划与最新代码，记录复用接口。
3. 在能力文档逐项列出引擎/版本/对象/操作，标记现有、待实现、不适用；明确 Oracle 管理操作边界。
4. 新建共享测试夹具：连接身份、只读配置、服务器版本、目录节点、对象基线；不复制每个驱动的完整 fixture。
5. 添加基线契约测试：非 PostgreSQL 当前不可用结果可观测，PostgreSQL 已有操作集合固定；后续任务显式更新预期。
6. 运行 `cargo test --locked --test object_mutation_contract`，预期基线测试通过。

**验收：** 每种数据库都有明确范围，未支持与不适用有区别；没有修改当前合并中的功能文件。

**提交：** `docs: define multi-database object mutation scope`。

### Task 02：扩展能力模型与创建位置规则

**Files:** 修改 `src/db/catalog_mutation.rs`、`src/db/catalog.rs`；新增 `tests/catalog_mutation_capabilities.rs`。

1. 写失败测试：MySQL Database 节点能直接产生表创建候选；PostgreSQL Database 只产生 Schema；SQLite 不产生用户创建候选。
2. 运行 `cargo test --locked --test catalog_mutation_capabilities`，预期新规则用例失败。
3. 增加创建位置描述、细粒度修改操作和可用原因；与现有 profile/create/edit 兼容迁移。
4. 创建位置解析使用 NamespaceModel 和适配器规则，不用数据库名称猜测引擎。
5. 增加版本未知、版本不足、适配器未实现的区别；权限未知不被当成硬性不支持。
6. 重新运行上述测试以及 `cargo test --locked --test catalog_mutation`。

**验收：** 一个引擎可以支持表重命名，同时明确不支持修改某种列属性。

**提交：** `refactor(db): model contextual object mutation capabilities`。

### Task 03：实现唯一的 Explorer 动作解析器

**Files:** 新增 `src/model/object_actions.rs`、`tests/explorer_object_actions.rs`；修改 `src/model/mod.rs`、`src/model/explorer_add.rs`、`src/model/explorer.rs`。

1. 写失败测试覆盖 Profile、Database、Schema、Group、Table、Column、连接分组及 Redis 选择上下文。
2. 运行 `cargo test --locked --test explorer_object_actions`。
3. 解析器输入使用选中对象身份、连接目标、只读状态和已协商能力，输出带原因和精确目标的动作描述。
4. 保留 Profile 的 `e` 编辑连接配置语义；对象 `e` 进入对象编辑。
5. 使用目录条目的资格名/父子关系解析目标，收敛 `native_path.first()` 与角色伪路径判断。
6. 增加跨 profile、过期连接身份、目标库不匹配测试，重新运行测试。

**验收：** 解析器纯计算、无 I/O；返回动作不可串到另一连接或数据库。

**提交：** `refactor(explorer): centralize object action resolution`。

### Task 04：接入菜单、快捷键、帮助和表单入口

**Files:** 修改 `src/app.rs`、`src/help.rs`、`src/input/keymap.rs`、`src/ui/mod.rs`、`src/action.rs`；测试 `tests/explorer_add.rs`、`tests/keymap.rs`、`tests/ui_render.rs`。

1. 添加失败用例：同一 Oracle/MySQL 上下文的菜单、帮助和按键使用同一能力结果。
2. 运行 `cargo test --locked --test explorer_add --test keymap --test ui_render`。
3. 替换 `explorer_add_options`、`open_profile_catalog_create`、`selected_catalog_create_options` 的 PostgreSQL 判断。
4. 替换 `OpenCatalogEdit` 的引擎判断及对象类型白名单，改用解析结果；删除 help 中调用 PostgreSQL 静态能力的路径。
5. 下级节点 `a/e` 已识别但不可用时派发说明动作，显示原因；帮助是否展示禁用条目按既有 UI 风格处理。
6. 菜单确认和实际打开时重新校验上下文，防止菜单打开后连接状态变化。
7. 运行上述测试，检查所有入口一致。

**验收：** 不再有 `PostgreSQL only` 作为通用对象管理限制；未接入适配器显示“暂未实现”，不错误开放。

**提交：** `fix(explorer): align create and edit entry points with capabilities`。

### Task 05：拆分公共定义、引擎扩展和中立变更输入

**Files:** 新增 `src/db/catalog_definition.rs`、`src/db/catalog_change_set.rs`；修改 `src/db/mod.rs`、`src/db/catalog_mutation.rs`、`src/model/catalog_editor.rs`、`src/db/postgres.rs`；新增 `tests/catalog_definition.rs`。

1. 写失败用例：仅修改 MySQL 表名时 engine/charset 保留；读取不完整的属性不会自动变成空值；PostgreSQL role 属性完整往返。
2. 运行 `cargo test --locked --test catalog_definition`。
3. 抽取公共身份、基线、列/约束结构；添加强类型方言扩展。不要为了满足 PostgreSQL owner 非空约束给 SQLite 填假值。
4. 建立 UI draft → 中立 change set 的单一转换，记录字段级 changed/unchanged，规划器只处理用户改动。
5. 将 planner 参数从 UI CatalogDraft 迁移为中立输入，PostgreSQL 暂用兼容转换保持行为。
6. 建立完整定义与仅可浏览定义的可编辑性差异；支持从原生元数据构建指纹。
7. 运行 `cargo test --locked --test catalog_definition --test catalog_mutation --test catalog_editor_reducer`。

**验收：** 数据库层的新接口不依赖 UI 草稿；不能因隐藏字段而删除原生属性。

**提交：** `refactor(db): separate native definitions from editor drafts`。

### Task 06：实现字段和操作感知的编辑表单

**Files:** 修改 `src/model/catalog_editor.rs`、`src/ui/catalog_editor.rs`、`src/app.rs`；测试 `tests/catalog_editor_state.rs`、`tests/catalog_editor_reducer.rs`、`tests/ui_render.rs`。

1. 添加字段显示/编辑/校验测试：Oracle 表无 PostgreSQL 专属选项，MySQL 展示 engine/charset，SQLite 无 owner 必填。
2. 运行 `cargo test --locked --test catalog_editor_state --test catalog_editor_reducer`。
3. 用字段描述控制隐藏、只读、可编辑和默认值，复用现有文本控件、选择器和表格列编辑器。
4. 数据库与主体表单按引擎变体渲染；复用外层 loading/form/preview/apply 状态机。
5. 禁用字段不接受键盘、粘贴或鼠标修改；验证错误准确聚焦字段。
6. 运行上述测试及 `cargo test --locked --test ui_render`。

**验收：** 表单生成的 change set 与用户修改严格对应，创建专属属性在编辑模式下不误开放。

**提交：** `feat(editor): render engine-aware object forms`。

### Task 07：扩展执行计划和结果协议

**Files:** 修改 `src/db/catalog_mutation.rs`、`src/action.rs`、`src/runtime.rs`、`src/app.rs`、`src/model/catalog_editor.rs`；新增 `tests/catalog_mutation_execution.rs`。

1. 添加可注入执行器测试：事务步骤失败并回滚、自动提交第二步失败、提交时断连、批次不可拆分。
2. 运行 `cargo test --locked --test catalog_mutation_execution`。
3. 为计划添加原子性、批次/步骤和同连接要求；计划身份保持 connection/request/epoch/target。
4. 引入 MutationProgress 和提交状态语义；运行时与 editor 支持部分完成、未知结果。
5. 保持现有只读检查和基线校验；DDL 使用独立操作连接，不隐式提交用户 SQL Console 的手动事务。
6. 失败后保留草稿和诊断，显示失败步骤；未知结果和部分完成触发重新加载，不自动重试完整计划。
7. 重新运行测试及 `cargo test --locked --test catalog_editor_reducer`。

**验收：** 结果文案不会将隐式提交错误描述成全部回滚；同连接和批次要求真实落实。

**提交：** `feat(runtime): track multi-step catalog mutation outcomes`。

### Task 08：统一对象身份重定位与影响刷新

**Files:** 修改 `src/db/mod.rs`、`src/db/catalog_mutation.rs`、`src/runtime.rs`、`src/app.rs`、`src/model/explorer.rs`；新增 `tests/catalog_mutation_refresh.rs`。

1. 添加失败用例：重命名对象后选中新 ID、表重建后刷新旧 tab、部分完成刷新目录、旧请求不能覆盖新连接。
2. 运行 `cargo test --locked --test catalog_mutation_refresh`。
3. 定义适配器重定位接口或等效解析策略，复用 CatalogMutationImpact/CatalogSelectionHint。
4. 根据 impact 失效对象定义、父目录、关系预览和相关补全元数据；优先局部刷新。
5. 处理已打开标签页：对象仍存在则重新解析并加载，消失则进入明确失效状态。
6. 执行后只恢复选择和对象页，不改写用户原来的 Console 执行目标。
7. 运行新测试及 `cargo test --locked --test connection_switch --test relation_tabs`。

**验收：** 无旧 ID 悬挂、跨连接污染或成功后目录仍保持旧结构的问题。

**提交：** `fix(catalog): refresh mutation impacts and object identities`。

### Task 09：PostgreSQL 接入新协议并完成回归

**Files:** 新增 `src/db/postgres/mutation.rs`；修改 `src/db/postgres.rs`、`src/db/mod.rs`；测试 `tests/postgres_adapter.rs`、`tests/catalog_mutation.rs`。

1. 记录现有能力和真实往返用例，保留 schema/table/view/materialized view/sequence/index/constraint/database/role 的已实现行为。
2. 将 mutation 相关实现按模块边界迁移，接入中立 change set 和新结果；不借此重写无关查询实现。
3. 用适配器实际能力替代 App 白名单，确认索引/物化视图等入口与声明一致。
4. 运行 `cargo test --locked --test catalog_mutation --test catalog_editor_reducer`。
5. 设置现有 `LAZYDB_TEST_POSTGRES_URL` 后运行 `cargo test --locked --test postgres_adapter -- --nocapture --test-threads=1`。
6. 检查测试确实执行数据库往返，没有因环境变量缺失提前返回。

**验收：** M1 的入口、表单、计划和执行协议均由真实 PostgreSQL 回归验证。

**提交：** `refactor(postgres): adopt shared object mutation pipeline`。

### Task 10：Oracle 表、视图、序列核心闭环

**Files:** 新增 `src/db/oracle/mutation.rs`、`tests/oracle_catalog_mutation.rs`；修改 `src/db/oracle.rs`、`src/db/mod.rs`；测试 `tests/oracle_adapter.rs`。

1. 为标识符大小写/引用、跨 Schema 目标、类型/default/nullability、视图定义、序列版本差异写纯规划测试。
2. 运行 `cargo test --locked --test oracle_catalog_mutation`，确认缺失规划能力的测试失败。
3. 从原生目录读取完整定义和基线，保存未编辑的原生属性。
4. 按表、视图、序列逐项实现 create/load/diff/plan/execute/identity，每项完成后再声明对应操作可用。
5. 接入 Oracle 现有阻塞驱动执行方式、同连接操作和 DDL 提交语义；不假设异步任务取消等于数据库取消。
6. 使用 `LAZYDB_TEST_ORACLE_URL`、`LAZYDB_TEST_ORACLE_USER`、`LAZYDB_TEST_ORACLE_PASSWORD` 配置真实环境。
7. 运行 `cargo test --locked --test oracle_adapter -- --nocapture --test-threads=1`；新增 mutation 往返测试使用明确的 `catalog_mutation_` 前缀，单独运行该过滤器并确认没有跳过。
8. 验证未开启驱动时仍可编译：`cargo check --locked --no-default-features --all-targets`。

**验收：** 在 Oracle Schema/Tables/Views/Sequences 下 `a/e` 可完成真实新增、编辑和刷新；至少覆盖一次中途失败刷新。

**提交：** 按对象拆分 `feat(oracle): support table catalog mutations`、view、sequence 三次提交。

### Task 11：Oracle Schema/主体与关系子对象

**Files:** 修改 `src/db/oracle/mutation.rs`、`src/db/oracle.rs`、`src/db/catalog_definition.rs`、`src/model/catalog_editor.rs`、`src/ui/catalog_editor.rs`；测试 `tests/oracle_catalog_mutation.rs`、`tests/oracle_adapter.rs`；更新 `docs/database-capabilities.md`。

1. 明确 Schema 创建采用用户关联语义，并设计用户名、认证、默认表空间等字段；展示原生可编辑属性，不提供虚假的通用 Schema rename。
2. 用户和角色使用独立定义/身份，凭据继续使用现有 secret 封装和预览脱敏机制。
3. 按索引、主键、唯一约束、外键、检查约束分别补齐加载、创建、修改策略与依赖刷新。
4. 为不支持原生 ALTER 的修改生成明确重建计划，验证原生属性保留。
5. 对实例/PDB 管理形成已确认范围：如纳入，在独立目标类型和管理表单实现并提供真实测试后开放；否则文档明确“不属于普通对象管理”，不能把 Schema 创建描述成实例建库。
6. 运行 `cargo test --locked --test oracle_catalog_mutation` 及配置好环境的 Oracle `catalog_mutation_` 往返测试。

**验收：** Oracle 原生对象管理完整度有逐项清单，普通用户/Schema 创建与实例管理不会混淆。

**提交：** `feat(oracle): add namespace principal and constraint mutations`，按主体/索引/约束拆分。

### Task 12：MySQL Database、表、视图闭环

**Files:** 新增 `src/db/mysql/mutation.rs`、`tests/mysql_catalog_mutation.rs`；修改 `src/db/mysql.rs`、`src/db/mod.rs`；测试 `tests/mysql_adapter.rs`。

1. 写失败用例覆盖 DatabaseIsSchema、反引号转义、engine/charset/collation 保留、auto increment、generated column、视图属性。
2. 运行 `cargo test --locked --test mysql_catalog_mutation`。
3. 读取原生元数据和定义，避免只解析显示用 DDL 推断全部结构；仅修改目标字段。
4. 实现 Database 创建和可变属性编辑、表和视图的创建/编辑；不提供不受支持的通用数据库 rename。
5. 处理多步骤隐式提交、同连接执行和对象重定位。
6. 设置 `LAZYDB_TEST_MYSQL_URL`，运行 `cargo test --locked --test mysql_adapter -- --nocapture --test-threads=1`。

**验收：** Database 节点按 `a` 能建表/视图；修改一列不丢失其他列、索引、引擎和字符集。

**提交：** 按 Database/table/view 分别提交 `feat(mysql): ...`。

### Task 13：MariaDB 独立版本能力与语法覆盖

**Files:** 修改 `src/db/mysql.rs`、`src/db/mysql/mutation.rs`、`src/db/mod.rs`；新增 `tests/mariadb_catalog_mutation.rs`；测试 `tests/mysql_adapter.rs`。

1. 添加失败用例：相似版本字符串不能混淆 MySQL 与 MariaDB；支持范围取决于明确的引擎种类。
2. 运行 `cargo test --locked --test mariadb_catalog_mutation`。
3. 为共享适配器保存实际引擎和解析后的服务器版本；公共 SQL 共用、差异功能显式分支。
4. 补齐 MariaDB 序列等已纳入范围的对象，必要时扩展 catalog groups/discovery；不能只增加按钮但对象无法浏览。
5. 使用 CI 已有 MariaDB 配置运行 `env LAZYDB_TEST_MYSQL_FUNCTIONAL_INDEX=0 LAZYDB_TEST_MYSQL_URL="$LAZYDB_TEST_MARIADB_URL" cargo test --locked --test mysql_adapter -- --nocapture --test-threads=1`。
6. 新增 MariaDB 专属真实用例到共享测试的明确分支或专属 suite，CI 显式运行。

**验收：** 同一套实现分别通过 MySQL 8.4 与 MariaDB 11.4 往返；能力文档列出实际验证版本。

**提交：** `feat(mariadb): specialize object mutation capabilities and SQL`。

### Task 14：MySQL/MariaDB 索引、约束与主体

**Files:** 修改 `src/db/mysql/mutation.rs`、`src/db/catalog_definition.rs`、`src/model/catalog_editor.rs`、`src/ui/catalog_editor.rs`；测试 `tests/mysql_catalog_mutation.rs`、`tests/mariadb_catalog_mutation.rs`、`tests/mysql_adapter.rs`。

1. 用户身份建模为 user+host，不把同名不同 host 用户合并。
2. 逐项实现用户/角色定义、创建及原生可变属性编辑，按引擎版本处理角色语义。
3. 实现索引/约束读取及创建/变更，保留前缀、表达式、排序、可见性等支持范围内属性。
4. 每类先写规划和往返失败用例，再接通对应能力；只在定义完整时允许重建式修改。
5. 运行两种引擎的纯测试与真实 adapter suite。

**验收：** 主体名称无歧义，索引修改不丢失未触及属性，版本不支持时有明确原因。

**提交：** 按 principals/indexes/constraints 拆分 `feat(mysql): ...`。

### Task 15：SQL Server 核心闭环与批次执行

**Files:** 新增 `src/db/mssql/mutation.rs`、`tests/sqlserver_catalog_mutation.rs`；修改 `src/db/mssql.rs`、`src/db/mod.rs`；测试 `tests/sqlserver_adapter.rs`。

1. 添加失败用例：方括号引用、数据库/Schema 目标、identity/computed 列、命名 default constraint、视图批次要求。
2. 运行 `cargo test --locked --test sqlserver_catalog_mutation`。
3. 从 `sys` 目录及原生模块定义读取完整基线；无法读取的加密定义不标记成可编辑。
4. 实现 Database/Schema/table/view 创建和原生可变属性编辑。
5. 批次作为协议单元提交给 Tiberius，不把客户端 `GO` 当成服务器 SQL；事务策略按对象操作决定。
6. 配置 `LAZYDB_TEST_SQLSERVER_URL` 后运行 `cargo test --locked --test sqlserver_adapter -- --nocapture --test-threads=1`。
7. 运行 `cargo test --locked --test sqlserver_transactions -- --nocapture --test-threads=1`，验证不影响 Console 手动事务。

**验收：** 新增/修改视图真实通过服务器批次约束；默认约束修改定位到正确对象。

**提交：** 按 namespace/table/view 拆分 `feat(sqlserver): ...`。

### Task 16：SQL Server 序列、索引、约束与主体

**Files:** 修改 `src/db/mssql/mutation.rs`、`src/db/catalog_definition.rs`、`src/model/catalog_editor.rs`、`src/ui/catalog_editor.rs`；测试 `tests/sqlserver_catalog_mutation.rs`、`tests/sqlserver_adapter.rs`。

1. 将服务器 Login、数据库 User、Role 分别建模，执行目标明确绑定服务器或数据库范围。
2. 实现创建与原生可变属性编辑，处理 owner/schema ownership 等字段的原生语义。
3. 实现 sequence/index/constraint 完整定义和规划，保留 INCLUDE、过滤条件及已支持索引属性。
4. 使用失败测试验证跨数据库重名主体不会被误修改；验证约束重建及对象页刷新。
5. 运行纯 planner suite 和配置环境后的 adapter suite。

**验收：** 主体与索引编辑的目标和保留属性可被真实测试证明。

**提交：** 按 principals/sequence/indexes/constraints 拆分 `feat(sqlserver): ...`。

### Task 17：SQLite 文件、表、视图、索引和触发器基础操作

**Files:** 新增 `src/db/sqlite/mutation.rs`、`tests/sqlite_catalog_mutation.rs`；修改 `src/db/sqlite.rs`、`src/db/mod.rs`、`src/model/profile_manager.rs`；测试 `tests/sqlite_adapter.rs`。

1. 用临时文件和内存数据库建立失败用例，覆盖文件新建与现有文件行为，不依赖外部服务。
2. 运行 `cargo test --locked --test sqlite_catalog_mutation --test sqlite_adapter`。
3. 文件库创建接入 Profile/连接流程，明确选择路径和已有文件行为，不发送 CREATE DATABASE。
4. 从 sqlite_schema 与 PRAGMA 读取定义；保留 STRICT、WITHOUT ROWID、生成列和对象源定义。
5. 实现建表、原生支持的 ALTER，以及视图、索引、触发器的创建和可实现的修改计划。
6. 复用 operation gate 和单连接池，验证读写操作不会相互死锁。
7. 重新运行上述测试。

**验收：** SQLite 核心对象 `a/e` 可真实往返，不出现 owner、角色或 CREATE SCHEMA 假选项。

**提交：** `feat(sqlite): add native object create and edit operations`。

### Task 18：SQLite 重建式改表与附加库生命周期

**Files:** 修改 `src/db/sqlite/mutation.rs`、`src/db/sqlite.rs`、`src/model/execution_target.rs`；新增 `tests/sqlite_table_rebuild.rs`；修改 `tests/sqlite_catalog_mutation.rs`。

1. 写真实失败测试：列类型/约束修改保留数据、索引、触发器、引用关系；失败后恢复原表。
2. 运行 `cargo test --locked --test sqlite_table_rebuild`。
3. 为复杂改表生成读取依赖→新表→显式列映射复制→替换→恢复依赖→校验的计划；处理 rowid/自增序列/生成列边界。
4. 按 SQLite 原生要求安排 foreign_keys 设置与事务边界，失败路径也恢复连接设置。
5. 检测不支持的虚拟表或无法完整保留的定义并说明原因，不对其生成有损重建。
6. 对 main/temp/attached namespace 提供准确入口；若支持 ATTACH，将附加库状态纳入连接重建策略并验证重连，不仅在临时连接 ATTACH。
7. 运行 `cargo test --locked --test sqlite_table_rebuild --test sqlite_catalog_mutation --test sqlite_adapter`。

**验收：** 至少覆盖外键、复合主键、触发器、索引、生成列和回滚恢复；附加库重连行为有明确测试。

**提交：** 分别提交 `feat(sqlite): rebuild tables for structural edits` 和附加库生命周期修改。

### Task 19：Redis 原生变更协议与驱动实现

**Files:** 新增 `src/db/redis/mutation.rs`、`tests/redis_object_mutation.rs`；修改 `src/db/redis/mod.rs`、`src/db/redis/types.rs`、`src/model/object_actions.rs`、`src/action.rs`、`src/runtime.rs`。

1. 先复核已有 Redis preview/navigation/delete 工作，复用其 request identity、binary key、选中目标和缓存失效接口。
2. 定义类型化的 Key/元素新增修改命令与结果，保留 key/value 原始字节，不经 SQL 字符串拼接。
3. 写失败测试覆盖 String 替换、Hash field、List 元素、Set member、ZSet score/member 和 TTL 保留/变更。
4. 运行 `cargo test --locked --test redis_object_mutation`。
5. 对依赖已有状态的替换使用原子命令或适当的事务/脚本；分页下 List 索引变化不得悄悄覆盖其他元素。
6. Stream 只开放真实支持的 append/delete/属性操作；不把删除并追加声称为原位编辑。
7. 新真实测试环境变量统一使用 `LAZYDB_TEST_REDIS_URL`，在 CI 必须存在；验证只读、过期 key、类型变化、二进制值和并发冲突。

**验收：** 原生类型操作不会意外改变 TTL、丢失集合其他成员或因预览文本转换破坏二进制值。

**提交：** 按 Key/集合/TTL 拆分 `feat(redis): add native object mutations`。

### Task 20：Redis 编辑器与统一快捷键体验

**Files:** 新增 `src/model/redis_object_editor.rs`、`src/ui/redis_object_editor.rs`、`tests/redis_object_editor.rs`；修改 `src/model/mod.rs`、`src/model/redis_browser.rs`、`src/ui/redis_browser.rs`、`src/ui/mod.rs`、`src/input/keymap.rs`、`src/app.rs`、`src/help.rs`。

1. 写失败用例：数据库/前缀节点 `a` 新建 Key，Key/元素 `e` 使用正确类型编辑器，Profile `e` 仍编辑连接配置。
2. 运行 `cargo test --locked --test redis_object_editor --test keymap`。
3. 表单支持类型选择、值/成员输入及 TTL；二进制编辑提供明确表示方式，不把显示文本当作原始值。
4. 接入 task 19 的执行与结果协议，保留草稿并显示冲突/类型变化。
5. 成功和不确定结果失效 scan/value/metadata 缓存，恢复选中 Key，维护分页和前缀上下文。
6. 运行上述测试及 Redis 真实变更 suite。

**验收：** Redis 提供与 SQL 对象管理一致的新增/编辑入口，但表单和命令符合 Key-Value 模型。

**提交：** `feat(redis): expose native create and edit forms`。

### Task 21：补齐程序对象和扩展对象编辑

**Files:** 修改 `src/db/catalog_definition.rs`、`src/db/catalog_change_set.rs`、`src/db/catalog_mutation.rs`、`src/model/catalog_editor.rs`、`src/ui/catalog_editor.rs` 及各引擎 mutation 子模块；新增 `tests/program_object_mutation.rs`；扩展各 adapter suite。

1. 将支持矩阵中的 function/procedure/trigger、Oracle package 等逐项映射到现有 CatalogKind；缺少类型时同步扩展 discovery/search/identity，不只扩展编辑器。
2. 建立源定义编辑变体，保留语言、参数/签名、执行上下文、definer、安全属性等引擎字段。
3. SQL 输入必须保持原生模块边界，不使用通用分号切分处理 PL/SQL 或存储过程体。
4. 对每种对象先写新建→读取→修改→读取的真实用例；定义不可读或操作原生不适用时输出明确原因。
5. 对 Oracle package spec/body、重载 routine 身份、SQL Server 批次、MySQL definer、SQLite trigger 分别验证。
6. 完成所有目标项后再开放对应 group 的 `a/e`；更新测试矩阵。
7. 运行 `cargo test --locked --test program_object_mutation` 和涉及引擎的真实 adapter suite。

**验收：** 目录已有的程序对象分组不会永久停留在“可浏览但没有新增/编辑闭环”；扩展对象有逐项覆盖记录。

**提交：** 按引擎和对象拆分 `feat(catalog): support native program object editing`。

### Task 22：CI、文档和全矩阵验收

**Files:** 修改 `.github/workflows/ci.yml`、`docs/database-capabilities.md`、`docs/keybindings.md`、`docs/architecture.md`、`docs/redis-browser.md`；新增 `docs/testing/catalog-mutations.md`；扩展 `tests/object_mutation_contract.rs`。

1. 公共 contract suite 检查每个已声明操作都有表单/定义读取/规划/执行支持，不通过枚举所有操作后一律返回 true 来满足测试。
2. 复用现有 PostgreSQL 16、MySQL 8.4、MariaDB 11.4、SQL Server 2022 CI 服务，追加 mutation round-trip 用例。
3. 新建 Oracle 专用作业或受控集成测试环境，安装驱动所需客户端，配置三个现有环境变量；新增 Redis 服务及 `LAZYDB_TEST_REDIS_URL`。
4. 数据库 CI 明确要求环境变量与连通性，不能靠测试提前 return 获得绿色结果；记录真实往返用例数。需要 ignored 测试时按具体名称显式运行。
5. 对对象创建使用唯一名称和 teardown，账户/数据库级用例使用专用测试目标；不要把当前用户业务连接当 fixture。
6. 更新文档：对象×操作×版本矩阵、`a/e` 语义、SQLite 重建、Oracle Schema、Redis TTL、部分完成与未知结果。
7. 运行项目现有完整质量检查：

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
cargo +1.94.0 check --locked --no-default-features --all-targets
```

8. 分别运行配置好环境的各 adapter suite；一般 Cargo 全测通过不能替代真实数据库验证。
9. TUI 手动验收各引擎：Profile、namespace、group、object/child 上的 `a/e`；创建、取消、编辑预览、执行失败、切换连接、重命名后 tab、只读连接。

**验收：** 所有目标矩阵项有真实验证记录；不适用项有原生理由；无“UI 可用但适配器返回未实现”的已发布路径。

**提交：** `test: validate multi-database object mutation matrix` 与 `docs: document cross-database object editing`。

## 5. 每个引擎/对象的合入检查

- [ ] 创建位置符合 NamespaceModel；菜单、帮助、按键和实际入口一致。
- [ ] 基线定义足够完整，未改字段和原生扩展保留。
- [ ] 方言标识符、字面量、参数、签名及命名空间正确。
- [ ] 能力按真实引擎/版本和具体操作开放。
- [ ] 修改计划与执行批次一致，提交/回滚/部分完成表达准确。
- [ ] 执行与用户 Console 手动事务隔离，目标连接身份不串用。
- [ ] 改名/重建后的目录、选中项、已打开页与缓存正确。
- [ ] 至少一条真实 create→load→edit→load 往返通过。
- [ ] 权限不足、基线变化和至少一种执行失败路径得到验证。
- [ ] 能力文档包含该项及真实验证版本。

## 6. 完成标准与执行交接

M2 是当前 Oracle 问题的优先交付点，M3/M4 是多引擎核心闭环交付点，M5 是本计划最终验收点。不能用“所有数据库都能打开空表单”或“能生成 SQL”作为项目完成标准。

本计划涉及 22 个可追踪任务，任务 10–21 需要进一步按对象拆为小提交。实际工期在 T01 的支持矩阵和 Oracle 测试环境确认后估算；SQLite 重建、主体跨作用域、程序对象完整定义是主要复杂度来源。

执行时可以在当前会话按依赖顺序实施，也可以在合并完成后的独立 worktree 会话中按里程碑推进；只有用户明确选择后才采用代理协作。当前工作区存在合并冲突，因此下一步应先确认实施基线，再开始 T01。
