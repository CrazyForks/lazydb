# SQL History Modal Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. If that skill is unavailable in the execution environment, follow the ordered tasks and verification gates below directly.

**Goal:** 将 SQL History 从工作区 Tab 改为 SQL 优先展示的大尺寸弹窗，提供高亮主列表、联动执行详情，以及与 DDL 一致的只读 SQL 选择和复制体验。

**Architecture:** 使用 `Overlay::SqlHistory(SqlHistoryState)` 管理临时界面，沿用历史存储和查询模型。列表采用轻量缓存高亮预览，右侧使用一个独立只读 editor session；抽取 DDL 的必要公共渲染能力。请求以弹窗实例、查询代次和分页请求身份关联，输入按列表、搜索、SQL 三种模式分发。

**Tech Stack:** Rust 2024 / Rust 1.94、Ratatui 0.30、Crossterm 0.29、现有 modalkit 编辑器封装、Tokio、SQLite/sqlx、现有 SQL 高亮与主题系统。

---

## 1. 范围与交互约定

本次交付：弹窗迁移、SQL 高亮主列表、右侧详情及全文、键鼠选择复制、搜索/现有筛选、分页、加载错误与重试、自适应布局。

“进入编辑区域”定义为聚焦只读 SQL 编辑器，与 DDL 保持一致。历史文本不可修改；本次不增加 Console 打开、SQL 执行、自动格式化、历史归并、删除历史或存储字段扩展。

### 布局

- 弹窗约占可用终端 94% 宽、90% 高，尺寸始终 clamp 到终端边界。
- 内部区域：标题、搜索/筛选栏、主内容、状态/快捷键栏。
- 宽屏主内容按左 60% / 右 40% 分配，右侧目标最小 36 cells；主内容宽度不足 92 cells 时采用上下布局。
- 终端高度不足以同时显示列表和详情时采用单面板：Browse 显示列表，Sql 显示详情/全文，Enter 和 Esc 切换。具体阈值由布局函数和尺寸测试固定。
- 列表每项 1～3 行 SQL + 1 行弱化元数据。预览溢出用省略标记；不修改原始 SQL。
- 右侧元数据压缩为 5～7 行，其余留给 SQL；小尺寸优先保留 SQL，较长详情允许独立滚动。
- 选中样式以背景及标记呈现，保留语法 token 前景色。

### 按键契约

| 上下文 | 输入 | 行为 |
| --- | --- | --- |
| Browse | j/k、↑/↓ | 切换记录，保持选中项可见 |
| Browse | PgUp/PgDn、Home/End | 当前已加载列表的视口翻页/首尾定位，尾部可触发下一页 |
| Browse | Enter | 聚焦当前 SQL，窄屏切换到详情 |
| Browse | y | 复制完整原始 SQL |
| Browse | / | 进入搜索编辑 |
| Browse | f/t | 沿用状态/事务筛选入口，栏内显示当前条件 |
| Browse | r | 刷新；错误状态下重试失败的请求 |
| Browse | Esc | 关闭弹窗 |
| Search | 普通字符、Backspace、Delete、光标/词编辑 | 使用 TextInput 编辑搜索，不触发列表快捷键 |
| Search | Enter | 立即提交当前搜索并返回 Browse |
| Search | Esc | 结束输入并保留当前条件，返回 Browse |
| Search | Ctrl-U | 清空搜索，沿用 TextInput 的现有语义 |
| Sql | 编辑器导航、v/V、y | 使用现有只读编辑器的移动、选择、复制语义 |
| Sql | Esc | 有编辑器选择/待完成模式时先退出该模式，否则返回 Browse |
| Sql | Tab | 清理临时选择并返回 Browse |
| 鼠标 | 单击列表项 | 选中并聚焦列表 |
| 鼠标 | 单击/拖动 SQL 区 | 聚焦 SQL 并复用编辑器光标/选区操作 |
| 鼠标 | 滚轮 | 按命中区域滚动列表、元数据或 SQL |

双击列表进入 SQL 仅在现有点击识别设施支持时接入，不新增通用双击框架。弹窗外点击不触发底层编辑器、Tab 或面板操作。

## 2. 已核实的代码基础

- `src/app.rs`：OpenSqlHistory 当前创建 History Tab；SqlHistoryOpenDetail 打开 TextDetail；加载结果依赖 active_tab；失败未清除 loading。
- `src/model/history_tab.rs`：已有选中 ID、list_offset、查询 generation 和筛选字段。
- `src/ui/sql_history.rs`：固定列纯文本渲染，仅展示 SQL 第一行，没有呈现选中状态和偏移。
- `src/ui/mod.rs`：已有 History 行点击区域注册，但按每项一行计算；需迁入实际列表布局逻辑。
- `src/ui/relation.rs::render_ddl_editor`：已有快照、高亮、鼠标选择、滚动条与光标，但绑定活动 Relation Tab，且光标要求 overlay 为空。
- `src/app.rs::active_ddl_editor_snapshot`：按记录所属连接解析 SQL 方言的可参考实现。
- `src/ui/sql_preview.rs`：已有预览高亮、换行与终端显示处理；当前逐字符生成 Span，不宜原样用于全部历史每帧重算。
- `src/persistence/sql_history.rs`：HistoryPageRequest 已支持 cursor/search/status/transaction/database；HistoryPage 有 next_cursor，现有界面未消费。
- `ExecutionHistory` 有执行状态、确定性、事务结果、行数、连接 ID、库和 Schema，没有错误正文和持久化连接名称。
- 当前工作区快照显式排除 History Tab；删除枚举时核实该路径，预计无需迁移持久化格式。

## 3. 开始实施前的基线

1. 检查 `git status --short`，记录用户已有改动；实施时按仓库约定准备功能分支或 worktree。
2. 再读取适用 AGENTS.md；按执行时的当前源码确认本计划的符号位置。
3. 运行 `cargo test --test sql_history_interaction --test sql_history_store --test sql_history_errors`。
4. 记录现有 DDL 键盘选择、鼠标拖选、滚动条行为，作为复用后的对照。
5. 若依赖/系统驱动阻塞测试，记录实际错误；不可把未执行的检查标记为通过。

测试以用户可观察行为、状态竞态和文本准确性为主；无需对简单字段访问或纯转发函数逐项编写镜像测试。

## Task 1: 定义弹窗状态和状态转换

**Files:**
- Create: `src/model/sql_history_view.rs`
- Modify: `src/model/mod.rs`
- Test: `tests/sql_history_interaction.rs`

**步骤：**
1. 定义 `SqlHistoryMode::{Browse, Search, Sql}`，与底层 `Focus` 分开。
2. 定义 `SqlHistoryState`：instance_id、mode、selected_execution、list_offset、TextInput 搜索、现有筛选、items、query_generation、next_cursor、in_flight、error、editor_session_id、loaded_execution_id、详情滚动偏移。
3. 将加载区分为替换列表和追加页面；请求身份包含 instance_id、generation、cursor，禁止同一个分页重复并发提交。
4. 添加选中项校正：刷新保留存在的 execution_id；旧项不存在选首项；空列表清空选中项和代码区。
5. 添加模式切换和边界行为测试：空列表 Enter 无副作用；切换记录重置详情滚动；查询切换重置分页。
6. 运行 `cargo test --test sql_history_interaction`，预期所有状态转换用例通过。

**完成标准：** 状态不依赖 active_tab，不把终端坐标或高亮颜色放入持久化历史模型。

## Task 2: 原子迁移打开入口、Overlay 与异步回包

**Files:**
- Modify: `src/model/workspace.rs`, `src/model/tab.rs`, `src/model/mod.rs`
- Modify: `src/app.rs`, `src/action.rs`, `src/runtime.rs`
- Modify: `src/ui/mod.rs`, `src/ui/sql_history.rs`, `src/input/keymap.rs`
- Remove after migration: `src/model/history_tab.rs`
- Test: `tests/sql_history_interaction.rs`, `tests/sql_history_errors.rs`

**步骤：**
1. 先增加打开/关闭行为测试：不改变 Tab 数量、active_tab、底层 Focus 和 Console 文本；不再发出历史打开导致的 workspace 保存命令。
2. 增加 `Overlay::SqlHistory`，修改 OpenSqlHistory 为打开弹窗；已有其他模态框时遵循项目现有覆盖规则，不擅自替换确认框。
3. 统一关闭路径，释放只读 session，并使该实例未完成的请求失效；同一历史弹窗重复打开不创建额外 session。
4. 将 LoadSqlHistory / Loaded / LoadFailed 及 runtime 接口贯通实例 ID、generation、cursor；所有失败路径返回同一请求身份。
5. 将回包定位改为当前历史 Overlay 实例。成功和失败均验证身份；失败结束加载并保存可重试请求。
6. 将历史渲染移入 render_overlay，先提供可编译的基础列表和关闭提示。
7. 删除 WorkspaceTab::History、TabKind::History 及相关布局、标题、图标、快照、输入分支；更新原 Tab 测试构造。
8. 添加竞态测试：关 A 开 B 后 A 的成功/失败被忽略；搜索旧 generation 被忽略；失败结束 loading；底层活动 Tab 不影响回包归属。
9. 运行 `cargo check` 和 `cargo test --test sql_history_interaction --test sql_history_errors`。

**完成标准：** 历史可作为弹窗打开关闭，所有枚举分支编译通过，旧回包无法污染新实例。

## Task 3: 提取 DDL 可复用的只读代码区

**Files:**
- Create: `src/ui/read_only_sql.rs`
- Modify: `src/ui/mod.rs`, `src/ui/relation.rs`, `src/app.rs`
- Test: `tests/ui_render.rs`, `tests/mouse.rs`

**步骤：**
1. 从 render_ddl_editor 提取仅负责绘制的公共函数，显式接受 area、Block、session_id、snapshot、focused、Theme、UiState。
2. 保持快照生成在调用方；公共组件不读取活动 Relation Tab，也不自行判断 overlay 是否存在。
3. 复用 editor_line_spans、register_text_selection_target、mouse_selection_cells 和 render_editor_scrollbars。
4. 将 DDL 原路径切换为公共组件；保留原先光标显示条件、样式和滚动行为。
5. 依据现有 ui_render/mouse 测试风格，补充独立 session 命中与焦点光标的必要回归断言。
6. 运行 `cargo test --test ui_render ddl` 和 `cargo test --test mouse`；确认前一命令实际匹配到 DDL 用例。

**完成标准：** DDL 行为不回退，公共组件可在 Overlay 内正常显示光标，不引入第二套选择逻辑。

## Task 4: 完成 SQL 主列表与响应式详情布局

**Files:**
- Modify: `src/ui/sql_history.rs`, `src/ui/sql_preview.rs`, `src/ui/mod.rs`
- Create: `tests/sql_history_ui.rs`

**步骤：**
1. 实现集中式布局函数，返回弹窗、搜索栏、列表、元数据、SQL、footer 的 Rect，渲染与鼠标共用。
2. 添加 160×48、100×30、80×24、40×12、20×5 尺寸渲染测试；断言无越界、内容优先级和面板切换行为。
3. 实现每条 1～3 行高亮 SQL + 元数据行；长行按显示 cells 裁切，省略号不进入原始文本。
4. 列表滚动以明确的视觉行偏移或布局索引实现；保存每项起止行，保证选择可见和 PgUp/PgDn 行为稳定。
5. 从实际可见布局注册整条记录的点击区域，覆盖 SQL 行与辅助信息行；不沿用旧的一项一行命中公式。
6. 右侧渲染时间、连接、库、Schema、状态、耗时、返回/影响行数、事务与确定性。缺失值显示 —，零显示 0；连接删除时显示可辨认的 profile ID 回退信息。
7. 显示首次加载、无历史、无匹配、加载下一页、失败重试等状态；追加加载时保留现有列表。
8. 按 Browse/Search/Sql 输出不同 footer 提示和焦点边框。
9. 运行 `cargo test --test sql_history_ui`。

**完成标准：** SQL 占据列表主要面积；选中状态可见且保留高亮；窄终端仍能进入完整 SQL。

## Task 5: 接通 SQL session、键盘和鼠标选择复制

**Files:**
- Modify: `src/app.rs`, `src/action.rs`, `src/input/keymap.rs`, `src/input/mouse.rs`
- Modify: `src/ui/sql_history.rs`, `src/ui/mod.rs`
- Test: `tests/sql_history_interaction.rs`, `tests/sql_history_ui.rs`, `tests/mouse.rs`

**步骤：**
1. 选中项变化时，仅更新该弹窗一个只读 session；相同记录重复渲染不重设文本。刷新相同 ID 但 SQL 变化时也更新文本。
2. 新增历史快照入口，根据 item.profile_id 对应连接解析方言；找不到时 Generic，不使用底层活动连接作为默认方言。
3. SqlHistoryOpenDetail 改为进入 Sql 模式，不再调用 OpenTextDetail。
4. 在全局导航和普通文本输入前分发 Overlay 键盘事件；接入 ReadOnlyEditorKey 和编辑器 yank effects。确认弹窗期间粘贴不会落入底层 Console。
5. 接通鼠标 session 识别、选区 revision 校验、坐标映射、滚动条和滚轮；按区域消费事件，阻止底层命中。
6. 实现分层 Esc：编辑器内部选择/待完成状态 → Sql 普通状态 → Browse → 关闭；每次只退一层。
7. 保留列表 y 的完整原始 SQL 复制；代码区选择复制使用现有编辑器映射，不混入行号、元数据或省略号。
8. 添加行为测试：Enter 不新建 Overlay；j/k 按模式分发；v/V/y 的实际 clipboard payload；切换记录清除旧选区；中文/emoji/tab/CRLF/超长 SQL 选取边界。
9. 运行 `cargo test --test sql_history_interaction --test sql_history_ui --test mouse`。

**完成标准：** 只读代码区达到 DDL 的选择复制效果，弹窗焦点与底层工作区互不串扰。

## Task 6: 搜索、防抖与完整分页

**Files:**
- Modify: `src/model/sql_history_view.rs`, `src/app.rs`, `src/action.rs`, `src/runtime.rs`, `src/input/keymap.rs`
- Test: `tests/sql_history_interaction.rs`, `tests/sql_history_store.rs`, `tests/sql_history_errors.rs`

**步骤：**
1. 将搜索迁至 TextInput 和独立 Search 模式；搜索字符 f/t/j/k/y 均可输入，Backspace 只删一个字符。
2. 输入内容改变立即增加 generation，让旧结果失效；通过 runtime 可取消的延迟任务实现约 150ms 防抖，避免在 UI 线程等待。
3. Enter、清空和筛选变更立即发起查询，取消待触发的旧防抖任务。关闭弹窗中止所属延迟任务。
4. 统一首屏、刷新、筛选和追加请求构造函数，避免现有多处分支重复构建请求。
5. 保存 next_cursor，距已加载尾部约一个视口时请求下一页；同 cursor 只能存在一个 in-flight 请求。
6. 追加结果按 execution_id 去重并保留选中与滚动；next_cursor=None 停止追加。新搜索使旧追加响应失效。
7. 失败重试原来的首屏或追加请求，不把追加失败变成清空列表；刷新同条件尽量保留选中记录。
8. 添加 >100 条记录翻页测试、重复触发测试、搜索途中追加回包测试和暂停 Tokio 时间的防抖测试；按项目可用 fixture 放在对应测试文件或 runtime 内部单元测试。
9. 运行 `cargo test --test sql_history_interaction --test sql_history_store --test sql_history_errors`；运行实际新增 runtime 防抖单元测试的准确名称。

**完成标准：** 可以浏览第 101 条以后的记录，快速搜索不会显示旧结果，失败后可重试且保持上下文。

## Task 7: 高亮缓存、长文本与显示准确性

**Files:**
- Modify: `src/ui/sql_preview.rs`, `src/ui/sql_history.rs`, `src/ui/mod.rs`
- Test: `tests/sql_history_ui.rs`

**步骤：**
1. 将预览高亮/布局缓存放入 UiState 对应区域，而非 durable 历史数据。键包含 execution_id、SQL 内容版本、dialect、预览宽度；缓存有界并在弹窗关闭时释放。
2. 若缓存存储带颜色的 Span，纳入主题身份；优先缓存 token kinds，在渲染时应用当前主题。
3. 只为可见项和少量邻近项构建预览；同记录同宽度的稳定重绘不重复语法分析。
4. 将连续同样式字符合并成 Span，避免把现有逐字符预览逻辑直接用于每帧全列表。
5. 为极长 SQL 限定列表预览处理预算，允许预览采用词法级回退；全文编辑器仍使用完整原文，不截断复制 payload。
6. 添加渲染断言：不同 token 有不同前景色、选中背景未覆盖 token 色、主题和 resize 后缓存更新、控制字符显示使用现有 sanitizer。
7. 用大量历史和一条超长 SQL 做稳定重绘检查；通过测试计数或本地临时测量验证重复高亮被消除，不写不稳定的毫秒级 CI 阈值。
8. 运行 `cargo test --test sql_history_ui`。

**完成标准：** 浏览性能随可见记录而非全部历史数量增长，预览与复制数据严格分离。

## Task 8: 帮助、兼容性与最终验收

**Files:**
- Modify: `src/help.rs`, `src/input/keymap.rs`
- Modify if applicable: `README.md` 中已有 SQL History 使用说明
- Test: `tests/sql_history_interaction.rs`, `tests/sql_history_ui.rs`, `tests/ui_render.rs`, `tests/mouse.rs`

**步骤：**
1. 保留现有 open-sql-history 配置动作名称与全局打开快捷键，更新帮助为弹窗模式及上下文快捷键。
2. 核查所有 History Tab 引用：生产代码不再依赖 WorkspaceTab::History/HistoryTab；既有工作区恢复用例仍通过。
3. 检查 DDL 和 TextDetail 的原有选择、复制、滚动行为；抽取公共组件不扩展到无关区域。
4. 运行 `cargo fmt --check`。
5. 运行 `cargo clippy --all-targets -- -D warnings`。
6. 运行 `cargo test`。前序已通过的定向测试不用在没有新变更时重复执行，最终全套作为集成验证。
7. 手工验收下列场景，记录终端尺寸、步骤及结果。

### 手工验收清单

- 从正在编辑 SQL 的 Console 打开/关闭历史，原文本、光标、Tab 和 Focus 保留。
- 主列表能辨认多行 SQL；单击辅助信息行仍选中正确记录。
- Enter 后光标进入 SQL，键盘选择与鼠标拖选复制结果准确。
- 长行横滚、跨屏纵滚、拖动滚动条均命中代码区。
- Esc 分层退出；底层的执行、编辑、切 Tab 快捷键不会被误触发。
- 搜索包含 j/k/f/t/y 的 SQL，退格、清空、确认均正确。
- 超过 100 条分页，刷新保留选中，错误重试不丢已加载内容。
- 快速输入和关闭重开，旧响应不覆盖新查询。
- 连接已删除、不同数据库方言、Redis 命令、空记录与缺失字段显示合理。
- 小终端、窗口缩放、中文、emoji、超长 SQL、主题切换均可用。
- DDL、TextDetail 选择复制和工作区恢复无回归。

**完成标准：** 全部必要自动检查和手工核心路径通过；阻塞项如实列出。

## 4. 依赖和建议提交边界

执行顺序：Task 1 → Task 2 → Task 3 → Task 4 → Task 5 → Task 6 → Task 7 → Task 8。

允许 Task 4 在 Task 3 完成前准备纯布局，但 session 接入必须依赖公共代码区完成。不要求并行 agent。

仅在用户要求提交时创建 git commit。建议保持每个提交可编译：

1. `refactor(history): move SQL history into an overlay`（Task 1–2）
2. `refactor(ui): share read-only SQL rendering`（Task 3）
3. `feat(history): add highlighted master-detail browsing`（Task 4–5）
4. `feat(history): add debounced search and cursor pagination`（Task 6）
5. `perf(history): cache visible SQL previews`（Task 7）
6. `docs(history): document modal navigation`（Task 8 的帮助与说明）

## 5. 最终交付物

- SQL History 大尺寸模态弹窗和响应式布局。
- 高亮 SQL 主列表、联动详情和 DDL 风格只读代码区。
- 完整 SQL / 选区复制、正确的键鼠焦点、搜索与分页。
- 请求竞态、错误恢复、渲染与 DDL 回归的自动验证。
- 更新的帮助说明和实际执行过的检查结果。
