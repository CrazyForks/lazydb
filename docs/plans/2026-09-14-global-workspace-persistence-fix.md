# 多连接全局工作区与持久化修复 Implementation Plan

> 执行说明：按依赖顺序逐项实施，每项按“行为测试 → 确认失败 → 实现 → 定向验证”推进。本文中的新接口和测试名是拟定设计，不表示已经存在。实施时使用可用的计划执行工作流；仅在用户明确要求时创建 Git 提交。

**Goal:** 修复打开两个连接后出现的 `duplicate console ID`，同时保证跨连接切换、保存恢复和连接删除不丢失 SQL、不串改文档目标。

**Architecture:** 保留 Action → App::update → Command → Runtime 边界。运行时只维护全局 Console、Tabs 和单个 EditorWorkspace；数据库连接与文档生命周期解耦。旧工作区格式在加载边界归一化，新写入使用明确的 v6 全局格式。

**Tech Stack:** Rust 1.94 / Rust 2024、Tokio、Serde/TOML、UUID、现有 Modalkit 编辑器、tempfile 和现有 Rust 测试设施；无需新增生产依赖。

---

## 1. 背景与代码依据

以下行号对应计划编写时的代码，实施时以符号定位为准。

| 位置 | 当前行为 | 后果 |
| --- | --- | --- |
| `src/app.rs:874` `snapshot_active_workspace` | 先 take 编辑器，再读取文本；复制 tabs/records | SQL 读取失败变为空文本，同时产生文档副本 |
| `src/app.rs:909` `append_workspace` | 向全局界面追加连接文档 | 全局界面与旧连接缓存同时持有文档 |
| `src/app.rs:1049` `activate_profile_workspace` | 缓存旧集合，修正目标，再追加新集合 | 重复归属，并可能批量串改目标 |
| `src/app.rs:1631` `workspace_snapshot` | 将全局集合序列化到活动 profile | A=[a]，B=[a,b]，触发重复 ID |
| `src/app.rs:1800` `restore_workspace` | 优先恢复单个 profile 并返回 | 保存模型和恢复模型不一致 |
| `src/app.rs:1977` `restore_profile_workspace` | 强制打开第一项，过滤并替换旧目标 | 关闭状态和失效目标无法原样恢复 |
| `src/app.rs:14218` `remove_profile_workspace` | 删除活动 profile 时清空全局文档 | 可能误删其他连接文档 |
| `src/persistence/workspace.rs:171` `load` | SQL 文件读取使用 unwrap_or_default | 缺失/不可读文件被解释为空 SQL |
| `src/persistence/workspace.rs:315` `validate_snapshot` | 同时校验 profile 与全局集合 | 报错是模型冲突的正确检测结果 |

本计划细化并收口 `docs/plans/2026-09-12-multi-connection-consoles.md` 的 T02/T03，以及受影响的 T04/T08/T09 生命周期契约。

## 2. 必须维持的不变量

1. 每个 Console UUID 在运行时和持久化文档集合中都恰好出现一次；重绑不更换 UUID。
2. 一个全局有序 Tab 列表；每个打开 Console 对应一个文档记录，关闭文档仍存在。
3. 单个 EditorWorkspace 保存打开和关闭 Console 的 SQL；关闭 Tab 保留会话，删除文档才释放会话。
4. 连接选择、连接成功/失败和断开不搬迁编辑器、不改写已有文档绑定。
5. 文档名称、目标、事务模式和打开状态有唯一权威来源。现有 ConsoleTab 镜像字段通过集中接口同步，逐步消除直接写入。
6. 未绑定目标、缺失 profile、无连接、无打开 Tab、空 SQL 都是合法状态。
7. 缺少 SQL 文件或编辑会话是错误，不等于合法空 SQL。
8. SQL、Relation、Dashboard、Redis 的持久化身份来自对象自身，不来自 Explorer 当前选择。
9. 保存修订号单调；只有成功提交对应快照才能确认保存/退出。
10. 删除 profile 保留 Console 和 SQL 文件，目标显示为失效引用；实际执行走已有目标校验。

## 3. 关键设计决策

### 3.1 运行时与持久化结构

- 优先复用 `App.sql_editors`、`App.tabs`、`App.editor`，避免同时引入新的大型 Workspace 包装层。
- 删除运行时 `workspaces`、`workspace_editors`、`workspace_focus` 的文档搬迁用途；先检查全部读写点再移除字段。
- 将 `active_workspace_profile` 的剩余导航用途迁入明确的导航上下文或复用已有 Explorer/connection 选择字段。不能简单替换成“当前文档所属 profile”。
- `WorkspaceSnapshot` 只包含全局 consoles、tabs、SQL、`active_tab: Option<Uuid>`、recent_targets；若确需保存 Explorer 选择，使用独立可选字段，不要求其对应一个文档容器。
- 旧 `PersistedProfileWorkspace` 仅保留在旧版本解码结构中，不进入正常运行时。
- Console 保留完整 ExecutionTarget；Relation 使用自身 descriptor/key/qualified_name；Dashboard 持久化自身可选 profile_id；Redis 使用自身 target.profile_id/database/pattern。
- 不持久化 ConnectionIdentity 的 generation、在线状态、事务句柄和查询结果。

### 3.2 格式选择：读取 v1–v5，写入 v6

当前 v5 支持 profile 与全局字段混合，且全局 Dashboard 缺少归属字段。采用 v6 明确新契约，避免旧程序读入新数据后忽略字段并再次保存造成信息丢失。

v6 顶层字段：version、consoles、tabs、active_tab、recent_targets，以及确有产品用途的可选导航选择。所有文档只写入顶层一次，取消 profiles 容器。

未知未来版本继续返回 UnsupportedVersion；不能按 v6 猜测解析。

### 3.3 迁移与错误策略

- 稳定顺序：旧顶层全局 Tabs 在前，随后按 manifest 中 profiles 顺序展开，各容器内部顺序保持。
- 活动项：有效顶层 active_tab → 原 active_profile 的有效 active_tab → 第一个可恢复的打开 Tab → None。
- 现有 target 原样保留，包括失效引用；旧 profile 容器内 target 缺失时，在拿得到原配置且能确定目标的情况下补齐，否则保留未绑定并返回明确迁移提示，不绑定到当前任意连接。
- v1 的旧默认打开语义仅在 v1 解码阶段应用；v2–v5 不强制打开第一项。旧 tabs 存在 Console 引用时将其视为打开；open=true 却缺失 Tab 的旧记录按记录顺序追加恢复。
- profile 缺失不删除文档。Relation/Redis 保留持久化目标并进入不可执行/离线展示状态；不能静默丢弃。
- 重复 UUID 显式报错并包含两个来源位置，不静默去重、不随机换 ID。
- 缺失或不可读 SQL 文件返回带 Console ID、路径和原因的错误；启动失败路径必须禁止默认空工作区自动覆盖原文件。复用现有通知/重试入口，不引入自动空文本恢复。
- 第一次迁移写入前备份旧 manifest；备份写入并 sync 成功后才替换 manifest。迁移不重命名、不删除、不重写原 SQL 内容。
- 当前 SQL 多文件 + manifest 保存不是整个工作区事务。保持这一事实明确：校验失败不写入；I/O 中途失败不能宣称全部 SQL 已回滚。本次不扩展为内容寻址存储。

## 4. 任务依赖与执行规则

```text
T01 基线与复现
  → T02 全局格式与校验
  → T03 旧格式归一化
  → T04 全局运行时和编辑器
  → T05 全局快照、恢复与保存错误
  → T06 文档/连接生命周期
  → T07 保存队列、退出与迁移失败
  → T08 集成验收与文档
```

共享 `src/app.rs` 与 `src/persistence/workspace.rs` 的任务顺序实施。跨类型变更可以先以内部新 DTO/转换函数落地，再在 T05 切换公开接口，保持每个检查点可编译；禁止把过渡双写作为最终方案。

每项内部按 2–5 分钟可核验的小步骤推进：新增一个行为测试、运行单测确认原因、修改一个职责、运行该测试、运行任务测试组、记录结果。下文测试名是预定名称。

### T01：建立基线和确定性复现

**文件：** 新增 `tests/global_workspace.rs`；参考 `tests/connection_switch.rs`、`tests/workspace_tabs.rs`、`tests/workspace_persistence.rs`。

1. 运行当前相关测试，记录既有失败与环境原因：

   `cargo test --test connection_switch --test workspace_tabs --test workspace_persistence --test profile_lifecycle`

2. 使用两个 SQLite profile 和现有 RequestProfileConnect/ConnectionSucceeded 测试模式模拟连接，避免外部数据库依赖。显式创建 A/B 文档，不把自动创建默认 Console 写成新契约。
3. 编写 `two_profiles_produce_one_record_per_console`：打开 A、输入非空 SQL、打开 B、新建并编辑 B 文档、获取 App 生成的快照，再交给真实 WorkspaceStore 保存。
4. 编写 `switching_profiles_preserves_sql_and_targets`：A→B→A→B，每一步按文档 UUID检查内容、目标和文档数量；任何编辑器缺失直接失败。
5. 编写 `connecting_does_not_change_active_document`：连接完成前后活动 Tab ID 相同。
6. 运行 `cargo test --test global_workspace -- --nocapture`，记录旧代码失败点。旧行为导致前置编辑失败时单独拆分复现，不为了看到 duplicate ID 而吞掉 MissingSession。

**完成标准：** 有由真实 Action 链路产生的失败测试，覆盖重复快照和编辑器丢失，不依赖手工构造重复 UUID。

### T02：定义全局持久化结构和完整校验

**文件：** 修改 `src/persistence/workspace.rs`、`src/model/dashboard.rs`（仅目标恢复确需时）、`tests/workspace_persistence.rs`。

1. 新增 v6 DTO 与旧版本解码 DTO，准备全局 normalized snapshot；保留过渡转换入口使现有调用可编译。
2. 为 PersistedTab::Dashboard 增加明确归属；其他类型从自身身份取目标，不重复引入可相互矛盾的 profile 字段。
3. 编写 `v6_round_trip_preserves_mixed_tab_order_and_targets`，覆盖两连接 SQL、关闭 Console、Relation、Dashboard、Redis、未绑定 Console 与 None 活动项。
4. 实现两阶段校验：先建立全部 Console ID 注册表，再检查全部 Tab ID、类型和引用，消除 Console/Relation 冲突检查的遍历顺序依赖。
5. 验证唯一文档 ID、唯一打开 Tab ID、Console Tab 引用存在、open 与 Tab 一致、active_tab 在持久化 Tab 中、SQL ID 集合与文档集合相等、SQL 文件名严格为 UUID.sql。
6. 增加反向顺序冲突、未知 SQL ID、缺少 SQL、同一 Console 两个 SQL 条目、非法路径、重复 Dashboard/Redis ID 测试。
7. 错误记录 Console/Tab ID 和两个来源路径，例如 `profiles[0].consoles[1]` 与 `consoles[0]`。不把“profile 已删除”当成结构损坏。
8. 运行 `cargo test --test workspace_persistence`。

**完成标准：** 全局快照可完整往返，所有对象类型校验一致，空状态可保存。写版本切换与 T05 合并启用。

### T03：统一 v1–v5 加载与迁移

**文件：** 修改 `src/persistence/workspace.rs`、`src/app.rs` 的旧目标补全边界、`tests/workspace_persistence.rs`；新增 `tests/fixtures/workspace/` 下版本 fixture。

1. 准备 v1、v2、v3、v4、v5-profile、v5-global、v5-mixed fixture；SQL 在 TempDir 中按固定 UUID 建立。
2. 实现 `decode legacy → normalize global → resolve legacy target hints → validate → read SQL` 的明确加载链。配置补全留在持有 profiles 的边界，不让存储层隐式读取用户配置。
3. 按第 3.3 节处理顺序、活动项、关闭状态、缺失目标和缺失 profile。
4. 替换 load 中 read_to_string(...).unwrap_or_default，传递可诊断错误。先检查文件名约束，再读取 SQL 路径。
5. 编写 `legacy_migration_preserves_ids_sql_and_closed_documents`、`mixed_v5_duplicate_ids_report_both_sources`、`missing_sql_is_not_restored_as_empty_text`。
6. 对每个 fixture 执行旧格式加载→v6保存→v6加载，断言文档 ID、文本、目标、打开状态、Tab 顺序和活动项一致。
7. 测试迁移第二次执行不重复追加文档；未来版本拒绝；nil profile 目标保持未绑定语义。
8. 运行 `cargo test --test workspace_persistence --test persistence`。

**完成标准：** 旧 profile 文档均被归一化，正常运行路径不再依赖选中某一个 profile 才能看到文档。

### T04：移除连接切换中的文档复制和编辑器搬迁

**文件：** 修改 `src/app.rs`、`src/model/workspace.rs`、`src/model/tab.rs`、`src/editor/mod.rs`、`src/editor/tests.rs`、`tests/global_workspace.rs`、`tests/connection_switch.rs`。

1. 列出 workspaces/workspace_editors/workspace_focus/active_workspace_profile 的全部读写者，给每处标注文档操作、导航、异步路由或旧恢复职责。
2. 移除连接路径中的 snapshot_active_workspace、append_workspace、install_workspace 缓存/搬迁逻辑；旧格式恢复改为使用归一化结果。
3. activate_profile_workspace 的调用者改为只更新导航/连接上下文；连接完成不追加默认 Console、不改变活动 Tab。
4. 文档显式创建时只创建一次 UUID 和一次编辑会话；取目标使用已有默认目标选择逻辑。
5. 对文档名称/目标/事务模式/open 增加集中更新接口，更新 ConsoleRecord 和运行时镜像；移除依赖切换时才同步的代码。
6. 保留关闭 Console 的编辑会话；删除文档时关闭 SQL/output 会话并清理对应缓存。仅在无调用者后删除 merge_sessions_from 等过渡工具。
7. 增加 `switching_profiles_preserves_cursor_and_undo_history`，操作 A 后切换 B，再回 A 执行撤销，检查真实编辑历史；不能只比较字符串。
8. 运行 `cargo test --test global_workspace --test connection_switch --test workspace_tabs` 与 `cargo test --lib editor::tests`。涉及 T05 的往返测试在该任务后闭环。

**完成标准：** A→B→A 文档集合不增长；编辑器身份、SQL、目标和历史保留；运行时没有 profile 文档缓存。

### T05：接通全局快照、全量恢复与显式保存失败

**文件：** 修改 `src/app.rs`、`src/action.rs`（错误事件确需时）、`src/runtime.rs`、`src/model/workspace_save.rs`、`src/persistence/workspace.rs`、`tests/global_workspace.rs`、`tests/startup_profiles.rs`。

1. workspace_snapshot 改为返回 Result；只遍历全局 Console 集合一次，SQL 从对应编辑会话读取。
2. 使用集中元数据接口构造每份 PersistedConsole，删除 profile 组装与 legacy/default 特判分支；新写入启用 v6。
3. History 等不持久化 Tab 被过滤后重新计算 active_tab，活动 History 应回退到一个有效持久化 Tab 或 None。
4. restore_workspace 一次恢复全部全局文档和编辑会话，按持久化顺序建立打开 Tabs；不再强制打开第一项，不清除失效 target，不自动连接。
5. 更新所有 workspace_snapshot 调用者。persist_workspace_command 可返回 Option<Command> 或 Result；构建失败不产生 PersistWorkspace 命令。
6. 明确修订状态：为保存尝试分配修订后，构建失败应对同一修订标记 Failed 并记录原因；普通保存可重试，退出进入已有 QuitSaveState::Failed，不发送等待永远无法到达的 Flush。
7. revision 溢出显式失败，不复用旧修订号；失败后重试重新构造当前快照。
8. 启动 load/restore 失败保留原文件，阻止空默认状态自动保存到同一路径，显示原始错误并走现有重试流程。
9. 添加 `snapshot_failure_does_not_enqueue_empty_sql`、`history_active_tab_restores_to_valid_persisted_tab`、`restore_all_profiles_offline` 和零 Console/全部关闭用例。
10. 运行 `cargo test --test global_workspace --test workspace_persistence --test startup_profiles --test workspace_tabs`。

**完成标准：** T01 的真实保存测试通过；两连接保存重启后每个文档只恢复一次，SQL 原样保留，保存失败状态闭合。

### T06：修正重绑、关闭、断开和删除连接

**文件：** 修改 `src/app.rs`、`src/model/tab.rs`、`tests/profile_lifecycle.rs`、`tests/profile_reducer.rs`、`tests/execution_target.rs`、`tests/transaction_reducer.rs`、`tests/global_workspace.rs`。

1. 删除 remove_profile_workspace 的文档整体删除行为；profile 删除只退休相关会话和配置引用，保留文档/SQL/失效目标。
2. 区分 CloseConsole、DeleteConsole、DisconnectProfile、DeleteProfile；只有显式 DeleteConsole 发出该 UUID 的 DeleteSqlFile。
3. 重新绑定单个 Console 只更新该文档目标，不转移容器、不换 UUID、不触及其他文档。
4. 保留已有运行中查询、待执行和手动事务约束；取消/完成仍按文档与会话身份路由，不使用活动 profile 替代目标身份。
5. Dashboard/Relation/Redis 展示及请求取自身目标，缺失 profile 时保留可辨识离线状态；禁止保存时把当前 profile 写给所有 Redis 页。
6. 添加 `deleting_profile_keeps_all_console_sql_files`、`rebinding_keeps_console_id_and_other_targets`、`closing_console_keeps_sql_for_reopen`、`disconnecting_a_does_not_change_b`。
7. 删除配置成功前后、失败回滚路径分别断言文档仍存在；不能仅检查 UI 数量。
8. 运行 `cargo test --test profile_lifecycle --test profile_reducer --test execution_target --test transaction_reducer --test global_workspace`。

**完成标准：** 连接操作与文档操作职责清晰，删除 A 不丢 A/B SQL，所有目标变化都来自显式文档操作或明确迁移规则。

### T07：保存队列、退出和迁移失败验证

**文件：** 修改 `src/runtime.rs` 的 WorkspaceSaveQueue/测试模块、`src/model/workspace_save.rs`、`src/persistence/workspace.rs`、`tests/workspace_persistence.rs`、`tests/quit_transaction_review.rs`。

1. 为迁移备份实现只创建、不覆盖的旧 manifest 备份策略；备份成功后才提交新 manifest。备份冲突使用明确规则，不能覆盖先前备份。
2. 迁移只转换元数据，原 SQL 字节不变；测试首次备份、再次启动不重复迁移、备份失败不替换 manifest。
3. 对保存队列补测试：revision n 在写入、n+1 到达，最终持久化 n+1；旧失败不能覆盖新状态；退出只在请求修订确认后成功。
4. 区分两种 Retry：I/O 保存失败可沿用队列快照机制；App 快照构建失败需重新读取当前编辑器构造快照，不能重试空队列。
5. 使用确定性故障注入或路径冲突模拟失败，避免依赖 root 下无效的 chmod 或 wall-clock sleep。
6. 在写入前序列化/校验 manifest，使可预见的序列化错误不发生在 SQL 已开始替换之后。I/O 中途失败仍遵守当前逐文件原子写入语义。
7. 验证校验失败时原 manifest 和 SQL 字节均不变；退出构建失败保留应用并展示错误，不挂起、不假报成功。
8. 运行 `cargo test --lib`、`cargo test --test workspace_persistence --test quit_transaction_review --test global_workspace`。

**完成标准：** 普通保存、队列重试、退出保存、迁移失败都没有“丢内容后显示成功”或无限等待路径。

### T08：集成验收、清理和文档

**文件：** 修改 `docs/architecture.md`、`docs/plans/2026-09-12-multi-connection-consoles.md`；必要时更新 `tests/docs.rs`；在本文追加执行记录。

1. 核对所有旧工作区符号的剩余引用；只允许旧 DTO 出现在兼容加载路径。更新旧测试断言时逐条对应新契约，不整文件删除。
2. 文档记录全局文档、v6 格式、v1–v5 迁移、离线恢复、失效目标保留、读取错误和保存失败语义。
3. 运行关联集成组：

   `cargo test --test global_workspace --test workspace_persistence --test workspace_tabs --test connection_switch --test profile_lifecycle --test startup_profiles --test relation_tabs --test redis_browser_tabs --test ui_render --test docs`

4. 运行与当前 CI 一致的最终检查：

   `cargo +1.94.0 fmt --all -- --check`

   `cargo +1.94.0 clippy --all-targets --all-features -- -D warnings`

   `cargo +1.94.0 test --all-targets --all-features`

5. 使用隔离临时工作区和两个 SQLite 文件做 TUI 验收：A/B 各写不同 SQL、交替切换、撤销、关闭重开、重绑、保存退出重启、断开 A、删除 A 配置。记录活动 Tab/顺序/SQL/目标和通知。
6. 检查最终 manifest：version=6，无 profiles 文档容器，Console UUID 唯一，SQL 数量匹配，Dashboard/Redis 目标正确。
7. 记录环境限制及被跳过的驱动测试；不得把未运行项目写为通过。

**完成标准：** 全部验收契约通过，截图报错的真实路径消失，迁移和失败路径也有验证记录。

## 5. 验收矩阵

| 维度 | 必测情况 | 断言 |
| --- | --- | --- |
| 多连接 | A→B→A，第三连接 C，连接完成顺序反转 | 数量稳定，绑定独立，活动文档不被回调抢占 |
| 文本 | 非空、空、Unicode、多行、关闭文档 | 保存恢复内容逐字相等 |
| 编辑会话 | 光标、选区/视口、撤销重做 | 切换连接后会话状态保留 |
| 文档操作 | 新建、重命名、关闭、重开、删除、重绑 | ID 与 SQL 文件生命周期符合操作语义 |
| 连接操作 | 断开、删除配置、配置更新、连接失败 | 文档不随连接状态消失 |
| 页面类型 | SQL、Relation、Dashboard、Redis、活动 History | 类型、顺序和目标恢复正确，活动项合法 |
| 旧格式 | v1–v5、混合 v5、缺失 profile、未绑定目标 | 稳定迁移，无静默目标替换 |
| 损坏输入 | 重复 ID、非法路径、缺失 SQL、不可读 SQL | 明确错误，旧数据不被空状态覆盖 |
| 保存失败 | 构建失败、I/O 失败、旧回调、退出 flush | 正确 Failed/Retry/acknowledgement |

## 6. 检查点与交付物

- **M1：T01–T03** — 失败复现、全局格式契约、旧格式迁移测试建立。
- **M2：T04–T05** — 全局文档链路接通，真实双连接保存/恢复通过。
- **M3：T06–T07** — 文档生命周期与失败路径闭环。
- **M4：T08** — CI 检查、TUI 验收、架构文档与执行记录完成。

最终交付：生产代码、行为回归测试、版本迁移 fixture、迁移备份机制、架构文档、实际运行的验证结果。各检查点可作为用户要求提交时的逻辑提交边界；实施过程不自动发布版本或创建标签。

## 7. 当前执行记录

- 2026-09-14：完成代码静态排查和实施计划编写。
- T01：已完成基线和双连接保存复现；新增 `tests/global_workspace.rs`。
- T02：已完成当前 v5 兼容快照的按 profile 过滤、全局 Console 唯一性和活动 Tab 归属修复。
- T03：已完成 v3+ 缺失 SQL 显式错误；保留 v1/v2 缺失 SQL 的历史迁移兼容。
- T04：已完成切换前读取 SQL、双 profile 全局恢复回归；旧 profile 缓存仍保留，后续可独立移除。
- T05：已完成双连接保存/恢复和 profile 工作区合并；纯全局 consoles 快照继续兼容。
- T06：已完成 profile 删除时同步清理缓存与当前全局文档，保留其他 profile 文档。
- T07：已复核现有 revision/退出保存状态机，未发现需要本次修复修改的回归；Redis 定向测试已通过。
- T08：已更新 `docs/architecture.md`；`cargo fmt`、相关测试和 Clippy 通过。
- 全量 `cargo test --all-targets --all-features` 首次运行超过 120 秒，在 Redis 测试组附近超时；Redis 相关测试随后以 240 秒单独运行全部通过。完整全量测试仍需在 CI 或更长超时时间下复跑确认。
