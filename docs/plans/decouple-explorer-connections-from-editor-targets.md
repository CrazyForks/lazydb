# SQL Editor target 隔离实施计划

> **执行者：Luna。** 按本计划完成实现、审查、纠偏及后续授权的提交合并；Astra 只执行分析与计划。当前阶段不启动子 Agent。

**Goal:** Explorer 连接操作不改变任何现有 SQL Editor 的所属目标，快捷键和鼠标 target 控件统一按文档 UUID 显式切换。

**Architecture:** 复用 `ConsoleTab.execution_target`、`bind_console_target` 与 `SessionRegistry`，移除普通连接生命周期中的隐式重绑定。新建文档负责目标初始化，激活既有文档只准备其已经绑定的会话。保留现有工作区合并和连接能力，不增加数据库适配层或存储格式。

**Tech Stack:** Rust 2024、项目 Rust 1.94.0、App action reducer、crossterm 输入、ratatui UI、现有 Rust 集成测试。

---

## 0. 基线、范围和交接约束

- 原工作区：`/Users/yelog/workspace/tui/lazydb`。
- 起点：`cb5bd810515557461710b4fef6de8ac50c09aebb`；目标分支 main；任务名与任务分支仍待 Luna 命名。
- plan 阶段复核 HEAD 与起点一致，`git diff --stat` 无输出。checkpoint.json 仍不存在；未创建插件状态。
- 前阶段根因与方案比较见同目录 `analysis.md`，关键源码已经在当前对话读取。本阶段不重复通读历史日志。
- 计划输出在本任务目录，业务代码与测试不在本阶段修改。
- 正式 plan 指令已指定本轮回执 `plan-f61db3bb-a282-4e6e-8eaf-fb3d32139c6c.json`，token 为 `f61db3bb-a282-4e6e-8eaf-fb3d32139c6c`；阶段完成后仅写该回执，不改历史 analyze 回执。
- 若实施时工作区变化，先核对实际 diff，只调整受影响步骤；不要覆盖用户工作或已有 `.git-opencode-tasks/`。

## 1. 端到端验收契约

本需求作为一个可验收业务闭环推进：连接隔离、显式选择器、持久化三部分全部完成才算功能完成。下面任务是内部执行顺序，不是需要用户逐次 resume 的交付边界。

### 验证分级及来源

| 类别 | 内容 | 完成判据 |
| --- | --- | --- |
| 用户需求验收（必须） | Explorer 开连接不改原 SQL Editor 所属连接；快捷键和鼠标 target 可显式切换 | 使用固定文档 UUID 的 reducer/输入回归证明；不要求必须采用人工操作 |
| 项目 Rust CI 门禁（必须） | `.github/workflows/ci.yml:81-83` 的 fmt、clippy、all-targets/all-features tests | 功能齐备后执行并记录真实退出结果；环境无法满足时明确记录未通过/未完成项，由 Luna 收尾处理，不能伪报通过 |
| 本修复定向回归（实施验证） | 连接失败/快路径/离线恢复/缓存/持久化、旧目标执行路由及事务守卫 | 任务二至四给出的测试覆盖可复用现有测试；根据实际改动选择定向命令，避免重复执行无关套件 |
| 补充建议（非新增门禁） | 人工 PTY 鼠标点击复现、额外真实数据库服务检查 | 有环境且有证据缺口时再执行；缺少人工环境不自动阻塞本需求 |

项目其他平台和服务 CI 作业继续遵循仓库 CI；本计划不把全部跨平台作业变成本地新增人工门禁。对环境受限检查最多一次有针对性的修复重试，随后区分必需项未完成与可选项省略，交由 Luna 收尾决定；不无限循环同一检查。

### 必须成立

1. 现有编辑器 E 属于 A，Explorer 请求/成功连接/失败连接/复用已连接 B 后，E 的 UUID、SQL 与 target=A 保持。全局 connection 可以变为 B。
2. B 的失败不写入 E.target_error；迟到事件不改绑 E。
3. 离线恢复的 E、target=None 或无效 target 的 E 不因普通连接被重新赋值。
4. 鼠标 target 和编辑器快捷键作用于同一具体 UUID，候选涵盖所有关系型连接；即便全局连接=B、E.target=A，打开选择器也以 E 为对象并选中 A。
5. 用户确认 B 后，仅 E 被显式绑定 B；取消不变。复用现有 Space B 语义：确认即绑定，连接失败仍保留用户所选 B，允许重试。
6. 查询运行或事务未结束时，显式绑定被既有守卫拒绝。
7. 新建文档仍可初始化目标；既有文档重新激活时可连接自己的目标；保存、恢复和缓存不会撤销显式绑定或重复 UUID。

### 范围约束

- 不要求 Explorer 激活前后 active tab UUID 恒定；已有打开/切换其他 tab 行为不是本次所述原文档改绑。测试始终按保存的 E UUID 断言。
- 不改变存储版本，不重构整个 session/transaction 控制器，不增加依赖。
- 旧版持久化迁移发生在加载边界，不能为方便本次测试而全面移除迁移逻辑。

## 2. 任务一：建立能定位原文档的回归用例

**文件：** `tests/connection_switch.rs`、`tests/global_workspace.rs`。

1. 在现有 memory_profile/server 与 connect helper 基础上，新增普通 A→B 连接回归。连接 A 后记录 E.id、E.execution_target、SQL；通过 `RequestProfileConnect` 请求 B，提取 Connect 命令 generation，发送匹配 `ConnectionSucceeded`。
2. 在请求前、请求后、成功后三处按 E.id 查找 SQL tab，断言 execution_target 未变，不能用成功后的 active_console 替代 E。成功后同时检查 E 的 SQL、结果状态未被 target-switch 输出替换。
3. 增加失败分支：预存 E.target_error，发送 B 的 `ConnectionFailed` 后原值不变；重试 B 成功仍不改 E。
4. 拓展 global_workspace 保存/恢复测试，按 E.id 检查持久化目标及恢复后的目标，保留 UUID 唯一与 SQL 文本断言。
5. 使用新测试的实际函数名定向执行，确认失败点是 target/target_error 的旧行为而不是 setup 或工具链。建议统一新测试名前缀 `explorer_connection_preserves_`，命令：

```sh
cargo +1.94.0 test --test connection_switch explorer_connection_preserves_
```

**预期：** 修复前在 A→B 成功归属断言或失败错误归属断言失败；将实际结果记录 validation.md。出现环境错误只做一次有针对性修复重试，不将环境失败当回归已经复现。

## 3. 任务二：切断普通连接与文档重绑定

**主文件：** `src/app.rs`。

按以下顺序修改，以减少中间状态误判：

1. 删除 `request_connection`（基线 15868-15885）设置 pending_target_console 的判断及单文档 generation=0 的直接 target 赋值。保留 target 解析、连接可用性、会话请求和现有事务/加载检查。
2. 删除私有字段 `pending_target_console`、初始化及其所有读取、回退和清理。成功分支（11133 附近）只允许匹配的显式 pending_editor_target_switch UUID 进入旧式延迟绑定；失败分支（11584 附近）同样不再把普通连接错误归给当前文档。
3. 删除 `ConnectionSucceeded` 中对全部 console 的 should_default 自动补绑块（11253-11267）；保留 `tab.execution_target == target` 时更新 execution_connection 的逻辑。
4. 检查该循环的其他逻辑：不能把 B 连接成功扩展为对 A 派生状态的重置。若新回归证明同循环还有跨目标副作用，以目标/identity 对应关系约束相关分支；不要顺便重构事务状态机。
5. 删除 `activate_profile_workspace`（1192-1207）对已加载 tabs/records 的归属归一化。保留 workspace 获取、append、tab 激活和 normalize_focus；清理因此不再需要的局部变量。
6. 保留 `empty_workspace_for`（1146-1165）、新建 console 的默认 target 初始化；保留 append_workspace 的 UUID 去重。
7. 检查 `request_connection_target_inner` 已连接快路径和异步路径都没有隐式 target 写入。保留 pending_editor_target_switch：它还用于 `prepare_active_console_target`，不能因字段名称相似而整体删除。

**定向验收：** 运行任务一前缀测试。通过后再继续 selector 与剩余边界；不在这里反复全量测试。

## 4. 任务三：统一鼠标、快捷键和配置 action

**文件：** `src/app.rs`；按必要性修改 `src/input/mouse.rs`，测试 `tests/mouse.rs`、`tests/keymap.rs`、`tests/connection_switch.rs`。

1. `Action::OpenTargetSelector`（10016）若有当前 SQL console，直接按其 UUID 转发 `OpenConsoleTargetSelector`。在取得 UUID 前不得按全局 active_profile 激活 workspace。
2. 无当前 SQL console 时不创建或重绑定其他文档；保持合适提示/无操作。若某个旧测试依赖该 action 自动建文档，将 setup 改为显式创建/连接，除非发现真实产品调用依赖；后者在实现记录中说明具体调用者与兼容处理。
3. 保留 `OpenConsoleTargetSelector` 的关系型全 profile 候选、当前项定位与 `console_id: Some(id)`。
4. `ConfirmTargetSelector` 有 UUID 的路径继续复用 `bind_console_target`，保持运行中/事务守卫、派生状态清理、record/cache 同步、目标准备与 PersistWorkspace。
5. 鼠标可以继续映射 OpenTargetSelector，通过 action 层统一行为；不必同时改多个输入入口。测试鼠标点击经过实际 hit-target action 后 overlay.console_id 确实等于 E，并且没有把 active tab 切到 B 的文档。
6. 保留已有快捷键约定：Space B 与 editor effect 都按 UUID；验证配置的 open-target-selector action 也经相同入口。
7. 旧 console_id=None overlay 的确认分支是否还能从生产路径到达，依据实际调用检查决定是否清理；无论清理与否，不移除 session 激活用 pending_editor_target_switch。
8. 将 `target_selector_switches_only_after_matching_connection_success` 等旧测试改为新显式绑定契约：确认即绑定；连接失败保留选择；验证过期成功不会改绑其他文档。不要仅删除原有失败、重试和 generation 断言。

**定向命令：**

```sh
cargo +1.94.0 test --test mouse target
cargo +1.94.0 test --test keymap target
cargo +1.94.0 test --test connection_switch target
```

**预期：** 鼠标/快捷键对象一致、取消无变更、显式绑定可跨 profile、连接失败可重试，且已有事务限制通过。

## 5. 任务四：快路径、离线恢复与缓存闭环

**文件：** `tests/connection_switch.rs`、`tests/consoles_lifecycle.rs`、`tests/global_workspace.rs`、`src/app.rs` 内 tests；必要时 `tests/workspace_persistence.rs`。

1. 新增 B 已在线复用测试：先连接 A、B，回到 A 文档，再激活 B；确保 E.target=A；随后完成另一个有效请求，确保没有旧 pending 串扰。
2. 复用 consoles_lifecycle 持久化 fixture，恢复单个离线 E/A，首次 Explorer 连接 B，覆盖原 generation=0 提前写入条件。
3. 构造已存在 target=None 文档与一个无效 target 文档，普通连接完成后各自目标保持；通过显式 selector 能修正目标。
4. 多文档 + workspace 缓存再次激活，检查各 UUID 目标。显式 E 从 A 绑到 B 后保存/恢复只出现一个 E，缓存不会恢复旧归属。
5. 修改 `src/app.rs` 中 `rebinding_a_cached_console_to_another_profile_does_not_duplicate_its_id_on_save` 的 setup：不再断言 Explorer 已把原 A 文档改为 B；需要 B 前置状态时明确调用 bind/selector，保留原保存去重目的。检查附近 rebinding 测试是否同样依赖旧行为。
6. 回归首次连接创建默认 console、新建文档目标解析、Explorer expand_after_connect/catalog 请求、激活旧 editor 连接其原目标；Redis 连接不得写入 SQL 文档 target。
7. 对运行中/事务 editor 做显式绑定负例；已有覆盖可复用，不增加镜像实现的测试。

**按实际改动执行一次相关套件：**

```sh
cargo +1.94.0 test --test connection_switch --test global_workspace --test consoles_lifecycle --test workspace_persistence
cargo +1.94.0 test --lib rebinding
```

如改动 app 内其他测试，使用实际函数名前缀补充定向运行。涉及真实 SQL 路由的既有 connection_switch runtime 测试应保留，用命令 target/实际数据库结果证明绑定隔离，而不只检查 UI 标签。

## 6. 任务五：最终检查与 Luna 收尾

**代码审查清单：**

- 普通 RequestConnect/RequestProfileConnect 的请求、成功、失败与已有 session 快路径没有主动写现有文档 target。
- 剩余 execution_target 赋值都可解释为新建、持久化恢复/迁移、显式绑定或其同步，不是 Explorer 当前连接推导。
- 点击 target 不先切换文档；合法候选排除 Redis；重绑守卫保留。
- pending_editor_target_switch 的 generation 校验、单飞请求、deferred_console_activation 仍一致。
- 新测试按固定 UUID 验证，旧测试契约更新有业务依据；持久化无重复 ID。

**功能闭环完成后执行项目 Rust CI 核心检查一次：**

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

来源 `.github/workflows/ci.yml:81-83`。只有后续代码/环境改变或新失败才重复相关检查。编译或普通断言错误由 Luna 修复，不能作为要求用户继续的阻塞。

**证据与环境：** 在本任务 `validation.md` 追加每个实际命令、退出结果、代码版本/工作区状态、环境和相关文件。运行前后清楚区分静态分析、未执行计划和已取得结果。数据库服务矩阵、macOS 二进制依赖、跨平台安装检查属于 CI 其他作业，按实际相关性记录，不能声称本地执行了全部平台验证。

人工 PTY 操作为可选补充：A editor 输入 SQL → Explorer 开 B → 回 A editor 检查 target → 点击 target 选 B。环境受限最多一次有针对性修复重试，然后由 Luna 收尾决定证据是否足够并记录限制；不得循环 progress。

**完成条件：** 七项验收契约满足、相关测试与最终检查结果有记录、Luna 审查通过。后续提交/合并按届时工作流授权执行，不由本 plan 阶段操作 main。推荐逻辑上一个完整 bugfix 提交，消息可为 `fix(editor): preserve targets when opening explorer connections`；不要求为了内部步骤制造多个不可验收提交。

## 7. 下一步

由 Luna 命名任务与任务分支，进入实现阶段后从任务一的固定 UUID 回归开始，连续推进到完整闭环。本计划不产生业务修改，不执行实现或审查，不需要用户再次选择执行方式。
