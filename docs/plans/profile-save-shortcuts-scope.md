# 连接编辑弹窗统一 Save — Implementation Plan

> 执行负责人：Luna。按本计划完成实现、验证、审查和后续提交合并；Astra 仅负责分析与计划。不得启动子 Agent。本文件不授权跨阶段执行；阶段调度遵循当前用户指令。

**Goal:** 连接表单以 Space 打开 Visible objects，提供唯一的 Save（保存并连接），以 Enter 替代旧保存快捷键。

**Architecture:** 统一表单字段与按钮枚举，键盘、鼠标复用现有 `ProfileSave { connect: true }`，保留 reducer/runtime 保存、校验和连接流水线。Enter 默认提交表单，但焦点在 Test/Cancel 时激活该按钮；Space 用于字段局部操作。渲染提示、共享帮助和文档与真实动作同步。

**Tech Stack:** Rust、crossterm KeyEvent、ratatui、现有 App/Action/Command 状态机及 Rust 集成测试。

---

## 0. 工作区、阶段与交接约束

- 原目录：`/Users/yelog/workspace/tui/lazydb`；目标分支：`main`；起点：`f7d03d54ac463cbe1c322e738f9a6fe7ee2d773c`。
- 计划阶段再次确认 HEAD 等于起点，`git status --short` 无输出；没有未提交业务依赖。`checkpoint.json` 仍不存在。
- 任务资料的唯一目录：`/Users/yelog/workspace/tui/lazydb/.git/opencode-tasks/ses_f4366bbcaffe4VBrDXVknpNbnl`。分析为 `analysis.md`，验证记录为 `validation.md`。后续新 worktree 中 `.git` 可能是文件，资料路径仍使用这里的绝对路径。
- 本阶段不创建分支或 worktree、不修改业务文件或 Git index、不修改 state/checkpoint、不 stash。工作流和任务分支由后续 Luna/调度器命名；不抢占名称。
- 本轮完成时先写 `change-scope.json`，最后写 `plan-bdcbb9e3-4fac-43bf-a83f-192f8a3fe7e8.json`，token 为 `bdcbb9e3-4fac-43bf-a83f-192f8a3fe7e8`。不覆盖 analyze 或其他历史回执。
- 实现恢复时先看实际 diff 与存在的 checkpoint，定位下面尚未完成的步骤；不要照搬历史 next 或等待 resume。
- 以下是**一个端到端业务闭环**的顺序步骤，不是可单独宣告完成的多个需求。实现阶段持续推进至全部验收成立。

### 文件范围清单

预计修改文件如下，已同步至同目录 `change-scope.json`。没有预计新增、删除或重命名业务文件，没有未提交依赖文件：

- `src/model/profile_manager.rs`
- `src/input/keymap.rs`
- `src/ui/mod.rs`
- `src/input/mouse.rs`
- `src/ui/profiles.rs`
- `src/help.rs`
- `tests/keymap.rs`
- `tests/profile_draft.rs`
- `tests/profile_reducer.rs`
- `tests/ui_render.rs`
- `tests/mouse.rs`（若渲染测试已完整覆盖鼠标路径，可不修改）
- `docs/keybindings.md`
- `docs/ui-dialog-guidelines.md`

`src/app.rs`、runtime、action 定义、`tests/profile_runtime.rs`、`tests/profile_lifecycle.rs` 和 CONTRIBUTING 是参考/回归运行范围，不计划修改，故不列入 change-scope。报告、计划、验证记录及回执属于任务元数据，不属于业务变更清单。后续若发现必须扩大业务修改范围，先更新范围依据再实施。

### 验收要求与验证等级

1. **用户需求验收（必需）**：Visible objects 焦点 Space 进入范围选择；移除旧仅保存入口与 Ctrl+S；原 Save & Connect 显示为唯一 Save，主提交快捷键改为 Enter。按钮行为仍为保存并连接。
2. **项目强制门禁（必需）**：CONTRIBUTING 要求同步共享快捷键目录和按键文档，并通过 `cargo fmt --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --all-targets --all-features`。功能齐备后执行一轮并记录当前版本结果。
3. **本计划针对性自动验证**：步骤 A–D 的键盘、状态机、导航、鼠标命中与渲染回归用于证明本需求及防止相关回归。按实际改动选择对应测试，不重复建立同义测试；失败修复后只重跑受影响检查。`git diff --check` 是建议的静态补充检查，不冒称 CONTRIBUTING 强制项。
4. **补充建议（非新增强制门禁）**：真实终端/PTY 宽窄屏人工操作、真实外部数据库连接验证。现有自动测试足以提供相关证据时不强制增加人工验收。环境受限最多一次针对性修复重试，由 Luna 收尾审查决定补充证据或记录限制，不阻塞在无限重试中。

## 1. 固定交互规则

| 上下文 | 无修饰 Enter | Space |
| --- | --- | --- |
| Form 文本字段，包括 URL | `ProfileSave { connect: true }` | 文本输入 |
| Form 分类/驱动 | 保存并连接 | 现有无动作 |
| Form 循环选项 | 保存并连接 | `ProfileCycle(1)` |
| Form 开关 | 保存并连接 | `ProfileToggle` |
| Form VisibleObjects | 保存并连接 | `ProfileOpenScope` |
| Form Test | `ProfileTest` | `ProfileTest` |
| Form Save | 保存并连接 | 保存并连接 |
| Form Cancel | `CloseProfileManager` | `CloseProfileManager` |
| Scope | `ProfileScopeBack` | 现有范围项切换 |
| 删除确认 | 现有焦点按钮激活 | 现有行为 |

Ctrl+S、Ctrl+Enter、Ctrl+Shift+Enter 均不再保存；Shift+Enter、Alt+Enter 不添加新保存别名。Ctrl+T、Esc、Tab/Shift+Tab、文本编辑快捷键保持原语义。新建和编辑连接共用这些规则。底层 `connect: false` 的内部能力与测试保留，UI 不再触发它。

## 2. 步骤 A：先建立有意义的按键回归边界

**文件：** `tests/keymap.rs`（现有 `profile_form_maps_navigation_editing_and_commands`，起点约 4381 行）；必要的流程断言放入 `tests/profile_reducer.rs`。

1. 修改既有表单测试中的 Ctrl+S、Ctrl+Enter、Ctrl+Shift+Enter 期望为 `None`。
2. 更新 URL、Kind、循环字段和按钮旧 Enter 期望，覆盖第 1 节矩阵。按钮枚举清理前可先保留尚待替换的旧变体，下一步同步完成，不为临时编译状态建提交。
3. 给现有表单测试增加 VisibleObjects 的 Space → ProfileOpenScope 断言，及文本 Space 仍输入空格的断言。
4. 在所有相关焦点上检查 Shift+Enter 不保存，且 Ctrl/Alt 组合不会穿过默认 Enter 分支。保留 Ctrl+T、导航、撤销、Scope/删除确认已有断言。

可直接使用现有 `key`/`ctrl`/`profile` helper，核心期望形状为：

```rust
app.profile_manager.as_mut().unwrap().selected_field = ProfileField::VisibleObjects;
assert_eq!(
    keymap.map(key(KeyCode::Char(' ')), &app),
    Some(Action::ProfileOpenScope)
);
assert_eq!(
    keymap.map(key(KeyCode::Enter), &app),
    Some(Action::ProfileSave { connect: true })
);
assert_eq!(keymap.map(ctrl('s'), &app), None);
```

**定向验证：** `cargo test --test keymap profile_form`。若先跑变更后的期望，应因旧保存映射不符而失败；记录实际结果，环境构建失败不能当成行为失败。此阶段无需新建镜像实现的纯文案测试。

## 3. 步骤 B：统一模型、键盘、鼠标和可见按钮

**修改文件：**

- `src/model/profile_manager.rs`：ProfileField（约 92 行）和六组字段数组（约 2558-2669 行）。
- `src/input/keymap.rs`：map_profile_manager、map_profile_form（约 3777-3929 行）。
- `src/ui/mod.rs`：ProfileButton（约 98 行）。
- `src/input/mouse.rs`：profile_button_action（约 1288 行）。
- `src/ui/profiles.rs`：render_form、field_value、field_label、is_button_field。
- `tests/profile_draft.rs`、`tests/keymap.rs`、`tests/ui_render.rs` 中受枚举变化影响的现有断言。

1. 删除 `ProfileField::SaveAndConnect`，保留唯一的 `Save`。六组数组删去第二个保存项，长度依次为 PostgreSQL 18、Oracle 17、MySQL 17、SQLite file 11、SQLite memory 10、Redis 16；以当前代码实际数组为准逐项核对。MariaDB/SQLServer 使用共享数组时也要覆盖导航行为。
2. 删除 `ProfileButton::SaveAndConnect`，将鼠标 `ProfileButton::Save` 映射为 `Action::ProfileSave { connect: true }`。
3. render_form 的按钮元组只保留 Test、Save、Cancel；selected_field 分别对应同名字段。删除旧枚举的 field_value/field_label/is_button_field 分支。
4. map_profile_manager 的控制键 match 移除 Char('s') 和 Enter 保存分支，保留 Ctrl+T 和文本编辑/历史分支。
5. 在 map_profile_form 的初始导航 match 之后、所有字段类别分支之前统一处理 Enter：

```rust
if code == KeyCode::Enter {
    if !event.modifiers.is_empty() {
        return None;
    }
    return Some(match field {
        ProfileField::Test => Action::ProfileTest,
        ProfileField::Cancel => Action::CloseProfileManager,
        _ => Action::ProfileSave { connect: true },
    });
}
```

6. 移除下方已不可达的字段 Enter 分支：URL 不再仅 ProfileCommitUrl，VisibleObjects 仅 Space 打开，cycle/toggle 去除 Enter，按钮匹配只处理 Space；唯一 Save 返回 connect:true。保留 `ProfileCommitUrl` action 本身，因为提交/切换相关内部路径可能仍使用它。
7. 将 `tests/profile_draft.rs` 的字段期望统一为一个 Save；检查正向/反向导航经过 Test → Save → Cancel，无隐藏停靠点。复用驱动参数化测试以覆盖六组数组。

**定向验证：** `cargo test --test keymap profile_form`，`cargo test --test profile_draft`。两个目标通过后不要无变化重复执行。

**完成条件：** 三种 UI 保存入口——默认 Enter、Save 焦点 Space、鼠标 Save——全部产生 connect:true；旧快捷键与隐藏字段均消失。

## 4. 步骤 C：上下文提示、共享帮助和文档一致化

**修改文件：** `src/ui/profiles.rs::form_hints`（约 1004 行）、`src/help.rs`（ProfileForm 条目及其单元测试）、`docs/keybindings.md` 的 Profile Manager/Form、`docs/ui-dialog-guidelines.md` 的 Profile manager 行。

1. form_hints 共用一份 Enter 动作描述决定逻辑，避免宽窄分支各自保留旧语义：Test 焦点描述 test，Cancel 描述 cancel，其余描述 save。KeyEvent 必须是 `Enter + NONE`。
2. VisibleObjects 增加可点击 `Space select`；cycle/toggle 增加 Space change/toggle。不能继续发 Enter 事件模拟字段局部操作。
3. 宽窄提示优先顺序：当前 Enter 行为、当前字段 Space 操作（如有）、Esc、Ctrl+T，再按可用空间展示导航提示。不要让窄屏的 Space 入口被旧全局提示挤掉。沿用现有 shortcut_hints 布局，不重做通用弹窗组件。
4. 宽屏保留文本 Tab/Shift+Tab 和驱动/循环字段左右键说明。分类/驱动不显示无效 Space 操作。提示区高度保持稳定。
5. 共享帮助更新 ProfileFormActivate 为 Space 的选项/按钮激活说明；ProfileFormSave 改为 Enter，说明“save and connect; focused buttons activate”。由于该目录按页面而非字段分组，不能简单声称任何焦点 Enter 都保存。同步目录的固定序列断言。
6. 文档表说明 Enter 默认保存并连接、Test/Cancel 焦点例外、Space 打开 Visible objects/切换选项/激活按钮，文本空格仍输入。删除 Ctrl-s/Ctrl-Enter，Test 的 F5 更正为代码实际支持的 Ctrl-t。
7. 弹窗规范的旧双保存按钮和快捷键行替换为 Test/Save/Cancel、新 Enter/Space 约定。历史分析/计划文档不批量改写。

**验证：** `cargo test --lib help::tests`；检查过滤器实际运行测试数，零测试不视为验证通过。

## 5. 步骤 D：验证真实输入链与渲染命中链

**测试文件：** `tests/keymap.rs` 或 `tests/profile_reducer.rs`（选一个承载跨层流程，避免重复）；`tests/ui_render.rs`；仅在缺少既有覆盖时修改 `tests/mouse.rs`。

1. 使用现有 SQLite 内存 profile fixture，以 ProfileStartEdit 打开有效连接草稿；焦点移到 VisibleObjects，真实 Keymap 的 Space action 交给 App::update，确认页面为 Scope，命令中没有 SaveProfile。Scope Enter 返回 Form，不保存。
2. 将普通表单焦点或 URL 焦点上的 Enter action 交给 App::update，确认发出 `Command::SaveProfile { connect: true, .. }`。对于 URL，采用与 fixture 一致的有效 URL，断言提交内容来自更新后的 URL，而不是陈旧字段。
3. 复用 reducer 保存完成测试，确认 ProfileSaved(connect:true) 后关闭编辑器并发起连接。对于无效 URL/草稿和忙状态，使用现有测试或补充一项断言证明不绕过校验、不重复发出保存命令；保存失败保留草稿。
4. 更新现有 ui_render 的 Save & Connect 与旧快捷键断言（起点约 7253、7555 行）。用渲染产生的 HitRegion 计数确认只有一个 Save 按钮，不能仅统计页面字符串 `Save`（提示也会含 save）。
5. 用现有 mouse helper 点击该 Save HitRegion，确认 connect:true；禁止仅手工构造一个与渲染无关的按钮目标就宣称整链通过。
6. 沿用现有正常/紧凑尺寸测试覆盖宽窄 form_hints 两分支：VisibleObjects 能看到 Space 提示，Cancel 的 Enter 提示确为 cancel，Save & Connect/Ctrl+S/Ctrl+Enter 文案不再出现。新增变体优先参数化现有测试。
7. 复用 busy 渲染约束，确保禁用按钮不生成可点击命中区。Scope loading 的 Space 禁用沿用既有测试。

**定向验证命令：**

```sh
cargo test --test keymap profile
cargo test --test profile_reducer --test profile_runtime --test profile_lifecycle
cargo test --test ui_render profile
cargo test --test mouse profile
```

若新跨层测试名称不含 profile，使用确切名称运行。若 mouse 目标没有匹配测试，则运行实际新增/修改测试名或完整 mouse 目标。记录真实运行数与结果，遇到失败定位相关文件并修复，不机械全量重跑。

## 6. 步骤 E：一次完整验证与 Luna 收尾审查

功能齐备后执行项目要求：

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
git diff --check
```

预期退出码全部为 0；fmt 若失败，执行 cargo fmt 后检查仅预期文件发生变化。编译/测试失败自行修复，并按改动影响定向验证；仅当相关代码或环境发生变化才重跑相应检查。

将命令、退出结果、实际代码版本、修改文件/工作区状态、环境及外部数据库测试跳过情况追加到任务目录 validation.md。不能将当前规划命令当成已执行记录。

**Luna 收尾审查清单：**

- [ ] 表单只剩一个 Save，所有 UI 路径均 connect:true；内部仅保存能力未被误删。
- [ ] Space 能打开有效草稿的 Visible objects；已有 URL/草稿错误定位仍有效。
- [ ] 普通 Enter 为默认提交，Test/Cancel 焦点激活自身；修饰键不泄漏到默认提交。
- [ ] 六组导航列表及所有枚举 match 清理完整，没有隐藏 Save 停靠点。
- [ ] 新建/编辑一致；Scope、删除确认、文本空格、Ctrl+T 和编辑快捷键未回归。
- [ ] URL 在保存路径解析，错误/忙状态不发出错误或重复保存命令。
- [ ] 渲染按钮和真实点击动作一致；宽窄提示、可点击提示 KeyEvent、共享帮助和文档一致。
- [ ] 当前版本所需定向与全量检查有实际证据；没有把可选数据库跳过写成实测通过。

人工/PTY 宽窄屏操作是补充检查，不是本次用户额外强制项。环境受限最多一次有针对性的修复重试，之后由 Luna 审查决定补充证据或记录限制；不得无限重试、无限 progress。

## 7. 提交、完成与下一步

在获得实现/提交阶段授权后，由 Luna 根据实际 diff 只暂存本任务文件，按逻辑闭环提交。建议一次原子提交包含行为、测试、帮助与文档，避免把破坏枚举编译的中间步骤提交。建议提交信息：`fix(profiles): unify save action and form shortcuts`。不要 git add 整个可能含用户改动的工作区。

本计划不包含自动发布、版本修改或配置预设修改。合并目标为 main，具体提交/合并操作按后续阶段指令执行。

**计划完成后的首个动作：** Luna/调度器确定工作流与任务分支名称，进入实现阶段后从实际起点与 diff 核对开始，执行步骤 A–D，完成同一个端到端闭环，再进行步骤 E 审查验证。Astra 在本次 plan 阶段到此结束，不代替 Luna 开始实现。
