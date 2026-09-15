# Redis 区域上下文帮助 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行环境若没有该技能，按本文任务顺序执行并记录验证结果，不依赖不可用工具。本文是实施计划，不代表代码或测试已经完成。

**Goal:** Redis Keys 和 Value 区域按默认 `?` / `F1` 可打开现有帮助面板，内容与当前焦点、输入模式、视图和动作可用性一致。

**Architecture:** 复用 `Keymap → Action::ShowHelp → HelpState → Overlay::Help`，将帮助判定放在 Redis 独占输入与应用级 pending 消费之前，并保留字符输入边界。细分 Redis 帮助上下文，共享动作可用性判断，让帮助目录、footer、前缀提示和帮助内执行保持一致；局部收敛重复 Redis 路由。

**Tech Stack:** Rust 2024 / Rust 1.94、Crossterm 0.29、Ratatui 0.30、现有 modalkit 只读编辑器、Cargo 单元及集成测试。

---

## 1. 已确认的代码基线

- `config/default.toml:69`：`help = ["F1", "?"]`。
- `src/input/keymap.rs:1222–1248`：文本 Redis Preview 完整转发输入，早于帮助处理。
- `src/input/keymap.rs:1336–1520`：Keys 独立分支末尾直接返回，帮助键落入 `None`。
- `src/input/keymap.rs:1740–1748`：现有配置化帮助键入口。
- `src/input/keymap.rs:1914–1994`：第二段 Redis 路由，重复处理 find，部分逻辑已被早期分支遮挡。
- `src/help.rs:245–248`：已有 `RedisKeys` / `RedisPreview`，尚未区分查找和 Table。
- `src/help.rs:821–840`：Help 目录条目未包含 Redis。
- `src/help.rs:1497–1554`：Redis 条目不完整，文本搜索仍包含 `?`。
- `src/help.rs`：`shortcut_capabilities`、`available`、`configured_sequence`、footer 和 prefix 过滤为既有扩展点。
- `src/app.rs:6077–6085`：现有 ShowHelp 已正确捕获上下文与用户键位配置。
- `src/app.rs:2485`：`execute_help_shortcut` 负责帮助内执行，包括执行前可用性校验。

行号仅对应分析时版本，实施时按符号重新定位。Table 当前理论上能到达帮助入口，应先用正向回归验证，不把所有 Value 模式都认定为同一故障。

### 与已有计划的衔接

先阅读 `docs/plans/2026-09-15-redis-key-tree-interaction-implementation.md`。该计划拟移除 Keys 的 `o` 打开映射，并细化 `r` 的刷新/继续行为。本计划不依赖它先完成，但帮助说明必须跟随实施时实际落地的动作：若 `o` 已移除，不重新添加；若 `r` 已调整，采用准确的刷新/继续描述。可用性也应跟随实际选中 Key / 已打开 Key 的模型，不自行假设二者相同。

## 2. 固定交互契约

| 状态 | 默认 `?` | 默认 `F1` | 帮助上下文 |
| --- | --- | --- | --- |
| Keys 浏览 | 打开帮助 | 打开帮助 | RedisKeys |
| Keys 查找输入 | 输入 `?` | 打开帮助 | RedisKeysFindEditing |
| Keys 查找确认 | 打开帮助 | 打开帮助 | RedisKeysFindConfirmed |
| 文本 Value Normal | 打开帮助 | 打开帮助 | RedisPreview |
| 文本 Value Visual | 打开帮助 | 打开帮助 | RedisPreviewVisual |
| 文本 Value 搜索/命令输入 | 输入 `?` | 打开帮助 | RedisPreviewPrompt |
| Table Value | 打开帮助 | 打开帮助 | RedisPreviewTable |

约束：

1. 使用当前 `KeyBindings` 匹配帮助，不能硬编码 F1 或额外保留已被用户移除的 `?` 默认绑定。
2. 文本输入态保留可打印字符；非字符帮助绑定可打开帮助。帮助面板中只显示当前真正可触发的帮助绑定，例如输入态默认只显示 F1。
3. 文本 Value 浏览态的 `?` 从 Vim 反向搜索改为帮助；`/`、`n`、`N` 保留原搜索语义，不新增替代搜索绑定。
4. 打开帮助之前处理并清理应用级 pending，关闭后不能恢复旧 Space/g/window 序列。编辑器内部搜索文本、模式、选择、光标和滚动不清空。
5. 保留已有 overlay 的输入优先级。本次只扩展底层 Redis 区域；Redis 对象编辑器、格式选择器等独立 overlay 不在此轮重构范围。
6. Empty / Loading / Failed Value 也能打开帮助；数据动作依真实能力过滤。
7. 帮助默认只呈现当前可用动作；已有帮助打开期间状态发生变化，执行前仍必须重新验证。
8. 浏览态公共控制经实际路由验证后才列入 Redis 目录，不批量复制 SQL 的运行、事务或 Relation 编辑命令。

## 3. 实施顺序

任务依赖：Task 1 → Task 2 → Task 3 → Task 4 → Task 5 → Task 6 → Task 7。每个任务内以小步完成：添加行为测试、运行确认、最小实现、定向验证。建议按下文边界形成逻辑提交，实际提交遵循用户的提交要求。

### Task 1：建立 Redis 帮助链路回归基线

**Files**
- Create: `tests/redis_help.rs`
- Reference: `tests/redis_browser_tabs.rs`
- Reference: `tests/redis_key_filter.rs`
- Reference: `src/input/keymap.rs` 现有配置键位测试

**Steps**
1. 复用已有测试的 RedisBrowserTab / App 构造方式，添加本地 fixture，显式设置 `Focus::Results`、Redis 内部焦点与文本/Table 格式。文本场景初始化真实只读 session，避免只测不存在的编辑器。
2. 用 `KeyEvent::new` 构造 F1、无修饰符 `?`，以及 SHIFT 形式的 `?`，通过真实 `Keymap::map` 和 `App::update` 检查帮助 overlay。
3. 添加 Keys、文本 Value 的失败用例；Table 添加预期通过的对照用例。
4. 验证打开帮助产生的 Commands 为空，不触发查询、扫描或变更。
5. 运行 `cargo test --test redis_help`，记录失败断言。预期故障集中在 Keys 和文本 Value 的触发链路；若 Table 失败，定位实际焦点/路由后补入修复范围。

**推荐测试名**
- `redis_keys_help_opens_from_configured_bindings`
- `redis_text_help_opens_from_configured_bindings`
- `redis_table_help_opens_from_configured_bindings`
- `redis_help_open_does_not_issue_database_commands`

**完成标准**：可稳定复现故障，fixture 无需真实 Redis 服务。

### Task 2：补全上下文与输入状态识别

**Files**
- Modify: `src/help.rs` — ShortcutContext、ALL_SHORTCUT_CONTEXTS、shortcut_context_with_overlay、context_name
- Modify if needed: `src/app.rs` — 只读编辑器模式/提示符状态访问
- Modify if needed: `src/editor/mod.rs` — 最小只读状态访问器
- Test: `src/help.rs` 内部测试

**Steps**
1. 添加上下文矩阵测试：Keys 的无 find / Editing / Confirmed；Value 的文本 Normal / Visual / Prompt / Table。
2. 添加串区用例：Keys find 仍存在时焦点切到 Value，必须得到 Value 上下文；焦点在 Explorer 时得到 Explorer 上下文。
3. 运行 `cargo test --lib help::tests`，确认新增断言暴露现有粗粒度分类。
4. 保留 RedisKeys、RedisPreview，新增 RedisKeysFindEditing、RedisKeysFindConfirmed、RedisPreviewVisual、RedisPreviewPrompt、RedisPreviewTable。
5. 检查现有编辑器模式与 prompt 状态接口；若单纯 EditorMode 无法区分提示符，复用已有 prompt 状态。只有缺少读取能力时才新增最小访问器，不在帮助层解析编辑器渲染文本。
6. 分类顺序固定：overlay → 主焦点 → Redis 内部焦点 → Table/text → 文本输入/选区模式。
7. 更新全部 exhaustive match 和测试上下文列表；标题使用 REDIS KEYS、REDIS KEYS · FIND、REDIS VALUE · TEXT、REDIS VALUE · TABLE，Visual/Prompt 可附状态后缀。
8. 再运行 `cargo test --lib help::tests`。

**完成标准**：上下文识别无需复制编辑器状态，不受另一 Redis pane 的残留 find 状态影响。

### Task 3：修复帮助优先级及状态恢复

**Files**
- Modify: `src/input/keymap.rs` — Keymap::map、pending 清理、帮助匹配
- Test: `tests/redis_help.rs`
- Test: `src/input/keymap.rs` 内部测试

**Steps**
1. 添加输入保护测试：Keys find 和 Value prompt 输入 `?`，正文真实更新；F1 打开帮助，关闭后能继续输入和确认。
2. 添加 Visual 状态恢复测试，比较打开前后的模式、选择、光标、滚动和内容。
3. 添加自定义 `help = ["F11"]` 测试：F11 打开，旧 F1/? 不再作为 Redis 帮助键；输入态字符仍归编辑器或 find。
4. 添加应用级 Space 前缀中按 F1 的测试，确认帮助优先、pending 清空，关闭帮助后普通键不作为旧前缀续键。
5. 抽取轻量帮助触发判定，参数使用实际绑定和上下文输入状态。仅命中合法帮助绑定时返回 ShowHelp；字符输入态不拦截可打印字符。
6. 在已有 overlay 分发之后、应用级 pending 消费和 Redis 独占输入之前调用。复用同一判定逻辑处理后方通用入口，避免维护两套条件；保留非 Redis 编辑器既有输入规则。
7. 不把 F1 交给只读编辑器；不通过先发送 Esc 再打开帮助实现，以免破坏选区或搜索输入。
8. 运行 `cargo test --test redis_help` 和 `cargo test --lib input::keymap::tests`。

**完成标准**：Task 1 全部通过；默认与自定义帮助键一致，关闭帮助保留底层状态。

**建议提交边界**：`fix(redis): route contextual help before pane input`

### Task 4：补齐快捷键目录和可用性

**Files**
- Modify: `src/help.rs` — HelpShortcutId、SHORTCUT_CATALOG、ShortcutCapabilities、available、configured_sequence
- Modify if needed: `src/model/redis_browser.rs` — 可共享的纯能力查询
- Modify if needed: `src/app.rs` — 调用同一能力判断的动作分支
- Test: `src/help.rs` 内部测试
- Test: `tests/redis_help.rs`

**Steps**
1. 建立目录内容断言，覆盖每个新上下文的必有/不得出现条目。
2. Keys 浏览目录补充上下移动、展开/折叠、主操作、查找、复制、新建、编辑、删除、刷新/继续和分页。按执行时真实语义决定是否包含 o，不覆盖另一计划的动作修改。
3. Keys FindEditing 只列输入、删除字符、确认、取消和合法非字符帮助绑定；Confirmed 列出可用浏览操作及 n/N/Esc。
4. 文本 Value Normal 列出移动、翻页、搜索、复制、选区进入、Space f/w/l；Visual 列出实际选区移动/复制/退出；Prompt 列出输入、确认和取消。移除浏览态 `?` 反向搜索描述。
5. Table 目录逐项核对 `map_results` 和配置导航：复用语义相同的导航/复制/列宽/详情 ID，分离文本特有操作；格式切换使用真实 Table 按键，不把文本 Space 前缀说明直接复制过来。
6. 为所有 Redis 上下文加入 Help 条目。公共焦点/Tab/Omni 等条目仅在实测有效时纳入；不增加 SQL 专属操作。
7. 先审查 create/edit/delete/copy/load-next 的现有 reducer 条件，再提取必要纯查询，供 help 和动作侧共同使用。避免构建通用命令框架或重复一套业务条件。
8. 能力测试覆盖：无选中项、分组、叶子、值未加载、加载中、可继续分页、已完成分页、无表格单元格。编辑/复制/删除分别按实际操作对象判断。
9. 让 Redis 导航条目的配置显示对应实际使用的绑定。组合说明如 y{motion} 标为 display-only，单个 ID 不对应多种含糊的执行语义。
10. 帮助显示的绑定过滤应使用与路由相同的字符输入策略，输入态不能显示实际被输入消费的 `?`。
11. 运行 `cargo test --lib help::tests` 和 `cargo test --test redis_help`。

**完成标准**：目录描述的是当前能用的操作；Text/Table/Find 不串区；配置展示与真实路由一致。

### Task 5：接通帮助内执行、footer 与前缀提示

**Files**
- Modify: `src/app.rs` — execute_help_shortcut
- Modify: `src/help.rs` — footer_priority / 上下文优先级、prefix 目录
- Modify if needed: `src/commands.rs` — 仅审查并接入已有语义命令映射
- Modify if needed: `src/ui/mod.rs` — 已有帮助/footer 渲染，优先由目录驱动
- Test: `src/app.rs`、`src/help.rs` 内部测试
- Test: `tests/redis_help.rs`

**Steps**
1. 审查每个 Redis executable ID：确认是经过 command_for_help 还是本地 match 分发，不能因目录标记 executable 就假设已有执行实现。
2. 为原子操作映射真实 Redis Action；打开编辑/删除时复用既有交互链路。对于 Vim 模式输入，如果没有明确的完整动作则保持 display-only。
3. 为帮助选中条目按 Enter 添加测试，比较直接按键与帮助执行的状态变化/Commands；破坏性操作只检查进入既有确认或编辑流程。
4. 添加能力失效测试：帮助打开后状态改变，执行前 `shortcut_is_available_in_app` 重新判断，禁止执行过期动作。
5. 明确底部优先级：Keys 优先移动、主操作、查找、常用编辑操作、帮助；文本优先移动、搜索、复制、格式、帮助；Table 优先单元格移动、复制、详情、格式、帮助。Prompt/FindEditing 优先确认、取消、帮助。
6. 确保 Space f/w/l 前缀候选遵循文本模式和分页能力；Table 不显示并不存在的文本前缀。
7. 用现有 Ratatui buffer 测试方式验证 Keys/Text/Table 帮助标题、代表性条目、过滤查询以及窄终端 footer；不为纯标题替换单独建立脆弱的全屏快照。
8. 运行 `cargo test --lib help::tests`、`cargo test --lib app::tests`、`cargo test --test redis_help`。

**完成标准**：帮助不仅能打开，展示、搜索、Enter 执行、footer 和前缀提示也相互一致。

**建议提交边界**：`feat(redis): complete state-aware shortcut help`

### Task 6：局部收敛 Redis 输入路由

**Files**
- Modify: `src/input/keymap.rs`
- Test: `tests/redis_help.rs`
- Existing regression: `tests/redis_key_filter.rs`、`tests/redis_key_tree.rs`、`tests/redis_browser_tabs.rs`、`tests/keymap.rs`

**Steps**
1. 在移动代码前增加关键保护用例：Keys find 为 Confirmed 时，焦点切到 Table，n/N/Esc 不能误操作 Keys find；Value 输入不能写进 Keys 查询。
2. 列出早期和后期 Redis 分支中实际可达路径，保留已有配置化导航、文本 Vim 转发和 Table 特殊键位。
3. 将 Keys 的输入态/确认态/浏览态收敛到一个入口；Value 明确分为文本和 Table。帮助判定保留在独占输入之前。
4. 移除已被外层 Keys 条件排除的 Preview 分支和重复 find 处理，避免无匹配的普通键掉入 SQL 专属兜底。
5. 若提取函数，明确“当前 pane 已消费但无 Action”与“交给公共处理”的区别，不用一个含糊的 Option 同时表示二者；仅在现有返回结构无法表达时增加局部路由结果类型。
6. 运行 `cargo test --test redis_help --test redis_key_filter --test redis_key_tree --test redis_browser_tabs --test keymap`，以及 `cargo test --lib input::keymap::tests`。

**完成标准**：Redis find 只影响 Keys；独占输入/公共帮助层次清晰；原导航、查询与 Value 交互回归通过。

**建议提交边界**：`refactor(redis): consolidate pane key dispatch`

### Task 7：文档同步与最终验收

**Files**
- Modify: `docs/redis-browser.md`
- Modify: `docs/redis-value-preview.md`
- Modify: 本计划，记录实际验证结果与必要偏差

**Steps**
1. 更新 Keys 与 Value 帮助入口、标题、文本/Table 区别。
2. 明确 `?` 在文本 Value 浏览态为帮助，在查找/命令输入态为输入；F1 为默认非字符帮助绑定；自定义绑定以配置为准。
3. 与 Key 树交互计划核对 o、Enter、r 的最终语义，只记录已经落地的操作。
4. 运行定向回归（若 Task 6 后没有代码修改且已通过，不重复同一轮）。
5. 执行与 CI 一致的检查：

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

预期均退出码 0；记录依赖外部数据库环境而跳过的测试，不能把未执行描述为通过。若遇到既有失败，先区分是否由本次改动引起并在交付说明中列出。

6. 手工验收：Keys 普通/查找确认/查找输入，Raw 或 JSON Value Normal/Visual/Prompt，Table Value，各按 ?/F1；确认标题与条目。
7. 测试关闭帮助后的位置、选区、搜索词和格式恢复；测试 Space 后 F1；测试自定义 F11；测试 narrow terminal。
8. 回归 SQL Editor Normal/Insert、Relation Data 浏览/编辑、Relation DDL 的帮助和输入边界。真实 Redis 手工验收需要现成连接；若环境缺少，标明尚未验证，不由该计划自动创建外部资源。
9. 审查 diff，确认没有引入无关配置、依赖或修改其他未提交计划。

**建议提交边界**：`docs(redis): document contextual help behavior`

## 4. 最终验收清单

- [ ] Keys、文本 Value、Table Value 默认帮助键均可打开现有帮助面板。
- [ ] 帮助严格对应当前主焦点、Redis pane、模式和视图。
- [ ] 查找/命令输入中的 ? 不被帮助抢占，F1 可用且目录说明准确。
- [ ] Help 关闭后恢复原状态，不遗留应用级 pending。
- [ ] 自定义帮助及导航绑定的显示和触发一致。
- [ ] Keys find 不影响 Value 的输入和帮助上下文。
- [ ] 文本与 Table 不互相展示专属快捷键。
- [ ] 不可用动作被过滤，帮助内执行再次检查当前能力。
- [ ] footer、前缀候选、帮助列表和执行动作一致。
- [ ] o/r 与已落地 Key 树交互保持一致。
- [ ] 无需真实 Redis 的新增回归全部通过，最终检查及手工验证有真实记录。

## 5. 交付内容

1. Redis 帮助路由和上下文识别修复。
2. 完整且按状态过滤的快捷键目录、footer、前缀提示及帮助内动作执行。
3. 聚焦本次故障的链路/输入状态/配置/能力回归测试。
4. Redis 使用文档及验证结果。

实施以七个任务顺序推进；现阶段只保存计划，不执行代码变更或创建提交。
