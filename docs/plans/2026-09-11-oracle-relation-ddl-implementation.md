# Oracle Relation DDL 修复与可靠性改进实施计划

**Goal:** 修复 Oracle DDL 页签的实际调用入口，保证输出可信、scope 校验一致、失败提示准确且可主动重试。

**Architecture:** 保留 DatabaseConnection → OracleAdapter → Oracle 原生元数据接口的现有结构，统一带 scope 与不带 scope 的加载实现。第一阶段修复入口与交互；第二阶段完善对象类型、错误语义和元数据完整性。优先复用 RelationLoad、请求身份和 save_after_metadata_load，不为本次修复引入通用重试框架。

**Tech Stack:** Rust 2024 / Rust 1.94、Tokio、oracle 0.6.3、Oracle DBMS_METADATA、现有 TUI 与集成测试。

---

## 范围与执行约定

- 本文件是实施计划；尚未完成任何产品代码修改或 Oracle 实测。
- 当前 `src/app.rs` 存在用户已有修改。执行前检查差异，在其基础上增量修改，禁止覆盖或还原。
- 无新增生产依赖、无连接配置格式迁移。提交和发布需另有明确指令。
- 文中行号是定位提示，以符号名为准。每项先补有意义的行为回归，再实现并执行对应验证。
- Oracle 集成验证使用专用测试 schema 与测试对象，不操作截图中的业务表。
- 测试需要 `LAZYDB_TEST_ORACLE_URL`、`LAZYDB_TEST_ORACLE_USER`、`LAZYDB_TEST_ORACLE_PASSWORD` 和可用 Oracle Client。使用已有安全配置，不将凭据写入文件、命令参数或报告。
- 现有 Oracle 测试可能在环境缺失时提前返回。测试进程返回成功不等于真实 Oracle 已验证；验收报告必须明确是否实际连接和执行了断言。

## 已确认的问题

1. `src/runtime.rs` 的 relation task 调用 `relation_ddl_with_scope()`。
2. `src/db/mod.rs:713` 的 Oracle 分支仍返回硬编码“未实现”；同文件 `relation_ddl()` 已接通 adapter。
3. `src/db/oracle.rs:282` 已有 DDL 实现，但自行构造 scope、对象种类固定为 TABLE、没有与 preview 一致的 service 校验。
4. `src/app.rs` 的 `accept_relation()` 将所有 DDL 错误包装成保存元数据错误。
5. `load_active_relation_with_page()` 允许 Failed 状态在非显式刷新时重发请求。
6. `tests/oracle_adapter.rs` 仅覆盖不带 scope 的 DDL 入口。
7. 原生 DDL 失败遇到 ORA-31603 会生成仅包含列的 CREATE TABLE；类型参数和约束等可能不完整。
8. `RelationDdl.children` 参与 `metadata_fingerprint()` 和保存数据逻辑，不能将空/不完整 children 伪装成完整可保存元数据。

## 交付顺序

| 阶段 | 任务 | 完成条件 |
| --- | --- | --- |
| A：功能修复 | 1–3 | 真正的 UI 入口可获取普通表 DDL；scope 有效；文案和重试正确 |
| B：可信输出 | 4–6 | 表/视图类型正确；原生 DDL 完整；无静默伪造；保存元数据完整性有保障 |
| C：验收 | 7 | 相关自动化检查与真实 Oracle 场景有明确验证记录 |

任务依赖：1 → 2 → 4 → 5 → 6；3 依赖 1 的回归定位，7 依赖全部必做任务。按顺序执行即可。

## Task 1：建立真正入口的回归与基线

**Files:**
- Modify/Test: `tests/oracle_adapter.rs`
- Inspect: `tests/relation_runtime.rs`、`tests/relation_tabs.rs`、`src/runtime.rs`

**步骤：**
1. 查看 `git status --short` 和 `git diff -- src/app.rs`，记录已有差异边界。
2. 复用 Oracle adapter 测试连接辅助逻辑，增加具名用例 `oracle_relation_ddl_with_scope_returns_native_table_ddl`。
3. 使用测试专属表，保留旧 `relation_ddl()` 覆盖，并通过 `relation_ddl_with_scope(&relation, &scope)` 断言 SQL 包含正确限定表名、预期列和原生 provenance。
4. 避免取“第一张可见业务表”作为新用例的唯一 fixture；测试对象用唯一名称，并通过现有测试清理惯例释放。
5. 执行 `cargo test --test oracle_adapter oracle_relation_ddl_with_scope -- --nocapture`。真实 Oracle 环境中预期先失败于占位错误；环境不具备则记录未执行真实回归。
6. 在 relation runtime 现有测试结构中确认 UI 命令携带 scope；有现成注入点时增加运行时路径断言，不为一条分发新建通用 mock 数据库架构。

**验收：** 至少一个实际调用带 scope 入口的测试会在旧代码上失败，并能在修复后验证正确对象的原生 DDL。

## Task 2：统一 Oracle DDL 入口与目标校验

**Files:**
- Modify: `src/db/mod.rs` — `DatabaseConnection::relation_ddl_with_scope`
- Modify: `src/db/oracle.rs` — `OracleAdapter::relation_ddl` 与新增 `relation_ddl_with_scope`
- Test: `src/db/oracle.rs` 内部测试、`tests/oracle_adapter.rs`

**步骤：**
1. 为 Oracle DDL 目标解析抽出小型内部校验函数；参数包含 connection id、configured service、CatalogId 和调用者 scope。
2. 补无数据库单元测试：错误 profile、非 relation 类型、非法 path、错误 service、scope 外 schema；补大小写和特殊字符名称原样保留的正例。
3. 所有数据库 I/O 之前执行上述校验。scope 判断复用现有 CatalogScope / CatalogRequest 校验规则，不另写一套模糊比较。
4. 新增 `OracleAdapter::relation_ddl_with_scope()`，使其把调用者 scope 传入 children 元数据请求。
5. 让不带 scope 的 `relation_ddl()` 按现有兼容语义构造目标 scope，随后委托同一个内部实现，消除重复业务逻辑。
6. 替换统一分发分支为：

```rust
Self::Oracle(adapter) => adapter.relation_ddl_with_scope(relation, scope).await,
```

7. 保持 `driver-oracle` 关闭时明确返回驱动未启用，不返回看似成功的生成 DDL。
8. 执行 `cargo test --lib oracle`、Task 1 的入口测试和 `cargo check --no-default-features`。

**验收：** UI 与直接 API 走同一 DDL 实现；有效 scope 查询成功；非法目标在查询前失败；默认和禁用 Oracle feature 均可编译。

## Task 3：修正错误用途、失败重试与旧响应处理

**Files:**
- Modify: `src/app.rs` — `accept_relation`、`load_active_relation_with_page`、`load_relation_metadata_for_save`
- Inspect/Modify as needed: `src/ui/relation.rs` — 失败状态提示
- Test: `tests/relation_tabs.rs`、`tests/relation_runtime.rs`、`tests/ui_render.rs`，必要时复用 `src/app.rs` 内部测试

**步骤：**
1. 用 `Action::RelationFailed` 驱动现有状态机测试，分别构造普通 DDL 查看和 `save_after_metadata_load = true` 的请求。
2. 在清除保存等待标记之前保存其原值。普通查看使用 `Could not load relation DDL: ...`；只有确实等待保存时使用现有保存元数据文案。
3. 普通浏览失败优先显示页签内错误，不额外堆叠相同 toast；保存失败保留一次必要通知。重复派发同一个旧结果仍应被现有请求身份校验忽略。
4. 收紧 DDL 加载条件：Empty 首次加载；Loading / Ready 不重复加载；Failed 不因普通激活重试；显式刷新可重试。Cancelled 的恢复维持现有合理行为并补断言。
5. 显式保存命令仍可以请求元数据重试；请求进行中只挂接等待保存标记，不发第二个请求。
6. 重连、scope 变化、对象身份失效应复用已有 generation / 失效逻辑；确认这些事件可重新加载，不把旧失败永久缓存。
7. 失败页提示使用现有刷新操作及其动态快捷键说明，不新建重试快捷键系统。
8. 增加测试：失败后切 DATA 再回 DDL不重发；主动刷新只发一个新请求；失败清空保存等待；旧连接/旧 scope/旧 request 不覆盖当前结果；有 previous snapshot 时仍保留旧内容的既有语义。
9. 执行 `cargo test --test relation_tabs`、`cargo test --test relation_runtime`、`cargo test --test ui_render`，以及新增的 app 内部用例。

**验收：** 普通查看不出现“for saving”；同一次请求不产生重复通知；失败可通过明确操作恢复；成功后等待保存流程只继续一次。

## Task 4：正确选择 Oracle 对象类型并验证 CLOB

**Files:**
- Modify: `src/db/oracle.rs` — `relation_ddl`、`native_relation_ddl`
- Test: `src/db/oracle.rs` 内部测试、`tests/oracle_adapter.rs`

**步骤：**
1. 增加 relation kind → Oracle metadata type 的集中映射，当前范围至少覆盖 TABLE 与 VIEW；未知对象明确 Unsupported。
2. 保证 `CatalogEntry.native_kind` 与查询类型一致，不再将 VIEW 标为 TABLE。
3. 原生读取使用绑定参数：

```sql
SELECT DBMS_METADATA.GET_DDL(:1, :2, :3) FROM dual
```

4. 参数顺序固定为 object type、object name、owner。对象名不转大写、不按点拆分，保持 CatalogId 原始含义。
5. 如现有目录已能可靠识别物化视图，接入 MATERIALIZED_VIEW；否则本次对该种类明确不支持，单独规划发现层扩展，不能把物化视图当普通表输出后声称完整支持。
6. 查阅 oracle 0.6.3 对 CLOB → String 的实际支持，用 >32 KiB 且末尾有唯一标记的定义验证现有读取方式；仅在无法完整读取时改用驱动 CLOB 完整读取 API。
7. 保留 `spawn_blocking` 与连接互斥锁，避免阻塞 Tokio worker。默认使用原生格式，不改 SESSION_TRANSFORM。
8. fixture 覆盖普通表、普通视图、带双引号/空格/特殊符号名称、NUMBER(10,2)、VARCHAR2 CHAR 语义、默认值、长文本。
9. 执行 `cargo test --lib oracle` 与 `cargo test --test oracle_adapter oracle_relation_ddl -- --nocapture`。

**验收：** 视图返回真实 CREATE VIEW 定义；名称正确引用；长 DDL 末尾完整；无 VARCHAR2 截断或人工 SUBSTR 截取。

## Task 5：消除 ORA-31603 的静默不完整 DDL 降级

**Files:**
- Modify: `src/db/oracle.rs` — `native_relation_ddl` 及现有列拼接 fallback
- Test: `tests/oracle_adapter.rs` 与 Oracle 内部错误映射测试

**推荐决策：** 本轮采用“原生 DDL 成功，或明确失败”，停用静默生成不完整 CREATE TABLE。比实现一个覆盖 Oracle 所有类型、分区、约束和依赖的重建器更简单可靠。

**步骤：**
1. 增加用例：原生元数据不可见时不返回 `AdapterGenerated` 成功；空/NULL 原生结果不返回空 CREATE TABLE。
2. 将 `native_relation_ddl` 的结果语义从“缺失就可回退”改成“必须是非空原生 DDL，否则返回明确错误”。
3. 用驱动结构化错误信息保留 Oracle 错误码；读取 API 前核实 oracle 0.6.3 支持。不要仅用 ORA-31603 字符串推断权限不足。
4. ORA-31603 提示“对象不存在或当前账号无法读取其原生元数据”，附限定对象名；已确认 ORA-01031 时分类为 Permission；连接和 SQL 错误保持对应类别。
5. 不自动扩大账号权限；不在元数据不可见时发起自动授权或连接切换。
6. 真实库测试至少包含自身 schema 成功、跨 schema 可 SELECT 但 GET_DDL 不可见、对象被删除。特权账号场景仅在专用测试环境具备时验证。
7. 如果未来需要“部分结构预览”，另行设计独立展示状态与显著完整性说明；不能沿用正常原生 DDL Ready 状态冒充完整定义。
8. 执行 Task 4 的 Oracle 测试组及错误状态机测试。

**验收：** 原生失败绝不会静默变成缺少类型长度、约束或视图定义的 CREATE TABLE；错误含真实码及可理解原因。

## Task 6：修复 children 完整性，明确 DDL 与保存的耦合边界

**Files:**
- Modify: `src/db/oracle.rs` — DDL 专用 children 获取与 `oracle_catalog_entries` 相关 SQL/helper
- Inspect: `src/db/catalog.rs` — `RelationDdl` / `CatalogPage`
- Inspect/Test: `src/db/mutation.rs` — `metadata_fingerprint`
- Test: `tests/oracle_adapter.rs`、Oracle 内部元数据测试

**步骤：**
1. 增加 >500 个列/子对象的测试，检查最后一列及主键信息仍存在；允许按测试 Oracle 版本选择可支持的宽表尺寸。
2. DDL/保存所需完整元数据使用 Oracle 内部完整加载 helper；Explorer 的分页查询继续分页。复用解码逻辑，不将 UI page_size 简单调大当作修复。
3. 不直接循环现有 RelationChildren cursor：先核实其查询推进语义；当前 SQL 无 cursor 下推且混合多个种类，直接循环可能重复首批或丢项。
4. 按列 ordinal 保留完整列序；主键/复合键按 position 保序。检查当前用 LISTAGG 再按逗号拆列名的做法，改为逐行读取再分组，以支持名称本身含逗号的列。
5. 完整 metadata 与 relation identity / scope 属于同一次有效请求；任何必需字段读取失败明确报错，不能以空 children 返回 Ready。
6. 评估原生 DDL与 children 的查询耗时和失败耦合，并记录结果。本轮保留现有 `RelationDdl` 数据契约，避免顺手修改所有数据库 adapter 和保存流程。
7. 若产品要求“children 失败也必须显示原生 DDL”，单列后续结构调整：独立 DDL 文本状态与保存元数据状态，保存仅接受完整元数据；需覆盖所有调用方后实施。不能只交换查询顺序或吞掉 children 错误声称完成解耦。
8. 执行 Oracle 元数据测试与 `cargo test --test relation_tabs`、`cargo test --test relation_runtime`。改动共享 fingerprint 时再运行对应数据库 mutation 回归。

**验收：** DDL 所附保存元数据不受 Explorer 单页上限影响；特殊名称和复合主键完整且顺序正确；不完整元数据无法进入保存就绪状态。

## Task 7：最终验证与交付记录

**Files:**
- Review: 本次修改的源码与测试
- Update: 本计划中的执行记录，或项目既有验证记录文档

**自动化检查：**

```bash
cargo fmt --all -- --check
cargo check --no-default-features
cargo test --lib
cargo test --test relation_tabs --test relation_runtime --test ui_render
cargo test --test oracle_adapter -- --nocapture
cargo clippy --all-targets --all-features -- -D warnings
git diff --check
```

上游已存在的 lint/测试失败单独记录，不混入无关修复。范围内检查通过后不无理由重复运行。

**人工验收矩阵：**

| 操作 | 预期 |
| --- | --- |
| 自有表打开 DATA → DDL | 数据可见，DDL 正确，未实现错误消失 |
| 切换 TABLE / VIEW | 生成对应种类的原生 DDL |
| 特殊对象名 | 引用和内容准确，无名称转义错误 |
| 长 DDL 滚动/复制到末尾 | 末尾唯一标记存在，文本完整 |
| 元数据无权限 | 说明不可获取原生定义，不输出伪造表结构 |
| 失败后切页签 | 不堆叠通知，不自动反复请求 |
| 明确刷新 | 只产生一次有效新加载，可从失败恢复 |
| 请求中切连接 / scope | 旧响应不能污染新上下文 |
| 保存等待元数据失败 | 使用保存错误文案，等待标记清除，无自动写入 |
| 非 Oracle relation 页签 | 原有加载、刷新与保存等待行为仍正确 |

**交付报告必须包含：** 修改文件、完成的任务、实际执行的命令、真实 Oracle/Client 版本、实际执行与跳过的场景、剩余限制。DDL 原生输出本身不自动等同于包含全部独立索引、触发器、授权的完整迁移脚本，本次不宣称 schema 导出能力。

## 完成标准

- [ ] 实际 UI 带 scope 入口有回归并成功。
- [ ] profile / service / schema scope 边界验证通过。
- [ ] DDL 查看与保存错误用途准确，失败重试可控。
- [ ] TABLE / VIEW 原生类型正确，长 CLOB 完整。
- [ ] 不再静默生成不完整 CREATE TABLE。
- [ ] 保存元数据完整性不受单页上限或列名分隔符影响。
- [ ] 自动化与真实 Oracle 验证结果有明确记录。
- [ ] 用户原有修改被保留。
