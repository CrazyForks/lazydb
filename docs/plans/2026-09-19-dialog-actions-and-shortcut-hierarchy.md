# Dialog Actions and Shortcut Hierarchy Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行环境若没有该技能，直接按本文任务顺序实施，每个任务完成定向验证后再继续。

**Goal:** 为 LazyDB 建立统一的上下文操作区、窗口动作栏和快捷键帮助栏，使操作、状态、帮助能够凭位置与样式快速区分，并保持键盘与鼠标行为一致。

**Architecture:** 扩展现有 `dialog`、`shortcut_hints`、`Theme` 组件，增加统一的底部区域测量与布局；业务窗口提供动作、状态、焦点与上下文提示。复用现有 `src/help.rs` 快捷键目录、`Keymap` 和 `HitTarget`，使帮助的展示与执行不依赖解析展示文案。以 Table Editor 为样板，随后迁移其他编辑器、确认框和浏览窗口。

**Tech Stack:** Rust 2024 / Rust 1.94、Ratatui 0.30.2、Crossterm 0.29、unicode-width 0.2、现有 App reducer / UI hit regions / Rust 单元与集成测试。

---

## 0. 基线与实施边界

- 分析日期：2026-09-19；分析时 HEAD：`9037713`；开始编写计划时工作区干净。
- 本文是实施计划，本次仅新增本文；下列测试命令均为后续实施步骤，尚未执行。
- 基于当前代码重新核对每个任务涉及的符号，不使用其他历史计划代替当前实现。
- 产品文案沿用当前英文；计划说明使用中文。
- 按“共享组件 → 样板 → 交互闭环 → 其他窗口 → 总体验收”实施。
- 新增测试集中覆盖布局边界、焦点、动作可用性、帮助真实性与鼠标命中；纯颜色和间距调整通过现有测试及人工视觉验收，不逐个属性补镜像测试。
- 每个任务是一个独立验收单元；获得提交授权后可按文末建议拆分提交。

## 1. 当前实现与需解决的问题

| 位置 | 已核对的实现 | 实施要求 |
| --- | --- | --- |
| `src/ui/catalog_editor.rs::render_table` | 四个动作在同一行；状态独立一行；帮助高度取决于当前焦点产生的提示行数 | 局部动作上移，底栏高度只受尺寸档位影响 |
| `src/ui/catalog_editor.rs::render_interactive_hints` | 帮助居中，依据 `hint.key` 展示字符串补充点击按键 | 左对齐；按键执行由结构化定义提供 |
| `src/ui/catalog_editor.rs::table_shortcut_hints` | General 显示合并导航键；Columns 显示大量别名和操作 | 按上下文排序、压缩；长尾进入完整帮助 |
| `src/ui/catalog_editor.rs::render_table_column_details_modal` | 类型说明与 Confirm/Cancel 控件都可能占用 `inner.y + 7`；帮助固定在 `+8` | 分离内容说明、错误区、动作区和帮助区 |
| `src/ui/dialog.rs` | 已有 `DialogButton`、`DialogTone`、焦点、禁用、点击区域返回；按钮默认居中，窄时纵向排列 | 扩展现有组件，不另建一套按钮系统 |
| `src/ui/shortcut_hints.rs` | 已支持 Unicode 单元格宽度、自动换行、鼠标点击；key 使用 action 色，描述使用正文色 | 提供更弱的帮助样式和单行优先级布局 |
| `src/help.rs` | 已有 ShortcutContext、HelpShortcutId、Shortcut、footer priority | 作为统一提示定义入口，接入窗口的真实状态 |
| `src/input/keymap.rs::map_shortcut` | 可将提示的 KeyEvent 序列送入与键盘相同的映射流程 | 保留执行通道，用契约测试核对提示 |
| `src/input/keymap.rs::map_table_editor` | General 和 Columns 的 Enter 都是 Preview；`e` 编辑列；单行字段 Up/Down 切换焦点 | 按现有行为写准确提示，不能把 Enter 全部改写成 activate |
| `src/ui/theme.rs` | 已有彩色和 plain 模式，`grid_active_cell` 已使用反显兜底 | 复用主题 token；焦点不单独依赖颜色 |

## 2. 固定产品规范

### 2.1 三类信息的责任

1. **上下文操作区**：紧邻被操作内容，例如 Columns 标题旁的 Add Column、Remove/Restore。
2. **窗口动作栏**：左侧可选状态，右侧窗口动作，例如 Cancel、Review SQL。
3. **快捷键帮助栏**：独立、左对齐，只包含“按键 + 当前行为”的轻量提示。

标准布局：

```text
│ COLUMNS · 6                         [ Add Column ] [ Remove ] │
│ #  NAME              TYPE          NULLABLE                   │
│ 1  id                integer       NOT NULL                   │
│ ...                                                           │
│ ───────────────────────────────────────────────────────────── │
│ 2 pending changes                  [ Cancel ] [ Review SQL ]  │
│ ───────────────────────────────────────────────────────────── │
│ Tab next  Shift+Tab previous  Enter review SQL  Esc cancel     │
```

这张图表示 General 焦点时的按键含义；焦点位于按钮时改为 `Enter activate`。空间不足时按完整提示项做优先级裁剪。

### 2.2 按钮与焦点

- 增加独立的 `DialogEmphasis::{Secondary, Primary}`，与现有 `DialogTone::{Normal, Danger}` 分开；主操作不等于危险操作。
- 普通按钮：正文色、方括号边界；主按钮：强调色文字或边界。
- 焦点按钮：实色背景/反显 + ASCII `>` 标记。
- 为 `>` 预留固定单元格，获得或失去焦点时宽度不变。
- 禁用按钮位置保留，不注册激活 hit region；键盘焦点遍历跳过不可用动作。
- 主操作、焦点与表格所选行分别表达；局部按钮获得焦点时仍保留其目标行的弱选中状态。
- 每个决策阶段最多一个默认主操作。Commit/Rollback 等对等决策不强行制造主操作；延续业务默认焦点。
- 对 Cancel/Review、Cancel/Save 等两项操作，视觉顺序使用“次要在左、主要在右”，同步调整焦点遍历和索引映射。

### 2.3 帮助栏

- key：`theme.text` + Bold；description：`theme.muted`；背景复用父区域。
- 组合键格式为 `Shift+Tab`、`Ctrl+S`；连续键为 `dd`；不把正反动作合成一个可点击项目。
- `Tab next` 与 `Shift+Tab previous` 分开。Up/Down 等别名在完整帮助中展示；底栏优先保留最容易理解的代表键。
- 继续支持已有快捷键点击，但不进入 Tab 焦点序列；按钮仍是主要的鼠标操作入口。
- 显示字符串不能再决定执行什么按键；每个可执行提示显式携带按键序列或已有 HelpShortcutId。
- 单行帮助按整项选择，不能只显示半个键名或说明。被省略的项目进入已有完整帮助。
- 帮助入口使用真实配置的绑定；只有映射确实可用时才显示。F1 是待核对的默认入口，不新增会被文本输入消费的 `?` 快捷键。
- 优先级：退出/返回、当前主要行为、导航、完整帮助入口、上下文辅助动作。最终显示顺序固定，不按剩余空位随机插入。
- 对被省略项目保留帮助入口；不出现无法打开的 `... (+N)`。

### 2.4 布局与缩放

以 **窗口内区** 宽高测量，不以整个终端宽度判定按钮是否放得下：

| 档位 | 规则 |
| --- | --- |
| 标准 | 内容分隔线 + 状态/动作行 + 帮助分隔线 + 一行帮助，总计通常 4 行 |
| 紧凑 | 省略第二条分隔线，保留 1 行动作、1 行帮助；通常 3 行 |
| 窄宽度 | 先缩短已定义的标签，再将动作组占用两行；状态可缩写/省略；保持所有必要动作可达 |
| 极小 | 隐去可选状态和分隔线；保留退出/返回与当前焦点动作可见；若连基本操作也放不下，显示终端过小提示，退出按键仍有效 |

- 同一窗口、同一尺寸下，切换焦点不能改变底栏高度和表格可见行数。
- Remove/Restore 共用同一个位置，测量时取两者最大标签宽度。
- 窄屏动作栏若需要两行，布局先预留两行，不能渲染阶段临时换行覆盖帮助。
- 所有宽度使用终端显示单元格，不能使用 `str.len()`。
- 所有返回矩形都位于父区域内；零宽/零高时安全返回；隐藏控件没有点击区域。

### 2.5 状态和可用性

- 未修改：`No changes`。
- 已修改：`N pending changes`；有空间可显示 added/modified/removed 分类。
- N 的定义：新增列数 + 修改列数 + 删除列数 + 表属性变更组（0/1）+ 列顺序变更组（0/1）；同一列按现有摘要状态计一次。
- 状态由已有 `TableChangeSummary` 派生；不建立另一份 dirty 状态。
- 无选中列时 Remove 不可用；选中已删除列时相同位置显示 Restore。
- busy 时按钮与提示均反映真实可用操作，不显示执行流程无法响应的激活提示。
- Review SQL 的可用性复用实际预览规则。本次不单凭 `No changes` 禁用 Review，以免创建模式、错误反馈或其他对象编辑行为被误改。
- 错误说明放在独立内容区域或已有错误区域，不能占用底栏控制所在行。

## 3. Task 1：固化迁移清单与交互合同

**Files**
- Create: `docs/ui-dialog-guidelines.md`
- Review: `src/ui/mod.rs::render_overlay`
- Review: `src/ui/dialog.rs`、`src/ui/shortcut_hints.rs`、`src/help.rs`
- Review: `src/input/keymap.rs`、`src/input/mouse.rs`

**Steps**
1. 从 `render_overlay` 的全部分支和 UI 模块列出窗口清单，标记“表单 / 确认 / 浏览 / 仅提示”。
2. 为每类记录当前动作、焦点顺序、Enter/Space/Esc 行为、busy 行为、鼠标目标、完整帮助入口。
3. 将第 2 节规范写入设计文档，补充标准与紧凑字符线框。
4. 为清单标记当前是否已有可聚焦动作，特别记录 Column Details 与 Redis 编辑器，避免只画出按钮却没有键盘焦点。
5. 核对帮助入口的上下文路由：输入框、子弹窗、busy 状态分别验证。若默认帮助键被提前返回吞掉，将修复列入 Task 4。

**Acceptance**
- 每个窗口有具体迁移任务归属；无动作的浏览窗口只迁移帮助栏，不凭空添加按钮。
- 文档明确 Enter 在不同上下文的真实含义。

## 4. Task 2：扩展共享按钮、样式与底栏布局

**Files**
- Modify: `src/ui/dialog.rs`
- Modify: `src/ui/theme.rs`
- Create: `src/ui/dialog_footer.rs`
- Modify: `src/ui/mod.rs`（模块声明）
- Test: `src/ui/dialog.rs`、`src/ui/dialog_footer.rs` 内联测试

**Steps**
1. 在按钮模型中加入 emphasis；逐个更新 `DialogButton` 构造点以保持编译，并标明主操作。
2. 将按钮布局分为测量和绘制：测量输出真实高度、每个按钮的矩形与原始动作索引；绘制和 hit region 共用这份结果。
3. 给按钮布局增加 Left/Right/Center 对齐参数。局部操作和底部动作采用 Right；旧调用点可先显式保留原布局，再随迁移改动。
4. 在 `Theme` 增加语义样式方法：次要按钮、主按钮、焦点按钮、禁用按钮、帮助 key、帮助描述、底栏分隔线。复用现有色值。
5. 新建 `dialog_footer` 的纯布局函数，输出 body、可选状态区域、动作区域、帮助区域、分隔线位置。它只处理布局，不拥有业务状态。
6. 标准行内先为动作预留宽度，再将剩余区域分给状态；两个区域至少间隔 2 个字符。
7. 根据按钮全量/短标签和最坏情况状态测量确定尺寸档位；焦点变化不参与高度计算。
8. 添加有价值的边界测试：零尺寸、中文标签、焦点前后矩形一致、窄屏多行不重叠、禁用动作没有激活区域。

**Checks**
```bash
cargo +1.94.0 test --lib ui::dialog::
cargo +1.94.0 test --lib ui::dialog_footer::
```
预期：布局与命中测试通过；对每个返回 Rect 断言属于父区域；焦点变化不改变几何。

## 5. Task 3：统一快捷键帮助的排版与点击几何

**Files**
- Modify: `src/ui/shortcut_hints.rs`
- Modify: `src/ui/dialog_footer.rs`
- Test: `src/ui/shortcut_hints.rs` 内联测试
- Test: `tests/mouse.rs`

**Steps**
1. 为紧凑帮助增加结构化的优先级与短文案输入；沿用 `ShortcutHint` 的 key/description/activation 数据。
2. 实现单行帮助布局结果：包含被展示项索引、每项 Rect、被省略项数量；彩色绘制与鼠标命中共用结果。
3. 帮助采用正文色按键、次级色说明、左对齐；保留已有多行渲染供完整帮助等合适场景使用。
4. 单行布局按完整项选择；预留退出和可用帮助入口；长尾导航别名优先省略。
5. 超窄时仍不能切断 key/description 对；空间不足的项不注册 hit region。
6. 将 footer 使用的新路径接入共享底栏；不让 legacy 的自动换行改变已测量高度。
7. 测试中文显示宽度、最后一个单元格命中、省略项无命中、焦点切换的提示条高度不变。

**Checks**
```bash
cargo +1.94.0 test --lib ui::shortcut_hints::
cargo +1.94.0 test --test mouse shortcut
```
预期：旧点击能力保留，新布局所见与所点一致。过滤命令必须实际命中测试，记录测试数。

## 6. Task 4：让窗口帮助与实际按键行为同源

**Files**
- Modify: `src/help.rs`
- Modify: `src/ui/catalog_editor.rs::table_shortcut_hints`
- Modify: `src/ui/catalog_editor.rs::render_interactive_hints`
- Modify: `src/ui/dialog.rs::render_interactive_hint`
- Modify: `src/ui/profiles.rs::form_hints`
- Modify: `src/input/keymap.rs`（帮助路由和确有需要的契约修正）
- Test: `src/help.rs` 内联测试、`tests/keymap.rs`

**Steps**
1. 在既有 help 模块提供窗口上下文的结构化提示入口：从 App、现有 ShortcutContext、焦点和 busy 状态得到可见项目。
2. 复用 HelpShortcutId、快捷键目录和当前绑定解析；支持可配置绑定的地方展示实际绑定。
3. 对静态目录缺少的细粒度状态补充定义，例如 Table General、Column Actions、Column Details 的开关/文本字段。
4. 用同一份上下文描述生成底栏摘要和完整帮助。物理执行继续由 Keymap 承担，不把输入业务迁进 UI。
5. 将 `Tab/Shift-Tab/Up/Down` 拆成明确的导航项目；底栏只选代表项，完整帮助保留别名。
6. 删除迁移窗口中通过展示文字推断按键的 match/split 逻辑；正反方向不能合并后只点击执行一个方向。
7. 核对帮助入口的事件优先级，确保在可展示完整帮助入口的窗口能真正打开；关闭帮助后恢复原焦点、草稿和子弹窗。
8. 新增契约测试：每个可执行提示的实际按键序列交给当前 Keymap，得到正确 Action；busy/disabled 时提示与动作一致。

**明确保留的行为**
- Table General/Columns：Enter → Review SQL；Columns：`e` → Edit Column。
- 单行字段：Left/Right → 光标；Up/Down 的既有切字段行为保留，底栏优先提示 Tab。
- 多行 SQL/Value 区域：方向键继续服务编辑或滚动，不被按钮导航抢占。
- 按钮：Enter 激活；Space 仅在该上下文实际支持时展示。
- 不同退出阶段分别显示 cancel、back、close，避免统一成含义模糊的 `close/cancel editor`。

**Checks**
```bash
cargo +1.94.0 test --lib help::tests::
cargo +1.94.0 test --test keymap
```
预期：提示契约、新帮助入口、既有键位回归全部通过。

## 7. Task 5：完成 Edit Table 样板改造

**Files**
- Modify: `src/ui/catalog_editor.rs::render_table`
- Modify: `src/ui/catalog_editor.rs::render_table_column_details_modal`
- Modify: `src/model/catalog_editor.rs::TableDraft::focus_next` / `focus_previous`
- Modify: `src/model/catalog_editor.rs::TableEditorFocus`（补齐字段详情动作焦点）
- Modify: `src/input/keymap.rs::map_table_editor` / `map_table_column_details`
- Modify: `src/app.rs`、`src/input/mouse.rs`（对应焦点/动作分发）
- Test: `tests/catalog_editor_state.rs`、`tests/catalog_editor_reducer.rs`
- Test: `tests/ui_render.rs`、`tests/mouse.rs`、`tests/keymap.rs`

**Steps**
1. 先增加必要回归：样板底栏不随 General/Columns/Action 焦点变化而跳动；局部按钮获得焦点时选中行仍可识别。
2. 把 Add Column、Remove/Restore 放到 Columns 标题右侧；宽度不足时使用单独的紧邻工具行，不能与标题重叠。
3. 底部只保留状态、Cancel、Review SQL；Review 使用 Primary 样式，但只有真正获得焦点才反显。
4. 用共享 footer 分配区域，移除根据 `hint_lines.len()` 调整表格高度的逻辑。
5. 将 `Changes  No changes` 改成 `No changes`；按已有摘要生成数量和分类。
6. 焦点遍历按阅读顺序统一为：General 字段 → Add → Remove/Restore（可用时）→ Columns → Cancel → Review → 循环。
7. Columns 中 Up/Down 或 j/k 仍移动行；Tab/Shift+Tab 才跨控件。局部工具区和窗口动作区中的方向键仅导航该组；跨组使用 Tab。
8. 空表时跳过无效 Remove，但 Add 和窗口操作可达；所选列被移除时焦点不会失效，动作切换成 Restore。
9. 点击局部按钮时按当前选中列操作，渲染生成的命中矩形不依赖文本起点猜测。
10. Column Details 改用共享底栏，将类型提示和错误文本留在 body 中；Confirm/Cancel 不再覆盖说明行。
11. 为 Column Details 增加明确的动作焦点状态（字段与按钮区分），同步处理 Tab、反向 Tab、Enter 和鼠标。原本字段 Enter 确认的快捷行为保留；Space 只在开关字段切换或按钮已支持时激活。
12. 子弹窗打开时不让父窗口按钮保留强焦点；关闭子弹窗后恢复到合理的列选择/原动作位置。

**Checks**
```bash
cargo +1.94.0 test --test catalog_editor_state --test catalog_editor_reducer
cargo +1.94.0 test --test ui_render table_editor
cargo +1.94.0 test --test keymap table
cargo +1.94.0 test --test mouse catalog
```
预期：焦点循环、子弹窗返回、鼠标动作和布局不跳动均通过；相关过滤测试数非零。

**样板验收门槛**
- 120×40、100×30、80×24 下截图可直接区分按钮与帮助。
- 同尺寸切换焦点，底栏位置和字段表格高度不变。
- 无修改、有修改、空列、已删除列、字段错误、规划中、执行中状态均有明确表现。
- 完成样板验收后再进入大范围迁移。

## 8. Task 6：迁移其他编辑器

**Files**
- Modify: `src/ui/catalog_editor.rs`（其他对象表单、picker/loading/preview）
- Modify: `src/ui/profiles.rs`
- Modify: `src/ui/redis_object_editor.rs`
- Modify: `src/ui/redis_table_editor.rs`
- Modify: `src/model/catalog_editor.rs`（其他表单 focus order）
- Modify: `src/model/profile_manager.rs`
- Modify: `src/model/redis_object_editor.rs`、`src/model/redis_table_editor.rs`
- Modify: `src/help.rs`、`src/input/keymap.rs`、`src/input/mouse.rs`、`src/app.rs`
- Test: `tests/ui_render.rs`、`tests/keymap.rs`、`tests/mouse.rs`
- Test: `tests/profile_reducer.rs`、`tests/redis_object_editor.rs`

按下列三个子单元逐个实施和验证：

### 6A. Catalog 其他表单
1. Database、Role、View、Materialized View、Sequence 等表单统一 Cancel/Review 动作栏。
2. 将对象特有说明和错误移出控制行，保留已有字段可用性规则。
3. 更新相应 focus order，与视觉顺序一致。
4. SQL Preview 使用“返回编辑 / 应用”含义的动作区；规划中和执行中只展示可响应的操作。
5. picker/loading 只有适用的帮助或取消入口，不强制加入空动作栏。

### 6B. 连接管理
1. Test 等局部/辅助操作与 Cancel、Save/Save & Connect 形成明确组别。
2. 保留 Ctrl+T、Ctrl+S、Ctrl+Enter 的现有行为，底栏显示当前绑定和对应含义。
3. 连接测试结果作为状态，验证错误留在内容区。
4. 检查切换数据库类型导致字段变化后的焦点与底栏稳定性。

### 6C. Redis 编辑器
1. Key/Value/TTL 等编辑内容与提交/取消操作分区。
2. 当前模型只有字段焦点的地方，补齐动作焦点；`focused: usize` 的数组索引不能直接复用为越界的按钮索引。
3. 推荐将 Redis Table 的焦点显式区分为 Field(index) 和 Action，逐个更新字段访问点。
4. 保留原字段 Enter 提交等快捷行为，按钮导航不能抢走 Value 编辑操作。
5. busy 时同步禁用按钮、提示和鼠标激活；失败返回后恢复可编辑状态与焦点。

**Checks**
```bash
cargo +1.94.0 test --test ui_render --test keymap --test mouse
cargo +1.94.0 test --test profile_reducer --test redis_object_editor
```
每个子单元先运行涉及上下文的过滤测试；整个 Task 完成后运行以上对应集成测试一次。

## 9. Task 7：迁移确认框、浏览窗口和全局底栏

**Files**
- Modify: `src/ui/mod.rs::render_overlay` 及其本地窗口渲染函数
- Modify: `src/ui/execution_confirm.rs`
- Modify: `src/ui/update.rs`
- Modify: `src/ui/profiles.rs`（删除确认）
- Modify: `src/ui/redis_table_editor.rs`（删除确认）
- Modify: `src/ui/omni.rs`
- Modify: `src/ui/sql_history_modal.rs`
- Modify: `src/ui/record_view.rs`、`src/ui/text_detail.rs`
- Modify: `src/ui/notifications.rs`
- Modify: `src/help.rs`、`src/input/mouse.rs`（必要的动作映射）
- Test: `tests/ui_render.rs`、`tests/mouse.rs`、`tests/keymap.rs`
- Test: `tests/quit_transaction_review.rs`、`tests/redis_unsaved_changes.rs`

**Steps**
1. 按 Task 1 清单迁移 SQL 执行、目录删除/放弃、连接删除、Redis 保存/删除/未保存、事务退出/回滚、更新窗口等确认框。
2. 危险动作保留已有 Danger 语义与默认焦点，不能因统一“主操作”而自动选中执行/删除。
3. 操作顺序变化时逐个核对 `action.index`、模型枚举和 HitTarget 映射，尤其三按钮窗口。
4. 复用共享测量，为长标签（如取消查询并回滚）分配足够高度，不截断成含糊动作名。
5. 浏览窗口迁移为独立弱化帮助栏；具备上下文动作的窗口按其实际作用范围放置，不强制增加按钮。
6. 主工作区 `render_footer` 复用同样的 key/description 样式、优先级和实际绑定。主工作区本身不额外添加窗口式动作栏。
7. 检查 overlay 堆叠时只有最上层可交互，关闭后父层焦点恢复。
8. 清理已无调用的字符串解析帮助适配器和重复按钮绘制函数。
9. 对照清单逐项标记完成，扫描残留的手工方括号按钮、居中大段提示与硬编码底部 y 坐标。

**Checks**
```bash
cargo +1.94.0 test --test ui_render --test mouse --test keymap
cargo +1.94.0 test --test quit_transaction_review --test redis_unsaved_changes
cargo +1.94.0 test --lib ui::update::
```
预期：所有迁移窗口保持原有业务动作；三个按钮的键盘和鼠标索引正确。

## 10. Task 8：最终验收与文档交付

**Files**
- Modify: `docs/ui-dialog-guidelines.md`
- Modify: `docs/keybindings.md`
- Modify: `docs/architecture.md`（UI 共享组件职责）
- Modify: 本计划（填写实际执行记录）

### 8A. 自动化检查

与 `.github/workflows/ci.yml` 一致：

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

预期：格式、lint 和测试通过。定向测试用于定位各阶段问题；最终门禁在整批实现完成后跑一次，后续有修复才重跑受影响检查。

### 8B. 视觉与交互验收矩阵

| 维度 | 样例 | 合格标准 |
| --- | --- | --- |
| 终端尺寸 | 160×50、120×40、100×30、80×24、60×20、40×12 | 正常尺寸清晰；小尺寸无覆盖、无 panic，关键操作可达 |
| 主题 | 默认彩色、plain | 单色仍分清焦点、按钮、帮助 |
| 图标 | Unicode 与 ASCII 模式 | 焦点标记和分隔线正常，无字符宽度错位 |
| 焦点 | 文本、开关、表格、局部按钮、全局按钮、子弹窗 | 只有一个强焦点，所选对象不丢失 |
| 状态 | 无修改、有修改、空集合、已删除、错误、busy | 文案和可用性准确，底栏位置稳定 |
| 鼠标 | 点击按钮、点击帮助、缩放后点击、禁用按钮 | 每项点击对应实际展示的动作，无幽灵命中区 |
| 文本 | 中文列名、长表名、长错误、长按钮标签 | 以显示宽度布局，正文与控制不覆盖 |
| 帮助 | 输入框打开帮助、子弹窗打开帮助、关闭恢复 | 真实快捷键可用，状态和焦点不丢失 |

关键人工路径：
1. Edit Table 修改名称 → 添加列 → 确认列详情 → 选列 → Remove → Restore → Review SQL → 返回编辑。
2. 同一尺寸依次切换所有焦点，观察表格高度和底栏 y 不变。
3. 调整到 80×24 和 60×20，重复操作，并验证主按钮始终可发现。
4. 连接表单测试连接、保存、取消；确认结果与状态区一致。
5. Redis 值编辑、保存失败、再次编辑、取消；验证焦点不落到隐藏字段。
6. 三按钮事务/未保存确认，逐一核对 Tab、Enter、Esc 与鼠标目标。
7. plain 模式重复样板核心路径，确认不靠颜色也能识别。

### 8C. 交付内容

- 统一按钮与底栏组件，以及上下文帮助生成入口。
- Edit Table 和 Column Details 改造后的标准/紧凑截图。
- 全窗口迁移清单及完成状态。
- 设计规范、键盘文档、架构说明。
- 自动化实际结果与人工验证记录；未执行项目明确标记，不将计划写成通过记录。

## 11. 实施顺序与建议提交边界

```text
Task 1 交互合同与清单
  → Task 2 共享按钮/布局
  → Task 3 帮助排版与命中
  → Task 4 上下文提示契约
  → Task 5 Edit Table 样板验收
  → Task 6A Catalog / 6B Profiles / 6C Redis
  → Task 7 其余窗口
  → Task 8 最终验收
```

建议提交主题（实施且获得提交授权后使用）：

1. `docs(ui): define dialog action and shortcut hierarchy`
2. `feat(ui): add shared dialog footer layout and action emphasis`
3. `refactor(ui): unify shortcut layout and hit regions`
4. `refactor(help): derive dialog hints from contextual shortcuts`
5. `feat(ui): redesign table editor action and help regions`
6. `refactor(ui): migrate catalog profile and redis editor footers`
7. `refactor(ui): unify confirmation and browser footer hierarchy`
8. `docs(ui): document dialog conventions and verification results`

## 12. 完成定义

- [ ] 用户不阅读具体文案，也能通过位置和外观区分操作与帮助。
- [ ] 局部动作位于内容附近，全局动作固定在底部。
- [ ] 主操作强调与键盘焦点样式不同。
- [ ] 所有展示的按钮均有合理的键盘与鼠标访问路径。
- [ ] 所有展示的快捷键在该上下文真实生效。
- [ ] 按键显示、完整帮助和点击行为没有分别维护的文案解析规则。
- [ ] 同尺寸切换焦点时，底栏高度和主体可见区域稳定。
- [ ] 缩放、中文文本、plain 模式下仍可读、可操作。
- [ ] 字段详情的提示、错误和按钮不再占用同一行。
- [ ] 窗口清单完成迁移，必要回归及项目门禁通过。
