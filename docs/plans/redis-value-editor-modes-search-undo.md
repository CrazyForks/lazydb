# Redis VALUE Vim Experience Implementation Plan

> **执行负责人：Luna。** 按下列可验收单元连续实施、审查和纠偏；不启动子 Agent，不等待用户反复 resume。Astra 仅负责分析/计划。工作流名称与任务分支由 Luna 在计划完成后确定。

**Goal:** 修复 Redis 非 Table VALUE 的撤销渲染崩溃，并提供与 SQL Editor 一致的模式标题、光标形状及搜索/命令输入体验。

**Architecture:** 保留 EditorWorkspace 的 Vim、历史和 Redis 保存路径，修复 preview document、wrap index 与 snapshot 的版本一致性。提取模式/光标/prompt 的小型共享 UI 能力，Redis 独立管理布局；输入按 prompt owner 和 session capability 路由。

**Tech Stack:** Rust、ratatui TestBackend、crossterm、现有 modalkit 集成；无需新增依赖或数据库服务。

---

## 0. 基线、交接和范围

- 分析依据：`.git/opencode-tasks/ses_f521a8f42ffecZ94JHnUezlSin/analysis.md`。
- 计划基于 `fe3134adf47b8002cf88cc467f251a71585fb3bb`，目标 `main`。
- 本计划不包含业务代码修改。现有未跟踪 `.git-opencode-tasks/`、`docs/plans/2026-09-17-mariadb-catalog-compatibility.md`、`docs/plans/2026-09-17-redis-preview-clean-state.md` 是其他工作，不得覆盖或纳入本任务提交。
- 没有可读取的本任务 checkpoint.json；本阶段不写入 state/checkpoint，也不复用 analyze 回执。本轮计划交付到任务目录 `plan.md`，完成后仅写本轮 `plan-ccf96d39-7508-4b66-bf1b-b387860de35f.json` 回执。
- 实施开始检查实际 HEAD/diff。若其他工作已修改相关代码，依据实际实现调整行号和测试，不覆盖它。
- 可编辑与只读 VALUE 都有模式显示和搜索；只读不获得写权限。Table、Redis keys 查找、格式/Wrap 控件、Ctrl-S 和 dirty 确认流程必须保持原有语义。
- command 指现有 Ex prompt，不扩展完整 Vim，不新增 `:w/:wq`。可编辑文档支持已有替换；SQL 专属命令在 Redis 中给出不适用错误；`:q` 保留退出应用语义。
- 完成每个单元后继续下一单元；阶段边界、提交合并权限仍按当前自动任务协议执行。

### 验证分级与复核原则

| 类别 | 内容 | 完成依据 |
| --- | --- | --- |
| 用户需求验收 | Normal/Insert 光标、VALUE 模式标题、可用的搜索/command、u 不崩溃 | 各单元的行为级回归及最终集成闭环；不能仅用编译通过代替 |
| 项目强制 Rust 门禁 | CI 中的 fmt、clippy、all-targets/all-features test | 单元 4 的确切命令及退出结果；环境受限时如实记录，不能标为通过 |
| 实施所需定向验证 | 缓存一致性、输入优先级、readonly、session owner、布局与保存回归 | 单元 1–3 中指定的自动测试；测试过滤必须实际命中用例 |
| 补充建议验证 | 人工/PTY 终端体验、ignored release 性能基线、真实 Redis 交互 | 按实际改动和 Luna 收尾审查决定；不自动提升为必需门禁 |

每个单元通过后由 Luna 复核该单元实际 diff、失败前后证据、受影响调用方和边界，再继续下一单元。验证记录绑定当时版本和工作区状态；本计划列出的预期结果不是已执行结果。

## 单元 1：撤销/编辑后软换行渲染稳定且内容正确

**修改文件**
- `src/editor/mod.rs`：`render_wrapped_preview_snapshot`（约 1110）、`render_snapshot_with_options`（约 1391）、preview 文档缓存入口。
- `src/editor/tests.rs`：新增确定性编辑后重绘回归。
- `src/editor/preview_perf_tests.rs`：仅在现有基线需补充时修改。
- `src/editor/preview.rs`：仅在必须调整索引映射时修改。

### 1.1 先建立可失败的用户场景回归

新增以 `redis_value_wrapped_` 为前缀的测试，使用现有 `EditorWorkspace::open_value`、`press`、`render_wrapped_preview_snapshot`：

1. 源文本为十行，各行带唯一标记；viewport width=80、height=24，语言 Plain，wrap=true。
2. 先 render 预热缓存；按 `i`、Enter、Esc 插入一行，再 render，使缓存明确对应十一行版本。
3. 按 Normal `u` 后立刻 render；断言源文本恢复十行、逻辑行数正确、每个可见 render line 对应源文本中的 `line.line`、没有多余旧行。
4. Ctrl-R 后再 render；断言新增行恢复，revision 与内容一致。
5. 测试失败应指向旧 wrap index 与新 snapshot 的失配，不接受仅 `catch_unwind` 后忽略失败。

Run: `cargo test --lib redis_value_wrapped_ -- --nocapture`

预期：修复前至少一个测试出现所述越界或内容不一致；记录实际结果而非假定。

### 1.2 修复缓存版本源头

1. 抽取按 session.revision 确保当前 preview document 的私有 helper，普通 preview 和 wrapped preview 共用。
2. wrapped 入口必须先取得当前 `(document_revision, document)`，再生成/复用 `(document_revision, width)` 对应的 wrap index。
3. 不从陈旧 document 构造 index 后写入当前 session revision；不在每帧清空所有缓存。
4. 逻辑 snapshot 与 visual index 使用同一份当前文档。以真实逻辑行号映射，避免尾部 viewport clamp 改变起行后仍使用旧局部下标。
5. 直接索引可增加 checked access 返回明确 EditorError，但不使用 filter_map 静默丢行来掩盖错误。
6. 保证临时 preview_render_first_line override 在错误返回时也清理；宽高变化/文本缩短时 clamp offset，保持光标可见。

### 1.3 增加边界用例并验收

使用小型表驱动 fixture 覆盖：
- 缩短/增长行数，等行数但改变长行宽度；普通删除与替换也触发相同缓存路径。
- 空文本、尾换行、中文/tab/长行；wrap=false 对照。
- 滚动到底部后撤销、viewport 宽度改变、尾部 overscan。校验行内容和行号，不只校验不崩溃。
- 连续两次无修改 render 应复用文档/index；可以在 editor 私有单测中断言 Arc identity 或缓存 revision，无需生产埋点。

Run: `cargo test --lib redis_value_wrapped_ -- --nocapture`

Run: `cargo test --lib editor::preview_perf_tests::wrapped_preview_baseline_preserves_navigation_and_source_text -- --exact --nocapture`

**完成标准：** 原触发序列通过，undo/redo、尾部起行、wrap on/off 都显示正确内容；没有 SQL analysis cache 污染、没有修改 Redis 保存行为。

建议逻辑提交：`fix(editor): keep wrapped previews revision-consistent`。

## 单元 2：VALUE 模式标题和终端光标闭环

**修改文件**
- 新增 `src/ui/editor_chrome.rs`：小型 mode label、cursor style、prompt 绘制 helper（prompt 在单元 3 接入）。
- `src/ui/mod.rs`：注册模块，SQL Editor 共用 mode/cursor 映射。
- `src/ui/read_only_sql.rs`：允许按 snapshot.mode 选择光标，支持后续显式正文区域或光标控制。
- `src/ui/redis_browser.rs`：动态标题、控件空间分配。
- `tests/ui_render.rs`：复用 render_with_state / TestBackend helpers。

### 2.1 添加行为级 UI 测试

增加 `redis_value_` 前缀测试。构造非 Table string fixture，设置 `Focus::Results` 和 RedisBrowserFocus::Preview，保证通过已有 open/edit action 初始化真实 session，不伪造 snapshot.mode。

通过 Keymap → Action → App.update 输入并 render：

| 输入状态 | 标题包含 | CursorSpec.style |
| --- | --- | --- |
| 初始/Normal | VALUE NORMAL | Block |
| i | VALUE INSERT | Bar |
| Esc | VALUE NORMAL | Block |
| v | VALUE VISUAL | Block |
| V | VALUE VISUAL LINE | Block |
| Ctrl-V | VALUE VISUAL BLOCK | Block |
| R | VALUE REPLACE | Underline |

测试切换各模式前回到 Normal，避免序列互相影响。校验 cursor 位置在 VALUE 正文内。

Run: `cargo test --test ui_render redis_value_ -- --nocapture`

### 2.2 提取共享映射并接入 Redis

1. 以 SQL Editor 当前映射为准提取函数，传入 EditorMode；prompt 存在时光标规则优先返回 Bar。
2. SQL Editor 使用 helper，保持原有布局；不把整个 SQL render_editor 移植到 Redis。
3. VALUE 标题取当前 preview snapshot.mode，不能用仅支持 SQL 的 active_editor_mode。
4. 移除/缩小恢复整个旧顶边的行为，防止新模式标题被覆盖；以标题实际 display width 安排右侧格式/Wrap 控件和命中区。
5. 窄面板优先显示 VALUE/模式，放不下时隐藏次要控件，不画重叠热区。

### 2.3 验收共享组件回归

- 增加非焦点、overlay、极窄/极矮窗口测试，确认不抢其他控件光标、不越界。
- Table 对照保持原标题与交互；格式/Wrap 命中区域仍正确。
- DDL/SQL History 的只读 Block 行为保持正常；SQL Editor Insert/Replace/Visual 映射不变。

Run: `cargo test --test ui_render redis_value_ -- --nocapture`

Run: `cargo test --test ui_render -- --nocapture`

第二条在此单元完成时跑一次；后续只因共享 UI 再改而重跑。

**完成标准：** 标题与真实模式一致，焦点内光标正确，控件不覆盖标题，Table 和共享只读视图没有回归。

建议逻辑提交：`fix(redis): show value editor mode and cursor style`。

## 单元 3：可见且可完整使用的搜索/command

**修改文件**
- `src/editor/prompt.rs`：prompt owner session id。
- `src/editor/mod.rs`：start/press/submit/snapshot/paste prompt 路由、只读能力检查、上下文命令限制。
- `src/app.rs`：轻量查询当前 VALUE prompt 与 session 生命周期/切换取消；保留现有 Changed/SaveRequested 分支。
- `src/input/keymap.rs`：VALUE prompt 优先级、只读 prompt 粘贴。
- `src/ui/editor_chrome.rs`、`src/ui/mod.rs`：共享 prompt 行绘制。
- `src/ui/read_only_sql.rs`、`src/ui/redis_browser.rs`：正文/prompt 区域分配、cursor 与 viewport 同步。
- `src/editor/tests.rs`、`src/input/keymap.rs` 内测试、`tests/ui_render.rs`。
- `tests/redis_unsaved_changes.rs`：仅在新增保存/切换覆盖需要时修改。

### 3.1 先建立完整输入路径回归

测试名分别使用 `redis_value_prompt_`、`redis_value_search_` 前缀：

1. `/` 后 render，底部出现 `/`；输入 `i a o O R Q:`，快照/屏幕包含完整输入，不触发 Redis leader 或只读过滤。
2. Ctrl-W 删除最后一个词，不产生窗口切换；Ctrl-U 清空，左右/Home/End 和 Unicode 光标使用显示列。
3. Enter 搜索定位；`n/N` 正反向定位；`?` 反向搜索；未命中显示 pattern not found；Esc 退出并把 cursor 还给正文。
4. `:%s/one/two/g` 在可编辑值修改文本、产生 revision/dirty；同命令在只读值给出明确错误且文本不变。
5. 未知命令保留可见错误；Redis 中 SQL 专属命令不产生 SQL 执行/事务 action。
6. 切到其他 key/tab/session 后，旧 prompt 不显示、不消费新文档输入、不对新文档执行命令。

Run: `cargo test --lib redis_value_prompt_ -- --nocapture`

Run: `cargo test --lib redis_value_search_ -- --nocapture`

### 3.2 修复 prompt 生命周期与路由

1. PromptSession 带 owner id；所有初始化（含错误重建）保存 owner。snapshot 只暴露所属 session 的 prompt。
2. 提供非渲染查询，例如 `prompt_active(id)`；keymap 不通过 render_snapshot 判断输入状态。
3. 活跃 VALUE prompt 的完整输入在 Redis 空格 leader、Ctrl-W 窗口分支之前处理；不提升到 overlay 之上。
4. EditorWorkspace::press 先处理 owner 匹配的 prompt，然后执行只读文档键过滤。允许只读 VALUE 打开 command prompt，但在提交执行前拒绝文本修改。
5. paste 仅在 owner 匹配且 prompt 活跃时允许只读输入；不能因此允许往只读正文粘贴。
6. 切 session/关闭/重开相同 preview id 加载新 key 时取消旧 prompt；只检查 id 不足以覆盖复用 id 的 key 切换。可利用既有 session open/close 与 App focus/tab action，不引入全局事件总线。
7. 用已有 session 元数据区分 SQL 与 value；如没有足够信息，增加最小文档用途标记，不能以“是否 editable”推断 SQL。Redis SQL 专属命令返回 prompt 错误；通用命令及可编辑文本替换复用现有逻辑。

### 3.3 接入底部 prompt 布局

1. preliminary snapshot 检测所属 prompt，给 VALUE 面板底部预留一行，再以最终正文 viewport 生成 snapshot。
2. 正文 renderer 只渲染正文区域；鼠标选区/文本命中区不得包含 prompt。UiState.redis_editor_viewport、滚动条与最终正文尺寸一致。
3. helper 绘制 prefix + text + error；使用 EditorPromptSnapshot.cursor 的显示列定位，clip/横向滚动保证长输入光标可见。
4. prompt cursor 优先，使用 Bar；没有焦点或存在 overlay 时不抢光标。prompt 关闭后正文恢复高度。
5. 对 0/1 行高度明确处理：优先可见 prompt，正文可为空，不用 max(1) 强行画到面板之外。
6. SQL Editor 使用共享 helper 时保持其横向滚动条布局和既有错误显示；不能扩大本任务为整体 UI 重写。

### 3.4 集成验收和保存回归

Run: `cargo test --test ui_render redis_value_ -- --nocapture`

Run: `cargo test --lib redis_value_ -- --nocapture`

Run: `cargo test --test redis_unsaved_changes -- --nocapture`

- 为 prompt 开/关、wrap on/off、短窗口、中文/长输入、未命中错误增加屏幕/位置断言。
- 从 Keymap 输入完成一次“插入 → Esc → 搜索 → 替换 → u → render”的闭环；确认仍通过单元 1 的版本一致性路径。
- Ctrl-S 进入原 Redis 保存确认，取消保持 dirty，撤销至 baseline 恢复 clean；不能自动写 Redis。
- 对 readonly VALUE 搜索粘贴、key/tab 切换，确认不会影响 SQL session 或原始值。

**完成标准：** 搜索/命令可见、输入完整、错误清晰，prompt 不串文档；readonly 与 Redis 保存边界正确。

建议逻辑提交：`fix(redis): integrate value search and command prompts`。

## 单元 4：收尾审查与一次完整验证

### 4.1 Luna 审查实际 diff

- 核对用户四项需求都存在测试证据，尤其原 panic 的修复不是吞异常/丢行。
- 审查所有 revision 变化入口都受缓存版本检查保护，无需逐一手动失效。
- 检查 prompt owner、重开同 id、Ctrl-W/空格、readonly 替换、SQL effect 路由。
- 检查共享 SQL/DDL/History 行为、Wrap 控件、极小尺寸、鼠标映射。
- 更新现有快捷键文档仅在实际行为有新增且现有文档需要说明时进行，不为显示修复扩写无关文档。

### 4.2 完整检查（项目 Rust CI）

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

分别执行并记录退出码，预期均为 0。以 `.github/workflows/ci.yml` 的当前内容为准。编译/测试失败普通修复自行完成；后续重跑仅针对修改、失败或未消除疑点，不周期性重复 check+clippy+全量测试。

### 4.3 补充验证与环境限制

如果缓存热路径发生实质变化，补充一次：

`cargo test --release --lib editor::preview_perf_tests::preview_navigation_baseline -- --exact --ignored --nocapture`

记录 cold/navigation_p95/snapshot_p95 的实际输出，无历史同环境数据时不能声称性能提升。

真实终端 Normal/Insert 光标和底部输入体验可补充人工/PTY 检查；不是当前用户强制检查。无需真实 Redis 才能证明本地编辑/渲染修复；若需声明真实写入正常，则必须另有实际服务证据。

环境受限检查最多一次有针对性的修复重试；之后由 Luna 收尾审查记录限制或决定替代证据，不无限 progress。

### 4.4 留档、提交与交接

- 将每次实际验证的命令、结果、版本/diff、环境和跳过原因追加到任务目录 validation.md。
- 仅暂存本任务明确修改的文件，不用 `git add .` 纳入其他任务文件。
- 提交/合并由 Luna 在相应阶段执行；不得在 plan 阶段进行。
- 回执必须使用后续阶段明确下发的新路径和 token；不改历史 analyze 回执或插件维护的 state/checkpoint。

## 总体验收清单

- [ ] VALUE Normal/Visual Block、Insert Bar、Replace Underline；prompt Bar 且光标位于输入行。
- [ ] 标题显示当前 VALUE session 的真实模式，控件无重叠，Table 不受影响。
- [ ] `/ ? n N :` 输入、提交、取消、错误和历史有效，空格/Ctrl-W/只读字符不被错误截获。
- [ ] prompt 不跨 key/tab/session 泄漏，只读与 SQL/Redis 命令边界正确。
- [ ] u/Ctrl-R 与编辑、替换后重绘稳定，行内容/行号/软换行/高亮/光标一致。
- [ ] Ctrl-S 与 dirty/未保存确认通过回归。
- [ ] 项目完整 Rust 检查完成且验证记录对应最终代码。
