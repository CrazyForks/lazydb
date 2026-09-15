# Data Grid Selection Alignment Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行环境若未安装上述技能，按本文任务顺序实施，每个任务完成后执行对应验证。

**Goal:** 修复 RESULT SET 与 RELATION DATA 的选中背景右移一格问题，使文字、高亮、排序、单元格点击、列宽调整和滚动区域共享一致的终端坐标。

**Architecture:** 在共享 `src/ui/data_grid.rs` 中修正内容起点，明确禁用 Ratatui 隐式选择标记槽。随后用文件内的轻量布局结构统一可见列的几何信息，继续采用 Buffer 后处理实现语义前景色与选中背景组合，并用真实渲染测试验证几何与交互的一致性。

**Tech Stack:** Rust 1.94、Ratatui 0.30.2、ratatui-widgets 0.3.2、TestBackend、项目现有 UiState/HitTarget 测试设施。

---

## 已确认的根因与实施边界

- `src/ui/mod.rs::render_result_table` 和 `src/ui/relation.rs::render_relation_result_table` 共用 `data_grid::render`。
- `table_area = block.inner(area)` 已扣除外层边框。
- `grid_constraints` 实际布局为 `[行号][一格分隔符][数据列][一格分隔符][数据列]`，`column_spacing(0)`。
- Table 使用默认、无选中行的 TableState；默认 `HighlightSpacing::WhenSelected` 不保留选择标记槽。
- `data_start_x` 却计算 `area.x + 1 + number_width + 1`，比实际内容起点多一格。
- 高亮、排序点击、单元格点击、列宽调整和横向滚动条使用这个错误起点。
- 本计划的编辑样式策略是保持当前实际 Buffer 输出的优先级：普通选中行使用 selection 背景，当前单元格使用 accent 和 BOLD，NULL/Unsupported 保持语义前景色；不把清理无效 Table 配置变成另一项编辑样式改版。
- 执行前检查工作区。本次规划时存在用户修改 `src/app.rs`，以及两份其他未跟踪计划文件；提交时只暂存本任务文件。

## 几何契约

1. 所有横向坐标单位均为终端字符格，范围为左闭右开 `[x, right)`。
2. 行号起点为 `table_area.x`，宽度为 `number_width`。
3. 首个数据列起点为 `table_area.x + number_width + 1`。
4. 下一列起点为上一列内容右边界加一格分隔符。
5. 数据内容、高亮和单元格命中区域不含分隔符。
6. 表头排序覆盖列内容，列宽调整覆盖已实际绘制的列间分隔符。
7. 右侧竖向滚动条独占 gutter；数据和点击区域不得侵入。
8. 被截断的列不生成虚构的右边界调整手柄；末列右侧没有实际分隔符时同样不生成。

## Task 1：建立能够复现错位的真实渲染测试

**Files:**
- Modify/Test: `src/ui/data_grid.rs::tests`

**Step 1 — 增加测试夹具。**

沿用现有 `hit_regions` 的 Terminal/TestBackend 调用方式，新增同时返回 `Buffer` 克隆与 `UiState` 的夹具。参数包括 Rect、Block、ResultSet、DataGridState、列宽覆盖和排序投影；固定 ASCII 图标以降低字体因素。不要修改现有夹具的所有调用以制造无关差异。

**Step 2 — 添加首个失败用例。**

建议命名 `selected_cell_background_matches_rendered_content`。使用三列、每列宽 6、一行 `a/b/c`，选择中间列，足够宽的无边框区域，例如 `Rect::new(0, 0, 40, 6)`。

独立预期坐标：行号宽 3、首列起点 4、首个列间分隔符 x=10、中间列起点 x=11、右分隔符 x=17、数据行 y=1。

逐项断言：
- Buffer[(11, 1)] 的字符是 `b` 且背景为 accent。
- x=11..17 的所有内容格背景为 accent，且带 BOLD。
- x=10 和 x=17 的字符是 `│`，背景为 surface。
- 首列和第三列首字符背景为 selection。
- 行号不被 accent 覆盖。

测试不能调用 `data_start_x` 或新布局结构来计算期望值，否则会复制被测错误。

**Step 3 — 运行失败测试。**

```bash
cargo +1.94.0 test --lib ui::data_grid::tests::selected_cell_background_matches_rendered_content
```

预期：编译成功，首字符背景或分隔符背景断言失败。先解决夹具编译错误，再记录真实渲染失败。

## Task 2：修复起点并明确高亮职责

**Files:**
- Modify: `src/ui/data_grid.rs::data_start_x`
- Modify: `src/ui/data_grid.rs::render` 与 widgets imports
- Test: `src/ui/data_grid.rs::tests`

**Step 1 — 修正共同起点。**

```rust
fn data_start_x(area: Rect, number_width: u16) -> u16 {
    area.x
        .saturating_add(number_width)
        .saturating_add(1)
}
```

**Step 2 — 明确 Table 不拥有选择槽。**

引入 `HighlightSpacing`，将表格构建改为：

```rust
let table = Table::new(rows, constraints)
    .header(header)
    .block(block)
    .column_spacing(0)
    .highlight_spacing(HighlightSpacing::Never);
```

保留当前默认 TableState 和 Buffer 后处理。移除失效的 row/cell highlight 配置、highlight_symbol，以及仅供这些配置使用的 `selected_row_deleted`、`row_highlight_style` 局部计算。更新注释，说明后处理用于保留 NULL 等语义前景色，布局无选择标记槽。

**Step 3 — 修正旧命中测试并增强依据。**

`relation_sort_hit_regions_cover_header_content_not_separators` 的宽 6 夹具中，第一列右分隔符由旧错误期望 x=11 改为 x=10；首列内容起点为 x=4。同时断言对应 Buffer 字符位置，不只更改数字。

**Step 4 — 运行共享网格测试。**

```bash
cargo +1.94.0 test --lib ui::data_grid::tests
```

预期：Task 1 用例通过；其余失败逐项对照实际字符布局判断，不机械平移全部预期值。

**Step 5 — 完成第一个逻辑提交。**

```bash
git add src/ui/data_grid.rs
git commit -m "fix(ui): align data grid selection with cell content"
```

## Task 3：让高亮与交互消费同一份列几何

**Files:**
- Modify: `src/ui/data_grid.rs` 中 VisibleColumn 附近的私有结构
- Modify: `render` 中排序、单元格命中、列宽调整和 Buffer 高亮循环
- Test: `src/ui/data_grid.rs::tests`

**Step 1 — 添加交互一致性测试。**

建议命名 `grid_hit_regions_match_rendered_columns`：在 Task 1 的场景中验证数据首格与最后一格命中同一 ResultCell，表头文字命中 GridColumnSort，实际分隔符命中 RelationColumnResize，后一列首格不被前列命中区域吞掉。使用独立坐标和 Buffer 中的实际符号作为判据。

**Step 2 — 新增轻量布局数据。**

在文件内部定义可见列几何，例如：

```rust
#[derive(Clone, Copy, Debug)]
struct GridColumnGeometry {
    column: VisibleColumn,
    x: u16,
    width: u16,
    separator_x: Option<u16>,
}
```

在 `visible_columns` 与内容右边界确定后计算一次。`width` 为内容宽度裁剪值；仅为完整且后面确有可见列的列提供 `separator_x`。同一份数据驱动四类循环，保留列的自然宽度以构造 resize target。

继续通过相同 VisibleColumn 宽度生成 Table constraints，保证手工几何与 Table 使用同一组宽度。保持现有 HitRegion 注册顺序，避免改变重叠目标的优先级。

**Step 3 — 加入边界保护。**

如果 `x >= content_right` 或裁剪后宽度为零，不生成内容区域；所有区域都裁剪在可绘制范围内。确保选中行 y 位于数据行区域，不落在横向滚动条上。

**Step 4 — 清理只验证实现公式的遗留测试辅助。**

检查 `#[cfg(test)] selected_data_cell`：它仅由测试调用，而生产高亮已经使用 Buffer。用真实渲染/命中测试承担契约后，移除该辅助函数及仅断言 2、4 的对应测试。

**Step 5 — 运行测试并提交。**

```bash
cargo +1.94.0 test --lib ui::data_grid::tests
git add src/ui/data_grid.rs
git commit -m "refactor(ui): share data grid column geometry"
```

预期：高亮和所有命中区域与实际内容一致，现有水平/垂直滚动测试通过。

## Task 4：统一可用宽度、裁剪与滚动条预留

**Files:**
- Modify: `src/ui/data_grid.rs::render` 中 base_available、available、content_right
- Modify: `src/ui/data_grid.rs::render_scrollbar`
- Review/Modify as needed: `render_vertical_scrollbar`、`visible_columns`、`viewport_start`
- Test: `src/ui/data_grid.rs::tests`

**Step 1 — 编写宽度边界测试。**

覆盖总数据宽恰好可容纳、差一格、尾列部分可见、超宽首列、竖向 gutter 出现，以及宽高极小的区域。重点验证恰好容纳时不误报横向溢出，差一格时只裁剪真实内容。

**Step 2 — 给当前减法明确含义。**

重点审查 `base_available` 和横向 track 中的 `saturating_sub(2)`。外边框已由 Block::inner 消化；选择槽为零；滚动条箭头占用的是 track 内部空间。逐项核对竖向滚动条坐标后，删除没有实际所有者的预留，保留有明确用途的预留并命名。

目标公式：

```text
fixed_width = number_width + separator_width
data_left = table_area.x + fixed_width
content_right = table_area.right - vertical_gutter_width
available = content_right - data_left（饱和减法）
horizontal_track = [data_left, content_right)
```

数据区没有宽度时应为空布局；不要仅为绕过零宽分支而产生越界的一格。水平/垂直滚动条相互影响的计算需收敛；保留现有有限轮计算的前提是新增边界用例证明有效。

**Step 3 — 统一传递几何。**

横向滚动条直接接收已计算的 track Rect 或共享布局结果，避免重新从 number_width 推导宽度。列可见性、高亮裁剪、hit regions 和 scrollbar 的数据范围必须一致。

**Step 4 — 运行测试并提交。**

```bash
cargo +1.94.0 test --lib ui::data_grid::tests
git add src/ui/data_grid.rs
git commit -m "fix(ui): align grid viewport and scrollbar bounds"
```

## Task 5：补全场景回归与两入口验收

**Files:**
- Test: `src/ui/data_grid.rs::tests`
- Review: `src/ui/mod.rs::render_result_table`
- Review: `src/ui/relation.rs::render_relation_result_table`
- Test as needed: 两入口现有 UI 测试模块

**Step 1 — 参数化位置与滚动场景。**

| 场景 | 核心断言 |
|---|---|
| 首列、中间列、末列 | 首格和末格有背景，分隔符没有选中背景 |
| 非零 Rect 起点、带边框 Block | 不遗漏或重复计算外边框 |
| 横向 column_offset 非零 | 显示内容、选中列索引、排序和 resize 对应实际列 |
| 纵向 row_offset 非零 | 仅可见选中行着色，不覆盖表头或滚动条 |
| 部分可见尾列、超宽首列 | 所有绘制与命中坐标落在视口内 |
| 行号从一位到多位 | 整体布局随 number_width 同步移动 |
| 空结果、零宽/零高、小尺寸区域 | 不越界、不 panic、不出现幽灵命中区域 |

**Step 2 — 验证语义样式。**

加入 NULL、Unsupported、普通字符串与中文宽字符数据。检查 NULL 的 muted/ITALIC、Unsupported 的 warning、选中单元格 BOLD；中文按终端格宽验证背景，不能按 UTF-8 字节数断言。

利用现有 RelationEditSession 构造方式覆盖 updated/deleted/inserted/conflict 行。记录并保持当前实际的前景色/背景优先级，未选中行编辑背景应保留，选中单元格仍能定位。不借清理未生效代码改变用户可见的编辑语义。

**Step 3 — 两入口人工验收。**

```bash
cargo +1.94.0 run --locked
```

使用可用的本地测试连接，分别打开 RESULT SET 与 RELATION DATA：
- 用 h/j/k/l 穿过各列；检查数字首字符、填充空白与边框。
- 水平和垂直滚动，再切换选中格。
- 点击首字符、列末字符、排序表头；拖拽实际分隔线。
- 缩窄终端产生尾列裁剪和滚动条，检查背景不侵入 gutter。
- 记录两入口的验证结果；若没有可用连接，明确标记人工验收待完成，并以入口渲染测试补足自动覆盖。

**Step 4 — 提交回归覆盖。**

```bash
git add src/ui/data_grid.rs
git commit -m "test(ui): cover grid selection alignment edge cases"
```

如果确实新增了入口测试文件，按实际文件逐个暂存。

## Task 6：最终检查与交付

**Step 1 — 执行与 Rust CI 一致的检查。**

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
git diff --check
```

预期全部成功。若环境或已有用户修改造成失败，记录具体命令、错误和归因；修复本任务引入的问题，不把未执行或受阻检查写成通过。通过后不重复运行无新变化的全量检查。

**Step 2 — 审核最终 diff。**

```bash
git diff --stat
git status --short
```

核对本任务提交和工作区：根因修复、布局收敛、回归覆盖与实际提交一致。

**Step 3 — 提交实施报告。**

报告包含修改文件、坐标契约、测试命令与结果、两入口人工验收结果、尚未完成事项。该计划文档可与实施记录单独提交。

## 完成标准

- 两视图所有可见数据单元格的首字符均与选中背景对齐。
- 分隔符保留网格样式；行号和滚动条不被当前单元格覆盖。
- 单元格点击、表头排序和列宽调整与实际绘制位置一致。
- 横向滚动、窄视口、多位行号和有边框布局均满足同一几何契约。
- NULL 等语义前景色及既有实际编辑样式优先级通过回归验证。
- 定向回归、fmt、clippy 和全量测试有清晰执行结果。

## 执行顺序与检查点

严格按 Task 1 → 2 → 3 → 4 → 5 → 6 执行，任务共享同一渲染器，顺序推进便于定位回归。Task 2 结束时得到可独立验证的最小修复；Task 3/4 完成后得到统一布局；Task 5/6 完成交付验收。
