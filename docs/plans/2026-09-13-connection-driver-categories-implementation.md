# Connection Driver Categories Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
>
> 若执行环境没有该技能，按本文依赖顺序逐项实施；每项实现后完成定向测试及差异复核。本文不授权提交、推送或合并。新增接口名为拟定设计，不表示已经存在。

**Goal:** 在 New/Edit Connection 的 Driver 上方增加“关系型数据库 / 非关系型数据库”分类选择，Driver 仅显示当前分类的候选且恢复产品全称，同时保证窄终端可操作、连接字段同步正确。

**Architecture:** 以驱动注册表统一产品分类、机器名称和展示全称；当前分类由 ProfileDraft.kind 派生，不添加 profile 持久化字段。表单维护分类内最近 Driver 和自动端口来源，鼠标/键盘共用模型动作；Driver 使用单行全称窗口，布局算法独立于渲染和连接 I/O。

**Tech Stack:** Rust 2024、现有 ProfileDraft/ProfileManagerState、Ratatui/Crossterm、Serde 配置与既有测试基础设施；无需新增生产依赖。

---

## 1. 工作位置与代码基线

- 实施 worktree：`/Users/yelog/workspace/tui/lazydb-redis-support`。
- 分支：`task/redis-support`。
- 该 worktree 有大量 Redis 未提交改动。先检查 `git status --short --branch` 和相关 diff，不覆盖、不回滚其他实现，不再创建同名 worktree。
- 本轮只调整连接表单和必要的字段同步、测试/帮助；不扩展 Redis 数据浏览、会话池或命令执行。

已核对的相关实现：

| 文件/符号 | 当前行为 | 需要变化 |
| --- | --- | --- |
| `src/db/descriptor.rs::DatabaseDescriptor` | kind、机器 name；注册七种驱动 | 增加 category、display_name；提供统一查询 |
| `src/model/profile_manager.rs::DRIVER_ORDER` | 已从全部 DRIVERS 生成 | 表单切换改为分类内列表，不能仅过滤 UI |
| `ProfileField`、`visible_fields()` | 首项为 Kind；不同产品字段数组 | 在 Kind 前统一加入 DatabaseCategory |
| `ProfileManagerState::cycle` | Kind 使用全量 DRIVER_ORDER | 分类与分类内 Driver 独立循环 |
| `ProfileManagerState::select_driver` | 直接调用 set_kind | 明确选择的 Driver 自动决定分类，更新最近选择 |
| `ProfileDraft::set_kind` | 端口为空或等于上个产品默认端口才替换 | 用草稿端口来源避免 SQL Server→SQLite→Redis 遗留 1433 |
| `src/ui/profiles.rs::render_driver_options` | 不够宽时显示 PG/MY/MA 等简称，随后超宽 break | 删除简称；使用全称可见窗口 |
| `src/ui/profiles.rs::render_form` | FormRow 单行模型及滚动 | Category 进入真实字段序列；Driver 保持单行 |
| `src/ui/mod.rs::HitTarget` | 有 ProfileDriver/Field 等 | 增加分类及窗口导航命中 |
| `src/input/keymap.rs`、`src/input/mouse.rs` | 表单优先路由，支持 cycle/click | 新字段遵守同一路由和 busy 状态 |

现有 `InteractionModel::{Relational, KeyValue}` 是执行/浏览能力分类，不能等同产品的 NonRelational。产品分类单独定义，避免未来文档数据库被误映射为 KeyValue。

## 2. 必须满足的行为契约

### 2.1 分类与 Driver

| 分类 | 显示的 Driver（顺序固定） |
| --- | --- |
| Relational / 关系型数据库 | PostgreSQL、MySQL、MariaDB、Oracle、SQL Server、SQLite |
| Non-relational / 非关系型数据库 | Redis |

界面沿用项目英文文案：字段 `Category`，选项 `Relational`、`Non-relational`。文档解释它们对应关系型/非关系型；本任务不建立国际化框架。所有 Driver 标签使用产品全称。

1. 新建默认 Category=Relational、Driver=PostgreSQL，初始焦点在 Category。
2. 编辑已有 profile 从 kind 推导 Category，保留原 Driver/端口/TLS，不因进入表单重设默认值。
3. Category 行 Left/Right 或鼠标切换分类；焦点留在 Category，Down/Tab 再进入 Driver。
4. Driver 行 Left/Right 仅在当前分类内循环，不切到隐藏产品。
5. 分类只有一个 Driver 时仍显示 Driver 行，左右键无变化，不触发 URL 刷新或发现失效。
6. 同一草稿生命周期内记住每个分类最后一个 Driver：MySQL→Redis→Relational 恢复 MySQL；首次进入分类选注册表中首个 Driver。
7. 新建另一份草稿重置该记忆；不缓存每种 Driver 的完整表单，也不跨进程持久化最近分类。
8. 显式 Driver 选择和成功 URL 解析自动更新分类及该分类的最近 Driver；失败解析不部分更新。
9. 测试/保存进行中，分类、Driver 和窗口导航动作均不可修改草稿，不能只禁用鼠标样式。
10. Category 改变必须同时选定合法 kind；模型中不允许 Category 与 kind 矛盾的中间状态。

### 2.2 持久化与接口兼容

- `ConnectionProfile.kind` 仍为产品身份的唯一持久化来源。
- 不新增 ConnectionProfile.category，不升级 profile 文件版本。
- descriptor.name 保持 `postgres/mysql/mariadb/oracle/sqlserver/sqlite/redis`，不更改 CLI/API 名称。
- 分类内最近选择、端口来源属于草稿临时状态，不序列化。
- descriptor 展示全称只用于 UI/文案，不替换日志/API 中依赖既有机器名称的字段。

### 2.3 Driver 全称窗口

- 宽度足够显示全部当前分类项；名称永不改为 PG/MY/MA 等简称。
- 空间不足时按选中项计算一个连续的完整候选区间，选中 Driver 始终可见；左右未显示部分以 `‹`/`›` 或 ASCII `<`/`>` 标识。
- 箭头操作与 Driver 左右循环一致；不是独立的列表滚动状态，不允许滚出当前选中项。
- 在当前候选范围内为每项注册命中；窗口外项没有命中区域。
- 可用宽度仅容纳当前名称时，优先省略该项图标及箭头，保留全称。极端情况下连全称都放不下，显示完整的单行切换器弹层/应用极小尺寸提示，不退回产品简称。
- 首版不让 Driver 自动换行，不重写 FormRow 为可变高度。Category 也可在窄窗口退化为当前全称加左右切换，仍可选择两类。
- 选中 Driver 的视觉高亮在焦点转移到 Name 等字段后保持；busy 时保留选中信息但禁用操作。
- 字符宽度使用终端 display cells，不使用 String::len。覆盖 NerdFont/Unicode/ASCII 图标模式。

### 2.4 字段切换与端口来源

采用最小状态 `PortOrigin::{Automatic, Explicit}`（拟定名），不为每个输入字段建立通用来源框架：

| 事件 | 端口来源及行为 |
| --- | --- |
| 新建草稿的默认端口 | Automatic |
| 用户修改、粘贴、删除 Port | 非空有效/无效输入均为 Explicit；空值在切 Driver 时允许重新默认 |
| 编辑已有 profile | 有端口则 Explicit，不猜测旧值是否用户自定义 |
| 成功 URL 导入 | 若 URL 显式指定 port 则 Explicit，否则 Automatic；解析结果需保留该信息 |
| Driver 切换 | Automatic 或空端口使用目标默认端口；Explicit 非空原样保留 |
| 经过 SQLite | 不清掉“自动”来源；回到网络产品时重新采用目标默认端口 |
| 字段内部程序化赋值 | 不冒充用户编辑，不把 Automatic 自动改成 Explicit |
| Port undo/redo | 视为用户编辑，保守标 Explicit；不承诺恢复来源历史 |

例如自动 SQL Server 1433→SQLite→Redis 应为 6379；用户明确设置 6380 后跨分类切换继续保留 6380。

切换 Driver 统一调用已有 set_kind 路径，完成：默认端口、URL 格式、适用 schema/DB/TLS、derived scope、连接发现失效、URL 重新生成。

- 首次从关系型切到 Redis：若 Database 不是合法 Redis 编号，草稿将其设为 0；schema 不传入 Redis profile。
- 合法 Redis DB 编号及用户自定义 host/user/port 保留。
- 显式 scope 不能直接沿用关系型 schema 限制到 Redis；跨分类时重建目标产品的 derived scope，清除旧发现快照，并在表单显示一次非阻塞说明。用户保存前仍可调整范围。
- TLS 显式配置不静默降级为明文；Redis 表单只给出实际支持的选项，不能满足的旧模式需明确提示或规范到等价验证模式。
- 类型改变不执行连接、不保存文件；只有 Test/Save 等动作触发相应 I/O。

## 3. 目标结构与更新流程

### 3.1 驱动注册表

拟定接口：

```rust
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DatabaseCategory {
    Relational,
    NonRelational,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DatabaseDescriptor {
    pub kind: DatabaseKind,
    pub name: &'static str,
    pub display_name: &'static str,
    pub category: DatabaseCategory,
}
```

提供 `descriptor(kind)`、`category(kind)`、`drivers_in(category)`。所有产品必须恰好注册一次，分类集合非空。默认选项来自注册顺序，不在 UI 另建数组。

### 3.2 原子更新

```text
鼠标/键盘 Category 选择
  → App 校验当前表单 + busy + 动作上下文
  → ProfileManagerState::select_category
  → 草稿记录离开分类的 Driver
  → 取目标分类最近合法 Driver，否则取首项
  → ProfileDraft::set_kind
  → 更新字段、scope、发现身份、URL
  → 下次渲染由 kind 派生分类和候选列表
```

URL commit 路径只在解析完全成功后更新 kind/分类记忆/port origin；模型不进行“重新生成 URL 后再次解析”的反向循环。

### 3.3 渲染与命中统一结果

新增纯布局函数（建议位于 `src/ui/profiles.rs` 的私有 helper），输入当前分类 descriptor 列表、选中 kind、available cells 和 icon mode，输出：

- 可见候选区间；
- 每项的实际 x/width/是否显示图标；
- 左右导航区域；
- 极窄单项模式。

绘制和 HitTarget 都消费同一输出；不要渲染计算一次、鼠标命中再估算一次。选择改变后重新计算，不持久化窗口 start index。

## 4. 逐项实施任务

每项步骤按测试→实现→复核顺序。编写新测试后先运行确认失败原因确实来自缺失行为，不用放宽断言让实现过关。测试命令均在 Redis worktree 执行。

### Task 1：基线与驱动分类注册表

**Files:** 修改 `src/db/descriptor.rs`；新增 `tests/driver_categories.rs`；必要时调整 `src/ui/profiles.rs::kind_name` 调用。

1. 检查 git status 和上述文件 diff，记录已有未提交实现及当前分支。
2. 执行 `cargo test --test profile_draft --test profile_url`，记录基线。
3. 为七种产品编写注册唯一性、机器名称不变、display_name 完整和分类成员/顺序测试。
4. 增加 DatabaseCategory、descriptor 元信息和分类查询函数。
5. 将 UI kind_name 委托到 display_name；保持 CLI 使用机器 name。
6. 执行 `cargo test --test driver_categories --test profile_url` 和 `cargo check --all-targets`。

**复核：** 新产品只需改一份注册数据；无“Relational 六项、NonRelational 一项”的 UI 手写复制；不引入 profile 序列化字段。

### Task 2：Category 字段及分类内切换状态

**Files:** 修改 `src/model/profile_manager.rs`；扩展 `tests/profile_draft.rs`、`tests/driver_categories.rs`。

1. 新增 ProfileField::DatabaseCategory，将其放在所有产品 visible_fields 的 Kind 之前；更新新建/编辑默认焦点。
2. 添加 category() 派生访问器和每分类最近 Driver 的草稿状态，new/edit 初始化一致。
3. 实现 select_category：验证目标分类、保存旧 Driver、恢复新分类最近 Driver 或首项，通过 set_kind 原子切换。
4. Kind 的 cycle 只使用当前分类候选；select_driver 显式更新分类记忆；单项列表和选择当前分类均为 no-op。
5. URL commit 成功时更新最近 Driver；失败保留原分类、字段和记忆。
6. 调整旧 driver_cycle 测试：SQLite 后回 PostgreSQL，不能再转 Redis；另测 Category 切到 NonRelational 得到 Redis。
7. 测试 MySQL→Redis→MySQL、编辑 Redis、新建第二草稿不继承记忆、焦点遍历次序、URL 导入失败。
8. 执行 `cargo test --test profile_draft --test driver_categories --test profile_url`。

**复核：** 任何时刻 kind 与派生 Category 一致；类别切换不直接做 I/O；新增字段可通过键盘导航到达。

### Task 3：修复端口来源与跨分类连接字段联动

**Files:** 修改 `src/model/profile_manager.rs`、`src/profile.rs`；扩展 `tests/profile_draft.rs`、`tests/profile_url.rs`。

1. 为 Automatic→SQLite→Redis、Explicit 6380 跨分类保持、Port 清空后切换默认等行为编写失败测试。
2. 增加草稿 PortOrigin；将用户输入、paste/delete/undo/redo 与程序化 set 的调用路径区分，不以字符串相等判断是否自动。
3. 解析结果增加“URL 是否显式指定端口”的临时元信息；在各 URL parser 中准确设置（SQLite 无端口）。不从已经补默认值的 ParsedConnectionUrl.port 反推来源。
4. URL commit 与 edit/new 使用第 2.4 节规则；若现有 parsed struct 调用方受影响，一次性更新所有构造和测试。
5. set_kind 在跨类别时规范 DB、schema、derived scope 和 TLS 可用选项；自定义 host/user/port 不无故覆盖。
6. 失效发现 fingerprint，防止切换后旧 Test/Visible Objects 回复污染新 Driver；URL 只刷新一次。
7. 测试带明确端口/缺省端口 URL、非法 Port 错误焦点、TLS 不静默降级、旧 SQL profile 行为。
8. 执行 `cargo test --test profile_draft --test profile_url --test redis_contract --test execution_target`。

**复核：** Redis 自动默认值不会再显示 1433；自定义端口不会因分类切换丢失；来源信息不写配置文件。

### Task 4：Action、键盘与鼠标语义接入

**Files:** 修改 `src/action.rs`、`src/app.rs`、`src/input/keymap.rs`、`src/input/mouse.rs`、`src/ui/mod.rs`；扩展 `tests/profile_reducer.rs`、`tests/keymap.rs`、`tests/mouse.rs`。

1. 增加 ProfileSelectCategory 动作、ProfileCategory 命中类型；命中参数携带 category，必要时携带草稿身份/当前 Driver 上下文。
2. App 仅在正确表单页面、当前草稿和 operation=None 时处理 Category/Driver 变更。沿用现有忙碌保护，补齐直接 Action 调用测试。
3. Category 行 Left/Right 调用分类切换；Up/Down/Tab 遍历字段，Enter/Space 沿用 cycle 字段习惯。
4. Driver 左右调用分类内循环；窗口箭头映射相同语义并将焦点定位 Driver。
5. stale 鼠标命中必须在当前 category/候选列表重新确认；不能通过旧分类隐藏 Driver 的点击切回类别。
6. URL 显式选择产品是合法跨类别入口，与旧鼠标命中校验区分处理。
7. 测试忙碌状态、没有草稿、Scope/ConfirmDelete 页面、键盘和点击一致性。
8. 执行 `cargo test --test profile_reducer --test mouse`；执行受影响的 keymap 测试。完整 keymap 有失败时对比基线，不仅凭“未改该函数”宣布无关。

**复核：** 不只绘制按钮，所有输入路径实际可用；操作不会串到工作区快捷键；busy 状态无副作用。

### Task 5：Category 行与全称 Driver 窗口

**Files:** 修改 `src/ui/profiles.rs`、`src/ui/mod.rs`；新增 `tests/profile_driver_layout.rs`；扩展 `tests/ui_render.rs`。

1. 删除现有 compact_name 分支及 PG/MY/MA 等产品简称映射；删除测试中接受“全称或简称”的放宽逻辑。
2. 实现第 3.3 节纯布局函数，先测试候选窗口在选中首/中/末项、精确可容纳、差一格和极窄宽度下的输出。
3. Driver 渲染只遍历当前分类；完整列表宽度足够时不显示多余箭头。
4. 超宽时枚举包含选中项的连续区间，计入图标/空格/箭头实际 cell width，选择能容纳最多候选且选中项靠近中间的区间，使用固定 tie-break 保证重绘稳定。
5. 当前全称可放下但图标不可放下时隐藏图标；不可放下全称时进入明确单项选择弹层或全局尺寸不足提示，不截成另一产品名。
6. Category 行绘制两个全称选项，选中样式与 Driver 一致；窄屏仅显示当前分类加切换提示。
7. 用同一布局结果注册所有命中；busy 不注册改变配置的区域；hit regions 不越过字段边界、不覆盖下一行。
8. 测试 160×36、120×36、100×30、80×24、56×16；三种 IconMode；选择 SQL Server/SQLite/Redis；所有输出只有当前分类 Driver。
9. 执行 `cargo test --test profile_driver_layout --test ui_render`。

**复核：** 全称恢复且可达性不退化；当前项始终可识别；图标/文字/点击区域一致；Category 不破坏固定底部 URL 与操作栏。

### Task 6：焦点、帮助、表单滚动和 UI 回归

**Files:** 修改 `src/ui/profiles.rs`、`src/help.rs`、`src/ui/shortcut_hints.rs`；扩展 `tests/profile_draft.rs`、`tests/profile_reducer.rs`、`tests/ui_render.rs`、`tests/keymap.rs`。

1. 更新 form_rows 分类分段：Category/Driver 均在 CONNECTION 下；不要创建重复 section。
2. 新增一行后的 viewport_start、selected_index、光标显示及鼠标 focus 行映射全部复核；初始 Category 在最小终端可见。
3. footer 在 Category 和 Driver 显示准确 Left/Right change 提示；不把 Category 当文本输入显示插入光标。
4. 更新依赖字段数量/索引/初始 Kind 焦点的测试，不只改 expected count，要验证完整遍历到 Save/Cancel 仍成立。
5. 保存/测试失败后错误焦点定位真实字段；分类切换清掉失效的 URL/发现消息但不掩盖新字段错误。
6. 执行 `cargo test --test profile_draft --test profile_reducer --test ui_render --test mouse`，再运行 keymap 相关测试。

**复核：** 新建/编辑/删除/Scope 原路径不被破坏；选择分类后用户不需要鼠标才能继续完成连接配置。

### Task 7：端到端验证与文档

**Files:** 新增 `tests/profile_category_flow.rs`；修改 `docs/configuration.md`、`docs/keybindings.md`；按需更新 `docs/redis.md`（若该文档尚不存在，先确认是否属于当前 Redis 文档工作，不编写未实现能力声明）。

1. 用真实 Keymap→Action→App 流程执行：新建→非关系型→Redis→填写名称→验证提交；断言 6379/DB 0/无 schema/URL 同步。
2. 执行 MySQL→Redis→MySQL，断言分类记忆、自定义端口保持、字段集重算；验证不因渲染改变模型。
3. 通过 URL 导入 rediss profile，断言 Category/Driver/TLS/字段一致；用失败 URL 验证原子性。
4. 用 TestBackend 渲染并从命中区域发送 mouse 事件，验证分类与 Driver 选择；分别验证窗口边界候选和键盘循环。
5. 序列化保存 profile 后断言没有 category、last_driver、port_origin 字段；旧机器 name 和配置兼容测试通过。
6. 文档说明分类选择和分类内循环，不把非关系型分类等同于“支持所有 NoSQL”。
7. 执行 `cargo test --test profile_category_flow --test persistence --test profile_lifecycle`。
8. 最后执行 `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo check --all-targets`、`cargo test`，记录实际失败及对照证据。
9. 检查相关 git diff 和所有新增测试，确认没有顺手覆盖 Redis 浏览任务；只在全部相关检查通过后执行 `cargo build` 并给出本 worktree binary 路径。

**复核：** 用户在 New Connection 可以实际用键盘/鼠标完成两级选择；测试证明恢复全称而非改宽截图掩盖溢出；没有配置格式迁移。

## 5. 任务依赖与里程碑

```text
Task 1 注册表
   → Task 2 分类状态
      → Task 3 字段来源与同步
         → Task 4 输入动作
            → Task 5 分类行与全称窗口
               → Task 6 帮助/焦点/滚动
                  → Task 7 端到端与交付
```

共享 `profile_manager.rs`、`app.rs` 和 `ui/profiles.rs` 的改动顺序集成，不同时建立多份相互冲突的列表。用户未要求子代理，本计划不要求启动子代理。

- 里程碑 A：模型层分类和 Driver 一致，URL/端口联动测试通过。
- 里程碑 B：New/Edit Connection 显示分类，全称在窄屏也可达。
- 里程碑 C：键盘/鼠标端到端、配置兼容和构建通过。

## 6. 最终验收清单

- [ ] Category 在 Driver 上方，真实可聚焦。
- [ ] 新建默认 Relational/PostgreSQL，编辑从原 kind 派生分类。
- [ ] Relational 只显示六个关系型 Driver；Non-relational 只显示 Redis。
- [ ] Driver 使用 PostgreSQL/MySQL/MariaDB/Oracle/SQL Server/SQLite/Redis 全称。
- [ ] 分类内循环不会选到隐藏产品，单项分类无副作用。
- [ ] 分类往返恢复该草稿最近 Driver，不缓存/持久化整份表单。
- [ ] URL 成功导入同步类别，失败不部分更新。
- [ ] 自动端口跨 SQLite 仍跟随默认值，自定义端口保留。
- [ ] Redis DB/schema/TLS/scope 不继承不兼容的关系型字段。
- [ ] 窄窗口保留全称、当前项可见，隐藏项没有鼠标命中。
- [ ] 忙碌状态和过期鼠标事件不能变更 Driver。
- [ ] 三种图标模式、最小终端、底部 URL/按钮区正确。
- [ ] profile 配置无新增类别字段，CLI 机器名称不变。
- [ ] 定向测试、Clippy、格式、构建有实际执行结果。

## 7. 完成记录要求

每个 Task 在执行时记录实现文件、定向测试命令、通过/失败结果及 diff 复核结论。未运行的测试不得标记通过；现有全仓失败应记录具体测试与基线对照，不重复声称“829 passed, 1 failed”是整个仓库测试总数。

本文件只描述实施计划，编写本文件时没有修改应用实现或运行功能测试。
