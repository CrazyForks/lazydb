# Consoles Panel Layout Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 将 Consoles 列表调整为名称后显示打开状态圆点、右侧显示带数据库类型图标及颜色的所属连接与 database/schema，并实现稳定的右对齐。

**Architecture:** 在现有 `render_console_manager` 中完成展示数据归一化、列表级列宽计算及分段行渲染。状态圆点只由 `record.open` 决定；执行目标有效性用于右侧异常提示。复用现有 IconSet、Theme 和终端字符宽度工具。

**Tech Stack:** Rust 2024、Ratatui 0.30.2、unicode-width、现有 UI 渲染测试。

---

## 实施顺序

1. 更新 `tests/ui_render.rs` 的 Console 展示基线与回归测试。
2. 修改 `src/ui/mod.rs::render_console_manager`，去除 OPEN/CLOSED 与连接状态文本，加入打开圆点及数据库图标。
3. 实现基于终端 cell width 的共享列宽、右对齐和窄窗口降级。
4. 核对 Browse/Search/Rename/Delete 模式及快捷键交互。
5. 运行定向测试、完整 UI 测试、格式化、编译和 diff 检查。

## 展示契约

- `●` 表示 Console 已打开，`○` 表示已关闭；状态只由 `record.open` 决定。
- ASCII 模式使用 `*` / `o`。
- 右侧按 `[数据库图标] 连接名    database/schema` 展示，连接列和位置列统一右对齐。
- 图标和颜色复用 `IconSet::database`、`IconSet::database_color`。
- 未绑定显示“未绑定”，无效目标显示“目标失效”；无 schema 时仅显示 database。
- 选中行的背景覆盖整行空白，但保留状态圆点和数据库图标的前景色。
- 长文本按终端 cell width 截断，窄窗口优先保留选择标记、Console 名称和打开状态。

## 验收命令

```bash
cargo fmt --check
cargo test --test ui_render console_manager
cargo test --test ui_render
cargo check
git diff --check
```
