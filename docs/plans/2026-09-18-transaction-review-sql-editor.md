# Transaction Review SQL Editor Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **OpenCode 执行约定：** 按任务依赖顺序实施；若没有上述技能，使用当前可用工具逐项执行并记录验证结果。默认由单个执行者完成。本文交付实施计划，列出的命令和预期结果不代表已经执行。

**Goal:** 将 Transaction Review 的 SQL preview 纳入 Tab/Shift-Tab 焦点循环，提供与 DDL 一致的只读 Vim 导航、搜索、选择复制和滚动条交互。

**Architecture:** Review 持有独立只读 editor session、审核 SQL 快照及局部焦点，复用 EditorWorkspace、ReadOnlySqlEditor 和现有鼠标选择/滚动条实现。通过 session 显式标识串联键盘、鼠标、复制、viewport 与生命周期；在 Review 上下文中限制编辑器应用级 effects，并统一鼠标与键盘的事务确认入口。

**Tech Stack:** Rust 2024 / Rust 1.94、Ratatui 0.30.2 TestBackend、Crossterm 0.29、现有 Modalkit 0.0.25 集成；使用现有测试 fixture。

---

## 0. 基线与协作接口

- 分析基线 HEAD：`527acb66edaf00779862e1831f4c1929c602c285`。
- 开始实施时记录 `git rev-parse HEAD`、`git status --short` 和相关 diff；本文行号只用于导航，以实际符号为准。
- 保留现有未跟踪 `.git-opencode-tasks/` 和其他 `docs/plans/*.md`，提交时只暂存本任务文件。
- `docs/plans/2026-09-18-relation-mutation-capabilities-and-sql-preview.md` 也涉及 `open_relation_transaction_control`、SQL 快照和提交校验：本任务从该函数得到的 SQL/审核身份初始化 session。若它已引入 mutation plan/fingerprint，则直接使用新的审核身份，不恢复旧的 Debug 字符串快照，也不另造 SQL 生成器。
- `docs/plans/2026-09-17-redis-value-vim-experience.md` 涉及 prompt owner、只读 prompt 粘贴、共享 renderer。执行前检查这些能力是否已落地；已有能力直接复用。
- 所有新测试过滤前缀使用 `transaction_review_`；编辑器基础回归用 `read_only_review_`，共享布局回归用 `read_only_sql_layout_`。

### 0.1 已确认的代码入口

| 事项 | 位置与当前行为 |
| --- | --- |
| Review 状态 | `src/model/workspace.rs:183`，`choice / sql / preview_offset / edit_snapshot` |
| Review 打开 | `src/app.rs:14918`，`open_relation_transaction_control`，初始 Cancel |
| Review 确认 | `src/app.rs:9919–10036`，两条确认分支、按钮循环与取消 |
| Review 键盘 | `src/input/keymap.rs:539`，Tab 仅切按钮，Enter 无条件确认 |
| Review UI | `src/ui/mod.rs:4747`，SQL 高亮行加 Paragraph |
| DDL 共享 UI | `src/ui/relation.rs:1120`，使用 `ReadOnlySqlEditor` |
| 通用 renderer | `src/ui/read_only_sql.rs`，行号、正文、prompt、cursor、selection、scrollbars |
| Editor 会话 | `src/editor/mod.rs:362 / :425`，`open_read_only / close_console` |
| 基础只读检查 | `src/editor/mod.rs:2205 / :2876`，按键和 Modalkit action capability 检查 |
| Visual 缩进旁路 | `src/editor/mod.rs:2773 / :2988`，直接调用 `indent_target`，需要只读回归 |
| 应用级 effects | `src/app.rs:16941`，会分发运行 SQL、切 tab、事务等动作 |
| Prompt 粘贴 | `src/editor/mod.rs:2448`，目前优先写入已有 prompt，需核对 owner |
| 鼠标入口 | `src/input/mouse.rs`，overlay 的 Down/Drag/Up/Scroll 路由与白名单 |
| 鼠标复制 | `src/app.rs:6404`，按 TextGestureSource 校验 session/revision |
| Editor 鼠标光标 | `src/app.rs:9572`，当前通过 `mouse_session_focus` 修改工作区焦点 |
| DDL 视口同步 | `src/runtime.rs:6239`，通过 Action 更新实际 editor viewport |
| Review 帮助 | `src/help.rs:210`，目前共用 TransactionExitConfirmation 上下文 |

## 1. 已确定的产品交互

### 1.1 焦点和模式

```text
Tab:       SQL preview → Commit → Rollback → Cancel → SQL preview
Shift-Tab: 上述顺序反向
初始焦点:  Cancel
```

1. 初次 Tab 从 Cancel 进入 preview；Shift-Tab 从 Cancel 到 Rollback。
2. Preview 聚焦时，边框/标题使用焦点样式，显示编辑器光标；三个按钮均不显示焦点高亮。
3. 点击 preview 正文、空白正文或滚动条均进入 preview 焦点。点击按钮直接激活相应动作，语义沿用现有按钮。
4. 按钮焦点下 Left/Right 只在三个按钮间循环；preview 中 Left/Right 属于编辑器。
5. 无 SQL 时依然保留 preview 焦点节点，正文显示现有 unavailable 提示；提示文字不作为 SQL 入库、不成为复制源。
6. 丢失焦点时保留光标和横纵 offset，退出 Visual、取消未提交 prompt、清除 pending count/operator/binding；重新聚焦为 Normal。

### 1.2 键盘优先级

| 条件 | 行为 |
| --- | --- |
| 任意 Review 焦点，Tab/Shift-Tab | 改变局部焦点，优先于 editor prompt 的 Tab |
| Preview 有 prompt | Enter 提交 prompt；Esc 取消 prompt；其他输入进入该 session 的 prompt |
| Preview 处于 Visual 或存在 pending sequence | Esc 只取消当前编辑器交互，保持弹窗 |
| Preview 普通 Normal | Esc 取消 Review；Enter 不激活事务按钮 |
| Preview 普通键 | 转发只读 editor，复用现有 motions、数字计数、Visual/yank、搜索、翻页 |
| 按钮 Enter | 根据当前按钮调用统一确认入口 |
| 按钮 Esc | 取消 Review |
| 粘贴到 preview prompt | 仅写该 session 的 prompt，不能改 SQL |
| 粘贴到 preview 正文或按钮 | 无操作 |

支持范围以现有 Vim 引擎为准：`hjkl`、`w/b/e`、`0/^/$`、`gg/G`、计数、`v/V/Ctrl-v`、`y/yy`、`/ ? n N`、`Ctrl-u/d/b/f`、PageUp/PageDown 等。不把 DDL 页面级 `is_read_only_editor_key` 原样用于 Review，因为该 helper 排除了 `1/2/3` 等工作区快捷键。

Ex prompt 复用现有解析器；只允许 Review 内有效的本地查看/复制效果。`:q` 明确映射为取消当前 Review。执行 SQL、提交/回滚、改变连接、切 tab、改变底层面板和保存等应用级效果在该上下文中不执行；Ex 中的不适用命令给出当前 prompt 的错误反馈，不替换 Review overlay。

### 1.3 显示和鼠标

- SQL 使用原始逻辑行，默认不自动折行；长行可横向滚动。
- 行号、高亮、选择背景、光标和 scrollbar 使用 DDL 的共享实现。
- 拖选松开后沿用已有自动复制行为；单击只定位，不复制。
- 鼠标滚轮只滚动命中的 preview；拖动开始后允许指针离开正文/轨道，使用现有 clamp 算法。
- 滚动条命中优先于正文，行号不加入 SQL 选区；prompt 行也不属于正文选区。
- 复制来自 `mouse_range_text` 或 session 原文，不从终端 Buffer/渲染字符串反推。

## 2. 目标模型和数据流

### 2.1 Review 独立状态

新增 `src/model/transaction_review.rs`，由 `src/model/mod.rs` 导出。

焦点定义与转换可以直接采用以下实现：

```rust
use crate::model::transaction::TransactionExitChoice;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TransactionReviewFocus {
    SqlPreview,
    Commit,
    Rollback,
    #[default]
    Cancel,
}

impl TransactionReviewFocus {
    pub fn next(self) -> Self {
        match self {
            Self::SqlPreview => Self::Commit,
            Self::Commit => Self::Rollback,
            Self::Rollback => Self::Cancel,
            Self::Cancel => Self::SqlPreview,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::SqlPreview => Self::Cancel,
            Self::Commit => Self::SqlPreview,
            Self::Rollback => Self::Commit,
            Self::Cancel => Self::Rollback,
        }
    }

    pub fn choice(self) -> Option<TransactionExitChoice> {
        match self {
            Self::SqlPreview => None,
            Self::Commit => Some(TransactionExitChoice::Commit),
            Self::Rollback => Some(TransactionExitChoice::Rollback),
            Self::Cancel => Some(TransactionExitChoice::Cancel),
        }
    }
}
```

`TransactionReviewState` 采用 `Clone, Debug, Eq, PartialEq`，保存以下字段：

| 字段 | 类型 | 用途 |
| --- | --- | --- |
| `tab_id` | `Uuid` | 被审核表，允许不是 active tab |
| `prompt` | `Option<DeferredTransactionPrompt>` | 关闭/退出等后续意图 |
| `focus` | `TransactionReviewFocus` | 唯一焦点来源，替代 choice |
| `editor_session_id` | `Uuid` | 每次打开独立创建 |
| `sql` | `String` | 本次审核 SQL 原文 |
| `dialect` | `SqlDialect` | 来自被审核表的 profile |
| `edit_snapshot` | `Option<String>` | 继承当前提交校验；已有新 fingerprint 时用新模型 |

Overlay 采用 `RelationTransactionConfirm(TransactionReviewState)`，统一迁移所有 match 和 fixture。删除 Review 的 `preview_offset`，保留其他弹窗自己的 offset。Review 不与 DDL 共用 session ID。

### 2.2 Action 和 helper 合约

按现有命名习惯增加以下小型接口，具体可见性保持最小：

- `TransactionReviewFocusNext` / `TransactionReviewFocusPrevious`：四项焦点循环。
- `TransactionReviewMoveButton(isize)`：按钮左右移动，preview 状态下无操作。
- `TransactionReviewFocusPreview { session_id }`：点击空白正文/滚动条时聚焦。
- `ReadOnlyEditorViewportChanged { session_id, viewport }`：验证当前 Review 会话后同步 viewport。
- `ReadOnlyEditorPromptPaste { session_id, text }`：仅当当前 preview 聚焦且 prompt owner 匹配时调用。
- 继续使用 `ReadOnlyEditorKey`、`ReadOnlyEditorScroll`、`EditorScrollBy`、`EditorSetScrollAxis`，接入 Review session 校验。
- `review_preview_target()`：返回当前 overlay 所属 session 信息；鼠标可在按钮聚焦时使用它。
- `focused_review_preview_target()`：额外检查 preview 焦点；键盘/复制使用它。
- `cancel_session_interaction(id)`：取消该 session 的 prompt/Visual/pending，保持文本、revision、位置和 viewport。
- `session_has_pending_input(id)`：覆盖 wrapper pending 与 Modalkit operator 状态，供 Esc 判定；不只检查 `pending_binding`。
- `resolve_relation_review(choice)`：统一确认、审核身份校验、session 释放及 deferred intent。
- `close_relation_review_session(id)`：统一释放 editor session，供确认、取消及 overlay 被替换的路径使用。

Editor key 到 effects 的分发必须携带来源 session。Review 使用局部 effect handler，处理 Yank/CopyBuffer/CopyStatement/局部关闭；其余保持原上下文 handler。不要让 Review 的 effects 经 `active_console_mut()` 或 active-tab 推断归属。

### 2.3 三条完整链路

```text
Keymap → Review 局部按键 → ReadOnlyEditorKey(session)
       → EditorWorkspace → Review effects → Clipboard / 本地关闭

Review layout → snapshot(viewport) → ReadOnlySqlEditor
              → UiState viewport → Runtime → session.set_viewport

Mouse hit → 当前 overlay/session 校验 → focus/cursor/drag
          → source position + revision → mouse_range_text → Clipboard
```

## Task 1：引入 Review 状态并建立会话生命周期

**文件**
- 新增：`src/model/transaction_review.rs`。
- 修改：`src/model/mod.rs`、`src/model/workspace.rs`、`src/app.rs`、`src/editor/mod.rs`。
- 迁移调用方：`src/input/keymap.rs`、`src/ui/mod.rs`、`src/help.rs`，以及受 Overlay 构造影响的测试。
- 测试：`src/app.rs` 内部 tests、`tests/quit_transaction_review.rs`、`tests/ui_render.rs`。

**步骤 1：建立真实打开路径的回归。**

基于 `RelationTab::new` 设置 Active 和 `transaction_review_sql`，通过 `Action::OpenTransactionControl` 打开，验证 session 已存在、ReadOnly、Normal、文本正确、ID 不同于 ddl_editor_id。Quit 场景使用非活动 relation fixture，验证 session 对应审核表且 active tab 不变。

先运行：`cargo test --lib transaction_review_ -- --nocapture`。新增会话断言在实现前应失败；若测试引用新类型未定义，记录编译失败并继续模型接入。

**步骤 2：迁移状态。**

按第 2 节定义模型、Overlay、初始化焦点；迁移所有模式匹配。现有 UI 构造测试改为通过真实打开路径建 session，避免仅伪造 overlay 后渲染一个不存在的 session。

**步骤 3：在打开入口创建一次 session。**

先从被审核表提取 SQL、dialect、prompt、审核身份，再结束 tab 借用并创建 session；重绘和焦点切换都不重建。保留 local edits 与 Active transaction 选择 SQL 的既有分支。`ensure_read_only_session` 识别当前 Review，不能凭已关闭 ID 重建会话。

**步骤 4：统一销毁路径。**

在确认、取消、Dismiss/overlay 替换及源 tab 失效路径释放会话；`close_console` 同时取消该 ID 拥有的 prompt，不影响其他 session。替换检测只做局部/既有集中入口复用，避免为此改写所有 overlay 类型。鼠标手势通过当前 session 的有效性在事件/重绘时清理。

**步骤 5：验证生命周期。**

增加关闭后 session 缺失、重开新 ID、DDL 文本/位置未变、旧 session Action 无操作、其他 session prompt 未误清理、重复 Quit 不重建 Review 的断言。

运行：

```bash
cargo test --lib transaction_review_ -- --nocapture
cargo test --test quit_transaction_review
cargo test --test ui_render relation_transaction_review_ -- --nocapture
```

预期：所有命令成功，过滤测试实际命中；此阶段原 UI 可暂时从只读 snapshot 的 offset 读取兼容状态，最终 Task 4 完整替换。不要为编译保留第二份可变滚动状态。

建议逻辑提交：`refactor(transaction): own review state and preview session`。

## Task 2：接入四项焦点循环和分层按键路由

**文件**
- 修改：`src/action.rs`、`src/input/keymap.rs`、`src/app.rs`、`src/editor/mod.rs`。
- 测试：`tests/keymap.rs`、`src/editor/tests.rs`、`src/app.rs` 内部 tests。

**步骤 1：添加用户输入路径测试。**

通过 Keymap → App.update 输入，不只断言 Action 枚举。新增：

- `transaction_review_tab_cycles_preview_and_buttons`：Cancel 开始四次 Tab 回 Cancel，四次 BackTab 反向。
- `transaction_review_shift_tab_normalization`：`Tab + SHIFT` 与 BackTab 等价。
- `transaction_review_enter_in_preview_does_not_confirm`：先选 Commit 再进入 preview，Enter 不产生 mutation/commit 命令。
- `transaction_review_arrows_follow_focused_region`：按钮左右循环；preview 左右移动光标。
- `transaction_review_navigation_keeps_numeric_counts`：`12j`、`gg`、`G` 真正改变 preview 光标。

运行：`cargo test --test keymap transaction_review_ -- --nocapture`，预期新增场景在实现前失败。

**步骤 2：添加局部 Action 和 reducer。**

实现第 2.2 节的焦点 Action；与 console 的 TransactionExitChoice 循环区分。新 Action 在 `App::update` 开头的非 console action gate 中可达，同时依照 overlay owner 校验，不依赖 active tab 必须为 Relation。

**步骤 3：调整 Keymap 优先级。**

在 Review 的局部处理入口先消费 Tab/BackTab，然后处理按区域划分的 Enter/Esc/左右键，最后转发只读键盘。检查 `Keymap::map` 前面的 omni、console list、历史/全局绑定：Review 的文字输入和 prompt 不能被工作区快捷键抢走。保持已打开 omni 自己的输入所有权。

**步骤 4：实现 session 交互取消。**

提供 pending 状态查询，覆盖 `g`、数字计数、`y` 等未完成 operator。取消时复用 Vim engine 的 Escape/reset 能力，清理 wrapper pending_count/pending_binding/current_sequence；仅调用现有 `set_mode(Normal)` 不足以保证所有 pending 都被清掉。

**步骤 5：验证嵌套状态。**

增加 `transaction_review_escape_unwinds_editor_before_closing` 和 `transaction_review_blur_preserves_position_but_clears_pending`：分别覆盖搜索、Visual、`g`、`12`、`y` 后 Esc/Tab。重新进入后输入 `j` 只移动一行，不接续旧命令；文本和 revision 相同。

运行：

```bash
cargo test --test keymap transaction_review_ -- --nocapture
cargo test --lib read_only_review_ -- --nocapture
```

建议逻辑提交：`feat(transaction): focus SQL preview in review dialogs`。

## Task 3：收敛只读能力、搜索粘贴和 Review effects

**文件**
- 修改：`src/editor/mod.rs`、`src/editor/prompt.rs`（仅 owner/错误接口需要时）、`src/app.rs`、`src/action.rs`、`src/input/keymap.rs`。
- 测试：`src/editor/tests.rs`、`src/app.rs` 内部 tests、`tests/keymap.rs`。

**步骤 1：先锁定只读与来源隔离。**

添加表驱动编辑命令测试，至少包含 `i/a/o/O/R`、`x/dd/cw/p`、`u/Ctrl-r`、`>>`、`V>`、Visual block 缩进、替换 prompt；每个序列从新 session 开始，断言文本和 revision 不变。另测 `ggVGy` 保留多行原始空白，不能只断言“没有 panic”。

运行：`cargo test --lib read_only_review_ -- --nocapture`。Visual 缩进旁路是否失败以实际结果为准。

**步骤 2：补齐统一只读检查。**

在直接写 buffer 的缩进等入口检查 capability，保证旁路也受约束。复用现有 Modalkit `is_readonly` 检查，不手工维护一套全部 Vim 编辑键黑名单。向只读正文直接 paste/insert 的路径同样守住 capability。

**步骤 3：接通 prompt owner 粘贴。**

`map_paste` 仅在 Review preview 聚焦且 prompt 属于该 session 时生成 prompt-paste Action；reducer 再验证一次。editor 粘贴检查 owner，不能因为“任意 prompt 存在”就写入。搜索框支持 Unicode/多字符粘贴；正文粘贴无操作。

**步骤 4：按来源 session 分发 effects。**

Review 的 `Yanked` 直接转换 WriteClipboard；CopyBuffer/CopyStatement 明确读取 Review session，复用现有选择/全文语义。`:q` 取消本 Review，其他工作区级效果不执行。Ex 不适用命令设置本地错误；不通过全局通知替换弹窗。应用级 effect 列表逐一匹配，新增 enum variant 时要求显式处理。

`active_read_only_session_id` 优先解析当前 overlay：按钮焦点返回 None，不能继续落到底层 DDL/输出。即使保留该 helper，Review effect handler 仍使用已知 session，避免 active tab 切换造成歧义。

**步骤 5：验证不会穿透工作区。**

添加 `transaction_review_effects_stay_in_preview`：`gt/gT`、Ctrl-W 组合、Ctrl-S、现有执行/事务 Ex 命令不改变 active_tab、workspace focus、连接或事务；`:q` 只关闭当前 Review。命令拼写以项目现有 parser 为准。正常 `/pattern` Enter、`n/N` 与 yank 依然工作。

运行：

```bash
cargo test --lib read_only_review_ -- --nocapture
cargo test --lib transaction_review_ -- --nocapture
cargo test --test keymap transaction_review_ -- --nocapture
```

建议逻辑提交：`fix(editor): isolate read-only review interactions`。

## Task 4：复用 DDL renderer 并统一布局与 viewport

**文件**
- 新增：`src/ui/transaction_review.rs`。
- 修改：`src/ui/mod.rs`、`src/ui/read_only_sql.rs`、`src/ui/dialog.rs`、`src/app.rs`、`src/runtime.rs`、`src/action.rs`。
- 同步共享布局调用方：`src/ui/relation.rs`、`src/ui/sql_history_modal.rs`、`src/ui/redis_browser.rs`。
- 测试：`tests/ui_render.rs`、`src/ui/read_only_sql.rs` 内部 tests、`src/runtime.rs` 内部 tests。

**步骤 1：建立 UI 行为回归。**

通过真实 Review 打开和焦点 Action 渲染；断言行号、SQL 高亮、preview 光标、按钮焦点互斥、prompt 光标、text selection target 的 session ID。长 SQL 同时产生横纵滚动条，记录真实可交互 hit region。

运行：`cargo test --test ui_render transaction_review_ -- --nocapture`，旧 Paragraph 实现应缺少相应能力。

**步骤 2：提取布局合约。**

给 ReadOnlySqlEditor 增加共享布局计算，输入外框、实际边框占位、逻辑行数、是否显示行号、是否有 prompt；输出 gutter/body/prompt/scrollbar 区域及 session 内容 viewport。行号宽度按实际位数，不使用固定 5 格。宽高使用饱和计算，零正文高度不产生光标、选区或负 offset。

明确 snapshot 与 session 的高度语义：正文 viewport 不含 prompt；renderer 使用独立 prompt rect，不能再次减去 prompt 高度。修正所有共享调用方到相同合约；需要保持对外接口时使用内部 layout 字段传递，避免混合两种高度定义。

**步骤 3：提取 Review renderer。**

保留表名、LOCAL/ACTIVE/ABORTED 状态和 Commit/Rollback 说明；SQL 区域改为共享 renderer。使用完整 editor 边框承载 scrollbar，使底部轨道不覆盖 SQL 最后一行。保持默认弹窗大小，在当前布局内给正文分配剩余空间。

`dialog::render_actions` 支持无焦点状态：优先新增兼容 wrapper/内部 `Option<usize>` 实现，原 API 仍将 usize 映射 Some，Review 在 preview 焦点传 None。不要传 `usize::MAX` 作为隐式哨兵。

**步骤 4：实现 viewport 同步闭环。**

UiState 增加当前 modal editor 的 `(session_id, viewport)` 和必要的 preview area，每帧清空/重新登记。Runtime 在首次绘制前、resize 和键盘 action 引发 prompt 高度变化后，通过同一布局 helper 同步 session viewport；绘制回报用于校对和尺寸变化同步，避免首次 PageDown 仍使用默认尺寸。只在尺寸变化时更新，抑制无意义 redraw 循环。

Action 校验当前 Review session，即使审核非 active tab 也能同步；不得调用仅支持 active DDL 的 setter。切到按钮后仍更新 viewport，以支持 hover 滚轮和 resize。

**步骤 5：验证几何一致性。**

覆盖 `(60,18)`、`(80,24)`、`(120,36)`，以及全局 TooSmall `(40,10)`；行数 9→10、99→100 的 gutter；中文/Tab/长行；prompt 开关；尾部滚动后缩小/放大。断言 cursor 位于 body/prompt，hit maps 不包含 gutter/prompt，Ctrl-D/PageDown 距离来自实际正文高度，offset 被正确 clamp。

运行：

```bash
cargo test --lib read_only_sql_layout_ -- --nocapture
cargo test --lib transaction_review_ -- --nocapture
cargo test --test ui_render transaction_review_ -- --nocapture
cargo test --test ui_render relation_transaction_review_ -- --nocapture
```

建议逻辑提交：`feat(ui): render transaction previews with the DDL editor`。

## Task 5：接通弹窗鼠标选择、滚轮和滚动条

**文件**
- 修改：`src/input/mouse.rs`、`src/ui/text_selection.rs`、`src/ui/mod.rs`、`src/ui/transaction_review.rs`、`src/app.rs`。
- 测试：`tests/mouse.rs`、`tests/ui_render.rs`、`src/app.rs` 内部 tests。

**步骤 1：建立真实鼠标路径测试。**

从 render_with_state 得到正文/scrollbar 热区，发送 Down → Drag → Up 并收集 App commands。验证选区内容，不能通过直接调用 WriteClipboard 或自己计算 SQL 子串代替鼠标链路。

运行：`cargo test --test mouse transaction_review_ -- --nocapture`，当前 overlay 拦截使新增用例失败。

**步骤 2：增加 Review 文本手势来源与目标查询。**

TextGestureSource 增加 TransactionReview；提取小型 modal editor target helper，供 Down/Drag/Up/Scroll 共用身份校验。返回 overlay session/revision，并在鼠标层结合 UiState area 判断命中；处理顺序在一般 overlay 拦截之前。

沿用现有 Editor/TextDetail/SqlHistory 语义；若共享分支需要调整，同时运行对应旧测试。读取 active tab 不能替代 overlay session 校验。

**步骤 3：先分发 scrollbar，再处理文本。**

轨道/滑块热区命中直接走现有 scrollbar Action/drag geometry，reducer 聚焦 Review preview。对于正文，复用 text_selection_target_at；空白正文发 focus-only Action。`SetEditorMouseCursor` 新增 Review session 分支，更新局部 focus 并设置 cursor，不改底层 `App::focus`。

**步骤 4：完善 Drag/Up/Scroll 的 owner 路由。**

已开始的 Review scrollbar drag 优先于 overlay 文本拖动分支，不因 overlay 存在就 cancel；文本拖动复用 clamped hit map。Up 同时核对 source、session、revision、当前 overlay；成功时调用统一 copy_editor_selection。滚轮支持四个方向，只在 preview/轨道区域内生效，按钮区和弹窗外不滚动底层工作区。

**步骤 5：验证事件失效与原文复制。**

覆盖单击不复制、反向拖选、多行选择、空行、中文宽字符、Tab、横向 offset、越界拖动；断言不包含行号且原文空白保留。关闭/重开/resize 后旧 session 的 Up 和 scrollbar drag 不操作新 session。重绘对几何失效手势进行取消，不继续使用过期 scrollbar max_offset。

运行：

```bash
cargo test --test mouse transaction_review_ -- --nocapture
cargo test --lib transaction_review_ -- --nocapture
cargo test --test mouse
```

最后一条用于共享 mouse 路由改动后的完整回归，执行一次即可。

建议逻辑提交：`feat(transaction): enable preview mouse selection and scrolling`。

## Task 6：统一确认入口和退出场景

**文件**
- 修改：`src/app.rs`、`src/input/mouse.rs`（仅按钮 Action 映射需要时）。
- 测试：`src/app.rs` 内部 tests、`tests/quit_transaction_review.rs`、`tests/app_flow.rs`。

**步骤 1：锁定键盘/鼠标校验差异。**

建立可提交的 relation edit fixture，打开审核后改变源 edit，分别通过键盘 Enter 与 `ConfirmTransactionExitChoice(Commit)` 请求确认，断言两者都不产生写入/commit 命令并给出审核已变化提示。后一条在当前代码可能暴露缺少 edit_snapshot 检查的问题。

运行：`cargo test --lib transaction_review_ -- --nocapture`。

**步骤 2：集中 resolver。**

键盘确认从 `focus.choice()` 获取 Some(choice)，None 保留 overlay 并返回空命令；鼠标直接传 choice，但同样进入 `resolve_relation_review`。函数先取出合法 Review 状态，再做审核身份校验、事务动作和 deferred intent。snapshot 不匹配路径也清理 session，沿用“重新审核”的提示。

**步骤 3：验证原 deferred intent 语义。**

覆盖 Cancel 保留草稿、Rollback 本地变更、Active/Aborted 事务、Quit 审核非活动表、连续多个审核、源表已关闭/事务 generation 失效。若原流程已有 generation 校验，复用该函数，不另立校验源。

**步骤 4：统一清理与幂等性。**

确认和取消仅关闭当前 session 一次；不能提前清理下一个 deferred Review 的新 session。重复旧 session 的鼠标/viewport Action 无操作；复用原 mutation/transaction 执行命令，不把预览文本作为执行输入。

运行：

```bash
cargo test --lib transaction_review_ -- --nocapture
cargo test --test quit_transaction_review
cargo test --test app_flow
```

建议逻辑提交：`fix(transaction): unify review confirmation and cleanup`。

## Task 7：更新帮助、提示与共享组件回归

**文件**
- 修改：`src/help.rs`、`docs/keybindings.md`、`src/ui/transaction_review.rs`。
- 测试：`tests/keymap.rs`、`tests/ui_render.rs`、`src/help.rs` 内部 tests。

**步骤 1：建立 Review 专属帮助上下文。**

新增 TransactionReview 上下文/条目并更新上下文完备性测试，不继续沿用 console TransactionExitConfirmation 的 `a/r/c` 快捷说明。不要误改 ExecutionConfirmation 的 Up/Down preview 行，该条属于另一个弹窗。

**步骤 2：更新动态 footer。**

- 按钮区：`Tab/Shift-Tab focus`、`←/→ choose`、`Enter activate`、`Esc cancel`。
- Preview Normal：`hjkl move`、`/ search`、`v/y select/copy`、`Tab/Shift-Tab focus`。
- Preview prompt/Visual：提示 Enter 搜索或 y 复制、Esc 返回 Normal；采用现有 shortcut_hints 宽度裁剪。

**步骤 3：更新文档。**

在 `docs/keybindings.md` 明确四项循环、默认 Cancel、嵌套 Esc、只读命令、鼠标松开复制与 scrollbar 行为。文档只列实际验证过的 Vim 能力。

**步骤 4：执行相关目标的完整回归。**

```bash
cargo test --test keymap
cargo test --test ui_render
cargo test --lib
```

验收 DDL、SQL History、Redis VALUE 的共享 renderer、prompt 和复制不回归；默认 SQL editor/console 的 effects 仍按原逻辑分发。

建议逻辑提交：`docs(transaction): document SQL preview navigation`。

## Task 8：最终验收与交付记录

**文件**
- 验证实现涉及的全部文件；在本计划追加实际结果/必要偏差。

**步骤 1：审阅最终 diff。**

确认 Review 不再使用 `preview_offset` 或 Paragraph 绘制 SQL；没有重复实现 Vim motions、滚动条算法或 SQL 生成；所有新 Action 在非 console/非 active relation 的场景可达；所有复制和 viewport 路径携带正确 session。

**步骤 2：执行项目 Rust 门禁。**

使用 `.github/workflows/ci.yml` 中相同的命令：

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

预期全部 exit 0。环境缺少工具链或驱动依赖时记录具体失败与已完成检查，不能将跳过标为通过。测试过滤命中数大于零；此前已通过的定向测试无需无变化重复运行。

**步骤 3：终端交互验收。**

使用项目现有本地 SQLite fixture/连接执行 `cargo run --`：

1. 在 Relation Data 制造足够多的本地更新，打开 Transaction Review。
2. 从 Cancel 按 Tab 进入 preview，`12j`、`gg/G`、PageDown 和半页滚动正确。
3. `/` 搜索 SQL 片段、Enter、`n/N`；Visual 选择、y、鼠标跨行拖选，将剪贴板贴入普通文本确认原文。
4. 拖动横纵 scrollbar、点击轨道、在 preview 滚轮；在按钮/弹窗外滚轮，底层表不移动。
5. Visual/search 中一次 Esc 回 Normal，第二次 Esc 取消；Normal Enter 不提交。
6. 缩小和恢复终端，光标、prompt、scrollbar、按钮热区不重叠。
7. 重新打开并通过鼠标/键盘分别确认一次；检查提交和取消后的工作区输入正常。

人工验收记录实际终端和完成项目；无法运行终端时明确自动测试已经覆盖的部分。

**步骤 4：交付。**

记录最终 HEAD、改动文件、定向/门禁测试结果和交互行为；逻辑提交按实际授权执行，不能使用 `git add .` 混入已有工作。

## 3. 验收清单

| 编号 | 完成标准 | 主要证据 |
| --- | --- | --- |
| A1 | 四项焦点循环和初始 Cancel；preview Enter 不触发事务 | Keymap + reducer |
| A2 | Vim 移动、计数、Visual/yank、搜索、分页有效 | Editor + 集成输入 |
| A3 | 编辑类输入不改变 SQL/revision；effects 不穿透工作区 | Editor + App |
| A4 | 搜索粘贴仅进入 owner prompt；Esc/Tab 正确清理 pending | Keymap + Editor |
| A5 | 共享 DDL 高亮、行号、cursor、选择、scrollbar | TestBackend |
| A6 | 横纵滚动、轨道点击和滑块拖动作用于正确 session | Mouse + reducer |
| A7 | 鼠标多行/宽字符/Tab 复制保持原文 | Clipboard command 断言 |
| A8 | 首帧/resize/prompt 下 viewport 与正文几何一致 | Layout + Runtime |
| A9 | 非活动 relation 审核、关闭重开、旧事件失效正确 | Quit + 生命周期 |
| A10 | 键盘和鼠标确认共享审核身份校验与释放流程 | App 行为测试 |
| A11 | DDL、SQL History、Redis VALUE 和普通 SQL editor 无共享改动回归 | 相关目标完整回归 |
| A12 | fmt、clippy、all-targets/all-features test 通过 | CI 同款命令结果 |

## 4. 依赖与里程碑

```text
Task 1 状态/会话
  → Task 2 焦点/输入
  → Task 3 只读/效果/prompt
  → Task 4 共享渲染/viewport
  → Task 5 鼠标
  → Task 6 确认闭环
  → Task 7 帮助/共享回归
  → Task 8 门禁/交付
```

- **M1（Task 1–3）**：Review 拥有真实只读会话，键盘焦点与 Vim 输入正确，源 SQL 和底层工作区隔离。
- **M2（Task 4–5）**：DDL 同款预览呈现及鼠标/scrollbar 完整可用。
- **M3（Task 6–8）**：提交/取消/退出场景一致，帮助准确，项目门禁完成。

最终交付以 A1–A12 为准，M1 或单纯替换 renderer 不算完成全部需求。
