# No Open Tabs Empty State Implementation Plan

> 执行交接：Luna 按本计划实施、复核、纠偏与提交合并；Astra 仅分析/计划。不启动子 Agent，不再询问执行方式。

**Goal:** 关闭右侧全部 tab 后显示与现有空工作区一致的空态，消除 DATA/OUTPUT 等面板残留，并能正常重新打开 tab。

**Architecture:** 在 `src/ui/mod.rs` 将连接空态扩展为工作区空态，保留原有断线/首次启动语义，增加在线无 tab 状态。统一使用同一个空态判定控制渲染、编辑器可见性和拖动失效，复用现有布局及逐帧 UiState 清理。

**Tech Stack:** Rust 1.94、Ratatui TestBackend、App Action reducer、UiState。

---

## 1. 输入、基线和执行范围

- 已读取同目录 `analysis.md`。根因：关闭 reducer 正确清空 tab 并保留连接；`DisconnectedWorkspace::for_app` 没有处理在线无 tab，渲染落入默认 SQL 分支，`render_result_tabs` 每帧生成 DATA/OUTPUT/no result。
- 用户指定原始起点：`fd294bd7e8b56d06d0ad945358ae7f0c83ace8d7`；目标分支 `main`；原工作区 `/Users/yelog/workspace/tui/lazydb`。
- 当前实际 HEAD：`af23ff65072f203b5266769cb9898203a09854b5`。之前已核对起点至当前 HEAD 的差异为 tab 排序相关变更，未修改本任务 UI 源码；本轮 HEAD 未变、tracked diff 为空。
- 已有未跟踪文件包括本任务 docs 计划、tab 排序计划和 Redis value dialogs 计划。后两者为其他工作，不覆盖、不纳入本任务提交。
- checkpoint.json 本轮仍不存在；不创建或修改它，不修改 state.json。
- 原图内容不可读取，以用户文字与现有空态为依据，不要求截图像素级比对。
- 工作流和任务分支由后续自动流程/Luna 命名。建议分支 `fix/no-open-tabs-empty-state`，创建前检查是否占用；按指定起点创建任务分支，合并时保留 main 后续已有改动。本阶段不创建分支、不实施业务代码。

## 2. 验收与门禁分级

### A. 用户需求与必要回归契约

1. 关闭全部 tab 后右侧显示统一空态，不显示 DATA、OUTPUT、no result、SQL EDITOR。
2. 连接、session、console 文档保留；Explorer 可用；重新打开 tab 恢复对应面板。
3. 空态无 Editor/Results focus、ResultView、EditorHeight resize 热区，旧 viewport/cursor/resize drag 不残留。
4. 首次启动空态、离线绑定 SQL console、其他 tab 类型、小屏/最大化布局不回归。

### B. 项目既有强制检查口径

来源 `.github/workflows/ci.yml:81-83`：Rust fmt、clippy（all-targets/all-features，warnings deny）、全量测试（all-targets/all-features）。功能齐备后执行一次，实际结果和环境限制写入 validation.md。服务型与平台型 CI 作业保留其原有 CI 职责，不将本地无服务造成的跳过称为全部集成验证通过。

### C. 本计划建议的定向自动验证

采用 TestBackend 和 Action 测试上述闭环，运行 ui_render/workspace_tabs 相关集，作为本次修复直接证据；新增测试验证用户行为和交互生命周期，不镜像私有枚举实现。`git diff --check` 是提交前补充静态检查。

### D. 可选补充证据

人工操作、PTY、真实数据库现场复现、截图比对均不是用户新增强制门禁。环境受限最多一次针对性修复重试，再由 Luna 收尾审查决定补充证据或记录限制；不能无限 progress。普通代码编译/测试错误自行修复，不以此要求用户接手。

## 3. 单个端到端业务闭环

**在线连接 → 打开 tab → 关闭全部 tab → 空态 → 再打开 tab → 正常工作区。** 以下步骤服务同一验收单元，持续推进直至闭环通过。

### Task 1：建立关闭与重开的回归测试

**Files:** 修改 `tests/ui_render.rs`；复用顶部 `fixture()`、TestBackend helpers 与现有空态测试附近结构。

1. 新增 `closing_all_tabs_renders_empty_workspace_and_reopens_console`。fixture 已用 ConnectionSucceeded/QueryFinished 构造在线且有结果的 App，不需要真实数据库。
2. 记录连接身份和 session 身份；在同一个 Terminal<TestBackend> 和 UiState 中先绘制正常帧，确认 DATA/OUTPUT 可见。
3. 收集所有 tab UUID，通过 `Action::CloseTab(id)` 逐个关闭，不用 tabs.clear() 代替真实关闭路径。断言 tabs 为空、focus=Explorer、连接/session 不变。
4. 同一 UiState 再渲染 120×36：断言 `NO OPEN TABS` 可见，DATA/OUTPUT/no result/SQL EDITOR 不可见；editor/output/grid viewport、cursor/result_area 为空；不存在 Editor/Results focus、ResultView、EditorHeight resize 热区，Explorer 仍可用。
5. Action::NewConsole 后再渲染，断言空态消失、编辑器与 DATA/OUTPUT 及 editor_viewport 恢复。
6. 运行测试确认修复前的业务失败，记录实际失败点，不将编译失败冒充复现。

```sh
cargo +1.94.0 test --test ui_render closing_all_tabs_renders_empty_workspace_and_reopens_console -- --exact
```

**验收：** 测试能够准确暴露在线无 tab 时的错误渲染；失败证据记录于 validation.md。

### Task 2：扩展空态并统一可见性

**Files:** 修改 `src/ui/mod.rs` 的 DisconnectedWorkspace、相关空态函数与 render_with_state_at；只读 `src/ui/layout.rs` 确认尺寸规则。

1. 将局部类型改名为 WorkspaceEmptyState，增加 NoOpenTabs；辅助函数改为 workspace_empty_area/render_empty_workspace，继续复用现有 logo、颜色和区域合并逻辑。
2. 使用以下分类实现，保留离线 console 和原有断线提示优先级：

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

3. NoOpenTabs 标题为 `NO OPEN TABS`。建议宽屏指引 `Open a tab from Explorer or the console list.`，紧凑版 `Open a tab from Explorer or consoles.`；Luna 可按实际操作名称细调，不能把在线状态称作断线。保留其他状态文案。
4. render_with_state_at 一次计算 `let empty_workspace = WorkspaceEmptyState::for_app(app);`。editor_rendered 使用 empty_workspace.is_none()；普通工作区分支使用同一值决定是否绘制空态。保留 Relation/Dashboard/RedisBrowser 专用分支优先级。
5. pane_drag_invalid 的 split match 新增 EditorHeight 分支：

```rust
PaneSplit::EditorHeight => {
    !editor_rendered || layout.pane_resize_region(drag.split).is_none()
}
```

复用已有失效处理释放 drag 和 PaneResize gesture，其他 split 逻辑不变。后续高亮不会再绘制隐藏分隔线。
6. 空态不调用 editor/result renderer，避免注册不可见面板热区；利用既有逐帧 reset 清理 viewport/cursor，不新增重复清理。ExplorerWidth 调整仍正常。
7. 重跑 Task 1 命令，预期通过。

**复核：** 不以 active_console_opt().is_none() 代替 tabs.is_empty()；不修改 App 关闭 reducer、连接/session 生命周期、事务确认或持久化格式；不重构 AppLayout。

**验收：** 正确显示在线无 tab 空态，真实关闭及重新打开闭环通过。

### Task 3：覆盖状态与布局边界

**Files:** 修改 `tests/ui_render.rs`；回归 `tests/workspace_tabs.rs`（默认不修改）。

1. session 兜底：fixture 关闭全部 tab 后保留 session，将 connection.status 投影设为 Disconnected；断言仍显示 NoOpenTabs，不落入 SQL 面板。
2. drag 生命周期：正常帧获取 EditorHeight resize 区，沿用现有测试方法建立 drag/gesture；关闭 tab 后复用 UiState 渲染，断言拖动状态和对应热区消失。
3. 尺寸表：120×36、180×40 显示空态；80×24 和最大化 Explorer 保持 Explorer 独占、不要求右侧 logo；40×10 保留 TERMINAL TOO SMALL。所有场景不得生成空 SQL 面板。
4. 运行既有启动无连接/有配置未连接、offline_console_renders_the_complete_workspace 和 Relation/Dashboard/RedisBrowser 测试；优先复用既有覆盖，仅在缺乏可见内容断言时补必要回归。

```sh
cargo +1.94.0 test --test ui_render --test workspace_tabs
```

**验收：** 定向集通过，旧空态、其他 tab 类型、焦点和重开路径无回归。新发现的普通失败自行诊断修复；记录基线/环境差异。

### Task 4：完整检查与 Luna 收尾审查

**Files:** 复核 `src/ui/mod.rs`、`tests/ui_render.rs` 和本任务文档；更新原任务目录 validation.md。

1. 业务齐备后按项目口径运行：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
git diff --check
```

2. 逐条记录命令、退出码、代码版本/工作区状态、feature、环境及外部服务测试的执行/跳过事实。只有相关代码或环境变化才重跑相应验证。
3. Luna 审查真实 CloseTab 证据、在线连接保留、文档保存、专用 tab 渲染、交互热区与拖动一致性、重新打开恢复。审查和纠偏不切回 Astra。
4. 如有可选 PTY 环境限制，按门禁分级处理并如实交付，不升级为必须由用户完成的工作。
5. 在后续阶段授权提交时按显式路径 stage，不使用 git add .，不纳入其他任务文档或 `.git` 插件 metadata。推荐 commit message：`fix(ui): restore empty workspace after closing all tabs`。合并前核对 main 新增工作，遵照自动流程继续。

**验收：** 完整业务闭环通过、项目检查有明确结果、限制如实记录、改动范围受控，Luna 完成阶段性审查。

## 4. 当前阶段完成与自动交接

此文件是任务系统指定的完整计划。docs/plans/2026-09-18-no-open-tabs-empty-state-implementation.md 为此前草案参考；其中“尚无 plan token”仅属历史，此次以当前用户授权和本文件为准。

本 plan 阶段仅写文档与指定回执；下一实施动作是 Luna 按自动命名/工作区流程建立任务工作环境并执行 Task 1。用户已选自动工作流，不再询问执行方式，不等待 resume。当前计划完成后写入本轮唯一 plan 回执，后续阶段使用届时新授权 token。
