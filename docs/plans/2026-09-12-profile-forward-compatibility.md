# Connection Profile Forward Compatibility Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 让 LazyDB 遇到未来版本的连接配置时仍可进入 TUI，可用连接照常使用，不支持的连接显示为只读占位项，并保证保存其他连接不会丢失未知配置及关联 SQL。

**Architecture:** 将原始 TOML 文档、列表可见条目、可执行 ConnectionProfile 分离。加载按条目分类，运行层仅接收已支持的强类型连接；保存以原始文档为基础应用明确变更，保留未知节点，并通过统一持久化事务处理并发修改与凭据回滚。

**Tech Stack:** Rust 1.94、Serde、toml 0.9、toml_edit 0.23、Tokio、Ratatui、现有 tempfile 与 mock secret-store 测试设施。

---

## 0. 范围、约束与实施顺序

本次交付包含加载、保存、运行入口、TUI、分组、工作区、Agent/LSP 和兼容性文档。保持 DatabaseKind 为当前支持数据库的封闭枚举，不向驱动层传递伪造的 Unknown 连接。

本计划中的新增类型、方法和测试名称是目标接口，实施时需随每个任务补齐所有调用方并保持可编译。每个任务按“添加行为测试 → 运行确认失败 → 实现 → 定向验证 → 提交”的顺序完成；提交时仅暂存该任务的明确文件，不使用 `git add .`。

依赖顺序：

```text
1 类型与契约 → 2 加载 → 3 定点保存 → 4 事务集成
                         ↓                 ↓
                    5 运行门禁 → 6 列表展示 → 7 分组组织
                         ↓
                    8 工作区保留 → 9 Agent/LSP → 10 版本恢复模式
                                                     ↓
                                                11 联调与文档
```

任务 1～10 是同一功能交付的组成部分，不能仅发布“过滤未知连接后正常启动”的中间状态。

### 明确的产品行为

| 情况 | 加载与展示 | 执行 | 写入 |
|---|---|---|---|
| 当前结构版本、正常连接 | 正常条目 | 允许 | 定点更新 |
| 未知 kind | 灰色 `?` + 原始 kind + UNSUPPORTED | 禁止 | 原文保留 |
| 已知 kind、未知字段或枚举 | 灰色 + CONFIG UNSUPPORTED | 禁止 | 原文保留 |
| 单条已知配置字段错误 | 灰色 + INVALID CONFIG | 禁止 | 原文保留 |
| 已知 kind、构建未启用驱动 | 灰色 + DRIVER UNAVAILABLE | 禁止 | 第一阶段只读 |
| 无法理解的未来文件版本 | 应用进入兼容只读模式 | 文件内连接禁止执行 | 禁止修改该配置文件 |
| TOML 语法损坏或根结构损坏 | 配置错误界面 | 不使用受损文件建立连接 | 禁止覆盖 |
| 配置文件不存在 | 沿用首次启动流程 | 允许临时连接 | 允许创建 |
| 文件权限或读取 I/O 错误 | 沿用明确的 I/O 错误处理 | 不冒充空配置 | 禁止覆盖 |

未知字段第一阶段统一保守处理：该连接不可执行。只有未来明确约定为可忽略的扩展命名空间才允许忽略其语义并保留原文，本次不发明该协议。

正常支持的 Oracle 构建不应因本机尚未安装 Oracle 客户端库而直接标记为未知类型；区分编译能力与连接时环境错误。

## Task 1: 建立配置条目与加载结果契约

**Files:**
- Create: `src/profile_compatibility.rs`
- Modify: `src/lib.rs`
- Modify: `src/persistence/profiles.rs`
- Test: `tests/profile_compatibility.rs`（新建）

**Step 1 — 添加分类模型行为测试。** 验证可用条目可取得 ConnectionProfile，不可用条目不能取得；重复或缺失 UUID 的行具有不同的临时展示标识。

**Step 2 — 定义类型并保持现有调用路径工作。**

```rust
pub enum ProfileEntry {
    Supported(ConnectionProfile),
    Unavailable(UnavailableProfile),
}

pub enum ProfileUnavailableReason {
    UnsupportedKind { kind: String },
    UnsupportedConfig { field: Option<String> },
    DriverUnavailable { kind: DatabaseKind },
    InvalidProfile { field: Option<String> },
}

pub enum ProfileEntryId {
    Persistent(Uuid),
    DocumentRow(usize),
}
```

UnavailableProfile 持有展示标识、可选原 UUID、名称、原始 kind、可安全解析的 group/access 信息以及原因。DocumentRow 仅用于当前加载结果，不写入文件，不进入连接和工作区 UUID 注册表。

LoadedProfileDocument 持有原始 DocumentMut、文件源状态、版本、条目顺序、诊断和写入模式；放在持久化层。UI 只接收条目摘要，不携带原文或凭据。原始文档不得派生会打印全部内容的 Debug。

保留 ProfileCollection 作为现有可执行连接的投影，新增加载接口供后续任务迁移。最终所有生产读写入口必须显式处理完整文档，不能保留静默丢弃不可用条目的快捷路径。

**Step 3 — 验证。** `cargo test --test profile_compatibility`；预期模型测试通过。

**Step 4 — 提交。** `feat(profiles): model profile compatibility entries`

## Task 2: 实现两阶段、逐条降级的配置加载

**Files:**
- Modify: `src/persistence/profiles.rs`（ProfileStore::load、历史版本 DTO、validate_collection）
- Modify: `src/profile_compatibility.rs`
- Test: `tests/profile_compatibility.rs`
- Test: `tests/persistence.rs`

**Step 1 — 添加混合文件测试。** 使用 import_connection_url 创建一条 SQLite 内存连接并序列化为基准；另追加 `kind = "future_db"`，携带未知嵌套表、未知凭据模式及注释。不要依赖 Oracle 始终是未知数据库。

覆盖：未知 kind、未知 ssl_mode、未知 credential_policy、未知顶层连接字段、错误 port 类型、缺失 UUID、重复 UUID、损坏的 group 引用和不认识的 access。

**Step 2 — 先解析 DocumentMut，再解析公共元数据和每一条 profile。**

1. 根结构与版本读取失败形成文件级诊断。
2. 对当前和已知历史版本按对应 DTO 解析单条连接。
3. 先检查原始 kind；未知 kind 不再尝试解析其他驱动字段。
4. 已知 kind 仍采用严格字段解析；未知字段不被直接吞掉。
5. 将 Keyring → System、已知 URL 格式规范化等现有迁移仅应用于成功解析的连接投影；加载不写盘。
6. 重复 UUID 的所有冲突项降级，避免“先出现者获胜”；完整文档保留它们。
7. 条目级引用错误单独降级；根级重复/无效分组导致文档只读，避免继续执行组织写操作。
8. 无法解释 access 的条目只显示本地配置诊断，不进入 Agent/LSP 可见范围；绝不能默认成 Global。

不要通过匹配完整 Serde 英文错误字符串建立长期协议。对关键字段显式检查，未分类的解码错误归为 InvalidProfile；面向用户输出字段路径和概要，避免回显包含凭据的 TOML 片段。

**Step 3 — 验证。** `cargo test --test profile_compatibility --test persistence`；预期已知连接数量正确、未知内容仍存在于文档、v2～v5 迁移通过。

**Step 4 — 提交。** `feat(profiles): load incompatible profiles as unavailable entries`

## Task 3: 实现无损定点保存

**Files:**
- Modify: `src/persistence/profiles.rs`
- Create: `src/persistence/profile_document.rs`
- Modify: `src/persistence/mod.rs`
- Create: `tests/profile_document.rs`

**Step 1 — 添加 round-trip 测试。** 修改已知连接名称后，断言未知连接整段文本（包括注释、嵌套表、凭据内容）保持不变；无修改保存时整文件字节相同。另测可选字段清除、增加连接、明确删除已知连接、分组修改及顺序变化。

**Step 2 — 引入明确的文档操作。** 操作至少包括 InsertProfile、UpdateProfile（携带预期旧值和新值）、DeleteProfile、InsertGroup、RenameGroup、DeleteGroup、ReorderProfiles。修改必须引用成功加载且无歧义的条目。

UpdateProfile 对旧、新强类型字段做差异计算，只设置或删除实际改变的字段；不能用整个新表替换原表。凭据字段仅在 CredentialUpdate 明确变更时改写，Preserve 必须保留原节点。新增条目使用规范序列化生成节点。

未知连接与未知根节点不参与重建。重新排序搬移原节点，不能从可用连接集合重新生成 profiles。不可用条目的顺序策略在任务 7 明确。

**Step 3 — 明确迁移写入规则。**

- 当前 v6 文档定点写入，保留 version。
- 已知历史版本全部条目可理解时，在第一次实际写入时执行显式迁移，保留未修改的扩展节点与注释。
- 历史版本混有不理解的条目时，文档只读，避免将未知旧格式原封不动塞入新版本后错误标注。
- 未来版本禁止写入，不降级 version；新建文件使用当前结构版本。

**Step 4 — 保留现有原子文件写入。** 继续使用唯一临时路径、私有权限、写入、sync_all、rename 和失败清理；只有成功写盘后才更新文档源快照。

**Step 5 — 验证。** `cargo test --test profile_document --test persistence`；预期未知文本和元数据无损，迁移边界正确。

**Step 6 — 提交。** `feat(persistence): preserve unknown profile data during edits`

## Task 4: 接入注册表、写入串行化与凭据事务

**Files:**
- Modify: `src/runtime.rs`（ProfileRegistry、save_profile_transaction、delete_profile_transaction、save_profiles 及组织保存路径）
- Modify: `src/persistence/profiles.rs`
- Test: `tests/profile_runtime.rs`
- Test: `tests/profile_lifecycle.rs`
- Test: `tests/profile_document.rs`

**Step 1 — 添加事务失败测试。** mock secret store 记录读写计数；模拟文档只读、外部文件变化、写盘失败以及两个并发保存。

**Step 2 — 让完整文档拥有唯一权威写入状态。** ProfileRegistry 持有加载的文档和可执行投影；引入专用异步写事务互斥锁，覆盖快照读取、凭据更新、文件提交和注册表替换。不要让所有连接查询长期持有该写锁。

**Step 3 — 迁移所有生产保存调用。** 使用明确变更操作，禁止通过 ordered_persisted_collection 的过滤结果推断删除。组织修改、连接新增、删除和凭据保存使用同一个事务入口。

**Step 4 — 实现源内容冲突检测。** 对比加载时原始字节或摘要，并区分文件不存在、被删除、被替换。写入前重读检测不一致即返回 ConfigChangedOnDisk，要求重新加载，不自动合并。

应用内写入由事务锁串行化；多个 LazyDB 进程通过同一配置专用 sidecar 锁协调提交，明确锁释放和异常退出恢复行为。原子 rename 本身不等于并发保护；任意不遵守锁协议的外部编辑器仍存在检查后的竞争窗口，文档中准确说明这一边界，不宣称具有跨任意写入者的 CAS 保证。

**Step 5 — 对齐凭据回滚。** 不可用/只读检查在凭据访问前完成；提交前再次检查源状态。任何后续保存失败沿用 rollback_after_failure，注册表和文档快照都不前移。

**Step 6 — 验证。** `cargo test --test profile_runtime --test profile_lifecycle --test profile_document`；预期无丢失更新、失败事务回滚、不可用连接不访问凭据。

**Step 7 — 提交。** `fix(runtime): commit profile document changes transactionally`

## Task 5: 启动、连接选择与统一操作门禁

**Files:**
- Modify: `src/runtime.rs`（StartupProfiles、load_startup_profiles、连接/测试/发现入口）
- Modify: `src/app.rs`
- Modify: `src/action.rs`
- Modify: `src/profile_compatibility.rs`
- Test: `tests/startup_profiles.rs`
- Test: `tests/profile_reducer.rs`
- Test: `tests/profile_runtime.rs`

**Step 1 — 测试三种启动场景。** 混合连接、全部不可用、显式 --profile 选中不可用条目；全部不可用不能被当作首次启动的空文件。

**Step 2 — 扩展启动数据。** 同时携带列表条目、可执行连接投影、诊断与配置写入模式；保持 --url 临时连接不写入原文档的行为。

**Step 3 — 提供统一的按 ID 解析接口。** 解析结果区分 Supported、Unavailable、NotFound、Ambiguous。TUI 显式选择不可用连接时打开界面并定位该条目；headless 执行返回明确不可用错误。

**Step 4 — 覆盖所有触发路径。** Enter、自动连接、重连、Test、SaveAndConnect、目录发现、SQL 执行及密码解析必须在副作用之前通过能力检查。已有类型但未编译驱动根据构建能力分类，不通过网络探测。

**Step 5 — 验证。** `cargo test --test startup_profiles --test profile_reducer --test profile_runtime`；预期不存在连接尝试、密码弹窗或目录请求泄漏。

**Step 6 — 提交。** `feat(runtime): gate unavailable profile operations`

## Task 6: Explorer 不可用条目与详情反馈

**Files:**
- Modify: `src/model/explorer.rs`
- Modify: `src/model/workspace.rs`（可见行模型）
- Modify: `src/ui/mod.rs`（连接行、搜索结果、详情和通知）
- Modify: `src/ui/icons.rs`
- Modify: `src/ui/profiles.rs`
- Modify: `src/app.rs`
- Test: `tests/explorer_state.rs`
- Test: `tests/ui_render.rs`
- Test: `tests/profile_reducer.rs`

**Step 1 — 添加 Ratatui buffer 与 reducer 测试。** 覆盖三种图标模式、选中/未选中、窄终端、搜索结果、全部不可用。

**Step 2 — 在行模型加入独立可用性。** 不复用 Failed 状态；损坏 UUID 使用 DocumentRow 对应节点标识，不创建可连接的 profile UUID。

**Step 3 — 渲染效果。** 名称与图标使用 theme.muted；未知类型使用问号图标，已知但驱动不可用可保留数据库图标；追加原始 kind 与状态文字。选中背景清晰，搜索高亮不遮蔽状态；长文本截断时优先保留状态。

**Step 4 — 交互反馈。** 可选中和搜索，无展开箭头；Enter 展示名称、类型、原因和升级/修复建议。不可用条目的编辑、复制、删除、分组移动禁用并说明原因。不得展示或复制原始凭据。

**Step 5 — 单次启动摘要。** 例如 `Loaded 5 connections; 1 connection is unsupported by this version.`。手动访问仍可查看详情，禁止每帧或每次重绘重复通知。

**Step 6 — 验证。** `cargo test --test explorer_state --test ui_render --test profile_reducer`；预期状态不只依赖灰色，且不会显示虚假的“正在连接”。

**Step 7 — 提交。** `feat(ui): show unsupported connections as read-only entries`

## Task 7: 保留混合条目的分组与排序

**Files:**
- Modify: `src/model/profile_organization.rs`
- Modify: `src/model/profile_group.rs`
- Modify: `src/app.rs`
- Modify: `src/runtime.rs`
- Modify: `src/persistence/profile_document.rs`
- Test: `tests/profile_groups.rs`
- Test: `tests/profile_document.rs`

**Step 1 — 添加混合分组测试。** 分组包含正常、未知和损坏条目；测试改组名、删除组、移动正常连接和重启顺序。

**Step 2 — 组织操作读取完整元数据。** 已知 group_id 的不可用条目仍显示在原组。缺失分组引用的条目进入配置异常展示区域，不自动修正原文件。

**Step 3 — 固定第一阶段边界。** 允许仅改组名；包含不可用条目的组禁止删除，因为当前 delete_group 会清空成员 group_id。不可用条目不可主动排序或移动；正常条目只能与同组可移动条目交换，未知节点作为固定位置锚点，文件和界面遵守同一算法。

**Step 4 — 排除集合重建。** 无论同组、跨组还是排序操作，都通过任务 3 的明确操作修改原节点，不能将不可用条目从持久化集合中漏掉。

**Step 5 — 验证。** `cargo test --test profile_groups --test profile_document`；预期未知成员、顺序和分组引用保留。

**Step 6 — 提交。** `fix(profiles): preserve unavailable entries in organization changes`

## Task 8: 工作区恢复与 SQL 绑定保留

**Files:**
- Modify: `src/app.rs`（restore_workspace、快照生成、执行目标恢复）
- Modify: `src/runtime.rs`（工作区加载/保存）
- Modify: `src/persistence/workspace.rs`
- Modify: `src/model/workspace.rs`
- Test: `tests/workspace_tabs.rs`
- Test: `tests/workspace_persistence.rs`
- Test: `tests/execution_target.rs`

**Step 1 — 添加“未知连接拥有 SQL”回归测试。** 工作区有未知连接 A 的 SQL 和正常连接 B；加载、编辑 B、保存、重载后，A 的 SQL 内容、tab ID、执行目标和原 profile ID 不变。

**Step 2 — 禁止错误重绑。** 当前 restore_workspace 中存在“目标校验失败后绑定 selected/first profile”的分支；对于明确绑定不可用连接的控制台，保留原绑定并标记目标不可用，不回退到 B。真正历史无目标控制台的现有迁移单独处理。

**Step 3 — 保存休眠工作区。** 将不可用连接对应快照保留为 opaque/dormant 数据；生成新快照时按 profile ID 合并未激活的原数据。未知连接不被解释为已删除，也不触发 SQL 文件清理。

**Step 4 — 防止未知工作区版本阻断启动或被降级覆盖。** 无法解析未来 workspace 版本时，通知用户并停用该工作区文件的自动保存；配置文件中的可用连接仍可进入应用。不要把工作区损坏等同于空快照后重写。

**Step 5 — 验证。** `cargo test --test workspace_tabs --test workspace_persistence --test execution_target`；预期不丢 SQL、不跨数据库重绑、不对未知目标执行查询。

**Step 6 — 提交。** `fix(workspace): preserve SQL bindings for unavailable profiles`

## Task 9: Agent、LSP 与其他读取入口

**Files:**
- Modify: `src/agent/service.rs`
- Modify: `src/agent/context.rs`
- Modify: `src/agent/selection.rs`
- Modify: `src/lsp/catalog.rs`
- Modify: `src/uninstall.rs`
- Test: `tests/agent_context.rs`
- Test: `tests/agent_selection.rs`
- Test: `tests/agent_service.rs`
- Test: `tests/lsp_catalog.rs`

**Step 1 — 添加 headless 测试。** 未知连接存在时已知连接仍可选择；显式请求不可用连接返回 UnsupportedProfile；同名条目不被过滤后误选；不认识的 access 不扩大可见范围。

**Step 2 — 迁移加载与选择。** 先执行能够可靠判断的范围过滤，再做名称/UUID 选择和可用性解析。自动选择仅在范围内可用连接中进行，显式选择必须区分不存在、不支持和歧义。只向调用方返回允许暴露的概要。

**Step 3 — 在 CredentialResolver 与 DatabaseConnection::connect 前拒绝不可用项。** LSP 正常提供与数据库无关的语言功能，目录加载报告可理解的原因，不让一条未知连接破坏整个服务。

**Step 4 — 审计其他 ProfileStore::load/save 使用方。** 特别处理 uninstall 的凭据清理：仅操作可验证的 UUID 与当前理解的凭据引用；无法解释的条目说明未自动清理其凭据，不猜测未来存储结构。避免保留“兼容加载后只取 known profiles”的隐式写回路径。

**Step 5 — 验证。** `cargo test --test agent_context --test agent_selection --test agent_service --test lsp_catalog`；补充运行卸载现有测试中被影响的测试目标。

**Step 6 — 提交。** `fix(integrations): handle unavailable profiles consistently`

## Task 10: 未来版本与配置错误恢复界面

**Files:**
- Modify: `src/persistence/profiles.rs`
- Modify: `src/runtime.rs`
- Modify: `src/app.rs`
- Modify: `src/ui/mod.rs`
- Test: `tests/profile_compatibility.rs`
- Test: `tests/startup_profiles.rs`
- Test: `tests/ui_render.rs`

**Step 1 — 测试未来 version、缺失 version、非法根结构及 TOML 语法错误。** 记录原始文件字节，模拟启动和退出后确认完全不变。

**Step 2 — 将可恢复的配置内容问题传入 TUI。** 未来版本只做保守元信息提取，所有该文件条目标记不可用；无法解析元信息则显示文件级详情，不制造“0 connections”首次启动表单。

**Step 3 — 完善只读模式。** 通知展示配置路径、发现版本、当前支持版本或安全的行列定位；保存、新增持久化连接、组织变更和凭据变更全部阻止。已知 --url 临时连接可继续使用，但不能保存到该文件。

**Step 4 — 外部修复后重新加载。** 提供使用现有命令/动作体系的 Reload profiles 操作；仅在成功解析后替换加载结果和诊断。正在进行的配置事务不能与 reload 交错；重新检查活动连接修订与工作区引用。

**Step 5 — 验证。** `cargo test --test profile_compatibility --test startup_profiles --test ui_render`；预期应用可打开、错误可见、原文件不被改写。

**Step 6 — 提交。** `feat(config): add read-only recovery for future profile formats`

## Task 11: 完整回归、手动验收与兼容协议文档

**Files:**
- Modify: `docs/configuration.md`
- Modify: `docs/architecture.md`
- Modify: `docs/database-capabilities.md`
- Modify: `docs/coding-agent-access.md`
- Modify: `docs/sql-language-server.md`
- Modify: `.github/workflows/ci.yml`（补充无默认驱动构建的定向检查）

**Step 1 — 文档约定。** 说明新增 kind 不提升结构版本，新增行为字段在旧版会禁用该条连接，结构语义破坏才提升版本；写清第一版支持前向兼容的应用版本、未知条目只读、配置冲突重载、未来工作区版本处理和手工修复流程。

说明保留语义与文本的区别：本次要求无操作整文件不变、未修改未知条目文本不变；已修改字段和显式迁移允许必要格式变化，不能承诺整个文件在编辑后字节不变。

**Step 2 — 执行定向组合测试。**

```sh
cargo +1.94.0 test --test profile_compatibility --test profile_document --test persistence --test startup_profiles --test profile_runtime --test profile_lifecycle --test profile_groups --test profile_reducer --test workspace_tabs --test workspace_persistence --test execution_target --test explorer_state --test ui_render --test agent_context --test agent_selection --test agent_service --test lsp_catalog
cargo +1.94.0 test --no-default-features --test profile_compatibility --test startup_profiles
```

预期全部通过；无默认驱动构建下 Oracle 是“已知但驱动不可用”，future_db 始终是未知类型。

**Step 3 — 执行与 CI 对齐的完整检查。**

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

数据库在线测试遵守项目现有环境变量和 CI 数据库服务配置。未配置数据库的本地运行不能当作已验证真实适配器连接；本功能的核心兼容回归必须不依赖外部数据库。

**Step 4 — 手动 TUI 验收。** 使用临时配置目录：

1. SQLite 正常条目 + future_db 未知条目，验证颜色、图标、标签、搜索和 Enter 原因。
2. 修改 SQLite 名称，退出，diff 配置，确认未知条目保持原样。
3. 验证不可用条目的编辑、复制、删除、测试、分组删除均有明确反馈。
4. 全部连接不可用仍显示列表，不自动打开创建表单。
5. 使用未来 version，验证只读提示和所有配置写入门禁。
6. 从外部修改配置，验证保存冲突与 reload。
7. 恢复未知连接所属 SQL，验证没有自动绑定到 SQLite。
8. 三种图标模式和窄终端各检查一次。

**Step 5 — 最终验收矩阵。**

| 验收项 | 自动覆盖 |
|---|---|
| 一条未知连接不阻断其余连接 | profile_compatibility / startup_profiles |
| 未知字段、枚举不静默降级 | profile_compatibility |
| 原始未知节点与凭据保留 | profile_document |
| 并发修改与事务失败不丢更新 | profile_document / profile_runtime |
| UI 可解释且不可执行 | ui_render / profile_reducer |
| 分组和排序不丢节点 | profile_groups |
| SQL 与执行目标不丢失、不重绑 | workspace_tabs / execution_target |
| Agent/LSP 不扩大访问范围 | agent_context / agent_selection / lsp_catalog |
| 未来配置与工作区版本不被覆盖 | startup_profiles / workspace_persistence |
| 缺少编译驱动正确分类 | no-default-features 定向测试 |

**Step 6 — 提交。** `docs(config): document forward compatibility guarantees`

## 完成定义

- 完整读写流程均保留不可用条目，不存在把已知子集写回整文件的生产路径。
- 未知条目在 TUI 中可发现、可解释，并且所有副作用入口被拦截。
- 原始配置、凭据节点、分组关联、SQL 内容与执行目标满足上述保留契约。
- 已知历史版本迁移与正常连接的原有生命周期通过回归。
- 本地检查结果、需要 CI 才能验证的项目及手动验收结果在实施总结中明确记录。
