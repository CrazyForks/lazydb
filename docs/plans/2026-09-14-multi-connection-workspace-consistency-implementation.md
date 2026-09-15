# 全局 Console 身份、命名与持久化一致性 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
>
> 执行适配：若环境没有上述技能，按本文依赖顺序逐项实施。拟定接口和测试名不表示已经存在；执行前按符号定位当前代码。默认由当前执行者顺序完成，仅在用户要求时委派或创建 Git commit。每个任务以行为测试、实现、定向验证为一个可审查单元。

**Goal:** 消除多连接 Console 的重复 UUID 保存失败，保证全局文档最新内容、名称、绑定和生命周期一致，并统一创建及重命名规则。

**Architecture:** 保留 Action → App::update → Command → Runtime 架构，建立全局唯一 Console 文档集合和唯一编辑器会话；profile 只保存连接相关 UI 状态，SQL tab 按 UUID 引用文档。保存从全局实时状态构建规范快照，旧格式仅在加载边界迁移；SQL 文件清理由成功提交的保存队列管理。

**Tech Stack:** Rust 2024 / Rust 1.94、Tokio、Serde/TOML、UUID v4、Modalkit、Ratatui/Crossterm、tempfile；优先复用现有依赖。

---

## 1. 基线与实施约束

- 分析日期：2026-09-14。
- 前一轮已运行 `cargo test --test global_workspace --test workspace_persistence`：4 + 17 个测试通过。这不是以下新增场景已经通过的证明。
- `src/app.rs`、`src/runtime.rs`、编辑器、Redis 和 UI 等存在进行中的工作；开始实现时先记录 `git status --short` 和相关 diff，逐块整合。
- 关联计划：`docs/plans/2026-09-12-multi-connection-consoles.md`、`docs/plans/2026-09-14-consoles-panel-ui-interaction-implementation.md`。本计划提供后者所需的统一文档来源与激活入口。
- 本计划落地全局文档及持久化一致性；数据库连接池和延迟执行架构遵循已有多连接计划。
- 每个 Task 内的编号步骤是独立执行动作；大型迁移先分批改调用点，保持每批可编译。
- 建议任务完成后形成独立 diff 检查点；若用户要求提交，使用各任务列出的提交主题，并只暂存本任务改动。

### 已确认的故障结构

1. `ConsoleTab::new()` 使用 UUID v4，名称与 ID 无关。
2. `empty_workspace_for()` 固定创建 `console`；自定义创建不检查重名，重命名检查 ASCII 大小写不敏感重名。
3. `snapshot_active_workspace()` 缓存 ConsoleRecord、tabs 和 SQL；`append_workspace()` 又保留全局共享文档。
4. `workspace_snapshot()` 混合读取活动 profile 的实时状态和其他 profile 的缓存状态。
5. `bind_console_target()`、重命名、关闭和删除更新实时状态，旧缓存可能仍然参与保存或恢复。
6. `WorkspaceStore::save()` 在写入前检查重复 UUID，因此错误快照整体保存失败。

## 2. 冻结行为契约

### 2.1 身份与状态

- 文档 UUID 创建一次后稳定；改名、重绑、关闭、重新打开、重启均保持 ID。
- 一个全局 UUID 对应一个 ConsoleDocument 和至多一个打开的 SQL tab。
- ConsoleDocument 是名称、执行目标、事务模式的权威来源；SQL 编辑器会话是文本和编辑历史的权威来源。
- SQL tab 保留运行结果、分页、completion、事务运行状态等临时状态；消除可变文档元数据的重复所有权。
- `open` 由 tab 引用派生，不同时维护独立可写的 record.open 与 tab 集合。
- 连接选择、连接切换和断开不复制、删除或重建文档会话。
- 无 profile、未连接、未绑定、所有 tab 关闭均是合法可保存状态。
- profile 删除后保留 SQL 文档和原目标引用，标记为失效目标；不静默绑定到另一个连接。
- 全局活动 tab 保持用户选择，不因连接回调或迁移重新分组而改变。

### 2.2 名称

- 新建默认名为当前全局文档中未占用的最小 `console_N`，N 从 1 开始；关闭文档仍占用名称，删除后允许复用。
- 不引入“历史永不复用编号”计数器；UUID 才是历史身份。
- 创建与改名共享规则：trim、非空、不含控制字符、ASCII 大小写不敏感比较。非 ASCII 保留精确字符语义，暂不引入 Unicode 归一化依赖。
- 同名检查涵盖所有打开和关闭的文档，不按执行目标分组。
- 改名排除自身 UUID；旧重名文档提交原名应视为无操作成功。
- 旧数据同名不同 ID 必须可加载；界面显示连接/database，仍无法区分时增加短 ID。展示后缀不写回名称。
- 自定义创建冲突在原输入界面显示错误、保留输入，不偷偷改名；自动命名在真正插入前分配。
- 连接建立不隐式新增默认文档，遵循已有多连接计划；显式新建和确有需要的初始占位文档共用名称分配入口。

### 2.3 保存与恢复

- 每个文档在规范快照中出现一次，SQL 内容与 UUID 一一对应。
- 保存不能用读取失败时的空字符串代替 SQL。
- 关闭只移除 tab；删除同时移除文档、会话及引用，文件清理由成功提交后的队列执行。
- 缺失目标不影响 SQL 保存；缺失 SQL 文件必须明确报告，不能伪装成正常空文档。
- 重复 ID 错误包括 UUID、名称和两个来源位置；日志不包含 SQL 正文。
- 格式迁移在首次新格式提交前保留原始 manifest 和其引用的 SQL，失败可恢复。

## 3. 目标结构与兼容策略

### 3.1 全局文档注册表

新增 `src/model/console_document.rs`，初始类型形状如下；沿用 UUID，不要求同时把全部代码改为新 ID 包装类型。

```rust
use std::collections::BTreeMap;
use uuid::Uuid;
use crate::model::{execution_target::ExecutionTarget, transaction::TransactionMode};

#[derive(Clone, Debug, PartialEq)]
pub struct ConsoleDocument {
    pub id: Uuid,
    pub name: String,
    pub execution_target: Option<ExecutionTarget>,
    pub transaction_mode: TransactionMode,
}

#[derive(Default)]
pub struct ConsoleDocuments {
    entries: BTreeMap<Uuid, ConsoleDocument>,
}
```

- 注册表提供查询、遍历、创建、重命名、绑定、删除入口；entries 保持私有。
- “创建新文档”检查名称并生成 UUID；“恢复旧文档”接受已有 ID 和历史重名，但拒绝未规范化的重复 ID。
- UI 排序显式按打开状态、名称、UUID 计算；BTreeMap 的存储顺序不是 tab 顺序。
- App 层更新执行目标时继续执行运行中查询/事务的原有限制，注册表不承担数据库调度。
- 迁移期间可保留只读 ConsoleRecord 投影以缩小调用面；最终不保留可被独立修改的第二份集合。

### 3.2 v6 持久化

- 顶层 `consoles`：所有文档；`tabs`：全局有序标签页；`active_tab: Option<Uuid>`。
- `profiles` 仅保存仍需要的 profile UI 状态，不含 Console 或 SQL 副本。
- Dashboard 在全局 tab 中显式保存 `profile_id`；Relation、RedisBrowser 保留现有目标身份。
- SQL tab 用 Console UUID 引用文档；仅该引用允许与文档 ID 相同，其他 tab ID 不得与 Console ID 冲突。
- v6 不再持久化独立 `open`，由 tabs 派生；v1–v5 的 open 字段仅用于迁移。
- SQL 继续使用 `<uuid>.sql`，这一阶段解决状态来源和删除顺序；整份 workspace 的跨文件崩溃原子性列为后续加固，不把单文件 rename 描述为跨文件事务。
- 内存快照改为规范模型，旧格式类型单独用于 decode，不让业务代码继续处理新旧两种形状。

### 3.3 重复记录迁移规则

1. 先检查版本、路径和记录结构，再读取 SQL；SQL 路径必须为对应 UUID 的合法文件名，拒绝绝对路径和父目录跳转。
2. 按原始位置遍历，保留 `profiles[i].consoles[j]` / `consoles[k]` 来源信息。
3. 同 ID、文档元数据与可获取 SQL 相同：合并文档，合并打开引用，保留一个 tab。
4. 同 ID、元数据或可获取 SQL 不同：在迁移期间为冲突副本分配新 UUID，按该来源重写局部 tab/active_tab 引用，名称追加可辨识恢复后缀并避让；生成迁移报告。
5. 旧格式同 UUID 通常指向同一个 SQL 文件；只能保留目前仍存在的内容，不能声称恢复已被覆盖的历史 SQL。
6. 全局 active_tab 遇到歧义，优先原 active_profile 的来源，再按稳定来源顺序选择，并报告选择结果。
7. 缺失 SQL：返回包含路径和来源的诊断；允许通过恢复导出流程处理可读取文档，不自动成功迁移为空文本。
8. 按“旧全局 tabs 原顺序，然后旧 profiles 文件顺序下的 tabs”合并；v1/v2 无 tabs 时按旧 consoles 顺序和 open 构造。
9. 迁移结果经过严格 v6 校验再交给 App；正常保存绝不临时换 ID 修补冲突。

## 4. 分任务实施

### Task 1：建立真实操作链路的回归用例

**Files:** 修改 `tests/global_workspace.rs`、`tests/workspace_persistence.rs`；必要时在 `src/app.rs` 现有测试模块增加内部绑定场景。

1. 记录工作区 diff 和基线；运行 `cargo test --test global_workspace --test workspace_persistence`。
2. 沿用 `memory_profile()` 和 `connect()`，添加“连接 A → 创建 X → 切换 B → 将 X 绑定 B → 保存”的测试。优先通过现有 TargetSelector Action；私有方法场景放 App 内部测试。
3. 添加 B 活动时对 A 文档编辑、改名、关闭、删除后 save/load 的测试；断言 SQL 正文、名称、ID、target 和 tab 状态，而不只断言 save 返回 Ok。
4. 添加未绑定文档跨连接切换、关闭文档重新加载会话、同名不同 UUID 正常保存的测试。
5. 分别运行新增用例，记录实际失败：重复 ID、旧内容、错误 open 状态或文档复活。若某场景已经通过，保留其保护作用，不强制制造失败。

**验收:** 至少明确复现一条重复 ID 路径，并区分其他风险哪些已被实际复现。

**检查点主题:** `test(workspace): reproduce cross-profile console consistency failures`。

### Task 2：引入唯一文档注册表和统一名称规则

**Files:** 新增 `src/model/console_document.rs`、`tests/console_documents.rs`；修改 `src/model/mod.rs`、`src/model/tab.rs`。

1. 添加注册表行为测试：重复 ID 拒绝、同名新建拒绝、旧重名可恢复、改名不改 ID、关闭文档名仍占用、删除后编号可复用。
2. 执行 `cargo test --test console_documents`，确认缺失实现导致失败。
3. 实现上述类型和私有集合；用 Result 返回 DuplicateId / InvalidName / DuplicateName / MissingDocument 等明确错误。
4. 实现最小空闲编号分配；编号溢出返回明确错误，不回退到可能冲突的名称。
5. 实现 rename 的自身排除及无操作优先逻辑；同一名称规范化函数供所有创建和改名入口使用。
6. 再次运行注册表测试，使用一组确定 UUID 验证迭代/冲突处理结果稳定。

**验收:** 业务可变元数据有唯一受控入口，不再依赖各 UI 分别检查名字。

**检查点主题:** `refactor(console): introduce canonical document registry`。

### Task 3：将 App 生命周期改为全局文档操作

**Files:** 修改 `src/app.rs`、`src/model/tab.rs`、`src/model/workspace.rs`、`src/editor/mod.rs`、`src/editor/tests.rs`、`tests/global_workspace.rs`。

1. 将新建、重命名、重绑、事务模式变更、打开、关闭、删除迁移到注册表；按调用点分批执行 `cargo check --all-targets`。
2. SQL tab 中名称/目标/事务模式读取迁移为 App 的统一文档访问；运行中的 ExecutionDraft 继续使用自己的不可变目标快照。
3. 移除 ConnectionWorkspace 的 SQL、文档和共享 tab 副本；profile UI 状态按需留下 focus/选择信息，不能从旧缓存重新填充文档。
4. 改写 `activate_profile_workspace()`、`snapshot_active_workspace()`、`append_workspace()`、`install_workspace()`：连接活动状态变化不再拥有文档迁移职责。
5. 编辑器区分“首次创建会话”和“修改现有正文”；恢复时使用 has_session 判断，不按 tab 是否打开判断。重复加载已有文档保持 revision、撤销栈、光标。
6. 关闭移除 tab，删除清理注册表、输入/输出会话、pending execution、completion 引用和列表选择；沿用事务退出机制。
7. 删除 profile 保留 SQL 文档，保留失效目标身份；Relation/Dashboard/Redis 按现有生命周期处理，避免批量误删 SQL。
8. 执行 `cargo test --test global_workspace --test transaction_reducer --test profile_lifecycle` 和 `cargo test --lib editor::`。

**验收:** 连接切换不生成第二份 Console；生命周期变更没有需要“稍后同步”的文档元数据。

**检查点主题:** `fix(workspace): remove profile-owned console copies`。

### Task 4：实现 v6 规范快照及完整校验

**Files:** 修改 `src/persistence/workspace.rs`、`src/app.rs`、`src/action.rs`、`src/runtime.rs`、`src/model/workspace_save.rs`、`tests/workspace_persistence.rs`、`tests/workspace_tabs.rs`。

1. 添加 v6 round-trip 测试：两 profile、未绑定和失效绑定、关闭文档、交错 SQL/Relation/Dashboard/Redis tabs、非 SQL active_tab。
2. 定义 v6 decode/encode 类型和规范 WorkspaceSnapshot；为 Dashboard 增加全局恢复需要的 profile 身份。
3. 分两遍校验：先收集所有 Console ID，再检查所有 tab；检查 SQL 一一对应、tab 唯一、active_tab 引用、路径和非 SQL tab 身份。避免校验结果依赖记录排列顺序。
4. `workspace_snapshot()` 改为返回 Result，仅遍历全局文档并读取实时 session SQL；移除所有吞掉文档读取错误的 `unwrap_or_default()`。
5. 更新保存命令构建和退出状态机：快照构建失败也产生有 revision 的保存失败状态，不能进入永不结束的 Saving；worker 不收到不完整快照。
6. 用缺失 session 的测试确认：保存失败携带 Console UUID，既有磁盘 SQL 不被写为空；未绑定/失效目标可保存。
7. 执行 `cargo test --test workspace_persistence --test workspace_tabs --test global_workspace` 及 `cargo test --lib workspace_save`。

**验收:** 保存源只有全局文档；同名不影响保存；无效身份或文本缺失在写入前明确失败。

**检查点主题:** `feat(workspace): persist canonical global workspace v6`。

### Task 5：旧格式迁移、冲突报告与原始数据备份

**Files:** 新增 `src/persistence/workspace_migration.rs`、`tests/workspace_migration.rs`；修改 `src/persistence/mod.rs`、`src/persistence/workspace.rs`、`src/runtime.rs`。

1. 用临时目录构造 v1–v5 fixtures，覆盖目标缺失、旧全局/profile 混合、重复 ID 相同/冲突、同名不同 ID、缺 SQL 和非法路径。
2. 执行 `cargo test --test workspace_migration`，确认尚未迁移的场景失败。
3. 将旧版本 decode 与迁移抽离为独立模块；按第 3.3 节规则输出规范 snapshot 与 MigrationReport。
4. 完成 v6 直接加载路径；未来不支持版本仍返回 UnsupportedVersion，不尝试猜测。
5. 旧版本第一次写 v6 前创建独立备份目录，保存原始 manifest、引用 SQL 和版本信息；备份未完成时不得覆盖原 manifest。load 本身不覆写旧数据。
6. 将迁移报告交给启动通知；明确列出合并数量、恢复副本数量、失效目标和文件位置。
7. 测试 load → save v6 → load 的幂等性：恢复副本 UUID 保持，第二次加载不继续复制；备份内容与原始字节一致。
8. 执行 `cargo test --test workspace_migration --test workspace_persistence --test startup_profiles`。

**验收:** 历史可恢复内容被保留，迁移可解释；备份包含 SQL，能够在后续 SQL 修改后仍还原旧版数据。

**检查点主题:** `feat(workspace): migrate legacy documents with recovery reports`。

### Task 6：恢复流程一次性安装全局状态

**Files:** 修改 `src/app.rs`、`src/runtime.rs`、`src/editor/mod.rs`、`tests/global_workspace.rs`、`tests/workspace_tabs.rs`、`tests/startup_profiles.rs`。

1. 添加未连接启动就能看到并读取已保存文档的测试，以及所有文档关闭时不强制打开首项的测试。
2. `restore_workspace()` 只接受规范 snapshot；创建全部文档及 session 一次，再按持久化顺序恢复 tabs，最后恢复 active_tab。
3. 移除按 selected_profile 过滤文档、强制第一条 console.open=true、默认将失效目标绑定到已选连接的分支。
4. profile 选择仅影响 Explorer/UI；有效性检查生成诊断，不改写文档原目标。
5. 恢复事务模式但清空运行事务状态，沿用现有 generation 恢复规则；不恢复为 Active 事务。
6. 执行 `cargo test --test global_workspace --test workspace_tabs --test startup_profiles --test transaction_reducer`。

**验收:** save/load 保留所有文档、顺序、关闭状态和选择；连接是否在线不决定文档是否存在。

**检查点主题:** `fix(workspace): restore global documents independently of connections`。

### Task 7：统一 UI 创建、改名及重名显示

**Files:** 修改 `src/app.rs`、`src/model/omni.rs`、`src/model/sql_editor_list.rs`、`src/ui/mod.rs`、`src/help.rs`；新增 `tests/console_naming.rs`。

1. 添加公共 Action 层测试：NewConsoleNamed 重复、Omni 输入重复、改名重复、首尾空白、空名、控制字符、大小写、自身无操作。
2. 所有入口调用注册表规则；自动命名在创建时调用，不先在空集合分配后再安装旧 workspace。
3. Omni 命名失败保留步骤和输入并显示字段错误；管理器改名复用同一错误语义。
4. 历史同名列表显示目标辅助信息；同目标同名增加短 UUID，按显示集合检查短 ID 是否仍冲突并加长。
5. 选择和热区继续使用完整 UUID；不以渲染名称回查文档。
6. 执行 `cargo test --test console_naming --test console_documents`，再运行现有 Console manager 的 App/UI 定向测试。

**验收:** 默认创建、自定义创建、改名行为一致；旧重名数据无需改名即可继续使用和保存。

**检查点主题:** `fix(console): unify naming across creation and rename`。

### Task 8：保存队列负责提交后的文件清理

**Files:** 修改 `src/runtime.rs`、`src/action.rs`、`src/app.rs`、`src/persistence/workspace.rs`、`tests/workspace_persistence.rs`；扩展 `src/runtime.rs` 的 WorkspaceSaveQueue 测试。

1. 添加失败注入测试：删除文档后 manifest 保存失败，旧 SQL 必须仍在；成功后才能清理。
2. 移除 App 删除路径独立发出的 DeleteSqlFile；将待清理 ID 与保存队列的 revision/最新引用集合统一管理。
3. 保存成功后根据已提交 manifest 引用集执行清理，检查更高 pending revision 不重新引用该 ID；相同 worker 串行化保存和清理。
4. pending 快照合并时不遗失删除意图；重试旧失败快照不能覆盖已成功提交的更新 revision。
5. 清理失败单独报告并留待重试，已成功提交的 manifest 不标记为未保存；仅清理应用管理的且已确认不被引用的 UUID SQL 文件。
6. 添加“save r1 失败 → offer r2 → r2 成功 → retry”及“保存中连续创建/删除”的顺序测试。
7. 执行 `cargo test --lib workspace_save`、`cargo test --lib failed_flush`、`cargo test --test workspace_persistence`。

**验收:** 文件删除不超前于 manifest 提交，旧重试不倒灌旧状态，pending 合并不导致悬空引用。

**检查点主题:** `fix(persistence): clean SQL files only after committed saves`。

### Task 9：准确诊断与失败恢复导出

**Files:** 修改 `src/persistence/workspace.rs`、`src/model/workspace_save.rs`、`src/action.rs`、`src/app.rs`、`src/runtime.rs`、`src/ui/mod.rs`、`src/input/keymap.rs`、`src/help.rs`；新增 `src/persistence/workspace_recovery.rs`、`tests/workspace_recovery.rs`，并注册 persistence 模块。

1. 为重复 Console ID、重复 tab、缺文档、缺 SQL/session 增加结构化错误信息；用户摘要与详细来源共用一个诊断对象。
2. 将“取消退出”“重试 I/O”“导出恢复副本”明确映射到保存状态；结构错误不无限重试同一个失败快照。
3. 恢复导出独立遍历可用 live session，不能依赖先构造成功的严格 snapshot；同 ID 冲突按来源序号生成独立文件名。
4. 导出到 App 数据路径下独立 recovery 子目录，用 create_new 防覆盖；写 SQL、元数据和错误报告，报告哪些文档未能读取。
5. 导出成功显示路径，但不把 workspace 标记为 Clean；取消退出后继续编辑，再保存时构建新 revision。
6. 在 keymap、帮助和弹窗增加恢复动作；验证窄终端的错误换行和按钮可达性。
7. 执行 `cargo test --test workspace_recovery`、`cargo test --lib workspace_save` 和保存失败弹窗的现有定向测试。

**验收:** 用户知道哪个文档冲突，并可在正常保存失败时导出仍在内存中的 SQL；恢复动作不伪造保存成功。

**检查点主题:** `feat(workspace): expose conflict diagnostics and recovery export`。

### Task 10：完整回归、文档与交付

**Files:** 修改 `docs/architecture.md`、`README.md` 中相应 workspace/Console 说明；检查本计划涉及的源码与测试。

1. 补齐第 5 节验收矩阵中尚未覆盖的组合场景，执行定向测试。
2. 检查生产代码中是否仍有 profile 持有 SQL 副本、按名称定位文档、读取失败写空串、连接切换重建 session 的路径。
3. 更新架构文档：唯一状态来源、关闭与删除语义、名字规则、v6 迁移备份路径和恢复导出操作。
4. 运行与 `.github/workflows/ci.yml` 一致的检查：

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

5. 区分缺少外部数据库环境与代码失败；记录实际执行、跳过及失败项目，不将跳过当作通过。
6. 在临时数据目录进行手工验收：双连接编辑/重绑/改名/关闭/删除/重启；用独立历史 fixture 验证迁移和恢复备份。
7. 审查 diff，确认每个新接口仅有一套职责，记录迁移行为变化与测试结果。

**验收:** 全部新增行为测试及项目要求检查通过；若受环境限制，明确列出未验证项及 CI 验证位置。

**检查点主题:** `docs(workspace): document global console persistence and recovery`。

## 5. 最终验收矩阵

| 场景 | 必须满足的结果 |
| --- | --- |
| A → B → A 反复切换 | 文档数和 UUID 稳定，SQL 和历史保持 |
| A 文档绑定 B，再保存 | ID 只出现一次，目标为 B，SQL 不变 |
| B 活动时编辑 A 文档 | 保存和重启后为最新正文 |
| B 活动时改名 A 文档 | 重启后为新名，ID 不变 |
| B 活动时关闭 A 文档 | 文档保留、tab 关闭，重启不强开 |
| B 活动时删除 A 文档 | 切换和重启不复活，成功提交后才删文件 |
| 未绑定 / 失效目标 / 无 profile | 可编辑、保存、恢复，目标不被静默替换 |
| 所有 tabs 关闭 | 合法保存并恢复为空 tab 集合 |
| 跨类型 tab 交错 | 全局顺序、active_tab、目标保留 |
| 旧同名不同 ID | 全量保留，不覆盖 SQL，列表可区分 |
| 新建和改名冲突 | 同规则阻止，输入仍在，无多余文档/session |
| 旧重复 ID 相同副本 | 迁移合并，有报告 |
| 旧重复 ID 冲突副本 | 可读取内容保留、引用重映射、二次加载幂等 |
| SQL/session 缺失 | 明确失败，不静默写空 |
| 保存失败后出现更新 revision | 旧重试不覆盖新成功状态 |
| 删除时保存失败 | 旧文件仍存在，恢复副本可导出 |
| 运行查询 / 手动事务时重绑 | 沿用事务限制，异步结果不应用到错误目标 |

## 6. 依赖、交付和后续加固

推荐顺序：Task 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9 → 10。

- 检查点 A：Task 1–3，确认重复状态来源被移除；中间重构不作为可发布迁移版本。
- 检查点 B：Task 4–6，完成 v6 保存、兼容迁移和恢复，新增一致性回归全部通过。
- 检查点 C：Task 7–10，统一交互、提交后清理、失败恢复和完整验证后交付。
- v6 上线后旧版本应用不能直接读新 manifest；回退操作使用独立备份中的 manifest + SQL 整体恢复，先另外保留升级后产生的新内容。
- 若需要提前发布止血补丁，应从 Task 1 复现结果中提取“现有格式下只从权威实时集合构造快照”的独立修复，连同删除和未加载文档测试单独验证；不能仅在 save 时静默去重。

### 后续加固：跨文件崩溃一致性

这项与重复 UUID 根因修复分开验收：引入不可变版本化 SQL 文件或 revision 目录，所有新 SQL 完成写入与同步后原子替换 manifest，旧文件仅在新 manifest 提交后清理。需要独立格式设计和失败注入测试，覆盖写到第 N 个 SQL 失败、manifest rename 失败和提交后进程退出；断言重新加载只能得到完整旧版或完整新版。当前 v6 固定 `<uuid>.sql` 方案只保证单文件替换与删除顺序，不宣称具备这一性质。
