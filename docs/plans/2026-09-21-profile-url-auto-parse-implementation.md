# 底部栏信息层级与更新入口 Implementation Plan

> 执行者：Luna。按以下单一业务闭环实施、验证、审查和提交；Astra 仅负责分析与计划。不启动子 Agent。当前阶段只写计划，不执行实现步骤。

**Goal:** 弱化默认版本，清晰标识更新，删除底部连接摘要及操作区徽标，并使可见更新入口与实际点击区域一致。

**Architecture:** 复用 UpdateState、Theme、IconSet 和现有 Update Center。footer 统一计算品牌、版本状态入口、快捷键和临时状态的区域；同一 Rect 用于绘制与鼠标命中。删除不再使用的连接摘要及专属鼠标目标，不修改更新服务或数据库行为。

**Tech Stack:** Rust 1.94、Ratatui 0.30.2、Crossterm、TestBackend。

---

## 0. 基线、边界与交接

- 已完成的分析：同目录 `analysis.md`；以其中三项需求和状态矩阵作为设计合同。
- 正式 plan 请求复核 HEAD 仍为 `aff7947dd7cef2f4d7796211fda5bbf3514d5fc9`。`git status --short` 仅显示未跟踪的 `docs/plans/2026-09-21-profile-url-auto-parse-implementation.md`；这是无关计划文件，不修改、不暂存、不纳入本任务。此前干净状态是上一轮的历史观察，不作为当前状态。没有未提交业务行为依赖需要纳入 worktree。
- 指定 checkpoint.json 仍不存在；不创建、不修改 checkpoint.json 或 state.json。
- 所有任务报告继续仅写本目录；不写 docs/plans。
- 工作流建议命名：**底栏精简与更新提示**。分支建议：`fix/footer-update-indicator`，由 Luna/插件在计划完成后确定并创建；本阶段不创建分支或 worktree。
- 若后续在新 worktree 实施，从指定起点建立；若原工作空间后来出现本地修改，先辨别所有者和依赖，不 stash、不清空、不整体提交。原工作空间未提交文件不会自动进入 worktree。
- 本轮完成回执为 `plan-f0296bfc-69e8-4c93-820e-05a9ea674025.json`，token 为 `f0296bfc-69e8-4c93-820e-05a9ea674025`；完成计划与 change-scope.json 后最后写入，不覆盖 analyze 回执。
- 预计修改文件固定为 `src/ui/mod.rs`、`src/input/mouse.rs`、`tests/ui_render.rs`、`tests/mouse.rs`；新增文件为 `src/ui/footer.rs`。没有预计删除/重命名文件，没有未提交依赖文件。具体清单同步到 change-scope.json；报告元数据不属于业务补丁范围。

## 1. 一个端到端可验收单元

本任务体量适合一个闭环：**状态化版本入口 + 删除连接/模式槽位 + 实际点击回归**。以下步骤依次执行，不将“只删文案”或“只改颜色”作为独立完成品。

### 步骤 1：建立有意义的回归合同

**文件：** `tests/ui_render.rs`、`tests/mouse.rs`。

1. 更新 `workspace_header_and_footer_render_without_redundant_status_rows`：只检查最后一行，要求 LAZYDB/当前版本，禁止 fixture 连接名、数据库摘要及 EXPLORE/NORMAL/DATA/DDL 等旧徽标。不要对整屏禁用这些词。
2. 将 `connected_header_database_summary_opens_database_selector` 改为渲染后底部不提供连接摘要操作的合同；在删除 HitTarget 变体前可先断言其缺席，删除后通过实际鼠标行为覆盖。
3. 添加状态矩阵渲染用例：Idle、UpToDate、Checking、Available、ManagerActionRequired、Installing、ReadyToRestart、Failed。Available/ManagerActionRequired 包含缺少 target_version 的样例。断言最终 footer 文本和对应 buffer cell 的样式，不只测 helper 输出。
4. 添加真实渲染后的点击用例：找到 UpdateCenter 区域，遍历可见文字 cell，调用现有 map_mouse 并断言 OpenUpdateCenter；检查入口前后相邻位置不触发更新中心。
5. 用现有 fixture 和 UpdateInspection 构造方式，避免网络、安装器、真实数据库依赖。

**先定向运行：**

```sh
cargo +1.94.0 test --test ui_render footer
cargo +1.94.0 test --test mouse footer
```

新用例命名包含 footer，确保筛选确实执行测试。预期修改实现前出现与新合同直接相关的断言失败；环境/编译失败不能冒充预期红灯。记录实际测试数量和失败原因。

### 步骤 2：实现状态化版本呈现

**文件：** 新增 `src/ui/footer.rs`，在 `src/ui/mod.rs` 声明私有模块并接入。呈现 helper 放入 footer 模块，保持其私有实现。已有 `state.activity_icons` 可提供 IconSet，`IconSet::mode()` 已存在，因此无需修改 `src/ui/icons.rs`。

1. 直接匹配 `app.update_state`，不用 `app.update_inspection().status` 驱动 footer，避免 Installing 沿用旧 Available。
2. 输入包括运行版本、UpdateState、IconSet 和 Theme；运行版本在生产调用处传 `env!("CARGO_PKG_VERSION")`，允许测试注入长版本验证布局。
3. 常态版本 Span：`Style::new().fg(theme.muted).bg(theme.surface)`，不加粗、不加 DIM、不加按钮背景。
4. 状态映射如下；版本本身始终 muted：

| 状态 | 后缀 | 颜色/字重 |
| --- | --- | --- |
| Idle / UpToDate | 无 | — |
| Checking | `Checking` | muted |
| Available / ManagerActionRequired | `↑ Update`；ASCII 为 `^ Update` | warning + BOLD |
| Installing | `Updating` | action |
| ReadyToRestart | `Restart` | success + BOLD |
| Failed | `! Update failed` | error |

5. NerdFont 和 Unicode 模式都使用普通 `↑`，ASCII 使用 `^`。不引入动画、私有字体字符或新主题配置。
6. 不把目标版本放进 footer，不因目标版本缺失隐去状态，不在安装后把运行版本替换为已安装版本。
7. 点击统一打开原 Update Center，由现有 dialog_actions 决定安装、说明、重启或重试。

### 步骤 3：重写 footer 区域分配并移除冗余信息

**文件：** `src/ui/mod.rs:4491–4672`（基线位置）、新增的 `src/ui/footer.rs`。

1. 保留单行背景、LAZYDB 品牌以及现有快捷键来源。删除 connection_summary 调用、identity_prefix、profile/database 截断预算、mode/mode_badge 分支和它们的样式。
2. 保留 connection_state 返回的 TARGET/LINKING/FAILED；保留 terminal_selection_mode 的退出提示。
3. 采用显式区域：左品牌、左版本入口、弹性快捷键、右临时状态。所有宽度使用 cell width 和饱和减法，每个区域都限制在 footer 内。
4. 正常布局先计入品牌、版本状态、两侧间距和右侧提示的真实宽度，再把剩余宽度交给 `shortcut_hints::line`。该 helper 不填满剩余区域，不能通过其文字长度推测右端坐标。
5. 右侧提示提前分配；终端选择完整文案放不下时缩为 `SELECT Esc`，连接瞬时状态保留原短词。快捷键可以降为省略提示或隐藏。
6. 若固定内容仍放不下，隐藏品牌并压缩版本数字；存在更新/重启等状态时优先保留状态词。对极小输入裁切到真实可用 cell，不画越界字符或空热区。应用 TooSmall 分支继续按原逻辑提前返回。
7. 版本入口用其实际分配且实际可见的 Rect 绘制并注册一个 UpdateCenter 热区；不要把右侧空白算进入口。旧尾部 update_badge 渲染及其右端热区一起删除。
8. 不改变现有 footer Help 兜底行为；确保更具体的 UpdateCenter 热区仍按既有命中优先级生效。
9. 删除仅供底部使用的 connection_summary 和 update_badge；保留其他区域使用的 header_text、截断工具、主题样式及局部编辑模式。

确定将 footer renderer 与其专属呈现/布局 helper 放入 `src/ui/footer.rs`，通过父模块统一接入；原各工作区调用可保留薄包装或统一调用 `footer::render`。保持只在 UI 模块内可见，不额外建设通用组件框架。

### 步骤 4：清理旧连接点击合同

**文件：** `src/ui/mod.rs`、`src/input/mouse.rs`、`tests/mouse.rs`、`tests/ui_render.rs`。

1. 删除 HeaderProfile、HeaderDatabase 两个 HitTarget 变体及全部生产匹配分支：包括鼠标主分派、手势分类/许可判断，不能只删 renderer。
2. 保留 Action::OpenDatabaseSelector、ExplorerSelect 以及真正使用它们的其他入口。
3. 迁移 `tests/mouse.rs` 在基线 694、700、1187、1661、1999、2048 附近对旧目标的构造。先确认每个测试原目的：若验证旧专属操作则替换为新底部操作合同；若验证覆盖层、选择、手势行为，则选能保持该场景的现有目标。
4. 将 `header_profile_hit_region_uses_terminal_display_width` 替换为版本/状态的真实区域测试，覆盖 Unicode、ASCII 和长版本预算；不要直接删除后留下交互宽度无覆盖。
5. 全仓检查旧名称只剩无关历史文档或完全消失，清理无用 import；局部编辑模式显示及上下文快捷键仍正常。

### 步骤 5：完成布局与可访问性回归

**文件：** `tests/ui_render.rs`、`tests/mouse.rs`；必要的私有宽度测试放在 renderer 所在模块。

- 56/80/120 列：版本可读、有更新标识不被快捷键挤掉、临时状态可见。
- ASCII 与 Unicode：标识按模式选择，点击宽度按 cell 一致；无颜色模式仍有状态文字。
- 长 prerelease 版本和 0/极小 helper 宽度：无越界/下溢/空热区。
- Available + FAILED、ReadyToRestart + terminal selection 等组合：状态和 Esc 提示不重叠。
- 切换 UpdateState 后重新渲染：旧热区不残留，恢复常态后不显示旧 Update。
- SQL、关系、Dashboard、Redis、Principal 的代表性 fixture 共用新 footer，TooSmall 保持原行为。无需为每种 tab 重复完整状态笛卡尔积。
- 保留基线连接瞬时状态测试的行为意义，位置断言按显式右侧 Rect 调整。

定向修复后运行一次相关测试集：

```sh
cargo +1.94.0 test --test ui_render --test mouse --test update_reducer
```

预期退出 0。footer 私有宽度边界测试放在 `src/ui/footer.rs` 的 tests 模块，运行 `cargo +1.94.0 test --lib ui::footer::tests`，确认筛选执行数大于 0 并记录实际测试名称。

## 2. 功能齐备后的统一验证与 Luna 审查

### 验证级别与来源

- **用户必需验收：** 默认版本为次要颜色；有更新时清晰可辨；底部连接身份及其操作被移除；底部当前操作区显示被移除。以真实渲染与点击测试证明。
- **实现回归验收：** 本方案引入的状态矩阵、热区同源、宽度预算、ASCII/无色及现有临时状态兼容，由步骤 1–5 的定向自动化覆盖；它们是本次改动的正确性证据，不是用户额外要求的人工演示。
- **项目现有强制门禁：** `.github/workflows/ci.yml` 的 Rust fmt、全目标全 feature clippy 和测试。下列三个 Cargo 命令对应现有门禁，不因缺少 PTY 而省略。其他平台/数据库 CI 按其原配置执行，本计划不新增本地搭建要求。
- **补充建议检查：** `git diff --check`、可选人工终端观察/截图。人工观察没有被用户或项目定义为必需门禁，不将缺失截图或 PTY 变成任务阻塞条件。补充检查发现真实代码缺陷仍需修复。

依照现有 CI Rust 合同执行一次，随后进行补充差异检查：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
git diff --check
```

预期全部退出 0。无需额外重复 cargo check，因为 clippy/test 已编译相关目标。后续仅在相关代码或环境变化时重跑受影响检查。

Luna 收尾审查清单：

1. 三项用户需求都在真实 footer 生效；未因全屏字符串断言误伤 Explorer 标题。
2. 所有状态语义正确；特别是 ManagerActionRequired、Installing、缺目标版本。
3. 可见入口与 hit region 同源，没有隐形连接点击或右端幽灵更新热区。
4. 无色、ASCII、窄屏、临时选择模式有自动化证据。
5. 保留局部模式、连接管理能力、快捷键上下文与 Update Center 工作流。
6. diff 仅包含本任务代码与必要测试，没有更新器/数据库的无关重构。

人工 PTY 检查属于补充视觉验证；没有截图/PTY 不阻塞已由 TestBackend 覆盖的合同。环境受限最多一次针对性修复重试，然后记录限制由 Luna 判断证据是否足够。数据库服务相关 CI 或外部依赖缺失必须明确记录，不能将跳过称为通过。

每次实际检查在本目录 validation.md 追加命令、退出码、执行用例数/失败摘要、代码版本、工作区状态与环境；当前计划列出的命令尚未执行，不预填成功。

## 3. 提交与完成条件

- 实现、验证、纠偏、提交和向 main 集成都由 Luna 按当前工作流协议执行，Astra 不参与后续审查。
- 一个业务闭环对应一个优先推荐的逻辑提交：`fix(ui): simplify footer and clarify update status`。仅暂存实际修改的任务文件，不使用全工作区盲目暂存。
- 提交前工作区出现未知文件/差异时先辨别来源；报告目录不进入业务提交。
- 完成证据：三项需求、状态/布局/交互验收全部满足，相关测试和统一检查有本次代码版本的记录，Luna 审查结论明确。若检查环境受限，按实际协议记录限制，不伪造通过。
- 下一阶段第一步：Luna 确定工作流/分支名并准备实现工作区，随后执行步骤 1 和同一闭环的后续步骤；不要求用户再次 resume 或手动完成剩余功能。
