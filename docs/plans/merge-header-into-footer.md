# 合并全局底部状态栏 Implementation Plan

> **执行交接：** 由 Luna 按本计划继续自动实施、审查、纠偏、提交与合并。用户已选择自动工作流，无需询问执行方式，不启动子 Agent。本文件为本任务携带的实施计划。

**Goal:** 删除顶部全局信息行，将应用、版本、连接、数据库信息接到底部现有模式与快捷键之前，归还一行主体空间。

**Architecture:** `AppLayout` 仅划分主体与单行 footer。统一底栏渲染复用原 header 数据、原 footer 模式/快捷键和主题，依据实际可见文字宽度生成鼠标命中区域，按预算压缩长上下文。

**Tech Stack:** Rust 2024 / Rust 1.94.0、ratatui 0.30.2、unicode-width、TestBackend。

---

## 1. 基线、范围与执行约束

- 原工作空间：`/Users/yelog/workspace/tui/lazydb`；目标分支 `main`；起点 `45265617278ab30d3af24070c081e5a7e8cca0a4`。
- plan 阶段重新检查：HEAD 仍为上述起点，`git status --short`、工作树 diff/stat、暂存区 diff/stat 均为空。当前业务源码与已分析版本一致。
- 不依赖任何未提交文件。未提交文件不会自动复制到新 worktree；实施开始时如发现新的本地依赖，先识别并更新范围清单，不清空、stash 或提交整个原工作区。
- 指定任务目录当前没有 `checkpoint.json`；不创建该文件，不修改 `state.json`。计划只写入本目录，不额外生成 `docs/plans` 文件。
- 后续任务分支由工作流自动命名；实施在任务 worktree 内进行。建立或选择 worktree 后先核对 `git rev-parse HEAD` 和 `git status --short`。
- 本需求为**一个端到端验收单元 U1：回收顶部行并完成底部信息、状态与交互迁移**。下列步骤是 U1 内部检查点，不是等待用户 resume 的边界。

### 预计修改文件（精确清单）

| 文件 | 修改内容 |
| --- | --- |
| `src/ui/layout.rs` | 去掉全局 header 区域；同步所有初始化与布局测试 |
| `src/ui/mod.rs` | 合并全局 header/footer 内容、宽度预算与点击区域 |
| `tests/ui_render.rs` | 调整顶部断言，覆盖合并顺序、状态、窄屏及不同工作区 |
| `tests/mouse.rs` | 校验迁移后可见文本的真实点击位置和动作 |

无预计新增、删除、重命名业务文件。`src/ui/shortcut_hints.rs`、`src/input/mouse.rs`、`.github/workflows/ci.yml`、`Cargo.toml` 仅作参考，不列入修改范围。若实现发现范围外确需修改，先记录理由和具体路径，再更新 `change-scope.json`；不顺带调整数据库逻辑、版本号、主题或发布文档。

## 2. 验收与门禁分类

### A. 用户需求（必须完成）

- 正常宽度底行从左到右为：应用名、动态版本、当前连接、`/`、当前数据库、原底行模式、原上下文快捷键。
- 顶部不再保留独立全局信息行或空白占位；主体从 `area.y` 开始，比旧布局增加一行。
- 底栏始终只占一行，短连接后立即跟随原底行内容，不人为预留固定半屏空白。

沿用现有 `LAZYDB` 徽标大小写与主题；示例 `v0.1.6`、`lssc-uat-redis`、`0` 不硬编码。显示版本使用 `env!("CARGO_PKG_VERSION")`。

### B. 实现必须保持的回归契约

- 原模式、自定义键绑定、上下文快捷键、终端选择模式仍有效。
- 保留连接状态 `TARGET` / `LINKING` / `FAILED`、更新状态 `UPDATE` / `RESTART`，不新增正常连接冗余状态。
- 版本/更新徽标、连接名、数据库名原点击动作随实际可见文字迁到底行；裁剪后不得出现幽灵目标或互相覆盖。
- 保留终端控制字符清洗与显示列宽处理。支持窄屏、非零布局原点、最大化，以及 SQL/Redis/Relation/Dashboard/Principal/空工作区。
- TooSmall 阈值仍是宽 <56 或高 <16。

### C. 项目已有 Rust 门禁（功能齐备后执行一次）

来自 `.github/workflows/ci.yml`：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

记录实际通过、失败和跳过项；不把缺少数据库环境的跳过解释成数据库矩阵通过。不额外重复 `cargo check`。CI 中安装器、跨平台发布和数据库服务矩阵继续由既有流水线负责，本任务不新增本地全量部署门禁。

### D. 补充建议（非强制门禁）

- 真实 PTY 下观察一张 Redis 和一张 SQL 画面、真实鼠标点击，作为视觉证据。
- 用户没有要求人工批准、截图或真实 Redis 服务启动成功才可验收。核心证据使用 TestBackend 的可重复渲染/点击测试。
- 人工/PTY 等环境受限检查至多进行一次有针对性的修复重试，仍不可用则记录限制，由 Luna 收尾审查判断证据是否充分；不无限维持 progress。

## 3. U1 逐项实施步骤

### 步骤 1：建立可观察的渲染回归断言

**修改：** `tests/ui_render.rs`。

1. 将 `workspace_header_and_footer_render_without_redundant_status_rows` 改为单底栏行为测试；使用现有 fixture 和 render helper。
2. 在 120×36 或足够宽的 180×50 画面中，检查底行应用、动态版本、profile、database、模式的出现顺序；用现有上下文中至少一项实际快捷键检查其位于模式之后。以原 `fixture` 的值生成期望，不固定示例连接名。
3. 保留“不出现 ONLINE、QUERY IDLE 等冗余状态”的断言。顶部使用有内容的常规 fixture 检查没有全局 `LAZYDB` 徽标；不要对空工作区中心 ASCII 标识做全屏禁止断言。
4. 将 `one_row_header_retains_only_transitional_and_failed_connection_status` 和 `one_row_header_keeps_failure_status_after_long_context` 迁到底行；长名称的 56 列场景必须仍看见 FAILED 和模式。

**定向命令：** `cargo +1.94.0 test --test ui_render workspace_header_and_footer_render_without_redundant_status_rows`（如重命名则用新完整测试名）。

**预期：** 实施前因信息仍位于首行而断言失败；记录真实原因，而非接受环境错误充当红测。后续实现完成后该断言通过。

### 步骤 2：回收全局 header 布局行

**修改：** `src/ui/layout.rs`、`src/ui/mod.rs`。

1. 删除 `HEADER_HEIGHT` 与 `AppLayout.header`，同步 TooSmall/Focus/Standard/Wide 各构造分支。
2. 在 `AppLayout::calculate` 用以下布局替换三段切分，其余业务面板仍从 body 分割：

```rust
let vertical = Layout::default()
    .direction(Direction::Vertical)
    .constraints([Constraint::Min(8), Constraint::Length(FOOTER_HEIGHT)])
    .split(area);
let body = vertical[0];
let footer = vertical[1];
```

3. 删除 `render_with_state_at` 的顶部 `render_header` 调用。不是把旧 header 区域设为零高或指向 footer。
4. 将布局测试改为 `body.y == area.y`、`body.height + footer.height == area.height`、`footer.height == 1`、`footer.y == area.bottom() - 1`，用表驱动覆盖常规/窄屏/最大化和非零原点。
5. 根据新主体高度更新真实受影响的尺寸断言，例如 120×30 的 editor clamp 高度由原先 17 重新计算；不能统一把所有 y 减 1，因为相对百分比布局也可能变。

**复核：** 仅移除全局 AppLayout 的 header；不要误改 SQL History、profile 弹窗或表格列标题等同名 header。

### 步骤 3：统一底栏内容与宽度预算

**修改：** `src/ui/mod.rs`。

1. 把 `render_header` 中 profile/database、动态版本、连接过渡状态和更新徽标的数据获取与样式迁入 footer 流程；移除废弃全局 header renderer，保留清洗 helper。
2. `render_footer` 改接收 `&mut UiState`，以支持底栏命中区域。现有各业务分支都调用统一 footer；不单独为 Redis 添加一份逻辑。
3. 保留原模式判定（包括 Redis Preview 的只读编辑模式）、`shortcut_context`、`shortcut_capabilities`、`footer_shortcuts_with_bindings`。原 `_sequence` 行为不在本次改造。
4. 按下面的确定性预算规则组织本文件内的局部辅助函数，无需通用状态栏框架：
   - 先计算 footer 显示宽度、应用/版本、模式以及连接状态所需宽度；用饱和运算避免溢出。
   - 更新徽标放右侧连接状态之前。对超长版本文本按剩余预算缩略，保障短 `UPDATE`/`RESTART` 标记；更极端空间冲突优先保留模式、短连接状态和版本入口。
   - 正常模式为 hints 尝试预留至多 24 列；先保障可用空间下连接和数据库至少各有一个可见列及分隔符，再取预留。该值是内部预算策略，不是用户要求，也不是固定空白。
   - context 的剩余预算先给 profile/database 各半，短字段未用预算转给长字段；用 `truncate_to_cell_width` 生成可见字符串。字段都能完整显示时不缩略。
   - 完成可见身份串后，模式紧随其后；将实际剩余宽度交给 `shortcut_hints::line`，继续使用完整项打包和 `... (+N)`，无换行。
   - 提示预算仅影响缩略，不绘制填充到预算宽度的空白；正文和模式实际连续排放。
5. 终端选择模式取消原来覆写整条底栏的提前返回；正常宽度显示身份前缀及原选择提示。窄屏使用紧凑 `TERMINAL SELECTION` 与 `Esc` 提示，减少上下文预算。
6. 同行背景仍使用 theme.surface，各徽标保持现有主题颜色。宽度为零的子区域不绘制。

**复核：** 足够宽时所有身份信息完整，短名字后无固定空洞；56 列长名字时 FAILED、模式仍可见；没有为挤入内容增加第二行。

### 步骤 4：迁移点击区域并验证真实坐标

**修改：** `src/ui/mod.rs`、`tests/mouse.rs`、`tests/ui_render.rs`。

1. 以 `footer.x` 为起点，按照实际输出 spans 的显示宽度逐段累加坐标，同时记录版本、profile、database 的区域；更新徽标使用受限后的实际区域。
2. 不复用旧 `profile_x = area.x + 10 + version_width + 2` 常量。旧前缀实际是宽度 8 的 `" LAZYDB "`，现有硬编码有偏移风险。
3. 继续使用 `HeaderProfile`、`HeaderDatabase`、`UpdateCenter`，无需重命名枚举或修改 reducer。数据库只有已连接且有可见文本时登记；所有目标高度 1、宽度非零、范围在 footer 内。
4. 扩展 `header_profile_hit_region_uses_terminal_display_width`：保留中文、半角组合字符、控制字符清洗的宽度断言，并检查目标在底行且对齐真正显示的 profile。
5. 通过已渲染 buffer 的文本列位置构造鼠标事件，传入 `map_mouse`，验证版本/更新中心、profile、database 的原动作。不要只用被测 HitRegion 自己的坐标生成点击，否则无法发现文字与目标一起错位的问题。
6. 更新 `connected_header_database_summary_opens_database_selector` 的底行和可见区域断言；加一组长名称裁剪后目标不覆盖模式、提示或相邻字段的检查。

**复核：** `state.target_at` 逆序查找仍能得到正确目标；不存在顶部残留全局命中区域，旧业务面板目标不被新底栏区域覆盖。

### 步骤 5：补齐状态和布局回归矩阵

**修改：** `tests/ui_render.rs`；必要时同文件复用或扩展已有 fixture。

- 按现有 fixture 选择 SQL、Redis、Relation、Dashboard、Principal、空工作区，检查底行身份与对应模式，并保证主体未侵占 footer。
- 重点边界：56×16、80×24、120×36、180×50；使用表驱动覆盖，不做全部状态的笛卡尔积。
- 检查 LINKING、FAILED、TARGET、UPDATE、RESTART 的展示及重要目标；使用已有 App 事件/状态构造方式，不为测试暴露额外生产接口。
- 检查 TERMINAL SELECTION 的身份前缀与 Esc 提示；保留自定义键绑定回归覆盖。
- 复用已有主题/图标测试；避免对所有颜色逐格写重复快照。

**定向验证：**

```sh
cargo +1.94.0 test --lib ui::layout::tests
cargo +1.94.0 test --test ui_render --test mouse
```

**预期：** 命令退出 0；新增/调整测试被实际执行。若只修复一个失败用例，先用测试名定向重跑；之后再运行尚未通过的相关集合。不得删除有效断言使检查变绿。

### 步骤 6：完成门禁、收尾审查和工作流交接

**业务修改范围：** 仅修复本单元发现的问题；报告追加到本任务目录 `validation.md`。

1. 功能齐备后执行第 2 节 C 的 fmt/clippy/全量 test；同一版本通过后不无原因重复。
2. Luna 复核 `git diff --check`、实际 diff 和 `git diff --name-only`，对照 `change-scope.json`，确认没有无关文件和工作区本地依赖遗漏。
3. 检查所有 AppLayout 初始化、footer 调用分支、坐标预算和保留动作。各测试变化应能追溯到顶部行被回收或合并后宽度变化。
4. 如补充 PTY 检查，遵循非强制与单次针对性重试限制。
5. 在 `validation.md` 逐项记录：命令、工作目录、代码 commit、相关未提交 diff、环境/工具链、退出码、通过/失败/跳过及限制。计划中的预期不能替代实际输出。
6. U1 全部契约与项目 Rust 检查通过后，由后续 Luna 提交/合并阶段仅暂存任务文件；建议提交说明 `feat(ui): merge header into bottom status bar`。不得在 plan 阶段或原工作区执行提交。

## 4. 完成判定与下一步

- U1 完成：一行主体被回收；底部内容顺序正确；动态模式、状态和点击动作全部迁移；定向测试及项目 Rust 门禁结果有证据；补充检查如受限已说明。
- 当前 plan 阶段完成：本计划和精确 `change-scope.json` 已落盘，实际检查记录已追加，最后写本轮指定 completed 回执。
- 下一阶段第一个具体动作：Luna 在任务 worktree 核对起点和 diff，执行 U1 步骤 1 的底行顺序断言，再连续完成整个单元；不要求用户重复确认或 resume。
