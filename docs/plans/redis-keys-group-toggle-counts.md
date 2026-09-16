# Redis Keys 分组快捷键与纯数字计数 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 本任务由自动工作流继续推进；若执行环境没有该技能，按本文逐项执行并记录结果。不要询问执行方式，不要启动子 Agent，只执行插件分派的当前阶段。

**Goal:** 恢复 Redis Keys 树中 `o` 对分组的展开/收起操作，并让分组数量以无括号数字显示。

**Architecture:** 在 Redis Keys 专用输入分支中优先处理普通 `o`，复用 `RedisToggleNode` 与 `toggle_prefix`，避免通用 Results 配置抢占。沿用现有搜索展开状态与 Value 显式打开机制；数量变更只涉及树行的计数 Span，帮助同步补充分组切换说明。

**Tech Stack:** Rust 2024（最低 Rust 1.94）、Crossterm 0.29、Ratatui 0.30.2；现有 Keymap → Action → App reducer 架构与 Cargo 集成测试。

---

## 0. 执行上下文与决策

- 工作区：`/Users/yelog/workspace/tui/lazydb`。
- 目标分支：`main`；起点：`56e1ded88a083b4193f1c330d4609c8a7154e65f`。
- plan 阶段复核：HEAD 仍与起点相同，当前分支 main；任务分支未提供，由后续工作流决定。
- 分析依据：同目录 `analysis.md`，已完整读取。
- 本计划按用户要求直接保存到任务目录 `plan.md`，不另建或覆盖 `docs/plans` 中的旧计划。
- 初始未跟踪文件：`docs/plans/2026-09-15-redis-key-tree-interaction-implementation.md`、`docs/plans/2026-09-15-sql-editor-target-completion-isolation.md`、`docs/plans/2026-09-15-workspace-autosave-notifications.md`。这些是已有工作，不能覆盖或纳入本次提交。
- plan 阶段只产出计划与回执。本文的代码、测试和命令均供实施阶段执行，尚未应用或运行。
- 默认 features 包含 `driver-oracle`，验证使用默认 features；如环境阻塞，记录实际错误和未覆盖范围。

### 根因摘要

默认配置 `config/default.toml:176` 将 Results 的 `toggle-view` 绑定到 `o`。Redis Keys 在 `src/input/keymap.rs:1432` 先调用 `map_configured_navigation`，该函数把非 Relation 的 Results 焦点也当作通用 Results 上下文，先返回 `ToggleResultView`。该动作在 Redis tab 无 SQL console，直接无效返回，后面的 Redis `o → RedisPrimarySelection` 分支不可达。计数括号由 `src/ui/redis_browser.rs:599` 的 `format!(" ({})", row.total_keys)` 显式产生。

### 选定方案

采用分析中的方案 A：搜索处理之后、通用配置导航之前，普通 o 仅对选中的 Prefix 返回 RedisToggleNode；叶子和无选择时返回 None。删除原来末尾的 o → RedisPrimarySelection 回退。这样既解除默认配置冲突，也不会意外恢复叶子 o 打开功能。数量模板改为 `format!(" {}", row.total_keys)`。

### 行为契约

| 上下文 | 普通 o 行为 |
| --- | --- |
| Redis Keys，折叠分组 | 展开当前分组 |
| Redis Keys，展开分组 | 收起当前分组 |
| Redis Keys，叶子 / 无选择 | 无操作；不落入 SQL Results 动作 |
| Redis Keys，搜索编辑态 | 输入字符 o |
| Redis Keys，搜索确认态选中分组 | 切换搜索投影中的分组展开状态 |
| SQL Results | 保留切换 Data / Output |
| Redis Preview / Explorer | 沿用各自原有输入逻辑，不进入 Keys 专用分支 |

数量保留名称后一格空白、muted 颜色、普通字重和当前背景；叶子不显示数量。Enter 的主操作、鼠标行为和既有 hjkl/方向导航沿用现有路径。

## Task 1：建立捕获默认配置抢占的回归用例

**Files:**
- Modify/Test: `tests/keymap.rs`（现有 Redis 用例在文件开头附近）。
- Read: `src/input/keymap.rs:1345-1507`、`src/app.rs:13377-13384`、`src/model/redis_browser.rs::toggle_prefix`。

**Step 1 — 准备测试数据。**

使用现有测试中的 `key()` helper、`App::new(Vec::new())` 和默认 Keymap。创建 Redis tab，插入 `users:1` 与 `users:2`，选中 `Prefix(b"users:")`，设置 `Focus::Results` 与 `RedisBrowserFocus::Keys`。不连接真实 Redis，不使用 sleep。

**Step 2 — 添加真实按键到 reducer 的回归测试。**

以下用例可直接追加到 `tests/keymap.rs`，已有顶层 imports 提供 App、WorkspaceTab、Focus、Keymap、KeyCode、Action 和 Uuid：

```rust
#[test]
fn redis_keys_o_toggles_groups_before_results_bindings() {
    use lazydb::db::redis::types::{RedisKeyId, RedisTarget};
    use lazydb::model::redis_browser::RedisBrowserTab;
    use lazydb::model::redis_key_tree::KeyTreeNodeId;

    let mut app = App::new(Vec::new());
    let target = RedisTarget {
        profile_id: Uuid::from_u128(101),
        database: 0,
    };
    let mut tab = RedisBrowserTab::new(Uuid::from_u128(102), target.clone());
    let prefix = KeyTreeNodeId::Prefix(b"users:".to_vec());
    tab.tree.rebuild(&[
        RedisKeyId { target: target.clone(), key: b"users:1".to_vec() },
        RedisKeyId { target, key: b"users:2".to_vec() },
    ]);
    tab.select(Some(prefix.clone()));
    let tab_id = tab.id;
    app.tabs.push(WorkspaceTab::RedisBrowser(tab));
    app.active_tab = app.tabs.len() - 1;
    app.focus = Focus::Results;
    let mut keymap = Keymap::default();

    for expanded in [true, false] {
        let action = keymap.map(key(KeyCode::Char('o')), &app).unwrap();
        assert_eq!(action, Action::RedisToggleNode { tab_id, node: prefix.clone() });
        assert!(app.update(action).is_empty());
        let WorkspaceTab::RedisBrowser(tab) = &app.tabs[app.active_tab] else {
            panic!("expected Redis browser tab");
        };
        assert_eq!(tab.tree.expanded.contains(&prefix), expanded);
        assert_eq!(tab.tree.selected.as_ref(), Some(&prefix));
        assert!(tab.opened_key.is_none());
        assert_eq!(tab.preview_generation, 0);
    }
}
```

**Step 3 — 运行回归，确认捕获旧问题。**

```sh
cargo test --test keymap redis_keys_o_toggles_groups_before_results_bindings -- --exact
```

预期运行 1 个测试，旧实现动作断言失败：实际是 ToggleResultView，期望 RedisToggleNode。若编译或依赖先失败，先记录环境/测试构造问题，不能当作缺陷已成功复现。

**Step 4 — 复核测试有效性。**

确认使用 `Keymap::default()`，没有移除 results-toggle-view 默认绑定，没有只调用 reducer 或模型来绕过冲突。确认两次按键都重新经过 map，且不是两次直接调用同一模型方法。

## Task 2：实现 Redis Keys 的 o 专用路由

**Files:**
- Modify: `src/input/keymap.rs`，Redis Keys 分支（起点约 1345-1507 行）。
- Test: `tests/keymap.rs`。

**Step 1 — 插入最小路由。**

在现有搜索编辑/确认处理之后、`map_configured_navigation(event, app, &self.bindings)` 之前加入以下逻辑，仍处于外层 `Focus::Results + RedisBrowserFocus::Keys` 条件内：

```rust
if event.modifiers.is_empty() && event.code == KeyCode::Char('o') {
    return match app.tabs.get(app.active_tab) {
        Some(crate::model::tab::WorkspaceTab::RedisBrowser(tab)) => {
            match tab.tree.selected.as_ref() {
                Some(node @ crate::model::redis_key_tree::KeyTreeNodeId::Prefix(_)) => {
                    Some(Action::RedisToggleNode {
                        tab_id: tab.id,
                        node: node.clone(),
                    })
                }
                _ => None,
            }
        }
        _ => None,
    };
}
```

**Step 2 — 移除冲突回退。**

删除末尾 `(RedisBrowserFocus::Keys, KeyCode::Char('o')) => Some(Action::RedisPrimarySelection)` 分支。Enter 仍映射 RedisPrimarySelection。普通 o 的叶子/无选择路径必须直接返回 None，不能继续流入 Results 配置匹配。带修饰键的 o 不应通过已删除的无修饰检查缺失的回退触发打开或切换。

**Step 3 — 运行 Task 1 的精确测试。**

```sh
cargo test --test keymap redis_keys_o_toggles_groups_before_results_bindings -- --exact
```

预期 1 个测试通过，展开/收起均经过既有 RedisToggleNode reducer。

**Step 4 — 复核改动面。**

确认没有修改全局 results-toggle-view 默认配置，没有改动 map_configured_navigation 的其它上下文策略，没有新增 Action 或绕过 toggle_prefix 直接操作 expanded 集合。新分支位于搜索编辑态返回逻辑之后。

## Task 3：补齐输入边界与搜索状态回归

**Files:**
- Modify/Test: `tests/keymap.rs`。
- Read/Run existing tests: `tests/redis_browser_tabs.rs`、`tests/redis_key_filter.rs`。

每个用例可复用 Task 1 的数据构造；若需要抽取 fixture，仅在本测试文件内返回 App，避免引入全局测试基础设施。

**Step 1 — 叶子与空选择。**

展开 users 分组后选中 `Key(b"users:1")`；普通 o 应映射 None，opened_key 与 preview_generation 不变。再用 `tab.select(None)` 清空选择，断言仍为 None。不能只断言“没有命令”，因为旧 ToggleResultView 本身也返回空命令。

**Step 2 — 搜索编辑态与确认态。**

保持分组选中，调用 `open_find()` 后断言 o 映射 `RedisFindInsert('o')`，此断言无需执行插入以免改变后续 fixture。确认空查询后仍选中 users 分组，按两次 o：每次均返回 RedisToggleNode；`find.expanded` 先插入再移除该 Prefix；`visible_rows()` 中子节点随之出现/隐藏；原始 `tree.expanded` 不因搜索投影切换改变。

**Step 3 — 保留已打开 Value。**

使用 `tests/redis_browser_tabs.rs` 的现有 open_key 初始化方式，在 fixture 上打开一个有效 Key，保存 opened_key、preview_generation、preview 状态和滚动位置，随后选中父分组。通过 map → update 按两次 o，断言这些 Value 字段保持一致且动作结果没有查询命令。这验证新增入口是纯树操作，而非叶子主操作别名。

**Step 4 — 上下文隔离与修饰键。**

- 默认 SQL App 的 Results 焦点：o 仍返回 ToggleResultView。
- Redis Preview 焦点：o 不返回 RedisToggleNode；只检查本次操作不泄漏，不为 Preview 定义新的 o 语义。
- 默认绑定下 Ctrl+o、Alt+o：不得返回 RedisToggleNode 或 RedisPrimarySelection。
- 保留现有 Enter、h/l、j/k 用例，避免重复构造等价测试。

**Step 5 — 运行定向验证。**

```sh
cargo test --test keymap --test redis_browser_tabs --test redis_key_filter
```

预期三套测试均通过，新用例实际执行。若搜索 fixture 不保留分组行，先修正 fixture（空查询、合法已选 Prefix），不要改搜索模型来迁就本次快捷键测试。

## Task 4：调整计数展示并同步帮助

**Files:**
- Modify: `src/ui/redis_browser.rs::render_row`（起点约 597-602 行）。
- Modify: `src/help.rs`（HelpShortcutId 枚举、排序分组、Redis 帮助行）。
- Run existing tests: `tests/redis_help.rs`、`src/help.rs` 内的单元测试。

**Step 1 — 修改计数模板。**

将 `format!(" ({})", row.total_keys)` 改为：

```rust
format!(" {}", row.total_keys)
```

保持 `if row.expandable` 和 `Style::new().fg(theme.muted).bg(background)` 原样。不要调整 total_keys 计算或把括号从 Key 名称中删除。

**Step 2 — 添加独立的分组快捷键帮助。**

在 `HelpShortcutId` 的 RedisKeysExpand / RedisKeysCollapse 附近新增 `RedisKeysToggle`。将它加入现有 RedisKeysOpen / RedisKeysExpand / RedisKeysCollapse 的排序分组（起点 694-700 行，返回 2）。在 Redis 帮助行中添加：

```rust
row!(
    RedisKeysToggle,
    [RedisKeys, RedisKeysFindConfirmed],
    "o",
    "toggle the selected Redis key group",
    display
),
```

现有 RedisKeysOpen 行继续使用 `Enter`，描述改为 `"open the selected Redis key or toggle its group"`。遵循现有 display-only 帮助行设计，不把该行注册为通用 Results 配置命令。编译检查如指出枚举穷尽匹配遗漏，在对应 Redis 同类分支补齐。

**Step 3 — 验证帮助与检查渲染差异。**

```sh
cargo test --test redis_help
cargo test --lib help::tests
git diff -- src/ui/redis_browser.rs src/help.rs
```

预期帮助测试实际运行并通过；计数部分 diff 只有格式模板去掉括号，独立样式保留。`tests/redis_help.rs` 主要验证帮助入口，不能将其通过表述为已验证每条帮助文字；还需检查实际帮助表的两个上下文均含 o 行且搜索编辑态不展示此操作。

**Step 4 — 视觉复核。**

在可用 Redis 测试连接或已有渲染测试 fixture 中，查看已选/未选、折叠/展开分组：名称后呈现 `users 2`，计数 muted 且不加粗，选中背景连贯，叶子不追加数字。包含括号的 Key 名称仍按原名显示。单纯格式变化不新增字符串镜像测试或测试框架；没有可用真实连接时记录人工视觉验收未执行，并以代码差异复核明确说明覆盖边界。

## Task 5：最终复核与交付

**Files:** 本次实际修改的 `src/input/keymap.rs`、`src/ui/redis_browser.rs`、`src/help.rs`、`tests/keymap.rs`。

**Step 1 — 工作区与差异检查。**

```sh
git status --short
git diff --check
git diff --stat
git diff -- src/input/keymap.rs src/ui/redis_browser.rs src/help.rs tests/keymap.rs
```

确认已有三个未跟踪计划文件未被改动；本任务仅围绕两个需求及必要帮助/回归验证。行号以符号定位为准。

**Step 2 — 执行最终检查。**

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

若格式检查失败，运行 `cargo fmt` 后复核只包含本次相关格式变更，再执行 fmt 检查。若修正了代码，再重新执行受影响的定向测试。Task 3/4 已通过且没有后续影响测试的改动时，无需机械重复全部测试；保留各阶段命令、退出状态和实际测试数。

**Step 3 — 验收矩阵逐项确认。**

- [ ] 默认配置下分组 o 连按两次分别展开/收起，选中节点不变。
- [ ] 叶子和空选择 o 无操作，不触发 SQL 动作或 Value 打开。
- [ ] 搜索编辑态 o 输入字符，确认态 o 正确切换搜索展开集合。
- [ ] 展开/收起保留已打开 Value 与请求 generation，操作不生成查询命令。
- [ ] SQL Results 的 o、Redis Enter 与现有方向导航继续通过回归。
- [ ] 分组计数只显示数字，保留空格与弱化样式；叶子不显示计数。
- [ ] 帮助准确区分 o 切换分组与 Enter 主操作。
- [ ] 定向测试、fmt、clippy 和 diff 检查通过，或如实记录具体阻塞与覆盖缺口。

**Step 4 — 交付记录与阶段边界。**

向后续工作流报告实际修改文件、测试结果、视觉复核情况及计划偏差。是否提交及使用哪个任务分支遵循插件后续阶段指令；不因本计划提前执行 git commit。本修复适合作为单个逻辑提交，建议消息 `fix(redis): restore group toggle shortcut and simplify counts`。只在获得提交阶段指令后暂存上述明确路径，避免把用户未跟踪文档一并纳入。

## 完成标准与计划阶段状态

实施完成标准是 Task 5 的验收矩阵全部满足，或明确记录确实存在的验证环境阻塞；不能将测试编译失败或 0 tests 算作验证通过。

本 plan 阶段已完成分析读取、writing-plans 技能调用、文件/步骤/复核/命令/验收标准规划。没有实施业务代码，没有新增测试文件或执行 Cargo 检查，也没有启动子 Agent。完成回执由本阶段在计划保存与复核之后最后写入，下一阶段由自动工作流安排。
