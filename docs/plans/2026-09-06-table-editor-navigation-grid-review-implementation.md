# Table Editor Navigation, Grid and Review Implementation Plan

> **For Claude:** 按 @writing-plans 的分步检查点执行；若环境提供 superpowers:executing-plans，使用该技能逐项实施。未经用户明确授权，不执行 commit、push 或创建其他工作树。

**Goal:** 统一编辑表格弹窗的双向导航，将 Columns 调整为预览表格的视觉样式，并让 Review SQL 使用 SQL Editor 相同的语法分类和颜色。

**Architecture:** 保留 TableEditorFocus、selected_column 和独立 Column Details 会话，在现有焦点状态机中实现逐行跨区导航。Columns 使用 Ratatui Table 和轻量共享样式，不调用绑定工作区的完整 data_grid::render。抽取执行确认弹窗的只读 SQL 展示能力，复用 highlight_sql 和主题颜色，并通过现有 UI 视口回传模式同步预览滚动边界。

**Tech Stack:** Rust 2024、Ratatui 0.30.2、Crossterm 0.29、sqlparser 0.62、unicode-width、现有 cargo test 和 TestBackend 渲染测试。

---

## 范围与约束

- 本文是实施计划；编写本文不修改业务代码，不声明任何测试已通过。
- 用户所述 Review Circle / circle editor 按已讨论的 Review SQL / SQL Editor 理解。
- 不修改数据库 DDL 生成、变更执行、权限、连接生命周期或持久化协议。
- 不新增生产依赖，不引入完整的编辑器会话，不重写 SQL 解析器。
- 保留 Enter 审核、e 编辑列、a 添加列、r 恢复列以及取消/放弃确认语义。
- 新增 UI 辅助模块只服务已经存在的复用点，不构建通用表格框架。
- 保留工作区其他变更；已有计划不是源码事实，实施前重新核对实际符号。
- 表格与 SQL 展示改变必须保留 sanitize_terminal_text；不以删除终端安全处理换取显示一致。

## 已确认的接入点

| 文件与符号 | 当前行为及本次影响 |
| --- | --- |
| src/input/keymap.rs:1920 map_table_editor | General 的 Tab/Down 一致，Columns 的 Tab 离区、Down 只移动行 |
| src/model/catalog_editor.rs:1716 move_column | 将列索引限制在首尾，不能跨区 |
| src/model/catalog_editor.rs:1721 focus_next / :1769 focus_previous | Columns 当前是单一停靠点 |
| src/app.rs:4355 CatalogEditorFieldNext | 表格已委托 draft.move_field，无需新增主导航 Action |
| src/ui/catalog_editor.rs:1374 render_table | Paragraph 拼接列行；compact 分支只显示 Name |
| src/ui/data_grid.rs:237 / :364 / :485 | 现有 Table、表头色、竖分隔线、行号与行状态的参考实现 |
| src/ui/catalog_editor.rs:2190 preview | SQL 作为一个原始 Line 插入，然后 Paragraph 自动折行 |
| src/ui/execution_confirm.rs:120 preview_lines | 已有安全投影、高亮、分行、折行、行号渲染 |
| src/ui/mod.rs:2866 editor_syntax_color | EditorHighlightKind 到 SyntaxColor 的映射 |
| src/editor/mod.rs:2202 map_highlight | SQL 分类到编辑器分类的映射 |
| src/ui/theme.rs:132 syntax_color | 语法颜色的统一来源 |
| src/app.rs:5368 / :5385 | Scroll 使用 usize::MAX 上限，End 使用 SQL 逻辑行数 |
| src/runtime.rs:4419 sync_grid_viewport | UI 几何指标回传 reducer 的已有模式 |
| tests/keymap.rs:724 | 明确断言 Tab 不逐行访问，需要替换旧契约 |
| tests/catalog_editor_state.rs:447 | 明确断言 Columns 是单停靠点，需要调整 |

行号仅用于初始定位，以执行时的符号位置为准。

## 交互契约

### 主表单

```text
Name <-> Schema <-> Owner <-> Comment
     <-> Column[0] <-> Column[1] <-> ... <-> Column[n-1]
     <-> Add <-> Remove/Restore <-> Review SQL <-> Cancel
```

1. Tab 和 Down 均为下一项；BackTab、Shift+Tab 和 Up 均为上一项。
2. 从 Comment 向下进入第一列，从 Add 向上进入最后一列，不恢复中间行。
3. Columns 首行向上离区，末行向下离区；主表单首尾保持位置，不循环。
4. 零列时跳过 Columns；删除标记行仍可访问，以便恢复。
5. 主表单不自动进入 Column Details，不改变文字内容，不提交任何操作。
6. 鼠标仍直接选中点击的列；选中后键盘从该行继续移动。
7. Column Details 保留自己的字段循环、确认与取消，关闭后恢复原列选择。
8. Left/Right/Home/End 等文本编辑键保持现有作用；本轮不新增 j/k 导航，以免吞掉输入。
9. 导航到的字段或列必须可见；隐藏内容不能成为不可见焦点。

### Columns 视觉

- 显示行号、NAME、TYPE、NULLABLE、COMMENT，保留独立的变更标记位置。
- 使用现有 grid_header、grid_header_text、grid_border、surface、selection 和变更色。
- 使用与结果表格一致的竖分隔线和选中标记，选择仍以整行为单位。
- 不引入结果单元格高亮、排序、列宽拖拽或结果区快捷键。
- 保持 NULLABLE 的业务含义；删除状态放在行标记/样式中，不用 REMOVED 覆盖 nullable 值。
- 长值按终端显示宽度截断，表头与正文共用同一列宽预算。
- 窄屏优先保障 Name/Type 可辨认，其余字段允许缩短；完整值仍可在详情查看。

### SQL 预览

- 同一 SQL、方言和主题，语法分类及前景色必须与 SQL Editor 一致；行号、选区、背景不要求一致。
- 方言从 plan.request.connection.profile_id 对应的 profile.kind 解析；缺失时使用 Generic，不借用其他活动 Console 的方言。
- 无 plan 时显示普通占位信息，不将错误或提示文本送入 SQL 高亮器。
- 使用安全显示投影计算高亮字节范围，保留缩进、换行与空行；原始 plan.sql() 完全不变。
- 显示行折行后再滚动，不再叠加第二次 Paragraph 自动折行。
- 底部操作提示固定；摘要、警告、SQL 和错误按实际显示行纳入正文滚动，使极小窗口也能访问完整内容。
- Home 回到顶部，End 到最后一屏；到边界后反向一次即可移动，不能积累不可见偏移。

## Task 1: 建立基线与回归清单

**文件：** 只读 Cargo.toml、相关源码、tests/keymap.rs、tests/catalog_editor_state.rs、tests/catalog_editor_reducer.rs、tests/ui_render.rs、tests/mouse.rs。

1. 执行 git status --short，记录并保留现有修改和未跟踪计划。
2. 重新核对上述符号、测试夹具和当前样式，特别确认其他任务是否已调整评论、放弃确认或 SQL 高亮。
3. 运行最相关测试基线：

```bash
cargo test --test catalog_editor_state --test catalog_editor_reducer
cargo test --test keymap table_editor
cargo test --test ui_render table_editor
cargo test --test mouse catalog_editor
cargo test --test sql_highlight
```

4. 记录实际成功/失败；基线已失败时保留输出，区分原有失败与本任务回归，不修改无关逻辑。
5. 执行后续每项时，先增加断言并运行得到行为失败，再实现；编译错误不能当作需求已复现。

**完成标准：** 基线结果和待替换旧契约明确；不需要连接或修改真实数据库。

## Task 2: 实现连续焦点状态机

**修改：** src/model/catalog_editor.rs。
**测试：** tests/catalog_editor_state.rs、tests/catalog_editor_reducer.rs。

1. 将 table_column_focus_is_a_single_stop_and_rows_move_separately 调整为逐行导航测试，保留独立 move_column 方法测试中仍有效的夹取语义。
2. 用已有 new/add/confirm 夹具构造三列，记录 (focus, selected_column) 的有效状态，断言从 Name 正向到 Cancel，再逆向返回的完整序列。
3. 增加从 Comment/首行、末行/Add 的四个边界断言，以及首尾不循环断言。
4. 增加零列、单列、删除标记行和过期选中索引的安全性断言。
5. 运行 cargo test --test catalog_editor_state table_，确认新增断言在旧行为下失败。
6. 在 focus_next/focus_previous 的 Columns 分支中先移动行，只有首尾才换区；入区按方向选首/末行。
7. 维持现有 focus 与 selected_column，不构建动态焦点数组，不新增主导航 Action。
8. 对空列表与越界索引使用安全分支；不修改 column_editor 内的循环行为。
9. 在 reducer 测试中验证 FieldNext/Previous 委托新规则，导航不改变 draft 业务数据、dirty summary 或 Column Details 会话。
10. 运行 cargo test --test catalog_editor_state --test catalog_editor_reducer，期望所有相关断言通过。

**完成标准：** 模型层已经能独立完成整条双向路径，且详情编辑的原子确认/取消不回归。

## Task 3: 统一键位入口与提示

**修改：** src/input/keymap.rs、src/ui/catalog_editor.rs；src/help.rs 仅在存在本次相关旧描述时修改。
**测试：** tests/keymap.rs、tests/catalog_editor_reducer.rs。

1. 替换 table_editor_tab_leaves_columns_without_visiting_each_row 的旧契约。
2. 增加键位矩阵：General、Columns 首/中/末行、Action 中，Tab/Down 返回同一 Action，BackTab/Shift+Tab/Up 返回同一 Action。
3. 分别覆盖 KeyCode::BackTab 和带 SHIFT 的 KeyCode::Tab，保留现有 normalize_shift_tab 的终端兼容处理。
4. 增加完整路径集成测试：同一夹具逐次 map + update，比较 Tab 与 Down 的状态轨迹、反向键的状态轨迹。
5. 运行 cargo test --test keymap table_editor，确认旧键位导致的新断言失败。
6. 在主表单共用分支处理前后导航，移除 Columns 中上下键到 MoveTableColumn 的特殊映射；子弹窗路由仍优先。
7. 检查 MoveTableColumn 的鼠标等调用者，保留仍需要的行移动能力，不因键位移除顺手删除公开 Action。
8. 更新 Columns 底部提示为统一前后导航，不再区分 move row 与 move focus；保留 e/a/r 和审核提示。
9. 验证 Enter/Space 操作按钮、Enter 审核、e 详情、Esc 放弃确认、左右光标和文本历史快捷键未改变。
10. 运行 cargo test --test keymap --test catalog_editor_reducer，期望通过。

**完成标准：** 所有主表单区域四组键共享同一状态机，无键盘跨区阻塞。

## Task 4: 改造 Columns 表格并保障焦点可见

**新增：** src/ui/grid_style.rs，仅提取实际被两处使用的表头、分隔及行选择样式函数。
**修改：** src/ui/mod.rs、src/ui/data_grid.rs、src/ui/catalog_editor.rs。
**测试：** tests/ui_render.rs、tests/mouse.rs、src/ui/data_grid.rs 内部测试。

1. 在现有 table_editor_column_list_has_headers_and_bounds_wide_values 基础上增加 Buffer 颜色、竖线对齐、行号、选中标记断言；不要只比较字符串。
2. 增加新增/删除行样式测试，明确活跃行仍有可辨认的选择反馈和状态标记。
3. 增加包含中文、组合字符、超长类型/评论的列值；确认表头、分隔线、正文位置一致，且不会越出弹窗。
4. 增加 compact 下 Name/Schema/Owner/Comment 逐项获得焦点后的可见文本和 hit region 断言。
5. 运行 cargo test --test ui_render table_editor，确认样式/焦点可见性新断言失败。
6. 提取小型共享样式函数，先替换 data_grid 中等价的样式构造，不改变结果表格宽度、状态、排序或命中逻辑。
7. 将 render_table 的 Columns 部分改为 Table/Row/Cell；表头、数据、分隔符用同一 Constraint 集合，去掉整行 format! 对齐。
8. 列宽先扣除选中标记、行号、变更标记和全部分隔线，再分配四个字段；对 0 宽和不足一个完整表头的区域做安全处理。
9. 原有选中行可见窗口算法先保持最小改动，但根据新表头和表格区域重新计算容量；不要引入结果区 DataGridState。
10. 焦点在 Columns 时启用行选择；在 General/Action 时不绘制活跃选择，保留 selected_column 供按钮操作使用。
11. 在 compact 布局中，General 有焦点时显示对应字段，其他时候显示 Name；修改标签、输入框与鼠标区域必须一起进行。
12. 按实际表格正文区域生成 CatalogEditorTableColumn(index)，不生成 ResultCell/GridColumnSort 或修改 UiState.grid_viewport。
13. 验证三种尺寸：106x34、80x24、60x16；另加极小区域的无 panic、区域不越界测试。若操作行在支持尺寸中不够宽，使用现有 compact 标签并按实际可用宽度折成操作行，不允许焦点落到完全不可见按钮。
14. 运行以下回归：

```bash
cargo test --test ui_render table_editor
cargo test --test mouse
cargo test --lib ui::data_grid
cargo test --test ui_render data_grid
```

**完成标准：** Columns 和预览区共享视觉来源，行选择/滚动/点击正确，未污染主工作区表格状态。

## Task 5: 抽取共享只读 SQL 高亮展示

**新增：** src/ui/sql_preview.rs。
**修改：** src/ui/mod.rs、src/ui/execution_confirm.rs、src/ui/theme.rs。
**测试：** src/ui/sql_preview.rs 内部测试、src/ui/theme.rs 内部测试、tests/ui_render.rs。

1. 将 execution_confirm::preview_lines 的显示行为先固化为测试：行号、空行、缩进、CRLF、Tab、控制字符、多字节字符、长行折行。
2. 设计唯一的展示入口，接收 SQL、SqlDialect、实际内容宽度和 Theme，返回拥有文本的 Vec<Line<'static>>；执行确认与 Review 均采用相同的行号规则。
3. 把现有逻辑迁入新模块，保持执行确认现有可见行为；其标题/摘要/按钮不进入共享模块。
4. 将 HighlightKind 到 SyntaxColor 映射集中在主题侧的小型函数，执行确认与新预览不各自维护 match。
5. 保留 EditorHighlightKind 映射的现有分层，通过覆盖全部分类的测试验证两条映射输出一致，不扩大为编辑器类型重构。
6. 对文本先 sanitize、规范化换行、投影 Tab，再调用 highlight_sql；不得先高亮原文后替换字节。
7. 完整文本只高亮一次后分显示行，以维持跨行注释/字符串语义；不要逐行独立解析。
8. 折行按终端单元宽度，并把行号占用纳入预算；续行的行号槽留空。对极小宽度明确隐藏行号或安全裁剪，保证不产生超宽输出。
9. 相邻同样式文本合并成 Span；通过有序范围游标匹配 token，避免逐字符扫描全部高亮范围。
10. 对解析不完整或未知语法保留既有 lexical/plain 降级，不能阻止 SQL 阅读。
11. 执行 cargo test --lib ui::sql_preview、cargo test --lib ui::theme、cargo test --test ui_render execution_confirmation，期望通过。

**完成标准：** 执行确认迁移后无行为回归，新模块无需 EditorWorkspace，会保留终端安全和多行高亮。

## Task 6: Review SQL 接入高亮与正确方言

**修改：** src/ui/catalog_editor.rs；如需小型 profile 方言解析 helper，放在 src/app.rs，避免持久化新增字段。
**测试：** tests/ui_render.rs、src/ui/sql_preview.rs 内部测试；必要的内部跨渲染测试放 src/ui/mod.rs。

1. 增加 Review 的 CREATE TABLE、ALTER TABLE、DEFAULT、COMMENT SQL 渲染测试，精确检查关键字、表名、列名、类型、字符串和数字前景色。
2. 增加目标 profile 与活动 Console 不同的测试；用反引号、方括号或 PostgreSQL 特有字符串语法验证实际使用目标方言。
3. 增加目标 profile 缺失的 Generic 降级测试，以及 no plan 的普通占位测试。
4. 运行 cargo test --test ui_render catalog_preview，确认旧纯文本实现失败。
5. 让 preview 从父 render 获得明确方言参数；从 plan.request.connection.profile_id 解析目标 profile，不调用基于 active_profile 的 App::sql_dialect 代替。
6. 用共享显示行替换 Line::raw(sql)，保持 target、changes、destructive warning、applying、error 的语义和样式。
7. 在内部测试中比较同一 SQL 的 Editor snapshot 分类经 editor_syntax_color 转换后的颜色与只读预览颜色；不把光标、当前语句背景纳入比较。
8. 断言渲染前后 plan.sql() 完全一致，没有触发格式化、SQL 执行或数据库请求。
9. 执行 cargo test --test ui_render catalog_preview、cargo test --test sql_highlight、cargo test --lib editor::tests，期望通过。

**完成标准：** Review 显示高亮且使用正确连接方言；现有高亮器不因本次接入而新增 SQL 语义规则。

## Task 7: 按显示行同步 Review 滚动边界

**修改：** src/ui/catalog_editor.rs、src/ui/mod.rs、src/model/catalog_editor.rs、src/action.rs、src/app.rs、src/runtime.rs。
**测试：** tests/catalog_editor_state.rs、tests/catalog_editor_reducer.rs、tests/ui_render.rs。

1. 增加多语句/长行折行的测试，要求 End 能看到末尾，随后 Up 一次便向上移动一行。
2. 增加顶部/底部反复滚动、Home、PageUp/PageDown、窗口缩放、错误/警告加入导致正文高度变化的测试。
3. 运行相关测试确认旧逻辑行数上限或无限偏移问题失败。
4. Review 将摘要/警告/错误按各自样式转成真实显示行，与共享 SQL 显示行组合；固定底部 footer，正文区域不用 Wrap 再折行。
5. 使用 usize 计算并切片可见行后渲染，不将无限大偏移强转 u16。
6. 在 UiState 中增加仅供当前 Review 的视口观测值，最小携带 plan 请求身份、总显示行数和可见行数；每次绘制清空旧值。
7. 在 runtime 中沿用 sync_grid_viewport 模式回传；新增专用视口 Action，reducer 只接受仍匹配当前 plan 的数据，忽略表单页、已关闭弹窗和过期请求。
8. 在 CatalogEditorState 保存非持久化滚动上限，收到指标后 clamp preview_scroll；仅在指标变化时触发状态更新，避免持续重绘反馈。
9. Scroll 和 End 使用同步后的真实上限；Home 为零。新 plan 到达和返回/重新审核时按既有重置语义清理指标，不继承旧 plan 的边界。
10. 检查 busy 状态下滚动/取消的现有规则，本次不放开执行中关闭或重复提交。
11. 执行 cargo test --test catalog_editor_state --test catalog_editor_reducer --test ui_render，期望通过。

**完成标准：** 显示行是唯一滚动计量；无需 usize::MAX 哨兵；旧视口不能影响新的审核计划。

## Task 8: 综合回归与人工验收

**测试：** 所有受影响测试；不新增真实数据库依赖。

1. 执行格式和差异检查：

```bash
cargo fmt --check
git diff --check
```

2. 执行完整相关集：

```bash
cargo test --test catalog_editor_state --test catalog_editor_reducer --test keymap --test ui_render --test mouse --test sql_highlight
cargo test --lib
cargo clippy --all-targets -- -D warnings
```

3. 环境允许时执行 cargo test；外部数据库用例如因缺少配置跳过，明确记录，不声称验证了在线变更。
4. 在可用的安全开发连接中打开编辑表格，分别用 Tab/Shift+Tab 与上下键走完整条路径；主流程仅停留在 Review，未经额外授权不按 Enter 应用 DDL。
5. 人工检查多列滚动、删除标记恢复入口、e 打开详情后 Esc 返回、compact 字段可见性及鼠标点击。
6. 在相同主题下比较 SQL Editor 和 Review 的同一 SQL；检查中文评论、长默认表达式、多个语句、警告与错误。
7. 检查 End/Up、Home/Down 与窗口缩放后内容末尾和 footer 可达性。
8. 无交互终端或安全测试连接时，以 TestBackend 覆盖并明确标注人工验收未执行。
9. 检查 git diff --stat 和 git diff，确认没有依赖/锁文件/数据库执行语义的意外变化，没有覆盖其他任务文件。
10. 汇报改动文件、实际运行的命令、结果、未验证项；不自动提交或推送。

## 执行顺序与风险控制

1. Task 1 -> Task 2 -> Task 3，先让行为契约成立。
2. Task 4 在新导航上保证可见性与鼠标一致。
3. Task 5 -> Task 6 -> Task 7，先稳定共享高亮，再接入 Review，最后完成真实视口滚动。
4. Task 8 做跨模块回归。多个任务共享 catalog_editor.rs、ui/mod.rs 和 ui_render.rs，默认顺序执行，不让多个写代码代理并发改这些文件。

| 风险 | 对策 |
| --- | --- |
| 更新旧测试时掩盖既有回归 | 只替换单停靠点契约，保留编辑确认/取消、文本历史与业务数据不变断言 |
| 新 Table 侵占列表和操作区 | 容量按真实表头/正文计算，覆盖正常/compact/极小尺寸和末行 |
| 共享样式影响主结果表格 | data_grid 仅做等价样式提取，保留其独立排序、选中单元格和鼠标测试 |
| 其他连接的方言污染 Review | 以 plan 的 profile_id 为来源，测试与活动 Console 不同的情况 |
| 清理文本造成高亮错位 | 高亮安全显示投影，按 UTF-8 边界和显示宽度分片 |
| 预览视口回传污染新计划 | 带请求身份、绘制时重置、reducer 校验、plan 切换重置 |
| 计划与其他同日改动冲突 | 每项开始重新定位符号，保留已完成能力，不按旧行号覆盖 |

## 最终验收清单

- [ ] Tab/Down 正向轨迹一致，Shift+Tab/Up 反向轨迹一致。
- [ ] General、每一列和操作按钮连续可达，主表单首尾不循环。
- [ ] 零列、删除行、子弹窗和 compact 焦点处理正确。
- [ ] Columns 使用预览表格的表头、竖分隔、行号和选择视觉。
- [ ] 中文/长值/窗口缩放下布局、选中行与鼠标命中一致。
- [ ] Review 和 Editor 的高亮分类及语法前景色一致。
- [ ] 方言来自目标连接，显示投影安全，原始 SQL 不变。
- [ ] 长 SQL 能滚动到末尾，End 后反向移动立即生效。
- [ ] 结果表格、执行确认和列详情编辑未回归。
- [ ] 已记录实际测试结果、基线失败与未执行的人工验收。
