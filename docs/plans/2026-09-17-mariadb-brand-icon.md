# MariaDB Brand Icon Implementation Plan

> **执行者：Luna。** 按本计划完成实现、验证、审查和提交合并；Astra 仅负责分析与计划。遵循当前工作流分配，不启动子 Agent。

**Goal:** 让默认 Nerd Font 模式下的 MariaDB 使用独立品牌图标，与 MySQL 清楚区分。

**Architecture:** 修正统一的 `IconSet::database` 映射，以已有依赖的 `dev::DEV_MARIADB` 替代误用的 `dev::DEV_MYSQL`。驱动选择器、Explorer、Omni 等调用方自动获得新字形；Unicode/ASCII 回退继续使用 `MA`。

**Tech Stack:** Rust、Ratatui、nerd-font-symbols 0.3.0、现有 Rust 单元与 UI 渲染测试。

---

## 基线与阶段约束

- 工作空间：`/Users/yelog/workspace/tui/lazydb`。
- 目标分支：`main`。
- 分析基线：`9cdd680416c5f5d13e5b89da14fb9d6cac9534a1`。
- 分析依据：`.git/opencode-tasks/ses_f512f26c4ffe4ZULY4R60ZVoGf/analysis.md`。
- 验证记录：`.git/opencode-tasks/ses_f512f26c4ffe4ZULY4R60ZVoGf/validation.md`。
- 计划阶段已确认 `src/ui/icons.rs` 没有工作区差异，`checkpoint.json` 仍不存在。既有未跟踪文件均保留。
- 工作流名称由 Luna 确定；计划完成后按工作流自动命名任务分支。本计划不提前创建分支或工作树。
- 本次只有一个可验收业务单元。实施者连续完成该单元及收尾，不要求用户反复 resume。

## 验收标准

1. Nerd Font 模式的 `DatabaseKind::MariaDb` 返回 `dev::DEV_MARIADB`（U+E828），MySQL 仍返回 `dev::DEV_MYSQL`（U+E704）。
2. 驱动选择器及 Explorer 等统一接口调用方展示新图标；宽度计算和选择区域验证通过。
3. Unicode/ASCII 模式仍分别显示 MariaDB `MA`、MySQL `MY`。
4. 无新增依赖、依赖升级、数据迁移、数据库协议改动或额外主题改色。
5. 定向与项目要求的最终检查有真实记录，未执行的人工检查不标记为通过。

## Task 1：修正统一品牌图标并完成验收

**Files:**
- Modify: `src/ui/icons.rs:245`。
- Existing tests: `src/ui/icons.rs` 的 `tests` 模块。
- Existing tests: `tests/ui_render.rs` 的驱动选项及 Explorer 图标模式测试。
- Record: `.git/opencode-tasks/ses_f512f26c4ffe4ZULY4R60ZVoGf/validation.md`。

### Step 1：确认实施工作区

在工作流指定的任务分支/工作树中执行：

```bash
git status --short --branch
git rev-parse HEAD
git diff -- src/ui/icons.rs
```

记录实际版本与工作区状态；如 main 已推进，以实际代码为准确认目标映射仍需修复，保留其他任务的文件和改动。只读检查当前 checkpoint（如存在），不修改 `state.json` 或 `checkpoint.json`。

### Step 2：实施最小映射修正

在 `IconSet::database` 的 `IconMode::NerdFont` 分支应用唯一业务变更：

```diff
 DatabaseKind::MySql => dev::DEV_MYSQL,
-DatabaseKind::MariaDb => dev::DEV_MYSQL,
+DatabaseKind::MariaDb => dev::DEV_MARIADB,
```

依赖已锁定 0.3.0 且提供该常量，`dev` 已导入，不需要新增 import。`database_color`、Unicode/ASCII 分支及各渲染入口不需要改动。

### Step 3：运行既有定向检查

```bash
cargo test --lib ui::icons::tests
cargo test --test ui_render driver_options_
cargo test --test ui_render explorer_uses_selected_icon_mode
```

预期：命令退出 0，各过滤条件实际匹配并运行测试，不能将运行 0 个测试当作验收通过。

现有覆盖包括映射字符安全性、不同图标模式、驱动标签/选择状态/点击区域以及 Explorer 模式选择。由于只是可逆的常量修正，本任务不新增镜像实现的测试；审查 diff 时确认专属 MariaDB 常量，现有动态读取 `icons.database` 的 UI 测试不能单独证明品牌常量选择正确。

逐条追加命令、退出结果、相关文件、实际 HEAD/未提交 diff 状态和环境到 validation.md。普通编译或测试失败自行修复后重跑受影响检查。

### Step 4：执行项目最终检查

按 `CONTRIBUTING.md` 约定，在功能齐备后运行一次：

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

预期退出 0。明确记录数据库环境未配置导致的集成测试跳过。若遇已有无关失败，记录失败位置及其与本次 diff 的关系，由 Luna 收尾审查决定处理方式。只有相关代码或环境变化才重跑相应检查，避免重复全量循环。

### Step 5：补充视觉检查（有环境时）

可在使用兼容 Nerd Font 的终端运行工作流已有构建产物，以 `--icons nerd-font` 查看驱动选择器中的 MySQL/MariaDB，确认字形不同且标签间距正常；必要时查看已有连接行。可用 `--icons unicode`、`--icons ascii` 检查 `MY`/`MA` 回退。

该检查属于补充证据，不是用户或项目强制的人工/PTY 检查；无需为此新建真实数据库或连接凭据。字体或 PTY 环境受限时最多一次有针对性的修复重试，再记录限制并由 Luna 收尾审查决定是否需要其他证据。当前分析没有验证用户终端字体实际支持 U+E828。

## Task 2：Luna 收尾审查与交付

### Step 1：审查实际差异和记录

```bash
git diff --check
git diff -- src/ui/icons.rs
git status --short --branch
```

确认业务差异只有 MariaDB Nerd Font 映射，MySQL/回退模式保持预期；确认验证记录对应当前代码，不引用旧轮次结果作为本轮通过证据。

### Step 2：按工作流提交、合并

由 Luna 使用提交工作流执行，建议提交信息：

```text
fix(ui): use dedicated MariaDB brand icon
```

精确暂存 `src/ui/icons.rs`；本计划文档是否纳入提交遵循项目工作流。不要使用宽泛暂存把既有未跟踪计划或其他任务文件带入提交。按流程合入 `main`，记录实际提交和验证结果。

### Step 3：完成当前阶段回执

后续各阶段只写当轮用户/插件明确指定的回执路径及 token，最后标记实际阶段结果；不复用 analyze 回执，不自行维护 checkpoint/state。当前 plan 消息没有指定新的 JSON 回执路径或 token，因此计划阶段仅产出本文件，不伪造回执。

## 交接结论

方案已确定，无待用户决定事项。下一步由 Luna 命名工作流与任务分支，并执行 Task 1 的单个业务闭环，再完成收尾。计划阶段没有实施代码变更，也没有运行或宣称通过上述 Rust 检查。
