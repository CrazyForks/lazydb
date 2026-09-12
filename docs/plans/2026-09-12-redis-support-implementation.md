# Redis 支持 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
>
> 执行约定：若环境没有上述技能，按本文任务依赖顺序逐项实施和验证。本文是实施设计，新增类型、文件、测试及命令接口均为拟定契约，不表示已经实现。仅在用户明确要求时执行 Git 提交或推送。

**Goal:** 为 LazyDB 增加单机 Redis 连接、二进制安全的键浏览、类型化只读数据查看和受限命令控制台，再分阶段增加写操作、Agent 接口和集群支持。

**Architecture:** 保留 Action → App::update → Command → Runtime → DatabaseConnection 的边界及现有 SQL 路径。复用连接配置、凭据、文本编辑与展示组件，为 Redis 新增键空间扫描、原生命令和回复模型；Redis 数据不进入关系型 Catalog、SQL 分析或 SQL 事务路径。

**Tech Stack:** Rust 2024、Tokio、Ratatui/Crossterm、Modalkit、Serde/TOML，新增 redis-rs 客户端；TLS 使用与现有项目一致的 rustls 技术路线。客户端具体版本与 feature 组合在 Task 1 完成编译验证后锁定。

---

## 1. 决策摘要

1. **可行，但不是仅新增驱动。** 改动涉及 profile、能力分派、异步任务、浏览器、编辑器、结果展示和持久化。
2. **首期支持单机 Redis 6.2 及以上，验证 Redis 6.2、7.x、8.x。** 这是计划中的支持基线，须以集成测试结果确定最终发布声明。
3. **首期所有 Redis 操作只读。** 即使 profile 的 `read_only=false`，也不开放尚未实现策略的写命令。
4. **不创建虚构 schema/table，不改变 SQL Catalog 的分页契约。** Redis 使用独立键空间状态，在相同左侧区域呈现。
5. **保留 DatabaseConnection 枚举。** 不提前引入通用 NoSQL trait、插件注册系统或所有数据库共用的查询语言。
6. **物理连接固定逻辑数据库。** 不在共享连接上为不同请求交替发送 SELECT。
7. **首期不承诺任意命令或任意大小数据都可执行。** 提供明确支持列表和有预算的数据查看；协议层资源保护需要单独验证。
8. **首期可独立于多连接 Console 重构交付。** 当前单活动连接按现有机制切换；如果多连接重构先合入，直接使用它的目标会话注册表。

## 2. 已确认代码基础与冲突

行号为制定计划时的定位参考，实施前按符号确认最新代码。

| 位置 | 当前事实 | 实施影响 |
| --- | --- | --- |
| `src/profile.rs::DatabaseKind` | 六种关系型产品 | 增加 Redis 并覆盖默认端口、名称、URL 和校验分支 |
| `src/profile.rs::ConnectionProfile` | host/port/user/database/schema、凭据策略、TLS、环境、只读属性 | 复用通用字段；Redis database 为规范化十进制编号，schema 为空 |
| `src/db/descriptor.rs::DRIVERS` | 固定驱动列表及名称列表 | 同步列表、CLI 驱动提示和表单入口 |
| `src/db/mod.rs::DatabaseConnection` | 枚举分派多个独立驱动 | 增加 RedisAdapter；不要求使用 SQLx |
| `src/db/capabilities.rs::DatabaseCapabilities` | catalog、relation、monitor、transaction 等布尔能力 | 增加交互模型能力；SQL-only 入口对 Redis 显式拒绝 |
| `src/db/catalog.rs::CatalogKind` | Database/Schema/Table/Column 等关系型对象 | 不直接承载 key |
| `src/db/catalog.rs::CatalogId` | `native_path: Vec<String>` | key 身份必须另用原始字节，不能依赖展示文本 |
| `src/db/catalog.rs::validate_page_progress` | 下一游标按 keyset 递增；有下一页时本页必须填满 | 与 SCAN 非递增、空批次和 COUNT 提示冲突 |
| `src/model/pagination.rs::PageRequest` | offset、page size、末页请求 | 不用于 SCAN/HSCAN/SSCAN 的服务端分页 |
| `src/sql/execution.rs::ExecutionDraft` | 固定 SQL、SqlDialect、SQL 风险与目录影响分析 | Redis 使用独立执行草稿，在此之前分流 |
| `src/sql/dialect.rs::for_database_kind` | 每个产品必定映射 SQL 方言 | 增加可失败的 SQL 方言查询，Redis 不映射 Generic |
| `src/db/query.rs::ResultSet` | 二维列/行和 affected_rows | 类型化查看可投影为表格，原生回复另外保存 |
| `src/db/value.rs::CellValue` | 已包含 Bytes | 复用单元格展示；不能把任意字节先有损转成 String |
| `src/model/tab.rs::WorkspaceTab` | Sql/Relation/Dashboard | 增加 RedisConsole、RedisKey 两类标签 |
| `src/runtime.rs::Runtime` | `connection: Arc<Mutex<Option<ActiveConnection>>>` | 首期不假定已经存在多目标并发会话 |
| `src/model/execution_target.rs::ExecutionTarget` | profile_id/database/schema | 边界校验后转换为 RedisTarget |
| `src/persistence/workspace.rs` | 当前版本 4，console 使用 sql_file | 增加独立 Redis 文档记录及标签迁移，旧 SQL 语义保持明确 |
| `src/agent/policy.rs::authorize_query` | 使用 classify_sql | 首期 SQL Agent 工具显式拒绝 Redis，后期新增专用接口 |
| `src/db/transaction.rs::TransactionRequest` | Commit/Rollback/SQL Page | 不映射到 MULTI/EXEC/DISCARD |

### 2.1 与其他计划的衔接

- `docs/plans/2026-09-12-multi-connection-consoles.md`：该文档是计划，不是当前能力。若先实施，Redis 使用其完整目标会话键、请求代次及文档生命周期；若尚未实施，首期只要求切库正确，不承诺跨库同时在线。
- `docs/plans/2026-09-12-connection-url-sync.md`：Redis URL 增量复用其结构化字段权威来源、原子解析和失败提示规则，不建立第二套同步机制。
- 实施时重新检查工作区/profile 版本；以当时最新版本迁移，不硬编码“必须升到 5/7”。

## 3. 交付范围

| 能力 | M1：只读闭环 | M2：写入与 Agent | M3：高级支持 |
| --- | --- | --- | --- |
| 单机连接、认证、TLS | 支持 | 延续 | 延续 |
| DB 编号选择 | 支持，目标固定 | 延续 | Cluster 固定 DB 0 |
| SCAN MATCH 键浏览 | 支持 | 延续 | 多主节点扫描 |
| String/Hash/List/Set/ZSet | 只读 | 按类型增量编辑 | 性能优化 |
| Stream | 显示类型和不支持提示 | 范围读取 | 消费组等按需求增加 |
| 控制台 | 单条、受限只读命令 | 分类后的写命令 | 状态型/阻塞命令独立设计 |
| SQL 手动事务 | 关闭 | 关闭 | 如需 Redis 事务，专门设计队列语义 |
| Agent/MCP | 可列出连接；SQL 工具明确拒绝 | Redis 专用只读接口优先 | 写接口按策略开放 |
| 持久化 | 配置、Redis 文档、目标、键详情引用 | 延续 | 延续 |
| 监控 | 关闭 | 可独立规划 | Redis 指标看板 |
| Cluster/Sentinel/PubSub | 明确不支持 | 不隐式支持 | 各自独立验收 |

M1 的控制台应标为“受限只读控制台”，不能宣传为 redis-cli 的完整替代。

## 4. 模块设计

### 4.1 建议新增文件

| 文件 | 职责 |
| --- | --- |
| `src/db/redis/mod.rs` | RedisAdapter、连接建立、probe/close、错误映射 |
| `src/db/redis/types.rs` | 目标、键身份、扫描请求/回复、元信息、预算 |
| `src/db/redis/scan.rs` | SCAN 执行、批次验证、目标关联 |
| `src/db/redis/read.rs` | 按类型读取、长度/TTL 查询和结果投影 |
| `src/db/redis/reply.rs` | 原生回复模型、客户端 Value 转换及截断标记 |
| `src/redis_command/mod.rs` | 公开命令解析、验证入口 |
| `src/redis_command/parser.rs` | 文本到字节参数的纯解析 |
| `src/redis_command/policy.rs` | 命令/参数支持矩阵及只读策略 |
| `src/model/keyspace.rs` | 扫描状态、去重、选择、刷新和内存上限 |
| `src/model/redis_console.rs` | 编辑文档身份、不可变执行草稿和回复状态 |
| `src/model/redis_key.rs` | 固定 key 详情、类型、TTL、分页和加载状态 |
| `src/runtime/redis.rs` | Redis Command 执行与带身份 Action 回传 |
| `src/ui/keyspace.rs` | 左侧键空间展示 |
| `src/ui/redis_console.rs` | 控制台输出与命令提示 |
| `src/ui/redis_key.rs` | 类型化数据详情和局部分页 |
| `docs/redis.md` | 面向用户的支持范围、语法、限制和操作说明 |

纯模型不依赖 redis-rs 客户端类型；redis-rs 类型限制在驱动层。模块数量可按实际代码体积合并，但职责和契约不能混淆。

### 4.2 交互模型能力

在 `src/db/capabilities.rs` 引入 `InteractionModel::{Relational, KeyValue}`，作为现有能力的补充。

- SQL 产品继续 `Relational`；Redis 为 `KeyValue`。
- Redis M1：`catalog=false`、`relation_ddl=false`、`relation_edit=false`、`monitoring=false`、`manual_transactions=false`、`cancellation=false`。
- `cancellation=false` 表示不支持现有“服务端取消”契约；UI 仍可提供“停止等待/停止继续扫描”。
- 关系目录和键空间分支由模型能力选择；不要把 `catalog=false` 解释为整个左侧区域不可浏览。
- 驱动的 SQL execute、relation preview、DDL、事务入口收到 Redis 时返回 Unsupported，不能让 SQL 文本落入原生命令执行器。
- SQL 方言解析改为可失败入口，例如 `SqlDialect::try_for_database_kind`；逐个处理调用方，不以 panic 或默认 Generic 兜底。

### 4.3 数据身份与请求信封

建议契约如下，类型名是实施时的目标接口：

```rust
use uuid::Uuid;
use crate::identity::ConnectionIdentity;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RedisTarget {
    pub profile_id: Uuid,
    pub database: u32,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RedisKeyId {
    pub target: RedisTarget,
    pub key: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisRequestIdentity {
    pub connection: ConnectionIdentity,
    pub target: RedisTarget,
    pub owner_id: Uuid,
    pub generation: u64,
    pub request_id: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScanPosition {
    Start,
    Continue(u64),
    Complete,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyScanRequest {
    pub identity: RedisRequestIdentity,
    pub position: ScanPosition,
    pub pattern: Vec<u8>,
    pub count_hint: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyScanBatch {
    pub identity: RedisRequestIdentity,
    pub keys: Vec<Vec<u8>>,
    pub next: ScanPosition,
}
```

必须满足：

1. `Start` 发送游标 0；服务端返回 0 转为 `Complete`；`Continue(0)` 非法；Complete 不得再次发送。
2. 仅接受与当前连接、目标、owner、generation、request_id 全匹配的回复。
3. generation 在换 DB、换 MATCH、刷新、取消、关闭详情时推进；旧回复不能更新新状态。
4. 同一扫描 owner 一次最多一个在途请求；回复顺序不由网络完成时间推断。
5. key 可以为空字节串，也可以包含 NUL、换行和非法 UTF-8；不可用 trim 或展示文本参与身份判断。
6. profile scope 是显示/目标范围，不等于 Redis ACL 授权；每次执行校验最新允许目标，同时保留请求目标快照。

### 4.4 SCAN 与缓存策略

- 首次进入数据库只发送一个 SCAN；用户选择“继续加载”再发下一批，空批次显示“本批无匹配，可继续”。不自动扫完整库。
- 首版不提供 key 类型服务端过滤，避免把 SCAN TYPE 及模块类型兼容性带入主路径；可在后续增加。
- 去重按完整字节 key；同一轮保持首次发现顺序；不承诺全库排序或快照一致性。
- 搜索是显式 MATCH glob 模式；默认 `*`。本地 `/` 只搜索已加载标签，不产生数据库请求。
- 按冒号分组留待后续；首版使用平面列表，避免不完整扫描产生“完整目录”的错觉。
- 已加载计数表示客户端去重后的缓存数量；DBSIZE 如有展示，标为某时刻数据库总 key 数，不能当作 MATCH 结果总数。
- 每个键空间建议上限 10,000 个 key、16 MiB 原始 key 字节，以先到者为准；这些是初始默认值，测试后可调整。
- 达到缓存上限时暂停加载，保留当前内容并提示缩小 MATCH/重新扫描。不能丢掉超限批次的一部分后仍把新游标标记为可完整续扫。
- SCAN COUNT 建议 200；它不是硬上限。批次超过预算时明确标记结果受限并停止本轮，不声称扫描完成。
- TTL/type 元信息只对可见 key 懒加载，以有上限的 pipeline 获取；失败保留 key，不因 NOPERM 隐藏整个键空间。
- key 在 TYPE 与读取之间可能过期或变更类型；返回 Missing/TypeChanged 状态并允许重载，不报告内部错误。

### 4.5 类型化数据读取

| 类型 | 元信息 | M1 读取方式 | 语义 |
| --- | --- | --- | --- |
| String | STRLEN、PTTL | GETRANGE，按字节范围读取 | 长度变化时刷新；文本与 Hex 两种投影 |
| Hash | HLEN、PTTL | HSCAN | 游标继续加载；按 field 字节去重；动态值不具备快照保证 |
| List | LLEN、PTTL | LRANGE，限制 index 范围 | 可范围翻页；并发插删会导致重复/遗漏，显示非快照提示 |
| Set | SCARD、PTTL | SSCAN | 去重及预算语义与扫描一致 |
| Sorted Set | ZCARD、PTTL | ZRANGE start stop WITHSCORES | 按 rank 范围；并发修改时非稳定分页 |
| Stream | TYPE、PTTL | M1 不读取内容 | 明确提示 M2 支持 |
| 模块类型 | TYPE、PTTL（有权限时） | UnsupportedType | 不尝试按 String 强行读取 |

TTL 用类型表达：`Missing`（-2）、`Persistent`（-1）、`ExpiresIn { millis }`、`Unavailable`。记录采样时刻；倒计时是估计，显示到 0 不等于服务端已确认删除。

建议单次 String 范围不超过 64 KiB，集合请求提示/范围不超过 200 项，详情缓存默认 4 MiB。页面投影使用现有 CellValue::Bytes；key/member/field 的原始字节必须独立保留。

### 4.6 控制台文本语法与策略

M1 每次只执行当前物理行，或选中的一条命令；完整 buffer、多条命令、pipeline 脚本暂不支持。

- 空白分隔参数；单/双引号都用于保留空格；支持空参数 `""` 和 `''`。
- 支持 `\\`、`\"`、`\'`、`\n`、`\r`、`\t`、`\xNN` 转义；未知转义和未闭合引号报带范围的解析错误。
- 非转义 Unicode 编码为 UTF-8；`\xNN` 可以构造任意字节。换行字符必须转义，选区包含多个物理行则拒绝。
- 不支持 shell 展开、变量插值、注释和续行；分号不是命令分隔符。
- 命令名按 ASCII 不区分大小写；参数不变更大小写、不做 trim。
- 限制输入长度、参数数量、参数总字节；建议 64 KiB/128 个参数/64 KiB。
- 命令执行草稿保存完整 argv、目标、文档修订和请求身份；等待连接期间编辑文档不改变待执行内容。

M1 初始支持矩阵：

| 类别 | 命令 | 参数限制 |
| --- | --- | --- |
| 连通性 | PING | 无参数或一个受输入预算限制的参数 |
| 固定规模元信息 | TYPE、TTL、PTTL、STRLEN、HLEN、LLEN、SCARD、ZCARD | 恰好一个 key |
| 范围 String | GETRANGE | 一个 key 和合法非负 start/end；跨度 <= 64 KiB |
| 键枚举 | SCAN | 使用当前浏览器的扫描预算规则；只接受已支持的 MATCH/COUNT |

M1 不执行 GET/HGET/MGET/HGETALL/SMEMBERS/LRANGE 0 -1 等可能返回无界大数据的控制台命令；提示用户使用类型化查看器。类型化查看器内部同样存在大元素风险，按下节明确处理。

SELECT 在 M1 控制台中拒绝并提示使用 DB 选择器；AUTH/HELLO/CLIENT/READONLY/MULTI/EXEC/DISCARD/WATCH、订阅、阻塞命令、脚本、管理命令和未知命令都不进入共享执行路径。不能只根据 COMMAND 元信息自动开放它们。

M2 扩展矩阵时分别评估“读写性质”“连接状态影响”“回复大小”“是否阻塞”；只读不等于低资源消耗。

### 4.7 回复模型与资源限制

驱动提供独立 RedisReply，至少覆盖 RESP2 的 Null、Integer、Bytes、Status、Error、Array；M1 固定 RESP2，RESP3 支持放入后续任务，不用不完整转换静默丢弃 Map/Push 等数据。

- 整数回复是整数，不自动映射 affected_rows。
- 顶层命令错误与网络错误分开；数组中的错误元素保留位置。
- UI 显示转义/Hex，保留原始数据；不可把净化后的文本写回 key 身份。
- 限制展示节点数、递归深度和保留字节数；被截断回复必须包含原因和原始长度（可知时）。

**三种预算必须分开：**

1. 命令参数预算：发送前限制范围、项数提示、输入长度。
2. 解码/网络预算：客户端是否在分配巨大 RESP frame 前拒绝，必须在 Task 1 用可控 RESP 服务验证。
3. 模型/UI 预算：限制已解码数据的持有和展示，不能冒充前一层保护。

HSCAN/SSCAN 的 COUNT 不是上限；List/ZSet 即使只读取一个元素也可能遇到大元素。因此首版若 redis-rs 无可靠可配置 frame 限额，应明确记录“预览预算不是严格峰值内存保证”。禁止为绕过此限制自研完整 RESP 客户端；严格硬上限成为发布要求时，先收缩不满足的读取能力或单独评估具有限额的传输方案。

### 4.8 连接、TLS、错误和取消

- ConnectionProfile.database 缺失默认 0；输入 `0002` 规范化为 `2`；负数、溢出、非数字非法；数据库实际可用范围由服务端验证，不写死 0–15。
- 复用 CatalogScope 的数据库选择部分，Redis 的 schema 子选择固定 All，不展示/生成虚构 schema。Visible Objects 文案按 Redis 分支显示“可见数据库”；不是键授权机制。
- DB 选择器首版列出当前目标和用户显式配置的 DB，并允许输入编号；不依赖 CONFIG GET 才能使用。不能把 INFO keyspace 的非空 DB 列表当成全部可选 DB。
- `redis://` 表示明文连接，`rediss://` 表示验证证书的 TLS。Redis 不支持 SQL 式 Prefer 自动降级；不能满足的 SslMode 明确拒绝或在表单隐藏，不静默关闭验证。
- 密码走现有 SecretString/credential policy，不写入持久化 URL、日志和诊断；URL 百分号编码只解码一次。
- 用 PING 确认连通，版本/user 等元信息允许 Unknown。INFO/ACL 元信息被拒绝时，不把成功连接判成失败。
- 明确发现 cluster 模式或收到 MOVED/ASK 时提示首期不支持 Cluster，不以单机重试循环掩盖。无法查询模式的代理/服务不宣称已验证单机能力。
- 建连/响应超时分别配置，初始建议 5s/10s；元信息 pipeline 并发建议上限 16。不持有 Runtime 的全局 mutex 等待网络 I/O。
- reconnect 创建新连接并恢复固定 DB，再发新请求；不改变旧请求快照。
- M1 “停止”等于停止继续扫描/等待并忽略迟到回复。redis-rs 丢弃 future 不等于取消服务端已发送命令。
- 不为 Redis 开启 SQL cancellation=true，不显示“已回滚”。M2 写入超时显示“结果未知”，不自动重放写命令。
- profile 更新/删除/断开会退休旧连接及相关任务，所有克隆句柄的释放路径必须可测。

## 5. UI 与持久化契约

### 5.1 UI

- 保留 profile 根节点、分组、状态和连接管理；选择 Redis 根后，下方导航投影使用 KeyspaceState。
- M1 可在现有 Explorer 模型中以最小桥接节点承载 Redis DB/键空间入口，键列表由独立状态渲染；不把 key 放入 CatalogId。
- key 列表显示转义名称、类型、TTL（可选/加载中）；只读详情 Enter 打开，刷新和继续加载使用现有语义快捷键。
- 同一 RedisKeyId 复用已打开详情；key 改名视为另一身份，M2 再定义重定位。
- RedisConsole 不提供 SQL format、SQL diagnostics/completion、结果 SQL 查询栏、DDL 和 SQL transaction 操作。
- 复用 EditorWorkspace 的按 UUID 文本编辑。语言服务入口显式判断文档语言；Redis 文档不注册到 SQL LSP。
- M1 Redis 命令补全只提供支持列表中的命令名及参数提示，不通过后台扫描补全全库 key。
- 不仅处理渲染，还检查键盘 action、帮助、快捷键提示和命令面板入口。

### 5.2 持久化

- 为 Redis 文档增加独立记录：ID、名称、命令文本文件、目标、open 状态。保存为 UUID 命名的 `.redis` 文本，不混用 sql_file 的含义。
- 增加 RedisConsole/RedisKey 的 PersistedTab 变体；RedisKey 保存 profile/DB、base64 key 和展示偏好。
- 不保存回复内容、凭据、扫描游标、在途请求、代次、已缓存键和过期时间倒计时。
- 恢复的详情为未加载状态；进入后重新查询；key 已过期显示 Missing。
- 旧 workspace/profile 可读取并迁移，写入新版本；迁移原子保存，未知未来版本报错，不覆盖原文件。
- Redis 文档恢复时不依据 profile kind 将未知/缺失连接的文档误判成 SQL；显式持久化文档类型。
- 若全局多连接文档模型已先合入，扩展它的文档类型，避免另建 Redis 独立工作区集合。

## 6. M1 实施任务

每个任务按“行为测试 → 观察预期失败 → 实现 → 相关测试”的顺序进行。下列命令为实施阶段验收命令，本计划编写时未运行它们。新集成测试目标必须先创建后运行。

### Task 1：建立基线与客户端可行性探针

**Files:** 修改 `Cargo.toml`、`Cargo.lock`；新增 `tests/redis_adapter.rs`、`tests/redis_protocol_limits.rs`；必要时新增 `tests/support/redis.rs`、`tests/support/mod.rs`。

1. 检查 git diff 和两份关联计划的实施状态，记录当前 workspace/profile 版本。
2. 运行 `cargo test --test profile_url --test execution_target --test workspace_persistence --test sqlite_adapter`，记录基线及环境缺失。
3. 选择支持当前 Rust MSRV 的 redis-rs 稳定版本；从当前文档确认 Tokio/rustls feature、证书根加载和异步连接配置，锁定依赖。
4. 测试无认证、密码、ACL 用户、TLS、指定 DB；客户端必须独立于 SQLx 工作。
5. 用本地可控 RESP 服务验证慢响应、断连、巨大 bulk 声明、嵌套数组、future 丢弃和 socket 关闭；测试服务器响应必须有限，不分配恶意声明长度的数据。
6. 明确是否存在有效 frame/aggregate 限额，记录到 `docs/redis.md` 的限制部分；不因 UI 截断测试通过就宣称协议层有界。
7. 运行 `cargo check --all-targets` 和 `cargo test --test redis_protocol_limits`。

**验收：** 客户端能集成当前 Rust/Tokio/TLS；超时与取消语义可验证；资源限制有证据；依赖没有无意启用 Cluster/Sentinel 等首期不需要的功能。

### Task 2：增加产品标识、profile 和 URL

**Files:** 修改 `src/profile.rs`、`src/db/descriptor.rs`、`src/model/profile_manager.rs`、`src/ui/profiles.rs`、`src/ui/icons.rs`、`src/cli.rs`、`src/persistence/profiles.rs`；新增 `tests/redis_profile.rs`；扩展 `tests/profile_url.rs`、`tests/profile_draft.rs`。

1. 添加 Redis 标识、默认端口 6379、DB 默认 0 和 URL 格式列表。
2. 完成 redis/rediss 往返测试，覆盖无密码、ACL 用户、IPv6、保留字符、空 DB、非法编号和百分号编码。
3. 表单隐藏 schema/sqlite path，显示 DB 编号；Redis TLS 选项只出现实际支持的模式。
4. 复用凭据解析与字段原子更新，失败不部分改写连接字段，不保留错误产品的旧 URL。
5. scope 只支持 DB 选择，既有 SQL scope 序列化保持兼容；新增字段时补旧版本迁移。
6. 覆盖驱动切换与已有 profile 编辑，切回 SQL 后恢复 SQL 校验。
7. 运行 `cargo test --test redis_profile --test profile_url --test profile_draft`。

**验收：** 能保存和恢复 Redis profile；密码不出现在规范 URL 或文件中；不依赖数据库服务即可完成 profile 单元测试。

### Task 3：建立数据契约与能力分派

**Files:** 新增 `src/db/redis/types.rs`；修改 `src/db/mod.rs`、`src/db/capabilities.rs`、`src/model/execution_target.rs`、`src/sql/dialect.rs`、`src/agent/policy.rs`、`src/agent/service.rs`；新增 `tests/redis_contract.rs`；扩展 `tests/execution_target.rs`。

1. 实现第 4.3 节的类型和边界验证，支持空/二进制 key。
2. 增加 InteractionModel；把 SQL 方言获取改为可失败，修复全部调用方。
3. Redis 的 relation/catalog/SQL transaction/SQL execute 显式 Unsupported；合法 Redis 操作从专用入口进入。
4. Agent 连接列表允许 Redis，SQL query/execute/schema 工具在连接或执行前给出稳定 Unsupported 错误。
5. 用测试验证 Redis 不被 Generic SQL 分析，DB 规范化、scope 和 profile 身份校验正确。
6. 运行 `cargo test --test redis_contract --test execution_target --test agent_service` 与 `cargo check --all-targets`。

**验收：** 数据身份不依赖展示文本；不适用入口不会 panic，也不会误执行命令。

### Task 4：实现固定目标 RedisAdapter

**Files:** 新增 `src/db/redis/mod.rs`；修改 `src/db/mod.rs`、`src/runtime.rs`；扩展 `tests/redis_adapter.rs`、`tests/profile_runtime.rs`。

1. 用 Task 1 验证的配置构建客户端；密码通过结构化连接配置传递，不拼接到可打印 URL。
2. 实现 connect/probe/close、固定 DB、超时和错误映射。
3. INFO 等元信息降级为 Unknown；NOPERM、WRONGPASS、网络断开、WRONGTYPE、MOVED/ASK 分别处理。
4. 测试 DB 0/1 相同 key 不串数据；失败切库不替换旧目标。
5. 重连重新选择目标；关闭/退休后拒绝新任务并释放全部句柄。
6. 运行显式集成测试以及 `cargo test --test profile_runtime`。

**验收：** 当前单活动连接模式下切库正确；不偷偷实现另一套多连接池；probe 元信息受限不导致连接假失败。

### Task 5：实现键扫描驱动及纯状态机

**Files:** 新增 `src/db/redis/scan.rs`、`src/model/keyspace.rs`；修改 `src/model/mod.rs`；新增 `tests/redis_scan.rs`、`tests/keyspace_state.rs`。

1. 建立 fake batch 序列：空批次/非零游标、游标数值减小、重复 key、最后空批次/零游标、超预算批次。
2. 实现 Start/Continue/Complete 转换；不调用 CatalogCursor::keyset_parts 或 CatalogPage::validate_for。
3. 模型合并按原始 key 去重并保持首次发现顺序，保留空 key。
4. 增加 single-flight、MATCH 修改、刷新、错误后重试和取消状态。
5. 错误刷新保留旧内容并标 stale；成功新轮次替换旧轮次数据，不能混合两个 MATCH。
6. 实现内存/条数上限和 Paused 状态；不把受限扫描显示为 Complete。
7. 运行 `cargo test --test redis_scan --test keyspace_state --test catalog_contract`。

**验收：** Redis SCAN 合法空页不报错、不死循环；原有 SQL 目录分页测试通过。

### Task 6：接入异步 Redis 请求与 stale-result 防护

**Files:** 新增 `src/runtime/redis.rs`；修改 `src/action.rs`、`src/runtime.rs`、`src/app.rs`、`src/model/workspace.rs`；新增 `tests/redis_runtime.rs`、`tests/redis_reducer.rs`。

1. 定义语义 Command/Action：ScanKeys、LoadRedisMetadata、LoadRedisValue、ExecuteRedisCommand 及对应完成事件；全部携带请求身份。
2. Runtime 在获得固定目标连接后启动任务，不在 App::update 内做 I/O。
3. 模型只接受当前 request；切 profile/DB、改 MATCH、关闭 owner、断开后的结果全部丢弃。
4. 重复点击继续加载不并发发送相同游标；元信息批次限制并发且失败可局部降级。
5. 停止等待推进代次并停止后续派发；完成事件不得将 Cancelled 状态改为成功。
6. 用受控 channel 而非 sleep 排序复现乱序返回和切库失败。
7. 运行 `cargo test --test redis_runtime --test redis_reducer --test profile_runtime`。

**验收：** SQL/Redis 任务不会互相覆盖结果；所有状态变更通过 reducer。

### Task 7：实现只读类型化查看和回复模型

**Files:** 新增 `src/db/redis/read.rs`、`src/db/redis/reply.rs`、`src/model/redis_key.rs`；修改 `src/model/mod.rs`；新增 `tests/redis_values.rs`、`tests/redis_reply.rs`。

1. 实现第 4.5 节的类型读取与 TTL 模型，Missing 和 Persistent 不混淆。
2. String 按原始字节分段；显示 UTF-8 跨段边界时保留解码余量或退回 Hex，不能改变原始数据。
3. Hash/Set 使用独立游标；List/ZSet 使用范围状态，不套用“稳定总页数”的承诺。
4. 将可表格化数据投影到 ResultSet，保留原始身份与字节，不使用 affected_rows 表示回复整数。
5. 处理空值、嵌套回复、错误、非 UTF-8、终端控制字符、过期和 WRONGTYPE；类型变化后清除旧类型分页状态。
6. 预算截断有明确元信息，超大元素/扫描批次按第 4.7 节限制说明处理。
7. 运行 `cargo test --test redis_values --test redis_reply`。

**验收：** 五种核心类型可读；不会对 String 默认全量 GET；二进制寻址与展示分离。

### Task 8：实现命令解析、策略与执行草稿

**Files:** 新增 `src/redis_command/mod.rs`、`src/redis_command/parser.rs`、`src/redis_command/policy.rs`、`src/model/redis_console.rs`；修改 `src/lib.rs`、`src/model/mod.rs`；新增 `tests/redis_commands.rs`、`tests/redis_policy.rs`。

1. 为第 4.6 节语法建立 table-driven 测试，包含空参数、Unicode、\x00、引号、错误范围和多行拒绝。
2. 实现纯解析器，产生字节 argv，不执行 shell，不使用 split_whitespace 作为解析器。
3. 实现明确命令/参数支持表；检查大小写、参数数量、GETRANGE 范围和 SCAN COUNT 上限。
4. 对 GETDEL、脚本、事务、订阅、SELECT、未知子命令和超预算请求拒绝；不只检查命令的首词是否以 GET 开头。
5. 构造不可变 RedisExecutionDraft，复用文档 UUID 和请求代次；编辑器变化不改写待执行快照。
6. 连接前本地验证，执行前再次验证目标和策略；profile.read_only=false 不开放 M1 写操作。
7. 运行 `cargo test --test redis_commands --test redis_policy --test redis_reducer`。

**验收：** 支持列表外的命令不会到达客户端；合法转义参数精确到字节；错误可定位当前文档。

### Task 9：接入键空间 UI、详情标签和控制台

**Files:** 新增 `src/ui/keyspace.rs`、`src/ui/redis_key.rs`、`src/ui/redis_console.rs`；修改 `src/ui/mod.rs`、`src/model/tab.rs`、`src/model/explorer.rs`、`src/app.rs`、`src/editor/mod.rs`、`src/ui/shortcut_hints.rs`、`src/help.rs`；新增 `tests/redis_ui.rs`；扩展 `tests/workspace_tabs.rs`、`tests/ui_render.rs`。

1. 增加 RedisConsole/RedisKey 标签访问器及穷举匹配，SQL 标签行为维持现有契约。
2. 复用 profile 根层和布局，Redis 内容投影到独立 keyspace UI；/ 为本地查找，MATCH 为服务端扫描。
3. 实现 Enter 打开、继续加载、刷新、切 DB、Missing/Unavailable/Paused/Failed 等状态。
4. 复用 EditorWorkspace 文本编辑；给 SQL 分析、格式化、补全、LSP 派发增加显式语言门控。
5. 接入受限命令名补全、范围错误和原生回复展示；停止操作准确标注“停止等待”。
6. 宽/窄终端渲染覆盖长 key、NUL/ANSI 字节、空 key、重复显示名，选择仍定位正确原始 key。
7. 运行 `cargo test --test redis_ui --test workspace_tabs --test ui_render --test redis_reducer`。

**验收：** UI 完成连接 → 扫描 → 详情 → 命令 → 切库闭环；SQL-only 快捷键在 Redis 上无副作用。

### Task 10：持久化与恢复

**Files:** 修改 `src/persistence/workspace.rs`、`src/model/workspace.rs`、`src/model/workspace_save.rs`、`src/app.rs`、`src/runtime.rs`；新增 `tests/redis_workspace.rs`；扩展 `tests/workspace_persistence.rs`。

1. 按当前最新版本增加 Redis 文档记录和 Tab 变体，定义迁移；不与并行多连接计划争用固定版本号。
2. `.redis` 文档通过现有原子保存队列写入 UUID 路径；key 使用 base64 表示，并测试空 key。
3. 恢复目标及文档内容，详情/键空间重新加载；不恢复 stale 游标和旧 connection generation。
4. 缺失 profile 保留失效目标与文档；不能将 Redis 文档静默当 SQL 打开。
5. 覆盖旧 SQL 工作区迁移、混合标签顺序、关闭再打开、未知新版本和保存失败重试。
6. 运行 `cargo test --test redis_workspace --test workspace_persistence --test workspace_tabs`。

**验收：** 重启不丢文档和目标；旧 workspace 正常读取；结果和凭据不进入文档清单。

### Task 11：集成矩阵、性能边界与用户文档

**Files:** 新增 `docs/redis.md`；修改 `docs/database-capabilities.md`、`docs/configuration.md`、`docs/keybindings.md`、`docs/architecture.md`、`docs/coding-agent-access.md`、`README.md`；完善全部 Redis 测试。

1. 完成第 7 节测试矩阵；独立 Redis 实例显式运行集成测试，不因 URL 缺失静默通过。
2. 在 100,000 个短 key 的专用实例中验证首屏只发一批、UI 可操作、缓存到上限停止；记录硬件、版本、内存与延迟，不给出未测性能声明。
3. 验证认证失败、INFO/TYPE 权限缺失、TLS、断网、重连、切库、刷新过期和大数据限制。
4. 更新支持矩阵和控制台命令清单，明确 SCAN 非快照、COUNT 提示、协议解码预算和取消语义。
5. 运行 `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test`。
6. 最小 feature 构建运行 `cargo check --no-default-features --all-targets`；如客户端配置确有必要，增加对应 feature 组合验证。
7. 记录需外部数据库/Oracle 客户端的既有测试状态，区分已有失败、环境不足和回归。

**验收：** M1 所有硬性行为测试通过；限制准确公开；SQL 回归检查完成。

### 6.1 任务依赖

```text
Task 1 → Task 2 → Task 3 → Task 4
                    │          │
                    ├→ Task 5 ─┴→ Task 6
                    ├→ Task 7 ───→ Task 9
                    └→ Task 8 ───→ Task 9
                              Task 6 → Task 9 → Task 10 → Task 11
```

Task 5/7/8 的纯模型可以分别推进，但驱动集成依赖 Task 4；共享 app/runtime/tab 文件必须顺序集成。本文不要求启动子代理或自动提交。

## 7. 测试方法与明确验收

### 7.1 测试分层

| 层 | 必测行为 | 运行方式 |
| --- | --- | --- |
| 纯解析/契约 | 字节、引号、非法游标、目标/策略 | 默认 cargo test，无服务依赖 |
| 状态机/reducer | 空批次、去重、预算、请求乱序、关闭后返回 | fake adapter/受控 channel |
| 协议边界 | frame 声明、嵌套、超时、断开、取消 | 受控本地 RESP 服务，默认测试 |
| 真实 Redis | 认证、DB、SCAN、五种类型、TTL | 显式 ignored 集成测试 |
| UI | 窄终端、二进制显示、键盘状态和错误 | Ratatui TestBackend/现有渲染测试模式 |
| 持久化 | 旧版本迁移、Redis 文档、二进制 key | tempfile，无 Redis 服务 |
| SQL 回归 | Catalog、profile、workspace、SQL 执行 | 现有相关测试及最终全套 |

### 7.2 真实服务测试约定

新建 `tests/redis_adapter.rs` 的真实服务测试使用 `#[ignore = "requires isolated Redis"]`；显式运行时缺少 `LAZYDB_TEST_REDIS_URL` 必须失败而不是 return 成功。纯函数测试放在其他目标，不依赖该变量。

仅针对测试专用 Redis：

```bash
docker run --detach --rm --name lazydb-redis-test --publish 127.0.0.1:16379:6379 redis:7.4 redis-server --save "" --appendonly no
LAZYDB_TEST_REDIS_URL=redis://127.0.0.1:16379/0 cargo test --test redis_adapter -- --ignored --test-threads=1
docker stop lazydb-redis-test
```

测试以 UUID 前缀创建数据；只清理本测试创建的 key，不执行 FLUSHDB/FLUSHALL，不修改实例全局配置。空 key 测试只在上述明确隔离实例执行并自行清理。DB 0/1 测试和 ACL/TLS 测试使用独立 fixture，密码不打印到失败输出。

Redis 6.2/8.x 重复同一测试矩阵，镜像使用当时可用且明确的补丁版本或 digest；7.4 命令仅为本地启动示例，不代表测试已经完成。TLS/ACL fixture 必须由实施任务补齐，不将明文测试成功当作 TLS 验收。

### 7.3 M1 发布检查表

- [ ] URL/profile/凭据保存和恢复正确，原始密码不会泄漏。
- [ ] 当前 DB 可输入合法编号，不要求 CONFIG GET 权限。
- [ ] 切库、重连、失败回退不串 key；Redis DB 目标始终可见。
- [ ] 空/二进制/含控制字符 key 可以读取、选择、恢复身份。
- [ ] SCAN 非递增游标、空批次、重复 key 均正确；无全库自动扫描。
- [ ] TYPE/TTL 权限失败可降级；key 过期/类型变化正常展示。
- [ ] String/Hash/List/Set/ZSet 读取方式和预算符合契约。
- [ ] 控制台只执行清单中的命令和参数；SQL 工具明确拒绝 Redis。
- [ ] 停止等待不宣称服务端取消；旧回复无法覆盖新标签或目标。
- [ ] 旧 workspace 可迁移；不持久化回复、扫描游标及运行状态。
- [ ] 已记录 redis-rs frame 限额验证结论，未将显示限制宣传成内存硬上限。
- [ ] fmt/clippy/相关测试与最终回归状态有记录。

## 8. M2：写操作、Stream 和 Agent 实施包

M2 以前述 M1 契约稳定为前提；每个任务可单独交付，不要求所有写类型一次完成。

### Task 12：类型化写入模型与策略

**Files:** 新增 `src/db/redis/write.rs`、`src/model/redis_edit.rs`；修改 `src/redis_command/policy.rs`、`src/action.rs`、`src/runtime/redis.rs`、`src/ui/redis_key.rs`、`src/ui/execution_confirm.rs`；新增 `tests/redis_mutations.rs`、`tests/redis_write_policy.rs`。

1. 先支持 String 替换、Hash 单字段写入、单 key 删除和 PTTL 修改，使用字节参数，复用现有确认/环境策略。
2. String 编辑默认保留 TTL（使用服务端支持的 KEEPTTL）；创建/覆盖/删除/过期修改分别呈现意图。删除使用明确的单 key 命令，不由 glob 隐式展开。
3. 对已过期 key 的编辑默认报告冲突，不静默重新创建；“新建”是独立意图。
4. 普通 GET 后再 SET 不是并发保护。若承诺冲突检测，采用独占连接 WATCH + 读取比对 + MULTI/EXEC，处理 EXEC 被放弃和所有 UNWATCH/DISCARD 清理路径；不向 SQL 事务 UI 暴露。
5. 未获得独占连接或相关 ACL 时报告不支持冲突保护，不静默退化为覆盖。大值 compare 必须有读取预算。
6. 已发送写操作遇断网/超时标记结果未知，不自动重试；完成后按原始 key 刷新，连接状态重建后才能继续操作。
7. 运行 `cargo test --test redis_mutations --test redis_write_policy --test redis_reducer`，真实服务补并发修改、TTL、过期和断网验收。

**验收：** 写入意图、TTL 和并发冲突策略明确；read_only profile 在 UI、runtime 和 Agent 边界都不能写。

### Task 13：Stream 与命令补全扩展

**Files:** 修改 `src/db/redis/read.rs`、`src/model/redis_key.rs`、`src/ui/redis_key.rs`、`src/redis_command/policy.rs`、`src/ui/redis_console.rs`；新增 `tests/redis_stream.rs`；扩展 `tests/redis_commands.rs`、`tests/redis_values.rs`。

1. 用 XRANGE/XREVRANGE 带 COUNT 做 Stream 读取，明确 exclusive ID 续读和删除后空批次；field/value 保留顺序与重复字段，不强制转 Map。
2. 扩展命令名、子命令和参数补全，不用服务端 COMMAND flags 直接替代本地策略。
3. 若开放 GET/HGET 或更多集合命令，必须先确定大回复限制策略，更新支持矩阵和预算测试。
4. 多命令执行单独定义失败停止、逐条结果和确认快照，不把换行 split 直接当完整脚本支持。
5. 运行 `cargo test --test redis_stream --test redis_commands --test redis_policy --test redis_values`。

**验收：** Stream ID 续读不重复边界项；命令扩展不绕过预算和连接状态限制。

### Task 14：Redis Agent/CLI/MCP 接口

**Files:** 修改 `src/agent/service.rs`、`src/agent/policy.rs`、`src/agent/types.rs`、`src/agent/cli.rs`、`src/agent/mcp.rs`、`src/cli.rs`；新增 `src/agent/redis.rs`、`tests/agent_redis.rs`；修改 `docs/coding-agent-access.md`。

1. 提供独立的 redis_scan、redis_read、redis_command 接口，不复用参数名 sql 或 QueryOutcomeJson 冒充 SQL 结果。
2. 首批只开放只读接口；复用项目可见性和凭据解析，使用和 TUI 同源的命令策略。
3. 二进制参数显式使用 text/base64 编码标签，返回值保留类型；不要推测普通字符串是否 base64。
4. 分页 token 绑定 profile、DB、MATCH 和 token 格式版本；无状态 headless 调用不继承 TUI generation，验证持久目标和 scope 后为每次调用创建独立运行身份。
5. 限制每次工具的请求数、时间、元素和输出字节；SCAN 的跨调用去重不承诺由服务端保证，向调用方说明非快照和重复可能。
6. 后续写工具复用 WritePolicy，未知命令、脚本、事务控制不得借通用 command 绕过。
7. 运行 `cargo test --test agent_redis --test agent_service`，真实 Redis 验证 JSON/CLI/MCP 的目标、二进制和预算语义。

**验收：** SQL API 向后兼容；Redis 访问在所有入口共享可见性和读写策略。

## 9. M3：高级能力任务与启动条件

### Task 15：Redis Cluster

**Files:** 新增 `src/db/redis/cluster.rs`、`tests/redis_cluster.rs`；修改 `src/profile.rs`、`src/model/profile_manager.rs`、`src/db/redis/types.rs`、`src/db/redis/scan.rs`、`src/model/keyspace.rs`、`docs/redis.md`。

- 单独配置拓扑模式和 seed endpoints，DB 强制 0，凭据/TLS 应用于重定向目标。
- 验证所选客户端 SCAN 路由，明确每个 primary 的独立游标；不能把对一个节点的 SCAN 当完整集群扫描。
- 续扫状态包含拓扑版本、节点身份、节点游标和已完成节点；扩缩容/故障切换时重启或标记不完整，不能静默漏扫。
- 去重、内存上限、MOVED/ASK、CROSSSLOT、resharding 和节点失败全部验收。
- `cargo test --test redis_cluster -- --ignored --test-threads=1` 使用独立多节点 fixture；缺 fixture 必须失败。

**启动条件：** M1 已稳定；已确认用户需要集群。**完成条件：** 拓扑变化下浏览状态正确，不能仅以 GET/SET 路由成功验收。

### Task 16：Sentinel

**Files:** 新增 `src/db/redis/sentinel.rs`、`tests/redis_sentinel.rs`；修改 profile、凭据解析及 RedisAdapter 连接建立路径。

- 配置 sentinel endpoints/service name，区分 Sentinel 与数据节点认证/TLS。
- failover 后退休旧主连接、恢复目标 DB、推进会话代次；未确认写入不重放。
- fixture 模拟主节点失效，验证文档、扫描和请求不串目标。
- 运行 `cargo test --test redis_sentinel -- --ignored --test-threads=1`。

**完成条件：** 故障切换可恢复只读访问，旧响应无效；配置迁移和凭据无混用。

### Task 17：监控、Pub/Sub 与状态型命令

**Files:** 监控新增 `src/db/redis/monitor.rs`、`tests/redis_monitor.rs`，修改 `src/model/dashboard.rs`、`src/ui/dashboard.rs`；订阅另增 `src/model/redis_subscription.rs`、`src/runtime/redis_subscription.rs`、`tests/redis_subscription.rs`。

- 监控定义 Redis 指标（ops、used memory、clients、hit/miss、evicted/expired keys、replication lag 等），不伪装成 commits/WAL。
- INFO 部分被拒绝时按指标降级；计数器重置/重连处理复用现有 gauge/counter 思路。
- Pub/Sub 使用独占连接与专用标签，定义消息保留条数/字节、背压、丢弃计数、断线恢复和关闭流程。
- 阻塞命令不进入普通多路复用控制台；Redis 事务如需支持，用队列/EXEC/DISCARD 模型独立设计，不提供执行后 Rollback。
- 运行 `cargo test --test redis_monitor --test redis_subscription`，再执行各自真实服务 fixture。

**完成条件：** 高流量订阅不阻塞普通查询和 UI；监控刷新单飞且旧连接指标不会污染新会话。

## 10. 工作量与风险管理

以下为排期参考，不是经过原型测量的承诺。按一名熟悉 Rust/TUI 与当前代码的开发者估算：

| 工作包 | 参考有效开发日 | 主要不确定性 |
| --- | --- | --- |
| Task 1–4：客户端/profile/分派/连接 | 4–7 | 客户端 MSRV/TLS、SqlDialect 调用方范围 |
| Task 5–8：扫描/状态/读取/解析 | 6–10 | 二进制、预算、异步状态覆盖 |
| Task 9–10：UI/编辑器/持久化 | 5–9 | 当前 SQL 假设与多连接文档重构的交叉 |
| Task 11：集成/回归/文档 | 3–5 | ACL/TLS fixture、平台及旧测试环境 |
| M1 合计 | 18–31 | 在 Task 1 与 Task 6 后重新估算 |
| M2 | 另行估算，建议分任务发布 | 冲突检测、大回复、Agent 公共接口 |
| M3 | 每项原型后独立估算 | 拓扑、故障切换、背压和连接生命周期 |

重点风险及应对：

- **SQL 假设分散：** 用可失败方言入口和 enum 穷举编译错误定位调用方，配合实际 UI/Agent 回归；不全仓机械替换 SQL → query。
- **Catalog 与 SCAN 错误复用：** 独立模型和测试证明空批次/非递增合法；SQL 目录测试作为保护。
- **大回复超内存：** 在原型期验证协议层限额；限制命令集合、分段读取、预算透明；需要严格硬上限时先收缩范围。
- **并行重构冲突：** 以当前 session/document 契约接入，不同时建立第二套全局会话管理。
- **恢复格式冲突：** 迁移版本在合入前统一分配，兼容旧数据，未知版本拒绝覆盖。
- **只读权限不完整：** PING/INFO/TYPE 的权限区分处理，显示局部不可用，不要求管理员权限才可连通。

## 11. 推荐交付顺序

1. 先完成 Task 1–4，证明连接与 SQL 分流正确。
2. 完成 Task 5–7，形成“连接 → SCAN → 五种类型只读查看”的内部演示。
3. 完成 Task 8–10，形成可恢复的控制台和标签闭环。
4. Task 11 验收后发布 M1；根据用户使用反馈选择 M2 优先任务。
5. Cluster、Sentinel、订阅分别评估和交付，不以“支持 Redis”自动暗示它们已可用。

## 12. 参考资料

- [Redis SCAN](https://redis.io/docs/latest/commands/scan/)：COUNT 提示、空批次、游标结束和重复元素。
- [Redis SELECT](https://redis.io/docs/latest/commands/select/)：连接级 DB 与 Cluster DB 0。
- [Redis Transactions](https://redis.io/docs/latest/develop/using-commands/transactions/)：MULTI/EXEC/DISCARD 与 SQL 回滚差异。
- [redis-rs](https://github.com/redis-rs/redis-rs)：Tokio/TLS feature 与异步客户端配置。
- [redis-rs MultiplexedConnection](https://docs.rs/redis/latest/redis/aio/struct.MultiplexedConnection.html)：future 丢弃与服务端取消的区别。
- [LazyDB Architecture](../architecture.md)：reducer/runtime、工作区、连接和持久化边界。

客户端 API 以 Task 1 锁定的版本为准；制定计划时查询的最新文档不替代编译和真实服务验证。
