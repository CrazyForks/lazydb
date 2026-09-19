# Users & Roles Implementation Plan

> **执行者：Luna。** 按下面的端到端单元连续实施、定向验证，再统一审查和收尾；Astra 仅负责分析与计划。用户禁止启动子 Agent。本计划覆盖技能默认的 worktree、文档目录和交接方式，不要求用户逐项 resume。

**Goal:** 让 Users & Roles 的 a/d/e 分别完成创建、确认删除、编辑密码/登录/角色权限，并同步列表与 DDL。

**Architecture:** 复用 ExplorerAdd、CatalogEditorState、RoleDraft 和现有角色变更执行链。principal 始终保留独立稳定身份；编辑通过携带 PrincipalEntry 的 typed anchor 接入角色管线，删除采用独立 typed principal drop 管线并复用现有 dialog 基元。

**Tech Stack:** Rust 1.94.0、ratatui、crossterm、tokio、sqlx/PostgreSQL；项目已有 reducer、Command/Action 和适配器测试模式。

---

## 0. 基线、交付边界与工作方式

- 原目录 `/Users/yelog/workspace/tui/lazydb`，指定起点 `5a41d82a7d2ae8363cb197b78f3ecfddcf2fb274`，目标 main。本轮正式 plan 检查发现 HEAD 已变为 `2a91556a642895a7b10d1231c71bf963971f53ec`，工作区及 index 均干净；与指定起点相比仅多出原两份持久化修复的提交 `fix(workspace): disambiguate principal tab kind`。不擅自更换任务起点，依赖处理见 5.1。
- 不改原工作区 index、不 stash、不替用户提交已有修改。实施工作树/分支由后续工作流创建和自动命名，本计划不自行创建。
- 所有工作流文档保存在本目录。checkpoint/state 由插件维护；本计划不修改它们。
- 菜单在 PrincipalGroup、Principal 条目显示且只显示 User、Role；连接原有五项菜单保留。PrincipalNotice 可复用分组创建语义，不能编辑/删除。
- **能力范围：本计划完成 PostgreSQL 端到端操作，其他驱动明确显示当前不支持修改。** 这是按现有只有 PG 支持角色创建的能力所作的实施范围，不表示用户明确限定 PG，也不表示已实现跨驱动管理。最终交付必须披露。不要为 MySQL/Oracle/SQL Server 套用 PG SQL。
- 权限覆盖现有角色属性和 Member of，不宣称提供表/列对象 ACL 编辑。
- 本次无新增业务代码；下列类型/函数为拟新增接口约定，不是已经存在的 API。实施时沿用当前命名及错误类型，编译器穷尽匹配决定必要的调用点更新。

## 1. 公共实现约束

### 1.1 身份与连接

添加 `CatalogMutationAnchor::Principal(PrincipalEntry)`，仅 role object types 可搭配此 anchor。其 profile 必须与 connection.profile_id 相同；Edit 需要 role baseline，Create 继续使用 Profile anchor。

新增 principal definition request，携带 connection、request_id、catalog_epoch、PrincipalEntry、ExecutionTarget。由专用 Command/Action 载入，返回同一个请求和 RoleDefinition，避免让公开定义请求继续假装 Database CatalogId。在 CatalogEditorState 里记录完整 pending principal request，用于 response equality；不要仅比 request_id。

PG definition 读取以 OID 为准，核对 scope/profile，使用 pg_roles；名称来自数据库结果。编辑规划以 baseline 的名字作为原名，以 draft 名字为新名。运行时在执行前按该 PrincipalId 重载并校验 fingerprint；旧 Catalog anchor 路径保留给现有调用者。

fingerprint 包括 OID、名字、全部可编辑非密码属性、规范化排序去重后的 Member of、备注和有效期。不得只 hash 名字。不要读取密码/hash。外部并发变化应返回 stale 并要求重新加载草稿；检查与 DDL 之间仍可能存在服务端并发窗口，不把应用层复核宣称为原子并发保证。

### 1.2 保存后收敛

role mutation 成功必须走 principal 刷新分支，而非 Databases。刷新结果按 PrincipalId/OID 更新已开 DDL tab 的 entry/title/kind，并重新加载其内容；创建以服务器列表中返回的新身份定位，编辑保留原 OID。删除清理该 OID 的 tab，先取消请求，修正 active_tab，选择回退分组。

principal list 的 pending request、连接 generation 和来源状态必须匹配。过期 definition/plan/success/failure 不得操作后开的弹窗。切换焦点不是换连接；连接身份以所选 profile 的 session 为准，不能无条件借当前全局连接。

### 1.3 秘密与权限

复用 SecretTextInput/RedactedSecret；编辑空密码表示保留。SQL 预览只出现 `<REDACTED>`，明文只通过已有 execution secret 通路进入执行，不写 workspace/DDL/通知。只读检查同时存在于 UI/reducer 和 runtime；数据库拒绝权限时展示真实错误，不额外猜测管理员身份。

## 2. 单元 A：a -> 创建 -> 列表与 DDL 可见

**修改文件**

- `src/model/explorer_add.rs`：菜单上下文/选项过滤。
- `src/input/keymap.rs::map_explorer`、`src/help.rs`：principal 上 a 的一致可用性和提示。
- `src/app.rs::open_explorer_add`、`explorer_add_options`、`confirm_explorer_add`、CatalogMutationSucceeded/PrincipalPageLoaded 分支。
- `src/db/postgres.rs::plan_role_mutation`：login 和成员 SQL 修复。
- `src/model/principal.rs` 或 App 现有 pending selection 状态：新增 principal 刷新定位状态。
- `tests/explorer_add.rs`、`tests/catalog_mutation.rs`；新增 `tests/principal_mutations.rs` 作为 reducer 集成测试入口。

**步骤**

1. 添加行为测试：选择 group/User/Role 时按 a -> ExplorerAdd；options 严格等于 `[User, Role]`；连接节点仍等于原五项。加入只读/离线/其他 profile/不支持驱动的无执行测试。
2. 定向运行：`cargo +1.94.0 test --test explorer_add --test principal_mutations`。新增断言在实现前应因无菜单入口失败；若测试编译失败先排除测试夹具错误，不能把夹具错误当成功复现。
3. 修改 open_explorer_add 解析 Profile 或 principal 上下文。以同一能力函数生成菜单禁用原因，确认时再次校验，沿用 User -> LoginRole / Role -> Role。
4. 保留 RoleDraft::new 的默认 login，删除 planner 内无条件根据 object_type 重写 login 的语句。以草稿值生成 LOGIN/NOLOGIN。
5. 修正 Member of 的 GRANT/REVOKE 方向；按排序去重的集合输出稳定 SQL。必须更新旧测试中的错误预期：

```rust
assert!(plan.sql().contains("GRANT \"reporting\" TO \"alice\""));
assert!(plan.sql().contains("GRANT \"new_group\" TO \"alice\""));
assert!(plan.sql().contains("REVOKE \"old_group\" FROM \"alice\""));
```

6. Role mutation 成功时清 owner context、强制 request_principals；加载返回后定位新对象，确保 group 展开。不得同时用旧 Databases selection 覆盖定位。创建和现有连接节点创建共用该修复。
7. 运行 `cargo +1.94.0 test --test explorer_add --test principal_mutations --test catalog_mutation`，记录命令、退出结果及当前版本。新增测试须覆盖“菜单确认产生角色表单”和“成功产生 principal refresh command”，不能只有菜单模型测试。

**完成条件**：从树上 a 可一路创建成功并看到真实条目，角色默认值/成员授权正确，原连接菜单不回归。若可用实库环境尚未配置，先完成 reducer/SQL 验证，实库证据在最后统一补齐并明确限制。

## 3. 单元 B：e -> 加载 -> 修改 -> 保存/重读

**修改文件**

- `src/db/catalog_mutation.rs`：Principal anchor、请求验证及 profile 匹配。
- `src/db/principal.rs`：定义加载请求类型；`src/db/mod.rs`：principal definition 适配器分发及能力声明。
- `src/db/postgres.rs`：OID definition、fingerprint、role planner 主体匹配。
- `src/action.rs`：OpenPrincipalEdit、definition Command/Action；`src/runtime.rs`：加载、执行前 definition 重载。
- `src/model/catalog_editor.rs`：pending principal definition 与 RoleDraft 接入；`src/app.rs`：编辑请求/回调和成功刷新。
- `src/input/keymap.rs`、`src/help.rs`、`src/ui/catalog_editor.rs`：入口、标题、加载失败和登录状态提示。
- `tests/principal_mutations.rs`、`tests/catalog_mutation.rs`、`tests/catalog_editor_state.rs`、`tests/principal_tabs.rs`。
- 全仓穷尽匹配受影响文件：`src/model/explorer_actions.rs` 接入共享 principal 可用性；`src/db/mysql.rs` 的 catalog create anchor 穷尽匹配须显式拒绝 Principal。Oracle/MSSQL/SQLite 当前使用 let-else 拒绝非支持 anchor，作为回归参考，不预先列为修改文件；实际需要修改时更新 scope。

**步骤**

1. 添加 reducer 测试：e 发出带真实 OID/所选 session 的定义请求；加载期间无可保存草稿；载入成功建立 RoleDraft；失败保留错误；错误 connection/request/entry 的结果不改变编辑器。
2. 实现 Principal anchor 与完整 definition request；修改请求 validate 和 role planner 的 baseline/mode 校验。不要为了通过验证注入伪造 CatalogEntry。
3. 新增 PG OID loader，共享现有 RoleDefinition 组装逻辑，将旧 name loader 的公开属性源改为 pg_roles。查询 Member of 的方向保持现状，fingerprint 覆盖可编辑快照和 OID。
4. e 仅在具体 PrincipalEntry 上触发，group 不触发。打开现有 CatalogEditor overlay，填充加载状态；完成后使用 RoleDraft::from_definition。保留空密码、正确 name/login、属性与 Member of。
5. 扩展 runtime baseline reload：Catalog anchor 走原 loader；Principal anchor 走 OID loader；两者均比较 fingerprint。新增分支不能跳过已有只读检查、目标连接解析、owned connection 关闭。
6. 测试 LOGIN -> NOLOGIN 和 NOLOGIN -> LOGIN 两个方向，保持旧 object_type 时也必须生效；测试单独密码变更、空密码不产生 PASSWORD、角色级属性、Member of 增删、引用名、无修改 NoChanges。有效期去除限制使用 infinity，禁止界面清空后静默忽略：清空可规范化 infinity，并在 UI 说明。
7. 改造成功刷新：同一 OID 的新 entry 覆盖 tab 和 explorer 的旧名称/kind，重载相关 DDL。创建刷新定位与编辑定位不要互相覆盖；外部删除后回退 group。
8. 运行 `cargo +1.94.0 test --test principal_mutations --test catalog_mutation --test catalog_editor_state --test principal_tabs`。

**完成条件**：用户能实际修改所列字段并重读一致；rename/login 切换保留 OID；失败不丢草稿；迟到结果不污染其他编辑器；未改变密码材料的持久化边界。

## 4. 单元 C：d -> Drop User/Role -> 删除与 tab 收敛

**新增文件**

- `src/db/principal_drop.rs`：typed request/plan/error 与验证。
- `tests/principal_drop.rs`：纯规划与 reducer 行为。

**修改文件**

- `src/db/mod.rs`、`src/db/postgres.rs`：规划/执行分发。
- `src/model/workspace.rs::Overlay`：PrincipalDropConfirm，含 plan、cancel/drop 焦点、busy/error。
- `src/action.rs`、`src/runtime.rs`、`src/app.rs`：plan/execute 回调、请求关联、成功收敛。
- `src/ui/mod.rs`：Drop Table 同风格渲染；`src/input/keymap.rs`、`src/input/mouse.rs`、`src/help.rs`：键鼠操作与帮助。
- `src/ui/animation.rs`：新增 overlay 分支；其余 overlay 穷尽 match 由编译定位。
- `tests/principal_tabs.rs`、`tests/ui_render.rs`：删除后的 tab、标题、焦点、错误和 busy。

**数据契约**

PrincipalDropRequest 保存 connection、request_id、完整 PrincipalEntry。PrincipalDropPlan 保存 request 和私有 sql；只由 adapter planner 构造，validate 校验 profile/scope/身份/非空名称。弹窗标题从 request.entry.kind 取，不从 CatalogKind 推断。PG SQL 用现有 quote_identifier：`DROP ROLE <quoted-name>`。

**步骤**

1. 添加 User/Role 标题、默认 Cancel、Esc、Tab/左右、Enter、鼠标两按钮、busy 拒绝重复命令的 reducer/keymap/render 测试。
2. 实现 typed request/plan、PG planner、数据库分发。其他驱动返回能力不支持。d 命令直接调用也必须校验只读、目标 session、所选 principal 是否仍存在。
3. PlanReady 仅在原 pending request/connection 仍有效时打开弹窗；取消清 pending，避免异步规划晚到重开。
4. 从 `render_catalog_drop_confirm` 复用 dialog frame/actions/body/sql_preview；保留其信息和行为，默认 Cancel，不引入额外输入确认或 CASCADE。宽度/高度随现有布局约定，小终端不得 panic。
5. runtime 以 request 的 session 执行，重新检查 profile read_only 与主体 OID/名字，拒绝旧实体被删除重建、名字改变、连接 generation 失效。使用正常数据库 DDL 执行及错误转换，不借任意 SQL console 绕过 mutation 路径。
6. 失败解除 busy，保留弹窗/SQL/error；成功关闭匹配弹窗、取消对应 DDL 请求并移除 tab，更新 active_tab 后刷新 principal list 与 owner context。旧成功不得关闭新弹窗。
7. 运行 `cargo +1.94.0 test --test principal_drop --test principal_mutations --test principal_tabs --test catalog_drop`，再运行实际新增 UI/keymap 测试的名称过滤命令。把真实测试名和结果追加 validation。

**完成条件**：两种实体可正确删除，取消无副作用，失败可恢复；列表与打开 tab 收敛；Drop Table 原路径仍通过。

## 5. 单元 D：持久化依赖、真实数据库闭环及最终验证

### 5.1 显式带入原工作区本地修复

**文件**：`src/persistence/workspace.rs`、`tests/workspace_persistence.rs`。

分析阶段该修复为未提交内容；本轮已确认它完整落在后续提交 `2a91556a642895a7b10d1231c71bf963971f53ec`，但不在指定起点。若任务 worktree 从指定起点创建，需在任务分支带入该提交的等价精确修复：PersistedPrincipalTab.kind 加 `#[serde(rename = "principal_kind")]`，并添加 `principal_ddl_tab_serialization_keeps_discriminator_and_principal_kind_distinct` 回归测试，验证 `kind = "principal_ddl"` 只出现一次，`principal_kind = "User"` 存在，TOML 可反序列化为原 tab。若后续工作流选择的基线已经包含该提交，确认后跳过重复应用。两个文件仍列入 change-scope，供相对指定起点的范围与依赖核对。

运行：`cargo +1.94.0 test --test workspace_persistence`。后续合并时识别 main 上的同等修复，不重复覆盖。原工作区无需清空、stash 或提交。

### 5.2 实库验收

新增 `tests/postgres_principal_mutations.rs`，沿用项目 `LAZYDB_TEST_POSTGRES_URL` 和测试清理模式。仅操作 UUID 后缀的测试角色，失败也尝试清理。本测试套件在 URL 不存在时可按项目模式跳过，但必须在 validation 明确“未运行实库”。CI/有 URL 时必须真正执行，数据库连接失败不可当成功跳过。

覆盖一个完整流程：

1. 创建测试 role 和 login user，Member of 指向前者；列表返回两个稳定 OID。
2. 定义加载结果与初值一致；空密码保存其他字段，不改变密码。
3. 修改密码，使用独立新连接验证登录；切换 NOLOGIN，新连接被拒绝；切回 LOGIN，登录恢复。不要用已有连接仍可查询作为禁止登录失败证据。
4. 改一个普通角色属性、添加/移除 Member of，重读确认正确方向；rename 后 OID 相同；打开 DDL 不包含密码。
5. 外部修改属性后旧 baseline 被拒；旧 OID 删除重建后旧编辑/删除请求被拒。
6. 以可读取 pg_roles 的受限用户验证定义加载无需 SELECT pg_authid；无足够 ALTER 权限时报服务端错误。
7. 删除测试主体，列表不再包含它；若建立依赖验证 drop 错误，测试清理依赖后再删除。

命令：`cargo +1.94.0 test --test postgres_principal_mutations -- --nocapture --test-threads=1`。环境须通过正常方式提供 URL，日志不打印密码。

修改 `.github/workflows/ci.yml` 的现有 PostgreSQL adapter 测试命令为 `timeout 10m cargo test --locked --test postgres_adapter --test postgres_principal_mutations -- --nocapture --test-threads=1`，复用已有 PostgreSQL service 与环境变量，确保新增实库测试实际进入数据库 job；不新增重复服务。

### 5.3 功能齐备后的统一检查

按 `.github/workflows/ci.yml` 运行一次：

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
git diff --check
```

预期各项 exit 0。默认含 Oracle feature；环境缺依赖时最多一次有针对性修复重试，再记录具体缺口，不能用 no-default-features 结果冒充 all-features。出现代码问题自行修复并重跑受影响检查，不每轮重复全量。

PTY/人工试用属于补充证据：如环境可用，用 PG 测试库走 a/e/d 并检查终端窄窗口、鼠标；环境受限不无限重试。用户强制要求的是本功能与本阶段约束，项目强制验证以 CI 检查为准，不自行加人工审批流程。

### 5.4 验证性质及各单元复核

| 性质 | 内容 | 完成判据 |
|---|---|---|
| 用户需求 | a 选择创建、d 同风格删除确认、e 修改密码/登录/权限 | 对照 A/B/C 完成条件，不能以只有入口或表单代替真实操作 |
| 项目现有门禁 | Rust fmt、all-targets/all-features clippy 和 test，现有 CI 数据库 job | 记录实际退出码；受环境限制不能标记通过，交由 Luna 收尾审查处理 |
| 本计划新增自动化回归 | 定向 reducer/SQL/UI 测试、PG principal 实库测试并接入已有 CI | 必须实现和执行适用测试；本地缺 URL 单独标记未执行，CI 有服务时应实际运行 |
| 补充建议 | PTY、人工试用、额外终端尺寸探索 | 非新增强制门禁；一次针对性修复重试后记录限制，不阻塞为无限 progress |

每单元完成后 Luna 复核实际 diff：A 检查菜单范围及创建后刷新；B 检查所有新增 anchor match、OID 与 fingerprint、Member of 方向及敏感字段；C 检查取消/迟到回调/重复执行和 tab 清理；D 检查持久化依赖是否重复、CI 新测试是否真的带数据库环境。复核只针对当前变更和未解决问题，不要求重复运行已通过且未受影响的测试。

`change-scope.json` 列出全部当前预计业务/测试/CI 修改和新增文件，包含上述两个起点之后的依赖文件；没有删除或重命名计划。只用作回归参考的 catalog_drop、Oracle/MSSQL/SQLite 源文件不列入。新增枚举导致 scope 外文件确需调整时，先更新最小具体范围，再实施相关修改，不能擅自借该任务大面积重构。

## 6. 验收清单与收尾

- [ ] group/User/Role 的 a 只有 User/Role，连接原菜单正常。
- [ ] 创建保存后条目可见且 DDL 可打开。
- [ ] e 的密码、登录、角色属性和 Member of 实际可保存重读。
- [ ] PostgreSQL 类型改变或重命名后选择/tab 身份稳定。
- [ ] d 标题与 Drop Table 交互一致，默认取消、单次执行、错误可见。
- [ ] 删除清理对应 tab 和请求，无迟到响应污染。
- [ ] 只读/离线/不支持驱动不执行变更，显示一致原因。
- [ ] 密码不进入日志、SQL 预览、DDL 或持久化。
- [ ] 原本地 principal_kind 修复显式纳入，workspace 回归通过。
- [ ] 定向、全量、实库结果按实际环境记录，未运行的验证明确列出。

Luna 根据实际 diff 自行审查、纠偏及后续提交合并；按端到端单元组织提交，暂存仅任务相关文件，具体分支名与提交时机服从工作流。建议单元标题分别为创建闭环、编辑闭环、删除闭环、持久化与回归，不把所有缺陷混进无说明的大提交。

本 plan 阶段至此完成。下一步是由 Luna 在任务工作树实施单元 A；不需要用户选择额外执行模式或反复 resume。本轮按用户指定顺序保存 plan.md、change-scope.json 与验证记录，最后写入 `plan-9f79c0df-f38c-4379-b473-782d84244c6c.json` 完成回执，保留历史回执。
