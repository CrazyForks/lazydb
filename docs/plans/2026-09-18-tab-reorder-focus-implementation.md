# Tab Reorder and Focus Scope Implementation Plan

> **执行者：Luna。** 按本计划逐项实施、验证并收尾；Astra 仅负责分析与计划。本任务不启动子 Agent。当前仅完成 plan 阶段。

**Goal:** 增加可配置的 Ctrl+Shift+n/p 相邻 Tab 交换，并将全部 Tab 切换和移动快捷键限制在右侧工作区。

**Architecture:** 输入层统一检查右侧焦点，保留模态上下文的优先级。模型层新增独立移动动作，用原地交换与活动索引更新保持活动 UUID 和内部状态，复用现有持久化顺序与帮助目录。

**Tech Stack:** Rust 2024、crossterm 0.29、ratatui 0.30、TOML 配置、Cargo 测试。

---

## 基线、范围与执行规则

- 原工作空间 `/Users/yelog/workspace/tui/lazydb`；目标分支 `main`。
- 分析基线 `16b05f4e3d1d7b78d126891d2b751a0440fc7e07`；计划开始时 `git status --short --branch` 无工作区变更。
- 分析见 `.git/opencode-tasks/ses_f4cb6ddf4ffe5myJhhlMir8sw9/analysis.md`。本轮读取 checkpoint 仍不存在，不创建插件状态文件。
- 建议任务名称：**Tab 排序与焦点作用域**；建议分支：`feat/tab-reorder-focus-scope`，交由工作流自动命名/创建，不在计划阶段切换分支。
- 用户指定的阶段协议优先于通用技能流程：不执行实现、不发起子 Agent、不要求用户重复 resume，也不自行生成未知 token 的阶段回执。
- 后续记录实际命令、退出码、代码版本及环境到任务 `validation.md`。本文所有测试结果均为预期，不代表已执行。
- 各步骤完成后继续推进所在验收单元；仅外部权限、输入缺失或必须用户裁决的互斥需求才是阻塞。

### 行为合同

| 操作 | Editor/Results 且存在活动 Tab | Explorer |
| --- | --- | --- |
| Ctrl+n / Ctrl+p | 下一/上一 Tab，保持原有循环规则 | 不触发 Tab 切换 |
| Ctrl+Shift+n / Ctrl+Shift+p | 与右/左邻交换，继续选中原 Tab | 不触发移动 |
| gt / gT / ]t / [t / Ctrl+PageDown / Ctrl+PageUp | 保留原有上下文中的切换 | 不触发 Tab 切换 |

移动到边界时无操作，不循环；空/单 Tab 无操作。四种 Tab 类型一视同仁。保持关闭操作、鼠标激活、程序化 ActivateTab 的原有行为。补全和模态输入优先级保持。

### 验证分级与门禁来源

1. **用户需求验收（必需）**：两个默认移动快捷键方向正确；交换后仍选中原 Tab；切换和移动仅在右侧焦点生效，Explorer 不触发。本文的 keymap 到 App 状态测试提供自动化验收证据。
2. **项目既有质量门禁**：`.github/workflows/ci.yml` 的 Rust job 要求 Rust 1.94.0 下 fmt、all-targets/all-features clippy（warnings 为错误）及测试通过。按 Task 6 运行并记录。其他既有 CI job 的要求由项目 CI 保持，本地没有对应环境不等于通过或取消门禁。
3. **实现正确性回归（本计划的自动化覆盖）**：边界、活动 UUID/状态保持、混合 Tab、配置覆盖/禁用、帮助一致性及持久化往返。这些用于证明实现没有引入回归，不应表述成用户额外提出的产品要求。
4. **补充建议验证（非强制门禁）**：真实终端人工/PTY 按键编码检查。无环境时记录限制，由 Luna 收尾审查决定是否补充证据；最多一次有针对性的修复重试，不因它无限延续 progress。

每个单元的复核由 Luna 对该单元实际 diff、行为测试结果与上述验收标准完成。最终审查也由 Luna 执行，不切回 Astra，不增加人工批准流程。

## 单元一：可配置的右侧移动闭环

### Task 1：定义状态动作和边界

**Files:**
- Modify: `src/action.rs`（NextTab/PreviousTab 相邻位置）。
- Modify: `src/app.rs`（update 的 Tab 动作分支）。
- Test: `tests/workspace_tabs.rs`。

1. 在现有 workspace_tabs fixture 模式下添加测试 `move_tab_right_preserves_active_identity_and_state`、`move_tab_left_preserves_active_identity_and_state`、`move_tab_at_edges_is_noop`。构造三个以上 Tab，使用 UUID 数组断言交换顺序；从中间向两侧移动；断言 active_tab 指向原 UUID，Focus::Results 不变，选定 Tab 的网格或查询焦点状态不变，并且 update 返回空 Command 数组。
2. 边界测试覆盖空列表、单 Tab、首位向左、末位向右；不要通过重新构造活动 Tab 来模拟交换。
3. 新增两个 action 枚举项后运行测试，确认缺少实现导致测试失败；如果枚举扩展触发穷尽匹配编译错误，先补齐编译所需分支，再执行行为红灯，不把编译错误当作行为验证。
4. 新动作只执行检查、swap 和索引更新。可使用以下共享方法（命名可适配现有风格）：

```rust
fn move_active_tab(&mut self, right: bool) {
    let current = self.active_tab;
    if current >= self.tabs.len() {
        return;
    }
    let neighbor = if right {
        current.checked_add(1)
    } else {
        current.checked_sub(1)
    };
    let Some(neighbor) = neighbor.filter(|index| *index < self.tabs.len()) else {
        return;
    };
    self.tabs.swap(current, neighbor);
    self.active_tab = neighbor;
}
```

动作分支分别调用 `self.move_active_tab(true)` 或 `false` 并返回 `Vec::new()`。不调用 record_active_location、clear_active_data_query_focus、normalize_focus_after_tab_switch 或 prepare_active_tab。
5. 运行 `cargo +1.94.0 test --test workspace_tabs move_tab`，预期新增测试全部通过。

**验收：** 模型能够交换真实相邻 Tab，活动身份与关键状态保留，无数据库/连接命令副作用。

### Task 2：配置与键盘路由

**Files:**
- Modify/Test: `src/config.rs`（SUPPORTED_COMMANDS 与配置测试）。
- Modify: `config/default.toml`。
- Modify/Test: `src/input/keymap.rs`。

1. 注册 `move-tab-right`、`move-tab-left`，加入默认值：

```toml
# Swap the active tab with its right/left neighbor while a right-hand pane is focused.
move-tab-right = ["Ctrl-Shift-n"]
move-tab-left = ["Ctrl-Shift-p"]
```

2. 配置测试确认默认值可解析、覆盖绑定可用、空数组可禁用。沿用现有配置加载 fixture，避免另建解析体系。
3. 添加 keymap 测试，逐一覆盖 Editor、Results、Explorer，Ctrl+Shift 的 n/N/p/P 事件和重绑定后的按键。最终输出必须精确对应移动动作；Explorer 中不能有切换/移动动作。
4. 提取作用域函数：

```rust
fn tab_shortcuts_have_focus(app: &App) -> bool {
    matches!(app.focus, Focus::Editor | Focus::Results)
        && app.tabs.get(app.active_tab).is_some()
}
```

5. 在现有 next-tab/previous-tab 附近接入移动绑定，仍位于已有 overlay、Omni、查询输入和补全处理之后。仅针对 Tab 快捷键匹配增加 Ctrl+Shift 字母大小写等价处理：先匹配原始 event，若 modifiers 恰为 CONTROL|SHIFT 且 code 为 ASCII 字母，再尝试改变字母大小写的副本。不能删除 Shift，不能改写下游编辑器收到的原始 event。原本配置禁用后不得退回硬编码快捷键。
6. 加一项端到端测试：Keymap 得到动作后调用 App::update，断言完整顺序及原活动 UUID。测试中分别使用两种移动方向，验证不是仅映射通过。
7. 执行 `cargo +1.94.0 test --lib input::keymap::tests` 和 `cargo +1.94.0 test --lib config::tests`，预期通过；失败时按具体失败测试缩小重跑范围。

**验收：** 默认/自定义移动从按键到状态更新完成闭环，Explorer 无移动，Shift 与普通切换不混淆。

## 单元二：统一全部切换入口的焦点限制

### Task 3：收紧直接绑定、序列与 Ctrl+Page

**Files:**
- Modify/Test: `src/input/keymap.rs`（map、map_pending、必要的 configured_command_action）。
- Inspect: `src/editor/mod.rs` 的 EditorEffect::NextTab/PreviousTab，`src/app.rs` 的 effect 转换。

1. 将现有 `control_tab_shortcuts_and_close_other_tabs_map_globally` 拆分/更新，分别验证切换的右侧作用域与关闭行为原状。
2. 添加 Explorer/Editor/Results 的 Ctrl+n/p 矩阵；Explorer 包括普通状态与搜索状态，断言最终输出不含 Tab 动作，并在可应用动作时确认 active_tab 和 focus 没有因 Tab 操作改变。
3. 添加 Explorer 下 gt、gT、]t、[t、Ctrl+PageDown/PageUp 的回归用例；右侧测试放在原本支持这些序列的模式下，编辑器普通模式与结果视图分别验证。不要假设 Insert 模式也应识别普通字符序列。
4. 直接绑定、map_pending 中的 Tab 切换分支及 Ctrl+Page 分支统一使用 Task 2 的焦点判断。保留 Explorer 的 gg、gm 等不同动作；不对整个 g/[ /] 输入分发做粗粒度禁用。
5. 检查 configured_command_action、编辑器 effect、帮助动作等入口。若配置前缀支持新 Tab 命令，则使用同一作用域函数；不借此新增任意长度快捷键序列框架。App::update 的 NextTab/PreviousTab 保持程序化语义，不为键盘限制引入全局动作禁令。
6. 回归补全显示时 Ctrl+n/p 仍选择候选；overlay/Omni 活跃时不移动底层 Tab；保留已有 Release 忽略行为。用配置的其他大小写敏感序列验证没有受移动匹配逻辑影响。
7. 执行 `cargo +1.94.0 test --lib input::keymap::tests`。如果改动确实涉及 editor effect 路由，再运行对应 editor 测试；不要仅因读过 editor 文件就扩大测试范围。

**验收：** Explorer 中所有现有 Tab 切换快捷键均不可达，右侧保留原有循环和上下文处理。

**逻辑提交点：** 单元一、二均通过且帮助穷尽匹配已可编译后，形成完整功能提交。仅在工作流进入允许提交阶段后使用 @git-commit 技能，建议信息 `feat(tabs): add focus-scoped tab reordering`；不要为了步骤编号提交不能编译的中间状态。

## 单元三：帮助、持久化与收尾验证

### Task 4：帮助和用户文档一致性

**Files:**
- Modify/Test: `src/help.rs`（HelpShortcutId、SHORTCUT_CATALOG、配置映射、排序、一致性测试）。
- Modify: `src/app.rs`（帮助项到 Action 的映射）。
- Modify: `docs/configuration.md`。
- Modify as needed: `config/default.toml` 的切换作用域注释。

1. 移除 PreviousTab、NextTab 及对应 Alias 的 Explorer 上下文，保留其他上下文。
2. 添加 MoveTabRight/MoveTabLeft 的帮助 ID、条目和 command 映射；右侧上下文含编辑器、SQL 结果/输出、Relation、Dashboard、Redis 的适当模式。模态状态按既有帮助规则筛选，不显示事实上被更高优先级输入接管的动作。
3. 同步 app 中帮助条目的 action 映射。新增枚举引出的其他穷尽匹配按语义补齐；不得将移动误分类为删除/console 管理动作。
4. 添加帮助测试：Explorer 不显示切换/移动；右侧显示移动，并呈现自定义绑定；保留 prefix catalog 与 keymap 一致性测试。
5. 配置文档命令清单添加新命令，写明默认快捷键、右侧作用域、相邻交换保持选中及首尾不循环。
6. 执行 `cargo +1.94.0 test --lib help::tests`。文档使用既有 Markdown 风格，无需引入网站构建或设计改造。

### Task 5：混合 Tab 与持久化往返

**Files:**
- Test: `tests/workspace_tabs.rs`。
- Test as necessary: `tests/redis_browser_tabs.rs`（仅在已有 fixture 更适合 Redis 身份检查时）。
- Inspect/Modify only if a regression is proven: `src/app.rs` 的 workspace_snapshot、persisted_workspace_from_parts、restore_workspace。

1. 在混合类型数组中执行左右移动，断言按 UUID 的完整顺序；使用现有构造方式覆盖 SQL、Relation、Dashboard、Redis，不能把只测两个 SQL Tab 当作混合类型覆盖。
2. 使用有效 profile/持久化 fixture 执行交换、snapshot、restore；断言快照顺序及恢复后顺序、active UUID。包括跨 profile 投影路径可用的现有 fixture，以发现旧缓存投影覆盖排序的情况。
3. 断言原 Tab 的编辑内容或结果网格状态保留；对查询子焦点有实际状态的 fixture 明确检查移动不会清空它。避免通过复制完整实现计算期望值，直接声明预期 UUID 顺序。
4. 若往返失败，先定位当前 tabs 与 profile/global 投影谁是权威来源，再做必要的同步修复；不要提前引入新字段或格式版本。
5. 执行 `cargo +1.94.0 test --test workspace_tabs`。如确有 Redis 文件修改，执行 `cargo +1.94.0 test --test redis_browser_tabs`。

**验收：** 帮助与实际可用范围一致，移动顺序可恢复，内部状态没有被当作切换重置。

### Task 6：全量质量检查与 Luna 审查

1. 确認定向测试均有实际记录，审查 diff 中只有本需求内容；必要时更新计划的简短进度，不覆盖历史验证记录。
2. 功能齐备后按 CI Rust job 一次执行：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
git diff --check
```

3. 预期以上退出码均为 0；若失败，修复实际问题后重跑相关失败项。仅相关代码/环境有变化才重复已经通过的检查；不额外重复 cargo check。
4. 若全量测试有外部服务条件或工具链环境缺失，记录准确失败/跳过情况，不把无服务的测试当作数据库集成通过；按项目现有 CI 要求保留后续 CI 验证。
5. 可选真实终端检查：支持增强键盘协议的终端中用 Ctrl+Shift+n/p 交换，再进入 Explorer 确认不会触发。它是补充证据而非用户新增的强制门槛。PTY/人工环境失败最多进行一次有针对性的修复重试，随后由 Luna 审查记录限制，不无限 progress。
6. Luna 收尾审查重点：所有键盘入口的作用域、Shift 大小写且不影响 gT、活动 UUID/焦点状态不变、混合 Tab/持久化往返、帮助与自定义配置一致、无关闭/鼠标语义变化。
7. 按当前工作流授权完成最终提交/合并阶段，使用 @git-commit；不得由 Astra 接手审查或提前在 plan 阶段提交业务实现。

## 完成定义

- Ctrl+Shift+n/p 默认可用且可重绑定/禁用。
- 右侧交换相邻 Tab，活动身份和内部状态保持，边界无操作。
- Explorer 的所有切换/移动快捷键入口均无 Tab 操作。
- 现有循环切换、模态上下文及关闭/鼠标行为无回归。
- 帮助、文档、配置同步，持久化往返有测试证据。
- 定向与项目全量质量检查有当前代码版本的记录，环境限制明确，由 Luna 完成收尾。
