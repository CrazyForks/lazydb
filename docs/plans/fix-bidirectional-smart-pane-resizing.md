# 智能面板双向缩放 Implementation Plan

> 执行者：Luna。Astra 只负责本分析/计划阶段；不启动子 Agent。任务名、任务分支由 Luna 确定。按下述可验收单元持续执行，无需等待用户 resume。

**Goal:** 同一焦点下 Explorer 的 Cmd+Ctrl+Shift+h/l 可缩小/增加宽度，SQL Editor 的 Cmd+Ctrl+Shift+k/j 可缩小/增加高度；内部不可调整时才回退 Kitty。

**Architecture:** 纯决策选择当前 pane 同轴的一条可见分隔线并双向移动；布局层提供与渲染一致的实际范围，Runtime 互斥执行内部尺寸动作或外部请求。保留现有配置、尺寸模型和 Kitty helper。

**Tech Stack:** Rust 2024、Ratatui 0.30.2、Crossterm 0.29、Tokio、TOML、Kitty。

## 0. 当前检查点与实施边界

- 指定起点：`23c63a1996ff24c69dcbd58a38c93629d4c55ccd`，目标 main，原工作区 `/Users/yelog/workspace/tui/lazydb`。
- 本次正式 plan 阶段只读核对：HEAD 为 `2c19622a3f377080d09e9c4e232e2bad45ca1b54`，工作区及 index 均干净。之前记录的 smart-maximize 本地修改已由其他工作提交为 `2c19622 feat(kitty): synchronize LazyDB pane maximize`，不是本任务的修改。起点至当前 HEAD 的 `src/model/pane_resize.rs`、`src/ui/layout.rs`、`src/input/panes.rs` 无差异。
- 本计划不依赖这些本地修改，也不依赖 HEAD 新增的 smart-maximize 功能。从指定起点创建隔离任务 worktree；未提交文件不会自动复制，不搬运、清空、stash 或整体提交原工作区。
- 与其他工作可能重叠：`src/input/keymap.rs`、`src/runtime.rs`、`docs/keybindings.md`。集成时按具体 hunk 处理，保持其他任务功能；不要以重置文件方式消除冲突。
- `checkpoint.json` 在本次读取时不存在；不创建或修改它，不读写 state.json。分析依据见同目录 analysis.md；历史计划中的单侧边界约束已由本次新契约取代。
- 本阶段只写本任务目录，不执行实现、提交或合并。本次专属回执为 `plan-6e507344-2b15-4606-a0f6-920aa00d3c05.json`，token 为 `6e507344-2b15-4606-a0f6-920aa00d3c05`；完成计划和 change-scope.json 后最后写入，不覆盖 analyze 回执。

### 验证分级与变更清单

- **用户需求验收：** Explorer 同焦点 h/l 减小/增加宽度、SQL Editor 同焦点 k/j 减小/增加高度，使用用户现有 Cmd+Ctrl+Shift 配置；以自动化按键及真实布局往返测试验证，不要求用户另行人工确认才算完成。
- **项目强制门禁：** CONTRIBUTING.md 规定的 fmt、全目标全特性 clippy/test，以及快捷键共享帮助目录与文档同步。命令见第 4 节。
- **本方案实现正确性检查：** 同轴分隔线矩阵、Redis 优先级、布局实际上下限、内部/外部互斥、Repeat 和输入隔离。这些是选定方案需要的自动回归，不冒称为用户逐项提出的需求。
- **补充建议验证：** 真实 Kitty/PTY、未修改 helper 的 Python 回归；受限时记录证据边界并由 Luna 收尾，不能提升为新的人工阻塞门禁。
- `change-scope.json` 精确列出 7 个预计修改文件；测试放在其中 Rust 文件的已有测试模块。无预计业务文件新增、删除或重命名，无未提交依赖文件。`src/app.rs`、配置文件、外部 Kitty/settings、helper、现有 tests/mouse.rs 仅参考或运行已有测试，不列为修改范围。
- 计划正文只保存在本任务目录，不额外创建 docs/plans 文档。若实施发现需要扩大范围，先记录实际理由并更新清单，不静默混入其他任务改动。

## 1. 固定行为契约

方向表示分隔线在屏幕上的移动：h/k 为 −3，l/j 为 +3。不是无条件增大当前 pane。

| 当前 pane | h / l 的 split | k / j 的 split |
|---|---|---|
| Explorer | ExplorerWidth | 无 |
| SQL Editor | ExplorerWidth | EditorHeight |
| SQL Results | ExplorerWidth | EditorHeight |
| Relation / Dashboard / PrincipalDdl | ExplorerWidth | 无 |
| Redis Keys | RedisKeysWidth 优先；该 split 不可见时用可见 ExplorerWidth | 无 |
| Redis Preview | RedisKeysWidth | 无 |

- 两个方向控制同一条选定分隔线。Redis Keys 两侧都有边界时优先 Keys/Preview，达到极限不换另一条内部线。
- split 不可见或夹紧后尺寸不变，返回 Boundary，保留原方向。剩余 1/2 格只移动剩余量，不同时回退 Kitty。
- current 来自本次实际布局，不能从可能超界的偏好直接加减。
- overlay/Omni、TooSmall、无有效源 pane 或终端尺寸读取失败：不做内部/外部操作。
- 窄屏/最大化不调整隐藏边界；Redis 主区域最大化但 Keys/Preview 可见时仍可内部缩放。
- 保持焦点、SQL 文本、选区和其他偏好。Press/Repeat 执行、Release 忽略；命中后清除 pending sequence。
- Kitty 不可用时内部操作仍有效，Boundary 无操作。现有四条用户绑定及 Kitty 放行无需变更。

## 2. 单元一：双向尺寸决策与实际布局闭环

**修改文件：** `src/model/pane_resize.rs`、`src/ui/layout.rs`、`src/runtime.rs`。测试优先放各文件已有测试模块。

### 2.1 建立失败回归

1. 在模型测试中以 `explorer=(50,34,59)`、`editor=(20,5,30)`、`redis=(30,16,50)` 建立表驱动断言：
   - Explorer Left → Internal ExplorerWidth 47；Right → 53。
   - Editor Up → Internal EditorHeight 17；Down → 23。
   - 表中其他 pane 两方向各一例，覆盖正确 split 和符号。
2. 增加 selected split=None、到 min/max、剩余 1/2 格及 Redis 双边界优先级测试。
3. 将旧 `right_edge_editor_falls_back_to_kitty` 改为 Editor Right 调整 ExplorerWidth；另用 ExplorerWidth=None 建立正确的外部回退测试。
4. 执行 `cargo test --lib model::pane_resize::tests`。新增缩小测试应在原实现返回 Boundary 时失败；记录真正的业务失败，不将编译错误计为红测试。

最小失败回归可直接加入该文件现有 `tests` 模块：

```rust
#[test]
fn smart_resize_shrinks_without_changing_focus() {
    for (pane, direction, split, size) in [
        (SmartResizePane::Explorer, PaneDirection::Left, PaneSplit::ExplorerWidth, 47),
        (SmartResizePane::Editor, PaneDirection::Up, PaneSplit::EditorHeight, 17),
    ] {
        assert_eq!(
            decide(pane, direction, Some((50, 34, 59)), Some((20, 5, 30)), None),
            SmartResizeDecision::Internal { split, size },
        );
    }
}
```

此测试只验证纯决策；焦点确实不变必须由 2.4 的 App 组件测试证明，不能用测试名称替代状态断言。

### 2.2 实现纯决策

1. 按第 1 节选定 split 和对应 bounds，再按方向加减；复用一个尺寸计算函数，避免每个 match arm 复制算法。
2. 算法：`next = if positive { current.saturating_add(3) } else { current.saturating_sub(3) }; next = next.clamp(minimum, maximum)`。仅 `next != current` 返回 Internal，否则 Boundary。
3. Redis Keys 只在 RedisKeysWidth 不可见时选 ExplorerWidth，不在移动量为 0 时换线。
4. 重跑模型测试，所有新增及现有契约测试通过。

### 2.3 统一真实尺寸边界

1. 在布局层添加小型 bounds 接口，以同一布局计算路径得到实际 `(current,min,max)`。
2. 优先方案：对选定 split 分别将偏好设为 `Some(0)`、`Some(u16::MAX)`，调用已有 calculate，读取实际 pane_metrics 作为 min/max；保持 area/focus/tab 类型/maximized/其他偏好一致。Redis 使用 RedisBrowserLayout 的相同方法。计算放在按键路径，不增加每帧探测。
3. 如采用共享公式 helper 替代探测，必须证明短窗口约束竞争下与实际布局一致；不能只把常量暴露给 runtime 后继续重建计算。
4. 在 runtime 去掉手写 `34/60`、`body.height-3`、`2+7`、`16/24` 范围推导。普通 180×50 SQL 布局应允许 Editor 实际上限 38，不是现有 runtime 的 37。
5. 测试真实布局尺寸 180×50、100×16、99×40、55×16；断言范围端点经重新布局可以达到，隐藏 split 无 bounds。保留 Ratatui 在低高度下的现有行为，不擅自改最小尺寸。

### 2.4 Runtime 接入及端到端组件验收

1. 提取小型纯请求 resolver（输入 app/area/direction，输出内部动作所需 metrics/size、Boundary 或 Blocked），不用真实 TerminalSession 才能测试。
2. 保留 Explorer 焦点优先于 Redis 子焦点的判断。显式检查 overlay、Omni、TooSmall、源 pane 有效性。
3. Internal 继续先 PaneLayoutChanged 后 SetPaneSize；Boundary 仅调用一次既有 Kitty adapter；Blocked 不操作。不要将 I/O 移到 App::update。
4. 建立 resolver→App actions→重算布局的组件测试：Explorer 50→53→50，Editor 20→23→20；断言实际矩形、focus、文本和其他尺寸偏好。
5. 覆盖连续按键未渲染、终端缩小导致偏好超界、标准/窄屏/最大化、Relation、Redis；对内外分流使用最小请求枚举或注入 callback seam 验证 Internal 外部调用 0 次、Boundary 1 次、Blocked 0 次。
6. 执行 `cargo test --lib smart_resize` 和 `cargo test --lib ui::layout::tests`；新增 resolver 用例统一带 smart_resize 名称，避免过滤遗漏。

**单元验收：** 用户两组增大/缩小需求均能在同一焦点下往返；实际布局与 bounds 一致，单次输入不会内部和外部同时调整。记录验证并继续单元二。

## 3. 单元二：按键链路、帮助与兼容回归

**修改文件：** `src/input/keymap.rs`、`src/help.rs`、`docs/keybindings.md`、`docs/kitty-integration.md`。

1. 配置四条与用户文件相同的 Cmd+Ctrl+Shift+h/j/k/l，在 Explorer/Editor 上验证正确 Action；覆盖 Press/Repeat/Release、小写/大写 Shift 字母、Insert、pending、overlay/Omni。
2. 核实当前 Crossterm KeyEvent 匹配行为。既有 KeyBindings::matches 是整个事件相等，不能因为分支允许 Repeat 就假定 Repeat 已能匹配。若需要查询第三方库 API，使用 Context7 获取当前版本资料。
3. 测试若揭示输入问题，仅规范化 smart-resize 用于匹配的事件：依据原事件决定是否允许执行，再以配置匹配表示处理 kind/state/Shift 字母；不可全局小写化普通字符。保留原输入事件供其他编辑处理。
4. 命中后 clear_pending，测试后续普通输入不被旧序列吞掉；验证没有把 h/j/k/l 插入 SQL 文本。
5. `src/help.rs` 将四条描述从“某方向边界”改成“向某方向移动面板分隔线”，保留命令 ID 和动态配置展示。
6. 更新两份指南，说明 Explorer h/l、Editor k/j 的双向操作，Results 的尺寸随同一分隔线反向变化，Redis 主分隔线选择，触底/隐藏时回退。默认绑定、配置 schema 无变化，不增加新选项。
7. 定向验证：
   - `cargo test --lib smart_resize`
   - `cargo test --lib set_pane_size`
   - `cargo test --test mouse pane_resize`
   - `cargo test --lib help::tests`
   若过滤结果为 0，依据实际测试名修正命令；不得计为有效覆盖。仅在对应代码变化后重跑已通过组。

**单元验收：** 四键及 Repeat 在编辑模式下均走正确分隔线操作；焦点与文本不变，帮助和文档不再承诺旧单侧语义。

## 4. 单元三：全量验证、Luna 审查及集成

### 强制项目门禁

功能齐备后在任务 worktree 记录 commit/diff 和环境，执行：

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

失败由 Luna 自行定位修复；根据相关代码或环境变化重跑，不逐轮无理由重复所有门禁。数据库测试受环境变量控制的跳过应如实记录，不能声称真实数据库验证通过。

### 补充验证

- 外部 helper 未改时可补跑 `python3 -B -m unittest discover -s contrib/kitty -p 'test_*.py'`；若后续改动 helper 则相关测试成为必要验证。
- 真实 Kitty/PTY 可用时，明确构建二进制、settings 来源及窗口状态，分别验证 Explorer l→h、Editor j→k 的实际尺寸往返及触底回退。不得把读取配置文件当成配置已加载证据。
- 真实 Kitty/PTY 是补充检查，不作为让用户手工实施的前置条件。环境受限最多一次有针对性的修复重试；随后记录限制，由 Luna 收尾审查决定是否补证据，不无限 progress。

### 审查与交付

1. Luna 检查完整行为矩阵、实际布局端点、内外互斥、键输入隔离和本任务 diff；必要纠偏由 Luna 完成。
2. 仓库外配置预计无需变更。若确需部署二进制或重载，先查明当前 LazyDB 启动路径，不猜测安装位置，记录实际操作与环境。
3. 按完整可验收单元显式暂存任务文件，不 `git add .`。建议提交主题 `fix: make smart pane resizing bidirectional`；最终分支/提交命名由 Luna 决定。
4. 集成 main 前重新核对目标分支及其他工作状态，保留原工作区本地修改；对重叠冲突作语义合并。集成若更改相关代码，追加必要验证，不用旧版本测试冒充合并后证据。
5. 所有结果追加同目录 validation.md；回执仅写工作流该阶段新指定的路径与 token。

## 5. 下一步

计划已准备完成，无产品决策或外部输入阻塞。下一实施动作：Luna 在指定起点的隔离任务工作区，添加 Explorer Left 与 Editor Up 的失败回归，开始单元一。用户已选择自动工作流，不询问执行方式。当前阶段不实施代码、不创建任务分支；完成文件校验后写本次专属 plan 回执。
