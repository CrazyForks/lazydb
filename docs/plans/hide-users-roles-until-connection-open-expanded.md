# Users & Roles 可见性修复 Implementation Plan

> 执行者：Luna。按下述步骤完成一个端到端业务闭环；实现、纠偏、审查与提交合并均由 Luna 负责，不启动子 Agent。本文遵循本任务的专用目录约束。

**Goal:** 只有非 Redis 连接已开启并展开时，连接树才显示 Users & Roles 及其允许展开的子行。

**Architecture:** 在 ExplorerTreeState 的模型投影层统一 principal 分组可见性，使用 profile 自身的 Online/Syncing 状态与 Profile 展开标记。完整树与局部 profile 投影共用该规则，保留 catalog、加载状态行及 Redis 的现有投影逻辑。

**Tech Stack:** Rust 1.94、现有 ExplorerTreeState、Rust 集成测试、ratatui TestBackend。

---

## 0. 起点、约束与当前证据

- 原工作空间 `/Users/yelog/workspace/tui/lazydb`，目标 main，起点 `45e330cedd3b8da493d1c198329ec38d5e1f38b5`。
- plan 阶段重新执行 `git status --short` 与 `git rev-parse HEAD`，退出 0；HEAD 仍为起点，仍仅有 `src/persistence/workspace.rs` 与 `tests/workspace_persistence.rs` 的原有未提交修改。
- checkpoint.json 再次读取返回 File not found。没有 checkpoint 可恢复，不创建它。
- 当前两项修改是 principal tab 序列化修复，不是本任务前置条件；新 worktree 不携带它们，原工作区不 stash、不清理、不暂存。
- 本计划未执行编译或测试，也未修改业务代码。分析的源码证据及方案比较见同目录 analysis.md；静态验证结果见 validation.md。
- 本次正式 plan 回执为 `plan-cb7ee4b9-71cb-4c3f-acbe-7d0f042cb9d5.json`，token 为 `cb7ee4b9-71cb-4c3f-acbe-7d0f042cb9d5`；完成计划与 change-scope.json 后最后写入，不覆盖历史 analyze 回执。
- 任务名称及任务分支名由 Luna 在计划完成后自动确定，不在 plan 阶段创建分支。

## 1. 单元：连接生命周期与 principal 可见性一致

### 验收与门禁来源

- **用户需求（必须满足）**：连接未开启或未展开时不显示 Users & Roles；开启并展开时允许显示。缓存子行不能绕过父级条件。
- **项目强制 Rust 门禁**：按现有 CI 执行 fmt、clippy、全量 test，具体命令见 Step 6。CI 的跨平台与数据库服务覆盖保留其项目属性，本机未具备环境的部分应如实记录，不能宣称已通过。
- **本计划选定的自动化回归验证**：状态矩阵、真实 App 初始化的 TestBackend 检查、缓存子树/多连接隔离及受影响导航测试；用于证明上述需求及防止回归。它们不要求人工参与。
- **补充建议（不是新增强制门禁）**：人工截图对照、真实终端 PTY、为此次显示修复额外启动真实数据库。无需等待用户人工确认才完成任务；环境受限最多一次针对性修复重试，再由 Luna 依据现有证据收尾。

### 涉及文件

必改：
- `src/model/explorer.rs`：ExplorerTreeState 内增加私有可见性判断；调整 `visible_profile`（原 1427 附近）、`append_profile`（原 1520 附近）。
- `tests/explorer_state.rs`：状态矩阵、缓存子树、局部投影、连接分组和多连接隔离回归。
- `tests/principal_tabs.rs`：复用现有 app_with_profile/render，增加真实启动状态的无数据库渲染回归。

按实际受影响断言定向调整：
- `src/model/explorer.rs` 和 `src/model/workspace.rs` 模块测试中的可见行数/末行断言。
- `tests/explorer_state.rs` 已有 profile_order/other_profiles 等测试明确把 Offline 分组行写入预期，必须更新此错误预期及相关 depth/index。
- 其他测试只有因新可见性契约失败时才调整；优先修正场景真实状态，禁止全局把所有 fixture 改 Online 或粗暴删除失败断言。

本次预计修改的完整文件清单已写入同目录 `change-scope.json`：`src/model/explorer.rs`、`src/model/workspace.rs`、`tests/explorer_state.rs`、`tests/principal_tabs.rs`。没有预计新增/删除/重命名的业务文件，也没有未提交依赖文件。`src/app.rs`、`src/ui/mod.rs`、CI 配置与其他测试文件当前只作为参考或执行验证，不列入修改清单。若实现中的实际失败证明必须扩展范围，Luna 应先记录原因并同步清单，再实施新增改动。

### Step 1：准备执行版本

Luna 按插件约定命名并从指定起点创建任务分支/worktree。记录 worktree 路径、分支、HEAD 与 git status 到任务 validation.md。若实际起点变化，先核对相关 diff，再调整行号，不将原工作区未提交修复视为 HEAD 行为。

### Step 2：添加首个可失败回归

在 `tests/explorer_state.rs` 添加下列测试（现有 imports 已覆盖所用类型）：

```rust
#[test]
fn principal_group_visibility_requires_open_expanded_connection() {
    let id = Uuid::from_u128(710);
    let profile_node = ExplorerNodeId::Profile(id);
    let principal_group = ExplorerNodeId::PrincipalGroup { profile_id: id };
    for status in [
        ExplorerConnectionStatus::Offline,
        ExplorerConnectionStatus::Linking,
        ExplorerConnectionStatus::Failed,
        ExplorerConnectionStatus::Online,
        ExplorerConnectionStatus::Syncing,
    ] {
        for expanded in [false, true] {
            let mut tree = ExplorerTreeState::default();
            tree.add_profile(id);
            tree.profiles.get_mut(&id).unwrap().status = status;
            if expanded {
                tree.expanded.insert(profile_node.clone());
            } else {
                tree.expanded.remove(&profile_node);
            }
            let expected = expanded
                && matches!(status, ExplorerConnectionStatus::Online | ExplorerConnectionStatus::Syncing);
            assert_eq!(
                tree.visible().iter().any(|row| row.id == principal_group),
                expected,
                "full tree: {status:?}, expanded={expanded}"
            );
            assert_eq!(
                tree.visible_profile(id).iter().any(|row| row.id == principal_group),
                expected,
                "profile projection: {status:?}, expanded={expanded}"
            );
        }
    }
}
```

运行：

```sh
cargo +1.94.0 test --test explorer_state principal_group_visibility_requires_open_expanded_connection -- --exact
```

预期旧实现因 Offline/局部投影的分组泄漏而断言失败。若失败来自编译或环境，不能称为证明缺陷，先定位对应原因。

在 `tests/principal_tabs.rs` 增加真实初始化回归：

```rust
#[test]
fn unopened_connection_does_not_render_users_and_roles() {
    let (app, _) = app_with_profile();
    let screen = render(&app, 120, 40);
    assert!(screen.contains("orbital-lab"));
    assert!(!screen.contains("Users & Roles"));
}
```

运行 `cargo +1.94.0 test --test principal_tabs unopened_connection_does_not_render_users_and_roles -- --exact`，记录实际结果。该测试使用现有 TestBackend，不需要真实 SQLite 连接、PTY 或外部数据库。

### Step 3：最小模型修复

在 ExplorerTreeState impl 中增加：

```rust
fn principal_group_is_visible(&self, profile_id: Uuid) -> bool {
    self.expanded.contains(&ExplorerNodeId::Profile(profile_id))
        && self.profiles.get(&profile_id).is_some_and(|profile| {
            profile.kind != DatabaseKind::Redis
                && matches!(
                    profile.status,
                    ExplorerConnectionStatus::Online | ExplorerConnectionStatus::Syncing
                )
        })
}
```

- `visible_profile` 中原 `if profile.kind != DatabaseKind::Redis` 条件改为 `if self.principal_group_is_visible(profile_id)`，其内部 push 不变。
- `append_profile` 中，在现有 `projection.append_state_rows(...)` 之后、创建 `principal_group` 之前加入：

```rust
if !self.principal_group_is_visible(profile_id) {
    return;
}
```

该位置是 principal 子树的唯一入口，保护分组、Principal 与 PrincipalNotice；不会抹掉现有目录/加载提示。不要改默认 expanded 行为、UI 图标函数、驱动或持久化格式。不要通过 active connection 判断开启状态，多连接可以同时开启。

### Step 4：补齐生命周期验收

在同一测试模块用现有 profile/PrincipalEntry fixture 增加以下场景；断言节点 ID、depth 和实际 visible 行，不只断言 helper 返回值：

1. 单个 tree Online + Profile/PrincipalGroup 均展开 + 一个缓存 principal：分组和 principal 可见。依次折叠 Profile、重新展开、切 Offline，再切 Online 并展开，验证隐藏/恢复；缓存 principal 不应绕过父节点条件。
2. principals_loaded=true 且列表为空，PrincipalGroup 展开：Online 时可见 PrincipalNotice，Failed/Linking/Offline 时整个 principal 子树均不可见。
3. Redis + Online + expanded：分组始终隐藏；SQLite + Online + expanded 仍允许分组及 unsupported 提示，避免把数据库能力支持与连接状态混为一谈。
4. 两个连接，一个 Online、另一个 Offline，二者内部 expanded：仅 Online 连接出现分组。外层 ConnectionGroup 折叠时，完整树不出现内部任何 principal 行；展开后恢复正确归属和 depth。
5. 分组隐藏时完整树的可见行不包含分组、用户、提示，visible_profile 不包含分组；现有 find/nav 路径使用完整树结果，应通过定向导航/查找回归保证不选到隐藏分组。

### Step 5：修正已有语义断言并定向测试

首先运行：

```sh
cargo +1.94.0 test --test explorer_state --test principal_tabs
cargo +1.94.0 test --lib model::explorer
cargo +1.94.0 test --lib model::workspace
cargo +1.94.0 test --test explorer_performance --test startup_profiles --test connection_switch
```

预期全部通过。重点检查：
- 排序/分组测试默认 Offline 时预期只剩连接行，而不是 Users & Roles 行。
- 原 expanded_table_rows_have_expected_shape 若测试意图明确为在线完整目录，可只在该 fixture 设置 Online 并保留末行 principal 断言；若保留 Offline fixture，应修正行数及末行预期，目录遍历断言不能被删除。
- find/nav、滚动、选中索引及局部 profile 索引相关断言保持有效，不让行数变更被错误地归因于排序逻辑。
- TestBackend 初始化测试确认连接名称可见，避免因 Explorer 整块未显示而出现假阳性。

### Step 6：一次完整验证与 Luna 审查

依项目 CI 的 Rust 检查执行：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
git diff --check
```

每条记录命令、退出码、执行目录、HEAD、未提交文件、实际数据库服务/工具链环境。不要额外重复 cargo check。修复导致相关代码变化后只重跑必要检查，再根据未解决风险判断是否重跑全量。

若全量失败与原工作区未提交的 principal_kind 修复相关，核对起点与失败代码后记录为独立基线问题，不静默把用户修改复制进任务。普通编译/断言失败由 Luna 自行解决，不以 progress 无限重复同一命令。

人工截图、真实数据库与 PTY 是本显示缺陷的补充检查，不是用户强制项。环境问题最多一次针对性修复重试，然后明确记录限制，由 Luna 收尾审查决定补证；未运行的检查不能标记为通过。

Luna 自审清单：
- 两个 principal 入口条件一致；Syncing 可见，Linking 隐藏。
- 整个 principal 子树受门控，无只隐藏标题留下子行的问题。
- 目录状态、Redis、默认展开意图与多连接状态未被意外改变。
- 修改范围内没有用户原有序列化修复，没有机械删除测试。

### Step 7：提交及交付

功能与验证完成后用明确文件列表暂存本任务改动，禁止 `git add .` 携入无关工作。建议提交信息 `fix(explorer): hide principals until connection is open and expanded`。按插件当前阶段执行提交/合并，不由 Astra 代执行，也不要求用户手动实施。

交付条件：矩阵、真实启动文本和生命周期测试通过；完整检查结果均有当次版本证据或明确归因的限制；计划中的显示需求闭环完成。
