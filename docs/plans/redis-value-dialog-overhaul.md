# Redis Value Dialogs Implementation Plan

> **执行负责人：Luna。** 按本计划逐个完成端到端验收单元，随后由 Luna 审查、纠偏、提交与合并。Astra 仅承担分析/计划。本任务不启动子 Agent。

**Goal:** 让 Redis value 编辑的保存、格式警告、未保存离开、对象/集合编辑和行删除弹窗与表格编辑保持一致，并完成可靠的一次确认保存闭环。

**Architecture:** 复用 `ui::dialog`、`shortcut_hints` 和现有 Redis mutation Commands，新增 Redis 专用审查/提交状态；文本草稿继续以 editor buffer 和 RedisBrowserTab baseline 为权威来源。将离开动作绑定到具体编辑 owner 和保存请求，只在匹配的成功回调后继续，避免通用消息框、对象表单和 pending 字段相互串接。

**Tech Stack:** Rust 2024、Ratatui 0.30、Crossterm 0.29、现有 App reducer / Command runtime / Redis adapter、TestBackend 与 Rust 集成测试。

---

## 0. 执行约束和已知基线

- 原始需求和完整分析：`.git/opencode-tasks/ses_f4ca03d77ffey3iIGC7n6HnOvi/analysis.md`。分析已在当前会话完成，不需要重新全文调查。
- 指定起点 `2df399013fd12a65816715e38180b23f58f99100`，目标 `main`。分析期间 main 前进至 `fd294bd`，新增关系表单元格列元数据与文档；未修改 Redis 或共享确认框。
- 任务分支与工作树由计划完成后的流程命名/创建。实施前读取插件当前 checkpoint 和实际 diff；在分配的工作树执行，不对共享 main 直接提交。若由指定起点建分支，最终集成保留 main 并行工作。
- 现存未跟踪 `docs/plans/2026-09-18-tab-reorder-focus-implementation.md` 属于其他工作。只显式 stage 本任务文件。
- 不改插件 `state.json`/`checkpoint.json`；只写当前轮指定回执，不能沿用本计划阶段的文件名作为后续回执。
- 三张图片在会话中只有占位符。采用代码中 `RelationTransactionConfirm` 为视觉参照，不能声称完成截图比对。
- 只读/完整加载限制、原 mutation adapter TTL 和 expected-value 检查保留；Redis 无 SQL 事务语义，按钮不能称 Commit/Rollback。

## 1. 选定设计与不可破坏的契约

### 验证分级与复核责任

- **用户需求验收**：Redis value 编辑涉及的弹窗样式及交互对齐表格编辑；保存、取消、离开、错误恢复等真实业务闭环可用。第 6 节矩阵将需求与已定位根因转成可验证行为，不代表用户要求采用特定人工验证工具。
- **项目强制门禁**：`CONTRIBUTING.md` 指定的 fmt、all-targets/all-features clippy 和 test，以及异步归属、终端文本清洗、快捷键目录与文档同步等项目规则。功能齐备后统一执行，结果写入 validation.md。
- **本计划选用的自动化回归**：各单元 reducer/keymap/mouse/TestBackend 定向测试用于验证此次状态和交互变更；可按实际实现调整 fixture/测试组织，但必须覆盖对应验收行为。它们是工程验证方案，不声称是用户额外指定的外部门禁。
- **补充建议验证**：真实 Redis、PTY 人工键鼠、截图视觉比对；有条件时补充，不自动升级为强制门禁。环境受限最多一次针对性修复重试，由 Luna 收尾审查决定补证或记录限制。
- **逐单元复核**：Luna 在提交每个完整业务单元前核对实际 diff、该单元验收条件、定向测试结果、与其他入口共用代码的影响；发现普通实现问题继续修复，不等待用户 resume。全量检查只在功能齐备后或相关变更确有必要时运行。

计划落盘时共享 main 已再次前进到 `af23ff65072f203b5266769cb9898203a09854b5`（ahead 9）。这不是本任务提交；后续必须以分配工作树的实际版本复核交叉变更，不把本文较早行号或基线观察当作当前 HEAD。另有未跟踪 `docs/plans/2026-09-18-no-open-tabs-empty-state-implementation.md`，同样保留。

### 1.1 权威状态与模块边界

新增 `src/model/redis_dialog.rs`，在 `src/model/mod.rs` 注册。这里定义保存审查、状态阶段和 typed leave intent；不要建立另一份不断同步的 editor buffer。

推荐类型轮廓（名称允许依周边风格调整，职责不可丢失）：

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedisSubmitPhase {
    Review,
    Planning,
    Saving,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RedisLeaveIntent {
    OpenKey {
        tab_id: uuid::Uuid,
        node: crate::model::redis_key_tree::KeyTreeNodeId,
    },
    CloseTab(uuid::Uuid),
    DisconnectProfile(uuid::Uuid),
    Quit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisSaveSnapshot {
    pub owner: crate::model::redis_value_edit::RedisValueEditOwner,
    pub revision: u64,
    pub format: crate::value_preview::PreviewFormat,
    pub text: String,
    pub draft: crate::db::redis::mutation::RedisValueDraft,
}
```

在同模块增加保存状态：snapshot、`EditValidation`（保留完整 warning）、phase、typed focused action、request_id、plan、error、optional leave intent。快照是提交边界的不可变副本，不随 editor 继续编辑而变化。编码失败时不构造有效 snapshot，原编辑器显示可返回的错误。

- `RedisBrowserTab.value_edit_baseline` 和 editor text 继续决定 dirty；不将旧 `RedisValueEditDraft` 整体接入再保留第二份 dirty 真相。可复用其 `RedisValueEditOwner`，revision/request 校验在真实 App 流程落实。
- `Overlay` 持有展示状态；App 保留足够的 in-flight owner/request 信息，异步事件不能仅凭“当前恰好是某个 overlay”辨别归属。小范围实现可让同一 save state 保持在 overlay 并禁止提交期间替换；同时 callbacks 必须验证完整 owner。若已有运行时入口必须允许切换 UI，则将 in-flight state 放 App，overlay 只引用其 ID，不复制两份可变状态。
- `RedisMutationPlanReady/Failed/Succeeded/Failed` 继续使用现有消息协议；本地 state 将 request_id 绑定完整 owner。优先不修改 runtime 与 adapter。
- 现有 create/object/table 操作保留各自 form model，复用同一 phase/按钮渲染规则，不强行把所有表单合并为通用框架。

### 1.2 状态转移表

| 当前状态与事件 | 行为 | 允许副作用 |
| --- | --- | --- |
| 编辑 clean → SaveRequested | 保持编辑，可给 No changes 轻提示 | 无写命令 |
| 编辑 dirty → SaveRequested | 捕获 owner/revision/format/text，encode + validate，显示 Review | 无写命令 |
| Review → Cancel/Back to edit | 关闭审查，清当前 leave intent，保留 dirty | 无写命令 |
| Review valid → Save | 分配 request_id，转 Planning | 恰好一个 PlanRedisMutation |
| Review Warning → 普通 Save action | 不允许绕过 warning | 无写命令 |
| Review Warning → Save anyway | 仅对当前 warning 快照授权，转 Planning | 恰好一个 PlanRedisMutation |
| Planning → matching PlanReady | 存有效 plan，转 Saving | 恰好一个 ExecuteRedisMutation |
| Planning/Saving → Enter repeat/Save/Apply | 忽略重复提交 | 无额外写命令 |
| Planning/Saving → Esc/切换 overlay/编辑 | 保持已提交状态，明确等待 | 不伪造远程取消 |
| matching failure | Failed，保留草稿/错误/意图 | 不续行 leave |
| Failed → Retry | 重新校验归属并重新规划，禁止复用过期 plan | 一个新的 Plan 命令 |
| Failed → Back to edit | 清 leave，回原编辑器 | 无写命令 |
| matching success | 更新提交快照对应 baseline，通知并刷新正确 key | 普通保存停留；有 leave 才续行一次 |
| 过期/不匹配 callback | 不关闭新 UI、不清新 dirty、不消费 leave | 不产生后续 mutation/leave |

若保存期间检测到更新 revision，不能 `set_text` 覆盖新 buffer。只记录实际提交的文本为 baseline，保留新文本 dirty；不继续 leave，不触发会覆盖新文本的刷新。正常 UI 已锁定提交期间编辑，但 reducer 仍要防迟到事件。

### 1.3 视觉与操作规格

- 保存框：KEY、TYPE、DB、FORMAT、`Preserve TTL`、有限字节摘要；隐藏实现用 revision 数字。正常 Save/Cancel 默认 Save。
- Warning：保留 JSON/YAML parser 具体错误，解释按当前文本保存；Save anyway / Back to edit 默认后者。
- 未保存离开：突出源 KEY，并描述目的 key/Close tab/Disconnect/Quit；Save / Discard / Cancel 默认 Cancel；Discard 为 Danger。
- 删除行：KEY + 类型化 row identity；Cancel / Delete 默认 Cancel，Enter 在 Cancel 上也能关闭。
- 正常按钮 Tab/Shift-Tab、Left/Right 循环，Enter 激活，Esc 取消；未保存框继续支持 s/d；表单文本区左右键仍移动光标。
- 键盘与鼠标分派同一语义 action；忙碌按钮 disabled，无点击命中区；release/repeat 不得发重复写入。
- 采用 `dialog::render_frame`、`render_actions`、`shortcut_hints`；布局为上下文、正文、操作、提示。按钮实际宽度不够时纵排并分配足够行数；小屏优先保留动作，极小屏裁剪且不 panic。
- 所有动态 key/value/error/profile 文本先 sanitize，再按显示宽度折行/截断；长错误应可读（必要时局部滚动），不覆盖 footer。
- 成功沿用 notification，不新增成功模态框。错误留在当前工作上下文，不迫使用户重新输入。

## 2. 单元一：文本保存端到端闭环（首先实施）

**Files**
- Create: `src/model/redis_dialog.rs`、`src/ui/redis_dialog.rs`、`tests/redis_value_save.rs`
- Modify: `src/model/mod.rs`、`src/model/workspace.rs`、`src/action.rs`、`src/app.rs`、`src/input/keymap.rs`、`src/input/mouse.rs`、`src/ui/mod.rs`
- Reuse: `src/model/redis_value_edit.rs` 的 owner、`src/value_preview/edit.rs` 编码/校验、`src/ui/dialog.rs`
- Test: `tests/keymap.rs`、`tests/mouse.rs`、`tests/ui_render.rs`

### Task 1.1 — 保存快照与 reducer 行为

1. 参考 `tests/redis_unsaved_changes.rs` 构造已连接且 profile registry 可分配 request_id 的 String fixture；通过编辑 action 建立 dirty，不只直接初始化 model。
2. 新增 `tests/redis_value_save.rs`，首批测试命名建议：
   - `save_confirmation_plans_and_executes_once_without_object_form`
   - `save_cancel_preserves_dirty_buffer`
   - `clean_save_does_not_plan_mutation`
   - `stale_plan_or_result_cannot_complete_another_save`
3. 测试调用 `App::update`，提取返回的 `Command::PlanRedisMutation`，用现有 plan builder 构造响应并送 `Action::RedisMutationPlanReady`，断言恰好一个 Execute；再送成功/失败并核对 baseline 与 overlay。不要假设真实 Redis 可用。
4. 先运行 `cargo test --test redis_value_save`，记录预期失败；随后增加 model/overlay/Action 和 App helper：捕获保存、确认提交、归属匹配、完成保存。替换 `EditorEffect::SaveRequested` 与 `RedisValueSave*` 原来跳转 `RedisObjectEditor` 的链路。
5. 统一 request 分配、readonly/connection/target/完整加载检查；快照重用原 `RedisMutationOperation::Replace(draft)` 和 TTL Preserve，不在本次偷偷替换底层写入策略。
6. 在 `RedisMutationPlanReady` 自动发 Execute；对 duplicate PlanReady 要求当前 phase == Planning。成功只处理匹配 request；失败保留快照与错误。
7. 更新已有构造旧 overlay 的测试以匹配新字段；再次运行上述定向测试。

**验收**：用户一次 Save 确认后完成规划→执行→成功/失败，不出现对象表单，不要求第二次 Enter；不会将迟到事件应用于另一文档。

### Task 1.2 — 格式校验和无损编码

1. 增加行为用例：RAW 字符串 `0x4142` / `hex:4142` 保留字面字节，Hex `00 ff 41` 变为 `[0,255,65]`，无效 Hex 拒绝提交，JSON/YAML warning 包含 parser error，SaveAnyway 只对 warning 生效。
2. `RedisSaveSnapshot.draft` 用 `draft_from_string(text, actual_preview_format)` 构造；warning 用 `validate_string` 保存完整结果。不要借用对象表单 `parse_bytes`。
3. reducer 再次验证当前 state 的 warning/phase/owner；不把 `Save` 与 `SaveAnyway` 无条件合并。保存确认时 revision 已变化则重新审查或退回编辑，不能读取当前文本替换已确认快照。
4. 添加 failure/retry/busy duplicate 与 newer revision result 的用例，断言 failure 不清 dirty、重试用新 request、更新文本不被回调覆盖。
5. 运行 `cargo test --test redis_value_save`。

### Task 1.3 — 保存框样式、键鼠、帮助

1. 建立 `redis_dialog` renderer，使用共享 frame/action/hint；在 `src/ui/mod.rs` 注册并派发新的保存状态，移除保存场景的 render_message。
2. 添加 typed focus/activate Actions 与 HitTargets；`src/input/mouse.rs` 将按钮点击映射到同一 action。
3. keymap 优先处理本 modal，再允许可能覆盖它的 omni/open-consoles 等全局入口；仅对本任务 modal 限制，不重写其他 overlay 规则。
4. 添加真实 route 测试：Tab/BackTab 环回、正常默认 Enter Save、warning 默认 Enter 返回编辑、Esc、点击可见按钮、busy 不提交、Repeat 不提交、底层 editor 未收到按键。
5. TestBackend 验证 key/type/db/error、按钮焦点与颜色、busy 文案、没有 revision 调试摘要和无关表单。
6. 在 `src/help.rs` 的 context/catalog 中同步保存相关提示，`docs/keybindings.md` 更新新键位；Task 5 再做全场景一致性审查。
7. 运行 `cargo test --test redis_value_save --test keymap --test mouse --test ui_render`。完整通过后才提交此业务单元，建议 `fix(redis): unify value save confirmation and submission`。

## 3. 单元二：未保存离开完整闭环

**Files**
- Modify: `src/model/redis_dialog.rs`、`src/model/workspace.rs`、`src/app.rs`、`src/action.rs`、`src/input/keymap.rs`、`src/input/mouse.rs`、`src/ui/redis_dialog.rs`、`src/ui/mod.rs`、`src/help.rs`、`docs/keybindings.md`
- Test: `tests/redis_unsaved_changes.rs`、`tests/redis_value_save.rs`、`tests/quit_transaction_review.rs`、`tests/keymap.rs`、`tests/mouse.rs`

### Task 2.1 — 原意图与取消语义

1. 新增以下 reducer 回归测试，再实现：
   - 切 key→Save→warning→Back to edit 后，后续普通保存不会切 key。
   - 未保存框显示源 key，不把目标 key 误作 dirty 对象。
   - Save valid 直接进入已授权提交链，不再出现普通 Save 确认。
   - Save invalid 进入与直接保存相同 warning；SaveAnyway 成功才续行。
   - Discard 恢复 baseline 后只 replay 一次；Cancel 清 intent 不改文本。
2. 将 `pending_redis_open_key` / `pending_redis_close_tab` / `pending_redis_disconnect_profile` 三个互斥 Option 合并为 typed intent；保存 state 持有本次 intent，回调只能消费匹配提交的 intent。
3. 让 open-key/close-tab/disconnect/quit 统一调用 dirty guard；用 origin owner 表示当前待处理草稿，intent 表示最终目的，两者不要混为同一个 key。
4. Cancel、Back to edit、不可继续的 owner 失效都清当前 leave flow。Failed 保留意图以允许用户重试，绝不自动续行。
5. 渲染 Save/Discard/Cancel 三按钮并同步键鼠/help；直接 s/d 必须走同一 action，不绕过 phase。
6. 运行 `cargo test --test redis_unsaved_changes --test redis_value_save`。

### Task 2.2 — Quit/Disconnect 与多个 dirty tab

1. 新增 first clean + second dirty、多个 dirty、不同 profile、取消第二个草稿的测试。fixture 明确保持两个不同 editor/owner。
2. 把 `find(tab) → dirty(tab)` 改为筛选满足操作范围的 dirty tab；Quit 遍历所有 Redis，Disconnect 只遍历对应 profile。
3. 逐个 Save/Discard 成功后再检查剩余 dirty，然后重放原 Quit/Disconnect。Quit 不用 CloseTab 代替，不在处理第一个草稿时关闭整个应用。
4. 续行原 Action::Quit/断连入口前先 take 已解决 intent，避免递归重放自身；原 SQL transaction review 和 workspace persistence guard 继续正常运行。
5. 加入 unrelated/stale mutation success 不能消费当前 leave 的测试；断连期间 profile generation 改变不能保存旧快照。
6. 运行 `cargo test --test redis_unsaved_changes --test redis_value_save --test quit_transaction_review --test keymap --test mouse`。
7. 提交建议：`fix(redis): preserve unsaved leave intent across save decisions`。

**验收**：每个离开目的都准确执行；任意 Cancel 保留对应 dirty 并停止离开；多 tab 不漏检；失败与过期回调不误跳转。

## 4. 单元三：对象、集合行编辑和删除

**Files**
- Modify: `src/model/redis_object_editor.rs`、`src/model/redis_table_editor.rs`、`src/ui/redis_object_editor.rs`、`src/ui/redis_table_editor.rs`、`src/app.rs`、`src/action.rs`、`src/input/keymap.rs`、`src/input/mouse.rs`、`src/ui/mod.rs`、`src/help.rs`、`docs/keybindings.md`
- Test: `tests/redis_object_editor.rs`、Create `tests/redis_table_dialogs.rs`、`tests/ui_render.rs`、`tests/keymap.rs`、`tests/mouse.rs`、`tests/redis_mutation.rs`

### Task 3.1 — 表单提交与错误恢复

1. 添加对象新建/整体编辑与行新增/编辑的 App/Command 测试：一次 Apply→PlanReady 自动 Execute；duplicate/busy 不执行；失败保留字段；Retry 重新规划；成功只刷新匹配对象。
2. 使用和文本保存一致的提交阶段概念；删除原来 `plan: Some` 时等待第二次 Apply 的行为。对象显式 `0x`/`hex:` 输入、类型/TTL 原有语义保留。
3. 对 object 和 table 一致禁止 busy cancel/焦点编辑；所有 input/paste/type/TTL 路径检查 phase。编辑后失效旧 plan。
4. 将表单改用共享 frame/action/hint：编辑内容、上下文、错误区、Apply/Cancel 操作；按钮和字段都加入明确焦点循环。
5. 行字段名来自现有 table 类型/列语义：Hash field/value、List index/value、Set member、SortedSet member/score、Stream id/field/value 按实际模型支持显示。identity-only 字段只读；针对 `operation()` 只提交 focused column 的现实，编辑模式明确只允许当前操作支持的字段，不能让用户同时改多列却只保存其中一列。
6. 活跃单行字段复用 `render_text_input`，可见 cursor、长文本横向视窗与文本编辑快捷键；对象 value 的多行表示维持现有输入能力，不新造一个文本编辑器。
7. 运行 `cargo test --test redis_object_editor --test redis_table_dialogs --test redis_mutation`。真实 Redis 测试若没有配置，记录 skip，不能算在线通过。

### Task 3.2 — 行删除一次确认

1. 测试 Cancel 默认焦点 Enter 可退出、Delete 确认后自动规划执行、busy 无法误关、failure 保留 row identity、迟到回调不删另一行。
2. 行删除用专用确认/提交 state，或现有确认 state 增加 phase/request/plan；不呈现一个可编辑的 row form 来等待第二次确认。
3. renderer 使用共享按钮，说明准确 row identity 与 key；Delete 为 Danger。BackTab、左右、鼠标和 Esc 与其他确认保持一致。
4. 恢复错误时明确 Retry/Cancel；执行中的取消不声称已撤销。对于结果不确定的网络失败不给自动重试。
5. 运行 `cargo test --test redis_table_dialogs --test keymap --test mouse --test ui_render`，通过后提交建议：`feat(redis): align object and row dialogs with shared controls`。

**验收**：Hash/List/Set/SortedSet/Stream 已支持的新增/编辑/删除动作各有准确表单和结果；不改变 Redis 操作支持范围；不会丢失用户输入或隐藏第二次确认。

## 5. 单元四：邻接弹窗、适配、文档与全量验证

**Files**
- Modify as needed: `src/ui/redis_dialog.rs`、`src/ui/redis_object_editor.rs`、`src/ui/redis_table_editor.rs`、`src/ui/mod.rs`、`src/help.rs`、`docs/keybindings.md`
- Shared `src/ui/dialog.rs` 仅在确实需要公共布局修复时修改，同时测试现有 SQL/删除对话框。
- Test: `tests/ui_render.rs`、`tests/redis_help.rs`、`tests/keymap.rs`、`tests/mouse.rs`、`tests/redis_key_delete.rs`、`tests/redis_preview_serialization.rs`、`tests/redis_yaml_preview.rs`

### Task 4.1 — 窄屏与动态文本

1. 用 TestBackend 覆盖 120×36、80×24、40×15、20×8、1×1。前两者信息/操作完整；窄屏纵排、正文受控裁剪/滚动；极小屏不 panic/越界，不注册不可见命中区。
2. 场景包含超长 key、中文与 emoji、二进制 key、含 ANSI 控制字符的 error/value、多行 parser error、disabled 状态。
3. 断言按钮区域在弹窗内部且不重叠正文；实际可见按钮点击触发正确 action。不要只断言快照包含固定标题。
4. `RedisPreviewFormat` 保留列表语义，统一 frame/选中样式/hint；选中格式按原支持规则，不借本任务扩展编码能力。
5. 回归已有 `RedisDeleteConfirm`（整个 key/prefix）和 relation transaction review，确保共享 helper 变更没有造成截断/命中回归。

### Task 4.2 — 快捷键目录与用户文档

1. 审查 `src/help.rs` 的 `ShortcutContext`、`SHORTCUT_CATALOG`、context mapping、executable action 映射；保存、unsaved、行删除、表单四类上下文不再错误使用 Message dismiss 提示。
2. `docs/keybindings.md` 列出本次 Tab/Shift-Tab、Left/Right、Enter、Esc、s/d；说明 busy 时等待、warning 默认返回编辑。
3. footer、帮助、真实 keymap 三者一致；不存在仅显示但不可执行的动作。无配置键默认值变动则不修改 `config/default.toml`。
4. 运行 `cargo test --test ui_render --test redis_help --test keymap --test mouse --test redis_key_delete --test redis_preview_serialization --test redis_yaml_preview`。
5. 提交建议：`test(redis): cover dialog layout and interaction contracts`，包含相关文档和最后修复。

### Task 4.3 — Luna 收尾审查与强制检查

1. 阅读实际 diff，对照以下最终矩阵逐项验收；检查未意外 stage 其他任务文件。由 Luna 执行审查和修正，不切回 Astra。
2. 功能齐备后执行项目要求的三条命令：

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

期望：各退出 0；fmt 无差异、clippy 无 warning、全部可运行测试通过。记录实际退出结果、测试跳过及环境依赖。失败修复后只重跑必要检查，不在每个提交重复全量检查。

3. TestBackend + reducer 是必需的可重复验证。PTY/真实 Redis 属于补充检查；若有可用 Redis 使用现有 `tests/redis_mutation.rs` 环境变量契约进行 round-trip，并记录确实执行的 case。不要创建新的环境要求来阻塞交付。
4. PTY/外部环境受限最多一次针对性修复重试，随后记录限制，Luna 判断是否需要其他证据。编译/逻辑错误不是外部阻塞，继续修复。
5. 写任务目录 `validation.md`：每组命令、退出结果、当时 commit/diff、环境、相关文件；不能用分析阶段的静态检查冒充当前测试结果。
6. 按工作流执行提交/合并前检查并集成 main 并行变化。合并导致相关代码变化时重跑受影响检查；最后写当前阶段新指定回执，不重写历史 receipt。

## 6. 最终验收矩阵

| 场景 | 必须验证 |
| --- | --- |
| RAW 普通保存 | 单次确认；原始字节不因 0x/hex: 被重新解释；TTL Preserve |
| JSON/YAML 正常/异常 | 错误可读；正常 Save；异常必须 SaveAnyway，默认返回编辑 |
| Hex | 有效字节解码；错误阻止提交，没有强制保存错误编码入口 |
| clean/取消/失败 | clean 无写入；取消保留 dirty；失败保留草稿；Retry 不重复旧 request |
| busy | Planning/Saving 文案准确；重复 Enter/Repeat/鼠标不重复 Execute；不能覆写 modal |
| 异步过期 | tab/key/generation/request/revision 不匹配不能清新内容或执行 leave |
| 切 key/关 tab | 显示源对象和目的；Save/Discard 成功才离开；Cancel 停止 |
| Quit/Disconnect | first clean + second dirty、多 dirty、profile 范围；逐项确认后继续原意图 |
| 与 SQL 混合退出 | Redis guard 完成后仍进入原事务/持久化确认，不提前退出 |
| 对象与集合行 | 有语义字段/输入光标/错误/操作区；一次 Apply；仅提交界面承诺的字段 |
| 行删除 | 默认 Cancel；Cancel Enter 有效；Delete 一次确认；不跳可编辑表单 |
| 键鼠帮助 | Tab/BackTab/左右/Enter/Esc/s/d 与鼠标等价；帮助准确；不泄漏底层编辑器 |
| 屏幕和文本 | 长 key/Unicode/控制字符/多行 error；窄屏不遮操作；极小屏不越界 |
| 范围回归 | 对象新建、TTL、collection targeted mutation、key/prefix 删除、SQL review 保留原语义 |

## 7. 交接状态

计划已明确四个端到端闭环；实现起点是 **Task 1.1：新增文本保存 reducer 回归测试并打通一次确认提交**。随后同一执行阶段持续完成其余单元，不以“本轮复核完毕”结束，不要求用户反复 resume。任务命名、分支及实现由后续 Luna 工作流接管。
