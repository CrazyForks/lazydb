# Redis Explorer DB Presentation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 区分 Redis 连接与逻辑数据库的图标，仅在连接行显示连接状态，并将 Redis 浏览器 Tab 标题显示为 `dbN@连接名`。

**Architecture:** 在 `ExplorerState::visible_rows` 修正 Redis DB 的展示语义，复用已有数据库图标、颜色和公共行渲染。在 `render_tabs` 根据每个 Redis Tab 自身的 `RedisTarget` 生成标题，继续复用连接名称解析、终端文本清理及宽度截断。Tab 身份仍由现有 ID 和 target 管理。

**Tech Stack:** Rust 2024、Ratatui 0.30、nerd-font-symbols、现有集成测试与 TestBackend。

---

## 方案与边界

- 连接行：Redis 品牌图标、连接名称、GLOBAL/PROJECT/SESSION、连接状态、endpoint。
- DB 行：通用数据库图标、`DB N`、已有 key 数量；不显示连接状态及其前置空白。
- NerdFont 使用现成的 `md::MD_DATABASE`，Unicode 使用 `◆`，ASCII 使用 `DB`。颜色采用已有 `kind_color(CatalogKind::Database, theme)` 即 `theme.action`。
- Tab：保留 Redis 品牌图标，正文严格使用 `db0@lssc-uat-redis`、`db1@lssc-uat-redis`，`@` 两侧无空格。左侧 DB 编号优先保留，长标题使用已有截断机制。
- 数量 `Some(0)` 仍显示 `0`，`None` 不显示数量；不将未知数量解释为空库。
- DB 列表继续使用发现结果，不能硬编码 16 个 DB。
- `WorkspaceTab::title()` 的 `Redis` 可继续作为通用类型标题；本需求针对带连接上下文的 Tab 栏标题，在现有组合层处理，避免为动态字符串修改公共借用接口或新增标题缓存。
- DB 的断连/重连状态只由父连接行表达；Keys/Preview 本身的加载与错误反馈照常工作。

## 已确认的代码定位

1. `src/model/workspace.rs:1183-1201`：RedisDatabase 分支当前设置 `kind=None`、`profile_kind=Some(DatabaseKind::Redis)`、`connection_status=父连接状态`。
2. `src/ui/mod.rs:2569-2712`：`explorer_list_item` 优先使用 profile 品牌图标，并无条件渲染非空 connection_status。
3. `src/ui/icons.rs:297-353`：已具备三种模式的数据库图标，无需新增图标接口。
4. `src/ui/mod.rs:2123-2194`：`render_tabs` 已通过 `redis.target.profile_id` 解析连接名，但统一组合 `tab.title()` 导致所有 Redis DB 标题相同。
5. `tests/ui_render.rs:7523`：已有 `redis_browser_tab_title_uses_its_connection_name`，当前断言 `Redis @cache`，实施时更新它。
6. `tests/redis_browser_tabs.rs`：已有打开数据库、重复打开及持久化相关回归基础。

## Task 1：修正 Redis DB 展示投影

**Modify:** `src/model/workspace.rs`，`ExplorerState::visible_rows` 的 RedisDatabase 分支。

1. 确认实施时工作区差异；本计划以外已有计划文档应保留。
2. 将该分支的完整返回元组改为：

```rust
ExplorerNodeId::RedisDatabase { database, .. } => (
    format!("DB {database}"),
    profile.and_then(|profile| {
        profile
            .redis_databases
            .iter()
            .find(|item| item.database == *database)
            .and_then(|item| item.keys.map(|keys| keys.to_string()))
    }),
    None,
    Some(CatalogKind::Database),
    None,
    None,
    None,
    None,
    None,
    false,
    None,
),
```

3. 确认 `CatalogKind` 使用当前文件已有导入形式。该调整会自然进入通用数据库图标路径，状态为 None 后也不会生成多余空白。
4. 检查普通 Explorer 与搜索结果的公共展示投影：DB 图标均应一致，节点 ID、depth、expandable、metadata 不变。

## Task 2：生成带 DB 编号的 Tab 栏标题

**Modify:** `src/ui/mod.rs`，`render_tabs` 非 Console 分支。

1. 保留现有 `connection_name` 解析（Redis 使用自身 target.profile_id；缺失 profile 使用“失效目标”）。
2. 将非 Console 分支末尾的 `format!("{} @{connection_name}", tab.title())` 替换为：

```rust
match tab {
    WorkspaceTab::RedisBrowser(redis) => {
        format!("db{}@{connection_name}", redis.target.database)
    }
    _ => format!("{} @{connection_name}", tab.title()),
}
```

3. 继续让结果进入原有 `sanitize_terminal_text`、48 字符限制、cell width 截断和 Tab hit region 计算。
4. 不从全局连接、Explorer 当前选择或 profile 默认 database 推导编号；Tab 的 target 是唯一来源。

## Task 3：更新现有验证并执行检查

**Modify:** `tests/ui_render.rs`，现有 `redis_browser_tab_title_uses_its_connection_name`。

1. 读取现有测试 fixture 的数据库编号，将期待值更新为该编号的 `dbN@cache`；旧 `Redis @cache` 不应出现。此处是更新已有行为断言，不为简单字符串映射单独增加单元测试。
2. 格式检查：`cargo fmt --all -- --check`，预期通过。
3. 运行相关现有集成测试：

```bash
cargo test --test redis_explorer --test redis_browser_tabs --test workspace_tabs
cargo test --test ui_render
```

预期全部通过；UI 测试覆盖其他 driver 图标、Tab 栏布局等共享区域。失败时先检查是否是本次语义变化导致的旧断言，再修复，不盲目更新快照。

4. 若后续为修复失败修改代码，重新运行对应检查；通过后进入手工验收。

## Task 4：终端验收

启动：`cargo run --`，使用现有可用 Redis 配置。

| 场景 | 预期 |
| --- | --- |
| 展开 Redis 连接 | 父连接保留 Redis 品牌图标及状态；所有子 DB 显示通用数据库图标，无状态点 |
| key 数量已知、0、未知 | 分别显示原数量、0、不显示；数量前无状态点遗留空白 |
| 打开同一连接 DB 0、1、10 | 分别显示 db0@连接名、db1@连接名、db10@连接名 |
| 切换 Explorer 选中项 | 已打开 Tab 的编号仍与自身 Keys 面板 DB 编号对应 |
| 不同连接打开相同 DB | 编号相同但连接名不同；标题不受当前活动连接影响 |
| 重复打开同一 DB | 沿用既有 Tab 复用规则，标题稳定 |
| 保存并恢复工作区 | 从恢复后的 target 重新生成正确标题，无新增持久化字段 |
| 断开、重连或连接失败 | 父连接展示相应状态，子 DB 不重复显示连接状态；面板错误反馈正常 |
| 长连接名、窄终端 | 左侧 DB 编号可辨认；截断、切换、关闭按钮和鼠标点击区域正常 |
| NerdFont、Unicode、ASCII | 各模式使用已有 DB 图标映射，父子图标可区分 |
| PostgreSQL/MySQL 等连接 | 图标、状态、既有 Tab 命名正常 |

## 完成标准

三项显示需求全部满足；相关现有测试与格式检查通过；手工场景记录实际验证结果。预期生产代码改动仅涉及 `src/model/workspace.rs` 和 `src/ui/mod.rs`，另更新现有 UI 断言。
