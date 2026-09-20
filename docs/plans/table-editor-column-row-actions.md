# Table Editor Row Actions and Change Summary Implementation Plan

> **执行负责人：Luna。** 按本文端到端验收单元连续实施；实现、审查、纠偏、提交及合并由 Luna 完成。Astra 仅负责分析和计划。不启动子 Agent，不要求用户反复 resume。

**Goal:** 在表结构编辑器中提供可单击的行尾删除/恢复图标，将底部操作按 Add Column → Review SQL → Cancel 排列，并对非零变更统计进行语义着色。

**Architecture:** 复用 ColumnDraft 的稳定 row_id，通过带明确删除/恢复意图的 Action 原子操作目标行。Table Editor 局部统一测量数据列、操作列、摘要和按钮的矩形，绘制与鼠标命中共用几何；保留原有草稿状态和 SQL 预览/执行流程。

**Tech Stack:** Rust 2024 / Rust 1.94、Ratatui 0.30.2、Crossterm 0.29、unicode-width、现有 App reducer / HitTarget / IconSet / Theme / Rust 集成测试。

---

## 0. 基线与执行约定

- 原工作空间：`/Users/yelog/workspace/tui/lazydb`，目标 `main`。
- 起点与 plan 阶段实际 HEAD：`f7d03d54ac463cbe1c322e738f9a6fe7ee2d773c`。
- plan 开始时 `git status --short`、`git diff --cached --stat` 均为空。没有未提交依赖；未提交文件不会自动复制到后续 worktree，如实施开始时状态变化，重新核对实际 diff。
- 已读取本轮 `analysis.md`；目前没有 `checkpoint.json`。后续恢复若存在 checkpoint，以其和真实 diff 确认完成单元，不采用历史轮次的 next。
- 分支、任务名及 worktree 由后续自动流程/Luna 确定，本计划不自行创建。
- plan 阶段仅写任务目录，不新增 docs/plans 副本，不改变业务文件、index、state.json 或历史回执。
- 持续按 Task 1 → Task 2 → Task 3 实施。每个单元完成后定向验证并记录，再进入下一项；普通实现/编译问题自行解决，不作为外部阻塞。
- `validation.md` 追加当前命令、退出码、实际测试数量、对应代码版本/工作区 diff、环境。以下所有测试均为未来执行要求，不是当前通过记录。

## 1. 产品合同和取舍

### 行尾操作

- 从 Columns 标题及底部移除独立 Remove/Restore 按钮；删除其不可见焦点入口。
- 每个可见数据行右端固定一列图标；正常可删除行红色删除，Removed 行保持灰色内容并显示绿色恢复。
- 点击图标直接操作该图标所在行，不要求预先选中；普通行点击仍只选中。
- 最后一有效列不允许删除，显示 muted 禁用图标且无删除命中区域；已删除行可恢复。
- Existing 删除后保留灰行；Added 删除直接撤销新增并从草稿移出，保留既有模型合同，不扩展 Added 墓碑状态。
- 恢复 Existing 时保留它删除前的字段修改，后续摘要可能重新显示 modified。
- 图标只改草稿，不返回数据库写命令，不新增删除确认框。

### 按钮和键盘

- 视觉及动作区焦点顺序：Add Column → Review SQL → Cancel；反向顺序相反。
- 保留 Columns 当前逐行 Tab/j/k/方向键合同和首尾 clamp，不附带引入全局焦点循环。
- 保留 dd、r、a/A、e、Enter 预览、Esc 取消。
- 不给每个行尾图标再增加 Tab 停靠点，键盘仍通过选中行和 dd/r 操作。

### 摘要

- `N added`（N>0）使用 `theme.success`，`N modified` 使用 `theme.action`，`N removed` 使用 `theme.error`；整段数字及描述一起着色。
- 零值和分隔符 muted；非零 pending 总数 `theme.text` + Bold；无变更 `No changes` muted。
- pending = added + modified + removed + properties_changed(0/1) + column_order_changed(0/1)，复用现有 TableChangeSummary。
- 摘要独立于动作行，避免三按钮挤掉非零统计。标准宽度显示零值分类，窄时先省略零值，再用短描述/预留第二行按完整片段布局。
- plain 模式依靠文字、符号和非零加粗；不能硬编码 RGB 或 ANSI。

### 布局决策

- 优先在 `src/ui/catalog_editor.rs` 内实现 Table 专用测量 helper，不改通用 `dialog_footer::measure` 的默认行为。
- 行尾槽位标准固定 6 cells（可显示 ACTION），窄宽度固定至少 max(delete_width, restore_width)+2，通常 3 cells；另保留 1 cell 分隔符。档位由尺寸/图标模式决定，与行状态无关。
- 标准摘要一行、动作一行、帮助一行；需要窄模式时按该档位最坏情况固定预留第二摘要行/多行动作，不根据实际计数或焦点增减主体高度。
- 三按钮先用全标签，放不下时统一短标签 Add / SQL / Cancel，再按完整按钮折行。测量与 hit regions 共用结果。
- 零宽/零高返回空布局；极小窗口沿现有顶层 too-small 策略；组件层不能返回越界区域。

## 2. Task 1：行尾删除/恢复及移除旧焦点的完整闭环

**修改文件**

- `src/model/catalog_editor.rs`
- `src/action.rs`
- `src/app.rs`
- `src/ui/mod.rs`
- `src/input/mouse.rs`
- `src/input/keymap.rs`
- `src/ui/icons.rs`
- `src/ui/catalog_editor.rs`
- `tests/catalog_editor_state.rs`
- `tests/catalog_editor_reducer.rs`
- `tests/mouse.rs`
- `tests/keymap.rs`
- `tests/ui_render.rs`

### Step 1.1：建立目标身份和状态回归

沿用已有 table fixture，至少构造三个 Existing 列，保存各自 row_id。不要用全 Added fixture 替代灰行恢复测试。

新增测试行为：

1. 当前选 A，删除 B：只有 B Existing→Removed，A 保持原样，选中 B，焦点 Columns。
2. 恢复 B：B Removed→Existing，原字段值与 row_id 不变；原有 comment 修改保留。
3. 捕获 B 的 row_id 后重排列，再请求删除 B：仍按身份删除 B。
4. 非存在 row_id、重复 Remove、重复 Restore：状态/选择不变，无 panic。
5. 仅一有效列时删除失败；存在 Removed 列仍可恢复。
6. Added 删除后撤销新增，失效 row_id 后续请求 no-op，不误删相邻行。

现有参照：`table_remove_protects_last_effective_column_and_restores_existing_columns`、`table_remove_drops_unconfirmed_added_column_without_resetting_other_rows`。

先运行新增测试，记录实际失败原因；新 API 不存在导致编译失败属于可预期 red，不冒充已有功能缺陷复现。

### Step 1.2：实现共享可用性和 row-specific 模型方法

建议接口（实现中命名可顺应上下文，但契约保持）：

```rust
pub fn remove_column_row(&mut self, row_id: Uuid) -> bool;
pub fn restore_column_row(&mut self, row_id: Uuid) -> bool;
```

算法顺序：

1. 先排除 column_editor 已打开。
2. 通过 `position(|column| column.row_id == row_id)` 查找；找不到返回 false。
3. Remove 要求不是 Removed，且有效列数>1；Restore 要求确实 Removed。
4. 全部验证通过后才 finish_edit_group、设置 selected_column 和调用现有模型变更方法，返回 true。
5. 失败路径不改变选中行或焦点，不能用索引 clamp 兜底。

把有效列计数与“某一列是否可删”规则抽为共享函数/方法。渲染每帧只计算一次有效列数，再逐可见行 O(1) 判断；不要循环内调用一次 O(n) 的计数方法。

现有无参数 remove/restore 方法继续服务 dd/r，必要时复用同一内部状态判断，不引入第二套删除实现。

### Step 1.3：补齐 Action、HitTarget、reducer 原子路由

新增两个明确意图，避免 toggle：

```rust
CatalogEditorRemoveTableColumnRow(Uuid),
CatalogEditorRestoreTableColumnRow(Uuid),
```

同时定义到 Action 与 HitTarget。reducer 收到新 Action 时检查：

- `overlay == Some(Overlay::CatalogEditor)`；
- editor.page 为 Form，且 `!editor.is_busy()`；
- draft 为 Table 且无 column_editor；
- 再交给上述 row-specific 模型方法。

返回空 Commands，仍由已有 Review/Apply 生成 SQL。不能在 mouse mapper 内修改 App，也不能通过两次事件拼接“选中+删除”。

在 reducer 测试覆盖 Loading/Planning/Applying、SqlPreview、Column Details、其他 overlay 的拒绝路径，以及 commands 为空。

**注意重复与双击：** 同一明确 Remove Action 重放必须幂等；重新渲染后的恢复按钮代表新的 Restore 意图。本任务不承诺屏蔽所有物理双击，也不为此新增定时状态。

### Step 1.4：更新鼠标白名单和命中映射

`src/input/mouse.rs` 同步修改三个位置：overlay 允许目标列表、target→Action、focus_for_target exhaustive match。保持 Column Details 的父层屏蔽；新两个目标不能加入子弹窗允许名单。

旧 Restore 目标在 overlay 白名单中遗漏。新增行目标必须显式允许 Remove 和 Restore 两种；如果旧无参 HitTarget 已无使用，可删除两者及对应鼠标映射。不要删除仍被键盘使用的无参 Action。

完成测试必须走 `ui.target_at → map_mouse → app.update`，而不是只验证渲染注册了一个目标。

### Step 1.5：渲染真实尾列并传递 IconSet

1. `icons.rs` 添加 Table 删除/恢复语义方法，采用实际依赖已存在的 Nerd Font 常量；Unicode `×`/`↶`，ASCII `x`/`r`。
2. 沿 `catalog_editor::render → form → render_table` 传递当前 IconSet，不创建默认图标集。
3. 修改 Table 的 Cell、header、Constraint 同步增加分隔符与 action cell。原 7 cells 固定预算增加尾部预留宽度；数据列在余量里布局。
4. 所有动作 x 从同一布局结果计算。若 Ratatui 约束不足时会压缩列，应采用已确定的实际宽度，或将尾部固定槽单独分割绘制，不能依赖“理想列宽”计算点击位置。
5. 普通行选择矩形排除操作槽与最后分隔符；操作只在可见且 enabled 时注册；若区域必须重叠，动作后注册，匹配 target_at 的逆序优先规则。
6. 灰行的恢复 cell 显式移除 DIM；删除 cell 为 error、恢复 success、禁用 muted。行背景继续保留 selected/added/removed 语义。
7. 移除 Columns 标题旁 Remove/Restore。此单元可暂保留 Add 在标题旁，Task 2 再整体搬入底部。

### Step 1.6：删除不可见 Remove 焦点

- 删除 `TableActionField::RemoveColumn`，更新 `focus_next/focus_previous` 的 Add→Review、Review→Add。
- 更新 keymap 对 Action(RemoveColumn) 的 Esc/Enter/Space match；保留独立 dd/r Action。
- 更新引用旧 Remove 焦点的 state/reducer/keymap 测试，保留所有其余字段、逐行 Tab 和 clamp 断言。
- 删除旧按钮相关 UI 测试断言，替换为尾列图标/目标身份/恢复交互断言，不能仅删除测试。

### Step 1.7：端到端复核与定向验证

```sh
cargo +1.94.0 test --test catalog_editor_state --test catalog_editor_reducer
cargo +1.94.0 test --test mouse catalog
cargo +1.94.0 test --test mouse table
cargo +1.94.0 test --test ui_render table
cargo +1.94.0 test --test keymap table
```

预期：实际匹配测试数非零，全部通过。新增测试名包含 table/catalog 以进入上述过滤。

**验收：** 未选中行单击删除→灰行→单击恢复闭环完成；排序/滚动后仍操作正确身份；最后有效列保护与 Added 撤销不变；无隐形 Remove 焦点；没有新增数据库执行动作。

## 3. Task 2：三按钮底栏和语义彩色摘要闭环

**修改文件**

- `src/ui/catalog_editor.rs`
- `src/help.rs`（仅校准该上下文的动作/说明，不重构全部帮助）
- `tests/ui_render.rs`
- `tests/mouse.rs`
- `tests/keymap.rs`
- `tests/catalog_editor_state.rs`、`tests/catalog_editor_reducer.rs`（若需要补充该链路断言）

### Step 2.1：先建立布局/样式回归

新增或扩展测试：

- 120×40、100×30、80×24、56×16 下动作依次 Add、Review、Cancel，标准宽度同一行，窄模式按行优先顺序一致。
- Columns 标题旁没有按钮；Add 的目标在底部。
- 固定尺寸切换 General/Columns/各 Action，按钮 y、footer 高度与表格容量不变化。
- 构造非零 modified/removed 和零 added 的 summary；同时另一个 fixture 覆盖非零 added。断言完整数字及描述单元格颜色，而非仅 contains 文案。
- 去除所有变更→No changes；修改 Existing→删除→恢复，modified 统计恢复。
- 鼠标点击三个新位置分别得到 Add、Preview、Cancel；间隙/分隔线不触发。

### Step 2.2：局部表编辑器布局测量

在 catalog_editor.rs 内定义小型 Table 专用布局结果（可包含 body/list、summary、actions、help 及按钮 rects）。使用单元格宽度，不使用字节长度；统一 clamp 到父 Rect。

步骤：

1. 按内区尺寸和图标模式确定 standard/compact 档位。
2. 计算三按钮全标签宽度及间隔；不够用短标签；仍不足按完整按钮换行，提前确定 rows。
3. 给摘要独立空间；固定档位预留一或两行，不因实际计数或当前焦点扩张。
4. 去掉多余分隔线优先于占用正文；重算 Columns list_capacity 和 selected row 可见范围。
5. compact Columns 起点改成固定偏移，General 的聚焦字段在同一预留行显示/隐藏，避免列区随焦点移动。
6. 帮助保持单独一行，动作不足时不得覆盖；极小布局采用既有终端过小退化，所有可见命中在框内。

本任务不修改 `src/ui/dialog_footer.rs` 通用默认布局，避免影响其他窗口。已有 Table 对该 helper 的调用可由局部结果取代；不为此清理其他无关 dead-code。

### Step 2.3：搬移 Add 并输出固定顺序

从 Columns 标题移除 Add；底部配置按语义字段顺序定义 AddColumn、Review、Cancel，尺寸测量、渲染、鼠标目标都使用同一配置。

Review 的主操作强调使用 Theme 的强调语义，焦点仍有独立反显/标记；不将三个按钮都渲染成主操作。焦点链已在 Task 1 去掉 Remove，本单元复核视觉顺序与链路一致。

### Step 2.4：摘要改为独立 Span

将单一 format!/muted Paragraph 改为 `Line`/`Span` 片段。建议局部纯 helper 接收 TableChangeSummary、Theme 和宽度档位，输出 Line/Vec<Line>。

实现规则：

1. 从既有 summary 计算 pending，不从渲染的可见行统计。
2. pending>0 时总数片段 text+Bold；分类片段按 success/action/error 着色且可加粗。
3. 分类值为0时 muted，分隔符 muted；保留数字和词语同一个 Span。
4. 未修改时只输出 No changes。
5. 宽屏完整显示。窄屏先删除零值片段，使用稳定预留行数，按整个片段换行；不得“1 modif…”或数字与标签分属不同颜色。
6. 行背景由父区域决定；plain Color::Reset 仍保留非零 BOLD。

### Step 2.5：帮助合同核对

`src/help.rs` 的 Table 上下文说明更新为行选中+dd/r，移除若有的独立 Remove 按钮导航说明；不得声称 icon 需要先选择行。UI 提示按实际 keymap 显示，避免为本轮重新定义完整帮助/快捷键架构。

### Step 2.6：定向验证与审查

```sh
cargo +1.94.0 test --test ui_render table
cargo +1.94.0 test --test mouse table
cargo +1.94.0 test --test keymap table
cargo +1.94.0 test --lib help::tests::
```

若修改了模型/reducer，额外重跑对应两项测试；否则复用 Task 1 的有效证据，不无故重复。

**验收：** 用户三项需求均完成；摘要不被按钮压掉；计数与颜色正确；新增/预览/取消的键盘与鼠标目标一致；所有尺寸下可见控件无越界。

## 4. Task 3：文档同步与最终验证

**修改文件**

- `docs/keybindings.md`
- `docs/ui-dialog-guidelines.md`
- `docs/architecture.md`

### Step 3.1：更新描述

- 改掉“Add/Remove 都位于 Columns 标题旁”的旧说明。
- 记录行尾图标精确操作点击行、灰行可恢复、最后有效列禁用、Added 删除撤销新增。
- 记录底部顺序 Add→Review→Cancel、dd/r 不变。
- 摘要使用 Theme semantic spans，行 hit region 通过 row_id 分发，坐标由布局统一测量。
- 不把此变更写成全软件 footer 重构，不改历史计划或上一轮执行记录。

### Step 3.2：验证分级

**A. 用户必需结果：** 行尾删除恢复、按钮顺序、非零统计颜色三项，按前述端到端测试证明。

**B. 实现必要自动化回归：** 当前计划选择的 row 身份、禁用、灰行恢复、mouse whitelist、focus、颜色、窄屏及 Unicode/plain 测试，用于验证新增行为；不是要求访问用户真实数据库。

**C. 项目强制门禁：** `.github/workflows/ci.yml:81–83`：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
git diff --check
```

功能齐备后执行一次。失败时修复并仅重跑受影响检查；普通工具超时按适当 timeout/background 继续，不冒称通过。仅有旧版本的通过结果不得用于当前代码。

**D. 补充建议而非新增门禁：** 真实终端/PTY 观察 Nerd Font 图标与一次点击体验，可生成对比截图供 Luna 审查；无用户要求时不强制做真实数据库 DDL。受环境限制最多一次针对性修复重试，然后记录限制，由 Luna 收尾审查决定是否需要补证据。

### Step 3.3：最终差异审查

检查 change-scope 中实际文件，确认：

- row_id 定位，非法目标 no-op，UI 与模型可用性一致；
- 最后列保护、Added 删除与恢复后修改保留；
- 图标绘制宽度/边界和命中一致；
- Remove 焦点确实移除，键盘删除仍可用；
- pending 公式未被更改，无 DB adapter/SQL 规划器意外变更；
- 工作区用户既有内容未被覆盖，索引只暂存本任务相关 diff；
- validation 记录真实命令与当前版本，不复制上一轮通过数字。

### Step 3.4：提交与合并交接

提交、合并由 Luna 按自动工作流当前授权阶段完成。建议两个业务提交（也可保持一个完整功能提交）：

1. `feat(ui): add row-scoped table column remove and restore controls`
2. `feat(ui): reorder table actions and color change summaries`

每个提交要编译且对应验收闭环；不把任意用户未提交文件混入提交。合并若目标 main 已前进，检查新增差异与冲突，按影响运行集成验证；不将本计划阶段的只读检查当作合并验证。

## 5. 文件范围与排除项

`change-scope.json` 列出预计变更的 17 个具体仓库路径。没有未提交依赖、无预计新增/删除/重命名业务文件。

不修改 DB adapters、SQL 规划器、Redis 编辑器、通用 footer 默认行为、Column Details 状态模型。只读参考 `.github/workflows/ci.yml`、`src/ui/theme.rs`、`src/ui/dialog_footer.rs` 不列入 change-scope。

如果实施中编译器发现新增 Action 的额外 exhaustive match，Luna 核对其是否属于本闭环，必要时在任务目录更新范围记录，不以普通文件范围调整要求用户介入。

## 6. 阶段交付

本 plan 阶段交付本文件与 `change-scope.json`，追加只读验证记录并写指定 completed 回执。用户已选择自动执行，下一阶段由 Luna 自动命名并从 Task 1 的真实鼠标删除/恢复闭环开始，随后持续完成 Task 2、Task 3；不再询问执行方式。

## 7. 实施记录（Luna，2026-09-20）

- Task 1 已完成：新增 row-specific remove/restore Action、HitTarget、reducer 和模型 API；行尾删除/恢复图标使用稳定 row_id；Removed 行恢复、最后有效列保护、Added 行撤销新增语义保留；移除 RemoveColumn 隐形焦点。
- Task 2 已完成：移除 Columns 标题操作按钮；底部顺序为 Add Column → Review SQL → Cancel；变更摘要按 added/modified/removed 使用 success/action/error 语义色。
- 文档同步完成：`docs/keybindings.md`、`docs/ui-dialog-guidelines.md`、`docs/architecture.md`。
- 定向回归：catalog editor state 68 passed；catalog reducer table navigation 通过；keymap table 9 passed；UI table editor 15 passed；mouse table 4 passed；row-specific mouse mapping通过。
- 项目门禁：`cargo +1.94.0 fmt --all -- --check`、`cargo +1.94.0 clippy --all-targets --all-features -- -D warnings`、`cargo +1.94.0 test --all-targets --all-features` 均通过；全量测试首次发现并修复了两条依赖旧 Remove 焦点链的测试断言，修复后受影响 state 测试 68/68 通过。
