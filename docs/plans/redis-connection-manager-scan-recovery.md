# Redis 断线恢复实施计划

**Goal:** Redis socket 断开后恢复正常扫描，用户无需在 Explorer 先关闭连接；恢复过程有界且不会自动重放写操作。

**Architecture:** 使用 redis-rs 1.5.0 的共享 ConnectionManager 替代永久持有的 MultiplexedConnection；SCAN 在明确传输错误后最多补发一次。保留现有逻辑 session、数据库目标和请求代次，使用协议级故障注入验证真实传输恢复。

**Tech Stack:** Rust 1.94、Tokio、redis-rs 1.5.0、现有 App/Runtime action-command 架构。

---

## 执行约束

- 分析依据：同目录 `analysis.md`。起点 `23a638fbc953adb5f956488693ab2d4825594581`，目标分支 main；分析时 HEAD 与起点一致，业务工作区干净。
- 实施、审查、纠偏、提交合并均由 Luna 完成；工作流和任务分支名由 Luna 确定。
- 本计划阶段只写任务目录，不创建 worktree、不改业务代码/index、不 stash、不改 state.json/checkpoint.json。
- 实施开始先检查实际 HEAD/diff 和插件 checkpoint（若存在）；未提交内容不自动复制进新 worktree，不覆盖用户修改。
- 每个单元完成业务闭环后继续下一单元。真实运行结果追加 validation.md，不把分析阶段静态结论当成测试通过。

## 需求、门禁与验证等级

| 类别 | 内容 | 完成判定 |
| --- | --- | --- |
| 用户需求 | broken pipe 后能够恢复使用，按 r 不再永久报错，不必先 x 关闭连接 | 用断线协议回归和 App 刷新测试证明 |
| 项目已有强制门禁 | `.github/workflows/ci.yml` 的 Rust fmt、all-targets/all-features clippy 和测试 | 单元 4 命令成功；记录实际工具链，CI 要求 1.94.0 |
| 本修复的自动化回归 | 断线恢复、一次补发边界、写操作不重放、目标 DB/并发/旧响应隔离 | 各单元定向命令成功，新测试不是 ignored；这些是方案验证，不冒称用户指定了测试方式 |
| 项目数据库 CI | 已有 Redis mutation ignored 用例在 CI Redis service 上运行 | 沿用现有 CI；本地有隔离实例时补跑，不把所有 ignored 用例扩张为本地必需门禁 |
| 补充建议 | 真实 Redis 重启/CLIENT KILL、真实 TLS、人工或 PTY 操作 | 有条件时补证；不能因缺少这些新建议自动阻止交付，不要求用户人工验收 |

25 秒总预算及退避数值是本计划设计取舍，不是用户提出的协议要求。实现时如有驱动行为或既有入口预算冲突，Luna 可调整并记录理由，保持有限恢复和不重放写操作的验收语义。

## 变更范围与本地依赖

同目录 `change-scope.json` 列出预计业务/测试/文档变更，包括条件性修改路径。没有预计删除或重命名。plan 阶段重新检查的 HEAD 仍为指定起点，status/diff stat 为空，无未提交行为依赖需要移入 worktree。报告、计划、验证记录和阶段回执属于任务元数据，不属于业务提交范围。

条件性路径：`src/db/redis/discovery.rs` 仅在入口等待预算需要调整时修改；`src/db/redis/mutation.rs` 仅在共享连接类型/超时兼容确有需要时修改，不能加入写重试；`src/app.rs`、`src/model/keyspace.rs`、`src/runtime.rs` 仅修复回归揭示的生命周期缺陷。参考用的 `tests/redis_protocol_limits.rs`、`tests/redis_contract.rs`、`tests/redis_browser_tabs.rs` 和 CI 文件只读取/运行，不计划改动。

## 单元 1：失效传输后的扫描恢复

### 文件

- 修改 `Cargo.toml`、`Cargo.lock`。
- 修改 `src/db/redis/mod.rs`、`src/db/redis/reconnect.rs`、`src/db/redis/read.rs`。
- 条件性兼容修改 `src/db/redis/discovery.rs`、`src/db/redis/mutation.rs`，范围见上文。
- 新增 `tests/redis_reconnect.rs`。

### 步骤

1. 在新测试文件实现最小 RESP TCP fixture。参考 `tests/redis_protocol_limits.rs`，正确解析请求，支持初始化 CLIENT、PING、AUTH/SELECT 和 SCAN，支持多个连接及明确的断线信号；维护请求计数与握手记录，测试结束终止 listener/worker。
2. 写失败回归：adapter 建连成功，server 接收第一次 SCAN 后关闭 socket；下一条连接返回完整 SCAN 响应。断言同一个 adapter 的 scan_keys 能得到正确结果。用外层 timeout 保证测试不会挂住。
3. 运行 `cargo test --test redis_reconnect`，记录旧实现因断线返回错误的失败证据；不能把 fixture 握手错误当成目标回归失败。
4. Cargo redis feature 增加 `connection-manager`，保持固定版本。通过 Cargo 正常更新 lockfile。
5. RedisAdapter 的 connection 字段和 connection_clone 改为 ConnectionManager；用同一个已配置 Client 创建 manager，保留 URL 编码、认证、TLS、目标 database。
6. 使用 `ConnectionManagerConfig`：connection timeout 5s、response timeout 10s、初次之外最多 3 次重试、min delay 250ms、max delay 1s。核对 1.5.0 构造 API；把 reconnect.rs 策略接入实际配置，避免维护两套失联策略。
7. 初次建连和单次 scan_keys（含补发）分别限制整体预算 25s；预算到期返回明确错误。若其他入口等待 manager 导致既有操作预算失效，补上入口级有界等待，复用小型帮助函数，不建立通用业务重试器。
8. SCAN 保留原始 RedisError，先排除 timeout，再判断 `is_unrecoverable_error()`/I/O 断线错误是否允许一次补发（timeout 可能也是 I/O 错误，不能仅用 is_io_error 放行）。普通响应超时、权限、认证及命令错误不做应用层补发。原命令 cursor/pattern/count 不变，最后再映射 DatabaseError。
9. read.rs 中三个绑定 MultiplexedConnection 的 helper 同步调整类型。discovery/mutation 等通过 connection_clone 自然获得新连接基础设施；保持业务写调用次数不变。
10. 运行 `cargo test --locked --test redis_reconnect`，确保目标回归通过。

### 完成条件

同一 adapter 在服务端主动断开 socket 后能够通过重连返回有效 SCAN 结果，整体等待有界。不是仅验证配置对象或编译通过。

**单元复核：** fixture 首次失败确实由目标 SCAN 断线引起；补发只有一个分支、最多一次；超时包住完整建连含初次 PING/扫描流程；没有新建每请求 manager。定向测试预期退出码 0，新增恢复测试实际执行而非跳过。

## 单元 2：并发、目标隔离和重复故障

### 文件

- 扩展 `tests/redis_reconnect.rs`。
- 必要时调整 `src/db/redis/reconnect.rs` 的内部配置构造以支持短预算测试；不为测试增加用户公开设置。

### 步骤

1. 建连后预先 clone adapter；断线后并发发起多个读请求。验证克隆共享恢复结果，单次故障不会为每个请求创建独立重连连接。
2. 使用非零 DB，记录每次新连接的 SELECT。验证重连前后都访问原数据库；另建第二个 DB adapter，确认未被串库。
3. AUTH fixture 验证每次重新建连都进行认证，日志/错误不泄露密码。TLS 沿用同一 Client 和已有 rediss 配置路径，通过代码审查确认未降级；有可用隔离 TLS 环境时补充实测。
4. 模拟一次重连周期失败耗尽，随后 server 恢复；下一轮 scan_keys 必须可以恢复，不能永久停留在失败 future。
5. 服务持续不可达时，扫描在预算内返回失败；以受控信号和测试专用短预算避免长 sleep。不给生产配置附加无限重试。
6. 运行 `cargo test --locked --test redis_reconnect`。

### 完成条件

克隆共享恢复、不同 DB 不串用、故障耗尽后仍能再次恢复，失败请求及时结束。

**单元复核：** 并发断言针对一次确定的故障，避免把正常重连退避多次尝试误判为风暴；DB/AUTH 校验实际网络握手而非字段值；错误日志不打印凭据。定向测试预期退出码 0。

## 单元 3：写操作与 UI 生命周期回归

### 文件

- 扩展 `tests/redis_reconnect.rs`。
- 扩展 `tests/redis_loading_lifecycle.rs`、`tests/redis_scan.rs`；按现有测试组织选择最合适位置，避免重复同一断言。
- 仅测试揭示缺陷时修改 `src/app.rs`、`src/model/keyspace.rs` 或 `src/runtime.rs` 的最小相关范围。

### 步骤

1. fixture 收到 DEL 后记录执行计数并关闭连接、不返回响应；adapter 返回错误，后续读可以恢复，DEL 计数仍为 1。
2. 通过 execute_mutation 真实入口模拟 EVAL 执行后丢失响应，记录 EVAL 计数为 1。另验证 EVAL 成功但后续 metadata 读取失败时，不重新执行 mutation。
3. NOPERM 等业务错误不触发 SCAN 第二次发送；常规响应超时不被当成永久断线反复重放。
4. App 中构造 Failed 的 Redis browser，发送 RedisRetryScan：新扫描从 cursor 0 开始，request/generation 更新，已有 snapshot 和未保存编辑内容保留。
5. 发送旧请求结果，断言不能污染新扫描；发送新结果，断言失败状态解除。
6. 恢复等待期间切 DB、关闭 tab、disconnect，验证延迟结果不能复活关闭状态或更新其他目标。优先复用现有 action 测试方式，无需依赖 PTY。
7. 运行 `cargo test --locked --test redis_reconnect --test redis_scan --test redis_loading_lifecycle --test redis_browser_tabs --test redis_protocol_limits`。

### 完成条件

写失败不会自动重复提交；r 恢复保持已有编辑/浏览状态；陈旧结果继续受 identity 隔离。

**单元复核：** DEL/EVAL 断言检查服务端接收次数，不能只检查客户端错误；未保存内容通过真实 editor 状态验证；关闭/切库后请求身份严格匹配。定向组合测试预期退出码 0。

## 单元 4：文档、完整验证和交付

### 文件

- 修改 `docs/redis-browser.md`。
- 向任务目录 `validation.md` 追加实际证据。

### 步骤

1. 文档说明：自动恢复物理连接，SCAN 有一次有限补发；服务长时间不可达时显示失败，之后可按 r 重试；写失败不会自动重放。不要承诺扫描快照一致性或所有读操作首次失败均被隐藏。
2. 使用隔离临时 Redis 实例做补充验证，覆盖非零 DB、认证、定向断开连接/重启、扫描/预览/监控恢复。本机分析时存在 redis-server 命令，但尚未验证实例可运行。所有故障注入只对测试实例执行。
3. 现有 redis_contract 集成用例为 ignored 且部分硬编码 DB 0，按原约定运行；非零 DB 用新增回归，不机械修改 URL 套跑旧用例。
4. 实现齐备后运行一次：
   - `cargo fmt --all -- --check`
   - `cargo clippy --locked --all-targets --all-features -- -D warnings`
   - `cargo test --locked --all-targets --all-features`
5. 记录工具链版本、代码 commit/diff、命令和退出状态。项目 CI 为 Rust 1.94.0，分析时本机为 1.94.1；如用本机版本验证，明确差异。数据库 ignored 测试未运行时不能报告其通过。
6. Luna 收尾审查：确认依赖锁定、克隆共享、命令重试分类、预算、认证/TLS/DB、不重放写操作、旧请求隔离和文档一致。修复后仅重跑受影响检查。
7. 按工作流进行提交及合并。提交范围只包含本任务业务/测试/文档变更，任务目录报告保留在 .git 内。

**完成标准：** 单元 1—3 自动化验收完成，fmt/clippy/完整测试退出码 0，Luna 审查无未处理问题，文档和实际恢复行为一致。真实实例/PTY/TLS 的补充限制可记录，不转化为新人工门禁。

**补充命令（仅已有隔离 DB 0 实例时）：** `LAZYDB_TEST_REDIS_URL=redis://127.0.0.1:<隔离端口>/0 cargo test --locked --test redis_contract --test redis_mutation -- --ignored --test-threads=1`。实际执行时替换为测试实例端口，在 validation.md 记录完整命令，禁止套用用户真实连接。命令预期退出码 0，但本地补跑不是新增强制人工门禁。

### 环境限制处理

常规回归和项目 Rust 检查是交付验证；PTY、隔离真实 Redis/TLS 为补充检查。环境受限的补充检查最多一次有针对性修复重试，之后由 Luna 决定其他证据或记录限制；不得循环 progress 重试同一环境。

## 关键设计边界

- 物理 socket 恢复不递增逻辑 ConnectionIdentity，不销毁 session 或清空草稿。
- manager 原生重连不重放命令；应用层重试仅添加到 SCAN，不包裹 mutation/delete 流程。
- metadata cache 现有 1 秒 TTL 保持，不声称重连后强一致，不扩展成重连通知总线。
- SCAN 原本不是快照；恢复不提供额外快照保证。手工 r 从 cursor 0 重启扫描。
- drop 一个 adapter 不代表所有 clone 同步取消；保持有限后台生命周期及 UI 过期结果防护。

## 当前状态与下一步

分析和计划已完成；业务实现、运行时故障回归及 Rust 检查尚未执行。下一步由 Luna 命名任务分支，从指定起点完成单元 1 的失败回归与扫描恢复，然后顺序完成其余单元，无需用户再次选择实现方案。
