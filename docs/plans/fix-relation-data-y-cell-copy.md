# Relation Data 单键复制 Implementation Plan

**Goal:** Relation Data 浏览模式按一次 `y` 立即复制当前单元格，同时保留 `yy` / `p` 的结构化行复制粘贴。

**Architecture:** 复用 `Action::CopyGridCell` 和现有剪贴板命令；首次 `y` 发出复制动作并保留可选的第二次 `y`。在通用 pending 提示菜单处理之前清除 RelationYank 的非匹配续键，让它们进入正常路由。为 Relation 浏览提供独立帮助条目，避免 SQL 自定义快捷键显示污染。

**Tech Stack:** Rust 2024、crossterm KeyEvent、现有 App/Action/Command 架构、Cargo 测试。

---

## 执行约束与前置条件

- 基线：`1e317fe1d3569e4f440ee6fefa07b4010e6c08e2`；目标分支 `main`。
- 分析文档：同目录 `analysis.md`。本计划只产出实施指令，不修改业务代码。
- 插件管理任务命名、分支、worktree、状态与后续阶段；执行者应在插件分配的工作空间中实现。不自行创建 worktree、不修改 state.json、不启动子 Agent。
- 不在本计划阶段提交代码。实现完成后的提交交由工作流相应阶段处理。
- 本次不更改数据库驱动、剪贴板后端、行寄存器实现，也不全面开放 Relation 的 SQL 自定义绑定。

## 相对分析文档的两项细化决策

### 非匹配续键必须在提示菜单之前处理

`src/input/keymap.rs:1026–1073` 的通用 pending 菜单会先截获 Up、Down、Enter，再进入 `map_pending`。仅修改 1270–1280 的尾部 fallback 无法解决 `y,Down` 被吞的问题。

采用前置清理：在上述 pending 有效性判断之前，只对 `Pending::RelationYank` 判断当前按键是否是既有合法续键（无修饰键的 `Char('y')`），以及当前是否仍为 Relation Data Browse。任一条件不满足就调用 `clear_pending()`，随后继续同次事件的正常路由。这样无须更改通用 pending 的无效续键逻辑，也不会影响 `dd`、leader 或其他提示菜单。

这是一个“已完成单键动作的可选后续”，因此 RelationYank 的箭头和 Enter 应恢复普通表格语义，不再作为提示菜单的导航/确认键。其他 pending 仍可使用原菜单交互。Esc 继续走既有全局取消逻辑。

### 独立帮助 ID 避免错误显示自定义绑定

`src/help.rs:3205–3239` 的 `configured_sequence()` 仅根据帮助 ID 映射 `results-copy-cell`，不接收当前 context；而该自定义绑定在 Relation 上被 mapper 排除。因此直接给 ResultsCopyCell 添加 RelationDataBrowse context 会显示一个实际无效的自定义键。

新增 `HelpShortcutId::RelationCopyCell`，固定显示 `y`，仅属于 RelationDataBrowse；在 App 帮助执行分派中复用 `Action::CopyGridCell`。保留 SQL 的 ResultsCopyCell 原有配置行为。此改动比修改整个帮助配置 API 或扩大快捷键配置作用域更局部。

## Task 1：固定单键与双键的输入契约

**Files**
- Modify/Test: `src/input/keymap.rs`（现有测试模块）
- Modify/Test: `tests/keymap.rs`

1. 更新 `sequence_state_covers_relation_and_record_prefixes`、`relation_data_dd_and_yy_are_pending_sequences`、`relation_browse_yy_maps_the_catalog_yank_row_binding` 中第一次 `y` 的期望为 `Some(Action::CopyGridCell)`。保留 pending prefix 与第二次 `y` 返回 RelationYank 的断言；按新语义重命名不再准确的测试名称。
2. 增加一个表驱动续键回归，使用已有 `relation_app(RelationGridMode::Browse)` 和 `key` 辅助函数。每个 case 使用新的 Keymap：先 `y`，再测试 `j` / Down、`k` / Up、`l` / Right、`p`、`Y`。断言分别返回原有 GridMove、RelationPaste、CopyGridRow 动作，而不是 None 或 ExecuteHelpShortcut。随后断言没有 RelationYank 残留。
3. 覆盖 `y,Space,Y`：第二步正常开启 leader，第三步返回带表头行复制。不要把新 leader 的存在误判为旧 RelationYank 残留。
4. 使用测试模块可见的 pending.started_at 和显式 Instant 参数验证超时，不使用 sleep。验证超时后的 `y` 为新的 CopyGridCell；沿用已有焦点/tab 失效测试模式，确认旧 `yy` 不跨上下文执行。
5. 补充模式隔离断言：EditCell 的 `y` 返回 RelationEditInsert('y')；VisualLine 返回 RelationYankSelected；WHERE/ORDER BY 输入不复制；保持 SQL `y` 和 Relation `dd` 的原断言。

运行：

```sh
cargo test --lib input::keymap::tests
cargo test --test keymap
```

实现前预期新增复制/导航断言失败，原因是首次 `y` 返回 None 或续键被 pending 消费。确认失败原因对应本需求后进入 Task 2，不修补无关测试。

## Task 2：最小快捷键修复

**Files**
- Modify: `src/input/keymap.rs`

1. 在现有 `if self.pending.as_ref().is_some_and(|pending| pending_is_valid(...))` 通用分派之前加入以下逻辑；放在全局事件和既有专用模式处理之后、通用 pending 菜单之前：

```rust
if self.pending.as_ref().is_some_and(|pending| {
    pending.pending == Pending::RelationYank
        && (!is_relation_data_focus(app)
            || !relation_grid_is_browse(app)
            || !event.modifiers.is_empty()
            || event.code != KeyCode::Char('y'))
}) {
    self.clear_pending();
}
```

无修饰的第二次 `y` 仍由现有 `pending_is_valid` 检查超时、tab、focus、generation 后分派。不要在这里绕开有效性检查直接执行 RelationYank，也不要递归调用 `map()`。

2. 修改 Relation Browse 原有 `y` 分支：

```rust
KeyCode::Char('y') => {
    self.set_pending(Pending::RelationYank, app);
    return Some(Action::CopyGridCell);
}
```

3. 保留原有 `map_pending(RelationYank, 'y')`、VisualLine `y`、`Y` 与 `dd`。不把 Relation 无条件加入通用 SQL grid copy 分支。
4. 运行 Task 1 的两个测试目标，预期全部通过。若专用模式的早返回暴露残留 pending，按现有模式隔离机制局部清理并加入对应行为回归，不扩展成全局 pending 重构。

## Task 3：验证实际复制命令与当前数据

**Files**
- Modify/Test: `tests/relation_tabs.rs`
- Reference: `src/app.rs:1452–1467,21234–21276`
- Reference: `src/model/relation_edit.rs:210–215`

1. 复用该测试文件现有 RelationTab / 加载快照构造方式，构造两行两列不同值，选中非零行列。通过真实 `Keymap.map(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE), &app)` 取得动作，再调用 `app.update(action)`。
2. 匹配返回的 `Command::WriteClipboard(payload)`，断言精确文本为目标单元格内容，并断言没有数据库写命令。使用包含换行和超过网格可见宽度的文本，确保复制不是展示截断值。
3. 对 `edit=None` 的只读关系执行同样测试。对编辑会话把目标单元格 current 改成不同于原始 snapshot 的值，确认复制 current。
4. 同一 Keymap/App 链路继续第二次 `y`，断言 RelationYank 更新 edit.yank 中的完整结构化行，第二次动作不产生系统剪贴板写命令。验证第一次复制没有修改寄存器或数据；复用现有行粘贴测试机制验证 `p` 仍消费该寄存器。
5. 空快照场景断言不产生 WriteClipboard，且走已有“Nothing to copy”提示。空数据不需要真实剪贴板或数据库。

运行：

```sh
cargo test --test relation_tabs
```

预期全部通过。除非上述行为测试证明底层取值有问题，否则不改 App 的复制实现或 runtime 后端。

## Task 4：帮助、提示与文档一致

**Files**
- Modify/Test: `src/help.rs`
- Modify: `src/app.rs`（help shortcut 动作分派，仅新增 ID 对应动作）
- Modify: `docs/keybindings.md`

1. 在 HelpShortcutId 的 Relation 区域添加 `RelationCopyCell`，在快捷键 catalog 添加：

```rust
row!(RelationCopyCell, [RelationDataBrowse], "y", "copy selected cell"),
```

沿用 ResultsCopyCell 的可执行元数据风格；不将该 ID 映射到 `configured_sequence()` 的 results-copy-cell，保持固定 `y`。

2. 在 `src/app.rs` 的 HelpShortcutId 动作分派中，将两种单元格复制 ID 对应到同一动作：

```rust
Id::ResultsCopyCell | Id::RelationCopyCell => vec![Action::CopyGridCell],
```

3. 按编译器提示补全枚举穷尽匹配。为 RelationCopyCell 设置与单元格复制相符的既有 category/priority；若 Relation footer 有显式 ID 排名，加入该 ID 并让 `y` 比 `yy` 更易发现，保留 `yy` 行为条目。不要修改其他 context 的排序。
4. 更新 `relation_browse_uses_distinct_yy_yank_row_metadata`：同时存在 RelationCopyCell 的 `y` 与 RelationYankRow 的 `yy`；SQL ResultsCopyCell 仍不属于 Relation context。检查固定 footer 列表断言并只调整受影响的 Relation 列表。
5. 增加自定义 SQL results-copy-cell 后的帮助回归：SQL 显示配置键；RelationCopyCell 仍显示 `y`。验证 Browse 的帮助执行能发出 CopyGridCell，EditCell / VisualLine / busy context 不额外显示 Browse 复制项。
6. `docs/keybindings.md` Browse 表新增 `y | Copy current cell to clipboard`；替换“Relation 不使用 SQL Results 的 y”陈述，明确：`y` 即时复制，紧接的第二次 `y` 把行放入内部寄存器供 `p` 使用；`Y` 是系统 TSV 行复制。说明 `yy` 的首次按键同样产生单元格剪贴板复制。

运行：

```sh
cargo test --lib help::tests
cargo test --test keymap
```

预期帮助条目、配置显示、帮助动作与输入契约一致。不要为了保持旧快照而隐藏新增功能。

## Task 5：最终定向检查与交接

1. 执行以下复核命令，检查 diff 仅涉及上述修复、必要测试和快捷键文档，没有新依赖或驱动改动：

```sh
git diff --check
git diff --stat
git diff -- src/input/keymap.rs src/help.rs src/app.rs tests/keymap.rs tests/relation_tabs.rs docs/keybindings.md
```

预期 `git diff --check` 无输出且退出码为 0；人工确认 CopyGridCell 复用现有命令、非匹配续键清理位于提示菜单之前、帮助 ID 不绑定 SQL 自定义键。复核清单中的差异若与插件分配的基线不同，应以插件工作空间的实际任务 diff 为准。
2. 格式检查：

```sh
cargo fmt --check
```

若失败，运行项目常用格式化方式并再次检查。

3. 在最终代码状态上运行尚未覆盖或在后续改动后受影响的目标：

```sh
cargo test --lib input::keymap::tests
cargo test --lib help::tests
cargo test --test keymap
cargo test --test relation_tabs
```

不因节约时间省略失败目标；如果构建依赖或环境阻塞，记录真实错误，不宣称测试通过。不要求额外数据库集成测试，因为生产逻辑改动不触及数据库操作。

4. 可用交互环境下做一次手动验收：Relation Data 选择单元格按 `y` 并粘贴核对完整值；马上按方向键应正常移动；可编辑关系的 `yy` / `p` 仍保留行操作；SQL Result Set 的 `y` 保持原行为。无交互环境时明确记录未做手测，自动测试以生成的 WriteClipboard 命令为证据。
5. 交接报告注明改动文件、通过的测试和未执行项目，以及 `yy` 首键即时复制的兼容语义，由插件继续下一阶段。

## 验收清单

- [ ] 单次 `y` 即时生成目标单元格的剪贴板命令。
- [ ] 只读关系与编辑后的 current 值均正确复制。
- [ ] `yy` 内部寄存器与 `p` 能力保留。
- [ ] `y` 后的字母导航、方向键、其他动作和 leader 不被截获。
- [ ] 超时、focus/tab/context 切换不会执行过期行 yank。
- [ ] EditCell、VisualLine、WHERE/ORDER BY、SQL Results 行为正确。
- [ ] 帮助、footer 与文档准确；Relation 不显示无效 SQL 自定义复制键。
- [ ] 格式检查和定向测试通过，工作流状态由插件管理。

## 本阶段执行记录

已先读取指定 analysis.md，并按本阶段要求调用 writing-plans 技能，复核和完善本计划。计划覆盖逐项文件、实施步骤、复核方式、验证命令及验收标准；pending 菜单与自定义绑定显示的细化决策有对应代码证据。

未执行上述实施步骤或测试，未修改业务代码或 state.json，未创建分支/worktree，未启动子 Agent。用户已选择自动工作流继续实施，由插件安排下一阶段。本阶段完成后最后写入 `plan-f8897fed-e77a-4a60-89d0-bc4b73c05496.json` 回执。
