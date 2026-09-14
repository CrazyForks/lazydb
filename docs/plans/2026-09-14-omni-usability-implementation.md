# Omni 可发现性、可读性与视觉层级 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行环境若没有该技能，则按本文任务顺序实施，并逐项记录验证结果。

**Goal:** 让用户能从帮助面板发现并打开 Omni，并获得配色清晰、类型准确、信息分层和窄屏操作可靠的全局搜索面板。

**Architecture:** 沿用 Help 快捷键目录、`Action::OpenOmni`、`OmniState`、`Theme` 和 `IconSet`。业务模型增加类型元数据，UI 用语义样式和独立 Span 渲染结果；布局、可见窗口和鼠标命中共用同一套几何计算。继续使用现有搜索、导航与异步结果校验路径。

**Tech Stack:** Rust 2024 / MSRV 1.94，Ratatui 0.30.2，Crossterm 0.29，unicode-width，nerd-font-symbols，现有 Rust 单元测试和 TestBackend 渲染测试。

---

## 1. 已确认的代码事实

以下使用符号定位，行号可能随其他工作变化。

| 问题 | 位置 | 事实 |
| --- | --- | --- |
| 帮助无 Omni | `src/help.rs`：`HelpShortcutId`、`SHORTCUT_CATALOG`、`configured_sequence` | 缺少 Omni 条目与配置键映射 |
| 键盘入口已存在 | `src/input/keymap.rs`：全局映射和 `map_omni` | 默认 F2，打开后同一绑定关闭，支持 Esc / Ctrl-C |
| 选中行低对比度 | `src/ui/omni.rs`：`render` | `fg(theme.background)` 配 `bg(theme.selection)`，默认主题两者均为深色 |
| 无信息层级 | 同上 | 标题、`[open]`、subtitle 拼成字符串，整行统一样式 |
| 长路径挤掉标题 | 同上 | 先扣除完整 subtitle 宽度，标题可用宽度可能归零 |
| 类型不准确 | `src/app.rs`：`refresh_omni_items`、`apply_omni_search_page` | 关系结果的 category 统一写为 Table |
| 状态覆盖结果 | `src/ui/omni.rs`：`render` | 结果区域与最后一行 status 重叠 |
| 编辑光标不准确 | 同上 | 用查询长度而非 `TextInput::cursor()` 定位 |
| 选中项不跟随滚动 | `src/model/omni.rs`：`move_selection`；`src/app.rs`：`Action::OmniMove` | 只更新 selected；渲染依赖 scroll，未确保 selected 可见 |

当前没有收到用户截图。可以确认选中态错误；若普通未选中文字仍发黑，实际验收时检查使用中的主题及最终 Buffer 样式。

## 2. 实施边界和设计决定

- 首轮交付任务 1–7；任务 8 搜索高亮为独立后续增强。
- 保留单行结果，布局为「选中标记 / 图标 / 标题 / 次级上下文 / 短标签」。
- 默认面板宽高上限沿用当前实现；先修正内部空间分配，再通过实际终端效果决定是否微调。
- 所有文案沿用现有英文 UI 风格，产品名称统一为 Omni。
- 颜色优先复用现有 Theme 字段，图标遵循 NerdFont / Unicode / Ascii 配置。
- 不新增第三方依赖。使用现有终端文本净化和显示宽度工具。
- 按相关行为添加回归测试；纯图标字形替换、文案和样式常量不逐条编写镜像测试。
- 开始实施前执行 `git status --short` 并检查当前差异。`src/app.rs` 等共享文件按符号做局部修改，保留其他工作。

## 3. 目标交互与样式契约

```text
╭─ OMNI ───────────────────────────────────────────────────╮
│ ⌕ users                                                  │
│ All · > Commands · @ Connections                  8 items │
│ ▸ ▦ users                  prod / app / public     Table  │
│   ◫ active_users           prod / app / public      View  │
│   › users-analysis         staging                 Open  │
│   ⌘ Refresh Catalog                              Command  │
│                                                          │
│ ↑↓ Select   Enter Open   Tab Actions   Esc Close          │
╰──────────────────────────────────────────────────────────╯
```

图中字符为视觉示意，最终使用 IconSet。实际过滤类型、连接作用域和结果数量由 OmniState 提供。

| 区域 | 样式/规则 |
| --- | --- |
| 面板 | `surface_raised` 背景、accent 边框、明确 text 前景 |
| 普通标题 | text |
| 选中标题 | text + Bold，selection 背景 |
| 次级上下文 | muted；选中背景上不足够清楚时由局部样式助手生成更接近 text 的次级色 |
| 选中标记 | accent，固定宽度；无颜色时仍有可见字符 |
| 图标 | accent/action，避免整行使用数据库品牌固定色 |
| 已打开 | success 短标签；从标题字符串中分离 |
| 不可用命令 | 标题仍可读，选中后状态行展示原因；不要只通过暗色表达不可用 |
| 底部按键 | 复用 `shortcut_hints` 的按键/说明分层和窄屏打包 |

选中行的次级色先验证现有 muted；如果需要增强，仅对 RGB 主题在局部样式助手中向 text 混合，Reset/Indexed 保留主题回退。不要为了所有颜色都能自动算对比度而引入大型色彩框架。自定义主题无法仅靠结构测试保证对比度，需做代表性样例验收。

## Task 1：补齐帮助入口与主界面提示（P0）

**Files:**
- Modify: `src/help.rs`：`HelpShortcutId`、`SHORTCUT_CATALOG`、`configured_sequence`、`footer_rank`。
- Modify: `src/app.rs`：`execute_help_shortcut`。
- Test: `src/help.rs`、`src/app.rs` 的现有测试模块；`tests/ui_render.rs`。

**Step 1 — 增加行为回归测试。**
在帮助目录测试中覆盖全部帮助启用上下文，断言搜索 `omni` 能找到可执行入口；默认键为 F2，自定义绑定时显示实际键。沿用当前配置测试的构造方式，不直接构造 KeyBindings 私有字段。

**Step 2 — 验证测试能捕获遗漏。**
运行 `cargo test --lib omni`。新测试应因条目不存在而失败；新增枚举前若编译失败，补完类型声明后再确认行为失败。

**Step 3 — 注册入口。**
新增 `HelpShortcutId::OpenOmni`，描述为 `Open Omni — search commands, connections and objects`。上下文覆盖依据当前帮助目录和 `ALL_SHORTCUT_CONTEXTS` 审核，不只复制旧的主面板上下文列表。配置映射新增：

```rust
HelpShortcutId::OpenOmni => Some("omni"),
```

**Step 4 — 接通帮助执行。**
在 `execute_help_shortcut` 的动作映射中新增：

```rust
Id::OpenOmni => vec![Action::OpenOmni],
```

使用现有选中项、可执行性、上下文校验及关闭 Help 的流程。不为此额外新增 CommandId，避免把“打开 Omni”再加入 Omni 命令结果。

**Step 5 — 主界面 footer 注册高优先级提示。**
在适用上下文为 OpenOmni 分配靠前优先级，描述使用简短 Omni。显示键由 configured_sequence 获取。窄屏下保证核心执行操作优先，同时在常见 80 列终端中能发现 Omni；更新受顺序影响的现有 footer 断言。

**Step 6 — 验证打开链路。**
增加“打开 Help → 搜索 Omni → Enter → Omni 出现”的行为测试，确认关闭 Omni 后回到正确主界面。运行 `cargo test --lib help`、`cargo test --test ui_render help`、`cargo test --test omni_input`。

**验收：** Help 可发现、可搜索、可执行；自定义快捷键正确显示。

**建议提交：** `fix(help): expose the configured Omni shortcut`

## Task 2：修正选中态与文字层级（P0）

**Files:**
- Modify: `src/ui/omni.rs`。
- Test: `tests/ui_render.rs`。
- Reference: `src/ui/theme.rs`、现有 `transaction_menu_selected_row_uses_readable_selection_style` 测试。

**Step 1 — 添加最终 Buffer 颜色回归测试。**
构造已选中结果，检查标题所在单元格的 fg 为 text、bg 为 selection，而不是只断言输出含有标题。普通行标题和 subtitle 分别检查正文与次级样式。用当前渲染测试访问 Buffer 的方式实现。

**Step 2 — 运行 `cargo test --test ui_render omni`，确认旧选中颜色导致失败。**

**Step 3 — 定义局部行样式助手。**
普通行与选中行共用角色定义；基础前景 text，背景依选中状态切换。选中标题增加 Bold。将 subtitle 和 opened 标签变为独立 Span，移除 title.push_str("  [open]")。选择器示意：

```rust
let background = if selected { theme.selection } else { theme.surface_raised };
let base = Style::new().fg(theme.text).bg(background);
let title_style = if selected { base.add_modifier(Modifier::BOLD) } else { base };
let subtitle_style = base.fg(theme.muted);
```

**Step 4 — 为无颜色模式保留选中标记。**
预留固定 marker 列，选中使用 `>` 或 IconSet 对应符号。清除会从基础样式错误继承的整行 Bold；保留必要的终端文本净化。

**Step 5 — 验证默认主题、一个内置浅色主题（如存在）、自定义浅色样例与无颜色模式。**
如选中 subtitle 对比不足，按上文局部策略调整，不修改全局 muted。再次运行同一组 Omni 渲染测试。

**验收：** 选中主标题清楚；连接信息弱化但可读；普通行与选中行都有稳定层级。

**建议提交：** `fix(omni): make selected results readable`

## Task 3：结构化结果类型并接入图标（P1）

**Files:**
- Modify: `src/model/omni.rs`：`OmniItem` 和构造器。
- Modify: `src/app.rs`：`refresh_omni_items`、`apply_omni_search_page`。
- Modify: `src/ui/icons.rs`、`src/ui/omni.rs`、`src/ui/mod.rs`。
- Test: `tests/omni_providers.rs`、`tests/omni_search.rs`、`tests/ui_render.rs`。

**Step 1 — 增加准确类型的 provider 回归样例。**
用现有 catalog fixture 构造 table/view，本地加载与远程页面都断言类型保留。连接条目保留 DatabaseKind。不要只测试枚举到固定字符的映射。

**Step 2 — 增加结构化展示类型。**
建议 enum 形状如下，具体 import 使用项目已有路径：

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OmniItemKind {
    Command,
    Connection(crate::profile::DatabaseKind),
    Console,
    Catalog(crate::db::catalog::CatalogKind),
    Recent,
    Resume,
}
```

给 OmniItem 增加 `kind`。构造器显式接收 kind，并迁移所有生产代码和测试调用，编译器帮助找出遗漏。category 继续用于显示/搜索，Catalog 的 category 从真实 kind 生成；不删除现有可搜索字段。

**Step 3 — 更新全部本地生产路径。**
覆盖命令、连接、控制台、缓存关系、最近位置、恢复交互、对象动作和连接选择步骤。动作行仍使用 Command 图标；最近位置用 Recent，实际导航身份继续由 id/action 管理。

**Step 4 — 更新远程页面映射。**
直接关系命中使用 hit.entry.kind。对于 column 等带 relation_id 的非关系命中，从 ancestors 或本地 catalog 解析实际目标关系；明确展示标题/图标指的是命中对象还是导航目标，禁止标题是列名却盲目标注 Table。推荐统一展示目标关系，命中字段作为辅助上下文/keywords；解析不到目标类型时保留真实命中类型，避免编造 Table。新增至少一个非关系命中回归测试。

**Step 5 — 接入 IconSet。**
为缺少的命令、控制台、最近位置、恢复交互提供图标访问方法，覆盖 NerdFont/Unicode/Ascii。关系与连接复用 catalog/database；所有图标宽度由终端 cell width 测量。向 omni::render 显式传入当前 IconSet，同步 `src/ui/mod.rs` 的正常与紧凑渲染两个调用点。

**Step 6 — 编译并验证。**
运行 `cargo check --all-targets`、`cargo test --test omni_providers --test omni_search`、`cargo test --test ui_render omni`。

**验收：** 本地/远程对象类型一致，表与视图可区分，所有图标模式可用，ID、action、异步 session/generation 校验保持正确。

**建议提交：** `feat(omni): render typed result icons`

## Task 4：结果行宽度分配与独立状态布局（P1）

**Files:**
- Modify: `src/ui/omni.rs`。
- Test: `src/ui/omni.rs` 内局部布局测试、`tests/ui_render.rs`。

**Step 1 — 添加针对长路径的回归样例。**
覆盖短标题+超长路径、长中文标题、空 subtitle、opened 标签、可用宽度 0/1/小于图标宽度。断言最终 line 宽度不超过区域，标题在足够空间时仍可见，不出现半个宽字符。

**Step 2 — 定义标题优先的宽度预算。**
先保留 marker、icon 和间距，再给主标题保证宽度（正常宽度目标至少 16 cells，受实际空间限制）。其余空间供 subtitle 和短标签；可选标签先退让，subtitle 再缩短，标题最后截断。正常宽度 subtitle 可限制为内容区域约三分之一，优先保留连接名。标题与 subtitle 都按 cell width 截断并加省略号，ASCII 模式使用适合当前空间的 ASCII 标记。

**Step 3 — 独立计算纵向区域。**
结果布局改为 query / context / results / optional status / footer。status 为空时释放该行；非空时从结果高度中扣除。区域计算使用实际 inner.height 和 saturating 运算，极小终端继续走原有安全提示。

**Step 4 — 动态状态选择。**
优先显示 omni.status，其次选中条目的 Disabled 原因，其次空结果说明。NeedsArguments 是可继续的流程状态，不当成 Disabled。不要生成与执行层状态矛盾的提示。

**Step 5 — 命中区域与最终行位置一致。**
只为实际绘制的结果注册 OmniItem hit region。status/footer 区域点击落到 Omni 背景屏障，不命中已被覆盖或未显示结果。

**Step 6 — 验证。**
运行 `cargo test --lib ui::omni`、`cargo test --test ui_render omni`。增加“结果恰好满屏且出现 status”的 Buffer 和 hit target 断言。

**验收：** 长 subtitle 不再挤没标题，状态信息不覆盖结果，窄屏无越界，点击与视觉一致。

**建议提交：** `fix(omni): reserve space for titles and status`

## Task 5：选中项可见窗口与正确输入光标（P1）

**Files:**
- Modify: `src/ui/omni.rs`。
- Reference: `src/ui/mod.rs`：`render_text_input`。
- Test: `tests/ui_render.rs`、`tests/omni_input.rs`。

**Step 1 — 添加跨屏导航回归测试。**
构造多于两屏结果，依次下移、上移、首尾环绕，并模拟终端缩放。每次渲染后断言选中项出现在结果区域，且其 hit target 指向 visible_items 中正确的全局索引。

**Step 2 — 抽取纯窗口计算函数。**
输入结果数量、实际 results.height、建议 scroll 和 selected_index，返回 start/end。先把 start 限制在有效范围，再在 selected 超出窗口时向上或向下调整；零高度直接返回空窗口。在只读 App 渲染接口下，用该纯函数派生窗口，无需为 UI 高度修改业务状态；旧 scroll 作为建议值使用，避免额外维护互相矛盾的滚动状态。

**Step 3 — 统一窗口使用。**
结果切片、结果计数/位置提示和鼠标 hit index 全部使用同一个 start/end。测试 selected=None 和异步刷新导致选中项消失的情况，不在渲染时擅自创建业务选中项。

**Step 4 — 复用文本输入渲染。**
替换查询字符串长度计算光标的代码，调用父模块 render_text_input，传入 `&omni.query`。如果搜索图标需要独立颜色，先在独立小区域渲染图标，再将剩余区域交给输入助手，避免重复实现 cursor/offset 算法。

**Step 5 — 光标回归验证。**
覆盖输入后 Left/Right/Home/End、插入中文、超长查询水平滚动；断言光标位于输入区域内且对应编辑位置。保留 Omni 对底层 Overlay 的光标接管与鼠标屏障测试。

**Step 6 — 运行验证。**
运行 `cargo test --test ui_render omni`、`cargo test --test omni_input`、`cargo test --test omni_navigation`。

**验收：** 键盘选中项始终可见，缩放后索引正确，输入光标与真实编辑位置一致。

**建议提交：** `fix(omni): keep selection and input cursor visible`

## Task 6：步骤标题、过滤信息和动态操作提示（P1）

**Files:**
- Modify: `src/ui/omni.rs`。
- Reference: `src/ui/shortcut_hints.rs`、`src/input/keymap.rs`：`map_omni`。
- Test: `tests/ui_render.rs`。

**Step 1 — 定义步骤标题。**
Root 显示 OMNI；ObjectActions 显示 OMNI / Actions 并在 context 区显示目标；PickConnection、PickTarget、NameConsole 使用当前实际步骤对应的简短标题。只展示已有流程信息，不能通过渲染新增业务步骤。

**Step 2 — context 行展示当前过滤状态。**
All / Commands / Connections 根据 OmniFilter 显示；profile_scope 解析为连接名并以次级颜色显示。结果数量使用 visible_items 的长度。显示 `> Commands`、`@ Connections` 作为语法帮助，而非伪装成尚未实现的可点击标签。窄屏优先保留当前作用域，再省略语法说明和数量。

**Step 3 — 建立按步骤/对象变化的 footer。**
根层 Esc Close，子层 Esc Back；只有实际支持 OmniShowActions 的当前选中对象显示 Tab Actions。Enter 的描述与当前动作一致：连接筛选、打开对象、执行命令或下一步。关闭提示可显示 Ctrl-C 或用户配置的 Omni 绑定，但不在窄屏重复占用空间。

**Step 4 — 使用共享 ShortcutHint 渲染。**
优先级为 Enter、Esc、导航、Tab（适用时）、额外关闭键；常见宽度展示完整提示，窄屏使用已有省略打包逻辑。不要把 `?` 设为帮助入口，因为它是当前搜索输入字符。

**Step 5 — 验证。**
运行 `cargo test --test ui_render omni`、`cargo test --test omni_flows --test omni_resume`。断言普通命令不提示 Tab Actions，子步骤不错误提示 Esc Close，自定义 Omni 绑定不显示过时 F2。

**验收：** 标题说明当前所在步骤，作用域可见，所有可见快捷键都与实际行为相符。

**建议提交：** `feat(omni): add contextual navigation hints`

## Task 7：文档、综合回归与终端验收（首轮交付门槛）

**Files:**
- Modify: `docs/omni-bar.md`。
- Modify if applicable: `README.md` 中已有快捷键说明。
- Update: 本计划中的任务完成与检查结果记录。

**Step 1 — 更新使用说明。**
记录 Help 可发现入口、F2 是默认可配置绑定、图标模式、类型/连接/已打开标签含义、Esc 返回与关闭区别、Tab 条件。查看 README 是否已有快捷键表，有则补充，避免复制完整 Omni 文档。

**Step 2 — 一次性运行相关回归集。**

```bash
cargo test --lib omni
cargo test --lib help
cargo test --test omni_input --test omni_providers --test omni_search --test omni_flows --test omni_navigation --test omni_resume
cargo test --test ui_render
```

期望：全部通过；没有只因截图文本包含 OMNI 就判定颜色通过的弱断言。

**Step 3 — 按现有 CI 执行最终检查。**

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

期望：格式正确、无新增警告、测试通过。工具链/数据库/本地库造成的环境阻塞单独记录，不能写成已通过；不为本次 UI 改动擅自改 CI 或数据库驱动。最终检查通过后，无新增改动或失败则不重复跑同一检查。

**Step 4 — 真终端人工验收矩阵。**

| 维度 | 样例 | 观察 |
| --- | --- | --- |
| 主题 | 默认深色、自定义浅色、无颜色 | 正文与辅助文字可读、选中明确 |
| 图标 | NerdFont / Unicode / Ascii | 无缺字、宽度稳定、类型能辨认 |
| 尺寸 | 120×36、80×24、40×12、极小尺寸 | 标题优先、提示降级、无越界 |
| 内容 | table/view、长连接路径、中文、opened | 信息层级、正确类型、标签不挤压 |
| 状态 | 空结果、Disabled、远程 truncated | 原因清楚、状态不覆盖结果 |
| 操作 | 跨页上下移动、首尾环绕、子步骤返回 | 选中可见、按键提示正确 |
| 输入 | 左右/Home/End、长查询、粘贴 | 光标正确、终端文本安全显示 |
| 入口 | Help 搜索执行、自定义绑定、底层 Overlay | 入口一致、关闭后恢复正确 |

无需跑完整笛卡尔积：自动化覆盖布局边界，人工检查有代表性的组合并记录截图/终端环境。

**Step 5 — 收尾审查。**
执行 `git diff --check`，确认只提交本任务文件。审查新模型字段没有进入不必要的持久化格式；审查本地/远程 provider 没有因展示改动改变导航目标或异步结果校验。补充每项实际运行命令与结果。

**建议提交：** `docs(omni): document shortcuts and result presentation`

## Task 8：搜索命中高亮（P2，首轮验收后独立实施）

**Files:**
- Modify: `src/ui/omni.rs`。
- Test: `src/ui/omni.rs` 内高亮区间测试、`tests/ui_render.rs`。

1. 使用 `omni.parsed_query()` 提取 token，剔除现有过滤语法，与当前多 token 搜索语义一致。
2. 在已净化显示文本上计算命中区间，合并重叠；标题命中用 accent/Bold，subtitle 命中提高可读性但不让整条路径抢过标题。
3. 原字符串与大小写规范化字符串建立安全位置映射，禁止把 lowercased 字符串字节偏移直接用于原字符串切片。优先复用现有项目匹配高亮工具；若无，则使用字符范围和显示映射实现。
4. 先计算命中，再根据实际截断范围裁剪 Span；高亮不改变宽度和点击范围。
5. 覆盖多 token、中文、大小写映射长度变化、空查询、路径命中与截断边界。运行 `cargo test --lib ui::omni`、`cargo test --test ui_render omni`。

**验收：** 高亮对应实际搜索命中，无 UTF-8 切片 panic，无布局漂移。

**建议提交：** `feat(omni): highlight search matches`

## 4. 依赖顺序与交付检查表

推荐串行顺序：Task 1 → 2 → 3 → 4 → 5 → 6 → 7；Task 8 在首轮视觉验收后单独实施。多个任务共享 `src/app.rs` 与 `src/ui/omni.rs`，默认由同一执行者顺序修改。

- [ ] Task 1：帮助入口与配置绑定
- [ ] Task 2：可读选中态与文字层级
- [ ] Task 3：真实类型与图标
- [ ] Task 4：宽度预算与独立状态布局
- [ ] Task 5：可见窗口与输入光标
- [ ] Task 6：步骤和动态提示
- [ ] Task 7：文档、CI 与实际终端验收
- [ ] Task 8：后续搜索高亮

本文为实施计划；创建计划时未修改功能代码、未执行以上测试，测试名称及辅助函数在实施时沿用仓库现有结构落地。
