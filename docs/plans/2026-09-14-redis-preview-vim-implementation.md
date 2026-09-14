# Redis Preview Vim Navigation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行环境若未安装上述技能，按本文件任务顺序执行与验证，不假定技能可用。

**Goal:** 将 Redis Preview 接入现有只读 Vim 编辑器，使光标移动、翻页、搜索、选择和复制具有一致且可验证的行为。

**Architecture:** 合并 Redis 按键分支，由当前子面板决定唯一输入上下文。复用 EditorWorkspace 的只读会话、modalkit 命令解析和剪贴板 effects；统一换行坐标映射，明确视口和异步内容更新的状态所有权。帮助、页脚与输入使用相同的 Preview 上下文。

**Tech Stack:** Rust 2024 / Rust 1.94、crossterm 0.29、ratatui 0.30.2、modalkit 0.0.25、现有 Action/Command 架构及 Rust 单元/集成测试。

---

## 0. 基线、范围与执行约束

- 本文件只规划实施，不代表代码已经完成，也不代表测试已经运行。
- 行号基于规划时源码，实施时以符号定位为准。
- 已确认：`src/input/keymap.rs:1303–1485` 的 Redis Results 分支始终返回，遮蔽 `1873–1939` 的同条件分支及其中的 ReadOnlyEditorKey 转发。
- 已确认：通用数字前缀在 `1217–1231` 抢占输入；前一个 Redis 分支的搜索、创建、编辑未全部限制在 Keys；纵向配置导航转换未区分子面板。
- 已有能力：`src/app.rs:1337` 识别 Preview 会话；`12762` 创建只读会话；`9232` 分发只读按键；`src/editor/tests.rs:1101–1167` 覆盖 Visual yank、yy、只读保护。
- 修正前期分析：`src/editor/mod.rs:2757` 已有 Wrap 光标可见性逻辑。需要复用并验证，不重新实现第二套跟随逻辑。确切缺口是 `key()` 的 Wrap 翻页分支只调用 scroll，以及布局/resize/坐标边界的统一性待验证。
- `ensure_read_only_session()` 当前未覆盖 Redis，首次加载前、会话缺失与旧内容失效需要明确策略。
- 规划时工作区存在用户修改：`src/ui/animation.rs`、`src/ui/data_grid.rs`、`tests/ui_render.rs` 及另一份计划。实施前重新检查 diff，对共享文件只添加本任务所需改动。
- 不新增 Vim 引擎、不升级依赖。先完成文本 Preview；TABLE 保持当前渲染形式，不在此任务扩展成可编辑数据表。
- 缓存性能优化以测量结果决定；功能验收不以引入复杂增量布局系统为前提。

## 1. 交互契约（作为测试依据）

| 上下文 | 输入 | 行为 |
| --- | --- | --- |
| Preview Normal | hjkl、方向键 | 移动文本光标，不修改 Redis Key 选择或焦点 |
| Preview Normal | w/b/e、W/B/E、0/^/$、gg/G、count G | 标准只读 Vim motion |
| Preview Normal | 10j、3w、5yy | 数量前缀归编辑器解析 |
| Preview Normal | f/F/t/T + 字符、;/, | 行内查找，应用不抢占后续字符 |
| Preview | Ctrl-d/u | 按可见内容区高度移动半页，保持光标可见 |
| Preview | Ctrl-f/b、PageDown/Up | 按可见内容区高度移动整页，边界夹紧 |
| Preview | j/k 与 gj/gk | 前者逻辑行，后者软换行后的视觉行 |
| Preview | v/V、Visual 下 o | 字符/行选择及切换选区端点 |
| Preview | yy、yw、y$、Visual y、ggVGy | 复制显示 buffer 的逻辑文本 |
| Preview | /、?、n/N | 搜索当前 Preview 文本；不触发 Keys find 或 Help |
| Preview Normal 且无未完成编辑器命令 | Space f/w/l | 格式选择、切换 Wrap、显式加载下一批 |
| Preview Search / operator pending / 字符查找 pending | 字符、空格、数字 | 归当前编辑器命令，不触发应用 leader |
| Preview Visual/Search | Esc | 退出当前选择/提示状态，焦点仍在 Preview |
| Preview Normal | Esc | 取消未完成序列；无序列时保持面板焦点 |
| Preview | Ctrl-w h/l、Tab/Shift-Tab | 按现有窗口约定切换面板，h/l 本身不切面板 |
| Preview | F1 | 打开上下文帮助；? 保留反向搜索 |
| Keys | /、n/N、a/e/d/y、hjkl | 维持 Keys 搜索、对象操作与树导航语义 |

应用已有 Ctrl-c/退出约定保留；有活动搜索或未完成序列时先按既有取消优先规则处理。SHIFT 编码的 G/V/W/?/$ 等与仅包含大写字符的事件应等价。不声称完整支持所有 Vim 命令，以上集合逐项验收。

## Task 1：建立经过真实 Keymap 的故障回归

**Files:**
- Modify/Test: `src/input/keymap.rs` 内测试模块。
- Reference: `tests/redis_browser_tabs.rs`、`src/editor/tests.rs`。

1. 新建局部 Redis Preview fixture：一个 RedisBrowserTab、两条 Key、已加载多行值、Focus::Results + Preview 焦点。通过现有加载 Action 初始化，避免只创建一个缺少会话的 tab。
2. 新建辅助函数逐个调用 `Keymap::map()` 和 `App::update()`，收集 Command；不直接调用 ReadOnlyEditorKey 绕开被测路由。
3. 添加 `redis_preview_routes_motion_to_editor`：hjkl 返回正确 session 的 ReadOnlyEditorKey，Key selection 和子面板焦点不变。
4. 添加 `redis_preview_count_and_yank_reach_editor`：10j、yy、ggVGy；可访问内部位置则断言坐标，复制断言 WriteClipboard 文本。
5. 添加 Keys 对照用例，验证相同输入在 Keys 保持树导航/复制 Key。
6. 运行 `cargo test --lib redis_preview_`，确认新路由用例在当前代码失败，记录失败原因。

**验收：** 测试能发现当前提前 return，不能只有 Action 直接调用或静态映射快照。

## Task 2：合并 Redis 路由与输入状态所有权

**Files:**
- Modify: `src/input/keymap.rs`（Keymap::map、pending/context、is_read_only_editor_key）。
- Modify: `src/app.rs`（只读会话查询接口，仅按需要调整 crate 可见性）。
- Modify: `src/editor/mod.rs`（若现有接口不能暴露 pending/prompt，则增加最小只读查询）。
- Test: `src/input/keymap.rs`。

1. 在测试中增加 `/e/a/n/N` 的 Preview/Keys 分离、confirmed Keys find 后切 Preview、空预览/加载中、SHIFT 大写事件。
2. 抽出 Redis 子面板判定及 `map_redis_keys`、`map_redis_preview`，保留唯一 Redis Results 入口。
3. 将 Preview 入口置于通用数字计数、grid navigation 和普通单字符全局快捷键之前；活动弹窗、明确的全局组合键和窗口序列仍有清晰优先级。
4. Keys find 的 Editing 状态要么明确持有输入焦点，要么在切出 Keys 时结束；Confirmed 状态不能抢 Preview 的 n/N。
5. Preview 不使用排除 1/2/3/o 的旧字符黑名单。只排除明确应用保留事件，其余交给只读会话；消费不支持的修改输入，禁止回落到 Redis mutation 分支。
6. 将 pending 身份包含 tab、Redis 子面板、会话/内容 generation；切焦点清应用 pending，并取消旧编辑器半条 operator/count，不丢正常阅读光标。
7. 确保 `f `、`r`、`3yy`、`y$` 等续键不会被应用 leader 截断；编辑器 prompt 活跃时优先输入 prompt。
8. 运行 `cargo test --lib redis_preview_` 和 `cargo test --test keymap`。

**验收：** Preview KeyEvent 单一路由，所有数字和 operator 由编辑器负责，Keys 行为无回归。

**建议提交：** `fix(redis): route preview keys through read-only editor`

## Task 3：Preview 专属快捷键与搜索交互

**Files:**
- Modify: `src/input/keymap.rs`、`src/help.rs`。
- Modify: `src/ui/redis_browser.rs`（预览控件标题）、`src/ui/mod.rs`（搜索 prompt 展示接入）。
- Test: `src/input/keymap.rs`、`tests/redis_browser_tabs.rs`。

1. 增加 `f{char}`、W、反向搜索 ?、Visual o、Space f/w/l 的路由和效果测试。
2. 从 Preview 删除裸 f/W 应用绑定；将 RedisPreviewCycleFormat、RedisPreviewToggleWrap、RedisPreviewLoadNext 接到上下文 leader。
3. 在帮助定义中加入 Preview 上下文动作，复用既有 leader/sequence 展示结构；不复用 SQL FormatCurrent、Run 等 editor application effect。
4. 确保搜索提示可见且焦点属于 Preview session；/ 和 ?、Enter、Esc、n/N 经过现有搜索执行链路。
5. F1 显示帮助，? 不再触发 Preview 帮助；鼠标格式/Wrap 控件仍调用原 Action。
6. 验证 Ctrl-w、Tab 与 Visual/Search 的优先级；焦点切换不留下隐藏输入提示或半条命令。
7. 运行 `cargo test --lib redis_preview_`、`cargo test --test redis_browser_tabs`、`cargo test --test keymap`。

**建议提交：** `feat(redis): add conflict-free preview shortcuts`

## Task 4：统一换行坐标与键盘翻页

**Files:**
- Modify: `src/editor/mod.rs`（key、scroll、render_wrapped_preview_snapshot、ensure_cursor_visible_at、page_cursor）。
- Create: `src/editor/preview_layout.rs`（仅承载抽出的共用坐标映射，不另建编辑器）。
- Modify: `src/runtime.rs`（sync_redis_preview_viewport）、`src/app.rs`（viewport actions）。
- Test: `src/editor/tests.rs`。

1. 建立固定 12 列 × 5 行视口测试；输入含短行、长行、中文、tab、emoji、空行和精确填满一行的文本。
2. 分别验证现有 j/k/G/搜索后 cursor_screen_cell；已有通过用例保留为回归，不预设全部失败。
3. 为 Ctrl-d/u、Ctrl-f/b、PageUp/Down 增加光标与 offset 联合断言，重现只 scroll 不移动光标的缺陷。
4. 抽取唯一视觉行映射：逻辑行、源字符区间、显示 cell 起点；render、ensure-visible、page motion 和鼠标 hit-test 使用一致边界规则。
5. 明确源字节偏移、字符索引和终端显示宽度的转换；复用现有 source_to_display_cells，不按 UTF-8 字节直接定位光标。
6. 替换 Wrap 模式 key() 的直接 scroll 分支：按真实 content viewport 高度计算半页/整页目标视觉行，映射回逻辑光标，更新可视区；边界及极小窗口使用饱和运算。
7. j/k 继续走逻辑 motion；验证 modalkit 当前 gj/gk 行为，不满足 Wrap 契约则在已有解析结果/运动适配层接入视觉 motion，不能新增独立 g 前缀状态机。
8. render 使用真实终端视口维度作为交互视口，避免用于构建全量文本快照的 height=total 污染翻页高度。
9. resize、Wrap 开关保留逻辑光标；重建视觉映射后进行一次必要的 ensure-visible。鼠标滚轮不强制光标跟随，下一次键盘移动再恢复跟随。
10. 排查 RedisPreviewViewportChanged 与 OutputViewportChanged 的双同步；文本编辑器只保留一个滚动权威，模型中仍供其他预览消费的字段不能直接删除。
11. 运行 `cargo test --lib editor::tests` 与 `cargo test --lib redis_preview_`。

**验收：** Wrap ON/OFF、窗口变窄、G/搜索/翻页后光标可见；视觉选区和复制文本无软换行污染；SQL/DDL/output 编辑器现有测试通过。

**建议提交：** `fix(editor): align wrapped preview paging with cursor state`

## Task 5：只读会话生命周期与异步更新

**Files:**
- Modify: `src/app.rs`（RedisValuePageFormatted、value-page success、ensure_read_only_session、format accept）。
- Modify: `src/model/redis_browser.rs`（仅在现有 generation 不足时补内容身份）。
- Modify: `src/editor/mod.rs`（set_read_only_text/替换策略按需扩展）。
- Test: `tests/redis_browser_tabs.rs`、`tests/redis_loading_lifecycle.rs`、`src/editor/tests.rs`。

1. 为 A Key → B Key → A 旧响应、关闭 tab 后响应、格式切换后旧格式结果增加状态断言。
2. 检查会话是否存在和内容身份是否匹配；无有效内容时不向旧 buffer 发送按键，加载/失败提示与实际可复制内容必须一致。
3. 将首次建立和新 Key 加载视为重置：Normal、首行、无选区、无 pending；清除旧搜索与布局缓存。
4. 同 Key 追加数据采用已有 set_read_only_text 或明确 Preserve 策略，不每页 open_read_only；保留阅读位置，不启用 output 的自动 follow-tail。
5. 追加保持原文本前缀不变时保留选区；格式化重排或无法可靠映射时取消选区、清 pending，光标夹紧到有效位置。
6. 同 Key refresh 优先保留有效逻辑位置；格式切换取消搜索/选区，明确重置到首行，避免假装可精确映射不同格式。
7. 异步结果继续校验 connection、tab、preview_generation 和 format。滚动/移动不额外触发加载；Space l 显式请求下一页，并沿用现有 single-flight 机制。
8. 补 Redis 会话缺失恢复：只使用当前 generation 的有效 Ready 文本；其他状态由视图消费输入，不创建带上一个 Key 内容的会话。
9. 运行 `cargo test --test redis_browser_tabs`、`cargo test --test redis_loading_lifecycle`、`cargo test --lib editor::tests`。

**建议提交：** `fix(redis): preserve preview state across value updates`

## Task 6：复制语义与只读行为端到端验收

**Files:**
- Modify: `src/app.rs`（CopyEditorYank、apply_editor_effects）。
- Modify: `src/action.rs`、`src/editor/mod.rs`（仅在必须传来源时扩展 effect；优先通用 Text selection 文案）。
- Test: `src/input/keymap.rs`、`src/editor/tests.rs`。

1. 经 Keymap → App.update 验证 yy、yw、y$、Vjy、ggVGy，断言 WriteClipboard 的精确文本。
2. 软换行不增加换行，行号/边框不参与复制；Vim linewise yank 保留其标准末尾换行语义。
3. 将固定 SQL selection 改为通用 Text selection；若 UI 需要来源，显式携带 session 来源，不在异步完成时猜当前活动 tab。
4. 当前 buffer 仅含已加载内容；页头/帮助中标明 loaded/truncated，不称完整 Value 导出。
5. 对 i/a/d/c/x/p/u/Ctrl-r 等输入断言文本、revision 不变，且没有 Redis mutation、SQL run 或对象编辑 Command。
6. 重复复制相同内容也应每次产生剪贴板 effect；验证 register 不变时现有实现是否漏发事件，若有则以实际 yank 动作完成为触发依据修复。
7. 运行 `cargo test --lib redis_preview_` 与 `cargo test --lib editor::tests`。

**建议提交：** `fix(redis): validate preview yank and read-only behavior`

## Task 7：状态栏、上下文帮助与文档

**Files:**
- Modify: `src/help.rs`（ShortcutContext、shortcut_context、动作注册）。
- Modify: `src/ui/mod.rs`（Focus::Results DATA badge 分支与页脚）。
- Modify: `src/ui/redis_browser.rs`（格式/Wrap 控件及内容范围提示）。
- Modify: `docs/redis-browser.md`。
- Test: `tests/ui_render.rs`（仅追加独立用例，保留用户已有改动）、`src/help.rs` 测试模块。

1. 新增 RedisKeys 与 RedisPreview 上下文；必要时由 editor mode 派生 Visual/Search 提示，避免所有 Redis 内容仍落入 DATA grid。
2. Preview 聚焦显示 NORMAL / VISUAL / V-LINE + READ ONLY；显示 Ln/Col、格式与 Wrap 状态，窄窗口按优先级裁剪。
3. 页脚提示 hjkl、半页、选择、复制、搜索、Preview leader；Keys 切回树操作提示。
4. 编辑器 pending/count/operator 与应用 leader 分别显示各自已有 sequence 状态，不创造第三套解析状态。
5. 确保提示、帮助和可执行快捷键使用同一注册定义；复制语义、Space f/w/l、? 反向搜索写入文档。
6. 添加局部文本断言：Preview 不显示 copy selected cell，Visual 有正确模式，Keys 帮助独立，窄屏不越界。
7. 运行 `cargo test --test ui_render redis_preview` 及 `cargo test --lib help`；确认过滤器实际执行了新测试而非 0 tests。

**建议提交：** `feat(redis): show preview vim mode and contextual help`

## Task 8：性能检查、完整回归与交付

**Files:**
- Inspect/Modify only if measured: `src/editor/preview_layout.rs`、`src/editor/mod.rs`。
- Test: `tests/redis_scale.rs`、`src/editor/tests.rs`。
- Update: `docs/redis-browser.md`（最终行为与限制）。

1. 使用项目当前允许的 preview payload 上限附近样本，分别测长单行 JSON 和多行内容的连续移动/resize。记录样本大小、窗口大小和测量方式，不新增未经讨论的硬性时延指标。
2. 若每次移动均重建全量映射且有可测卡顿，按 session revision + width + tab 展示规则缓存布局；格式/Wrap 变化正确失效。高度变化仅重算窗口范围，尽量复用宽度映射。
3. 缓存验证采用布局构建计数等确定性断言，避免 CI 毫秒阈值测试；不缓存与选区/光标绑定的过期快照。
4. 运行以下检查；依赖环境导致失败时记录失败原因，区分预存失败与此次回归：

```bash
cargo fmt --check
cargo test --lib
cargo test --test keymap --test redis_browser_tabs --test redis_loading_lifecycle --test redis_values --test redis_scale --test ui_render
cargo clippy --all-targets -- -D warnings
```

5. 若仓库 CI 有额外 feature/check 要求，按现行工作流补齐；不以 no-default-features 通过代替默认配置检查。
6. 人工 TUI 验收：小 JSON、超屏多行、中文长行、RAW/JSON/YAML/HEX/TABLE 现有表示、分页集合；逐一执行 motion、搜索、Visual yank、翻页、Wrap、resize、焦点切换、快速切 Key。
7. 在可用终端确认 Ctrl-w/Tab 事件行为、系统剪贴板结果；不要求自动测试访问真实 Redis 或系统剪贴板，自动测试使用现有 Action fixture 和 Command 断言。
8. 查看最终 diff 与测试报告，仅按逻辑任务显式暂存本次文件/改动块。提交步骤由实施时用户授权决定，规划阶段不提交。

## 依赖顺序与完成标准

```text
Task 1 回归基线 → Task 2 路由 → Task 3 快捷键
                          → Task 4 换行/翻页
                          → Task 5 会话更新
Tasks 3/4/5 → Task 6 复制验收 → Task 7 UI/文档 → Task 8 完整验收
```

默认按此顺序串行实施，共享 keymap/app/editor 文件不做并发编辑。

- [ ] 所有目标 Vim 输入经真实 Keymap 到达正确只读会话。
- [ ] Preview 文本操作不改变 Key selection，不触发对象创建/编辑/删除。
- [ ] 数字、operator、搜索、leader 状态归属清晰，焦点切换无串键。
- [ ] Wrap/非 Wrap 翻页高度正确，导航/搜索后光标可见，Unicode 映射一致。
- [ ] yy/Visual y 可复制，重复 yank 不漏，软换行和界面装饰不进入剪贴板。
- [ ] 新 Key 重置、同 Key 追加保留位置、过期异步响应不污染当前内容。
- [ ] 状态栏/帮助准确反映 Preview 模式及实际绑定。
- [ ] 相关单元、集成、格式、lint 检查通过，并记录人工验收结果。
