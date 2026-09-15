# Redis Table DataGrid Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行适配：若环境没有该技能，按本文任务顺序和检查点执行。本文接口和新增测试名称为拟定名称，实施时以最新源码为准。本文件是实施计划，不表示功能已经完成。

**Goal:** Redis Table 获得与 Relation Data 一致的序号、单元格选择、移动、翻屏、列宽、鼠标、复制和详情体验，并具备可靠的增量加载状态。

**Architecture:** 保留 RedisPageValue 和原始字节作为数据来源，以薄适配层生成共享 DataGrid 所需的 ResultSet 显示投影。RedisBrowserTab 持有独立 DataGridState；通过现有 Action → App → Command → Runtime 路径接通交互和异步读取。Grid 翻屏只浏览已加载数据，Redis continuation 单独负责加载更多。

**Tech Stack:** Rust 1.94 / edition 2024、Ratatui 0.30.2、Crossterm、Tokio、现有 Redis 驱动、TestBackend 和 Rust 集成测试；无需新增生产依赖。

---

## 1. 产品契约

1. 集合 Auto 模式进入真实 Table 渲染；格式标签必须与实际 body renderer 一致。
2. 序号从 1 开始，表示已加载结果中的显示顺序，不是服务端排序或 Redis List index；List 的 Index 数据列继续从真实 index 展示。
3. 沿用共享 Grid 的表头、分隔线、弱行高亮、强单元格高亮、列宽与滚动条几何规则。
4. Table 默认单行单元格；截断只影响展示。Enter 打开完整详情，详情支持既有格式化与文本操作。
5. 单击单元格只选择并聚焦 Preview；复制读取完整源值；序号不进入复制行内容。
6. Table 的 h/j/k/l 和方向键移动单元格；PgUp/PgDn 按实际 viewport 翻屏；gg/G 到已加载首末行，不能触发全量读取。
7. 沿用共享 Grid 的 0/^/$、[/]/= 和复制快捷键；Redis Table 的 ] 不再加载下一批。
8. Space l 和底部 Load more 显式加载下一批；已有内容保持可见，失败允许重试。
9. Keys 与 Preview 使用明确的面板切换动作。实施时复用现有全局面板导航；若没有可用绑定，Redis Results 内使用 Tab/Shift+Tab 切换这两个 pane，并更新帮助与测试。
10. Keys、Table、Text、Overlay 分别分流按键；Table 不再向隐藏的只读编辑器发送移动/复制事件。
11. W 仅作用于文本/详情；Table 不显示 Wrap ON 控件，也不改变文本视图保存的 wrap 状态。
12. 切换 key 重置 Grid；同 key 追加保留选择/视口；同 key 刷新尽力按稳定行标识恢复选择；数据减少时夹紧边界。
13. 格式切换分别保存 Grid 与文本滚动状态；每个 Redis Tab 的状态隔离。
14. 第一版不提供服务端任意页号、总页数、Last page 或自动滚动触底无限加载。

### 数据列约定

| 类型 | 数据列 | 稳定行标识 |
|---|---|---|
| String | Value | 当前 key 内唯一行 |
| Hash | Field, Value | field 原始字节 |
| List | Index, Value | index；并发头部插入后只能尽力恢复 |
| Set | Member | member 原始字节 |
| ZSet | Member, Score | member 原始字节 |
| Stream | 沿用 from_page 的 ID/字段展示契约 | entry ID 原始字节 |

String 保持现有 Auto 文本检测，只有明确 Table 格式才用单行表格。集合 Auto 继续优先 Table。多批 String 续读是拼接同一行，不新增伪数据行。

## 2. 代码依据与依赖接缝

行号是规划时定位参考，执行时以符号名为准。

| 文件/符号 | 当前行为与实施方向 |
|---|---|
| `src/value_preview/table.rs::RedisTable/from_page` | 已有列、显示 cells、源字节 identity；增加显示投影与源单元格访问 |
| `src/model/redis_browser.rs::RedisBrowserTab` | 只有 preview_scroll；增加独立 Grid 状态、缓存及追加请求状态 |
| `src/ui/redis_browser.rs::render_table_preview` | 独立 Table、默认 TableState；替换为共享 Grid 包装 |
| `src/ui/data_grid.rs::render` | 已有行号、高亮、滚动、命中；复用并完成最小通用化 |
| `src/model/tab.rs::DataGridState` | 直接复用其选择、偏移、viewport、column_widths 字段 |
| `src/app.rs::active_grid_dimensions/active_record_snapshot` | 当前不接 Redis；补齐受视图和焦点约束的路由 |
| `src/app.rs::copy_grid_cell/copy_grid_row` | 复用剪贴板 Command 和 TSV 约定；源值不得来自截断投影 |
| `src/app.rs::redis_preview_cell_detail` | 已从原始字节生成详情；复用其格式检测与无损表示 |
| `src/input/keymap.rs` Redis Preview 分支 | 目前可能进入 ReadOnlyEditorKey；增加 Table 分流并处理组合键前缀 |
| `src/input/mouse.rs` RedisPreviewTableCell | 当前单击详情；改为选择，统一 hit-region 布局 |
| `src/app.rs::load_next_redis_page` | 当前按 page.position 构造请求；补精确会话、in-flight 与响应校验 |
| `src/model/redis_browser.rs::append_value_page` | 当前直接 extend；补去重、缓存预算和稳定选择 |
| `src/runtime.rs::load_redis_value_page` | 延续请求身份，避免返回时读取全局活动 Tab |
| `src/help.rs` | 与真实 Table 能力、快捷键同步 |

先阅读并衔接：

- `docs/plans/2026-09-15-data-grid-selection-alignment.md`：复用已经落地的布局修复；没有落地时将对应修复作为共享 Grid 的前置，不复制另一套坐标逻辑。
- `docs/plans/2026-09-15-redis-browser-loading-lifecycle.md`：复用按 RedisTarget 准备精确会话的入口；Keyspace SCAN 与 Value HSCAN/SSCAN 是不同请求状态，不共用 cursor 或 in-flight 字段。
- 计划编写时上述 Redis 生命周期文档及 consoles 生命周期文档为未跟踪文件。实施前检查最新 git diff，保留其他工作；提交只暂存本任务文件。

## 3. 状态与数据设计

### 3.1 最小状态

在 RedisBrowserTab 增加 `preview_grid: DataGridState`。显示缓存建议由现有模块承载，不另建大范围通用数据框架。

缓存需满足：

- 由已接受的数据响应及格式变化驱动更新，render 不重新解析整个集合。
- 原始字节由 value_page 保持权威；显示投影存完整可显示文本，最终截断交给 Grid。
- `identity` 现有含义是逐单元格源值，不能直接作为整行唯一键；行键单独按类型计算。
- 避免新增 ResultSet 缓存破坏 RedisBrowserTab 现有 Clone/Eq 派生约束。若采用仅 PartialEq 的 ResultSet，先验证 tab 上层派生依赖；优先缓存 Eq 兼容的 RedisTable 并在版本变化时维护投影，避免无关全局改型。
- Grid 内部选择索引保持 0-based；UI 序号单独 +1。
- 这些状态仅进程内保存，不修改 workspace 持久化格式。

### 3.2 请求身份与追加状态

每个 value 请求至少校验 tab_id、RedisTarget/key、connection generation、preview_generation 和唯一请求标识。优先复用现有身份类型；若现有响应缺 request_id，再贯通补充 Action/Command/Runtime。

追加状态独立于现有 Ready 数据：Idle / Loading / Failed；Complete 与 budget-limited 从加载结果/累计预算决定。进入 Loading 后再次请求不派发命令。失败不覆盖已加载的 Ready page，不推进 continuation。

### 3.3 底部状态

```text
Rows 101–140 · 300 loaded · More available    [Load more]
Rows 101–140 · 300 loaded · Loading…
Rows 1–36 · 36 loaded · Complete
Rows 101–140 · 300 loaded · Load failed       [Retry]
Rows 101–140 · 300 loaded · Cache limit reached
```

空表显示 `0 loaded`，不显示 `Rows 1–0`。窄窗口优先保留 loaded、加载状态和可用操作；不可用按钮不注册点击目标。状态条占用的高度必须从 Grid viewport 扣除。

## 4. 实施任务

每项按小步执行：补行为断言 → 运行确认真实失败 → 最小实现 → 定向验证。单一步骤目标约 2–5 分钟；较复杂步骤按列出的场景逐个完成。以下命令以满足 Cargo.toml MSRV 的 Rust 工具链运行。新增测试 target 创建前不能执行对应 cargo test。

### Task 1：建立真实 Table 入口与回归夹具

**Files:** Modify `tests/redis_browser_tabs.rs`, `tests/ui_render.rs`, `tests/value_preview.rs`；按发现必要修改 `src/ui/redis_browser.rs`, `src/app.rs` 的格式/响应同步入口。

1. 复用既有 Redis profile/tab/page 构造方式，创建可指定类型、行数、格式、焦点的本地 fixture。
2. 用 Action 回填 Ready Hash，走 Auto 检测，再真实渲染；断言格式 view 与 Table body 一致，不仅断言右上角字符串。
3. 覆盖 Hash 中一个长 JSON value、短 value、中文 field 和非 UTF-8 value。
4. 补显式 Table、显式 Text、尚未 Ready 三种分支，验证标签与 body、控件的对应关系。
5. 若出现截图所示标签/Table body 不一致，修正状态同步或 renderer 选择，保留回归用例。

**Run:** `cargo test --test redis_browser_tabs --test ui_render --test value_preview`

**Expected:** fixture 可编译；入口行为可靠。共享 Grid 的序号/选中断言放在 Task 4，不将所有未实现能力加入此任务。

### Task 2：数据投影、原始值和稳定行标识

**Files:** Modify `src/value_preview/table.rs`, `src/model/redis_browser.rs`；Test `tests/value_preview.rs`, `tests/redis_preview_serialization.rs`。

1. 为各 Redis 类型添加列数量、行数量、源单元格字节的行为断言；沿用现有 Stream 形状。
2. 添加用于共享 Grid 的列元数据/CellValue 显示投影，使用普通可显示文本类型，避免误判为 SQL 数值或启用关系表编辑。
3. 增加源单元格访问与按类型行键计算，Hash value 变化不改变 row key。
4. 保留已有 identity 外部行为，除非完整迁移全部调用方；不要将源字节存到仅用于展示的省略文本中。
5. 测试 List 非零起始 index、String 只有一行、ZSet score 源文本、二进制与空值；空字符串不能变成 SQL NULL。

**Run:** `cargo test --test value_preview --test redis_preview_serialization`

**Expected:** 投影适合 Grid，源值保持精确；稳定键与显示字段解耦。

### Task 3：Grid 状态与 App 动作接入

**Files:** Modify `src/model/redis_browser.rs`, `src/app.rs`；Create `tests/redis_table_grid.rs`。

1. 添加 preview_grid，构造时默认初始化；补 key 切换和格式往返状态测试。
2. 先为 GridSelect、GridMove、首末行/列、翻屏写 Action 层测试，断言 Redis 状态实际变化。
3. 补齐 App 中尺寸、当前行列、移动、选择、滚动、列宽、viewport 回写的所有 Grid 分支；复用现有辅助函数，避免再写一套边界数学。
4. 使用统一的 Redis Table 可交互判断，阻止 Keys/Text/无数据时误操作；空表不创建伪选择。
5. 测试 viewport=1、0 行、1 行、列越界、翻屏到末尾、两个 Redis Tab 状态隔离。
6. 同 key 刷新保存 row key 和列，回填时按 key 恢复；不存在时 clamp。追加不重置 row_offset。

**Run:** `cargo test --test redis_table_grid --test redis_browser_tabs`

**Expected:** 仅通过 App::update 操作即可验证网格状态；文本状态和其他 Tab 不受影响。

### Task 4：共享 Grid 渲染、几何与鼠标

**Files:** Modify `src/ui/redis_browser.rs`, `src/ui/data_grid.rs`, `src/ui/mod.rs`, `src/input/mouse.rs`, `src/app.rs`；Test `tests/ui_render.rs`, `tests/mouse.rs`, `tests/redis_table_grid.rs`。

1. 用 Redis 显示投影调用 data_grid::render，传入 preview_grid，禁用 SQL sort_interactive，edit=None。
2. 将 Redis 独立表格的手写宽度、行遍历、hit-region 代码替换掉；Table 不再回写文本 preview_scroll。
3. 对 RelationColumnResize 等现有交互目标做最小通用化，或在处理器按 active Grid owner 分流；名称迁移必须覆盖所有调用方。
4. 鼠标单击产生 GridSelect，同时先聚焦 Preview；注意 Action 的守卫不能在聚焦前拒绝此次选择。
5. 复用 Grid viewport 和 scrollbar 回传；更新滚轮、拖拽、列宽操作的 Redis 路由。
6. 加真实 Buffer 测试：序号、首字符高亮、分隔符不着色、第二列点击、窄屏横滚、中文宽字符、空集合表头。
7. Table 隐藏 W 控件；同一渲染入口仍保留文本控件、文本行号和现有详情 renderer。
8. 验证失焦/overlay 时显示状态沿用共享 Grid 约定，事件不能落到背后的表格。

**Run:** `cargo test --lib ui::data_grid::tests`

**Run:** `cargo test --test ui_render --test mouse --test redis_table_grid`

**Expected:** Redis 与 Relation/SQL 使用同一套几何。测试预期坐标不能调用被测布局函数生成，避免同错同过。

**Checkpoint commit:** `feat(redis): render table previews with shared data grid`

### Task 5：按键、组合键前缀和面板焦点

**Files:** Modify `src/input/keymap.rs`, `src/action.rs`（仅缺少必要动作时）, `src/app.rs`, `src/help.rs`；Test `tests/keymap.rs`, `tests/redis_table_grid.rs`。

1. 对 Keys/Table/Text 三种上下文编写同一组方向键、h/j/k/l、PgUp/PgDn 的映射测试。
2. 在 ReadOnlyEditorKey 分支之前识别 Table，复用 configured navigation 和公共 Grid keymap，不能只复制硬编码默认键。
3. 审计前置全局分支及 Pending/leader：gg、G、复制组合键和翻屏可能早于 Redis 局部分支处理，逐项增加能力判断。
4. Table 的 [/]/= 使用共享列宽操作；Space l 保留 LoadNext。Keys/Text 的现有行为通过独立上下文测试固定。
5. 明确 pane 切换按键，按照产品契约复用或增补绑定；验证从第一列向左不会意外离开 Table。
6. Table 的 Enter 打开选中值详情；Esc/Overlay 按既有优先级处理。W 对 Table 不产生文本 wrap mutation。
7. 更新 help capability/prefix 提示，Table 中提示与真实 Action 一致，Text 不显示 Grid 列宽提示。

**Run:** `cargo test --test keymap --test redis_table_grid`

**Expected:** 同一输入在不同上下文产生正确动作；Table 按键不送入隐藏编辑器。

### Task 6：完整值复制、行复制与详情

**Files:** Modify `src/app.rs`, `src/value_preview/table.rs`；Test `tests/redis_table_grid.rs`, `tests/redis_preview_serialization.rs`, `tests/mouse.rs`。

1. 给共享复制动作补 Redis 源值访问；普通 UTF-8 使用源文本，非 UTF-8 使用已有无损转义约定。
2. 复用 WriteClipboard 与现有 TSV 编码，支持当前 cell、整行、带表头行；行号不参与。
3. 若 active_record_snapshot 提供的值只是显示投影，复制入口必须转到独立 raw accessor，不能依赖显示 snapshot 还原。
4. Enter 调用现有 redis_preview_cell_detail 的原始字节/自动格式化链路；保留 tab/session/revision 身份。
5. 测试长于列宽的值、JSON 原文、tab/newline、中文、空值、非 UTF-8；断言 Command payload 精确值，不访问真实系统剪贴板。
6. 断言鼠标单击没有 OpenTextDetail，Enter 有；连续移动后详情和复制均来自最新选中单元格。
7. 若沿用公共 map_results 中 v/RecordView，则补齐其 Redis 源值及行导航；若本版不暴露该能力，明确在 Redis capability/keymap 禁用，避免继承一个无效入口。

**Run:** `cargo test --test redis_table_grid --test redis_preview_serialization --test mouse --test keymap`

**Expected:** 复制无视觉省略号、无意外解码变更；详情只读且来源正确。

**Checkpoint commit:** `feat(redis): add grid navigation copy and cell details`

### Task 7：可靠的 Load more 请求生命周期

**Files:** Modify `src/model/redis_browser.rs`, `src/app.rs`, `src/action.rs`, `src/commands.rs`, `src/runtime.rs`（按现有事件身份是否完整决定改动）；Create `tests/redis_value_pagination.rs`；Test `tests/redis_loading_lifecycle.rs`。

1. 检查 `src/commands.rs` 的 LoadRedisValuePage 与 `src/action.rs` 的成功/失败事件，列出已有身份字段，复用足够的字段，仅补缺失部分。
2. 测试连续两次 LoadNext 只发一个请求，成功后才允许下一批；完成状态不发请求。
3. 添加 tab-local 追加 in-flight 状态；按精确 RedisTarget 的会话身份派发，复用加载生命周期计划已经落地的会话入口。
4. 贯通请求身份；回填匹配 key、tab、target、generation、request_id。关闭 Tab、切 key、刷新、断开/重连均使旧请求失效。
5. 追加期间保持 Ready 内容与 Grid 交互；失败保存错误并保留旧 continuation，Retry 重试相同位置。
6. 测试两个 profile/DB 的响应逆序、旧请求成功/失败晚到、关闭 Tab 后响应、后台完成不抢焦点。
7. 若请求返回时 key 类型发生改变，停止类型不匹配的 append，显示可刷新状态；不能更新 cursor 却悄悄丢弃数据。

**Run:** `cargo test --test redis_value_pagination --test redis_loading_lifecycle --test redis_browser_tabs`

**Expected:** 命令去重、目标正确、旧响应无效、失败后内容完整。

### Task 8：合并去重、预算与缓存更新

**Files:** Modify `src/model/redis_browser.rs`, `src/value_preview/table.rs`, `src/db/redis/read.rs`（仅现有预算/position 契约不足时）；Test `tests/redis_value_pagination.rs`, `tests/value_preview_limits.rs`, `tests/redis_scale.rs`, `tests/redis_protocol_limits.rs`。

1. Hash 按 field 去重，重复 field 更新 value 且保留首次插入位置；Set 按 member 去重。
2. ZSet 按 member、Stream 按 ID 合并重叠数据；List 按 index 合并。明确这是动态数据的尽力浏览，不宣称一致性快照。
3. 空批次且 continuation 未结束保持 More available；第一版不自动循环追扫，用户可再次 Load more。
4. 审计 raw_bytes/formatted_bytes 当前定义，区分读取量与缓存占用；重复项不重复累计 retained 行数，更新值时扣旧加新。
5. 复用现有累计预算；若只有单请求预算，为集合缓存新增可测的累计行数/字节限制，建议起点为 10,000 行、16 MiB 原始字节及 32 MiB 显示文本上限，按现有更严格限制取小值。
6. 达到预算标为 Cache limit reached，禁用继续加载；允许显式刷新重新开始。不要自动淘汰前页造成序号/选择跳变。
7. 若一个 scan 响应因预算只接收部分条目，禁止直接用响应尾 cursor 继续而跳过未保存条目；首版停止并标记受限，只有完整接收才允许 continuation。
8. String byte 续读保持已有截断/编码契约；partial UTF-8 或序列化数据继续使用 NeedsMoreData 语义。
9. 数据接受后更新投影缓存。追加只更新变化行和自动列宽候选；用户覆盖列宽不变。
10. 检查共享 Grid 是否每帧仍扫描全部数据计算默认宽度；必要时允许调用方传缓存的基础宽度，override 仍独立。不要以本次范围重构所有 ResultSet 所有权。

**Run:** `cargo test --test redis_value_pagination --test value_preview_limits --test redis_scale --test redis_protocol_limits`

**Expected:** 不丢未消费数据、不伪装 Complete、重复扫描不增加重复行；预算和缓存行为可确定性验证，不用 sleep 或脆弱的墙钟耗时断言。

### Task 9：加载状态条与端到端浏览行为

**Files:** Modify `src/ui/redis_browser.rs`, `src/ui/mod.rs`, `src/input/mouse.rs`, `src/help.rs`；Test `tests/ui_render.rs`, `tests/mouse.rs`, `tests/redis_value_pagination.rs`。

1. Table 底部预留一行，显示 viewport 范围、已加载数、完成/加载/失败/受限状态。
2. Load more/Retry 仅可用时注册带 tab 身份的 HitTarget；加载中不可重复点击。
3. range 使用 Grid 实际 row_offset/viewport，不能取上一帧文本 viewport；0 行与超小窗口有明确降级。
4. 用较多数据验证 PgDn 只改变视口且不发网络请求；点击 Load more 只追加并保留选中项。
5. 用键盘与鼠标分别走失败→Retry→成功→Complete，断言状态条与请求状态同步。

**Run:** `cargo test --test ui_render --test mouse --test redis_value_pagination --test redis_table_grid`

**Expected:** 用户能明确区分翻屏和远端读取；控件文案、可点击性与实际状态一致。

**Checkpoint commit:** `feat(redis): add bounded incremental table loading`

### Task 10：回归、用户说明与最终验收

**Files:** Modify `src/help.rs`, `README.md`（在已有 Redis 使用说明处补充）；必要测试修改限于上述文件。

1. 文档说明 Grid 导航、复制、Enter 详情、面板切换、Space l、翻屏与加载更多区别、受限状态。
2. 运行格式与静态检查，修复本次引入的问题；预先存在的问题记录来源，不混入无关修复。
3. 运行 Redis、共享 UI/输入及 Relation 定向回归；通过后进行一次完整测试，不无理由重复执行。
4. 以本地 Redis 测试数据人工操作：Hash 300 行含长 JSON；List 非零 index；Set/ZSet；Stream；binary/String。
5. 终端尺寸覆盖 80×24、120×40、较宽窗口；鼠标点击列边界、拖宽、翻屏、复制、切换格式和快速切 key。
6. 验证 Relation Data/SQL Results 的高亮、列宽、排序、编辑与复制原有行为；Redis 不获得 SQL 写入动作。
7. 对照第 1 节逐项验收，记录执行命令、通过结果、人工检查及无法运行的环境项。

**Checks:**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --test redis_table_grid --test redis_value_pagination --test redis_browser_tabs --test redis_loading_lifecycle --test redis_preview_serialization --test redis_values --test redis_protocol_limits --test value_preview --test value_preview_limits
cargo test --test keymap --test mouse --test ui_render --test relation_tabs
cargo test
```

需要真实服务或 Oracle 环境的现有测试按仓库既有机制运行；环境缺失不算通过，记录具体未验证项。不为计划文档本身运行产品测试。

**Checkpoint commit:** `docs(redis): document table navigation and loading`

## 5. 依赖与完成标准

```text
Task 1 → Task 2 → Task 3 → Task 4 → Task 5 → Task 6
                   ↓
                Task 7 → Task 8 → Task 9
Task 6 + Task 9 → Task 10
```

建议按编号顺序实施，尤其 src/app.rs、src/model/redis_browser.rs、src/ui/mod.rs 存在共享编辑面。提交点是执行阶段检查点，不表示编写计划时自动提交。

完成条件：

- [ ] Redis Table 与共享 DataGrid 使用同一套几何和样式。
- [ ] 单元格选择、移动、翻屏、列宽和鼠标动作形成闭环。
- [ ] 复制/详情使用完整源值，二进制语义明确。
- [ ] Keys/Table/Text/Overlay 输入分流正确，帮助同步。
- [ ] 增量请求精确目标、去重、旧响应隔离、失败重试均验证。
- [ ] 集合去重、缓存预算、空批次和 Complete 状态正确。
- [ ] Relation/SQL 共享 Grid 回归通过。
- [ ] 无新增 workspace 持久化迁移或生产依赖。
- [ ] 格式、静态检查、适用测试及人工 TUI 验收有记录。
