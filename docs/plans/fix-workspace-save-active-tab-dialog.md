# Workspace 保存一致性与失败弹窗优化实施计划

执行者：Luna；分析者：Astra。仅在实施阶段修改业务代码。任务/分支由 Luna 命名。

本计划在 plan 阶段调用 writing-plans 技能后定稿，取代分析阶段的草案。用户已选择自动工作流，按下述单元连续实施、复核和交付，不询问执行方式。原工作空间仅写任务报告；实施开始时由工作流准备任务分支/worktree。

**Goal:** 消除快照归一化导致的悬空活动标签引用，并让真实保存失败拥有明确、紧凑、键鼠一致的退出决策界面。

**Architecture:** 应用层在最终结构归一化后修复可选活动引用；持久化层保留严格校验。失败窗口复用 dialog、Theme、既有退出 Action 和保存状态机，仅扩展展示/焦点与帮助上下文。

**Tech Stack:** Rust、现有 Ratatui TUI、WorkspaceStore；沿用项目测试设施，不引入依赖。

## 单元一：快照生成→保存→恢复闭环

涉及 `src/app.rs:1993–2242`、`src/persistence/workspace.rs:357–529`、`tests/workspace_persistence.rs`，必要时 app.rs 内部测试（用于构造私有缓存状态）。

1. 阅读已有 workspace_persistence 及 app 内保存测试，增加能在原实现失败的归一化回归：缓存连接仍含 Console(C)/active=C，而权威 live record 已将 C 归属另一连接；以及重复 C 被 seen 去重。调用真实 App::workspace_snapshot 后 validate_snapshot，原实现应返回 active tab is not open。
2. 记录原失败，勿仅手造一个无效快照测试校验器，那不能证明修复生成器。
3. 在 normalize_workspace_snapshot 的最后一个结构性过滤后，对所有 profile 检查最终 tabs：Some(id) 不存在则置 None；有效 Some 和 None 保持。使用同一个 PersistedTab ID 方法，避免重复枚举。
4. 验证正常 SQL/Relation/Dashboard/RedisBrowser/PrincipalDdl 活动标签不会被误清空；含角色标签的场景保存及恢复成功。确认 SQL 文本、有效标签顺序保持，错误 owner/重复项仍按既有规则剔除。
5. 使用临时 WorkspaceStore 做 save/load roundtrip；保留 validate_snapshot 对外部手造非法 active UUID 的拒绝测试。无需修改 workspace schema/version。
6. 定向运行新增测试，然后 `cargo test --test workspace_persistence` 和 `cargo test --test workspace_tabs`；命令、退出码及当前 diff 记录 validation.md。

验收：上述原失败回归转绿；无需关闭角色标签即可生成合法快照；不弱化完整性校验，不删除有效编辑内容。

## 单元二：保存失败→选择→退出/继续闭环

涉及 `src/ui/mod.rs:7379`、新增 `src/ui/workspace_save_failed.rs`、`src/model/workspace.rs:140`（Overlay 定义）、`src/model/workspace_save.rs`（类型化选择及可用操作）、`src/action.rs`、`src/app.rs:5215–5308,7170–7174`、`src/input/keymap.rs:347–368`、`src/input/mouse.rs`、`src/help.rs:208,2734`、`docs/keybindings.md`；复用并扩展 `src/ui/dialog.rs` 的按钮布局测量。测试放在 `tests/keymap.rs`、`tests/mouse.rs`、`tests/ui_render.rs` 及相关源文件内部测试。

1. 精确定位 Overlay 定义、HitTarget→Action 路由以及现有帮助条件机制，不扩大到其它窗口整体重构。
2. 为失败 overlay 添加类型化焦点（Stay/Retry/QuitWithoutSaving）及必要的详情滚动状态；创建错误窗口时默认 Stay，非 retryable 排除 Retry。导航与激活复用统一 action 选择逻辑。
3. 实现分析报告中的面向用户文案与紧凑分区：摘要、影响、技术详情、按钮、快捷键提示；原始错误保留可查看。正文文本按项目现有终端文本处理规范处理。
4. 用实际显示宽度计算按钮横排/纵排及所需高度；不向可能纵排的 render_actions 传一行固定高度。详情视口可缩小/滚动，操作始终保留可达路径。
5. 增加按钮热区并接入既有输入路由。Esc/d/r 兼容，Tab/BackTab/方向键/Enter 与鼠标一致，retryable=false 时任何入口都不能触发不可用 Retry。
6. 新增失败窗口专属帮助上下文；底栏不再显示无效 q，真实动作提示随 retryable 状态变化。按 CONTRIBUTING.md:70–72 同步修改共享快捷键目录 `src/help.rs` 与 `docs/keybindings.md`。不新增配置项。
7. 扩展 app 退出状态测试：取消留在应用；丢弃仅走原退出动作；retryable 的重试走已有保存队列；成功必须等待 acknowledged+flushed；重复或旧 revision 回调不得使状态错误退出。
8. 用 TestBackend 渲染两类错误，覆盖 120×40、80×24、50×16、40×12、极小尺寸；长路径/中文/多行错误；验证可见操作、焦点、热区及帮助一致，不以逐字镜像实现的无意义测试代替行为断言。

验收：正常保存不弹窗；真实失败信息清楚，默认继续工作，所有显示的快捷键有效，全部操作支持合理的键鼠访问，窄/短窗口不隐藏危险操作的含义、不越界。

## 单元三：整体回归与交付

1. 在所有业务修改完成后执行一次项目要求的完整检查，使用下文已从 CONTRIBUTING.md 与 CI 确认的命令。独立 cargo check 与 clippy 的编译检查重复，不额外要求。
2. 对需要外部数据库、显示服务器或 PTY 的测试区分强制与补充，环境问题最多一次针对性重试，记录限制。正常的编译/断言失败由 Luna 修复；代码变化后仅重跑受影响检查，再完成必要全量验证。
3. 可选 PTY 补充：打开两个 PostgreSQL 角色标签、切换连接、退出；用可控测试 fixture 注入失败显示优化后窗口，不破坏用户真实 workspace 文件。
4. 对实际 diff 做最终审查，确认业务文件之外无附带修改，记录版本和最终验证。提交/合并由 Luna 执行，Astra 不接审查阶段。

## 状态与依赖

- 此计划基于干净起点 `b43731bdefd4acea8154f6365059306390734274`；没有需从原工作区复制的未提交业务文件。
- 不需要真实用户的 workspace 文件即可构造已确认缺陷的回归；截图历史路径未实机证明应在交付中如实说明。
- 继续维护当前任务目录的 validation.md；不覆盖既有历史检查，不把分析期静态阅读记为运行测试通过。

## 精确文件范围及职责

以下为预计全部业务改动；无删除或重命名，无未提交依赖。已同步 `change-scope.json`。参考的 CI、CONTRIBUTING、runtime、workspace_tabs 测试不列为修改文件。

| 文件 | 预计改动 |
| --- | --- |
| `src/app.rs` | 归一化引用修复；失败窗口焦点/激活/滚动更新；私有状态回归与退出状态测试 |
| `src/persistence/workspace.rs` | 将既有 tab_id 改为 pub(crate)，维持校验及磁盘格式 |
| `src/model/workspace_save.rs` | WorkspaceSaveChoice 类型、按 retryable 返回固定操作序列 |
| `src/model/workspace.rs` | 失败 Overlay 新增 focus、detail_scroll 字段 |
| `src/action.rs` | 窗口移动焦点、激活指定/当前操作、详情滚动动作 |
| `src/input/keymap.rs` | 专属按键映射及修饰键策略 |
| `src/input/mouse.rs` | 模态内按钮点击及详情区滚轮路由 |
| `src/ui/mod.rs` | 声明新 renderer 模块、HitTarget、渲染分发，移除旧私有 renderer |
| `src/ui/workspace_save_failed.rs` | 新增紧凑自适应 renderer、分区测量和局部渲染测试 |
| `src/ui/dialog.rs` | 提取供测量与绘制共用的按钮排布/高度计算，不改变其它窗口的传参 |
| `src/help.rs` | 专用上下文/条件化动作，内部测试覆盖帮助与输入一致性 |
| `docs/keybindings.md` | 对应失败窗口快捷键说明 |
| `tests/workspace_persistence.rs` | 保存/恢复与非法引用拒绝回归 |
| `tests/keymap.rs` | 重试/不可重试窗口按键矩阵 |
| `tests/mouse.rs` | 实际渲染热区→输入→Action 行为 |
| `tests/ui_render.rs` | 响应式界面、帮助提示与主题可读性回归 |

如实际枚举构造点位于其他文件，先搜索构造点并将必要的编译适配文件加入范围，再进行改动；不把整个 src 作为默认范围。直接复用 runtime 保存队列，不计划修改 runtime.rs。

## 单元一的具体实施与复核补充

### A1：建立可执行的红色回归

建议新增测试名 `workspace_snapshot_normalization_clears_removed_active_reference`（app 内部测试）和 `workspace_snapshot_preserves_valid_active_references`。构造至少两连接：一个陈旧缓存引用和一个权威 live console；保留另一有效标签作为负对照。通过真实快照生成验证目标错误是否出现。

运行：`cargo +1.94.0 test --lib workspace_snapshot_ -- --nocapture`。预期新增缺陷测试在修复前失败，且失败文本是目标校验错误，不是初始化或数据库环境错误。若上游过滤已挡住某个 fixture，调整到直接覆盖真实 normalize 方法的内部测试，并明确区分“归一化单元缺陷”与“完整 App 路径可达性”；不得硬编一个不存在的用户操作序列。此时同时保留正常 App::workspace_snapshot 端到端测试。

### A2：实施最小修复

在所有 profile tab 删除完成后，对每个 profile 使用现有 ID 函数做以下处理；最终收集 SQL 的逻辑保持原语义：

```rust
for profile in &mut snapshot.profiles {
    if profile.active_tab.is_some_and(|id| {
        !profile.tabs.iter().any(|tab| {
            crate::persistence::workspace::tab_id(tab) == id
        })
    }) {
        profile.active_tab = None;
    }
}
```

将既有 `fn tab_id` 的可见性改为 `pub(crate)`。不改变 validate_snapshot 错误文本、不删除其检查、不修改 schema version。复核每种 tab 类型的 UUID 来自同一函数。

### A3：保存恢复验收

使用 TempDir 和 WorkspaceStore，验证修复后的 snapshot 可 save/load，保留权威 SQL 文本、有效标签顺序和有效 active UUID；另外验证所有 tab 类型（尤其 PrincipalDdl）有效引用不被清空，原 None 不被强行补值。保持外部非法 active 引用被拒绝。

运行一次：`cargo +1.94.0 test --test workspace_persistence --test workspace_tabs`。预期退出 0。本单元通过后形成独立逻辑检查点，继续下一单元，不重复全套检查。

## 单元二的具体实施与复核补充

### B1：模型与输入先成闭环

在 workspace_save.rs 定义选择类型 Stay/Retry/QuitWithoutSaving，统一可用操作序列：非重试 `[Stay, QuitWithoutSaving]`，可重试 `[Stay, Retry, QuitWithoutSaving]`。所有窗口新建/再次失败默认 Stay，detail_scroll=0。App 更新先核对当前 Overlay 与 retryable，失效动作返回无命令；不要只在 keymap 层阻止 Retry。

按键协议定稿：Tab/Right/Down 下一按钮，Shift-Tab/Left/Up 上一按钮，Enter 激活，Esc Stay，d QuitWithoutSaving，r 仅可重试时有效；PageUp/PageDown 滚动技术详情。滚轮只在详情区域消费；其它区域不穿透到底层表格。普通快捷键只接受适当无修饰事件，沿用项目已有规范处理 Shift 字符。

复核：默认 Enter 等价 Esc 取消退出；鼠标和快捷键通过同一操作分发；不可重试时 r/伪造 Retry Action 不生效。沿用旧 revision 和 flush 的状态约束，不能因 UI 变化绕开事务检查。

定向测试：

```bash
cargo +1.94.0 test --lib workspace_save -- --nocapture
cargo +1.94.0 test --lib workspace_flush -- --nocapture
cargo +1.94.0 test --test keymap workspace_save -- --nocapture
```

新增测试统一使用 workspace_save 前缀，保证命令实际匹配；记录运行数量，零匹配不算验证。

### B2：实现 renderer 与按钮测量

提取独立 renderer 而不继续膨胀 ui/mod.rs。复用 Theme、DialogButton、DialogTone 与现有文本清理工具。抽出 dialog 的按钮宽度及所需行数计算，render_actions 与新窗口布局共用结果，防止两边各自猜阈值。

明确布局规则：76 列最大宽度，终端足够时左右至少两格外边距；内部左右两格在窄屏时压缩。先计算正文换行高度及操作高度，再分配详情视口。短错误只用所需高度；长错误详情受可用空间约束并可滚动。固定底部按钮与提示，不把整窗作为一个可裁切 Paragraph。

标题 error 色，边框 theme.border，正文 theme.text，revision theme.muted；聚焦使用项目现有可见焦点样式。Keep/Stay 为默认操作；危险退出使用 Danger。统一文案如下：

- 标题：`Workspace not saved`
- 摘要：`Your latest workspace changes could not be saved.`
- 影响：`Quitting may lose the latest tab layout and SQL editor changes.`
- 详情标题：`Details · Revision {revision}`（ASCII 模式使用可兼容分隔符）
- 按钮：`Stay in LazyDB` / 条件 `Retry save` / `Quit without saving`

极小终端不可能显示完整正文，不要求物理上不可能的“所有文字同时可见”；必须不 panic、不生成越界热区，并保持 Esc 取消与键盘选择可用。常规最小验收尺寸 40×12 必须完整显示所有按钮，优先收缩详情和留白。

### B3：帮助、鼠标和集成验证

共享快捷键目录使用专属 WorkspaceSaveFailed 上下文及 retryable 条件。同步 docs/keybindings.md；详情翻页和按钮导航都进入目录，底栏只显示真实可用操作，不出现旧 q。不要为了让旧提示成立而新增 q 别名。

在 tests/ui_render.rs 和 renderer 内部构造两类错误；断言关键用户信息、危险标签、默认焦点与按钮区域，不锁死无关背景像素。尺寸覆盖 120×40、80×24、50×16、40×12，以及 1×1 等不崩溃边界。中文/长不可断词/多行错误、ASCII 图标模式、现有亮暗主题均沿用现有测试 fixture。

tests/mouse.rs 从实际 UiState 提取按钮热区中心，执行 map_mouse 和 App::update，验证 Stay、Retry、Quit 各自行为；验证点击窗外不触发底层动作，调整尺寸后旧区域不能误触。

运行：

```bash
cargo +1.94.0 test --test keymap --test mouse --test ui_render
cargo +1.94.0 test --lib workspace_save -- --nocapture
```

若测试文件较大，开发中先使用 workspace_save 名称过滤，单元结束再运行上面的相关测试目标。对单元一文件无新增修改时不重复其独立测试；全量门禁仍会覆盖它们。

## 验证分级：需求、项目门禁与补充建议

### 用户需求与本计划的功能验收

用户要求根因分析、最佳方案和专业视觉优化；本计划将其落实为：悬空引用问题有可执行回归与修复、合法快照能保存恢复、真实保存失败可清楚解释并操作、布局和提示与实际行为一致。自动化定向测试是此实现方案的验证手段，不声称用户强制要求某个新增测试名称或人工操作。

### 项目已有本地门禁（必须执行并记录结果）

来源：CONTRIBUTING.md:6–12，.github/workflows/ci.yml:62–83。CI 固定 Rust 1.94.0；本地为可比性采用同版本。业务修改完成后执行一轮：

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

预期均退出 0。fmt 不通过先运行格式化再检查；编译/测试逻辑错误自行修复。只有代码或环境新变化影响结果时重跑相关检查，避免每个检查点重复全量。

### 项目 CI 环境门禁（保持现有工作流）

CI 还包含发行契约、Windows 安装器、Linux 数据库服务以及 macOS 二进制依赖检查。这些是现有 CI job，不因本任务取消，也不要求 macOS 本地伪造 Windows/Linux 通过证据。本地 macOS 可执行对应依赖检查：

```bash
cargo +1.94.0 build --locked
sh scripts/release/check-macos-dependencies.sh target/debug/lazydb
```

数据库集成测试是否实际运行取决于配置环境；全量测试成功但未配置外部 DB 时，明确记录相关用例跳过/未执行，不能宣称真实数据库回归通过。已有 CI 结果由 Luna 在交付审查中据实记录，本计划不新增远端 CI 权限或人工批准门禁。

### 补充建议（不是新增阻塞门禁）

- PTY 实机截图比较紧凑度、主题观感、键盘与鼠标使用体验。
- 在一次性测试连接上打开两个角色标签、切换连接、退出并重开。
- 检查更多终端字体、更多分辨率；不修改用户终端配置。

这些补充不可用时记录限制，由 Luna 收尾审查判断现有自动化是否充分，不要求用户提供生产凭据或手工实施剩余工作。环境受限最多一次有针对性的修复重试；普通代码问题不适用此次数限制。

## 最终复核和提交边界

1. 复核差异与 change-scope 对齐；不要写入运行时快照、真实连接资料、临时截图或凭据。
2. 复核关键不变量：有效活动标签不变，修复仅影响悬空偏好；非可重试错误仍不可重试；取消退出不会退出；保存成功仍需 ack/flush；显示动作均有输入实现。
3. 复核设计验收：短消息没有固定大空白，详情可读，默认焦点可见，危险操作文案明确，窄屏常规尺寸所有按钮可见，底栏无 q 假提示。
4. validation.md 记录每条实际命令、退出码、测试数、环境、代码版本/当前差异及限制。计划中的预期结果不得直接复制成实际通过结果。
5. 由 Luna 按工作流提交、审查并合并 main；可按逻辑修复与 UI 闭环分别提交，全部交付前完成集成门禁。不回切 Astra 做实现审查，不再询问执行模式。

## Plan 阶段完成状态

本轮重新检查 HEAD 与工作区，仍为指定起点且干净，没有新 worktree 需复制的未提交文件。已读取正式 analysis.md、checkpoint、CI 和贡献约定，补全计划及变更范围。本阶段未运行 cargo/PTY、未修改业务文件；实施验证由 Luna 执行。
