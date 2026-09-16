# SQL Editor 关系型目标限制 Implementation Plan

**Goal:** SQL Editor 仅允许选择和自动采用关系型数据库目标，Redis 保留其正常连接及浏览能力。

**Architecture:** 在数据库类型上定义关系型判断，在 App 的 SQL Editor 边界组合通用目标有效性校验。候选、绑定、默认目标和 editor 专用连接入口共享规则；不修改通用 `ExecutionTarget::is_valid()` 的 Redis 语义。

**Tech Stack:** Rust 2024 / Rust 1.94、App action 状态机、ratatui、现有 Rust integration tests。

---

## 执行上下文与约束

- 前置分析：同目录 `analysis.md`，其中包含根因、候选方案和决策。
- 原工作空间：`/Users/yelog/workspace/tui/lazydb`。
- 基线：`6fc85a946e0c291abb2d02d71fe710bd97768331`；目标分支：`main`。
- 本次 plan 阶段确认 HEAD 仍为上述基线，工作区干净。
- 任务名及分支待插件调用 Luna 确定；worktree、状态文件、阶段流转由插件管理。
- 本计划不创建 worktree、不修改 `state.json`、不启动子 Agent、不实施业务变更。后续在插件指定工作区执行。
- 以下行号为基线导航提示；实现时以函数名定位。
- 本次不要求新依赖、持久化格式变更、数据库服务或运行时 capability 框架。
- 提交时机交由后续工作流安排，不在原工作区直接提交。

## 明确的实现选择

1. 采用穷尽 match 定义 `DatabaseKind::is_relational()`，六种 SQL 类型为 true，Redis 为 false。
2. 在 App 内增加私有 `is_sql_editor_target(&self, target: &ExecutionTarget) -> bool`，校验 profile 存在、关系型、通用目标有效。
3. 保留通用 `execution_target_candidates()`；只在 SQL 专属聚合及当前 profile 的 SQL 选择入口过滤。
4. 保留通用默认目标解析器；SQL 调用方提供过滤后的 profiles，过滤当前 console 继承目标及传入的新建目标。
5. 已保存 Redis 绑定不做磁盘迁移，不丢弃用户 SQL；激活时不发起 editor Redis 连接，允许用户选择关系型目标恢复使用。
6. 空候选采用现有安全空列表及取消行为，本次不增加空列表提示布局改动。原因：并非达成选择限制的必要条件，可以降低 UI 行坐标回归范围；验证空列表可用即可。

## Task 1：建立 SQL 目标资格规则并修复跨连接候选

**Files**
- Modify: `src/profile.rs`，`DatabaseKind` 定义附近。
- Modify: `src/app.rs`，`execution_target_candidates_all_profiles()`（15920 起）附近。
- Test: `tests/workspace_tabs.rs`。

### Step 1 — 添加真实行为回归

增加测试 `sql_editor_target_selector_only_lists_relational_profiles`：

- 使用现有 profile 构造风格创建有效的 PostgreSQL、MySQL、MariaDB、Oracle、SQL Server、SQLite 和 Redis profiles，保证每种 profile 的 database/schema/catalog scope 合法。
- App 创建空 console 后再放入 profiles，或显式选择关系型初始目标，避免测试准备本身产生与断言无关的连接。
- 通过 `OpenConsoleTargetSelector { console_id }` 打开候选。
- 断言六种关系型 profile 的有效目标均出现，Redis profile ID 不出现。
- 将其中一个关系型 profile 命名为 `redis`，Redis profile 命名为普通业务名称，确认规则不依赖名称。
- 断言不要求真实数据库连通。

### Step 2 — 验证缺陷测试失败

Run: `cargo +1.94.0 test --test workspace_tabs sql_editor_target_selector_only_lists_relational_profiles`

Expected: Redis 目标仍出现在候选，断言失败；若失败是 profile 无效或构造错误，先修复测试准备。

### Step 3 — 添加资格定义

在 `DatabaseKind` 的既有 impl 中添加方法；如没有适合的 impl，则放在枚举附近：

```rust
pub fn is_relational(self) -> bool {
    match self {
        Self::Postgres
        | Self::MySql
        | Self::MariaDb
        | Self::Oracle
        | Self::SqlServer
        | Self::Sqlite => true,
        Self::Redis => false,
    }
}
```

App 私有 helper：

```rust
fn is_sql_editor_target(&self, target: &ExecutionTarget) -> bool {
    self.profiles.iter().any(|profile| {
        profile.id == target.profile_id
            && profile.kind.is_relational()
            && target.is_valid(profile)
    })
}
```

### Step 4 — 过滤候选

在 `execution_target_candidates_all_profiles()` 的 `iter()` 和 `flat_map()` 之间添加：

```rust
.filter(|profile| profile.kind.is_relational())
```

保留现有函数名以减少非必要改动，保留候选顺序及 catalog scope 校验。不要过滤通用单 profile 候选构造器。

### Step 5 — 重跑针对性测试

运行 Step 2 命令，Expected: PASS。

## Task 2：覆盖当前连接选择器与确认绑定边界

**Files**
- Modify: `src/app.rs`，`OpenTargetSelector`、`bind_console_target()`、`request_connection_target_for_editor_target()`。
- Test: `tests/workspace_tabs.rs`、`tests/connection_switch.rs`。

### Step 1 — 添加绕过回归

新增以下行为用例：

- `sql_editor_rejects_stale_redis_target_selection`：手动构造包含 Redis 目标的旧 `Overlay::TargetSelector`，`console_id: Some(id)`；确认后 console target、record target 和 SQL 文本未变，命令不包含 Connect/PersistWorkspace。
- `legacy_sql_target_selector_rejects_redis`：同样构造 `console_id: None` 的旧候选；确认不发起 Redis Connect。
- 当前连接为 Redis 时调用 `OpenTargetSelector`，不会产生 Redis 候选或为了 SQL selector 激活新的 Redis SQL workspace。不要断言新的提示文案。

### Step 2 — 运行并确认上述用例揭示缺陷

Run: `cargo +1.94.0 test --test workspace_tabs redis_target`

Run: `cargo +1.94.0 test --test connection_switch redis`

Expected: 实现前至少绑定/连接拒绝断言失败。检查实际匹配测试数量，不能把零测试当成验证通过；必要时用完整测试函数名运行。

### Step 3 — 加入 SQL 边界校验

- `OpenTargetSelector`：取到 profile 后、`activate_profile_workspace()` 之前，非关系型 profile 直接返回空命令；不产生 Redis 候选。
- `bind_console_target()`：在借用可变 tab 或清空结果之前调用 `is_sql_editor_target()`，不合格直接返回；用组合校验替代现有重复的 profile 查找及通用有效性检查。
- `request_connection_target_for_editor_target()`：调用通用连接实现前加入同一 guard。
- 保留 query running、transaction active、pending generation 等现有校验及顺序语义。
- 不修改 `request_connection_target()`、通用 session registry 或 Redis 数据库选择动作。

### Step 4 — 运行回归

Run: `cargo +1.94.0 test --test workspace_tabs explicit_console_target_binding`

Run: `cargo +1.94.0 test --test workspace_tabs redis_target`

Run: `cargo +1.94.0 test --test connection_switch`

Expected: 正常关系型改绑仍更新目标并发出连接命令；事务保护仍生效；Redis 候选不能绕过限制。

## Task 3：统一新建与恢复编辑器的自动目标策略

**Files**
- Modify: `src/app.rs`，`default_console_target()`、`create_and_activate_sql_editor_named()`、`create_sql_editor_named()`、`prepare_active_console_target()`。
- Test: `tests/workspace_tabs.rs`。
- Reference only: `src/model/execution_target.rs`，`resolve_default_target()`。

### Step 1 — 添加默认目标场景测试

至少覆盖：

1. Explorer 选中 Redis，同时存在关系型连接：新 editor 跳过 Redis，按既有策略选关系型目标。分别覆盖 Profile 节点以及 Database Catalog 节点，防止解析器提前返回 None。
2. 最近目标和当前 console 的目标为 Redis：不会继承 Redis；关系型 fallback 可用。
3. console manager 保留的 origin target 为 Redis：新建仍过滤该目标。
4. 仅 Redis 与零 profiles：editor 保持无绑定，不产生 Redis Connect。
5. 从已有 record 打开 Redis 绑定 editor：SQL 文本保留，不因准备目标发起 Redis Connect，仍能通过选择器改绑到关系型。

优先通过公开 Action 设置场景；仅在私有 origin 状态无法用公开动作合理到达时，使用 `src/app.rs` 现有单元测试模块补足，不为测试新增生产公开接口。

### Step 2 — 运行新增测试，确认自动选中 Redis 的缺陷

Run: `cargo +1.94.0 test --test workspace_tabs redis`

Expected: 修复前目标类型或 Connect 断言失败；记录具体失败测试。

### Step 3 — 过滤继承与回退

- `default_console_target()`：当前 console 目标使用 `Option::filter` 配合 `is_sql_editor_target()`；relation 目标用同一规则检查。
- 构造关系型 profile 集合再传给 `resolve_default_target()`。可使用 `self.profiles.iter().filter(...).cloned().collect::<Vec<_>>()`，避免为本次小修复重构解析器 API。
- 向解析器传入 recent_targets；Explorer selected 则先按所属 profile 是否在关系型集合中归一化，非关系型 Profile/Catalog 节点转为 None，再传入解析器。注意 `resolve_default_target()` 的 Catalog 分支使用 `profile_for(id.profile_id())?`：仅过滤 profiles 而仍传入 Redis Catalog 节点会提前返回 None，导致无法回退到最近或其他关系型目标。此处在 App 调用方处理，不改变通用解析器语义。
- 解析结果再次校验 SQL target 有效性，保证异常 profile 不成为新建目标。
- `create_and_activate_sql_editor_named()`：先 take origin target，再用 helper 过滤，在激活 workspace 之前完成。
- `create_sql_editor_named()`：传入的 default_target 也先过滤，再 fallback 到默认解析，覆盖直接调用者。

### Step 4 — 阻断旧 Redis 绑定的准备连接

`prepare_active_console_target()` 取得目标之后、任何连接状态分支或 catalog 请求之前，增加：

```rust
if !self.is_sql_editor_target(&target) {
    return Vec::new();
}
```

确认此前仅用于此函数的 Redis 特判已被此 guard 覆盖；移除该局部冗余分支即可，不扩大到其他 Redis 特判。保持旧 record 和 SQL 内容，用户可重新选择目标。

### Step 5 — 验证默认策略与已有编辑器行为

Run: `cargo +1.94.0 test --test workspace_tabs`

如新增 App 单元测试，再运行其精确名称对应的 `cargo +1.94.0 test --lib <test_name>`。

Expected: 新建、改绑、打开已保存 editor、事务保护等现有测试通过。只有 Redis 时允许无绑定，不能因为没有 SQL 目标而破坏已有离线 SQL 编辑能力。

## Task 4：空候选及 Redis 通用能力回归

**Files**
- Test: `tests/workspace_tabs.rs`、`tests/connection_switch.rs`。
- Reference: `tests/ui_render.rs`、`tests/mouse.rs`、`tests/keymap.rs`。
- 不预期修改 `src/ui/mod.rs`。

### Step 1 — 空列表操作测试

在只有 Redis 的场景中打开跨 profile SQL selector，断言 candidates 为空；依次测试移动及确认不会 panic，不会产生 Connect。重新打开后取消，断言 overlay 关闭。已有 `get(selected)` 和 `len().max(1)` 应使此测试无需新增业务逻辑。

### Step 2 — 通用目标及数据库切换回归

保留一项有效 Redis `ExecutionTarget::from_profile(...).is_valid(...)` 断言，并用现有 Redis profile/连接状态 fixture 覆盖普通数据库选择或普通连接 Action，确认 SQL 限制未扩散到通用入口。不能通过真实网络连接成功与否判断此规则。

### Step 3 — 运行受影响交互套件

Run: `cargo +1.94.0 test --test workspace_tabs --test connection_switch --test keymap --test mouse --test ui_render`

Expected: 全部通过；关系型目标行索引、键盘选择和鼠标命中继续使用同一 candidates，无 UI 层额外过滤。

## 逐项复核检查点

每项任务结束时先完成该项复核，再进入下一项；已通过的相同测试仅在后续改动影响其路径时重跑。

| 任务 | 必须复核的内容 | 验收证据 |
| --- | --- | --- |
| Task 1 | 六种关系型均保留、Redis 被排除、按类型而非名称过滤；helper 不应长期处于未使用状态，Task 2 接入后统一检查编译警告 | 混合 profiles 的 action 测试通过，候选 profile ID 集合符合预期 |
| Task 2 | 类型校验位于任何 tab、workspace、缓存或 session 变更之前；两种 console_id 路径均覆盖 | 旧 Redis 候选确认无连接命令、无改绑，正常关系型改绑与事务测试通过 |
| Task 3 | 过滤候选输入而非仅过滤最后输出；Redis Catalog 节点不触发解析器提前返回；origin target 在 workspace 激活前过滤 | 各自动目标场景可回退到关系型，只有 Redis 时无绑定，恢复 SQL 文本保留 |
| Task 4 | 通用 Redis target 仍合法、空候选安全；没有为键盘和鼠标维护不同候选列表 | 空列表操作和通用 Redis action 回归通过，交互套件通过 |
| Task 5 | 文件范围与需求一致、无状态文件修改、验证结果完整可追溯 | diff 检查通过，记录格式/clippy/测试的实际结果 |

## Task 5：最终检查与交接

### Step 1 — 审查差异

Run: `git diff --check`

Run: `git diff --stat`

检查：
- 生产改动集中于 `src/profile.rs` 与 `src/app.rs`；如果出现额外文件，说明必要原因。
- 通用 `ExecutionTarget::is_valid()`、Redis adapter、session registry 没有被加入 SQL 专属限制。
- SQL 文本、工作区记录格式、事务控制、候选顺序均保持原语义。
- 未修改任务 state.json。

### Step 2 — 执行仓库要求的质量检查

按 `.github/workflows/ci.yml:81-83`：

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

Expected: 格式检查、零警告 clippy、全目标测试通过。如果工具链、Oracle 驱动或外部数据库环境导致阻塞，记录准确错误、已通过的定向测试和未完成的命令，不把跳过的数据库集成测试描述为实际验证成功。

### Step 3 — 输出实现交接

总结修改文件、六种允许类型、Redis 排除策略、默认/恢复目标行为及实际验证结果。提交和后续 review/release 阶段遵循插件安排。

## 完成标准

- SQL Editor 的两类选择入口均不能提供 Redis 目标。
- 旧候选或直接 editor 连接路径不能把 Redis 新绑定到 SQL Editor。
- 新建时不会通过当前 editor、Explorer、recent 或 origin target 自动选择 Redis。
- 无关系型连接时行为稳定，保留无绑定 SQL 编辑能力。
- 正常 Redis 连接和数据库浏览功能保留。
- 关系型改绑、事务约束、键鼠选择和既有持久化行为有回归验证。

## 本阶段记录

已先读取指定 `analysis.md`，调用 writing-plans 技能，并据分析复核与完善本计划，补充 Redis Catalog 默认目标回退的提前返回问题及逐项复核检查点。本阶段未修改业务代码、未运行实现测试、未创建分支或 worktree、未修改 state.json、未调用子 Agent。

用户已选择自动工作流继续实施；本阶段交付后由插件安排下一阶段，不再询问执行方式。计划完成回执使用本次指定 token `70b44d8d-95f7-4b0f-adcc-1d54f5908624`，在本计划保存及检查完成后写入同目录 `plan-70b44d8d-95f7-4b0f-adcc-1d54f5908624.json`。
