# Relation Data 上下行导航修复实施计划

**阶段：** plan（Astra）；实施、审查、纠偏、提交合并由 Luna 完成。

**Goal:** 恢复 Relation Data 浏览模式的 `j/k` 和 Up/Down 行导航，并以真实按键到选中状态测试防止再次回归。

**Architecture:** 删除 Relation Browse 分支中错误的权限导航提前返回，复用既有 `Keymap::map → map_relation → map_results → GridMove → App::update` 路径。不增加动作或导航抽象。

**Tech Stack:** Rust、crossterm KeyEvent、现有单元与集成测试。

## 1. 基线、范围与决策

- 工作区：`/Users/yelog/workspace/tui/lazydb`。
- 起点：`fef2489f75e3fc3e81a0f47a07062d16c2f04980`；目标分支：`main`。
- plan 阶段重新执行 `git status --short; git rev-parse HEAD; git diff --stat`，退出 0；HEAD 与起点一致，工作区状态及 diff 均为空。
- `checkpoint.json` 仍不存在；本计划依据当前源码分析及用户要求，不依赖历史 next。
- 不存在需要迁移到新 worktree 的未提交行为。若实施时出现本地改动，Luna 应先区分其归属，不清空、stash 或提交整个原工作区。
- 工作流和任务分支由 Luna 自动命名。本阶段只写任务目录，业务代码和 Git index 保持未改动。

根因见同目录 `analysis.md`：`src/input/keymap.rs:1788–1793` 在 Relation Browse 中错误生成 `MovePrincipalPermission(±1)`；`src/app.rs:4453–4462` 仅处理 PrincipalDdl，因此该动作无效果。删除两段错误提前返回，比重复增加 GridMove 映射更符合现有结构。

权限 Overview 缺失同类导航是相邻发现，独立于本任务；不将这六行迁移到权限分支。本次仅恢复 Relation 已有快捷键契约。

## 2. 唯一端到端可验收单元：Relation Data 行导航

### 涉及文件

| 文件 | 修改/核对内容 |
|---|---|
| `src/input/keymap.rs` | 删除错误提前返回；复用 tests 中 relation_app，补齐四键和输入上下文回归 |
| `tests/relation_tabs.rs` | 多行 preview 下通过 Keymap 和 App::update 验证真实选中状态 |
| `src/help.rs`、`docs/keybindings.md` | 核对 relation 移动已有契约；恢复既有行为无需修改，若实际改变快捷键则按 CONTRIBUTING 同步 |

### 步骤 A：确认基线上的回归

在实施工作区执行：

```sh
cargo test --lib relation_cell_copy_does_not_consume_followup_navigation
```

已有测试位于 `src/input/keymap.rs:5256`。预期失败：`y` 后按 `j` 实际得到 MovePrincipalPermission(1)，期望 GridMove。必须记录实际执行结果；若因编译/环境中断，只能记为验证未到达断言，不能当成已复现。

### 步骤 B：建立必要的行为回归覆盖

1. 在 keymap 单元测试中覆盖以下输入矩阵，可用循环避免重复测试函数：
   - Relation Data，Results 焦点，无 query 输入焦点。
   - `edit=None` 与 `RelationGridMode::Browse`。
   - `j/Down → GridMove { rows: 1, columns: 0 }`。
   - `k/Up → GridMove { rows: -1, columns: 0 }`。
   - Press 与 Repeat 都可导航；Release 被忽略。
2. 在 `tests/relation_tabs.rs` 新增 `relation_data_navigation_keys_move_selected_row`（建议名称），沿用现有 `relation_grid_actions_update_relation_grid_using_preview_dimensions` 的 snapshot 构造方式：
   - 至少三行、两列已加载 RelationPreview，初始行 1、列 1。
   - 调用 `Keymap::map` 获取动作，再交给 `App::update`，不直接发送 GridMove。
   - 按 `j, k, Down, Up`，依次验证行索引 `2, 1, 2, 1`，列始终为 1。
   - 连续向上到首行后再向上仍为 0；连续向下到末行后再向下仍为 2。
   - 对产生的 update commands 验证没有数据库副作用。
3. 对上下文保护优先运行已有测试，缺口才补充：
   - VisualLine 的 j/k 继续扩展选中行。
   - EditCell 中 j/k 继续插入字符。
   - DataQuery 输入聚焦时 j/k 继续属于输入处理，不变成网格导航。
   - Busy 不接受行导航。

新增测试应在修复前失败于实际路由错误；上下文保护用例应在基线已通过。记录定向测试名称和结果。

### 步骤 C：最小业务修复

在 `Keymap::map` 的 `is_relation_data_focus(app)` / `relation_grid_is_browse(app)` 分支内删除且仅删除下列两段：

```rust
if matches!(event.code, KeyCode::Char('j') | KeyCode::Down) {
    return Some(Action::MovePrincipalPermission(1));
}
if matches!(event.code, KeyCode::Char('k') | KeyCode::Up) {
    return Some(Action::MovePrincipalPermission(-1));
}
```

保留随后 d/y/Y 处理，导航自然回落到已有 map_relation/map_results。无需修改 app reducer、runtime 批处理、数据适配器或配置。

### 步骤 D：单元闭环验证

```sh
cargo test --lib input::keymap::tests
cargo test --test relation_tabs
```

预期：原有 copy 后导航测试、新增四键测试、真实行移动测试和相关上下文用例全部通过。定向结果通过后，本业务单元完成，不人为拆成需要反复 resume 的阶段。

### 验收标准

- 普通 Relation Data 浏览下 j/k 与 Up/Down 均正确移动行。
- edit=None 与 Browse 均覆盖；至少一个多行测试完整经过 Keymap 和 reducer。
- 列索引稳定，首尾边界正确；复制后导航正常。
- EditCell、Query 输入、VisualLine、Busy 保持各自上下文语义。
- Relation 路由不再生成 MovePrincipalPermission。
- 不产生数据库写入或导航外的异步命令。

## 3. 功能齐备后的项目验证

### 验证要求分类

- **用户需求验收：** Relation Data 的 j/k 恢复上下行移动；同源 Up/Down 问题一起修复。以本计划的按键到状态回归测试作为自动化证据，无需用户执行人工验收。
- **本修复的定向验证：** 步骤 A 的已有回归复现、步骤 B–D 的四键/多行/上下文测试，是实现正确性证据；预期结果不是已经执行的结果。
- **项目强制门禁：** 下列 CONTRIBUTING 规定的 fmt、clippy、全量测试。失败必须由 Luna 分析处理并如实记录，不用补充手工测试替代。
- **补充建议：** 手工 SQLite/PTY 导航观察，不新增为必需门禁。外部数据库实测依既有环境配置和项目约定执行，本修复不引入新的数据库服务要求。

依据 `CONTRIBUTING.md` 执行一次最终检查：

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

- 不额外重复 cargo check；只有相关代码或环境变化才重跑对应检查。
- 将命令、退出码、相关文件、HEAD/工作区 diff 状态、环境记录到任务目录 `validation.md`，追加本次结果，保留 analyze 历史。
- 外部数据库测试按配置条件执行；跳过不是实际数据库验证通过。
- 手工 SQLite/PTY 导航是补充检查，不是用户强制验证。遇到环境限制最多一次有针对性的修复重试，然后记录限制，由 Luna 收尾审查决定是否需要补充证据。
- 普通编译和测试错误由 Luna 继续修复；确属基线或外部环境问题则提供可核实证据，不无限循环相同命令。

## 4. Luna 收尾与交付

1. 复核最终 diff：业务修复仅错误路由，新增测试确实覆盖原失效链路。
2. 确认快捷键目录与文档仍准确；不把修复扩展为权限工作区重构。
3. 根据真实验证结果决定是否需补证，完成必要纠偏。
4. 按自动任务流程命名、提交并交付到 main；只暂存本任务文件，不包含无关用户改动。建议提交主题：`fix(input): restore relation data row navigation`。
5. 回执只写当次调度明确指定的路径和 token；不复用 analyze 回执，不修改插件维护的 checkpoint/state。

## 5. 本阶段完成状态

计划已完成，无待用户裁决的实施取舍。下一步是 Luna 执行步骤 A，复现已有回归，再按 B–D 完成唯一业务单元和最终验证。

本阶段未执行 Cargo、未修改业务代码，也未创建工作树或分支。已依据正式 plan 调度读取 analysis.md 并调用 writing-plans 技能；精确预计修改范围写入同目录 change-scope.json。只读参考文件不列入修改范围；未提交依赖文件为空。不额外创建 docs/plans 文档。

本轮完成回执使用 `plan-c8d56eaa-1595-4860-8e49-7091c6d6d1e9.json`，token 为 `c8d56eaa-1595-4860-8e49-7091c6d6d1e9`；在计划、修改范围与记录检查完成之后最后写入，保留全部历史回执。
