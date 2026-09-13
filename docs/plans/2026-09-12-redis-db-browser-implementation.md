# Redis DB Browser Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
>
> 若该技能不可用，按本文步骤顺序执行。每项完成实现、行为验证和 diff 复核后才能标记完成。本计划不授权 Git commit、push 或合并；新增类型和测试名均为拟定接口。

**Goal:** 用户可以新增 Redis 连接，展开连接看到逻辑 DB 列表，点击 DB 打开独立浏览 Tab；Tab 左侧显示 Key 树，右侧初始空白，选择真实 Key 后显示只读预览。

**Architecture:** 保留 Action → App::update → Command → Runtime 的状态与 I/O 边界，接入 DatabaseConnection::Redis，并为 SQL-only 操作返回明确 Unsupported。全局 Explorer 仅承载连接和 Redis DB，新增 RedisBrowserTab 独立拥有扫描、前缀树、局部焦点与预览；Runtime 管理固定 DB 的 Redis 目标会话。

**Tech Stack:** Rust 2024、Tokio、redis-rs =1.5.0（已有未提交依赖）、Ratatui/Crossterm、Serde/TOML、现有凭据解析、网格、终端文本净化与工作区存储。

---

## 1. 执行位置、范围与现状

- 分支：`task/redis-support`。
- 目录：`/Users/yelog/workspace/tui/lazydb-redis-support`，符合 `task/{task_name}` 与 `../lazydb-{task_name}` 规则。
- 以该 worktree 的未提交代码为起点。开始前重新检查 git diff；不要从 main 复制整文件覆盖已有实现。
- 本计划替代原 Redis 大计划中本轮交付顺序，聚焦 DB Browser。命令控制台、编辑、Agent Redis 工具、Cluster、Sentinel、Pub/Sub 不属于本轮交付。
- 保留现有命令解析原型，但不将它作为浏览器内部命令生成器，也不自动公开执行入口。
- 如果多连接 Console 重构已合入，复用其目标会话注册表；若没有，仅增加 Redis 所需的有限目标会话，不扩展整个 SQL 多连接工作区。

### 1.1 代码证据与必须修正的认识

| 文件/符号 | 实际状态 | 本轮处理 |
| --- | --- | --- |
| `src/db/mod.rs::DatabaseConnection::connect` | Redis 返回 redis_runtime_pending | 实际接入 Redis variant，不能继续报告功能已完成 |
| `src/db/redis/mod.rs::RedisAdapter` | connect/ping/probe/scan_keys/close 原型 | 修正认证构造、TLS、错误、身份与退休生命周期 |
| `src/model/keyspace.rs` | 平面 key 缓存；自行构造 connection generation | 改为真实请求身份；修复暂停、刷新和代次耗尽 |
| `src/db/redis/read.rs` | 请求枚举及预算校验，无实际读取 | 增加 TYPE/PTTL/长度和五种类型的读取 |
| `src/db/redis/reply.rs` | 自定义回复、弱预算裁剪 | 本轮优先类型化预览；若复用裁剪必须修复实际内存/节点边界 |
| `src/sql/dialect.rs` 及 App | 旧入口仍将 Redis 映射 Generic | 实际调用方分流；Redis 浏览 Tab 不拥有 SQL editor |
| `src/model/tab.rs::WorkspaceTab` | 仅 Sql/Relation/Dashboard | 新增 RedisBrowser |
| `src/model/explorer.rs::ExplorerNodeId` | 没有 Redis DB 节点 | 新增独立语义节点，Key 不放入 CatalogId |
| `src/ui/layout.rs::AppLayout` | 用 is_relation 区分主布局 | 改为明确 workspace 布局种类，新增内部左右分栏 |
| `src/runtime.rs::Runtime` | 单个 SQL 活动连接 | Redis 会话按 profile+DB 路由，不 SELECT 共享连接 |
| `src/persistence/workspace.rs::PersistedTab` | 无 Redis 标签 | 新版本迁移及恢复 |
| `tests/redis_protocol_limits.rs` | RESP 首参数解析不正确，取消测试未确认发送 | 修正 fixture 与行为断言 |

上一轮测试通过只证明执行过的断言通过，不证明原计划 Task 1–8 已全部实现。实施记录重新从“待复核”开始，不沿用过度乐观的完成标记。

## 2. 产品行为契约

### 2.1 连接与 DB 列表

1. Profile 表单支持 Redis、host、port、ACL user/password、DB 编号、TLS、凭据保存及可见 DB 范围。
2. Test Connection 使用临时连接，完成后释放；不切换主工作区或建立浏览 Tab。
3. 展开 Redis 连接触发按需连接及 DB 发现，状态为 Loading/Ready/Failed；重复展开单飞。
4. DB 列表包含空 DB（服务端配置可读时），按数值排序，不能将 DB 10 排在 DB 2 前。
5. 点击 DB 行或 Enter 打开对应 Tab；DB 行不继续展开成 SQL schema/table。
6. 折叠连接只改变导航显示，不关闭已打开 Tab，也不停止其合法预览任务。
7. 连接失败在该 profile 根下显示可重试状态，不清空其他 profile 的工作区。
8. Cluster 明确不在本轮支持范围；发现 cluster 模式或收到 MOVED/ASK 时给出准确提示。

### 2.2 DB 浏览 Tab

- 唯一逻辑目标为 `(profile_id, database)`；同目标重复点击激活已有 Tab，不重新扫描或重置选择。
- 不同 DB 打开不同 Tab，标题为 `DB 0 @ connection-name`；窄标签截断显示，完整目标可在标题/上下文查看。
- 新 Tab 初始左侧加载首批 Key，右侧空白，不自动预览首项。
- 鼠标点击真实 Key、键盘明确移动到真实 Key 后触发预览；聚焦 Key 树本身不自动读取默认行。
- 选择 Prefix 节点使右侧为空；双击/Enter/展开键只改变其展开状态。
- 选择新 Key 后立即清除旧预览并显示 Loading，禁止“B 的标题 + A 的内容”。
- 激活另一 Tab 不改写各 Tab 的固定 DB，也不重新创建其状态。
- Close 关闭 Tab 并取消它的待执行任务；其他 Tab 和 profile DB 列表保留。
- profile 断开使相关 Tab 显示离线，保留已加载内容并明确 stale；重新读取需要重新连接。

### 2.3 键树

- 默认按原始字节中的 `:` 分段，只是一种客户端展示约定。
- 首版一 DB 一扫描流，MATCH 默认为 `*`；不为每个 Prefix 建立额外扫描。
- 展开/折叠 Prefix 完全是本地操作；计数标为已加载数量，不冒充全库数量。
- `/` 查找已加载节点；服务端 MATCH 是独立操作，修改后启动新轮次扫描。
- “继续加载”显式派发下一次 SCAN；不默认扫描到库尾。
- 无匹配的非终止批次不是空库，不自动结束；游标 0 才结束。
- 保留原始 key 字节，支持空 key、NUL、非法 UTF-8、控制字符、空前缀段和尾部冒号。
- `user` 与 `user:1001` 共存时展示两个不同身份：Key(user) 和 Prefix(user:)；可采用“user [key]”与“user:”区分。
- 稳定排序：Prefix 与 Key 分组，再按原始字节排序；重新投影后按节点身份保持选择，不能只保留行号。

### 2.4 预览

- 首版只读；String/Hash/List/Set/ZSet 有真实内容预览。
- 通用头部显示完整转义 key、类型、TTL、长度/元素数（可用时）。
- String 支持 Text/Hex，默认有效 UTF-8 为 Text，否则 Hex；分段不破坏原始字节。
- Hash 显示 field/value，List 显示 index/value，Set 显示 member，ZSet 显示 member/score。
- 空 String 与不存在 Key 不混淆。TYPE/PTTL/内容之间 key 过期或类型变化为正常业务状态。
- Stream/模块类型显示实际类型和不支持内容预览的提示，不强制 GET。
- 支持“加载更多/下一范围/刷新”；集合扫描不承诺随机跳页或快照一致性。
- 只读预览不出现 SQL query bar、DDL、事务提交/回滚或 SQL LSP。

## 3. 数据与架构契约

### 3.1 新增模型

| 模型 | 必要内容 |
| --- | --- |
| RedisDatabaseDiscovery | DB 编号列表、可选 key/expire 计数、发现来源、列表完整性、局部警告 |
| RedisTarget | profile UUID、u32 DB 编号 |
| RedisBrowserTab | UUID、target、KeyspaceState、KeyTreeState、PreviewState、局部焦点、分栏偏好 |
| KeyTreeNodeId | Prefix(raw_prefix_bytes) 或 Key(raw_full_key_bytes) |
| KeyTreeState | 展开集合、可见行、选择身份、viewport、已明确选择标记 |
| RedisPreviewRequest | 请求身份、完整 RedisKeyId、读取方式/范围、预算 |
| RedisPreview | 类型、TTL、采样时间、长度、类型化内容、续读状态、截断说明 |
| PreviewState | Empty / Loading / Ready / Missing / TypeChanged / Failed / Offline |

不要直接把 `KeyspaceState.keys` 渲染为树，也不要把 `RedisReadRequest` 的存在当作完成实际读取。

### 3.2 请求身份

扫描、DB 发现、预览都携带不可变请求快照。预览最少绑定：

```text
真实 ConnectionIdentity
+ profile 配置修订（Runtime 校验）
+ RedisTarget
+ Tab UUID
+ Tab 生命周期 generation
+ preview generation
+ request ID
+ 原始 Key 字节
```

- connection generation 只能由会话创建流程分配，不能由扫描 generation 推算。
- DB 发现由 profile 身份、配置修订和发现 request ID 绑定，不假装为 Tab 请求。
- 扫描与预览代次独立；换 Key 不重启 SCAN，继续 SCAN 不使预览失效。
- refresh、MATCH 修改、断开/重连、Tab 关闭分别推进相关代次并退休 pending。
- 使用 checked_add，代次耗尽时拒绝新请求，不用 saturating_add 复用最后一个身份。
- Runtime 在 I/O 前重新确认会话和目标有效；App 在接收结果时再次检查 pending 的完整身份。

### 3.3 会话路由

本 worktree 尚未具备通用多连接注册表。最小接入方案：

1. 保留现有 SQL active connection 行为。
2. Runtime 增加受限 Redis 会话表，key 为 RedisTarget，value 包含真实 ConnectionIdentity、profile revision、DatabaseConnection::Redis 和最后使用时间。
3. 请求同目标时复用有效会话；建连按目标 single-flight；不同 DB 各有固定物理连接。
4. DB 发现可复用 profile 默认目标会话，不额外长期占用管理连接。
5. 默认缓存最多 8 个空闲目标会话；在途会话不能被淘汰，容量耗尽时明确等待/报忙，不能无界扩容。
6. profile 实质配置修改退休所有旧 revision 会话；断开显式取消其任务并停止新请求；关闭 Tab 只释放该 Tab 的请求及使用引用。
7. RedisAdapter 不向外泄漏无限制 clone 的客户端连接；共享退休状态令已取到 adapter 的迟到任务无法继续派发。
8. 不持有全局 Runtime/profile mutex 等待网络操作。

多连接重构如果先完成，复用它的统一注册表，删去重复设计的需求，不另建一套 Redis profile/credential store。

### 3.4 DB 发现

优先 `CONFIG GET databases` 获得 N，构建 0..N 的完整范围；用 `INFO keyspace` 补充有数据的 DB 统计。

- CONFIG 被拒绝时：合并 INFO 中已知 DB、当前目标 DB、显式 scope DB，标记 Partial。
- 两者均被拒绝时：至少显示当前及显式配置 DB，并提供“打开 DB 编号”。
- INFO 没列出的 DB 不能自动视为不存在；CONFIG 不可用时不能写死默认 16。
- DB 数量配置异常巨大时限制一次物化行数，通过本地范围分段加载剩余行，不发 SCAN 探测每一个 DB；完整性和 UI 加载状态分别表达。
- scope 为 All/Selected 决定展示和可打开目标；手工 DB 编号不得绕过 Selected 范围。
- 若未知编号可打开，直到连接 SELECT 成功才确认可用；失败保留原 Tab/会话。
- 统计是某一时刻样本，不是事务快照；未知计数显示未知，不伪装为 0。

### 3.5 扫描与资源预算

- 单轮默认 COUNT 200、缓存 10,000 key 或 16 MiB 原始 key 字节，先到者为准。
- COUNT 是提示，不是硬限制。超大批次明确转为 Paused/Truncated；不丢弃剩余数据后仍宣称可完整续扫。
- Paused 不允许 start_scan；需重新过滤/刷新开始新轮次。
- 刷新保留旧数据作为 stale 快照；新轮次首批成功后替换，失败保持旧内容；新旧 key 不能混合。
- 树节点数和前缀字节也设置预算。原始 key 很多分隔符时不为每个字节产生无限节点；超过最大展示深度后用剩余完整 suffix 作为叶子标签，保留真实 key。
- 前缀树缓存增量更新，可见投影只访问展开节点；渲染只读 viewport，不每帧重建整个树。
- String 范围默认最多 64 KiB；List/ZSet 最多 200 项；Hash/Set COUNT 最多 200 且必须大于 0。
- 所有整数范围先验证 Redis 接受的有符号范围，再使用 checked arithmetic 验证闭区间长度。
- 每 Tab 预览保留默认 4 MiB；限制输出节点与文本长度，停止收集后不为剩余每项分配占位节点。
- 这些预算不等于协议解码峰值内存限制。redis-rs 的 frame 限额必须实际核查，测试和文档不能夸大保证。

### 3.6 选择与预览请求节流

- 选择高亮同步变更；150ms debounce 后派发最后一个 Key 预览。
- 使用受控 timer/generation；快速 A→B→C，只允许 C 更新当前面板。
- 点击同一个已加载 Key 不重复读取；刷新显式重发。
- debounce 期间换 Tab/关闭 Tab/选 Prefix 取消待发请求；已发请求结果按身份丢弃。
- key 预览的内容、标题、TTL 作为同一请求快照展示；局部元信息失败有明确 Unavailable，不混合不同 Key 的数据。

## 4. 分步实施任务

所有命令在 Redis worktree 内运行。下列测试名为计划新增文件，先创建再运行。每项遵循：补能复现缺陷的测试 → 执行并确认预期失败 → 最小完整实现 → 定向检查 → 复核 diff 与行为 → 更新记录。不能为了维持进度把任务改名成更小内容后标记原任务完成。

### Task 1：重新建立基线和可信测试 fixture

**Files:** 修改 `tests/redis_protocol_limits.rs`、`tests/redis_contract.rs`；新增 `tests/support/redis.rs`、`tests/support/mod.rs`；新增 `docs/plans/2026-09-12-redis-db-browser-progress.md`。

1. 记录当前分支、HEAD、git status/diff 和已存在未提交文件。核对是否有新的多连接重构合入。
2. 修复 RESP fixture：先读数组长度，再读第一个 bulk string 长度及内容；支持 TCP 分片和 pipeline 多命令；未知 fixture 请求明确失败。
3. 取消测试通过 channel 等待服务端已收到命令后才取消客户端等待，随后验证服务端仍能完成处理；所有 spawned task 有句柄、超时和清理。
4. 真实服务 fixture 以子进程句柄拥有 Redis，绑定 loopback，使用临时目录/独立配置；Drop 清理自己的子进程。不能假定固定端口空闲，更不能关闭端口上不属于测试的进程。
5. 集成测试自建 UUID 测试 key 并只清理这些 key，不依赖手工预置数据；显式 ignored 测试缺服务配置必须失败。
6. 对之前失败的 `relation_help_executes_space_tc_transaction_control` 在本分支和创建分支的基准提交进行同条件对照；未对照前不得断言为既有失败。
7. 运行 `cargo test --test redis_protocol_limits`；基线相关测试运行 `cargo test --test profile_url --test profile_draft --test execution_target --test workspace_persistence --test keymap`。

**复核通过标准：** 测试能识别真实命令而非“响应固定 OK”；日志不打印密码；基线状态有证据；fixture 不污染其他实例。

### Task 2：修复 Redis Profile、TLS 与目标规范化

**Files:** 修改 `src/profile.rs`、`src/model/profile_manager.rs`、`src/model/execution_target.rs`、`src/ui/profiles.rs`、`src/cli.rs`；扩展 `tests/profile_url.rs`、`tests/profile_draft.rs`、`tests/execution_target.rs`；新增 `tests/redis_profile.rs`。

1. 统一 DB 编号解析函数：空输入默认 0，拒绝符号/非十进制/溢出，0002 规范为 2；范围最终由服务端验证。
2. 直接调用 formatter、表单提交、导入、持久化恢复、ExecutionTarget 校验都使用同一规则；验证 port 不能为 0，schema 必须为空。
3. Redis scope 只允许 DB 层次，schemas 固定 All；初始化草稿时同步 DB 0 scope，不能残留空 schema。显式 scope DB 同样规范化并去重。
4. 结构化 `ssl_mode` 为连接权威来源；UI 只暴露 Disable/VerifyFull。rediss 导入为 VerifyFull；旧原型 Require 如需兼容则在 Redis 专用迁移中规范为 VerifyFull，不能影响 SQL 的 Require 语义。
5. 改 TLS 或 URL format 时同步所有字段，禁止 rediss 文本对应实际明文连接；Prefer/VerifyCa 等未支持状态不能静默降级。
6. 覆盖 IPv6、用户名/密码含空格/加号/%/@、缺 DB、未知 query、TLS 往返、切换驱动后的遗留字段。
7. CLI drivers 列表类型依赖 descriptor 长度而非重复手写 7；更新测试契约，功能支持声明必须与最终接入状态一致。
8. 运行 `cargo test --test redis_profile --test profile_url --test profile_draft --test execution_target`。

**复核通过标准：** 所有连接入口得到相同规范目标；TLS 不存在两套冲突来源；SQL profile 原测试通过。

### Task 3：修复 RedisAdapter 生命周期并接入 DatabaseConnection

**Files:** 修改 `src/db/redis/mod.rs`、`src/db/mod.rs`、`src/db/capabilities.rs`、`src/runtime.rs`；新增 `tests/redis_adapter.rs`；扩展 `tests/database_capabilities.rs`。

1. 使用 redis-rs 1.5 的结构化 ConnectionInfo/认证配置建立 RESP2 连接，不拼接明文密码 URL，不用 form encoding 处理 userinfo。
2. 建连握手完成 AUTH/SELECT；校验固定 profile、DB、配置修订；默认 5s 建连、10s 响应超时。
3. probe 先 PING 再查询可选 INFO；权限不足降级 Unknown，网络错误返回失败。禁止 `.ok()` 吞掉所有错误。
4. 错误优先按客户端 error kind/server code 分类；NOAUTH/WRONGPASS 为认证，NOPERM 为权限，MOVED/ASK 为不支持的集群；有密码不等于网络错误也是认证失败。
5. Adapter 生命周期使用共享退休状态和可释放连接槽；close 后其他 adapter clone 不能发新请求。已发请求“停止等待”不宣称服务端取消。
6. 增加 DatabaseConnection::Redis；connect/kind/probe/close 实际分派，移除 redis_runtime_pending。
7. 逐项处理 Catalog/DDL/Relation/事务/SQL execute 分派：能力为空或返回类型正确的 Unsupported，不能套用其他驱动能力；字符串型 quote_identifier 等不能表达不支持的 API 增加可失败边界并检查调用方。
8. 不把这些穷举编译错误当作任务阻塞，不使用 todo!/panic!/Generic SQL 兜底。
9. 运行 `cargo check --all-targets`、`cargo test --test redis_adapter --test database_capabilities --test redis_protocol_limits`；真实 Redis 测试覆盖无认证、ACL、TLS、DB 0/1、重连和 close clone。

**复核通过标准：** 应用通用连接入口可以创建 Redis；SQL execute 对 Redis 在发送前拒绝；无凭据打印；类型/能力一致。

### Task 4：实现 DB 发现、有限会话表与 Profile Test

**Files:** 新增 `src/db/redis/discovery.rs`、`src/runtime/redis.rs`；修改 `src/db/redis/types.rs`、`src/db/redis/mod.rs`、`src/runtime.rs`、`src/action.rs`；新增 `tests/redis_discovery.rs`、`tests/redis_sessions.rs`；扩展 `tests/profile_runtime.rs`。

1. 先补 CONFIG/INFO 回复解析测试：完整范围、非空 DB 概况、空库、NOPERM、未知字段、非法 count、超大 N。
2. 实现第 3.4 节发现模型，scope 过滤在输出前进行，Partial 来源及警告可呈现。
3. 建立目标会话表和按目标 single-flight 建连，使用真实 ConnectionIdentity 和 profile revision；明确容量/空闲淘汰。
4. Profile Test 使用临时 Redis 连接，probe 成功即可显示连通；DB 发现局部拒绝为 warning，不将其伪装成连接失败。
5. 现有 SQL Test/Connect 路径保持原契约；Redis 展开连接不调用 SQL catalog 初始化或生成默认 SQL Console。
6. 断开、修改配置、删除 profile、退出进程正确退休 Redis 任务/会话；启动密码不能跨 profile 复用。
7. 运行 `cargo test --test redis_discovery --test redis_sessions --test profile_runtime`。

**复核通过标准：** 同 DB 合并建连、不同 DB 不串目标；权限受限仍可浏览已知 DB；会话及任务可释放。

### Task 5：全局 Explorer 展开 Redis DB 列表

**Files:** 修改 `src/model/explorer.rs`、`src/model/workspace.rs`、`src/action.rs`、`src/app.rs`、`src/ui/mod.rs`、`src/input/keymap.rs`、`src/input/mouse.rs`；新增 `tests/redis_explorer.rs`；扩展 `tests/explorer_state.rs`。

1. 新增 RedisDatabase 语义节点和 profile 级发现状态，保持 profile 根和分组行为。
2. Action 定义 RequestRedisDatabases/RedisDatabasesLoaded/Failed；回复携带 profile revision、session/request 身份。
3. 展开根节点按需发现、重复操作单飞；成功更新 DB 子节点，失败显示重试行；旧回复不能使已断开 profile 变在线。
4. DB 数值排序，空 DB 与未知计数独立显示；CONFIG 降级时展示 Partial 提示及输入 DB 入口。
5. 新增 OpenRedisDatabase 语义动作，鼠标单击和 Enter 一致；本任务测试动作派发，实际 Tab 消费在 Task 6 完成。
6. schema/DDL/create/drop 等 Catalog 动作对 Redis DB 节点不生效；不能继承 profile 的关系型菜单。
7. 运行 `cargo test --test redis_explorer --test explorer_state --test profile_runtime`。

**复核通过标准：** 新增/测试/保存 Redis 后可以展开 DB 列表；折叠不影响会话使用；没有 key 出现在全局 Catalog 中。

### Task 6：新增 RedisBrowserTab 与双窗格空壳

**Files:** 新增 `src/model/redis_browser.rs`、`src/ui/redis_browser.rs`；修改 `src/model/mod.rs`、`src/model/tab.rs`、`src/ui/mod.rs`、`src/ui/layout.rs`、`src/app.rs`、`src/model/workspace.rs`；新增 `tests/redis_browser_tabs.rs`、`tests/redis_browser_layout.rs`。

1. WorkspaceTab/TabKind 增加 RedisBrowser；补 id/title/profile/target 等访问器的穷举分支，不返回 SQL Console。
2. OpenRedisDatabase 按 RedisTarget 去重打开/激活标签；已有 Tab 保持树、预览和 scroll 状态。
3. Tab 模型明确 Empty preview，无默认 first-key 读取；先显示左侧 Loading/Empty 和右侧空白。
4. 将 is_relation 布局选择改为明确 MainContentKind，支持 Sql/Relation/Dashboard/RedisBrowser；不把 Redis 冒充 Relation。
5. 标准布局在主工作区内再次分割为 KeyTree/Preview，默认 KeyTree 35%（最小 20 cells，预览最小 30）；不足时 Tab 内采用局部焦点单面板模式。
6. 外层保留 Focus::Explorer/Results 等兼容边界，RedisBrowser 自己拥有 Keys/Preview 局部焦点；不依赖全局 Editor 焦点装载虚构编辑器。
7. 运行 `cargo test --test redis_browser_tabs --test redis_browser_layout --test workspace_tabs --test ui_render`。

**复核通过标准：** 点击 DB 确实打开新类型 Tab；不同 DB 可同时保留；右侧初始空白；SQL/Dashboard 布局回归通过。

### Task 7：修复 SCAN 状态机并接入 Runtime

**Files:** 修改 `src/model/keyspace.rs`、`src/db/redis/types.rs`、`src/db/redis/mod.rs`、`src/runtime/redis.rs`、`src/action.rs`、`src/app.rs`、`src/model/redis_browser.rs`；扩展 `tests/redis_scan.rs`；新增 `tests/redis_reducer.rs`。

1. start_scan 接受真实会话身份/目标，不自行构造 connection generation；pending 保存完整 KeyScanRequest。
2. 新增 ScanRedisKeys/RedisKeysLoaded/Failed/CancelRedisScan；Runtime 通过请求目标取对应会话。
3. 打开 Tab 派发首批扫描，后续仅显式继续加载；空批次可继续，不按游标大小判断前进。
4. Paused/Complete/在途/代次耗尽均不能派发；完整身份验证失败不改变任何状态。
5. 批次合并先计算去重后增量预算，再决定接纳策略；受限批次明确停止，不丢后半批还显示完整可续扫。
6. 保留旧树作为刷新快照，失败保留 stale，新轮首批成功才替换；MATCH 变更重置新轮状态。
7. 以 channels 控制重连、切 DB、关闭 Tab、两次刷新乱序，证明旧事件不会覆盖新数据。
8. 运行 `cargo test --test redis_scan --test redis_reducer --test redis_sessions --test catalog_contract`。

**复核通过标准：** UI 已能接收真实 SCAN key；暂停/重试不循环；原 SQL Catalog 分页未被放宽。

### Task 8：从扫描缓存构建二进制安全 Key 树

**Files:** 新增 `src/model/redis_key_tree.rs`、`src/ui/redis_key_tree.rs`；修改 `src/model/mod.rs`、`src/ui/mod.rs`、`src/model/redis_browser.rs`、`src/ui/redis_browser.rs`；新增 `tests/redis_key_tree.rs`。

1. 实现 Prefix/Key 身份，按原始 `:` 字节分段；标签渲染时转义，身份不经过 UTF-8 有损转换。
2. 覆盖 user 与 user:1、a::b、:a、a:、空 key、包含 ANSI/NUL 的 key 和相同显示标签。
3. 初始折叠 Prefix，可展开/折叠/定位父节点；根据已加载内容投影，不为展开动作发新的 SCAN。
4. 定义默认行高亮但未明确选择状态；首批合并后右侧仍为空。
5. 增量更新树，维护节点/深度/字节预算；长分隔符串降级 suffix 叶子显示而不丢 Key 身份。
6. 继续加载引发排序变化时按 node ID 保持选择；若选中 Key 仍存在，避免多余预览请求。
7. 运行 `cargo test --test redis_key_tree --test redis_scan --test redis_reducer`。

**复核通过标准：** Tab 左侧是真实树交互而非平面列表；预算包含前缀索引开销；所有 key 可无损定位。

### Task 9：实现五种核心类型真实预览读取

**Files:** 修改 `src/db/redis/read.rs`、`src/db/redis/mod.rs`、`src/db/redis/types.rs`；必要时修正 `src/db/redis/reply.rs`；新增 `src/model/redis_preview.rs`、`tests/redis_preview.rs`；扩展 `tests/redis_values.rs`。

1. 校验请求目标与 adapter 固定目标一致，再执行 TYPE、PTTL 和长度查询；元信息回复区分 NOPERM/网络失败/Missing。
2. 实现 String GETRANGE、Hash HSCAN、List LRANGE、Set SSCAN、ZSet ZRANGE WITHSCORES；内置命令使用字节参数，不经过文本 parser。
3. PTTL -2/-1/非负分别映射 Missing/Persistent/ExpiresIn；其他异常值拒绝，不强制转换为大 u64。
4. 提供类型化预览结果和续读 token；Hash/Set 游标、List/ZSet rank 范围、String byte offset 不混用。
5. 修复范围溢出、COUNT 0、数据类型突变、非 UTF-8 和跨段 UTF-8；元信息读取后过期不返回伪造空值。
6. 预算裁剪限制实际持有的节点和字节，截断为独立状态；不要用正常 Status 文本替代数据类型。若无法知完整 original size，用 Unknown/Observed 表达。
7. 用真实 fixture 准备五种类型、空值、大值、TTL 和二进制 key；纯测试覆盖服务端乱序/WRONGTYPE/权限失败。
8. 运行 `cargo test --test redis_preview --test redis_values` 和显式 fixture 测试。

**复核通过标准：** 实际从 Redis 取回内容；首段有界、可续读；Unknown/Empty/Missing 不混淆。

### Task 10：选中 Key → debounce → 预览面板

**Files:** 修改 `src/model/redis_browser.rs`、`src/model/redis_preview.rs`、`src/runtime/redis.rs`、`src/action.rs`、`src/app.rs`、`src/ui/redis_browser.rs`；新增 `src/ui/redis_preview.rs`、`tests/redis_preview_reducer.rs`、`tests/redis_preview_ui.rs`。

1. 定义 SelectRedisNode、PreviewDue、LoadRedisPreview、RedisPreviewLoaded/Failed 及显式刷新/续读动作。
2. 选择 Prefix 清空预览并退休旧请求；选择 Key 立即进入该 Key 的 Loading，150ms 后只派发最新选择。
3. 用 tokio test-util 控制时间，测试 A→B→C、关闭后 due、旧失败晚于新成功、重连后同 key 回复迟到。
4. 同 key 重选不请求；显式刷新推进 preview generation；关闭标签释放 debounce 和在途任务。
5. 复用 data_grid 展示集合，复用文本净化/Hex 显示 String；网格选择、copy 和滚动只作用于当前 preview。
6. 信息头与内容共同归属同一 snapshot，权限不足/不存在/不支持类型分别显示。
7. 运行 `cargo test --test redis_preview_reducer --test redis_preview_ui --test redis_reducer --test redis_browser_tabs`。

**复核通过标准：** 从 UI 选 Key 能看到正确预览；快速导航不会发请求洪峰、串 Key 或复活已关闭标签。

### Task 11：键盘、鼠标、焦点与窄终端完整接入

**Files:** 修改 `src/input/keymap.rs`、`src/input/mouse.rs`、`src/help.rs`、`src/ui/shortcut_hints.rs`、`src/ui/layout.rs`、`src/ui/redis_key_tree.rs`、`src/ui/redis_preview.rs`；新增 `tests/redis_navigation.rs`；扩展 `tests/mouse.rs`、`tests/keymap.rs`。

1. Keys 中 j/k/方向键移动，h/l 折叠/展开，Enter 选择/展开，分页和 gg/G 使用已有导航习惯。
2. 窗格焦点顺序 Explorer → Keys → Preview → Explorer，反向一致；Preview 不存在数据也可聚焦，但不产生编辑动作。
3. 全局 Focus+局部 Focus 共同计算 hit regions/help；SQL editor 的 run/format/transaction 快捷键不适用于 RedisBrowser。
4. 鼠标命中携带 Tab/node 身份；单击 Key 同键盘选择，拖动分栏只更新该 Tab 布局；重绘后过期 hit map 不操作其他 Tab。
5. 120×36、100×30、80×24、56×16 验证窄布局与焦点切换，极小终端沿用应用 fallback。
6. 实现刷新、MATCH、本地查找和继续加载对应提示；快捷键不与现有 profile/SQL 操作冲突。
7. 运行 `cargo test --test redis_navigation --test redis_browser_layout --test mouse --test keymap --test ui_render`。

**复核通过标准：** 键盘和鼠标都能完成完整流程；窄终端右侧预览可访问；提示与实际动作一致。

### Task 12：Tab 持久化、断开和恢复

**Files:** 修改 `src/persistence/workspace.rs`、`src/model/workspace.rs`、`src/model/workspace_save.rs`、`src/app.rs`、`src/runtime.rs`；新增 `tests/redis_workspace.rs`；扩展 `tests/workspace_persistence.rs`。

1. 按实施时最新 workspace version 增加 RedisBrowser 记录，字段为 Tab UUID、profile UUID、DB、MATCH 字节编码、分栏偏好。
2. 旧格式 SQL/Relation/Dashboard 正常迁移；未知未来版本拒绝覆盖；不硬编码与其他计划冲突的版本号。
3. 不持久化连接身份、扫描游标、key/value 缓存、TTL 倒计时和 pending 请求。
4. 恢复 Tab 后右侧为空，左侧按需重扫；不自动选择并读取上次 Key。
5. profile 重命名更新显示，删除保留可识别的失效 Tab 目标或按现有 workspace 契约处理，不能绑定到同名新 profile。
6. 断开/重连保留 Tab 固定 DB，独立配置更新退休旧 revision；退出清理 Redis 会话。
7. 运行 `cargo test --test redis_workspace --test workspace_persistence --test workspace_tabs --test redis_sessions`。

**复核通过标准：** 重启恢复目标与布局；旧文件向后兼容；恢复不会重用旧请求身份或自动读取值。

### Task 13：端到端验收、文档和最后复核

**Files:** 新增 `tests/redis_browser_flow.rs`、`docs/redis.md`；修改 `docs/architecture.md`、`docs/database-capabilities.md`、`docs/configuration.md`、`docs/keybindings.md`、`README.md`、进度文档。

1. fixture 执行真实端到端：新增 profile → Test → Save → 展开 → DB 列表 → 点击 DB → Tab → SCAN → 选 key → 正确预览。
2. DB 0/1 使用相同 key、不同值，交替 Tab 与并发返回证明目标隔离；重复点击不新增 Tab。
3. 验证完整 DB 发现含空 DB，权限受限为 Partial，手工 DB 输入受 scope 限制。
4. 100,000 个短 key fixture 验证只请求首批、缓存达上限暂停、UI 渲染不遍历全库；记录环境，不写未测性能承诺。
5. 最终运行 `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo check --no-default-features --all-targets`、`cargo test`。
6. 默认 cargo test 的 ignored 不能当作真实集成通过；单独执行并记录 ACL/TLS/Redis 版本矩阵。失败按 fixture/实现/基线证据分类。
7. Review 所有 tracked diff 和新增文件；确认没有 runtime_pending、错误 Generic fallback、todo!/空成功结果或声称已经实现的空壳入口。
8. 文档写明 SCAN 非快照、前缀仅展示、列表 Partial、缓存预算与协议解码区别。更新进度表只勾选实际验收通过项。

**复核通过标准：** 用户要求的流程可以实际操作完成；验证记录可复现；剩余限制明确而不隐藏。

## 5. 依赖与阶段交付

```text
Task 1 → Task 2 → Task 3 → Task 4 → Task 5 → Task 6
                                                ↓
                                   Task 7 → Task 8
                                                ↓
                                   Task 9 → Task 10 → Task 11 → Task 12 → Task 13
```

- Gate A（Task 1–5）：真实 Redis 连接成功，全局 Explorer 可见 DB 列表。
- Gate B（Task 6–8）：点击 DB 打开专用双窗格 Tab，左侧有可操作 Key 树，右侧仍默认空白。
- Gate C（Task 9–11）：选 Key 正确预览，换 Key/Tab、重连和窄终端均正确。
- Gate D（Task 12–13）：恢复、回归及用户文档齐备，可以交付。

不以“枚举修改文件较多”作为停止理由；它是已知工作。真正阻塞应记录缺少的外部能力、失败证据或需要用户决策的接口变化。每 Gate 可向用户展示已运行的真实行为，不只展示通过的单元测试数量。

## 6. 最终验收清单

- [ ] 新增 Redis profile、密码与 TLS 可正确保存和测试。
- [ ] 展开 Redis 根显示 DB 列表，空 DB 和 Partial 降级语义正确。
- [ ] 点击 DB 打开 RedisBrowserTab，相同目标去重。
- [ ] Tab 左侧是 Key 树，右侧初始空白。
- [ ] 首批 SCAN 不自动选择或读取第一个 Key。
- [ ] 前缀展开/折叠、空段、空 key、二进制 key 正确。
- [ ] 选 Key 加载预览；选 Prefix 清空预览。
- [ ] 五种类型有真实读取、TTL/长度及受限续读。
- [ ] A→B→C 快速选择与多 DB 相同 key 不串数据。
- [ ] SCAN 空批次/重复/非递增游标与预算暂停正确。
- [ ] 连接代次与扫描/预览代次严格分离。
- [ ] Profile 断开、修改、删除和 Tab 关闭退休正确请求。
- [ ] SQL-only 操作在 Redis 上不可用，SQL 原行为回归通过。
- [ ] 鼠标、键盘、分栏和窄终端完整可用。
- [ ] 工作区迁移与恢复正确，右侧恢复为空。
- [ ] 协议/真实 Redis/ACL/TLS/端到端测试有实际执行记录。
- [ ] 未以单元测试通过代替 UI/runtime 功能交付。

## 7. 任务复核记录模板

每项在 progress 文档记录：

```text
Task N / 名称：
状态：pending | in_progress | completed | blocked
涉及文件：
实现行为：
失败复现及原因：
执行命令与结果：
真实服务测试环境（如适用）：
diff 复核发现与修正：
尚未验证内容：
是否满足本 Task 通过标准：
```

本文为计划，不代表上述任务已经执行。本轮编写只增加本文件；已有未提交实现保持原状。
