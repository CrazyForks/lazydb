# Redis Keys / Value Focus Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 自动工作流约定优先：后续阶段由插件调度；技能不可用时按本文顺序执行并记录结果，不启动子 Agent，不再询问执行方式。

**Goal:** Redis 叶子 Enter / 双击打开 Value 后立即聚焦 Value，并使 `Ctrl-w h/l` 按 Explorer ↔ Keys ↔ Value 的相邻关系移动。

**Architecture:** 保留外层 `App.focus` 与内层 `RedisBrowserTab.focus`。公共显式打开入口负责同步焦点，Keymap 统一处理 Redis 窗口前缀和水平目标，复用现有 Focus / RedisFocusPane Action；普通 Value Vim 输入仍由只读 editor 处理。异步读值结果只更新内容，不改变焦点。

**Tech Stack:** Rust 2024 / Rust 1.94.0、Crossterm、Ratatui、现有 Action → App → Command 架构、Cargo 集成及单元测试。

---

## 0. 执行上下文

- 分析依据：同目录 `analysis.md`，已完整阅读。
- 原工作空间：`/Users/yelog/workspace/tui/lazydb`。
- 目标分支：`main`；起点：`6e469907120147e9ac21bc265a69642d1d74dcf4`。plan 阶段复核 HEAD 与该起点一致。
- 任务名称、分支和 worktree 由插件/Luna 管理；本计划不要求手工建立、移动或清理 worktree，也不修改 state.json。
- 此文件是唯一实施计划交付物，按用户指定放在任务目录，不另写 docs/plans 副本。
- 现有三个未跟踪 docs/plans 文档属于原工作区已有内容，不删除、不覆盖、不加入本任务提交。
- 本阶段只写计划。下述业务编辑与测试均为后续实施阶段的动作，不代表已执行。
- 每个编号步骤尽量作为一个小检查点；依赖顺序为 Task 1 → 2 → 3 → 4 → 5 → 6。无须另行并行委派。
- 如后续阶段允许提交，按 Task 2、Task 4、Task 5 的逻辑边界提交且仅显式暂存相关文件；提交权限及最终提交/合并动作遵守插件该阶段指令，不在本阶段执行。

## 1. 最终交互契约

| 操作/位置 | 预期 |
| --- | --- |
| Keys 实际叶子 Enter | 打开该 key；立即 Results + Preview；显示 Loading |
| Keys 实际叶子双击 | 第一击仅选择，第二击打开一次并聚焦 Preview |
| 分组 Enter / 单双击 | 保持现有展开/收起、选择规则，不打开 Value |
| Keys 单击、j/k、裸 h/l、搜索确认 | 保持 Keys；不因为选择变化打开 Value |
| Explorer + Ctrl-w l | 进入 Keys，即使 tab 记忆焦点为 Preview |
| Keys + Ctrl-w l | 进入 Preview |
| Preview + Ctrl-w h | 进入 Keys |
| Keys + Ctrl-w h | 进入 Explorer |
| Explorer + Ctrl-w h；Preview + Ctrl-w l | 原地且结束窗口序列，不循环 |
| 切焦后迟到的读值成功/失败 | 保持最新用户焦点 |

方向关系不依赖 Value 是否加载或是文本/表格。Ctrl-w 表示先按 Control+w，再按方向键；不把普通 h/l 改为切窗。Tab / BackTab、Ctrl-w w 继续使用现有循环语义。

## Task 1：复核输入链并固定回归测试基础

**Files / 只读复核：**
- `src/app.rs`：`open_redis_key`、`primary_redis_selection`、`ReadOnlyEditorKey`、`apply_editor_effects`、FocusNext / FocusPrevious / RedisFocusPane。
- `src/input/keymap.rs`：PendingState、`Keymap::map`、`Pending::Window`、`map_pending`、Redis Preview 提前转发分支。
- `src/editor/mod.rs`：Window pending、普通/Visual 模式、计数和搜索输入状态。
- `src/help.rs`：Redis context、窗口序列候选、可执行 shortcut。
- `tests/redis_browser_tabs.rs`、`tests/redis_help.rs`、`tests/mouse.rs`。

**Step 1 — 确认实施工作区。**

```bash
git status --short
git rev-parse HEAD
```

预期：在插件提供的任务工作区；基于指定起点或插件明确记录的后续提交。记录已有修改，不能重置它们。

**Step 2 — 补齐前缀状态细节。**

使用 codegraph 优先定位上述符号；未覆盖内容才定向读取。确认以下已知事实与当前代码一致：
- `open_redis_key` 是 Enter 与双击唯一共用入口。
- keymap 在解析 continuation 前通过 `self.pending.take()` 消费前缀；边界直接 return None 即可结束，但不能随后重新 `continue_pending`。
- Preview 文本输入比通用 Ctrl-w 分支更早 return。
- Help 窗口菜单的 Enter 使用 ExecuteHelpShortcut，不能只修字母 continuation。

**Step 3 — 做计数/搜索兼容性基线记录。**

在现有 keymap/editor 测试 fixture 下记录 Preview 的 `3h`、`3 Ctrl-w >`、Visual 模式 Ctrl-w、`/` 搜索输入中的 Ctrl-w，以及有未完成 operator 时的 Ctrl-w 当前路由。使用真实事件，必要时增加针对这些行为的回归测试。该步骤是为下一任务确定安全拦截点，不增加新的编辑器特性。

**Step 4 — 运行基础套件。**

```bash
cargo +1.94.0 test --test redis_browser_tabs --test redis_help
cargo +1.94.0 test --lib input::keymap
```

预期：现有用例通过；若有环境或已有失败，记录具体命令、错误及其与本改动关系，不把编译失败当成功复现。

**复核/验收：** 确定原始焦点缺失、两层目标解析缺失、Preview 路由三个修改点；拥有可复用的 Redis fixture，无需真实 Redis 服务。

## Task 2：在公共显式打开入口聚焦 Value

**Files：**
- Modify: `src/app.rs`，`App::open_redis_key`（起点约 19378 行）。
- Test: `tests/redis_browser_tabs.rs`。

**Step 1 — 添加失败的行为用例。**

复用现有 `selecting_a_key_does_not_open_a_preview_but_explicit_open_does` 的连接和树 fixture，新增/扩展：
- `redis_leaf_enter_focuses_value_immediately`：外层 Results、内层 Keys、选中叶子，用 Keymap 映射 Enter 并 dispatch；断言 opened_key、Loading、App.focus=Results、tab.focus=Preview。
- `redis_explicit_open_focuses_value`：直接 OpenRedisKey 入口也满足相同焦点契约。
- `redis_invalid_open_preserves_focus`：不存在 tab_id、不存在 Key、Prefix 均不改变焦点或 generation。
- `redis_background_open_preserves_active_focus`：打开另一个 tab 的有效叶子，该 tab 记忆焦点可改为 Preview，但 active_tab 与当前 App.focus 保持不变。

**Step 2 — 定向验证失败。**

```bash
cargo +1.94.0 test --test redis_browser_tabs redis_leaf_enter_focuses_value_immediately -- --exact
cargo +1.94.0 test --test redis_browser_tabs redis_explicit_open_focuses_value -- --exact
```

预期：在现有实现下由于内层焦点仍为 Keys 而断言失败；测试必须实际运行，不能是零匹配。

**Step 3 — 最小实现。**

在获取目标 tab 的 mutable borrow 前计算当前活动 tab 是否是目标；保持现有节点验证和调度逻辑。成功进入 Loading 后加入等价逻辑：

```rust
tab.focus = crate::model::redis_browser::RedisBrowserFocus::Preview;
if is_active_tab {
    self.focus = Focus::Results;
}
```

其中 `is_active_tab` 定义为：

```rust
let is_active_tab = self
    .tabs
    .get(self.active_tab)
    .is_some_and(|tab| tab.id() == tab_id);
```

不要在 `RedisBrowserTab::select`、模型 `open_key`、异步结果处理或鼠标专用分支重复增加焦点副作用。

**Step 4 — 验证与复核。**

```bash
cargo +1.94.0 test --test redis_browser_tabs
```

预期全部通过，特别是原有快速连续打开只调度最后一次、generation 更新、选择与打开分离测试。复核只有合法显式打开进入 Preview；后台 tab 不抢焦点。

**验收：** Enter 与 OpenRedisKey 共用正确行为，读取仍异步进行，不因焦点更新多发请求。

## Task 3：实现相邻水平目标与窗口前缀统一路由

**Files：**
- Modify: `src/input/keymap.rs`。
- Test: `tests/redis_help.rs`（复用 redis_app fixture）及 `src/input/keymap.rs` 单元测试。
- Conditional Modify: `src/editor/mod.rs`、`src/app.rs`，仅当 Task 1 证实计数/编辑器 pending 需提供窄范围状态衔接；不能改所有 editor 的全局 FocusPane 语义。

**Step 1 — 加入真实序列失败测试。**

新用例统一命名为 `redis_window_*`，逐个按事件调用 Keymap，返回 Some(Action) 时立即 `app.update`：
1. Explorer（tab 内层预置 Preview）→ Ctrl-w l → Keys → Ctrl-w l → Preview → Ctrl-w h → Keys → Ctrl-w h → Explorer。
2. Explorer+h 和 Preview+l 边界：无焦点变化，`sequence_state` 为 None；随后普通移动有效。
3. Preview 参数化为 Empty、Loading、Failed、Ready 文本、Ready 表格，各自向左只进入 Keys。
4. 方向 binding 自定义 continuation 时仍有效；沿用已有 binding fixture 和配置 API，不增加配置项。

**Step 2 — 运行并确认失败位置。**

```bash
cargo +1.94.0 test --test redis_help redis_window_
```

预期：至少 Keys 向右、Preview 向左、Explorer 记忆 Preview 三种现有路径失败。

**Step 3 — 在 Pending::Window 内实现 Redis 优先解析。**

在原有通用 focus-pane-left/right 解析之前：活动 tab 必须是 RedisBrowser，使用现有 `bindings.matches_sequence` 识别方向。目标选择采用下列完整转换逻辑（变量 `is_left` 仅在已匹配左右 binding 时产生）：

```rust
let action = match (app.focus, tab.focus, is_left) {
    (Focus::Explorer, _, true) => None,
    (Focus::Explorer, _, false) => {
        Some(Action::RedisFocusPane(RedisBrowserFocus::Keys))
    }
    (Focus::Results, RedisBrowserFocus::Keys, true) => {
        Some(Action::Focus(Focus::Explorer))
    }
    (Focus::Results, RedisBrowserFocus::Keys, false) => {
        Some(Action::RedisFocusPane(RedisBrowserFocus::Preview))
    }
    (Focus::Results, RedisBrowserFocus::Preview, true) => {
        Some(Action::RedisFocusPane(RedisBrowserFocus::Keys))
    }
    (Focus::Results, RedisBrowserFocus::Preview, false) => None,
    (Focus::Editor, _, _) => None,
};
```

只在外层 Explorer/Results 的有效 Redis 上下文使用该分支；Focus::Editor 不应进入 Redis 优先路径，继续原有 normalize/通用行为。方向已匹配时清零菜单选择并直接 return action，即使 action 是 None，也不落入后续 pending 恢复逻辑。若抽辅助函数，返回值需显式区分“未处理”与“已处理但无 Action”，不能只用一个 Option 混淆两者。

**Step 4 — 将 Preview 的合法窗口前缀交给 keymap。**

扩展目前只覆盖 Keys 的 Ctrl-w 前缀入口，放在 Preview 的 ReadOnlyEditorKey 提前 return 前；普通/Visual 窗口命令可进入 `Pending::Window`，文本搜索输入仍交给 editor，Keys find 编辑态仍保留原优先级。

实施时按 Task 1 的基线处理 editor 已有计数/待决输入：
- 未有 editor 待决输入的 Ctrl-w，由 keymap 完整消费首键及 continuation。
- 已有数字计数时，优先用现有 editor 状态能力转交该计数并消费旧状态；若没有该能力，增加仅用于窗口命令的窄接口，返回 count 并清理对应 pending，不能把数字统一截走，破坏 `3h` 等 Vim motion。
- 搜索输入或未完成 operator 不能只截走后半序列，造成返回 Value 后残留计数/操作。沿用已有取消/交接行为并以真实事件测试证明。避免以发送任意 Esc 的方式顺带破坏 Visual selection。
- 首先复用现有 WindowCount / resize 解析；不修改全局 editor 的 Window+h 为 Redis 目标。

**Step 5 — 加入兼容性用例并运行。**

验证文本/Visual 的裸 h/l、`3h`、搜索输入、带计数窗口宽度、窗口命令完成后下一普通 motion、Ctrl-w Ctrl-w 与已有 Ctrl-w w、最大化和重置。无法支持某项既有行为时应修复再继续，不能降低原有测试断言。

```bash
cargo +1.94.0 test --test redis_help
cargo +1.94.0 test --lib input::keymap
cargo +1.94.0 test --lib editor::
```

最后一个覆盖真实 editor 状态交接；若没有修改 editor 仍用于确认 Preview 路由兼容性。

**复核/验收：** 两条 Value 视图路由都遵循真值表；边界被消费；文本计数及搜索不回归；未修改非 Redis 的方向目标。不存在通过循环 Action 实现水平边界的捷径。

## Task 4：补齐打开与异步、鼠标、搜索的跨入口回归

**Files：**
- Test: `tests/mouse.rs`、`tests/redis_browser_tabs.rs`、`tests/redis_key_filter.rs`。
- Review: `src/input/mouse.rs`、`src/ui/redis_browser.rs`；预期不需要业务修改。

**Step 1 — 鼠标真实路径测试。**

复用现有 UiState / hit-region / MouseEvent fixture，通过渲染后的叶子行位置或真实 RedisKeyNode hit region 送入两次左键点击并 dispatch：
- 第一次：Keys，未打开；第二次：Preview，目标 key 正确。
- 第二击前后比较 generation 增量与 RedisPreviewTick 的加载 Command 数量，确认只打开一次。
- 对分组重复事件，保持已有第二击不重复 toggle 规则且不进入 Preview。
- 优先沿用现有可控时间双击设施；不得依赖 sleep 和真实网络。

**Step 2 — 异步不抢焦点测试。**

在 redis_browser_tabs 原有合法响应 fixture 中，打开 A → 手动切回 Keys 或 Explorer → 派发成功/失败响应，分别断言最终焦点未变。继续验证旧 generation 的结果被忽略。只构造现有 Action，不需要 Redis 进程。

**Step 3 — 搜索与普通树导航测试。**

搜索输入 Enter 只确认，确认后叶子再 Enter 才打开并进入 Preview；搜索编辑 Ctrl-w 不启动窗口序列；裸 h/l 不离开 Keys。已有测试若假设打开后仍为 Keys，应在下一树操作前显式聚焦 Keys，不删除有意义的原断言。

**Step 4 — 运行组合回归。**

```bash
cargo +1.94.0 test --test mouse --test redis_browser_tabs --test redis_key_filter --test redis_key_tree
```

**复核/验收：** Enter 与双击的最终体验一致，单击与分组不受影响；异步结果、搜索、树选择仍保持原契约。若测试暴露业务遗漏，回到对应公共入口修复，不在测试中直接修改焦点掩盖问题。

## Task 5：同步 Redis 窗口帮助与可执行条目

**Files：**
- Modify: `src/help.rs`。
- Modify: `src/app.rs`，HelpShortcutId → Action 映射（起点约 2529 行）。
- Test: `tests/redis_help.rs` 与 `src/help.rs` 单元测试。

**Step 1 — 加入行为断言。**

在 Explorer + Redis、Keys、Preview 三种 context 断言 Window 菜单的合法方向目标与真值表相同；Preview 左侧描述为 Keys，Keys 右侧为 Value。执行菜单条目（方向选择后 Enter / ExecuteHelpShortcut）得到与直接键入方向完全一致的最终焦点。

**Step 2 — 修改帮助条目和分派。**

优先为 Redis 添加清晰的内层目标条目，使用现有 context predicate 限制可见性。Explorer+Redis 的向右项应明确指向 Keys；勿将 SQL/Relation “向左去 Explorer”的条目全局重解释。需要新增 HelpShortcutId 时，完整更新其枚举、条目、排序/上下文与 App 分派；复用 RedisFocusPane(Keys/Preview) 和 Focus(Explorer)。

**Step 3 — 检查自定义绑定与前缀菜单。**

沿用项目既有绑定展示方式，保证菜单显示与 configured direction 一致；检查边界不提供误导性的环绕条目。菜单取消、超时、切 tab 失效按原规则工作。

**Step 4 — 定向验证。**

```bash
cargo +1.94.0 test --test redis_help
cargo +1.94.0 test --lib help::
cargo +1.94.0 test --lib input::keymap
```

**复核/验收：** 文案、可执行帮助、直接按键三者一致，SQL/Relation/Dashboard 的帮助与导航不变。

## Task 6：最终复核、CI 等价检查和交接

**Files：** 所有本任务修改文件，及只读参考 `.github/workflows/ci.yml`。

**Step 1 — 检查差异。**

```bash
git diff --check
git diff --stat
git diff -- src/app.rs src/input/keymap.rs src/help.rs src/editor/mod.rs tests/redis_browser_tabs.rs tests/redis_help.rs tests/mouse.rs tests/redis_key_filter.rs
```

复核：没有新增焦点枚举，没有修改读值/SCAN 数据层，没有按全局 Focus(Explorer) 改变鼠标直达语义，没有异步加载聚焦，没有重复鼠标/键盘打开逻辑，没有混入已有用户文档。

**Step 2 — 执行本项目 Rust CI 的必需检查。**

`.github/workflows/ci.yml:81-83` 使用下列命令：

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

预期：全部退出 0；无格式差异、无 clippy warning、全部启用的测试通过。若需格式修正，先 `cargo +1.94.0 fmt --all`，检查没有无关文件变化，再重新运行受影响检查。无需因本次焦点修复额外运行发布/安装器测试；Rust 全套通过后不重复无变化的定向套件。

**Step 3 — TUI 体验复核（环境具备时）。**

使用已有 Redis 测试连接，选择普通 string 和可表格显示的集合各一次：叶子 Enter / 双击后高亮边框应在 Value，连续左右切换经过三窗格，边界无循环；加载期间离开 Value 后响应不抢焦点。没有真实 Redis 时以 Task 2–5 自动化事件链为验收依据，交接中明确“未做真实 TUI 人工复核”，不得声称已测试。

**Step 4 — 交付实施结果。**

按插件后续阶段指定位置记录修改文件、测试命令及结果、是否做过 TUI 复核、未解决问题。提交/回执 token 以该阶段指令为准，不复用本 plan 阶段 token。遇到工具链、依赖或测试阻塞需准确报告；不能将未运行的命令记作通过。

## 最终验收清单

- [ ] 叶子 Enter 与双击均立即聚焦 Value，第一击及分组操作不自动打开。
- [ ] Explorer → Keys → Value 及反向逐项成立，无跨越；方向边界不循环且 pending 被清除。
- [ ] 文本/表格及 Empty/Loading/Failed Value 行为一致。
- [ ] 普通 h/l、Vim 计数/搜索/Visual、Keys 搜索编辑语义保留。
- [ ] 旧请求去重与 generation 检查仍成立，响应不会抢焦点。
- [ ] 非活动 tab 打开不改变当前活动 tab/外层焦点。
- [ ] Tab/BackTab、窗口循环、大小调整、最大化/重置及配置方向绑定通过回归。
- [ ] Redis 帮助说明与可执行菜单和真实按键一致；其他 tab 类型无回归。
- [ ] fmt、clippy、Rust 全目标全特性测试结果明确，无未解释的失败。
- [ ] 只包含本任务修改，插件管理状态和已有用户文件保持完好。

## Plan 阶段完成情况

已依据 analysis.md 拆分为逐项实施、复核、验证与验收计划；复核了当前 HEAD、原工作区状态、Pending 消费位置、已有 fixture 和 CI 命令。本阶段没有实施业务代码、运行上述实施测试或创建 worktree。后续由自动工作流继续，不需要用户选择执行方式。
