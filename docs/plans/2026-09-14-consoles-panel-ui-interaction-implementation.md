# Consoles 面板显示与鼠标交互 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
>
> 执行适配：若环境没有上述技能，按本文顺序逐项实施即可。本文接口和测试名为拟定名称，实施时遵循现有命名。无需子代理；仅在用户要求提交时创建 Git commit。针对布局和交互风险补充行为测试，不为机械搬迁代码编写镜像测试。

**Goal:** 让 Consoles 显示绑定数据库图标和统一连接状态点，使目标信息贴齐内容区右侧，并支持整行单击打开、底部操作点击以及长列表导航。

**Architecture:** 保留 Action → App::update → Command 的业务路径，将 Consoles 渲染拆为输入区、可滚动列表区、固定操作区。每条记录按 ExecutionTarget 派生图标和状态；渲染与 HitRegion 使用同一份布局结果，鼠标和键盘复用业务 Action。

**Tech Stack:** Rust 2024 / Rust 1.94、Ratatui 0.30.2、Crossterm 0.29、现有 IconSet、Theme、SessionRegistry、Unicode 单元格宽度工具；无需新增生产依赖。

---

## 1. 基线、范围与衔接

分析日期：2026-09-14。行号仅作定位参考，执行时重新按符号定位。

已确认代码：

| 文件与符号 | 事实 |
| --- | --- |
| `src/ui/mod.rs:5893 render_console_manager` | 所有行及 footer 拼成一个 Paragraph；弹窗最大宽 72、高 24 |
| `src/ui/mod.rs:5940–6001` | 详情预算最多 40 列，名称只截断不补齐；导致目标右边界和选中背景不固定 |
| `src/ui/mod.rs:5955–5960` | 已按完整 ExecutionTarget 查询 Session，但最终颜色还依赖 record.open |
| `src/ui/mod.rs:2566 explorer_list_item` | 已有数据库图标及类型颜色 |
| `src/ui/mod.rs:6699 connection_status_spans` | Online/Offline/Linking/Syncing/Failed 的统一样式来源 |
| `src/ui/shortcut_hints.rs` | 可排布和截断提示，但没有返回逐项可点击区域 |
| `src/input/mouse.rs:485–547` | overlay 白名单会阻止未放行的 Consoles 点击 |
| `src/app.rs:5026,5289,13808` | Enter 和 ActivateSqlEditor(UUID) 最终共用 activate_sql_editor |
| `src/model/sql_editor_list.rs` | 选择基于 UUID，支持循环移动和过滤协调，无列表 viewport 状态 |
| `src/app.rs:829 visible_console_records` | 按 UUID 去重、过滤，当前排序为 open 优先，再按名称和 UUID |

相关计划：`docs/plans/2026-09-14-multi-connection-workspace-consistency-implementation.md`。

- 跨 workspace 的记录来源和激活归属由该计划统一治理。本计划验证“列表可见记录可被公共入口激活”，不新增另一套 workspace 切换逻辑。
- 若执行时该计划已落地，以新的统一记录入口为准；本计划中所有状态仍按记录自身 ExecutionTarget 解析。
- 本计划不改变 Console 排序、SQL 执行语义或连接触发策略。打开行为与当时的 Enter 行为保持一致。
- 建议优先完成统一记录/激活入口，再做本计划的跨 workspace 端到端验收；纯 UI 布局、状态和热区工作不依赖该重构完成。
- 本次只创建计划文档；下文测试命令是执行阶段要求，并非已经通过的结果。

## 2. 冻结行为契约

### 2.1 行结构与图标

```text
[选择标记][数据库图标] [名称列] [状态点] [弹性间距][右对齐目标]
```

1. 名称前使用 `IconSet::database(profile.kind)` 和 `database_color()`。
2. profile 存在但 database/schema 失效，仍保留数据库类型图标。
3. profile 已删除或未绑定，显示中性 `?`，颜色为 theme.muted。
4. 选中行背景覆盖 inner 的整行宽度；空白区域也可点击。
5. 同一 viewport 内名称及状态列对齐；名称列宽按过滤后记录计算并限宽，避免滚动时列位置跳动。
6. 图标、名称和目标先做终端文本安全处理，再按显示单元格计算宽度。

### 2.2 状态语义

去掉 OPEN/CLOSED、已连接/未连接文字。**圆点仅表达该记录绑定目标的连接状态，不编码标签页打开状态。**

| 绑定/会话 | 标记 | 色彩 | 右侧内容 |
| --- | --- | --- | --- |
| 未绑定 | ○ | muted | 未绑定 |
| profile 不存在 | ● | error | 失效目标 |
| target.is_valid(profile) 为 false | ● | error | 连接名 database/schema · 失效目标 |
| 有效目标，无 Session | ○ | muted | 连接名 database/schema |
| Session Connecting | ◐ | warning | 连接名 database/schema |
| Session Connected | ● | accent | 连接名 database/schema |
| Session Failed | ● | error | 连接名 database/schema |

优先级：未绑定 → 失效校验 → 精确目标 Session 状态。禁止用 app.connection 或 profile 聚合状态替代精确目标状态。检查 SessionRegistry 的查询接口是否包含连接尝试；如 Connecting/Failed 在 attempts 中，复用能查询尝试状态的现有接口，必要时补只读查询方法，不维护 UI 状态副本。

共享状态样式方法返回 `(marker, color)`，不带前置空格或文字。Explorer 继续保留其 CONNECTING/SYNCING/FAILED 文案，Consoles 只取 marker/color。继续沿用 Explorer 的状态字符；数据库图标按现有 IconSet 模式切换，本次不扩大为整个 Explorer 的 ASCII 改造。

### 2.3 目标与宽度降级

- 正常：`连接名 database/schema`；schema=None 时显示 `连接名 database`，不输出 `/-`。
- 目标最后一个可见字符落在列表行内容区最右列，不侵入边框，不增加额外尾部空格。
- 名称和状态之间至少一列，状态与目标之间至少一列；总宽不够时先缩短名称和目标，不能相互覆盖。
- 优先完整显示目标；超长时采用单元格安全的中间省略，保留连接名开头和 namespace 末尾。
- 失效文案作为独立后缀优先预留预算，不被普通路径截断吞掉；极窄终端允许对整个短状态文案安全裁切。
- 默认保持当前弹窗宽度上限；通过内容布局解决空白，不靠扩大弹窗掩盖问题。
- 极低高度优先保留当前模式的关键操作，允许列表 viewport 高度为 0；不发生减法下溢、不在边框外注册热区。

### 2.4 点击、滚动与模式

- Browse/Search：左键单击整行直接打开该 UUID；按下一次触发一次，不在 MouseUp 重复打开。
- footer 每项包含按键和说明文字，整项可点击；间隔空白没有操作热区。
- `j/k move` 拆为 `j down`、`k up` 两项；对应 +1、-1。
- 滚轮仅在列表区移动选择，复用 SqlEditorListMove，默认每个滚轮事件移动一条；移动后选择保持可见。
- footer 固定在底部，宽度足够一行，否则按完整项目换行。优先保留 open/close 及当前模式确认/取消；通常最多两行，放不下的项目不注册热区。
- 空列表：open/delete/rename/down/up 禁用；new/search/close 仍可用。禁用项用 muted 样式且没有有效操作热区。
- Search 的 Enter 打开选择，Esc 清空搜索返回 Browse；Rename 的 Enter 保存，Esc 返回 Browse；DeleteConfirm 复用现有确认流程。
- Rename/DeleteConfirm 不保留底下 Browse 行热区；点弹窗外不触发底层面板操作。

## 3. 分步实施

每项中的编号为独立工作步骤。回归测试先运行确认暴露目标问题，再补实现；某测试已通过时记录事实，避免为了制造失败破坏代码。

### Task 1：建立显示与激活基线

**Files:**
- Test/Modify: `tests/ui_render.rs`
- Test/Modify: `tests/mouse.rs`
- Inspect: `src/app.rs`、`src/model/session.rs`、`src/ui/mod.rs`

1. 读取现有 ui_render 和 mouse 的 App/Profile/Session 构造方法及 TestBackend 渲染 helper；复用，避免新建通用测试框架。
2. 构造同名不同 UUID、不同数据库、同 profile 不同 database/schema、未绑定、失效目标、连接中/失败的记录集合。
3. 加入 `consoles_panel_target_status_is_independent_of_open_state`：两条绑定同一在线目标、open 不同的记录得到同样在线标记颜色。
4. 加入 `consoles_panel_right_aligns_targets_and_fills_selection`：断言右侧实际 cell 位置与选中背景，而不是仅断言字符串包含。
5. 运行 `cargo test --test ui_render consoles_panel_`，确认当前实现的失败点为显示契约。
6. 通过现有激活入口验证跨 workspace 记录。若仍有列表可见但找不到记录，记录为关联计划的前置问题，后续鼠标测试不可绕过入口掩盖失败。

**完成标准：** 有可重复的状态、右对齐回归场景；明确当前激活入口的适用基线。

### Task 2：统一数据库图标、状态与目标表示

**Files:**
- Modify: `src/ui/mod.rs`（connection_status_spans、模块声明）
- Create: `src/ui/console_manager.rs`
- Modify if necessary: `src/model/session.rs`（只读状态查询）
- Test: `tests/ui_render.rs`

1. 在 ui/mod.rs 提取状态 marker/color 的小型纯函数，让 connection_status_spans 调用它，原 Explorer 文案、间距和颜色保持原实现。
2. 新建私有 console_manager UI 子模块；将 render_console_manager 的主体迁移进去，保留调用点并通过 super 访问已有 UI helper。
3. 为单条记录实现派生呈现信息：profile_kind、target_valid、status、target 文本。仅持有渲染所需派生数据，不写回 ConsoleRecord。
4. 引入 IconSet 参数，调用现有 database/database_color；通过 overlay 渲染调用链传入当前模式，而非创建默认 IconSet。
5. 检查 SessionRegistry::get 及 attempts 读取语义，保证 Connecting/Failed 也能被正确观察。
6. 按第 2 节规则去掉 OPEN/CLOSED 和中文连接状态词，保留状态点及目标。
7. 运行 `cargo test --test ui_render consoles_panel_` 与 `cargo test --test ui_render explorer`；状态用例应通过，未完成的布局用例可仍失败并明确记录。

**完成标准：** 目标状态来自精确 Session；开闭状态不影响状态点；Explorer 的显示没有非预期改变。

### Task 3：实现行布局与真正右对齐

**Files:**
- Modify: `src/ui/console_manager.rs`
- Test: `tests/ui_render.rs`

1. 在子模块实现行布局计算：输入 row Rect、名称列预算、图标显示宽度，输出 marker/icon/name/status/target 子 Rect；所有宽度用 saturating 运算。
2. 根据过滤后的所有名称计算稳定的 name 列预算，上限建议为可用宽度约三分之一；目标紧张时进一步压缩，优先保证状态点和最少列间距。
3. 先绘制整行背景，再渲染各子 Rect；目标在自己的 Rect 内右对齐，禁止再用固定 40 列字符串拼接模拟布局。
4. 实现或复用 Unicode 安全的中间省略；若现有工具仅支持末尾截断，新增仅服务目标显示的小 helper，失效后缀独立预留预算。
5. 增加多宽度渲染用例：终端宽 40、72、100，短名称、中文、长 namespace、NerdFont/Unicode/Ascii 图标模式。断言无越界、无覆盖、目标贴右以及整行背景。
6. 运行 `cargo test --test ui_render consoles_panel_`，已建立的显示用例全部通过。

**完成标准：** 名称变短不会把目标拉离右边界；图标宽度变化不会破坏布局。

### Task 4：拆分固定 footer 与列表 viewport

**Files:**
- Modify: `src/ui/console_manager.rs`
- Modify: `src/ui/mod.rs`（UiState 的 Consoles viewport 状态及初始化）
- Inspect/Modify only if needed: `src/model/sql_editor_list.rs`
- Test: `tests/ui_render.rs`

1. 从 Paragraph 整体裁切改为显式分配 search_area、list_area、footer_area；先根据 footer 项目测量高度，再计算 list_area。
2. 优先将渲染滚动 offset 保存在 UiState 中的专属 Consoles viewport 状态：包含 offset、最近 query、弹窗是否可见；selected_id 仍由 SqlEditorListState 管理。
3. 开启弹窗/查询改变时重置或重新夹紧 offset；选择移出可见范围时将 offset 调整到能看到选择的位置；resize、删除、空结果时夹紧到合法范围。
4. 可将 `offset、selected_index、total、height → 可见范围与新 offset` 提取成纯函数。无需为了像素/单元格布局给 App 增加 viewport Action；若现有项目模式已有可复用状态则优先沿用。
5. 只渲染范围内记录，保存实际 UUID 和 row Rect，为后续 HitRegion 使用。
6. 搜索和重命名继续调用 render_text_input/register_input_selection_target，保留光标、选区、水平偏移功能。
7. 增加 40 条记录场景：循环移动至末项、向上回绕、搜索缩小、删除最后项、终端变矮，断言选择可见且 footer 不被列表挤掉。
8. 运行 `cargo test --test ui_render consoles_panel_`，新增 viewport 用例通过。

**完成标准：** 列表和 footer 高度互不侵占，所有实际展示行可建立准确坐标映射。

### Task 5：结构化底部操作与共享布局结果

**Files:**
- Modify: `src/ui/shortcut_hints.rs`
- Modify: `src/ui/console_manager.rs`
- Modify: `src/ui/mod.rs`（新增有限类型的 HitTarget）
- Inspect: `src/help.rs`、`src/input/keymap.rs`、`src/action.rs`
- Test: `tests/ui_render.rs`

1. 为 Consoles 定义有限的 UI 操作类型，例如 Down/Up/Open/New/Delete/Rename/Search/Close/Save/Cancel；不要把任意 Action 装进无约束通用 HitTarget。
2. 从现有 Help/键盘上下文核对可见按键与模式行为；构造各模式 footer 项目表，包含 key、description、operation、enabled。
3. 为 shortcut_hints 新增返回逐项 Rect/索引的布局接口，支持最多两行和完整项目换行；保留现有 line/render 的兼容行为，避免其他面板跟着改变。
4. 同一个布局结果用于绘制和热区注册，提前考虑 disabled、被省略项、换行和优先项；不要根据渲染后的文本重新反推坐标。
5. 普通操作在 footer 使用结构化渲染；删除确认继续复用 dialog::render_actions 的现有按钮及精确区域，避免重复实现确认按钮。
6. 增加测试：窄屏项目换行、按键与说明文字均在同一热区、间隔无热区、空列表禁用项、Rename 错误增加一行后保存/取消仍可见。
7. 运行 `cargo test --lib shortcut_hints` 和 `cargo test --test ui_render consoles_panel_`。

**完成标准：** footer 文案、可用性与点击位置由统一结构决定，其他面板原 footer 行为保持一致。

### Task 6：接通整行点击、footer 与滚轮

**Files:**
- Modify: `src/ui/mod.rs`（HitTarget）
- Modify: `src/ui/console_manager.rs`（注册热区）
- Modify: `src/input/mouse.rs`
- Test: `tests/mouse.rs`

1. 新增 `HitTarget::ConsoleRow(Uuid)` 及有限操作热区类型。每个可见 row 的 area 等于整行 Rect，不能仅覆盖文字。
2. 在鼠标 overlay 拦截处显式处理 SqlEditorList 的模式和允许的热区，之后再进入具体映射。禁止只把 SqlEditorList 无条件加入全局放行。
3. 左键 Down 的 ConsoleRow 映射到 `Action::ActivateSqlEditor(id)`；仅 Browse/Search 接受该目标。
4. footer 映射复用现有 Action：Open→SqlEditorListActivate、New→Create、Delete→DeleteRequest、Rename→RenameStart、Search→SearchStart、Close/Cancel→Cancel、Save→RenameCommit。
5. DeleteConfirm 按钮沿用 DeleteActivate/DeleteCancel 的现有语义；确认点击、取消点击和键盘聚焦行为分别验证。
6. 在 ScrollUp/ScrollDown 的 overlay 提前返回之前，限定 list_area 内的 Browse/Search 滚轮事件，映射 SqlEditorListMove(-1/+1)。footer 与弹窗外滚轮不影响列表或底层面板。
7. 补全 mouse.rs 对 HitTarget 的其他穷举分支，尤其光标/拖拽/文字选区分支；整行单击不得启动编辑器选区。
8. 在 tests/mouse.rs 复用 assert_click_maps/click_action，测试行左端、中间空白、右端；同名 UUID、过滤后、滚动后都映射正确。
9. 加入 footer 每项点击、禁用项、模式切换残留热区、弹窗外点击、MouseUp 不重复触发、滚轮位置限制测试。
10. 运行 `cargo test --test mouse consoles_panel_`，全部通过。

**完成标准：** 鼠标真正通过 overlay 拦截层并产生正确 Action；没有向底层面板穿透。

### Task 7：端到端行为与跨计划接口验收

**Files:**
- Modify/Test: `tests/mouse.rs`
- Modify/Test if needed: `src/app.rs` 的现有 console_manager 测试
- Inspect: `src/app.rs`（activate_sql_editor、visible_console_records、重命名/删除入口）

1. 点击已打开 Console：执行 map_mouse 的 Action，经 App::update 后断言 active_tab、focus、overlay，并确认未创建重复 Tab。
2. 点击已关闭 Console：断言恢复原 UUID 和原 SQL 文档，保持原目标绑定；通过已存在的编辑器 fixture 检查文档内容。
3. 点击 footer 打开与 Enter 分别从相同初始 App 执行，比较最终活动 Console、overlay、目标及 Command 类型，避免只验证 Action 名称。
4. 从非 SQL Tab 打开管理器并点击记录，验证 App::update 的全局 Action 限制允许通过。若使用新增 Action，必须同步其白名单；优先复用已有 ActivateSqlEditor 避免扩大该改动。
5. 验证打开、取消搜索、重命名、删除确认保持当前业务语义，new 继续使用打开面板时捕获的来源目标。
6. 运行多 workspace 可见记录的激活、重命名、删除场景；如果关联一致性计划尚未解决记录归属，明确记录失败和依赖，不能将其改为忽略测试或通过 UI 过滤隐藏记录。
7. 运行 `cargo test --test mouse consoles_panel_`、`cargo test --lib console_manager`、`cargo test --lib console_list`。

**完成标准：** 行和 footer 不仅能产生 Action，还能经公共业务入口完成操作；与键盘一致。

### Task 8：最终检查与人工验收

**Files:**
- Review: 上述全部变更文件
- Update: 本计划实施记录（实际执行时追加结果）

1. 运行 `cargo fmt --all -- --check`，预期通过；如需格式化，执行 cargo fmt --all 后仅审查本次相关差异。
2. 运行 `cargo test --test ui_render`、`cargo test --test mouse`，预期完整渲染和鼠标回归通过。
3. 运行 `cargo test --lib`，预期模型、键盘映射、App 和共享 helper 回归通过。
4. 运行 `cargo clippy --all-targets -- -D warnings`；若存在既有问题，记录具体错误和文件，不能宣称整体通过。
5. 在终端人工验收：短/长/中文名称、图标模式、连接中/在线/失败、未绑定/失效、70+条记录、搜索无结果、缩放终端、footer 换行。
6. 检查实际字体下图标占宽，单击左右整行都能打开；点击底部文字说明也能执行；搜索输入选区和重命名光标仍正常。
7. 审查 `git diff --check`、`git diff --stat` 和 `git diff`，确认变更符合本计划行为契约。
8. 记录执行命令及结果、人工验收结果、关联计划依赖状态。只有用户要求时才按逻辑阶段提交。

环境补充：默认 feature 包含 Oracle。若本机缺少相关构建环境，可额外用 `--no-default-features` 检查其他驱动路径；必须注明覆盖范围，不能将该结果等同于默认配置通过。

## 4. 依赖与推荐顺序

```text
Task 1 基线
  → Task 2 状态与图标
  → Task 3 行布局
  → Task 4 viewport
  → Task 5 footer 布局
  → Task 6 鼠标映射
  → Task 7 业务验收
  → Task 8 总体验收

多连接一致性计划的统一记录/激活入口 → Task 7 跨 workspace 验收
```

建议按顺序由同一实现者推进。ui/mod.rs、mouse.rs 和 App 入口存在共享修改点，不宜同时编辑。

如用户要求提交，可按三组组织：

1. `refactor(ui): share connection markers for console rows`（Task 2）
2. `feat(ui): align console targets and preserve list viewport`（Task 3–5）
3. `feat(input): support console row and footer clicks`（Task 6–7）

每组提交前运行覆盖其变化的定向检查；Task 8 完成全局验收。

## 5. 最终验收清单

- [ ] 每条 Console 显示自身绑定数据库的图标及颜色。
- [ ] OPEN/CLOSED 和重复连接状态文案移除，状态点与 Explorer 规则一致。
- [ ] 同目标的打开/关闭 Console 显示相同连接状态，同 profile 不同目标可显示不同状态。
- [ ] 未绑定、profile 删除、namespace 失效可区分，失效提示不会被普通目标路径截断隐藏。
- [ ] 目标末尾贴齐内容区右侧，中文和不同图标模式下无错位。
- [ ] 选中背景覆盖整行，整行任意位置单击打开正确 UUID。
- [ ] 所有显示且启用的 footer 操作可点击，换行后热区准确。
- [ ] 搜索、重命名、删除确认模式的操作正确，空列表禁用行为正确。
- [ ] 長列表选择保持可见，footer 固定，过滤/删除/resize 后 offset 合法。
- [ ] 鼠标事件不穿透弹窗，MouseUp 不重复打开，滚轮只在列表区生效。
- [ ] 点击与 Enter 共用激活行为，跨 workspace 依赖已通过或明确报告阻塞。
- [ ] 定向测试、完整相关回归、格式及 lint 结果已记录。
