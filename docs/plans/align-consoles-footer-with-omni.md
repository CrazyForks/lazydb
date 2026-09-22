# Consoles Footer Implementation Plan

> 执行者：Luna。按下列步骤完成一个端到端可验收单元；实现、审查、纠偏及提交合并均由 Luna 执行，不启动子 Agent。用户规定的阶段和目录约束优先于技能的通用执行流程。

**Goal:** 将 Consoles 快捷键提示固定在弹窗内底部，并统一为 Omni 的单行交互提示样式。

**Architecture:** 将 Consoles 的边框、正文和 footer 独立绘制，正文高度扣除 footer，删除模式另扣按钮行。复用 `ShortcutHint` 和 `shortcut_hints::render_interactive`，保留各模式输入与操作语义；最大弹窗宽度提高至 80，使标准 80 列终端可完整显示浏览态提示。

**Tech Stack:** Rust 1.94、Ratatui 0.30.2、Crossterm 0.29、项目既有 TestBackend 渲染测试。

---

## 基线与执行约束

- 原工作空间：`/Users/yelog/workspace/tui/lazydb`。
- 起点：`7b76fe9286fd99199ce6fc99a9e013264640b885`；目标分支：`main`。
- 分析时 HEAD 与起点一致；计划阶段复查 `git status --short --branch` 退出 0，仍为 `main...origin/main [ahead 2]`，工作区干净。
- `checkpoint.json` 在分析和本次计划时均不存在；不创建、不修改插件状态文件。
- 没有依赖需要复制到 worktree 的未提交文件。后续若出现新修改，应保留并重新确认与本任务的关联。
- 所有分析、计划和验证记录只写入 `.git/opencode-tasks/ses_f38c780acffeVR8tDuUOZLChUo/`。报告不会自动进入新 worktree，执行者应使用原工作空间下的绝对报告路径读取和更新。
- 任务名称建议：`Consoles 底部快捷键对齐 Omni`；最终任务名与分支名由 Luna/工作流决定。本阶段不创建分支或 worktree，不修改业务代码。
- 本计划仅一个业务闭环，不拆成需要反复用户确认的半成品任务。

## 单元 1：Consoles 贴底快捷键的完整修复

### 验收与验证的约束级别

| 级别 | 内容 | 完成判定 |
| --- | --- | --- |
| 用户需求，必需 | Consoles 提示参考 Omni 样式；定位在弹窗内部最下面，提示下方无多余内部空白 | 使用实际渲染结果验证贴底位置、按键/说明层级和居中布局 |
| 本次修复的回归要求，必需 | 输入、错误信息、删除确认不退化；长列表不覆盖 footer；窄尺寸不越界 | 复用既有自动测试，必要时补充针对根因的几何断言和定向检查 |
| 项目既有 Rust 门禁，必需执行并记录 | CI 中的 fmt、clippy、全量 Rust tests | Step 6 的三条 cargo 命令；环境受限须明确记录实际失败和审查结论，不能宣称通过 |
| 本计划的定向验证 | Step 5 三条测试命令及 `git diff --check` | 快速定位本次变更问题；这是实施安排，不冒称用户指定的测试命令 |
| 补充建议，可选 | 人工 PTY 截图、额外主题/终端组合的手工巡检、逐个点击所有提示 | 不作为自动工作流完成的新增强制门禁；自动渲染证据足够时可不执行 |

Step 5 的场景清单是用于组织自动回归和代码复核的检查点，并不要求为每一项新建测试或人工验收。优先加强已有测试中能捕获原始缺陷的断言。环境受限检查最多一次有针对性的修复重试，随后由 Luna 收尾审查记录限制或决定是否需要其他证据，不无限保持 progress。

### 修改范围

- 主要修改：`src/ui/mod.rs` 的 `render_console_manager()`（基线 6587–6901）。
- 验证/必要断言调整：`tests/ui_render.rs` 中 `console_manager_*` 用例（基线 3116–3283）。
- 复用但预计无需修改：`src/ui/shortcut_hints.rs`、`src/ui/theme.rs`、`src/ui/omni.rs`、`src/ui/dialog.rs`。
- 回归运行：`tests/console_manager_input.rs`、`tests/keymap.rs`。
- 不添加依赖，不变更公共主题或公共提示打包算法。

准确的预计变更清单写入同目录 `change-scope.json`：仅 `src/ui/mod.rs` 和 `tests/ui_render.rs`。上述回归运行和参考文件不在变更清单内。没有预计新增、删除、重命名的业务文件，也没有未提交依赖文件。报告和回执是 `.git` 下的任务元数据，不是将进入任务提交的仓库文件。不额外创建 `docs/plans` 文档。

### Step 1：核对实施工作区

在实施工作区运行：

```sh
git rev-parse HEAD
git status --short --branch
```

确认基于指定起点及当前实际 diff。任务分支/worktree 由后续工作流按其约定创建；不要复用原工作区全部未提交改动。记录实际版本和环境到原任务目录的 `validation.md`。

### Step 2：提取按模式提示，移除正文中的提示行

在 `render_console_manager()` 内按模式构造 `ShortcutHint::with_keys` 数组或 Vec。使用同文件既有 import 风格，避免建立新的抽象模块。

| 模式 | 按键标签 | 说明 | 点击发送的单个 KeyCode |
| --- | --- | --- | --- |
| Browse | `j/k` | `move` | `Down` |
| Browse | `Enter` | `open` | `Enter` |
| Browse | `a` | `new` | `Char('a')` |
| Browse | `d` | `delete` | `Char('d')` |
| Browse | `r` | `rename` | `Char('r')` |
| Browse | `/` | `search` | `Char('/')` |
| Browse | `Esc` | `close` | `Esc` |
| Search | `Enter` | `open` | `Enter` |
| Search | `Esc` | `cancel` | `Esc` |
| Rename | `Enter` | `save` | `Enter` |
| Rename | `Esc` | `cancel` | `Esc` |

每个事件使用 `KeyModifiers::NONE`。导航提示点击一次只向下移动一次，不能发送 j/k 两个互相抵消的事件。搜索模式不能展示 `j/k move`，因这两个字符属于搜索输入。

移除基线 6774–6783 浏览/搜索的正文尾部空行和纯文本 footer；移除 6802–6805 重命名的正文 footer。保留重命名标题、输入前间隔及错误信息。

删除确认继续走已有按钮和 `dialog::render_interactive_hint`，避免重复绘制或重复注册提示点击区域。

### Step 3：拆分绘制区域并贴底

将最大期望外宽从 72 调整为 80，仍调用现有 `centered()` 并保留高度范围 8–24。

布局和绘制顺序：

1. Clear 弹窗。
2. 构造 `panel_block(title, true, theme)`，使用 `block.inner(popup)` 获取内部区域。
3. 绘制 block；inner 为零尺寸时提前结束，防止生成虚假点击区域。
4. footer 为内部最后一行；删除模式还预留倒数第二行用于按钮。
5. body 为顶部起始、扣除保留行后的 Rect，仅将正文 `Paragraph::new(lines).style(theme.base())` 绘制到 body；不能再次给正文套一层 block。
6. 浏览/搜索/重命名 footer 调用 `shortcut_hints::render_interactive(frame, footer, &hints, theme, theme.surface, Alignment::Center, state)`。
7. 搜索、重命名输入单独绘制并注册原有光标、命中和选择目标，但必须先确认对应行落在 body 内。
8. 删除模式保留原有按钮、焦点、命中目标和 footer 文案；高度不足时仅绘制实际存在的区域。

核心区域计算可按下述形式实现；`deleting` 表示当前模式是 DeleteConfirm：

```rust
let block = panel_block(title, true, theme);
let inner = block.inner(popup);
frame.render_widget(block, popup);
if inner.is_empty() {
    return;
}
let reserved_rows = if deleting { 2 } else { 1 };
let body = Rect::new(
    inner.x,
    inner.y,
    inner.width,
    inner.height.saturating_sub(reserved_rows),
);
let footer = Rect::new(inner.x, inner.bottom() - 1, inner.width, 1);
```

删除按钮要求 `inner.height >= 2` 才有独立按钮行。搜索输入要求 body 至少一行，重命名输入要求 body 至少三行。所有命中区域应跟随实际绘制区域，不在不可见行注册交互。

正常模式高度计算可保持现有值：去除“正文 footer 行”后，独立 footer 占同一预算；最小高度留下的空白自然移到 footer 上方。搜索在内容较多时允许正文按现有裁剪方式显示，但 footer 必须保留。

### Step 4：确认样式与宽度预算

- 单行交互组件实际使用 `dialog_help_key()`：正文色加粗；说明使用 `dialog_help_description()`：muted 色。不要误用另一条非交互 `line()` 的 action 色逻辑。
- 背景传入 Consoles 的 `theme.surface`，不把整个面板强改成 Omni 的 `surface_raised`。
- 七项浏览提示共 74 列。最大外宽 80 可提供 78 列；80 列终端经 `centered()` 限制得到 76 列外宽、74 列内部，刚好全显。
- 更窄终端复用完整提示项省略及 `... (+N)`；允许末尾提示被省略，但键盘仍有效。
- 不添加多行 footer 或改公共组件；不引入 Omni 的滚动条/完整视窗系统。

### Step 5：定向验收并增强既有断言

先复用既有 `tests/ui_render.rs` 辅助及 `console_manager_fixture()`。这是一项低影响布局修复，不要求新测试框架或为每个提示编写镜像测试；优先在既有紧凑布局和模式用例内增加针对根因的几何断言。

最低验收集合：

1. **单条记录、80×24**：找到 Consoles 自身边框，最后一条内部行含 `Esc close`，其下一行就是底边框；footer 下方无内部空白行。
2. **四条记录、100×30 与 80×16**：保留既有排序、开放状态标记，以及 a/d/r/search/Esc 提示断言。
3. **空记录与空搜索**：空态仍显示，footer 在最后一行。
4. **搜索和重命名含错误**：原有光标与文本选择目标存在，错误未被正常尺寸 footer 覆盖，提示符合当前模式。
5. **长列表**：构造超过可见高度的记录，footer 仍在底部可见，最后一行不出现记录正文。无需新增滚动功能测试。
6. **窄窗口及极小窗口**：提示使用整项省略、边框没有被提示覆盖；无 panic，新增 Shortcut 命中区域不超出 popup/终端边界。
7. **删除确认**：既有 Cancel/Delete 按钮、默认焦点、`Enter activate` 仍正常。

如检查样式，直接检查 TestBackend 中 footer 的按键单元格为 BOLD、说明为 muted；不要仅凭整个终端字符串判断，因为背景可能有同名提示。交互验证可以复用 `render_with_state()` 返回的命中区域，确认 Enter/Esc 提示各自绑定一个正确事件。

运行：

```sh
cargo +1.94.0 test --test ui_render console_manager
cargo +1.94.0 test --test console_manager_input
cargo +1.94.0 test --test keymap console_manager
```

预期：命令退出 0，匹配测试全部通过。出现失败先判断是实际回归还是原测试假设与新布局不符，不应简单删除有效断言。将实际命令、退出码、代码版本、diff 范围和环境追加到 `validation.md`。

### Step 6：功能齐备后执行项目 Rust 检查

依据 `.github/workflows/ci.yml:81–83`，运行一次：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
git diff --check
```

预期全部退出 0。若有格式差异，用项目 formatter 修正后重跑相关检查；编译错误自行解决。只对后续代码或环境变更影响的检查重跑，不循环执行所有检查。

PTY 截图为可选补充证据，不是本需求额外强制项。若有环境限制，最多一次有针对性修复重试，之后由 Luna 审查决定补证或记录限制。数据库服务矩阵、安装器和发行检查未运行时应如实说明，不宣称通过。

### Step 7：Luna 收尾审查和交付

审查实际 diff，重点确认：

- 业务修改局限于 Consoles 渲染；公共 Omni、主题和提示布局未产生无关变化。
- footer 并非仅覆盖在原来的列表最后一行上，body 确实扣除 footer 空间。
- 普通 80 列窗口全显提示；搜索字符输入与重命名错误没有回归。
- 删除按钮和提示无重复命中区域，微小终端的输入区域没有越界。
- 验证记录对应最终业务代码；不拿早期结果冒充修改后的结果。

通过审查后按工作流授权完成提交合并；建议提交消息 `fix(ui): align console manager footer with omni`。仅暂存本任务业务文件和必要测试，不将 `.git/opencode-tasks` 报告放入提交，不将原工作区其他变更一并提交。

## 完成标准

- 上述一个业务单元完整通过定向验收与最终检查，或对确实环境受限项作出明确审查结论。
- 用户截图中的单条 Console 情况下，快捷键位于下边框上方，样式与 Omni 公共组件一致。
- 实际验证结果和限制记录在任务目录内。
- 本阶段只交付计划，后续由 Luna 执行；不要求用户手动实施或重复 resume。

## 本计划阶段的检查记录

`git status --short --branch` 退出 0，业务工作区干净。读取 checkpoint 返回不存在。读取既有渲染测试 imports，确认当前文件已有 `TestBackend`、`Terminal`、`Modifier`、`HitTarget` 等可复用工具。本阶段未运行测试、编译或 formatter，未修改业务文件。
