# LazyDB / Kitty Smart Pane Resize Implementation Plan

> **执行者：Luna。** 本文由 Astra 使用 writing-plans 技能在 plan 阶段编写；实施、审查、纠偏、提交合并均由 Luna 完成。不启动子 Agent，不修改插件维护的 state.json/checkpoint.json。用户已选择自动工作流，无需再次选择执行方式。本次 plan 回执 token：81780289-387a-45b8-954e-791302699bda。

**Goal:** Cmd+Ctrl+Shift+h/j/k/l 优先调整 LazyDB 当前 pane 指定方向的可见内部边界，无可移动内部边界时由 Kitty 相对调整承载窗口。

**Architecture:** 纯布局决策返回内部目标尺寸、Kitty 回退或阻止三种结果；Runtime 负责准确布局、状态动作及有界远程调用。复用已有 IS_LAZYDB 标记和 pane 尺寸模型，由项目维护的无 UI Kitty helper 处理外部分隔线，不依赖私人 Neovim 插件路径。

**Tech Stack:** Rust、Ratatui、Crossterm、Tokio、TOML；Kitty remote control 与 Python no_ui kitten。

---

## 0. 基线与执行约束

- 工作区 `/Users/yelog/workspace/tui/lazydb`；目标 main；分析基线 `5f8c149a947364eac09eb761a8141a93adcba4a9`。分析时 HEAD/main 一致且工作区/index 干净；这是历史证据，实施前仍须核对当时状态。
- 分析详见同目录 `analysis.md`，实际检查见 `validation.md`。当前计划依据本会话已读取的源码，未重跑测试。
- 报告、进度与验证结果只写本任务目录。计划阶段不改业务代码，不创建分支/worktree、不部署用户配置。任务名和实施分支由 Luna/工作流在计划完成后命名。
- 实施允许工作区存在用户未提交修改，不 stash、不整体提交或清空。若切换 worktree，必须明确未提交文件不会自动复制。本方案不依赖分析时的未提交文件。
- 本文分三个可验收闭环；逐个完成后继续，不以等待 resume 结束。每个闭环内定向测试，功能完整后统一全量验证。提交粒度以完整可验收单元为准。
- CONTRIBUTING.md 要求：配置变更同步 `config/default.toml`、配置测试及文档；快捷键同步共享帮助目录与 `docs/keybindings.md`；App::update 不做 I/O。

### 门禁分级

- **用户需求验收：** LazyDB 与 Kitty 智能调整链路可用；指定方向可调整的内部边界优先，否则外部回退；核心示例为非全屏 Explorer 的 Cmd+Ctrl+Shift+l。实现应提供完整配置与部署路径。
- **项目强制门禁：** CONTRIBUTING.md 的格式化、clippy、全目标全特性测试，以及配置、共享快捷键目录、文档同步。相关自动测试必须能证明内部/外部互斥分流，不能只证明命令构造。
- **本方案实现决策：** 四方向、步长 3、opt-in、尺寸极限回退、Redis 可见子边界及目标窗口精确定位。相应自动测试作为实现正确性证据；这些是为落实需求做出的设计选择，不声称用户逐项指定过。
- **补充建议验证：** 真实 Kitty/PTY 人工操作、截图或尺寸采样、Neovim 交互回归。它们不成为新强制门禁；环境受限按一次针对性修复重试和 Luna 收尾审查处理，不要求用户手工验收才能继续自动工作流。

### 预计变更范围

完整仓库相对路径保存在同目录 `change-scope.json`。新增 Rust 测试放在本计划列出的源码文件 `#[cfg(test)]` 模块内，Python 测试放在 `contrib/kitty/test_lazydb_resize.py`；本计划不预计修改现有独立 tests 文件，只按需运行其回归测试。无预计删除或重命名，无未提交依赖文件。

仓库外 Kitty 配置、helper 部署与实际生效 settings 不可伪装为仓库相对路径，因此不放进 change-scope.json；其准确已知路径与 settings 定位步骤单独列在 4.1，实施阶段记录实际变更。仅读取的 Neovim 配置、relative_resize.py、CONTRIBUTING.md、src/terminal.rs 不列入修改清单。

## 1. 固定行为契约

1. 步长 3；新增 opt-in 命令 `smart-resize-pane-left/down/up/right`，默认空绑定。
2. h/l 分别移动当前 pane 左/右内部边界，k/j 分别移动上/下内部边界，不寻找相反侧替代边界。最右 SQL Editor 按 l 必须回退 Kitty。
3. 候选尺寸 clamp 后与当前实际尺寸不同，则只内部处理；仅剩 1–2 个字符时只移动剩余量。完全不能移动才回退 Kitty，不拆分一次按键为两次操作。
4. overlay、Omni、无法取得终端尺寸、无法解析可见焦点、TooSmall：Blocked，内部与外部都不操作。
5. 最大化/窄屏模式不调整隐藏分隔线。Redis 最大化主区域仍展示 Keys/Preview 时，该可见内部边界仍有效。
6. Kitty 不可用时内部操作正常，Boundary 无操作。外部失败不得退出 TUI 或产生字符输入。
7. 焦点、编辑文本、选区和数据状态不受 resize 影响。Press/Repeat 调整，Release 忽略。

### 分隔线映射

| Source | Left | Right | Up | Down |
|---|---|---|---|---|
| Explorer | Boundary | ExplorerWidth +3 | Boundary | Boundary |
| SQL Editor | ExplorerWidth -3 | Boundary | Boundary | EditorHeight +3 |
| SQL Results | ExplorerWidth -3 | Boundary | EditorHeight -3 | Boundary |
| Relation/Dashboard/PrincipalDdl | ExplorerWidth -3 | Boundary | Boundary | Boundary |
| Redis Keys | ExplorerWidth -3 | RedisKeysWidth +3 | Boundary | Boundary |
| Redis Preview | RedisKeysWidth -3 | Boundary | Boundary | Boundary |

任何内部映射都以对应可见分隔线存在且实际可移动为前提。

## 2. 单元一：水平按键 → LazyDB / Kitty 完整闭环

### 2.1 建立真实布局上的纯决策

**文件：**
- 新增 `src/model/pane_resize.rs`，在 `src/model/mod.rs` 注册。
- 修改 `src/ui/layout.rs`：仅提取必要的尺寸边界/复算接口，复用现有 min/max 常量。
- 修改 `src/runtime.rs`：布局 snapshot 构建与焦点目标解析。

**步骤：**
1. 为 `smart_resize_explorer_right_is_internal`、`smart_resize_editor_right_is_boundary`、`smart_resize_maximized_explorer_is_boundary` 建立布局驱动用例，使用 180×50 SQL 布局，断言目标 split、实际尺寸和分流结果。
2. 运行 `cargo test --lib smart_resize`，记录失败确实来自待实现行为，不以编译环境错误冒充业务失败。
3. 实现纯决策结果类型，建议：

   ```rust
   pub enum SmartResizeDecision {
       Internal { split: PaneSplit, size: u16 },
       Boundary(PaneDirection),
       Blocked,
   }
   ```

   函数输入当前源 pane、方向及由当前 AppLayout 构建的可见边界/尺寸信息。尺寸限制来自 layout，model 不导入 Kitty，也不调用 Runtime。
4. 实现表内 Explorer/SQL Editor/SQL Results 水平方向；候选尺寸以当前实际 width 为起点加减 3，在布局约束内钳制，比较实际结果。
5. 构建 snapshot 时复用 AppLayout::calculate，特殊 tab 列表与当前渲染一致；先判断 app.focus==Explorer，再读取 Redis tab.focus，避免复制已有 handle_smart_focus 的 Redis 源目标错误。
6. 再运行同一过滤测试，预期通过。新增边界辅助函数如影响原布局，补跑 `cargo test --lib ui::layout::tests`。

**注意：** SQL Editor/Results 之间有 tabs 行，不应把所有合法边界定义为矩形严格相邻。不能把 l 翻译为旧 Vim `>` 操作。

### 2.2 配置、按键和 Runtime 接入

**文件：** `src/config.rs`、`config/default.toml`、`src/input/panes.rs`、`src/input/keymap.rs`、`src/action.rs`、`src/app.rs`、`src/runtime.rs`。

**步骤：**
1. 在配置命令集合与 `[keybindings.panes]` 默认中加入四条空绑定；建立默认配置加载、显式 Cmd+Ctrl+Shift 绑定的测试。
2. 添加独立 SmartResizePane Action 和方向命令映射，不更改 SmartFocusPane 的语义。
3. 在 overlay/Omni 接管之后、普通编辑输入之前匹配 resize。清除 pending sequence；支持 Press/Repeat，忽略 Release。
4. 检查 KeyBindings::matches 与 parse_key 对 Shift 字母编码的实际行为，增加小写/大写携带修饰键两种输入用例；只在必要范围规范化，不能把普通大写文本都转成小写。
5. Runtime 在现有 SmartFocusPane 拦截位置旁接入 resize handler。读取当前 terminal.size，创建最新 snapshot，调用纯 resolver。
6. Internal 分支先用当前 snapshot 更新 PaneLayoutChanged，再经 apply_action 发 SetPaneSize。不得直接写 app.pane_sizes，不依赖上一帧的指标；当前偏好可能超限，要用布局钳制后的实际尺寸。
7. Boundary 分支调用 Kitty adapter；Blocked 无副作用。App::update 的新 runtime-only Action 保持无 I/O。
8. 建立可注入/可替代外部调用的最小测试 seam，验证内部分支调用外部次数为 0、Boundary 为 1、Blocked 为 0。无需把整个 Runtime 抽象成新框架。
9. 运行 `cargo test --lib smart_resize`；配置用例纳入该名称过滤，或显式运行 `cargo test --lib config::tests`。验证 Insert 模式文本、焦点不变。

### 2.3 Kitty 定向相对 resize

**文件：** `src/terminal/kitty.rs`；新增 `contrib/kitty/lazydb_resize.py`、`contrib/kitty/test_lazydb_resize.py`。

**步骤：**
1. 为 adapter 请求构建与外部调用建立测试；命令必须使用独立 argv：

   ```text
   kitten @ --to <socket> kitten --match id:<window> lazydb_resize.py <direction> 3
   ```

   方向为 left/right/up/down；不要沿用 neighboring_window 的 top/bottom 转换。
2. 小幅提取已有 focus 的 remote executor，保留 stdio null、1 秒超时、kill 与错误返回。环境变量缺失不执行；不硬编码配置目录/socket，不用 shell 字符串拼接。
3. helper 校验方向和 amount，按 target_window_id 取得 window，并从该 window 确定所属 tab。实施时核对本机 Kitty 0.47.4 的实际 Python API，再选取能指定目标窗口的 resize 调用；不能用当前 active_tab/active_window 替代，也不能切换焦点。
4. 用目标 tab 的 layout 查询目标窗口邻居。方向计算独立为可测试小函数：

   | 方向 | 正向/反向邻居情况 | resize 操作 |
   |---|---|---|
   | left | 有 right（含两侧都有） | narrower |
   | left | 仅 left | wider |
   | right | 有 right（含两侧都有） | wider |
   | right | 仅 left | narrower |
   | up | 有 bottom（含上下都有） | shorter |
   | up | 仅 top | taller |
   | down | 有 bottom（含上下都有） | taller |
   | down | 仅 top | shorter |

   该轴无邻居、stack 不可调整、目标关闭时返回无操作。不加入旧 relative_resize.py 的 tmux/foreground_cmdline 特例。
5. 使用 Python unittest 与最小 Kitty API doubles，测试所有邻居组合和目标窗口不等于活动窗口；核心验收是“只对目标所属 tab/window 操作且不切焦点”。
6. 运行 `python3 -B -m unittest discover -s contrib/kitty -p 'test_*.py'`，预期全部通过。Rust adapter 测试覆盖成功、非零退出、spawn 失败、timeout kill；不要通过并行全局改 PATH/环境制造不稳定测试。
7. 补齐 helper 缺失/remote 失败的日志路径，保持 UI 可用。不在失败后再调用另一种 resize 导致双重动作。

### 2.4 单元一文档与验收

**文件：** `docs/kitty-integration.md`、`docs/keybindings.md`、`src/help.rs`。

- 将 guide 从“只支持焦点”更新为同时支持智能 resize，区分内部指定侧语义和 Kitty 相对 resize 语义。
- 给出 helper 安装、LazyDB 四条显式绑定、Kitty 四条 IS_LAZYDB shift 放行配置；说明放在普通 resize 映射之后。默认仍 opt-in。
- 在共享 shortcut catalog 增加新命令，展示实际配置绑定，不伪装成旧 Ctrl-w operator。配置与目录相关测试同步。
- 验收：Explorer l 内部 +3；最右 Editor l 仅外部一次；最大化 Explorer l 仅外部；缺 Kitty 环境仍可内部 resize；旧焦点键与普通输入保持正常。
- 记录定向结果和 diff 状态到 validation.md。可提交为一个完整水平闭环提交；只显式暂存本任务文件，不 `git add .`。

## 3. 单元二：四方向和全部可见 pane

**文件：** `src/model/pane_resize.rs`、`src/ui/layout.rs`、`src/runtime.rs`、`src/input/keymap.rs`；测试放在这些文件的测试模块内。

**步骤：**
1. 将第 1 节映射表逐行转为测试，覆盖 Relation/Dashboard/PrincipalDdl 与 Redis。
2. 实现 EditorHeight 和 RedisKeysWidth 处理，复用 RedisBrowserLayout::calculate / resize_region，不复制宽度下限常量。
3. 添加 Redis Explorer 焦点用例：Redis tab 激活但 Explorer 有焦点时，l 必须处理 ExplorerWidth，而不是 RedisKeysWidth。
4. 如果提取 helper 被 smart-focus 共用，添加相应焦点回归；只修复该共享路径直接相关的问题，不扩展为导航重构。
5. 添加 Focus 模式（宽度 99）、Standard 起点（100）、Wide（180）、TooSmall（宽<56 或高<16）及各 pane 最大化测试。Redis 最大化情况下 Keys/Preview 可见边界继续有效。
6. 用实际边界构建 min/max 测试：有 1/2 格空间时只内部；空间为 0 时 Boundary。显式断言一次请求不会既内部又外部。
7. 连续执行两次 resize，中间不依赖旧 UiState 刷新，断言总位移按当前实际尺寸累计。再测试终端变窄和偏好超界后不会尺寸跳跃。
8. 加入 overlay/Omni、Press/Repeat/Release、Editor Insert、pending sequence 回归。测试不得只验证 action 名称，至少检查文本/焦点/外部调用次数。
9. 定向运行：

   ```bash
   cargo test --lib smart_resize
   cargo test --lib smart_pane
   cargo test --lib ui::layout::tests
   python3 -B -m unittest discover -s contrib/kitty -p 'test_*.py'
   ```

   仅相关代码改变时重跑相关组。Ctrl-w 与鼠标受影响时，运行现有 pane resize 定向用例；不要无理由运行全部数据库测试。
10. 更新 guide 中的行为表与边界说明。记录结果并完成第二个验收闭环。

## 4. 单元三：本机接入、全量验证与 Luna 审查

### 4.1 本机接入

**仓库外目标：** `/Users/yelog/.config/kitty/kitty.conf`、其目录下独立 `lazydb_resize.py`，以及实际生效的 LazyDB settings 文件。

1. 实施阶段重新确认用户配置，仅读取必要字段；分析时 `~/.config/lazydb` 不存在，须循项目 settings discovery 查到真实路径，不能创建一个猜测路径并宣称已启用。
2. 对 Kitty 增量加入四条 `map --when-focus-on var:IS_LAZYDB kitty_mod+shift+<h/j/k/l>`。保留普通 relative_resize 与 IS_NVIM 条目。对 LazyDB 活跃配置增量加入四条 smart-resize 绑定，保留用户其他配置。
3. 部署独立 helper，不覆盖 Neovim 的 relative_resize.py。记录仓库外文件和变更范围，它们不随 Git worktree/合并自动部署。
4. 可行时重载并验收；不能确定重载成功时如实记录，不把文件写入等同生效。

### 4.2 项目强制验证

功能齐备后执行一次 CONTRIBUTING.md 规定检查：

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

记录命令、真实退出结果、当时 commit/diff、环境。测试跳过数据库集成时记录其前置条件；不能称数据库测试全覆盖。普通编译/lint/测试错误由 Luna 修复，相关改变后只重跑必要项，最终保持对应代码版本的有效结果。

### 4.3 补充 Kitty/PTY 检查

- 分别验收 SQL Explorer 内部 l、最右 Editor 外部 l、h/k/j、最大化、Redis、overlay、连按、退出后 Kitty/Neovim 映射。
- 实际记录窗口 ID、焦点及前后尺寸；命令返回 0 不能单独证明目标分隔线动了。
- 分析环境有 kitten 与 KITTY_LISTEN_ON，但没有 KITTY_WINDOW_ID。此限制不等于用户实际 Kitty 无法使用功能；实施时先判断能否构建独立受控环境，不修改任意活动用户 pane 来凑验证。
- 人工/PTY 是补充证据，与项目强制自动检查区分。环境失败最多一次针对性修复重试，再由 Luna 收尾审查记录限制或补充其他证据；不重复 progress 无限尝试。

### 4.4 Luna 审查与交付

逐项审查：

- 指定边界语义严格符合需求；不存在 Editor l 错误调整 Explorer 的路径。
- 纯决策无外部副作用，runtime-only Action 已接入事件循环且能重绘。
- 真实布局限制、快速重复与当前 metrics 一致；未把隐藏 pane 算作可调整。
- helper 使用目标 window/tab，焦点不变；错误/timeout 不造成双重动作。
- opt-in 默认、配置校验、帮助目录和文档一致；本机配置部署有独立记录。
- 测试证明行为而非只重复实现；所有“通过”对应最终相关代码版本。

Luna 处理审查问题、提交及按工作流合并 main。提交仅包含任务代码与项目文档，不提交 `.git/opencode-tasks` 或用户配置。操作前核对用户未提交工作并遵循当时工作流指令。

## 5. 完成标准与下一步

实现完成须同时满足：水平核心场景、四方向表格、边界和失败处理、配置/helper/文档交付、自动化定向证据、项目强制检查，以及对补充运行验证的真实结论。环境限制可由 Luna 审查记录，不能把未实现能力记为限制。

**当前计划阶段已完成；下一执行动作：Luna 按工作流建立任务分支/工作区后，从单元一的真实布局 resolver 用例开始，完成水平按键到 Kitty 的完整闭环。** 本阶段未写业务代码、未运行实现测试；已补齐 change-scope.json，最后写入本次指定 plan 完成回执。
