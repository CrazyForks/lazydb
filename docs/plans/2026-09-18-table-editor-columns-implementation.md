# Table Editor Columns Implementation Plan

> 执行者：Luna。按下述端到端验收单元连续推进；实现、审查、纠偏、提交合并均由 Luna 完成。Astra 本阶段只产出计划。无需子 Agent，也无需用户逐项 resume。

**Goal:** 为表结构编辑器补齐 Columns 的 j/k、dd、增删背景色、完整多行快捷键和内容自适应列宽。

**Architecture:** 复用现有 CatalogEditor 动作和 TableDraft 增删状态，输入层增加 Catalog 专属组合键作用域，UI 层统一计算行样式及显示文本。快捷键先完整换行再分配高度；列宽先测量当前草稿的显示内容，再在 viewport 预算内收缩。

**Tech Stack:** Rust 2024 / Rust 1.94、Crossterm 0.29、Ratatui 0.30.2、unicode-width、现有 reducer 与 TestBackend 测试。

---

## 0. 基线、范围与执行约定

- 基线：`af23ff65072f203b5266769cb9898203a09854b5`；目标 `main`。任务名和工作分支由后续自动命名环节决定，本计划不预占分支。
- 分析文件：`/Users/yelog/workspace/tui/lazydb/.git/opencode-tasks/ses_f4c933ed2ffeQtLlTRdVGsqLNO/analysis.md`。
- 记录目录：上述任务目录；实际检查结果追加至 `validation.md`。本计划中的“预期”均为将来验收条件，不是已通过的测试结果。
- 开始实施时读取插件 checkpoint（存在才读）、实际 git diff 和本计划。不要重复通读历史日志，不修改 state/checkpoint；只写当次 activeReceipt 指定的回执。
- 既有三个未跟踪计划文档属于其他任务：no-open-tabs-empty-state、redis-value-dialogs、tab-reorder-focus，保留且不加入本任务提交。
- 每个单元完成定向验证后记录简短进度，直接进入下一单元。普通编译失败自行纠正，不作为外部阻塞。
- 避免大规模重构。数据库驱动、DDL 生成、持久化格式均不需要变化。Relation data 是视觉参考，当前任务不改其交互。
- 新增测试集中在序列状态生命周期、动态布局和实际渲染这些易出回归的行为，不为简单键位别名编写大量镜像测试。

### 验证要求的来源与等级

1. **用户需求（必须满足）**：Columns 的 j/k、dd、增删背景色一致、快捷键完整换行、具有上下界的内容自适应列宽。第 1 节明确具体操作语义，各单元给出可观察验收结果。
2. **项目强制门禁**：`.github/workflows/ci.yml:81–83` 的 Rust 1.94 fmt、all-targets/all-features clippy 与 test。第 6.2 节列出原命令；环境受限时如实记录未完成项，不能把跳过当通过。
3. **本计划选择的自动化回归证据**：按键状态、TestBackend Buffer、56×16 等尺寸、Unicode、命中区域和草稿状态测试，用于证明需求与既有行为兼容。具体 fixture、断言组织和过滤器允许按最终实现调整，但五项需求的有效证据不能缺失；这些测试设计不是用户额外指定的人工门禁。
4. **补充建议验证（非必需门禁）**：人工操作、PTY、截图比对或真实数据库视觉演示。用户没有强制此类检查；不得因新建议的人工验证未执行而阻塞完成。若尝试后受限，最多一次针对性修复重试，记录限制，由 Luna 收尾审查决定是否需要其他证据。

完整计划同步保存在任务目录 `plan.md`；其内容应与本文件一致，供插件后续阶段读取。

## 1. 已确定的产品行为

| 操作/状态 | 明确行为 |
|---|---|
| Columns j/k | 无修饰键，分别作为现有 Down/Up 别名；中间移动行，边缘沿用现有整体焦点流 |
| 文本字段 j/k/d | 仍作为文本输入，不触发导航/删除 |
| Columns dd | 两个真实 Press、在配置的组合键超时内完成；单 d 无删除副作用 |
| d 后 Esc | 先取消组合键，保留表单；再 Esc 才走原取消流程 |
| d 后其他键 | 清掉组合键，并执行该键原有行为，例如 j 仍下移、e 仍编辑 |
| d 长按 | Repeat 不能完成 dd 或连续删除；Release 不当作第二个按键 |
| 删除 Existing | 草稿变 Removed，仍可看到且可 r 恢复，Review SQL 后按原流程提交 |
| 删除 Added | 取消新增并移除草稿行；最后一个有效列仍受已有保护 |
| Added/Removed 选中 | 状态背景优先；焦点用现有 `▌` 和必要的字重/前景表示 |
| hints | 当前焦点下所有条目完整换行，不出现 `... (+N)` |
| 列宽 | 基于所有草稿行的显示文本，不因上下滚动改变；设定最小/最大值，宽度不足时确定性压缩 |

## 2. 单元一：Columns 键盘编辑闭环

**修改文件**
- `src/input/keymap.rs`：`Pending` / `PendingState`、CatalogEditor overlay 分发、`map_table_editor`、`pending_is_valid`、`pending_display`、相关穷尽匹配与单元测试。
- `src/ui/catalog_editor.rs::render_table`：Columns hints 显示 `j/k · ↑/↓` 和 `dd`。
- 测试：`tests/catalog_editor_reducer.rs`、`tests/catalog_editor_state.rs` 中复用已有 Table fixture 和草稿断言。
- 只读参考：`src/app.rs:7455–7510,7775–7816`；`src/model/catalog_editor.rs:2059–2180,2303–2365`；`src/runtime.rs:5887–5934`。

### 步骤 1.1：建立关键状态回归

使用现有 Catalog Editor fixture，或抽出本文件内的小 fixture 构造三列 Existing 表单。写成按键→Action→App reducer→草稿的断言，不只断言返回枚举。

覆盖以下最小矩阵：

1. j/k 与 Down/Up 在中间和首尾位置产生相同结果；General/ColumnDetails 的 `jkd` 可正常输入。
2. 一个 d 返回 None，列不变；第二个 d 仅产生一次 RemoveTableColumn，Existing 变 Removed；r 恢复。
3. d、j、d 不删除；d、e 后在详情里 d 是文本；d、Tab 后不保留组合键。
4. 超时后的 d 只启动新序列；测试直接使用现有 Instant/超时测试模式，不用 sleep。
5. d Press + d Repeat 不删除；完整 Press/Release/Press 正常完成。
6. 变化为 busy / SqlPreview / 非 Table / 选中另一列 / 另一个目标后原 d 失效。
7. 新增列删除和最后一个有效列保护复用已有模型测试，缺失的才补。

先运行新增测试过滤器，确认它们在当前实现下因预期行为缺失而失败，而非 fixture 或编译问题。

### 步骤 1.2：实现有作用域的 pending

- 新增 `Pending::CatalogColumnDelete`，复用 `sequence_timeout`、`started_at`、clear/expire 方法。
- 增加只在 CatalogColumnDelete 使用的上下文快照；至少包含 Catalog target/anchor、Form 页面、Columns 焦点及 selected column（现有列可用稳定身份，新增列用与索引/编辑上下文一致的标识）。不要把组合键状态写入 TableDraft。
- `pending_is_valid` 同时验证快照和原来的 focus/editor_mode/tab/generation/timeout。校验函数只读 App。
- 在 CatalogEditor overlay 现有的无条件 `self.pending = None` 位置加入专属处理：非有效 Columns 上下文清除并回到原映射；无修饰 Press d 时 take/验证旧前缀，有效则返回 RemoveTableColumn，否则设置新前缀。Repeat d 返回 None；非 d 清除后调用原映射。
- `pending_display(CatalogColumnDelete)` 返回 None，不借用 RelationDelete 的 help 文案；`map_pending` 等穷尽分支明确处理该 variant。
- Busy 的早返回路径也应使 Catalog 前缀失效，避免忙碌结束后复活。
- 现有 runtime 对鼠标非 Moved 事件、Paste、Resize、FocusGained/Lost 已调用 clear_pending，无需重复实现一套；键位测试调用 clear_pending 模拟这些边界。单纯移动鼠标而不改变选择不必取消。
- 处理完毕后，导航别名仅在 Columns 分支增加 `Char('k') | Up` 与 `Char('j') | Down`，仍返回原 FieldPrevious/Next。

### 步骤 1.3：同步提示并验证

Columns hints 至少包含 move row、move focus、a add below、e edit column、dd delete column、r restore、Esc close/cancel editor。此单元允许沿用单行布局，下一个布局单元负责全部展示。

运行：

```sh
cargo +1.94.0 test --lib input::keymap
cargo +1.94.0 test --test catalog_editor_state
cargo +1.94.0 test --test catalog_editor_reducer
```

预期：新增和既有用例通过，实际执行数量非零；无数据库执行副作用。记录 diff 和命令结果。若工作流已授权提交，可形成 `feat(catalog): add column navigation and delete shortcuts` 原子提交，仅 stage 本单元相关文件。

## 3. 单元二：增删状态背景闭环

**修改文件**
- `src/ui/catalog_editor.rs::render_table`（现基线 1843–1966）。
- `tests/ui_render.rs` 的 Table Editor Buffer 测试。
- 只读参考：`src/ui/data_grid.rs:200–215,253–285`、`src/ui/theme.rs` 的语义色。

### 步骤 2.1：用真实 Buffer 固定状态优先级

在同一表单放 Existing、Added、Removed 三种行，分别选中每一行。通过 `CatalogEditorTableColumn(index)` hit region 定位实际屏幕行，断言名称单元格、序号槽、分隔符的背景。

- Added：背景 `theme.row_inserted`，选中和未选中均成立。
- Removed：背景 `theme.row_deleted_background`，选中和未选中均成立。
- 普通行：选中用 selection，未选中用 surface。
- 恢复后删除背景消失；`+/-`、REMOVED 和焦点标记仍存在。

### 步骤 2.2：集中确定最终样式

优先级实现等价于：

```text
Removed => fg muted + bg row_deleted_background + DIM
Added => fg text + bg row_inserted
Existing, active => fg text + bg selection
Existing, inactive => fg text + bg surface
```

分隔符只覆写 grid_border 前景，背景继承所处行；行 highlight 不再统一覆盖为 selection。保持焦点标记 `▌` 可见，必要时只对标记加粗，不改变语义背景。沿用主题 token，禁止 RGB 常量。

注意：relation data 当前选中 Deleted 数据单元格会变为 selection，此处按需求让删除背景始终可识别，而不是复制其选中覆盖行为。

### 步骤 2.3：定向验证

```sh
cargo +1.94.0 test --test ui_render table_editor
```

新增颜色测试须覆盖项目已有替代主题或 ColorMode 测试方式；无色模式用状态文字/符号证明仍可辨。若测试名称未含 table_editor，单独运行相应过滤器。可形成 `fix(catalog): preserve column mutation backgrounds` 原子提交。

## 4. 单元三：完整多行快捷键布局闭环

**修改文件**
- `src/ui/shortcut_hints.rs`：新增多行接口与其单元测试，保留单行接口的现有行为。
- `src/ui/catalog_editor.rs::form` / `render_table`：先测量后布局，action/summary/list/hints 共用几何预算。
- `tests/ui_render.rs`：完整 hints、compact、滚动、鼠标命中区域。

### 步骤 3.1：实现无省略的多行 hint 接口

建议接口：

```rust
pub(super) fn lines(
    hints: &[ShortcutHint<'_>],
    width: u16,
    theme: Theme,
    background: Color,
) -> Vec<Line<'static>>
```

算法：

1. 零宽或空 hints 返回空 Vec。
2. 按既有 key/空格/description 样式生成完整 span；正常按整个 hint 装箱，hint 间沿用三空格分隔。
3. 当前行放不下完整 hint 时另起一行，行首不加分隔符。
4. 单个 hint 超宽时按显示单元宽度拆行，保留全部文字和 span 样式；对宽度小于一个宽字符的极端情况保证算法前进，交给极小尺寸降级处理，不允许死循环。
5. 不调用 packed_count，不生成 omitted marker。Paragraph 接收已经分行的结果，不再次用不同规则计算行数。

测试验证：所有 hint 按序且无缺失；行宽符合预算；单条超长 description、中文、emoji、组合字符、width=0/1 都可终止；键和说明的颜色/字重保留。既有 `line` 的省略测试继续通过。

### 步骤 3.2：重算 Table 垂直布局

将 hint 构造移动到 render_table 布局计算之前。先取得 lines 和行数，再计算：

```text
footer_height = wrapped_lines.len()
footer_y = content.bottom - footer_height
action_y = footer_y - 1
summary_y = action_y - 1（仅有空间时）
list_bottom = summary_y 或 action_y
list_capacity = list_bottom - list_start
```

以上减法均用饱和运算并限制在输入 Rect 内。不要在其他位置保留旧的 bottom-2/bottom-1 魔数。

- Table 分支可以重新分配 `form` 外层两行 footer 空间；先确认其中 error/状态文本用途，保留实际错误显示，不将其他对象表单的布局一起改动。
- compact 决策基于扣除实际 footer 后的空间，保证至少一行选中列；优先省略 summary、装饰空行、展开的 General 字段。
- selected row 的 visible_start 根据新 list_capacity 计算；同一份实际 Rect 供渲染和 hit region 使用。
- 普通尺寸完整显示全部 hints，不限制为两行。详情 modal 打开时沿用其独立提示；不能因为父表单 footer 腾空间覆盖 modal。
- 极小尺寸无法同时容纳所有内容时使用项目风格的明确尺寸不足/紧凑显示；无越界、无 panic、不制造可点击但不可见的区域。56×16 不属于可随意降级丢掉列表的范围。

### 步骤 3.3：验证用户可见结果

尺寸矩阵：106×34、80×24、56×16，以及零/极窄安全用例。

每个正常尺寸验证 Columns 下全部提示的 key 和 description 均能从实际 Buffer 读出，特别是 dd、r、Esc；不存在 `... (+`；选中列有真实行区；Add/Remove/Review/Cancel hit region 在窗内、不同区域不重叠。检查切换 General/Action 焦点后的提示和列表高度，不依赖固定 y 坐标。

```sh
cargo +1.94.0 test --lib ui::shortcut_hints
cargo +1.94.0 test --test ui_render table_editor
cargo +1.94.0 test --test ui_render table_column_details
```

可形成 `fix(catalog): wrap complete table editor shortcuts` 原子提交。

## 5. 单元四：内容驱动列宽闭环

**修改文件**
- `src/ui/catalog_editor.rs::table_column_widths`、`table_column_constraints`、`render_table` 及局部测试。
- `tests/ui_render.rs`：列宽变化和显示边界。
- 只读参考：`src/ui/data_grid.rs::automatic_widths`、现有 sanitize/truncate helpers。

### 步骤 4.1：统一显示投影

为每个 ColumnDraft 提取 UI 实际使用的四项文本：

```text
name = sanitize_terminal_text(name)，空值显示 <unnamed>
type = sanitize_terminal_text(native_type)
nullable = REMOVED / NULL / NOT NULL
comment = sanitize_terminal_text(comment)
```

同一投影用于测量和渲染；不得先截断再测量。测量扫描全部 draft.columns（包含 Added/Removed），不是当前 viewport 可见切片。不为此创建 ResultSet 或 RelationEditSession。

### 步骤 4.2：分开理想宽度和预算分配

- 每个文本列理想宽度：`max(header.width(), max_content.width()) + 2`，clamp 到 6..40。
- 正常预算下最小宽度：NAME=6、TYPE=6、NULLABLE=10、COMMENT=9，保证表头和 padding；每列最大 40。
- 序号槽宽度为行数十进制位数 + focus/state 两个字符，最少 3。四个分隔符各 1。
- 可用数据宽度为 viewport 减去序号槽及四个分隔符。
- 理想总宽不超预算：使用理想宽，右侧留白不分配到 COMMENT。
- 理想总宽超预算且最小总宽可放下：循环从超过最小值最多的列削减，平局按固定列顺序，直到满足预算。四列规模很小，无需复杂优化器。
- 预算连正常最小值都放不下：明确压缩策略，优先给各列至少一个显示位置，剩余按固定规则分配；最终宽度总和不超预算。所有数据/表头按最终宽度处理，不依赖 Table 自动裁约束。
- 实际渲染兑现两格 padding（例如左右各一格），正文截断宽度是 `final_width.saturating_sub(2)`；超窄时可先收缩 padding，再给内容一格。header 和 body 采用一致规则。
- 行尾留白与增删背景保持一致；上一单元分隔符背景规则不能回退。

### 步骤 4.3：定向测试

通过实际表头分隔符 x 坐标、Buffer 文本和颜色验证：

1. 在相同宽窗口只增加 TYPE 或 COMMENT 长度，相应列变宽；短内容不占满可用空间。
2. 极长名称/注释被最大 40 限制，仍有省略且不越列。
3. 短内容和空草稿仍满足表头最小宽度。
4. 中文、emoji、控制字符投影、<unnamed> 按终端宽度计算。
5. 最长内容在屏幕外，滚动前后列边界不变。
6. 新增/编辑详情确认后重新测量；取消详情不改变正式行布局；Removed 列仍参与测量。
7. 100+ 列的序号不吞掉分隔符；56×16 及更窄区域总宽不超预算，hit region 合法。

```sh
cargo +1.94.0 test --lib ui::catalog_editor
cargo +1.94.0 test --test ui_render table_editor
cargo +1.94.0 test --test ui_render table_column_details
```

确保新增局部测试名称能被实际过滤器匹配；不得把零用例执行记作验收。可形成 `feat(catalog): size schema columns from content` 原子提交。

## 6. 完整验收、审查与交接

### 步骤 6.1：核对五项需求的联合场景

用已有测试 fixture 构造三列 Existing 表：进入 Columns → j/k 移动 → dd 标记删除并显示删除背景 → r 恢复 → a 添加并确认新列 → 新列显示插入背景 → 长类型/注释改变列宽 → 缩至 56×16 仍可见所选行和完整提示 → Review SQL 沿用原有草稿变更。此场景不需要真实数据库执行。

通过既有测试和少量联合行为断言拼接证据，避免构建重复大型测试框架。

### 步骤 6.2：一次性项目门禁

功能完整后运行 CI 中的核心检查：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

每条命令记录开始时 HEAD、相关工作区 diff、环境、退出码和摘要；失败修复后只重跑受影响检查，最终确保当前代码有有效证据。数据库条件测试实际跳过需明确记录，不冒充数据库集成通过。

项目强制项是 CI 门禁及本任务功能验收；PTY/截图是补充视觉检查。环境问题最多一次有针对性的修复重试，再由 Luna 收尾审查评估 TestBackend 证据和限制，不能保持 progress 无限重复环境尝试。

### 步骤 6.3：Luna 审查重点

- dd 前缀是否跨 overlay、目标、焦点、行或 busy 状态复活；Repeat 是否触发删除。
- 首尾 j/k 与原箭头行为是否一致，文本字段是否被抢键。
- 新增取消与既有列删除是否沿用模型；未误增数据库执行路径。
- Table row_highlight、cell separator 是否覆盖增删背景。
- hints 全文是否存在于实际 Buffer，而不仅存在于生成的 Vec；summary/action/footer/list 是否重叠。
- 宽度测量与截断是否使用同一安全文本和同一显示宽度；超过预算时约束与实际内容是否一致。
- 既有详情弹窗、错误提示、compact、列表滚动、hit region 测试是否仍有效。
- diff 是否仅含当前任务文件；没有带入另三个计划、state/checkpoint 或生成文件。

### 步骤 6.4：阶段收尾

按插件当时的阶段授权提交/合并与写回执，不在本 plan 阶段启动实现。无需用户选择执行模式；由工作流交给 Luna 继续。最终摘要列出五项完成状态、实际检查结果、必要的环境限制和提交标识。
