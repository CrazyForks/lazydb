# Redis Dashboard Implementation Plan

**Goal:** 在 Explorer 选中 Redis 连接或逻辑库后，使用现有 Dashboard 快捷键打开属于该 Redis 实例的 Dashboard，展示实例概况、实时指标、趋势和可搜索 INFO。

**Architecture:** 复用 `WorkspaceTab::Dashboard`、Dashboard 异步任务与刷新生命周期；通过类型化 Redis 数据扩展现有监控快照，提供独立的 Redis 内容渲染器。打开前根据焦点解析目标，Tab、请求和响应绑定 profile / connection generation，Redis Dashboard 按 profile 去重，逻辑库仅作为浏览上下文。

**Tech Stack:** Rust、Ratatui、Tokio、redis crate、现有 Action / Command reducer、Serde workspace persistence。

---

## 1. 代码依据与设计结论

### 参考项目

`/Users/yelog/workspace/rust/rust-redis-desktop/src/ui/server_info.rs`：

- `ServerInfoContent`（277 起）：连接状态、版本/运行模式、PID、端口、OS/架构、运行时间；随后是内存、连接/Keys/OPS、持久化，以及可搜索的 INFO 分组表。
- `ServerInfoPanel`（595 起）：首次加载、手动刷新和自动刷新。
- `load_server_info`（580 起）分别调用 raw info 与 parsed info，实际会读取 INFO 两次。LazyDB 应一次采样同时生成概况和详情，保证同一时间点并减少请求。
- `src/redis/commands.rs:43` 的 `ServerInfo` 已有大部分需要的字段；可以参考解析规则，但应使用 LazyDB 自身类型和错误处理。

### 当前项目

- `src/ui/dashboard.rs:18`：Dashboard 外框、Overview / ProcessList 页签、刷新状态、点击详情。
- `src/ui/dashboard.rs:120`：4 列 × 2 行指标卡，下面是 metadata 和最近十分钟趋势。
- `src/app.rs:4386`：`OpenDashboard` 使用 active workspace / active connection，并直接找第一个 Dashboard，没有依据 Explorer 选择解析目标。
- `src/app.rs:802`：`dashboard_supported` 仅排除 SQL Server，且依据活动 profile；当前活动 SQL Server 会影响选中 Redis 时的快捷键可用性。
- `src/input/keymap.rs:2486`：自定义 `open-dashboard` 使用上述 capability 判断；默认快捷键为 `Space b`。
- `src/db/mod.rs:279`：Redis 监控明确返回 unsupported；metadata 返回空数据。
- `src/db/redis/mod.rs:114`：probe 只读 `INFO server` 中的版本，不能直接提供完整 Dashboard。
- `src/runtime.rs:2747`：现有任务防重、连接身份、结果回传机制可复用；`active_database`（5162 起）实际按 expected identity 查连接表，而非只读 UI 活动连接。
- `src/model/dashboard.rs:101,169`：已有历史窗口、generation / uptime 回退检测；但 rate 在分类前拒绝下降值，且首次 Gauge 无数据点，需要修正。
- `src/model/execution_target.rs:7`：默认 console target 不处理 RedisDatabase，且允许 MRU 回退，不适合直接用于 Explorer Dashboard 精确路由。
- `src/persistence/workspace.rs:84`：持久化 Dashboard 当前仅有 id/page/refresh_enabled，需要兼容新增归属和 Redis 页面状态。

## 2. 交互契约

### 2.1 打开目标

1. `Focus::Explorer`：从 `explorer.selected_id()` 解析。Profile 指向该连接；RedisDatabase 指向其 profile 并携带 database 上下文；状态行可沿 owner 解析。分组/空根节点不选择其他连接作为兜底。
2. 其他焦点：优先当前 Tab 明确的连接归属，随后才使用当前 workspace / active connection。
3. 将解析结果立即固化为 `DashboardOpenTarget`（建议字段：profile_id、kind、execution_target、context_database），不能在连接完成时重新读取 Explorer 选中行。
4. 在线连接复用 SessionRegistry；离线连接通过现有连接流程连接，并保存绑定目标和请求 generation 的 pending Dashboard 打开意图。成功后完成打开；失败展示对应目标的错误并支持重试。
5. 连续打开同一个 profile 的 Dashboard 复用 Tab；选择同实例 db0/db2 不新增 Dashboard，更新 context_database。指标标题始终表明 Instance scope。
6. 新的显式导航取代旧的 pending 导航；迟到的连接成功可以保留 session，但不能抢焦点、打开错误页面。
7. 完成打开后焦点进入 Results，Explorer 保持可见，Dashboard 使用现有占满右侧 workspace 的布局。

快捷键、Omni 命令、帮助提示应使用同一个目标解析和能力结果；避免一条入口可用、另一条入口被活动连接类型拦截。

### 2.2 页面

推荐一期两个页签：`Overview` / `Info`。

```text
╭─ REDIS DASHBOARD · production-cache ─────────────────────────────╮
│ [Overview]  Info                         AUTO · refresh 2s        │
│ Redis 7.4.2 · standalone · primary       ● Connected              │
│ cache.internal:6379 · server port 6379 · PID 1234                  │
│ Uptime 12d 03:24:18 · Linux 64-bit · Instance scope · context db2  │
│ ┌ Ops/s ──────┐┌ Clients ─────┐┌ Used memory ─┐┌ Total keys ───┐ │
│ │ 12.8k       ││ 24 / 10000   ││ 1.23 GiB     ││ 182.4k       │ │
│ └─────────────┘└──────────────┘└──────────────┘└───────────────┘ │
│ ┌ Hit ratio ──┐┌ Net in/s ────┐┌ Net out/s ───┐┌ Evicted/s ───┐│
│ │ 98.42%      ││ 2.4 MiB/s    ││ 8.1 MiB/s     ││ 0            ││
│ └─────────────┘└──────────────┘└───────────────┘└───────────────┘│
│ Memory: peak 1.8 GiB · RSS 1.5 GiB · frag 1.22 · max 4 GiB       │
│ Persistence: AOF enabled/ok · RDB idle/ok · pending changes 120   │
│ Replication: primary · replicas 2 · Keyspace: db0 … · db2 …       │
│ ┌ Commands/s · last 10 minutes ────────────────────────────────┐ │
│ │                        trend                                │ │
│ └─────────────────────────────────────────────────────────────┘ │
│ ┌ Network in/out · last 10 minutes ────────────────────────────┐ │
│ │                        trend                                │ │
│ └─────────────────────────────────────────────────────────────┘ │
│ o switch view · r refresh · p pause                             │
╰─────────────────────────────────────────────────────────────────╯
```

示例值仅用于说明；刷新时间展示实际 DashboardConfig 值。

- 复用 theme.surface/border/muted/accent/action/success/warning/error，复用 IconSet 的 Unicode / ASCII 兼容机制。
- 实例信息置顶；版本/端口/uptime 首屏可见。连接 endpoint 来自 profile，server port 来自 INFO，代理/TLS 映射时不会混淆。
- 状态和数值并存：正常 evicted=0 不显示为错误红色；持久化失败使用 error，执行中使用 action。
- 同单位曲线共用坐标：Commands/s 一张，Network in/out 一张；后续内存曲线独立，不与 OPS 混轴。
- 宽度以右侧内容区计算：建议 >=96 四列、64–95 两列、<64 紧凑键值列表；高度不足保留实例信息和核心指标，趋势收起并给出提示。实现时通过 buffer 测试校准阈值。
- Info 页面按 Server / Clients / Memory / Persistence / Stats / Replication / CPU / Keyspace 等分组展示；`/` 搜索字段名和值，复用文本输入和数据表操作，支持完整值详情/复制。
- 保持现有 `o` 页面切换、`r` 刷新、`p` 暂停；Redis 的 `o` 在 Overview/Info 间切换。INFO 页键盘优先级要处理搜索输入态。
- 暂停时明确显示 PAUSED；首次加载显示 loading；后续刷新保留上一帧；失败显示 stale、上次成功时间与原因。INFO NOPERM 时显示指标不可用，不能伪装成零值或健康状态。

## 3. 数据模型与采样

### 3.1 推荐扩展

- 保留 `WorkspaceTab::Dashboard`，避免再增加一套 Tab 的关闭、布局、焦点、持久化分支。
- `DashboardTab` 增加明确的 engine/kind、Redis context 和 Redis 页面状态；渲染不得根据全局 active_profile 猜类型。
- `MonitorSnapshot` 增加类型化扩展 `details: MonitorDetails`，枚举可为 None / Redis(RedisMonitorDetails)。Redis details 保存 parsed INFO、keyspace、服务器身份与运行状态；关系型 adapter 填 None。
- `MetricKey` 增加语义明确的 Redis 指标；复用通用 Connections/BytesRead/BytesWritten/ServerUptime。不要把 Redis commands 映射成 Transactions，或把 Redis misses 隐藏成 SQL block reads。
- Redis 一次 `INFO` 返回既生成 metrics，也生成 details。Runtime 沿现有 LoadDashboardMetrics 回传，Reducer 原子更新最新快照与详情。
- Redis 不再额外派发每次 INFO metadata 请求；数据库公共 metadata 分支可返回兼容结果，但 Dashboard 的 Redis 身份字段从同一份 sample 更新。
- 提取小型 Dashboard card/chart 渲染辅助组件，关系型与 Redis 共用，不设计泛化监控插件框架。

### 3.2 字段映射

| 区域 | INFO 字段 | 语义 |
|---|---|---|
| 身份 | redis_version, redis_mode, role, os, arch_bits, process_id, tcp_port, run_id | 字符串/整数，缺失显示 -- |
| Uptime | uptime_in_seconds | Gauge；同时辅助检测重启 |
| Ops/s | instantaneous_ops_per_sec | Gauge，首帧即可展示；累计 commands 保留在详情 |
| Clients | connected_clients, maxclients, blocked_clients | Gauge；无 maxclients 时只显示当前值 |
| Memory | used_memory, used_memory_peak, used_memory_rss, maxmemory, mem_fragmentation_ratio, mem_allocator | Gauge；maxmemory=0 显示 unlimited，不除零 |
| Keys | dbN:keys=...,expires=...,avg_ttl=... | 有效 keyspace 内全实例合计，保留逐库明细，db 编号按数字排序 |
| Hit ratio | keyspace_hits, keyspace_misses | 概览累计 hits/(hits+misses)，标明 since reset；两者为 0 时 -- |
| Network | total_net_input_bytes, total_net_output_bytes | Counter，按相邻成功采样时间差计算 B/s |
| Evicted / Expired | evicted_keys, expired_keys | Counter；Evicted/s 卡片，累计值与 Expired 在 Info |
| Persistence | aof_enabled, aof_rewrite_in_progress, aof_last_write_status, rdb_bgsave_in_progress, rdb_last_bgsave_status, rdb_last_save_time, rdb_changes_since_last_save | 状态、时间与累计字段分别解析 |
| Replication | role, connected_slaves, master_link_status 等 | 根据 role 显示 primary/replica 字段，保留 Redis 原始字段名供检索 |

解析要区分 absent / malformed / actual zero；有效且为空的 Keyspace 可显示 0，Keyspace 未返回显示 --。保留未知 section/field 供 Info 检索。只在第一个冒号处分割字段，keyspace 子字段按名称解析，未知字段忽略但保留原文。对展示文本应用现有终端字符清理。

一次标准 `INFO` 足以覆盖一期页面；不需要 SCAN/KEYS、逐 key MEMORY USAGE 或 MONITOR。不借用 key browser 的部分扫描数量作为总 keys。当前 adapter 是直接连接模式，若 INFO 报告 cluster，应标明当前节点数据，不声称集群汇总。

### 3.3 历史与请求归属

- Gauge / Ratio 当前值只需 finite，允许下降，首次采样应产生点；Counter 才进行前值比较与时间差计算。
- Counter 首帧、回退、run_id 改变、uptime 下降时产生缺口，重新建立基线。
- run_id 可生成进程内稳定 generation token，或在 Redis 状态中直接比较并重置 history；不要使用每次增长的 uptime 作为 generation。
- 保留十分钟、最多 3600 个样本的现有上限；暂停/失败后曲线应保留时间间隔，不用零值填补，长缺口不连成连续活动。
- 打开、自动刷新、手动刷新统一使用 Tab 的 ConnectionIdentity，通过 SessionRegistry 验证仍存活；不可将当前活动连接覆写进旧 Dashboard。
- 响应验证 tab_id + tab_generation + ConnectionIdentity；旧 generation 响应不能清掉新请求 loading。
- 延续 Runtime 的单飞和 CancelDashboardTasks；关闭/重连递增 generation 并取消旧任务。

## 4. 实施任务

### Task 1：固化目标解析与交互契约

**Modify:** `src/app.rs`, `src/input/keymap.rs`, `src/commands.rs`（仅需要时）, `src/help.rs`。

**Create tests:** `tests/redis_dashboard.rs`。

1. 编写目标解析测试：活动 PostgreSQL/SQL Server + Explorer 选中 Redis；Redis Profile/db2；无活动 workspace 的离线 Redis；分组无目标。
2. 执行 `cargo test --test redis_dashboard`，确认新契约测试能揭示当前错误路由。
3. 增加共享 Dashboard 目标 resolver；让 capability 和 Action 使用同一 resolver。
4. 新增绑定目标/generation 的 pending open 状态，接入 SessionRegistry 和连接成功/失败事件。
5. 按 profile 复用 Dashboard，选择逻辑库只改变上下文；测试连接迟到、不抢焦点与重复打开。
6. 执行目标测试与 `cargo test --test workspace_tabs`。

### Task 2：Redis INFO 解析及 adapter

**Create:** `src/db/redis/monitor.rs`。

**Modify:** `src/db/redis/mod.rs`, `src/db/mod.rs`, `src/db/monitor.rs`, `src/db/postgres.rs`, `src/db/mysql.rs`，以及编译器指出的 MonitorSnapshot 构造位置。

1. 编写 INFO fixture 单元测试，覆盖普通/空库、字段缺失、无效数值、CRLF、未知字段、replica、persistence error。
2. 实现纯解析器和 RedisMonitorDetails；保留可检索分组数据。
3. 增加 RedisAdapter::load_monitor_snapshot，一次 INFO 得到完整快照。
4. 公共 DatabaseConnection 分发改为调用 Redis adapter；关系型新增 details 使用 None。
5. 执行 `cargo test db::redis::monitor`，确认缺失与零值契约。

### Task 3：修正历史指标语义

**Modify:** `src/model/dashboard.rs`。

1. 添加真实语义回归测试：Gauge 首帧、内存下降、连接数下降、Counter 回退、run_id 更换、uptime 下降、非有限值、零时间差。
2. 新增 Redis MetricKey 与明确 kind 分类；将下降检测移到 Counter 分支。
3. 分开处理累计 hit ratio 与速率，不再调用只适用于 SQL BlockHits/BlockReads 的函数。
4. 检查下采样/曲线切段是否跨 None 连线；如有则显式拆分数据段。
5. 执行 `cargo test model::dashboard`。

### Task 4：接入快照生命周期

**Modify:** `src/app.rs`, `src/action.rs`（载荷变化需要时）, `src/runtime.rs`, `src/model/dashboard.rs`。

1. 更新 DashboardMetricsLoaded，使 metrics 与 Redis details 同一批更新。
2. Redis 仅派发一次 metrics INFO；不发 SQL process 请求或重复 INFO metadata 请求。
3. 统一初次加载、手动刷新和轮询的 Tab identity 检查及 loading 防重。
4. 错误保留上次成功样本，区分首次失败与 stale；重连恢复后清除旧错误并建立新采样基线。
5. 补充 A/B profile 和新旧 generation 响应交错测试、tab 关闭测试、暂停/恢复测试。
6. 执行 `cargo test --test redis_dashboard` 和已有 `cargo test dashboard`。

### Task 5：Overview 与 Info 渲染

**Create:** `src/ui/redis_dashboard.rs`。

**Modify:** `src/ui/dashboard.rs`, `src/ui/mod.rs`, `src/ui/icons.rs`, `src/model/dashboard.rs`, `src/input/keymap.rs`, `src/help.rs`。

**Tests:** `tests/ui_render.rs`, `tests/redis_dashboard.rs`，必要时新增 `tests/redis_dashboard_render.rs`。

1. 提取现有 card/chart 的最小共用渲染辅助函数；保留现有关系型样式约定。
2. 根据 Tab engine 派发 Redis Overview / Info 或关系型 Overview / ProcessList。
3. 实现实例信息、8 个指标卡、内存/持久化/复制摘要与两张趋势图。
4. 实现 INFO 分组表、搜索输入、滚动、完整值详情与复制；复用现有 grid/text detail/text input。
5. 动态生成页签、hit target、快捷键和 footer，Redis 不出现 ProcessList 的输入行为。
6. 测试宽/窄/低高度、ASCII 图标、长 endpoint、missing metrics、paused、stale、NOPERM。
7. 执行 `cargo test --test ui_render` 及新 Redis 渲染测试。

### Task 6：持久化与兼容

**Modify:** `src/persistence/workspace.rs`, `src/app.rs`, `tests/workspace_persistence.rs`。

1. Dashboard 持久化增加可选 profile_id/context_database/Redis page，使用 serde default 兼容旧 workspace。
2. 可从父 profile workspace 恢复的旧记录沿用父归属；global tab 则用明确保存的归属，不能推断成当前活动连接。
3. 旧 Redis Dashboard 若保存为 Processes/Charts，恢复为 Overview；关系型原有页面兼容。
4. 仅保存导航状态和刷新偏好；采样历史、INFO、loading、ConnectionIdentity 在运行时重建。
5. 添加旧记录迁移、新记录 round trip、多 profile/global tab 恢复测试。
6. 执行 `cargo test --test workspace_persistence`。

### Task 7：验证与验收

1. 在隔离 Redis 上验证 standalone、replica、INFO NOPERM、无流量、实例重启；沿 `tests/redis_contract.rs` 的 ignored integration test 模式增加测试。
2. 从 PostgreSQL/SQL Server 当前页选中另一 Redis，用 Space b 打开；随后切换连接、关闭/重开、重启应用验证归属。
3. 确認每轮采样仅有一次 INFO，键浏览器已有 key 数不参与 Dashboard 总数。
4. 执行 `cargo fmt --check`、`cargo check --all-targets`、`cargo test`；按项目 CI 的实际 flags 执行 Clippy。
5. 检查帮助中 Space b/o/r/p 与实际行为一致；将新的 Dashboard 行为补到现有使用文档相应章节。

## 5. 一期完成标准

- Explorer 选中哪个 Redis，快捷键就打开哪个 Redis；不受之前活动数据库类型影响。
- 同 profile 不重复打开，db 上下文与实例统计范围清楚。
- 第一帧可见版本、端口、uptime、OPS、clients、memory、keys；需差分的速率等待第二帧。
- 一次 INFO 提供概览和可检索详情；缺失显示 --、有效零值显示 0。
- 自动刷新不闪屏，暂停/失败/重连状态清楚，迟到响应不会污染其他连接。
- 终端缩放后可读，ASCII 图标可用，现有关系型 Dashboard 回归通过。

后续独立增强可加入 Clients（CLIENT LIST）、Slowlog、更多趋势或集群汇总；本期页面以 INFO 可完整支撑的 Overview / Info 为交付闭环。
