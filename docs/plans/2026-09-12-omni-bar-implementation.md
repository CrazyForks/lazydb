# Omni Bar Implementation Plan

> 执行说明：按依赖顺序逐项实施，每项先建立可观察的行为测试，再实现并定向验证。计划中的新增类型、动作、测试文件与测试名均为拟定接口，不表示已经实现。仅在用户明确要求时执行 Git 提交。执行技能以当时实际可用工具为准，不依赖当前环境未提供的技能。

**Goal:** 提供任意交互状态下可唤起的统一任务入口，搜索并打开表、连接、Console 和命令，支持跨连接导航、参数补全，以及中断后的工作现场恢复。

**Architecture:** 保留 `Action → App::update → Command → Runtime` 边界，在现有 Overlay 之上增加独立 Omni 交互层。Help、键盘和 Omni 共享语义命令入口；对象操作携带稳定身份；导航、异步搜索和暂存编辑会话分别管理生命周期。

**Tech Stack:** Rust 2024、Ratatui、Crossterm、Tokio、Modalkit、现有 TextInput、CatalogSearchRequest、ExecutionTarget、UUID 与 workspace persistence；第一版不增加生产依赖。

---

## 1. 交付边界与关联计划

### 1.1 首版必须交付

- 可配置的 `F2` 全局唤起；编辑器、单元格、WHERE、现有弹窗和忙碌界面均可打开。
- 混合搜索命令、连接、表/视图、打开的 Tab、打开/关闭的 Console。
- `>` 命令筛选、`@` 连接范围选择，普通文本支持多字段匹配。
- 明确身份的打开表、查看 DDL、定位 Explorer、打开/新建 Console。
- 参数选择在一个 Omni 会话内完成；Esc 返回上一步，到根页面再关闭。
- 跨连接动作保留最终目标；事务处理、连接、身份解析完成后准确续接。
- 取消 Omni 不改变底层现场；跳转后能返回原工作位置；编辑表单可以暂存并恢复。
- 所有结果和操作支持 owner/generation 校验；失效对象、失败、取消有可解释反馈。
- MRU 与返回历史先限于进程内，避免为排名系统增加持久化迁移。

### 1.2 当前连接架构下的明确限制

Omni 可在查询运行中打开，但不能绕过已有连接切换限制。跨连接切换被正在执行的 SQL、Relation 加载、Catalog Apply 或未解决的 Relation 事务阻止时，显示原因，不偷偷排队自动执行。Console 事务沿用现有确认流程。打开 Omni 本身不取消后台操作。

多连接同时执行、搜索全部离线连接、持久化编辑表单、动态插件注册、任意快捷键宏录制不属于本计划首版。

### 1.3 与已有多连接计划的衔接

仓库已有 `docs/plans/2026-09-12-multi-connection-consoles.md`，目前它是计划文件，不能假定其中接口已实现。

- Omni 的命令目录、顶层输入层、本地检索、多步骤选择可先实施。
- 若两项安排在同一开发周期，推荐先完成该计划的全局文档/编辑器生命周期及显式目标路由，再实施本计划 T04、T09。届时复用全局 Console/Tab 集合，不再增加按 profile 缓存 EditorWorkspace 的过渡结构。
- 若 Omni 独立先交付，执行本计划 T04 的当前架构路径，保留单活动连接策略；T09 通过当前连接切换流程执行。
- 两条路线只选择一条落地，不在生产代码中并存两套 Workspace 后端、功能开关或双重连接策略。
- 多连接改造完成后，导航仍保留“解析目标→确保可用会话→打开目标”的结构，但无需为了查看另一个连接而退出旧 Workspace；已存在的单连接限制测试改为多会话隔离测试。
- `Command` 实际定义在 `src/action.rs:1084`，不存在 `src/command.rs`；相关修改均落在前者。

## 2. 已确认的代码落点

| 代码 | 当前职责 | 本计划处理 |
| --- | --- | --- |
| `src/input/keymap.rs` / `Keymap::map` | 多个 Overlay、busy 和编辑器输入提前分流 | 唤起判断置于模态提前返回前；Omni 输入置于底层输入前 |
| `src/input/keymap.rs` / `map_paste` | 按现有 Overlay/Editor 分流粘贴 | 优先路由给 Omni |
| `src/input/mouse.rs`、`src/ui/mod.rs` | 命中区域、光标、Overlay 绘制 | 顶层命中屏障、Omni 光标、避免穿透 |
| `src/help.rs` | SHORTCUT_CATALOG、HelpState、可执行性判断 | 保留帮助条目和上下文说明，关联去重后的语义命令 |
| `src/app.rs` / `execute_help_shortcut` | Help 到 Action，部分模拟编辑器按键 | 抽出共享语义分发，移除迁移命令的按键模拟 |
| `src/app.rs` / `open_selected_relation` | 从 Explorer 选择提取对象，再去重打开 | 提取按 CatalogId/descriptor 打开的公共内部入口 |
| `src/app.rs` / `open_sql_editor` | 从当前 ConsoleRecord 打开 | 提供显式 profile/console ID 的激活或重开入口 |
| `src/app.rs` / `request_connection` | Workspace exit check、事务延迟、目标连接 | 返回可分类的导航进度，保留原目标 |
| `src/model/transaction.rs` | DeferredIntentQueue | 与导航请求 ID 关联，不另建事务队列 |
| `src/app.rs` / take/install_workspace | 文本缓存、重建 EditorWorkspace | T04 保留运行态会话或复用全局文档架构 |
| `src/editor/mod.rs` | sessions 与 workspace 级 prompt/keys/substitute | 明确暂态状态归属，防止跨 Console 输入串线 |
| `src/db/catalog.rs` | 搜索请求、返回和范围校验 | 复用协议，不复制各驱动 SQL |
| `src/runtime.rs` / search_catalog | 单搜索任务、150ms debounce | 按 consumer/session 路由与取消 |
| `src/model/sql_editor_list.rs` | UUID 选择和文本输入 | 复用规则，管理器不充当 Omni 状态容器 |

实现前阅读 `CONTRIBUTING.md`、`docs/architecture.md`、`docs/keybindings.md`。现有架构文档与源码不一致处以实际调用路径为准，并在 T14 同步。

## 3. 行为契约

### 3.1 输入与多步骤

- `F2` 首次打开 Omni；已打开时取消整个 Omni 会话并返回原界面；忽略唤起键 repeat，避免长按闪开闪关。
- `Esc` 在参数/对象操作页回退一页；根页关闭。`Ctrl-c` 在 Omni 内取消整个会话，不触发应用 Quit；底层 Ctrl-c 契约保持原有行为并更新文档说明例外。
- `Enter` 执行当前稳定 ID 对应结果的默认行为；`Tab` 进入对象操作页，页脚展示其具体用途；无可展开对象时不穿透到底层。
- `Up/Down` 改变结果；普通 j/k 输入查询文本；Home/End、左右、删除、撤销、粘贴复用 TextInput。
- `>` 只在查询首部进入命令模式；`@` 提供连接候选，确认后存 UUID scope chip。名称带空格、重复名不能按分词结果直接推断唯一 Profile。
- 搜索框内输入、方向键高亮、打开对象操作页都不连接数据库，不改变当前 Tab/Explorer 选择。
- Omni 存在时鼠标、滚轮、粘贴和应用级输入序列都由顶层处理；窗口 Resize 和后台结果仍正常更新。

### 3.2 目标与语义

- 表默认打开 Data，DDL 是显式次级行为；同一 RelationKey 去重。
- Console 按 UUID 激活/重开，不能因重名打开另一个文档；首版延续当前 default Console 等生命周期规则，除非多连接计划已正式替换。
- 对象名称、连接限定名、database/schema、打开/离线/缓存状态分字段显示。
- 上下文命令绑定唤起来源。执行前重新验证目标、能力及修订，不以 Omni 的焦点当命令上下文。
- “执行当前语句”在用户确认命令时从来源 Console 建立既有 ExecutionDraft；之后的执行确认仍使用不可变 SQL 快照。
- 浏览对象时只提供已接入真实执行入口的行为；不展示尚未实现的占位命令。

### 3.3 底层现场与恢复

| 场景 | 取消 Omni | 成功开始新任务 |
| --- | --- | --- |
| Editor/WHERE/单元格草稿 | 底层状态完全保留 | 归属原 Tab，可返回继续 |
| Help/列表/只读详情 | 返回仍有效的原界面 | 正常关闭/释放原浏览会话 |
| Profile/Catalog 编辑草稿 | 保留文本、选择、页面 | 按 owner 暂存，之后从“恢复编辑…”恢复 |
| 执行/事务/替换确认 | 返回当前有效确认 | 按语义取消或解决；不能把确认框作为普通草稿挂起 |
| Apply/连接/查询进行中 | 后台继续；接受合法完成事件 | 按真实冲突限制导航；不复用过期确认 |

“取消后原样”不表示回滚后台结果：请求可能已经完成，返回其最新合法界面；不能恢复已经失效的旧 Overlay 快照。

## 4. 数据结构与模块边界

以下是实现契约，而非要求预先写完整框架；只有首次调用时才加入对应变体。

| 新概念 | 必须保存的内容 | 所属位置 |
| --- | --- | --- |
| `CommandId` | 稳定语义 ID，别名快捷键合并 | `src/commands.rs` |
| `CommandSpec` | ID、标题、别名、类别、参数要求、keybinding 名称 | `src/commands.rs` |
| `CommandContext` | 来源 profile/target、Tab UUID、focus、选中对象、所需修订/模式 | `src/commands.rs` |
| `Availability` | Ready / NeedsArguments / Disabled(reason) | `src/commands.rs` |
| `UserIntent` | OpenRelation / OpenConsole / NewConsole / RunCommand / ResumeSession | `src/commands.rs` |
| `OmniState` | session_id、query generation、TextInput、scope、step stack、selected_id、provider 状态、origin | `src/model/omni.rs` |
| `OmniItemId` | Command / Profile / Console / Catalog / Tab / SuspendedSession 的稳定身份 | `src/model/omni.rs` |
| `OmniItem` | ID、主标题、副标题、类别、状态、默认行为、match 信息 | `src/model/omni.rs` |
| `PendingNavigation` | request_id、最终 intent、origin、阶段、预期连接/对象解析身份 | `src/model/navigation.rs` |
| `WorkspaceLocation` | profile/target、Tab UUID、focus、可选对象引用 | `src/model/navigation.rs` |
| `SuspendedInteraction` | session_id、owner、typed draft、修订、原位置 | `src/model/interaction.rs` |
| `SearchOwner` | Explorer 或 Omni(session_id) | `src/model/catalog_search.rs` |

新增 reducer 拆到 `src/app/commands.rs`、`src/app/omni.rs`、`src/app/navigation.rs`、`src/app/interaction.rs`，由现有 `src/app.rs` 声明子模块，继续使用 `impl App`。不要同时把全部 App 字段搬家。

`src/action.rs` 保留输入 Action 和副作用 Command；`src/commands.rs` 是用户语义目录，二者不可混淆。Omni 模型不持有 `&App`、trait object closure 或 Runtime 句柄。

## 5. 阶段、依赖与发布门槛

```text
T01 基线与契约
  → T02 语义命令
  → T03 显式对象入口
  → T04 编辑现场归属
  → T05 Omni 状态与检索模型
  → T06 顶层输入/渲染
  → T07 本地 providers
  → T08 多步骤与对象操作
  → T09 导航 continuation
  → T10 异步搜索隔离
  → T11 暂存编辑会话
  → T12 命令覆盖与 Help 一致性
  → T13 历史、排名与性能
  → T14 回归、文档与验收
```

T04 与关联多连接计划共享架构检查点。其余任务按上述顺序可单人执行，不要求子代理或并行修改共享文件。

- M1 / T01–T04：命令和明确目标可独立调用，编辑现场不因切换丢失。
- M2 / T05–T08：本地数据驱动的完整 Omni 交互闭环。
- M3 / T09–T11：跨连接、异步、表单中断恢复闭环。
- M4 / T12–T14：统一入口覆盖和首版验收。只有到 M4 才宣传完整“任意过程唤起并继续另一件事”。

## 6. 详细任务

每项是一个可评审单元；内部步骤按顺序推进。测试名在实现时使用下列名称，便于定向复现；先补行为断言再运行确认旧实现失败，避免仅因测试文件不存在就认定完成红灯验证。每项完成后执行列出的检查并记录结果。

### T01：建立基线与状态矩阵

**读取：** `src/input/keymap.rs`、`src/input/mouse.rs`、`src/app.rs`、`src/editor/mod.rs`、`src/model/workspace.rs`、`src/ui/mod.rs`。

**修改：** 本计划的实施记录；后续实现任务中的测试 fixture 按需落地。

1. 运行 `cargo test --test keymap --test mouse --test ui_render --test workspace_tabs --test connection_switch --test transaction_reducer --test catalog_reducer`。
2. 记录既有失败、环境缺失和被跳过的数据库用例；后续不能将其误记为 Omni 回归。
3. 列举所有 Overlay 变体、Editor 输入模式、关系编辑模式和 Runtime 完成回调写 Overlay 的入口。
4. 为每种 Overlay 标记 Browse / SuspendableDraft / Confirmation / Busy，并记录 owner 字段和释放动作。
5. 核对多连接计划实施状态，选择 T04 唯一实现路线，记录决定。
6. 确认默认 F2 没有映射冲突，记录用户重映射与模态专用键冲突的校验需求。

**完成标准：** 每个当前模态均能在矩阵中找到其唤起、取消和导航策略；后续不会靠兜底 `overlay = None` 处理所有情况。

### T02：抽取语义命令与来源上下文

**新增：** `src/commands.rs`、`src/app/commands.rs`、`tests/commands.rs`。

**修改：** `src/lib.rs`、`src/app.rs`、`src/action.rs`、`src/help.rs`、`src/editor/mod.rs`。

1. 新增 `aliases_share_one_semantic_command`：前后 Tab 的不同 Help 别名归一，命令枚举不重复。
2. 新增 `command_uses_origin_console`：从 A 捕获 context，改变当前焦点后执行仍作用于 A，或在上下文失效时明确拒绝。
3. 建立 CommandId/Spec/Context/Availability；目录只含第一批可落地入口：打开/新建 Console、打开表/DDL、格式化、当前语句/Buffer 执行、打开 Help/通知/Dashboard、返回位置。
4. 将 `execute_help_shortcut` 中迁移的语义分支提取为共享分发，保留 Help 特有选择验证。
5. 格式化通过 EditorWorkspace 明确 session ID 的方法执行；检查现有 effects 消费顺序，不注入 `Space f`。
6. 命令执行捕获返回值：Completed / NeedsArguments / Pending / Rejected(reason)，禁止把空 `Vec<Command>` 当通用成功。
7. 不把所有底层 Action 和 Vim 字符动作转换成命令；可展示但不可语义执行的帮助项仍保持只读说明。

**验证：** `cargo test --test commands`；`cargo test --lib help`；`cargo test --test keymap --test sql_format --test sql_execution`。

**完成标准：** Help 与语义分发执行一致，来源目标不受 Omni 焦点影响；命令拒绝有明确原因。

### T03：提取显式对象打开和 Console 生命周期入口

**修改：** `src/app.rs`、`src/app/commands.rs`、`src/commands.rs`、`src/action.rs`。

**测试：** `tests/relation_tabs.rs`、`tests/workspace_tabs.rs`、`tests/commands.rs`。

1. 补 `open_relation_does_not_require_explorer_selection`，构造真实 CatalogEntry，令 Explorer 选择另一个对象。
2. 从 `open_selected_relation` 提取 CatalogId→descriptor 校验及 descriptor→去重打开两步；原方法仅负责取选中对象。
3. 用 RelationKey 查重，激活时保留已有编辑状态，只切换明确要求的 Data/DDL 视图。
4. 补 `open_console_by_id_reuses_existing_tab`、`reopen_console_keeps_sql`，实现激活/重开统一入口，不直接重复调用会 append Tab 的底层函数。
5. 新建 Console 接收显式目标和可选名称，复用现有命名、重复名、持久化及生命周期政策。
6. Catalog 搜索命中先验证并合并真实对象/祖先或构造合法 descriptor，不伪造 Explorer 选择；定位 Explorer 是独立行为。
7. 跨 profile 尚未激活时返回 NeedsNavigation，不在此层临时切连接。

**验证：** `cargo test --test commands --test relation_tabs --test workspace_tabs --test workspace_persistence`。

**完成标准：** 通过明确 UUID/CatalogId 即可打开目标；不存在依赖列表 index 的延迟执行。

### T04：保留编辑现场并明确编辑器暂态归属

**修改：** `src/app.rs`、`src/editor/mod.rs`、`src/editor/tests.rs`、`src/model/workspace.rs`。

**按当前架构需要新增：** `src/app/workspace_session.rs`，保存不可克隆的运行态 Workspace 容器。

**测试：** `tests/workspace_tabs.rs`、`tests/connection_switch.rs`、`tests/workspace_persistence.rs`。

1. 补 A→B→A 测试，验证 SQL、光标、选择、Insert/Visual 模式、撤销链、滚动位置不丢失。
2. 单独测试同一 Workspace 两个 Console 之间的 `:` prompt、搜索 prompt、未完成 Vim operator、替换确认；这些字段目前部分位于 EditorWorkspace 全局，不能假定按 Console 隔离。
3. 将需要跨 Tab 恢复的编辑暂态绑定 session UUID；应用级 leader pending 在唤起 Omni 时清空，编辑器内部未完成状态在取消 Omni 时保持，在跳转时保存给来源 session。
4. 当前架构路径：增加 App 私有运行态容器，移动 EditorWorkspace 所有权与焦点；避免把不可 Debug/Clone 的 EditorWorkspace 塞入公开且派生 Clone/Debug 的模型。take/install 使用所有权移动而非文本重建。
5. 多连接路径：复用全局 EditorWorkspace 与文档存储，删除本任务的按 profile 容器步骤，但仍完成步骤 2–3 的 prompt/keys owner 隔离。
6. 保存到磁盘仍投影为现有 WorkspaceSnapshot；恢复磁盘文档时才从文本创建会话。不持久化 Modalkit 内部结构，不借此修改文件格式。
7. 删除/关闭文档时释放其会话状态、失效 prompt 和关联只读 session；effects 在原 owner 上处理，不得切换后重放给新 Console。

**验证：** `cargo test --lib editor::tests`；`cargo test --test workspace_tabs --test connection_switch --test workspace_persistence`。

**完成标准：** 同进程导航保持编辑现场；跨 Tab 不串 prompt/操作符；落盘格式兼容。

### T05：Omni 状态、查询解析和稳定结果身份

**新增：** `src/model/omni.rs`、`tests/omni_state.rs`。

**修改：** `src/model/mod.rs`。

1. 定义 OmniState、Step、Scope、ItemId、ProviderState，查询框复用 TextInput。
2. 实现 Root / ObjectActions / PickConnection / PickTarget / NameConsole 步骤；每步保存查询、selected_id 和滚动，pop 后恢复。
3. 补普通文本、首部 `>`、`@` 候选、连接名空格、同名连接消歧和取消 chip 的解析测试。
4. 查询匹配使用小写归一化的多字段 token；排序按精确名称、前缀、包含、多 token 覆盖、有限子序列评分分层，再以当前范围/MRU/稳定 ID tie-break。
5. 同一 CatalogId 的已打开 Tab/缓存/远端命中合并为一个结果，保留打开状态；Console 按 UUID 合并；其他 Tab 类型保持自己的 ID。
6. 新查询递增 generation 并重设首项；同查询异步合并保持 selected_id；若用户已主动移动且目标被删，清空选择而非让 Enter 意外执行下一项。
7. 补多字节文本、零结果、删除当前结果、异步重新排序测试。匹配偏移不能直接用于切 UTF-8 字节。

**验证：** `cargo test --test omni_state`。

**完成标准：** 模型无数据库/终端副作用；排序确定；异步刷新不改变用户指向的实体。

### T06：顶层唤起、输入隔离与最小 UI

**新增：** `src/app/omni.rs`、`src/ui/omni.rs`、`tests/omni_input.rs`。

**修改：** `src/app.rs`、`src/action.rs`、`src/input/keymap.rs`、`src/input/mouse.rs`、`src/ui/mod.rs`、`src/ui/text_selection.rs`、`src/config.rs`、`config/default.toml`、`src/help.rs`、`docs/keybindings.md`、`docs/configuration.md`。

1. 在 App 增加 `omni: Option<OmniState>`，Omni action 在无活动 Console 的 reducer guard 下也必须可达。
2. 配置 `omni = ["F2"]`，加入 supported command 和冲突检测。其上下文覆盖所有 Pane/Overlay；与同键的上下文命令也应识别冲突，不沿用当前仅同分组对比逻辑。
3. Keymap 在过滤 Release/规范化事件之后、任何 modal/busy 提前返回之前识别唤起；清理应用 pending sequence。打开状态按 3.1 路由并拦截所有底层输入。
4. map_paste 和 Mouse 优先 Omni；文字粘贴按单行输入归一化；按下前已存在的拖拽/按键前缀不能在关闭后继续影响底层。
5. 绘制背景、现有 Overlay、Omni；清理底层可交互 hit regions 或添加最高优先级屏障；使用 Omni 输入光标和既有主题/icon/text sanitation 能力。
6. 首次提供 80×24 可用布局，小尺寸折叠副标题和结果数；更窄时仍可输入/Esc，不访问越界坐标。只渲染可见结果。
7. 补参数化输入测试：Insert、Visual、prompt、WHERE、EditCell、ProfileForm、CatalogForm、busy、ExecutionConfirm、Update、零 Tab。
8. 补 `cancel_omni_preserves_underlying_state`、`omni_blocks_paste_mouse_and_quit_fallthrough`、`open_repeat_does_not_toggle`；后台完成改变原 Overlay 后取消应返回最新状态。
9. F2/例外 Ctrl-c/Tab 语义与配置改动同一任务更新文档，不等最终阶段。

**验证：** `cargo test --test omni_input --test keymap --test mouse --test ui_render --test docs`；`cargo test --lib config`。

**完成标准：** 每个已存在界面均可唤起，无输入穿透，无底层模式切换；最小终端可退出。

### T07：本地结果源与默认行为

**新增：** `src/omni.rs`（固定 provider 适配与结果聚合）、`tests/omni_providers.rs`。

**修改：** `src/lib.rs`、`src/app/omni.rs`、`src/app.rs`、`src/ui/omni.rs`。

1. 提供只读 App 访问器，统一枚举当前及缓存 Workspace 的 Console/Tab；多连接架构下读取全局集合。
2. 从命令目录、当前项目可用连接、Console、已加载 Catalog 生成轻量搜索记录；不复制 SQL 内容或整个 Catalog 树。
3. 按 profile 可见性和 CatalogScope 过滤；不能把 App.profiles 的所有记录无条件暴露为可访问搜索对象。
4. 命令只呈现 Ready/NeedsArguments，明确查询命中时可显示 Disabled(reason)。连接重名显示分组限定名，必要时附短 UUID。
5. 补表名/Console 同名、跨连接同名、关闭 Console、无连接、scope 修改、profile 删除测试。
6. Root Enter 分发 T02/T03 intent；跨连接保留待完成目标给 T09。连接候选 Enter 形成 scope chip，不直接切换工作区。
7. 空查询先给最近位置/恢复会话/最近对象/新建入口；恢复会话 provider 在 T11 接入前不展示占位行。

**验证：** `cargo test --test omni_providers --test omni_state --test commands --test catalog_scope`。

**完成标准：** 输入已有对象名后立即返回本地结果，浏览和高亮不产生 Connect/Query Command。

### T08：参数补全和对象操作页

**修改：** `src/model/omni.rs`、`src/app/omni.rs`、`src/commands.rs`、`src/ui/omni.rs`。

**新增测试：** `tests/omni_flows.rs`。

1. 表操作页接入打开数据、查看 DDL、定位 Explorer、基于对象新建 Console，默认目标来自所选对象。
2. Console 操作页接入打开、重命名、删除的既有语义入口；删除沿用确认，不通过管理器选中状态或伪造按键执行。
3. 新建向导按需要选择连接和 database/schema；缺省目标明确显示；多连接计划先落地时复用其默认挂载/未绑定规则，不另订冲突规则。
4. 名称接受现有默认命名，拒绝空/重复输入时留在当前步，保留错误和输入。
5. 选择结果后保留实体 ID，后续步骤显示最新标签；期间 profile 删除/目标失效时回到可修正步骤。
6. 每一步 Esc 恢复上一步 query/selection；整会话取消不创建 Console、不修改已有绑定。
7. 需联网发现尚未缓存目标时展示显式“连接并加载目标”，由 T09 接入后才成为可执行项；不将空候选当数据库无对象。

**验证：** `cargo test --test omni_flows --test execution_target --test workspace_tabs`。

**完成标准：** 新建 Console 从命令到创建为单一可回退流程，动作只在最终确认发生。

### T09：可取消、可校验的跨连接导航

**新增：** `src/model/navigation.rs`、`src/app/navigation.rs`、`tests/omni_navigation.rs`。

**修改：** `src/model/mod.rs`、`src/app.rs`、`src/action.rs`、`src/model/transaction.rs`、`src/commands.rs`。

1. 建立 `PendingNavigation` 阶段：Validating → AwaitingExitDecision → AwaitingConnection → ResolvingTarget → Opening → Completed，任何阶段可 Failed/Cancelled。
2. 把 request_connection 结果映射为 Proceed/Pending/Blocked(reason)，不根据通知文本或 Command 是否为空推断进度。
3. 新请求分配 request_id；从入口一直保存原始 UserIntent、profile/ExecutionTarget 和 Catalog/Console 身份。当前单连接架构同时只允许一个前台导航。
4. Console 事务退出关联 navigation ID，保留现有 prompt ownership/generation；取消、失败不重放目标，确认成功后继续同一请求。
5. 对已有 SQL 执行/事务确认采取显式冲突策略：Omni 可搜索，但冲突命令 Disabled；需要取消待执行操作时调用其语义 cancel 并使快照失效，不能直接清 Overlay。
6. Connection 成功回调先匹配目标、attempt/generation、导航 ID；只唤醒匹配 continuation。不递归调用会重新从 active_tab 读取目标的命令。
7. 重连或对象缓存过期时使用既有 Relation identity resolver 与 scope 校验。找不到对象则失败，不按模糊相似名称改开另一张表。
8. 导航开始后的等待页可重新唤起 Omni；用户替换导航使旧 continuation 失效。只取消该导航独占的请求；共享连接尝试是否取消遵守原 owner 规则。
9. 连接失败保留旧有效连接/文档；若目标连接已成功但对象解析失败，保留新连接的真实状态与失败目标信息，提供显式返回，不自动发起反向重连。
10. 补事务确认后打开指定表、取消后不打开、A→B→C 乱序回调、profile 删除、目标 database 不同、对象重建、连接失败和来源 Tab 关闭测试。

**验证：** `cargo test --test omni_navigation --test connection_switch --test transaction_reducer --test quit_transaction_review --test relation_tabs --test execution_target`。

**完成标准：** 最终打开对象始终等于用户确认目标；旧回调不能导航或抢焦点；阻塞时原始查询和选择可继续修改。

### T10：隔离 Omni/Explorer 异步搜索

**新增：** `src/model/catalog_search.rs`、`tests/omni_search.rs`。

**修改：** `src/model/mod.rs`、`src/action.rs`、`src/runtime.rs`、`src/app.rs`、`src/app/omni.rs`、`src/model/omni.rs`；`src/db/catalog.rs` 仅在协议确需扩展时修改。

1. 在应用层包裹 SearchOwner 与 CatalogSearchRequest；数据库驱动继续只收现有 request，consumer 信息不下沉为各驱动参数。
2. Runtime 搜索任务由单槽改为按 owner/session 索引；新 query 仅取消同 owner 的旧任务，完成后清理对应 handle。
3. 将裸 CancelCatalogSearch 改为明确 owner/session 的取消；连接退休时取消所有属于退休身份的请求。
4. 本地结果立即合并；远端保留现有 150ms debounce、limit≤100、scope 与 truncated 校验。
5. 当前架构只在 scope 对应有效活动连接时发远端请求；离线或其他连接提供“连接并搜索”显式动作，走 T09 后续接 query。
6. 返回必须同时匹配 consumer/session、query generation、ConnectionIdentity 和当前可见范围；目录 epoch/profile scope revision 变化时失效现有搜索会话，不能把旧 scope 的结果合并进新界面。
7. 关闭 Omni 取消它的搜索并使 session 失效；失败按 provider 显示，本地结果仍可使用；空查询不发 CatalogSearchRequest。
8. 用可控 fake/暂停时间测试 debounce、旧结果、close/reopen、Explorer/Omni 并存、失效 scope、truncated、取消和连接丢失；无有效 database 必须产生可结束 Loading 的取消/失败状态。
9. 确认实际搜索触发路径，保留 Explorer 的前端投影规则；不让 Omni 更新 explorer.search.query 或 selected。

**验证：** `cargo test --test omni_search --test catalog_contract --test catalog_reducer --test explorer_state`；Runtime 私有任务测试放在 `src/runtime.rs` 的测试模块并通过 `cargo test --lib omni_search` 定向执行。

**完成标准：** 两个搜索消费者互不取消/污染，旧请求不能复活关闭的 Omni，所有 Loading 都有终态。

### T11：暂存编辑表单与 owner-aware 恢复

**新增：** `src/model/interaction.rs`、`src/app/interaction.rs`、`tests/omni_resume.rs`。

**修改：** `src/model/mod.rs`、`src/app.rs`、`src/app/navigation.rs`、`src/app/omni.rs`、`src/model/catalog_editor.rs`、`src/model/profile_manager.rs`、`src/omni.rs`。

1. 依据 T01 矩阵只为可编辑草稿建立 typed SuspendedInteraction，保存 owner、revision、原位置、表单页面/字段选择/滚动。
2. 来源表单在用户确认启动新任务时移动到会话存储；同 owner 防重复暂存；不复制整个 App，也不克隆凭据到 OmniItem/MRU。
3. Profile/Catalog 测试、发现、Apply 等异步操作若未 owner 化，先阻止会话切走并说明原因；要支持测试期间暂存，则同步给结果路由增加 session ID，不能结果回写当前单槽表单。
4. 导航失败/取消：若尚未离开来源环境则恢复原表单；若已换连接则保留暂存项，用户显式恢复走 T09。
5. “恢复编辑…”先导航至 owner 再移动草稿回当前编辑器；profile 改名只更新标签，字段配置/目录 epoch 改变则重新校验依赖上下文。
6. owner 被删或对象已变更时保持草稿可查看/复制并显示不可继续 Apply 的原因，提供明确关闭/丢弃路径，不把草稿挂到同名新对象。
7. 非编辑 Overlay 使用原有关闭动作释放 TextDetail session 等资源。执行/事务/替换确认由原操作所有者管理，禁止进入通用暂存集合。
8. Runtime 合法完成事件可在 Omni 上方之外更新所属状态；需要用户决策时显示有待处理提示，取消 Omni 后显示最新确认，不覆盖 Omni session。
9. 暂存草稿纳入现有退出未完成编辑检查；不持久化密码/表单，不设置静默 LRU 淘汰，不在退出时无提示丢弃新增的待处理草稿。
10. 补表单→表→恢复、A 草稿→B 草稿→恢复 A、后台测试回调、Apply 冲突、owner 删除、对象重建、退出与恢复失败测试。

**验证：** `cargo test --test omni_resume --test profile_reducer --test profile_runtime --test catalog_editor_reducer --test catalog_editor_state --test quit_transaction_review`。

**完成标准：** 任一暂存草稿有明确归属、恢复入口和释放路径；旧表单结果不能修改新表单。

### T12：扩大命令覆盖并统一 Help/按键展示

**修改：** `src/commands.rs`、`src/app/commands.rs`、`src/help.rs`、`src/input/keymap.rs`、`src/config.rs`、`src/editor/mod.rs`、`docs/keybindings.md`。

**测试：** `tests/commands.rs`、`tests/omni_flows.rs`、既有 keymap/help 测试。

1. 列出 SHORTCUT_CATALOG 的可执行项，分类为已迁移语义命令、低层编辑动作、尚不支持来源绑定的动作。
2. 优先覆盖关闭 Tab、重命名/删除 Console、连接管理、Dashboard、执行目标选择、刷新数据、通知、事务控制和布局命令。
3. 每新增命令必须有明确 context/arguments、Availability 和语义入口；事务命令沿用已有检查，不直接发送 SQL。
4. 将 keyboard/help 的对应分支逐项接入统一入口，保留原模式适用性。Editor 专有 Vim 动作仍由 preset 管理，不保证任意模式下模拟快捷键。
5. 展示快捷键来自用户 KeyBindings；一个语义命令可以显示多个绑定，但结果只出现一次。
6. 精确搜索不可用命令时显示原因；普通空查询不堆满灰色命令。别名可以包含 UI 语言外的常见关键词，但不按自由文本猜测高风险行为。
7. 加入 registry 唯一 ID/有效 binding 引用检查以及键盘→Help→Omni 同目标执行等价测试。

**验证：** `cargo test --test commands --test omni_flows --test keymap --test docs`；`cargo test --lib help`；`cargo test --lib config`。

**完成标准：** 已暴露的命令无第三份独立执行逻辑；实际用户配置与显示一致；上下文外动作不会落错目标。

### T13：返回位置、MRU 与性能收尾

**修改：** `src/model/navigation.rs`、`src/app/navigation.rs`、`src/omni.rs`、`src/model/omni.rs`、`src/ui/omni.rs`。

**测试：** `tests/omni_navigation.rs`、`tests/omni_providers.rs`、`tests/performance_regression.rs`。

1. 导航成功后记录来源位置和实际目标，搜索高亮/后台回调/失败动作不更新历史。
2. 历史与 MRU 使用有界进程内集合（初始各 100 项）；只保存对象 ID/位置，不保存查询文本、SQL 或凭据。
3. “返回上一个位置”使用同一导航检查；返回操作不把自己再次推入历史形成 A/B 无限栈；删除/关闭目标按既有重开能力解析，失效项清理并提供解释。
4. 同名/重命名记录按 ID 更新标签；缓存与远端去重后再应用有限 MRU 加权，不覆盖精确名称匹配层级。
5. 预构建可搜索字段，按数据修订增量失效；query 变化才重排，render 不扫描全库。不要为 cache 引入第二份权威 Catalog。
6. 建立 10,000 本地对象 + 1,000 Console 的固定 fixture，限制结果 top-K 与可见绘制行数；用结构性断言检查不复制 SQL/不创建数据库请求、不在每帧重算。
7. 使用发布构建测量开框和查询更新时间，初始目标本地 p95≤50ms；记录硬件、样本、构建模式。共享 CI 不用未经校准的严格 50ms 时间断言。
8. 窄终端、长连接名、Unicode、ASCII 图标模式、无结果、provider 错误、结果截断均有可读状态。

**验证：** `cargo test --test omni_navigation --test omni_providers --test performance_regression --test ui_render`；`cargo test --release --test performance_regression omni -- --nocapture`（新增对应名称测试后运行）。

**完成标准：** 返回可预测，历史不泄露输入，常见大目录交互响应无整帧全量检索。

### T14：集成回归、文档和交付验收

**新增：** `docs/omni-bar.md`。

**修改：** `docs/architecture.md`、`docs/keybindings.md`、`docs/configuration.md`、`docs/performance.md`、`README.md`、`tests/docs.rs`。

1. 完成下方端到端场景；使用 fake runtime 做结果乱序，用 SQLite 临时数据库验证真实 Catalog/表打开路径。
2. PostgreSQL/MySQL/Oracle/SQL Server 仅在测试环境具备时做真实驱动烟测；环境未配置导致跳过必须明确记录，不能以 suite exit 0 宣称驱动已验证。
3. 更新 Omni 用法：F2、>、@、对象动作、参数向导、取消/返回、缓存与搜索范围、事务限制、会话暂存。
4. 更新架构：独立 top-layer、来源 context、search owner、navigation ID、运行态 Workspace 与落盘投影的差异。
5. README 添加入口链接；帮助按键和配置文档与实际默认一致；不改发布版本、不把未发布功能记为已发布版本。
6. 执行所有新增定向测试后运行项目要求的最终检查：

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

7. 若 all-features 被 Oracle 客户端/本地库环境阻塞，记录具体失败，补跑无默认特性检查以验证可用部分；不得将其标记为完整门槛已通过。
8. 复查 diff：新增配置同步文档、异步结果均有身份、无无意覆盖用户文件、无数据库凭据进入结果/快照/历史。

**完成标准：** M4 场景通过；实际未验证驱动/环境项列出；最终检查结果可复现。

## 7. 集成验收矩阵

| 编号 | 场景 | 成功标准 |
| --- | --- | --- |
| A01 | SQL Insert/Visual/prompt 打开 Omni→输入→Esc | 模式、光标、选择、输入、撤销不丢失；Omni 文本不进 SQL |
| A02 | EditCell/WHERE 打开→关闭 | 不提交、不丢草稿；输入仍属于原 Tab |
| A03 | busy Catalog/Update/确认框打开 | 可搜索；冲突动作说明原因；取消返回最新合法状态 |
| A04 | 粘贴、拖动、滚轮、Ctrl-c、F2 repeat | 只影响顶层；不退出、不触发底层动作 |
| A05 | prod/staging 同名 users | 路径消歧，按 CatalogId 打开正确表 |
| A06 | 已打开表再打开/关闭 Console 重开 | 不重复 Tab；SQL 和已有表编辑状态保留 |
| A07 | 新建向导中途回退/取消 | 各步选择保留；最终确认前无创建或绑定变化 |
| A08 | 手动事务→Omni 跨连接打开表 | 按既有确认处理；确认后打开原目标，取消不继续 |
| A09 | 运行 SQL 时请求跨连接 | 单连接路线明确阻塞；多连接路线按各自会话执行 |
| A10 | 导航 B 后改成 C，B 迟到 | B 不抢焦点、不重放目标，不错误关闭 C 的资源 |
| A11 | 快速输入/关闭重开/两个搜索消费者 | 旧结果不覆盖；互不取消；loading 终结 |
| A12 | 搜索中 scope 改变/对象删除重建 | 旧对象失效；不按同名猜测执行 |
| A13 | Profile/Catalog 草稿→另一任务→恢复 | 草稿、字段位置和 owner 正确；旧回调不串线 |
| A14 | 跨连接 A→B→A 和同连接 Console 切换 | 编辑历史、prompt 和模式正确归属 |
| A15 | 用户重映射唤起/业务命令 | 全局冲突正确检出，Help/Omni 显示实际绑定 |
| A16 | 零 Profile/零 Tab、80×24、更窄终端 | 入口正常、无 panic、可退出/创建合法对象 |
| A17 | 有暂存草稿退出 | 沿用未完成编辑处理，不静默遗失 |
| A18 | 10k 对象查询与连续绘制 | 本地即时响应，绘制不反复全量检索 |

## 8. 工时与执行检查点

以下为单人熟悉代码后的粗估，不是交付承诺；不包含多连接计划自身工时、外部数据库环境搭建和发布流程。

| 阶段 | 任务 | 预计工程日 | 主要不确定性 |
| --- | --- | --- | --- |
| 语义与现场基础 | T01–T04 | 4–7 | Modalkit 暂态归属、Workspace 既有测试 |
| Omni 交互 | T05–T08 | 4–6 | 跨模态输入与鼠标、目标选择规则 |
| 异步与中断恢复 | T09–T11 | 5–8 | 事务 continuation、表单结果 owner 化 |
| 覆盖与收尾 | T12–T14 | 3–5 | 命令数量、真实驱动回归 |
| 合计 | | 16–26 | 完整首版，包括恢复能力 |

检查点：T01 后确认 Workspace 路线；T04 后验证现场恢复原型；T08 后手动评审交互；T11 后验证事务/草稿边界；T14 后决定发布。每个检查点以测试证据与可操作场景验收，不以类型或 UI 已存在为完成依据。

## 9. 实施记录

- 2026-09-12：完成源码与相关计划核对，创建本实施计划。尚未执行产品实现、基线测试或构建；本文所有测试命令均为后续实施步骤。
