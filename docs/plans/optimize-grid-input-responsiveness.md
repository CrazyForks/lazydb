# 大结果集交互响应优化 Implementation Plan

> **执行者：Luna。** 用户已选择自动工作流，按下列验收单元连续实施、审查和提交合并；不再询问执行方式，不启动子 Agent。Astra 本阶段仅制定计划。以本任务执行协议为准。

**Goal:** 修复 relation data / result set 在 500、1000 行单页下 j/k 长按拖尾、滚轮到边界后阻塞快捷键和点击的问题。

**Architecture:** 保留现有可见行渲染，消除 relation 逐帧全页复制；用显式显示数据版本缓存精确内容列宽，用有界文本预览绘制单元格。对无变化表格导航停止派发/重绘，对连续可安全合批的导航顺序消费、有限合并绘制，保持输入与命中坐标的一致性。

**Tech Stack:** Rust edition 2024，Tokio，Crossterm EventStream 0.29，Ratatui 0.30.2，现有 TestBackend / unit / integration tests。

---

## 0. 起点与执行约束

- 分析依据：同目录 `analysis.md`，根因源码路径见其第 2 节。本计划是可执行细化，不把静态推断冒充实际性能数据。
- 原工作区 `/Users/yelog/workspace/tui/lazydb`；目标 main；指定起点及本阶段实际 HEAD 均为 `23a638fbc953adb5f956488693ab2d4825594581`。
- plan 阶段重新检查：main ahead origin/main 5，tracked/untracked 与 index 均无变化。不存在未提交依赖；不需要拷贝工作区额外行为到 worktree。
- checkpoint.json 仍不存在。state.json、历史回执保持原样。任务分支由自动工作流在计划后命名；本阶段不建分支、不改业务代码、不 stage/stash/commit。
- 实施使用任务 worktree；续做先核对实际 diff 与 checkpoint，选第一个未完成验收项继续。若当时存在他人修改，保留并核对依赖，不能清空整个工作区。
- 本计划只落在任务目录，不创建 docs/plans 副本。`change-scope.json` 只列预计业务/测试改动，任务目录元数据不作为产品提交范围。

## 1. 必需结果、项目门禁、补充验证

### A. 用户必需结果（功能验收）

1. 500/1000 行 SQL result 与 relation 的连续 j/k 不再因逐事件全量绘制形成秒级队列拖尾。
2. 到边界后重复 wheel 不进行无意义的数据处理与重绘，后续快捷键/点击正常执行，反向 wheel 立即恢复。
3. 不降低用户单页容量，不丢弃普通文本/命令输入，不改变单步导航、自动列宽、编辑及复制语义。
4. 顶/底/左右边缘、空结果、不满一屏、编辑草稿、换页和同形状数据替换正确；无 stale cache / stale hit map。

### B. 项目既有强制门禁

依据 `.github/workflows/ci.yml` 的 Rust job，最终代码运行：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

对应退出码应为 0。原 CI 的其他平台/数据库/发行检查仍由 CI 按既有配置执行，本任务不把它们增加为每单元本地重复步骤。当前本机默认 1.94.1，若用默认工具链验证，记录差异；不声称等价完成 1.94.0 门禁。缺数据库 URL 的集成测试可能跳过，明确记录而非称实库验证通过。

### C. 本修复必须提供的自动化证据

缓存失效、边界 no-op、输入保序/公平性和绘制次数的确定性回归测试；对应单元测试应先证明旧行为失败，再完成修复。性能问题不能只靠截图或几个最终行号断言验收。

### D. 补充建议验证（不是新增强制门禁）

- release wall-clock 对比、真实终端/PTY 长按与触控板实验是补充证据；用户未要求必须具备某种人工/PTY 环境。
- 建议目标 warm frame p95 <16ms、队列尾延迟/边界后响应 <100ms，只作为同机测量参考，不是跨平台 CI 硬阈值。
- 环境受限时最多一次针对性修复重试，之后记录限制，由 Luna 收尾审查决定是否需要补证；不无限 progress，也不要求用户代为实施。

## 2. 设计落实与接口约束

### 2.1 显示数据与列宽缓存

采用 `UiState` 的单项缓存，而非无限 tab→result 历史映射。缓存存每列**内容宽度**，不存全量 rows、preview 或可见选中样式；header/sort marker 与 override 每帧轻量合成，因此 icon、排序优先级或手动列宽变更不会污染内容缓存。

新增内部类型的建议命名：`GridDataRevision`、`GridWidthCacheKey`、`GridWidthCache`；字段/API 可按项目风格调整，但满足以下契约：

- 显示数据 owner 的修订身份在数据真正替换时更新，不能用行数、指针、开始查询的 generation 或逐帧内容 hash 代替。
- key 包含 tab_id、当前显示源身份（base/derived/relation/edit）和内容修订。修订可用新 UUID token 简化 reset/overflow/undo 碰撞问题；**token 只在数据变化时生成，不在 navigation/render 时生成**。
- 优先把 owner 修订字段放在 SQL tab / RelationTab，而非会频繁 reset 的 DataGridState；RelationEditSession 维护独立内容 token。会话重建生成新 token，undo/redo 换回行内容后也生成新 token，不能恢复旧 token。
- 若选择 u64 generation，必须另含 owner/session epoch，以免 grid 重置或新会话 revision 从 0 开始与旧 cache key 相撞。
- baseline 与 derived source 区分；移除 derived 后回退 baseline 必须命中正确源或重新计算。Loading/Failed/Cancelled 保留 previous 时绑定实际显示的旧数据身份。
- 不改 `ResultSet` 的序列化格式，不改 workspace 持久化结构；仅运行时 UI/model 元数据。
- 共享 dashboard/Redis renderer 暂不传稳定缓存身份（`None`，每次重算）以保持现有正确性；SQL/relation 显式传身份。无身份路径不能复用其他 source 的缓存。只适配这两处调用，不扩展为 Redis/dashboard 性能重写。

**修改入口核对清单：**

| 所在处 | 必须覆盖的变化 | 处理 |
|---|---|---|
| app 的 QueryFinished/结果安装 helper | 新结果、分页、清空、重新绑定 | 换 owner token |
| DerivedQueryFinished/DerivedQueryPageFinished 及 derived 移除 | 派生结果替换、回退 base | 换 token 或 source key |
| RelationSucceeded/安装 snapshot helper | 新页、刷新、相同行列数新内容 | 换 relation token |
| relation edit constructor / 切换 session | 新会话与相同维度数据 | 换 session token |
| update_cell/restore_unprovided/insert_row/paste_row/delete_rows | 提交值或结构改变 | 换 edit token；mode-only 不变 |
| undo/redo/discard_changes/commit_changes | 还原、重做、删除行清理 | 操作完成后换 edit token |
| RelationMutationSucceeded / server value 回填 | 直接修改 current/rows、服务端默认值 | 经统一 helper 更新 token |
| query focus、navigation、viewport、popup 草稿 | 未改变底表 | 不更新 token |

`RelationEditSession::record_change` 在实际操作前调用，失败操作也可能经过它；**不要仅在那里更新 token**。实施时查找直接 `edit.rows` / `row.current` 赋值，纳入上述 helper；tests 中手动替换数据也必须明确刷新身份。此部分是 Luna 实施审查重点。

### 2.2 有界显示预览与借用

新增 `CellValue::display_preview(max_len) -> String`（crate 内部即可），返回与现有 `.preview(max_len).text` 相同的文本，但不计算 original_len。文本核心算法可直接按下式实现：

```rust
fn bounded_preview_text(value: &str, max_len: usize) -> String {
    let mut chars = value.chars();
    let mut text: String = chars.by_ref().take(max_len).collect();
    if chars.next().is_some() {
        text.push_str("...");
    }
    text
}
```

Text、Unsupported 和 MySqlGeometry 用该路径；其余枚举分支保持原 formatting。Bytes 只输出限长字节，不能先复制整个值。旧 preview/clipboard_text 语义保留，grid body 和 width scan 调用新方法，终端净化位置保持一致。

relation render 直接借用最后一个 ResultSet，无结果用局部空值引用；删除全部 `row.current.clone().collect()`。有效行数使用 `edit.map_or(result.rows.len(), |edit| edit.rows.len())`，共同驱动 footer/detail、No rows、滚动条和实际行访问。

### 2.3 无变化判定

model 暴露不带 heap 字段的导航快照，包含 selected row/column、offset row/column、viewport rows；App helper 读取当前活动 grid 和 O(1) 维度。mouse vertical wheel 使用现有 `scroll_rows` 同一规则预测是否改变，不拷贝 column_widths 或数据。

纯 navigation 结果由 runtime 比较前后轻量状态；有 query focus/completion 清理时算真实变化，gesture/快捷键 prefix 的变化继续按现有路径 OR 到 dirty。无法安全判断的 action 默认 redraw。不要用 `Vec<Command>::is_empty()` 判断 no-op。

横向使用目标 offset 和选择边界夹取后的结果判断，不可只比较 offset；相同 offset 仍可能需要调整 selected_column。滚轮边界 guard 只针对真正表格路径，保留 overlay、DDL、output、Redis 专有路由。

### 2.4 安全输入合批

新建 `src/runtime/input_batch.rs` 放窄小 batch budget / 队列屏障策略与 unit tests；App action 应用仍沿用 runtime，避免引入全局调度框架。

规则：

1. 一批最多 64 个事件或约 2ms（先到即止）。只取已 ready 的事件；队列暂空立即完成一次 dirty draw，不等待用户松手。
2. 同一稳定 SQL/relation 表格上下导航按顺序逐次应用，只合并 draw，不把 `j,k` 换成净位移；普通 Press 自动重复与 Repeat 均工作。
3. 点击、拖动、resize、非导航键、paste、横向依赖布局的 wheel 为屏障：先 flush 待绘制状态并同步 viewport，再用新 hit map 处理屏障事件。用至多一个 pending raw event 保存 lookahead。
4. `Keymap::map` 有状态，不能“试映射再映射”同一键。分类器必须保守且不消费 keymap 状态；如果某键/自定义绑定无法证明为导航，作为屏障，按常规路径仅 map 一次。支持确认后的自定义导航绑定可优化，但不能以丢失绑定语义换合批。
5. vertical wheel 合批也受 pane 身份约束；如果没有可靠的稳定区域身份，wheel 按屏障处理仍可通过边界 no-op 快速消费。不能用任意单元格 HitTarget 猜布局未变。
6. 每批后回到外层 select，让后台 completion/ticker/theme 获得处理机会；pending event 不得被新到达的 terminal event 超越。错误/EOF 保留现有传播退出语义，不能吞掉流错误。
7. 所有新 dirty 累加用 OR，避免 ticker 覆盖已有 dirty；所有 viewport sync 在实际 draw 后执行。Release 不承担清空队列职责。

## 3. 单元一：大页稳态渲染只处理可见数据

**修改文件：**
`src/db/value.rs`、`src/ui/data_grid.rs`、`src/ui/relation.rs`、`src/ui/mod.rs`、`src/model/tab.rs`、`src/model/relation.rs`、`src/model/relation_edit.rs`、`src/app.rs`、`src/ui/dashboard.rs`、`src/ui/redis_browser.rs`。

**测试文件：** 上述局部 tests；`tests/ui_render.rs`、`tests/relation_tabs.rs`、`tests/performance_regression.rs`。

### 任务 1.1：建立可比较 fixture（每步独立完成）

1. 扩展现有 performance fixture，参数化 100/500/1000 行 ×20 列，构造 SQL 与 relation（有/无 edit）路径。固定屏幕 160×48，固定数据内容，fixture 构造在计时之外。
2. 修正 fixture 的 render helper：每次 draw 后将 `UiState.grid_viewport` 同步回 App，按真实 runtime 顺序导航，否则基准会测到未初始化 viewport。
3. 添加普通非 ignored 的 renderability/导航正确性用例；保留现有 ignored baseline 的原始输出，新增 cold/warm/navigation 分组及 sample 数、中位数/p95 输出。
4. 可选运行未优化版本 release baseline：`cargo test --release --test performance_regression -- --ignored --nocapture`；旧/新对比必须用同一 harness 与同一数据，分开标注 cold 与 warm。

**复核：** fixture 没有计入数据库访问/大数据构造，没有把 `20 renders` 总时长误记为 p95，没有省略 relation 编辑态。

### 任务 1.2：消除复制及全文扫描

1. 先加测试：新显示预览与旧 preview.text 对所有 CellValue 类型一致；测试 max_len=0、刚好长度、超长 Unicode、控制字符、Bytes、geometry、时间/空值。
2. 实现有界显示 preview，grid 的 width/body 两处改用它。
3. 移除 relation ResultSet 与 edit rows 全页克隆；修正 footer/detail/空态有效行数。
4. 增加空结果 insert draft、删除/撤销行数、semantic 样式回归用例。
5. 运行 `cargo test --lib db::value`、`cargo test --lib ui::data_grid`、`cargo test --test ui_render --test relation_tabs`，预期全部通过。

**复核：** 保留既有字符净化、宽度 clamp 6..40、省略号、完整复制/详情；没有把大字符串 clone 隐藏到新 helper。

### 任务 1.3：内容修订与精确宽度复用

1. 添加缓存独立 unit test，以测量 closure/counter 验证 same key 只测量一次、new token 重测、无 token 不复用。采用局部计数器，避免跨并行测试全局可变计数。
2. 建立 owner/edit revision token 和集中 invalidation helper，按第 2.1 节逐项覆盖结果安装、回退和编辑路径。
3. 将 key 从 SQL/relation renderer 传到共享 grid；dashboard/Redis 显式传 None；所有直接调用的局部测试同步新参数。
4. 在 UiState 保存一项内容宽度缓存，render 开头清理 hit regions 时不清掉缓存。每帧只合成 header/override 与布局。
5. 增加集成测试：同形状不同值替换会更新宽度；切 source/tab、Loading(previous)、失败保留 previous 不出现旧宽度泄漏。
6. 增加 edit commit/insert/delete/undo/redo/discard/服务端回填后宽度正确测试；导航、resize、只改焦点/编辑 popup 不增加内容测量次数。
7. 运行 `cargo test --lib ui::data_grid`、`cargo test --lib model::relation_edit`、`cargo test --test ui_render --test relation_tabs --test performance_regression`。

**单元验收：** 1000 行连续导航 warm draw 无全页 clone、无全页宽度扫描；自动宽度与旧算法一致；版本碰撞/失效矩阵有测试；有效行数和编辑样式正确。Luna 审查通过后继续单元二，可作逻辑提交 `perf(grid): reuse widths and borrow visible data`，仅 stage 此单元文件。

## 4. 单元二：边界滚动不派发昂贵无效工作

**修改：** `src/model/tab.rs`、`src/app.rs`、`src/input/mouse.rs`、`src/runtime.rs`。
**测试：** `tests/mouse.rs`、`tests/keymap.rs`、相关 model/app/runtime 局部 tests。

### 任务 2.1：建立边界行为测试

1. 给 SQL/relation 渲染并同步真实 viewport，分别构造 top/bottom/left/right 状态。
2. 对边界继续同向 wheel 断言 map 不产生无效 scroll；反向 wheel 有效；空表、少于一屏、viewport=0、极端 delta 安全。
3. 单独覆盖 query focus/completion 尚未关闭的边界输入，第一次仍能产生必要可见变化，后续 no-op。
4. 增加边界重复 `GridMove` / scrollbar set offset 的 dirty 判定测试，确保水平相同 offset 但选中列需夹取时没有误过滤。

### 任务 2.2：实现和验收

1. model/App 实现轻量导航快照和预测 helper，复用现有夹取逻辑。
2. 在 mouse 表格路由加入 boundary guard；不修改 overlay/editor/DDL/output/Redis 的路由优先级。
3. runtime 纯 grid action 使用前后快照判断 redraw，其它 action 保持原 redraw；gesture/prefix/focus 单独 OR。
4. 添加 fake draw 计数测试：边界 1000 次 wheel 后 0 次由无效滚动引发的 draw；随后快捷键/点击仍执行；真正焦点变化只产生必要的 draw。
5. 运行 `cargo test --lib model::tab`、`cargo test --lib runtime::`、`cargo test --test mouse --test keymap`。预期全通过；检查测试列表确认新增案例实际运行，不能接受 filter 命中 0 条作为验证。

**单元验收：** 边界重复事件最多做事件读取/路由/常数状态判定，不做宽度测量、表体构建或 draw；反向立即生效；不会把关闭 query focus 的第一次真实变化吞掉。Luna 复核后继续单元三，可提交 `fix(input): skip unchanged grid scrolling`。

## 5. 单元三：连续导航有限合批、保序绘制

**新增：** `src/runtime/input_batch.rs`。
**修改：** `src/runtime.rs`、必要的 `src/input/keymap.rs`（仅无副作用分类 helper）、`tests/performance_regression.rs`。
**测试：** input_batch 模块和 runtime 局部 tests、`tests/keymap.rs`。

### 任务 3.1：先测调度契约

1. 用可控 ready/pending event stream、fake clock 和 fake draw 建立 batch 测试，避免 sleep 依赖。
2. 预置 `[j,j,k,click]`、`[wheel...,resize,click]`、`[j,tab-switch,j]`，断言顺序与 draw→viewport sync→map barrier 的次序。
3. 测 Press 自动重复、Repeat、缺少 Release、快捷键前缀、paste、overlay 和自定义 binding；每个按键只能 map 一次。
4. 测数量预算、时间预算、队列暂空、EOF/error；持续 ready 流也能结束 batch 并返回外层。

### 任务 3.2：接入实际 runtime

1. 抽出并复用现有 draw + cursor + viewport sync 路径，避免 initial draw/regular draw/barrier draw 各复制一套逻辑。
2. 接入第 2.4 节预算及最多一个 pending raw event；优先只优化证明安全的 SQL/relation vertical grid 导航。
3. 把 redraw 累加保持为 OR；避免 ticker/非输入事件清掉 pending dirty。预算结束返回 select，无无界 channel、无无限 drain。
4. 在真实 runtime 驱动测试中注入 1000 次 ready 导航 + 快捷键，证明 draw 数显著小于事件数且最终 action 按序执行；加后台事件 ready 场景证明能被服务。
5. `cargo test --lib runtime::`、`cargo test --test keymap --test mouse --test performance_regression`，预期通过。
6. 补充 release 对比 `cargo test --release --test performance_regression -- --ignored --nocapture`。若导航 reducer 仍昂贵，记录阶段计时定位，不能用丢弃重复键规避。

**单元验收：** 连续导航合并绘制、单事件空队列立即可见；事件不丢失/不重排；屏障前后 hit map 与模型同步；预算保证不会无限占有 UI 线程；不依赖松手 Release。Luna 复核后继续最终验收，可提交 `perf(runtime): batch grid navigation redraws`。

## 6. 最终复核与一次完整门禁

1. Luna 检查实际 diff 和 change-scope：新增/修改文件在范围内；若实现必须扩展范围，先按真实依赖更新范围记录，不随意触碰其他功能。
2. 核对所有缓存写入路径：相同尺寸换内容、derived 回退、session 重建、undo/redo、直接 server 回填；不存在 render 时新增 token 或全表 hash。
3. 核对热路径：没有 ResultSet/App/rows clone 判脏、没有完整文本长度扫描、没有无界批处理；共享 dashboard/Redis None 路径正确。
4. 核对调度：Keymap 无重复消费、custom binding 保留、raw 屏障不丢失、终端流 error/EOF 正常、ticker OR dirty、viewport 只在 draw 后同步。
5. 功能齐备后跑第 1B 节的 fmt/clippy/全量 tests 一次。定向修复后重跑受影响检查；只有修改影响全量可信度才再次全量，不惯性执行 check+clippy+test 多轮。
6. 补充真实终端实验若环境允许：本地 SQLite 500/1000 行，两个视图长按 j/k 5 秒，触底继续 wheel 随后快捷键/点击，快速反向、resize、切 tab、编辑撤销、手动列宽。记录 terminal/窗口/输入方式；触控板惯性的新事件与应用 backlog 分开解释。
7. 所有验证追加到同目录 validation.md：命令、exit、HEAD+dirty diff 状态、toolchain、OS、数据库环境、测试 skip；失败与限制原样保留。补充验证不能升级为要求用户亲自操作的阻塞门禁。
8. Luna 负责最终审查、任务分支提交及合并 main。使用明确路径 stage，只提交任务改动，遵循当时自动工作流的提交/合并阶段指令。本 plan 不授权在原工作区提交其他未提交内容。

## 7. 完成清单与当前下一步

- [ ] 单元一：fixture、借用/预览、版本化精确列宽缓存、完整失效回归。
- [ ] 单元二：边界 no-op、真实可见变化 dirty 判定、后续输入可用。
- [ ] 单元三：有限合批、保序屏障、实际 runtime 计数/公平性测试。
- [ ] 最终：Luna diff 复核、既有 Rust 门禁、验证记录与限制、提交合并。

**第一个未完成可验收单元：单元一。** 实施阶段应先在任务 worktree 扩展参数化 fixture 与回归测试，再实现有界 preview/借用与宽度版本化。无需再次询问执行方式，不等待手工 resume。
