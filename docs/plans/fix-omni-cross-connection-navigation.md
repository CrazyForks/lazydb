# Omni Connected Relation Navigation Implementation Plan

> **执行者：Luna。** 按以下闭环逐项实施、审查、纠偏并完成提交合并；Astra 仅负责分析和计划。本计划不要求启动子 Agent，也不要求用户反复 resume。

**Goal:** 从 Redis 或其他连接通过 Omni 选择已连接 SQL profile 的表时，一次确认即可打开准确的表，同时正确接续正在建立的连接并保留真正的切换保护。

**Architecture:** 保留现有 SessionRegistry、request_connection 和 relation 加载管线。在 relation 导航入口根据请求后的连接状态判断同步就绪、异步等待或拒绝，不再根据是否返回新 Connect 命令推断成功。异步导航继续通过 profile + generation 校验。

**Tech Stack:** Rust 2024 / Rust 1.94、现有 App reducer、Cargo 集成测试；无需真实数据库即可验证导航状态机。

---

## 0. 基线、范围及执行约束

- 仓库：`/Users/yelog/workspace/tui/lazydb`；目标分支 `main`。
- 分析起点：`92c6bf758a3f3beaae74f3fb80a6f1985301e27a`。
- 分析：`.git/opencode-tasks/ses_f51380958ffeRuSYdPpL1V0YEO/analysis.md`。
- 记录验证到同目录 `validation.md`，标明实际命令、退出状态、HEAD/diff 及环境。
- 任务/分支名称由 Luna 在计划完成后自动确定；本阶段没有创建分支或工作树。
- 执行前核对 HEAD 和实际 diff，保留已有未跟踪文件；在任务分支实施。若使用新 worktree，确认计划和任务记录可访问，不把其他任务文档或状态目录一并提交。
- `checkpoint.json` 在分析和计划读取时均不存在；不要主动创建它，也不要修改 `state.json`。本轮唯一回执为任务目录中的 `plan-604d7299-017a-4e1d-84fb-7de042d4a09a.json`，token 为 `604d7299-017a-4e1d-84fb-7de042d4a09a`；保留历史回执。
- 必改范围：`src/app.rs`、`tests/omni_navigation.rs`、`docs/omni-bar.md`。
- 本次只闭环 relation 导航。`navigate_to_console` 和 `return_to_previous_location` 中同型 Connect 判断已在分析中标记，不在本修复中顺带重构。

## 验收与验证的级别

| 类别 | 内容 | 完成判定 |
| --- | --- | --- |
| 用户需求，必需 | 按 SQL → Redis → Omni 选原 SQL 表的顺序，一次确认打开正确表 | 自动化 reducer 回归验证活跃 tab、完整 RelationKey、会话归属及无错误警告 |
| 本修复必要回归 | 新建连接、已有连接中请求、真实拒绝条件、过期事件、已有标签和 Data/DDL 视图 | 下列各单元定向测试通过；不能因修复首次打开而破坏原行为 |
| 项目强制门禁 | `.github/workflows/ci.yml` 中 Rust 1.94.0 fmt、clippy、all-targets/all-features test | 收尾一次执行并记录实际结果；未运行或环境受限不得标记通过 |
| 项目 CI 的其他作业 | 平台安装器、发行契约、macOS 依赖及外部数据库服务矩阵 | 保留现有 CI 要求，由对应环境执行；本次不另加本地手工替代门禁 |
| 补充建议 | 真实实例或 PTY 操作、截图核对 | 有环境则提供证据，缺环境记录限制；不自动提升为用户必需门禁 |

对环境受限检查最多一次有针对性的修复重试；之后由 Luna 收尾审查判断补证或记录限制。项目强制门禁的未执行状态必须如实保留，补充人工检查不可使工作流无限停留在 progress。

## 1. 需要理解的现有契约

| 位置（起点版本行号） | 契约 |
| --- | --- |
| `src/app.rs:3305–3311`、`3413–3415` | Omni 直接结果和语义命令共用 navigate_to_relation |
| `src/app.rs:3464–3534` | 从目标 catalog 构造 descriptor；当前错误地只检查 Connect 命令 |
| `src/app.rs:3565–3592` | open_relation_descriptor 复用完整 RelationKey、激活 tab 和加载视图 |
| `src/app.rs:16224–16295` | request_connection 验证 profile、查询/事务保护并确定 target |
| `src/app.rs:16313–16418` | 已连接会话同步激活；连接中会话 single-flight；只有新请求发 Connect |
| `src/app.rs:11348–11376` | pending navigation 只由匹配 profile/generation 的成功事件消费 |
| `src/model/session.rs:91–109` | Existing 同时包含 Connected 和 Connecting |
| `tests/omni_navigation.rs` | 现有异步测试及 add_table 辅助函数，可复用 |

关键断言不是“返回命令非空”，而是“一次确认后目标 relation 是活跃 tab，且加载归属正确会话”。读取最新源码再应用下文片段；行号用于定位，不作为盲目补丁偏移。

## 单元一：已连接 SQL → Redis → SQL，一次确认打开表

**文件：** 修改 `tests/omni_navigation.rs`；修改 `src/app.rs::navigate_to_relation`。

### 步骤 1：构造真实状态转换的回归

新增测试 `omni_opens_cached_relation_from_redis_using_connected_session`：

1. 创建 `sqlite::memory:` SQL profile 和 `redis://127.0.0.1:6379/0` Redis profile。它们仅用于 reducer fixture，不执行返回的网络命令。
2. 用 `Action::RequestConnect` 获取 SQL Connect generation，投递匹配 `Action::ConnectionSucceeded`；用现有 `add_table` 注册 agreement。SQLite fixture 的 database/schema 应保持 `:memory:`/`main`。
3. 请求 Redis 连接并投递匹配成功事件。ServerInfo.kind 必须为 Redis、database 为 `0`。根据实际入口建立/激活 RedisBrowser，覆盖用户从 Redis 面板发起操作的场景；不要仅伪改 `connection.profile_id`。
4. 调用 `Action::OpenOmni`，确认目标 CatalogId 存在于候选；设置该选择，执行一次 `Action::OmniConfirm`。
5. 断言 active tab 是 agreement 的 Relation、view 为 Data、focus 为 Results、Omni 已关闭，SQL profile 与原 SQL generation 被复用。
6. 断言未对原 SQL ExecutionTarget 重发 Connect；表加载请求的 relation/profile/connection identity 均属于 SQL，而非 Redis。允许目录/元数据等附带命令，不能要求命令向量只有一项。
7. 对比操作前后的通知，断言没有新增错误的 `Cannot switch connections while a query or unresolved transaction is active`。

可为本测试文件新增 `connect_profile` 辅助函数，封装 RequestConnect → 查找匹配 Connect → ConnectionSucceeded；从 profile 构造正确 ServerInfo，不沿用固定 SQLite 的 server helper 为 Redis 构造错误元数据。命令匹配使用 iter/find_map，而不是单元素切片。

### 步骤 2：确认基线失败

```sh
cargo test --test omni_navigation omni_opens_cached_relation_from_redis_using_connected_session -- --exact
```

预期：测试编译成功，活跃目标表/表加载断言失败；不能把 fixture 的 panic、错误 target 或服务不可用当作成功复现。记录首次失败证据。

### 步骤 3：修复同步完成路径

把 `navigate_to_relation` 中 `let commands = self.request_connection(profile_id);` 改为 mutable，并在原 Connect 检索之前插入：

```rust
let mut commands = self.request_connection(profile_id);
if self.connection.profile_id == Some(profile_id)
    && self.connection.status == ConnectionStatus::Connected
    && self.active_workspace_profile == Some(profile_id)
{
    commands.extend(self.open_relation_descriptor(descriptor, view));
    return commands;
}
```

暂保留之后的异步分支以完成首个闭环。此处必须 append 原命令，不能丢失同步切换产生的 metadata/加载命令；也不能绕过 request_connection 提前激活 workspace。

### 步骤 4：验证本闭环

```sh
cargo test --test omni_navigation
```

预期：新增 Redis 回归与原异步测试均通过。核对在旧 relation 已活跃的 workspace 恢复过程中，不重复创建同一 RelationKey 的 tab，不错误取消目标请求。

## 单元二：single-flight 接续及拒绝/过期事件保护

**文件：** 修改 `tests/omni_navigation.rs`；修改 `src/app.rs::navigate_to_relation`。

### 步骤 1：补充有区分度的行为测试

- `omni_waits_for_existing_connection_attempt`：SQL 目标已缓存 catalog，但 SessionRegistry 中已有 Connecting attempt。通过公开 sessions API（若不可访问则测试放在 app.rs 内部模块）注册 attempt，不投递成功。由其他连接通过 Omni 选表，断言不重发 Connect、不提前激活表、不误报拒绝；投递同 identity 成功后激活表。
- `omni_ignores_unrelated_connection_success`：在上一场景中先投递其他 profile 或错误 generation 的成功事件，断言不打开目标；再投递正确事件才打开。复用或扩充现有 `selecting_cross_profile_relation_resumes_only_after_matching_connection_success`，避免重复相同覆盖。
- `omni_does_not_open_relation_when_switch_is_blocked`：通过已有测试 fixture 设置运行中查询/加载或未解决 relation transaction，确认一次 Omni 导航保留原活跃会话、不开目标、不发 Connect，保留真实拒绝通知。关系事务与 running 至少各一例，优先使用现有测试构造方式。
- 失败事件场景：目标连接失败后不打开表；失败后新 generation 的重试只能由新成功事件完成，不能被旧成功事件打开。

这些测试验证行为，不依赖 private pending_navigation 字段；可通过 success/failure 后的活跃 tab 和发出的命令推断导航是否接续。

### 步骤 2：确认 Existing Connecting 回归失败

```sh
cargo test --test omni_navigation omni_waits_for_existing_connection_attempt -- --exact
```

预期：当前代码没有记录 pending navigation，匹配成功后未打开表。若测试 fixture 未生成合法 attempt，先修正 fixture。

### 步骤 3：改为请求后状态识别异步身份

用以下状态推导替换 `commands.iter().find_map(Command::Connect ...)`；保留现有 PendingNavigation 构造和返回命令：

```rust
let generation = (self.connection.status == ConnectionStatus::Connecting
    && self.connection.pending_profile_id == Some(profile_id)
    && self
        .connection
        .pending_target
        .as_ref()
        .is_some_and(|target| target.profile_id == profile_id))
    .then_some(self.connection.pending_generation)
    .flatten();
let Some(generation) = generation else {
    return commands;
};
self.pending_navigation = Some(PendingNavigation {
    profile_id,
    generation,
    intent: crate::commands::UserIntent::OpenRelation {
        catalog_id: id,
        view,
    },
    descriptor: Some(descriptor),
});
commands
```

Rejected 分支保留 request_connection 发出的具体通知和 connection.error，移除旧的统一“查询/事务受阻”错误推断。检查请求前即存在 pending state 的情况：若保护拒绝了本次请求，不应仅因遗留 pending profile 碰巧相同而创建导航。若定向测试证明现有状态字段不足以区分拒绝和合法接续，使用最小的本地请求结果标记/小型 helper 明确报告 accepted/ready/pending，不以命令列表或通知数量推断；不要重构所有连接调用者。

保留 `ConnectionSucceeded` 的现有 profile + generation 校验，不合成事件、不强制 reconnect、不引入延迟或自动重复 OmniConfirm。新请求应替代旧导航意图，不能留下能被晚到成功事件触发的过期打开；用事件测试确认这一点，必要时在新导航受理边界清理旧 pending navigation。

### 步骤 4：定向验证

```sh
cargo test --test omni_navigation
```

预期所有异步、阻止和首次打开测试通过。普通编译或断言错误由 Luna 修复，不作为外部阻塞。

## 单元三：身份复用、视图动作及文档验收

**文件：** 修改 `tests/omni_navigation.rs`；修改 `docs/omni-bar.md:29–32`。

### 步骤 1：补齐两个直接受影响的分支

1. 参数化已连接来源：除 Redis 外用另一个 SQL profile，确认没有 Redis 特判依赖。
2. 目标 relation 已存在时保留原 tab id，切回后仅一个相同 RelationKey 的 tab；通过 `OmniShowActions`/DDL 项或语义 ShowRelationDdl 命令打开 DDL，断言 view 为 Ddl。同名表分属不同 profile 时不能选错。

尽量复用连接和 CatalogId fixture。已有 relation 若处于 Loading，应先投递匹配完成事件或按现有测试方法构造完成状态，避免被正确的 running-load 保护拦住而误判修复失败。

### 步骤 2：更新文档说明

将 `docs/omni-bar.md` 跨连接打开段落更新为：

```text
Opening a cached relation on another profile switches to that profile and
opens the exact relation. An already-connected session is reused immediately;
otherwise navigation waits for the matching connection attempt, including an
attempt already in progress. A running query or an unresolved transaction may
block the switch; Omni reports that condition instead of silently interrupting
database work.
```

保留紧接着的 offline filter / remote search 说明。

### 步骤 3：一次运行相关集成测试

```sh
cargo test --test omni_navigation --test omni_flows --test omni_resume --test omni_search --test omni_providers --test omni_input
```

预期全部通过；验证记录写入当前代码状态，不能引用分析阶段 6/6 当修复验收。

## 收尾：项目验证、Luna 审查与提交

功能齐备后按 `.github/workflows/ci.yml:81–83` 一次执行 Rust 检查：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
git diff --check
```

各命令期望退出 0。如 Rust 1.94.0 不可用，先确认工具链并记录真实环境；不能把别的工具链结果写成指定版本通过。外部数据库测试的跳过/缺少服务应单独记录；这些 reducer 测试无需真实 Redis/UAT。

Luna 审查重点：

- 首次 Redis → SQL 表导航确实激活准确对象，而不仅 connection.profile_id 改变。
- Ready/Pending/Rejected 均覆盖，没有绕过查询/事务保护。
- 原请求命令未被丢弃，已有 relation tab 无重复创建，view/focus 正确。
- stale success/failure 不消费错误导航，合法 Existing Connecting 不重复发 Connect。
- 改动局限在本任务，未把其他任务文件或插件状态提交。

人工/PTY 复现是补充验证：有可用测试实例时按用户顺序操作一次并记录结果；环境受限最多一次针对性修复重试，再记录限制，不无限 progress。它不替代 reducer 回归，也不要求访问真实 UAT 凭据。

完成上述闭环后由 Luna 做逻辑提交并按工作流合并 main。建议提交信息：`fix(omni): resume relation navigation on reused connections`。精确暂存本任务源码、测试、文档及本计划，禁止 `git add .` 混入既有未跟踪文件。

## 本计划阶段的完成证据

- 完整计划保存于本文件及 `.git/opencode-tasks/ses_f51380958ffeRuSYdPpL1V0YEO/plan.md`，业务代码及测试未修改。
- 已复查当前 git 状态与 CI 要求；checkpoint 仍不存在。
- 没有重新运行未变化业务代码的测试。分析阶段的六项基线结果仅保留为历史记录。
- 下一阶段由 Luna 自动命名任务并实施单元一，不需要用户选择执行方式。
