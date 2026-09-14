# 工作区退出保存一致性 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
>
> 执行适配：若执行环境没有该技能，按本文顺序执行即可。本文是实施计划，拟定的类型、接口及测试名不是已实现 API。不要求子代理；多个任务修改同一 App/Runtime，应串行执行。仅在用户要求提交时创建 Git commit。

**Goal:** 消除多连接与 Redis 浏览器场景中的重复 Tab ID 保存失败，保证所有文档最新状态可靠保存，并提供明确、可恢复的退出失败交互。

**Architecture:** 全局 Tab、Console 记录和 EditorWorkspace 是唯一运行时文档来源，连接状态及导航状态不持有文档副本。保存从全局实时状态构建可校验快照，旧格式只在恢复入口迁移。沿用 Action → App::update → Command → Runtime，将保存错误分类、修订号确认与恢复导出接入现有生命周期。

**Tech Stack:** Rust 2024 / Rust 1.94、Tokio、Serde/TOML、Ratatui/Crossterm、现有 WorkspaceStore、tempfile 及 Rust 单元/集成测试；无需新增生产依赖。

---

## 1. 基线、范围与关联计划

分析基线：`v0.1.4-115-g2dc464d`，Cargo 包版本 `0.1.4`。行号仅作导航，实施时按符号定位。

已确认的因果链：

| 代码位置 | 当前行为 | 修复责任 |
| --- | --- | --- |
| `src/app.rs:874 snapshot_active_workspace` | 克隆全部 Tab/Console，搬移 editor | 去除连接切换时的文档缓存 |
| `src/app.rs:1050 activate_profile_workspace` | 将全局副本挂到旧 profile，并修正缓存 Console 目标 | 分离导航、连接、文档归属 |
| `src/app.rs:1632 workspace_snapshot` | 活动 profile 取实时筛选结果，其他 profile 取旧缓存 | 所有文档统一从实时状态保存 |
| `src/app.rs:17770 open_redis_browser` | 先创建 B 的 Tab，再连接 B | 该顺序应保持合法，不让旧连接缓存污染归属 |
| `src/app.rs:1819 persisted_workspace_from_parts` | Redis profile_id 取外部分组参数 | 使用真实 target，拒绝错归属 |
| `src/persistence/workspace.rs:327 validate_snapshot` | 非 Console ID 跨 profile 检查，诊断缺少冲突来源 | 完整、顺序无关的引用与唯一性校验 |
| `src/runtime.rs:243 WorkspaceSaveQueue::retry` | 可重放原无效快照 | 按错误类型决定重试策略 |
| `src/app.rs:4291 WorkspaceSaveFailed` | 同一退出失败既 Toast 又弹窗 | 单一主提示、结构化错误 |

截图无法确认实际冲突 UUID；W01 必须用真实 reducer 操作顺序把代码推导转化为可重复测试。

与 `docs/plans/2026-09-14-multi-connection-workspace-consistency-implementation.md` 的关系：

- 本文 W02/W03 对应原计划 T02/T03，是同一份实现，不建立第二套文档注册表或迁移入口。
- 若原任务已完成，先运行本文回归测试，只补缺口。
- 全局 Console CRUD/列表去缓存是本次所有权收敛的必要部分；连接路由、目标选择器、新 Console 焦点策略等原计划后续需求继续由原计划管理。
- 本文新增 Redis 特定复现、校验完整性、保存队列状态、退出提示与恢复导出验收。
- 不更改真实事务的提交/回滚语义；现有事务退出检查仍先于工作区退出保存。

## 2. 必须成立的契约

1. 一个 Console UUID 对应一个全局文档记录；同名不同 UUID 均保留。
2. 一个已打开 Tab UUID 在全局 Tab 顺序中只出现一次。Console 记录与其 Console Tab 共享 UUID 是合法引用，不是重复实体。
3. 连接切换不搬移 editor、不隐式重绑其他文档、不恢复旧关闭状态。
4. 保存覆盖打开及关闭 Console 的最新 SQL、名称、目标、事务模式；History 按现有规则不持久化。
5. 保存保留全局 Tab 顺序、活动 Tab、Redis profile/database/pattern、Dashboard 归属及设置。
6. 文本读取失败不能用空串替代。显式空文档仍是合法文本。
7. 只有目标退出 revision 被成功确认后才能正常 Quit；恢复导出成功不等于工作区保存成功。
8. 旧 revision 的回调不能改变新 revision 的退出结果，取消退出后的迟到回调不能触发 Quit。
9. 不以任意去重、重建 UUID、放宽 validator 的方式修复运行时错误。
10. 无效快照不得覆盖现有工作区；迁移冲突不得覆盖源文件。

## 3. 数据与接口决策

### 3.1 运行态

- 保留 `App.tabs: Vec<WorkspaceTab>` 的顺序语义和现有 Console 集合；本次无需强制改成 HashMap。
- `ConnectionWorkspace` 如仍保留，仅作为旧格式导入 DTO，恢复完成即消费。
- profile 导航缓存只保存 `active_tab_id` 和焦点等小型状态，不能含 Tab/SQL/editor。
- Console 元数据集中更新，并同步现有 Tab 派生字段；不要额外引入第三份权威记录。
- 增加可失败的快照构建入口，建议 `try_workspace_snapshot() -> Result<WorkspaceSnapshot, WorkspaceSaveFailure>`；生产保存路径必须使用它。原 `workspace_snapshot()` 的调用者统一迁移，避免生产路径保留默认空文本包装。

### 3.2 格式决策门

W03 首先验证当前 v5 是否可无歧义表达全局文档：顶层已有 consoles/tabs，但 Dashboard 缺少 profile_id，active_profile 现有校验又依赖 profiles。

首选沿用原计划的格式决策：若补充可选归属字段和明确的顶层语义仍保持兼容，则继续 v5；若旧 reader 会丢归属、错误恢复或无法区分语义，升级 v6。先写兼容性用例，再确定版本，不先提交格式变更。

无论版本如何，当前格式只能有一份权威文档区。profile 段若存在，仅含导航信息；旧格式 profile 文档段只用于读取转换。不能为满足旧 active_profile 校验继续复制文档。

旧 profile 段恢复 Dashboard 时继承外层 profile；Redis 保留自身 target 并核对外层归属。旧格式没有全局顺序信息时采用确定性顺序，记录兼容规则；不能声称恢复了旧文件未保存的顺序。

### 3.3 保存失败模型

建议在 `src/model/workspace_save.rs` 定义可 Clone/Eq 的失败 DTO，包含：

- 类别：Io、InvalidSnapshot、Serialization、WorkerFailure。
- 简短用户消息、可展示的详细诊断。
- 对重复错误：UUID、Tab 类型、首次位置、重复位置；位置含 profile/全局及索引。
- 重试能力由类别推导；I/O 允许用户重试但不保证成功，不自动无限重试。

`WorkspaceError` 保留底层错误与 source；在 Runtime 边界转换 DTO。App 侧快照构建错误也转换为同一 DTO。Action、QuitSaveState、Overlay 共享该对象，不通过匹配英文错误文本判断类别。

## W01：建立基线并复现截图路径

**Files**
- Modify: `tests/global_workspace.rs`
- Modify: `tests/redis_browser_tabs.rs`
- Modify: 本文执行记录

**步骤**
1. 记录 `git status --short`、`git rev-parse HEAD`、`rustc --version`；保留已有未提交计划及其他用户改动。
2. 运行基线：`cargo test --locked --test global_workspace --test workspace_persistence --test workspace_tabs --test redis_browser_tabs`。
3. 在 `tests/global_workspace.rs` 复用 memory_profile/connect helper，先补以下完整用例，确认编辑器被搬移的问题：

```rust
#[test]
fn switching_profiles_keeps_the_first_live_editor() {
    let first = memory_profile("first");
    let second = memory_profile("second");
    let first_id = first.id;
    let second_id = second.id;
    let mut app = App::new(vec![first, second]);
    connect(&mut app, first_id, "first");
    app.update(Action::ReplaceEditor("SELECT 'latest first'".into()));
    let console_id = app.active_console().id;
    connect(&mut app, second_id, "second");
    assert_eq!(app.editor_text(console_id).unwrap(), "SELECT 'latest first'");
}
```

4. 运行：`cargo test --locked --test global_workspace switching_profiles_keeps_the_first_live_editor -- --exact`。预期旧实现失败于 editor 缺失，而非编译或连接前置条件。
5. 在 Redis 测试构造两个不同 UUID 的有效 Redis profile。通过 RequestConnect/ConnectionSucceeded 建立 A，再发送 `Action::OpenRedisDatabase { profile_id: b, database: 0 }`，消费返回的 Connect generation 并模拟 B 成功。
6. 新测试命名 `opening_redis_on_second_profile_produces_a_valid_snapshot`：检查实时 R 属于 B；构建快照后统计各文档区 R 的次数必须为 1；所有 Redis 持久化归属必须为 B；临时目录 store.save 必须成功。
7. 运行：`cargo test --locked --test redis_browser_tabs opening_redis_on_second_profile_produces_a_valid_snapshot -- --exact`。记录实际报错及重复来源，允许诊断先遇到其他不变量，但不能手工注入重复来替代操作链复现。
8. 再逐个增加 A→B→A→B、同 profile DB0/DB1、重复打开同一 DB 的测试，检查 UUID 与打开数量。

**验收**：记录旧代码真实失败路径；原有通过用例与新增失败用例清晰可区分。此阶段不宣称修复完成。

## W02：统一全局文档所有权

**Files**
- Modify: `src/app.rs` — snapshot/append/install/activate、Console CRUD、visible_console_records、profile 删除
- Modify: `src/model/workspace.rs` — ConnectionWorkspace 职责
- Modify as needed: `src/editor/mod.rs` — 一次性恢复接口
- Test: `tests/global_workspace.rs`, `tests/workspace_tabs.rs`, `tests/editor_projection.rs`, `tests/profile_lifecycle.rs`

**步骤**
1. 新增反复切换后文本、目标、undo/redo 和文档数量不变的行为测试，逐个运行确认基线。
2. 将恢复导入与连接激活拆开；连接只更新会话和必要导航，用户显式入口负责创建新文档。
3. 移除 `snapshot_active_workspace` 日常路径中的全量 clone 和 `take(self.editor)`；清理相应 workspace_editors 消费逻辑。
4. 让 restore 一次性建立所有 SQL/output/DDL 会话，切换时不再 append 旧快照。
5. 收敛 Console 创建、重命名、重绑、关闭、删除入口；列表只读全局记录，删除后无缓存可复活文档。
6. 移除 `activate_profile_workspace` 对夹带 Console 的目标重写；测试 B 连接完成不会修改 A 的 execution_target。
7. 按调用关系逐个清理 workspaces 的运行期读写；profile 最近焦点/Tab 若需要，迁移到导航缓存。
8. 重跑：`cargo test --locked --test global_workspace --test workspace_tabs --test editor_projection --test profile_lifecycle`。

**验收**：编辑器会话持续存在，连接切换不改变文档所有权；W01 editor 用例通过。W02 与 W03 为同一根因修复里程碑，不单独发布仅内存正确的版本。

## W03：从实时状态生成快照并完成旧格式恢复

**Files**
- Modify: `src/app.rs` — workspace_snapshot、persisted_console、restore_workspace、persist_workspace_command
- Modify: `src/persistence/workspace.rs` — schema、load、迁移、save
- Modify: `src/model/workspace_save.rs` — 构建失败 DTO
- Test: `tests/global_workspace.rs`, `tests/workspace_persistence.rs`, `tests/workspace_tabs.rs`, `tests/profile_lifecycle.rs`

**步骤**
1. 增加非活动 A Console 修改后直接保存的测试，不通过切换 A 来同步旧缓存。
2. 增加关闭 Console 文本/名称、显式重绑、未绑定 Console、已删除 profile 引用、Redis pattern、Dashboard 归属、混合 Tab 顺序和活动项的 round-trip 测试。
3. 按 3.2 完成格式决策，记录 reader/writer 兼容结果；为选择的格式补旧版 fixture，旧版本已支持的读取路径必须继续覆盖。
4. 实现可失败构建入口：遍历全局记录读取真实 editor 文本；缺失会话返回带 Console ID 的失败；关闭文档若有独立合法存储，必须从其权威存储读取而非补空串。
5. 遍历全局 tabs 一次生成顺序；Redis 从自身 target 取归属；History 被排除时活动引用按明确规则回退为首个可持久化 Tab 或 None。
6. 只从当前状态生成 SQL 内容和 Console 元数据，不读取旧 workspace.sql；全局 SQL ID 与 Console 一一对应。
7. 旧格式在加载边界迁移，当前格式直接严格校验。完全相同的旧镜像可按明确优先级合并；同级冲突且没有 revision 证据时返回冲突，保持源文件和 SQL 文件不变。
8. 恢复不发 Connect，不恢复可执行事务会话；失效目标按既有兼容契约呈现，不自动重绑到其他 profile。
9. 临时目录执行 save→load→restore→save，比较语义快照和 SQL 文本；校验第二次结果幂等。
10. 运行：`cargo test --locked --test global_workspace --test workspace_persistence --test workspace_tabs --test profile_lifecycle --test redis_browser_tabs`。

**验收**：W01 Redis 保存用例通过；最新文本、全局顺序及归属正确；格式版本选择有测试证据；缺失 editor 不再静默保存为空。

## W04：顺序无关的严格校验与精确诊断

**Files**
- Modify: `src/persistence/workspace.rs` — WorkspaceError、validate_snapshot
- Modify: `src/model/workspace_save.rs` — 结构化校验诊断
- Test: `tests/workspace_persistence.rs`

**步骤**
1. 逐个增加跨 profile Redis 重复、同 profile 重复 Tab、Relation/Dashboard/Redis 跨类型重复、Console 与其他 Tab UUID 冲突用例。
2. 交换 fixture 的 profile 顺序再次验证，预期冲突始终被发现；允许首次/重复位置随顺序变化，但不能漏检。
3. 先收集全部 Console 实体 ID，再检查全局打开 Tab ID、Console 引用、SQL 引用和活动项；将所有当前文档区纳入同一唯一性规则。
4. Console 记录→Console Tab 的同 ID 只验证引用，不误判为实体冲突。重复的 Console 记录与重复的打开引用分别报告。
5. 使用记录位置的 Map 替代只保存布尔结果的集合，错误必须包含 UUID、类型和两个来源；内部名称不再用 relation_ids 代表所有非 Console Tab。
6. 显式验证 Redis/Relation/Dashboard 归属及活动 profile 的新格式语义，禁止通过修改外层 profile 掩盖错归属。
7. App 侧尽早校验；WorkspaceStore::save 仍在任何写入前防御性校验，防止非 App 调用绕过。
8. 增加“先保存有效快照，再保存无效快照”的测试，逐字节比较 manifest 和已有 SQL 文件保持不变。
9. 运行：`cargo test --locked --test workspace_persistence`。

**验收**：所有类型和顺序组合都能发现冲突，且错误足以定位源对象；无效快照不写盘。

## W05：贯通错误类型与修订号状态机

**Files**
- Modify: `src/action.rs` — 保存 Action/Command payload
- Modify: `src/model/workspace_save.rs` — QuitSaveState/失败类型
- Modify: `src/model/workspace.rs` — Overlay payload
- Modify: `src/app.rs` — persist_workspace_command、失败/重试/取消/丢弃/flush 分支
- Modify: `src/runtime.rs` — WorkspaceSaveQueue 与错误映射
- Test: 上述文件中的现有单元测试

**步骤**
1. 补结构错误不重放、I/O 失败可以重试的队列测试；测试通过 DTO 判别，不匹配错误英文文本。
2. 为每次保存尝试分配 revision，包括构建失败；构建失败进入相同 Failed 生命周期，但不发送 Persist/Flush，不留无任务可完成的 Closing 状态。
3. Runtime 将 WorkspaceError/worker join 失败转换 DTO，保留 revision；App 构建错误使用同样的失败展示入口。
4. I/O 重试优先处理已有更新 pending 快照；没有新快照时可复用原失败快照。结构错误不允许盲目 Retry，用户取消退出并修复后由正常入口生成新 revision。
5. 队列只对真实成功的 revision 更新 acknowledged；失败快照不会被自动轮询无限重试。失败缓存清除以 revision 为准，不清掉更新失败。
6. 补旧失败晚到、新保存成功先到、取消退出后迟到成功、重试后旧 flush 到达、pending 更新覆盖失败版本的测试。
7. 明确取消退出只解除当前退出等待；已有后台写入可完成，但不能触发 Quit。丢弃退出不承诺撤销此前已成功的自动保存。
8. 运行：`cargo test --locked --lib workspace_save`，再运行 `cargo test --locked --lib quit`。确认输出实际包含目标测试，不能将 0 tests 当成功。

**验收**：结构错误不会循环重试，I/O 重试保留可恢复性，只有当前退出等待 revision 的成功确认可触发正常 Quit。

## W06：增加独立恢复导出

**Files**
- Create: `src/persistence/workspace_recovery.rs`
- Modify: `src/persistence/mod.rs`
- Modify: `src/action.rs`, `src/model/workspace_save.rs`, `src/app.rs`, `src/runtime.rs`
- Create: `tests/workspace_recovery.rs`

**步骤**
1. 定义独立 RecoveryBundle DTO：来源 revision、导出时间、文档 ID/名称/目标、原始 SQL、原始 Tab 顺序及保存错误诊断。不能依赖正常快照先校验通过。
2. 加入重复 Tab 的恢复导出用例，要求每个可读取 Console 的 SQL 都被保留；不同冲突候选不能共用文件名覆盖。
3. App 从实时 editor 捕获恢复内容，Runtime 负责写文件；editor 缺失作为明确条目记录，不能生成伪造空 SQL。
4. 在 workspace 所在目录旁的 recovery 子目录创建 UUID 命名的新 bundle 目录，用条目序号加 UUID 命名 SQL 文件。使用安全创建策略，不覆盖已有 bundle 或正常 manifest/SQL。
5. 先写临时 bundle，完成后 rename 为最终目录；失败返回准确路径与错误，界面不能显示导出成功。
6. 增加导出 Action/Command 与成功/失败回调，携带导出请求 ID、revision；同一导出进行中不重复提交。用户取消后迟到结果不重新打开退出弹窗。
7. 成功后仍保持未保存状态，展示目录并允许用户自行选择取消或退出；不更新 acknowledged_revision、不自动 Quit。
8. 测试正常文件逐字节不变、多个导出互不覆盖、I/O 失败、缺失 editor 的部分导出明确标注、导出成功仍不算工作区保存成功。
9. 运行：`cargo test --locked --test workspace_recovery`。

**验收**：结构错误下仍可导出可读编辑内容；导出失败或不完整有明确结果，不污染正常工作区。

## W07：退出失败 UI 与按键行为

**Files**
- Modify: `src/app.rs` — notify/overlay 决策
- Modify: `src/ui/mod.rs` — render_workspace_save_failed
- Modify: `src/input/keymap.rs` — 弹窗键位
- Modify: `src/help.rs` — 若现有帮助列举相关动作，同步文案
- Test: `tests/ui_render.rs` 及 App/keymap 内单元测试

**交互规格**

| 情况 | 主提示 | 可用动作 |
| --- | --- | --- |
| 后台保存失败 | 单个 Workspace 通知，保留详细诊断 | 现有后台处理入口 |
| 退出时 I/O 失败 | 一个弹窗，说明保存失败和目标路径 | r 重试、e 导出恢复副本、d 不保存本次更改并退出、Esc 取消退出 |
| 退出时结构/序列化失败 | 一个弹窗，说明工作区状态异常、重试同一状态无效 | e 导出恢复副本、d 退出、Esc 取消退出；不显示 r |
| 导出进行中/完成/失败 | 更新当前弹窗状态及结果路径 | 禁止重复导出；完成后由用户决定是否退出 |

**步骤**
1. 补 reducer 测试：同一退出 revision 失败只创建 overlay，不同时追加错误 Toast；后台失败仍通知。
2. 若同 revision 已有后台通知，进入退出失败时合并/隐藏该条，避免旧 Toast 留在弹窗上方；不清除其他通知。
3. 渲染动作由失败能力决定，keymap 使用同样的状态判断；隐藏 r 时按 r 不产生 Retry。
4. 用业务提示替代以 Revision 为标题的文案，revision/UUID/profile 放详细区；d 明确表示不保存本次工作区更改。
5. 按内容高度布局，长 UUID/路径换行；若详情过长，复用现有滚动/详情能力，操作区保持可见。覆盖 80×24 和 60×18。
6. 补 Esc 取消、d 退出、e 导出、I/O r 重试、结构 r 无动作测试；错误提示不包含 SQL 正文。
7. 运行：`cargo test --locked --test ui_render`、`cargo test --locked --lib workspace_save`，并运行新增 keymap 测试的实际名称过滤器。

**验收**：一个错误只有一个主提示，能力与按键一致，窄终端可完成取消/导出/退出。

## W08：完整验收与交付记录

**Files**
- Modify: 本文执行记录
- Modify if needed: `README.md` 中已有工作区/恢复说明；不新建重复使用指南

**步骤**
1. 在隔离的测试工作区中手动复现：A Redis DB0 → B Redis DB0 → B DB1 → A → 修改 Console → 关闭一个 Tab → 退出 → 重启。
2. 核对仅一个同目标 Redis Tab、正确归属、最新 SQL、Tab 顺序、活动项、关闭状态；重启未自动连接。
3. 使用测试注入模拟 InvalidSnapshot 与 I/O 失败，验证单弹窗、重试差异、导出目录和取消行为；不破坏真实用户工作区来制造故障。
4. 运行定向集成组：`cargo test --locked --test global_workspace --test workspace_persistence --test workspace_tabs --test redis_browser_tabs --test profile_lifecycle --test workspace_recovery --test ui_render`。
5. 运行格式与静态检查：`cargo fmt --all -- --check`、`cargo clippy --locked --all-targets -- -D warnings`。
6. 运行项目完整默认测试：`cargo test --locked`。实施时读取仓库当前 CI/贡献说明并补齐其规定检查；外部数据库或服务不可用时记录具体阻塞，不把未执行标成通过。
7. 执行 `git diff --check`，审查 diff；确认生产保存路径无旧缓存 SQL、无读取失败转空串、无盲目 dedup 或重建 UUID。
8. 记录格式版本决策、迁移样例、自动化结果和手动验证结果。文档说明恢复副本位置、部分导出的含义、退出不保存与数据库事务的区别。

**最终验收清单**
- [ ] 截图对应操作链保存与重启成功。
- [ ] 反复连接切换不复制或隐式重绑文档。
- [ ] 打开及关闭 Console 的最新内容完整保存。
- [ ] 混合 Tab 的顺序、活动引用、Redis/Dashboard 归属可 round-trip。
- [ ] 当前格式严格校验，旧格式迁移冲突保留源文件。
- [ ] 跨类型/跨 profile 冲突诊断包含 UUID 和双方来源。
- [ ] 结构错误不盲目重试；I/O 重试与 revision 确认正确。
- [ ] 取消退出、迟到回调、恢复导出均不会误触发正常 Quit。
- [ ] 退出只显示一个主提示，窄终端操作可见。
- [ ] 必要自动化、静态检查及真实 Redis 手动验收有执行证据。

## 4. 依赖与建议交付单元

```text
W01 故障回归
  → W02 全局所有权
  → W03 实时快照与迁移
  → W04 严格校验与诊断
  → W05 保存状态机
  → W06 恢复导出
  → W07 退出 UI
  → W08 完整验收
```

- M1 根因修复：W01–W04，必须包含所有权与保存恢复，不能只过滤重复 Tab。
- M2 可恢复退出：W05–W07，错误类型、队列、UI、导出作为一致交付。
- M3 发布就绪：W08。只有用户另行要求发布时才更新 CHANGELOG、版本或标签。

如用户要求提交，建议通过检查后分为 `fix(workspace): preserve canonical documents across connections`、`fix(persistence): validate and persist canonical workspace state`、`fix(runtime): classify workspace save failures and retries`、`feat(workspace): export recovery bundles on save failure`、`fix(ui): clarify workspace quit save failures`。失败测试随对应修复一起交付。

## 5. 执行记录

- 计划创建：2026-09-14。
- 当前状态：仅完成代码分析与计划编写；未修改生产代码，未运行本文所列测试。
- 待记录：实际复现输出、格式版本选择、每个任务变更及检查结果、环境阻塞、人工验收结果。
