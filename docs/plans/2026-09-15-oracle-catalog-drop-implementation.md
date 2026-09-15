# Oracle Catalog Drop Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 本仓库执行者应先确认该技能是否可用；不可用时按本文逐项执行并记录验收结果。

**Goal:** 让 Oracle Explorer 的 Table、View、Sequence 删除完整可用，正确处理标识符、连接目标、事务隔离及删除后的界面状态。

**Architecture:** 由 OracleAdapter 生成经过校验的 CatalogDropPlan，复用既有确认框、Command/Action 和目录更新流程。Oracle 删除通过目标明确的独立短生命周期连接执行，避免共享会话中的未提交事务被 DDL 隐式提交。删除支持情况使用适配器能力查询，成功后定向同步目录、补全及已打开对象页。

**Tech Stack:** Rust 2024 / Rust 1.94、Tokio、ratatui、oracle 0.6.3、现有 CatalogDrop 与 reducer 测试体系。

---

## 1. 已确认事实与实施边界

- `src/db/mod.rs:488` 的 `plan_catalog_drop` 中 Oracle 分支固定返回 Unsupported，是截图错误的直接来源。
- `src/db/oracle.rs:67` 已支持 Table/View/Sequence 创建和编辑；`:943` 已能执行非查询 SQL。
- `src/db/catalog_drop.rs` 已提供身份绑定、执行目标和 SQL 校验；复用这些检查。
- `src/app.rs:8475–8770` 已有规划、确认、执行、成功和失败处理。
- `src/runtime.rs:2759` 当前通过 `active_database` 查找连接后直接执行；该查找按 ConnectionIdentity，未精确限定 ExecutionTarget。
- OracleAdapter 持有共享 `Arc<Mutex<oracle::Connection>>`；事务后端克隆适配器。
- `resolve_catalog_mutation_connection` 命中活动连接时会复用会话，不能直接当成隔离连接获取器。
- `CatalogMutationCapabilities` 当前没有 drop 字段；删除支持信息需要补充。
- `CatalogDropSucceeded` 已处理目录子树、补全和搜索，尚未处理 Relation 页失效。
- 默认 Cargo feature 已包含 `driver-oracle`；纯规划测试应同时支持 `--no-default-features`。

本次交付支持三类顶层对象。Oracle Schema/Database、列、索引、约束、物化视图等不在本次支持集合内；能力查询需准确给出未支持原因。普通删除不生成 CASCADE CONSTRAINTS、PURGE、IF EXISTS 或匿名 PL/SQL 包装。

### 核心不变量

1. 元数据原始大小写不变；标识符逐段双引号转义。
2. SQL 只有 `schema.object`，数据库/Service 用于连接选择。
3. 取消确认不建立删除连接、不执行 SQL。
4. 删除连接与控制台事务连接不同；失败不回退到共享连接。
5. 删除结果只更新匹配请求、连接代次和目录版本的状态。
6. 数据库报错保留 ORA 编号；失败不能先从目录移除对象。

## 2. 任务顺序

| 任务 | 依赖 | 完成标志 |
| --- | --- | --- |
| 1. Oracle 删除规划器 | 无 | 三种对象生成正确 SQL，错误元数据被拒绝 |
| 2. Oracle 独立执行连接 | 1 | 真实 DROP 走独立目标连接，所有退出路径释放资源 |
| 3. 删除能力查询及入口提示 | 1 | Oracle 支持集合一致，其他驱动行为保持兼容 |
| 4. 请求生命周期与确认展示 | 1、2 | 过期结果不覆盖新界面，重复确认只执行一次 |
| 5. 删除后的目录与对象页同步 | 4 | 数量、分页、搜索、补全、对象页同步 |
| 6. Oracle 集成验证 | 1–5 | 真实对象删除、依赖错误、事务隔离通过 |
| 7. 文档与最终检查 | 1–6 | 功能说明及验证记录完整 |

每个任务按“补充有意义的回归测试 → 确認新增断言失败 → 实现 → 运行定向测试 → 审阅 diff”执行。下文的 commit message 为逻辑提交建议，实际提交按用户要求执行。

## Task 1：实现 Oracle 删除规划器

**Files:**
- Modify: `src/db/oracle.rs`
- Modify: `src/db/mod.rs`
- Test: `tests/catalog_drop.rs`

### Step 1：添加 Oracle 元数据 fixture 和测试

在现有测试中增加 OracleAdapter import；新增 Oracle fixture，父节点必须是 Schema，避免复用现有 `parent_id: None` 的宽松 fixture。

fixture 示例身份：

```text
database = SUPPORTDB
schema = MFGSUPPORT
object = tt1
id.native_path = [SUPPORTDB, MFGSUPPORT, tt1]
parent_id = Schema([SUPPORTDB, MFGSUPPORT])
```

建议测试名：
- `oracle_drop_plans_quote_supported_objects`
- `oracle_drop_preserves_identifier_case_and_escapes_quotes`
- `oracle_drop_rejects_inconsistent_metadata`
- `oracle_drop_rejects_unsupported_kinds`

覆盖 Table/View/Sequence、小写、空格、内嵌双引号、标识符中的分号/CASCADE 字样。标识符内的这些文本不得误判为 SQL 子句。非法输入覆盖 profile 不符、entry/request ID 不符、kind 不符、缺少 schema、路径长度错误、路径和 qualified_name 不一致、错误父 Schema。

### Step 2：运行新增测试，确认当前缺少规划器

```bash
cargo test --no-default-features --test catalog_drop oracle_drop
```

预期：新增测试因 Oracle 规划器不存在而编译失败，或因当前 Unsupported 分支失败；记录真实失败，不把无关环境故障当成红灯。

### Step 3：实现规划器

新增 `OracleAdapter::plan_catalog_drop(request, entry)`，返回 `Result<CatalogDropPlan, CatalogDropError>`。

实现顺序：
1. 校验 request/profile、entry/request、kind 一致性。
2. 将三种 CatalogKind 映射为固定关键字 TABLE、VIEW、SEQUENCE，其他类型返回 Unsupported。
3. 校验三段身份路径、Schema 父节点、qualified_name 的一致性。元数据损坏返回 InvalidMetadata，不混用 Unsupported。
4. 使用现有 `oracle::quote_identifier` 拼接 schema 和 object；不 trim 或 uppercase 对象名。
5. 调用 `CatalogDropPlan::new`，再验证完整 plan 后返回。

关键 SQL 生成形式：

```rust
let sql = format!(
    "DROP {} {}.{}",
    keyword,
    quote_identifier(schema),
    quote_identifier(&entry.qualified_name.object),
);
```

`src/db/mod.rs` 改为 `Self::Oracle(_) => OracleAdapter::plan_catalog_drop(request, entry)`。

### Step 4：验证

```bash
cargo test --no-default-features --test catalog_drop
cargo test --test catalog_drop
```

验收：截图目标生成 `DROP TABLE "MFGSUPPORT"."tt1"`，所有已有驱动规划测试通过。

**建议提交：** `feat(oracle): support catalog drop planning`

## Task 2：在独立 Oracle 会话中执行删除

**Files:**
- Modify: `src/runtime.rs`
- Test: `src/runtime.rs` 中新增私有 helper 的单元测试模块
- Test: `tests/oracle_catalog_drop.rs`（Task 6 创建真实数据库验证）

### Step 1：明确目标和生命周期测试

对可提取的纯逻辑增加测试：由已验证 entry 得到 ExecutionTarget，匹配 profile 和数据库，拒绝过期 ConnectionIdentity；验证相同 identity 下不同数据库目标不会互相替代。不要为了模拟一条 SQL 新建完整驱动抽象。

### Step 2：增加 Oracle drop 专用执行分支

在 `execute_catalog_drop` 普通目标路径中识别 Oracle，交给独立 async helper，例如 `execute_oracle_catalog_drop`。

执行顺序固定为：
1. `plan.validate()`。
2. 取得最新 profile，检查只读和 Oracle 类型。
3. 由 request.entry 的数据库和 Schema 构造 ExecutionTarget；验证 profile 与目标匹配。
4. 按 `ConnectionKey::new(identity, target)` 的项目约定验证活动目标。如果活动连接以另一种 Schema 范围存储，先核实 `connect_target`/ConnectionKey 的规范化，避免凭猜测产生误拒绝。
5. 使用 `resolve_profile_password` 复用凭据解析。
6. 通过 `DatabaseConnection::connect_target` 新建连接，绝不直接克隆活动 OracleAdapter。
7. 建连后再次核实活动身份、profile 只读状态及目标有效性；不持有 registry/connection 锁跨数据库 I/O。
8. 在新连接上执行 `plan.sql()`；保存结果后关闭/释放该连接。
9. 发出原有 CatalogDropSucceeded 或 CatalogDropFailed。

CatalogDropExecutionTarget::CurrentConnection 保持“当前逻辑连接目标”的含义，不承诺复用物理会话；在代码注释中写清。暂不为单一 Oracle 策略扩大公共枚举。

### Step 3：落实失败语义

- 凭据解析、建连、目标校验失败：返回执行错误，不能降级成共享连接执行。
- Oracle 执行错误：保留经终端清洗的原始 ORA 信息。
- 网络中断导致执行结果不确定：不自动重试 DROP；提示刷新目录确认结果。
- 审查 `spawn_blocking` 生命周期：任务 abort 不等于 Oracle 调用取消，不将关闭 UI 描述成数据库已取消。
- 成功、SQL 失败、建连后身份失效均释放自有连接；不要写会误导的“回滚 DDL”逻辑。

### Step 4：验证

```bash
cargo test --lib oracle_catalog_drop
cargo test --test profile_runtime
```

纯测试不证明会话隔离，Task 6 的第二观察连接测试为该任务最终验收条件。

**建议提交：** `fix(oracle): isolate catalog drops from console transactions`

## Task 3：统一删除能力与入口提示

**Files:**
- Modify: `src/db/catalog_drop.rs`
- Modify: `src/db/oracle.rs`
- Modify: `src/db/mod.rs`
- Modify: `src/app.rs`
- Modify: `src/input/keymap.rs`、`src/help.rs`（仅现有删除入口确实需要接入的位置）
- Test: `tests/catalog_drop.rs`、`tests/catalog_reducer.rs`、`tests/keymap.rs`

### Step 1：定义单一能力来源

优先在 catalog_drop 模块定义独立 drop availability 接口，复用已有 `CatalogMutationAvailability` 的 Available/Unavailable 语义。避免为加入 drop 修改大量 CatalogMutationCapabilities 结构体初始化。

OracleAdapter 提供按 CatalogKind 查询的能力函数，并由规划器复用支持集合；DatabaseConnection 提供分发。App 需要连接前静态查询时，通过 DatabaseKind 包装同一 Oracle 函数。

其他驱动暂沿用各自规划器决定细粒度支持情况；不要把 Oracle 三种类型的限制套到所有数据库。若接口要覆盖所有驱动，必须逐一按已有 planner 分支映射并补回归测试后接入。

### Step 2：接入 App

在 RequestDropCatalogObject 中已有只读、归属检查之后、发送 PlanCatalogDrop 之前查询 Oracle 能力。

未支持类型给出具体信息，例如 `Oracle catalog drop is not available for Index`。菜单/帮助若有删除可用性展示则复用同一查询；快捷键和直接 Action 都经 reducer 校验。

### Step 3：验证

增加测试证明：Oracle 三类对象产生 PlanCatalogDrop，未支持类型不产生命令，已支持驱动原有删除请求行为不变。

```bash
cargo test --test catalog_drop
cargo test --test catalog_reducer
cargo test --test keymap catalog_drop
```

**建议提交：** `feat(catalog): expose Oracle drop availability`

## Task 4：确认框与异步请求生命周期

**Files:**
- Modify: `src/app.rs`
- Modify: `src/model/workspace.rs`（若待处理请求需独立状态）
- Modify: `src/ui/mod.rs`
- Test: `tests/catalog_reducer.rs`
- Test: `tests/ui_render.rs`

### Step 1：补充现有状态机回归测试

现有 CatalogDropConfirm 和 Succeeded 有部分身份检查；PlanReady、PlanFailed、DropFailed 仍需核实并统一防过期处理。

测试情形：
1. A 请求返回前已切换连接，A 的 PlanReady 不得弹窗。
2. B 请求替代 A 后，A 的失败不能覆盖 B 的确认框。
3. 目录 epoch 变化后旧结果不更新当前目录。
4. 重复确认只产生一次 ExecuteCatalogDrop。
5. 取消只清理对应待处理状态。

### Step 2：实现请求匹配

沿用现有请求状态存储模式，必要时增加一个 pending drop request 字段，绑定 connection、request_id、catalog_epoch。初次规划和维护数据库重规划均使用同一匹配函数；避免两个判定口径。

执行阶段的成功和失败都只处理当前匹配的 plan。过期结果不覆盖现有 overlay；后台实际成功但当前结果过期时，使所属目录下次加载能重新获取状态，不把未更新 UI 当成执行未发生。

### Step 3：确认框展示

复用已有 SQL 预览与默认 Cancel。Oracle 对象上下文展示完整 schema.object，SQL 保持原始引号；避免修改通用 `qualified_name` 字段含义后影响其他驱动断言。

### Step 4：验证

```bash
cargo test --test catalog_reducer catalog_drop
cargo test --test ui_render catalog_drop
cargo test --test keymap catalog_drop
```

**建议提交：** `fix(catalog): reject stale drop planning and execution results`

## Task 5：同步目录数量、分页缓存与已打开对象页

**Files:**
- Modify: `src/app.rs`
- Modify: `src/model/relation.rs`
- Modify: `src/model/explorer.rs`
- Modify: `src/model/workspace.rs`
- Test: `tests/catalog_reducer.rs`
- Test: `tests/explorer_tree.rs`（实施前确认现有对应测试文件；不存在则将测试放入 catalog_reducer）

### Step 1：检查现有删除计数逻辑

沿 `remove_dropped_subtree → remove_subtree` 检查 CatalogCount、owner 分页、游标、加载状态更新。已经正确实现的部分直接补验证，不重复减数。

### Step 2：补充状态测试

- 955 个对象、仅加载一页时，删除已加载表后数量正确。
- 补全与前端搜索不再命中被删除对象。
- 后续旧分页响应不能让对象重新出现；新分页无重复。
- 打开的目标 Relation 页失效，另一个 Schema 的同名页不受影响。
- 删除失败时目录、数量和对象页保持原状态。

### Step 3：实现精确失效

成功后继续复用现有子树删除和补全清理。对象页匹配使用 CatalogId/profile/namespace，不用裸表名；优先复用 `invalidated_by_catalog_mutation`、`invalidate_catalog_mutation`，若现有逻辑不能表达“对象已删除”则增加专用状态。

已删除对象页可保留历史结果供查看，但不得继续刷新或提交；晚到的该对象查询/DDL/元数据结果不能恢复可操作状态。

对相关 Tables/Views/Sequences owner 失效分页请求。计数或缓存需要服务器重取时，只刷新目标分组及相关 summary，并处理 epoch/generation；不全量加载整个 Schema。

### Step 4：验证

```bash
cargo test --test catalog_reducer
```

运行实际修改的 explorer/relation 对应测试目标；命令以仓库中已有目标为准。

**建议提交：** `fix(catalog): synchronize dropped objects across explorer and tabs`

## Task 6：真实 Oracle 集成测试

**Files:**
- Create: `tests/oracle_catalog_drop.rs`
- Reference: `tests/oracle_adapter.rs`

### Step 1：建立测试 fixture

复用现有环境变量：

```text
LAZYDB_TEST_ORACLE_URL
LAZYDB_TEST_ORACLE_USER
LAZYDB_TEST_ORACLE_PASSWORD
```

通过环境读取，不把凭据写入代码、计划或命令行。测试对象使用短随机后缀，名称总长度控制在 30 字节以内以兼容旧版 Oracle。使用当前测试 Schema，资源清理只针对测试自己创建的对象。

未配置环境时明确输出 SKIP；已配置但连接失败、驱动加载失败应失败，避免误报已验证。每个测试的最后清理在主体断言失败时仍应尝试执行。

### Step 2：覆盖数据库语义

1. 创建小写双引号表，通过真实 catalog 元数据构建请求并删除，目录查询确认不存在。
2. 同 Schema 创建大写和小写两个不同对象，删除小写对象后大写对象仍在。
3. View 和 Sequence 分别创建、规划、执行并验证不存在。
4. 创建父子外键，普通 DROP 父表返回依赖错误，父表仍在，外键仍在；不自动追加 CASCADE。
5. 删除已不存在对象，错误保留 ORA 信息。
6. 跨 Schema 权限测试仅在测试账号具备相应 fixture 条件时执行，并明确记录条件性覆盖。

### Step 3：验证事务隔离

使用连接 A、删除专用连接 B、观察连接 C：
1. 准备待写入表与独立待删除对象。
2. A 执行未提交 INSERT，C 查询不可见。
3. 通过 Task 2 的实际 runtime 执行路径删除待删除对象，不以测试中手写的独立连接代替被测路径。
4. C 再次查询，A 的 INSERT 仍不可见。
5. A ROLLBACK 后确认数据未保存，删除对象确已不存在。

可以在 `src/runtime.rs` 内编写受 feature/env 控制的测试以访问私有 helper。Oracle 事务 backend 当前使用 `BEGIN` 的行为需要单独核实；隔离测试可直接使用 oracle 连接的未提交 DML 和 rollback，不让既有手动事务启动问题遮蔽本次验证，也不将其计作“手动事务端到端已通过”。

### Step 4：运行

```bash
cargo test --features driver-oracle --test oracle_catalog_drop -- --nocapture
cargo test --features driver-oracle --lib oracle_catalog_drop -- --nocapture
```

验收报告区分 PASS、FAIL、SKIP；只有配置真实 Oracle 并通过隔离断言，才宣称数据库执行和事务隔离已验证。

**建议提交：** `test(oracle): cover catalog drops and transaction isolation`

## Task 7：文档及最终检查

**Files:**
- Create: `docs/oracle-catalog-drop.md`
- Modify: `docs/oracle-table-editor.md`（增加关联说明）

### Step 1：文档内容

说明支持对象、Schema 限定与大小写、独立会话执行、普通 DROP 的回收站条件、ORA 依赖/权限错误、失败后如何刷新，以及集成测试配置。说明 Oracle 普通 DDL 不能用 ROLLBACK 撤销；不承诺所有对象都进入回收站。

参考：Oracle 官方 DROP TABLE 文档：
https://docs.oracle.com/en/database/oracle/oracle-database/19/sqlrf/DROP-TABLE.html

### Step 2：运行最终检查

```bash
cargo fmt --all -- --check
cargo test --no-default-features --test catalog_drop --test catalog_reducer
cargo test --test catalog_drop --test catalog_reducer --test profile_runtime
cargo test --test keymap catalog_drop
cargo test --test ui_render catalog_drop
cargo check --all-targets
git diff --check
```

如仓库 CI/贡献文档另有要求则补充相应命令。真实 Oracle 测试按 Task 6 单独记录；不要将默认跳过视为通过。测试无新增故障后不重复扩大测试范围，除非本次公共状态机变更暴露额外回归。

### Step 3：交付记录

- 列出实际修改文件和关键决策。
- 列出命令及结果，明确真实 Oracle 环境是否可用。
- 给出截图场景验收：选中小写表 → 正确 SQL 确认框 → 确认 → 成功删除 → 分组数量和搜索更新。
- 保留已有 `docs/plans/2026-09-15-consoles-panel-layout.md` 用户工作；不纳入本任务提交。

**建议提交：** `docs(oracle): document catalog drop behavior`

## 3. 最终验收清单

- [ ] Table/View/Sequence 可删除，未支持类型原因清晰。
- [ ] 小写和特殊字符标识符正确，数据库名称不拼入对象 SQL。
- [ ] 规划器拒绝损坏或不匹配元数据。
- [ ] 确认前无删除 SQL，重复确认单次执行。
- [ ] Oracle DROP 使用独立正确目标会话，控制台未提交数据不被提交。
- [ ] 过期规划/执行消息不覆盖新 overlay 或目录。
- [ ] 失败保留对象并显示实际 Oracle 错误。
- [ ] 成功同步目录、计数、分页、搜索、补全及已打开对象页。
- [ ] 默认 feature 和 no-default-features 的纯测试通过。
- [ ] 真实数据库覆盖情况按 PASS/FAIL/SKIP 如实记录。

## 4. 执行方式

推荐在当前会话按任务顺序逐项实现，每完成一项审阅差异并验证后推进。也可在独立工作树的新会话按本文执行；如需子代理分任务执行，由用户明确选择后再启用。
