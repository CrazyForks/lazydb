# Redis Collection Preview Clean-State Implementation Plan

> **执行分工：** 本计划由 Astra 编写；实现、审查、纠偏、提交合并由 Luna 完成。只执行当前被分配的阶段，不启动子 Agent。遵循任务插件提供的当前阶段回执路径，不复用 analyze 回执。

**Goal:** Redis 集合仅浏览后切换 key 或退出不弹未保存提示，同时保留真实 String 编辑的保护。

**Architecture:** 让文本编辑基线与一次打开值的预览生命周期绑定。在开始新预览时清除旧基线，在集合加载的只读分支保持无文本基线；保持现有统一 dirty 判定与 Table mutation 路径。

**Tech Stack:** Rust、App reducer、现有 editor、Cargo integration tests；核心回归无需 Redis 服务或 PTY。

---

## 上下文与约束

- 起点与目标：`fe3134adf47b8002cf88cc467f251a71585fb3bb`，目标 `main`。
- 详细分析：原工作区 `.git/opencode-tasks/ses_f521f8629ffe12NWdTLM86zZ99/analysis.md`。
- 任务名/分支由后续 Luna 或插件命名；本计划不创建分支或 worktree。
- 原有未跟踪 `.git-opencode-tasks/`、`docs/plans/2026-09-17-mariadb-catalog-compatibility.md` 不纳入提交。
- checkpoint.json 在分析及计划时均不存在；不要写入或推断 checkpoint/state 状态。
- 本轮完整计划交付路径：原工作区 `.git/opencode-tasks/ses_f521f8629ffe12NWdTLM86zZ99/plan.md`；本文件与其内容一致。
- 本轮 plan 回执仅写 `plan-c5826e4a-75fa-4642-84c3-40fb6c28c698.json`，token 为 `c5826e4a-75fa-4642-84c3-40fb6c28c698`；后续阶段使用各自最新指令的回执，不复用此路径。

### 验收与验证等级

| 等级 | 内容 | 完成要求 |
| --- | --- | --- |
| 用户需求验收 | 集合仅浏览后换 key/退出不误报；真实修改仍受保护 | 必须实现，以 Task 1–3 的 App 级行为回归提供证据 |
| 本修复自动化回归 | 五种集合、基线生命周期、过期结果、真实 String 编辑保护 | 作为本计划的代码回归要求执行；属于实现验证，不是要求用户人工操作 |
| 项目强制门禁 | CONTRIBUTING.md 的 fmt、all-targets/all-features clippy 和 test | 功能齐备后执行，记录真实结果；受环境限制不能冒充通过 |
| 补充建议验证 | 真实 Redis/PTY 人工浏览；关闭 tab/断开连接的额外行为测试 | 非必需门禁；按环境和审查需要补充，不因缺少人工证据自动阻塞 |

所有验证记录写入指定任务目录 `validation.md`，包括命令、退出结果、相关文件、代码版本/工作区状态和环境。人工/PTY 环境问题最多一次针对性修复重试，再由 Luna 收尾审查决定是否补充证据或记录限制。普通编译和业务测试失败自行修复，不归类为等待用户的阻塞。

### 已确认的缺陷与调用点

`RedisBrowserTab::open_key` 更新 key/generation/Loading，却保留 `value_edit_baseline`。`Action::RedisValuePageLoaded` 的 String 分支建立 baseline，集合分支仅替换 editor 文本。String → 集合之后，旧基线与集合文本不同；切 key、退出、关闭 tab、断开连接共用 `value_is_dirty`，因此误报。

实际生产代码的 `open_key` 调用点只有两处：

1. `src/app.rs:19954–20029`，`open_redis_key`：先检查 dirty，再开始预览。清理放在 model 方法中不会绕过这里的真实 dirty 保护。
2. `src/app.rs:20376–20420`，`apply_redis_mutation`：mutation 成功后重新加载结果。这是新预览生命周期，应清理旧基线。现有代码已在此替换打开的值，本次不重构异步 mutation owner 协调。

`clear_opened_key` 已清理 baseline/revision，保持其行为即可。`src/model/redis_value_edit.rs` 的 draft 状态机未用于当前误报链路，不修改。

## 单元划分

仅一个业务验收单元：**只读集合始终 clean，真实 String 编辑仍被保护**。以下任务是该单元的实施步骤，不是相互独立的功能发布。完成定向验证后继续全量检查和收尾，不要求用户反复 resume。

### Task 1：建立 App 级失败回归

**Files:**
- Modify/Test: `tests/redis_unsaved_changes.rs`
- Reference: `tests/redis_loading_lifecycle.rs:15–25`
- Reference: `tests/redis_browser_tabs.rs:108–175`
- Reference: `src/db/redis/read.rs:7–73`

**Step 1 — 增加最小可复用 fixture。**

沿用现有 App/editor 测试风格，新增 helper，保持已有 `dirty_fixture` 和两项 Cancel/Discard 测试。

- `App::new(Vec::new())`，设 connection.profile_id、generation、ExecutionTarget(database="0")、ConnectionStatus::Connected。
- 加入一个 RedisBrowserTab，tree 包含 `string`、`collection`、`next` 三个叶 key；设置 active_tab、overlay=None。
- 加载 helper 必须调用 `app.open_redis_key(tab_id, KeyTreeNodeId::Key(...))`，再读取该 tab 最新 generation，派发 `Action::RedisValuePageLoaded`，connection 使用 `app.connection.active_identity().unwrap()`。
- page.metadata.key 必须匹配打开的 key；使用 Persistent TTL、Complete position、complete=true、truncated=false，字节计数设为 fixture 实际负载大小。
- 用 `RedisPageValue::String(b"baseline".to_vec())` 首先加载 String，确认 baseline 已建立且与 `app.editor.text(preview_editor_id)` 相同。
- 不调用编辑 action，不直接给集合的最终 baseline 赋值；必须经过生产 PageLoaded 分支。

集合用例数据可直接使用：

```rust
fn collection_cases() -> Vec<(RedisType, RedisPageValue)> {
    vec![
        (RedisType::Hash, RedisPageValue::Hash(vec![(b"field".to_vec(), b"value".to_vec())])),
        (RedisType::List, RedisPageValue::List(vec![(0, b"value".to_vec())])),
        (RedisType::Set, RedisPageValue::Set(vec![b"member".to_vec()])),
        (RedisType::SortedSet, RedisPageValue::SortedSet(vec![(b"member".to_vec(), b"1".to_vec())])),
        (RedisType::Stream, RedisPageValue::Stream(vec![(
            b"1-0".to_vec(),
            vec![(b"field".to_vec(), b"value".to_vec())],
        )])),
    ]
}
```

**Step 2 — 增加两个参数化行为测试。**

- `collection_preview_after_string_allows_key_switch_without_unsaved_prompt`：每种集合独立 App；加载 String → 集合，断言自动 Table、`value_is_dirty(editor_text)==false`，然后打开 next，断言 opened_key 已是 next，overlay 不是 RedisUnsavedValueConfirm。
- `collection_preview_after_string_allows_quit_without_unsaved_prompt`：每种集合新建独立 App；String → 集合后调用 `Action::Quit`，断言没有 RedisUnsavedValueConfirm、`app.should_quit` 为 true、返回包含 `Command::Quit`。fixture 不设置事务、运行查询或工作区退出确认，以免其它退出条件混入。

可以在上述两个测试中附加 baseline=None 的诊断断言，但不能只测字段而不测导航/退出结果。每次断言添加类型上下文，便于定位失败用例。

**Step 3 — 运行并记录预期失败。**

```bash
cargo test --test redis_unsaved_changes collection_preview_after_string
```

预期：原代码因残留基线/dirty 或 unsaved overlay 断言失败。若编译或 fixture 出错，先修正测试，再取得业务断言失败；编译失败不能作为缺陷复现证据。命令和真实输出记入任务 `validation.md`，不要覆盖之前记录。

### Task 2：修复生命周期，完成核心闭环

**Files:**
- Modify: `src/model/redis_browser.rs:262–275`
- Modify: `src/app.rs:13120–13130`
- Test: `tests/redis_unsaved_changes.rs`

**Step 1 — 新生命周期清理基线。**

在 `RedisBrowserTab::open_key` 设定新 opened_key 的同一流程中加入下列两行；不放在 `different_key` 条件内，因为同 key 重开也增加 generation。

```rust
self.value_edit_baseline = None;
self.value_edit_revision = 0;
```

补一条简短注释，说明文本 baseline 属于本次 preview，不应跨 key/generation 复用。不要重置其它无关状态，不更改 `value_is_dirty` 判定。

**Step 2 — 集合安装保持只读、无文本基线。**

将 PageLoaded 内现有 String/集合编辑器安装分支保持为：

```rust
if matches!(
    page.value,
    crate::db::redis::read::RedisPageValue::String(_)
) {
    tab.value_edit_baseline = Some(text.clone());
    self.editor.open_value(tab.preview_editor_id, &text);
} else {
    tab.value_edit_baseline = None;
    tab.value_edit_revision = 0;
    self.editor.open_read_only(tab.preview_editor_id, &text);
}
```

String 建基线和编辑器初始化保持现状。不要给集合设置 `Some(text)`，因为 editor session 初始化还用 baseline 是否存在判断可编辑性。

**Step 3 — 运行核心回归。**

```bash
cargo test --test redis_unsaved_changes
```

预期：新增集合换 key/退出回归与原有真实 String Cancel/Discard 测试全部通过。记录本次结果及业务 diff，不能复用分析阶段的 2 passed 结果。

### Task 3：补齐生命周期边界与真实修改保护

**Files:**
- Modify/Test: `tests/redis_unsaved_changes.rs`
- Reference: `src/app.rs:13469–13505`（失败 action）
- Reference: `src/app.rs:13187–13223`（切预览格式）

**Step 1 — 用同一 fixture 验证过渡状态。**

新增定向测试覆盖以下流程，避免为每种集合重复全部边界用例：

1. String → 新 key Loading：baseline=None、revision=0；派发该新 key 的 PageFailed 后仍 clean；可以继续打开 next。
2. 记录旧 String generation，开始集合加载后派发旧 generation 的 String PageLoaded：不得恢复旧 baseline；随后当前 generation 集合 PageLoaded 正常接受。
3. 首次直接打开集合 → 另一集合 → String：只读阶段 clean，最后建立新 String baseline，与 editor 当前文本一致。
4. clean String 重开同 key：开始新 generation 时基线清理，收到对应 String page 后重建一致基线。

**Step 2 — 验证只读展示变化。**

使用 List 构造两页，首个 page complete=false/ListOffset，派发下一页时仍用当前 generation，确保追加后 baseline=None、dirty=false。通过格式菜单 action 选择 RAW，再检查 clean 并可切 key；菜单索引从 `model::redis_preview::FORMATS` 获取，避免写死易变下标。这样覆盖集合浏览文本变化而非模拟用户编辑。

**Step 3 — 验证真实 String 不被提前清除。**

沿用已有 `dirty_fixture` 的 EditorKey/EditorPaste 路径，记录 baseline、opened_key、editor text 和 generation。请求打开另一个 key 后应弹 RedisUnsavedValueConfirm，上述状态仍保留；Cancel 后再次请求仍弹窗。补一个独立 dirty String Quit 测试：弹窗且 should_quit=false、不返回 Quit。现有 Discard 回归继续证明丢弃后可到达目标 key。

无需为两个新增清理赋值编写仅镜像实现的模型测试；上述 App 级过渡和行为测试就是语义验证。

**Step 4 — 一次性运行关联测试。**

```bash
cargo test --test redis_unsaved_changes --test redis_browser_tabs --test redis_loading_lifecycle --test redis_object_editor
```

预期全部通过。若新失败暴露与本次 diff 有关的问题，修复后重跑受影响测试；不要每次小修改都跑全量套件。

### Task 4：Luna 收尾审查、项目检查与交付

**Files:**
- Review: `src/model/redis_browser.rs`, `src/app.rs`, `tests/redis_unsaved_changes.rs`
- Update: 任务目录 `validation.md`
- Include when committing: 本计划文件及上述明确相关业务/测试变更

**Step 1 — 审查实际 diff。**

确认：

- dirty 保护仍在调用 model.open_key 之前；真实修改不会被清理绕过。
- 同 key 刷新和成功 mutation 重载属于新 generation；没有将只读集合变为可编辑文本。
- 没有修改 Redis 命令、Table mutation 操作、保存语义或未接入的 draft 状态机。
- 没有把多 tab Quit、String 格式切换或大 String 可编辑能力的邻近问题顺带扩展为本次重构。
- fixture 真正经过 PageLoaded，而不是手动清基线后自证正确；断言包含导航/退出实际结果。

**Step 2 — 完成 CONTRIBUTING.md 的项目检查。**

先按需运行 `cargo fmt` 格式化实际修改，再执行：

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

预期各命令 exit 0；记录实际退出码、相关工作区版本和环境。若修改代码修复检查失败，只重跑受影响检查；最终结果须对应最终代码。不要使用 skip、禁用 feature 或放宽 lint 来冒充项目要求已满足。

**Step 3 — 有条件补充人工检查。**

真实 Redis/PTY 是补充，不是当前需求的强制环境验证。已有环境可用时手工浏览 String → Set/List/Stream → next，及退出，确认不弹 unsaved；另外修改 String 确认真正保护仍在。若环境限制，最多一次针对性修复重试，再记录限制并由 Luna 收尾审查判断已有 App 级证据是否充分，不无限尝试。

**Step 4 — 交付。**

同一业务单元用一个聚焦提交即可，建议消息：`fix(redis): clear stale edit baseline when opening previews`。按当前流程获准进入提交阶段后使用 @git-commit 技能，仅精确 stage 相关路径，不能 `git add .`。分支命名、合并与工作区选择由 Luna/插件的该阶段指令决定。

阶段完成后只写该阶段最新指令给出的回执路径/token；不写 state.json/checkpoint.json，不覆盖历史回执。最终说明根因、修复、实际测试结果及必要环境限制。

## 完成标准

- 五种集合的 String → 集合 → 其它 key/退出均不误报。
- Loading/Failed/过期结果、分页、只读格式变化不继承旧文本编辑状态。
- 新 String 基线正确，真实 String 修改的 Cancel/Discard/退出保护仍有效。
- 关联测试及项目强制检查结果有当前代码版本对应的真实记录。
- 实际修改范围保持为生命周期修复与行为回归；用户无须额外提供普通实现决策。
