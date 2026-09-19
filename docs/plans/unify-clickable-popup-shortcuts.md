# 统一弹窗快捷键样式与鼠标执行 Implementation Plan

> **For Luna:** 按本计划逐项实施并持续完成自动工作流。writing-plans 的通用执行交接约定服从本任务的模型分工：实施、纠偏、审查、提交合并均由 Luna 完成；不要求用户选择执行方式，不启动子 Agent。

**Goal:** 所有现有弹窗底部快捷键采用新建连接的统一视觉样式，点击快捷键或其描述能够执行与键盘一致的操作。

**Architecture:** 扩展 `ui::shortcut_hints`，以结构化提示和一次布局同时生成文本与命中区域。提示携带显式按键或完整序列，在点击时通过现有 Keymap 映射为 Action；由现有 action/reducer/runtime 执行。新增命中通道遵循当前模态所有权，不放开背景交互。

**Tech Stack:** Rust 2024；ratatui 0.30.2、crossterm 0.29、unicode-width 0.2；现有 App/Keymap/UiState；ratatui TestBackend 与 Cargo 测试。

---

## 0. 执行上下文与范围约束

- 正文及任务产物位置：`/Users/yelog/workspace/tui/lazydb/.git/opencode-tasks/ses_f472b9eebffezDuHTdrThEDtGl`，下文简称“任务目录”。本计划阶段只写该目录。
- 原工作区：`/Users/yelog/workspace/tui/lazydb`；目标 main；任务起点固定为 `45e330cedd3b8da493d1c198329ec38d5e1f38b5`。
- 计划开始实测原工作区 HEAD 为 `c634324b6cc3a94dbb4735349fc20c4516f23282`，main ahead 8；新增提交涉及 explorer principal 连接状态，未修改分析中的 UI/input/runtime。
- 后续由自动工作流/Luna 命名并创建任务分支/worktree。不要在原 main 实施，不自行把最新 main 改为任务起点。已有任务 worktree 时先核对实际 diff，不重建或丢弃已有工作。
- 原工作区存在两个未提交文件：`src/persistence/workspace.rs`、`tests/workspace_persistence.rs`，内容是 principal_kind 序列化修复及测试。本计划无依赖，不纳入任务提交；新 worktree 不会自动继承。
- 不改 Git index、不 stash、不提交原工作区用户内容；实施提交只在任务 worktree 精确暂存任务文件。
- checkpoint.json 当前不存在；后续若出现先读它和实际 diff，不把旧 next 当新指令；state.json/checkpoint.json 由插件维护。
- 本阶段已调用 writing-plans。按用户要求不创建 docs/plans 副本，也不调用不存在的执行技能、不启动子 Agent。

## 1. 必须满足的功能验收

### 1.1 统一样式

1. 按键使用 `theme.action` + BOLD，描述使用 `theme.text` 且不加粗，组间三格空白；统一 footer 使用参考连接表单的 `theme.surface`，居中排列，换行后逐行居中。
2. 窗口形状、正文、操作按钮和危险操作焦点可保持各自现有样式；统一的是底部快捷键区域。
3. 窄屏优先完整项换行，必要时裁剪/省略；高度必须受 footer 可用区域约束。主操作、取消/关闭优先于辅助编辑提示。
4. Error/feedback 另占行，不能替换全部快捷键；不允许 footer 覆盖表单输入或按钮。

### 1.2 点击与键盘一致

1. 左键 Down 点击同一提示的 key、description、两者之间内部空格均触发同一动作；Up/Drag/Move/右键不重复执行。
2. 组间空白、边框、非交互说明、省略标记与不可见项没有热区。
3. 同义按键可保留组合显示；不同方向/功能必须拆为明确项，例如 `Tab next field`、`Shift+Tab previous field`，`j down`、`k up`，`J move down`、`K move up`。
4. `dd delete column` 一次点击产生完整删除命令；显式声明 `[d, d]`，不从字符串猜按键。
5. `Type edit`、`Read only`、`Unavailable`、`wait for completion`、拖动说明不伪装为按钮；不可用操作不激活。
6. 当前字段、owner picker、列详情、表单/preview、确认框焦点、busy 等决定命令语义。点击不绕过预览、输入校验、二次确认和 reducer 限制。
7. 顶层 modal/Omni/列详情遮挡下层，toast 覆盖优先；resize、关闭、切模式后没有遗留命中。

### 1.3 全量覆盖口径

按第 4 节逐族迁移全部现有底部快捷键，包括用户举例以外的弹窗。没有底部快捷键的浮层无需新加 footer；全局状态栏、补全候选列表和数据库执行语义不在本任务范围。

补充代码核对：`render_key_sequence_popup` 的导航提示位于**顶部 Block title**（`src/ui/mod.rs:1410`），并非底部 footer；本任务不迁移它的顶部提示，但要验证它出现时下层新增快捷键不会穿透。不能将其 Enter 转入一个无 pending 的临时 Keymap 来运行序列选择；该浮层依赖 runtime 的真实 pending 状态。

## 2. 实现契约（先确定，避免迁移时分叉）

### 2.1 结构化数据和布局

- 在 `src/input/keymap.rs` 定义轻量点击按键序列类型/辅助方法，在 UI 使用。类型只包含键事件/序列，不包含业务 App 快照或已解析 Action。
- `ShortcutHint` 提供纯说明构造及可激活构造；`key` 展示内容与 `activation` 独立。显式启用状态让 busy/无效提示不注册命中。
- 共享测量器输出每行 styled spans、行宽、提示标识对应的可见 Rect 片段及实际使用高度；渲染器直接消费这些坐标，禁止另写第二套点击位置计算。
- 使用 terminal cell width，不用 UTF-8 byte length；片段矩形与绘制区域求交。key 和描述跨行时可有多个 Rect，均指向同一命令。
- 对空面积返回空布局；行居中偏移以最终裁剪后的实际行宽计算；分隔符始终排除在热区外。
- 保留 `line/lines` 等纯展示包装供全局 footer 使用；交互迁移调用统一 renderer，不能在每个调用点继续拼坐标。
- 删除/替换 `dialog::render_hint` 的纯字符串入口，或改为结构化委托入口；完成后弹窗 footer 不应继续使用旧入口。

### 2.2 点击解析

- 输入层提供 `map_shortcut(sequence, app) -> Option<Action>` 等等价入口，使用 `app.key_bindings` 初始化无 pending 的 Keymap。
- 单键直接 map；多键要求中间键均不产生 Action、末键产生唯一 Action。对无结果/未完成序列/中途已产生动作的非法声明返回 None，不能丢弃中间 Action 或执行多次。
- 只接受程序显式声明的完整序列。声明 dd 前先测试当前表格导航上下文；文本字段中不提供 dd 提示。
- 保持现有真实键盘优先级，包括全局自定义绑定；已有动态键名的提示同时生成正确事件。冲突测试中，点击必须与真实键盘一致，不能另设 UI 私有快捷键含义。
- `map_mouse` 签名保持不变；命中后返回已有 Action，经现有 runtime apply_action；不增加任意字符串执行 Action，不在 UI 执行业务 reducer。

### 2.3 模态与事件路由

- 在 `HitTarget` 加通用快捷键目标（序列及可核对的 owner），在 UiState 保存当前可交互提示层信息；每帧清理。
- 从普通 overlay 进入嵌套列详情/Omni 时显式清除或屏蔽下层 shortcut targets。浮层的空白区域也不能透过其 owner 检查激活下层。
- 左键路由位于 toast 优先、残留 gesture 取消之后，TextDetail/SqlHistory 特殊提前返回、输入框选择及旧 overlay 白名单之前。只对当前层的通用提示目标走新入口。
- 以 `target_at` 最上层可见目标为准；不能从另一套区域直接命中并绕过 toast。
- key-sequence chooser 处于最上层时屏蔽底层新增 footer 命中；不更改其序列导航实现。

## 3. 单元实施步骤

每项步骤为一个小动作；完成一个业务单元及其定向检查后继续下一单元，不要求人工 resume。所有新增集成场景统一放在 `tests/popup_shortcuts.rs`，测试名使用下列前缀以确保定向命令真实匹配。复用已有 tests 的 App/fixture 构造模式，不引入真实数据库依赖。现有 `tests/mouse.rs`、`tests/ui_render.rs`、`tests/keymap.rs` 仅更新因合法文案/热区变化而需要调整的断言，不能降低行为断言。

### 单元 1：新建/编辑连接快捷键端到端闭环

**文件**：修改 `src/ui/shortcut_hints.rs`、`src/ui/dialog.rs`、`src/ui/mod.rs`、`src/ui/profiles.rs`、`src/input/keymap.rs`、`src/input/mouse.rs`；新增 `tests/popup_shortcuts.rs`；按需调整 `tests/mouse.rs`、`tests/ui_render.rs`、`tests/keymap.rs`。

1. 在新测试文件创建 TestBackend fixture：构造 App、打开 ProfileManager、渲染到 UiState，使用真实鼠标事件命中。禁止只有手造 HitRegion 的测试作为端到端证据。
2. 添加 `popup_profile_` 场景：分别点击 save 的 key 与 description、点击 Esc cancel；比较真实 Keymap Action；对无效表单保存调用现有 reducer，断言保留表单并显示校验反馈。
3. 运行 `cargo test --test popup_shortcuts popup_profile_`，记录现有实现无 footer 点击导致失败，不要求固定 panic 文本。
4. 扩展结构化提示数据；增加单键/序列映射入口与非法多 Action 序列防护。
5. 增加共享布局器/渲染器；添加模块测试覆盖居中、terminal cell width、换行、0 面积、no-color 粗体、裁剪与非交互间隔。
6. 增加通用 HitTarget、每帧清理和 owner；接入左键解析的正确位置。
7. 迁移连接表单、确认、scope 列表/加载状态，拆分方向提示；为提示行分配真实高度，保持反馈与按钮可见。
8. 扩展 `popup_profile_`：test/save/save+connect、字段前后切换、choice/toggle、busy、确认焦点；验证不产生重复 Down/Up 动作。只检查生成的正确 Action/Effect，不实际联网。
9. 复核：文本区域是否每个可见字符都有热区；按键及描述样式与参考一致；插入状态点击不变成文本输入；原按钮仍可工作。
10. 运行：`cargo test --lib ui::shortcut_hints`；`cargo test --test popup_shortcuts popup_profile_`；`cargo test --test profile_reducer`。预期退出 0，新增场景数量大于 0。

**验收**：新建和编辑连接在正常/窄屏的全部有效 footer 命令可点击，错误和 busy 状态正确；共用组件已覆盖布局边界。此为当前第一个未完成业务单元。

### 单元 2：Catalog 普通表单与预览闭环

**文件**：`src/ui/catalog_editor.rs`、`tests/popup_shortcuts.rs`、`tests/ui_render.rs`。

1. 添加 `popup_catalog_form_`：database/schema 表单中 Enter 与 Esc 的 key/description 点击，字段前后切换与 owner picker 的选择/关闭；先运行测试确认旧代码失败。
2. 迁移 ObjectPicker、database/schema/view/materialized view/sequence/role 表单，根据 focus kind 生成真实命令；说明字段保持非交互。
3. 迁移 owner picker 独立状态；只有当前 picker 的命令激活，Esc 关闭列表不丢草稿。
4. 将 error 与 footer 分开布局；保持底部主操作可见。
5. 迁移 loading、SQL preview、preview error；applying 用等待说明，不能把无效 Esc 画成可执行取消。
6. 加入 `popup_catalog_form_` reducer 场景：有效表单进入既有预览流程，无效表单保留错误；从 preview 返回 form 保留输入。
7. 复核所有 CatalogDraft 非 table 类型都有同一 renderer 路径，Enter 与当前键盘含义一致，不统一硬编码成 apply。
8. 运行 `cargo test --test popup_shortcuts popup_catalog_form_`、`cargo test --test catalog_editor_state`。预期退出 0。

**验收**：database/schema 等创建/编辑表单、owner 选择、预览/返回/错误形成完整键鼠一致流程。

### 单元 3：Table 编辑、嵌套列详情与 Catalog 确认闭环

**文件**：`src/ui/catalog_editor.rs`、`src/ui/mod.rs`、`src/input/keymap.rs`、`src/input/mouse.rs`、`tests/popup_shortcuts.rs`、`tests/ui_render.rs`。

1. 添加 `popup_catalog_table_`：导航态 dd 删除、a/A 添加、J/K 重排、字段模式下的 Enter/Space、列详情 Esc；先运行确认失败。
2. 将 table_shortcut_hints 改为明确动作项；拆分所有不同方向，保留真实 dd 序列。
3. 在布局前测量 footer 高度，并限制在有效 area；预留 action row 和最小内容区域，窄高窗口不下溢。
4. 列详情渲染前切换快捷键 owner，注册自己的 confirm/cancel/field navigation；屏蔽父表单命中。
5. 迁移 CatalogEditorDiscardConfirm、CatalogEditorDestructiveConfirm、CatalogDropConfirm，包括 maintenance database、busy/error 状态。
6. 添加 `popup_catalog_table_` 行为断言：dd 点击一次正确作用于选中列；Esc 仅关闭列详情；destructive confirm 未达到输入条件时不会实际应用；错误后仍可取消。
7. 复核 keymap 的 Pending::CatalogColumnDelete 与上下文校验；只有最后一步产生 Action；不在文字编辑时暴露导航命令。
8. 运行 `cargo test --test popup_shortcuts popup_catalog_table_`、`cargo test --test catalog_editor_state`。若同一 reducer 文件自单元 2 后完全未变且相关行为已有当前测试覆盖，可复用该项证据并注明，避免机械重跑。

**验收**：table 增改列与预览流程保持原语义；嵌套弹窗不穿透；全部 Catalog 确认 footer 可点击。

### 单元 4：选择器和连接/SQL 编辑器管理闭环

**文件**：`src/ui/mod.rs`、`tests/popup_shortcuts.rs`、`tests/ui_render.rs`。

1. 添加 `popup_selector_` 参数化场景，覆盖 ProfileAccess、ProfileGroup 的 picker/edit/delete、ExplorerAdd、TargetSelector、DatabaseSelector、PageSizeSelector、SqlEditorList 的列表/搜索/rename、DeleteConsole。
2. 先运行 `cargo test --test popup_shortcuts popup_selector_` 确认迁移前失败。
3. 逐个将纯字符串/手拼 Span 改为结构化 hints，拆分 select up/down、text edit 组合键。
4. 文本输入态只显示当前支持的 Enter/Esc/编辑命令，退出搜索应返回其原层而不是直接关整个管理器。
5. 将命中区域与各子状态的 footer rect 对齐，列表滚动不滚动 footer。
6. 复核每个确认框的 Enter 依当前 focus 执行，不直接绑定删除/确认；列表空态与错误态仍可返回。
7. 运行 `cargo test --test popup_shortcuts popup_selector_`、`cargo test --test console_manager_input --test explorer_add --test profile_groups`。预期退出 0。

**验收**：本族各子状态均统一且可点击，选择、搜索、重命名、确认/取消实际工作。

### 单元 5：执行、事务、关系值编辑闭环

**文件**：`src/ui/execution_confirm.rs`、`src/ui/relation.rs`、`src/ui/mod.rs`、`tests/popup_shortcuts.rs`、`tests/ui_render.rs`。

1. 添加 `popup_transaction_`：ExecutionConfirm、ManualCancelConfirm、TransactionExitConfirm、RelationTransactionConfirm、ClearTransactionOutcome、TransactionMenu；两种按钮焦点与 preview 导航至少各一例。
2. 添加 relation 单行/多行 cell editor 的 apply/cancel/newline/format 和 NULL/DEFAULT/value 提示场景，使用 `popup_transaction_cell_` 前缀。
3. 运行 `cargo test --test popup_shortcuts popup_transaction_`，确认失败。
4. 迁移确认框底部字符串；拆分 preview 的 hjkl/v/y 为真实动作/说明，不用一个点击替代整组命令。
5. 迁移 relation 弹出编辑器底部，保留 multiline Enter newline 与 Ctrl-S apply 的区别；错误反馈不遮取消。
6. 复核事务忙碌、未知结果/abandon 状态，只生成实际允许的命令；保留输入选择和 preview 滚动。
7. 运行 `cargo test --test popup_shortcuts popup_transaction_`、`cargo test --test transaction_reducer --test relation_tabs`。预期退出 0。

**验收**：点击确认仍遵循既有事务/执行流程，无额外数据库执行路径，输入编辑与 footer 互不干扰。

### 单元 6：Redis 编辑与确认闭环

**文件**：`src/ui/redis_object_editor.rs`、`src/ui/redis_table_editor.rs`、`src/ui/mod.rs`、`tests/popup_shortcuts.rs`、`tests/ui_render.rs`。

1. 添加 `popup_redis_`：ObjectEditor、TableEditor、TableDeleteConfirm、ValueSaveConfirm、UnsavedValueConfirm、DeleteConfirm、DeletePreparing、PreviewFormat；先运行失败测试。
2. 迁移 object/table 编辑器 footer 与 delete confirm 的混合正文提示，保留表单输入高度与焦点。
3. 迁移 save/unsaved/delete/format 确认；保存、丢弃、取消各自有明确项；输入 validation error 不掩盖提示。
4. loading/preparing 只显示当前有效取消键；无效按键说明不创建热区。
5. 复核每个 Enter 与当前 focus 相同；点击取消无隐式 discard；底层 key/value 浏览器不被点击穿透。
6. 运行 `cargo test --test popup_shortcuts popup_redis_`、`cargo test --test redis_object_editor --test redis_unsaved_changes`。预期退出 0，不需要 Redis 服务。

**验收**：Redis 所有现有底部命令完成迁移，编辑/保存/丢弃/返回语义一致。

### 单元 7：详情、历史、通知、系统弹窗和 Omni 闭环

**文件**：`src/ui/record_view.rs`、`src/ui/text_detail.rs`、`src/ui/sql_history_modal.rs`、`src/ui/notifications.rs`、`src/ui/update.rs`、`src/ui/omni.rs`、`src/ui/mod.rs`、`src/input/mouse.rs`、`tests/popup_shortcuts.rs`、`tests/ui_render.rs`。

1. 添加 `popup_viewer_`：RecordView（含失效记录）、TextDetail、SqlHistory 浏览/搜索/SQL、NotificationHistory 浏览/搜索/清空确认、NotificationDetail。
2. 添加 `popup_system_`：Update 不同状态、Message、WorkspaceSaveFailed retryable/nonretryable、SubstituteConfirm、Help、Omni root/子级/小窗口；只需为实际 footer 存在的命令写点击测试。
3. 运行 `cargo test --test popup_shortcuts popup_viewer_`、`cargo test --test popup_shortcuts popup_system_` 确认旧代码失败。
4. 迁移 viewer footer，将 TextDetail 拖动说明与真实关闭/滚动操作分开；copy-all 若原本为独立按钮保持其原有执行入口，不虚构不存在的快捷键。
5. 迁移通知历史与其嵌套确认/搜索状态；搜索输入态不显示会被解析为插入文字的浏览快捷键。
6. 迁移 Update/Message/WorkspaceSaveFailed/SubstituteConfirm/Help 的底部提示；无 footer 的分支记为无底部提示，不额外改造整个面板。
7. Omni 把 title_bottom 中底部操作移入统一 footer 预留行，root 的 close 与子级 back 保持区别；小窗口若显示 Esc 关闭提示，也采用结构化可点击形式。
8. 复核 TextDetail/SqlHistory 的早返回分支已让新快捷键路由通过，且拖拽选择不误触 footer；Omni 覆盖已有 overlay 时所有下层 hints 被屏蔽。
9. 运行 `cargo test --test popup_shortcuts popup_viewer_`、`cargo test --test popup_shortcuts popup_system_`、`cargo test --test sql_history_interaction --test omni_navigation --test update_reducer`。预期退出 0。

**验收**：本族各现有 footer 都可点；详情选择、搜索、复制、返回、更新流程保持既有行为。

### 单元 8：覆盖清点、隔离回归与收尾

**文件**：`tests/popup_shortcuts.rs`、`tests/mouse.rs`、`tests/ui_render.rs`、`tests/keymap.rs`；必要的修正仅限上述实施文件。覆盖/验证结果写任务目录 `validation.md`。

1. 按第 4 节覆盖表逐条填入“实现函数/测试名/无底部提示理由”；不接受“其他弹窗同理”作为覆盖证据。
2. 搜索余留：`rg -n 'render_hint|title_bottom|ShortcutHint|Esc|Ctrl[-+]' src/ui`。逐条分类，允许正文说明、非 popup 全局 footer、顶部 sequence 提示保留；不以 grep 零结果代替语义复核。
3. 添加 `popup_isolation_`：列详情、Omni 覆盖普通 overlay、toast 覆盖 hints、sequence chooser 屏蔽下层、背景空白、resize/关闭后旧位置、输入/文本 drag 生命周期。
4. 添加 `popup_geometry_`：以 120x40、80x24、60x20 及小于最小布局阈值的窗口做 TestBackend；至少对每个族一个代表场景断言可见文字/区域边界；小窗口走 TooSmall 时验证热区清空而非强求完整弹窗可见。
5. 对每个有效提示的 key 与 description 首尾取样，断言 key/description 同 Action；验证三格分隔内无 shortcut target，省略项无 target。避免把字段按钮/背景 Help target 误当新快捷键命中。
6. 运行 `cargo test --test popup_shortcuts`（全部新增场景），随后 `cargo test --test mouse --test ui_render --test keymap`。若失败，根据当前具体差异修复；不为过期文案继续强行保留错误行为。
7. 运行第 5 节最终质量门禁一次，保存命令、版本、exit code、实际测试数和 skipped 外部服务情况。
8. Luna 自审 diff：无键名字符串解析、无 Action 缓存执行、无白名单全开、无用户持久化修改混入、无抑制断言/警告、无正文被新 footer 挤出、无遗漏子状态。
9. 自动工作流允许提交时，在任务 worktree 精确暂存业务单元文件并提交；建议按单元 1、单元 2–3、其余族合理成批，不为小步骤制造未编译提交。提交信息可用 `feat(ui): unify clickable popup shortcut hints` 等描述；禁止 `git add .` 吸入无关文件。合并 main 前评估 main 自起点新增提交，冲突修复后只重跑受影响检查，再执行必要的合并门禁。

**验收**：覆盖清单完整，所有用户功能标准满足，必须门禁通过或明确记录真实外部限制/基线问题供 Luna 收尾处理；不能拿旧测试结果或仅三类弹窗通过宣告完成。

## 4. 覆盖矩阵（实施时逐项核销）

| 单元 | 必须核对的变体/子状态 | 位置 |
| --- | --- | --- |
| 1 | ProfileManager 新建/编辑、确认、scope 列表/加载、busy/error | profiles.rs |
| 2 | CatalogEditor ObjectPicker、database/schema/view/materialized view/sequence/role form、owner picker、loading/preview/error | catalog_editor.rs |
| 3 | Table 导航/输入/按钮/column details；CatalogEditorDiscardConfirm、CatalogEditorDestructiveConfirm、CatalogDropConfirm | catalog_editor.rs、mod.rs |
| 4 | ProfileAccess、ProfileGroup picker/edit/delete、ExplorerAdd、TargetSelector、DatabaseSelector、PageSizeSelector、SqlEditorList browse/search/rename、DeleteConsole | mod.rs |
| 5 | ExecutionConfirm、ManualCancelConfirm、TransactionExitConfirm、RelationTransactionConfirm、ClearTransactionOutcome、TransactionMenu；relation cell editor 单行/多行 | execution_confirm.rs、relation.rs、mod.rs |
| 6 | RedisObjectEditor、RedisTableEditor、RedisTableDeleteConfirm、RedisValueSaveConfirm、RedisUnsavedValueConfirm、RedisDeleteConfirm、RedisDeletePreparing、RedisPreviewFormat | redis_object_editor.rs、redis_table_editor.rs、mod.rs |
| 7 | RecordView（含记录失效）、TextDetail、SqlHistory browse/search/sql；NotificationHistory browse/search/clear、NotificationDetail | record_view.rs、text_detail.rs、sql_history_modal.rs、notifications.rs |
| 7 | Update、Message、WorkspaceSaveFailed 两种 retryable、SubstituteConfirm、Help；Omni root/子级/too small | update.rs、omni.rs、mod.rs |
| 8 | 无底部提示的 Overlay 分支明确标注；sequence popup 顶部提示不迁移但屏蔽下层；completion list 不新增 footer | mod.rs 和全部 renderer |

## 5. 验证分级与执行纪律

### A. 用户需求的验收证据（必须）

第 1 节是功能完成标准；通过 TestBackend、map_mouse、Keymap parity 和既有 reducer 的真实状态/Effect 断言提供自动证据。测试应覆盖真实风险（几何、描述命中、状态、隔离、序列），不逐行复刻实现，也不为只换一条说明文字新增低价值测试。

### B. 项目质量门禁（必须按项目/合并流程履行）

依据 `.github/workflows/ci.yml`：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

预期全部 exit 0；全量只在功能齐备后执行，不每单元重复 check+clippy+全部测试。已有工具链是 1.94.1；实施先检查 1.94.0 可用性，如使用 1.94.1 必须显式记为本机验证版本，1.94.0 结果由项目 CI/实际运行提供，不能冒充。

CI 的 macOS binary dependency 检查按适用流程执行：`cargo +1.94.0 build --locked` 后 `sh scripts/release/check-macos-dependencies.sh target/debug/lazydb`。数据库服务/分发/Windows installer 作业是现有 CI 职责，不因 UI 改造在本地新增“必须搭建所有服务”的门禁；合并所要求的作业仍由 CI 验证并记录。没有运行/服务缺失跳过与实际通过分开记。

起点与用户未提交 principal_kind 修复有差别。干净 worktree 若出现持久化基线失败，先用具体失败和起点代码确认，不把用户修复自动加入任务。确有新依赖时先更新计划/change-scope 明确变更，再由 Luna 按工作流处理；普通编译和本次回归自行修复，不上报 blocked。

### C. 补充建议检查（非新增必需门禁）

- 真实终端鼠标检查宽屏/窄屏、no-color、连续点击与 drag；可录屏/截图辅助查看样式。
- 真实数据库连接/建库操作可作为体验补充，但本次核心命中与 action 语义不依赖联网。
- 用户未要求人工/PTY 验收；环境受限只允许一次针对性修复重试，此后 Luna 收尾评估 TestBackend 等替代证据或记录限制，不能无限 progress。

### D. 日志与提交纪律

每次实际验证向任务目录 `validation.md` 追加：完整命令、exit code、相关文件、HEAD、未提交 diff 摘要、工具链/环境、失败或跳过原因。同一代码/环境的已通过结果可以引用；相关文件/环境发生变化才重跑。计划中的命令只是待执行项，不能记录为已通过。

## 6. 预计文件清单与退出条件

预计修改 17 个 src 文件及 3 个既有测试文件，新增 `tests/popup_shortcuts.rs`，合计 21 个文件；完整机器清单见 `change-scope.json`。没有删除或重命名，没有未提交依赖文件。`src/runtime.rs`、`src/action.rs`、`src/help.rs`、`src/model/workspace.rs`、Cargo/CI 配置仅作参考，不预计修改。

计划阶段退出条件：本计划、change-scope.json 已写入且路径有效，验证日志已记录本阶段只读检查，最后写入本轮 plan completed 回执。实施尚未开始；下一业务动作由 Luna 在任务 worktree 执行单元 1，然后持续完成其余单元与收尾。
