# Update Center Implementation Plan

> **执行者：Luna。** 按本计划逐个完成端到端可验收单元；实施、审查、纠偏、提交合并均由 Luna 完成。不要启动子 Agent，不等待用户反复 resume。本文由 Astra 在 plan 阶段编写。

**Goal:** 将更新中心改为状态明确、操作真实、布局紧凑、具有真实下载进度和安装阶段反馈的终端对话框，满足用户七项需求。

**Architecture:** 保留 Native 自更新及外部包管理器的职责边界，修复公共版本判定。共享状态驱动的动作模型连接 reducer、键盘、鼠标与独立更新 UI；通过带 request_id 的进度事件将安装流程接入现有动画与重绘机制。

**Tech Stack:** Rust 2024、Ratatui 0.30.2、Tokio、reqwest 0.12、semver、现有 Theme/dialog/shortcut_hints/clipboard。

---

## 0. 执行约束与起点

- 工作区 `/Users/yelog/workspace/tui/lazydb`，目标 `main`，用户指定任务起点 `303058ce01b77955379d7e2bf0bdebb3493388d1`。
- 本轮正式 plan 阶段实测 HEAD 为 `a1a028a8b6e1336f0a515c82645ac2c677fcaa21`，main ahead origin/main 3。起点后新增 `7884b20` 和合并提交 `a1a028a`，涉及 catalog_editor 的模型/UI、`tests/catalog_editor_state.rs` 与 `tests/ui_render.rs`，不涉及更新中心实现。不得重置或覆盖这些已合入工作；插件若从指定起点创建任务分支，后续合并需保留目标 main 的新提交。
- 设计依据：`.git/opencode-tasks/ses_f47bb673dffe5Y70LS11AHRfxw/analysis.md`。完整状态矩阵、布局、配色、根因已在其中，不重新讨论普通实现取舍。
- 任务/分支名称由 Luna 在后续阶段确定。本阶段不创建工作树或分支、不修改业务代码。
- 当前既有未跟踪文件 `docs/plans/2026-09-19-table-editor-modified-cell-highlights-implementation.md` 不属于本任务，不能纳入提交。
- 本阶段检查时 checkpoint.json 仍不存在。恢复时若已生成，先读取 checkpoint 与实际 diff；不得修改 checkpoint.json/state.json，不沿用旧日志的等待要求。
- 验证记录追加到任务目录 `validation.md`；保留分析基线，写明新检查对应的代码版本/diff、环境、命令与退出结果。
- 原始图片未实际传入会话；使用代码与确定性终端渲染作为设计依据，不宣称完成截图对比。
- 不执行真实用户安装目录的更新来测试；使用现有 fixture、临时目录和模拟 HTTP。
- 仅为行为变化、布局命中、进度可靠性等有实际回归价值的项目增加测试，不为简单文案逐字镜像实现。

## 1. 固定设计契约

### 界面与动作

| 状态 | 首行/颜色 | 可用操作 |
| --- | --- | --- |
| Idle | Ready to check for updates / 普通正文 | Check now、Close |
| Checking | Checking for updates… / action | Close（后台继续） |
| UpToDate | You're up to date / success；版本领先时 No update needed | OK；r check again |
| Native Available | Update available / action | Update、Not now |
| 外部 ManagerActionRequired | Update available / action；自然语言说明管理器 | How to update、Not now |
| 指引展开 | 真实更新说明、可选命令 | 有命令：Copy update command、Close；无命令：OK |
| Installing | Updating to … / action；阶段与进度 | Run in background |
| ReadyToRestart | Update installed / success；重启说明 | Restart now、Not now |
| Failed | Could not check for updates 或 Update failed / error | Check again、Close |

- 唯一边框标题 UPDATE CENTER，正文无 LAZYDB UPDATE。默认无 `Cargo: cargo install lazydb` 原始拼接。
- Not now/OK/Close 都只关闭；Run in background 只关闭且后台安装继续。没有 Skip this version 持久语义，不提供假的 Cancel。
- Copy 仅复制已知的结构化命令，不执行。无命令的安装指导不能出现 Copy。
- 默认焦点优先关闭/暂缓动作，单按钮选中唯一动作。检查完成或安装完成时不得自动把关闭焦点转为 Update/Restart。
- 状态文字和标记与颜色同时存在，复用主题色及 ASCII 回退，不靠颜色单独表达结果。
- 常规宽度上限 64 格，展开指引可到 76 格；高度按实际换行和操作/底栏行数计算，普通状态约 7–9 行，安装约 10–12 行。不是固定高度。
- 按钮和底栏分别占独立区域，底栏直接复用 ShortcutHint；窄屏可简写/换行，按钮不可见时不能留下命中区域。

### 进度

- 下载总量可靠且 >0 时显示实际字节百分比，标明是 Downloading 的进度。
- 检查、校验、解压、发布及未知下载总量显示实际阶段与不定进度；不用时间或阶段权重编造全流程百分比。
- 下载完成不代表更新成功；只有安装状态最终验证通过才展示 Update installed。
- Full/Reduced/Off 遵守现有动画设置；Off 仍显示状态、静态条和真实数字。

## 2. 单元一：检查结果 → 准确状态/紧凑界面 → 实际操作

**完成条件：** 同版本 Cargo/Homebrew 安装显示绿色最新与 OK，无命令；Native 新版本可用时 Update 真正发起安装；键盘和鼠标作用一致；弹窗无重复标题与多余底部空白。

### Task 1.1：修复公共版本分类

**文件**
- Modify: `src/update.rs` — `inspect_installation`、版本比较辅助函数与现有模块测试。
- Test: `src/update.rs` 内 inspection/report 测试；保留 `tests/update_native.rs`。

**步骤**
1. 扩展现有 ManifestHttp/FakeProbe 夹具，覆盖 Cargo、Homebrew、Unknown 的目标版本小于、等于、大于当前运行版本；夹具版本必须与 `CARGO_PKG_VERSION` 匹配，避免未来发布破坏测试假设。
2. 先运行新增的 inspection 测试，确认“等版本仍返回 ManagerActionRequired”能复现。
3. 在非 Native 分支先运行现有 semver/version_status_kind：Available 映射 ManagerActionRequired，UpToDate 保持，Error 保持；action 只为真实需外部更新或错误生成。Native 分支保留已安装版本/运行版本/降级策略。
4. 补充相同版本报告 `status=up_to_date`、schema=1、JSON 字段未变的断言；网络和 manifest 错误不能误显示为最新。
5. 修正外部来源有更新的通知判断与文字：明确需管理器更新，不说可在应用中直接安装。

**验证**
```sh
cargo test --lib update::tests::
cargo test --test update_native --test update_reducer
```
预期相关测试全通过。既有 Unknown + 1.3.0 fixture 在当前版本下仍要求人工操作，不能删除其异常元数据测试。

### Task 1.2：统一可执行动作和焦点

**文件**
- Modify: `src/model/update.rs`、`src/action.rs`、`src/app.rs`。
- Modify: `src/input/keymap.rs`、`src/input/mouse.rs`、`src/ui/mod.rs` 中 HitTarget。
- Test: `tests/update_reducer.rs`、`tests/mouse.rs`；键盘测试沿现有测试组织。

**实现契约**
在 model/update.rs 引入有类型的动作，而不是以 primary 布尔值决定行为。推荐定义：

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateDialogAction {
    Check,
    Install,
    Restart,
    ShowInstructions,
    CopyCommand,
    Close,
}
```

同一 helper 根据 UpdateState 与 overlay 指引展开状态提供有序可执行动作列表。标签由场景决定，例如 Close 可以显示 OK、Not now、Run in background。焦点保存动作或列表下标均可，但必须通过同一列表归一化。

**步骤**
1. 增加回归：默认关闭焦点时点击 Install 必须安装；对不可用或过期动作直接激活不能产生副作用；最新状态 Enter 关闭而非重新检查。
2. 增加 `Action::UpdateOverlayActivate(UpdateDialogAction)`；Enter 将当前焦点转换为显式动作；鼠标 HitTarget 直接携带动作。Install/Restart 在 reducer 再次验证当前状态与能力。
3. 所有动作合法性、可用按钮与焦点导航使用共享列表；Tab/Shift+Tab/左右方向循环当前动作，单动作无需切换。
4. 检查或安装状态变更后归一化焦点，优先保留 Close；清理与新状态不兼容的展开指引。
5. 重新检查只在允许的静止态执行，Checking/Installing/ReadyToRestart 的 r 不覆盖当前状态。失败按钮明确为 Check again。
6. 保留既有 workspace_exit_check 重启链路；不新增绕过未保存/事务保护的重启。
7. Task 1.3 完成实际指引入口时接通 ShowInstructions；中间代码状态不能作为单元完成提交。

**验证**
```sh
cargo test --test update_reducer
cargo test --test mouse update
```
若新增测试过滤名称不是 update，按实际命名调整并记录运行测试数，不能把“0 tests”记为验证通过。

### Task 1.3：紧凑渲染与底栏

**文件**
- Create: `src/ui/update.rs`。
- Modify: `src/ui/mod.rs` — 模块声明、overlay 分派、移除旧 render_update_overlay。
- Reuse: `src/ui/dialog.rs`、`src/ui/shortcut_hints.rs`、`src/ui/theme.rs`、`src/ui/icons.rs`。
- Test: 新模块内部 TestBackend 测试及 `tests/ui_render.rs`、`tests/mouse.rs`。

**步骤**
1. 构造全部状态的 TestBackend 夹具，优先覆盖最新/可更新色差、唯一标题、命中区域与可见按钮对应、长文本换行。
2. 独立生成状态正文与可选详情，不再让整个弹窗成为一个含空行/按钮/底栏的 Paragraph。
3. 先确定 clamped 宽度，再计算正文换行、按钮横排或竖排高度、ShortcutHint 行数；最终求出 popup/body/actions/footer Rect。测量和渲染必须一致。
4. 复用 dialog::render_actions 返回的精确区域注册动作 HitTarget；不得继续用半个弹窗宽度和 bottom()-2 估算。
5. 新建连接底栏用的是 shortcut_hints::render；更新中心沿用其键与描述样式、居中与背景。需要多行时调用同模块 lines 并按真实行数分配高度。
6. 添加自然语言来源说明和展开指引状态。此处最低要求 How to update 能打开文字指导，复制由单元二完成；不得显示不可用 Copy 按钮。
7. 窄屏优先显示结论和操作，长详情可滚动；如果增加滚动需为 overlay 保存 offset、映射 PgUp/PgDn 或上下键并显示对应提示，不只截断唯一的更新指导。屏幕小到不足一行时安全返回，无越界命中。
8. 默认状态中不展示 action 的原始拼接。对错误/外部来源文本复用 sanitize_terminal_text。

**验证尺寸**
80×24、120×40、40×16、24×8；另覆盖零宽/零高等安全边界。确认总高度由内容决定，正文不挤占按钮和底栏。长错误/CJK 文本须按终端格宽处理。

```sh
cargo test --lib ui::update::tests
cargo test --test ui_render --test mouse --test update_reducer
```

**单元一检查点**
记录实际 diff、状态/鼠标/布局测试结果；提交建议 `fix(update): align version status and dialog actions`。明确列出进度尚未完成，但继续进入单元二，不向用户请求 resume。

## 3. 单元二：外部更新指引 → 真实复制 → 反馈

**完成条件：** 默认无 Cargo 命令；主动打开指引才出现命令；复制确实发出既有剪贴板命令，有成功/失败反馈，不启动包管理器。

### Task 2.1：结构化指导与复制动作

**文件**
- Modify: `src/update.rs` — 管理器指导 helper，保留 CLI action 输出兼容。
- Modify: `src/model/update.rs`、`src/app.rs`、`src/ui/update.rs`。
- Reuse: `src/clipboard.rs`、`src/action.rs` 的 WriteClipboard/ClipboardWritten/ClipboardWriteFailed 管线。
- Test: `tests/update_reducer.rs`、`src/update.rs` 与 UI 模块测试。

**步骤**
1. 创建指导值类型，分离自然语言说明与 `Option<String>` 命令。不能从任意 action 字符串猜测它是否可执行。
2. Homebrew 使用已存在的 `brew upgrade yelog/tap/lazydb`；Cargo 使用已存在的 `cargo install lazydb`。Deb/Rpm/Arch/Unknown/Npm 保留准确文字指导，不凭空猜测包名或加入 sudo。
3. 展开区域用文字解释“在终端运行以下命令”；命令独占可换行区域。未知来源的错误诊断与指导分开呈现，不混入复制内容。
4. CopyCommand 仅在展开且有真实命令时合法；构造现有 ClipboardPayload，返回 WriteClipboard。使用项目现有成功/失败通知，不维护第二套剪贴板后端。
5. 复制后保留指引便于查看；Close 关闭弹窗。若剪贴板不可用，指引文本仍可查看，不宣称复制成功。
6. 覆盖 reducer 发出的确切命令载荷、无命令时 Copy 不可用、展开前 Copy 不可用、鼠标复制与 Enter 复制一致。

**验证**
```sh
cargo test --test update_reducer
cargo test --lib ui::update::tests
cargo test --lib update::tests::
```
真实 macOS 剪贴板为补充检查；核心验收是 payload 与反馈链路。环境失败最多一次定向修复重试，记录限制后继续。

**单元二检查点**
提交建议 `feat(update): add actionable external update instructions`；验证日志写明复制测试是否使用模拟边界，不能混同真实系统复制。

## 4. 单元三：Native 更新 → 真实进度 → 完成/失败/重启

**完成条件：** 更新时能看到当前工作阶段；已知总量显示真实下载进度，未知总量不伪造百分比；后台继续和重开有效；过期进度不能覆盖最终结果。

### Task 3.1：安装观察接口与实际阶段

**文件**
- Modify: `src/update.rs` — UpdateHttpClient/SystemUpdateHttpClient、install_current_native、apply_native_update。
- Test: `src/update.rs` 既有 Native/HTTP/文件系统夹具；`tests/update_native.rs`。

**数据契约**
建议将非 UI 专属进度类型放在 update.rs，model 引用它，避免安装模块反向依赖 UI：

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateStage {
    Preparing,
    Downloading,
    VerifyingChecksum,
    Extracting,
    VerifyingInstallation,
    Activating,
    Finishing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateProgress {
    pub stage: UpdateStage,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}
```

进度 observer 使用 Send+Sync 的回调/共享回调；无 observer 的旧入口作为薄封装继续可用。更新结果仍以 Result 返回，observer 只提供进度。

**步骤**
1. 给 UpdateHttpClient 增加具有默认实现的进度下载入口：旧 mock 仍可仅实现 download，默认入口报告开始/结束，真实客户端覆盖流式读取。
2. SystemUpdateHttpClient 保留 URL/redirect/status/timeout 校验，逐块读取响应并累计实际字节。实现时查阅对应 reqwest 版本文档决定 chunk 接口，不为未知 API 盲目添加 stream 依赖。
3. 不根据 manifest 或计时猜字节总量；使用响应可靠的 content length，0/未知或已读超过声明值则退回不定进度。
4. 测试小块多次读取、未知长度、读到一半失败、错误 HTTP 状态和重定向校验仍生效；测试可用本地 HTTP fixture，不触达真实更新服务。
5. 在真正进入每个阶段前发送事件；下载完成后再校验 checksum，然后解压、检查暂存二进制、原子发布、最终安装状态读取。
6. 将校验/解压/同步文件操作/暂存进程检查的耗时同步部分隔离到受控 blocking 任务。锁必须覆盖原有完整临界区，不能因拆分任务提前 drop；JoinError 必须传回安装失败。
7. 保留 checksum、安全路径检查、暂存清理、版本验证、原子发布与回滚逻辑；不要为进度重写安装算法。
8. 补充有序阶段测试和失败后不得发送成功阶段的测试；已验证的安全/回滚 fixture 继续运行。

**验证**
```sh
cargo test --lib update::tests::
cargo test --test update_native
```

### Task 3.2：runtime → reducer 事件与终态归一化

**文件**
- Modify: `src/model/update.rs`、`src/action.rs`、`src/runtime.rs`、`src/app.rs`。
- Test: `tests/update_reducer.rs`；runtime 内相关模块测试。

**步骤**
1. Installing 增加 progress；启动安装即保存 Preparing，使下载请求尚未返回时有明确反馈。
2. 新增 `UpdateInstallProgress { request_id, progress }`，runtime observer 转发。字节事件约每 100ms 或显著变化后发送，阶段切换和最后一条必须发送；不为节流另起无限定时任务。
3. reducer 仅在 Installing 且 request_id 匹配时更新，拒绝旧请求、晚于成功/失败的进度；同阶段字节不可倒退，阶段不得回退。转换为下一阶段时允许该阶段的无总量状态。
4. 关闭 overlay 不取消安装；重开读取同一 update_state，不重新发起安装。完成或失败只写当前请求结果。
5. 修复“锁内发现另一个进程已完成安装”分支：已安装版本不同于运行版本且验证匹配时 ReadyToRestart；运行版本已匹配时 UpToDate；真正无法验证才 Failed。不能把所有返回 UpToDate 的成功检查当作安装失败。
6. 保留重新检查后的显式 Update 确认，失败后不自动重试安装；ReadyToRestart 的 Restart 走原退出流程。
7. 测试交错旧请求、完成后迟到进度、失败后迟到进度、后台关闭重开、重复 Update、无 tab、并发已安装结果。

**验证**
```sh
cargo test --test update_reducer
cargo test --lib runtime::tests
```
后一个过滤器需以实际模块名为准，记录运行数量；如果 runtime 没有可注入测试边界，用纯 observer/节流 helper 的确定性测试与 reducer 集成测试覆盖，不触发真实安装。

### Task 3.3：进度条与动画重绘

**文件**
- Modify: `src/ui/update.rs`、`src/ui/animation.rs`、`src/ui/mod.rs` 的 animation_observation。
- Reuse: `src/ui/loading.rs`、`src/ui/icons.rs`。
- Test: UI update/animation 模块及 `tests/ui_render.rs`。

**步骤**
1. 新增以 request_id 区分的更新 LoadIdentity；仅可见更新弹窗需要持续动画观察，后台字节事件照常更新状态。
2. 更新观察放在 animation_observation 的 active_tab 提前返回之前，无 tab 的启动页也能显示动画。
3. 已知下载总量显示一行细进度条、百分比及字节；百分比只属于下载。计算分母检查并限制合法范围；未知/异常长度不显示 100% 假象。
4. 其他阶段显示流动段/静态段与准确阶段名；动作固定为 Run in background；下载完成切入验证时维持稳定窗口高度。
5. 复用已有时间源和 motion mode，不能在渲染中创建 timer 或使用 sleep；Full/Reduced 降低到现有节奏，Off 稳定不移动。
6. 用受控时间推进 TestBackend 检查动画变帧、Off 静态、状态数字仍更新；ASCII 模式条与标记可读，窄窗口无越界。
7. 覆盖下载 0%、中段、100% 后仍校验、未知长度、失败与成功退出动画，及后台关闭再打开当前进度。

**验证**
```sh
cargo test --lib ui::update::tests
cargo test --lib ui::animation::tests
cargo test --test ui_render --test mouse --test update_reducer --test update_native
```

**单元三检查点**
提交建议 `feat(update): show download progress and installation stages`。如果拆分提交，只有 Task 3.1–3.3 连通后才算本业务单元验收完成。

## 5. 最终验收与收尾（Luna）

### 用户需求映射

下表是用户要求的功能验收标准，必须实现；用户没有指定必须使用人工、PTY 或真实联网升级来证明。各任务的定向自动化测试是本方案为这些行为选定的验证方法，其夹具/命名可按现有代码调整，不将具体工具或建议尺寸升级为用户额外需求。

| 编号 | 可观察验收 |
| --- | --- |
| 1 | 边框只有 UPDATE CENTER，正文首行是状态而非第二标题 |
| 2 | 已是最新及默认更新结果不出现 Cargo 原始命令；需要时主动打开指引 |
| 3 | 最新 success、有更新 action、失败 error，且有文字/标记 |
| 4 | 每个按钮都有对应真实动作；OK/Not now/后台/重启含义明确；不存在 disabled Copy 占位 |
| 5 | ShortcutHint 与新建连接一致，且提示只包含当前有效快捷键 |
| 6 | 高度随内容和换行调整，按钮下无固定大片空白，命中 Rect 与可见按钮一致 |
| 7 | 真实下载百分比或不定进度，实际阶段可见，UI 响应不被同步安装工作阻塞 |

### 项目必要检查

强制门禁的来源是 `.github/workflows/ci.yml` 中现有 Rust CI，而非本次新提出的人工检查。门禁失败由 Luna 修复；若确由工具链/外部环境限制导致无法执行，应明确记录未验证项，不能以补充人工检查代替或声称通过。

CI 使用 Rust 1.94.0，计划时本机为 1.94.1。功能齐备后按项目约定执行一次；如工具链不可用，应记录实际原因和现有工具链替代结果，不能冒称 CI 通过：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

若修改了公共发行契约，对照 `.github/workflows/ci.yml` 追加相关 distribution 检查（`sh scripts/release/test-distribution.sh`）；只有实际相关变化才增加执行。没有变更或新问题不要反复全量 check/clippy/test。

### 补充检查

本节全部为建议性补充证据，不是用户或项目强制门禁。人工/PTY/系统剪贴板不可用本身不构成任务阻塞，不要求用户提供环境或反复恢复任务。

- PTY/人工观察：最新、原生可更新、外部指引、下载未知长度、下载完成后校验、失败、待重启；键盘和鼠标均走一次真实可见控件。
- 真实剪贴板可用时验证一次；不可用时保留可替代边界的确定性测试。
- 原始截图不可见，不伪造截图对比结果。可提供 TestBackend 文本/颜色断言作为主证据，PTY 截图为补充。
- 环境受限最多一次有针对性修复重试，之后由 Luna 收尾审查判定记录限制或补充其他证据，不无限 progress。

### 收尾顺序

1. 按七项验收表审查完整 diff，关注按钮动作、过期进度、版本判断与旧 CLI 兼容。
2. 补充必要纠偏与相应定向复测；仅实际影响全局时重复最终全量检查。
3. 更新 validation.md：命令、退出码、对应版本/工作区状态、环境及限制；不把分析时的 7 项通过当作改动后结果。
4. `git diff --check`，确认无无关文件、真实安装产物或用户文件进入暂存。
5. 按后续阶段授权由 Luna 提交/合并至 main，不自动重置现有 ahead 提交；不得覆盖用户已有改动。
6. 收到该阶段新指定回执路径/token 后只写该回执；绝不复用 analyze 回执或凭空生成阶段 token。

## 6. plan 阶段记录

早先收到简短 plan 阶段消息时已创建计划草稿；本轮按正式阶段指令先读取 analysis.md，再调用 writing-plans 技能，复核实际 diff、完善验证分级，并将完整计划复制到任务目录 plan.md。正式计划以任务目录副本为交接依据。

本轮 checkpoint.json 仍不存在；未改动 state.json/checkpoint.json，未启动子 Agent，未实施业务代码。当前 main 的其他任务提交已在第 0 节记录。本阶段仅检查计划与工作区，不重新执行编译/测试；分析阶段的 7 项测试仅属于旧起点基线，不代表当前 HEAD 或未来实现结果。

交接后第一个未完成业务单元为单元一，从 Task 1.1 的版本分类回归开始。后续自动进入 Luna 实施，无需用户选择执行方式。本轮完成回执限定为 `plan-00c0e26a-d5df-41b8-b0d5-c0f80baa68a4.json`，不覆盖历史 analyze 回执。
