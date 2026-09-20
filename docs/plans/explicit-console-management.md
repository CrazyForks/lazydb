# Console 显式管理 Implementation Plan

**Goal:** 新建、连接、重连和切换连接不自动生成 Console；Console 由用户通过 Consoles 管理流程显式创建和管理。

**Architecture:** 保留 Action → App → Command → Runtime 架构，让连接工作区可以为空，将文档构造与连接激活解耦。恢复既有文档继续使用当前持久化结构；启动不注入默认文档，外部管理快捷入口进入 Consoles。

**Tech Stack:** Rust 2024 / Rust 1.94、Ratatui、现有 App reducer、WorkspaceStore、SessionRegistry；测试使用公开 Action、TestBackend、tempfile 和既有连接成功事件夹具。

---

## 执行约束与基线

- 本计划由 Astra 编写；实施、审查、纠偏、提交合并由 Luna 负责。当前阶段仅写计划，不启动子 Agent，不创建 worktree 或分支，不修改业务代码。
- 起点及当前 HEAD、main 均为 `dd9246da608700d84958e61479f82e44ea1a1b55`。任务/分支名称留给后续工作流确定。
- 原工作空间唯一未跟踪文件为 `docs/plans/2026-09-20-console-lazy-connection-implementation.md`，它不作为本任务依赖，不复制进新 worktree，不实施其离线编辑/按需连接方案。
- checkpoint.json 当前不存在；不创建或修改它。后续若插件提供检查点，以实际 diff 和当前用户任务为准。
- 报告、计划和验证记录只放当前 `.git/opencode-tasks/ses_f41fa7367ffeGwGUSPQKkZXqjS/`。本轮完成后仅写指定 `plan-d4567b34-0f1e-4936-98f3-fed9b5c1e220.json` 回执，不改写历史回执。
- 不自动删除历史名为 `console` 的文档；名称不能用来判断是否属于可丢弃文档。
- 本计划采用分析中严格的“管理中手动管理”解释：启动不创建 Console；Omni 新建和直接删除快捷入口进入 Consoles，由用户在其中完成操作。关闭标签和打开已存文档不等于新增/删除文档。
- 手动新建/打开 Console 后的现有自动连接策略保持现有语义。不要把本任务扩大成“只有执行 SQL 才联网”。

## 已核实的关键入口

| 位置 | 实施要点 |
|---|---|
| `src/app.rs::empty_workspace_for`，1233 起 | 当前构造一个绑定 target 的 console；替换为真正空工作区 |
| `activate_profile_workspace`，1255 起 | 冷连接成功和热 session 复用的共同入口；保留已有工作区恢复 |
| `create_and_activate_sql_editor_named`，16398 起 | 16423 的兜底也调用上述构造器，必须避免手动一次建出两个文档 |
| `with_profiles`，781 起 | 零 profile 启动注入默认文档；移除 |
| `remove_placeholder_console` 与 placeholder_console_id | 移除启动注入后清理专用状态，避免误删正常文档 |
| `SqlEditorListCreate`，5826 起 | 管理中的显式创建入口，恰好创建一个文档 |
| `confirm_omni_item`，3413 起 | Root NewConsole 自带 NameConsole/PickConnection 流程，不能只改 execute intent 而遗漏此分支 |
| `src/input/keymap.rs:3131` | Leader x → RequestDeleteActiveConsole；应进入管理中的删除确认 |
| `src/commands.rs:130` | Omni 的 New Console 命令，可保留可发现性但导向管理界面 |
| `tests/console_manager_input.rs:41` | 已有管理创建用例明确期望 Connect 命令，本需求不应改成无 Connect |

## 变更范围清单

机器可读范围见同目录 `change-scope.json`。预计业务修改为 `src/app.rs`、`src/model/workspace.rs`、`src/commands.rs`、`src/input/keymap.rs`、`src/model/omni.rs`、`src/ui/mod.rs`；条件性文件只在相应步骤确有需要时修改。测试主要文件列于各单元；由于取消默认 Console 可能影响分散在集成测试中的准备夹具，尚不能精确列出所有被全量回归暴露的测试文件，因此清单使用最小具体目录 `tests` 覆盖它们，不扩大到仓库根目录。App 和 keymap 内部单元测试已包含在对应源码范围内。

无预计业务文件新增、删除或重命名；允许在 `tests` 中增加聚焦的生命周期测试文件。未提交的本地按需连接计划没有被实现或依赖，不列入修改范围；本次也不额外创建 docs/plans 文档。

## 验收与验证的约束等级

1. **用户需求必需验收**：连接相关操作不自动新增 Console；用户在 Consoles 手动创建/管理；已有文档不因本次变更丢失。各单元的生命周期、入口和保存恢复断言是证明需求完成的必要证据。
2. **项目既有 Rust 门禁**：沿用 CI 的 fmt、clippy 和 all-targets/all-features 测试命令，功能齐备后执行，具体命令见单元 3。现有跨平台及外部数据库 CI 作业仍由 CI 环境执行，本任务不新增本地启动所有数据库服务的要求。
3. **计划选择的定向验证**：各单元列出的 reducer、临时目录 round-trip 和 TestBackend 检查用于定位改动影响；测试命令运行顺序可按实际修改调整，但不得以同构实现测试取代业务结果断言。
4. **补充建议验证**：人工终端/PTY 体验、不同终端尺寸现场操作与真实外部数据库手动连接均属于补充，不作为新设强制门禁。环境不可用时记录限制，由 Luna 收尾审查是否需要补充证据；最多一次针对性修复重试。

每个单元复核实际 diff 是否仅覆盖其验收目标，确认未复制本地未跟踪计划、未改变 SQL 自动连接策略。所有验证记录真实退出结果，不把本计划的预期结果当成已执行结果。

## 单元 1：连接空工作区 → 手动创建一个 → 保存恢复

**修改文件**
- `src/app.rs`
- `src/model/workspace.rs`（仅添加空工作区构造能力时）
- `tests/consoles_lifecycle.rs`
- `tests/console_manager_input.rs`
- `tests/connection_switch.rs`
- `tests/workspace_persistence.rs`

### 步骤 1：建立核心生命周期回归

使用已有 connection-switch 测试中的 server 信息和成功事件构造方式，避免通过实际网络建立 session。建议增加以下行为用例：

1. `connecting_profile_without_documents_keeps_workspace_empty`：App::new 含一个 profile，RequestConnect 后投递匹配 generation 的 ConnectionSucceeded；断言 connection 在线、active_workspace_profile 正确，但 sql_editors、SQL tabs 和 snapshot SQL 均为空。
2. `reusing_connected_profile_does_not_create_console`：在上述状态再次 RequestConnect；断言没有新 Connect、文档仍为空，覆盖 17368 行热连接激活路径。
3. `console_manager_creates_exactly_one_document_after_connect`：连接成功后 OpenSqlEditorList → SqlEditorListCreate；断言 record/tab 均恰好一个且 UUID 相同、目标正确，命令包含 PersistWorkspace。编辑文本并取 snapshot。
4. 将该 snapshot 经临时 WorkspaceStore save/load 后交给新 App restore；断言文档 UUID、文本、目标和打开状态一致，无额外 console。
5. 在 profile 管理相关现有测试中补断言：仅保存、保存并连接完成均不增加文档；不要把“仅保存”误写成当前必然失败的用例，它本来就不直接创建文档。

定向执行：

```sh
cargo +1.94.0 test --test consoles_lifecycle --test console_manager_input --test connection_switch --test workspace_persistence
```

旧代码预期在“连接后为空”断言失败，而不是因编译或环境错误失败。记录真实结果。

### 步骤 2：实现真正空工作区

推荐在 `ConnectionWorkspace` 增加 `Default` derive；其字段全部支持默认值：

```rust
#[derive(Clone, Debug, Default)]
pub struct ConnectionWorkspace {
    pub tabs: Vec<WorkspaceTab>,
    pub sql_editors: Vec<ConsoleRecord>,
    pub sql: Vec<(Uuid, String)>,
    pub active_tab_id: Option<Uuid>,
}
```

删除 `empty_workspace_for` 的默认文档构造逻辑，两个调用点统一使用 `ConnectionWorkspace::default()` 或 `unwrap_or_default()`。若 `activate_profile_workspace` 不再需要 target，移除该参数并同步 ConnectionSucceeded 和热连接复用两个调用点；其他后续仍使用 target 的代码继续保留其变量。

不能跳过整个 activate_profile_workspace：它仍负责旧工作区快照、恢复已有文档、更新 active_workspace_profile、焦点归一化。切换到空的新 profile 时保留其他 profile 已打开的文档，不清空全局 tabs，也不重绑它们。

### 步骤 3：完成删除后不补建闭环

增加用例：在管理中创建文档并写 SQL，关闭 tab 后从管理中重开仍是同一 UUID；删除最后文档后再次连接/切换回来仍为零；保存并恢复空快照也为零。断言 SQL snapshot 无已删除记录，使用现有删除文件命令链验证文件清理职责。

### 步骤 4：修正相关夹具并定向验证

原本为了测试 SQL 而假设 ConnectionSucceeded 自动提供 Console 的夹具，改为显式执行管理创建动作。不要在所有连接成功 helper 中统一偷偷建文档，否则新行为无法被覆盖。

重跑上述定向目标；全部通过后记录首个业务闭环完成。建议逻辑提交标题：`fix(console): keep connection workspaces empty until explicit creation`。仅后续提交阶段按工作流授权提交。

**验收**：冷/热连接均零新增，手动一次恰好一个，SQL 可保存恢复，删除后不复活。

## 单元 2：启动和管理快捷入口一致

**修改文件**
- `src/app.rs`
- `src/commands.rs`（命令描述或分发需要调整时）
- `src/input/keymap.rs`（快捷键分发/测试）
- `src/model/omni.rs`（仅移除确实不再使用的创建专用步骤时）
- `src/ui/mod.rs`
- `tests/startup_profiles.rs`
- `tests/console_manager_input.rs`
- `tests/omni_flows.rs`
- `tests/omni_navigation.rs`
- `tests/ui_render.rs`

### 步骤 1：补零 profile 和入口回归

零 profile App 启动后 tabs、sql_editors 均为空；F6 打开空 Consoles，按 a 创建恰好一个 target=None 的文档。管理取消、搜索取消不增加文档。

Omni New Console 选中后打开 Consoles 而不创建文档、不发连接请求；接着用户按 a 才创建。Leader x 在有活动 Console 时打开管理的对应条目/删除确认；取消保留文档，确认后删除同一 UUID，并继续遵守事务退出保护。

### 步骤 2：去掉启动自动注入

`with_profiles` 中 tabs 和 sql_editors 从空集合初始化；删除只为默认 Console 使用的 editor.open_console/open_read_only 调用。清理 placeholder_console_id 字段、初始化、引用及 remove_placeholder_console；逐一检查引用属于占位语义，不删除持久化或恢复普通文档的逻辑。

如果初始 editor 不再需要 mut，按编译器提示去掉。初始焦点设置为 Explorer 或调用已有归一化机制，确保没有 Console 时不残留可编辑焦点。

### 步骤 3：统一用户入口到管理流程

在 `confirm_omni_item` 的 Root NewConsole 专用分支完成现有 origin 恢复/挂起处理后，进入 OpenSqlEditorList；来源 target 保存在 console_manager_origin_target 中，避免进入管理后丢失 Explorer 或 Relation 的明确来源。用户在管理中执行创建，而不是 Root 分支直接新增。

检查其他 UserIntent::NewConsole 分发入口，同样导向管理。NewConsole/NewConsoleNamed 是否保留为内部动作由现有调用需要决定；不得新增绕过用户管理流程的公开按键路径。

RequestDeleteActiveConsole 复用管理列表选中活动 UUID 后的 SqlEditorListDeleteRequest，不能复制一套删除实现。保留活动事务的 defer/确认行为，让取消或事务失败不删除文档。关闭 tab 与跳转已有文档保留原交互。

### 步骤 4：空状态引导与界面验证

连接已在线且没有 tab 时使用现有 NO OPEN TABS，并明确指向 F6 Consoles。零连接启动可以保留创建连接引导，同时提供 Consoles 入口提示；不在本任务实现未绑定 Console 的完整离线编辑能力改造。

使用现有 TestBackend 验证空启动、连接成功后的空工作区、空管理列表和创建后的 tab。若未绑定 Console 在当前 UI 中仍使用既有 NO CONNECTIONS 空状态，记录为现有离线编辑需求范围；本次至少保证管理创建/保存文档动作可用，不借此扩大到独立离线能力。

```sh
cargo +1.94.0 test --test startup_profiles --test console_manager_input --test omni_flows --test omni_navigation --test ui_render
```

**验收**：不存在启动自动生成文档；管理快捷入口不直接改文档；管理中的操作与取消、事务保护正常。

## 单元 3：恢复兼容与全仓回归

**主要测试文件**
- `tests/global_workspace.rs`
- `tests/workspace_persistence.rs`
- `tests/workspace_tabs.rs`
- `tests/consoles_lifecycle.rs`
- `tests/connection_switch.rs`
- `src/app.rs` 内部 tests
- 全量测试发现依赖自动文档的其他测试文件，只改确有需要的准备逻辑。

### 步骤 1：验证历史文档和多工作区

覆盖历史保存的 `console` 名称记录；它必须原 UUID/SQL 恢复。覆盖一个 profile 有打开文档、另一个 profile 无文档；切换往返无新增、不丢文本。覆盖只有关闭文档的 profile，连接不另建默认文档。覆盖已有 relation/Redis tab 的工作区，不给它们追加 SQL console。

使用实际 WorkspaceStore 的临时目录完成空/非空 round-trip。格式保持当前版本，无需 migration 或批量删除历史 SQL 文件。

### 步骤 2：定向回归

```sh
cargo +1.94.0 test --test global_workspace --test workspace_persistence --test workspace_tabs --test consoles_lifecycle --test connection_switch
```

只在这里涉及的业务断言失败时继续修改；不得以移除原有 SQL、事务或会话隔离断言方式让用例通过。

### 步骤 3：功能齐备后完整检查

按仓库 CI 的 Rust 检查执行一次：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

预期命令退出 0。若失败，修复相关代码/夹具后只重跑受影响检查，最后保证记录对应最终代码版本。数据库服务缺失时记录测试跳过或失败的实际范围；不能把有环境门槛的全量测试退出 0 等同于真实数据库全部执行。

人工/PTY 检查属于补充，核心验收以 reducer、持久化和渲染测试为准。环境受限最多一次针对性修复重试，由 Luna 收尾审查决定补充证据或记录限制，不无限循环。

## 审查清单与交付

- [ ] 所有非恢复的 ConsoleTab::new 生产调用都属于显式管理创建，没有连接或启动兜底构造。
- [ ] ConnectionSucceeded 和在线 session 复用均已覆盖。
- [ ] 手动创建恰好新增一个 UUID，tabs/records/SQL snapshot 一致。
- [ ] 关闭、删除、重连、切换及重启均不补建。
- [ ] 历史文档与其他 tab 保留；无自动删除历史 console 的逻辑。
- [ ] Omni 专用 NewConsole 分支及 Leader x 行为有输入级测试。
- [ ] SQL 行为测试显式建文档，而无 Console 的连接测试保持真正空状态。
- [ ] 本地未跟踪计划未被提交或作为未实现行为使用。
- [ ] validation.md 记录实际命令、退出结果、版本/工作区状态及环境限制。

Luna 按单元 1 → 单元 2 → 单元 3 连续推进；普通实现问题自行解决。实施完成后由 Luna 审查、纠偏并按插件后续授权执行提交合并；无需用户反复 resume。
