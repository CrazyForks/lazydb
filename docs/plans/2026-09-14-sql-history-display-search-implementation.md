# SQL History Display and Search Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. If that skill is unavailable, follow the ordered tasks and verification gates below directly.

**Goal:** SQL History 左侧显示无行号高亮预览，右侧持续显示带行号和语法高亮的只读 SQL，搜索支持正常文本编辑、可见光标及 Esc 返回列表。

**Architecture:** 保留 Browse/Search/Sql 三态；列表复用可配置的 SQL 预览，详情复用独立只读 editor session。搜索复用 TextInput 和共用单行按键映射，使用现有 overlay_id/query_generation 请求身份确保最新输入对应最新结果。只读编辑器统一计算行号栏和正文区域，将快照、光标、选区和滚动命中保持在同一坐标系。

**Tech Stack:** Rust 2024 / Rust 1.94、Ratatui 0.30.2、Crossterm 0.29、现有 modalkit 封装、TextInput、SQL 高亮及主题系统、Cargo tests。

---

## 一、实施依据与范围

本计划承接 `docs/plans/2026-09-14-sql-history-modal-implementation.md`，以当前源码为实施基线。已有计划中对旧 History Tab 的描述不能视为当前实现。

已确认问题：

| 位置 | 根因 | 修复方向 |
| --- | --- | --- |
| `src/ui/sql_preview.rs::styled_chunk` | 无条件添加行号和续行缩进 | 新增显式行号选项，历史列表关闭 |
| `src/app.rs::sql_history_editor_snapshot` | `Generic` 方言且传入空 SQL 高亮范围 | 全文高亮接口 + 记录所属方言 |
| `src/ui/read_only_sql.rs::ReadOnlySqlEditor::render` | 只画正文，没有行号栏 | 可选固定 gutter，正文独立滚动 |
| `src/ui/sql_history_modal.rs::render_detail` | 仅 Sql 模式显示 SQL | 显示与焦点解耦 |
| `src/input/keymap.rs::Keymap::map` | 搜索字符要求 modifiers 为空 | 使用现有单行文本编辑映射 |
| `src/ui/sql_history_modal.rs::render_header` | 普通 Paragraph，无光标注册 | 使用 render_text_input |
| `src/input/keymap.rs::map_paste` | History 命中 overlay 通用拒绝分支 | Search 专用整段粘贴 Action |
| `src/app.rs::load_sql_history_overlay` | loading 时直接返回，丢失新搜索请求 | 首页新查询允许替换，分页仍防重复 |

## 二、交互与验收契约

1. 列表每条仍为最多三行 SQL 预览和元数据；无行号、无为行号保留的空列；保留 token 颜色及选中背景。
2. 有选中记录时，右侧在 Browse/Search/Sql 均显示对应完整 SQL；Enter 聚焦详情，Sql 中 Esc/Tab 沿用现有返回 Browse 语义。
3. 详情使用 1-based 绝对行号。垂直滚动后行号对应源文件行；横向滚动时行号固定。
4. Search 中 `/jftry`、大写、空格、引号、括号、百分号、下划线及中文均为输入文本；浏览快捷键不触发。
5. Search 支持 Backspace/Delete、Left/Right/Home/End 及已有 TextInput 单行编辑快捷键，包括撤销/重做。
6. Enter/Esc 结束输入，保留当前查询和筛选；再次在 Browse 按 Esc 关闭弹窗。
7. 仅 Search 显示输入条形光标；仅 Sql 显示详情块光标；Browse 不显示详情光标。每帧只允许当前焦点拥有光标。
8. 内容变化才发搜索请求；光标移动、无效删除及退出搜索不重复加载。一次粘贴最多发一次请求。
9. 快速连续输入的最新查询最终生效；旧成功、旧失败和旧分页响应不能覆盖新查询。
10. SQL 原文用于复制，显示投影与行号不进入复制内容。

## 三、任务与执行步骤

每项按“行为回归测试 → 确认复现 → 最小实现 → 定向验证”推进。下面步骤可拆为约 2～5 分钟的编辑或检查单元；编译耗时单独计算。提交信息用于实施后的逻辑分组，是否提交由执行时的用户指令决定。

### Task 0：确认工作区和测试基线

**Files:**
- Reference: `Cargo.toml`、`.github/workflows/ci.yml`。
- Reference: `tests/sql_history_interaction.rs`、`tests/keymap.rs`、`tests/ui_render.rs`。
- Reference: `docs/plans/2026-09-14-help-sql-history-crash-fix.md`。

**步骤：**
1. 执行 `git status --short`，记录已有改动及未跟踪计划；按明确路径处理本任务文件。
2. 检查实施时适用的 AGENTS.md，并按符号定位当前实现，行号只作参考。
3. 执行 `cargo +1.94.0 test --test sql_history_interaction`，记录基线；预期现有用例通过。
4. 确认 UI 测试使用的 TestBackend、主题及 cursor/hit-region 断言方式，复用现有 fixture。

### Task 1：统一搜索编辑与粘贴入口

**Files:**
- Modify: `src/action.rs`。
- Modify: `src/input/keymap.rs::Keymap::map`、`map_paste`。
- Modify: `src/app.rs::update` 的 History 搜索分支。
- Test: `tests/sql_history_interaction.rs`、`tests/keymap.rs`。
- Reference: `src/model/text_input.rs`。

**步骤：**
1. 在真实 `Keymap::map → App::update` 链路添加测试，输入带 SHIFT 的 `SELECT`、`_\"()%` 和无修饰键中文，断言搜索值完整。
2. 添加 `/jftry` 输入测试，断言 History 模式保持 Search，筛选不变化，不发复制/刷新等浏览操作。
3. 添加中间插入、Delete、Home/End、撤销/重做及 Esc 后查询保留测试；测试光标编辑不增加加载命令。
4. 执行 `cargo +1.94.0 test --test sql_history_interaction`；新输入/编辑测试应复现当前缺陷。
5. 在 Action 中增加 `SqlHistorySearchEdit(TextInputEdit)` 和 `SqlHistorySearchPaste(String)`；旧 Insert/Backspace/Clear 调用归并到统一应用路径，先检查所有调用者再决定删除旧变体。
6. Search 先处理 Esc/Enter，再使用 `map_single_line_text_input_edit(event)` 映射为统一编辑 Action。它已支持 SHIFT、删除、导航、撤销及重做，不再维护另一套字符分支。
7. Search 输入路由置于全局 Omni 打开快捷键之前；已有 Omni 实例仍优先处理自身输入。Browse 的既有快捷键及普通字符开始搜索行为保留，普通字符同样接受 SHIFT。
8. App 应用编辑前保存旧值，调用 `view.search.apply(edit)`，比较前后字符串，仅在内容变化时加载。不能把“光标移动成功”当成“查询变化”。
9. `map_paste` 在 overlay 通用拒绝分支前识别 History Search，一次产生一个 Paste Action；其他 History 模式保持只读。
10. 粘贴复用 `TextInput::paste`，沿用项目单行输入规范化策略；核实其当前换行处理并用含 CRLF、Tab、中文的测试固定行为。不要逐字符派发加载。
11. 添加自定义普通字符 Omni 绑定测试，确认 Search 的字符仍被输入框消费；Ctrl/Command 快捷键沿用单行编辑语义。
12. 运行 `cargo +1.94.0 test --test keymap --test sql_history_interaction`，预期通过。

**提交分组：** `fix(sql-history): route search through text input editing`

### Task 2：修复搜索异步一致性

**Files:**
- Modify: `src/app.rs::load_sql_history_overlay`、History 加载结果分支。
- Reference/必要时 Modify: `src/model/sql_history_view.rs`。
- Test: `tests/sql_history_interaction.rs`。

**步骤：**
1. 添加确定性测试：发送查询 `S`，不返回响应，继续输入 `E`；断言第二个命令 search 为 `SE` 且 generation 更大。
2. 添加乱序成功测试：先返回 `SE`，再返回 `S`，结果必须仍为 `SE`。
3. 添加旧失败、旧分页响应及关闭重开 overlay 测试，旧请求不得修改当前 items/loading/error。
4. 执行定向测试，确认 loading 条件能复现查询丢失。
5. 将 loading 限制收窄为分页防重：`append && (view.loading || view.next_cursor.is_none())` 时返回；新首页查询始终 `begin_query()` 并注册新请求。
6. 保持 `overlay_id + generation + in_flight` 校验，只有被接受的响应才更新状态和详情 session。
7. 保持实时查询；先利用现有 generation 实现正确性。去抖、任务取消需要性能证据后再评估，不在此引入额外调度状态。
8. 运行 `cargo +1.94.0 test --test sql_history_interaction` 和 `cargo +1.94.0 test --lib sql_history_view`，预期全部通过。

**提交分组：** `fix(sql-history): keep search results aligned with latest query`

### Task 3：渲染搜索输入框及分模式提示

**Files:**
- Modify: `src/ui/sql_history_modal.rs::render`、`render_header`。
- Reference: `src/ui/mod.rs::render_text_input`。
- Test: `tests/ui_render.rs`。

**步骤：**
1. 添加 Search 空输入的 UI 测试，断言光标位于 `Search: ` 后；输入中文和超长查询后光标仍在搜索区域内。
2. 修改 `render_header` 接收 UiState，分配独立搜索区域和状态/事务区域，防止长查询覆盖筛选标签。
3. Search 调用 `render_text_input(frame, search_area, "Search: ", &view.search, style, state)`；Browse/Sql 显示安全投影后的查询摘要，空查询显示 all SQL。
4. 窄窗口优先保证输入区域和光标；状态标签按可用空间截断或另行分配，所有 Rect clamp 到父区域。零宽高时不设置光标。
5. 页脚按模式显示实际支持的操作：Browse 包含 `/ search`；Search 包含 `Enter confirm`、`Esc back`；Sql 显示现有导航/返回操作。
6. 确保详情未聚焦时不覆盖搜索光标，Browse 不遗留上一帧光标。
7. 执行 `cargo +1.94.0 test --test ui_render sql_history`，预期输入光标、长文本与窄屏用例通过。

**提交分组：** `fix(sql-history): render focused search input and contextual hints`

### Task 4：列表预览支持关闭行号

**Files:**
- Modify: `src/ui/sql_preview.rs`、`src/ui/sql_history_modal.rs::render_list`。
- Reference: `src/ui/execution_confirm.rs`、`src/ui/catalog_editor.rs`、`src/ui/mod.rs` 的 preview 调用。
- Test: `src/ui/sql_preview.rs` 内部测试、`tests/ui_render.rs`。

**步骤：**
1. 新增 `lines_with_options` 和小型 `SqlPreviewOptions { show_line_numbers: bool }`；现有 `lines` 作为保留行号的兼容入口，默认行为明确。
2. History 列表调用关闭行号的入口；避免通过移除第一个 Span 来隐藏行号。
3. 新入口以总可用宽度为参数，先扣动态 gutter，再按 Unicode cell 宽度折行；无行号时 gutter 为零。
4. 对旧 `lines` 调用检查原来的 width 是否为正文宽度；兼容 wrapper 保持旧宽度语义，避免此次顺带改变其他确认框的折行结果。
5. 关闭行号时续行也不添加行号缩进；源行高亮 byte offset 继续基于规范化后的文本计算。
6. 验证两行中文 SQL、长行折行及带注释 SQL：无行号输出不留空列、token 颜色存在；默认入口仍显示行号。
7. 执行 `cargo +1.94.0 test --lib sql_preview` 和 `cargo +1.94.0 test --test ui_render sql_history`。

**提交分组：** `fix(sql-history): show compact SQL previews without line numbers`

### Task 5：详情全文高亮及选中记录联动

**Files:**
- Modify: `src/app.rs::sql_history_editor_snapshot`、History Select/Move/Loaded/OpenDetail 分支。
- Modify: `src/ui/sql_history_modal.rs::render_detail`。
- Reference: `src/model/sql_history_view.rs::loaded_execution_id`。
- Test: `tests/sql_history_interaction.rs`、`tests/ui_render.rs`；私有 snapshot 断言可放 `src/app.rs` 内部测试。

**步骤：**
1. 添加右侧颜色测试，限定断言区域为详情正文，避免被左侧已有高亮误导。验证 SELECT、字符串、注释的主题颜色。
2. `sql_history_editor_snapshot` 改用 `render_snapshot_with_dialect`，方言通过选中 item.profile_id 调用 `sql_history_dialect` 获取；缺失连接沿用既有降级。
3. 提取 `sync_sql_history_editor` 私有方法：读取选中记录 ID/SQL；ID 与 loaded_execution_id 一致且 session 有效时返回，否则对显示文本执行安全处理、打开只读 session 并记录 ID。
4. 在有效 Loaded、Select、Move 后调用同步；OpenDetail 仅确保同步后切换焦点。无选中记录时清理或关闭旧 session 并清空 loaded ID，详情显示空状态。
5. 同一记录仅切换 Browse/Sql 不重开 session，保留滚动位置和选区；切换记录重置到新 SQL 开头。
6. 在任意模式有选中项时渲染同一个 ReadOnlySqlEditor，`focused` 仅在 mode == Sql 时为 true。
7. 关闭弹窗沿用既有 session 清理路径；确认持续预览增加的 session 同样能被释放。
8. 测试首次加载即可预览、A/B 切换、空结果、同一记录 Enter/Esc 不重置位置，以及旧响应不改变当前详情。
9. 执行 `cargo +1.94.0 test --test sql_history_interaction` 和 `cargo +1.94.0 test --test ui_render sql_history`。

**提交分组：** `fix(sql-history): synchronize highlighted SQL details with selection`

### Task 6：只读详情增加固定行号栏

**Files:**
- Modify: `src/ui/read_only_sql.rs`、`src/ui/sql_history_modal.rs::render_detail`。
- Modify as needed: `src/ui/mod.rs` 的 ReadOnlySqlEditor 调用和 viewport/hit-region 注册。
- Reference/Modify as needed: `src/input/mouse.rs` 的只读 editor 命中处理。
- Test: `src/ui/read_only_sql.rs` 内部布局测试、`tests/ui_render.rs`。

**步骤：**
1. 为 ReadOnlySqlEditor 增加 `show_line_numbers` 配置，History 启用，其他构造点显式保留原显示选项。
2. 抽取同一套布局计算：外部 block.inner → 行号 gutter → 正文 Rect；gutter 为总行数位数加分隔空格，并在极窄区域退让正文空间。
3. 在请求 snapshot 前取得总行数元信息以计算准确正文 viewport；优先复用现有 editor 元信息。若没有轻量元信息接口，增加局部接口，避免为计算宽度每帧做两次全文高亮快照。
4. History 当前 viewport 使用固定减 2、包含上边框高度的近似值，改为来自实际正文 Rect 的宽高。绘制与编辑器 viewport 更新必须复用相同几何信息。
5. 每一可见源行在 gutter 绘制 `line.line + 1`，按动态宽度右对齐、使用 theme.muted；正文沿用 editor_line_spans。
6. 正文独立应用 horizontal_offset；行号不参加滚动，也不进入复制或选区内容。
7. cursor_screen_cell 加正文原点；register_text_selection_target、mouse_selection_cells、滚动条和拖动命中使用同一正文区域。点击行号不应被误判为正文第一个字符。
8. 验证总行数 9/10/99/100、纵向滚动、水平偏移、中文长行及不足一列的窗口，无越界或 panic。
9. UI 集成测试断言：水平滚动前后 gutter 相同；选区、光标落在预期正文 cell；复制仍是 SQL 原文。
10. 运行 `cargo +1.94.0 test --test ui_render`，并运行共用只读组件相关的已有鼠标/选区测试；预期通过。

**提交分组：** `feat(sql-history): add fixed line numbers to SQL details`

### Task 7：集成验收与最终检查

**Files:**
- Test: `tests/sql_history_interaction.rs`、`tests/keymap.rs`、`tests/ui_render.rs`。
- Review: 本任务实际改动文件与本计划。

**步骤：**
1. 构造含多行、注释、中文、长行、空行的历史记录进行 TUI 验收。
2. 宽屏检查左右联动；窄屏检查上下布局、搜索区域以及输入光标位置。
3. `/` 后连续输入 `SELECT "中文" FROM users WHERE name LIKE '%A_B%'`，检查大写及符号；输入 `/jftry` 确认未触发浏览快捷键。
4. 测试中间编辑、粘贴、快速连续输入、Enter/Esc，再按 Esc 关闭。
5. 进入详情，检查行号、高亮、纵横滚动、鼠标选择与复制；切换记录后检查 SQL、方言和选中项一致。
6. 检查执行确认、DDL 等现有 SQL 预览/只读界面。
7. 执行与 CI 一致的检查：

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
git diff --check
```

预期：格式和差异检查无输出，Clippy 无警告，测试通过。环境依赖导致无法运行的用例需准确记录命令和原因；不能将跳过写成通过。

8. 审查 diff，确认改动仅涉及已列出的行为及必要公共接口适配，记录实际验证结果。

## 四、依赖与交付判定

执行顺序：Task 0 → 1 → 2 → 3 → 4 → 5 → 6 → 7。

- Task 1/2 完成后，搜索文本和异步结果具备一致性。
- Task 3 完成后，搜索光标、输入和退出形成完整交互。
- Task 4 完成后，左侧满足无行号要求。
- Task 5/6 完成后，右侧满足持续预览、方言高亮、固定行号及正确选区要求。
- 最终交付必须同时通过输入行为、请求乱序、UI cell/颜色/光标和真实终端四类验证。

本计划编写阶段仅新增此文档。代码修复及上述验证命令由实施阶段执行。
