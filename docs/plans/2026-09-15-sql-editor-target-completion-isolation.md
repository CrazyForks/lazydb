# SQL Editor Target Completion Isolation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. 若执行环境没有该技能，则按本文任务顺序实施，每项完成后执行对应验证。

**Goal:** SQL Editor 切换 target 后，补全只使用目标连接的有效元数据；缺失元数据自动加载，异步响应不污染其他 Editor。

**Architecture:** 保留按 profile 共享的 catalog/CompletionIndex，以 Editor 的完整 ExecutionTarget、方言和 session 身份构造补全请求上下文。移除全局补全索引回退，复用现有 catalog 请求去重、分页与失效状态机补齐目标加载，使用 profile 级补全版本控制刷新。

**Tech Stack:** Rust 1.94 / edition 2024、现有 App action/command reducer、Tokio runtime、内置 SQL completion engine、Rust 集成测试。

---

## 0. 实施约束与完成标准

- 当前依据：`src/app.rs`、`src/sql/completion.rs`、`src/model/{tab,workspace}.rs`；行号以计划生成时为准，实施时按符号定位。
- profile 是不可跨越的对象隔离边界；database/schema 决定方言可见性、解析优先级和插入限定名。
- 缓存缺失、空数据库、失败、未绑定 target 都不得回退到其他 profile。
- 连接 READY 与 catalog 已加载分开判断；使用现有 load_states，不增加一套含义重复的 ready 布尔值。
- 自动补全使用已有缓存和关键词；元数据加载异步进行。没有 target 时仍允许纯 SQL 关键字/内置表达式补全。
- 列元数据继续按 RelationChildren 懒加载；catalog_scope 始终生效，不因切换目标扩大配置范围。
- 旧异步事件首先通过现有 connection/catalog_epoch/request_id 校验；仍有效的其他 profile 响应允许更新自己的缓存，但不得更新当前弹窗。
- 每项采用“小型行为回归测试 → 确认失败原因 → 最小实现 → 定向测试”的顺序。测试通过后形成独立提交；实际提交前遵循 @git-commit。
- 不引入固定 sleep 验证竞态；通过 Action 注入及请求 key 控制事件顺序。

## 1. 已确认的问题与入口

| 问题 | 入口 |
|---|---|
| 目标 profile 无索引时回退到全局索引 | `src/app.rs:16519–16526`, `complete_now` |
| 每个 profile 的 catalog 页都覆盖全局索引 | `src/app.rs:18267–18297`, catalog page reducer |
| editor target 切换成功只恢复中断请求，可能没有首次加载 | `src/app.rs:11218–11244`, ConnectionSucceeded 分支 |
| 已连接 session 复用有另一套激活路径 | `src/app.rs:15677–15747`, `request_connection_target_inner` |
| 请求记录用全局 connection，校验优先使用 tab connection | `src/app.rs:16397–16624` |
| 全局 catalog_generation 导致无关 profile 更新触发重算 | `src/app.rs:18351–18364` |
| 无限定名候选未按数据库可见性筛选 | `src/sql/completion.rs:2286–2413` |
| 测试 fixture 仍直接注入全局索引 | `tests/sql_completion.rs:4441–4549` |

## Task 1：建立跨连接回归，并移除补全索引回退（P0）

**Files:**
- Modify: `src/app.rs` — `complete_now`、列元数据消费入口（约 20374、20414 行）。
- Modify: `src/model/workspace.rs` — `ExplorerState` 的索引访问接口。
- Test: `tests/sql_completion.rs`。

**Steps:**
1. 在现有 App completion fixtures 基础上创建 A/B 两个 profile；用不同表名标记归属，并增加同名表不同列的场景。
2. 添加 `app_completion_does_not_fallback_to_another_profile`：A 有索引，B 无索引，Editor target 为 B，显式补全不出现 A 表。
3. 添加 B 的索引存在但为空、未绑定 target、表别名列补全三个对应断言。纯关键词用 `sel` 等实际会产生关键词的输入验证，不要求 `FROM` 上下文必然出现关键词。
4. 运行 `cargo test --test sql_completion app_completion_does_not_fallback_to_another_profile`，预期在旧代码下因出现 A 对象失败。
5. 建立统一的目标 profile 索引访问入口：返回 `Option<&CompletionIndex>`；调用方仅在 None 时使用局部空索引，不访问全局兼容投影。
6. `complete_now` 使用 tab.execution_target.profile_id；针对明确 relation 的列查询使用 relation.profile_id()。所有返回的 relation 依赖必须属于本次补全目标 profile。
7. 将既有 App fixtures 改为注入 `completion_indexes[profile_id]`；保留关键字、schema 优先级、异步列补全的原有断言。
8. 运行 `cargo test --test sql_completion`，预期全部通过。

**Acceptance:** B 无缓存和空缓存都不出现 A 对象；已有 profile 缓存可正常复用。

**Suggested commit:** `fix(completion): isolate catalog lookup by editor target`

## Task 2：补齐冷 target 的元数据加载生命周期（P0）

**Files:**
- Modify: `src/app.rs` — `prepare_active_console_target`、`request_connection_target_inner`、ConnectionSucceeded、catalog page reducer、`complete_now`。
- Modify if needed: `src/model/explorer.rs` — 使用现有 owner/load-state 能力记录目标加载需求。
- Test: `tests/connection_switch.rs`、`tests/catalog_reducer.rs`、`tests/sql_completion.rs`。

**Steps:**
1. 添加冷 target 切换成功测试：断言至少发出必要的 `LoadCatalogPage`，不能只更改连接标题。
2. 添加热 session / 冷 catalog 测试：复用 session 后仍启动 catalog 加载，并保留目标 console 的身份、文本和光标。
3. 添加 single-flight 测试：重复激活/补全只保留同 owner 的一个在途请求。
4. 运行 `cargo test --test connection_switch completion_catalog`，预期新增测试暴露当前空加载分支；测试名统一包含 `completion_catalog`。
5. 实现 `ensure_completion_catalog`：输入明确的 ExecutionTarget；选择能服务其 database 的有效 catalog session；没有可用 session 时不向旧 session 发请求，等待连接成功入口重试。
6. 依据现有 catalog owner 覆盖情况推进：数据库根未发现则加载 Databases；目标数据库存在但 schemas 缺失则加载 Schemas；目标 schema 的 groups/objects 缺失则加载对应节点。已有根列表不代表目标子树已加载。
7. 使用现有 `start_catalog_request_for_connection` 去重与分页；不要对正在加载的 owner 发 Refresh。Loaded 空结果视为完成；Stale 可发起一次刷新；Failed/PermissionDenied 不在每个按键上自动重试，重试沿用明确的刷新/重连入口。
8. 在绑定/激活、连接成功、session 复用及补全缺失时调用同一 ensure 入口。ConnectionSucceeded 继续恢复有效中断请求，再 ensure 缺失目标节点。
9. 对补全发起的加载保留“目标需求”信息以推进后续页；当前目标的 database/schema 优先，显式限定名补全可以请求 scope 内合法的其他命名空间。不要把补全冷加载直接变成所有数据库的全量对象预加载。
10. 验证 `catalog_sessions` 仍按 profile 保存的约束：同 profile 切 database 后，沿用现有 session 替换及 pending recovery 逻辑，禁止将旧数据库会话当作新目标的加载会话。
11. 按顺序注入 Databases/Schemas/Groups/Objects 响应；SQL 不变，最终能得到新目标对象候选。覆盖分组为空、分页、失败、手动重试。
12. 运行 `cargo test --test connection_switch --test catalog_reducer --test sql_completion`。

**Acceptance:** 新连接和已连接 session 两条路径都能补齐目标 catalog，重复输入不导致请求风暴；合法目标被 catalog_scope 排除时正常返回无对象候选。

**Suggested commit:** `fix(completion): ensure catalog loading for rebound editor targets`

## Task 3：统一补全请求身份与切换失效规则（P1）

**Files:**
- Modify: `src/sql/completion.rs` — `CompletionScheduleKey`。
- Modify: `src/model/tab.rs` — `CompletionRequest`。
- Modify: `src/app.rs` — `completion_key`、`set_completion_request`、`completion_request_is_current`、target 更新入口、`accept_completion`。
- Modify as required by ownership changes: `src/action.rs`、`src/runtime.rs`。
- Test: `tests/sql_completion.rs`、`tests/connection_switch.rs`。

**Steps:**
1. 添加 A→B 后 A 的 CompletionDue 晚到测试、schema 改变但文本/光标不变测试、后台连接与 tab.connection 不同测试。
2. 使用一个不可变请求上下文记录 console_id、完整 target、dialect、execution connection、document revision、cursor；调度 key 与 CompletionRequest 使用同一生成函数。
3. session 优先取 tab.execution_connection 并验证归属；需要回退时从 SessionRegistry 查找完整 target 的匹配 session，不能使用未验证的全局 active_identity。
4. 无连接也能生成关键词请求上下文；catalog 加载必须单独验证可用 session。
5. 将 target 加入 schedule key 后移除不再成立的 Copy 派生，调整 Action/runtime 的 move/clone，运行 `cargo check --all-targets` 查全消费点。
6. 收敛 target 真正变更后的补全清理：清除 popup/request；不同切换入口，包括连接成功后写入 target 的路径，使用相同失效规则。切换失败且 target 未提交时保持现有失败语义。
7. 处理 CompletionDue、catalog 触发刷新及接受候选前验证身份；过期候选不能插入。显式补全和自动补全共用规则。
8. 加入 A→B→A、tab 切换、连接重连 generation 改变的回归。
9. 运行 `cargo test --test sql_completion --test connection_switch`。

**Acceptance:** 文本没变化也能因 target/schema/dialect/session 变化正确淘汰旧请求；tab 与全局连接不一致时不会丢失正确刷新。

**Suggested commit:** `fix(completion): bind request lifecycle to full editor context`

## Task 4：按 profile 管理补全版本，隔离异步刷新（P1）

**Files:**
- Modify: `src/model/workspace.rs` — profile 级 completion generation。
- Modify: `src/app.rs` — catalog 接收、drop/mutation/refresh、profile 失效和删除路径。
- Modify: `src/model/tab.rs`、`src/sql/completion.rs` — 补全版本字段语义。
- Test: `tests/catalog_reducer.rs`、`tests/sql_completion.rs`、`tests/catalog_drop.rs`。

**Steps:**
1. 添加 A 的有效页面晚到且 B 弹窗打开的测试：A 缓存更新，B 请求 key/选择项和候选不变。
2. 给每个 profile 维护补全内容版本；保留全局 catalog_generation 给已有 diagnostics 等消费者，补全改用目标 profile 版本。
3. 将补全版本更新集中到索引内容变更/失效入口；catalog 页、对象删除、重命名、scope 更新、重连失效都覆盖，不能只在页面成功时更新。
4. 区分“调度尚未执行”和“已显示候选等待元数据”：旧调度事件必须校验上下文与版本；相关新页面到达后，若 Editor 身份仍匹配，允许用新版本重新计算并更新请求记录。
5. 特别覆盖 B 页面先于 B 首次 debounce 到达：旧定时 key 失效后必须已有立即重算或新的调度，不能静默丢失补全。
6. 删除/清理 profile 的索引时同步处理版本；结合 session generation/catalog epoch 防止版本重置造成旧请求重新有效。
7. 运行 `cargo test --test catalog_reducer --test sql_completion --test catalog_drop`。

**Acceptance:** 无关 profile 更新不重算当前补全；相关目标页面能刷新相同 revision/cursor 的弹窗；删除对象立即退出候选。

**Suggested commit:** `refactor(completion): scope catalog revisions to connection profiles`

## Task 5：统一方言可见性与限定名解析（P1）

**Files:**
- Modify: `src/sql/completion.rs` — `qualified_candidate_indices`、`catalog_entry_navigable`、DDL 候选分支、relation 解析/插入逻辑。
- Test: `tests/sql_completion.rs`、`tests/lsp_completion.rs`。

**Steps:**
1. 添加同 profile、两个 database、同名 schema/table 的表驱动测试；覆盖无前缀、schema.、database.、alias. 与 DDL 对象上下文。
2. PostgreSQL 普通关系/列候选限制当前 database，当前数据库内其他可用 schema 仍可补全；区分数据库对象管理 DDL 与普通关系访问，避免误删 DROP DATABASE 等合法数据库候选。
3. MySQL/MariaDB 与 SQL Server 保留合法跨库限定访问，当前命名空间优先，生成合法插入文本。
4. SQLite 保留 main/temp/attached schema 语义；Oracle 按已有 owner/schema 语义处理，不直接套用 PostgreSQL 数据库规则。
5. 将可见性规则应用于无限定名、限定名找不到父节点时的 fallback、DDL 与 relation dependency 解析，避免仅过滤最终显示而仍加载错误列依赖。
6. 保留 LSP 共用 CompletionIndex/CompletionContext 接口的合理兼容性；没有 database 上下文的调用遵循原有通用语义。
7. 运行 `cargo test --test sql_completion --test lsp_completion --test catalog_scope`。

**Acceptance:** 跨 profile 永不混入；同 profile 下的跨库/跨 schema 行为符合方言，不以简单 database/schema 全等替代 SQL 解析规则。

**Suggested commit:** `fix(sql): apply namespace visibility consistently in completion`

## Task 6：删除全局兼容索引并优化热路径（P2）

**Files:**
- Modify: `src/model/workspace.rs` — 删除 `completion_index` 及兼容投影注释。
- Modify: `src/app.rs` — `complete_now`、catalog 更新、索引清理及列查询。
- Modify: `tests/sql_completion.rs` 及编译器列出的旧索引 fixture 使用点。

**Steps:**
1. 列出 `completion_index` 的全部读写与测试 fixture，确认所有读取均已按 profile 选择。
2. 删除全局字段及其维护逻辑；catalog 页只收集一次 entries，并重建所属 profile 的一份索引。
3. 将 complete_now 分为只读计算阶段和可变更新阶段：在借用索引期间产生 owned dependencies/candidates，借用结束后派发加载请求、更新 popup/request。
4. 删除整份 CompletionIndex 的按键级 clone；默认用借用解决，不预先引入 Arc 或增量索引复杂度。
5. 检查 profile 删除、catalog scope 修改、对象 drop/rename 的缓存一致性。
6. 执行 `cargo check --all-targets` 与 `cargo test --test sql_completion --test catalog_reducer --test catalog_drop`。
7. 用固定的较大合成 catalog 做一次实施前后相同操作的性能采样，记录对象数、补全输入序列、运行配置和时延/分配变化；不以不稳定的墙钟阈值编写 CI 单测。

**Acceptance:** 编译期不存在全局兼容索引消费点；单次输入不深拷贝索引；候选结果与前序正确性测试一致。

**Suggested commit:** `perf(completion): remove global index projection and hot-path clones`

## Task 7：端到端验收与项目检查

**Files:**
- Test: `tests/connection_switch.rs` — 利用已有临时 SQLite 文件、runtime 和 catalog drain helper。
- Update: 本计划，记录实际检查结果、性能采样和剩余环境相关验证。

**Steps:**
1. 使用两个临时 SQLite 文件分别创建唯一 marker 表及同名不同列的表，以真实 runtime 验证 A 加载→Editor 切 B→B 冷加载→表/列补全，不依赖外部服务。
2. 确认真实运行路径覆盖新建 session 和复用 session，Editor 文本/光标/身份符合既有切换语义。
3. 汇总运行定向套件：

```sh
cargo test --test sql_completion --test connection_switch --test catalog_reducer --test catalog_scope --test catalog_drop --test lsp_completion --test lsp_catalog
```

预期：全部通过。遇到既有测试对命令列表的严格匹配时，明确新增命令的归属/数量，不将断言弱化为任意命令均可。

4. 执行与 `.github/workflows/ci.yml` 一致的检查：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

预期：格式检查通过、无 clippy warning、测试通过。外部数据库集成测试按 CI 现有服务与环境变量运行；记录实际执行和被跳过的测试，不把跳过视为已验证。

5. 手工复现截图场景：A 已加载→当前 Editor 切到 B→立即输入 `select * from f`→等待元数据→切回 A；期间操作第三个连接的 Explorer。预期候选始终符合 Editor target，B 未加载时没有 A/C 对象，B 加载后自动显示 B 对象。
6. 手工检查失败/重试、已打开弹窗时切 target、相同文本多 tab、同连接跨 schema、目标无表等边界。

**Suggested commit:** `test(completion): cover target switching through runtime`

## 交付顺序与审查检查点

1. **M1：Task 1–2** — 目标隔离和冷加载一起交付，避免仅隐藏错误候选却留下永久空 catalog。
2. **M2：Task 3–5** — 完整请求身份、异步刷新隔离与方言规则。
3. **M3：Task 6–7** — 删除兼容结构、性能采样和完整验收。

所有任务共享 App/catalog/completion 状态，按依赖顺序实施；每个里程碑检查“目标归属、加载完成、异步失效”三个行为是否同时成立。

## 最终验收矩阵

| 场景 | 必须满足 |
|---|---|
| A 有缓存，B 无缓存 | B 无 A 对象，且发起 B 的必要加载 |
| B 加载完成，SQL/光标未动 | B 候选自动更新 |
| B 数据先于 debounce 到达 | 不丢失补全刷新 |
| A 响应晚到 | 仅更新有效的 A 缓存 |
| A/B 同名表不同列 | alias/列补全归属正确 |
| 同 profile 切 database/schema | 方言可见性及插入限定名正确 |
| B 空结果/失败/权限不足 | 不跨连接回退，不自动无限重试 |
| B session 已连接但 catalog 冷 | 复用 session 并启动 catalog 加载 |
| target 变化而 revision/cursor 不变 | 旧请求/候选失效 |
| 对象删除/重命名/scope 修改 | 索引及版本同步失效/更新 |
| 未绑定 target | 纯 SQL 补全可用，无数据库对象 |
| 大 catalog 连续输入 | 无整份索引深拷贝，无重复加载风暴 |
