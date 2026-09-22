# Explorer 新增连接入口统一 Implementation Plan

**执行者：** Luna。Astra 仅负责本分析/计划；不启动子 Agent。工作流及任务分支名称由 Luna 在计划完成后确定。

**Goal:** 移除普通 Explorer 的 n 新增快捷键，使无连接时 Enter 直达新连接表单、a 打开 ADD TO CONNECTION 菜单。

**Architecture:** 复用现有 ExplorerAdd overlay、菜单导航和 ProfileStartNew reducer，通过 Option<Uuid> 表达无连接上下文。保留已有 Enter→ExplorerOpenSelected→EmptyProfiles→ProfileStartNew 链路，新增真实按键路径回归。帮助可用性独立表达 Add，避免放宽 Edit 权限。

**Tech Stack:** Rust 1.94、crossterm 按键、ratatui/TestBackend、现有 App reducer 与集成测试。

---

## 基线与执行约束

- 起点/计划时 HEAD：`7b76fe9286fd99199ce6fc99a9e013264640b885`，目标 `main`；本次 `git status --short` 无输出。
- 无需要迁移的未提交文件。后续若工作空间有变化，先辨别归属，不能清空、整体提交、stash 或假定其自动进入 worktree。
- 本阶段只写本任务目录；不创建分支/worktree、不修改 index 或业务代码。后续按任务编排确定 worktree；如采用新 worktree，必须从明确基线创建。
- `checkpoint.json` 本阶段仍不存在，不创建、不修改。本轮完成回执固定为 `plan-ea9728b4-0c0d-4098-bbf1-1007c97ffb10.json`，token 为 `ea9728b4-0c0d-4098-bbf1-1007c97ffb10`，不复用 analyze 回执。
- 实现、验证、审查与提交合并均由 Luna 完成。本计划不要求询问实现方式或等待反复 resume。
- 只做一个可验收业务闭环；下列步骤是该闭环内部顺序，不把中间代码状态当成交付。

## 单元：空 Explorer 建立首个连接，普通新增入口收敛

### 需求、门禁与补充验证的等级

- **用户需求（必须交付）**：移除默认 Explorer n 新增入口；No profiles 上 Enter 打开新增连接表单；a 打开 ADD TO CONNECTION 菜单。
- **实现正确性与回归验收**：真实按键链路、禁用菜单项、帮助一致性与原有搜索/节点快捷键保持正常，采用自动化测试证明。
- **项目强制 Rust 门禁**：`.github/workflows/ci.yml` 已规定的 fmt、clippy 与 all-targets/all-features tests；步骤 5 给出原命令。其他平台/真实数据库 CI 的结果按实际环境记录，不声称本地已执行。
- **补充建议验证**：人工首次启动操作、PTY 体验检查。本任务不把它们新增为必需门禁；受限时最多一次针对性修复重试，再由 Luna 收尾审查决定补证或记录限制。

### 补充发现：公开自定义按键配置兼容性

分析阶段未覆盖的一条引用已在本阶段确认：`src/config.rs:88` 接受公开配置命令 `explorer-new-profile`，`src/input/keymap.rs:2744-2745` 可将用户显式绑定的按键映射为 ProfileStartNew，`docs/configuration.md:182-184` 公开描述该能力。`KeyBindings::matches` 查询配置中的 commands，不是 map_explorer 中的默认裸键分支。

本任务决定保留显式配置兼容性，只移除默认 n 新增及其默认帮助元数据；不移除 `src/config.rs` 的允许项或 `map_configured_navigation` 的显式绑定分支。用户主动将 n 重新绑定为新增属于明确配置覆盖，默认 n 无动作的验收以未设置该覆盖为前提。`docs/configuration.md` 补充该命令没有默认快捷键、可显式配置的说明；在 `tests/keymap.rs` 补充自定义非 n 按键仍可新增的兼容性测试。删除帮助 ID 前核对多键配置分发是否借用帮助目录；若借用，需在同一 keymap 文件保留独立动作解析以维持已支持的序列能力，并增加相应回归。这是本计划对 analysis.md 中“未发现持久化契约”的补充修正。

### 步骤 1：建立行为回归与引用清单

**文件：** `tests/keymap.rs`、`tests/startup_profiles.rs`、`tests/explorer_add.rs`；参考 `tests/ui_render.rs` 和 `tests/mouse.rs` 的菜单夹具。

1. 核对 `ExplorerAddMenu::new`、`menu.profile_id`、`ExplorerNewProfile`、`explorer-new-profile` 的全部引用，以及普通 Explorer 与已确认 find/search 的输入优先级。
2. 在现有 keymap 测试中加入 `empty_explorer_enter_opens_new_connection_form`：创建空 App、设 Focus::Explorer，用 Keymap 映射 Enter，把所得 Action 交给 App::update，断言 overlay 为 ProfileManager 且页面为 Form。此测试在基线应通过，不能把它描述为现有 Enter 缺陷。
3. 加入 `empty_explorer_add_menu_opens_new_connection_form`：同样从空 App 开始，a 映射 OpenExplorerAdd，update 后断言 ExplorerAdd，默认 Connection；再通过菜单的 Enter keymap/update 断言新连接 Form。此测试在修改前预期失败于 a 映射。
4. 修改 `explorer_catalog_mutation_maps_selected_profile_root_actions_by_stable_id` 中 n 的旧期望；增加空状态普通 n 不触发新增的断言。
5. 拆开 `explorer_catalog_mutation_synthetic_nodes_are_noops` 的 EmptyProfiles 新语义，继续验证 Others、Status、Empty 的 a/e 无操作；不要删除整组保护。

定向运行（预期为上述明确的新行为差异，而非无关错误）：

```sh
cargo +1.94.0 test --test keymap empty_explorer
cargo +1.94.0 test --test keymap explorer_catalog_mutation
```

记录实际结果；如果测试名放在其他集成文件，命令相应使用实际文件，不虚构已执行记录。

### 步骤 2：菜单无连接上下文与 reducer 闭环

**文件：** `src/model/explorer_add.rs`、`src/app.rs`、`src/ui/mod.rs`；所有菜单构造测试引用。

1. 将 `ExplorerAddMenu.profile_id` 改为 `Option<Uuid>`，构造器同样接收 `Option<Uuid>`。既有 profile 菜单传 Some，EmptyProfiles 传 None；同步现有直接构造调用，不引入 nil UUID。
2. `open_explorer_add` 的目标匹配规则：
   - Profile(id) → `(Some(id), false)`。
   - PrincipalGroup/Principal/PrincipalNotice → `(Some(id), true)`。
   - EmptyProfiles → `(None, false)`。
   - 其他节点与无选中节点 → 无操作。
   - Some(id) 仍需检查 profile 存在；None 不执行已有连接查找。
3. `explorer_add_options` 接收可选 ID。无连接时 Connection、Connection Group 可用；Database/User/Role 不可用，统一给出 `Create a connection first`。保持五项顺序与默认首个可用项，不另建 overlay。
4. `confirm_explorer_add`：Connection/Connection Group 继续分发 ProfileStartNew/ProfileGroupCreate；数据库类分支先匹配 Some(profile_id)，None 不发出数据库创建命令。保留既有 selected_kind 对禁用项的过滤和后续权限检查。
5. `render_explorer_add`：通过可选 ID 查找 profile；无连接目标显示 `TARGET  No connection selected`，有连接保持连接名与驱动。标题仍为 `ADD TO CONNECTION`。
6. 修改模型签名影响的现有测试构造器，追加空菜单不可选择数据库项的行为覆盖；复用原有跳过禁用项测试机制。

### 步骤 3：按键与帮助一致性

**文件：** `src/input/keymap.rs`、`src/help.rs`、`src/app.rs`。

1. 删除 `map_explorer` 中普通 `Char('n') => ProfileStartNew` 分支。
2. `a` 对 EmptyProfiles 返回 OpenExplorerAdd；保留 Redis、Profile、principal、ConnectionGroup 与 catalog 路由的顺序和语义。不要将所有无 profile_id 节点都视为 EmptyProfiles。
3. 保留普通 Enter 映射和当前 reducer 链路，不添加第二套空状态 Enter 逻辑。
4. 移除 ExplorerNewProfile 帮助 ID、目录行、旧字符串 ID 映射和 App 中对应帮助动作分发。保留 Action::ProfileStartNew 和它的其他调用者。
5. 新增独立的 Add 能力/requirement，例如 `explorer_add_available` / `ExplorerAddAvailable`。根据与打开菜单一致的节点规则计算：EmptyProfiles 可用，Profile/principal 要有对应真实 profile；其他节点不通过该菜单能力。让 ExplorerAddToConnection 使用它，ProfileEditAvailable 保持原逻辑。
6. 同步 ShortcutCapabilities 的默认值和内部测试构造。帮助执行仍走现有 OpenExplorerAdd。
7. 验证已确认 find/search 的 n/N 优先级维持原有导航行为，其他 overlay/编辑器/确认框的 n 不改。

### 步骤 4：提示、文档与交互验证

**文件：** `src/ui/mod.rs`、`docs/keybindings.md`、`docs/configuration.md`、`tests/ui_render.rs`；必要时 `src/help.rs` 内部测试及 `tests/mouse.rs`。

1. fallback 文案改为明确的 `Enter: new connection · a: add menu` 或同义表达，移除 `Press n`。
2. 快捷键文档移除普通 n 新增行；补充 Enter 在 No profiles 上新建连接，a 在 No profiles/Profile 上打开菜单，并保留 catalog/Redis/分组的上下文说明与 n/N 搜索说明。不批量改写历史计划。
   在 `docs/configuration.md` 说明 `explorer-new-profile` 仅作为显式自定义绑定入口保留；默认新增使用 Enter/a。
3. 用 TestBackend 渲染空 App 按 a 后的菜单，断言标题、无连接目标及禁用原因；保留已有 ASCII 图标测试。
4. 验证空状态帮助有 Add，无旧 n 新增、无 Edit profile；有连接时 Add/Edit 正常。避免依赖整屏快照或与实现逐行同构的测试。
5. 通过真实 keymap→update 验证 Escape 关闭菜单，以及默认 Connection 的 Enter 打开 Form。检查 Connection Group 复用原创建表单。
6. 复核有连接 a、Redis a、分组 a 和 catalog a 的现有测试；仅为没有覆盖的受影响边界补测。

单元定向检查：

```sh
cargo +1.94.0 test --test keymap --test startup_profiles --test explorer_add --test ui_render --test mouse
cargo +1.94.0 test --lib help::
```

预期所有被选中测试通过；外部依赖、无匹配测试或环境跳过需要实际记录，不能等同于行为已验证。

### 步骤 5：整体验证与 Luna 审查

代码齐备后执行一次仓库 CI 的 Rust 检查：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
git diff --check
```

预期退出码均为 0。fmt 若发现格式问题，格式化相关变更后复查；编译/测试失败修复实际问题，不以普通错误阻塞任务。只有代码或环境发生相关变化时才重跑已通过检查；无需另跑重复的全量 cargo check。

将命令、退出结果、相关文件、代码版本/工作区状态和环境写入本任务 `validation.md`。保留 analyze 阶段记录，不把其静态检查当作本阶段测试结果。

审查要点：

- 只有普通 Explorer n 新增入口被移除，搜索导航不受影响。
- 无连接菜单不伪造连接身份、不误发数据库命令。
- Enter 与 a 后 Enter 均能从真正空 App 到达新增 Form。
- Add 帮助可用性不借用 Edit 能力，普通帮助与可执行帮助一致。
- 不误改用户文件、不新增持久化配置迁移、不引入不需要的动作/弹窗体系。

人工/PTY 检查属于补充验证，不是用户明确强制项。环境受限最多一次针对性修复重试，再由 Luna 审查记录限制或补充 TestBackend 证据，不无限延续 progress。真实数据库及其他平台 CI 覆盖如未本地执行，要明确记录。

### 步骤 6：闭环交付

Luna 完成审查及必要纠偏，确认目标改动和测试全部就绪后，仅提交本任务实际改动文件；不 `git add .`。推荐一个完整业务提交：`fix(explorer): unify connection creation shortcuts`。提交与合并按当前工作流授权执行，目标分支 main，不覆盖后来出现的用户修改。

验收清单：

- [ ] 普通空/非空 Explorer 的 n 不再新增。
- [ ] No profiles Enter 打开新增 Form。
- [ ] No profiles a 打开 ADD TO CONNECTION，默认 Connection，Enter 打开新增 Form。
- [ ] 空菜单禁用 Database/User/Role，Connection Group 仍可创建；Esc 正常关闭。
- [ ] 既有各节点 a 行为、find/search n/N 无回归。
- [ ] 帮助、可执行帮助、UI 提示与当前文档一致。
- [ ] 定向与项目级验证有本次实际记录，环境限制明确。
- [ ] Luna 完成审查、提交/合并和按该阶段指定 token 写入回执。

## 本计划阶段完成情况

计划已经明确实现范围、依赖顺序、单一端到端验收单元和验证方法，无实现方案待用户裁定。当前首个未完成动作是由 Luna 从上述基线准备实施上下文并建立步骤 1 的行为回归；Astra 本阶段不进入实现。

预计变更文件的机器可读清单见同目录 `change-scope.json`；不包含仅读取的 src/config.rs、CI 配置、模型 workspace 或历史计划。没有未提交依赖文件、重命名或删除文件，也不额外生成仓库 docs/plans 文档。任务目录内分析/计划/回执是编排产物，不计入业务变更清单。
