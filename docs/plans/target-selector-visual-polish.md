# Target Selector 视觉一致性 Implementation Plan

**Goal:** 消除 SQL Editor 执行目标选择面板的重复标题，补齐彩色数据库类型图标，并统一快捷键提示样式。

**Architecture:** 在 `src/ui/mod.rs` 的 `Overlay::TargetSelector` 分支局部调整渲染，复用 `IconSet` 与 `shortcut_hints`。以同一个内部矩形和可见行数量计算绘制位置与鼠标区域，保持候选索引及交互语义。

**Tech Stack:** Rust 2024、ratatui 0.30.2、项目现有 IconSet / Theme / ShortcutHint、Rust 集成测试。

---

## 执行约束与上下文

- 前置分析：同目录 `analysis.md`，以其中方案 A 为依据。
- 起点：`5ea15a79c351d50e79c4422dad9133d8e4fea8d2`；目标分支 main。
- 本文仅制定实施计划，不执行代码修改、测试、提交或创建 worktree。
- 名称、任务分支和 worktree 生命周期由插件管理；实施阶段使用插件指定的工作空间。
- 不修改 `state.json`，不启动子 Agent，不自行进入下一个阶段。
- 主要修改文件：`src/ui/mod.rs`、`tests/ui_render.rs`。行号均为分析基线的参考，实施时按符号定位。
- 不需要新增依赖、配置、用户文档或公共组件 API。

## Task 1：移除重复标题并统一布局坐标

**Files**
- Modify: `src/ui/mod.rs:4932-5040`，`render_overlay` 中的 TargetSelector 分支。
- Test: `tests/ui_render.rs:6858-6910`，已有两个 TargetSelector 渲染用例。

### Step 1：更新现有验收断言

将 `target_selector_renders_real_target_and_navigation_hint` 的正文标题预期改为外框唯一标题：

```rust
assert_eq!(output.matches("TARGET SELECTOR").count(), 1);
assert!(!output.contains("EXECUTION TARGET"));
```

继续保留真实目标、current、Enter confirm、Cancel 等已有断言。

在 `target_selector_renders_visible_rows_and_mouse_regions` 中，基于已有渲染输出与实际 hit region 检查对应行包含相应数据库名称、Cancel 区域所在行包含 Cancel，避免只检查区域枚举存在却遗漏一行偏移。

### Step 2：调整布局

1. 保留 `panel_block(" TARGET SELECTOR ", true, theme)`；移除正文标题行及其占用空间。
2. 自然高度使用最多 16 个候选项加 5 行（2 行边框、1 行间隔、1 行提示、1 行 Cancel），沿用最低高度 8 的策略。
3. 从 block 获取 `inner`，绘制外框后在 inner 中绘制正文；或者保留带 block 的 Paragraph，但所有坐标必须源自同一 inner。
4. 对正常空间，候选列表从 `inner.y` 起，提示位于 `inner.y + visible_count + 1`，Cancel 位于 `inner.y + visible_count + 2`。
5. 最终可见数量应为候选总数、16、`inner.height.saturating_sub(3)` 三者的最小值；用该值计算 start/end，而不是先按 16 截取后再裁切。
6. 继续使用现有选择窗口算法，确保当前 selected 在可见窗口内，注册的索引是 `start + offset`。
7. 极小窗口没有容纳完整内容的空间时，按实际 inner 裁切；空 inner 不绘制正文也不注册正文 hit region。提示或 Cancel 仅在对应行位于 inner 内且宽度非零时绘制和注册。不要把边框当作正文可点击区域。

期望：常规窗口候选行比原来上移一行；候选名称与点击索引完全一致；原有 Cancel 操作仍存在。

### Step 3：运行局部验证

```sh
cargo test --test ui_render target_selector
```

期望：更新后的标题与列表 / Cancel 行对应断言通过。若还有旧坐标断言，只按新可见布局修正，不削弱行为断言。

## Task 2：按候选连接类型绘制彩色图标

**Files**
- Modify: `src/ui/mod.rs`，同一 TargetSelector 候选行构造代码。
- Reference: `src/ui/icons.rs:240-280`。
- Reference: `src/ui/mod.rs:3028-3073`，现有独立图标 Span 的模式。
- Test: `tests/ui_render.rs`，扩展已有 TargetSelector 渲染覆盖。

### Step 1：拆分候选行样式

每条候选项只查找一次 `target.profile_id` 对应的 profile。将当前单一 label Span 拆成以下三个部分：

1. 选择标记及空格，沿用行文字样式。
2. 图标及一个空格，使用候选 profile 的 `icons.database(profile.kind)`；图标前景色使用 `icons.database_color(profile.kind)`。
3. 连接名称、数据库、可选 schema、current 后缀，沿用原文本样式。

行样式继续遵循 selected 优先于 current：selected 为 `theme.text + theme.selection + BOLD`，未选中的 current 为 `theme.accent`，普通行为 `theme.text`。图标独立指定类型前景色，背景与其所在行一致。

目标形式：

```text
> [数据库图标] connection: database.schema current
```

其中方括号仅为本文说明，实际显示 IconSet 返回的字形或缩写。

### Step 2：保持宽度与文本清理

- 对名称、数据库和 schema 继续调用 `sanitize_terminal_text`。
- 根据 marker 和图标前缀的实际显示单元格宽度，使用 saturating subtraction 计算剩余文本宽度，再调用现有 `truncate_to_cells`。
- 使用模块现有显示宽度工具，不能用字符串字节数估算 Nerd Font / 中文宽度。
- 图标前缀自身也受正文区域裁切，不得越过边框。
- 找不到 profile 时跳过类型图标和连接名称，保留数据库 / schema 文本，不 unwrap、不使用活动 profile 冒充目标 profile。

### Step 3：验证实际渲染颜色

复用 `tests/ui_render.rs` 中已有 TestBackend / buffer 检查模式，以两种不同 kind 的 profile 构造候选项，验证：

- 图标来自各自 profile，而非全行使用活动连接类型。
- 图标实际 buffer 前景色等于对应 `database_color`。
- 选中图标与同一行文字具有相同选中背景，图标保留类型色。
- current 行仍可辨识。

优先扩展现有测试；仅在现有输出辅助函数不暴露 buffer 时添加一个有针对性的样式测试，不为每种数据库复制一套相同用例。

```sh
cargo test --test ui_render target_selector
```

期望：图标、色彩和已有列表交互渲染断言通过。

## Task 3：复用 New Schema 快捷键组件

**Files**
- Modify: `src/ui/mod.rs`，TargetSelector 底部提示。
- Reference: `src/ui/catalog_editor.rs:350-405`。
- Reference: `src/ui/shortcut_hints.rs:32-95`。
- Test: `tests/ui_render.rs`，TargetSelector 渲染覆盖。

### Step 1：替换纯文本提示行

用如下结构替换原 `Line::raw` 提示（变量 `inner` 使用 Task 1 的统一正文区域）：

```rust
shortcut_hints::line(
    &[
        shortcut_hints::ShortcutHint::new("j/k or Up/Down", "select"),
        shortcut_hints::ShortcutHint::new("Enter", "confirm"),
        shortcut_hints::ShortcutHint::new("Esc", "cancel"),
    ],
    inner.width,
    theme,
    theme.surface_raised,
)
```

该组件负责键名的 `theme.action + BOLD`、说明的 `theme.text`、三空格间隔以及窄宽度省略标记。保留左对齐和独立 Cancel 行。

### Step 2：验证提示样式与窄宽度

在 Task 2 使用的 buffer 验证中检查 Enter 的前景色和加粗属性，confirm 的前景色为正文色，两者背景为面板背景。正常宽度仍包含三组操作。

窄宽度使用组件已有省略行为，不为这个调用点重复测试组件所有打包算法；仅检查实际提示没有写入面板边框或覆盖候选行。

```sh
cargo test --test ui_render target_selector
```

期望：Target Selector 的提示与 New Schema 使用相同样式规则，全部局部测试通过。

## Task 4：完整局部回归与交付检查

**Files**
- Review: `src/ui/mod.rs`、`tests/ui_render.rs`。
- Existing tests: `tests/mouse.rs`、`tests/keymap.rs`、`tests/connection_switch.rs`、`tests/workspace_tabs.rs`。

### Step 1：复查边界场景

使用已有 fixture 和可用的终端预览方式检查：

- 普通候选列表与超过 16 项的末尾选择。
- 多 profile 候选列表，包含 selected 和 current 状态。
- Nerd Font、Unicode、ASCII 三种图标模式。
- 中文 / 长连接名称及控制字符清理。
- 短窗口、窄窗口、空候选列表、缺失 profile，不 panic，hit region 对应实际绘制的正文。

不要求连接真实数据库来验证纯渲染内容；已有目标切换测试负责行为回归。无法做真实终端视觉检查时，交付说明实际完成的是 buffer 验证，不声称截图验收。

### Step 2：执行检查

```sh
cargo fmt --check
cargo test --test ui_render target_selector
cargo test --test mouse target_selector
cargo test --test keymap target_selector
cargo test --test connection_switch target_selector
cargo test --test workspace_tabs target_selector
git diff --check
```

期望：命令退出码均为 0，筛选后的测试实际被执行而非全部过滤。出现环境依赖问题时记录原始失败原因，不能标记测试通过。

如果 Task 3 后没有进一步修改，可复用已完成的 ui_render 测试结果，无需仅为形式再次运行。

### Step 3：审查 diff 并提交实施结果

- 核对只有计划内的渲染及必要测试调整。
- 确认连接请求、候选目标生成、键位动作和 current 目标解析语义没有被顺带改写。
- 记录执行过的检查及结果，按插件指定的阶段交付路径输出实施总结。
- 提交和后续阶段由插件流程安排；本计划不要求在 main 上直接提交或执行阶段之外的 git 操作。

## 完成判据

1. 面板仅有一个 `TARGET SELECTOR` 标题。
2. 每个已知 profile 的连接名前都有其数据库图标与统一类型颜色，选中态不覆盖图标前景色。
3. 底部快捷键使用共享组件，与 New Schema 的键名 / 说明样式一致。
4. 列表选择、确认、取消、滚动窗口及鼠标命中区域保持正确。
5. 相关局部检查通过，或明确报告阻塞；无无关业务逻辑修改。

## Plan 阶段产物说明

已按本次正式 plan 指令重新读取 `analysis.md`，调用 writing-plans 技能，并复核本计划的文件范围、逐项步骤、验证命令和验收标准。计划保存于任务目录 `plan.md`。

用户已选择自动工作流继续实施，后续由插件安排阶段、调用 Luna 命名及管理任务分支和 worktree，不再询问执行方式。本阶段不实施业务代码，不运行实施阶段的验证命令。

完成计划复核后，写入 `plan-893b062f-020f-43a2-ae25-f78fa6a1c673.json`，token 为 `893b062f-020f-43a2-ae25-f78fa6a1c673`、stage 为 `plan`、status 为 `completed`。
