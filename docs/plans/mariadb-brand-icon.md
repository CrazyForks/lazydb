# MariaDB Brand Icon Implementation Plan

> **执行者：Luna。** Astra 仅完成分析与计划；后续实现、审查、纠偏、提交和合并均由 Luna 执行。用户已选择自动工作流，直接按阶段继续，不询问执行方式，不启动子 Agent。

**Goal:** 为 MariaDB 使用合适的独立品牌图标，使其与 MySQL 可区分。

**Architecture:** 在统一接口 `IconSet::database` 中将 MariaDB 的 Nerd Font 映射改为已有依赖提供的 `dev::DEV_MARIADB`。驱动选择器、Explorer 和 Omni 等调用方自动使用新字形；现有 Unicode/ASCII 回退仍为 `MA`。

**Tech Stack:** Rust、Ratatui、nerd-font-symbols 0.3.0、现有 Rust 单元测试和 UI 渲染测试。

---

## 1. 基线、依据与范围

- 原工作空间：`/Users/yelog/workspace/tui/lazydb`；目标分支：`main`。
- 指定起点与本轮实际 HEAD：`9cdd680416c5f5d13e5b89da14fb9d6cac9534a1`。
- 本计划依据同任务目录中的 `analysis.md`，该文件已按本轮要求首先读取。
- 当前工作区 `src/ui/icons.rs` 无差异；main 领先 origin/main 35 个提交。既有未跟踪目录与计划文件均保留。
- `checkpoint.json` 本轮读取仍为 File not found，不创建或修改该文件；`state.json` 由插件管理。
- 工作流名称与任务分支在计划完成后由自动工作流/Luna 确定，本阶段不创建分支。
- 本文件为当前自动任务的完整执行计划；此前 `docs/plans/2026-09-17-mariadb-brand-icon.md` 保留为前序计划产物，涉及回执时以本轮明确指定的路径/token 为准。

### 根因与选定方案

`src/ui/icons.rs:245` 的 MariaDB Nerd Font 分支误用了 `dev::DEV_MYSQL`。分析已确认锁定依赖提供 `dev::DEV_MARIADB`，码点为 U+E828，与 MySQL U+E704 不同。采用品牌常量优于通用数据库符号、Emoji 或文本替代，无需修改依赖和调用方。

## 2. 验收标准与验证分级

### 用户需求及功能验收

1. MariaDB 在默认 Nerd Font 模式使用 `dev::DEV_MARIADB`，MySQL 保持 `dev::DEV_MYSQL`，两者图标不同。
2. 驱动选择器、Explorer、Omni 等统一映射调用方一致生效。
3. Unicode/ASCII 的 `MA` 与 `MY` 回退保持正确，标签布局、选中状态、点击区域无回归。
4. 修改仅限图标需求；不新增依赖、配置迁移或数据库协议行为变化，保留既有颜色。

### 项目强制检查

按 `CONTRIBUTING.md:6-12`，功能齐备后执行一次：

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

### 本计划选择的定向自动验证

用于快速验证受影响单元，执行命令见步骤 3。此为实施验证安排，不声称是用户额外提出的验收要求。

### 补充验证（非强制门禁）

兼容 Nerd Font 终端中的人工/PTY 视觉检查用于补充实际字体绘制证据，用户和项目未要求必须提供截图或 PTY 验收。环境不可用时记录限制，不将其自动升级为必需门禁。最多一次有针对性的修复重试，然后由 Luna 收尾审查决定补证据或记录限制。

## 3. 唯一业务单元：MariaDB 专属品牌图标

**业务修改文件：** `src/ui/icons.rs:240-270`，仅 Nerd Font 的 MariaDB 分支。

**复核调用方（预期无需改动）：**
- `src/ui/profiles.rs:659-718`：`render_driver_options` 使用统一图标接口及 `cell_width()`。
- `src/ui/mod.rs:3068-3071`：Explorer 连接行使用 `icons.database(kind)`。
- `src/ui/omni.rs:149,165`：连接项及 profile 图标复用统一接口。

**既有验证文件：** `src/ui/icons.rs` 的测试模块、`tests/ui_render.rs`。

**记录文件：** 原工作空间 `.git/opencode-tasks/ses_f512f26c4ffe4ZULY4R60ZVoGf/validation.md`；在工作树实施时也记录到此任务指定路径。

### 步骤 1：确认实际实施基线

在自动工作流建立的任务分支/工作树中执行：

```bash
git status --short --branch
git rev-parse HEAD
git diff -- src/ui/icons.rs
```

记录实际版本、分支和工作区变化；检查当前 checkpoint（如存在）及目标映射。若目标分支已推进，以实际源码定位该分支，不盲用行号。保留其他任务文件，不读取旧 next 作为新的要求。

### 步骤 2：应用最小修正

仅修改 `IconSet::database` 的以下映射：

```diff
 DatabaseKind::MySql => dev::DEV_MYSQL,
-DatabaseKind::MariaDb => dev::DEV_MYSQL,
+DatabaseKind::MariaDb => dev::DEV_MARIADB,
```

`dev` 已导入，依赖 0.3.0 已有常量。保留 Unicode/ASCII `MA` 分支、MySQL 映射与 `database_color`。

### 步骤 3：运行定向自动验证

```bash
cargo test --lib ui::icons::tests
cargo test --test ui_render driver_options_
cargo test --test ui_render explorer_uses_selected_icon_mode
```

预期各命令退出 0，过滤条件实际匹配到测试。现有覆盖包括映射安全性、图标模式、驱动选中状态、点击区域和 Explorer 图标输出。

本次为低影响、可逆的常量映射修正，不新增仅镜像实现的测试。现有 UI 测试动态读取统一图标接口，并不能独立证明品牌常量选对；Luna 必须结合 diff 复核 MariaDB 指向 `DEV_MARIADB`，MySQL 指向 `DEV_MYSQL`。

每个实际命令追加退出结果、关联文件、HEAD/未提交修改状态和环境到 validation.md。普通编译/测试失败自行修复并重跑受影响检查。

### 步骤 4：完成项目强制检查

执行第 2 节三个项目命令，预期退出 0。记录真实失败或环境跳过，数据库集成测试未配置导致的跳过不能写成真实连接验证通过。只有相关代码或环境变化才重跑对应检查；不要重复全量 check/clippy/test 循环。

### 步骤 5：可选实际终端视觉检查

如已有可用构建产物和兼容 Nerd Font 终端，以 `--icons nerd-font` 查看驱动选择器中 MySQL/MariaDB 的字形差异、标签间距和选中状态；可查看已有连接行。需要时用 `--icons unicode` 或 `--icons ascii` 补看 `MY`/`MA`。

无需为图标检查创建真实数据库或新凭据。本检查受字体/PTY 限制时按第 2 节的补充检查规则收尾，不能无限保持 progress 重试同一环境，也不能要求用户代做剩余实现。

## 4. Luna 收尾复核、提交与合并

### 步骤 1：审查差异

```bash
git diff --check
git diff -- src/ui/icons.rs
git status --short --branch
```

验收：业务 diff 只有目标映射变更；调用方继续共用接口；没有依赖/配置变更或误带其他任务文件；验证记录与实际待提交版本一致。补充视觉检查缺失如实记录，强制检查失败不得冒充通过；需要修复的普通问题由 Luna 继续处理。

### 步骤 2：提交和合并

由 Luna 使用适用的 git-commit 工作流精确暂存本任务文件。建议业务提交命令/信息：

```bash
git add src/ui/icons.rs
git commit -m "fix(ui): use dedicated MariaDB brand icon"
```

计划文档是否入库遵循自动工作流；禁止宽泛暂存既有未跟踪文件。按当前自动流程合入 `main`，记录真实提交版本，不硬编码尚未生成的任务分支名。合并发生相关冲突或代码变化时，再验证受影响内容。

### 步骤 3：阶段记录

后续实施/审查阶段使用其当轮明确指定的回执路径/token，不复用 plan 或 analyze 回执。由插件维护 checkpoint/state。整个流程无需重新询问用户执行方式或反复要求 resume。

## 5. 计划阶段交付

本阶段仅产出计划与记录，不实施业务代码、不运行 Rust 测试或声称上述检查已通过。本轮最后写入：

`.git/opencode-tasks/ses_f512f26c4ffe4ZULY4R60ZVoGf/plan-758d0149-ccad-4d02-b984-d0e7a5a4f285.json`

```json
{"token":"758d0149-ccad-4d02-b984-d0e7a5a4f285","stage":"plan","status":"completed"}
```

下一阶段由 Luna 命名并执行第一个且唯一的业务单元。方案确定，无需外部输入或用户裁决。
