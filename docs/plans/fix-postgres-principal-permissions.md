# 用户与角色权限详情修复实施计划

**Goal:** PostgreSQL 用户/角色权限可读取、可完整浏览，读取错误可诊断、可重试，受支持的授权操作执行后正确刷新。

**Architecture:** 沿用 adapter → PrincipalDetails → reducer → TUI；PostgreSQL 以原生 catalog ACL 为主要来源。详情携带独立的操作目标，展示文本不参与 SQL 目标解析。读取状态、覆盖范围与可变更能力分别表达。

**Tech Stack:** Rust 1.94 / SQLx / Tokio / Ratatui / PostgreSQL。

**执行者：** Luna，包含实施、审查、纠偏、提交合并。Astra 本阶段仅制定计划。

## 基线与约束

- 原工作空间 `/Users/yelog/workspace/tui/lazydb`，起点 `5f8c149a947364eac09eb761a8141a93adcba4a9`，目标 `main`；任务名及分支由 Luna 自动命名。
- 根因、源码位置、dbx 对比及取舍详见同目录 `analysis.md`；该报告包含技术设计细则，本文定义执行顺序和验收门槛。
- analyze 时工作区干净，没有需要复制的未提交行为。实施前重新检查实际状态；不清空、不 stash、不批量提交原工作区。
- 当前任务目录没有 checkpoint.json；后续若插件生成，则优先读取，但不能把历史 next 覆盖当前阶段要求。
- 报告、计划、验证记录仅写当前任务目录；不改插件 state.json/checkpoint.json。
- 本轮正式 plan 回执为 `plan-8fdef47b-5280-44b9-b72c-be8fa5ff7adb.json`；写入前完成 `change-scope.json`，不覆盖历史 analyze 回执。
- 正式 plan 阶段再次核对：HEAD 仍为指定起点，工作区及 index 无差异；本计划不依赖任何未提交文件。未提交文件不会自动进入新 worktree，实施前若状态变化须明确纳入范围，不能隐式依赖。

## 执行单元 1：修复权限读取、错误显示及重试

**修改文件：** `src/db/postgres.rs`、`src/ui/principal.rs`、`src/app.rs`，必要时 `src/model/principal.rs`。
**测试文件：** `tests/principal_tabs.rs`、`tests/postgres_principal_mutations.rs`。

1. 新增 reducer/TestBackend 回归：首次加载失败显示真实错误；用户刷新发出详情请求；刷新失败保留旧数据并标记过期；成功但零行与失败不同。
2. 将纯读取数据库用例与需要 CREATEROLE 的写入 fixture 分开。隔离 PostgreSQL 中建立角色、schema/table 授权，调用实际 adapter；记录旧代码失败位置和 SQLSTATE。
3. 删除不存在的 `information_schema.role_schema_grants` 查询，复用 schema 原生 ACL。实测 String OID 绑定，必要时统一显式文本转换或原生 OID 类型。
4. 将权限来源与成员关系读取分别聚合：局部失败返回已有结果及 Partial 原因；全部来源失败标 Unavailable；身份/连接错误继续硬失败。不要以空集合掩盖错误。
5. 概览使用详情状态和 coverage；刷新同时请求详情及 DDL，保留现有请求去重和响应身份校验。
6. 运行定向测试并记录后进入下一单元。

**验收：** LOGIN/NOLOGIN 角色都能打开；schema/table 授权可见；失败原因可见；刷新可以恢复权限查询；未提升账号权限。

## 执行单元 2：修正权限语义与完整浏览

**修改文件：** `src/db/postgres.rs`、`src/db/principal.rs`、`src/model/principal.rs`、`src/ui/principal.rs`、`src/app.rs`；输入与命中区域落在 `src/input/keymap.rs`、`src/input/mouse.rs`、`src/ui/mod.rs`、`src/action.rs`。
**测试文件：** `tests/principal_tabs.rs`、`tests/principal_contract.rs`、`tests/postgres_principal_mutations.rs`。

1. relation ACL 按 relkind 分类；sequence 用正确默认 ACL 类型；column 来自 attacl；routine 保留签名。删除重复来源或以对象标识、权限及来源去重。
2. default ACL 按授权接收者读取，保留创建者、schema 和对象类型，标明只影响未来对象；Direct/PUBLIC/Owner/Default 不混为一类。
3. 实际数据库与详情 database 对齐，请求不匹配不伪装成功；显示完整有效继承权限未计算的范围说明。必要的角色属性按只读元数据表达。
4. 替换固定 take(5)/take(2) 为可浏览视口；权限、member_of、members 均可访问；短终端仍能看到状态；选中项与鼠标索引同步。
5. 测试超过 5 条权限、超过 2 条成员、短终端、特殊标识符、重复刷新后选择夹紧，以及局部来源不可读。

**验收：** 已返回的每条权限和成员关系均可找到；数据无错误分类和重复；覆盖不足明确展示，不将缺少直接 ACL 等同没有有效权限。

## 执行单元 3：结构化授权与刷新闭环

**修改文件：** `src/db/principal.rs`、`src/db/postgres.rs`、`src/db/mysql.rs`、`src/db/mssql.rs`、`src/db/oracle.rs`、`src/db/mod.rs`、`src/model/principal.rs`、`src/app.rs`、`src/input/keymap.rs`、`src/ui/principal_mutation_form.rs`；按需要 `src/action.rs`、`src/help.rs`。
**测试文件：** `tests/principal_contract.rs`、`tests/principal_tabs.rs`、`tests/postgres_principal_mutations.rs`、`tests/keymap.rs`、`tests/mouse.rs`。表单状态若需联动 overlay，修改 `src/model/workspace.rs`；在途请求生命周期联动位置为 `src/runtime.rs`。

1. permission 增加独立于展示文本的可选结构化操作目标；adapter 填充，所有构造点同步；移除 split_once('.')。
2. 明确 Database/Schema/Relation/Column/Membership 的能力和 privilege 集合；纠正 PostgreSQL 列语法为 `GRANT SELECT (column) ON TABLE schema.table TO role`，REVOKE 同理，保持标识符 quoting。
3. 不把 PUBLIC/Owner/Default/Inherited 行当作目标角色的直接撤权；sequence/routine 等没有专用 mutation 支持时只读展示并给准确说明。
4. 接通表单真实输入、scope 字段和 operation/option 操作，允许无权限行的角色发起新授权。复用既有 SQL 预览、确认和执行流程。
5. mutation 成功按 plan principal/profile/connection 定位目标标签，刷新详情和 DDL；切换活动标签不误刷新；在途旧读取结束后仍须完成变更后的新读取。
6. 实测 schema/table/column grant → 查看 → revoke → 查看，以及 membership grant/revoke；覆盖带点、引号、非 ASCII 标识符及失败重试。

**验收：** 操作对象不依赖展示字符串；列 SQL 合法；空权限角色可以新授权；执行结果刷新正确角色；其他数据库没有因共享 UI 而误开放未支持的能力。

## 执行单元 4：CI 回归与交付

**修改文件：** `.github/workflows/ci.yml`、`tests/postgres_principal_mutations.rs`。本次操作说明统一落在 `src/help.rs` 与表单提示，不另扩展文档目录。

1. 在 databases job 的隔离 PostgreSQL 服务上显式运行 principal 集成测试；目前只执行 postgres_adapter，不足以覆盖本问题。
2. 对必需数据库测试启用 `LAZYDB_REQUIRE_DATABASE_TESTS=1`：缺连接或连接失败必须报错；离线可跳过但不能记为数据库验收通过。
3. fixture 有失败清理路径；只读测试不依赖 CREATEROLE。保证至少 CI PostgreSQL 16 实测证据。
4. 功能齐备后执行一次全量 Rust 检查；审查实际 diff、数据库与 UI 证据，完成有逻辑边界的提交及目标分支合并。

## 验证与记录

### 验证分级（优先于分析报告中含糊的“必需证据”措辞）

| 层级 | 来源与要求 | 环境受限时处理 |
|---|---|---|
| 用户功能要求 | 修复权限详情问题并实施；可用账号能读取，受支持授权能形成读写反馈闭环。用户没有要求人工签到或指定终端截图 | 功能缺失继续实施，不能标完成；不能以一次不可用 PTY 判定功能必然失败 |
| 项目既有门禁 | `.github/workflows/ci.yml` 的 Rust fmt/clippy/all-targets all-features test；保留既有 CI 服务与各数据库作业 | 如实记录实际失败与环境，按项目门禁处理；不将未执行写成通过 |
| 本任务设计的自动化回归 | principal reducer/TestBackend、输入映射、PostgreSQL SQLx round-trip；新增 CI PostgreSQL principal 测试步骤。它们是为本次缺陷设计的证据，不冒称用户先前指定 | 优先隔离库；本地无法运行可交 CI 取证，明确待验证；Luna 收尾审查决定是否已有充分证据，不要求用户反复补环境 |
| 补充建议验证 | 手工 PTY、截图、用户原连接人工对照、额外 PostgreSQL 版本及其他数据库现场试用 | 全部可选，不新增为合并硬门禁；最多一次针对性修复重试，仍不可用记录限制后继续 |

不增加新的人工确认流程。只读普通账号测试是自动化用例设计：fixture 可由隔离库管理员准备，实际读取由普通账号执行，不要求用户为验收提供管理员权限。

单元定向命令：

```sh
cargo +1.94.0 test --locked --test principal_tabs --test principal_contract
LAZYDB_REQUIRE_DATABASE_TESTS=1 cargo +1.94.0 test --locked --test postgres_principal_mutations -- --nocapture --test-threads=1
```

第二条只在显式配置的隔离 PostgreSQL 环境执行。当前 shell 已有指向外部数据库的测试连接变量，不能直接继承作为 fixture 写入目标；也不在记录中输出凭据。

功能齐备后的项目 Rust 检查：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

离线全量先移除外部数据库测试连接变量；真实数据库测试单独注入隔离连接。上面均为待执行命令，不是已通过结果。

每次实际检查追加到 `validation.md`：命令、退出码、代码版本/工作区状态、环境、是否发生真实数据库执行、相关文件。只在相关代码或环境变化后重跑；不每个单元重复全量检查。

TestBackend/reducer 与隔离 PostgreSQL 是主要验收证据；PTY 是补充检查。环境受限最多一次有针对性的修复重试，此后由 Luna 收尾审查决定补证或记录限制，不无限循环。不得把未运行/跳过测试表述为通过。

## 逐项实施与复核清单

每项采用“先添加有意义的行为回归 → 定向执行记录旧失败（环境允许时）→ 最小实现 → 同组定向通过 → Luna 检查 diff”的顺序。下列命令除最后全量外仅在对应文件有变化时执行；同一单元可合并一次运行，避免每个小编辑重复构建。

| 项 | 具体改动/复核点 | 定向验证与预期 |
|---|---|---|
| 1.1 | `tests/principal_tabs.rs` 构造 Failed/Loading/Ready 与 previous；断言错误文本、陈旧提示、空结果提示彼此不同 | `cargo +1.94.0 test --locked --test principal_tabs`；旧 UI 应不能满足错误显示断言，完成后通过 |
| 1.2 | `tests/postgres_principal_mutations.rs` 增加实际 SQLx 角色读取；`src/db/postgres.rs` 修 schema catalog/OID；复核不得吞错或回退为空列表 | 隔离环境运行上文 principal 数据库命令；实际返回 schema USAGE/table SELECT，不能 skipped |
| 1.3 | `src/app.rs` 刷新发出两个请求；`src/ui/principal.rs` 渲染 status/coverage；保留请求身份检查 | `cargo +1.94.0 test --locked --test principal_tabs`；失败后重试得到新 request，旧 response 无法覆盖 |
| 2.1 | `src/db/postgres.rs` 按真实对象分类 ACL，default ACL 保留创建者/种类；复核 PUBLIC 与 Direct 不错误合并，column/routine/sequence 不冒充表 | principal 数据库命令；fixture 各来源与 grantable 断言通过 |
| 2.2 | `src/model/principal.rs` 视口/分区状态；`src/ui/principal.rs` 完整成员与权限呈现；命中区域使用 inner rect | `cargo +1.94.0 test --locked --test principal_tabs --test mouse`；越过第 5 行仍可见、点击正确，短终端不越界 |
| 2.3 | 确认 actual database 与请求范围一致；刷新后夹紧 index；角色属性或覆盖说明表达继承限制 | principal_tabs + principal 数据库命令；错数据库不得返回贴错标签的成功结果 |
| 3.1 | `src/db/principal.rs` 增可选 mutation_target；同步所有 adapter 构造；移除 app 字符串解析 | `cargo +1.94.0 test --locked --test principal_contract --test principal_tabs`；含点标识符完整进入结构化目标，不支持来源不可撤权 |
| 3.2 | `src/db/postgres.rs` privilege/scope 验证及列 SQL 生成；复核 SQL 如 `GRANT SELECT ("c.x") ON TABLE "s.x"."t.x" TO "r.x";`，REVOKE 对应 `FROM` | `cargo +1.94.0 test --locked --test principal_contract` 加真实 principal 数据库命令；合法列操作成功、非法组合拒绝 |
| 3.3 | `src/model/principal.rs` 表单 TextInput、`src/action.rs` 输入动作、`src/input/keymap.rs` 编辑映射、`src/ui/principal_mutation_form.rs` 字段布局；scope 只显示有关字段 | `cargo +1.94.0 test --locked --test keymap --test principal_contract --test principal_tabs`；字符输入（含 q）、删除、Tab、取消/确认按编辑语境正确工作；空权限角色可新 grant |
| 3.4 | `src/app.rs` 成功按 plan 查标签；必要时 `src/runtime.rs`/model 处理在途详情完成后的重载；复核 profile/connection generation 均匹配 | principal_tabs；A 角色授权期间切到 B，完成后只刷新 A；旧请求不能覆盖授权后的数据 |
| 3.5 | 数据库 grant/read/revoke/read 及 membership 双向 round-trip；复核测试清理覆盖失败路径，不读取真实业务对象进行写测试 | principal 数据库命令；数据库证据与 reducer/输入证据一起证明交互闭环，不声称仅 adapter 测试覆盖整套 TUI |
| 4.1 | `.github/workflows/ci.yml` 增实际 principal 测试；环境必需模式下连接失败报错；普通账号读用例不依赖其 CREATEROLE | 隔离库正向执行；受控无连接环境验证 required 模式会失败。该失败为预期负向结果，单独记录 |
| 4.2 | 全量检查、审查变更范围与能力声明；只提交任务 worktree 内实际任务文件 | 上文 fmt/clippy/test；结果逐条记录。PTY 不参与硬门禁判定 |

### 关键实现契约

- `PrincipalPermission` 的展示 target 保留兼容；新增 `Option<PrincipalMutationTarget>` 只代表对象可定位。撤权还必须判断来源为 Direct，不能仅因 `Some` 就放行；其他 adapter 没有可靠映射时填 None。
- details 读取成功但 coverage 为 Unavailable/Partial，不可在 UI 宣称“无权限”；权限和成员分别标注。各区段成功但本就未计算继承时仍为 Partial，而不是 Complete。
- membership 不需要虚构表权限行，单独从成员列表或新建表单进入 GrantRole/RevokeRole。权限行 grantable 表示该授权附带 grant option，不是当前操作者的授权许可。
- mutation 后需要重载但已有请求在途时，可以设置一次性 reload-needed 标志；在旧请求完成后分配新 request ID。若连接 generation 已变更则丢弃旧操作的刷新，不能重载到新连接上。
- 用同一读取连接确认实际数据库并执行 catalog 查询；不因 details.target.database 的字符串而假装连接已切库。跨库路由若确需扩大，本任务先显式提示范围不匹配。

### 修改范围与提交边界

预计修改文件完整列在 `change-scope.json`，包含 input、runtime、overlay 及测试联动位置。没有计划删除、重命名或业务新增文件；新增任务产物位于 .git，不属于待合并业务范围。dbx 与 analysis 参考路径不列入业务范围。

建议提交按单元分为：读取与恢复、完整浏览与来源、授权闭环、CI 回归。由 Luna 在任务 worktree 中复核后精确暂存实际修改，不使用原工作空间的 `git add -A`；不要求每个小步骤单独提交。实施发现新增构造点或文件需求时，先更新计划/范围，再继续普通实现决策，无需用户确认。

## 交接

实施阶段第一个动作：Luna 确认起点、当前工作区与届时 checkpoint，创建自动命名任务分支/worktree，直接开始单元 1 的失败状态和权限读取回归。完成一个闭环后持续推进下一单元，不要求用户反复 resume。
