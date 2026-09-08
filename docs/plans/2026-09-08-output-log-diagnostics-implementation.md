# OUTPUT LOG 样式与错误诊断实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** OUTPUT LOG 的时间使用低调主题色，错误正文与诊断续行使用错误主题色，并保留 PostgreSQL 返回的 SQLSTATE、severity、Position、Detail、Hint 和 Context。

**Architecture:** 保留 OutputEntry 文本、SQL 范围和只读编辑器，增加显式时间范围，在 UI 层叠加日志前景色，不重写 SQL 分析器。DatabaseError 保留可选结构化诊断，通过仅用于 Console 输出的格式化入口传递详细错误，不改变全局 Display 和关系编辑脱敏行为。

**Tech Stack:** Rust 2024、SQLx 0.9、ratatui 0.30、现有 EditorWorkspace/modalkit、Rust 单元与集成测试；不新增生产依赖。

---

## 实施顺序

1. 建立 OUTPUT LOG 回归测试：固定时间、错误正文、Position 的文本和颜色契约。
2. 扩展 `DatabaseError`，保留 PostgreSQL 结构化诊断，保持全局 `Display` 不变。
3. 增加仅用于 SQL Console 的错误格式化方法，验证安全边界。
4. 接通普通查询、分页查询和手动事务的失败路径。
5. 为 `OutputEntry` 增加显式时间范围，统一错误续行的时间格式。
6. 在现有编辑器样式合成中实现时间低调色和错误红色，保留 SQL 高亮、选择、复制和滚动。
7. 执行完整测试、隔离 PostgreSQL 验证和最终 diff 复核。

## 关键约定

- 时间及方括号使用 `theme.muted`；错误正文及诊断续行使用 `theme.error`。
- SQL 保留现有语法高亮，不把日志语义伪装成 SQL token。
- 使用显式范围元数据，不通过正则猜测时间或错误。
- `DatabaseError::Display` 不改变；详细诊断只进入 SQL Console。
- 关系编辑、agent/MCP 输出及已有脱敏边界不扩大。
- PostgreSQL Internal Position 不映射为用户 SQL 位置；Position 只显示服务器返回值。
- 本次不改变 UTC 时间语义，不实现自动折行、错误跳转或 SQL 波浪线。

## 主要文件

- `src/db/mod.rs`: 结构化数据库诊断和 Console 专用格式化。
- `src/db/transaction.rs`, `src/runtime/transaction.rs`: 手动事务错误边界。
- `src/runtime.rs`: 普通、分页和派生查询失败接线。
- `src/model/tab.rs`, `src/app.rs`: OutputEntry 时间范围和日志构造。
- `src/editor/mod.rs`: 仅在需要时扩展渲染范围投影能力。
- `src/ui/mod.rs`: OUTPUT LOG 前景色合成。
- `tests/ui_render.rs`, `tests/sql_execution.rs`, `tests/transaction_reducer.rs`, `tests/postgres_adapter.rs`: 回归和集成测试。

## 每项任务的执行规则

每项任务按以下顺序执行：

1. 先写或补充失败测试。
2. 运行最小范围测试，确认失败原因是预期行为而非编译或环境问题。
3. 做最小实现，不扩大公共接口和无关错误路径。
4. 运行任务对应测试。
5. 检查 `git diff --check`、工作区状态和相关 diff，确认没有覆盖其他改动。
6. 复核通过后再进入下一项。

## 最终验收

目标输出应类似：

```text
[2026-09-08 11:08:17:413] moss_biz.tools> SELECT * FROM sdfsdf;
[2026-09-08 11:08:17:413] [42P01] ERROR: relation "sdfsdf" does not exist
[2026-09-08 11:08:17:413] Position: 15
```

最终运行：

```sh
cargo fmt --check
cargo check --all-targets
cargo test --lib
cargo test --tests
cargo clippy --all-targets -- -D warnings
```

已配置隔离 PostgreSQL 时，再运行对应门控集成测试；不打印连接 URL 或凭据。交付时分别说明已运行、跳过和人工验证的项目。
