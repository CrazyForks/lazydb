# Redis Value Table Interaction Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行适配：本任务由自动工作流安排阶段；如果该技能不可用，按本文顺序和检查点实施。禁止启动子 Agent；分支命名、worktree 和状态由插件管理。本文列出的新增接口和测试名是实施约定，不表示它们已存在。

**Goal:** 修复 Redis Value Table 的横向滚动条和 `y` 复制，并增加与 Relation WHERE 样式一致、通过 `/` 聚焦及 Enter 提交的关键词过滤框。

**Architecture:** 继续复用共享 DataGrid、TextInput 与 WriteClipboard。RedisBrowserTab 持有独立的提交式过滤状态，由统一 RedisTable 投影向渲染、导航、复制和详情提供相同可见行。Value 外层分配互不重叠的 FILTER、Grid 和状态区域，过滤仅处理已加载内容。

**Tech Stack:** Rust 1.94 / edition 2024、现有 Ratatui/Crossterm、Redis typed pages、TextInput、App Action/Command reducer、TestBackend 与 Rust 集成测试；无需新增依赖。

---

## 0. 基线、范围和执行纪律

- 依据：同目录 `analysis.md`，本阶段已完整读取。
- 原工作空间：`/Users/yelog/workspace/tui/lazydb`。
- 目标：`main`；起点及 plan 阶段复核的 HEAD：`714eadc269b96b440950f5fe53d5942061665d5b`。
- 当前 `git status --short --branch`：`## main...origin/main [ahead 3]`，工作区无修改。
- plan 阶段仅写本计划和完成回执；下述业务编辑、测试与提交均供后续实施阶段执行。
- 在插件提供的工作目录实施，先检查工作区和该目录的 AGENTS.md。不能因任务分支当前为空而自行命名、建 worktree 或改 state.json。
- 按 Task 1→8 顺序执行，每项内按“行为断言→确认失败原因→最小实现→定向测试→复核”推进。每个编号步骤是一项独立操作；较大的场景表逐行实施。
- 测试重点是已确认缺陷、输入优先级和过滤后的行身份一致性；不用测试复刻内部算法或为简单视觉封装增加机械单测。
- 检查点提交仅在后续工作流允许提交时执行；否则保留同样的逻辑变更分组。此 plan 阶段不提交业务变更。
- 新增 Action、HitTarget 或 InputSelectionTarget 的当步必须同步处理所有穷尽匹配，使该步可编译；例如 Task 5 新增 HitTarget 时，提前完成 Task 6 对应的最小鼠标焦点分支，Task 6 再完善选择手势和行为验证。不能将穷尽匹配编译错误误判为预期功能测试失败。

## 1. 产品契约

### 横滚与复制

1. 存在隐藏且可横向浏览的数据列时，显示 Relation Data 同款列级滚动条；复用共享 Grid 的轨道、thumb、点击、拖拽和列宽逻辑。
2. 状态条与横滚条不共享坐标，无横滚时状态条也不能覆盖最后数据行。
3. 单元格超过自动列宽的省略仍遵循 Relation Data 现有规则，不新增字符级横滚。单列长值的完整内容通过复制和现有详情访问。
4. Preview Table 浏览时，单次 `y` 产生 `CopyGridCell` 并复制完整当前单元格；不复制视觉省略字符串。二进制沿用现有无损转义文本。
5. Stream Fields 是字段数量派生列，复制该数量；不能因为 identity 只有 ID 就复制失败，也不能把计数写进原始 identity。

### FILTER

1. Table 表头上方常驻全宽 `FILTER` 字段，输入一行、下划线一行，使用 WHERE 相同图标/标签/背景/聚焦 accent/光标规则。
2. Preview Table `/` 聚焦 FILTER；Keys `/` 仍搜索 Keys，文本 Preview `/` 仍交给只读 Vim。
3. draft 编辑不改变结果。Enter 将 draft 提交为 applied 并返回 Grid；Esc 恢复 applied 并返回 Grid；空字符串 Enter 清除过滤。
4. 匹配当前 key 所有已加载行的任一完整显示数据列，Unicode lowercase 后按字面子串比较；保留关键词首尾空格，不做 SQL/正则/glob/分词，不跨列拼接。
5. 不匹配行号、标题及未显示的 Stream 嵌套内容。List Index、ZSet Score、Stream Fields 计数属于数据列。
6. 二进制匹配已有转义表示；新查询不触发网络读取。过滤后状态显示 `matched / loaded` 和原 continuation 状态，零匹配仍可显式加载更多。
7. 点击 FILTER 编辑，点击 Grid 取消未提交 draft 后选择单元格；切 pane/Tab/格式结束本次编辑，保留 applied。再次 `/` 从 applied 开始。
8. 新查询选择首个可见行并重置 row_offset，保留列选择及用户列宽。追加/同 key 刷新尽力按 row_key 保留选择，消失则夹紧；不同 key/清空预览重置过滤。
9. 格式切到文本时 applied 不影响文本；回到同 key Table 恢复过滤。各 Redis Tab 状态独立且不持久化。
10. Overlay/Omni 优先于 FILTER；FILTER 编辑优先于数字前缀、pending、help、Keys find、Grid 和只读编辑器。`123y/hjkl?fz` 必须能作为普通输入。

## 2. 实施接口约定

### 模型

在 `src/model/redis_browser.rs` 使用以下最小状态（字段名作为计划约定）：

```rust
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RedisValueFilterState {
    pub draft: crate::model::text_input::TextInput,
    pub applied: String,
    pub editing: bool,
}
```

RedisBrowserTab 增加 `value_filter`。统一 `preview_table() -> Option<RedisTable>`：从 Ready page 调用 from_page，再用 applied 过滤；Tab 之外的消费者不各自重建过滤规则。底部 loaded 数量从未过滤页面计算，不使用 filtered rows 冒充 loaded。

在 `src/value_preview/table.rs` 增加以下完整的纯过滤方法：

```rust
impl RedisTable {
    pub fn filtered(mut self, keyword: &str) -> Self {
        if keyword.is_empty() {
            return self;
        }
        let keyword = keyword.to_lowercase();
        self.rows.retain(|row| {
            row.cells.iter().any(|cell| cell.to_lowercase().contains(&keyword))
        });
        self
    }
}
```

完整单元格访问分清来源：raw 列使用 `source_cell` + 已有 `display_bytes_lossless`；派生列使用完整 cells 文本。建议增加返回借用枚举的 `cell_content(column)`，枚举分 `Source(&[u8])` / `Derived(&str)`；在 App 侧转 CellValue/详情，避免 value_preview 新增对 UI 模块的依赖。越界返回 None，不能退回另一列。

### Action / 命中目标

在 `src/action.rs` 添加语义动作：

```rust
RedisValueFilterFocus { tab_id: Uuid },
RedisValueFilterEdit(crate::model::text_input::TextInputEdit),
RedisValueFilterPaste(String),
RedisValueFilterSubmit,
RedisValueFilterCancel,
```

Ui HitTarget 与 InputSelectionTarget 均新增 `RedisValueFilter(Uuid)`。带 tab_id 的焦点和鼠标动作必须验证活动 Tab 身份；一般编辑动作只作用于当前 Results/Preview/Table 的 editing 状态。

### 选择锚点

追加前读取当前投影选中行的 row_key，合并后在新投影查找；同 key 刷新会在 open_key 中把 Ready 换成 Loading，故必须在替换前暂存 row_key。可在 Tab 增加 `preview_restore_row_key: Option<Vec<u8>>`；新 key/clear 清除，成功回填恢复后消费，失败留待同 key 重试。新查询提交清除旧锚点，避免后来回包把选择恢复到旧过滤语义。

---

## Task 1：建立过滤及完整单元格数据契约

**Files**
- Modify: `src/value_preview/table.rs`（RedisTable、RedisTableRow）。
- Modify/Test: `tests/value_preview.rs`。

**步骤**
1. 在既有 from_page 测试附近添加 `redis_table_keyword_filter_preserves_source_identity`：两条 Hash 行，只有后者 Value 含 `Needle`；过滤 `needle` 后断言列不变、只剩后者且 identity/row_key 原样保留。
2. 添加表驱动场景：空查询、零匹配、中文、首尾空格、跨列不匹配、字面 `.*`、二进制转义及 List Index；期望值直接写出，不调用被测 helper 生成。
3. 运行 `cargo test --test value_preview redis_table_keyword`。新增接口未落地前应失败；确认原因是功能缺失而非 fixture 错误。
4. 实现上面的 filtered 方法及 `cell_content` 来源访问；保留 from_page 的现有列形状和 identity。
5. 添加 `redis_table_cell_content_covers_derived_stream_fields`：Stream 两个字段时 Fields 返回 `Derived("2")`，ID 为源字节，越界 None。
6. 运行 `cargo test --test value_preview`，期望全部通过。

**复核**：没有截断后再匹配、修改源数据、跨列拼接或改变 Stream raw identity。**验收**：所有表格类型均有可解释的过滤/完整复制来源。

## Task 2：Value 过滤状态和 reducer

**Files**
- Modify: `src/model/redis_browser.rs`（Tab 字段/new、preview_table）。
- Modify: `src/action.rs`（新增动作）。
- Modify: `src/app.rs`（update 新分支及非 Console 守卫）。
- Create/Test: `tests/redis_value_filter.rs`。

**步骤**
1. 创建 Ready Hash App fixture，参考 `tests/keymap.rs` 的 Redis Tab 构造及 `tests/ui_render.rs::redis_table_status_exposes_loaded_state_and_load_more_target` 的 RedisValuePage。fixture 接受行集合，以 App::new(Vec::new()) 构造无 Console 的真实使用场景。
2. 添加 `redis_value_filter_submit_is_local_and_deferred`：Focus、Paste 后可见行不变，Submit 后筛选且返回空 Command 集合；draft/applied/焦点状态分别断言。
3. 运行 `cargo test --test redis_value_filter redis_value_filter_submit_is_local_and_deferred`，确认未实现状态/动作失败。
4. 添加状态和 preview_table；Focus 将 draft 设置为 applied 并置 editing；Edit 调用 TextInput::apply；Paste 调用 TextInput::paste；Submit 保存原样 draft、editing=false、重置垂直选中与视口后夹紧；Cancel 恢复 draft 并结束 editing。
5. 审计 App::update 的无 Console 动作守卫，允许合法 Redis 过滤动作通过；在 reducer 校验焦点/视图/Tab。无 Ready page 时允许编辑查询，结果保持空且不发起 I/O；后续 Ready 使用 applied。
6. 添加 `redis_value_filter_cancel_and_clear`、错误 tab_id/非 Redis/非编辑态动作无效、空表和保留列宽用例。
7. 运行 `cargo test --test redis_value_filter`，期望通过。

**复核**：无 query SQL 状态耦合，查询不调用 Redis 命令；state 不进入持久化。**验收**：draft 与 applied 真正分离，空查询可恢复全部数据，无 Console 时可工作。

## Task 3：统一可见行、复制、详情和网格尺寸

**Files**
- Modify: `src/app.rs`（active_grid_dimensions、active_record_snapshot、redis_preview_cell_detail、RecordView 相关 Redis 读取点）。
- Modify: `src/ui/redis_browser.rs`（Table 投影来源）。
- Modify/Test: `tests/redis_value_filter.rs`。

**步骤**
1. 添加 `redis_value_filter_visible_row_drives_copy_and_detail`：至少 9 条数据，仅源索引 8 匹配；过滤后选中可见索引 0，调用 CopyGridCell、详情/RecordView 路径。
2. 运行 `cargo test --test redis_value_filter redis_value_filter_visible_row_drives_copy_and_detail`，确认现有源索引访问导致错误值或数量。
3. 将 Redis UI、尺寸、快照、详情入口统一接 `tab.preview_table()`。Grid 中的行索引始终是可见索引，复制快照按照列迭代并通过 cell_content 构造完整 CellValue；不要按 identity.len() 迭代漏掉派生列。
4. 原始列详情保留自动格式识别和原字节 clipboard 文本；派生字段计数直接用完整文本创建只读详情。
5. 检查 GridMove/GridSelect/viewport 回写及 RecordView 总行数都来自可见行数；0 行复制沿用现有 Nothing to copy 提示且无 WriteClipboard。
6. 添加完整值复制表驱动场景：长 JSON、空文本、中文、tab/newline、非 UTF-8、List Index、ZSet Score、Stream Fields；只检查 Command payload，不写真实系统剪贴板。
7. 运行 `cargo test --test redis_value_filter` 和 `cargo test --test redis_browser_tabs`，期望通过。

**复核**：搜索 src/app.rs 和 Redis UI 的 from_page 调用，剩余使用须明确是未过滤 loaded 计数/元信息，不能是可见行读取。**验收**：画面、选择、y 后端、详情来自同一逻辑行。

**检查点 A（后续允许提交时）**
`git add src/value_preview/table.rs src/model/redis_browser.rs src/action.rs src/app.rs src/ui/redis_browser.rs tests/value_preview.rs tests/redis_value_filter.rs`

`git commit -m "feat(redis): add submitted value table filtering"`

## Task 4：接通 `y`、`/` 和完整输入优先级

**Files**
- Modify: `src/input/keymap.rs`（map、map_paste、网格焦点谓词、pending 检查）。
- Modify/Test: `tests/keymap.rs`, `tests/redis_value_filter.rs`。

**步骤**
1. 扩展 `redis_table_preview_routes_motion_to_the_shared_grid`，断言 y→CopyGridCell，/→RedisValueFilterFocus；以 Ready fixture 补一次实际 Keymap→App→WriteClipboard 集成断言。
2. 运行 `cargo test --test keymap redis_table_preview`，确认 y 和 / 的现有错误映射。
3. 增加 `is_redis_table_grid_focus`（Results、Redis、Preview、Table、非 filtering editing），将其纳入共享只读网格能力。检查扩大谓词对配置导航/z/count 的影响；若入口需要不同条件，显式使用同一个 Redis 谓词组合，避免复制多个不一致条件。
4. 在 Overlay/Omni 接管之后、help/数字/pending/网格之前接管 FILTER editing：Enter Submit，Esc Cancel，其余交给 map_text_input_edit，未识别键也结束分流，不能继续落到后台。
5. Table 浏览态 `/` 在 Redis Keys find 分支之前处理；Keys find 的 editing/confirmed 输入只在 Keys pane 生效。进入 FILTER 后清除失效 pending，不能让同一个全局 Results 焦点残留序列消费新输入。
6. map_paste 保留已有 Overlay/Omni 优先级，随后把过滤编辑内容作为单个 RedisValueFilterPaste，保证一次粘贴为一个 undo 原子操作；原文通过既有安全显示投影渲染，不默默 trim。
7. 添加表驱动键盘测试 `redis_value_filter_input_owns_printable_keys`：`123y/hjkl?fz`、Shift 字符、左右/Home/End/Delete、Ctrl-W 删除词、undo/redo；输入均不产生后台动作。测试 Keys confirmed find 与 FILTER 同时存在时的 n/N/Esc。
8. 验证 Keys y/、文本 Preview y/、Overlay/Omni、SQL/Relation query 和 Dashboard read-only keymap；运行 `cargo test --test keymap --test redis_value_filter`。

**复核**：必须通过真实 map 分流测试，不仅直接发送 Action。**验收**：单次 y 工作，/ 焦点准确，编辑输入不被全局快捷键截获。

## Task 5：FILTER 视觉与布局修复

**Files**
- Modify: `src/ui/query_bar.rs`（最小字段 renderer 可复用接口）。
- Modify: `src/ui/redis_browser.rs`（filter/grid/status 布局、状态计数、cursor）。
- Modify: `src/ui/mod.rs`（HitTarget）。
- Modify/Test: `tests/ui_render.rs`。

**步骤**
1. 创建 `redis_table_horizontal_scrollbar_survives_status_render`：Ready Hash 两个长数据列，窄 Preview 使两列不能同时显示。断言最终 Buffer thumb/rail 存在且与状态文字不在同一行。
2. 运行 `cargo test --test ui_render redis_table_horizontal_scrollbar_survives_status_render`，确认状态覆盖导致失败。
3. 给 query_bar 中现有 render_query_field 提供最小 crate 内复用接口。Redis 使用图标+FILTER 标签、空 highlights、TextInput、水平 offset；抽出公共绘制时保留 SQL 原调用参数及输出。
4. Value 内部按顺序分配 2 行 filter、剩余 Grid、1 行 status。极小区域用安全饱和分配：区域高度不得超过 Value 边界，0 高区域不绘制/不注册命中。Grid 自己保留横滚行，status 放 Grid 外部。
5. Table 模式（包括 Empty/Loading/Failed）渲染 FILTER，数据未 Ready 时剩余区域显示现有加载/空状态。用 enabled/active 区分是否绘制光标，不把旧文本 editor 的 cursor 留在表格上。
6. 使用 `ui.grid_viewport` 当帧指标计算可见行范围；Table 不再依赖文本 preview_scroll 的手算高度。必要时仍输出 redis_preview_viewport_rows，也必须来自相同真实 Grid viewport，不能和 runtime 回写互相振荡。
7. 状态统计：空 applied 保持 loaded 文案；非空显示 matched/loaded，再附 Complete/More available/Loading。0 匹配不显示倒置 range，加载更多目标依据原 page.complete/value_page_loading 注册。
8. 添加 `redis_table_filter_layout_keeps_header_grid_and_status_disjoint`：中文输入、长关键词水平偏移、光标在有效区域；输入 underline accent；无横滚最后行完整；0 匹配保留表头；单列截断保持 Relation 契约。
9. 运行 `cargo test --test ui_render redis_table_`、`cargo test --lib ui::query_bar::tests`、`cargo test --lib ui::data_grid::tests`，期望通过。

**复核**：断言最终 Buffer 与独立坐标期望，不用被测 Layout 返回值推导期望。**验收**：搜索框位于表头上方、风格一致，滚动条/状态/数据没有覆盖。

## Task 6：鼠标焦点和输入选择

**Files**
- Modify: `src/input/mouse.rs`, `src/ui/text_selection.rs`。
- Modify: `src/ui/redis_browser.rs`, `src/app.rs`（输入命中注册与 begin/update/complete selection）。
- Modify/Test: `tests/mouse.rs`, `tests/redis_value_filter.rs`。

**步骤**
1. 新增 `redis_value_filter_click_focuses_input`，通过真实渲染 UiState 点击 FILTER，断言 Preview/编辑态；点击 Grid 单元格断言取消 draft、选中正确可见行。
2. 运行 `cargo test --test mouse redis_value_filter`，确认新增命中链路未实现。
3. 为 HitTarget::RedisValueFilter(tab_id) 添加焦点路由，注册 InputSelectionTarget::RedisValueFilter(tab_id)，沿用 register_input_selection_target 的 prefix/Unicode offset 映射。
4. App 鼠标输入 begin/update/complete 分支对专属目标操作 draft TextInput，校验活动 tab_id；同一个输入内点击/拖动不能每次 Focus 都重置 draft 或撤销历史。
5. 点击格子先退出编辑再执行共享 GridSelect；保留已有 Overlay 命中屏障和拖拽所有权。滚动条命中区必须落在 Grid 内，不能被 PreviewLoadMore 吞掉。
6. 添加 `redis_table_scrollbar_click_does_not_load_more` 及拖拽用例：检查横向 offset/selection 的变化，返回动作不是 RedisPreviewLoadNext；状态行点击仍可加载。补中文光标点击/拖选、stale tab target、Overlay 不穿透。
7. 运行 `cargo test --test mouse redis_` 和 `cargo test --test redis_value_filter`，期望通过。

**复核**：新增枚举的所有匹配分支均已处理；不借用 DataQuery 的身份或动作。**验收**：鼠标和键盘操作同一输入/可见行状态，滚动条可操作。

**检查点 B（后续允许提交时）**
`git add src/input/keymap.rs src/input/mouse.rs src/ui/query_bar.rs src/ui/redis_browser.rs src/ui/mod.rs src/ui/text_selection.rs src/app.rs tests/keymap.rs tests/ui_render.rs tests/mouse.rs tests/redis_value_filter.rs`

`git commit -m "fix(redis): wire value table input and scrollbar layout"`

## Task 7：追加、刷新、切换与帮助收口

**Files**
- Modify: `src/model/redis_browser.rs`（open_key、clear_opened_key、append_value_page）。
- Modify: `src/app.rs`（RedisValuePageLoaded、格式/pane/Tab 切换）。
- Modify: `src/help.rs`（Table/filter editing 上下文）。
- Modify/Test: `tests/redis_value_filter.rs`, `tests/redis_browser_tabs.rs`, `tests/redis_help.rs`。

**步骤**
1. 添加 `redis_value_filter_append_and_refresh_preserve_row_identity`：非首行选中→追加→同 key refresh Loading→响应回填，验证 row_key、applied、列宽保留。
2. 运行 `cargo test --test redis_value_filter redis_value_filter_append_and_refresh_preserve_row_identity`，确认状态生命周期缺口。
3. 在 append 变更前捕获选中 row_key，之后按新投影恢复并夹紧；不更改已有分页去重/预算/请求协议。
4. open_key 同 key 时在丢弃 Ready 前保存恢复锚点，保留 applied 并结束编辑；不同完整 RedisKeyId 时清空过滤/锚点并重置 Grid。clear_opened_key 同样清理。回填只在原有 connection/preview_generation 校验通过后恢复。
5. Submit 新查询和新 key 清理旧锚点；失效回包不消费当前锚点。匹配行消失时选择夹紧；原无匹配变为有匹配时选中首行。
6. pane/Tab/format 切换统一结束编辑并恢复 draft=applied，格式往返不影响文本状态；不同 Tab 不共享 applied。
7. 添加多 Tab、切 key、空结果后 append、过期回包、刷新失败重试、文本/Table 往返用例；断言过滤 Submit 没有 Redis Command，明确 More available 仍由既有加载触发。
8. 帮助增加 Table `/` filter、Enter apply、Esc cancel 提示，editing context 不显示格子复制为当前输入操作；现有 y 提示通过键盘测试支撑。
9. 运行 `cargo test --test redis_value_filter --test redis_browser_tabs --test redis_help`，期望通过。

**复核**：实际源码清空方法名是 clear_opened_key；不要按历史文档另造 clear_preview。**验收**：过滤状态与 row_key 生命周期一致，帮助与真实行为一致。

## Task 8：集中复核与交付验证

**Files**：复核本任务 diff；按失败涉及的文件修复，不引入无关重构。

**步骤**
1. 执行 `git diff --check`，期望退出 0。
2. 执行 `cargo fmt --check`，期望退出 0；如需格式化只处理本任务文件，再复查 diff。
3. 执行 `cargo test --lib ui::data_grid::tests` 和 `cargo test --lib ui::query_bar::tests`，期望测试通过。如果报告 0 tests，核实模块测试名，不能把无匹配视为验证完成。
4. 执行以下集中回归（这是最后一次跨 Task 集成验证）：

```sh
cargo test --test redis_value_filter --test value_preview --test redis_browser_tabs --test redis_help --test keymap --test mouse --test ui_render --test relation_tabs
```

期望所有非忽略测试通过；若共享谓词/UI 的更改涉及 lib 内其他测试，运行 `cargo test --lib` 完成共同调用方检查。外部数据库集成测试不为纯本地关键词过滤额外要求新服务；环境失败须记录具体错误，不伪报通过。

5. 如现成 Redis 连接可用，手工验证 Hash/Set/ZSet/List/Stream：窄 Preview→横滚；移动→y；/→输入→Enter；清空→Enter；filter 0 匹配→LoadMore；切 Keys/Text/返回。没有服务时以 TestBackend 和真实 Action/Command 链路验证，并记录未做 live 手测。
6. 用以下审查表逐项查 diff 与测试证据，全部完成后才交付。

| 验收项 | 主要证据 |
|---|---|
| 隐藏列横滚可见，状态不覆盖 | ui_render 最终 Buffer + mouse 横滚/LoadMore 分离 |
| 单次 y 复制完整当前格 | Keymap→App→WriteClipboard，包括 Stream 派生列 |
| FILTER 常驻表头上方且风格一致 | UI 布局/underline/cursor/中文偏移 |
| / 正确分流、输入不被快捷键夺取 | keymap 输入矩阵、Keys confirmed find、Overlay |
| Enter 才过滤，Esc 取消，空提交清除 | redis_value_filter reducer |
| 所有已加载行、任意数据列字面子串 | value_preview 过滤表驱动测试 |
| 过滤后显示/复制/详情同一行 | 源索引 8→可见索引 0 集成用例 |
| 0 匹配/append/refresh/key/Tab 安全夹紧 | 生命周期测试 + matched/loaded 状态 |
| 无网络提交、副作用和持久化变更 | Commands 断言及 diff |
| Relation/SQL/Dashboard 共享逻辑回归 | keymap/ui_render/relation_tabs/相关 lib 测试 |

7. 执行 `git status --short` 和 `git diff --stat`，确认变更仅属于本任务。交付摘要列实际测试命令/结果、未执行验证原因、行为边界；不改插件状态文件。

**检查点 C（后续允许提交时）**
`git add src/model/redis_browser.rs src/app.rs src/help.rs tests/redis_value_filter.rs tests/redis_browser_tabs.rs tests/redis_help.rs`

`git commit -m "fix(redis): preserve value filter state across preview updates"`

## 3. 风险和实现复核提示

- **共享滚动契约**：本次恢复已存在的列级滚动，不声称任意单格截断都可字符横滚。真正隐藏列用两列长值/窄宽度 fixture 验证，避免自动列宽上限导致测试未制造溢出。
- **双层焦点**：App Focus::Results 不足以识别 Keys/Preview/FILTER，所有关键输入谓词必须同时检查 pane 与 editing。
- **Identity 与展示区别**：Stream Fields 的源字节不存在，不应静默取 ID 或把它当 NULL；派生值显式走 Derived。
- **刷新先丢数据**：open_key 会将 value_page 置 Loading，恢复锚点必须此前保存。不要等 RedisValuePageLoaded 再从旧 Ready 中读取。
- **性能**：沿用按需 from_page；过滤不是每个字符触发重算。避免同一消费者重复构建表格，第一版不增加 ResultSet 缓存/失效框架。
- **测试真实性**：已有 y 帮助和旧计划不代表实现完成。新增测试需观察键盘入口、最终 Buffer、ClipboardPayload 和逻辑行身份。

## 4. 本 plan 阶段交付

本文件已按 analysis 的方案细化文件、依赖顺序、接口、逐项步骤、复核、命令和验收。业务代码尚未实施，本文测试命令尚未运行。无需再次询问执行方式；由插件继续安排实施阶段并调用 Luna 命名任务分支。

本次 plan 重入已重新读取 analysis.md、调用 writing-plans 并完整复核本计划，补充了跨任务枚举穷尽匹配的编译检查要求；工作区仍无业务代码修改。本次完成回执 token 为 `0e38fdc8-34f7-4fca-bc4b-b92a06970d10`。
