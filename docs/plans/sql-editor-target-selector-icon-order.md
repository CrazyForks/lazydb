# SQL Editor 数据库图标前置 Implementation Plan

**Goal:** 将 SQL Editor 的 TARGET SELECTOR 候选行调整为“选中标记 → 数据库图标 → 连接名称 → 数据库/schema”。

**Architecture:** 在 `render_overlay` 的 `Overlay::TargetSelector` 分支内拆分连接名与选中标记的文本片段，调整 Span 顺序。沿用现有图标颜色、文本样式及终端单元宽度计算；两个打开目标选择器的入口共用该修复。

**Tech Stack:** Rust 2024、ratatui 0.30.2、现有 IconSet 与终端单元宽度工具。

---

## 执行上下文

- 分析依据：同目录 `analysis.md`。
- 原工作空间：`/Users/yelog/workspace/tui/lazydb`。
- 目标分支：`main`；分析基线：`f1d897400be6cb356194d2bfe3d4b11eee15e989`。
- 本文件仅为 plan 阶段产物，实际实施由插件安排；任务分支名称待 Luna 确定。
- 用户已选择自动工作流继续实施，完成计划后直接交由插件推进，无需再次询问执行方式。
- 分支、worktree、阶段状态与提交时机遵循插件后续指令。不要修改 `state.json`，不要启动子 Agent。
- 下述行号以分析基线为准；实施时按 `Overlay::TargetSelector` 和局部变量定位。

## Task 1：确认实施基线

**Files:**
- Read: `src/ui/mod.rs:4932–5063`
- Read: 本任务目录下的 `analysis.md`

1. 在插件提供的实施工作目录执行 `git status --short` 与 `git rev-parse HEAD`，确认当前分支基线以及是否存在用户改动。
2. 定位 `render_overlay` 的 `Overlay::TargetSelector` 分支，确认当前首个 Span 中包含 `profile_label`，图标在其后追加。
3. 若代码已发生相关变化，先核对分析是否仍适用，避免覆盖已有修复。实际目标是 `TARGET SELECTOR`，不是相邻的 `Overlay::DatabaseSelector`。

**完成条件：** 找到与分析对应的拼接代码，并确认局部修改可以安全应用。

## Task 2：重排目标行片段

**Files:**
- Modify: `src/ui/mod.rs:5005–5024`

**Step 1：实施局部替换**

将从 `let icon = ...` 到 `Line::from(spans)` 的片段替换为：

```rust
let icon = profile.map(|profile| icons.database(profile.kind));
let icon_width = icon.map_or(0, |icon| usize::from(icon.cell_width()));
let prefix = format!("{marker} ");
let text_width = usize::from(inner.width)
    .saturating_sub(usize::from(prefix.cell_width()))
    .saturating_sub(usize::from(profile_label.cell_width()))
    .saturating_sub(icon_width.saturating_add(1));
let mut spans = vec![Span::styled(prefix, text_style)];
if let Some(profile) = profile {
    spans.push(Span::styled(
        format!("{} ", icons.database(profile.kind)),
        Style::new()
            .fg(icons.database_color(profile.kind))
            .bg(background),
    ));
}
spans.push(Span::styled(profile_label, text_style));
spans.push(Span::styled(
    truncate_to_cells(&target_label, text_width),
    text_style,
));
Line::from(spans)
```

**Step 2：检查差异**

执行 `git diff --check` 和 `git diff -- src/ui/mod.rs`。

预期：prefix 只包含标记及空格；连接名宽度被单独扣除；连接名 Span 位于图标之后。图标继续单独着色，连接名使用原来的 `text_style`。不存在 profile 时保留原有降级行为及原有一列间距预算。

**完成条件：** 业务修改集中在上述局部片段，显示顺序正确，原有终端文本清理和目标文本截断仍然生效。

## Task 3：验证渲染与交互

**Files:**
- Existing tests: `tests/ui_render.rs:6858–6911`
- Existing tests: `tests/mouse.rs` 中名称包含 `target_selector` 的测试

这是低影响、可逆的显示顺序修复，复用已有测试，不新增只复述 Span 拼接实现的测试。

**Step 1：格式检查**

```sh
cargo fmt --check
```

预期退出码 0。如需格式修正，修正后检查差异，避免引入无关格式改动。

**Step 2：现有 UI 验证**

```sh
cargo test --test ui_render target_selector
```

预期两项现有目标选择器测试通过，覆盖目标内容、current、导航提示、滚动行与鼠标区域。

**Step 3：现有鼠标验证**

```sh
cargo test --test mouse target_selector
```

预期匹配的行选择、取消与点击确认测试通过。

**Step 4：视觉验收**

在可用的 TUI 环境打开 SQL Editor 的默认执行目标选择面板，核对：

- 普通行与选中行都显示为 `标记 图标 连接名: 数据库.schema`，current 仍按原条件显示。
- 图标只出现一次，颜色仍跟随数据库类型；选中背景和文本粗体正确。
- NerdFont、Unicode、Ascii 模式顺序一致；例如 Ascii SQLite 行形如 `> SQ 连接名: :memory:.main current`。
- 中文 / 长连接名及窄窗口下没有因漏扣连接名宽度导致目标文本预算增大。
- 超过 16 项时仍能导航，点击候选行与取消操作正常。
- 复核无对应 profile 的候选能够安全降级，以及连接名、数据库名、schema 的终端文本清理仍然生效。

若环境无法实际进行视觉核对，应在实施结果中明确记录未执行项；已有自动测试没有直接断言图标与连接名的顺序，不能将测试通过等同于视觉验收完成。

**完成条件：** 格式检查和针对性测试通过，视觉核对结果有记录；如出现失败，定位并解决与本次修改相关的问题后再交付。

## 交付与验收摘要

1. 实际代码改动预计只有 `src/ui/mod.rs`。
2. 两种 `TargetSelector` 入口共用图标前置顺序。
3. 提供改动摘要、执行的检查及结果、未执行的验证项。
4. 提交、命名及阶段完成回执按插件当阶段给出的路径和 token 执行，不沿用 analyze 阶段 token。

## 本阶段完成记录

已先读取 `analysis.md`，调用 writing-plans 技能，并完成逐项计划复核：修改文件与步骤明确，替换代码完整，包含差异复核、验证命令及预期结果、视觉验收标准和降级情况检查。本阶段未修改业务代码，未执行上述实施或验证命令，未修改 `state.json`，未创建或切换 worktree，未启动子 Agent。

计划已保存到用户指定路径。复核完成后，最后写入 `plan-fec5b242-d476-4b0b-8749-b30cd4f377c6.json`，使用本次 plan 阶段 token `fec5b242-d476-4b0b-8749-b30cd4f377c6`，状态为 `completed`；随后由插件安排下一阶段。
