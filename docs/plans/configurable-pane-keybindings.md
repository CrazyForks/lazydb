# Configurable Pane Keybindings Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 工作流覆盖：本任务由 Luna 实施、审查、纠偏和提交合并；不启动子 Agent，不询问执行方式。上面的技能模板不要求切换模型，也不允许调用不存在的技能。Astra 本阶段仅编写计划。

**Goal:** 使 `focus-pane-left = ["Cmd+Ctrl+h"]` 从配置加载到 SQL/Redis 等工作区操作真实生效，并完整支持六个面板命令的组合键、序列、覆盖/禁用及一致的帮助。

**Architecture:** 保留 Rust 单体、Action/reducer 和现有编辑器边界，提取轻量按键规范化及面板命令元数据；在应用输入层统一六个面板命令的匹配和窗口前缀所有权。实际配置序列驱动 pending 和候选；Help 复用面板语义解析，不重写整套 Vim、全局快捷键或终端协议。

**Tech Stack:** Rust 2024 / Rust 1.94.0 CI，Crossterm 0.29，Ratatui 0.30，TOML/Serde，现有 modalkit 编辑器；不新增外部依赖。

---

## 0. 执行契约与基线

- 分析来源：本目录 `analysis.md`。起点 `23a638fbc953adb5f956488693ab2d4825594581`，目标 main；计划复核时 HEAD 与起点相同、工作区无未提交修改，main ahead origin/main 5。
- 没有依赖未提交文件；后续 worktree 从指定 SHA 创建，不从 origin/main 创建。分支在计划完成后由工作流命名，本阶段不创建。
- checkpoint.json 仍不存在；不要创建它或修改 state.json。实施恢复时优先读取插件实际 checkpoint 和 diff，不重读所有历史，不把历史 next 视为新需求。
- 所有任务报告写本目录；本阶段不生成额外仓库文档。计划与 change-scope 不属于业务提交。
- 后续步骤中的 git add/commit 只适用于 Luna 实施阶段的任务 worktree，且服从该阶段工作流权限。按闭环提交；不在原 main 中提交、不全量 stage 无关修改。
- 新增/修改范围见 change-scope.json。若实际发现额外必要文件，先补范围记录和理由再实施；不借本任务改数据库驱动或引入框架。
- 计划采用三个业务单元，内部步骤按单一动作拆分。一个步骤完成后继续同单元，单元通过后继续下一单元；不等待人工 resume。

## 1. 确定的行为契约

### 1.1 配置语法

- `Cmd+Ctrl+h`、`Cmd-Ctrl-h`、`Super+Control+h` 等价，修饰键顺序任意，修饰键及命名主键不区分大小写。
- Cmd/Command/Super → SUPER；Ctrl/Control → CONTROL；Alt/Option → ALT；Shift/Meta/Hyper 各自独立，不混同。
- 空白分隔连续按键；一个 chord 内不允许修饰符两侧空白。沿用 F1–F12；增加 Left/Right/Up/Down、Delete/Del、Insert/Ins、BackTab、Plus/Minus。
- 字符大小写语义和 Shift-Tab 规范化保持原契约。裸 `+`/`-` 是字符；修饰后的这两个字符用 Plus/Minus，避免将分隔符替换当解析。
- 明确选择：混合 `Ctrl+Shift-h` 分隔写法拒绝并提示统一使用 + 或 -；重复修饰键（如 Ctrl+Control+h）拒绝并指明重复项。旧默认不依赖这些形式。
- 数组替换而非追加；`[]` 禁用绑定；同时保留默认须列出 `["Cmd+Ctrl+h", "Ctrl-w h"]`。
- 失败含配置文件路径（load/启动边界）、section.command、数组下标、原值、token 和原因，不伪造行号，不回退/忽略错误。

### 1.2 六命令与输入所有权

目标命令：focus-pane-left/down/up/right、toggle-pane-maximized、reset-pane-sizes。

- 应用层单一面板语义解析器区分 SQL、关系表、Dashboard、Redis Keys/Preview；返回有效 Action 或无目标 no-op。匹配 no-op 仍消费事件，不下沉为文本。
- 组合键可在 SQL Normal/Visual/Insert/Replace 工作区切换面板；普通字符或字符首键序列仅在导航模式启动。SHIFT 本身不算允许在输入模式抢键的修饰符，`Shift+h` 仍是文本 H。
- 编辑器命令行/搜索、非 Vim 文本输入、弹窗、忙碌表单保留输入所有权。Ctrl-w 在 Insert/Replace 始终是删除前词，Space 仍输入字符。
- Ctrl-w Ctrl-w、计数和窗口 +/−/</> 尺寸调整保留为窗口 preset。应用层统一协调它们与配置的共享前缀；不能存在应用和编辑器两个活动 Window pending。
- 面板六命令被替换/禁用后，旧 Ctrl-w h/j/k/l/f/= 不能从编辑器固定分支继续生效。
- Release 不触发；面板 Press 才执行/推进序列，Repeat 不二次最大化，也不重置 pending 超时。其他编辑/导航重复语义不变。

### 1.3 序列、冲突和 UI

- 缓存真实规范化 chord，使用小集合线性前缀过滤，支持单键、两键、三键以上。不要以固定前缀枚举决定可配置序列。
- Pending 绑定焦点、编辑模式、tab、generation、输入上下文；变化后失效，恢复旧上下文不复活。采用现有 sequence_timeout_ms，超时只取消，不执行短命令。
- Escape/Ctrl-c 优先取消；非法延续保持 pending 至超时/取消且不重放文字。
- 一个输入先尝试完整的配置延续；若该键不是任何有效延续，再允许 Up/Down 选择候选、Enter 执行候选，避免新支持的方向键/Enter 被 UI 导航吞掉。显式保留 Esc/Ctrl-c 为取消键；对它们作为 panes 起始键/延续的不可达配置给出诊断。
- 同命令同序列别名去重；不同命令在可重叠上下文的同序列或严格前缀歧义拒绝。相同命令的短绑定若遮蔽它自己的长绑定也拒绝，不保留不可达长别名。
- panes 与现有应用绑定及窗口 preset 的冲突使用运行时相同上下文约束；保留前缀节点不是可执行命令，不因为共享 Ctrl-w 就报冲突。确实不可达的配置（如把 pane action 绑成 Ctrl-w Ctrl-w 或其被完整 preset 抢占的扩展）报告保留快捷键冲突。
- Help/候选/底部提示显示真实生效绑定；禁用后不显示可执行快捷键。候选选择按稳定命令 ID/序列标识，不依赖静态 suffix。
- 原有终端增强协商足够；Cmd 语法成功不等于终端物理事件能送达。说明边界，不自动更改终端设置。

## 2. 计划阶段补充源码发现

`src/app.rs:2664 execute_help_shortcut` 先验证 Help 选中项、可执行性和可用性，再清理 Help overlay，然后优先经 `commands::command_for_help` 执行语义命令；还有显式 Focus/Maximize 分支。`src/commands.rs:220` 将多个方向 Help ID 映射成固定 FocusExplorer/Results/Editor。

因此分析中“Help 不能只换标签”依然正确，但实际修复不能只修改 map_shortcut。本计划将 `src/app.rs` 明确纳入范围：六个面板方向 Help 操作在 generic semantic command 之前走共享面板解析器，保留其它 Omni 语义命令不变。预计无需修改 commands.rs；它只是参考文件，不列 change-scope。

## 3. 单元 A：用户原始单键配置在 SQL 工作区生效

### Task A1：建立真实配置到焦点的失败回归

**Files**
- Modify/Test: `tests/keymap.rs`
- Modify/Test: `src/config.rs`（现有 tests）

**步骤**
1. 在 tests/keymap.rs 增加 `pane_binding_cmd_ctrl_h_moves_focus_from_sql_modes`；复用现有 App、apply_key helper，不模拟数据库连接。
2. 使用 TempDir 写入用户原始片段，通过 AppConfig::load 读取并构造 Keymap，给 App 安装同一份 bindings；测试 Results、Editor Normal、Insert、Replace 的 Cmd+Ctrl+h。
3. 每个场景对 map 返回的 Action 调 app.update 后断言 focus=Explorer、SQL 文本/模式无意外修改。按目前代码预期首先失败于配置加载，不可只测试 parse_key。
4. 在 config tests 增加 `pane_binding_aliases_load_and_preserve_other_defaults` 和 `pane_binding_diagnostics_locate_bad_modifier`；记录旧默认值仍保留，错误不是笼统 InvalidKeybinding。
5. 运行下面两条定向命令，确认非零退出由新断言/原始报错导致，而不是编译环境问题。

```sh
cargo test --test keymap pane_binding_cmd_ctrl_h_moves_focus_from_sql_modes -- --exact
cargo test --lib config::tests::pane_binding -- --nocapture
```

**核心测试模式（适用于 config tests，完整示例）**

```rust
#[test]
fn pane_binding_original_settings_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, "[keybindings.panes]\nfocus-pane-left = [\"Cmd+Ctrl+h\"]\n").unwrap();
    let config = AppConfig::load(path).unwrap();
    let bindings = config.keybindings.key_bindings().unwrap();
    assert!(bindings.matches(
        "focus-pane-left",
        KeyEvent::new(KeyCode::Char('h'), KeyModifiers::SUPER | KeyModifiers::CONTROL),
    ));
    assert!(bindings.matches("help", KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE)));
}
```

**验收**：红测试真实覆盖用户片段，不把 from_toml 当作合并入口；记录命令/退出结果到 validation.md。

### Task A2：实现兼容解析与有定位的错误

**Files**
- Create: `src/input/keybinding.rs`
- Modify: `src/input/mod.rs`
- Modify/Test: `src/config.rs`
- Modify: `src/runtime.rs`（仅加载错误路径和后续 pending UI 适配）

**步骤**
1. 新模块承载 KeyChord、解析错误、规范化方法；不依赖 App，避免输入/配置循环类型依赖。将模块以 pub(crate) 声明。
2. 将旧 parse_key 改为委托解析模块；主键、别名、分隔符和错误按 1.1 实现。保留调用者需要的 KeyEvent 视图，避免一次改掉全软件 `sequence_for` API。
3. KeyChord 只由 code/modifiers 组成；配置/事件共同规范化 Shift+ASCII 和 BackTab。kind/state 从匹配身份剥离，但不从实际事件剥离。
4. 暂保留非 panes 的现有匹配/重复策略；新 panes 路径用规范化 chord。若抽取了公共比较，显式传入事件策略并添加现有 Repeat 回归，不全局把 Repeat 变 Press。
5. 编译配置时保留 section/name/index/token 来源；InvalidKeybinding 扩展结构化 cause 并更新使用它的匹配测试。load 保留文件路径上下文；runtime 错误仍含 failed to load application settings 并补真实路径，不丢内部原因。
6. 添加带修饰 Plus/Minus、裸字符、别名、混合分隔、重复修饰符、F99、空字符串、Shift-Tab/BackTab、Meta 与 Super 区别的表驱动解析用例。
7. 运行配置定向命令；新原始片段配置测试应通过，完整焦点测试此时可仍失败，继续 A3，不将解析通过当作闭环完成。

```sh
cargo test --lib input::keybinding::tests -- --nocapture
cargo test --lib config::tests -- --nocapture
```

**复核**：没有字符串全局 replace；没有静默丢修饰符；F1–F12/大小写/默认合并不变；源码错误路径不泄漏无关配置内容。

### Task A3：共享面板动作与单键分发

**Files**
- Create: `src/input/panes.rs`
- Modify: `src/input/mod.rs`
- Modify: `src/input/keymap.rs`
- Modify: `src/editor/mod.rs`
- Modify/Test: `src/editor/tests.rs`, `tests/keymap.rs`

**步骤**
1. 在 panes.rs 定义六命令 enum、配置名映射，以及依 App 当前布局返回 Action 的纯函数。把 keymap 1270–1365 的方向语义抽取，而非重新猜 SQL/Redis 布局。
2. 明确返回值语义，避免 no-op 与未匹配混淆，例如：

```rust
pub(crate) enum PaneDispatch {
    Unmatched,
    Pending,
    Consumed,
    Action(crate::action::Action),
}
```

3. 在 keymap 中保留 overlay/prompt/忙碌输入的优先级，在编辑器 SUPER 丢弃和普通字符分发之前处理可用 panes chord；Press 才执行，匹配 Release/Repeat 不产生面板 Action。
4. 将现有默认 Ctrl-w 序列在应用层用到的六命令动作也转到共享 resolver，先保证布局语义单一。A 阶段不要宣称已完成窗口 prefix 全迁移。
5. 添加 `pane_binding_does_not_leak_at_boundary`：Explorer 上 left 命中但不插入 h、不触发其他动作。
6. 添加输入隔离测试：搜索 prompt、非 Vim 输入、普通 SHIFT 字符、Insert Ctrl-w/Space 保持原语义；编辑器 SUPER 只有被应用匹配才消费，其余沿用原逻辑。
7. 重跑原始场景和新输入隔离用例，输出期望所有指定测试通过。

```sh
cargo test --test keymap pane_binding -- --nocapture
cargo test --lib editor::tests::pane_binding -- --nocapture
```

**复核**：同一按键不会同时返回 EditorKey 和面板 Action；resolver 不直接修改 App；Results 与 Editor 的原始组合键都经 App::update 改变焦点。

### Task A4：原始绑定的 Help 展示和点击一致

**Files**
- Modify/Test: `src/help.rs`
- Modify/Test: `src/app.rs`
- Modify/Test: `tests/keymap.rs`
- Modify: `config/default.toml`, `docs/keybindings.md`

**步骤**
1. 为方向相关 HelpShortcutId 建立到六个 PaneCommand 的唯一映射（FocusExplorer→left；FocusResults→down；FocusResultsFromL/FocusEditorFromL→right；FocusEditorFromK→up），最大化/恢复同理。不要误把 FocusExplorerLeader 当作 left。
2. configured_sequence 对这些 ID 使用有效绑定；原始配置在 Help 中展示 Cmd+Ctrl+h。
3. app.execute_help_shortcut 在通用 command_for_help 之前识别面板 ID，恢复原工作区上下文后调用共享 resolver；验证当前 Help 选择/可用性仍有效，不为所有语义命令改派发。
4. 添加 `pane_binding_help_matches_keyboard`：配置后的 Help 标签、选择执行和直接按键落到相同焦点。
5. 文档加入当前原始组合键示例、保留默认双绑定示例、Cmd 终端条件。默认配置值保持不变。
6. 执行单元 A 定向检查并复核 diff；记录闭环已完成的范围，序列和解绑仍由 B 补齐。

```sh
cargo test --lib help::tests::pane_binding -- --nocapture
cargo test --test keymap pane_binding -- --nocapture
git diff --check
```

**A 验收**：原始文件片段→Keymap→App::update→正确焦点，SQL Results/Normal/Insert/Replace 自动化通过；Help 标签/操作一致；默认配置可加载且基本文本输入无回归。

**建议实施阶段提交**：仅 stage 此闭环实际修改的文件，`fix(keybindings): support configured pane chords in SQL workspaces`。不得 stage 未完成 B 的占位代码。

## 4. 单元 B：六命令、多布局、自定义序列和禁用完整闭环

### Task B1：先锁定替换/禁用和多布局失败行为

**Files**
- Modify/Test: `tests/keymap.rs`, `src/editor/tests.rs`
- Modify/Test: `src/config.rs`

**步骤**
1. 新增 `pane_binding_replaces_editor_window_defaults`：替换 left 后 Ctrl-w h 不再切面板，自定义 chord 有效；Normal/Visual 均覆盖。
2. 新增 `pane_binding_empty_array_disables_command_everywhere`：逐一禁用六个动作，Editor/Results/Explorer 不走旧动作；保留 Ctrl-w Ctrl-w 与 resize。
3. 新增 `pane_binding_layout_matrix`：沿现有 sql_window_directions、relation_window_directions、Redis 场景 fixture，覆盖 SQL/Relation/Dashboard/Redis Keys/Preview 六命令及边界 no-op。
4. 新增 `pane_binding_custom_prefix_and_three_key_sequence`：使用安全首键 `Alt+x h` 及 `Alt+x a h`（分开配置避免短前缀歧义），另测导航模式纯字符首键。
5. 运行定向测试确认旧后门/固定前缀行为产生预期红测试。

```sh
cargo test --test keymap pane_binding -- --nocapture
cargo test --lib editor::tests::pane_binding -- --nocapture
```

### Task B2：统一窗口前缀所有权与动态序列

**Files**
- Modify: `src/input/keybinding.rs`, `src/input/panes.rs`
- Modify: `src/input/keymap.rs`, `src/editor/mod.rs`
- Modify/Test: `src/editor/tests.rs`, `tests/keymap.rs`

**步骤**
1. 新 panes pending 保存已输入 chord、候选序列、上下文快照、started_at；不使用写死 `[Ctrl-w, event]` 拼序列。
2. 将六命令从编辑器 PendingBinding::Window 的 h/j/f/= 固定派发中移出；应用层在 Normal/Visual 拦截窗口前缀，编辑器 Insert/Replace Ctrl-w 文本路径保留。
3. 窗口 count 状态接入统一前缀所有者，包括编辑器先输入计数再 Ctrl-w 的路径。只转交窗口相关 count，不截断 Vim 其它 motion/operator count。为现有 count 等价输入复用既有 tests。
4. 把保留的 Ctrl-w Ctrl-w 和尺寸调整纳入该窗口前缀候选/终结处理；它们无需加入用户六命令表，但要参与冲突和候选生成。
5. 某命令替换/[] 后过滤掉旧默认，不因编辑器 fallback 或旧 pending 再激活。移除六命令重复路径；保留非窗口 Pending 的行为。
6. 规范化 state，Press 才推进 pending；实现 consumed no-op。允许共享合法前缀，完成后清空所有权，避免事件重复执行。
7. 覆盖 timeout、Escape/Ctrl-c、invalid continuation；上下文变化清空（包括进入/离开 Help/Omni、prompt、cell edit），generation 保证恢复旧焦点不恢复序列。
8. 跑 B1 测试使其绿，再跑现有窗口与序列测试。不要通过删除或弱化旧编辑器测试让重构通过。

```sh
cargo test --test keymap pane_binding -- --nocapture
cargo test --test keymap ctrl_w -- --nocapture
cargo test --test keymap window -- --nocapture
cargo test --test keymap counted -- --nocapture
cargo test --lib input::keymap::tests::sequence -- --nocapture
cargo test --lib editor::tests -- --nocapture
```

**复核**：编辑器 count 迁移是本单元最高风险；检查 Ctrl-w 删除词、普通 count/motion、撤销/重做和视觉选择仍归编辑器。上述 filter 必须输出实际测试数，零测试不算验证通过。

### Task B3：配置冲突与保留键诊断

**Files**
- Modify/Test: `src/config.rs`
- Modify: `src/input/keybinding.rs`, `src/input/panes.rs`, `src/input/keymap.rs`

**步骤**
1. 为 panes 表建立共享静态 scope 描述：导航上下文与允许的工作区编辑上下文；动态可用方向另由 resolver 处理，不把“此刻无目标”当成静态不冲突。
2. 复用描述做运行时是否可启动判断及配置校验，覆盖 panes 与已有 global/explorer/results/editor/leader 等可达绑定的交集；对未迁移组保持原有校验，不趁机全量改其语法执行能力。
3. 比较规范化序列的 exact/prefix，保留双方配置位置；同序列同命令去重，同命令严格前缀仍报不可达别名。冲突错误引用真实相撞项而不是数组首项。
4. 将完整保留窗口命令加入检查，单纯 prefix 节点不当作完整键。检测取消键冲突；保留原配置无效时拒绝启动。
5. 新增 `pane_binding_conflicts_use_overlapping_contexts`、`pane_binding_prefix_ambiguity_is_rejected`、`pane_binding_reserved_window_sequences_are_reported` 和不同上下文合法复用用例。
6. 每个负例断言命令、数组下标与具体序列；运行完整 config tests，内置 defaults 必须合法。

```sh
cargo test --lib config::tests -- --nocapture
cargo test --test keymap pane_binding -- --nocapture
```

**复核**：不通过放宽一切冲突让 defaults 通过，也不把所有键当“global”导致原默认冲突。用户写进错误分组或重复跨分组同命令的情况给清晰诊断，不能 BTreeMap 后插入悄悄覆盖。

### Task B4：动态候选、禁用和帮助执行完整一致

**Files**
- Modify: `src/help.rs`, `src/input/keymap.rs`, `src/input/panes.rs`
- Modify: `src/ui/mod.rs`, `src/runtime.rs`, `src/app.rs`
- Modify/Test: `tests/ui_render.rs`, `tests/keymap.rs`

**步骤**
1. 扩展 KeySequenceState 以携带真实已输入显示和候选（稳定命令 ID、实际余下序列）；旧静态 prefix 分支仍供未迁移前缀使用。
2. UI 读取有效 candidates，不再为配置面板前缀回查固定 Ctrl-w suffix。进入/续键/取消/失效时 runtime redraw 比较能感知候选与显示变化。
3. valid sequence continuation 优先于候选导航；仅不构成有效延续的 Up/Down/Enter 才导航或执行选项。选项执行调用共享 resolver，不向不在 Help overlay 的 App 盲发只能处理 Help 选中项的 Action。
4. configured_sequence 与 shortcut filtering 处理全部别名和 []；禁用条目隐藏，相关 footer 不回退旧键。可达性来自相同布局语义，Redis 不显示 SQL-only 固定目标。
5. app.execute_help_shortcut 的面板路径在清理 Help 后重新验证实际上下文，禁用/过期候选不执行；其余语义命令和 Omni 保持现有路线。
6. 新增 `pane_binding_custom_prefix_renders_candidates`、`pane_binding_disabled_shortcuts_are_hidden`、`pane_binding_help_executes_redis_direction`、`pane_binding_enter_and_arrow_suffixes_precede_picker_navigation`。
7. 运行新 UI/交互测试和既有 pending prefix/counted prefix 渲染回归。

```sh
cargo test --test ui_render pane_binding -- --nocapture
cargo test --test ui_render pending_prefix -- --nocapture
cargo test --test ui_render counted_pending_prefix -- --nocapture
cargo test --lib help::tests -- --nocapture
cargo test --test keymap pane_binding -- --nocapture
```

### Task B5：单元 B 集中复核与提交点

**Files**：B1–B4 的所有实际修改文件。

1. 按六命令×关键布局×输入模式查验测试矩阵；确保有 reducer 最终状态断言，不能只有 resolver 输出断言。
2. 检查 `src/editor/mod.rs` 和 `src/input/keymap.rs` 六命令没有绕过配置的硬编码后门；允许保留 count/resize preset，不要求删掉所有 Ctrl-w 文本。
3. 检查 `src/app.rs` Help 语义路径、Keymap 前缀 Enter、UI mouse click 三者均遵守禁用和实际上下文。
4. 执行相关完整 integration targets（此轮只跑一次，不再把之前所有过滤命令重放）。

```sh
cargo test --test keymap --test ui_render --all-features
git diff --check
```

**B 验收**：六命令单键/序列/多别名/禁用可用，多布局正确；旧窗口与编辑器契约保留；生命周期及冲突诊断正确；键盘和帮助/候选交互一致。

**建议实施阶段提交**：`fix(keybindings): unify pane sequences and contextual shortcut hints`，精确 stage 本单元文件。

## 5. 单元 C：文档、强制验证与 Luna 收尾

### Task C1：文档与契约最终同步

**Files**
- Modify: `config/default.toml`, `docs/keybindings.md`
- Modify/Test: `src/config.rs`（仅如增加文档/默认一致性断言需要）

**步骤**
1. 在 defaults 的 keybindings 注释中说明 + 和 -、序列空格、Cmd/Super、覆盖/禁用；原默认值、preset 和 timeout 保持原值。
2. 文档写原始完整四方向示例、保留默认双绑定、禁用例、自定义前缀、冲突和保留键诊断、编辑模式边界、终端 Cmd 条件。
3. 明确任意序列支持的范围是本次六个 panes 命令；不宣称全局/Leader/Vim/overlay 已完全任意重映射。
4. 文档示例用配置加载测试覆盖，必要时复制原字符串做表驱动测试，防止文档给不可用键。
5. 定向运行 defaults/config tests；若文档/注释修改不影响代码，不重复 editor/UI suite。

```sh
cargo test --lib config::tests -- --nocapture
git diff --check
```

### Task C2：用户验收、项目强制门禁、补充验证分层

**用户需求/本方案实现验收（必需自动化）**
- 原始配置可加载且端到端切换；六命令配置完整；默认与输入契约兼容；准确错误与帮助。
- A/B 上述测试及最终全量中实际执行，不允许零测试结果代替通过。

**项目强制门禁（CONTRIBUTING.md 与 CI）**

在功能与文档齐备后记录工具链，再执行以下命令。CI 基线 1.94.0；本机如有该工具链可将 cargo 换 `cargo +1.94.0`，所有结果记录实际命令。

```sh
rustc --version
cargo --version
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
git diff --check
```

预期所有门禁退出 0；记录测试 passed/ignored/服务门控实际情况。无需在同轮额外叠加 cargo check。若 clippy/测试代码失败，由 Luna 修复并重跑受影响检查；不得通过改成 --no-default-features 冒充全量通过。若出现项目强制门禁环境失败，明确标记未通过并交由该工作流收尾规则处理，而不是宣称完成验证。

**补充建议（非新增强制门禁）**
- 真终端手按 Cmd+Ctrl+h、默认 Ctrl-w h、协议恢复；仅在有可用交互终端时做。
- Linux/macOS 物理键一致性、tmux/终端组合兼容，可由现有 CI 或额外环境补充。
- 未配置数据库服务的外部集成测试不为此任务新增搭建环境硬门槛；记录其门控状态，不冒充执行过。
- 补充 PTY/人工检查受限时最多一次针对性修复重试，然后记录限制，由 Luna 收尾审查决定是否需补证。不将不存在 TTY 当成阻止自动化业务验收的无限循环。

### Task C3：Luna 复核、最终证据与提交合并交接

**步骤**
1. 复核完整 diff 与 change-scope，确认没有无关文件、未提交本地依赖或自动生成文件混入。
2. 对照风险复核：输入所有权、count/Visual 兼容、别名规范化、Repeat/state、冲突交集、disabled fallback、Redis Help 方向、候选失效、实际文件错误定位。
3. 若纠偏改动代码，按影响重跑定向检查，必要时更新全量结果；不对同一未变化代码反复执行全套 fmt+clippy+test。
4. validation.md 逐条记录：命令、退出值、相关文件、HEAD/工作区差异、工具链/平台、服务或 TTY 限制。旧版本结果保留为历史，不标成当前 diff 的通过结果。
5. 通过后按工作流在任务分支精确 stage/commit；最终文档提交可用 `docs(keybindings): document configurable pane shortcuts`，也可与最后功能提交整合。只提交业务、测试、用户文档，不提交 .git 任务报告。
6. 后续提交合并阶段由 Luna 执行；不在 plan 阶段承诺 main 已更新或测试已过。

**最终完成定义**：A+B+C 必需自动化/强制门禁有真实结果，功能闭环全部完成，补充检查限制如实记录；不要求用户选择执行方式或代替实施。

## 6. 文件清单与依赖

新增：`src/input/keybinding.rs`、`src/input/panes.rs`。

修改：`src/input/mod.rs`、`src/config.rs`、`src/input/keymap.rs`、`src/editor/mod.rs`、`src/editor/tests.rs`、`src/help.rs`、`src/app.rs`、`src/ui/mod.rs`、`src/runtime.rs`、`tests/keymap.rs`、`tests/ui_render.rs`、`config/default.toml`、`docs/keybindings.md`。

没有预计删除或重命名。没有未提交依赖文件。Cargo.toml/Cargo.lock、commands.rs、terminal.rs、CONTRIBUTING.md、架构文档及 CI 为参考，不预计修改。若实现确需额外文件，不用最小任务范围掩盖实际变动，应先修订 change-scope。

## 7. 计划阶段完成记录

- 已读取 analysis.md，已调用 writing-plans；用户指定目录优先于技能默认 docs/plans 存放位置。
- 当前阶段不运行红绿测试、不修改业务、不创建 worktree、不 stage/commit。文中测试名是明确的实施目标，计划阶段尚未创建这些测试；预期结果不等同已执行结果。
- 先写 change-scope.json 并验证与本节文件列表一致，再写指定 plan token completed 回执。
- 下一实施动作：Luna 在任务工作树执行 A1，建立原始配置经 AppConfig::load → Keymap → App::update 的失败回归，然后连续完成 A2–A4，不停在 parser-only 修复。
