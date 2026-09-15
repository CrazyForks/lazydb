# Consoles 全局生命周期与按需连接 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行适配：上述技能若不可用，按本文任务顺序直接执行。所有新增接口和测试名称均为拟定名称；实施时根据最新源码调整。每项先补行为测试，验证预期失败，再实现并验证。本文只规划实施，不表示功能已经完成。

**Goal:** 让 Consoles 在无连接启动时即可全局浏览、创建与打开，打开或重绑目标后自动复用或启动连接，并获得目标隔离的对象补全。

**Architecture:** 保留 Action → App::update → Command → Runtime 边界。Console 文档与全局 Tab 独立于连接 workspace；SessionRegistry 按 ExecutionTarget 管理连接，统一 target preparation 负责激活、连接复用、异步身份校验和补全元数据准备。优先完成现有全局 workspace v5 的运行时收敛，不同时引入另一份文档注册表。

**Tech Stack:** Rust 2024 / Rust 1.94、Tokio、Ratatui/Crossterm、Modalkit、Serde/TOML、现有数据库 adapter、SessionRegistry、WorkspaceStore；原则上无需新增生产依赖。

---

## 1. 本计划采用的产品契约

### 1.1 与旧计划的关系

本计划补全 `docs/plans/2026-09-12-multi-connection-consoles.md` 的全局工作区设计，并明确覆盖其中“打开、激活、重新绑定 Console 不发送 Connect”的旧约定：

- 用户主动打开、激活已绑定 Console，或者确认新 target 后，应当确保目标连接可用。
- 新建且激活的 Console 若已获得有效默认 target，同样走统一准备流程；无 target 时可离线编辑。
- 仅恢复工作区、打开 Consoles 面板、筛选、重命名或浏览 selector，不启动数据库连接。
- 执行 SQL 继续沿用已有待执行与事务流程；预热连接或元数据本身不执行用户 SQL。

### 1.2 “全局”和“打开”的定义

- 全局范围是当前 LazyDB 实例加载的 workspace 文档集合，不跨其他 workspace 搜集文件。
- 所有已保存 Console 都进入全局文档集合，包括未连接、连接失败、目标失效和未绑定文档。
- 当前全局 `tabs` 中存在该 Console UUID，才算“已打开”。
- 启动时恢复保存的 Tab 顺序及活动 Tab，但恢复动作不触发连接；用户随后主动激活已恢复的 Tab 时准备连接。
- 关闭 Tab 保留文档与 SQL；断开连接保留文档与 Tab；删除文档才删除 SQL 文件。

### 1.3 快捷键和弹窗

- 保留现有 `Space s` 的 Leader 入口。
- 新增建议默认全局键 `F6`，绑定到现有 `open-consoles` 命令，可配置；遵守用户对 F6 的显式自定义绑定。
- F6 在无 profile、无连接、无 Tab、Editor Insert、普通搜索输入、其他弹窗中都能打开面板；普通空格仍是输入。
- 面板已经打开时，再按全局键只聚焦现有面板，不重置搜索或创建嵌套面板。
- 原有交互弹窗的输入和待确认动作需要保留；取消 Consoles 返回原弹窗。确认打开 Console 时若存在未完成事务/确认动作，先恢复并处理原动作，再执行导航意图，不能覆盖 pending intent。
- 应用进入不可交互的最终退出阶段除外；连接中、查询中本身不是禁止打开面板的条件。

### 1.4 排序和命名

- 已打开文档在前，组内按 Tab 顺序；未打开文档在后，组内按大小写不敏感自然名称顺序，最后用原名和 UUID 稳定兜底。
- 搜索后保持相同分组规则，选择身份使用 UUID。
- 新名称扫描当前全部保存文档中完整匹配 `console_<ASCII 数字>` 的名称，大小写不敏感；取最大数字加一，无匹配时为 `console_1`。
- `console_01` 视为编号 1；`console_1_copy`、`console_-1`、`console_+1` 不参加编号计算。
- 删除最高编号后允许复用这一编号；这是当前集合 max+1，不是永久历史计数器。
- 溢出时返回明确的命名错误，不回绕、不 panic；旧重复名称允许恢复，保留 UUID/SQL，后续新建和重命名仍执行全局唯一性校验。

### 1.5 Target selector

- 鼠标与键盘入口统一定位具体 Console UUID，候选覆盖所有可用连接配置。
- 未连接/失败/首次连接中的 profile 仅展示配置默认 database/schema，不展示断线残留 catalog。
- 有可用会话的 profile 展示默认目标与有效 catalog 目标；候选继续遵守 catalog_scope。
- 没有可解析默认 database 的 profile 仍显示连接行，标明“需选择 database”，不能直接构造空字符串 ExecutionTarget。
- 此类连接行使用已有 profile 连接能力做发现，再在当前 selector 中选具体目标；如果 adapter/config 本身无法建立发现连接，显示可操作错误并引导编辑配置，Console 保持原绑定。
- 确认具体有效目标后，立即保存绑定，启动或复用连接；连接失败保留新绑定、SQL 与 Tab。
- 选择当前目标：在线且元数据就绪时无副作用；离线时触发重试；在线但元数据缺失时补加载。
- 当前 Console 正在运行、存在待执行 SQL 或非 Idle 事务时，复用既有等待/取消/事务解决规则；无关 Console 的任务不构成全局阻塞。

## 2. 已确认的代码落点

实施以符号定位为准，`src/app.rs` 较大且可能并行变化，不依赖本文行号。

| 文件 | 当前落点 | 目标改动 |
| --- | --- | --- |
| `src/app.rs` | restore_workspace / restore_profile_workspace / append_workspace / workspace_snapshot | 离线全局恢复，去掉按 profile 文档副本回灌 |
| `src/app.rs` | has_active_workspace / update 中早退条件 / OpenSqlEditorList | 文档功能独立于活动连接 |
| `src/app.rs` | next_console_name / create_sql_editor_named / empty_workspace_for | 统一全局命名和创建 |
| `src/app.rs` | visible_console_records / activate_sql_editor / prepare_active_console_target | Tab 派生排序、统一激活 |
| `src/app.rs` | OpenTargetSelector / OpenConsoleTargetSelector / bind_console_target | 合并 selector 和绑定入口 |
| `src/app.rs` | request_connection_target_inner / ConnectionSucceeded / ConnectionFailed | 区分前台连接导航与 Console 后台准备 |
| `src/model/session.rs` | SessionRegistry / SessionRequest | 复用已有 single-flight 语义，不重新实现连接注册表 |
| `src/runtime.rs` | Connect 分发、连接尝试追踪、成功/失败事件 | 校验目标隔离、并发和后台路由 |
| `src/model/workspace.rs` | ConnectionWorkspace / Overlay / ExplorerState | 缩减文档缓存；selector 和返回上下文；补全缓存作用域 |
| `src/model/tab.rs` | ConsoleRecord / ConsoleTab | 文档与派生运行状态；binding revision |
| `src/model/console_document.rs` | ConsoleDocuments::next_name / name validation | 共享命名/校验算法，不再形成第二份可变文档状态 |
| `src/persistence/workspace.rs` | v5 WorkspaceSnapshot / load / save | 全局文档与 Tab 的规范化、旧格式兼容 |
| `src/input/keymap.rs`, `src/input/mouse.rs`, `src/editor/mod.rs` | global keys / selector hit / EditorEffect | 相同业务入口 |
| `src/sql/completion.rs`, `src/app.rs` | CompletionIndex / refresh completion / catalog response | profile/target 隔离，按需预热 |
| `src/ui/mod.rs`, `src/help.rs`, `src/config.rs`, `config/default.toml` | 面板状态、快捷键与默认配置 | 全局 F6、离线/连接中/失败显示 |

重要事实：`CompletionIndex::replace_scoped()` 替换整个索引，scope 指 catalog 可见范围，不是自动按 profile 分桶。不能仅增加 target 参数而继续将多个 profile 轮流写进同一个索引。

## 3. 状态与接口设计

### 3.1 单一文档来源

本轮继续以全局 `App.sql_editors` 保存 Console 元信息，以 `EditorWorkspace` 保存 SQL/编辑历史，以 `App.tabs` 保存已打开视图。集中创建、重命名、绑定和删除入口，禁止 profile workspace 回灌覆盖已有文档。

`ConsoleRecord.open` 若因兼容调用暂留，仅作为从 tabs 生成的投影；保存和排序必须由 tabs 判断。`ConsoleDocuments` 共享纯算法，不向 App 新增另一份同时可写的注册表。后续容器改名/替换可以独立进行。

### 3.2 统一准备流程

```text
用户打开/激活/新建已绑定 Console
  → ensure_console_tab(console_id)
  → activate tab（仅响应用户动作）
  → prepare_console_target(console_id)
      → target missing/invalid：显示状态，允许编辑
      → SessionRequest::Existing + Connected：复用
      → SessionRequest::Existing + Connecting：登记等待，零 Connect
      → SessionRequest::Started：登记等待，恰好一个 Connect
  → 成功：更新匹配的 Console 状态 + ensure_completion_catalog
  → 失败：更新匹配的 Console 错误，保留文档/目标
```

新增最小等待记录建议放在 `src/model/console_target.rs`：Console UUID、目标快照、binding revision、ConnectionIdentity。App 按 Console UUID 保存等待记录。generation 使用现有应用级分配器。

成功事件先交 SessionRegistry 验证身份；再核对等待记录与 Console 当前 target/revision。迟到事件可使对应 session 成为可用，但不得改变已经重绑的 Console、活动 Tab、SQL 或当前弹窗。关闭/删除清理该 Console 等待者，不取消其他 Console 共用的连接。

只更新已存在 Tab 的连接状态，不在后台回调中重建编辑器或隐式生成默认 Console。是否继续更新旧全局 `connection` 投影由前台 Explorer 导航意图决定，不用它判定其他 Console 是否在线。

### 3.3 补全准备

- 优先使用按 profile 保存的 CompletionIndex，加上 Console target 的 database/schema 过滤；需要独立有效性时附 catalog/session generation。
- 同 profile 的多 database 请求不得复用一个不能服务该 database 的 catalog session；通过现有 session 能力判断选择连接。
- 如现有 catalog_sessions 仅按 profile 保存而无法表达并发 database，改为按 catalog 服务范围选择/索引，不能简单最后写入覆盖。
- 当前目标加载链：目标 database → schema → tables/views 等可补全对象；分页续取沿用现有目录调度器。
- 列信息依赖 SQL 中的 relation 时加载；不因打开一个 Console 扫描所有 database 的列。
- 在线复用路径也执行 metadata ensure；断线/DDL 更新只失效相关范围。
- 静态 SQL 关键字离线即可用；Redis 走自身能力分支，不发 SQL schema/table 请求。

## 4. 分阶段实施任务

每个步骤是独立动作；大面积调用者迁移按文件分成小步骤。每任务完成后形成可审查 diff 和建议提交边界，获得提交指令后再创建 Git commit。

### Task 1：建立行为基线和冷启动回归夹具

**Files:**
- 新增：`tests/consoles_lifecycle.rs`
- 参考/修改：`tests/global_workspace.rs`, `tests/workspace_persistence.rs`, `tests/startup_profiles.rs`

**Steps:**
1. 执行 `git status --short`，记录既有改动；检查本计划与旧多连接计划的冲突条目。
2. 执行 `cargo test --test global_workspace --test workspace_persistence --test startup_profiles`，记录基线失败及环境依赖。
3. 创建可复用夹具：A/B 两个内存 SQLite profiles，分别包含 open/closed Console、不同 SQL、保存顺序不同于名称顺序。
4. 通过 WorkspaceStore 保存/加载夹具；使用原始 App::new 后直接 restore_workspace，禁止手动赋值 connection.profile_id 或 active_workspace_profile。
5. 写入并单独运行 `cargo test --test consoles_lifecycle cold_restore_exposes_all_documents_without_connecting`，断言所有 UUID/SQL 恢复、无活动连接、面板可打开。预期旧实现失败于全局文档数量或面板状态。

**Done:** 回归场景是真实离线启动，而非人为构造“已安装 profile workspace”。

### Task 2：收敛全局文档恢复与 v5 持久化

**Files:**
- 修改：`src/app.rs`, `src/model/workspace.rs`, `src/persistence/workspace.rs`
- 测试：`tests/consoles_lifecycle.rs`, `tests/global_workspace.rs`, `tests/workspace_persistence.rs`, `tests/persistence.rs`

**Steps:**
1. 补充失败测试：无 profile 但有 targetless 文档；目标 profile 已删除；所有 Tab 已关闭；跨 profile 绑定后再次保存恢复。
2. 确认 v5 全局 consoles/tabs 的读取规范化；保留 loader 已支持的全部旧版本，并让旧 profile 文档通过同一规范化入口进入全局集合。
3. 删除 restore_workspace 中因 connection.profile_id 为空而清空文档并返回的分支，恢复元信息、SQL 与 Tab 视图，不返回 Connect。
4. 恢复以 UUID 去重；全局规范记录优先于兼容 profile 副本；冲突保留可恢复 SQL，不以名称去重或静默重命名。
5. 失效 target 保留原引用和可辨识状态，不自动换绑到第一个 profile；旧格式确实没有 target 的迁移逻辑单独处理。
6. 让 workspace_snapshot 从全局文档、全局 tabs、EditorWorkspace 生成唯一规范快照；旧 profile 缓存不再输出重复文档。
7. 移除 append/install/snapshot profile workspace 对已存在 Console 的回灌；Relation/Dashboard/RedisBrowser 的现有恢复需求仍由其视图状态负责。
8. 将 PersistedConsole.open 作为兼容字段由 Tab 集合生成，v5 tabs 决定新格式打开状态；旧格式 open 仅用于迁移。
9. 执行 `cargo test --test consoles_lifecycle --test global_workspace --test workspace_persistence --test persistence`；本任务只要求恢复/保存测试通过，后续行为测试随对应任务加入。

**Done:** 打开面板前全部文档已经存在；离线保存再重启不丢 SQL、目标、关闭状态或全局 Tab 顺序。

**建议提交:** `refactor(console): restore documents independently of connections`

### Task 3：解除面板门槛并提供全局入口

**Files:**
- 修改：`src/app.rs`, `src/action.rs`, `src/input/keymap.rs`, `src/model/workspace.rs`, `src/help.rs`, `src/config.rs`, `config/default.toml`
- 测试：`tests/consoles_lifecycle.rs`, `tests/keymap.rs`，相关模块内测试

**Steps:**
1. 参数化测试无 profiles、有离线 profiles、空 Tab、正在连接、查询中，OpenSqlEditorList 均能打开且不产生 Connect。
2. 从 OpenSqlEditorList 和 Console 列表输入/关闭/重命名路径移除 has_active_workspace 依赖；审查 update 顶部无 Console/无活动连接的早退白名单。
3. 在配置与 shortcut catalog 中加入 F6 默认全局 open-consoles 绑定；保留 Space s，输入路由先解析全局命令再交给 Editor Insert/弹窗文本输入。
4. 补充自定义 help=F6 测试，确保用户显式配置按现有优先级生效，不被新增硬编码劫持。
5. 复用 Omni 已有返回位置/延迟导航能力；如不足，仅新增 Consoles 所需的一个返回上下文，不建立通用无界 overlay stack。
6. 测试从重命名输入和事务确认中打开面板再取消，原输入/选择/待处理意图保留；面板打开时重按 F3 幂等。
7. 执行 `cargo test --test keymap --test consoles_lifecycle` 和 `cargo test --lib input::keymap`。

**Done:** F6 不受数据库状态或编辑模式限制；Space s 保持已有编辑器模式语义。

**建议提交:** `feat(console): make the manager globally accessible`

### Task 4：统一编号、创建入口和打开状态排序

**Files:**
- 修改：`src/app.rs`, `src/model/console_document.rs`, `src/model/tab.rs`, `src/model/sql_editor_list.rs`, `src/ui/mod.rs`
- 测试：`tests/consoles_lifecycle.rs`，`src/app.rs`、`src/model/console_document.rs` 模块内测试

**Steps:**
1. 写编号测试：1/3→4、仅 closed console_20→21、跨 profile、前导零、非完整匹配、删除最高值、整数溢出。
2. 提取唯一名称生成函数，输入全局名称迭代器，返回 Result；App 与 ConsoleDocuments::next_name 共用。
3. 查全 NewConsole、面板 create、默认 workspace 初始化等所有创建路径；用户新建调用统一函数，连接成功不额外生成隐式名为 console 的文档。
4. 保留已有默认 target 解析优先级，明确新建入口保存的 origin target 不依赖在线状态。
5. 用 tabs 构造 Console UUID→Tab 位置映射；列表排序和 open 展示都读取映射，不读取 profile 缓存中的 open。
6. 为 closed 文档加入 ASCII 数字片段自然排序，避免解析超长数字导致溢出；数字段按去前导零后的长度/字节比较即可。
7. 更新名实不符的旧排序测试；验证筛选、重命名、关闭后 UUID 选择稳定，不强制列表回到第一项。
8. 执行 `cargo test --lib console` 和 `cargo test --test consoles_lifecycle`。

**Done:** 所有新建路径采用全局 max+1；open 与 Tab 完全一致，关闭文档进入后半组。

**建议提交:** `fix(console): unify global naming and tab-based ordering`

### Task 5：增加幂等的 Console target preparation

**Files:**
- 新增：`src/model/console_target.rs`
- 修改：`src/model/mod.rs`, `src/model/tab.rs`, `src/model/session.rs`, `src/app.rs`, `src/runtime.rs`, `src/action.rs`
- 新增测试：`tests/console_target_lifecycle.rs`

**Steps:**
1. 写 reducer 测试：同 target 两个 Console 只产生一个 Connect；不同 target 可各自产生一个 Connect；已有 session 无 Connect。
2. 新增最小等待记录和 binding revision；将等待准备与 PendingExecution 明确分开，准备连接不创建待执行 SQL。
3. 实现 prepare_console_target(console_id)，只校验指定文档/Tab 的 target，不以全局 connection.target 判断在线。
4. SessionRequest::Started 才发送 Connect；Existing/Connecting 仅登记等待；Existing/Connected 直接更新状态并进入 metadata ensure。
5. 保留 request_connection_target 的 Explorer 前台导航语义；抽取共享底层连接请求，Console 调用不再设置单一 pending_editor_target_switch 来强制重连。
6. 检查 Runtime 的 Connect 去重和 tracker 是否按目标/identity 隔离；若仍存在全局 latest，改为目标级追踪并保留 credential prompt 的既有交互。
7. 成功/失败事件按 identity 更新 registry，再按 Console ID/target/revision 更新等待者；绝不在此选择 active_tab。
8. 测试 A→B→A 快速重绑与事件乱序；关闭/删除等待者；连接失败后重试；连接断开时仅失效相关 target。
9. 执行 `cargo test --test console_target_lifecycle` 和 `cargo test --lib model::session`。

**Done:** 连接准备幂等、跨目标独立；单个后台完成不改变用户当前视图。

**建议提交:** `feat(console): prepare target sessions with single-flight reuse`

### Task 6：所有 Console 激活入口接入统一准备

**Files:**
- 修改：`src/app.rs`, `src/commands.rs`, `src/input/mouse.rs`, `src/editor/mod.rs`
- 测试：`tests/consoles_lifecycle.rs`, `tests/console_target_lifecycle.rs`, `tests/workspace_tabs.rs`

**Steps:**
1. 按入口建立测试表：面板 Enter/鼠标、ActivateSqlEditor、前后 Tab、Tab 点击、Omni Console 导航、新建 Console。
2. 让各入口共用 ensure/open + activate + prepare；确保已经打开的同 UUID 不新建 Tab、不重置 EditorWorkspace。
3. 删除 activate_sql_editor/open_sql_editor 对活动 profile workspace 的要求。
4. 新建已绑定 Console 立即准备连接；targetless 文档只编辑。更新旧“新建已绑定 Console 无 Connect”测试为本计划契约。
5. 打开失败不回滚 Tab，状态栏显示错误；切走后连接成功不抢焦点。
6. 普通恢复操作和仅选择 Explorer 节点不调用 activate 流程，保证启动不自动连接全部已恢复 Tab。
7. 执行 `cargo test --test consoles_lifecycle --test console_target_lifecycle --test workspace_tabs --test global_workspace`。

**Done:** 从任何入口打开同一份 Console，连接与编辑器行为一致。

**建议提交:** `refactor(console): route activation through target preparation`

### Task 7：统一 selector 模型和离线候选

**Files:**
- 新增：`src/model/target_selector.rs`
- 修改：`src/model/mod.rs`, `src/model/workspace.rs`, `src/app.rs`, `src/action.rs`, `src/ui/mod.rs`
- 新增测试：`tests/console_target_selector.rs`
- 回归：`tests/execution_target.rs`

**Steps:**
1. 表驱动候选测试覆盖各数据库种类，以及 offline/connecting/online/failed、残留缓存、缺默认 database、catalog_scope。
2. selector state 必须含 console_id，选择身份用 profile+目标键而非裸下标；候选刷新仍定位相同目标。
3. 候选类型区分具体 ExecutionTarget 与仅 profile 的发现入口；每行带连接状态、是否可确认和原因。
4. offline 仅调用 from_profile/config 默认解析；仅 online 且 catalog 身份仍有效时加入目录项。
5. 缺默认 database 的 profile 显示发现行；选择后调用已有 profile 连接/发现能力，成功更新 selector 候选，不提前绑定 Console。
6. adapter 不支持默认发现连接时保留该行和错误，不编造 database/schema；Redis 不生成 schema。
7. UI current 标记只查 selector.console_id，修复 None 分支可能标记第一个 Tab 的问题；同名 profile 使用既有分组或短 UUID 消歧。
8. 执行 `cargo test --test console_target_selector --test execution_target`。

**Done:** 无论在线与否，每个可用 profile 都有可理解的入口；离线缓存不会扩展候选。

**建议提交:** `feat(console): unify cross-profile target selection`

### Task 8：确认绑定自动连接并保留事务语义

**Files:**
- 修改：`src/app.rs`, `src/action.rs`, `src/input/keymap.rs`, `src/input/mouse.rs`, `src/editor/mod.rs`
- 测试：`tests/console_target_selector.rs`, `tests/console_target_lifecycle.rs`, `tests/transaction_reducer.rs`

**Steps:**
1. 写失败测试：跨 profile 确认更新文档且发送 Connect；同 target 离线重试；在线同 target 不重连。
2. OpenTargetSelector 仅解析活动 Console ID，再委托统一 OpenConsoleTargetSelector；确认统一调用 bind + prepare。
3. 绑定前检查指定 Console 的 Running、PendingExecution 和事务状态；复用已有 deferred intent，不依赖其他 Console 的运行状态。
4. 绑定更新文档、Tab 投影和 binding revision；清理旧目标的 completion/diagnostics/结果派生状态，保留 SQL、光标和编辑历史。
5. 清理旧等待记录，持久化新绑定，再请求准备；失败不恢复旧 target，避免界面和保存值分离。
6. 测试切换目标不会自动执行此前等待的旧 SQL；目标切换日志与状态输出跟随对应 Console。
7. 执行 `cargo test --test console_target_selector --test console_target_lifecycle --test transaction_reducer`。

**Done:** Space d、编辑器命令、鼠标 target 点击使用完全相同的候选和确认逻辑。

**建议提交:** `fix(console): connect after rebinding execution targets`

### Task 9：隔离补全缓存及 catalog session 路由

**Files:**
- 修改：`src/model/workspace.rs`, `src/app.rs`, `src/sql/completion.rs`
- 新增测试：`tests/console_completion_lifecycle.rs`
- 回归：`tests/sql_completion.rs`, `tests/lsp_completion.rs`

**Steps:**
1. 构造 A/B 同名 database/schema、不同表的 catalog；交错提交目录响应，断言 A Console 只能看到 A 的对象，B 同理。
2. 在 Explorer/应用层按 profile 隔离 CompletionIndex，或将索引归入已有 profile catalog 状态；保留底层单索引算法供 LSP 使用。
3. 所有 Console completion/diagnostic context 从 Console target 选择索引和 database/schema；离线缺索引使用空索引返回静态关键字。
4. 更新 catalog response、DDL/drop、disconnect、profile edit 的索引更新/失效调用；只影响对应 profile/范围。
5. 审查 catalog_sessions 对同 profile 多 database 的限制；按可服务 database 的 ConnectionIdentity 选 session，必要时扩展映射 key。
6. 测试 A 的断开或目录变更不会删除 B 的候选；同 profile 不同 database 响应乱序不会污染或丢失有效范围。
7. 执行 `cargo test --test console_completion_lifecycle --test sql_completion --test lsp_completion`。

**Done:** 补全不依赖 Explorer 当前 profile，不会被另一连接最后返回的数据覆盖。

**建议提交:** `fix(completion): isolate console catalogs by execution target`

### Task 10：连接就绪后预热当前目标对象信息

**Files:**
- 修改：`src/app.rs`, `src/model/console_target.rs`，必要时目录请求状态所在 model
- 测试：`tests/console_completion_lifecycle.rs`, `tests/console_target_lifecycle.rs`

**Steps:**
1. 写测试：冷 Console 连接成功后，即使 Explorer 未展开，也产生当前目标必要目录请求并最终得到表名候选。
2. 实现 ensure_console_completion_catalog(target, identity)，复用 start_catalog_request_for_connection 和既有 pending/load 状态去重。
3. 只从当前 database/schema 开始补缺；必要的数据库发现之后只继续目标分支，不触发全库列扫描。
4. 当前目标无显式 schema 时按 adapter 已有默认解析或可见 schema 策略加载，不硬编码 public/dbo。
5. 在线复用路径也调用 ensure；列名依赖沿用 CompletionRequest 的按需 relation_children 请求。
6. 对 metadata 请求保存 target/revision/identity 有效性；目标切走后的响应只能更新对应缓存，不刷新错误 Console 弹窗。
7. 展示 Connecting / Loading catalog / Ready / Failed 状态；连接失败与目录加载失败分别可重试，SQL 始终可编辑。
8. 测试分页、目录失败后重试、两个 Console 共用元数据加载、Redis 跳过 SQL 目录分支。
9. 执行 `cargo test --test console_completion_lifecycle --test console_target_lifecycle`。

**Done:** “自动启动所属连接”真正带来对象提示，而不是只有连接状态变绿。

**建议提交:** `feat(completion): warm console target metadata on activation`

### Task 11：清理兼容分支与同步用户文档

**Files:**
- 修改：`src/app.rs`, `src/model/workspace.rs`, `src/model/console_document.rs`, `src/ui/mod.rs`, `src/help.rs`
- 文档：`docs/keybindings.md`, `docs/architecture.md`
- 旧计划：`docs/plans/2026-09-12-multi-connection-consoles.md`（仅增加 superseded 说明和新计划链接）

**Steps:**
1. 检查是否仍存在两套 next_name、两套 selector confirmation、Console 激活强制重连、profile 文档副本回灌。
2. 删除已无调用者的 Console 专属兼容路径；保留 Explorer database selector 独立语义。
3. 确认列表展示区分 open 状态与连接状态；失败不改变 open 分组。
4. 更新快捷键、命名、离线恢复、确认当前离线 target 会重试等说明。
5. 给旧计划增加本计划覆盖条目的链接，避免后续按旧约定回退行为。
6. 运行 `cargo fmt --all -- --check`；检查文档中的文件引用和命令与实际代码一致。

**Done:** 代码与文档只有一套 Console 生命周期契约。

**建议提交:** `docs(console): document global lifecycle and target preparation`

### Task 12：集成回归与最终验收

**Files:**
- 测试：`tests/consoles_lifecycle.rs`, `tests/console_target_lifecycle.rs`, `tests/console_target_selector.rs`, `tests/console_completion_lifecycle.rs`
- 必要回归：`tests/global_workspace.rs`, `tests/workspace_tabs.rs`, `tests/workspace_persistence.rs`, `tests/execution_target.rs`, `tests/keymap.rs`

**Steps:**
1. 编写一个保存→真实离线恢复→F6→打开 B→连接成功→表名补全→切 A→连接成功→再次保存恢复的贯穿测试。
2. 执行目标测试组合：

```bash
cargo test --test consoles_lifecycle --test console_target_lifecycle --test console_target_selector --test console_completion_lifecycle --test global_workspace --test workspace_tabs --test workspace_persistence --test execution_target --test keymap
```

3. 对齐现有 CI，一次执行最终静态检查和全量测试：

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

4. 数据库行为按 `.github/workflows/ci.yml` 的现有 fixtures/环境变量运行相应集成测试；本地缺少外部数据库时记录未运行项，由已有数据库 CI 验证，不把 mock 通过表述成真实数据库验证。
5. 用 TUI 手工检查下方矩阵中的可见行为；重点检查 Editor Insert 下 F6、不同弹窗返回、快速切换、窄终端目标状态。
6. 执行 `git diff --check`，审查最终改动范围及遗留兼容字段；将验证结果附于实施总结。

**Done:** 新契约测试、既有 Console/事务/目录回归和 CI 检查通过；任何环境阻塞有明确记录。

## 5. 最终验收矩阵

| 场景 | 预期 |
| --- | --- |
| 无配置、无连接、无 Tab | F6 打开空面板，可创建 console_1 |
| 有 A/B 配置，但从未连接 | F6 正常；打开面板不产生 Connect |
| A/B 有 open/closed 文档，冷启动 | 全部可见、SQL 完整，恢复不连接 |
| 已打开文档与未打开文档混合 | 前者按 Tab 顺序在前，后者自然名称排序 |
| closed console_30 属于离线 B | 新建默认名称至少 console_31 |
| 打开离线 B 文档 | Tab 立即显示原 SQL，B 发起一次连接 |
| 两份文档共享 B target | B 连接中不重复 Connect，成功都可用 |
| A/B 同时启动 | 互不阻塞；乱序完成不抢 Tab 焦点 |
| 打开已在线 target 文档 | 复用原 session，不 force reconnect |
| 连接或元数据失败 | 文档可编辑，错误定位明确，可重试 |
| 未展开 Explorer 即编辑 SELECT | 当前 target 的表/视图候选可自动获得 |
| A/B 的 database/schema 同名 | 候选按 profile 隔离 |
| selector 中 B 离线且留有旧缓存 | 仅显示 B 配置默认目标 |
| B 没有默认 database | B 仍可见，进入发现/配置处理，不空目标重绑 |
| 确认切到 B | 保存新 target，自动启动 B，SQL/编辑历史保持 |
| 当前目标离线，再次确认它 | 发起重试，而非无条件 no-op |
| 查询运行/待执行/事务未结束 | 复用该 Console 的既有解决流程，不转移旧 SQL |
| A→B→A 后旧响应返回 | 旧响应不覆盖当前绑定、诊断或补全弹窗 |
| 连接中关闭 Console | 关闭状态保持；后台完成不重新打开 |
| 删除 profile 再重启 | Console 与 SQL 保留，目标失效可见，可重绑 |
| 旧格式/旧同名 Console 恢复 | UUID/SQL 不丢；后续新建保持全局唯一 |
| 新建后连接成功 | 不额外生成名为 console 的隐式 Tab |
| F6 从 Insert 或其他弹窗打开 | 不插入字符、不丢原输入/待确认动作，取消可返回 |

## 6. 依赖与里程碑

```text
Task 1 → Task 2 → Task 3 → Task 4
                   Task 2 → Task 5 → Task 6
                            Task 5 → Task 7 → Task 8
                            Task 5 → Task 9 → Task 10
Task 4 + Task 6 + Task 8 + Task 10 → Task 11 → Task 12
```

- **M1（Task 1–4）：** 真正离线可见、可管理，命名/排序满足要求。
- **M2（Task 5–8）：** 所有打开/绑定入口统一按需连接，连接中去重，异步状态不串线。
- **M3（Task 9–10）：** 自动连接后有正确对象补全，跨连接缓存隔离。
- **M4（Task 11–12）：** 兼容路径收敛、文档更新、端到端验收完成。

多个任务都会修改 `src/app.rs`，默认按以上依赖顺序串行实施；若后续明确采用多代理，必须先划分无共享写入的任务和接口边界。

## 7. 执行交接

计划完成后可逐任务在当前会话实施；也可在独立 worktree 的新会话按里程碑执行。若选择多代理方式，先取得明确的代理执行指令，再分派任务并在每阶段合并审查。
