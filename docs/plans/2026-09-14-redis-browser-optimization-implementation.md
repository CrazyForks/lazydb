# Redis Browser Optimization Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行环境没有该技能时，按下列任务顺序逐项实施、验证和记录；不要假定技能可用。

**Goal:** 将 LazyDB Redis 浏览器升级为可续扫、可分页、字节语义完整、内存受控且适合大规模键空间的浏览器。

**Architecture:** 保留 Action → App reducer → Command → Runtime → RedisAdapter 的架构，以及按 RedisTarget 隔离的连接。扫描进度、键存储、树视口、值分页分别建模；小数据使用内存存储，大数据使用临时 SQLite 索引。所有后台结果统一携带连接与请求 identity，数据库 I/O 和索引 I/O 均不在 UI reducer/render 中执行。

**Tech Stack:** Rust、Tokio、redis-rs 1.5.0、SQLx SQLite、Ratatui；复用现有测试基础设施。

---

## 0. 范围、依据与执行规则

本计划基于 2026-09-14 两个项目的工作区代码分析。行号会变化，以符号定位为准。

- LazyDB：`/Users/yelog/workspace/tui/lazydb`。
- 参考项目：`/Users/yelog/workspace/rust/rust-redis-desktop`。
- 参考 `src/ui/key_browser.rs::scan_all_keys_from` 的 cursor + pending 续扫思想。
- 参考 `src/redis/key_index.rs` 的 BLOB 去重、批量事务、前缀树索引。
- 参考 `src/ui/value_viewer/data_loader.rs` 的类型化分页和总量状态。
- 参考 `src/redis/commands.rs::get_key_types` 的按需批量元数据加载。

本轮交付覆盖扫描、分页、渲染、临时索引和连接恢复。编辑、任意命令终端、监控面板、复杂序列化解码、Cluster/Sentinel/SSH 是后续独立项目，见第 7 节。

执行规则：

1. 每个任务先定位当前符号与调用方，再修改；既有计划可提供历史背景，当前代码是实施依据。
2. 测试应验证行为和资源约束；先补能重现问题的失败用例，再实现并运行该任务的测试。
3. 任务内每个编号是一项执行动作；较大实现按“模型、Adapter、Runtime、UI”拆成小步完成。
4. 不把新 SQLx 类型和磁盘连接放进需要 Clone/Eq 的 UI 状态；UI 仅持索引句柄和页快照。
5. 每项完成记录测试结果；下方 commit message 是建议的逻辑提交边界，提交时只暂存该任务文件。
6. 本文中的数值是初始调优值或验收目标，不是已经测得的性能结论。

## 1. 必须保持的语义

### 1.1 Identity

所有扫描、元数据、预览和索引结果至少绑定：

`ConnectionIdentity + RedisTarget + owner_id + generation + request_id`。

预览额外绑定选中 Key/preview generation。连接查找和结果接收都验证 identity；取消用于节约资源，identity 校验负责正确性。丢弃 future 不表示 Redis 服务端命令已取消。

### 1.2 SCAN

- COUNT 是提示，不是上限；空批次且 cursor 非零不是完成。
- 区分 Redis cursor、客户端分页 token 和索引排序锚点。
- cursor 为零但 pending 未消费完：服务端遍历结束，客户端仍有下一页。
- 重复 Key 去重；读取期间数据库变化时不承诺快照一致性。
- 到达内存硬预算后不能通过“继续”无限扩容；必须消费 pending、释放窗口或切换磁盘索引。
- pending 本身也计入保留内存预算；无法保留的超大响应必须明确中断或转存，不能静默跳过后宣称扫描完整。

### 1.3 字节与分页

- Key、Hash field、集合 member、value 保持原始字节；只有显示层做转义/UTF-8 展示。
- String 按字节偏移；Hash/Set 按游标；List/ZSet 按索引；Stream 按 ID。
- 分页读取不是快照：类型变化、Key 过期、集合修改有明确结果状态。
- PTTL：-2=Missing、-1=Persistent、非负=ExpiresIn，查询不可用=Unavailable。
- 展示预算和解码/传输预算分开描述；结果裁剪不等于接收响应之前限制内存。

## 2. 分期与依赖

| 里程碑 | 任务 | 可交付结果 |
|---|---|---|
| M0 基线 | T01 | 行为基线、测试夹具、统一预算定义 |
| M1 核心浏览 | T02–T08 | 可恢复扫描状态、快速搜索渲染、类型化值分页 |
| M2 大键空间 | T09–T11 | 内存/SQLite 存储、边扫描边浏览、完整索引模式 |
| M3 连接体验 | T12–T14 | 请求合并、并发限制、元数据缓存、受控重连 |
| M4 验收 | T15 | 回归、规模测试、使用文档 |

依赖图：

```text
T01 → T02 → T03 → T04
T01 → T05 → T06 → T07 → T08
T02 + T03 → T09 → T10 → T11
T04 + T07 → T12 → T13
T07 → T14
T08 + T11 + T13 + T14 → T15
```

默认按 T01 到 T15 顺序实施。不同分支修改 `src/app.rs`、`src/action.rs`、`src/runtime.rs` 时应串行集成。

## 3. 详细任务

### T01：建立回归基线和预算配置

**文件**
- 修改：`src/config.rs`、`config/default.toml`、`tests/redis_protocol_limits.rs`。
- 新增：`tests/support/redis_server.rs`、`tests/redis_config.rs`。
- 回归：`tests/redis_scan.rs`、`tests/redis_values.rs`、`tests/redis_loading_lifecycle.rs`。

**步骤**
1. 运行已有 Redis 测试命令，记录通过/失败和运行环境；已有失败先定位，不掩盖。
2. 将本地假 Redis 服务端提取成可复用夹具：解析 RESP 数组中的命令参数、记录命令序列、按命令延迟或回包。当前 `first_command()` 需要按完整数组解析，不能把数组长度当首参数长度。
3. 为夹具加入命令解析、pipeline 多命令、断连重连行为测试。
4. 添加 `#[serde(default)]` Redis 配置段及对应默认值，兼容旧配置缺省；测试合法值、零值、过大值和配置合并。
5. 将预算分成 scan、value page、retained cache、scheduler 四组，替换魔法常量时保持第一版默认体验。

**初始配置**
- SCAN count hint：200；集合页：200 项；String 页：64 KiB。
- 内存索引转换触发值：10,000 Key 或 16 MiB 原始 Key；不是 RSS 上限。
- 值页保留原始字节预算：1 MiB；格式化页预算：1 MiB；单独展示超限原因。
- 预览防抖：100 ms；每目标读请求并发：4；每 Tab 预览 single-flight。
- 自动填页：一次最多 8 个 SCAN 请求或 100 ms，达到任一预算即交还调度。
- 临时索引磁盘预算：初始 1 GiB，达到后暂停并保留已提交页。

**验证**：`cargo test --test redis_config --test redis_protocol_limits`，预期全部 PASS。

**提交边界**：`test(redis): establish browser budgets and protocol fixtures`。

### T02：修复扫描批次接收与续扫模型

**文件**
- 修改：`src/model/keyspace.rs`、`src/db/redis/types.rs`、`tests/redis_scan.rs`。

**步骤**
1. 增加边界回归：已有 9,950 Key，收到 200 个新 Key，页预算 10,000，必须接收可容纳部分并保留余量和 next cursor。
2. 增加 cursor=0 但 pending 非空、重复 Key、空批次、旧 generation、单个超长 Key、pending 超预算测试。
3. 建立明确的接收结果：Stale / Applied(delta) / Paused(reason)，包含新增 Key、状态变更和下一调度建议。
4. 普通批次原地去重/追加，移除整个 `keys` 和 `key_set` 的逐批 clone；Key 字节可用内部共享句柄，保持对外 RedisKeyId 的字节契约。
5. 保留“刷新时旧结果直到首个成功新批次才替换”的现有语义；用独立首批替换过程处理，不将其混入普通追加。
6. 为后续存储切换保留 PendingBatch：保存已经收到但尚未提交的部分；新批次提交前先消费 pending。

**验收**：不丢已接收结果、不重复请求已消费批次、不越过硬保留预算。磁盘阶段完成前，超硬预算明确停在可恢复状态；不通过盲目追加实现继续。

**验证**：`cargo test --test redis_scan`。

**提交边界**：`fix(redis): preserve scan progress and pending batches`。

### T03：增量键树与稳定节点索引

**文件**
- 修改：`src/model/redis_key_tree.rs`、`src/model/redis_browser.rs`、`src/app.rs`、`tests/redis_key_tree.rs`。

**步骤**
1. 补充不同插入顺序下 `a`、`a:b`、`a:`、空 Key、二进制 Key 的树一致性测试。
2. 新增 insert_batch(delta) 和节点定位索引；正常扫描仅追加新增节点，刷新首批执行一次 replace。
3. 节点 ID 保持稳定，保留 Prefix/Key 双重身份，保证同名 Key 与前缀可分别选中。
4. 用节点索引验证 selection/expanded，避免每个展开节点都递归扫描整树。
5. 增加树 revision，只在结构变化时更新；App 按接收 delta 调用增量接口。

**验收**：扫描下一批后选中/展开位置保持稳定，输出排序与全量构建一致；普通批次不再全树 rebuild。

**验证**：`cargo test --test redis_key_tree --test redis_browser_tabs --test redis_scan`。

**提交边界**：`perf(redis): update key trees incrementally`。

### T04：缓存可见行，分离本地查找与 MATCH 扫描

**文件**
- 修改：`src/model/redis_key_tree.rs`、`src/model/redis_browser.rs`、`src/ui/redis_browser.rs`、`src/input/keymap.rs`、`src/action.rs`、`src/app.rs`。
- 新增：`tests/redis_search.rs`、`tests/redis_viewport.rs`。

**步骤**
1. 先测移动选择、折叠、窗口滚动、搜索取消恢复、扫描期间新增 Key 的匹配更新。
2. 增加可见行缓存和 NodeId→row index；结构 revision/展开 revision 改变才失效，移动选择不失效。
3. 删除 render 内 `find.rows × rows.iter().find()`，直接索引到行；最终只格式化视口和少量预取行。
4. `/` 搜索所有已加载 Key，结果是独立扁平列表；确认时展开选中 Key 的祖先，取消时恢复进入搜索前状态。
5. 添加服务端 MATCH 编辑动作，按现有键位注册方式接入；提交才刷新 generation 和扫描模式，输入期间不发网络请求。
6. 保持 MATCH 为 Redis glob；UI 转义输入与原始字节转换使用单一函数，避免复用命令解析器时混入引号/空参数语义。
7. 标题显示本地已加载/扫描是否完成；部分扫描的无匹配显示“当前已加载范围无匹配”。

**验收**：10,000 可见行的搜索模式不执行二次复杂度的 ID 匹配；折叠节点内的已加载 Key 可被找到。

**验证**：`cargo test --test redis_search --test redis_viewport --test redis_key_tree`。

**提交边界**：`perf(redis): cache viewport rows and separate search scopes`。

### T05：定义类型化分页与预算契约

**文件**
- 修改：`src/db/redis/read.rs`、`src/db/redis/types.rs`、`src/db/redis/reply.rs`、`tests/redis_values.rs`。

**步骤**
1. 增加范围溢出测试，覆盖 u64::MAX，验证前使用 checked arithmetic，不能用饱和减法后无检查加一。
2. 定义 RedisValuePage，包含 identity、metadata、typed data、next position、truncation/partial 状态。
3. 将页位置区分为 String offset、Hash/Set cursor、List/ZSet rank、Stream ID；Start 与 Complete 显式区分。
4. 定义页内容：字节串、field/value 对、带原始 index 的列表项、member、带 score 的 member、Stream entry；不预先合成为 String。
5. 预算分别计入原始字节、元素/节点、格式化字节；超大单项返回可识别的部分项，不允许复制部分数据时伪装成完整值。
6. 修正 `RedisReply::bound`：耗尽预算后停止构造剩余子树，计入 Status/Error，截断标记也纳入输出预算。
7. 明确原始节点数/字节数语义：若停止遍历，不再把已访问统计命名为精确 original total；改为 visited 或 Option total，更新调用方与测试。

**验证**：`cargo test --test redis_values --test redis_contract`。

**提交边界**：`refactor(redis): define typed value pages and output budgets`。

### T06：实现 Adapter 分页读取与元数据降级

**文件**
- 修改：`src/db/redis/read.rs`、`src/db/redis/mod.rs`。
- 新增：`src/db/redis/error.rs`、`tests/redis_read_pages.rs`。
- 复用：`tests/support/redis_server.rs`。

**步骤**
1. 假服务端记录命令，先测 TYPE/PTTL、各类型起止位置、空回复、NOPERM、WRONGTYPE、连接中断。
2. 实现 metadata + read_value_page；接入 T05 请求校验后才发送命令。
3. TYPE/PTTL 使用 pipeline，并用逐项可保留错误的解码方式处理 PTTL 失败；普通 pipeline 的整体错误不能吞掉可用 TYPE。
4. String 使用 STRLEN/GETRANGE；Hash/Set 保留扫描游标；List/ZSet 保留索引；Stream 使用排他 ID 续读，并测试服务端兼容性。
5. 将类型已知后的长度与数据查询适当组合发送；TYPE→数据期间的 WRONGTYPE 标记为类型已变化，最多重新获取一次元数据。
6. 超大 HSCAN/SSCAN 响应先检查保留预算；可保留时分页消费 pending，否则返回受限状态，不能静默丢元素并推进为完整结果。
7. 统一错误分类为认证、权限、网络、超时、不支持、类型变化/缺失；不根据“是否提供密码”推断认证失败。

**验收**：Hash/Set 空批次仍能续读；String 64 KiB 边界正确；PTTL 无权限但数据可读时展示值和 Unavailable。

**验证**：`cargo test --test redis_read_pages --test redis_values --test redis_protocol_limits`。

**提交边界**：`feat(redis): read typed value pages with partial metadata`。

### T07：接入 Runtime/状态机的分页生命周期

**文件**
- 修改：`src/action.rs`、`src/runtime.rs`、`src/app.rs`、`src/model/redis_browser.rs`。
- 新增：`src/model/redis_value.rs`，并在 `src/model/mod.rs` 注册。
- 修改测试：`tests/redis_loading_lifecycle.rs`、`tests/redis_browser_tabs.rs`。

**步骤**
1. 补充相同 profile/DB 不同连接 generation、快速切换 Key、关闭 Tab、旧页结果晚到的回归。
2. LoadRedisPreview/Loaded/Failed 改为类型化请求结果，完整携带 RedisRequestIdentity；连接查找也匹配 connection generation。
3. ValuePageState 管理当前页、有限历史页、pending、请求位置和加载状态。
4. 每 Tab 同一值页 single-flight；下一页失败保持当前页，retry 使用原页请求位置。
5. 新 Key/refresh/重连清理不兼容的页 token，递增 generation；响应处理先验 identity 再修改状态。

**验收**：旧连接/旧 Key 的任何结果不能改变新页；请求失败不会清空已成功读取的内容。

**验证**：`cargo test --test redis_loading_lifecycle --test redis_browser_tabs --test redis_read_pages`。

**提交边界**：`feat(redis): integrate identity-safe value page lifecycle`。

### T08：TUI 类型化预览、翻页和字节展示

**文件**
- 修改：`src/ui/redis_browser.rs`、`src/input/keymap.rs`、`src/action.rs`、`src/app.rs`。
- 新增：`src/ui/redis_value.rs`，并在 `src/ui/mod.rs` 注册；`tests/redis_value_ui.rs`。

**步骤**
1. 用 Ratatui TestBackend 覆盖多行值、窄窗口、TTL、分页 footer、截断标志。
2. 预览按类型渲染：Hash 两列、List index/value、Set member、ZSet score/member、Stream ID/fields；String 支持文本/Hex。
3. 修正当前把完整多行 content 塞进单个 Line 的方式；分页和滚动使用有界行窗口。
4. 统一 byte display：控制字符、无效 UTF-8、原始反斜杠都明确表示；复制原始值与复制展示文本区分。
5. 复用已有页导航命令习惯，按 Preview focus 分派；Hash/Set 上一页仅访问有界本地历史，历史被淘汰时显示不可回退。
6. 显示类型、TTL、人类可读长度、已展示范围、next/complete/partial。过期和持久 Key 不再显示为负毫秒。

**验证**：`cargo test --test redis_value_ui --test redis_values --test redis_browser_tabs`。

**提交边界**：`feat(redis): render paginated typed value previews`。

### T09：抽象 KeyStore，隔离状态与存储

**文件**
- 新增：`src/db/redis/key_store.rs`、`tests/redis_key_store.rs`。
- 修改：`src/db/redis/mod.rs`、`src/model/keyspace.rs`、`src/model/redis_browser.rs`、`src/runtime.rs`、`src/action.rs`、`src/app.rs`。

**步骤**
1. 定义与存储实现无关的契约测试：batch 去重、页排序、二进制 Key、父节点页、search scope、revision。
2. 接口覆盖 insert_batch、page_after、tree_page_after、search_page、lookup、stats、close；使用具体 enum/service 即可，不提前引入通用数据库插件框架。
3. 为内存实现接入现有增量树/键集合，保持已有行为测试通过。
4. 将全量 Key 所有权逐步移入存储服务；UI 保留 KeyStoreId、页快照和 loaded/unique 统计，不再依赖 `keyspace.keys.len()`。
5. 所有查询经 Command/Runtime，响应绑定 store ID、generation、query revision；页内操作仍在 UI 内存完成。

**验证**：`cargo test --test redis_key_store --test redis_scan --test redis_viewport --test redis_search`。

**提交边界**：`refactor(redis): separate key storage from browser state`。

### T10：实现临时 SQLite KeyStore

**文件**
- 新增：`src/db/redis/key_index.rs`、`tests/redis_key_index.rs`。
- 修改：`src/db/redis/mod.rs`、`src/db/redis/key_store.rs`。

**初始 schema**

```sql
CREATE TABLE keys (
    key BLOB NOT NULL PRIMARY KEY
) WITHOUT ROWID;

CREATE TABLE tree_entries (
    parent BLOB NOT NULL,
    name BLOB NOT NULL,
    path BLOB NOT NULL,
    is_leaf INTEGER NOT NULL,
    total_keys INTEGER NOT NULL,
    PRIMARY KEY (parent, is_leaf, name)
) WITHOUT ROWID;
```

**步骤**
1. 用临时目录运行与 Memory 相同的契约测试；事务失败时不能提交一半 keys/tree。
2. SQLx 创建临时数据库，设置 WAL、busy timeout 和有界连接数；初始一个写入器，页查询并发受限。
3. 批量事务 `INSERT OR IGNORE`，仅新 Key 更新前缀计数；返回新增量累计统计，避免每批 COUNT(*)。
4. 平铺列表用 `WHERE key > ? ORDER BY key LIMIT ?`；第一页独立 SQL，确保空字节 Key 不会被跳过。
5. 树页锚点使用 `(is_leaf, name)`，并固定 parent。精确/前缀查询走索引，包含查询如 BLOB instr 属于扫描操作，后台运行并允许取消。
6. 切换 Memory→Sqlite 时先迁移、提交、验证 generation，再发布新 store；失败保留内存结果和 pending。
7. close 明确关闭 SQLx pool 后删除主文件/WAL/SHM；异步关闭不藏在 Drop 中。异常退出残留用专属目录和存活标识识别，只清理本应用可判定失效的索引。

**验收**：非 UTF-8 Key 往返相同；深分页不使用大 OFFSET；前缀/同名 Key 计数正确；关闭后文件释放。

**验证**：`cargo test --test redis_key_index --test redis_key_store`。

**提交边界**：`feat(redis): add temporary sqlite key index`。

### T11：扫描调度、自动转存与渐进索引浏览

**文件**
- 新增：`src/runtime/redis.rs`、`tests/redis_scan_scheduler.rs`。
- 修改：`src/runtime.rs`、`src/action.rs`、`src/app.rs`、`src/model/keyspace.rs`、`src/ui/redis_browser.rs`。

**步骤**
1. 用 fake clock/可控扫描源测试连续空批、预算让出、暂停恢复、写索引慢于扫描、Tab 关闭。
2. 将 Redis 扫描调度迁入 Runtime 子模块，建立容量为 2 个批次的有界队列；队列满时不发下一 SCAN。
3. 每 Tab 一个 ScanSession，优先写入 pending；游标前进与批次接收/持有绑定，不能在索引失败后跳过该批。
4. 渐进模式默认加载首屏/少量预取；用户接近末尾时续扫；时间/请求预算耗尽后重新排队。
5. 到内存阈值自动迁移临时 SQLite，恢复原游标消费 pending；磁盘预算超限显示暂停原因，保留已提交结果。
6. 完整模式后台扫描，首批提交即可浏览。进度按约 100 ms 合并发布，避免每个 Key 触发 UI 更新。
7. DB 总 Key 数仅作参考，不用 SCAN cursor 推算百分比；filtered scan 显示已发现唯一数和扫描状态。
8. 刷新/修改 MATCH 创建新 scan generation；取消后停止后续命令并拒绝旧索引结果。

**验证**：`cargo test --test redis_scan_scheduler --test redis_scan --test redis_key_index --test redis_loading_lifecycle`。

**提交边界**：`feat(redis): schedule resumable scans with index backpressure`。

### T12：预览防抖与请求生命周期管理

**文件**
- 修改：`src/runtime/redis.rs`、`src/runtime.rs`、`src/model/redis_browser.rs`、`src/app.rs`。
- 新增：`tests/redis_preview_scheduler.rs`。

**步骤**
1. fake clock 下连续选择 A/B/C，100 ms 稳定后只发 C；已发 A 的结果不能覆盖 C。
2. 按 owner 管理预览 task/cancel token，替换选择时终止等待任务，关闭 Tab 时清理。
3. 目标级读并发初始 4；获取 permit 之前后均检查 identity/取消状态，优先交互预览，后台扫描不得饿死。
4. 清理完成的 task handle，不让后台任务集合随每次移动增长。
5. 超时以完整逻辑读取为边界，不让多条 10 秒命令叠加成数十秒无反馈；保留命令级 timeout。

**验证**：`cargo test --test redis_preview_scheduler --test redis_loading_lifecycle`。

**提交边界**：`perf(redis): debounce previews and bound request concurrency`。

### T13：可见 Key 元数据批量加载与缓存

**文件**
- 新增：`src/model/redis_metadata.rs`、`tests/redis_metadata.rs`。
- 修改：`src/model/mod.rs`、`src/db/redis/read.rs`、`src/runtime/redis.rs`、`src/ui/redis_browser.rs`。

**步骤**
1. 测试可见 Key 去重、缓存过期、重连失效、NOPERM、部分 Key 已过期。
2. 可见窗口+少量预取生成元数据请求，每批最多 100 Key，用 pipeline 批量 TYPE；默认不对全库查询 TTL/MEMORY USAGE。
3. 缓存 key 包含 connection identity、target、原始 Key；初始最多 2,000 项、类型有效期 5 秒，可按压测调整。
4. 缓存失效后后台刷新，显示不阻塞；TTL 主要在选中详情查询，并按获取时刻呈现剩余时间。
5. 无权限逐项降级；pipeline 错误仍走统一分类，不能把失败当作 Missing。

**验证**：`cargo test --test redis_metadata --test redis_read_pages --test redis_viewport`。

**提交边界**：`perf(redis): batch and cache visible key metadata`。

### T14：受控连接恢复与发现降级

**文件**
- 修改：`src/db/redis/mod.rs`、`src/db/redis/error.rs`、`src/db/redis/discovery.rs`、`src/runtime/redis.rs`、`src/runtime/connections.rs`、`src/app.rs`。
- 新增：`tests/redis_reconnect.rs`。
- 修改测试：`tests/redis_discovery.rs`。

**方案选择**：首版优先复用 Runtime 现有连接 generation 和重建流程，使用受控重建。避免自动重连隐式发生而 UI 完全不知道连接代际改变。只有能明确关联恢复事件和 generation 时才改用 ConnectionManager。

**步骤**
1. 假服务端建立两次连接，验证 DB 参数、AUTH、连接失败分类、旧 generation 结果拒绝。
2. 按目标 single-flight 重建，使用有上限的退避：初始 250 ms、500 ms、1 s；刷新/退出可取消。
3. 重建时显式绑定目标 DB，绝不通过共享连接 SELECT 为其他 Tab 改变数据库。
4. 恢复后递增连接 generation，旧 scan/index 标记 stale；重新扫描从 Start 建立新会话，不宣称旧 cursor 能跨服务端重启延续。
5. 浏览读失败可在新 identity 下重新发起一次；执行结果未知的未来写命令不自动重放。
6. CONFIG/INFO 发现失败按权限或网络保留原因；单项不可用时合并 current/explicit DB。INFO 支持缓存，重连失效。
7. 测试数据库发现异常格式，避免解析错误被无条件归类成 Permission。

**验证**：`cargo test --test redis_reconnect --test redis_discovery --test redis_loading_lifecycle`。

**提交边界**：`fix(redis): recover target connections with explicit generations`。

### T15：规模验收、完整回归与文档

**文件**
- 新增：`tests/redis_scale.rs`、`docs/testing/redis-browser-performance.md`、`docs/redis-browser.md`。
- 更新：`README.md`、`config/default.toml`。

**步骤**
1. 添加显式 ignored 的规模测试，用合成 scan batches 和临时索引构造 1 万/10 万/100 万 Key，默认测试不生成百万数据。
2. 数据覆盖平铺、深前缀、空 Key、非 UTF-8、重复批次、超长 Key；分开统计构建、首屏、查询、移动和搜索。
3. 在本地独立 Redis fixture 验证大 String、Hash/Set 紧凑编码、超大单元素、PTTL 无权限、扫描期间修改集合。
4. 记录命令数、索引写入时间、页查询 P95、UI 事件耗时、队列深度、保留字节、进程峰值 RSS；不只记录总扫描时间。
5. 跑完整检查；检查失败先分辨新回归与基线失败并记录，不能用 no-default-features 代替正常项目验收。
6. 更新文档：本地查找/MATCH/索引搜索范围、分页、暂停/恢复、预算、非快照语义、重连后的 stale 状态。

**建议性能门槛**
- 固定基准机器、release profile：10,000 已加载 Key 下，选择移动/视口绘制 CPU P95 ≤16 ms；输入搜索反馈 ≤100 ms。
- 百万短 Key 索引模式，暖缓存页查询 P95 ≤50 ms；记录冷热缓存差别。
- 百万 Key 时 UI 只保留有界页/元数据，首轮目标额外 RSS ≤128 MiB；超标时先检查 SQLx/SQLite page cache、pending、行缓存和重复字节。
- 本地假服务端稳定数据下，TYPE/PTTL 为一个发送批次，快速选择只触发最终稳定 Key 的新预览。
- 性能阈值用于受控基准验收，不在普通共享 CI 中作易抖动的硬时钟断言。

**验证**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test --release --test redis_scale -- --ignored --nocapture
```

**提交边界**：`test(redis): validate browser scale and document workflows`。

## 4. 测试命令与预期

开始实施前运行现有基线：

```bash
cargo test --test redis_scan --test redis_values --test redis_key_tree --test redis_browser_tabs --test redis_loading_lifecycle --test redis_discovery --test redis_explorer --test redis_commands --test redis_contract --test redis_protocol_limits
```

新测试文件随对应任务创建后再执行任务命令。每项任务遵循：新增行为测试应先因真实未实现行为 FAIL，实现后 PASS；编译错误仅说明 API 尚未接入，完成 API 后还要验证实际行为。

检查优先级：纯模型测试 → 可控假服务端 → 临时 SQLite → 本地真实 Redis → 规模基准。普通自动测试不连接用户保存的 Redis 实例。若受本机 Oracle SDK 等环境约束影响，记录阻塞及替代检查的准确范围。

## 5. 关键验收清单

- [ ] SCAN 重复、空批次、超 COUNT、cursor=0 + pending 均正确。
- [ ] 内存阈值触发磁盘迁移后，游标和结果完整性连续。
- [ ] 数据未保留时不会推进游标并错误标记 Complete。
- [ ] Memory/SQLite 对同一输入的去重、排序和前缀页结果一致。
- [ ] Key/field/member 字节从网络、存储到复制全链路不损失。
- [ ] 搜索不依赖展开状态；部分索引无匹配不等于整个 DB 无匹配。
- [ ] 每帧没有全量树重建或 O(V²) 查找。
- [ ] String/Hash/Set/List/ZSet/Stream 可以继续读取，截断状态准确。
- [ ] 值保留、pending、格式化和缓存各有预算，不将原始 Key 字节预算称为 RSS 上限。
- [ ] PTTL 不可用仍能看值；Key 过期和类型变化不会伪装为空值。
- [ ] 切换 Key/Tab/DB、关闭、刷新、重连后旧响应不生效。
- [ ] 关闭索引先释放数据库连接，再清理文件。
- [ ] 已有 Redis 测试和全项目检查通过，规模验收有可复现记录。

## 6. 预计工作量与交付切片

估算为一名熟悉 Rust 的开发者的有效工作日，包含本模块测试，不含等待外部环境和跨平台故障处理。

| 切片 | 工作量 | 合并条件 |
|---|---:|---|
| T01–T04 基线/扫描/树/搜索 | 4–6 天 | 原有浏览行为回归通过 |
| T05–T08 类型化分页 | 4–6 天 | 所有支持类型可分页，identity 回归通过 |
| T09–T11 大规模键空间 | 5–8 天 | 索引契约一致，转存/取消/背压通过 |
| T12–T14 调度与恢复 | 3–5 天 | 防抖、缓存、重连行为可控 |
| T15 回归与规模验收 | 2–3 天 | 全套检查及基准记录完成 |
| **合计** | **18–28 天** | **M0–M4 全部完成** |

第一轮优先交付 T01–T08，预计 8–12 天，直接解决当前扫描状态、分页和搜索热路径问题。达到硬缓存预算后的完整大库浏览在 T09–T11 交付；首轮 UI 明确呈现该边界。

## 7. 后续功能项目的进入条件

### F1：基础编辑与 TTL
- 进入条件：T05–T08、T14 完成。
- 范围：String/Hash/Set/ZSet 单项操作、TTL、重命名、删除。
- 要求：原始字节操作；String 覆盖的 TTL 策略显式；写成功后精确失效页/元数据/索引；未知执行结果不自动重试。
- List 编辑需独立处理索引漂移，不能把浏览时 index 当作稳定元素 ID。

### F2：Redis 命令终端
- 进入条件：预算模型、错误分类和调度已稳定。
- 复用 `src/redis_command/parser.rs`、`src/redis_command/policy.rs`，先补参数解析/策略测试，再接执行。
- SELECT、事务、阻塞命令、Pub/Sub 使用独立会话连接；不能改变浏览连接状态。

### F3：INFO/SLOWLOG 监控与格式视图
- 优先复用 Desktop 的 INFO/commandstats 解析思想，后台低频轮询、切走暂停。
- JSON/Text/Hex 优先；复杂解码按需执行、限制输出、保持原字节作为真值。

### F4：Cluster/Sentinel/SSH
- Cluster：每主节点独立 scan cursor、全局去重、拓扑 revision、DB 0 约束。
- Sentinel：真正执行主节点发现和故障后重新发现，不能仅连接到配置地址。
- SSH：隧道生命周期与连接 generation 关联，明确 TLS 主机名验证策略。
- 这些项目开始前单独编写实施计划及真实拓扑集成测试，不混入本轮浏览器优化。
