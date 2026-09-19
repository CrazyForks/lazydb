# Table Editor Modified Cell Highlights Implementation Plan

> **执行者：Luna。** 按下述单一端到端业务单元实施、验证、审查和提交。Astra 本阶段只生成计划；不启动子 Agent。用户指定的模型分工优先于通用技能的执行建议。

**Goal:** 编辑已有表并确认列属性修改后，在 COLUMNS 中用 relation data 的修改背景色标记对应单元格，恢复原值时清除标记。

**Architecture:** 保留现有 baseline、ColumnDraft 和详情编辑事务，在模型中增加无状态字段差异查询。通过 existing_name 匹配原定义，在现有四个列表 Cell 上叠加 theme.row_updated；新增/删除行和布局保持既有语义。

**Tech Stack:** Rust 2024、ratatui 0.30.2、现有 TableDraft/CatalogEditorState、TestBackend 集成测试。

---

## 0. 执行上下文

- 原工作空间：`/Users/yelog/workspace/tui/lazydb`；目标 `main`。
- 用户指定起点：`16b32bfc29d11e5f503ec51feea4fa8d381e988a`。
- 分析收尾时外部提交 `303058ce01b77955379d7e2bf0bdebb3493388d1` 仅提交 8 份既有计划文档；与起点相比，相关 src/tests/Cargo/CI 无变化。不要回滚该提交或将其归为本功能。
- 工作流名称、任务分支及工作树由后续 Luna/插件流程处理，勿提前改写 state.json 或 checkpoint.json。
- 分析：`.git/opencode-tasks/ses_f47c7137affeCG926Br17K0cqs/analysis.md`；验证记录：同目录 `validation.md`。这些位于原工作空间，不应误写到任务工作树的 `.git` 文件下。
- 开工先读实际 checkpoint（存在时）并看实际 diff，再执行未完成项；不要重复通读历史文档，不等待用户 resume。
- 本功能只有一个业务闭环。下面分步骤便于执行，不应把“查询接口完成”当成独立交付或停止点。

## 1. 已确认语义与非目标

1. NAME、TYPE、NULLABLE、COMMENT 分别比较，仅变更格着色。
2. 修改色复用 `theme.row_updated`，在选中/未选中状态都可见。行号、分隔线、未修改格保持原行色，左侧 `▌` 继续表示焦点。
3. 相对打开编辑器时 baseline 比较，而非相对上次确认或编辑历史；恢复原值自动清色。
4. 详情弹窗编辑只修改 session.draft，确认才替换 columns，取消保留之前确认的草稿及标记。
5. 原始列名匹配，不使用当前行下标；重命名/重排/插入列不能错配。
6. 缺 baseline 或缺匹配返回 false；Added/Removed 不使用字段修改色。
7. 字段字符串精确比较，复用 optional_string/optional_bool；不 trim、忽略大小写或进行数据库类型别名归一化。
8. 空注释和文本右侧空白仍需着色到 Cell 宽度。颜色以完整 Cell Style 设置，不能只给文本 Span 着色。
9. 默认值/Identity 没有列表展示格，不映射成其他字段变化；继续由现有变更摘要表达。
10. 不新增持久 dirty 集合、不扩展 DraftRowState、不修改 SQL/数据库适配器、relation data 选中逻辑或主题定义。

## 2. 文件与现有定位

**Modify:**
- `src/model/catalog_editor.rs`：现有 `impl ColumnDraft`，约 2635 行；使用附近 `optional_string` / `optional_bool`。
- `src/ui/catalog_editor.rs`：`render_table`，约 1845–1925 行，行样式与四个内容 Cell。

**Test:**
- `tests/catalog_editor_state.rs`：字段身份与确认/取消的非平凡状态行为。
- `tests/ui_render.rs`：实际 buffer 背景、选中优先级、空值、行状态和紧凑窗口回归。

**Read as needed:**
- `src/ui/data_grid.rs::data_cell_style`：现有修改背景使用 theme.row_updated。
- `src/ui/catalog_editor.rs::table_column_constraints`：3 字符行号 + 4 条分隔线 + 四个内容列。
- `tests/ui_render.rs::table_editor_marks_removed_columns_and_exposes_restore_action`：已有 buffer/hit_region 颜色断言模式。
- `tests/catalog_editor_state.rs::table_column_edit_session_is_atomic_across_confirm_and_cancel`、`table_column_reordering_preserves_identity_and_tracks_order_changes`：已有状态流程。

## 3. 单元 A：字段差异与实际高亮形成闭环

### Step 1 — 添加有意义的失败回归

复用状态测试中 TableDefinition 构造方式，建立两列已有表：`id integer NOT NULL` 与 `name text NULL`，第二列原注释为 `display name`。UI fixture 用 `TableDraft::from_definition`，CatalogEditorState.mode 为 Edit、page 为 Form、baseline 为 Some(Table(definition))、draft 为 Some(Table(draft))，overlay 为 CatalogEditor。不要使用创建表草稿伪装已有列，也不要只测纯颜色 helper。

新增测试建议名称与断言：

| 测试 | 动作 | 核心断言 |
| --- | --- | --- |
| `table_editor_highlights_only_modified_cells` | 用四个字段的参数表分别编辑第二列并确认，循环 Columns 焦点/General 焦点 | 仅目标内容格 bg=row_updated；其余内容格、行号和分隔线保持 selection/surface |
| `table_editor_highlights_empty_comment_and_restores_baseline` | 同时改 name/type、清空 comment；再次进入详情恢复 type/comment | 空 comment 区域含最后一个空白格均高亮；恢复的格清色，name 仍高亮 |
| `table_editor_preserves_modified_cells_across_cancel_and_reorder` | 已确认 name 修改，再打开详情改 type 后取消，随后重排列 | 已确认 name 标记跟随原列；取消的 type 不高亮；未改动列无标记 |

每次编辑通过 `begin_edit_selected_column()` → 修改 `column_editor.as_mut().unwrap().draft` → `confirm_column_details()`，取消用 `cancel_column_details()`。避免只直接改 columns 而漏测原子流程。

UI 坐标：从 `HitTarget::CatalogEditorTableColumn(index)` 找行 Rect；读取该行或 header 的 `│` 分隔位置得到四格范围，以独特字段内容/NAME、TYPE 等标题核对范围，确保不依赖易变的 popup 绝对坐标。固定 fixture 不含 `│`。可写本测试文件内的小 helper 返回四个 Range<u16>，并断言恰有四格；不要导出生产布局 helper。

针对选中行和非选中行分别渲染。range 首尾及中间填充区都断言背景；同时验证相邻格与分隔线，才能捕获整行误染与只染文字的问题。

Run:

```sh
cargo test --test ui_render table_editor_highlights_
cargo test --test ui_render table_editor_preserves_modified_cells_
```

Expected: 当前实现因目标 Cell 背景仍为 selection/surface 而 FAIL。记录实际失败断言；若先遇到测试构造编译问题，先修正测试再确认语义失败。

### Step 2 — 增加最小无状态查询

在现有 `impl ColumnDraft` 内增加以下方法，使用现有 TableColumnField、DraftRowState、optional_string、optional_bool；无需添加依赖或结构体字段。

```rust
pub fn field_changed_against(
    &self,
    field: TableColumnField,
    baseline: Option<&crate::db::catalog_mutation::TableDefinition>,
) -> bool {
    if !matches!(&self.state, DraftRowState::Existing { .. }) {
        return false;
    }
    let original_name = self.existing_name.as_deref().unwrap_or(self.name.value());
    let Some(before) = baseline.and_then(|table| {
        table.columns.iter().find(|column| column.name == original_name)
    }) else {
        return false;
    };
    match field {
        TableColumnField::Name => self.name.value() != before.name,
        TableColumnField::Type => self.native_type.value() != before.native_type,
        TableColumnField::Default => {
            self.default_expression.value() != optional_string(&before.default_expression)
        }
        TableColumnField::Comment => self.comment.value() != optional_string(&before.comment),
        TableColumnField::Nullable => self.nullable != before.nullable,
        TableColumnField::Identity => self.identity != optional_bool(&before.identity),
    }
}
```

该接口穷尽既有枚举；UI 只查询四个展示字段。每次最多为可见行做四次短查找，先保持实现简单，不引入缓存。若实际 profiling 发现大型表热点，再另行优化，非本任务前置条件。

### Step 3 — 给四个实际 Cell 叠加背景

`render_table` 已有 `baseline: Option<&TableDefinition>` 局部变量和 `row_style`。在构造该行 Cell 前加入局部闭包：

```rust
let field_style = |field| {
    if column.field_changed_against(field, baseline) {
        Style::new().bg(theme.row_updated)
    } else {
        Style::new()
    }
};
```

将现有 Row 构造替换为下列内容（row_number、separator、name 等保持既有值）：

```rust
Row::new([
    Cell::from(row_number),
    separator.clone(),
    Cell::from(name).style(field_style(TableColumnField::Name)),
    separator.clone(),
    Cell::from(native_type).style(field_style(TableColumnField::Type)),
    separator.clone(),
    Cell::from(truncate_cells(
        nullable.to_owned(),
        usize::from(nullable_width),
    ))
    .style(field_style(TableColumnField::Nullable)),
    separator,
    Cell::from(comment).style(field_style(TableColumnField::Comment)),
])
.style(row_style)
```

保持返回 `(index, Row)` 的现有结构、空 row_highlight_style、约束、命中区域和分隔线样式。修改背景自然覆盖普通行底色；查询只处理 Existing，Added/Removed 整行背景继续生效。

### Step 4 — 运行闭环测试

```sh
cargo test --test ui_render table_editor_
```

Expected: 新测试与既有 table_editor_ 测试全部 PASS。若背景未覆盖空白，检查 Cell 级样式和实际 buffer；优先修复局部渲染，避免引入 relation data 的后绘制选中流程。

### Step 5 — 补齐身份、元数据与行状态边界

在 `tests/catalog_editor_state.rs` 添加参数化或少量组合测试，覆盖以下独立风险，不按实现每个分支机械复制测试：

- 初始无变化；只移动 TextInput 光标不产生字段修改。
- rename 后再改 type/comment，确认仍匹配 existing_name；交换列位置和插入新列后不按数组下标误配。
- Supported(None)、Supported(Some(""))、Unsupported、Unavailable（按实际枚举参数构造）在初始投影为空时不误标；非空注释改为空时为 true。
- 缺 baseline、无匹配列为 false；Added/Removed 即使字段不同也为 false。
- 重新编辑恢复最初字段值后 false；Default/Identity 查询与其原始值一致，且不应改变 Name/Type 查询结果。

在 `tests/ui_render.rs` 复用 fixture 补充：

- 修改后删除：删除整行色优先；恢复列后原差异重新出现。已有表至少两列以满足“不能删除最后有效列”。
- baseline 存在时 Added 行仍整行 inserted；创建模式 baseline=None 无假修改色。
- 用 `100x30` 与 `60x16` 渲染修改行；足够多列时选最后修改列验证滚动后可见。使用 Unicode 长值验证截断不会改变着色位置。不可见、宽度为零的格不要求凭空显示背景。

Run:

```sh
cargo test --test catalog_editor_state
cargo test --test ui_render table_editor_
```

Expected: PASS。此轮包含新增边界，允许重跑上述定向检查；未变代码不要无理由重复运行。除非为解决真实缺陷不得扩大到 reducer/数据库实现。

## 4. 单元完成后的验证与 Luna 收尾

### 验证分级（不得混淆）

| 类别 | 内容 | 完成规则 |
| --- | --- | --- |
| 用户需求验收 | 已确认修改在对应单元格显示修改背景，符合第 1 节的字段粒度与恢复/取消语义 | 必须实现，以实际 buffer 与状态回归测试提供证据；未完成继续实施 |
| 项目强制门禁 | `.github/workflows/ci.yml:81–83` 的 Rust fmt、clippy、all-targets/all-features 测试 | 功能齐备后执行并记录；代码失败自行修复，环境限制如实记录，不以补充检查替代或宣称通过 |
| 本计划的定向回归 | Step 1–5 的 UI/模型自动测试，含空注释、身份匹配、行状态与紧凑布局 | 属于实现验证策略，不声称每个测试名称或尺寸是用户原始要求；可按等效覆盖合并用例 |
| 补充建议验证 | 真人/PTY 预览、截图、额外真实数据库手工操作 | 不自动升级为必需门禁；现有 CI 数据库作业仍按项目流程运行，不能把跳过当成通过 |

环境受限检查至多一次有针对性的修复重试，随后由 Luna 收尾审查决定补证或记录限制；强制门禁未通过应明确标记未通过，补充检查不可执行不单独阻塞功能交付。

### Step 6 — 统一格式与项目检查

实现期间可运行 `cargo fmt --all` 整理格式，但提交前检查实际 diff，仅保留本功能必要变更。功能齐备后按 CI 工具链执行一次：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

Expected: 全部退出 0。本机分析阶段实际 rustc 为 1.94.1；若 1.94.0 未安装，明确记录安装或替代工具链及其理由，不把默认 cargo 结果声称为精确 CI 结果。编译/测试代码失败自行修复，仅对受影响检查重跑。

将每个实际命令、退出码、测试数、HEAD、工作区状态、环境/feature 写入原任务目录 `validation.md`。真实数据库测试可能由环境变量控制，记录实际跳过情况，不能将跳过说成完成真实数据库验证。

人工/PTY 预览属于补充检查，非此需求指定必过项。环境受限检查最多一次定向修复重试，再由 Luna 收尾审查决定补证或记录限制；不能以此无限 progress。

### Step 7 — Luna 审查与提交

审查实际 diff，重点确认：

- 只修改两个生产文件及必要测试，没有新 dirty 状态或数据库写入逻辑。
- 原始列名身份稳定，模型比较值而非光标/历史/截断文本。
- 已选中变更格可见，空注释背景覆盖全格，分隔线不染色。
- 确认/取消/恢复值/重排/新增删除均有有意义的证据。
- 无旧验证结果冒充新代码结果；未触碰其他工作流产物或插件状态。

在已由工作流建立的任务分支上提交一个完整业务闭环，建议提交信息：

```sh
git add src/model/catalog_editor.rs src/ui/catalog_editor.rs tests/catalog_editor_state.rs tests/ui_render.rs
git commit -m "fix(catalog-editor): highlight modified column cells"
```

计划文档如由工作流要求纳入该任务分支，显式单独加入；不要使用 `git add .` 带入其他任务文档。提交、合并遵循当时的工作流阶段要求，由 Luna 执行，本计划不授权跨阶段提前合并。

## 5. 完成判据与交接

只有实现、定向测试、项目检查及 Luna 审查完成，才能报告此功能闭环完成。当前交付仅为计划，尚未编写业务实现或新增测试。下一步由工作流交给 Luna 自动命名并在任务分支实施单元 A，不需要用户选择执行模式或反复 resume。
