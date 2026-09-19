# Help / Omni Unified Panel Implementation Plan

> **执行者：Luna。** Astra 仅负责分析与计划。按下列可验收闭环顺序实施、验证、审查及提交；不启动子 Agent，不等待人工 resume。本文不要求调用不可用的 executing-plans 技能。

**Goal:** 将上下文帮助和 Omni 合并为可用 Tab 切换、配置默认页并记住最近选择的面板。

**Architecture:** 保留 `Overlay::Help` 和 `App.omni` 作为两个活动视图的现有容器，通过薄会话协调层保存非活动页及真实来源，统一切换和退出。复用搜索与命令执行，通过 `Action → App → Command → Runtime → SettingsStore` 定向保存 `[ui].help_panel`，不整体序列化设置。

**Tech Stack:** Rust 1.94、ratatui、crossterm、Tokio、serde、现有 toml/toml_edit；TestBackend 与 tempfile。

---

## 基线与范围

- 分析：`.git/opencode-tasks/ses_f4c88d45dffehhzq3PCa4zprRG/analysis.md`。
- 原始起点：`af23ff65072f203b5266769cb9898203a09854b5`，目标 `main`。
- 首次编写计划时 `main` 为 `5a67033`，已合并空工作区修复；本轮正式产出时为 `7d92f523d25a2cde39911e2ee7951d8fdc6645ee`，又合并了该修复的计划文档。保留空工作区修复及其他既有工作；不要用分析时行号覆盖当前源码。
- 任务名称、任务分支由 Luna/工作流命名。本阶段不创建分支、不实施、不提交。实施前检查实际 HEAD/diff，并由工作流选择工作树；若从原始起点建分支，合并前须整合最新 main 并验证空工作区路径。
- 保留另外四份既有未跟踪计划文件，不把它们加入本任务提交。
- 无 `checkpoint.json`；不要自行创建，也不修改 `state.json`。本轮正式计划保存至 `.git/opencode-tasks/ses_f4c88d45dffehhzq3PCa4zprRG/plan.md`；完成后仅写本轮指定的 `plan-b46f81d8-03bc-4ff3-98a4-93d95dab3b27.json` 回执，禁止重用 analyze 回执。

## 验证要求的来源与级别

| 类别 | 内容 | 完成判定 |
| --- | --- | --- |
| 用户明确需求 | 默认帮助内容、帮助快捷键入口、双向 Tab 及右上角提示、配置默认页、记住切换选择 | 必须实现，以自动行为测试验证；不要求用户人工签收 |
| 项目现有门禁 | `.github/workflows/ci.yml` 的 Rust fmt、clippy、all-targets/all-features tests | 功能齐备后执行并记录真实结果；失败需修复或明确记录实际环境限制，不能宣称已通过 |
| 本方案工程验收 | F2 兼容、Shift-Tab 保留对象操作、查询/步骤和上下文恢复、搜索隔离、原子定向写回 | 属于本次方案的回归保护，使用本文定向自动测试；不是额外要求用户提供环境或手动操作 |
| 补充建议 | PTY 实际交互、人工视觉检查、额外外部数据库环境复测 | 不自动升级为门禁；由 Luna 收尾审查按未解决风险决定是否执行或记录限制 |

计划阶段仅核对文档完整性、基线及产物，不运行尚未实现功能的测试。后续环境受限检查最多一次有针对性的修复重试；不得重复同一失败环境保持 progress。

## 固定交互契约

1. `[ui] help_panel = "help"` 为默认，另一个值为 `"omni"`。`?` / F1 及自定义 help 绑定遵循原有输入上下文规则，关闭状态下按偏好打开。
2. 纯 Tab 在两页以及 Omni 子步骤均切页；忽略 Repeat/Release，不落到底层窗格。右上角分别提示 `Tab -> Omni`、`Tab -> Help`，使用 ASCII 文案可同时兼容全部图标模式。
3. Omni 原 Tab 对象操作改为 Shift-Tab；Help 的 Shift-Tab 不切页。
4. 同一次打开保留各页查询/选择和 Omni 子步骤；关闭后重开只记视图，重新捕获上下文。
5. F2/omni 绑定从关闭状态直达 Omni，不修改默认偏好；从 Help 显式切到 Omni 则更新偏好；在 Omni 时关闭整个面板。Help 中“open Omni search”执行为切页。
6. Help Esc 关闭；Omni Esc 先退子步骤，根页关闭；Omni Ctrl-C 只关闭。搜索中的 `?` 仍为字符。
7. 切换立即更新内存偏好并保存设置；写盘失败提示但不回滚当前页。只写该偏好，不持久化查询、表单、CLI 覆盖。
8. Help/Omni 共用位置和尺寸：沿用 Help 的最大宽 74、高 34及终端钳制，保留现有 Help 列表容量。Omni 内容按实际矩形布局；极小终端使用可退出的降级提示。

## 会话状态设计

### 建议类型与接口

在 `src/config.rs` 定义可共享枚举，给 UiConfig 新字段加 `#[serde(default)]`：

```rust
#[derive(Clone, Copy, Debug, Default, serde::Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum HelpPanelView {
    #[default]
    Help,
    Omni,
}

impl HelpPanelView {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Help => "help",
            Self::Omni => "omni",
        }
    }
}
```

在 `src/model/help_panel.rs` 放会话元数据，`src/model/mod.rs` 导出；App 增加内存偏好和可选会话。采用以下所有权约定，避免多份 origin 不同步：

- Help 活跃：HelpState 在 overlay，暂停的 OmniState 在 session；真实 origin 在该 OmniState（尚未创建 Omni 时暂存于 session）。
- Omni 活跃：OmniState 在 `app.omni`，暂停的 HelpState 在 session；真实 origin 在 OmniState。
- 一次打开时捕获 Help 快照及 Omni origin context/tab/profile，即使默认直接进入 Omni 也预备 Help 快照；每一页状态只有一个可修改实例。
- session 用可选字段搬移状态，并用辅助方法约束组合；不得出现 `app.omni` 与 `Overlay::Help` 同时活动。
- 任何业务导航仍读取现有 `OmniState.origin_overlay`，不把合并面板自己的 Help 作为 origin。

App 内集中实现 `open_help_panel(view)`、`switch_help_panel()`、`close_help_panel()`、`finish_help_panel_for_action()` 一类辅助方法。命名可按实际代码调整，但关闭（恢复来源）与执行完成（交接来源、禁止旧 overlay 回弹）必须区分。禁止在 update 尾部仅凭 `app.omni.is_none()` 猜测关闭。

---

## 闭环一：默认打开 → 切页 → 使用 → 关闭 → 再打开

### Task 1：会话、偏好及键盘路由

**Files**
- Modify: `src/config.rs`, `config/default.toml`, `src/action.rs`, `src/app.rs`, `src/model/mod.rs`, `src/input/keymap.rs`
- Create: `src/model/help_panel.rs`, `tests/help_omni_panel.rs`
- Test/update: `tests/omni_input.rs`, `tests/omni_flows.rs`, `tests/keymap.rs`, `tests/redis_help.rs`

**步骤**
1. 在新集成测试文件建立 `App::new(Vec::new())`、`Keymap::default()` 和 KeyEvent 测试 helper，优先走真实 keymap→update 路径。
2. 添加默认 Help→Tab Omni→输入查询→Tab Help→Tab Omni→Esc→帮助键再开 Omni 的行为测试；断言 Help 查询独立、Omni 查询保留、底层编辑器文本和焦点不变。
3. 添加配置三类测试：缺字段默认 Help；`help_panel="omni"` 可加载；非法值报错。再添加旧完整 TOML 直接 `from_toml` 的兼容测试，避免只验证 merge 路径。
4. 运行 `cargo +1.94.0 test --locked --test help_omni_panel`，记录预期的新接口缺失或行为失败；这不是完成验证。
5. 实现枚举/UiConfig/default.toml、App 偏好 setter、session 及切换 Action；`ShowHelp` 作为偏好入口，内部构建 Help 使用独立 helper。
6. 在两个活动面板输入分支优先消费纯 Tab、阻止 Repeat；Omni Shift-Tab 映射 `OmniShowActions`；保留输入中的 `?`。检查默认/custom help 和 omni 绑定、清空 pending sequence。
7. 审核 `update_inner` 无 console 动作过滤，允许新面板动作，确保零 tab、仅关系 tab、Redis 场景不会丢弃动作。
8. 加 F2 关闭/直达、不改偏好，以及 Help 内 F2 修改偏好的测试；Help 搜索选择 OpenOmni 后执行应切页而非新建嵌套会话。
9. 运行 `cargo +1.94.0 test --locked --lib config::tests` 和 `cargo +1.94.0 test --locked --test help_omni_panel --test omni_input --test omni_flows --test redis_help`，预期全通过。

**完成条件**：所有打开路径共享面板语义，Tab 无穿透，同次保留状态，重开遵从内存选择；此时写盘交给闭环二，不在 reducer 内做文件 I/O。

### Task 2：退出、上下文及异步搜索生命周期

**Files**
- Modify: `src/app.rs`, `src/help.rs`, `src/model/help_panel.rs`
- Test: `tests/help_omni_panel.rs`, `tests/omni_input.rs`, `tests/omni_search.rs`, `tests/omni_resume.rs`, `tests/omni_navigation.rs`, `tests/keymap.rs`

**步骤**
1. 建立来源为真实 `Overlay::Message` 以及现有 ProfileManager/CatalogEditor fixture 的测试，验证切页后 Esc 恢复来源，导航保留暂存与 busy guard。更新 `omni_input` 原来恢复 Help 的测试：合并后用真实业务 overlay 验证恢复，单独断言 Help 不作为嵌套 origin。
2. 测试 Omni 子步骤→Help→Omni 保留 step/query/selection，Esc 仍回退；Help 快照保持原 context/capabilities/keybindings，不变成 Help context。
3. 列举 `self.omni.take()/self.omni = None`、`DismissOverlay`、`execute_help_shortcut`、OmniConfirm/ResumeInteraction、Help 行打开详情的所有终止入口，分为“暂时 take 后放回”“关闭恢复”“执行交接”，分别接入对应 helper。
4. Help 执行前保留 selected-id 与当前 availability 检查；涉及原 overlay 的校验应面对真实底层上下文。执行后清理隐藏页；遇到尚未完成的 Omni 步骤不清理 session。
5. 在切出 Omni 前获取 `omni_search_command`，搬走活跃 Omni 后用既有 owner/session/generation 取消请求。切回时递增 generation、刷新本地 items，必要时重发远程查询；不重置子步骤和查询。
6. 新增迟到成功/失败、返回后旧 generation、Explorer 搜索隔离测试。以 fake page/action 测试，不依赖真实数据库。
7. 测试 Help 行打开只读详情后再关详情不会复活暂停的 Omni；后台删除原 tab/对象后执行仍走现有可用性判定，不 panic。
8. 运行 `cargo +1.94.0 test --locked --test help_omni_panel --test omni_search --test omni_resume --test omni_navigation --test keymap`；有意改变的旧断言按新契约更新，禁止为了通过删掉 guard/上下文测试。

**完成条件**：退出恢复/导航交接不串页，不丢来源表单，取消和迟到事件不影响错误的视图。

### Task 3：共享弹窗框架、右上角提示和输入屏障

**Files**
- Create: `src/ui/help_panel.rs`
- Modify: `src/ui/mod.rs`, `src/ui/omni.rs`, 必要时 `src/input/mouse.rs`, `src/ui/text_selection.rs`
- Test: `tests/ui_render.rs`, `tests/mouse.rs`, `tests/help_omni_panel.rs`

**步骤**
1. 用 TestBackend 添加两页标题/右上角提示、相同边框坐标、帮助列表内容、Omni 底边颜色测试；覆盖普通尺寸、窄终端、极小终端及无 tab 工作区。
2. 抽取共同 popup 矩形和标题栏构造。原 Help 内容布局使用共同矩形；Omni layout 改为接收相同矩形并计算 input/results/status，不重复计算纵向 1/3 位置。
3. 右上角提示使用右对齐标题或明确分配的不重叠标题区域；极窄时缩短文案。保留 Help 底部说明，Omni 操作提示显式写 Shift-Tab actions。
4. 同时更新 `src/ui/mod.rs` 的空工作区与常规绘制路径。保留最新 main 的空工作区修复，不恢复已经删除的空壳框线。
5. 检查隐藏页不注册 cursor、selection/hit regions；保留 Omni 全屏屏障和动画处理。Help 切页后粘贴/鼠标文本选择应归新页面，底层点击不穿透。
6. 运行 `cargo +1.94.0 test --locked --test ui_render --test mouse --test help_omni_panel --test omni_providers`。
7. 闭环一验收通过后，仅暂存本闭环实际变更，提交建议：`feat(ui): unify help and omni panel navigation`。计划文件可随本任务提交；不能夹带其他计划或用户变更。

**闭环一总验收**：用户从现有帮助键开始完成两页切换、查询、选择执行/退出、再次打开；正常和空工作区均可用。

---

## 闭环二：用户配置 → 启动 → 切换保存 → 重启记忆

### Task 4：最小 SettingsStore 原子写回

**Files**
- Modify: `src/persistence/settings.rs`
- Create: `tests/help_panel_settings.rs`
- Reference only: `src/persistence/profiles.rs`, `src/persistence/paths.rs`

**接口**
- `SettingsStore::new(path: PathBuf)`。
- `SettingsStore::save_help_panel(view: HelpPanelView) -> Result<(), SettingsWriteError>`。
- 独立写入错误类型可放 settings.rs；现有 `AppSettings`/`SettingsError` 再导出继续可用。

**步骤**
1. 用 tempfile 写含注释、ui.icons、keybindings、自定义缩进的 TOML；保存 Omni 后检查解析值和原注释/无关字段仍在，并通过 AppSettings::load 重读。
2. 测试文件及父目录不存在时创建；旧文件无 ui 表时插入；`ui` 是 inline table 时正确编辑或明确报错且原文不变，禁止 panic。
3. 测试非法 TOML、错误 ui 类型、确定性 I/O 失败（例如目标父路径是文件）不覆盖原数据；避免依赖 chmod 在不同权限环境中的表现。
4. 每次保存重新读取文件；仅 NotFound 当成新文档。使用现有 toml_edit，修改 `ui.help_panel`，不从 AppConfig 序列化全文件。
5. 借鉴 profiles 写入流程：同目录 UUID 临时文件、create_new、write_all、sync_all、rename，失败删除临时文件；存在目标文件时复制其权限到临时文件，新文件遵循项目私有设置惯例。
6. 按顺序保存 Omni→Help→Omni 并重读，证明最后值为 Omni；每次读最新文件以保留两次保存之间加入的无关字段。
7. 运行 `cargo +1.94.0 test --locked --test help_panel_settings`，预期全部通过，fixture 全部在临时目录，实际用户配置无变化。

### Task 5：Runtime 顺序提交与启动注入

**Files**
- Modify: `src/action.rs`, `src/app.rs`, `src/runtime.rs`
- Test: `tests/help_omni_panel.rs`, `tests/help_panel_settings.rs`, `src/runtime.rs` 测试模块

**持久化调度选择**

为最小单字段写入，在 `Runtime::dispatch` 的新 Command 分支内顺序调用 SettingsStore，同步完成本次小文件写入再返回。这保证 Tab 动作顺序及退出前完成，不引入后台竞态或 quit flush 协议。此任务不构建通用设置写入队列；如实测小文件写入导致明显交互阻塞，改为单 worker 串行队列并增加退出 drain 测试，不能随意每次 spawn 写文件。

**步骤**
1. 增加 `Command::PersistHelpPanelView(HelpPanelView)` 和对应失败 Action；App 切页时先更新偏好，仅当值改变才发 Command。纯打开/F2直达、关闭和查询编辑不发写盘命令。
2. 在 Runtime 增加 `Option<SettingsStore>` 和 setter，与现有 workspace store 注入方式一致。测试 Runtime 不自动发现 HOME；缺 store 的测试实例不触碰磁盘。
3. `run_tui` 读取 settings 后调用 App 的偏好 setter，使用同一 `paths.settings_file()` 注入 Runtime；不要使用 CLI connections 路径，不序列化 apply_cli_overrides 后的其他字段。
4. dispatch 写盘失败向现有 action 通道发送失败消息，App 使用 notify_warning/notify_error 给出保存失败提示；不回滚 App 偏好，不自动循环重试。新的失败 Action 须通过无 console 分流。
5. 增加 Runtime/临时文件端到端测试：加载默认→help 入口→Tab→dispatch→重读 settings→新 App 注入→help 入口，结果应为 Omni；再反向验证 Help。
6. 快速按序切换三次并 dispatch，验证最后文件值；失败 store 验证当前会话重开仍是新值且产生通知。
7. 运行 `cargo +1.94.0 test --locked --test help_panel_settings --test help_omni_panel` 和 Runtime 新增测试的精确名称过滤；记录实际名称及结果。
8. 闭环二通过后提交建议：`feat(config): remember the selected help panel view`。

**闭环二总验收**：使用实际 settings 路径重读能恢复最近选择；错误输入与写入失败不破坏已有配置，非目标设置与注释保留。

---

## 闭环三：文档、整体验证与交付

### Task 6：同步用户契约

**Files**
- Modify: `docs/configuration.md`, `docs/keybindings.md`, `docs/omni-bar.md`, `docs/architecture.md`, `config/default.toml`, 必要时 `src/help.rs`

**步骤**
1. 配置文档添加可直接复制的 `[ui] help_panel = "help"` 示例，列出 omni 值、切换自动写回、下次启动读取手工修改、保存失败只保留本进程选择。
2. 默认配置注释及快捷键表改为帮助入口按偏好开页，Tab 双向切页，Shift-Tab 对象操作；保留 F2 直达及 toggle 说明。
3. Omni 文档解释 Esc 多步骤回退与统一面板关闭、F2 对偏好的规则；架构文档说明活动状态、真实来源和暂停页的区别。
4. 搜索 `Tab`、`F1`、`F2`、`Omni` 的相关现行文档/帮助条目，修正矛盾说明；历史 plans 不批量重写。

### Task 7：全量验证、审查、提交合并

**步骤**
1. 检查 `git diff --check`，核对实际 diff 只含本任务文件和计划；逐条对照本文固定契约与分析验收集。
2. 功能齐备后运行一次项目 CI 对应检查：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

3. 在任务目录 `validation.md` 追加各命令、退出码、HEAD/业务 diff 状态、环境及跳过信息。未启动数据库的测试不记录成真实数据库通过；禁止拿分析阶段无 diff 结果代替功能验证。
4. 由 Luna 审查源状态所有权、全部关闭/执行路径、键盘/鼠标隔离、旧搜索返回、配置只改一个字段、同步写入顺序和当前 main 空工作区修复。发现业务/编译问题直接修复，按影响范围重跑。
5. 可补充一次 PTY 人工交互检查（不是用户额外强制门槛）：F1→Tab→输入→Tab→关闭→?重开、重启、空工作区。环境受限时最多一次针对性重试，然后记录限制，由审查判断是否需要替代证据，不能无限 progress。
6. 文档提交建议：`docs: describe the unified help and omni panel`。最终提交/合并遵从工作流当轮授权，由 Luna 执行；需要 git commit 时使用 `git-commit` 技能，不由 Astra 提前执行。

## 完成清单

- [ ] 默认 Help 与自定义默认 Omni 均生效，旧配置兼容。
- [ ] 两向 Tab、右上角提示、Shift-Tab 对象动作、F2规则准确。
- [ ] 同次查询/步骤保留；关闭/动作完成无隐藏状态泄漏。
- [ ] 表单恢复、busy guard、帮助可用性校验和异步取消正确。
- [ ] 持久化仅改目标字段，快速切换顺序正确，失败不损坏配置。
- [ ] 常规/空工作区、小终端、鼠标/粘贴/光标测试通过。
- [ ] 文档同步，必要定向测试和最终检查有真实记录。
- [ ] Luna 完成审查与工作流交付，不触碰其他任务文件。

## 下一步

本计划完成后由 Luna 命名任务并进入实施。第一个可验收单元是闭环一（Task 1–3），顺序推进至闭环二、三；无需用户代为执行或重复要求 resume。后续回执只使用后续阶段明确指定的新路径/token。
