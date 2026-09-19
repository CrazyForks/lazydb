# No Open Tabs Empty State Implementation Plan

> **执行者：Luna。** 按本计划完成实现、验证、审查与收尾；不启动子 Agent。Astra 仅负责分析与计划。

**Goal:** 关闭最后一个 tab 后，即使数据库连接仍在线，工作区也显示统一空态；重新打开 tab 后恢复正常内容与交互。

**Architecture:** 将 UI 中的连接空态扩展为工作区空态，在保留现有离线 console 行为的基础上加入 NoOpenTabs。一次计算空态并同时用于绘制、编辑器可见性和 resize drag 失效判断，复用现有区域合并和空态布局。

**Tech Stack:** Rust 1.94、Ratatui TestBackend、现有 App Action reducer、UiState。

---

## 基线、范围与交接

- 分析：`.git/opencode-tasks/ses_f4c9cdcdfffeuyPrHq6gNe4i2K/analysis.md`。
- 指定原始起点：`fd294bd7e8b56d06d0ad945358ae7f0c83ace8d7`；目标分支 `main`。
- 计划阶段观察到当前 HEAD 为 `af23ff65072f203b5266769cb9898203a09854b5`，main 相对 origin/main ahead 9。原始起点之后增加了 tab 排序相关变更；检查 diff 确认 `src/ui/mod.rs`、`src/ui/layout.rs`、`tests/ui_render.rs` 未变，app 新增 MoveTab 分支但未改变关闭逻辑。
- 实施时按任务系统指定起点建立分支，保留最新 main 已有工作；合并前核对最新 main，不能重置或覆盖 tab 排序变更。
- 工作流与最终任务分支命名交由 Luna；建议描述为“关闭全部标签后恢复工作区空态”，分支可用 `fix/no-open-tabs-empty-state`，先检查名称是否已占用。
- 用户已有未跟踪文件 `docs/plans/2026-09-18-tab-reorder-focus-implementation.md` 不属于本任务。
- checkpoint.json 仍不存在。本轮用户没有提供新的 plan 回执 token/路径，因此不得复用 analyze 回执或自行编造 plan 回执。
- 图片原始内容不可读取；以用户文字和现有空态布局为准。在线空列表使用准确的 NoOpenTabs 文案，不把实际在线连接称为断开。

## 唯一业务验收单元

**在线连接 → 打开 tab → 关闭所有 tab → 正确空态 → 重新打开 tab → 正常工作区。**

以下任务是同一个闭环内的步骤。完成步骤后继续推进到整个闭环通过，不在中途等待用户 resume。

### Task 1：建立有状态的渲染回归证据

**Files:**
- Modify/Test: `tests/ui_render.rs`，靠近 `disconnected_workspace_without_profiles_renders_first_run_empty_state` 等现有测试。
- Read: 同文件顶部 `fixture()` 及现有 render helpers、PaneResizeDrag 测试用法。

1. 使用 `fixture()` 创建已连接、已有 query 结果的 App。记录 `connection.active_identity()` 与 session 身份列表。使用同一个 Terminal<TestBackend> 和 UiState 渲染关闭前后，避免 helper 每次新建 UiState 掩盖旧状态残留。
2. 添加 `closing_all_tabs_renders_empty_workspace_and_reopens_console`：先断言正常帧包含 DATA/OUTPUT；收集 tab UUID，通过 `Action::CloseTab(id)` 逐一关闭，而不是 `tabs.clear()`。断言最终 tabs 为空、focus 为 Explorer、连接身份和 session 保留。
3. 在关闭后的 120×36 帧中断言包含 `NO OPEN TABS`，不包含 `DATA`、`OUTPUT`、`no result`、`SQL EDITOR`。断言 `editor_viewport`、`output_viewport`、`grid_viewport`、`result_area`、cursor 为空；无 `Focus(Editor)`、`Focus(Results)`、`ResultView(_)`、`PaneResize(EditorHeight)` 热区。Explorer 热区仍存在。
4. 通过 `Action::NewConsole` 重新打开 console，再渲染：空态消失，编辑器及 DATA/OUTPUT 恢复，editor_viewport 存在。
5. 运行下面命令，预期在修复前于 `NO OPEN TABS` 或残留标签断言失败。记录实际失败，不将编译失败当成业务复现。

```sh
cargo +1.94.0 test --test ui_render closing_all_tabs_renders_empty_workspace_and_reopens_console -- --exact
```

测试无需真实数据库：fixture 已用 ConnectionSucceeded 和 QueryFinished Action 构造状态。不要为此新增外部服务或 PTY 依赖。

### Task 2：统一空态判定并同步交互状态

**Files:**
- Modify: `src/ui/mod.rs`，原 `DisconnectedWorkspace`、`disconnected_workspace_area`、`render_disconnected_workspace`、`render_with_state_at`。
- Read: `src/ui/layout.rs`，仅确认现有布局行为。

1. 局部重命名 `DisconnectedWorkspace` 为 `WorkspaceEmptyState`，辅助函数改为 `workspace_empty_area` 与 `render_empty_workspace`。保持 logo、主题、居中及尺寸适配逻辑。
2. 枚举新增 `NoOpenTabs`。分类使用以下等价实现，保留旧条件的优先级：

```rust
fn for_app(app: &App) -> Option<Self> {
    if app
        .active_console_opt()
        .and_then(|console| console.execution_target.as_ref())
        .is_some()
    {
        return None;
    }
    if app.connection.status == ConnectionStatus::Disconnected
        && app.sessions.iter().next().is_none()
    {
        return Some(if app.profiles.is_empty() {
            Self::NoProfiles
        } else {
            Self::NoActiveConnection
        });
    }
    app.tabs.is_empty().then_some(Self::NoOpenTabs)
}
```

3. title 新增 `Self::NoOpenTabs => "NO OPEN TABS"`；instruction 新增宽屏 `Open a tab from Explorer or the console list.`、紧凑版 `Open a tab from Explorer or consoles.`。原有 NoProfiles/NoActiveConnection 文案保留。
4. `render_with_state_at` 先计算 `let empty_workspace = WorkspaceEmptyState::for_app(app);`。将原 `editor_rendered` 中重复调用替换为 `empty_workspace.is_none()`，普通渲染分支也使用该变量。Relation/Dashboard/RedisBrowser 专用分支优先级保持现有行为，不能拿“无 active console”判定为“无 tab”。
5. pane_drag_invalid 的 split match 为 EditorHeight 增加：

```rust
PaneSplit::EditorHeight => {
    !editor_rendered || layout.pane_resize_region(drag.split).is_none()
}
```

其余 split 保留现有行为。复用现有失效分支清空 pane_resize_drag 与 PaneResize gesture；后续高亮就不会再画隐藏分隔线。ExplorerWidth 分割条仍可使用。
6. 确认空态分支不调用 editor/result renderer，因此不会新增这些面板的 focus/ResultView 热区；UiState 现有每帧 reset 继续清理 viewport/cursor，不添加重复 reset。
7. 重跑 Task 1 命令，预期通过。业务 reducer、连接生命周期、persisted console 文档均不需要修改。

### Task 3：覆盖易漏状态并完成定向验收

**Files:**
- Modify/Test: `tests/ui_render.rs`。
- Regression: `tests/workspace_tabs.rs`（默认不修改）。

1. 新增 session 兜底测试：使用 fixture 保留 session，关闭所有 tab 后将投影 connection.status 设置为 Disconnected，确认仍显示 NO OPEN TABS 而非 SQL 面板。此测试验证分类不能只检查 Connected。
2. 新增 drag 生命周期测试：正常帧先获取 EditorHeight resize 热区，按现有测试方式建立 drag/gesture；关闭全部 tab 后用同一个 UiState 再渲染，断言 drag 与对应 gesture 已清空且不存在该热区。
3. 用尺寸表覆盖 120×36、180×40：空态存在且无 SQL 面板；80×24 和 pane_maximized=true/Explorer：保持 Explorer 独占且无 SQL 面板，不要求不可见右侧出现 logo；40×10：显示 TERMINAL TOO SMALL。
4. 运行已有启动空态、离线 console 测试及完整相关集，覆盖仍有 tab 时行为与空列表焦点。优先复用已有 Relation/Dashboard/RedisBrowser 正常渲染覆盖；如果现有测试没有对应保护，补充能验证真实可见内容的一条定向断言，避免枚举逐分支镜像测试。

```sh
cargo +1.94.0 test --test ui_render --test workspace_tabs
```

预期全部通过。若有失败，先区分新增逻辑、基线失败、环境依赖并记录实际结果；普通编译/测试问题自行修复。

### Task 4：最终验证、审查与交付

**Files:**
- Review: `src/ui/mod.rs`、`tests/ui_render.rs`、本计划文件。
- Update evidence: `.git/opencode-tasks/ses_f4c9cdcdfffeuyPrHq6gNe4i2K/validation.md`（如在 worktree 中实施，写到用户指定原任务目录）。

1. 完整闭环通过后按项目 `.github/workflows/ci.yml` 的 Rust 检查口径运行一次：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
git diff --check
```

2. 记录每条命令、退出码、运行版本或工作区 diff 状态、环境/feature 以及服务型测试跳过情况。只对相关代码或环境变化重跑，不重复无意义 check+clippy+全量测试。
3. Luna 自行审查：真实 CloseTab 路径是否覆盖，在线 session 是否保留，旧空态及离线 console 是否维持，交互热区/resize drag 是否与可见面板一致，重新打开 tab 是否恢复。不切回 Astra 审查。
4. PTY/截图人工复现属于补充检查，非本任务新增强制门槛。遇环境限制最多一次有针对性的修复重试，然后由 Luna 收尾审查决定补充证据或记录限制。
5. 在任务流程要求提交时，按显式路径 stage（不使用 git add .），推荐提交消息 `fix(ui): restore empty workspace after closing all tabs`。不要纳入已有 tab 排序计划或插件 metadata。提交、合并、回执由 Luna 遵照届时阶段授权与指定新 token 执行。

## 完成标准

- 关闭所有 tab 后无 DATA/OUTPUT/no result 或不可见面板交互残留。
- 连接/session 与 console 文档保留，Explorer 可用。
- 无连接空态、离线绑定 console、其他 tab 类型及尺寸模式无回归。
- 重新打开 tab 正常工作，定向及最终验证结果如实记录。
- 只完成当前获授权阶段；计划阶段不实施代码、不创建或覆盖旧回执。
