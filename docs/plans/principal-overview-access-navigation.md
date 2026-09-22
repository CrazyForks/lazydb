# Principal Overview ACCESS Implementation Plan

> 执行者：Luna。按下面的端到端验收单元连续推进，实现、审查、纠偏、提交合并均由 Luna 完成。无需等待用户逐项 resume；不启动子 Agent。技能模板中的 Claude 专用交接方式不适用于本任务。

**Goal:** 让角色/用户 Overview 的 ACCESS 可通过键盘和鼠标进入、浏览，并以分类表格和完整详情清楚展示权限与成员关系。

**Architecture:** 沿用 `Focus::Results` 作为 Overview 的 ACCESS 焦点，上方 PRINCIPAL 保留静态身份摘要。引入轻量分类与列表状态，共享布局几何驱动绘制、命中及翻页；复用现有 TextDetail 查看完整条目，保持已有权限修改确认流程。

**Tech Stack:** Rust 2024 / 项目 CI Rust 1.94.0、ratatui 0.30.2、crossterm 0.29、既有 App/Action reducer、TestBackend 集成测试。

---

## 0. 基线、约束和交付位置

- 实施依据：同目录 `analysis.md`，其中列明根因、产品方案比较和设计决策。本计划已将其收敛为可执行步骤，无需实施时重复全面探索。
- 基线：`23c63a1996ff24c69dcbd58a38c93629d4c55ccd`；目标 `main`。
- plan 阶段重新执行 `git status --short && git rev-parse HEAD`：退出 0，工作区干净，HEAD 未变化。不依赖任何未提交代码。未创建 worktree 或分支。
- 当前任务 `checkpoint.json` 仍不存在；不得自行创建或修改插件维护的 checkpoint/state。
- 报告与计划只保存在 `/Users/yelog/workspace/tui/lazydb/.git/opencode-tasks/ses_f37edc781ffe99lxxNndQelLXm`。工作流/任务分支由 Luna 后续命名，计划阶段不代为创建。
- 实施前重新检查 diff；允许原工作区存在新增未提交修改，不 stash、不要求整库清理、不将用户修改自动带入 worktree。若发现本计划涉及文件已改变，依据实际 diff 调整并记录。
- 不更改数据库适配器协议、全局 Focus 枚举、全局 hit-test 遍历顺序，不重建通用数据 grid。需要扩大范围时记录实际原因。
- 当前无可读取截图附件，视觉规格以源码与 analysis.md 为准，不阻塞执行。

## 1. 固定实施决策

### 1.1 焦点与几何

1. Overview 只有一个主内容 pane：ACCESS。`Focus::Results` 且无覆盖弹层时 ACCESS 活动；PRINCIPAL 不再以 Results 状态高亮。
2. 通用 Focus hit region 先注册，分类/行/Overview-DDL 控件后注册。保留 `UiState::target_at` 的逆序优先语义。
3. 绘制、命中、视口计算共同使用 `principal_overview_layout` 一类纯布局函数，返回摘要、ACCESS 外框、分类、表头、body、状态和提示矩形。不在 mouse 模块重写行偏移公式。
4. 行命中仅覆盖实际 body 内已绘制数据，不含边框、表头、尾部空白。紧凑两行模式的两行映射到同一条目。
5. 通用 pane 快捷键和方向 pane 导航不变。FocusNext/Previous 进入 Results 就是进入 ACCESS，不增加静态摘要停靠点。

### 1.2 状态与数据

- 新增 `PrincipalAccessSection::{Permissions, MemberOf, Members}`，默认 Permissions。
- 权限选择继续由现有 `selected_permission` 保存，避免现有 mutation 路径出现两个真值。新增权限 offset，成员两个分类各有 selected/offset；如果实现选择合并成统一状态，必须一次性迁移所有 selected_permission 读取与测试，不能留同步副本。
- 状态位于 `PrincipalDdlTab`，不做跨 tab 全局状态，不加入持久化格式。
- 当前分类长度由 details 快照对应 Vec 决定。移动均 bounded，长度 0 时 selected/offset 均归 0。选中行始终在可见窗口中，偏移稳定，不按每次选择从底部重建窗口。
- body 不足一条记录时 visible_rows=0，不伪造可点击行；翻页步长可退化为 1，但绘制命中仍为空。
- 成功刷新后对三类状态 clamp；尺寸变更按当前 body 可见条目数约束 offset。保留有效索引即可，不在此任务引入复杂稳定 ID 或列表排序。
- 显示顺序保留原 Vec 顺序，显示索引直接对应原始数据索引；不得把来源颜色排序顺手带入 mutation 目标映射。

### 1.3 展示与操作

- 分类常驻：`Permissions (N)` / `Member of (N)` / `Members (N)`，含零条。
- 权限宽表：Target / Privilege / Origin / Source / Option；成员分类：Role 或 Member / Admin option。
- ACCESS body 内宽 >=88：五列；56–87：Target / Privilege / Origin；<56：每条两行，目标一行、权限和来源一行。Option/Source 在详情完整可见。列宽可按项目渲染效果小幅校准，但不能消除中窄屏策略。
- 文本按终端显示宽度裁切并使用省略标记；处理 CJK/emoji、控制字符与换行，使用现有 sanitize/Unicode 宽度工具。
- Origin 使用明确的 Direct/Public/Owner/Default/Inherited 文案。Coverage 使用 Complete/Partial/Unavailable/Unsupported 标签及原因，不用 Debug 输出。
- 使用现有 theme token，表头与选择背景形成层级；grant/admin option 使用显式文字加辅助颜色，不仅用红绿表达。
- 覆盖范围、加载/失败/旧快照提示、上下文和操作提示留在固定区域；小高度压缩辅助文字，完整说明通过详情可达。
- 上下和现有 results-move-up/down 配置选择当前分类；左右和 results-move-left/right 配置循环切换分类。先走既有 pane/window/pending sequence 与 overlay 优先级，不抢占 Ctrl-w 等快捷键。
- PageUp/PageDown 按可见条目数，Home/End 到首末；滚轮在 ACCESS body 内按三条移动当前分类选择，借稳定 offset 保证可见。
- Enter 打开完整条目详情；在空分类但存在 coverage 原因时可打开覆盖说明。复用 TextDetail 的滚动/复制/关闭能力。
- `o` 切换 DDL，`r` 刷新；g/v 仅 Permissions 分类可用，沿用现有 direct/structured target/capability 等约束及确认流程。成员分类不得操作旧权限选择。
- `grantable` 不是 UI 可编辑能力；MySQL SHOW GRANTS 长字符串必须通过完整详情可读，不新增 SQL 字符串解析。
- 空 Partial/Unavailable 分类表达“当前快照未列出”，不宣称没有有效访问权限。system 限制说明不能遮盖刷新错误。

## 2. 单元一：完成真实焦点、点击与权限导航闭环

**修改文件：**
- `src/ui/principal.rs`：render_overview、共享布局、焦点样式和 hit region。
- `src/input/keymap.rs`：Principal Overview 导航分支及相关配置路径。
- `src/input/mouse.rs`：Overview body 滚轮路径。
- `src/app.rs`：SelectPrincipalPermission、MovePrincipalPermission、SetPrincipalView 焦点/边界处理。
- `src/ui/mod.rs`：确有需要时增加 Overview viewport/hit metadata。
- `tests/principal_tabs.rs`：扩展保留 UiState/Buffer 的渲染辅助函数；如测试体量较大，新建 `tests/principal_overview.rs`，不复制整套 App 初始化基础设施。

### 步骤

1. 构造包含至少 3 条不同 target/privilege 的 PrincipalDetails fixture；使用现有 App 打开 principal、加载快照，焦点设为 Explorer。
2. 加入回归：真实 Keymap pane 切换 → App update → render 后 ACCESS 边框活动，摘要不活动；再切回 Explorer。
3. 加入真实坐标回归：渲染后找到第二条权限的坐标，调用 mouse mapper，再执行 Action，验证索引恰为 1；首行、表头、边框、空白分别断言。点击 DDL 的文本坐标必须产生 PrincipalView 动作并切换视图。
4. 加入键盘上下和自定义上下绑定回归，验证选择真的变化，不只是 mapper 返回通用 GridMove。滚轮测试以事件真实坐标输入。
5. 定向运行新增测试，确认当前失败由预期根因引起，而不是 fixture 或环境错误。
6. 将通用背景 hit region 移到具体目标之前，ACCESS 使用正确活动样式，数据行几何来自 body。不要改全局 hit test 顺序。
7. 让 Principal Overview 导航生成权限导航 Action；选择 reducer 验证可用快照/索引并建立 Results 焦点。滚轮只在 body 范围内路由，不影响 DDL 文本滚动或 Explorer。
8. 再运行本单元相关测试，记录命令、退出结果、实际代码版本与工作区状态。

**定向命令：**

```sh
cargo +1.94.0 test --test principal_tabs
```

如果新增 `tests/principal_overview.rs`，补 `cargo +1.94.0 test --test principal_overview`。期望所有定向测试通过；记录真实测试数，不预填。

**验收出口：** 用户从 Explorer 进入的是可见高亮 ACCESS；点第二条选择第二条；Overview 的 DDL 按钮可点击；上下与滚轮能访问权限列表后部；原 DDL 编辑器导航不变。

建议由 Luna 在单元通过后提交本单元精确文件集合：`fix(principal): restore overview access interaction`。不得 `git add .` 收入无关改动。

## 3. 单元二：分类表格与完整详情闭环

**修改文件：**
- `src/model/principal.rs`：分类、offset 和成员选择状态、边界方法。
- `src/action.rs`：分类选择、当前分类移动/首末/翻页、详情等必要动作。
- `src/app.rs`：统一导航处理、刷新 clamp、分类 mutation guard、TextDetail 请求。
- `src/ui/principal.rs`：分类条、权限表、成员表、响应式密度、状态区域和详情格式。
- `src/ui/mod.rs`：分类/条目命中目标与 viewport metadata，复用既有 TextDetail 路径。
- `src/input/keymap.rs`、`src/input/mouse.rs`：当前分类导航及详情。
- `src/help.rs`：当前分类/能力下的提示。
- `tests/principal_tabs.rs` 或新增 `tests/principal_overview.rs`；必要时 `tests/principal_contract.rs`。

### 步骤

1. 先加入模型状态测试：空分类、首末边界、三类独立位置、切分类恢复位置、移动滚动窗口稳定、刷新条目减少后的 clamp。
2. 定义 AccessSection 和局部状态，接入 tab 初始化及 details 成功应用路径。保持旧快照在加载/失败时可浏览。
3. 新增分类及当前条目 Action，使键盘、鼠标调用同一 reducer 路径；保留现有 permission mutation 的原始索引语义。
4. 先做标准宽度表格和分类条，给行、来源、选项语义样式；不再把 membership 和 coverage 追加到权限 Paragraph 尾部。
5. 加入三类各超过一屏的 fixture，验证任何一类末项都能经键盘和滚轮到达；分类点击同时建立 Results 焦点。验证分类零条仍可切换并显示正确空态。
6. 实现中等宽度隐藏辅助列和窄屏两行条目；所有行命中/可见数量均基于共享布局。窗口变更不改变选中条目的原始索引。
7. 实现完整详情：App 中根据当前分类/索引格式化数据，构造现有 `TextDetailRequest::new` 并走既有 OpenTextDetail 处理路径。不要从截断后的屏幕文字重建详情。
8. 详情权限内容包含 Target、Privilege、Origin、Source、Grant option、数据库/作用域与 coverage；成员内容包含 Role、Member、Admin option 及成员覆盖范围。长原生授权单独成段，完整可滚动。
9. TextDetail display_text 使用 sanitize_terminal_text；copy_text 遵循项目既有安全约定，至少完整包含所有未被列宽截断的文字。代码中不要让 App 依赖 ui::readonly_detail_request；该 helper 只可用于 UI 侧构造。
10. 接入 Enter、PageUp/PageDown、Home/End、左右分类与配置绑定。通用 GridMove/GridScroll 分支前完成 Overview 特化；已有 pending/overlay/pane 快捷键优先级保持。
11. 更新 g/v 上下文判断：仅权限分类进入旧授权操作；不可结构化/非 Direct 等维持现有拒绝路径，提示与能力相符。空权限和成员分类不能复用历史选择执行操作。
12. 删除或明确隔离 Overview 中无实际内嵌表单的旧 Tab/Space mutation_draft 快捷键路径；不得损坏真正 Overlay::PrincipalMutationForm 的字段导航。
13. 更新帮助与底栏，只显示当前可用的操作；验证 o/r 与 DDL 切换后返回分类位置。
14. 定向运行相关测试通过后记录结果，再继续单元三。

**行为测试必须包括：**
- Permissions→Member of→Members→Permissions 后各自位置恢复。
- 点击紧凑条目的任意显示行均选同一个数据项。
- Enter 详情包含屏幕省略掉的 source/option 和完整 MySQL 原生授权；Esc 返回原分类/选择。
- 加载有旧快照时保留内容并显示 Refreshing；失败有旧快照时错误与旧内容都可见。
- 在 Members 下按 g/v 不打开旧权限的修改表单。
- 刷新收缩或清空任意列表后导航、详情、渲染不越界。

**定向命令：**

```sh
cargo +1.94.0 test --test principal_tabs --test principal_contract
```

按实际新增测试 target 补充相应命令；局部模型/keymap 测试用 `cargo +1.94.0 test --lib <具体测试名>`。期望 PASS。

**验收出口：** 权限与两类成员均完整可达，长内容有完整详情，表格/标签/颜色明确，选项和来源语义正确，成员分类不会触发权限修改。

建议提交：`feat(principal): present access in navigable categorized tables`。

## 4. 单元三：状态、尺寸与跨页面回归收尾

**主要文件：** 上述测试文件、`tests/mouse.rs`、`src/input/keymap.rs` 的测试模块；仅对暴露的缺陷修改对应业务文件。无需新增独立主题系统或数据库测试服务。

### 步骤

1. 加入确定性 TestBackend 矩阵：120×40、80×24、56×16，以及最大化主内容区。按实际 ACCESS 内宽断言宽/中/紧凑模式，不按终端总宽猜测分支。
2. 覆盖 0/1/大量条目、中文和 emoji 名称、嵌入换行/控制字符、长权限/原生 SQL；检查字符宽度、裁切标记、命中区域都在 body 内。
3. 覆盖全部 source_kind 与 coverage 枚举，system principal + 请求失败、DdlOnly/Unsupported、刷新旧快照；空 Partial 不能写成“无权限”。
4. 检查默认和另一现有主题、ASCII 模式；断言选中背景和活动边框样式，避免只断言缓冲区字符串中有 ACCESS。
5. 重跑受影响的 principal/mouse/keymap 定向测试；DDL、SQL/Relation、Redis pane/grid 行为测试按实际影响选择，不无理由反复全量。
6. Luna 自行审查 diff：确认没有全局 Focus 新枚举、target_at 顺序修改、数据库字符串解析、跨层 UI 依赖、伪可编辑标记或 mutation 索引混淆。
7. 功能齐备后执行下列 CI 对齐验证一次；如果修复导致相关代码变化，重跑受影响检查，最终记录最新结果。

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

期望均 exit 0。项目依据 `.github/workflows/ci.yml:81-83`。不要将环境变量未设置导致的真实数据库测试跳过写成数据库集成验证通过。

8. 如环境可用，补一次真实终端鼠标/方向 pane 导航视觉检查；此为补充验证，用户未强制要求 PTY/截图。环境受限最多一次针对性修复重试，之后记录限制，由 Luna 收尾审查确定是否用 TestBackend/输入链路证据替代，不无限 progress。
9. 更新任务目录 validation.md，注明命令、退出结果、代码提交/未提交状态、相关文件及环境，保留历史记录并增加简短最新摘要。
10. Luna 完成审查、纠偏与后续提交合并流程。只提交任务涉及文件，不挪用原工作区用户改动；任务分支名称与 workflow 名称由 Luna 决定。

**验收出口：** 所有规格有确定性证据；剩余环境限制准确记录；没有新引入的导航、修改目标或内容可达性回归。

建议测试收尾提交：`test(principal): cover access layout and navigation regressions`；修复内容较多时按实际 diff 选准确提交类型。

## 5. 最终验收清单

### 门禁来源与验证分级

- **用户需求验收（必须）：** 通过焦点快捷键和鼠标进入 ACCESS；权限/成员展示可理解、可浏览。采用本计划的分类表格、语义标签和详情作为落地方案，下列清单中的行为正确性用于证明该方案完成需求。
- **项目既有门禁（必须）：** CI 中的 Rust fmt、clippy、all-targets/all-features test，命令见单元三。不能用定向测试替代项目最终门禁；环境失败与业务失败分别记录。未改动发布/安装器代码，不新增本地运行全套分发流程的要求。
- **本计划针对性自动回归（实施验证）：** 实际鼠标/键盘事件链路、行索引、分类状态、刷新与 mutation 边界测试；属于本次修改的正确性证据，不冒称用户指定了测试矩阵。尺寸/主题/Unicode 矩阵通过确定性 TestBackend 完成，不要求人工逐项签收。
- **补充建议（非阻塞门禁）：** 真实 PTY、截图、真实数据库视觉检查及额外主题抽查。用户未要求人工验收；不可因补充检查环境不可用而无限继续 progress。最多一次有针对性修复重试，再由 Luna 收尾审查决定补充确定性证据或记录限制。
- 本阶段只验证计划产物完整性，不执行尚未实现业务的上述测试，也不把预期通过写成实际通过。

### 行为与交付清单

- [ ] pane 快捷键/方向导航进入 ACCESS，可见焦点与实际输入目标一致。
- [ ] 鼠标可聚焦空白、切分类、选中正确行、切换 DDL，表头/边框不误选。
- [ ] 三分类键盘及滚轮均能到末项，各自选择与视口独立稳定。
- [ ] 表格列语义清楚、来源/选项有标签与辅助颜色，不显示 Debug 枚举包装。
- [ ] 长内容/隐藏列可以在详情完整查看，Unicode/ASCII 与窄屏不破坏命中。
- [ ] 刷新、失败旧快照、空列表、覆盖不足和系统限制文案准确且可达。
- [ ] g/v 只面向当前有效权限，原授权确认流程不回归。
- [ ] 可配置导航键、帮助与真实动作一致；其他页面导航无回归。
- [ ] CI 对齐验证及补充检查的实际结果完整记录，没有用分析期静态结论冒充运行通过。

## 6. 阶段交接

plan 阶段仅产出本计划、change-scope.json 并更新验证记录，不执行以上业务修改/测试/提交。完成产物核对后，最后写入本轮指定 `plan-97965528-c21e-4a08-b267-eea230f7ba51.json` completed 回执；不改写 analyze 历史回执。用户已选择自动工作流，Luna 后续从单元一开始执行，不再询问执行方式。

### 预计变更范围

`change-scope.json` 为本计划全部预计业务变更的仓库相对路径清单，包含可选新增测试文件；本轮没有删除、重命名或未提交依赖文件。仅读取复用的数据库适配器、主题、TextDetail 和 CI 文件不列入。任务目录内的报告及回执是工作流产物，不作为业务源文件进入任务提交。

预计修改：`src/ui/principal.rs`、`src/ui/mod.rs`、`src/input/keymap.rs`、`src/input/mouse.rs`、`src/app.rs`、`src/model/principal.rs`、`src/action.rs`、`src/help.rs`、`tests/principal_tabs.rs`、`tests/principal_contract.rs`、`tests/mouse.rs`。预计按测试体量新增：`tests/principal_overview.rs`。如实施发现必须超出清单，先记录原因并同步范围，不能把仅读取的参考文件无差别纳入。
