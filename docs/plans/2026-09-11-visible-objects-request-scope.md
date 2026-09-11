# Visible Objects 请求级 Scope 一致性 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 修改 Visible Objects 后，同一活动连接立即支持新范围内的目录、Data、DDL 和对象身份解析，普通刷新即可恢复，无需重启或重连。

**Architecture:** 将 CatalogScope 统一作为操作请求的不可变快照，由 App 经 Command、Runtime、DatabaseConnection 传至 adapter。复用现有 RelationRequest.scope、连接 identity、请求 ID 和 catalog epoch；连接池只负责连接资源管理，范围校验由持有请求上下文的操作入口负责。

**Tech Stack:** Rust 2024 / Rust 1.94、Tokio、SQLx、Tiberius、现有 reducer/runtime/adapter 测试；不增加生产依赖。

---

## 1. 问题与约束

当前 `src/db/postgres.rs` 等 adapter 在建立连接时复制 `profile.catalog_scope`，但 `src/app.rs` 在 scope-only 保存后保留连接，只刷新目录。目录请求和 `RelationRequest` 已携带最新 scope，`src/runtime.rs` 的 Data/DDL 执行却没有向 DatabaseConnection 传递它，导致 adapter 继续按旧范围拒绝请求。SQL Server 的 `pool_for_database` 还会以旧 scope 校验数据库。

实施必须：

1. 不因 Visible Objects 变化重建连接池或重连。
2. 复用已有 request scope、connection identity、catalog epoch 和 stale-result 机制。
3. 保留原有数据库身份、OID、分页、事务和取消语义。
4. 区分对象不存在与对象超出 Visible Objects；范围外快照复用 `OutOfScopeSnapshot`。
5. 不覆盖主工作空间已有未提交文件；不新增生产依赖。

## 2. 目标接口

将 Data/DDL/身份解析改为显式接收不可变 scope 快照：

```rust
pub async fn preview_relation(
    &self,
    relation: &CatalogId,
    scope: &CatalogScope,
    options: &RelationPreviewOptions,
    page: PageRequest,
) -> Result<RelationPreview, DatabaseError>;

pub async fn relation_ddl(
    &self,
    relation: &CatalogId,
    scope: &CatalogScope,
) -> Result<RelationDdl, DatabaseError>;

pub async fn resolve_relation_identity(
    &self,
    relation: &CatalogId,
    scope: &CatalogScope,
) -> Result<Option<CatalogEntry>, DatabaseError>;
```

所有 DatabaseConnection 和 adapter 实现、内部调用方与测试必须同步迁移。Runtime 的预览调用必须使用 `&task_request.scope`，不能重新从 registry 读取 scope。

建议新增 `catalog_target_out_of_scope` 错误：`ErrorCategory::Configuration`、稳定 code `catalog_target_out_of_scope`、消息包含安全净化后的 database/schema/object。真实不存在或身份不匹配继续使用 `catalog_target_not_found`。

## 3. 逐项实施

### Task 1：建立回归基线

**文件：**
- Modify: `tests/sqlite_adapter.rs`
- Modify: `tests/relation_runtime.rs`
- Reference: `tests/profile_reducer.rs`

**步骤：**
1. 复用 SQLite 临时数据库 fixture，建表并建立一个排除目标 schema 的旧 scope。
2. 使用包含目标 schema 的新 scope 执行同一 DatabaseConnection 的 relation preview，旧实现应因 adapter 旧 scope 失败。
3. 增加 `visible_objects_scope_change_reaches_live_adapter` Runtime 测试，确保 RelationRequest 已包含新 scope，但旧 Runtime 调用链仍稳定暴露问题。
4. 运行定向测试并确认失败原因是范围校验，不是 fixture、编译或连接超时。

```bash
cargo test --test sqlite_adapter visible_objects -- --nocapture
cargo test --test relation_runtime visible_objects_scope_change_reaches_live_adapter -- --exact --nocapture
```

**复核：** 检查新增测试只依赖本地 SQLite；`git diff --check`；确认红灯来自旧 scope 未下传。

### Task 2：贯通 Data/DDL scope

**文件：**
- Modify: `src/db/mod.rs`, `src/runtime.rs`
- Modify: `src/db/postgres.rs`, `src/db/mysql.rs`, `src/db/sqlite.rs`, `src/db/mssql.rs`
- Modify: `tests/sqlite_adapter.rs`, `tests/postgres_adapter.rs`, `tests/mysql_adapter.rs`, `tests/sqlserver_adapter.rs`, `tests/postgres_relation_mutations.rs`

**步骤：**
1. 添加统一的范围外 `DatabaseError` 构造和净化测试。
2. 修改 `DatabaseConnection::preview_relation`、`relation_ddl` 及四种 adapter 签名。
3. Runtime 从 `RelationRequest.scope` 显式传入 Data 和 DDL；不读取连接创建时的 scope。
4. PostgreSQL 的预览和 DDL 在已有 relation identity 验证后按请求 scope 判断 schema；保留现有 SQL 和 xmin 逻辑。
5. MySQL、SQLite 按请求 scope 执行现有 database/schema 校验，保留大小写、attached database 和分页语义。
6. SQL Server 将范围判断从旧 scope 的 `pool_for_database` 移到具备请求上下文的目录/关系操作入口；保留连接池 closed 检查和数据库选择。
7. 逐一检查 SQL Server 的 probe、discovery、execute、object DDL、mutation 等调用，只有目录/关系操作使用请求 scope；无 scope 的物理执行路径遵循原 ExecutionTarget 契约。
8. 删除 adapter 中不再需要的 `catalog_scope` 字段、构造赋值、Debug 输出和测试 fixture 字段。
9. 迁移所有生产与测试调用方，测试传入实际 scope，不统一传 All 掩盖错误。
10. 添加同一连接 `allowed -> denied -> allowed` 的 Data/DDL 回归测试，并区分真实删除和范围外。

```bash
cargo check --all-targets
cargo test --test sqlite_adapter
cargo test --test relation_runtime visible_objects_scope_change_reaches_live_adapter -- --exact --nocapture
```

**复核：** 检查 `catalog_scope` 不再出现在 adapter 运行期状态；检查所有 preview/DDL 调用已传 scope；审阅 SQL Server 每个 `pool_for_database` 调用的校验上下文。

### Task 3：修复对象身份解析

**文件：**
- Modify: `src/action.rs`, `src/app.rs`, `src/runtime.rs`, `src/db/mod.rs`, `src/db/postgres.rs`
- Modify: `tests/catalog_reducer.rs`, `tests/relation_runtime.rs`, `tests/postgres_relation_mutations.rs`

**步骤：**
1. 给 `Command::ResolveCatalogRelation` 增加 `scope: CatalogScope`，由 App 从最新 profile 克隆。
2. Runtime 在解析任务中将该快照传给 DatabaseConnection。
3. PostgreSQL 先按 OID 解析当前对象，再按解析后的真实 database/schema 判断 scope，覆盖重命名和跨 schema 移动。
4. 增加 reducer 测试，断言命令携带最新 scope。
5. 增加范围扩大、范围缩小、对象真实删除三类身份解析测试。
6. 保留连接 identity、catalog epoch 和 pending request 检查，验证旧解析结果不能重绑定标签。

```bash
cargo check --all-targets
cargo test --test catalog_reducer
cargo test --test relation_runtime
```

**复核：** 检查范围外不会被误报为对象删除；检查重命名后的对象按实际位置校验；审阅旧 epoch 结果处理。

### Task 4：补齐保存、刷新和异步边界

**文件：**
- Modify: `tests/profile_reducer.rs`, `tests/profile_lifecycle.rs`
- Modify: `tests/relation_runtime.rs`, `tests/relation_tabs.rs`, `tests/catalog_reducer.rs`
- Modify only if a tested reducer gap exists: `src/app.rs`

**步骤：**
1. 扩展 active scope-only save 测试：断言连接 generation 不变、旧 Data/DDL 请求取消、epoch 增加、补全清除、目录命令带新 scope、不发重连命令。
2. 增加完整 App → ProfileSaved → Catalog refresh → Relation preview 生命周期测试。
3. 覆盖 Data/DDL 的范围扩大、缩小、`A -> B -> A`，以及旧成功/旧失败结果迟到。
4. 验证旧目录页、旧身份解析结果不能污染新 epoch。
5. 范围缩小后复用 `OutOfScopeSnapshot`，不关闭标签、不丢弃 dirty edit、不结束事务；重新允许后按 r 恢复。
6. 使用现有 request equality、pending map、epoch 检查；仅在测试证明不足时补 reducer 逻辑，不引入额外 scope revision。

```bash
cargo test --test profile_reducer --test profile_lifecycle --test relation_runtime --test relation_tabs --test catalog_reducer
```

**复核：** 人工审阅状态迁移；确认两种 `r` 都重新发起带新 scope 的请求；确认迟到结果被丢弃。

### Task 5：真实数据库回归

**文件：**
- Modify: `tests/postgres_adapter.rs`, `tests/postgres_relation_mutations.rs`
- Modify: `tests/mysql_adapter.rs`, `tests/sqlserver_adapter.rs`

**步骤：**
1. PostgreSQL 创建唯一临时 schema 和至少 11 行数据，建连时排除它，随后用新 scope 在同一 DatabaseConnection 读取 Data、第二页、DDL、过滤/排序。
2. 切换为排除 scope，验证明确范围错误；重新加入后验证恢复；验证 xmin/列元数据不变。
3. 覆盖 OID 重命名/移动、真实删除与范围外的区别，并确保失败时清理临时对象。
4. MySQL 覆盖 database-as-schema；SQL Server 覆盖 schema 和可用时的新增 database。
5. 未配置环境变量时记录“未执行真实数据库测试”，不得将直接 return 视为通过。

```bash
cargo test --test postgres_adapter visible_objects -- --nocapture
cargo test --test postgres_relation_mutations
cargo test --test mysql_adapter
cargo test --test sqlserver_adapter
```

**复核：** 确认测试确实连接并执行 SQL；不打印任何凭据；检查临时对象清理。

### Task 6：文档与最终验收

**文件：**
- Modify: `docs/architecture.md`
- Modify if needed: `CONTRIBUTING.md`

**步骤：**
1. 文档增加动态 Visible Objects 属于请求、不能缓存于 adapter/连接池的架构规则。
2. 说明范围外和不存在的错误区别，以及历史快照的 scope 归属。
3. 搜索检查所有 `catalog_scope` 引用和三组目标 API 调用方。
4. 运行完整检查：

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

5. 人工操作：仅显示 tools → 勾选 test_schema → 保存 → 打开表 → Data/DDL → 翻页/过滤 → 表格 r → 连接 r；不退出、不重连。
6. 取消并重新勾选 schema，验证范围外状态和恢复路径。

**复核：** 汇总改动文件、每阶段测试结果、真实数据库是否执行、剩余风险；未经明确要求不提交 commit。

## 4. 顺序与验收矩阵

执行顺序严格为：Task 1 → Task 2 → Task 3 → Task 4 → Task 5 → Task 6。每个 Task 完成后必须先运行其命令、检查 diff 和相关调用方，再进入下一项。

| 场景 | 预期 |
| --- | --- |
| 新增 schema 后打开表 | 同一连接立即成功，generation 不变 |
| 表格 r / 连接 r | 都使用最新 scope |
| Data / DDL / 分页 / WHERE / 排序 | 范围一致、数据与元数据正常 |
| 移除 schema | 新读取被明确标记范围外，已有快照不被静默删除 |
| 再次加入 schema | r 后恢复 |
| 旧请求迟到 | 不覆盖最新状态 |
| OID 对象重命名/移动 | 按解析后的实际范围判断 |
| 对象真实删除 | 仍报告不存在 |
| dirty edit / 非 Idle 事务 | 遵循现有约束，不静默丢失 |
