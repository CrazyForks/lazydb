# Omni / Help Mouse Interaction Implementation Plan

> **执行者：Luna。** 按端到端验收单元顺序实施；实现、审查、纠偏及提交合并均由 Luna 完成。仅分析/计划使用 Astra。不启动子 Agent，不要求用户反复 resume。本计划沿用用户指定任务目录，覆盖技能默认 docs/plans 路径及执行模式选择提示。

**Goal:** 为 Omni/Help 提供 Explorer 风格的滚动条、拖动与滚轮滚动、单击选择，以及右上角提示点击切换。

**Architecture:** 复用现有 scrollbar 几何和纵向绘制模式，以模型持久视口状态为依据，经 Action/reducer 更新选择和偏移。UiState 保存最新渲染几何和专属手势状态，runtime 按现有 viewport 同步模式更新尺寸；最上层面板拥有鼠标事件，标题点击复用 ToggleHelpPanel。

**Tech Stack:** Rust 1.94 / ratatui 0.30.2 / crossterm 0.29 / 现有 Action、App reducer、TestBackend 集成测试。

---

## 0. 基线、范围和执行约束

- 起点/本阶段 HEAD：`aff7947dd7cef2f4d7796211fda5bbf3514d5fc9`；目标 `main`；工作空间 `/Users/yelog/workspace/tui/lazydb`。
- 本 plan 阶段 `git status --short` 无输出，工作区干净。analyze 时出现的未跟踪 profile-url 计划现在不在 status 中；不推断其去向、不恢复/删除该文件。本功能不依赖任何未提交代码。
- 当前任务目录没有 checkpoint.json；若后续出现，优先读取并用实际 diff 校对进度。state.json、checkpoint.json 均由插件维护。
- 本阶段不创建任务分支/worktree，不实现业务代码。任务名与分支名留给工作流/Luna 自动命名。
- 报告、计划和验证日志只放在 `.git/opencode-tasks/ses_f3d1e0ae3ffeXd3inM67NwG1Xz/`。后续若使用 worktree，必须继续将任务材料写入原工作空间的此绝对目录。
- 本轮正式 plan 回执为 `plan-416f7df0-eeb1-4965-a923-f6b88c05aade.json`，token 为 `416f7df0-eeb1-4965-a923-f6b88c05aade`；在计划及 change-scope.json 完成后最后写入，不覆盖 analyze 回执。
- 分析依据见同目录 analysis.md。本计划落实其选定方案，不引入新依赖、不重构整个列表框架。

### 0.1 完整预计修改范围

以下 15 个文件均为现有文件，预计在实施阶段修改，无预计新增、删除或重命名业务文件。`change-scope.json` 列出同一集合；包含计划中的条件性修改，以免遗漏影响范围。仅执行现有测试而不修改的 `tests/omni_input.rs`、仅读参考的 `src/model/explorer.rs` 和 `.github/workflows/ci.yml` 不列入。

```text
src/action.rs
src/app.rs
src/help.rs
src/input/mouse.rs
src/model/help_panel.rs
src/model/omni.rs
src/runtime.rs
src/ui/mod.rs
src/ui/omni.rs
src/ui/scrollbar.rs
src/ui/text_selection.rs
tests/help_omni_panel.rs
tests/keymap.rs
tests/mouse.rs
tests/ui_render.rs
```

无未提交依赖文件需要转移；后续从指定起点创建 worktree 即具备本方案依赖。任务目录内的报告/回执是工作流元数据，不属于待提交业务变更范围。本阶段不额外创建 docs/plans 文档。

### 0.2 验证分级

| 类别 | 来源与要求 | 执行方式 |
| --- | --- | --- |
| 用户功能验收 | 两种面板滚动条、拖动/滚轮、行单击选择、右上角点击切换 | 单元 1–3 的 TestBackend -> map_mouse -> App.update -> 重绘证据必须覆盖 |
| 实现正确性与回归 | 为满足需求而选定的偏移边界、事件隔离、ID 选择、查询/resize/关闭生命周期规则 | 用定向自动化回归验证，不新增人工签字门禁 |
| 项目既有 Rust 检查 | `.github/workflows/ci.yml` 中 fmt、clippy all-targets/all-features、test all-targets/all-features | 功能齐备后执行单元 4 的三条命令，失败或环境限制如实记录；不以定向通过替代全量通过 |
| 项目其他 CI 作业 | 数据库服务矩阵、发布/分发及平台作业 | 继续遵循仓库原有 CI；不因本 UI 计划新增本地全平台复刻要求，不声称未运行的作业已通过 |
| 补充建议验证 | 真实终端/PTY 鼠标拖动、人工外观复核 | 有环境时补充；不是用户要求或新增必需门禁。受限最多一次针对性修复重试，随后由 Luna 审查证据/记录限制 |

下面各单元“验收”描述的是行为结果，默认用自动化测试证明，不隐含新增人工验收步骤。测试命令预期均为退出 0 且目标用例实际运行；任何零用例、跳过或环境不可用必须单独记载。

## 1. 固定行为契约

1. 列表溢出且高度足够时显示 `▲│┃▼` 纵向滚动条，rail muted、thumb accent，弹层背景 surface_raised。列表无溢出时隐藏；高度不足 3 时不用不可拖动的伪轨道。
2. 左键单击行只选择。Help 不再打开 TextDetail；Omni 保留现有选择语义。Enter 执行仍走原有动作链。
3. 鼠标滚轮一次 3 行，偏移边界钳制、不循环；thumb 设置绝对偏移；轨道前后空白点击翻一页。箭头仅保留 Explorer 的符号样式，不扩展新点击功能。
4. 鼠标滚动使选中行离开可见范围时，将其钳制到最近可见行；键盘上下键保持既有循环选择，必要时移动视口。不能在 render 中无条件把鼠标滚动拉回原 selected。
5. Help/Omni 切换保留各自搜索、选择及 scroll；关闭恢复来源弹层。标题点击使用 ToggleHelpPanel，保持 PersistHelpPanelView 行为。
6. 仅在最上层面板列表 Rect 内处理滚轮；面板外滚轮/横向滚轮不穿透。保留 toast 优先级和 Help Search 文本选择。
7. Help 一条结果占一行，按终端 cell width 截断长描述，行命中与视觉位置一致。搜索框、状态行、footer 不进入滚动区域。

## 2. 数据与接口设计

以下命名是拟新增接口，实施时可按相邻风格微调，但不能省略行为。

### 2.1 状态归属

- `HelpState` 增加 `scroll: usize` 与 `viewport_rows: usize`，构造器初始化为 0。`OmniState` 保留已有 scroll，增加 viewport_rows。
- Help 增加统一 filtered entries 方法，所有 count/select/selected_id/render 都使用配置 bindings 的同一过滤结果。
- 双方实现 `set_viewport_rows`、`set_scroll_offset`、`scroll_rows`、选择后 ensure-visible 方法。不要用反复 Move 来模拟设置绝对 offset。
- 选择操作结束 query edit group；查询值变更、结果变化、步骤切换后归一化偏移。空列表时 scroll=0，Help selected=0，Omni 沿用其 None 语义。
- Omni 用 stable ID 选择：新增 `OmniSelectItem(OmniItemId)` 或将仅内部使用的现有选择动作迁移为 ID；Help 新增 `HelpSelect(HelpShortcutId)`。保持原有下标 Action 的兼容与否由实际调用点决定，不能留下两条不同选择语义。
- Reducer 从当前 filtered entries 重新查 ID，失效 ID 忽略，不清空已有有效选择、不选中另一个同下标项。

### 2.2 偏移规则（可直接落实到模型方法）

设 `n` 为过滤后条目数，`h` 为可见行数：

```text
max_scroll = n.saturating_sub(h)
鼠标新 offset = old_offset.saturating_add_signed(delta).min(max_scroll)
绝对新 offset = requested.min(max_scroll)
当 n == 0 或 h == 0：scroll = 0；无可见行选择/鼠标滚动
当 selected 有效且 h > 0：
    visible_last = min(scroll + h - 1, n - 1)
    鼠标滚动后 selected = clamp(selected, scroll, visible_last)
键盘选择后：
    selected < scroll 时 scroll = selected
    selected >= scroll + h 时 scroll = selected - (h - 1)
    最后 clamp scroll 到 max_scroll
```

Omni 原先在刷新时保留 None 的规则不变：无 selection 时不要因为视口归一化擅自执行/恢复被删除条目。渲染以同一有效 offset 生成行、命中和滑块；首次渲染或 resize 在 runtime 同步前可纯计算有效 window，但下一次模型同步必须采用相同规则。

### 2.3 UI 快照和生命周期

- UiState 增加当前面板列表 viewport metadata：面板类型、列表 Rect、有效 start、总数、可见行数；Omni 携带 session_id/query_generation。
- 新增面板 viewport Action，runtime 在同现有 `sync_explorer_viewport` 的调用位置附近同步最新可见面板。只在实际尺寸变化时发动作；reducer 验证目标面板仍然存在。每帧重置 metadata，防止不可见面板残留。
- 新增 `PanelScrollbarDrag` 和专属 `GestureOwner::PanelScrollbar`。保存面板身份、rail/start/length/thumb_length/max_offset、pointer_offset，以及开始拖动时的布局/内容身份。
- 内容身份必须在过滤结果有序 ID 集合变化时失效，不能只依赖总数或 query_generation（异步结果可能在同查询同数量下重排）。推荐记录当前有序稳定 ID 快照进行比较；这些列表规模不要求为此建立新的全局 revision 系统。
- drag 有效性比较不能包含 offset/selected，因为拖动本身会改变它们。布局 Rect/条目集合/面板身份变化时取消；Up、关闭、切换、新手势也清理。
- 帮助面板同次会话关闭/重开时必须清理手势。沿 render 可见面板切换检查和标题/Esc 动作后的重绘清理实现，不把 HelpSearch 的原有文本拖选一并误清除。

## 单元 1：双面板行选择和标题点击切换

**文件**
- 修改：`src/action.rs`、`src/app.rs`、`src/help.rs`、`src/model/omni.rs`
- 修改：`src/ui/mod.rs`（render_help、HitTarget）、`src/ui/omni.rs`、`src/input/mouse.rs`
- 测试：`tests/mouse.rs`、`tests/ui_render.rs`、`tests/help_omni_panel.rs`

**步骤**
1. 在真实 TestBackend 渲染基础上写回归测试：点击 Help 第二条可见行后选中它，overlay 仍为 Help、不产生 OpenTextDetail/执行 Command；Omni 点击行仍只选择。使用 ID 而非测试中的固定文案索引断言。
2. 添加双向标题点击测试：读取实际标题 HitRegion，点击得到 ToggleHelpPanel；更新 app 后检查面板变化、PersistHelpPanelView Command、往返保留 query/selection、关闭恢复来源 overlay。
3. 定向跑新增测试，记录旧实现缺少标题 hit / Help 返回 OpenTextDetail 的预期失败；若新增枚举尚未定义可先用行为测试表达，避免把无关编译错误当红测证据。
4. 增加稳定 ID 选择动作及 reducer。HelpMove 的过滤计数与渲染统一使用 bindings-aware entries；检查 App 的无 active console 准入和 Action 分类方法，确保相关动作在空工作区和 relation 页生效。
5. Help 行改为单行渲染，去掉 Paragraph wrap，使用项目已有终端安全和 cell-width 截断函数；根据需要把 render_overlay 的 icons 传到 render_help。替换旧 OpenTextDetail hit 为选择 hit。Omni 行 hit 改为稳定 ID。
6. 提取标题提示的共同文本/Rect 计算，绘制与命中共享 cell width、右对齐位置及裁剪规则；只注册实际可见提示区域。
7. 在鼠标左键通用 overlay 拦截/底层文本命中之前识别当前可交互面板。仅放行当前面板的行/标题，Help Search 保留现有路径；不能为全部 overlay 放宽访问。
8. 更新 `rendered_help_rows_open_readonly_detail_without_consuming_form_input` 和 `readonly_regions_register_help_dashboard_and_relation_detail_targets` 中 Help 部分为新契约；保留 dashboard/relation 通用详情测试及密码框保护。
9. 跑下列定向测试并记录真实结果；处理本单元失败后继续单元 2。

**命令**
```sh
cargo +1.94.0 test --test mouse help
cargo +1.94.0 test --test help_omni_panel
cargo +1.94.0 test --test ui_render readonly_regions
cargo +1.94.0 test --test omni_input
```

**验收**：两种列表单击只选择；右上角两个方向点击均与 Tab 等价；Help 长描述不造成下一条 hit 错位；背景点击和 Search 行为不回归。

## 单元 2：持久视口、滚轮和 Explorer 风格滚动条

**文件**
- 修改：`src/help.rs`、`src/model/omni.rs`、`src/action.rs`、`src/app.rs`
- 修改：`src/runtime.rs`、`src/ui/mod.rs`、`src/ui/omni.rs`、`src/ui/scrollbar.rs`、`src/input/mouse.rs`
- 测试：对应模型文件的单测、`tests/mouse.rs`、`tests/ui_render.rs`、`tests/keymap.rs`

**步骤**
1. 写模型边界测试：长列表、首次/末页、鼠标上下滚动钳制、键盘循环后 ensure-visible、空列表、过滤后总数减少、viewport resize。添加 Help 自定义 bindings 搜索计数与 selected_id 一致的回归测试。
2. 写真实渲染滚轮回归测试，比较前后列表首 ID、offset/selected、滑块位置；特设 Omni 盖在 TextDetail/Message 上，以及后台 focus 为 Explorer/Editor 的用例，确认背景不变。
3. 跑模型与新增集成测试，记录旧实现无滚动路径的失败，然后按第 2 节实现模型方法和 Actions。同步修改 Omni query_changed/set_items/push_step/pop_step 的归一化调用，保证异步刷新不越界。
4. UiState 每帧保存活动面板 viewport，runtime 加同步函数并接到实际 draw 后同步调用链；新 Action 经过 no-console 准入。测试 fixture 模拟同样的 viewport 同步后重新绘制，另保留首次帧安全测试，不能只手工设置 h 掩盖集成缺口。
5. `src/ui/scrollbar.rs` 增加只负责纵向字符绘制的小 helper，输入 geometry/theme/background，复用 geometry/offset_at。将 Explorer renderer 的相同字符绘制换为 helper，保留其原有 HitTarget 和滚动行为。
6. Help/Omni 只在列表右列绘制滚动条，内容宽度和 row hit 同时减去该列；两个 renderer 都使用同一有效 window，更新 Help 单行截断宽度。搜索、status/footer 不占 rail。
7. 在 `map_mouse` 最上层面板路径中处理 ScrollUp/Down：位置属于当前列表 Rect 时发 ±3 行动作，其他位置消费为 None；ScrollLeft/Right 也不能穿透。该路径必须早于 TextDetail/Notification 等底层 overlay 的滚轮分支。
8. 修正渲染与模型 ensure-visible 的关系，避免每次鼠标改变 scroll 后 `visible_window` 把 offset 拉回；切换缓存的模型保持 scroll。
9. 跑定向测试和已有 Explorer wheel 测试，记录实际结果后继续单元 3。

**命令**
```sh
cargo +1.94.0 test --lib help::tests
cargo +1.94.0 test --lib model::omni
cargo +1.94.0 test --lib ui::scrollbar
cargo +1.94.0 test --test mouse wheel
cargo +1.94.0 test --test ui_render help
cargo +1.94.0 test --test ui_render omni
cargo +1.94.0 test --test keymap filtered_help
```

新增测试命名使用 `wheel` / `help` / `omni` 等对应过滤词；确认输出确有执行目标测试，不能将零测试输出计为通过。

**验收**：两面板长列表滚轮立即滚动，滑块跟随；不循环、无回弹；条目不足时无滚动条；过滤/resize/状态行变化安全；被遮盖区域不受鼠标滚动影响。

## 单元 3：拖动与轨道翻页闭环

**文件**
- 修改：`src/ui/mod.rs`、`src/ui/omni.rs`、`src/ui/scrollbar.rs`（如需补几何返回）、`src/ui/text_selection.rs`
- 修改：`src/input/mouse.rs`、`src/action.rs`、`src/app.rs`、必要的 `src/model/help_panel.rs` 面板身份类型
- 测试：`tests/mouse.rs`、`tests/help_omni_panel.rs`、`tests/ui_render.rs`

**步骤**
1. 添加真实渲染 thumb Down -> Drag -> Up 测试，覆盖 Help/Omni 两种面板、抓住滑块中段、拖到端点和轨道之外、释放后拖动不继续。
2. 添加轨道 before/after 点击翻页测试及非零 offset 行选择测试。使用渲染返回的 rail/thumb 几何构造坐标，不硬编码终端绝对位置。
3. 添加失效手势测试：开始拖动后切换/关闭/resize/修改查询/异步刷新有序结果，旧 Drag 不影响新面板或后台。包含同 session、同 query、同条目数但结果重排场景。
4. 增加专属 page/thumb HitTarget，注册顺序为列表区域/行先、滚动条后，避免 row hit 抢占滚动条列。轨道页偏移采用有效 start ± viewport_rows。
5. thumb Down 保存 pointer_offset 和 drag 身份，使用专属 GestureOwner；Drag 通过既有 ScrollbarGeometry.offset_at 生成绝对 offset Action。Help 路径必须位于通用 overlay drag 取消逻辑之前，Omni 路径必须挡住后台 editor/input 手势。
6. Up 清理面板 drag/owner，不借用 GridEndColumnResize。所有取消路径检查并清除 panel drag；不要将有效 Help Search input gesture 误识别为 scrollbar。
7. 每次渲染/鼠标事件核验 drag 的面板身份、布局及有序内容快照；偏移变化本身不使 drag 失效。开启新面板时清除遗留后台拖动，避免关闭后旧 scrollbar drag 恢复。
8. 跑新增及已有 scrollbar/gesture 测试；检查短轨道（高度 3、usable rail=1）不可移动滑块的安全表现，仍可通过滚轮浏览。

**命令**
```sh
cargo +1.94.0 test --test mouse scrollbar
cargo +1.94.0 test --test mouse gesture
cargo +1.94.0 test --test help_omni_panel
cargo +1.94.0 test --test ui_render omni
cargo +1.94.0 test --test ui_render help
```

**验收**：拖动连续、两端可达、抓取点不跳变、释放即停止；轨道翻页准确；面板切换及内容/布局变化不复用陈旧拖动；Explorer 既有拖动/滚轮无回归。

## 单元 4：收尾审查与完整验证（Luna）

1. 以实际 diff 审查模型/UI/routing/runtime 四层闭环，确认没有只有 Action 却未通过准入、只有 metadata 却未接同步调用的问题。
2. 补齐必要边界证据：空工作区/无 active console、Help Search 拖选、toast 优先级、窄终端标题裁剪、中文/宽字符、空结果、最后一页、关闭恢复来源弹层。
3. 仅对新改动涉及的测试定向复跑；功能齐备后执行一次 CI 对齐的 Rust 检查：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

预期退出码均为 0。实际命令、结果、代码版本/工作区状态与环境写入原任务目录 validation.md。fmt 不通过则在实现工作区运行 formatter 并重新核对 diff；编译/测试错误自行修复，不视作外部阻塞。

4. TestBackend 是核心功能回归证据。真实 PTY 鼠标拖动为补充检查，不是用户强制要求；如可用，在长 Help/Omni 列表拖动、滚轮、行选择、标题往返各走一次并记录终端尺寸。
5. 环境受限时区分项目 CI 要求和补充检查，最多一次有针对性的修复重试。随后由 Luna 收尾审查决定可用证据或记录限制；不能把未执行的全量/数据库 CI/PTY 称为通过，也不能无限 progress 重试。
6. 检查 `git diff --check`、`git status --short`、最终 diff 范围。阶段授权允许提交时只 stage 本功能明确路径；不 `git add .`，不包含用户其他改动，提交/合并按后续工作流权限执行。

## 提交建议与完成标准

- 推荐逻辑提交：`feat(help): support mouse selection and panel switching`、`feat(help): add scrollable omni and help lists`、`feat(help): support panel scrollbar dragging`。不是本计划阶段执行 git commit 的授权；若工作流要求统一提交，可保留同样的验收检查点后在收尾合并提交。
- 不为“技能默认频繁提交”改变用户 index/worktree 限制。原工作区的未知变化始终视作用户工作。
- 完成意味着以上四个单元已获得真实证据并由 Luna 审查；不能以只绘制滚动条或完成一轮复核作为结束。
- 首个实施动作：在 Luna 的任务工作区用当前 diff 核对基线，然后编写单元 1 的真实渲染 Help 行选择与标题点击回归测试；无需等待用户选择方案。

## Plan 阶段交付

本文件是完整实施计划；本阶段没有执行上述业务修改、测试或提交。新增手势枚举及 viewport runtime 接入位置已用当前代码核对。本轮已重新读取 analysis.md 并调用 writing-plans 技能；完成文件范围及计划一致性检查后，最后写入指定 plan 回执。自动工作流随后交由 Luna 实施，不询问执行方式。
