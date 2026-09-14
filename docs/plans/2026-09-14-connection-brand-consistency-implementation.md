# Connection Brand Consistency Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 新建连接默认聚焦 Driver，统一 SQL Server 品牌色、Redis 品牌图标及工作区 Tab 的图标颜色，并保证多连接与窄窗口行为正确。

**Architecture:** 继续以 UI 层的 `IconSet::database()` / `database_color()` 作为数据库品牌的唯一来源。在 UI 层按 Tab 自身绑定解析数据库类型，将图标与标题拆为独立样式片段，并以实际终端单元宽度统一驱动渲染和命中区域。新建焦点通过 `ProfileManagerState` 初始化控制。

**Tech Stack:** Rust 1.94 / edition 2024、Ratatui 0.30.2、nerd-font-symbols 0.3.0、现有 TestBackend 渲染测试。

---

## 执行说明

- 这是实施计划；当前提交的工作产物仅为本文档。
- 执行时先检查工作区及同文件中的其他进行中修改，按符号定位；下文行号是规划时参考位置。
- 使用当前环境实际可用的计划执行能力；若头部引用的技能不可用，按本文任务顺序执行即可。
- 改动较小的焦点与常量优先更新既有测试；新增测试集中验证跨界面一致性、多连接归属和分段渲染布局等行为。
- 每个编号步骤尽量控制在 2–5 分钟；编译和完整检查耗时另计。

## 一、已核实的现状

| 位置 | 当前行为 | 目标 |
| --- | --- | --- |
| `src/model/profile_manager.rs:1843` `new()` | 默认 Category | 默认 Kind（UI 标签 Driver） |
| `src/model/profile_manager.rs:1863` `start_new()` | 重置为 Category | 重置为 Kind |
| `src/model/profile_manager.rs:1873` `start_edit()` | 编辑从 Category 开始 | 维持当前编辑策略 |
| `src/ui/icons.rs:204` `database()` | Redis Nerd Font 使用 `md::MD_DATABASE` | 使用 `dev::DEV_REDIS` |
| `src/ui/icons.rs:236` `database_color()` | SQL Server 为 RGB(204,41,48) | RGB(13,127,228)，即 `#0D7FE4` |
| `src/ui/profiles.rs:659` `render_driver_options()` | 图标、名称分段；图标调用品牌色 | 复用现有行为 |
| `src/ui/mod.rs:2525` Explorer 连接节点 | 调用统一图标及品牌色 | 自动获得新映射 |
| `src/ui/mod.rs:2048` `render_tabs()` | 图标和标题拼成单色字符串 | 图标品牌色，标题状态色 |
| `render_tabs()` Dashboard 分支 | 硬编码 PostgreSQL | 根据 Dashboard 自身绑定取 kind |
| `render_tabs()` SQL 分支 | 目标缺失时回退到活动连接 | 缺失时通用图标，避免串连接 |

当前依赖的本地源码已确认存在 `nerd_font_symbols::dev::DEV_REDIS`，无需升级依赖。

当前三个品牌展示入口已经共享 `IconSet::database()`。Redis 需求应落实为统一采用专属品牌图标与修复 Tab 样式，而不是假设存在三套 Redis 字形配置。

## 二、明确展示契约

### 2.1 焦点与图标

1. 每次新建连接，焦点为 `ProfileField::Kind`。
2. 表单视觉顺序和字段遍历顺序仍为 Category → Driver → 后续字段；从 Driver 向上可以返回 Category。
3. Redis 在 Nerd Font 模式使用 `dev::DEV_REDIS`；Unicode / ASCII 模式使用 `RD`。
4. SQL Server 使用 `Color::Rgb(13, 127, 228)`。
5. 同一个图标模式、正常展示状态下，Driver、Explorer 连接节点与品牌型 Tab 的图形和颜色一致。
6. New Connection 的 busy 灰化和 Explorer 的 unavailable 提示仍属于状态反馈；验收品牌色时区分这些状态。
7. Explorer 子节点继续表达数据库、键等对象类型；品牌一致性针对连接节点。

### 2.2 Tab 数据库类型解析

在 `src/ui/mod.rs` 增加小型私有辅助函数：

```rust
fn tab_database_kind(app: &App, tab: &WorkspaceTab) -> Option<DatabaseKind> {
    let profile_id = match tab {
        WorkspaceTab::Sql(tab) => tab.execution_target.as_ref()?.profile_id,
        WorkspaceTab::Relation(tab) => tab.descriptor.key.profile_id,
        WorkspaceTab::Dashboard(tab) => tab
            .connection
            .map(|connection| connection.profile_id)
            .or(tab.profile_id)?,
        WorkspaceTab::History(_) => return None,
        WorkspaceTab::RedisBrowser(_) => return Some(DatabaseKind::Redis),
    };

    app.profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .map(|profile| profile.kind)
}
```

解析结果同时服务于品牌图标和品牌色：

| Tab | 图形 | 颜色 |
| --- | --- | --- |
| SQL / Dashboard，能解析 profile | `icons.database(kind)` | `icons.database_color(kind)` |
| SQL / Dashboard，未绑定或 profile 缺失 | 通用 Database 图标 | 当前 Tab 文字状态色 |
| Redis Browser | Redis 品牌图标 | Redis 品牌色 |
| Relation | `icons.catalog(tab.descriptor.kind)` | 所属连接品牌色；profile 缺失时状态色 |
| History | 当前通用 Table 图标 | 当前 Tab 文字状态色 |

目标 profile 存在但数据库选择失效时，品牌仍可由该 profile 确定，标题继续使用现有失效提示。此辅助函数不改变连接有效性判定。

## 三、实施任务

### Task 1：调整新建连接初始焦点

**Files:**
- Modify: `src/model/profile_manager.rs`，`ProfileManagerState::{new,start_new}`。
- Test: `tests/profile_draft.rs`，`manager_state_initializes_new_and_edit_forms`。
- Inspect: `tests/keymap.rs`、`tests/app_flow.rs` 中新建后直接导航的测试。

**Step 1：更新既有状态测试。**

在 `manager_state_initializes_new_and_edit_forms` 中断言构造及 `start_new()` 后 `selected_field == ProfileField::Kind`；编辑后显式断言 `DatabaseCategory`。增加一次 `start_new()` 重入检查，确认新建不会继承上一表单焦点。

**Step 2：运行目标测试，确认新建焦点断言失败。**

```bash
cargo test --test profile_draft manager_state_initializes_new_and_edit_forms
```

预期：实现修改前失败于 Category / Kind 差异。

**Step 3：修改初始化。**

仅将 `new()` 和 `start_new()` 中的赋值改为：

```rust
selected_field: ProfileField::Kind,
```

`start_new()` 对应语句为 `self.selected_field = ProfileField::Kind;`。

**Step 4：检查并更新已有导航预期。**

从新建初始状态调用 `move_field(-1)` 应到 Category，再向下返回 Kind。对依赖旧默认焦点的测试调整导航起点或显式聚焦，不改变已有字段顺序。

**Step 5：运行相关行为测试。**

```bash
cargo test --test profile_draft --test keymap --test app_flow
```

预期：焦点、驱动切换及打开表单路径通过；若出现不相关基线失败，记录具体测试名和原因。

### Task 2：更新统一品牌映射

**Files:**
- Modify: `src/ui/icons.rs`，`database()`、`database_color()`。
- Test: 同文件既有 `nerd_font_uses_database_brands_and_object_icons`。

**Step 1：更新既有图标契约断言。**

在现有品牌测试中加入 Redis 的 `dev::DEV_REDIS` 断言和 SQL Server `Color::Rgb(13, 127, 228)` 断言。沿用已有 Unicode / ASCII 测试结构核对 Redis 的 `RD`。

**Step 2：运行目标品牌测试，确认 Redis 断言失败。**

```bash
cargo test --lib nerd_font_uses_database_brands_and_object_icons
```

**Step 3：修改两处匹配分支。**

```rust
DatabaseKind::Redis => dev::DEV_REDIS,
```

```rust
DatabaseKind::SqlServer => ratatui::style::Color::Rgb(13, 127, 228),
```

**Step 4：运行图标单元测试。**

```bash
cargo test --lib ui::icons::tests
```

预期：所有图标模式及对象图标断言通过。

### Task 3：统一 Tab 品牌归属解析

**Files:**
- Modify: `src/ui/mod.rs`，新增 `tab_database_kind()`，调整 `render_tabs()` 图标选择。
- Test: `tests/ui_render.rs`，扩展 `sql_tab_icon_prefers_its_bound_profile_over_the_active_connection` 及相关 Tab 测试。

**Step 1：补充会揭示错误回退的行为用例。**

使用现有离线 fixture 和 profile URL 导入工具构建多连接状态：
- SQL Tab 绑定 PostgreSQL，但活动连接为 SQLite；品牌仍为 PostgreSQL。
- SQL Tab 未绑定，活动连接存在；显示通用数据库图标。
- SQL Tab 引用已删除 profile；显示通用图标，不能借用活动连接品牌。
- Dashboard 自身 profile 存在时由该 profile 决定图标；缺失时显示通用图标。

建议新增测试名称统一带 `tab_brand`，便于定向执行。Dashboard 的非 PostgreSQL 归属用离线构造状态验证 renderer；这不代表开放该驱动的 Dashboard 功能入口。

**Step 2：运行新增用例。**

```bash
cargo test --test ui_render tab_brand
```

预期：实现前未绑定 / 已删除目标回退用例失败。

**Step 3：实现第二节的辅助函数。**

在 `render_tabs()` 附近定义 `tab_database_kind()`；每个 Tab 生成展示信息时调用一次。SQL 和 Dashboard 用返回 kind 选择图标，删除图标逻辑中对 `app.active_profile()`、`app.connection.server` 的回退。Relation、History 的图形采用第二节契约。

**Step 4：复查 fixture 与已有品牌断言。**

尤其检查 `workspace_tabs_use_content_icons_instead_of_sequence_numbers`：若测试意图是绑定 SQLite，应显式建立该绑定；若意图是未绑定，应期待通用图标。不能为了保留旧字符串断言恢复错误回退。

**Step 5：运行解析相关测试。**

```bash
cargo test --test ui_render tab_brand
cargo test --test ui_render sql_tab_icon_prefers_its_bound_profile_over_the_active_connection
cargo test --test ui_render workspace_tabs_use_content_icons_instead_of_sequence_numbers
```

预期：图标归属只由 Tab 自身状态决定。

### Task 4：Tab 图标独立着色与宽度一致性

**Files:**
- Modify: `src/ui/mod.rs`，`RenderedTab`、`render_tabs()`；必要时增加局部 label 分段辅助函数。
- Test: `tests/ui_render.rs`，品牌色、截断和命中区域测试。

**Step 1：增加单元格级品牌色回归测试。**

通过 `render_buffer_with_icons()` 获取 buffer 和 `UiState`，使用 Tab 命中区域定位实际图标单元格：
- 激活 / 未激活 SQL Server 图标均为 RGB(13,127,228)。
- Redis Tab 图标为 Redis 品牌色。
- Relation 图形为表 / 视图，颜色来自所属连接。
- 图标背景与当前 Tab 背景一致，标题和关闭按钮继续使用状态样式。

断言必须定位到具体 Tab 的图标，不使用“整个 buffer 任意位置存在该颜色”作为成功条件。

**Step 2：运行新增 `tab_brand` 测试，确认当前整段单色渲染失败。**

```bash
cargo test --test ui_render tab_brand
```

**Step 3：拆分展示数据。**

将 `RenderedTab.label` 拆成 `icon: &'static str`、`icon_color: Option<Color>` 和 `title: String`，保留 index、id、marker、can_close、width。图标颜色取 `kind.map(|kind| icons.database_color(kind))`。

完整 label 的单元宽度仍等于 `" {icon} {title} "` 的宽度，Tab 总宽度再加 marker 宽度。使用现有 `cell_width()`，避免字节数或字符数计算布局。

**Step 4：按统一预算生成有样式的片段。**

1. 沿用当前 viewport 和关闭标记预留规则，得到 `max_label_width`。
2. 按原 label 顺序处理前导空格、图标、分隔空格、标题、尾随空格。
3. 每个片段只消耗剩余终端单元预算；图标作为整体放入，宽度不足时停止，不拆开 `RD` 或其他多单元标识。
4. 标题继续使用 `truncate_to_cell_width()`；停止后不继续输出后续 label 片段。
5. 普通片段使用现有 Tab style；图标有品牌色时使用 `style.fg(color)`，否则使用原 style。
6. 所有片段实际宽度相加得到唯一的 `label_width`。

**Step 5：统一命中区域依据。**

- `HitTarget::Tab` 的范围由实际 label 宽度决定。
- `HitTarget::CloseTab` 从 `x + label_width` 开始，采用实际可见 marker 宽度。
- 确保两者不重叠、均位于 tabs area 内；空宽度不创建虚假可点击区域。
- 按实际输出宽度推进渲染位置，保留完整逻辑宽度供 `tab_viewport()` 计算。
- 复用当前 viewport 算法，不引入第二套滚动状态。

**Step 6：运行品牌与布局回归。**

```bash
cargo test --test ui_render tab_
cargo test --test ui_render workspace_tabs
cargo test --lib tab_viewport
```

重点覆盖既有测试 `console_tab_hitbox_matches_the_rendered_truncated_unicode_label`、`workspace_tab_viewport_recalculates_after_resize`、`workspace_tabs_publish_close_targets_for_each_tab`。

### Task 5：跨界面一致性与边界验收

**Files:**
- Test: `tests/ui_render.rs`，既有 Driver / Explorer 测试与新的品牌一致性测试。
- Test: `tests/mouse.rs`，仅在现有命中区域测试缺少实际点击验证时补充。

**Step 1：强化现有 Explorer 测试。**

`explorer_redis_icon_uses_the_driver_brand_color` 目前只检查整个 buffer 存在 Redis 色。改为定位 cache 连接行，核对该行品牌图标单元格的 symbol 和 fg，避免其他区域的红色造成假通过。

**Step 2：扩展现有 Driver 渲染覆盖。**

在 `driver_options_use_database_icons_in_each_icon_mode` 中覆盖全部七种驱动（当前数组漏了 Oracle）。通过 `HitTarget::ProfileDriver(kind)` 的区域定位图标，检查正常状态的品牌色。

**Step 3：增加 Redis 跨入口对照。**

对 Nerd Font / Unicode / ASCII 三种模式分别渲染新建 Driver、Explorer Redis 连接节点、Redis Browser Tab，比较具体图标单元格；对多单元图标检查整个 symbol 范围。Redis 色同时与明确的 `Color::Rgb(220,45,45)` 契约相符。

**Step 4：覆盖极窄 Tab 与多字节标题。**

使用现有窄窗口 fixture / 私有布局测试覆盖剩余 label 预算为 0、1、2 以及刚好容纳图标的情况。标题包含中文、emoji、长连接名；检查实际显示宽度、活动 Tab 可见性、溢出箭头和关闭区域。

**Step 5：运行 UI 与鼠标测试。**

```bash
cargo test --test ui_render --test mouse
```

预期：全部离线渲染和交互测试通过；不需要数据库网络连接来验证品牌渲染。

### Task 6：最终检查与交付

**Step 1：执行格式检查和 CI 同口径静态检查。**

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
```

预期：退出码 0，无新 warning。

**Step 2：执行一次 CI 同口径完整 Rust 回归。**

```bash
cargo +1.94.0 test --all-targets --all-features
```

预期：通过；外部数据库依赖用例是否实际执行，按项目既有环境条件报告，不把跳过描述为已完成真实数据库验证。若有环境或既有失败，记录命令、测试名与原因。

**Step 3：终端手工核对。**

- 首次打开和取消后再次新建，默认均落在 Driver。
- Driver 上左右切换；向上进入 Category，选择 Non-relational 后显示 Redis。
- SQL Server 蓝色在 Driver、Explorer、激活与未激活 SQL Tab 一致。
- Redis 三处同一图形、同一品牌色。
- 多个不同驱动连接的 Tab 同时存在，切换 Explorer 选择不会改变其他 Tab 品牌。
- 未绑定 / 缺失 profile Tab 为通用图标。
- 调窄终端、滚动 Tab、点击关闭，命中位置和渲染一致。
- 可用主题下检查品牌图标可辨识；如存在选中背景对比问题，记录真实组合后再定向调整，不能通过改写品牌色绕过需求。

**Step 4：审查最终差异。**

```bash
git diff --check
git diff --stat
git status --short
```

验收报告列出实际修改文件、通过的检查、任何未完成的环境验收。

## 四、依赖顺序与提交边界

执行顺序：Task 1 → Task 2 → Task 3 → Task 4 → Task 5 → Task 6。

建议逻辑提交（实际提交应在执行任务得到提交授权时进行）：

1. `fix(profiles): focus driver when creating connections`：Task 1。
2. `fix(ui): unify database brand icons and tab colors`：Task 2–5；品牌归属和分段渲染作为完整行为提交。

## 五、完成标准

- [ ] 新建焦点为 Driver，Category 仍可按键和鼠标访问。
- [ ] SQL Server 品牌色唯一映射为 `#0D7FE4`。
- [ ] Redis Nerd Font 图标唯一映射为 `dev::DEV_REDIS`，文字模式为 `RD`。
- [ ] Driver、Explorer 连接节点、品牌型 Tab 的正常状态图标一致。
- [ ] Tab 激活 / 未激活只改变状态样式，不覆盖品牌前景色。
- [ ] 图标和颜色来自同一个 Tab 所属 kind，不回退到其他连接。
- [ ] Relation 保留对象图形并使用所属连接色；History 使用通用样式。
- [ ] 窄窗口、三种图标模式、Unicode 标题和关闭按钮命中区域通过回归。
- [ ] 格式、Clippy、测试结果和手工验收情况已记录。
