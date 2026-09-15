# PostgreSQL Explorer 会话交接修复实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 修复恢复 PostgreSQL 表格 tab 后 schema 分组永久 Loading，确保目录会话保留、替换和失效时所有在途请求都有明确归宿。

**Architecture:** Explorer 保持独立、稳定的目录会话归属；同一数据库的表格 schema 会话建立或复用不自动接管有效目录会话。将目录绑定与交接集中处理：同数据库替换时恢复并重发在途目标，跨数据库切换时重建目录，无可用会话时结束 Loading。保留现有连接身份和完整请求 key 校验。

**Tech Stack:** Rust 1.94 / edition 2024，App action-command reducer，SessionRegistry，Tokio runtime，现有 Cargo 集成测试。

---

## 0. 事实、范围与交付物

### 已确认的代码路径

行号为编写计划时的位置，实施时按函数名定位。

| 位置 | 行为与风险 |
| --- | --- |
| `src/app.rs:22527` `relation_execution_target` | 从持久化表格描述生成含 schema 的 ExecutionTarget |
| `src/app.rs:22072` `load_active_relation_with_page` | 找不到目标会话时建立第二个连接 |
| `src/app.rs:10864` `ConnectionSucceeded` | 处理连接安装，同时更新 Explorer；11189 无条件覆盖目录会话 |
| `src/app.rs:11106` | 仅 editor target switch 恢复 pending，普通表格恢复没有覆盖 |
| `src/app.rs:15627` 已连接会话复用分支 | 15641 同样无条件覆盖目录会话 |
| `src/app.rs:11493` 附近的连接失效分支 | 自动选择 replacement 并直接更新目录归属，没有统一交接 |
| `src/app.rs:17540` `start_catalog_request_for_connection` | 设置 Loading 和 pending；非 Refresh 会被已有 pending 拦截 |
| `src/app.rs:17957` / `18224` | 成功、失败均拒绝不属于当前目录会话的结果 |
| `src/model/explorer.rs:896` `recover_pending_requests` | 可复用的 pending 恢复原语 |
| `src/runtime.rs:2191` `load_catalog_page` | 已有 30 秒超时；超时事件仍可能被 reducer 的旧身份检查拒绝 |
| `tests/workspace_tabs.rs:1066` | 已有恢复 `test_schema.all_types_test` 的测试，但没有完成目录链路验收 |

现场尚未记录默认 schema 和事件 generation；用可控 reducer 事件顺序复现代码缺陷，再用真实 PostgreSQL 验收现场步骤。

### 预计修改文件

- `src/app.rs`：目录会话决策、交接及连接成功/复用/失效接入。
- `src/model/explorer.rs`：仅在现有恢复原语无法覆盖已验证状态转换时调整，并配套模型测试。
- `tests/workspace_tabs.rs`：恢复表格端到端 reducer 回归。
- `tests/catalog_reducer.rs`：请求交接、旧响应隔离、失败恢复与时序矩阵。
- `src/model/session.rs`：仅当需要补足现有会话查询 API 时调整；优先使用已有接口。

不需要修改 workspace 持久化格式、PostgreSQL 分组 SQL 或扩大超时。计划不要求引入依赖或后台 watchdog。

## 1. 状态约束与决策表

### 必须成立的约束

1. 目录会话必须对应 SessionRegistry 中 Connected 且未 retired 的会话；仅 `get_by_identity().is_some()` 不足以表达有效性。
2. 每个 `Loading { request_id }` 都有匹配 owner、request_id、目录身份和 epoch 的 pending 请求。
3. 同身份绑定是幂等操作：不恢复、不重发、不增加 epoch。
4. 保留 A 时，A 的正常成功/失败仍可消费；B 的表格加载独立推进。
5. 从 A 交接给 B 后，A 的成功/失败均不能修改 B 的状态。
6. pending 清理、目录身份修改、新请求状态安装必须在同一次 reducer 更新中完成；返回命令后才由 runtime 执行。
7. 一个 profile 的会话交接不得清理其他 profile 的目录请求。
8. 列表还有在途请求时，连接展示状态为 Syncing；不能被另一个 tab 的连接成功无条件改为 Online。

### 目录归属决策

| 条件 | 决策 |
| --- | --- |
| 首次没有目录会话 | 绑定有效候选会话，初始化目录请求 |
| 候选与目录身份相同 | 保留；仅按实际未加载状态初始化必要请求 |
| 同 profile、同 database、仅 schema 不同，旧会话仍有效 | 保留旧目录会话，表格/SQL 使用候选会话 |
| 新连接替换了同一个 ExecutionTarget，旧身份已不在 registry 中 | 交接到新身份，恢复并重发在途目标 |
| 原目录会话失效，有同 profile、同 database 的有效候选 | 同数据库交接 |
| 明确切换目录数据库目标 | 新目录上下文；取消旧 pending，清理旧目录，增加 epoch，从 Databases 重新发现 |
| 原会话失效，只剩其他 database 会话 | 不把旧 CatalogId/请求直接重发；若按既有交互切换目录到该库，走完整重建；否则保留可重试状态等待连接 |
| 无有效候选 | 移除目录绑定，恢复/终止 pending，展示 Offline 或可重试状态 |

目录加载与表格加载都必须读取 SessionRegistry 中与 identity 对应的真实 target，不能用当前全局 `self.connection.target` 推测其他会话的数据库。

## Task 1：建立确定性失败回归

**Files:** Modify `tests/workspace_tabs.rs`; reuse `tests/catalog_reducer.rs` 的 page/entry 构造模式。

1. 从 `restored_postgres_relation_prepares_its_schema_session` 提取文件内测试夹具，显式设置 profile 默认 schema 与 `test_schema` 不同。
2. 新增 `restored_postgres_relation_keeps_catalog_loading_across_schema_session`，恢复带 `all_types_test` 的 WorkspaceSnapshot。
3. 通过 `RequestConnect` 捕获会话 A；注入 A 的 ConnectionSucceeded，分别保存初始目录命令和连接 B 的命令。
4. 注入 A 的 Databases 页和含 `test_schema`、`tools` 的 Schemas 页，保存两个 Groups 请求。
5. 注入 B 的 ConnectionSucceeded，再注入 A 的两个合法 Groups 页。不要依赖 sleep、真实网络或随机顺序。
6. 断言两个 schema 均拥有预期 group_summaries，Groups owner 已 Loaded，旧 Groups pending 已删除。注意 Groups 成功会自动预加载 Objects，不能在尚未消费这些命令时断言整个 profile 的 pending 为空。
7. 消费必要的 Objects 页及表格预览成功事件，断言右侧表格完成加载、目录最终无 Loading；命令队列仅模拟本测试关心的连接、目录、预览命令。
8. 执行：

```bash
cargo +1.94.0 test --test workspace_tabs restored_postgres_relation_keeps_catalog_loading_across_schema_session -- --exact --nocapture
```

**预期：** 修复前在目录会话归属或 Groups Loaded 断言失败，证明不是仅检查表格状态的测试。

## Task 2：集中目录会话决策，修复新建及复用路径

**Files:** Modify `src/app.rs`; Test `tests/workspace_tabs.rs`, `tests/catalog_reducer.rs`。

1. 在 `tests/catalog_reducer.rs` 增加同数据库不同 schema 新会话和已有会话复用两个测试；都先让 A 有在途目录请求。
2. 运行新增测试，确认当前直接覆盖目录身份导致失败。
3. 在 `src/app.rs` 增加私有目录会话协调方法，输入候选 identity 及必要的目录切换意图，返回需要执行的 Commands。用明确的私有枚举表达普通会话可用与目录上下文切换，避免多个含糊布尔参数。
4. 方法通过 SessionRegistry 获取候选与旧会话的真实 target/status，按决策表选择保留、首次绑定或交接。优先保持接口局部，不增加全局第二套 session registry。
5. 将 ConnectionSucceeded 与 `request_connection_target_inner` 的复用路径接入协调方法；移除这两处直接覆盖目录身份的代码。
6. ConnectionSucceeded 应从已接受 identity 对应 session 取得 target，避免当 `pending_matches` 为 false 时回退 profile 默认目标而误判 schema/database。
7. 保留 A 时，初始化/续发目录命令使用实际目录 identity A，不再传 B；表格加载仍使用其目标会话 B。
8. 将 `Online/Syncing` 设置放到协调后的状态推导，避免后续赋值覆盖真实 pending 状态。
9. 运行：

```bash
cargo +1.94.0 test --test workspace_tabs restored_postgres_relation
cargo +1.94.0 test --test catalog_reducer catalog_session
```

**预期：** Task 1 回归通过；新会话与复用会话均不抢占同数据库的有效目录归属；重复事件不产生目录重复请求。

## Task 3：实现真正的目录会话交接

**Files:** Modify `src/app.rs`; optionally `src/model/explorer.rs`; Test `tests/catalog_reducer.rs`。

1. 增加测试 `catalog_session_replacement_reissues_pending_targets`：A 有两个 Groups pending，A 被退休/替换，B 是同数据库的 Connected 会话。
2. 增加测试 `catalog_session_replacement_rejects_old_success_and_failure`：重发后依次注入旧成功、旧失败，验证 B 请求仍有效，再消费 B 成功结果。
3. 运行测试确认失败。
4. 在统一交接方法中，在改变目录身份之前调用 `recover_pending_requests()`，收集受影响 CatalogTarget。
5. 同数据库替换增加 catalog epoch，保留有效目录快照，绑定 B；以新 request_id、当前 epoch、`cursor: None`、Refresh 意图重发受影响目标。跨连接不复用旧 cursor。
6. 先检查 epoch 容量再执行状态修改；无法增加 epoch 时必须给出错误并结束旧 Loading，不留下半交接状态。
7. 请求范围有祖先和后代重叠时先合并：Databases 覆盖其后代，Schemas 覆盖该数据库下待加载子级，Groups 覆盖其自动预加载的 Objects。让已存在的逐级自动加载推进，避免父页替换删除刚重发的子请求。RelationChildren 不由 Objects 自动加载，必须在父对象可用后补发；用明确测试验证该边界，再决定是否需要最小的延后目标集合。
8. 对不同数据库采用完整重建：清理旧 pending/load states/catalog，增加 epoch，绑定新目录会话并从 Databases 请求。沿用现有目录清理时的选择/展开回退规则。
9. 复用现有恢复原语；只有测试证明模型方法不足时才修改 `src/model/explorer.rs`。
10. 运行：

```bash
cargo +1.94.0 test --test catalog_reducer catalog_session_replacement
cargo +1.94.0 test --test catalog_reducer newer_request_wins_and_every_wrong_request_dimension_is_ignored -- --exact
```

**预期：** 新 pending 全部属于 B，旧响应不能更改目录；分页刷新保持现有可见快照直到新页成功替换。

## Task 4：连接失效、断开和无候选时闭合 Loading

**Files:** Modify `src/app.rs` 的连接失效/断开处理与目录清理辅助方法；Test `tests/catalog_reducer.rs`。

1. 增加 `catalog_session_invalidation_hands_off_to_same_database`：目录 A 失效，右侧表格 B 仍 Connected，验证目录重新发起而表格不受影响。
2. 增加 `catalog_session_invalidation_without_replacement_ends_loading`：无可用会话时，所有原 pending 和 Loading 被清理/恢复，可重新连接并加载。
3. 增加 `catalog_session_database_switch_rebuilds_catalog`：明确从 db1 到 db2，验证旧 db1 CatalogId 不出现在新请求中。
4. 运行新增测试确认失败后，将失效分支中的直接 remove/insert/entry 接入统一协调方法。主动断开整个 profile 时使用整体清理，不能因还有待退休的会话而自动重新连接。
5. 候选选择限定为相同 profile 和兼容数据库；多个候选采用确定性规则，例如目标优先后 generation 顺序，避免 HashMap 迭代顺序决定归属。
6. 检查活动连接失效及后台目录连接失效两条路径；不能因全局连接已投影到其他 profile 就跳过原目录 profile 的 pending 清理。
7. 请求分发器 `start_catalog_request_for_connection` 仅验证/使用已绑定 identity，移除其冗余目录归属写入，保持单一绑定入口。
8. 所有早返回分支都必须返回交接产生的 Commands，不能协调完状态后丢掉重发命令。
9. 运行：

```bash
cargo +1.94.0 test --test catalog_reducer catalog_session
cargo +1.94.0 test --lib invalidated_active_connection
cargo +1.94.0 test --lib stale_connection
```

**预期：** 有兼容候选可恢复，无候选可重试；不跨 profile 或 database 重发旧目标。

## Task 5：完成时序与错误回归矩阵

**Files:** Modify `tests/catalog_reducer.rs`, `tests/workspace_tabs.rs`。

1. 将核心恢复场景表驱动化：B 分别在 A 的 Databases、Schemas、Groups、Objects 完成前后连接成功。
2. 断言命令和状态不变量，不依赖不同 schema 请求的 HashMap 排序。
3. 覆盖以下场景：

| 场景 | 必须验证 |
| --- | --- |
| A 有效，B 成功后 A 返回成功 | A 结果仍被接受，目录正常加载 |
| A 有效，B 成功后 A 返回 Network 失败 | A owner 进入 Failed 或 Stale，pending 清理 |
| A 有效，B 成功后 A 返回 Permission 失败 | PermissionDenied 或保留快照的 Stale |
| A 被替换，旧结果晚到 | 不影响 B 的 pending 和目录快照 |
| 同一 identity 重复绑定/复用 | 不改变 epoch，不额外重发 |
| 多个恢复 tab，目标 schema 不同 | 目录仍唯一归属有效会话，表格各自使用正确目标 |
| 第二个 profile 切换到前台 | 第一个 profile 的有效目录请求可完成 |
| Continuation 在途时替换 | 从首页刷新，旧 cursor 不跨身份继续使用 |
| Groups 成功触发 Objects 自动预加载 | 最终完成，且没有重复/孤儿 pending |
| 子级刷新与父级刷新同时在途 | 父页替换不会留下消失 owner 的 Loading 或永久延后请求 |

4. 针对超时，在 reducer 测试中注入 runtime 已使用的 Network 失败事件即可验证本次缺陷；仅当实现修改 runtime 超时行为时才新增真实计时测试。
5. 执行受影响测试套件：

```bash
cargo +1.94.0 test --test catalog_reducer --test workspace_tabs --test relation_tabs
```

**预期：** 全部通过。使用固定事件调度即可确定性覆盖竞态，无需循环重跑碰运气。

## Task 6：集成检查、真实环境验收与交付

**Files:** Review 所有实际改动文件；更新本计划末尾执行记录。

1. 检查 diff：目录归属更新集中管理，成功/失败完整 key 校验仍在，表格 schema 目标行为仍符合原测试。
2. 按当前 CI 执行一次最终检查：

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

3. 记录数据库集成测试是否实际配置并执行；未配置数据库时的提前返回不能计为真实 PostgreSQL 验证。
4. 在具备权限的 PostgreSQL 测试连接或用户现场环境手工验收：
   - 默认 schema 与 test_schema 不同；保留 `test_schema.all_types_test` 数据预览 tab。
   - 退出并重启 LazyDB，打开该连接。
   - 展开 `test_schema`、`tools`，确认 Tables/Views/Sequences 等分组显示。
   - 确认右侧预览同时成功。
   - 打开第二个 profile 并切回，确认第一个目录仍可继续展开。
   - 分别执行同数据库 schema 目标切换、重连和明确数据库切换，验证目录归属与数据正确。
   - 关闭全部 tab 再重启，验证原先可用的路径仍可用。
5. 故障验收使用可控测试环境中断目录会话，确认节点退出 Loading，并能通过既有重试/刷新交互恢复。
6. 交付说明列出根因、实际改动路径、自动测试结果、真实环境结果及尚未执行项。

### 建议提交边界

每个提交包含相关修复与通过的回归测试，避免独立提交失败测试：

1. `fix(explorer): preserve catalog sessions during relation restore`（Tasks 1–2）
2. `fix(explorer): reconcile requests when catalog sessions change`（Tasks 3–4）
3. `test(explorer): cover catalog session handoff event ordering`（Task 5）

执行者按实际项目提交要求操作；本计划阶段只创建文档。

## 最终验收标准

- 保留 `all_types_test` tab 重启后，目录分组和表格预览都能完成加载。
- 目录会话稳定，不随同数据库其他 schema 的 tab 会话无条件切换。
- 真正交接后没有引用旧目录身份的 pending 请求，也没有孤儿 Loading。
- 同数据库请求可恢复；跨数据库目录完整重新发现。
- 有效请求成功、失败和超时均会结束对应 Loading；过期结果仍被隔离。
- 现有恢复表格、目录分页、多 profile、断线与错误状态测试通过。

## 执行记录

- 计划阶段：已完成源码定位与实施设计；尚未修改运行时代码或执行新增回归测试。
- 实施阶段：逐任务记录新增测试名称、首次失败原因、修复后结果及设计调整。
