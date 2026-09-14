# Omni 颜色与边框渲染修复 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行环境若没有该技能，则按本文任务顺序实施，并逐项记录验证结果。

**Goal:** 修复 Omni 下半部分文字、图标和边框变黑的问题，使所有内容从首帧即可读，并消除弹窗动画反复重启和区域错位。

**Architecture:** 每帧只解析一次最上层弹窗身份，统一同步动画生命周期；结果动画在工作区层完成，弹窗效果在对应弹窗层完成。Omni 使用单一布局结果驱动绘制和命中测试，直接显示最终前景色；普通弹窗动画使用各自实际绘制区域，并在 resize 时取消旧过渡。

**Tech Stack:** Rust 2024 / MSRV 1.94，Ratatui 0.30.2 TestBackend，TachyonFX 0.25.1，现有 Theme、IconSet 和 Rust 测试设施。

---

## 1. 代码依据与范围

以符号定位为准，以下行号来自计划编写时的代码。

| 位置 | 已确认事实 |
| --- | --- |
| `src/ui/omni.rs:89–165` | 每个结果均设置 title/text、icon/accent、subtitle/muted；不存在前四项专用样式 |
| `src/ui/mod.rs:1041–1070` | 普通 overlay 与 Omni 分两次准备、处理同一个动画状态 |
| `src/ui/animation.rs:207–218` | clear_overlay 清空 key；prepare_overlay 发现 key 变化就重启动画 |
| `src/ui/animation.rs:170–178` | 前景色从 Black 淡入，时长 160ms |
| `src/ui/omni.rs:25–42` | Omni 使用最多 86×20、垂直偏上 `/3` 的布局 |
| `src/ui/mod.rs:1063,6727–6746` | 动画却使用 centered(area,88,22)，尺寸与偏移不同 |
| `src/ui/mod.rs:1229–1257` | 数字 overlay key 存在重复：18、17、16 均被复用 |
| `tests/ui_render.rs:7578–7600` | 现有 Omni 测试覆盖文字、光标、命中屏障及极小窗口，未验证多帧颜色 |

本计划接续 `docs/plans/2026-09-14-omni-usability-implementation.md`，专门解决最终 Buffer 的后处理与动画生命周期问题。

## 2. 固定设计决定

1. 最上层弹窗优先级：`app.omni` > `app.overlay` > 无弹窗。
2. Omni 不执行前景淡入，也不新增背景动画；文字、图标、边框首帧即使用最终主题色。
3. 普通弹窗保持现有淡入行为，但只在自身成为最上层弹窗时启动一次，范围必须是实际 popup。
4. Omni 打开时取消已有视觉过渡；底层查询仍执行，但结果完成信号应消费，关闭 Omni 后不补播过期动画。
5. 同类弹窗内部输入、列表刷新、滚动、选中项变化不视为新弹窗；关闭后重新打开视为新生命周期。
6. Omni 关闭后露出的普通弹窗成为新顶层，可以启动一次正常过渡。
7. resize 或进入极小终端模式时取消旧效果；重新绘制正确布局，不因 resize 重播。
8. 不增加依赖或用户配置项。沿用现有主题和 motion 配置。

## 3. 最终行为契约

| 场景 | 预期 |
| --- | --- |
| 仅 Omni | 所有行、图标、四边框与底部提示从首帧正常显示 |
| Help → Omni | Help 仍可作为底层绘制，Omni 不受 Help 效果污染 |
| 同一 Omni 连续绘制 | 不创建视觉过渡，不因动画持续请求重绘 |
| Omni 输入/滚动/结果更新 | 保持颜色与光标正确，不重启动画 |
| 查询在 Omni 下完成 | 信号被处理，不覆盖 Omni、不在关闭后补播 |
| 普通弹窗保持打开 | 160ms 后效果结束，后续帧保持最终色 |
| 不同普通弹窗切换 | 正确识别新身份，最多启动一次效果 |
| resize | 旧区域效果取消，绘制与命中使用新区域 |
| Full / Reduced / Off | Omni 最终配色一致；后两者保持现有动画降级语义 |

## 4. 实施任务

### Task 1：建立可控时钟和真实渲染回归用例

**Files:**
- Modify: `src/ui/mod.rs`：`render_with_state_using_icons_sequence_and_theme` 及内部测试模块。
- Test: `src/ui/mod.rs` 内新增 `omni_animation_tests`。
- Reference: `tests/ui_render.rs` 的 Omni 场景。

**Step 1 — 增加时间注入入口。**

现有公开渲染函数保留签名，只取一次 `Instant::now()`，转发到私有 `render_with_state_at(..., now: Instant)`；后者包含原渲染主体。将主体内动画观察与效果推进使用的 `Instant::now()` 全部替换为同一个 `now`。内部测试直接调用该入口，无需公开测试专用 API。

**Step 2 — 准备 Buffer 断言助手。**

使用 `Terminal<TestBackend>` 和持久化 `UiState::with_motion(Full)`。通过 `Action::OpenOmni` 打开真实面板；颜色分层测试按现有 Omni 模型构造至少 16 个具有明确 ASCII 标题、subtitle 的结果，避免依赖外部数据库。助手按命中区域或标题符号定位 cell，断言 fg/bg/modifier，而不是只把 Buffer 拼成字符串。

**Step 3 — 写两个失败测试。**

- `omni_animation_colors_are_readable_from_first_frame`：检查第 1、5、末项的标题和副标题，以及窗口底边框、底部提示。
- `omni_animation_does_not_restart_across_frames`：复用同一个 state，在 `t0`、`t0+80ms`、`t0+200ms`、`t0+400ms` 绘制，断言最终颜色正确且没有残留视觉效果。

多帧测试不得重新创建 UiState，不得使用 sleep。颜色断言使用 Theme 字段；不把所有 cell 都限制为非黑色，因为空格或其他主题允许黑色。

**Step 4 — 验证复现。**

Run: `cargo test --lib omni_animation -- --nocapture`

Expected: 在当前实现上颜色断言或效果状态断言失败，证明覆盖了真实主渲染链路。时间注入后首次 elapsed 为零是合理且必要的边界测试。

**Step 5 — 保存检查点。**

可作为本地红灯测试检查点；和后续修复组成同一个可通过测试的提交后再交付。

### Task 2：统一 Omni 布局计算

**Files:**
- Modify/Test: `src/ui/omni.rs`。
- Modify: `src/ui/mod.rs`：Omni 调用点。

**Step 1 — 添加布局数据类型。**

在 Omni 模块定义 `pub(super) struct OmniLayout`，包含 `popup`、`input`、`results`、`status: Option<Rect>`；实现纯函数 `calculate(area: Rect, has_status: bool) -> Option<OmniLayout>`。

计算规则：沿用当前窗口尺寸上限和 `/3` 垂直偏移；宽小于 10 或高小于 5 返回 None。先得到带一格边框的 inner；input 占第一行、第二行留空，results 占剩余可用行；有 status 时预留最后一行。所有减法使用 saturating_sub。

**Step 2 — 增加必要的边界测试。**

覆盖 `(80,24)`、`(100,40)`、`(160,60)`、`(12,7)`、`(9,4)` 以及非零原点 Rect。断言 popup 在终端内，内部区域在 popup 内，results 与 status 不重叠，极小尺寸正确返回 None。

Run: `cargo test --lib ui::omni::tests -- --nocapture`

**Step 3 — 接入实际渲染。**

先计算 visible/status，再计算布局；列表可见容量直接来自 `layout.results.height`。输入、列表、status、Omni 背景命中与逐项命中全部读取 layout；逐项 y 使用 `layout.results.y + offset`。返回实际 popup 给调用层，例如让 render 返回 `Option<Rect>`；极小终端提示返回 None。

**Step 4 — 验证既有交互。**

Run: `cargo test --test ui_render omni -- --nocapture`

Expected: 现有光标、命中屏障及极小窗口测试继续通过。

### Task 3：以类型化顶层身份管理效果生命周期

**Files:**
- Modify/Test: `src/ui/animation.rs`。
- Modify: `src/ui/mod.rs`：`overlay_key` 与主渲染流程。

**Step 1 — 替换数字 key。**

新增内部 `ModalKind` 枚举，Omni 和 Overlay 每个变体对应独立值；将 `overlay_key` 改为返回 `ModalKind` 的穷尽匹配函数。特别拆开 NotificationDetail/CatalogEditor、NotificationHistory/CatalogEditorDiscardConfirm、CatalogDropConfirm/CatalogEditorDestructiveConfirm。

不需要为每个普通弹窗实例增加 UUID：当前契约是同类弹窗内容变化不触发重播。保留 ResultIdentity 作为结果动画身份。

**Step 2 — 定义一个每帧一次的同步入口。**

用 `sync_modal(current: Option<ModalKind>) -> bool` 替代散落的 prepare/clear：返回是否发生顶层身份变化；记录 current；变化时取消旧效果；相同身份不重置计时。模式关闭不影响身份记录。入口本身不启动区域效果，待真实 popup 已知后再决定。

状态转换表：

| 旧值 → 新值 | 行为 |
| --- | --- |
| None → Omni | 记录身份、取消旧效果、无淡入 |
| Help → Omni | 取消 Help 效果、无淡入 |
| Omni → Omni | 无状态重置 |
| Omni → Help | 身份变化，允许 Help 绘制后启动一次 |
| Help → Help | 保持正在推进或已经完成的效果 |
| 任意弹窗 → None | 清理身份和旧效果 |

**Step 3 — 写状态回归测试。**

测试同身份连续同步不会取消一个已启动的普通弹窗效果；Omni 会取消旧效果；不同类型不会碰撞；关闭再打开返回 changed；Reduced/Off 不创建效果。

Run: `cargo test --lib ui::animation::tests -- --nocapture`

**Step 4 — 重排主渲染流程。**

在 TooSmall 分支之前解析 top_modal 并同步一次。正常渲染顺序固定为：

1. 绘制工作区。
2. 消费 result_ready；仅在没有弹窗时启动结果效果，并只在工作区层推进一次。
3. 存在普通 overlay 则 dim_background、绘制普通 overlay；仅当它是顶层时处理其效果。
4. 存在 Omni 则按现有屏障语义绘制背景、Omni 和输入光标，不调用前景效果。
5. 保留现有快捷键提示、通知层的行为，完成最终光标设置。

极小终端分支取消效果并消费已到达的结果信号，再绘制提示及可用的 Omni。返回正常尺寸后同身份不自动重播。

**Step 5 — 验证主问题修复。**

Run: `cargo test --lib omni_animation -- --nocapture`

Expected: Task 1 红灯测试全部通过，Omni 首帧和后续帧颜色一致。

### Task 4：普通弹窗效果使用实际区域并处理 resize

**Files:**
- Modify: `src/ui/mod.rs`：`render_overlay` 及其分支调用的局部弹窗 renderer。
- Modify: `src/ui/animation.rs`：效果区域验证。
- Modify: `src/ui/record_view.rs`、`src/ui/text_detail.rs`、`src/ui/execution_confirm.rs`。
- Modify: `src/ui/notifications.rs`、`src/ui/profiles.rs`、`src/ui/catalog_editor.rs` 中被 render_overlay 调用的顶层 renderer。
- Reference: `src/ui/dialog.rs`：render_frame 已接收真实 popup。

**Step 1 — 建立区域返回契约。**

让 `render_overlay` 返回 `Option<Rect>`，表示实际画出的最外层弹窗。每个分支的实际 popup 在 renderer 内计算一次，绘制完成后返回；早退但已经画出框架的分支也应返回 Some(popup)，完全没有画出弹窗的分支返回 None。嵌套子框不要替代最外层 popup。

修改前用代码索引检查上述 renderer 调用者，再逐个调整返回值。以 render_overlay 的穷尽 match 为迁移清单，禁止遗漏分支后回退到固定 80×20。renderer 返回区域即可，无需为了动画重写全部普通弹窗内部布局。

**Step 2 — 在真实位置启动普通弹窗动画。**

只有 `top_modal_changed && top_modal != Omni && actual_popup.is_some()` 才启动 Overlay 效果。动画推进只在该弹窗绘制之后执行一次。删除主渲染中的 `centered(area,80,20)` 和 `centered(area,88,22)` 动画区域猜测。

**Step 3 — 处理区域失效。**

每帧校验 effect_area 与对应当前真实区域一致且在 frame.area 内；区域改变或消失则取消旧效果。本帧直接显示最终样式，不将旧动画裁剪后继续播放，也不因 resize 重启。

**Step 4 — 添加区域与生命周期测试。**

在 animation 模块构造可控时间和不同 Rect：原区域内效果正常推进；改变区域后无活动效果；区域外 sentinel cell 的 fg/bg/symbol 不被改变。主渲染测试至少覆盖一个与原固定 80×20 不同的弹窗和 Help → Omni → Help。

Run: `cargo test --lib ui::animation::tests -- --nocapture`

Run: `cargo test --test ui_render -- --nocapture`

Expected: 弹窗绘制、光标和点击行为继续通过；普通弹窗动画在 200ms 后结束。

### Task 5：补齐真实场景矩阵

**Files:**
- Modify/Test: `src/ui/mod.rs`：内部多帧测试。
- Modify/Test: `tests/ui_render.rs`：Omni 集成测试。

**Step 1 — 验证叠层与输入。**

覆盖仅 Omni、Help 上打开 Omni、关闭 Omni 回到 Help、重复打开关闭。使用真实 Action 改变查询和选择，确认当前可见项前景正确，输入光标位于 Omni，面板外点击由 Omni 屏障拦截。

**Step 2 — 验证异步与滚动。**

使用既有 Omni page/reducer 测试设施构造一次合法结果更新，检查不会启动过渡。制造超过可见容量的结果，移动到末项后验证末项颜色、底边框和 status 区域；底层结果完成期间检查 ready 信号不会积压到关闭 Omni 后补播。

**Step 3 — 验证尺寸、主题和 motion。**

参数化覆盖：80×24、100×40、160×60、极小终端；Full/Reduced/Off；默认主题与一个自定义高辨识度主题。检查期望的语义颜色而非固定 RGB。至少一例开启后连续 resize，并返回初始尺寸。

**Step 4 — 检查停止重绘。**

静态 Omni、无加载任务时，推进时间后 `advance_animations` 返回 false；有真实加载任务时允许继续推进 spinner。普通弹窗完成后同样不再因残留 effect 请求重绘。

Run: `cargo test --lib omni_animation -- --nocapture`

Run: `cargo test --test ui_render omni -- --nocapture`

Run: `cargo test --test performance_regression -- --nocapture`

Expected: 全部通过；测试不依赖人工等待、外部数据库或截图像素比较。

### Task 6：完整检查、终端验收和交付

**Files:**
- Update: 本计划中的执行结果记录。

**Step 1 — 完成仓库检查。**

分别执行：

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
git diff --check
```

执行前核对仓库当前 CI 的 feature/环境要求；若有额外强制检查按 CI 补齐。区分环境阻塞、已有失败和本次引入的失败，记录具体命令与错误，不将未运行标为通过。

**Step 2 — 真实终端验收。**

分别运行：

```bash
cargo run -- --motion full
cargo run -- --motion reduced
cargo run -- --motion off
```

使用 F2 打开 Omni，在有足够表结果的连接中确认 16 项左右的列表；检查首次打开、连续输入、上下滚动、等待超过 1 秒、Help 上打开、窗口变高变矮、关闭重开。保存 full 模式截图，确认首项、第五项、末项、底边框及底部提示全部可见。

**Step 3 — 审查改动。**

检查所有效果调用点：顶层身份每帧仅同步一次；同一效果每帧至多推进一次；无固定 popup 猜测；无数字 key；Omni 无前景淡入。检查结果信号消费与 resize 路径。

**Step 4 — 建议提交分组。**

- `fix(ui): keep omni readable and unify modal effect ownership`：Task 1–3，保持测试通过。
- `fix(ui): bind overlay effects to actual popup bounds`：Task 4。
- `test(ui): cover omni animation and resize regressions`：Task 5 与验收记录。

提交前只 stage 对应文件；保留其他工作。由实际执行任务的提交要求决定是否创建提交。

## 5. 验收清单

- [ ] 默认 Full 模式下 Omni 首帧即可读，颜色不依赖等待动画结束。
- [ ] 所有可见列表项和四边框均正确，无水平失色分界线。
- [ ] Omni 与普通弹窗共存时动画身份不来回切换。
- [ ] 普通弹窗动画能完成且不影响真实 popup 之外的 cell。
- [ ] resize 与极小终端切换会取消旧区域效果。
- [ ] 静态 Omni 不产生持续动画重绘。
- [ ] 光标、点击屏障、选中项滚动、status 布局和异步结果更新保持正确。
- [ ] 颜色测试覆盖最终 Buffer、多帧、motion 模式与自定义主题。
- [ ] 检查命令与真实终端验收结果已记录。

## 6. 执行结果记录

计划编写完成，尚未实施代码修改或运行修复验证。执行时逐任务记录：修改范围、验证命令、结果、必要的偏离原因，以及终端截图位置。
