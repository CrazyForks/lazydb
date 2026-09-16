# LazyDB Update Result Presentation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 自动工作流执行约束优先：由插件安排后续阶段；不自行启动子 Agent、创建 worktree 或修改 state.json。若执行环境没有上述技能，按本文逐项实施和复核，不以技能名称缺失阻塞工作。

**Goal:** 让用户一眼区分 `lazydb update` 的更新成功、已经最新及需要操作等结果，并清楚看到版本信息。

**Architecture:** 在现有 CLI `run → format_update_report → main println!` 路径增加少量非序列化展示上下文，用纯字符串 renderer 输出分层摘要。保持 UpdateReport 的 JSON 契约和共享更新逻辑，终端样式策略在普通文本输出边界计算。

**Tech Stack:** Rust 2024、现有 crossterm 0.29、semver、std::io::IsTerminal、现有 Rust 单元测试与本地 HTTP 集成测试。

---

## 基线、范围与执行约定

- 已先读取同目录 `analysis.md`；它是本计划的需求及代码调查依据。
- 原工作空间：`/Users/yelog/workspace/tui/lazydb`；目标分支 `main`；指定起点 `1285bc0bf315b0f83efa3489d8365effa41b07a7`。
- 本阶段只新增本计划和最后的阶段回执，不实施业务代码。本轮已检查工作区干净。
- 分支名称由计划完成后的插件/Luna 流程决定；执行者使用插件分配的工作空间。
- 图片当前仅有占位符，采用分析确认的「自然语言标题 + 版本 + 次要操作」方案，不宣称精确复刻 omp 截图。
- 主要业务文件：`src/update.rs`。验证文件：`src/update.rs` 内 tests 模块、`tests/update_native.rs`。
- 复核但原则上无需修改：`src/main.rs:86`、`src/cli.rs:172-186`、`Cargo.toml`。
- 每个编号步骤是独立执行动作；表格驱动用例可逐行处理。先完成单项复核再进入下一项。
- 测试针对版本语义、检查模式和机器输出兼容等真实回归风险；不为每个空格、颜色调用或可逆文案变化建立重复快照。
- 本计划不要求在实施中自行提交；最终提交由自动工作流相应阶段管理。

## 最终展示契约

交互式、允许颜色的终端参考输出：

```text
✓ LazyDB updated successfully
  Version  0.1.4 → 0.1.5
  Channel  stable
```

```text
✓ LazyDB is already up to date
  Version  0.1.5
  Channel  stable
```

```text
↑ A LazyDB update is available
  Version  0.1.4 → 0.1.5
  Channel  beta
  Run      lazydb update --channel beta
```

版本为示意；实际输出必须来自报告和本次安装事实。标题加粗，成功绿色、可更新青色、需操作黄色、错误红色；版本正常亮度，标签和通道弱化但保持可读。没有固定边框，不依赖 Nerd Font 或 emoji。

纯文本使用 `[OK]`、`[INFO]`、`[WARN]`、`[ERROR]` 和 `->`，保留相同信息顺序。每条消息无开头空行，formatter 无尾部换行，main 的一次 println! 提供最终换行。

| 条件 | 标题 | 内容与约束 |
| --- | --- | --- |
| updated 且本次成功安装 | LazyDB updated successfully | 更新前版本 → 实际安装后版本；允许向低版本迁移，不使用 upgraded |
| updated 且本次未安装 | LazyDB update is already installed | 显示已安装版本；不显示伪造迁移，不声称本次成功安装 |
| up_to_date 且有效版本相等 | LazyDB is already up to date | 当前版本和通道 |
| up_to_date 且当前高于目标 | No LazyDB update needed | Installed 与 Channel latest 各一行，通道单独一行 |
| up_to_date 但版本缺失或不可比较 | No LazyDB update needed | 仅显示已知版本；不声称与通道最新版本相等 |
| update_available | A LazyDB update is available | 当前和目标版本、通道、`lazydb update --channel <有效通道>`；统一保留有效通道避免检查后使用错误通道 |
| manager_action_required | Update LazyDB using <管理器显示名> | 当前版本、通道和 action；未知管理器使用 `Manual update required for LazyDB` |
| error | Could not check for LazyDB updates | 原因完整保留；不得出现成功标记 |
| 未识别状态 | LazyDB update status | 单独显示原始 status 和非空 action，使用 INFO 而非成功语义 |

管理器显示名显式匹配：Native、Homebrew、npm、Debian package manager、RPM package manager、Arch package manager、Cargo、Unknown。现有 action 只有 Homebrew/Cargo 的命令标注 Run，其他描述标注 Next step；不根据文本猜测并执行命令。已知非 native 状态不自动表示发现新版本。

## Task 1：保留本次安装的展示上下文

**Files**
- Modify: `src/update.rs:309-356`（run），以及 formatter 附近的私有类型。
- Review: `src/update.rs:41-49`（UpdateReport）、`:564-580`（report_from_inspection）。
- Test: `src/update.rs` 的 tests 模块。

### 步骤

1. 在执行工作空间运行 `git rev-parse HEAD` 和 `git status --short --branch`，确认插件提供的基线和已有改动；若有他人改动先识别归属。不要自行重置文件。
2. 在 formatter 附近增加私有、非 Serialize 类型，用以下最小数据结构保存事实：

   ```rust
   #[derive(Default)]
   struct UpdateDisplayContext {
       previous_version: Option<String>,
       applied_in_this_run: bool,
   }
   ```

3. `inspect_local_installation(...).await` 完成后，以 `report.current_version.clone()` 初始化 previous_version，applied_in_this_run 初始化 false。
4. 仅在 `apply_native_update(...).await?` 返回成功之后设置 applied_in_this_run 为 true。保留现有 report 更新顺序和所有返回错误路径。
5. 为 formatter 增加上下文参数并从普通文本分支传入；暂时可保留旧输出，使改动可编译。JSON 分支继续原样序列化 report。
6. 复核 `--check` 和所有未进入安装分支的调用均不会设置 true；ReadyToRestart 对应的 updated 因而可与真实安装成功区分。

**复核与验收**
- 不给 UpdateReport 添加 previous_version 字段；成功 JSON 的 current_version 仍为安装后的版本。
- 不用编译时包版本推测安装前版本，不通过 current_version != target_version 推断是否安装。
- 不改变 source.state、锁、下载、校验、替换文件和 manifest 获取次数。

**验证命令**
`cargo test --lib update::tests::update_report_serializes_the_stable_contract_on_one_line`

预期：现有 JSON 契约测试通过，代码编译成功。此处只验证结构接入，显示语义在 Task 2 验证。

## Task 2：实现按状态分层的纯文本摘要

**Files**
- Modify: `src/update.rs:729-735`（format_update_report）及其附近的少量私有辅助函数。
- Test: `src/update.rs` 的 tests 模块。

### 步骤

1. 添加 `update_report_distinguishes_applied_and_preinstalled`：构造同一 updated 报告，以 true/false 上下文渲染；断言只有 true 分支显示 successfully 和旧到新迁移，false 分支显示 already installed。这是丢失安装上下文的关键回归测试。
2. 运行 `cargo test --lib update::tests::update_report_distinguishes_applied_and_preinstalled`，确认旧格式无法满足语义断言，再实现 updated 两分支使其通过。
3. 增加一个表格驱动语义测试 `update_report_preserves_version_and_action_semantics`，覆盖下面的案例。用标题、关键版本/操作片段和禁止出现的错误结论断言，不比对整段装饰快照。

   | 输入 | 必须证明 |
   | --- | --- |
   | 相同版本 up_to_date | already up to date，显示当前版本，无 action: none |
   | 当前版本高于通道 up_to_date | No update needed 语义，分别显示 Installed 和 Channel latest |
   | beta 的 update_available | 展示版本变化和 `lazydb update --channel beta`，不出现 successfully/will be applied |
   | updated 且向低版本迁移 | 真实旧到新方向，不出现 upgraded |
   | Homebrew manager_action_required | 显示正确 Run 命令，不宣称自动安装成功 |
   | Unknown manager_action_required | 原因/下一步完整保留，标题表示手动更新 |
   | error | 显示完整原因，不使用 OK 成功结论 |
   | 缺失或无效版本 | 不出现 None、虚构版本、空箭头或不成立的相等声明 |
   | 未知状态 | 保留 status/action，以中性结果兜底 |

4. 运行新测试确认失败来自所需行为，而非 fixture 错误。
5. 根据前述展示契约实现 status match；用 `semver::Version::parse` 比较版本。迁移行只有两端都存在时输出；仅一端已知时使用 Current/Target 或 Installed 标签，不补造未知端。
6. 添加显式 manager 显示名匹配，继续使用现有 `channel_name`。action 为空时省略操作行，错误原因和未知来源说明不截断。
7. 用 `Vec<String>` 组织标题和各行，最后 `join("\n")`；不在 formatter 中 print、不添加尾部换行。不要将本命令扩展为公共跨命令渲染框架。
8. 运行 `cargo test --lib update::tests::update_report_`，修复语义断言失败，确认已有序列化测试一起通过。

**复核与验收**
- 检查只读 `--check` 输出中的动词，没有「已执行」含义。
- ReadyToRestart、当前高于通道版本等边界符合真实事实。
- 视觉信息顺序固定为结论、版本、通道、必要操作；普通 native 成功不显示多余 manager/debug 信息。

## Task 3：接入终端样式与纯文本降级

**Files**
- Modify: `src/update.rs` 的导入、文本输出边界和 formatter 辅助函数。
- Review: `Cargo.toml` 中已有 crossterm 依赖；`src/main.rs:86` 的单次 println!。
- Test: `src/update.rs` 的 tests 模块。

### 步骤

1. 增加私有样式选项，优先使用 `Plain` / `Styled` 两种模式；formatter 显式接收模式，不直接读取环境。
2. 将终端策略实现为纯函数，输入 stdout 是否 TTY、NO_COLOR 是否为非空值、TERM 是否 dumb。策略固定如下：

   ```rust
   fn use_styled_output(is_terminal: bool, no_color: bool, dumb_terminal: bool) -> bool {
       is_terminal && !no_color && !dumb_terminal
   }
   ```

   NO_COLOR 采用非空值关闭颜色的约定；空值不关闭。读取使用 var_os，避免非 Unicode 环境值被错误当作未设置。TERM 缺失不单独禁色，仍以 TTY 为必要条件。
3. 仅在 `args.json` 提前返回之后读取 stdout IsTerminal 与上述环境值，选择模式；不读取 stdin 的终端状态。
4. 根据展示契约着色、加粗标题并弱化标签，复用 crossterm 样式能力；每个片段重置属性。Plain 模式不经过带样式字符串路径，使用 ASCII 标记和箭头。
5. 添加 `update_report_respects_plain_output_policy`：覆盖 TTY/非 TTY、NO_COLOR、dumb 的策略组合，并验证 Plain renderer 输出没有 `\x1b` 且保留标题、版本与命令。测试传参数，不修改进程级环境变量。
6. 运行 `cargo test --lib update::tests::update_report_respects_plain_output_policy`。
7. 检查 Styled 字符串仍包含可读文本且没有尾部样式泄漏；若需要 API 细节，实施时查阅对应版本文档，而非引入新库解决。

**复核与验收**
- 重定向、NO_COLOR 非空或 TERM=dumb 均无 ANSI；FORCE_COLOR 等未约定变量不覆盖此策略。
- 显示不依赖特殊字体，纯文本依旧一眼可辨。
- 不改全局颜色设置，不影响 TUI 或其他命令；Cargo.toml/Cargo.lock 无需变化。

## Task 4：验证 CLI 接线及机器输出兼容

**Files**
- Modify/Test: `tests/update_native.rs`。
- Review: `src/update.rs` 的 run、JSON 分支和 renderer；`src/main.rs:86`。

### 步骤

1. 复用 `installed_launcher_reports_native_manager` 的临时安装目录和本地 HTTP fixture，将仅在测试内重复的准备逻辑提取为小辅助函数，支持设置 manifest 版本和命令参数。
2. 每次 CLI 调用使用独立 fixture/服务，或明确接受预定请求数；不要使现有仅接受一次请求的线程为第二次调用永久等待。保留读超时和总截止时间。
3. 保留现有 `update --check --json` 回归，补充断言 stdout 是单行 JSON、字段集合仍为 schema/manager/channel/current_version/target_version/status/action，且无 ANSI 和额外摘要文字。
4. 新增 `native_check_text_reports_available_update`：目标版本高于当前，使用 `update --check` 和捕获 stdout；断言可用更新标题、两端版本、下一步命令、无 ANSI，安装状态文件仍为原版本。
5. 新增 `native_check_text_reports_up_to_date`：manifest 版本取 `CARGO_PKG_VERSION`，若它是预发布则 fixture/channel 一并匹配；断言已经最新标题和当前版本，不出现成功安装、箭头和操作建议。
6. 不用本地 fixture 尝试伪造可下载的 GitHub 更新地址；真实安装成功后的展示由上下文/renderer 语义测试验证，本次不改下载抽象或进行真实自更新。
7. 运行 `cargo test --test update_native`，确认退出状态、服务线程结束、stdout 内容和状态文件断言均通过。

**复核与验收**
- 测试只使用临时目录和本地 manifest，不访问生产 release，不改开发者安装。
- 子进程可以独立设置环境变量；避免在并行 Rust 测试进程中修改全局环境。
- `tests/update_native.rs` 受 cfg(unix) 约束；跨平台核心展示行为由单元测试覆盖。不得把 Windows 上跳过的集成测试报告为执行成功。

## Task 5：视觉复核、最终检查和交付

**Files**
- Review: `src/update.rs`、`tests/update_native.rs` 及最终 diff。
- Conditional regression: `tests/update_reducer.rs`（仅在共享更新类型/逻辑确有变化时）。

### 步骤

1. 在允许颜色的终端预览 renderer 生成的成功、最新、可更新、错误四种示例；可用临时测试打印真实 formatter 结果后移除预览代码，不增加公开 CLI 开关。
2. 在约 40 列和 80 列宽度检查：首行结论明确、标签与值分层、长操作自然换行、无边框错位和截断。窄终端允许自然折行，不实现自适应面板系统。
3. 将相同示例切换 Plain 模式，复核 ASCII 标记、版本方向和后续操作。确认样式结束后 shell 颜色正常。
4. 执行 `cargo fmt --check`。若失败，只格式化本次涉及文件后重新检查。
5. 执行 `cargo test --lib update::tests`，预期更新相关单元测试全部通过。
6. 若 Task 4 后未再改动相关实现，沿用已通过的 `cargo test --test update_native` 结果；有改动则重跑。不要无新理由重复测试。
7. 若实际触及共享检查逻辑/类型，执行 `cargo test --test update_reducer`；否则记录未触及共享逻辑，无需扩大测试范围。
8. 执行 `git diff --check`、`git diff --stat`、`git status --short`，检查空白问题和文件范围，再人工检查 diff。
9. 最终说明实现内容、通过的命令、视觉检查情况以及实际未执行/被环境阻塞的检查。由插件进入相应复核/提交阶段。

### 最终验收清单

- [ ] 更新成功与已经最新在第一行明确区分，无需阅读内部状态码。
- [ ] 成功显示真实版本迁移；未执行安装时不声称本次成功安装。
- [ ] 相等、高于通道、允许降级、缺失版本的文案均准确。
- [ ] `--check` 的下一步命令带有效通道，不暗示更新已执行。
- [ ] 管理器指引和错误原因完整，没有 `action: none` 或 Debug 枚举格式。
- [ ] TTY 有清晰但克制的颜色层级；纯文本和窄终端保持可读。
- [ ] 非 TTY、NO_COLOR 非空、TERM=dumb 输出无 ANSI。
- [ ] JSON 字段、单行性质和安装后的版本语义未改变，没有附加文字。
- [ ] 安装逻辑、TUI、退出码及依赖未因展示优化发生附带改动。
- [ ] 所有必需验证有真实结果；真实自更新未被用作展示验收手段。

## 本阶段完成说明

本计划仅定义后续实施步骤，没有执行上述业务改动、测试或真实更新。任务命名、分支与 worktree 生命周期及后续执行方式已由用户选择的自动工作流接管，无需再次询问。完整计划以当前任务目录中的本文件为准，不另外向仓库 docs/plans 写入副本。
