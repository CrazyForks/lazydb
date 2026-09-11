# 窗口双轴滚动条统一 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. 若执行环境无该技能，按本计划顺序逐项实施和验证。

**Goal:** 以 RELATION DATA 为视觉和交互基准，统一 Explorer、SQL Editor、OUTPUT LOG、RELATION DDL 的双轴滚动条，支持可靠的鼠标拖动、轨道翻页和滚轮操作。

**Architecture:** 提取轻量滚动条几何与绘制组件，建立携带目标身份的滚轮区域和单一滚动条拖动状态。Grid、Explorer、Editor 各自维护真实偏移和边界，公共组件不接管内容模型；SQL、日志和 DDL 复用 Editor 适配。

**Tech Stack:** Rust 2024 / MSRV 1.94、Ratatui 0.30.2、Crossterm 0.29、modalkit 0.0.25、unicode-width；复用 TestBackend、现有模型测试和鼠标集成测试，无新增依赖。

---

## 1. 范围与交付约定

- 五个窗口双轴使用相同轨道字符、颜色、滑块几何和命中规则。
- RELATION DATA 的公共渲染器同时服务 SQL Result Data 和 Dashboard Processes，迁移后验证这两个调用方。
- Explorer 包括普通树、Find 和 Search 结果；搜索输入框、状态行不随内容滚动。
- 表格滚动范围限于已加载页，不借用数据库总行数，也不由拖动触发分页查询。
- 保留 Grid / Explorer 的现有选择夹取规则；文本窗口鼠标滚动不改变光标、选择或文本，后续键盘导航沿用现有编辑器行为。
- 不引入新持久化格式。滚动目标注册、抓取状态和布局有效性信息都是运行时状态。
- 本文是实施计划。实施时先检查工作区，保留已有改动和计划文件；用户另行授权前不执行 commit / push。

## 2. 已确认事实与代码入口

行号仅用于初次定位，实施前按符号获取最新源码及调用方。

| 文件 / 符号 | 现状及需要处理的问题 |
| --- | --- |
| `src/ui/data_grid.rs::render_scrollbar` / `render_vertical_scrollbar` | 双轴轨道、分页、拖动几何基准；横向端点属于分页命中，纵向端点尚无对应命中 |
| `src/ui/mod.rs::render_editor_scrollbars` | 与 Grid 重复的几何；纵向单字符 Paragraph 与多行滑块区域不匹配；视觉缺少端点 |
| `src/input/mouse.rs::map_mouse` | Grid / Editor 两套拖动；Editor 抓取偏移相对 track 而非 thumb 起点；单轴动作传另一轴 0 |
| `src/input/mouse.rs::focus_at` / `is_relation_ddl_focus` | Editor 滚动条被统一认成 SQL Editor；DDL 检测依赖键盘焦点 |
| `src/ui/mod.rs::render_output` | 已有 output editor 快照、选择映射和 viewport，但没有滚动条 |
| `src/app.rs::with_active_grid` | 只处理 Data / Processes，因此 OUTPUT 错误的 Grid 滚轮通常无效，不能据此声称它必然改动隐藏表格 |
| `src/editor/mod.rs::scroll` / `set_scroll_offset` | 有边界夹取；绝对 setter 同时写入两个轴；宽度计算路径需统一 |
| `src/ui/relation.rs::render_ddl_editor` / `ddl_editor_viewport` | DDL 已接入编辑器条；渲染和 runtime 预同步的视口计算必须保持一致 |
| `src/model/explorer.rs::scroll_nodes` / `scroll_lines` / `body_height_for_scroll` | 现有滚轮滚动及选择夹取；祖先固定行让正文高度随位置、选择变化 |
| `src/model/workspace.rs` 的 Explorer 包装、Find/Search 状态 | 模式切换、搜索滚动、可见行转换入口；不能只改 normalized tree |
| `src/editor/mod.rs::sync_output_viewport` | pending tail 会把纵向设末尾，同时把横向设 0，需要修正横向保留行为 |
| `src/app.rs::sync_output_editor` / `src/editor/mod.rs::set_read_only_text` | 输出追加和只读缓冲更新，必须在旧内容被替换前判断是否跟随末尾 |
| `src/runtime.rs::sync_editor_viewport` / `sync_ddl_editor_viewport` / `sync_output_viewport` / `sync_explorer_viewport` | 视口反馈路径，防止渲染尺寸与模型尺寸不同导致回弹 |

## 3. 统一交互契约

### 3.1 样式与布局

- 横向端点 `‹` / `›`，轨道 `─`，滑块 `━`。
- 纵向端点 `▲` / `▼`，轨道 `│`，滑块 `┃`。
- 轨道及端点 `theme.muted`，滑块 `theme.accent`，背景 `theme.surface`。
- 对应轴溢出且轨道长度至少 3 个终端单元格时显示。否则不注册不可操作的滑块命中；滚轮仍可处理有内容的有效视口。
- 滚动条位于内容右沿 / 下沿，可以复用经过验证的边框或 gutter；不要机械地给每个窗口再次减一行一列。
- 内容区域、两个轨道、SQL 行号、提示行、状态行、表头与右下角交点互不重叠。滚动条命中与面板缩放命中发生冲突时，明确分割区域，滚动条所在单元格优先。
- 隐藏滚动条时不留下上一帧字符或命中区域。

### 3.2 鼠标

| 输入 | 行为 |
| --- | --- |
| 按下滑块 | 捕获相对滑块起点的抓取偏移；偏移不变 |
| 拖动滑块 | 单轴绝对定位；另一轴保持；抓取后允许移出窗口，夹取首尾 |
| 点击滑块前后轨道及端点 | 对应方向一页，端点与 DATA 现有横向语义一致 |
| 内容区上下滚轮 | 纵向 3 行 |
| 原生左右滚轮 | 横向；文本 / Explorer 为 3 个显示单元格，Grid 保留现有按列规则 |
| Shift + 上下滚轮 | 终端上报 Shift 时转换为横向；不依赖该组合才能操作横轴 |
| 横向条上的上下滚轮 | 横向，普通鼠标无需修饰键也可操作 |
| 纵向条上的上下滚轮 | 纵向；Shift 明确请求横向时仍以修饰键为准 |
| 鼠标在非焦点窗口 | 滚动该窗口，不改变键盘焦点 |
| 鼠标在无滚动目标区域 | 不回退到当前键盘焦点误滚其他窗口 |

滚轮方向解析优先级：原生左右事件 > Shift 转换 > 所在滚动条的轴 > 内容区域默认纵向。

### 3.3 生命周期与模型

- 鼠标滚动期间不启动文本选择、列宽调整或 pane resize；已有其他抓取时不接受新滚动条抓取。
- 抓取期间滚轮忽略，避免同时修改同一基准；松开应用最后坐标并清理状态。
- 目标关闭、切换 Tab / ResultView / Explorer 模式、弹层或终端选择模式开启、布局或内容度量变化使抓取失效时取消。
- 仅 offset 改变不能让每次拖动后的重绘取消抓取。有效性比较轨道范围、内容版本、最大偏移和视口尺寸，不比较滑块位置。
- Grid / Explorer 纵向滚动按现有规则夹取选择；Editor 只改 viewport。所有路径都由模型进行最终边界夹取。
- 日志初次输出 / 原来位于底部时追加跟随；用户离开底部后追加保留阅读位置；回到底部恢复；横向滚动不影响纵向跟随，追加不无故归零横向。

## 4. 数据与接口设计

### 4.1 共享概念

在 `src/model/scroll.rs` 放置不依赖 UI 的类型，避免 Action / 模型为滚动逻辑依赖渲染模块：

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScrollAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExplorerScrollView {
    Tree,
    Find,
    Search,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScrollTarget {
    Explorer { view: ExplorerScrollView },
    Grid { tab_id: uuid::Uuid },
    Editor { session_id: uuid::Uuid },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScrollMetrics {
    pub offset: usize,
    pub viewport_extent: usize,
    pub content_extent: usize,
    pub max_offset: usize,
}
```

`max_offset` 显式传入：Grid 横向可变列宽、Explorer pinned rows 都不能由公共组件自行推算。横向文本度量使用终端显示宽度而非字节数或源字符下标。

### 4.2 Action 与适配

在 `src/action.rs` 增加 `SetScrollOffset { target, axis, offset }` 和 `ScrollBy { target, axis, delta }`。公共 UI 负责把轨道翻页转成目标轴上的有界绝对偏移；Grid 横向滚轮继续复用现有目标列解析后发出绝对动作。

旧键盘 Action 可保留作为兼容入口，最终委托同一模型 setter；不要为统一鼠标顺手重写整个键盘导航体系。删除没有调用者的旧鼠标专用 Action，并审查 Action 分类、编辑态与弹层守卫。

### 4.3 几何计算规则

设包含端点的长度为 `L`，轨道有效长度 `R = L - 2`，可见量 `V`、内容量 `C`、最大偏移 `M`：

```text
thumb_length = clamp(floor(R * V / C), 1, R)
travel = R - thumb_length
thumb_position = floor(travel * clamp(offset, 0, M) / M)
drag_position = clamp(pointer - rail_start - grab_offset, 0, travel)
drag_offset = round(drag_position * M / travel)
```

- `L < 3`、`V == 0`、`C == 0` 或 `M == 0` 返回无可操作滚动条。
- 乘法采用 `u128` 中间值，避免大内容长度溢出；除数为 0 时单独处理。
- 正反变换允许终端单元格量化误差，但 offset=0 / M 必须能到对应端点。
- 即使计算中发生边界夹取，按下不移动鼠标时也不应反算出不同偏移；抓取状态保留起始 pointer / offset，首次未移动事件直接保留原值。

## 5. 实施任务

### Task 1：建立共享类型和公共几何组件

**Files:**
- Create: `src/model/scroll.rs`
- Modify: `src/model/mod.rs`
- Create: `src/ui/scrollbar.rs`
- Modify: `src/ui/mod.rs`（模块声明）
- Test: `src/ui/scrollbar.rs` 内单元测试

**步骤：**
1. 获取当前模块导出风格，加入上述纯数据类型。
2. 为 geometry 建立表驱动测试：空内容、恰好一屏、0/1/2/3 长度轨道、首/中/尾位置、超范围 offset、超大 extent。
3. 运行 `cargo test --lib ui::scrollbar`，确认新增行为测试失败原因是实现缺失而非 fixture 错误。
4. 实现 geometry、拖动反算和分页偏移计算；返回同一份 geometry 供绘制和 hit region 使用。
5. 增加双轴 TestBackend 测试：滑块的每一个单元格都真的绘出，端点和轨道区域无重叠，无溢出绘制。
6. 实现公共 renderer，返回语义区域而非在组件内读取 App；绘制参数包含明确的 track Rect。
7. 再运行同一测试命令。

**完成标准：** 计算与绘制只存在一套轴无关算法；视觉与命中使用同一几何结果；不引入内容模型访问。

### Task 2：补齐单轴模型更新和目标有效性校验

**Files:**
- Modify: `src/action.rs`
- Modify: `src/app.rs`
- Modify: `src/editor/mod.rs`
- Modify/Test: `src/model/tab.rs`
- Test: `src/editor/tests.rs`
- Test: `src/app.rs` 内测试

**步骤：**
1. 新增 `scroll_axis_` 测试：初始 row=20 / column=15，分别更新一个轴，断言另一个轴、光标、文本选择和文本不变。
2. 运行 `cargo test --lib scroll_axis_`，确认捕获原来的跨轴归零问题。
3. 在 EditorWorkspace 增加单轴 setter；保留双轴 setter 的明确双轴调用语义，不把 0 偷换成“保持”。统一 scroll / absolute setter 的显示宽度度量。
4. 添加 Action 和 reducer。Grid 必须核对 active tab ID 与实际 Data / Processes 视图；Editor 核对 session 属于当前可见 SQL / Output / DDL；Explorer 核对当前模式。不可见或过期目标 no-op。
5. Grid 适配委托既有 row/column setter，保留编辑态行数、列宽及选择规则。
6. 检查所有 Action guard/classification，滚动不越过模态、编辑提交或事务守卫，也不生成数据库命令。
7. 增加 stale target / hidden view / missing session 测试，验证不误改其他对象；运行 `cargo test --lib scroll_target_` 和 Task 2 的轴测试。

**完成标准：** 单轴绝对更新语义可靠，旧键盘行为仍可用，模型负责最后夹取。

### Task 3：迁移 RELATION DATA，建立公共拖动生命周期

**Files:**
- Modify: `src/ui/data_grid.rs`
- Modify: `src/ui/mod.rs`（HitTarget、UiState、拖动状态与重绘清理）
- Modify: `src/ui/text_selection.rs`（GestureOwner）
- Modify: `src/input/mouse.rs`
- Test: `tests/mouse.rs`
- Test: `tests/ui_render.rs`

**步骤：**
1. 扩展现有 scrollbar 测试，保留原有横/纵轨道和拖动覆盖，新增纵向端点翻页、抓住滑块中部不跳位、释放坐标应用测试。
2. 运行 `cargo test --test mouse scrollbar`，确认新增缺陷用例能失败。
3. Grid 双轴 renderer 改用公共组件；保留实际列宽计算、横轴 max_offset、表头避让和当前页行数。
4. 新增通用 ScrollbarThumb / ScrollbarPage 命中和一个 `ScrollbarDrag`，捕获 target、axis、track、grab_offset、initial pointer/offset 及有效性信息。
5. 在 map_mouse 中实现统一按下、拖动、释放，Grid 先迁入；其他编辑器分支到 Task 5 完成后再删除，避免中间提交无法编译。
6. 注册条命中时避开 pane resize、列宽调整和文本选择区域；鼠标捕获期间采用 owner 判定。
7. 将取消逻辑集中，覆盖 overlay、toast 阻断、终端选择、目标变化、resize；旧 GridEndColumnResize 不再承担滚动条释放。
8. 用有效性 fingerprint 检查重绘后的抓取：单纯滚动不取消，track/extent 改变则取消。Explorer pinned 状态造成的内部几何变化在 Task 8 专门处理。
9. 运行 `cargo test --test mouse scrollbar`、`cargo test --lib ui::data_grid`、`cargo test --test ui_render`。

**完成标准：** DATA 不回归，纵向端点可点击，SQL Result / Dashboard Processes 的共享渲染得到同样能力。

### Task 4：统一滚轮目标区域与轴解析

**Files:**
- Modify: `src/ui/mod.rs`（scroll_regions 和注册/清理）
- Modify: `src/ui/data_grid.rs`
- Modify: `src/ui/relation.rs`
- Modify: `src/input/mouse.rs`
- Test: `tests/mouse.rs`

**步骤：**
1. 在 `UiState` 增加独立滚动区域，包含 Rect、ScrollTarget 和可选 scrollbar axis；每帧重新注册，查询遵循当前可见层级。
2. 为 SQL、Output、DDL、Grid、Explorer 注册明确内容目标；Explorer 适配尚未完成时纵轴保留可用路径，横轴 Task 8 接通。
3. 增加 `scroll_routing_` 测试矩阵：实际 target × keyboard Focus；重点验证 DDL 条、Output 正文、无目标区域、空白内容区域。
4. 运行 `cargo test --test mouse scroll_routing_` 确认旧分发不能满足要求。
5. 把四类 wheel event 归一化为 axis/direction，按交互契约处理 Shift 和横条上的滚轮；Grid 横向仍读取现有左右列目标。
6. 取消 workspace wheel 对 `focus_at(...).unwrap_or(app.focus)` 的依赖；保留 TextDetail、Notification 等现有 overlay 专属行为和阻断规则。
7. 对标题按钮、分页按钮等非内容区域不注册宽泛的穿透滚动目标；不会因点击命中覆盖而误用其他窗口。
8. 验证无焦点变化、无数据库请求；运行 `cargo test --test mouse scroll_routing_` 和 `cargo test --test mouse`。

**完成标准：** 分发由当前可见内容目标决定，横向普通鼠标操作有可靠入口。

### Task 5：迁移 SQL Editor 和 RELATION DDL

**Files:**
- Modify: `src/ui/mod.rs`（SQL 布局、render_editor_scrollbars）
- Modify: `src/ui/relation.rs`（DDL 布局、ddl_editor_viewport）
- Modify: `src/runtime.rs`（Editor / DDL viewport 同步）
- Modify: `src/input/mouse.rs`（删除已无调用者的 Editor 专用拖动）
- Test: `src/editor/tests.rs`
- Test: `tests/ui_render.rs`
- Test: `tests/mouse.rs`

**步骤：**
1. 为 SQL/DDL 建立布局测试：仅纵向溢出、仅横向溢出、双轴溢出、提示行开启、DDL loading/error 状态行、小窗口。
2. 明确 content Rect、gutter、horizontal/vertical track、prompt/status Rect，并共享给快照、绘制、选择映射和 runtime viewport 计算。布局固定求解后再生成最终快照，避免不断请求快照造成闪动。
3. 替换 editor scrollbar renderer 为公共组件，纵向改为逐格 `┃`，双轴补齐端点。
4. SQL 横轨道对齐可滚动文本区域，行号 gutter 固定；保留 prompt 区域。DDL 轨道避让 title/status 并与 runtime 的 `ddl_editor_viewport` 一致。
5. 把 Editor 命中全部迁入 Task 3 的统一拖动；删除 EditorScrollbarDrag、旧 editor hit variants、重复 scrollbar_geometry 以及无调用者的鼠标 Action。
6. 增加鼠标按下、移动、重绘、viewport 反馈、再拖动的完整测试，验证抓取中部不跳、单轴不清零、DDL 不依赖 Focus。
7. 检查宽字符/Tab/横向裁剪下文本选择命中和光标位置；有问题时复用现有 display-cell 投影修正，不扩展为编辑器重写。
8. 运行 `cargo test --lib editor::tests`、`cargo test --test mouse`、`cargo test --test ui_render`、`cargo test --test relation_tabs`。

**完成标准：** SQL / DDL 与 DATA 同风格；拖动错误修复，窗口缩放、提示行和选择不回归。

### Task 6：接入 OUTPUT LOG 和稳定的末尾跟随

**Files:**
- Modify: `src/ui/mod.rs::render_output`
- Modify: `src/app.rs::sync_output_editor` 及其调用方
- Modify: `src/editor/mod.rs::set_read_only_text` / `sync_output_viewport`
- Modify: `src/runtime.rs::sync_output_viewport`（仅在反馈接口需要时）
- Test: `src/editor/tests.rs`
- Test: `src/app.rs` 内输出测试
- Test: `tests/ui_render.rs`
- Test: `tests/mouse.rs`

**步骤：**
1. 新增 `output_scroll_` 用例：底部追加、上滚后追加、回底后追加、横滚后追加、内容清空、隐藏 Output 期间追加、首次输出时未知 viewport。
2. 运行 `cargo test --lib output_scroll_` 确认当前无条件 follow 或横向归零被捕获。
3. 为日志复用 Task 5 的文本滚动条适配；保留日志样式映射与左侧留白，空日志不注册滑块。
4. 在替换旧日志文本前计算旧视口是否位于尾部；新追加是否跟随由该状态决定。复用 pending_tail_scroll 表达尚未完成的尾部定位；仅当旧状态确实不足以表达未知 viewport 时增加运行时 follow 意图字段。
5. 用户主动纵向离开尾部应取消 pending tail，防止下一次 viewport 反馈拉回；横向操作不更改纵向跟随意图。
6. `sync_output_viewport` 跟随时仅更新 row，column 保留并夹取；窗口缩放时重算边界，初始空内容按默认跟随处理。
7. 检查 set_read_only_text 不通过重建光标/选择状态间接强制视口追随；用户阅读历史时追加保持视口。
8. 运行 `cargo test --lib output_scroll_`、`cargo test --lib execution_events_append_via_output_editor_and_follow_tail`、`cargo test --test mouse`、`cargo test --test ui_render`。

**完成标准：** 日志双轴可拖可滚，追加不会打断历史阅读，旧的底部跟随功能保留。

### Task 7：Explorer 模型提供绝对偏移与统一度量

**Files:**
- Modify: `src/model/explorer.rs`
- Modify: `src/model/workspace.rs`
- Modify: `src/action.rs`
- Modify: `src/app.rs`
- Test: `tests/explorer_state.rs`
- Test: `src/model/explorer.rs` / `src/model/workspace.rs` 内测试

**步骤：**
1. 新增 `explorer_scroll_` 测试：绝对定位首/中/尾、选择仍可见时保持、移出时夹取、折叠后夹取、深层祖先固定、极小视口。
2. 新增模型级横向偏移（Tree / Find 共享树内容的偏移；Search 在搜索状态中拥有偏移）；避免在 UiState 保存第二份真实偏移。
3. 抽出正文高度/祖先提示/最大 row_offset 的统一度量入口，让 viewport 和绝对 setter 使用相同规则；现有 `body_height_for_scroll` 与渲染祖先提示的差异一起消除。
4. 用受祖先深度约束的有限收敛过程计算请求 offset、正文高度和选择夹取，检查首尾稳定且不会无限循环。空内容、0 高度及内容不足一屏单独处理。
5. `scroll_lines` 委托绝对 setter；Page/HalfPage 的既有选择导航语义保留。
6. Find 使用真实 tree_area 高度（排除输入和状态），Search 使用真实 result_area；搜索绝对 setter 同步合法选择范围，消除渲染中 selected 对 scroll 的强制回拉。
7. 模式切换、查询变化、目录更新后合法夹取偏移；不要因连接异步载入就无条件归零。
8. 接入 Task 2 reducer 的 Explorer 分支，校验模式匹配，运行 `cargo test --test explorer_state` 和 `cargo test --lib explorer_scroll_`。

**完成标准：** 三种模式都能表达真实双轴位置，拖动到尾部稳定，祖先固定行不会让状态反馈回弹。

### Task 8：Explorer 双轴绘制、裁剪与命中同步

**Files:**
- Modify: `src/ui/mod.rs::render_explorer` / `render_explorer_find` / `render_explorer_search`
- Modify: `src/ui/mod.rs::explorer_list_item` / `register_explorer_row_hits`
- Modify: `src/runtime.rs::sync_explorer_viewport`
- Modify: `src/model/workspace.rs`（需要的视口反馈字段）
- Test: `tests/ui_render.rs`
- Test: `tests/mouse.rs`
- Test: `tests/explorer_performance.rs`

**步骤：**
1. 将树行/搜索行的“构造带样式 Line”与 ListItem 包装拆开，复用同一行内容计算宽度、绘制与命中；避免复制三套文本生成。
2. 横向 extent 包含缩进、展开标记、图标、名称和已显示的 metadata/comment，按当前模式完整展开行集合计算，纵向滚动不改变横向 extent。
3. 先实现正确的宽度计算，使用已有性能测试测量；如每帧遍历带来可观退化，按内容/展开/模式/图标版本缓存 extent，而非按 offset 失效。不为了本任务扫描尚未加载的目录。
4. 使用显示单元格裁剪保留 Span 样式。中文宽字被切到半格时安全留白，源字符串不破坏。
5. 展开按钮位置使用 `content_x + depth*2 - horizontal_offset` 的有符号计算并与 content Rect 求交；不能先 saturating_sub 造成已滚出按钮残留在最左侧。
6. 为 pinned rows 应用同一个横向偏移，保持树列对齐；输入、状态行固定。祖先提示行按可用宽度绘制。
7. 调用公共双轴 renderer 和 scroll region 注册；实际 content 尺寸反馈到模型，重绘后不产生无限同步。
8. 专门验证 pinned rows 变化时连续纵向拖动：拖动使用按下时映射，模型按当前边界夹取；内部 pinned 数量变化不当作外部 resize 取消，结束后用新几何重绘。树结构变化/模式变化则取消。
9. 增加真实 render → mouse → reducer → render 测试：拖动后节点点击/展开仍指向正确 stable ID，Find/Search 可连续拖到末尾。
10. 运行 `cargo test --test explorer_state`、`cargo test --test ui_render`、`cargo test --test mouse`、`cargo test --test explorer_performance`。

**完成标准：** 三种模式横纵都能操作，宽度稳定，树按钮和高亮不因裁剪错位，性能无明显回归。

### Task 9：生命周期、视口反馈和跨窗口回归

**Files:**
- Test: `tests/mouse.rs`
- Test: `tests/ui_render.rs`
- Test: `tests/workspace_tabs.rs`
- Test: `tests/relation_tabs.rs`
- Modify: 前述实现文件，仅用于修复本任务发现的问题

**步骤：**
1. 为五个窗口构建相同数据规模的表驱动测试，分别验证双轴、端点、轨道、拖动和滚轮。
2. 测试抓取后鼠标出界、松开、Tab 切换、Output/Data 切换、Explorer 模式切换、目标关闭、终端 resize 和 pane resize。
3. 测试 toast/overlay、文本选择和列宽拖动的互斥与清理，保证隐藏窗口不会收到滞留动作。
4. 增加只改变 offset 的连续重绘测试，避免每帧更新区域时丢失拖动 owner；内容或外部布局真正变化时取消。
5. 模拟一次完整 runtime viewport 反馈后再次 render，断言内容偏移、滑块位置、命中区域一致，不跳回选中行或光标。
6. 复查 SQL Result Data / Dashboard Processes 和 Relation 编辑态新增行范围。
7. 运行 `cargo test --test mouse --test ui_render --test workspace_tabs --test relation_tabs --test keymap`。

**完成标准：** 集成行为通过，模型/渲染/输入之间无分叉的度量和 stale target 漏洞。

### Task 10：文档、最终检查与人工验收

**Files:**
- Modify: `README.md` 中现有鼠标操作说明，补充悬停滚动、端点翻页、Shift/横条滚轮操作和日志跟随语义
- Modify: 本计划，记录实际验证结果和必要偏差
- Test: `tests/docs.rs`（使用现有检查；不为文案新增镜像测试）

**步骤：**
1. 搜索旧 scrollbar 专用类型/算法/Action 的残留；删除无调用者重复实现，确认旧键盘兼容入口仍委托正确模型。
2. 更新用户操作说明，明确 Shift 取决于终端是否上报，横条滚轮是可用的普通鼠标路径。
3. 运行 `cargo fmt --check`。
4. 运行 `cargo clippy --all-targets -- -D warnings`；若基线存在无关警告，记录并区分，不能自动修复无关代码。
5. 运行 `cargo test --lib`。
6. 运行尚未在最终代码状态验证的受影响集成测试：`cargo test --test mouse --test ui_render --test explorer_state --test explorer_performance --test relation_tabs --test workspace_tabs --test keymap --test docs`。已在相同代码状态跑过且通过的检查不重复执行。
7. 在真实终端进行下列人工验收，记录终端名称、尺寸与 mouse mode；终端层输入兼容性不能只靠合成事件断言通过。
8. 检查 `git diff --check` 与最终 diff，列出改动、通过检查及未完成项。

**人工验收清单：**
- [ ] 五个窗口：横向/纵向/双轴溢出时外观一致，未溢出无多余滑块。
- [ ] 每个窗口抓住中段滑块拖动，两轴互不归零，可到首尾。
- [ ] 普通鼠标在横条上滚轮可横滚；原生横滚和可上报的 Shift 横滚有效。
- [ ] 键盘焦点放 Explorer，悬停 SQL / DDL / Output 仍只滚动目标窗口。
- [ ] 小终端、最大化面板、拖动分割线后无内容覆盖和错位。
- [ ] SQL 行号/提示行不被覆盖；中文、Tab、长行的选择与光标正确。
- [ ] Explorer 深层祖先、长名称、Find/Search、折叠与异步载入表现稳定。
- [ ] 日志历史阅读不被追加打断，回到底部恢复跟随，横向位置保留。
- [ ] 数据滚动不触发查询分页；选中行/列和编辑态行为符合原有约定。

## 6. 执行依赖与阶段验收

```text
Task 1 共享几何
  → Task 2 单轴模型与目标
  → Task 3 DATA 迁移与拖动
  → Task 4 滚轮路由
  → Task 5 SQL / DDL
  → Task 6 OUTPUT
  → Task 7 Explorer 模型
  → Task 8 Explorer 渲染
  → Task 9 集成回归
  → Task 10 文档与最终验收
```

按顺序执行：这些任务共享 `src/ui/mod.rs`、`src/input/mouse.rs` 和 `src/app.rs`，不安排未经协调的并行文件修改。

阶段 A（Task 1–4）：公共机制可用，DATA 不回归，错误滚轮路由修复。

阶段 B（Task 5–6）：三个文本窗口完整接入，已确认拖动缺陷修复，日志跟随可靠。

阶段 C（Task 7–8）：Explorer 三模式双轴完整，祖先固定和横向命中稳定。

阶段 D（Task 9–10）：相关检查与真实终端验收完成，形成可交付结果。

## 7. 实施中的关键约束

1. **不要同时存多份偏移。** UI 保存命中和抓取快照，真实 offset 留在内容模型。
2. **不要把另一轴的 0 当“不变”。** 单轴 Action 必须明确 axis。
3. **不要让公共组件猜总范围。** Grid 横向和 Explorer pinned 行由适配层提供边界。
4. **不要让滚轮依赖键盘 Focus。** 目标区域、会话 ID 和当前视图决定动作去向。
5. **不要把正常重绘当布局失效。** 连续拖动必须跨帧保持捕获。
6. **不要无条件恢复日志尾部。** 旧内容替换前确定跟随意图，用户主动滚动优先。
7. **不要把计划中的命令标成已通过。** 实施时填写实际结果，记录环境限制。

## 8. 当前验证记录

- 计划编制：已核对共享表格、编辑器滚动、输出视口同步、鼠标路由和 Explorer 相关入口。
- Worktree：`../lazydb-unified-pane-scrollbars`，分支 `task/unified-pane-scrollbars`。
- 已实现：公共滚动条几何模块；Grid 横纵绘制迁移；Editor 单轴 setter；Editor 滑块抓取位置修复；OUTPUT LOG 滚动条渲染和滚轮动作；Explorer 普通树 / Find / Search 纵向滚动条及绝对纵向偏移。
- 已验证：`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test --lib`（794 项）、`cargo test --test mouse`（64 项）、`cargo test --test explorer_state`（43 项）、`cargo test --test ui_render`（215 项）、`cargo test --all-targets` 均通过。
- 尚未完成：Explorer 横向偏移、横向内容裁剪、三种 Explorer 模式的横向滚轮和横向拖动；基于鼠标实际滚动区域而非键盘 Focus 的完整跨窗口路由；公共滚动条端点和上述新增行为的专门集成测试；真实终端人工验收。
- 备注：当前代码已保持可编译和既有测试通过，但不应在未完成上述事项前宣称本计划全部交付。
