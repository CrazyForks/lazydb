# Help SQL History Crash Fix Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 修复帮助面板中选择 SQL History 并按回车导致的 panic，并用真实按键入口的回归测试锁定行为。

**Architecture:** 在 `App::execute_help_shortcut()` 中补齐 `HelpShortcutId::OpenSqlHistory` 到 `Action::OpenSqlHistory` 的映射，复用现有历史面板初始化和加载命令。通过帮助筛选、Enter 键映射和 reducer 更新验证完整入口，确保加载命令与新面板的身份一致。

**Tech Stack:** Rust、Cargo、crossterm、现有 App reducer / Keymap、Rust integration tests。

---

## 背景与定位

- `src/help.rs:1124`：SQL History 的 `row!` 注册默认 `executable: true`。
- `src/input/keymap.rs:221`：帮助面板的 Enter 转为 `Action::ExecuteHelpShortcut`。
- `src/app.rs:2315`：`execute_help_shortcut()` 先检查选中项和可用性，再分发。
- `src/commands.rs:220`：`command_for_help()` 没有 SQL History 映射。
- `src/app.rs:2337–2573`：本地 Action 分发也遗漏此项，最终进入 `unreachable!("display-only shortcut passed execution guard")`。
- `src/app.rs:4614`：已有 `Action::OpenSqlHistory` 负责打开面板、初始化查询并返回 `Command::LoadSqlHistory`。
- `tests/sql_history_interaction.rs:66`：现有测试只覆盖直接调用 Action，没有经过帮助入口。

行号是编写计划时的参考；实施时使用符号名定位。

## 验收标准

1. 帮助中选中 SQL History 后按 Enter，打开 `Overlay::SqlHistory`，无 panic。
2. 返回且仅返回一个 `Command::LoadSqlHistory`，其 `overlay_id`、`generation` 与面板状态一致。
3. 面板处于 Browse 模式及加载状态，首次加载从首页开始。
4. 活动标签和标签数量保持正确，焦点落在 Results。
5. F7、历史复制/详情/关闭、其他帮助操作的现有相关测试通过。

### Task 1：添加能够复现崩溃的入口回归测试

**Files:**
- Modify: `tests/keymap.rs`，放在 `filtered_help_moves_to_non_first_id_and_executes_it` 附近。
- Reference: `src/input/keymap.rs`、`src/model/sql_history_view.rs`。

**Step 1：检查工作区基线（2 分钟）**

```bash
git status --short
```

记录已有改动。本工作区已有其他实施计划文档，保留它们；后续暂存使用明确路径。

**Step 2：添加回归测试（5 分钟）**

复用该文件现有的 `key()`、`App`、`Action`、`Keymap`、`Focus`、`Overlay` 等导入。新增代码：

```rust
#[test]
fn help_sql_history_enter_opens_history_and_loads_first_page() {
    use lazydb::{
        action::Command,
        help::HelpShortcutId,
        model::sql_history_view::SqlHistoryMode,
    };

    for focus in [Focus::Explorer, Focus::Editor, Focus::Results] {
        let mut app = App::new(Vec::new());
        app.update(Action::EditorKey(key(KeyCode::Esc)));
        app.focus = focus;
        let tab_count = app.tabs.len();
        let active_tab = app.active_tab;

        app.update(Action::ShowHelp);
        app.update(Action::HelpPaste("SQL execution history".into()));
        assert_eq!(
            app.help_selected_id(),
            Some(HelpShortcutId::OpenSqlHistory),
            "history should be selected from {focus:?}"
        );

        let mut keymap = Keymap::default();
        let action = keymap.map(key(KeyCode::Enter), &app);
        assert_eq!(
            action,
            Some(Action::ExecuteHelpShortcut(HelpShortcutId::OpenSqlHistory))
        );
        let commands = app.update(action.unwrap());

        let Some(Overlay::SqlHistory(view)) = app.overlay.as_ref() else {
            panic!("expected SQL history overlay from {focus:?}");
        };
        assert_eq!(view.mode, SqlHistoryMode::Browse);
        assert!(view.loading);
        assert_ne!(view.overlay_id, Uuid::nil());
        assert!(view.query_generation > 0);
        assert_eq!(app.focus, Focus::Results);
        assert_eq!(app.tabs.len(), tab_count);
        assert_eq!(app.active_tab, active_tab);

        let [Command::LoadSqlHistory {
            overlay_id,
            generation,
            request,
        }] = commands.as_slice()
        else {
            panic!("expected exactly one SQL history load command");
        };
        assert_eq!(*overlay_id, view.overlay_id);
        assert_eq!(*generation, view.query_generation);
        assert!(request.cursor.is_none());
        assert!(request.limit > 0);
    }
}
```

三个焦点覆盖主要帮助上下文；测试只运行 reducer，不需要连接数据库或读取真实历史。

**Step 3：运行测试，确认失败位置（2–5 分钟，首次编译可能更久）**

```bash
cargo test --test keymap help_sql_history_enter_opens_history_and_loads_first_page -- --exact
```

预期：在 `app.update(action.unwrap())` 内触发原来的 `display-only shortcut passed execution guard` panic。如果失败发生在筛选或按键断言，应先修正测试设置，保证复现的是用户报告的分发遗漏。不要使用 `#[should_panic]`，最终目标是正常打开面板。

### Task 2：补齐帮助入口的 Action 映射

**Files:**
- Modify: `src/app.rs`，`App::execute_help_shortcut()`。
- Test: `tests/keymap.rs`。

**Step 1：添加显式分发（2 分钟）**

在相邻分支间插入一行：

```rust
Id::OpenSqlEditors => vec![Action::OpenSqlEditorList],
Id::OpenSqlHistory => vec![Action::OpenSqlHistory],
Id::OpenNotificationHistory => vec![Action::OpenNotificationHistory],
```

后面的 `flat_map(|action| self.update(action))` 将复用既有初始化逻辑。此映射是直接修复点，不需要新建 CommandId 或复制历史加载代码。

**Step 2：运行新增测试（2–5 分钟）**

```bash
cargo test --test keymap help_sql_history_enter_opens_history_and_loads_first_page -- --exact
```

预期：1 个测试通过，内部三个焦点场景全部通过。

### Task 3：相关回归、交互验收与交付

**Files:**
- Verify: `src/app.rs`、`tests/keymap.rs`、`tests/sql_history_interaction.rs`。

**Step 1：检查格式（2 分钟）**

```bash
cargo fmt --all -- --check
```

预期：退出码 0。如有格式问题，仅调整本次修改涉及的代码，并重新检查。已有无关格式问题应单独记录。

**Step 2：运行相关集成测试（2–5 分钟，视编译耗时而定）**

```bash
cargo test --test keymap --test sql_history_interaction
```

预期：两个测试目标全部通过，覆盖帮助操作和历史面板现有交互。测试成功后，无新改动或失败疑点时即可进入交付。

**Step 3：本地终端手动验收（5 分钟）**

```bash
cargo run
```

1. 使用现有帮助快捷键打开帮助面板。
2. 搜索 SQL History，选中 `open SQL execution history`，按 Enter。
3. 确认帮助面板被历史面板替换；有记录时显示记录，无记录时正常显示空态，进程不退出。
4. 按 Esc 关闭面板，确认回到原工作区且没有新增标签。
5. 按 F7，确认同样可以打开历史面板。
6. 分别从 Explorer、Editor、Results 重复帮助入口操作。

如果执行环境无法提供交互式终端，在交付说明中明确手动验收未执行，保留自动测试结果。

**Step 4：审查补丁（2 分钟）**

```bash
git diff --check
git diff -- src/app.rs tests/keymap.rs
```

预期：生产代码只增加必要映射，测试覆盖实际帮助入口；无无关文件改动。

**Step 5：提交（需要提交时执行，2 分钟）**

按 @git-commit 流程复核暂存内容，并将代码和回归测试作为一个原子提交：

```bash
git add src/app.rs tests/keymap.rs docs/plans/2026-09-14-help-sql-history-crash-fix.md
git commit -m "fix(help): dispatch SQL history shortcut correctly"
```

交付说明应包含根因、修改路径、执行过的测试及结果、手动验收状态。

## 后续架构改进建议

当前问题源于帮助目录的 `executable: bool` 与执行映射分别维护。未来若继续增加帮助操作，可独立规划将目录项改为明确的执行描述（仅展示 / 语义命令 / 直接操作），由执行描述统一决定可执行性与分发目标。

这一改进需要逐项梳理带参数、依赖上下文、编辑器按键序列等操作，并为目录与执行目标的一致性建立测试；应先单独设计再迁移。本次修复的验收以映射补齐和入口回归通过为准。
