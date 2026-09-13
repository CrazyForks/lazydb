# Redis Browser Loading Lifecycle Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
>
> 若该技能不可用，按本文依赖顺序执行。每项完成行为测试、实现和 diff 复核后才能标记完成。本计划不授权 commit、push、合并或子代理；类型及新增测试名为拟定接口。

**Goal:** 修复在 Redis DB 上回车后 Keys 空白的问题，使新建、已有、恢复、失败及跨 DB 切换后的 Tab 都能正确按需加载，并明确显示目标、已加载数量和加载状态。

**Architecture:** 将打开/激活 Tab 与确保目标连接和扫描就绪拆开，所有入口共用 ensure-loaded 状态机。保留当前单活动连接架构，严格绑定连接尝试、扫描和预览身份，保留失效数据快照；Keys UI 根据扫描状态展示加载、失败、空结果、继续扫描或预算暂停。

**Tech Stack:** Rust 2024、Tokio、redis-rs、Ratatui/Crossterm、现有 Action → App → Command → Runtime、工作区持久化和测试基础设施；不新增生产依赖。

---

## 1. 工作位置和修复范围

- worktree：`/Users/yelog/workspace/tui/lazydb-redis-support`。
- 分支：`task/redis-support`。
- 开始前检查 git status 和目标文件 diff，保留已有 Redis/分类/Keys 交互的未提交实现。
- 本轮修复加载生命周期及其可观测性，不以引入多目标连接池为前置，不扩展 Redis 写入、Cluster、Sentinel。
- 本计划覆盖之前计划中未完整兑现的“恢复重新扫描”“重连后恢复加载”“状态可见”契约；不得把新增枚举或构建通过当成流程完成。

## 2. 已确认事实与待验证路径

| 证据 | 当前实现 | 风险 |
| --- | --- | --- |
| `src/app.rs::open_redis_browser` | 找到已有 Tab 后只激活、设置 Results 并返回；仅新 Tab 发 SCAN | 恢复的空 Tab、失败 Tab 不加载 |
| 工作区恢复 | 新建 RedisBrowserTab，不恢复 Key 缓存 | 正确的持久化原则缺少重新加载入口 |
| `pending_redis_browser_target` | 只有目标，ConnectionSucceeded 中直接 take | 打开意图未完整绑定连接尝试；旧回复可能消费新意图 |
| `src/runtime.rs::scan_redis_keys` | 按 profile/generation/DB/schema 匹配物理连接 | 校验正确，不能通过放宽校验掩盖调度错误 |
| `src/model/keyspace.rs` | Idle 同时承担初始/批次间状态，Paused 不阻止 start_scan | 决策含糊，超限后可能继续发送 |
| `src/ui/redis_browser.rs` | 主要渲染 tree rows，固定标题 Keys | 未加载、失败、加载中、空库都可能显示空白 |
| UI `.skip(tab.scroll)` | 刷新后可能保留过大 scroll | 有数据也可能看不到 |
| 截图左 DB 0/33、顶部 /1 | 数据发现计数与执行/Tab 上下文可能不同 | 不能仅凭截图认定串库或权限错误 |

首先用合法 profile 和真实形状的状态复现，不连接截图中的用户实例。诊断需区分：没派发、等待连接、请求被拒绝、服务端失败、空批次和渲染越界。

## 3. 必须保持的行为

### 3.1 打开与就绪

1. DB Enter 总是找到或创建唯一 `(profile UUID, DB)` Tab；同目标重复打开不新增标签。
2. 找到 Tab 不等于已加载。新建、恢复、显式重开、普通激活、连接成功均进入同一就绪判断函数。
3. 若目标连接就绪且 Tab 从未加载，发首批 SCAN；合法请求在途时重复操作不重复发起。
4. 已完成且会话有效的缓存直接显示，包括已确认的空结果；不因 keys.len()==0 自动重扫。
5. 显式 DB Enter 或 Retry 可重试 Failed；仅普通 Tab 激活不无限自动重试确定失败。
6. 连接改变使旧结果为 stale；连接就绪后新一轮扫描从 0 开始，不跨会话续用旧游标。
7. 连接失败保留原活动连接；目标 Tab 显示失败，不标记为可执行就绪。用户可以重试或返回原 Tab。
8. 初次进入 Tab 聚焦 Keys，导航落点与读取预览分开；首次 SCAN 不自动读取第一个 Key 值。
9. 已有 Tab 的正常激活保留展开/选择/预览快照；显式 DB Enter 聚焦 Keys但不清空有效内容。
10. 关闭、删除 profile、断开、改变配置和取消打开意图后，旧完成事件不能重新打开 Tab、重发 SCAN 或抢焦点。

### 3.2 推荐状态模型

连接准备与扫描状态分开，避免笛卡尔积式大枚举：

- Tab 准备状态：Offline / AwaitingConnection(intent identity) / Ready(connection identity) / ConnectionFailed。
- 扫描状态：NotLoaded / Loading(pending request, optional previous snapshot) / Partial / Complete / Failed(previous snapshot, message) / Paused(reason)。

已有字段可按实际结构重组；不要再用独立 in_flight、status 和可选 connection 表达互相矛盾的事实。

| 连接/扫描条件 | ensure-loaded 决策 |
| --- | --- |
| 无合法目标 | 显示失效目标，不连接 |
| 等待同目标连接 | 保留单飞 pending |
| 当前连接不匹配 | 按现有连接切换协议请求目标，绑定打开意图 |
| Ready + NotLoaded | 发首批 SCAN |
| Ready + Loading 且身份有效 | 不发请求 |
| Ready + Partial/Complete | 显示缓存，Partial 提供 Continue |
| Ready + Failed | 普通激活保留错误，显式重开/Retry 发新轮次 |
| Ready + Paused | 提示预算限制，不自动重试 |
| 会话代次变化 | 退休旧 pending，保留 stale，重新首批加载 |

### 3.3 待打开意图

将 pending_redis_browser_target 替换为完整意图，最少保存：

```text
intent_id（单调且不回绕）
tab_id
RedisTarget
expected_connection_attempt: ConnectionIdentity
focus_intent: EnterKeys | PreserveTabFocus
```

- 本轮保持单活动连接，所以一次只保留最后一个有效目标切换意图；后来的 DB 打开明确取代前一个，不建立无界等待队列。
- expected attempt 必须取自成功派发的 Connect，不能在校验失败、运行中禁止切换时预先写 pending。
- ConnectionSucceeded/Failed 仅处理匹配意图；不能无条件 take。
- 用户在等待期间切换到其他 Tab/面板时，退休或降级焦点意图，不能让迟到成功夺回 UI。
- 同目标重复 Enter 合并 pending。关闭目标 Tab 时清掉对应意图。
- 调用现有切库约束（事务、运行中查询等），不绕过安全退出/连接 generation 机制。

### 3.4 扫描身份与预算

- 请求绑定真实 ConnectionIdentity、RedisTarget、Tab UUID、扫描 generation、request ID、MATCH 快照和 cursor。
- Runtime 发送前校验 profile 配置/目标有效及完整连接身份；App 接收时校验当前 pending 和会话。两处校验均保留。
- generation/request ID 使用 checked_add，耗尽时失败且不发请求；不使用 saturating_add 重复身份。
- 首批 SCAN 返回空数组且非零 cursor：Partial，不是 Complete，不是空库。
- 首批返回 cursor 0 且零条匹配：CompleteEmpty。只有 MATCH=* 且没有其他限制时文案可为 No keys found；有过滤时为 No matching keys。
- 默认一次用户操作发一批。首版不增加自动扫描到非空功能，避免引入新的后台循环；空批次有明确 Continue 即可。
- Continue 仅在 Partial 且会话匹配时发送；Loading/Complete/Paused 都不发送。
- 超预算批次禁止截半批后继续使用新 cursor 假装无遗漏。选择原子拒收并 Paused，保留此前已接受缓存；提示缩小 MATCH 或重新扫描。
- Paused 与空结果不可混淆；配置范围或过滤改变后启动新轮次。

### 3.5 UI 可见状态与数量

Tab 标题至少为 `DB 0 @ connection-name`，Keys 标题为 `Keys · DB 0 · 20 loaded`。

| 状态 | 面板展示 |
| --- | --- |
| NotLoaded | Not loaded / Retry 或即将触发首批 |
| AwaitingConnection | Connecting to DB 0… |
| 首次 Loading | Loading keys… |
| 带旧数据 Loading | 保留旧树 + Refreshing…（stale） |
| Failed | 错误摘要 + Retry；存在旧树则同时保留 |
| Partial，零缓存 | No matches in this batch · Continue scanning |
| Partial，有缓存 | 已加载树 + Continue scanning |
| Complete，零匹配 | No keys found / No matching keys |
| Paused | Loading paused · 缓存上限原因 |
| Offline/stale | 缓存树 + Disconnected / Reconnect |

- 错误文本使用现有 terminal sanitization，NOPERM/NOAUTH/连接目标失效等保留准确类别；不打印原始认证 URL。
- Explorer 的 INFO/DB 概况计数是某时刻数据库总 Key 数；Keys loaded 是当前去重缓存的真实 Key 数，不计 Prefix 行。
- 有 MATCH 时不把数据库总数当作匹配总数，不承诺 `loaded/total` 精确进度。
- 顶部目标若表示全局活动连接，必须明确标识；Redis Tab 内标题总是来自自己的 target，不能用 active console 的目标替代。
- Retry/Continue 有键盘、鼠标和帮助入口；不能只画提示而不注册动作。
- Preview 默认空白不影响 Keys 状态可见性；两个面板空状态分别处理。

### 3.6 刷新、滚动与 Find 联动

由 Tab 级 begin_refresh/accept_scan/invalidate_session 协调，不由 UI 各自清字段：

1. 新轮次保留旧树/选择/scroll 作为 stale 快照。
2. 新轮次第一批有效成功后替换旧缓存，随后按节点 ID 保留合法选择/展开，否则回退导航落点。
3. 成功空批次也结束旧数据的权威性；显示新轮次 Partial/Complete 状态，不混合旧 Key。
4. 刷新失败保留旧内容和可见性，同时显示失败。
5. 每次投影变化后 clamp scroll；零行时为 0，有行时选中项在实际 viewport 内。
6. MATCH、会话或扫描 generation 变化退休旧 Find 快照和 pending preview；缓存预览可保留但必须标 stale，不显示成新结果。
7. 状态行占用高度后上报实际树 viewport，避免选中项被 footer 遮挡。
8. 恢复不读取持久化游标、generation、预览数据；恢复的扫描状态固定 NotLoaded，之后通过 ensure-loaded 获取内容。

## 4. 分步实施任务

### Task 1：建立真实缺陷复现与基线

**Files:** 新增 `tests/redis_loading_lifecycle.rs`；扩展 `tests/redis_browser_tabs.rs`；参考 `tests/connection_switch.rs` 与 `tests/workspace_tabs.rs`。

1. 检查当前 worktree diff，记录已有 Redis pending/restore/keyspace 的真实结构。
2. 使用合法 Redis profile、scope、连接目标和发现 DB fixture。禁止伪造不存在 profile 的活动连接让测试绕过校验。
3. 加入失败案例：已有 NotLoaded Tab + DB Enter 应发送 SCAN；恢复 Tab 激活应加载；已 Loaded Tab 不重复扫描。
4. 加入失败 Tab 重试及切库后同目标 Tab 重新绑定新 generation 的测试。
5. 测试真实 Keymap Enter 输出并经 App 消费，不能只调用私有 helper。
6. 执行 `cargo test --test redis_loading_lifecycle`，记录预期失败点；执行 `cargo test --test redis_browser_tabs --test redis_scan --test workspace_persistence` 记录相关基线。

**验收：** 至少一个测试准确复现“找到已有 Tab 后直接 return，不发 SCAN”，不是仅检查 Tab 数量。

### Task 2：明确扫描状态与有效 pending

**Files:** 修改 `src/model/keyspace.rs`、`src/db/redis/types.rs`、`src/model/redis_browser.rs`；扩展 `tests/redis_scan.rs`、`tests/redis_loading_lifecycle.rs`。

1. 增加 NotLoaded，区分从未加载与 Partial；将请求快照作为在途事实来源，消除不必要的重复 in_flight 状态。
2. 定义 begin_first/begin_continue/begin_refresh/accept/fail 的状态转移；禁止 Complete/Paused 续扫。
3. 连接身份变化使旧请求退休并重启新轮次，MATCH 与 cursor 保存在不可变 pending 中。
4. 用 checked_add 处理 request/generation 耗尽；拒绝 Continue(0)/Complete 输入请求。
5. 超预算批次先去重计算增量，再原子接纳或暂停；不一边修改缓存一边丢弃剩余数据。
6. 测试空批次非终止、终止空批次、重复键、非递增游标、超限、重复点击、旧 generation 和错误 request ID。
7. 执行 `cargo test --test redis_scan --test redis_loading_lifecycle`。

**验收：** 状态能够无歧义判断是否需要 SCAN；过期事件不修改缓存；合法空批次可继续。

### Task 3：拆分打开 Tab 与 ensure-loaded

**Files:** 修改 `src/app.rs`、`src/model/redis_browser.rs`、`src/action.rs`；扩展 `tests/redis_loading_lifecycle.rs`、`tests/redis_browser_tabs.rs`。

1. open_or_activate 只负责目标校验、去重建 Tab、导航意图；所有分支随后调用 ensure-loaded，不再在已有 Tab 分支提前结束整个流程。
2. 给 ensure-loaded 输入明确原因：ExplicitOpen、Activate、ConnectionReady、Retry、Refresh、Continue。
3. 新 Tab 即使连接未就绪也有可见准备状态；请求切换被现有事务约束拒绝时显示原因，不创建无反馈 pending。
4. 同目标有效缓存复用；NotLoaded 派发首批；Failed 仅显式原因重试；Loading 单飞；Paused 不自动重试。
5. next/previous/ActivateTab、DB Enter、恢复后的激活及 Redis connection ready 全部调用统一入口。
6. 首次自动扫描成功后建立导航选择，不调用 preview_key；旧已有选择的保留遵守快照状态。
7. 执行 `cargo test --test redis_loading_lifecycle --test redis_browser_tabs --test workspace_tabs`。

**验收：** 新建与已有/恢复 Tab 行为一致，重复激活没有网络请求洪峰。

### Task 4：绑定连接打开意图与跨 DB 生命周期

**Files:** 修改 `src/app.rs`、`src/action.rs`、`src/runtime.rs`；扩展 `tests/redis_loading_lifecycle.rs`、`tests/connection_switch.rs`；新增 `tests/redis_loading_runtime.rs`。

1. 使用第 3.3 节完整 intent 替换 target-only pending，绑定实际成功派发的 Connect generation。
2. ConnectionSucceeded/Failed 按 expected attempt + target + tab 校验后才消费 intent；不无条件 take。
3. DB 1→DB 0 连接完成后更新执行目标，再绑定 Tab 并 ensure-loaded；发现 DB 统计请求不能改写浏览目标。
4. 旧会话在途扫描无论已失败或成功都不得污染新轮次；请求接收时核对当前会话，而不仅比较 Tab 内旧字段。
5. 关闭 Tab、取消、断开、删除/修改 profile 清理意图并退休 pending；旧成功不能重建已关闭 Tab。
6. Runtime 保留完整 profile/generation/DB/schema 校验；未匹配时返回明确错误事件，不悄悄 return 留下 Loading。
7. 不持全局 mutex 等待网络，测试用受控 channel 排序而非 sleep 猜测。
8. 覆盖连续 DB 0→DB 1 打开、旧成功晚到、旧失败晚到、连接拒绝、用户后来切走、Tab 关闭和 profile 配置修改。
9. 执行 `cargo test --test redis_loading_runtime --test redis_loading_lifecycle --test connection_switch`。

**验收：** 请求/物理连接目标一致；过期意图不消费新请求；单活动连接限制被正确表达而非绕过。

### Task 5：Tab 刷新事务、scroll 与 Find 一致性

**Files:** 修改 `src/model/redis_browser.rs`、`src/model/keyspace.rs`、`src/model/redis_key_tree.rs`、`src/app.rs`；扩展 `tests/redis_loading_lifecycle.rs`、`tests/redis_key_tree.rs`。

1. 用 Tab 级方法开始刷新，保存 stale 树快照及选择/scroll，不立即清屏。
2. 第一批新结果成功后原子替换；之后的 Continue 才追加；新旧 generation 不混合。
3. 失败保留旧树，成功空批次切到新空/Partial 状态；旧预览只可标 stale 不能伪装为新读取。
4. 投影改变后 clamp scroll：100 行滚到末尾刷新为 2 行，仍能看到新行；刷新为 0 行 scroll=0。
5. 会话/MATCH/扫描轮次变化关闭旧 Find，清理旧 preview pending；保持 preview_generation 与 scan generation 独立。
6. 测试空 key、同名 Prefix/Key、选中 key 被删除、刷新中取消、错误重试和 Find 编辑时重扫。
7. 执行 `cargo test --test redis_loading_lifecycle --test redis_key_tree --test redis_browser_tabs`。

**验收：** 不会因旧 scroll 或 Find 快照隐藏真实数据，刷新错误不丢有效旧快照。

### Task 6：加载状态、目标标题与数量展示

**Files:** 修改 `src/ui/redis_browser.rs`、`src/ui/mod.rs`、`src/model/tab.rs`、`src/model/redis_browser.rs`；新增 `tests/redis_loading_ui.rs`；扩展 `tests/ui_render.rs`。

1. Keys 渲染先匹配准备/扫描状态，再绘制树；状态文案按第 3.5 节实现，不把所有无行状态等同空库。
2. 错误保留类别与净化后的摘要；有旧树时预留状态行并继续显示树。
3. Redis Tab 标题显示 DB 与连接名，重命名后跟随展示，不将连接名称复制成持久化身份。
4. Keys 标题/状态栏显示 DB、loaded 数、扫描是否完成；loaded 来源为去重 Key 缓存，不是树可见行数。
5. 顶部全局连接与当前 Tab 目标的文案明确，必要时标注正在连接的目标；不能使用 SQL Console 目标代替 Redis Tab.target。
6. 为 Loading、Failed、NotLoaded、Partial-empty、Complete-empty、Paused、Offline/stale 分别做 TestBackend 断言。
7. 覆盖三种 IconMode、80×24/120×36，以及长连接名/错误/过滤条件；状态不覆盖 Key 行或 Preview。
8. 执行 `cargo test --test redis_loading_ui --test ui_render --test workspace_tabs`。

**验收：** 无数据时也能判断原因；截图中的 DB 0/DB 1 不一致可被清晰辨认；空白不再吞掉错误。

### Task 7：Retry、Continue、Refresh 与恢复触发

**Files:** 修改 `src/action.rs`、`src/app.rs`、`src/input/keymap.rs`、`src/input/mouse.rs`、`src/help.rs`、`src/ui/shortcut_hints.rs`、`src/persistence/workspace.rs`；扩展 `tests/redis_loading_lifecycle.rs`、`tests/redis_loading_ui.rs`；新增 `tests/redis_loading_restore.rs`。

1. 状态行 Retry/Continue 注册真实语义命中，操作对象绑定 Tab ID 和状态修订，旧 hit map 不操作新轮次。
2. Keys 刷新键作用于该 Tab，不落入全局 Explorer Catalog；按状态开放 Retry/Continue 并限制 single-flight。
3. 第一次空批次非终止有键盘/鼠标 Continue，不无界后台扫到尾部。
4. 恢复的 Redis Tab 一律 NotLoaded；连接就绪且激活时进入 ensure-loaded。只恢复目标/过滤/布局，不保存 scan cursor/pending/data。
5. workspace snapshot 必须保存 Tab 自身 profile/DB，不用外层活动 profile 覆盖目标。检查现有文件版本规则，修正需要的兼容分支但不任意覆盖旧 workspace。
6. 增加实际 WorkspaceStore save/load→App restore→Activate/DB Enter 测试，断言派发首批 SCAN、Preview 仍为空。
7. 帮助显示与状态一致：不可 Continue 时不宣传继续；加载中重复回车不会重复发请求。
8. 执行 `cargo test --test redis_loading_restore --test workspace_persistence --test redis_loading_lifecycle --test redis_loading_ui --test mouse`。

**验收：** 用户能从每一种可恢复失败/Partial 状态走出来，恢复 Tab 不再永久空白。

### Task 8：真实 Redis 端到端与最终复核

**Files:** 新增 `tests/redis_loading_flow.rs`；完善 `tests/support/redis.rs`（若已有则复用）；修改 `docs/redis.md`（存在时）及 `docs/architecture.md`，记录本轮行为。

1. fixture 自行拥有隔离 Redis 子进程、loopback 端口和临时目录，清理由进程句柄保证；不启动/关闭不明归属的固定端口实例。
2. 在 DB 0/DB 1 创建 UUID 前缀 key，包含相同 key 不同值；测试自行准备和清理，不要求手工预置数据。
3. 经真实 Runtime 和 Keymap→App 执行：连接 DB 1→发现 DB 0 有 key→DB 0 Enter→目标切换→SCAN→树可见；断言目标身份和指定 key，而非只断言非空字符串。
4. 保存 workspace 后新建 App/Runtime，加载恢复空 Tab；激活后必须读回 DB 0 的 key，不能依赖上个进程缓存。
5. 再切回 DB 1，验证不复用 DB 0 cursor/key；关闭等待连接的 Tab 后到达旧回复不能复活标签。
6. 受控 fake adapter/RESP fixture 验证 NOPERM、空非终止批次、超时、断开和乱序，真实服务测试用于证明协议及完整流程，不把两者混为一项。
7. 显式 ignored 集成测试缺 fixture/URL 必须失败，不悄悄 return 成功；测试输出不得泄漏凭据。
8. 执行 `cargo test --test redis_loading_flow -- --ignored --test-threads=1`（按 fixture 约定配置），记录实际 Redis 版本与结果。
9. 最终运行 `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo check --all-targets`、`cargo test`。全仓失败记录具体目标、是否与修改前同条件一致；不要将 lib 的 829 项计数当作全仓统计。
10. 复核 tracked diff 和新增文件，检查是否还有“已有 Tab 直接返回”“无条件 take intent”“错误不展示”“伪首屏计数”等路径。完成后 `cargo build`，给出 worktree 二进制路径；不自动提交。

**验收：** 用户场景经过真实服务和恢复/切库测试验证；主流程失败有明确状态与可执行恢复入口。

## 5. 依赖与阶段交付

```text
Task 1 缺陷复现
  → Task 2 状态模型
  → Task 3 统一 ensure-loaded
  → Task 4 跨 DB/连接身份
  → Task 5 快照/滚动/Find
  → Task 6 状态 UI/数量/目标
  → Task 7 操作入口/恢复
  → Task 8 真实端到端/交付
```

- Gate A：已有和恢复 Tab 会产生正确 SCAN，Loaded Tab 不重复扫描。
- Gate B：跨 DB 切换、失败与过期连接结果不会串目标或丢失新意图。
- Gate C：所有无 Key 状态都有解释和相应 Retry/Continue，scroll 不隐藏数据。
- Gate D：真实 DB 0/DB 1 与工作区恢复端到端通过。

## 6. 最终验收清单

- [ ] 已有 NotLoaded Tab 回车产生首批 SCAN。
- [ ] 恢复 Tab 激活后重新加载，Preview 仍为空。
- [ ] 有效缓存/在途请求重复激活不重复派发。
- [ ] Failed 可重试，Paused 不被普通激活偷偷重启。
- [ ] DB 1→DB 0→DB 1 的连接/请求/Tab 目标一致。
- [ ] 过期连接成功或失败不消费新打开意图，不抢焦点。
- [ ] 关闭/取消/断开/profile 变更退休正确 pending。
- [ ] SCAN 空批次非终止显示 Continue，而非完成空库。
- [ ] 错误类别/摘要可见，凭据和控制字符不泄漏。
- [ ] Keys loaded 数不包含 Prefix，且不冒充 INFO 总数。
- [ ] Tab 明确显示 DB 编号和连接名，顶部上下文不误导。
- [ ] 刷新失败保留旧树，刷新变少/为空时 scroll 合法。
- [ ] 搜索快照和 pending preview 不跨扫描轮次复用。
- [ ] workspace 真正 save/load/restore 后仍能完成首屏加载。
- [ ] 真实服务端到端测试断言了 Key 身份和目标，不只测独立 adapter。
- [ ] 所有完成说明都有实际验证证据。

本文件为实施计划。编写时只新增文档，未修改应用实现、未连接用户数据库、未运行上述功能测试。
