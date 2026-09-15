# Redis Preview Navigation Performance Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行环境未安装上述技能时，按本文件任务顺序实施与验证，不假定技能可用。

**Goal:** 消除 Redis Java/JSON/YAML 文本预览在持续 hjkl 移动时的重复全文处理，使预热后的导航与快照生成主要取决于可见内容，而不是文档总大小或光标深度。

**Architecture:** 在 EditorSession 内建立按内容版本失效的文档投影缓存、按语言失效的高亮缓存，以及按有效宽度失效的换行索引。渲染和光标可见性判断共享坐标索引，UI 先确定逻辑行号栏宽度再生成一次快照；继续复用 modalkit 的只读 Vim 语义。

**Tech Stack:** Rust 2024 / Rust 1.94、modalkit 0.0.25、ratatui 0.30.2、crossterm 0.29、现有 Rust 单元/集成测试、std::time::Instant 与 tracing。

---

## 0. 基线和执行约定

- 本文是实施计划；未实现优化、未运行性能测试，耗时目标不是已测结果。
- 行号依据规划时源码，执行时按函数和类型定位。开始前运行 `git status --short`，识别用户修改。
- 原交互契约见 `docs/plans/2026-09-14-redis-preview-vim-implementation.md`；本任务延续其中已有行为。
- 每个 Task 内按编号逐项执行，测试失败时先确认是预期行为缺口。性能基线和原有正确性测试应当通过，不人为制造失败。
- 不需新增长期运行的数据库环境来验证核心渲染算法；实际终端复测单独记录。
- 建议按各 Task 的提交信息拆分变更；实际提交在执行任务获授权时完成。

### 已确认的热路径

| 位置 | 当前行为 | 后果 |
| --- | --- | --- |
| `src/ui/redis_browser.rs:307–323` | 为推断 gutter 连续生成两次快照 | 同一帧重复执行整个预览计算 |
| `src/editor/mod.rs:978` `render_wrapped_preview_snapshot` | 以全文行数作为 viewport.height，最后才截取可见行 | 每帧全文投影、高亮、换行 |
| `src/editor/mod.rs:1234` `render_snapshot_with_options` | 获取全文并计算全文最大行宽 | Wrap Off 也存在全文扫描 |
| `src/editor/mod.rs:2739` `ensure_cursor_visible_at` | 从文档开头投影并累计视觉行 | 按键成本随光标深度增加 |
| `src/editor/mod.rs:2370` `input_vim_key` | 导航前后读取全文 snapshot 并比较 | 只读移动仍承担全文复制成本 |
| `src/runtime.rs:5745–5919` | 每个事件完成后同步重绘 | 耗时超过重复输入间隔时可能积压 |

Java 在 value 加载/格式切换时转换成编辑器文本，普通 hjkl 不重复解码。截图 Size 字段来自 memory_usage_bytes，不能作为展开文本大小。真实各阶段占比由 Task 1 测量。

## 1. 必须满足的设计契约

### 1.1 内容、坐标与滚动语义

1. 源文本坐标继续使用逻辑行 + 字符列；source_start/source_end 继续为全文 UTF-8 字节范围。
2. 逻辑行数用于行号栏；视觉行数用于 Wrap 下的滚动条、first_line 和滚动边界。
3. 保留 `EditorRenderSnapshot.total_lines` 的现有滚动含义，新增 `logical_line_count`；显式记录其含义，避免修改所有消费者的滚动契约。
4. 共享只读渲染器以 logical_line_count 计算 gutter；非 Wrap 快照令 logical_line_count == total_lines。
5. 原有 sentinel newline、空文档、末尾换行语义继续使用编辑器既有定义，不能直接用一个独立 split 结果替代而不验证。
6. 宽度、Tab、控制字符的投影继续由 `project_editor_line` 及其现有规则决定，禁止引入不同的 chars().count() 换行算法。
7. 选择、搜索提示、光标、selection_newline 是动态状态，不放入静态行缓存。
8. j/k、数字前缀、搜索、Visual、复制的行为以既有 modalkit 路径为准。优化不能把逻辑行移动改为视觉行移动。

### 1.2 缓存结构与生命周期

新增 `src/editor/preview.rs`，承载预览内部结构和纯坐标算法。建议类型职责如下，字段在实现时匹配已有投影类型：

| 类型 | 核心数据/职责 |
| --- | --- |
| `PreviewDocument` | revision、logical_line_count、max_line_width、每行全文字节起点及共享的静态显示投影 |
| `PreviewHighlightCache` | 当前语言、按逻辑行懒构建的 token/span；不包含主题颜色或选区 |
| `PreviewWrapIndex` | 有效 body width、每逻辑行 wrap 起始 cell 列、逻辑行的首视觉行前缀数组、总视觉行数 |
| `PreviewViewportState` | 当前高度、视觉滚动位置、Wrap 开关；替代不透明四元组的交互职责 |

会话内通过 `RefCell` 实现已有 `&self` 快照 API 的缓存更新；限制借用作用域，不跨编辑器 buffer 锁持有可变缓存借用。只保留当前 revision、当前语言和最近一个有效宽度，避免每次 resize 留下一份历史布局。

换行采用 `line_visual_starts`（长度为逻辑行数 + 1 的前缀数组）和每行 `wrap_starts`。空逻辑行也占一个视觉行。边界搜索必须保证每次前进，宽字符比视口宽时复用既有可显示策略。

缓存预热后的核心计算：

```text
cursor_cell = projection[cursor.line].source_to_display_cells[cursor.column]
segment = upper_bound(wrap_starts[cursor.line], cursor_cell) - 1
cursor_visual_row = line_visual_starts[cursor.line] + clamp_to_last_segment(segment)

first_logical_line = upper_bound(line_visual_starts, first_visual_row) - 1
读取该位置开始的 H + 2 个视觉行描述，按需高亮其涉及的逻辑行
```

行尾恰落在宽度边界的光标行为用原有正常/选择模式的语义测试约束，不能只靠上述公式推断。

### 1.3 失效矩阵

| 事件 | 文档投影 | 高亮 | 换行索引 | 视口/光标 |
| --- | --- | --- | --- | --- |
| hjkl、选择变化、主题变化 | 复用 | 复用 | 复用 | 更新 |
| 仅滚动 | 复用 | 新出现行按需补充 | 复用 | 更新 |
| 仅高度变化 | 复用 | 可补充新可见行 | 复用 | 夹紧/保持光标可见 |
| body width 变化 | 复用 | 复用 | 重建一次 | 保持源位置锚点并夹紧 |
| 仅语言变化 | 复用 | 失效 | 复用 | 更新 |
| 格式切换导致文本变化 | 重建 | 失效 | 重建 | 遵守现有格式切换行为 |
| Wrap Off/On | 复用 | 复用 | 相同宽度可复用 | 转换滚动坐标 |
| 内容替换/追加、revision 变化 | 失效 | 失效 | 失效 | 重新夹紧 |
| 同 UUID 重新 open_session | 新会话新缓存 | 新缓存 | 新缓存 | 重新初始化 |
| 会话关闭 | 释放 | 释放 | 释放 | 释放 |

重要：`open_session` 会把 revision 初始化为 0，因此不能用全局 `(UUID, revision)` 缓存而忽略会话重建。同 UUID 重开测试必须覆盖这一情况。

## Task 1：建立可复现性能基线与行为对照

**Files:**
- Modify/Test: `src/editor/tests.rs`。
- Create: `src/editor/preview_perf_tests.rs`，仅 cfg(test) 引入。
- Modify: `src/editor/mod.rs`，测试模块声明及必要的测试专用统计。
- Create: `docs/performance/redis-preview-navigation.md`，保存环境、方法与前后数据。
- Reference: `tests/redis_preview_serialization.rs`、`tests/redis_browser_tabs.rs`。

1. 构造固定 JSON fixture 生成器，模拟 class/fields/annotations/Block、深缩进、长类名；生成约 100、1,000、10,000 行及接近输出上限的样本，记录实际字节数与逻辑行数。
2. 加入宽字符、Tab、空行、末尾换行、一个超长逻辑行的独立样本。真实 Java 脱敏样本若可用，用现有格式化接口生成文本；合成 JSON 不标记为 Java 解码基准。
3. 添加当前应通过的行为对照：Wrap on/off、hjkl 位置、翻页、滚动、选择复制、源坐标映射。优先复用现有 wrapped_preview 测试。
4. 添加 `#[ignore]` 的 `preview_navigation_benchmark`，使用 Instant 和 black_box，在顶部/中部/底部交替执行不会快速撞边界的 h/l、j/k 序列；区分冷启动、预热后的单按键、单快照、按键+快照，统计 P50/P95/P99。
5. 添加 `preview_ui_benchmark`，利用测试后端走 Redis 面板真实渲染，测量双快照和全 UI 构建成本。测试后端结果不等同于真实终端输出耗时。
6. 使用测试专用会话计数记录全文物化、投影行数、高亮行数、换行构建次数、快照调用数。计数更新放在实际工作点，不统计 getter 调用；不为生产提供额外公共接口。
7. 在性能文档记录 CPU、Rust 版本、release profile、终端 cell 尺寸、Wrap 状态、fixture 大小、采样轮次，保存优化前结果。

**Commands:**
```bash
cargo test --lib wrapped_preview
cargo test --release --lib preview_navigation_benchmark -- --ignored --nocapture --test-threads=1
cargo test --release --lib preview_ui_benchmark -- --ignored --nocapture --test-threads=1
```

**Expected:** 行为测试通过；基准输出各阶段耗时、样本数及工作量计数，不设置依赖机器速度的 CI 失败阈值。

**验收：** 基准能揭示完整 UI 的重复快照，以及顶部/底部的定位成本差异；测试数据可重复且不依赖线上 Redis。

**建议提交：** `test(redis): establish preview navigation performance baseline`

## Task 2：分离逻辑行数，删除 gutter 探测快照

**Files:**
- Modify: `src/model/editor.rs`（EditorRenderSnapshot）。
- Modify: `src/editor/mod.rs`（快照构造、line_count）。
- Modify: `src/app.rs`（Redis 预览轻量逻辑行数查询）。
- Modify: `src/ui/redis_browser.rs`（render）。
- Modify: `src/ui/read_only_sql.rs`（gutter 计算）。
- Test: `src/editor/tests.rs`、`tests/ui_render.rs`；更新实际搜索到的其他 snapshot 构造点。

1. 添加行为回归：逻辑行数为 9/10、99/100 的边界；单行 wrap 成超过 100 个视觉行时 gutter 仍依逻辑行号；光标与 body 的 x 坐标一致。
2. 为 EditorRenderSnapshot 增加 logical_line_count，所有构造点明确赋值。total_lines 保留原有滚动含义。
3. 在 App 层封装会话逻辑行数查询，复用 editor.line_count，缺失会话继续走当前 loading/fallback 渲染。
4. Redis render 先取逻辑行数并算有效 body width，只调用一次 redis_preview_snapshot；删除预估 width - 4 的完整快照。
5. ReadOnlySqlEditor 使用相同 gutter 规则；极小窗口的 Constraint::Min(1)、边框与实际 body.width 必须一致，避免预览布局宽度与渲染宽度不一致。
6. 增加真实 UI 调用的计数断言：Ready 文本面板一帧只有一次预览快照请求。

**Commands:**
```bash
cargo test --lib preview_
cargo test --test ui_render
cargo test --test redis_browser_tabs
```

**Expected:** gutter 与坐标回归通过，一帧请求数从 2 降至 1，SQL/DDL 共用只读渲染不回归。

**建议提交：** `perf(redis): render preview once with logical line gutter`

## Task 3：建立会话级静态文档和高亮缓存

**Files:**
- Create: `src/editor/preview.rs`。
- Modify: `src/editor/mod.rs`（EditorSession、open_session、set_read_only_text、record_changed、预览快照入口）。
- Reference: `src/security.rs`（DisplayLineProjection / project_editor_line）。
- Test: `src/editor/tests.rs`、`src/editor/preview.rs` 内测试。

1. 添加缓存失效契约测试：相同 revision 连续快照、同文本 set_read_only_text、不同文本替换、语言变化、同 UUID 重开、关闭会话释放。
2. 为 EditorSession 添加 preview cache，初始化为空；建立按 revision 读取全文一次的入口，计算所有逻辑行起点、投影和最大宽度。
3. 将 preview_highlight_spans 的纯高亮逻辑迁入 preview 模块，按逻辑行懒构建，缓存 token kind 而非最终颜色。
4. 静态投影通过共享所有权复用；先在 preview 内共享，旧 EditorRenderLine 的兼容适配仅发生在可见行出口。不得每帧 clone 整个文档缓存。
5. 新缓存不保存 selection_newline、当前选择或光标等动态字段。
6. Preview 的 Wrap Off 路径也读取缓存和可见逻辑行，绕开 render_snapshot_with_options 的 full_text/full_line_width；SQL 分析路径继续使用其既有缓存。
7. 审计内容修改入口：既有 revision 检查覆盖变更；重开替换会话自动释放缓存。对仍可能同 revision 换 buffer 的入口明确清空。
8. 基准增加缓存估算占用，至少统计字符串容量和坐标数组容量；接近 8 MiB 输出样本测冷启动和内存增长，禁止无界保留旧 revision。

**Commands:**
```bash
cargo test --lib preview_
cargo test --lib read_only_
```

**Expected:** 相同版本与语言重复访问不重建投影和已高亮行；主题变化复用 token；文档替换不显示陈旧内容。

**建议提交：** `perf(editor): cache preview document projections and highlights`

## Task 4：缓存换行索引，只构建可见快照

**Files:**
- Modify: `src/editor/preview.rs`（PreviewWrapIndex、视觉行迭代器）。
- Modify: `src/editor/mod.rs`（render_wrapped_preview_snapshot、动态 snapshot 构建）。
- Test: `src/editor/tests.rs`、`src/editor/preview.rs`。

1. 添加 wrap 索引边界测试：空行、宽度 0/1、中文宽字符、Tab、控制字符投影、恰好整倍宽度、连续长行、末尾空行。
2. 从静态投影构造每行 wrap_starts 与 line_visual_starts，缓存 key 只包含影响换行的内容版本、有效宽度及实际存在的投影参数。高度和光标不能进入 key。
3. 以二分定位首个可见逻辑行，迭代 H+2 个视觉行，按需请求这些逻辑行的高亮；不先创建高度等于 total 的完整 snapshot。
4. 从编辑器 buffer 读取当前光标、选择和模式，叠加到可见 snapshot。复用/抽取既有动态选择映射逻辑，避免独立复制一套 SQL 选择实现。
5. 跨全文选择仅对可见行计算 selection_cells/selection_newline，不建立覆盖整个选区行数的临时 Vec/HashSet。
6. 保留 source 坐标和 wrap_offset 的现有约定；可见的同一逻辑行只高亮一次。
7. 高度变更只更新窗口范围，宽度变更只重建 wrap 索引；保存首可见源位置锚点，布局改变后再夹紧并保持光标可见。
8. 添加测试工作量约束：预热后同视口移动，全文投影和 wrap 构建增量为 0；跨屏只高亮新逻辑行；构造的行数不超过 H+2。

**Commands:**
```bash
cargo test --lib preview_
cargo test --lib wrapped_preview
cargo test --test mouse
```

**Expected:** 视觉输出、鼠标映射和选区与基线一致；快照构建不遍历全文，不因高度变化重建换行。

**建议提交：** `perf(editor): virtualize wrapped preview snapshots`

## Task 5：使用换行索引处理光标跟随和滚动

**Files:**
- Modify: `src/editor/mod.rs`（ensure_cursor_visible_at、scroll、所有 preview_layout 消费者）。
- Modify: `src/editor/preview.rs`（源/视觉坐标查询、视口状态）。
- Reference/Modify as needed: `src/runtime.rs`（sync_redis_preview_viewport，确认高度/滚动含义）。
- Test: `src/editor/tests.rs`。

1. 添加从顶部/中部/底部执行相同移动的工作量回归，以及宽字符行尾、缩放后的光标可见性测试。
2. 用明确的 PreviewViewportState 替换现有四元组的视口职责，所有消费者使用同一 wrap index。
3. ensure_cursor_visible_at 在预览分支进入前不调用 self.text；由源字符列查投影 cell，再用行内二分得到视觉行，按高度调整 offset。
4. scroll 的预览分支也先使用缓存；显式滚轮滚动保持当前行为，不能每帧渲染都强制拉回光标所在位置。
5. Wrap Off 使用当前行投影及逻辑行数，不执行全文 split。宽度/高度变化、Wrap 切换时执行一次坐标转换和边界夹紧。
6. 审计翻页、同步视口、鼠标拖动滚动条等读取旧 preview_layout 的分支，清除重复 wrap 算法。
7. 测试 gg/G、count motion、Ctrl-d/u、PageDown/Up、Wrap Off/On、窗口缩小后再次按键、底部最后一行及空文档。

**Commands:**
```bash
cargo test --lib wrapped_preview
cargo test --lib preview_
cargo test --test redis_browser_tabs
cargo test --test mouse
```

**Expected:** 已有翻页/定位行为通过；预热后定位不物化全文、不重建前序行投影；底部与顶部查询工作量同阶。

**建议提交：** `perf(editor): locate preview cursor through cached wrap index`

## Task 6：只读输入取消前后全文快照

**Files:**
- Modify: `src/editor/mod.rs`（input_vim_key、apply_action、sync_session_from_buffer、历史/effect 路径）。
- Test: `src/editor/tests.rs`。
- Reference/Test: `src/input/keymap.rs`、`tests/redis_browser_tabs.rs`。

1. 审计 apply_action 的只读写入保护，确认不能仅因去除文本比较而漏掉修改动作；审计 input_vim_key 后半段对 before/after position、jump_history、history 的依赖。
2. 添加真实输入回归：hjkl、10j、w/b/e、f/F/t/T、搜索和 n/N、v/V、yy/yw/Visual y、取消 pending；确认文本/revision 不变，复制结果与 baseline 相同。
3. 将按键前后状态拆成轻量位置/模式状态和可选的编辑文本历史；ReadOnly 分支不创建 EditorSnapshot.text，不执行全文比较或编辑 history。
4. 保留位置跳转记录、modalkit 命令解析、寄存器同步、Yanked effects；需要复制或搜索正文的动作可以按自身语义读取数据，普通 hjkl 不承担这些成本。
5. 不增加裸字符 hjkl 的绕路实现；操作符续键、数量前缀、组合键仍进入同一 Vim 解析链。
6. 加入工作量断言：预热后至少 1,000 个有效移动不增加全文物化计数，revision 不变，文档/换行缓存不失效。
7. 跑只读与可编辑器回归，验证 SQL 编辑、撤销重做及程序化替换仍保留历史。

**Commands:**
```bash
cargo test --lib read_only_
cargo test --lib editor::tests
cargo test --test keymap
cargo test --test redis_browser_tabs
```

**Expected:** 只读导航不复制全文，Vim 行为和 editable history 均通过。

**建议提交：** `perf(editor): skip text snapshots for read-only navigation`

## Task 7：长行共享与渲染端尾部优化（以基准结果决定）

**进入条件：** Task 2–6 后，长行 fixture 的快照/渲染仍显著随整行长度或可见 wrap 段数放大，或者内存复制占主要时间。

**Files:**
- Modify as needed: `src/model/editor.rs`（共享静态行表示）。
- Modify: `src/editor/preview.rs`、`src/ui/read_only_sql.rs`。
- Modify as needed: `src/ui/mod.rs`（editor_line_spans、选择目标注册及实际消费者）。
- Test: `src/editor/tests.rs`、`tests/ui_render.rs`、`tests/mouse.rs`。

1. 测量每个 wrap 段的完整逻辑行克隆、editor_line_spans 以及 Paragraph 横向跳过前缀的耗时。
2. 若证实为热点，将静态行内容以共享对象承载，visual row 只携带逻辑行引用与显示 cell 范围；不使用泄漏 'static 生命周期的办法消除 clone。
3. 根据可见 segment 的字符边界截取 token/spans，避免每个屏幕行都为整个长逻辑行分配和生成 spans。
4. 保留原始源字节范围/字符映射；若向 Paragraph 传入的已经是分段文本，不再重复应用 wrap_offset。鼠标选择、跨段复制和搜索命中使用源坐标。
5. 将高亮器 token 扫描中的从头 position 查找改为单调索引前进，仅当高亮采样显示为热点时实施。
6. 对超长行的最前/中间/最后一段测试颜色、双宽字符、选区、复制文本和光标位置。

**Commands:**
```bash
cargo test --lib preview_
cargo test --test ui_render
cargo test --test mouse
cargo test --release --lib preview_navigation_benchmark -- --ignored --nocapture --test-threads=1
```

**Expected:** 可见段构建/分配随可见字符增长，长行滚动不重复扫描全部前缀，交互坐标无回归。

**建议提交：** `perf(ui): render preview segments from shared line data`

## Task 8：重绘合并（仅在真实输入仍积压时实施）

**进入条件：** 核心计算优化完成后，真实终端采样仍显示逐事件绘制/输出是主要瓶颈。只修改 33 ms ticker 不能作为本任务实现。

**Files:**
- Modify: `src/runtime.rs`（事件消费、redraw 调度及测试模块）。
- Test: `src/runtime.rs` 内测试；必要时新增 `tests/runtime_redraw.rs`。

1. 使用真实终端输入回放或计时 trace 区分事件处理、UI 构建、终端 diff/write，记录输入队列延迟。
2. 抽取最小 redraw 调度状态，在已排队事件中有界地顺序执行；每批最多 32 个事件或 2 ms，先到上限则绘制，参数依据测量调整。
3. 只合并绘制，保留每个 KeyEvent 的顺序和次数。没有积压时立即绘制，不引入固定等待。
4. Resize、焦点变化、Paste、模式/弹窗切换作为明确边界，及时同步 viewport，防止后续翻页使用旧高度。
5. 测试 10j、混合 hjkl、搜索输入与 Enter、Visual 后 yank、Resize 后 PageDown、多来源 Action 和持续输入时绘制不被饿死。
6. 若收益不足或基线已满足，记录该阶段跳过原因，避免把全应用事件循环修改作为核心优化的依赖。

**Commands:**
```bash
cargo test --lib redraw
cargo test --test keymap
cargo test --test redis_browser_tabs
```

**Expected:** 所有输入被依次处理，突发输入的绘制次数减少，首键延迟不增加；持续输入有明确绘制时间上界。

**建议提交：** `perf(runtime): coalesce redraws for queued input`

## Task 9：最终性能对照、兼容性验证与交付

**Files:**
- Update: `docs/performance/redis-preview-navigation.md`。
- Test: `src/editor/tests.rs`、`tests/redis_preview_serialization.rs`、`tests/redis_browser_tabs.rs`、`tests/ui_render.rs`、`tests/mouse.rs`。

1. 使用 Task 1 同一 fixture、同一 release 构建方式和同一终端尺寸跑对照；记录每个阶段而不只给总加速倍数。
2. 表格包含 cold build、warm key、warm snapshot、full UI、真实终端按键到可见变化、缓存估算内存；无法测得的项目标为未测。
3. 人工检查 Java/JSON/YAML/Raw/Hex 文本预览；查看 DDL、SQL 输出、TextDetail 等共用组件，确认行号、选区和滚动条一致。
4. 检查异步旧结果保护仍以 connection、preview_generation、format 为准；缓存不能复活旧 key 的文本。同 key 刷新和格式切换分别复测。
5. 执行最终 CI 等价检查，完成后不在无新增变更的情况下反复全量测试。

**Commands:**
```bash
cargo test --release --lib preview_navigation_benchmark -- --ignored --nocapture --test-threads=1
cargo test --release --lib preview_ui_benchmark -- --ignored --nocapture --test-threads=1
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

**验收门槛：**
- Ready 文本预览每帧只请求一次快照。
- 内容/宽度不变且完成预热后，hjkl 不物化全文、不重建投影/换行；相同视口不重复高亮。
- 普通视口快照只构建 H+2 以内的视觉行；跨屏最多补充新可见逻辑行的高亮。
- 源坐标到视觉行的定位不扫描前序逻辑行；宽度变化重建一次，高度变化不重建。
- 同 UUID 重开、文本刷新、格式切换、会话关闭的缓存生命周期测试通过。
- 1k/10k 行在顶部/底部、相同可见内容下的 warm 导航耗时不出现随全文大小/深度的近线性增长。
- 性能目标：常用 120×40 与 240×65 cell 布局、10k 行标准 fixture 上，warm key + snapshot P95 尽量低于 5 ms；常见终端完整帧 P95 尽量低于 16 ms。具体机器与终端配置必须随结果提供，CI 以确定性的工作量断言为硬门槛。
- 冷启动、最大样本与内存单独报告；若投影缓存造成不可接受的内存/首屏增长，先改为紧凑行元数据和按需详细投影再交付，不通过关闭 Wrap 或高亮掩盖问题。

**建议提交：** `docs(perf): record redis preview navigation improvements`

## 2. 依赖、里程碑与预估

```text
Task 1 基线
  → Task 2 单快照
  → Task 3 静态缓存
  → Task 4 换行索引与可见快照
  → Task 5 索引化定位
  → Task 6 只读输入轻量化
  → 性能复测
      → 按需 Task 7 长行优化
      → 按需 Task 8 重绘合并
  → Task 9 最终验收
```

| 里程碑 | 交付内容 | 粗略工程时间 |
| --- | --- | --- |
| M1：可测量的小修 | Task 1–2，基线、逻辑行号、单快照 | 0.5–1 天 |
| M2：核心热路径消除 | Task 3–6，缓存、虚拟化、定位、导航 | 2–3 天 |
| M3：验收 | Task 9，回归、真实性能对照 | 0.5–1 天 |
| 按需扩展 | Task 7–8，长行共享和调度 | 另加 1–2 天 |

时间为包含测试和评审的初步估计；Task 1 的长行与内存结果可能调整后续工作量。核心交付是 M1–M3，Task 7/8 由可重复的测量结果触发。
