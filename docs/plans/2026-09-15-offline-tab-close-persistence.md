# 离线关闭 Tab 持久化修复实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 未连接数据库时关闭任意已恢复的 Tab，保存并重启后不再恢复该 Tab；目标连接缺失时使用英文提示。

**Architecture:** 共享 `App.tabs` 表示当前打开的视图，`App.workspaces` 保存各连接的兼容缓存。以统一关闭入口 `close_tab()` 为一致性边界，按 Tab UUID 同步删除所有缓存中的视图引用并修复活动 Tab 指针，再交由既有持久化机制保存。Console 文档独立于 Tab 生命周期，关闭视图后继续保留 SQL 内容。

**Tech Stack:** Rust、Cargo、现有 App action/command 状态流、WorkspaceStore、TOML 工作区文件、tempfile 集成测试。

---

## 当前进度

- 已完成：`src/app.rs` 中关闭 Tab 时同步清理缓存。
- 已完成：`src/ui/mod.rs` 中相关 Tab 标题提示改为英文。
- 已完成：`tests/workspace_tabs.rs` 中新增离线关闭后的磁盘往返测试。
- 已完成：workspace_tabs 与 workspace_persistence 共 64 项测试通过；格式与 diff 检查通过。
- 待补充：同一进程内激活连接的回归验证、英文提示的终端人工验收。
- 本计划中的提交步骤属于交付建议，尚未执行。

## 问题依据

1. `restore_workspace()` 把恢复的 workspace 保留在 `self.workspaces`，同时通过 `append_workspace()` 合并到共享 Tab 列表。
2. `workspace_snapshot()` 对活动 workspace 使用共享列表，对非活动 workspace 使用缓存。
3. 原 `close_tab()` 仅删除共享列表中的 Tab；Console 的缓存记录虽更新为 `open = false`，缓存 Tab 引用仍存在。
4. `restore_profile_workspace()` 会根据 Console Tab 引用重新设置 `open = true`，导致已关闭 Console 重新打开；其他 Tab 直接从旧引用恢复。
5. Relation 标题查不到 `descriptor.key.profile_id` 对应的连接配置时显示硬编码“失效目标”。这不是数据库连接状态检查，也不是表存在性检查。

## Task 1：建立离线关闭的回归用例（已完成）

**Files:**
- Modify/Test: `tests/workspace_tabs.rs`
- Reference: `src/persistence/workspace.rs` 的 `WorkspaceStore`、`WorkspaceSnapshot`、`PersistedProfileWorkspace`

**Step 1：构造两个连接的持久化快照。**

第一个连接作为启动选择，第二个作为非活动 workspace。第二个 workspace 包含 Console、Relation、Dashboard、Redis Browser，缓存活动 Tab 指向 Relation；Console SQL 使用 `select 42`。分别覆盖第二个连接配置存在和缺失的情况。配置缺失时 Redis Browser 会被现有恢复逻辑忽略，因此该分支验证其余三种 Tab。

**Step 2：恢复 App 并检查断开状态。**

调用 `restore_workspace(snapshot, Some(first.id))`，断言 `ConnectionStatus::Disconnected`。测试过程通过 `Action::CloseTab(id)` 操作，而不是直接修改 Tab 容器。

**Step 3：逐个关闭并执行真实保存/恢复。**

先关闭 Relation，再关闭 Console、Dashboard、Redis Browser；每次从返回命令中取出 `Command::PersistWorkspace`，使用临时目录中的 `WorkspaceStore` 保存、读取，在新 App 中恢复。

每一步断言：
- 返回持久化命令。
- 保存成功，活动 Tab 引用符合快照校验。
- 已关闭 UUID 不再出现。
- 恢复后的 Tab 数量等于关闭后的数量。

全部关闭后断言列表为空，Console 文档仍存在、`open == false`、SQL 仍为 `select 42`。

**Step 4：运行定向测试。**

```bash
cargo test --test workspace_tabs closing_restored_tabs_without_connecting_survives_disk_round_trip -- --exact
```

预期：修复前旧缓存会导致 Tab 恢复；当前修复版本测试通过。当前工作区已经包含修复，不应为了重演失败覆盖现有代码。

## Task 2：统一关闭路径中的缓存一致性（已完成）

**Files:**
- Modify: `src/app.rs` 的 `close_tab()`
- Reference: 同文件的 `request_close_tab()`、`sync_cached_console_state()`、`workspace_snapshot()`、`activate_profile_workspace()`

**Step 1：确认一致性边界。**

在完成现有请求取消处理并执行 `self.tabs.remove(index)` 后，清除所有 workspace 的同 UUID 引用。按 UUID 清理所有投影，避免依赖连接是否可用或当前活动连接是否等于 Tab 所属连接。

**Step 2：实施核心修改。**

```rust
for workspace in self.workspaces.values_mut() {
    workspace.tabs.retain(|tab| tab.id() != id);
    if workspace.active_tab_id == Some(id) {
        workspace.active_tab_id = workspace.tabs.first().map(WorkspaceTab::id);
    }
}
```

处理顺序：共享列表删除 → 缓存引用删除及指针修复 → Console 文档关闭状态同步 → 共享活动位置与焦点修复 → 生成持久化命令。

**Step 3：检查数据语义。**

- Console：保留文档记录、编辑器内容及 SQL 文件，只关闭视图。
- Relation/Dashboard/Redis Browser：从打开列表及缓存删除。
- 缓存活动 Tab 被关闭：选择剩余第一个 Tab；为空时使用 `None`。
- 所属 profile 缺失：仍按 UUID 正常关闭。
- 缓存存在重复投影：全部清理。

**Step 4：运行相关回归测试。**

```bash
cargo test --test workspace_tabs --test workspace_persistence
```

预期：全部通过；当前已验证 64 项通过。

## Task 3：统一英文 Tab 状态提示（已完成，待人工验收）

**Files:**
- Modify: `src/ui/mod.rs` 的 `render_tabs()`

**Step 1：替换状态文案。**

| 场景 | 文案 |
| --- | --- |
| Console 没有执行目标、Dashboard 没有可解析绑定 | `Unbound` |
| Console/Relation/Redis Browser 引用的 profile UUID 不存在 | `Invalid target` |
| Console profile 存在，但 `target.is_valid(profile)` 为 false | `Invalid:连接名` |

**Step 2：检查判断条件。**

保留原目标解析规则，只替换相关 Tab 标题文案。有效 profile 尚未连接时仍显示连接名称。对于截图中的 Relation，只有 profile 查找失败才显示 `Invalid target`。

**Step 3：人工检查显示。**

在临时测试配置中恢复缺失目标的 Relation Tab，确认显示 `@Invalid target`；关闭该 Tab 后正常退出，再启动确认不再出现。检查窄窗口下标题截断和 Tab 切换仍正常。

## Task 4：补充同一进程内连接激活验证（待完成）

**Files:**
- Test: `tests/workspace_tabs.rs`
- Reference: `src/app.rs` 的 `activate_profile_workspace()`、`install_workspace()`、`append_workspace()`

**Step 1：复用离线恢复场景。**

选择两个有效 profile，恢复后关闭第二个 workspace 的 Relation，并关闭其 Console 视图，保留 Console SQL。

**Step 2：在同一 App 实例中激活第二个连接。**

沿用项目现有连接安装测试的 action/event 流；不重启 App，以确保测试确实经过运行时缓存安装路径。数据库连接事件可由现有测试辅助代码模拟。

**Step 3：断言缓存不会复活关闭的视图。**

- Relation 与 Console 的原 UUID 不出现在打开 Tab 列表。
- Console 文档仍存在，SQL 未改变。
- 从 Console 管理器显式重新打开该文档后，仅出现一个同 UUID Tab。

若连接初始化按既有策略创建了新默认 Console，不将其误判为旧 Tab 复活；以关闭的 UUID 为核心断言。

**Step 4：覆盖统一入口的批量关闭。**

检查关闭其他 Tab 的现有路径是否逐个调用 `close_tab()`；使用已恢复的非活动 workspace 验证批量关闭后保存/恢复仅保留预期 Tab。如果发现绕过入口的路径，将删除操作接入相同的一致性处理。

## Task 5：最终检查与交付（部分完成）

**Files:**
- Review: `src/app.rs`
- Review: `src/ui/mod.rs`
- Review: `tests/workspace_tabs.rs`
- Documentation: 本文件

**Step 1：执行相关测试。**

补充测试后运行：

```bash
cargo test --test workspace_tabs --test workspace_persistence
```

预期：所有测试通过。仅在新修改或失败引出其他影响时扩大测试范围。

**Step 2：格式与差异检查。**

```bash
cargo fmt --all -- --check
git diff --check
git diff --stat
```

预期：无格式或空白错误，代码变化符合计划范围。

**Step 3：人工验收。**

使用独立测试配置，启动 `cargo run`：

1. 不连接数据库，关闭多个不同连接的恢复 Tab，正常退出并重启，关闭的 UUID 不恢复。
2. 关闭全部 Tab 后退出并重启，恢复列表为空。
3. 对缺失 profile 的 Tab 验证英文提示与关闭持久化。
4. 不重启而激活原所属连接，关闭的 UUID 不重新出现。
5. 从 Console 管理器显式重新打开已关闭 Console，SQL 保留。

**Step 4：提交交付（获得提交指令后执行）。**

建议提交信息：`fix(workspace): persist offline tab closures across cached workspaces`。

交付说明包含根因、英文提示语义、测试结果与人工验收结果。明确区分自动测试完成项和人工未执行项。

## 最终验收标准

- 已恢复的 Tab 不需要启动所属数据库连接即可关闭并持久化。
- 当前打开列表、缓存引用与保存文件在关闭操作后保持一致。
- 正常退出并重启后不恢复已关闭 UUID，包括最后一个 Tab。
- 缺失 profile 的 Tab 能关闭，相关提示为英文。
- 关闭 Console 不删除 SQL 文档，显式重新打开可以恢复内容。
- 既有 workspace 持久化与 Tab 生命周期测试继续通过。

## 范围说明

本次针对关闭操作及相关状态文案。跨 profile 的全局 Tab 排序和全局活动 Tab 恢复策略是独立持久化议题：当前恢复从 workspace 映射合并 Tab，不能把上述测试解读为已保证完整全局顺序。若“上次退出时的 Tab 列表”还要求精确保留跨连接交错排序，应另行设计显式有序的全局 Tab UUID 列表及迁移规则。
