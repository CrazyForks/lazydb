# Redis Keys Pane Resize Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **本自动工作流约束：**以上为 writing-plans 模板中的实施提示；实际按插件分配的当前阶段执行。用户已选择自动继续，不询问执行方式，不启动子 Agent；若该技能不可用，直接依照本计划和阶段指令逐项实施。

**Goal:** Redis Keys pane 支持 `Ctrl-w >` / `Ctrl-w <` 和鼠标拖拽右边框调整宽度，方向、反馈和尺寸限制与 Explorer 的交互方式一致。

**Architecture:** 新增 `PaneSplit::RedisKeysWidth` 和独立的会话级宽度偏好，复用现有 ResizePane / SetPaneSize / PaneLayoutChanged 闭环。用一份 Redis 内部布局几何驱动渲染、实际尺寸指标、边框命中、拖拽有效性及高亮；通过焦点感知的共享解析函数把 Keys 的宽度命令路由到内部 split。

**Tech Stack:** Rust 2024 / Rust 1.94.0，ratatui 0.30.2，crossterm 0.29，现有 App action/update 状态模型、Keymap pending 序列及 TestBackend 测试。

---

## 基线、范围与实施约定

- 分析依据：同目录 `analysis.md`，已在 plan 阶段完整读取。
- 原工作空间：`/Users/yelog/workspace/tui/lazydb`；目标 `main`；分析起点 `56e1ded88a083b4193f1c330d4609c8a7154e65f`。
- 任务分支当前为空；实施阶段使用插件指定工作空间/分支，不在 plan 阶段创建分支、worktree 或提交。
- 计划直接存放到用户指定位置，不另写 `docs/plans` 副本。保留分析中列出的三个已有未跟踪文档。
- 下文行号为起点附近的导航线索，实施时以符号及最新源文件为准。修改前用 codegraph 查看相关符号；索引缺失或返回不完整的片段再针对性读取。
- 每个编号步骤是一次可独立执行/复核的小动作，通常 2–5 分钟；构建或全套测试时间不计入该估计。按任务顺序执行，完成一个闭环后再进入下一个。
- 回归测试针对真实行为：按键是否到达、是否改对 pane、拖动是否跨帧存活以及边界是否正确；避免纯字段赋值测试。
- 本计划的验证命令均为后续实施阶段要运行的命令，plan 阶段未执行业务测试，不宣称通过。

## 已确定的行为契约

1. `Focus::Results + RedisBrowserFocus::Keys` 下，`>` 对 Keys 宽度加 count，`<` 减 count；默认 count=1。保留 `10 Ctrl-w >` 这种已有计数入口，不新增 `Ctrl-w 10 >` 语法。
2. `>`/`<` 本身不触发 resize。Keys 搜索 Editing 阶段拥有文本输入和数字；此时 Ctrl-w 不开启窗口 resize 序列，保持原有不调整宽度的行为。Confirmed 搜索视图可调整。
3. Explorer 仍使用 ExplorerWidth；Preview 文本编辑器/表格的既有按键语义保持现状。Keys 的 `+`/`-` 沿用原解析，不新增内部高度 split。
4. `redis_keys_width: Option<u16>` 为 App 会话内共享偏好，多个 Redis tab 共用；不扩展持久化 schema。ResetPaneSizes 将其恢复 None。
5. 内部区域足够时，Keys 最小宽度 16，Preview 最小宽度 24，均含边框。偏好 None 时先按原 Percentage(35)/Percentage(65) 求默认几何，然后只在违反最小宽度时 clamp。这样在常规尺寸保留原舍入效果，并解决宽度 40–45 时“默认 35% 少于 16”的冲突。
6. 内部区域宽度 <40 或高度 <3 时，渲染使用原百分比降级，resize metric/命中区均为 None，保留偏好供区域恢复后使用。面积为零时安全返回，不进行 right()-1 等非饱和运算。
7. Standard/Wide、Focus 模式和最大化 Results，只要内部两 pane 实际可见且满足尺寸要求，都可 resize。TooSmall 或仅 Explorer 可见时禁用。
8. 终端正常重绘不取消 drag；终端矩形发生变化时取消 Redis drag，防止旧指针起点继续应用到新坐标系。切 tab、关 tab、overlay/Omni 出现、Keys 区域隐藏/不可调整时也取消。
9. 鼠标按下仅占有 resize 手势，不改树选择/焦点，不打开 key；左右移动实时更新；释放提交最后指针位置并清空手势。新尺寸以按下时实际渲染宽度为基准。

## Task 1：用真实按键入口锁定回归

**Files**
- Create: `tests/redis_pane_resize.rs`
- Reference: `tests/redis_help.rs:19-35`（无服务端 Redis App fixture）
- Reference: `tests/keymap.rs:2578-2605`（计数与窗口命令）
- Reference: `src/runtime.rs::sync_pane_layout`、`tests/ui_render.rs` 中 TestBackend 渲染方式

**步骤**
1. 在新测试文件建立最小 fixture：App::new、添加 RedisBrowserTab、选为 active_tab、Focus::Results、tab.focus=Keys。不连接真实 Redis；使用稳定 UUID。
2. 加入渲染 helper：通过公开 UI 渲染入口和 UiState 渲染一帧，调用 `app.update(Action::PaneLayoutChanged(ui.pane_layout))` 模拟 runtime 同步；初始终端设为 180×50。
3. 编写 `redis_keys_width_shortcuts_reach_app`：使用 Keymap 发送 Ctrl-w 再 `>`，断言第二键产生 Some(action)，更新 App 后重新渲染，并检查 Keys/Value 分界向右移动一列、Explorer 边框位置不变。该测试首次只使用现有符号；避免因尚未新增枚举而只有编译失败。
4. 运行 `cargo +1.94.0 test --test redis_pane_resize redis_keys_width_shortcuts_reach_app -- --nocapture`。预期当前失败在 Ctrl-w 序列没有产生动作，记录红灯原因。
5. 新增鼠标基线用例 `redis_keys_border_exposes_resize_target`：从 Keys 实际边框位置检查对应 hit region 是 pane resize，而非树行点击。当前应失败；后续 Task 3/5 补齐具体 split 和完整拖动断言。

**复核/验收**：测试失败证明输入或命中功能缺失，而非 Redis 连接、fixture tab 索引或终端面积错误；fixture 可复用，树初始为空也能验证布局。

## Task 2：扩展尺寸模型与 App 更新闭环

**Files**
- Modify: `src/model/workspace.rs::PaneSplit, PaneSizePreferences, PaneLayoutMetrics`
- Modify: `src/app.rs` 的 ResizePane / SetPaneSize / ResetPaneSizes / PaneLayoutChanged 分支（约 5814–5854）
- Modify: `src/ui/layout.rs::AppLayout::calculate, pane_resize_region`
- Modify: `src/input/mouse.rs::pane_resize_action, pane_resize_pointer` 及 Down 尺寸读取分支
- Modify: 现有上述结构的显式构造点，包括 `tests/keymap.rs`、`tests/mouse.rs`、`tests/ui_render.rs` 和 lib 测试（按编译结果定位）

**步骤**
1. 添加 `PaneSplit::RedisKeysWidth`，两个尺寸结构各增加 `pub redis_keys_width: Option<u16>`；保持 Default 为 None。
2. 补齐所有结构字面量；已有测试优先使用 `..Default::default()` 表达无关尺寸。外层 AppLayout 指标中该值始终为 None，`AppLayout::pane_resize_region(RedisKeysWidth)` 返回 None，由 Task 3 统一查询内部几何。
3. 在 App 增量分支仅当 `pane_layout.redis_keys_width` 为 Some 时，从实际宽度 saturating_add_signed 保存偏好；采用现有 delta 到 i16 的钳制方式，避免新旧 pane 运算不同。
4. 在 App 绝对尺寸分支同样以 metric 可用为条件保存；ResetPaneSizes 继续通过 Default 清空全部偏好。
5. 为鼠标现有匹配增加 RedisKeysWidth：读取 redis_keys_width、横向 pointer 用 event.column。此时尚未注册新命中区，不引入第二套动作。
6. 在 `src/app.rs` 现有 pane 测试旁补一个 `redis_keys_pane_size_uses_visible_metric` 行为测试：实际宽度 28、历史偏好 100 时按增量 +1 应保存 29；指标 None 时 resize/set 都不改变偏好；Reset 恢复 None。
7. 运行 `cargo +1.94.0 check --all-targets --all-features`，预期无非穷尽匹配/缺失字段；运行 `cargo +1.94.0 test --lib redis_keys_pane_size_uses_visible_metric`，预期通过。

**复核/验收**：新增独立维度，不借用 ExplorerWidth；ResizePane 使用真实 metric 而非越界偏好；非 Redis layout 不泄露旧指标。Task 1 的端到端测试此时仍可能红灯，属于后续路由/布局尚未接通。

## Task 3：建立唯一的 Redis 内部几何并接入渲染

**Files**
- Modify: `src/ui/layout.rs`（新增 RedisBrowserLayout 及同文件单元测试）
- Modify: `src/ui/redis_browser.rs::render`（约 22–42、290–328）
- Modify: `src/ui/mod.rs` 主渲染函数（约 954–983、1022–1032、1124–1150）
- Test: `tests/redis_pane_resize.rs`

**步骤**
1. 编写 `redis_browser_layout_*` 测试覆盖非零原点 Rect、默认比例、显式宽度、16/24 最小限制、宽度39/40/45、高度0/2/3和 u16::MAX 偏好。几何断言：keys.right()==preview.x、两块宽度之和等于 area.width、尺寸/边框不越界。
2. 运行 `cargo +1.94.0 test --lib redis_browser_layout`；预期新增 helper 尚未实现时编译失败，记录为预期红灯。
3. 实现轻量 `RedisBrowserLayout`，包含 `keys: Rect`、`preview: Rect`、可用 `keys_width: Option<u16>` 和右边框查询。先求原 35/65 布局；可调整时对 `preference.unwrap_or(default_keys.width)` 在 `[16, area.width-24]` clamp，再用 Length/剩余区域构造布局；不可调整时保留默认几何并返回 None metric。
4. 主 UI 入口在计算外部 AppLayout 后、拖动校验前计算 optional RedisBrowserLayout；以当前 tab 类型和 `layout.relation` 是否存在为条件。统一 split→Rect 查询：外层两种委托 AppLayout，Redis 委托内部布局。
5. `state.pane_layout` 先整体赋外部默认指标，再合并内部 Keys 指标。TooSmall/隐藏页面确保 None；不要依赖 renderer 执行到函数尾才填指标。
6. `redis_browser::render` 接收已计算的内部布局，直接使用 keys/preview Rect；移除自己的固定 Percentage 切分。不新增第二次独立计算，保证 Table 提前 return 不影响命中注册及指标。
7. 把主入口命中区注册、drag 几何有效性检查及高亮都切到同一个 split→Rect 查询；注册数组增加 RedisKeysWidth。Overlay/Omni 存在时不注册 resize target。
8. 运行 `cargo +1.94.0 test --lib redis_browser_layout`，预期所有边界用例通过；运行 `cargo +1.94.0 test --test redis_pane_resize redis_keys_border_exposes_resize_target`，预期能命中正确 split。
9. 扩展集成测试 `redis_keys_layout_tracks_visibility_and_preferences`：普通/Focus/最大化 Results、仅 Explorer、Redis→SQL、终端缩小后扩大；检查 metric 和实际边框一致、偏好保留、SQL 不使用 Redis 偏好。

**复核/验收**：Keys右边框命中范围是 `Rect(right-1, y+1, 1, height-2)`，不覆盖角、树内容、搜索输入或内部滚动条；默认正常宽度画面保持旧比例。40列内部区域允许16/24，不能因保留35%而破坏最小值。

## Task 4：修复 Keys Ctrl-w 路由与帮助命令一致性

**Files**
- Modify: `src/app.rs`（新增共享 resize 解析、约2724–2731帮助执行）
- Modify: `src/input/keymap.rs::Keymap::map, map_pending`（约1259、1345–1507、2768）
- Modify: `src/help.rs`（宽度快捷键上下文及 capability，按既有声明方式）
- Test: `tests/redis_pane_resize.rs`、`tests/redis_help.rs`

**步骤**
1. 在 Task 1 测试补齐 `<`、带 SHIFT 的 >/<、`10 Ctrl-w >`、超大计数无溢出、裸 >/< 不产生 resize；增加 Explorer 聚焦时目标仍为 ExplorerWidth 的断言。
2. 增加 App 共享 `focused_pane_resize(operator, count) -> Option<PaneResize>`（名称可按现有风格微调）：仅当 Results+RedisBrowser+Keys 且 operator 为 >/< 时返回 RedisKeysWidth，count 通过 i32::try_from 并拒绝0；其它情况委托现有 `pane_resize(self.focus, operator, count)`。
3. `map_pending` 与 App 帮助执行中的宽度/高度 resize 入口统一调用该 helper；保留 EditorEffect 既有含义，不在 App 全局重写所有 ExplorerWidth action。
4. Keys handler 内，在处理 Editing 搜索分支之后、普通树分支终结 return 之前，识别 Ctrl-w 并 set_pending(Window{count:1})。保持非表格 Preview 早期 ReadOnlyEditorKey 路径。
5. 对通用 `Pending::WindowCount` 起始数字分支加“Redis Keys 正在 Editing 搜索时不启用”的局部条件；使 `123><` 等字符正常插入搜索字符串。这是确保新 resize 不干扰搜索的必要隔离，不改写整个输入路由。
6. 检查 RedisKeys 的帮助/序列提示上下文，将现有宽度增加/减少命令加入可用集合；避免在不支持的上下文新增高度命令。帮助命令通过现有 ExecuteHelpShortcut 流程关闭 overlay 后执行共享解析。
7. 测试帮助执行 >/< 与键盘得到同样的 Keys metric 变化；测试搜索 Editing 的 Ctrl-w 不建窗口 pending，Confirmed 搜索可以调整；按键序列超时/切 tab 不在新 tab 生效。
8. 运行 `cargo +1.94.0 test --test redis_pane_resize redis_keys_width`，预期 Task 1 红灯转绿；运行 `cargo +1.94.0 test --test redis_help`，预期原有帮助/搜索隔离测试和新宽度帮助测试通过。

**复核/验收**：快捷键增大的是 Keys 而非外部 Explorer；数字计数与原语法一致；搜索文本拥有自己的数字；Preview 正常/Visual、表格输入不被新的 Ctrl-w 特判截获。

## Task 5：完善拖拽所有权、跨帧反馈和取消

**Files**
- Modify: `src/ui/mod.rs::PaneResizeDrag`、主渲染拖动校验（约970–982）
- Modify: `src/input/mouse.rs::map_mouse, pane_resize_action`（约189、327、716、1144）
- Test: `tests/redis_pane_resize.rs`、`tests/mouse.rs`、`tests/ui_render.rs`

**步骤**
1. 新增 `redis_keys_drag_survives_redraw_and_finishes_on_release`：渲染取真实 border→Down→Drag(+8)→应用 action→重绘/同步→Drag(-3)→Up(最终+4)。断言各帧真实宽度、最终值、外部 Explorer不变和GestureOwner清理。
2. 运行 `cargo +1.94.0 test --test redis_pane_resize redis_keys_drag`，识别是否因现有校验/缺少所有权而失败，不跳过重绘步骤。
3. 在 PaneResizeDrag 增加 Redis 所有权信息：可选 owner tab UUID及起始终端 Rect；外层 split 用 None。终端 Rect可存放在UiState并每帧更新，复用已有等价字段（若有），避免保存易随自身拖动变化的Keys Rect作为取消依据。
4. Down 开始 Redis drag 时记录活动 Redis tab和当前终端 Rect；尺寸仍从UiState实际metric读取。旧外层构造补None，不改变Explorer手势语义。
5. 在Drag、Up和每帧绘制校验Redis所有权、当前tab类型/id、终端Rect、可用内部几何以及overlay/Omni。失效时清空pane_resize_drag和对应GestureOwner，返回None，绝不在另一tab应用旧位置。用共享校验helper避免三个分支各写一套规则。
6. 沿用现有 `start_size + pointer - start_pointer` 的 i32 计算并 clamp到u16。Up必须在已确认有效的情况下应用最终位置；只有size确实改变才发SetPaneSize。
7. 增加参数化取消测试：切SQL、切另一Redis、关闭当前tab、Overlay、Omni、正常尺寸改变、变窄/变矮、切仅Explorer；分别在重绘前发送Drag/Up和重绘后发送，确保事件顺序不会绕过owner校验。
8. 新增高亮测试：活动Redis drag只强调当前Keys边框，松开恢复；保留 `tests/ui_render.rs::pane_resize_drag_highlights_the_rendered_explorer_border`。运行 `cargo +1.94.0 test --test redis_pane_resize redis_keys_drag` 和 `cargo +1.94.0 test --test ui_render pane_resize_drag`，预期通过。
9. 在有树节点和opened_key的fixture中验证拖动不改变selected/opened_key/preview_generation；分别用文本与Table格式渲染，确保右边框不落入文本选择或列宽拖动目标。

**复核/验收**：拖拽跨多次普通重绘继续生效，终端尺寸变化才按契约取消；边界clamp后再新建drag从实际宽度起算；失效手势不留下所有权、不修改新tab，也不吞掉后续正常鼠标操作。

## Task 6：集成复核、回归检查与实施交付

**Files**
- Review: Task 2–5列出的业务文件及测试文件
- Reference: `.github/workflows/ci.yml:62-88`

**步骤**
1. 用新集成测试覆盖“键盘调整→鼠标调整→切tab→Reset→重绘”完整状态链，验证会话级偏好和打开key状态。测试统一使用公开action与UI几何，不添加仅供测试用的生产接口。
2. 检查所有 `PaneSplit`、尺寸结构和PaneResizeDrag匹配/构造点，重点复核EditorHeight、外部Explorer、SQL/Relation页面以及help执行路径；确认没有业务文件外的意外修改。
3. 运行快速回归：`cargo +1.94.0 test --test redis_pane_resize --test keymap --test mouse --test ui_render --test redis_help`。预期所有相关用例通过，新增target确实运行测试，不能把0 tests视为验证成功。
4. 运行布局/App定向测试：`cargo +1.94.0 test --lib redis_browser_layout` 和 `cargo +1.94.0 test --lib redis_keys_pane_size_uses_visible_metric`。预期命中的新增用例全部通过。
5. 运行CI同款格式与静态检查：`cargo +1.94.0 fmt --all -- --check`；`cargo +1.94.0 clippy --all-targets --all-features -- -D warnings`。预期退出码0。如格式需修正，先运行fmt，复核diff后重跑格式检查。
6. 运行CI Rust回归：`cargo +1.94.0 test --all-targets --all-features`。预期退出码0；记录环境门控/ignored数据库测试，不能把跳过真实数据库测试描述成已验证服务端行为。此功能的自动验收不依赖真实Redis服务。
7. 如当前实施环境具备可交互TUI及现成Redis profile，手动核对180×50、100×30、90×30窗口下Keys快捷键与拖动，以及Explorer对照效果。没有该环境时以TestBackend跨层测试为主要证据，明确记录手动项未执行。
8. 执行 `git diff --check`、`git diff --stat`、`git status --short`；预期无空白错误，仅包含实施文件和既存用户文档。不要staging三个无关未跟踪文档。
9. 按实施阶段指令整理变更及验证记录。若该阶段授权提交，可在Task 2–3、Task 4、Task 5–6各自达到可编译且相关测试通过的检查点创建逻辑提交，建议消息分别为 `feat(redis): add resizable keys pane layout`、`fix(redis): route keys window resize shortcuts`、`feat(redis): support keys pane border dragging`；显式staging该检查点文件，避免 `git add .`。未经实施阶段要求不提前执行提交。

**验证失败处理**：代码导致的失败先修复，再重跑受影响检查；缺Rust1.94/toolchain组件、构建依赖或执行环境属于真实阻塞，应记录准确命令/错误，不把未经运行的检查标成通过。检查全部通过后不反复扩大同类测试；仅新修改或未解决问题才要求额外复跑。

## 最终验收清单

- [ ] Keys聚焦时Ctrl-w >/<按正确方向改变内部宽度，支持现有计数及SHIFT形式。
- [ ] 外部Explorer边界不随Keys宽度动作改变；Explorer原快捷键/拖动通过回归。
- [ ] 鼠标Down/Drag/多帧重绘/Up完整闭环正确，活动边框高亮与Explorer一致。
- [ ] 点击树、搜索编辑、滚动条、Preview文本选择/表格交互和打开key状态未被resize动作污染。
- [ ] 偏好与实际metric分离；默认比例/16和24最小宽度/窄窗口降级/扩大恢复/Reset行为满足契约。
- [ ] Focus和最大化Results内部可见时可调整；隐藏或TooSmall时无有效metric/命中区。
- [ ] tab、overlay、Omni、终端几何变化导致Redis drag安全取消，无旧动作跨tab或残留手势。
- [ ] 键盘、帮助执行和鼠标最终使用同一尺寸更新闭环，正常无服务端副作用。
- [ ] 定向测试、格式、Clippy及CI Rust测试结果已记录；业务改动和已有用户文件分清。

## Plan 阶段交付记录

本阶段已读取analysis、调用writing-plans技能、核对现有计数语法与CI命令，并将分析建议中的最小宽度/默认比例冲突和终端变化取消语义具体化。此文件为完整实施计划；未实施业务代码、未运行业务测试、未启动子Agent。下一阶段由插件自动调度。
