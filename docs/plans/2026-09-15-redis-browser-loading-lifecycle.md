# Redis Browser Loading Lifecycle Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行适配：若环境没有该技能，按本文任务顺序和检查点执行。接口与测试名称为拟定名称，实施前按最新源码调整。本文件是实施计划，不表示功能已经完成。

**Goal:** 用户重新打开 Redis 连接后，恢复的活动数据库 Tab 自动加载 keys；鼠标、键盘及导航激活其他数据库 Tab 时按需加载，整个过程目标正确、请求幂等、状态清晰。

**Architecture:** 保留 Action → App::update → Command → Runtime 边界，统一 Tab 激活后的准备入口。Redis Tab 持有瞬态会话等待意图，按精确 ExecutionTarget 复用会话，连接完成按 Tab 身份续接；复用 KeyspaceState 的扫描与回填校验，并对首屏空批次增加有限预算续扫。

**Tech Stack:** Rust 1.94 / edition 2024、Tokio、Ratatui、SessionRegistry、Redis SCAN、现有 Rust 集成测试；无需新增生产依赖。

---

## 1. 产品契约与验收目标

1. 仅反序列化工作区不发起连接；用户打开 Redis 连接并恢复活动页后，自动准备该页目标并加载。
2. 默认活动页优先：恢复 db0/db1，只自动扫描活动页；另一页首次激活时加载。
3. 点击 Tab、NextTab、PreviousTab、导航到 Tab、关闭活动页后选中相邻页，采用一致的数据准备规则。
4. 自动续接保留用户当前 Tab、焦点与 Explorer 选择；后台完成不能抢回原页面。
5. 精确目标为 profile_id + database + schema=None；不能使用另一个 DB 的活动连接扫描。
6. 同一 Tab 同时最多一个扫描请求；已完成的空库与非空库不因反复激活重复扫描。
7. Failed 仅由明确重试或明确重新打开触发恢复，连接完成/普通切换不形成重试循环。
8. 首屏 SCAN 空批次且游标未结束时，有限续扫；预算耗尽显示 Partial，用户可按 r 继续。
9. Tab 和 pattern 继续持久化；keys、等待意图、请求身份、扫描预算为进程内状态。

### 可见行为

| 操作/状态 | 结果 |
|---|---|
| 默认连接 db0，活动恢复页 db0 | 会话就绪后自动扫描 db0 |
| 默认连接 db0，活动恢复页 db1 | 准备 db1 精确会话，再扫描 db1 |
| 后台恢复页 db1 | NotLoaded，首次激活再加载 |
| 目标会话已连接 | 直接扫描，不依赖再次收到 ConnectionSucceeded |
| 目标会话正在连接 | 登记等待，不重复 Connect |
| 快速 db0 → db1 → db0 | 任务回填各自 Tab，焦点保持在最后选择页 |
| 扫描完成且 keys=0 | 明确显示 No matching keys |
| 连接或扫描失败 | 显示具体失败阶段，r 可重试 |

## 2. 已确认的代码依据

行号仅供定位，以符号名为准。

| 位置 | 当前行为/缺口 |
|---|---|
| `src/app.rs:2354` restore_profile_workspace | 重建 Redis Tab 和 pattern，不发扫描 |
| `src/model/keyspace.rs:54` KeyspaceState::new | keys 为空，NotLoaded，position=Start |
| `src/app.rs:5681–5721` Tab actions | 鼠标 ActivateTab 有 Redis 分支，Next/Previous 没有 |
| `src/app.rs:11094,11263` ConnectionSucceeded | Redis 续接依赖全局 pending_redis_browser_target |
| `src/app.rs:11315` | 连接后补加载 Relation，缺少通用 Redis 补加载 |
| `src/app.rs:15619` request_connection_target_inner | 已连接会话复用分支同步返回，不再发连接成功事件 |
| `src/app.rs:19058,19109` | open_redis_browser 改焦点，ensure_redis_browser_loaded 依赖全局当前连接 |
| `src/app.rs:12855` RedisKeysLoaded | apply_batch 后更新树，没有首屏空批次续扫 |
| `src/app.rs:19493` retry_redis_scan | Partial 分支取全局活动连接，应纳入精确目标准备 |
| `src/runtime.rs:1138` scan_redis_keys | 按请求 connection identity 和 database 寻找会话 |
| `src/ui/redis_browser.rs:459` | 标题只有 loaded 数量，状态主要放面板底部 |

### 与已有计划的接缝

- 阅读 `docs/plans/2026-09-15-relation-preview-loading-lifecycle.md` 与 `docs/plans/2026-09-15-consoles-global-lifecycle-implementation.md`。
- 三者共用一个 Tab 激活后准备入口；如果前两项已经实现，直接扩展 Redis 分支。
- 如果尚未实现，抽取最小分发入口，SQL/Relation/Dashboard 分支委托现有逻辑，保留各自刷新、事务保护和焦点语义。
- 实施前检查 git diff，特别是已有的 `src/ui/mod.rs` 改动；按明确文件范围提交本功能。

## 3. 状态和接口设计

### 3.1 激活入口与加载意图区分

建议由 `prepare_active_tab(reason) -> Vec<Command>` 统一激活后的数据准备，位置历史和焦点仍由调用方维护。

原因至少区分自动恢复/普通激活、明确打开、明确重试。连接完成是按原 Tab ID 续接，不重新执行用户导航动作。

Redis 下层按 Tab ID 工作，例如 `prepare_redis_browser(tab_id, reason)`，不要把可变化的 Vec index 保存到异步等待状态中。

### 3.2 Tab 级会话等待

在 `src/model/redis_browser.rs` 增加瞬态准备状态，建议形状如下；具体复用已有类型：

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RedisBrowserPreparation {
    Idle,
    WaitingForSession {
        target: crate::model::execution_target::ExecutionTarget,
        connection: crate::identity::ConnectionIdentity,
        keyspace_generation: u64,
    },
    Failed(String),
}
```

- 首次打开/恢复页激活建立准备意图；后台恢复页保持 Idle + NotLoaded。
- 已连接：获取精确会话身份并调用 start_scan；不要通过切换全局活动连接完成加载。
- 正在连接：记录身份并等待同一 attempt。
- 需要新建连接：使用 SessionRegistry 的 Started/Existing 语义，仅 Started 发 Connect。
- 连接完成：按 target、connection identity、keyspace generation 与 Tab 存在性匹配等待者。
- 已发起加载后切到后台，允许首个响应回填原页；不因回填触发额外导航。
- 清理全局 pending_redis_browser_target 时，先把所有读取/写入点迁移到 Tab 意图；pending_redis_object_create 的业务意图仍独立处理。
- 只有当前仍有效的显式打开动作可以影响导航；普通异步连接完成不能再调用会强制选页的 open_redis_browser。

### 3.3 扫描状态规则

| 状态 | 普通激活 | 明确重试 |
|---|---|---|
| NotLoaded / Idle+Start | 准备会话并首次扫描 | 首次扫描 |
| Loading | 不重复 | 不并发追加 |
| Partial | 保留进度 | 精确会话就绪后从当前 cursor 继续 |
| Complete / CompleteEmpty | 复用结果 | 沿用现有刷新操作约定 |
| Paused | 显示缓存上限 | 沿用现有上限保护 |
| Stale | 使用正确会话重建扫描 | 重建扫描 |
| Failed | 展示错误 | 重新准备会话并刷新/重试 |

重连使旧快照失效时，必须同时处理旧 in_flight、cursor 和扫描代次；不能只把状态标成 Stale，却保留 Complete cursor 导致 start_scan 拒绝。优先调用现有 refresh/失效接口，并通过测试核实。

### 3.4 首屏空批次续扫

- 首屏展示成功条件：收到至少一个可展示节点，或 cursor 结束；不承诺一次加载完整 DB。
- 复用 `src/db/redis/scan_scheduler.rs` 的预算语义。拟定默认预算为最多 4 次请求、累计 200ms；这些是待本地验证的客户端默认值。
- 200ms 约束是否继续派发下一条请求，不是单条 Redis 请求的超时；保留现有网络超时。
- 下一条只在上一条成功、仍为空、cursor 未结束、Tab 仍活动、预算未耗尽且会话身份有效时发出。
- 预算耗尽保持 Partial，不伪装为空库；r 是用户明确追加扫描。
- 时钟/预算可测试，避免 sleep。RedisBrowserTab 需要 Clone/Eq，不能直接嵌入不满足这些约束的 ScanScheduler；采用兼容的轻量状态或 App 按 Tab ID 管理预算，并在关闭/失效时清理。

## 4. 实施任务

每个任务按“补行为测试 → 运行并确认预期失败 → 最小实现 → 定向验证”执行。已有覆盖不重复编写。提交点是实施阶段建议，计划编写阶段不提交产品代码。

### Task 1：建立恢复与激活行为回归

**Files:** Modify `tests/redis_loading_lifecycle.rs`, `tests/redis_browser_tabs.rs`。

1. 复用 redis_profile/connect_redis 测试辅助函数，增加能返回 ConnectionSucceeded 产出 commands 的 helper。
2. 使用真实 WorkspaceSnapshot/工作区恢复路径构造 db0/db1，活动页 db0；RequestConnect 后回放 ConnectionSucceeded。
3. 断言只有 db0 的一个 ScanRedisKeys，db1 仍 NotLoaded；断言请求保留恢复的 pattern 和 owner_id。
4. 参数化 ActivateTab、NextTab、PreviousTab，验证从另一个 Tab 激活未加载 Redis 页时只发一次扫描。
5. 增加默认 db0、活动恢复页 db1 的场景：只接受指向 db1 的扫描；没有该会话时先 Connect db1。

**Run:** `cargo test --test redis_loading_lifecycle --test redis_browser_tabs`

**Expected:** 新增恢复/键盘路径用例因缺少扫描命令失败；既有显式打开用例通过。不能把构建失败当成预期红灯。

### Task 2：抽取统一激活后的准备入口

**Files:** Modify `src/app.rs`；Test `tests/redis_loading_lifecycle.rs`, `tests/redis_browser_tabs.rs`。

1. 抽取 `prepare_active_tab(reason)`，Redis 下层按 tab_id 分发；保留已有 SQL/Relation 准备顺序和返回命令。
2. NextTab、PreviousTab、ActivateTab 共用入口，保持各自位置历史和焦点归一化操作。
3. 审计 `active_tab =` 写入点，逐项覆盖导航返回、工作区激活、关闭活动 Tab 的相邻页选择。
4. 对内部临时切页（如批量 Relation 操作）保留其原语义，不将临时赋值视作用户激活。
5. 明确打开可设置 Redis Keys 焦点；自动恢复和异步续接保留原焦点。
6. 补充关闭活动页后进入未加载 Redis 页、非 Redis 活动页不触发 Redis 扫描的回归。

**Run:** `cargo test --test redis_loading_lifecycle --test redis_browser_tabs`

**Expected:** 三种显式激活路径通过；恢复和会话相关红灯由后续任务解决。

### Task 3：精确会话准备与恢复续接

**Files:** Modify `src/app.rs`, `src/model/redis_browser.rs`；Read/reuse `src/model/session.rs`, `src/model/keyspace.rs`；Test `tests/redis_loading_lifecycle.rs`。

1. 为 Redis Tab 增加 WaitingForSession/Failed 准备状态，默认 Idle；不写入持久化模型。
2. 将当前连接依赖改为按 RedisTarget 对应的 ExecutionTarget 获取 SessionRegistry 会话。
3. 新建、Connecting、Connected 三类会话分别处理；对 Connected 立即推进扫描，覆盖同步复用分支。
4. 工作区因用户打开连接而恢复/激活后，为活动 Redis 页建立加载意图；仅反序列化不加载。
5. ConnectionSucceeded 在完成会话登记后续接有效等待 Tab；默认 db0 与活动 db1 不同，则继续准备 db1。
6. 不通过临时修改 active_tab/self.connection 来加载后台等待者；扫描使用得到的 ConnectionIdentity。
7. 将 pending_redis_browser_target 的浏览器加载职责迁移到 Tab 状态，保留 Redis 对象创建的独立续接语义。
8. 将 retry_redis_scan 的 Partial 路径也接到精确会话准备，防止 r 使用其他数据库的当前连接。
9. 增加已连接会话复用、正在连接去重、pattern 保留、连接成功与恢复触发重叠仍只扫描一次的测试。

**Run:** `cargo test --test redis_loading_lifecycle --test redis_browser_tabs --test redis_object_editor`

**Expected:** Task 1 全部通过；对象创建等待连接后的行为仍正确；复用会话不新增 Connect。

**Checkpoint commit:** `fix(redis): load restored and activated browser tabs`

### Task 4：错误、重连与异步竞态收敛

**Files:** Modify `src/app.rs`, `src/model/redis_browser.rs`, `src/model/keyspace.rs`（仅现有失效接口不足时）；Test `tests/redis_loading_lifecycle.rs`, `tests/redis_scan.rs`。

1. 构造 A/B 两页连接逆序完成，断言两页请求目标正确、最终 active_tab 和 focus 不被回调改变。
2. 构造关闭等待页后连接完成，断言不复活 Tab、不发该页扫描。
3. 构造旧连接代次完成/旧扫描批次到达、pattern 或扫描代次已变化，断言结果被丢弃。
4. 连接失败清除匹配等待状态并显示准备失败；r 发起新的有效 attempt；无事件驱动无限重试。
5. 断开/重连/Profile 删除清理对应等待与首屏预算，更新受影响 keyspace 代次，保留既有快照策略。
6. 为 Loading、Complete、CompleteEmpty、Partial、Paused 补充或复用幂等断言；真实空库不能通过 keys.is_empty 判定需要扫描。
7. 验证旧在途状态清除和旧 Complete cursor 重置，避免重连后永久不加载。

**Run:** `cargo test --test redis_loading_lifecycle --test redis_scan --test redis_browser_tabs`

**Expected:** 逆序事件与旧响应不会污染目标/焦点；失败可显式恢复。

**Checkpoint commit:** `fix(redis): guard browser loading across reconnects and tab changes`

### Task 5：首屏空批次有限续扫

**Files:** Modify `src/app.rs`, `src/db/redis/scan_scheduler.rs`（按需）, `src/model/redis_browser.rs`（按选定预算归属）；Test `tests/redis_loading_lifecycle.rs`, `tests/redis_scan.rs`。

1. 增加空批次且 cursor=Continue 的行为测试：预算内派发下一条，使用返回 cursor 与原 target/pattern。
2. 增加“得到首个 key 停止”“cursor=Complete 停止”“4 次上限停止”“时间预算耗尽停止”的测试。
3. 实现每个首屏加载周期的预算记账；空批次不改变真实完成语义。
4. 切到后台时允许在途批次落地，但不再自动追加空批次；关闭、错误、刷新和代次变化清理预算。
5. 确保自动续扫只发生在成功接受的 batch 后；旧批次不能消耗预算或产生新命令。
6. 复用 keyspace MAX_KEYS/MAX_KEY_BYTES 和 Paused 保护，预算耗尽后 r 使用现有用户续扫语义。

**Run:** `cargo test --test redis_loading_lifecycle --test redis_scan --test redis_scale`

**Expected:** 首批为空但后续有 key 时自动展示；长期无匹配的大库在预算处停下并显示 Partial。

**Checkpoint commit:** `perf(redis): bound initial empty-batch scan continuation`

### Task 6：加载状态可见性

**Files:** Modify `src/ui/redis_browser.rs`；Test 新建 `tests/redis_loading_ui.rs`，优先复用现有 Ratatui TestBackend 测试工具。

1. 标题组合 database、已加载数量与关键状态：Not loaded / Connecting / Loading / Partial / Complete / Failed。
2. 无树节点时在内容区顶部显示准备或扫描状态；保留底部快捷键提示，避免仅在长面板底部出现错误。
3. 准备失败与 SCAN 失败区分显示，均可引导 r；CompleteEmpty 才显示 No matching keys。
4. 有快照时保留树和 stale/错误提示，不因重试清屏；Loading 的数量来自当前有效快照。
5. 添加两个有价值的渲染回归：NotLoaded/Connecting 不显示空库结论，失败阶段和 r 提示在窄终端可见。
6. 沿用现有主题色和终端宽度处理，检查长错误文本、窄面板和无边界溢出。

**Run:** `cargo test --test redis_loading_ui --test redis_browser_tabs`

**Expected:** 用户无需观察面板最底部即可区分未加载、连接中、扫描中和真正空库。

**Checkpoint commit:** `fix(ui): expose Redis browser loading and preparation states`

### Task 7：整体验证与交付记录

**Files:** 检查本计划涉及文件及 `.github/workflows/ci.yml`；在本计划尾部记录实际验证结果。

1. 执行定向集成测试，确认 reducer、扫描、UI、删除和对象编辑功能相互兼容。

```sh
cargo test --test redis_loading_lifecycle --test redis_browser_tabs --test redis_loading_ui --test redis_scan --test redis_scale --test redis_key_filter --test redis_key_tree --test redis_key_delete --test redis_object_editor
```

2. 按 CI 的工具链/特性集合运行最终检查；若本机缺少 Oracle 等依赖，记录阻塞和已执行范围，不将未执行项目标为通过。

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

3. 使用本地测试 Redis 手动验收以下矩阵，使用独立测试配置/数据：
   - db0 有分层 keys，db1 有不同 keys；保存两 Tab，重启打开连接，活动页立即进入加载。
   - 上次活动 db1、profile 默认 db0；恢复后展示 db1 数据和正确标题。
   - 键盘往返与鼠标往返，观察已加载页不重复扫描、目标不混淆。
   - 空数据库、无匹配 pattern、连续空批次、超过缓存上限。
   - 连接中快速切页/关页、网络失败、重连后重试。
   - Redis 与 SQL/Relation/Dashboard 混合 Tab，检查焦点和原有准备行为。
4. 记录命令计数：每个新激活页首次扫描一次；Connecting 重复激活无重复 Connect；空批次自动续扫不超过预算。
5. 审查 git diff，仅提交本功能文件，确认 workspace schema 与序列化输出兼容。

## 5. 实施顺序与完成标准

```text
Task 1 回归复现
  → Task 2 激活入口
  → Task 3 精确会话与恢复续接
  → Task 4 竞态与错误
  → Task 5 有限首屏续扫
  → Task 6 状态呈现
  → Task 7 全量检查与人工验收
```

- 核心修复检查点：Task 1–4 完成，重启恢复与全部激活路径自动加载正确 DB，无重复和抢焦点。
- 完整交付：Task 1–7 完成，首屏空批次有界推进、状态清晰、回归检查通过。
- 本计划按顺序实施即可；共享 `src/app.rs` 的任务避免同时编辑同一区域。
- 后台所有恢复页的并发预取另行评估；本次产品默认是活动页自动加载和后台页按需加载。

## 6. 执行记录

- 2026-09-15：计划编写完成；尚未实施产品代码或运行产品测试。
