# Redis Value Input and Connection Implementation Plan

> **执行负责人：Luna。** Astra 仅执行分析与计划；实现、审查、纠偏、提交合并均由 Luna 完成。逐个完成端到端验收单元，不启动子 Agent，不等待用户反复 resume。

**Goal:** Redis 非 Table Value 的 `r2` 立即替换，并且跨连接切回 Redis tab 后保存始终使用该 tab 所属的 Redis 会话。

**Architecture:** 修复共享 EditorWorkspace 的 Vim 字符参数与应用级次数/快捷键分发边界；使用 Redis tab 完整 target 从已有 SessionRegistry 解析连接。把已加载 tab 的连接准备与扫描需求解耦，沿用既有 mutation 请求、适配器校验及异步结果归属机制。

**Tech Stack:** Rust 2024、modalkit 0.0.25、Crossterm 0.29、Ratatui 0.30、App reducer / Command runtime、Redis adapter、Rust 单元及集成测试。

---

## 0. 基线、执行约束与交叉工作

- 工作区 `/Users/yelog/workspace/tui/lazydb`；起点及计划时 HEAD：`84c5df482680bdaad719afdf1e7719e3b02cb78f`；目标分支 `main`。
- 分析：`.git/opencode-tasks/ses_f4c5d473cffekEDCFiUO3nUu2t/analysis.md`；验证日志为同目录 `validation.md`。
- 任务名称、分支和工作树由后续流程/Luna 分配。本计划阶段只创建本文档，不实施、不提交。
- 本阶段 checkpoint.json 仍不存在。恢复时先读届时 checkpoint 及实际 diff，按真实完成项推进；禁止修改插件 state.json/checkpoint.json。
- 完整计划同步到 `.git/opencode-tasks/ses_f4c5d473cffekEDCFiUO3nUu2t/plan.md`。本轮完成后只写指定 `plan-101a04ae-942c-430a-a1a7-8336f115a576.json` 回执；后续阶段使用其各自新指定的路径与 token，不沿用本轮或 analyze 回执。
- 工作区存在其他未跟踪计划；显式 stage 本任务文件，禁止 `git add .`。计划期间新增了他人的 `2026-09-18-table-editor-column-order-implementation.md`，保留。
- `docs/plans/2026-09-18-redis-value-dialogs-implementation.md` 是独立弹窗任务，尚不是当前业务代码。它也计划改保存 owner/确认状态：集成时沿用实际已落地类型，把本任务目标解析接到其 snapshot 构建处，避免恢复旧确认框或重复造状态机。其历史回执、分支及 next 不适用于本任务。
- `CONTRIBUTING.md`：App::update 只返回 Command，异步结果必须带 owner/generation。本任务修复现有快捷键语义，不新增快捷键或配置；若实现确实改变公开语义，再同步 shortcut catalog / docs/keybindings.md。

## 1. 需要保持的契约

### 输入所有权

`r` 已进入底层 Vim 参数等待状态时，随后 `2` 是替换字符，不是应用 count。第二键处理结束后，buffer、revision、Changed effect 与下一帧 preview 一致。第三键 `j` 只执行移动。

外层 `press` 当前在 `src/editor/mod.rs:2278-2326` 截留数字，之后在 `:2350-2477` 拦截自定义键。修复需要将“底层正在等待字符参数”置于这些拦截之前；仍保留 prompt、read-only、应用 pending_binding 及 count 的正确归属。

### 保存所有权

`RedisTarget(profile_id, database)` → `ExecutionTarget(profile_id, database.to_string(), schema=None)` → Connected SessionState.identity。

- 不能以全局 active_identity 作为无条件 fallback。
- 若兼容旧 fixture/路径采用全局 fallback，必须匹配完整 target、Connected 状态及有效 identity；注册表中明确 Failed/Connecting/retired 的目标不能被陈旧 fallback 绕过。
- mutation key、tab target、page metadata target 必须相等；会话不匹配不生成 mutation Command，草稿保留。
- request 一旦提交，其 connection/request_id 固定；执行/回包不重新绑定到当前前台连接。
- 同 profile 不同 Redis database 必须区分；禁止 generation=0 的伪造连接。

## 2. 单元一：Vim 数字替换与预览即时更新

**Files**
- Modify: `src/editor/mod.rs` — `EditorWorkspace::press`、必要的私有 pending 状态辅助函数。
- Test: `src/editor/tests.rs`。
- Test: `tests/redis_unsaved_changes.rs` — 经 App/EditorKey 路径检查 dirty/save 内容。

### Step 1：新增能稳定失败的最小回归

在 editor 模块测试里创建 Value session，先初始化一次预览缓存，再按 r、2。核心断言示例（置于现有模块可直接访问 pub(crate) 接口）：

```rust
#[test]
fn redis_value_replace_digit_is_immediate() {
    let id = Uuid::new_v4();
    let mut workspace = EditorWorkspace::new();
    workspace.open_value(id, "abc\ndef");
    let revision = workspace.revision(id).unwrap();
    workspace.drain_effects();
    workspace.press(id, EditorKey::Character('r')).unwrap();
    workspace.press(id, EditorKey::Character('2')).unwrap();
    assert_eq!(workspace.text(id).unwrap(), "2bc\ndef");
    assert!(workspace.revision(id).unwrap() > revision);
    assert!(workspace.drain_effects().iter().any(|effect| {
        matches!(effect, EditorEffect::Changed { console_id, .. } if *console_id == id)
    }));
    let revision = workspace.revision(id).unwrap();
    workspace.press(id, EditorKey::Character('j')).unwrap();
    assert_eq!(workspace.text(id).unwrap(), "2bc\ndef");
    assert_eq!(workspace.revision(id).unwrap(), revision);
}
```

扩展此测试或邻近测试：render_wrapped_preview_snapshot 在 wrap=true/false 的第二键之后即含新字符；使用已有 snapshot 字段断言实际显示文本，不能只检查渲染函数成功。

Run: `cargo test --lib redis_value_replace_digit_is_immediate -- --nocapture`

Expected：修改前 text 断言失败，实际仍为 abc；记录实际失败输出，不预先宣称已复现。

### Step 2：确定参数等待状态并最小修复

查看实际锁定 modalkit 0.0.25 的 Vim machine 状态/接口，以及现有 `has_pending_interaction` 对 get_cursor_indicator 的使用。区分底层字符参数等待与应用 pending_count/pending_binding。必要时查依赖文档；不升级依赖。

分发顺序：已有 prompt/read-only 校验 → 合法的应用待续绑定 → 底层 Vim 参数消费 → 应用 count/快捷键 → 通常 Vim 输入。具体条件遵循底层真实状态，不能使用 current_sequence 非空或整个 has_pending_interaction 作为参数等待的等价物。避免自建仅针对 r 的替换状态机。

复用 input_vim_key 的同步、history、record_changed 路径，不添加 redraw 定时器或 Redis UI 特判。若底层无可靠的直接等待谓词，使用明确可验证的底层状态组合或局部输入路由状态，保持与实际完成/取消动作同步。

### Step 3：验证共享行为及 App 可见效果

数据驱动覆盖：`r1/r2/r9/r0/rR/ru/rg/r空格`、`3r2`、`f2/t2`、普通 `2j`、`d2w`、`2 Ctrl-W +`；取消待续输入后下一条 count 不受污染；undo/redo、`.` 重复仍正确。复用现有 normal_fixture，不要求为每个字符建独立 fixture。

App 测试使用 EditorKey Action 完成 r2，立即断言 dirty 与保存请求包含替换文本，不补 j 作为“刷新”。与只读 session 现有测试一起验证不能绕过 capability。

Run:
```bash
cargo test --lib editor::tests::
cargo test --test redis_unsaved_changes
```

Expected：定向测试全部通过；记录此单元代码状态及结果。

### Step 4：Luna 单元审查并提交

检查真实 diff：count/快捷键未被整体禁用、字符参数即时消费、共享 SQL 编辑无回归。按最终文件列表显式 stage，建议提交 `fix(editor): handle vim character arguments before counts`。若执行流程统一最终提交，则保留独立可审查 diff 单元，不强行拆坏未完成的共享修改。

## 3. 单元二：跨连接 Redis 保存闭环

**Files**
- Modify: `src/app.rs` — SaveRequested/RedisValueSave/RedisValueSaveAnyway、Redis table/object 操作入口、ensure_redis_browser_loaded。
- Test: `tests/redis_loading_lifecycle.rs`。
- Create: `tests/redis_value_connection.rs` — 本次跨连接端到端 reducer 回归，避免扩大混合测试文件。
- Test: `tests/redis_object_editor.rs`、`tests/redis_browser_tabs.rs`（复用相关 fixture/断言）。
- Conditional modify/test: `src/runtime.rs` — 仅在实际回归暴露 target 选择问题时使用既有 active_database_for_target，不为名称含 active 而重写连接池。

### Step 1：建立两个已连接会话的失败用例

用真实 ConnectionProfile、ExecutionTarget、SessionRegistry 注册/成功事件建立 Redis A 与 SQL B；SQL profile 可用 SQLite，避免 lssc-uat 外部依赖。Redis A keyspace 已加载，value page 为完整 String，预览已编辑。

先按正常 UI Action 打开/切换 B，再通过正常 tab 导航回 A；不要只赋 active_tab 来绕过 prepare_active_tab。触发实际保存快捷键和确认 Action。

断言：
- loaded tab 回来不会覆盖草稿、不会重新扫描；连接上下文正确恢复。
- 输出 PlanRedisMutation.request.connection 等于 A identity；request.key.target 等于 A target；operation 是替换后的 String，TTL 保留。
- 可再设置全局连接为 B，然后从已归属 A 的确认框提交，验证保存自身也不依赖前台恢复这个副作用。

Run: `cargo test --test redis_value_connection -- --nocapture`

Expected：旧代码在连接归属断言失败；无真实数据库连接需求。

### Step 2：统一完整 target 的会话解析

在 App 增加小型私有解析函数（建议 `redis_connection_for_target`），输入 `&RedisTarget`，返回可用 identity 或明确不可用结果。复用 SessionRegistry.get 和 SessionStatus::Connected，校验 identity.profile_id 与 target 对齐。使用同一函数处理保存开始和最终确认，不分散复制条件。

确认时从 overlay.tab_id 找到确切 Redis tab/page/editor；检查 opened key/page target 一致，校验已捕获 revision/owner 是否仍有效。若并行弹窗任务已提供 RedisSaveSnapshot，直接在该 snapshot 归属逻辑接入解析，避免两套所有权状态。

不可用时给出目标连接不可用的反馈并保留草稿；不得将全局 SQL identity、任意同 profile 会话或 generation=0 写入请求。沿用现有异步连接准备，不无条件自动重放旧保存。

### Step 3：已加载 tab 也准备目标连接

重排 ensure_redis_browser_loaded：先确认/恢复 tab 目标连接，再判断 needs_scan；已连接 session 复用原 request_connection_target 路径，未连接按既有 Connect/pending target 流程推进。

仔细处理 request_connection_target_inner 再次进入 ensure 的路径：目标已正确激活后必须直接走不需连接分支，不能递归请求。Loaded/Complete/Partial 等状态不扫描，不改变 editor buffer/baseline/undo history。

增加 lifecycle 回归：loaded tab + 已有异目标会话、CompleteEmpty、Partial；断开后 reconnect pending 不重复 Connect，成功后保留草稿。保留既有 unloaded/offline 场景。

### Step 4：同源写入口统一归属

替换 RedisPreviewEdit/Table add/delete、open_redis_object_create/edit 中依赖全局 identity 的逻辑，使用已选定 target 解析。删除 table edit 的 generation=0 fallback。仅修改该根因涉及的连接选择，不重构表格编辑或弹窗布局。

检查只读 profile 使用目标 profile，而非前台 B；完整 value/截断限制和格式转换保持现有语义。

### Step 5：完成保存成功/失败的闭环测试

在新测试文件注入匹配 PlanReady，断言 ExecuteRedisMutation 仍携带 A connection；注入成功结果，检查 A baseline/dirty 与刷新命令归属，B tab 内容不变。失败则 A 仍 dirty 且可重试。

补边界：A/B 都是 Redis、同 profile 的 database 0/3、连接 generation 失效、确认期间 owner 已变化、只读 A + writable B。使用少量参数化 fixture，避免每个组合都连真实服务。

如果保存确认可切换 key，必须拒绝陈旧确认；若当前 modal 已禁止此行为，则通过可达的切 tab/连接变化验证 owner，避免制造不可能 UI 状态驱动宽泛重构。

Run:
```bash
cargo test --test redis_value_connection --test redis_loading_lifecycle --test redis_unsaved_changes
cargo test --test redis_object_editor --test redis_browser_tabs --test redis_mutation
```

Expected：全部非 ignored 定向测试通过。mutation 的真实 Redis ignored 测试另计，不能用 reducer 注入成功描述为实际服务写入成功。

### Step 6：Luna 单元审查并提交

追踪 tab → request → runtime → adapter → callback；检查 full target、generation、request_id、dirty 保留和 B 未变。建议提交 `fix(redis): bind value saves to the tab connection`，显式 stage 实际修改文件。

## 4. 完整验证与收尾

### 验证分级

- **用户需求验收**：`r2` 无需第三键即时生效；打开其他连接表格再切回 Redis 后，编辑保存到正确 Redis target。这两项业务结果必须完成。
- **项目强制门禁**：下列 CONTRIBUTING.md 的 fmt、clippy、test，以及既有异步 owner/generation、终端文本清洗和只读限制等工程规则。
- **本计划选用的自动化回归**：前述编辑器、App reducer 与加载生命周期测试用于提供业务结果的可重复证据。允许 Luna 按实际代码结构调整测试组织，但不能省略其对应行为断言。
- **补充建议验证**：真实服务读回、PTY/人工观察不属于用户额外指定的强制门禁；可用时补充，不可用时记录限制，由 Luna 收尾审查决定是否还需补证。

### 强制门禁（项目 CONTRIBUTING.md）

功能齐备后统一执行一次：
```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

每条记录命令、退出码、测试数量/跳过项、HEAD/diff 状态、Rust 版本及环境。普通编译/测试错误自行修复；只有相关代码或环境变更才重跑对应检查，最终确保记录对应最终代码。CI Rust 任务使用 1.94.0，本机已确认 1.94.1；不要把不同工具链结果说成同一次 CI 结果。

### 补充证据

- 在可用的一次性测试 Redis 上复现：打开 String → r2 第二键立即显示 → 打开其他连接表格 → 切回 → 保存 → 读回原 Redis key，确认其他 target 不变。
- 现有数据库 CI 的 Redis 命令为 `cargo test --locked --test redis_mutation -- --ignored --nocapture --test-threads=1`，需要 LAZYDB_TEST_REDIS_URL；没有配置时记录未执行。不要访问用户真实 lssc-uat 来充当测试 fixture。
- PTY/人工验证用于补充真实终端显示证据，不自动升级为用户强制验证。环境限制最多一次针对性修复重试，再由 Luna 收尾审查决定补证或记录限制。

### 最终验收清单

- [ ] r2 第二键后文本、revision、dirty 和显示一致；下一键无延迟补写。
- [ ] count、字符参数、undo/redo、只读及 SQL 共用编辑行为通过。
- [ ] loaded Redis → SQL → Redis 保存归属 A，完整 plan/execute/success 或 failure 闭环通过。
- [ ] 相同 profile 不同 database、陈旧 generation、确认 owner 变化不会串写。
- [ ] loaded tab 连接准备不重复扫描、不覆盖草稿、不形成递归。
- [ ] 同源 table/object 写入口不再生成错误/伪造连接身份。
- [ ] 最终代码通过项目检查；环境受限检查明确记录，没有借用分析阶段通过结果。
- [ ] Luna 完成 diff 审查、必要纠偏与任务流程规定的提交合并；保留其他任务工作。

## 5. 当前计划阶段证据

本阶段未改变业务代码，未重复运行分析阶段测试。实际读取当前 git status/HEAD、CONTRIBUTING.md 与 CI 门禁；tracked diff 起初为空，HEAD 仍为指定起点。分析阶段 24 项定向通过只作为基线背景，后续实现必须有自己的测试结果。

下一步由 Luna 在分配工作树中，从单元一的 `redis_value_replace_digit_is_immediate` 失败回归开始推进，再完成单元二与统一门禁。
