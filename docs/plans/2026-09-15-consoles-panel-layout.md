# Consoles Panel Layout Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. 如执行环境没有该技能，按本文任务顺序实施并逐项验收。

**Goal:** 将 Consoles 列表调整为名称后显示打开状态圆点、右侧显示带数据库类型图标及颜色的所属连接与 database/schema，并实现稳定的右对齐。

**Architecture:** 在现有 `render_console_manager` 中完成展示数据归一化、列表级列宽计算及分段行渲染。状态圆点只由 `record.open` 决定；执行目标有效性用于右侧异常提示。复用现有 IconSet、Theme 和终端字符宽度工具。

**Tech Stack:** Rust 2024、Ratatui 0.30.2、unicode-width、现有 UI 渲染测试。

---

## 一、设计契约

### 1. 行结构

```text
选择标记 + Console 名称 + 空格 + 打开圆点 + 弹性空白 + 连接列 + 两格空白 + 位置列 + 右侧一格留白
```

- 名称左对齐；圆点紧随名称，名称截断后仍保留圆点。
- 所属连接列包含「数据库图标 + 一个空格 + 连接名称」，整体右对齐，图标紧邻名称。
- 位置列为 `database/schema`，右对齐；同一次渲染所有记录使用同一组列宽。
- 右侧锚点为 popup 内部右边界减一格，不是终端窗口的右边界。
- 默认 popup 最大宽度沿用当前 72 列；通过重新分配空间消除现有固定 40 列详情区。

### 2. 状态与颜色

| 元素 | 规则 |
| --- | --- |
| 打开 | `●`，`theme.accent` |
| 关闭 | `○`，`theme.muted` |
| ASCII 状态 | 打开 `*`，关闭 `o` |
| Console 名称 | 正文色；选中时加粗 |
| 数据库图标 | `IconSet::database` 和 `IconSet::database_color` |
| 连接名称 | 正文色 |
| database/schema | `theme.muted`，在选中背景上保持可读 |
| 选中行 | `>` 标记与整行 `theme.selection` 背景，包括中间空白和右侧留白 |

- 不再显示 OPEN、CLOSED、已连接、未连接，也不依据会话是否连接改变行颜色。
- 未绑定：右侧显示「未绑定」，弱化色。
- 目标失效：右侧显示「目标失效」，警告色；有可解析 profile 时保留数据库类型图标和连接名称，异常提示占位置列。profile 已不存在时不猜测数据库类型。
- schema 缺失或为空时仅显示 database，不输出 `/-` 或尾部 `/`。
- 打开状态与目标异常独立，例如已打开且目标失效仍显示实心圆点。
- 状态图例在 Browse 模式现有空白行显示 `● open  ○ closed`；ASCII 模式同步替换符号。Search 模式优先保留搜索输入与快捷键。

### 3. 宽度分配与降级

1. 使用终端 cell width 统计当前搜索结果的连接列和位置列最大自然宽度。
2. 预留选择标记 2 格、名称后空格和状态 2 格、左右区最小间距 2 格、右侧留白 1 格。
3. 正常宽度下名称至少预留 12 格；右侧总预算最多占可用内容宽度的 60%，且不得挤占上述预留空间。
4. 在右侧预算内按自然宽度分配连接和位置列；溢出时先压缩超长列，正常空间下连接名称至少 6 格、位置至少 8 格，图标宽度另计。
5. 内容较短时不拉伸文字列，剩余宽度交给中间弹性空白，因此右侧始终贴齐同一锚点。
6. 空间不足以满足最小宽度时，依次收缩位置列、连接名称、Console 名称；极窄时隐藏位置列，再隐藏连接区，优先保留选择标记、可见名称和状态。
7. 名称和连接名称采用尾部省略；database/schema 优先保留 schema，database 前缀截断后拼接 `/schema`。schema 本身过长时保留可容纳的末尾信息；无 schema 时采用尾部省略。
8. Unicode/Nerd Font 模式使用 `…`；ASCII 模式使用 `...`，预算小于省略标记宽度时安全裁剪。
9. 所有减法使用饱和运算；零宽区域直接返回空行。不得以字节数代替显示宽度。

## 二、已确认的代码位置

- `src/ui/mod.rs:6093-6338`：`render_console_manager`，包括 Browse/Search 列表、Rename/Delete 模式、输入命中区域。
- `src/ui/mod.rs:6140-6212`：当前固定状态宽度、连接状态拼接和行样式，是主要替换区域。
- `src/ui/mod.rs:6905-6925`：Explorer 圆点样式参考；其连接状态语义与 Console 打开状态不同。
- `src/ui/icons.rs:240-280`：数据库类型图标、颜色；已有 NerdFont/Unicode/ASCII 适配。
- `src/model/sql_editor_list.rs`：列表状态与搜索输入，实施时参考其现有行为。
- `tests/ui_render.rs:2179`：现有连接信息与目标字段搜索测试。
- `tests/ui_render.rs:2962-3124`：Console fixture、排序、打开/关闭、空列表、搜索、重命名、删除及紧凑窗口测试。

行号是编写计划时的定位参考，执行时按函数名定位。

## 三、实施任务

### Task 1：更新现有验收基线

**Files:** 修改 `tests/ui_render.rs`。

1. 阅读现有 `console_manager_fixture` 及上述 Console 测试的完整实现，复用现有 App、Buffer 和渲染辅助函数。
2. 将现有 OPEN/CLOSED 文字断言调整为对应记录行的打开/关闭符号断言；保留原有排序验证。
3. 将连接状态展示测试重命名为体现「目标信息与搜索」的名称，保留连接、database、schema 搜索覆盖。
4. 确认 Console 行不包含旧的 OPEN/CLOSED 或连接状态文案；断言限定在弹窗记录行，避免误匹配其他面板。
5. 运行 `cargo test --test ui_render console_manager`，确认新展示预期在旧实现上失败，并记录失败原因。

**验收:** 既有测试准确表达新语义，且原有搜索与排序检查仍存在。

### Task 2：替换状态与目标信息展示

**Files:** 修改 `src/ui/mod.rs`；复用 `src/ui/icons.rs`。

1. 在 Browse/Search 分支将每条记录整理为名称、open、selected、数据库类型、连接名称、位置及目标异常信息。
2. 移除列表渲染中的 `app.sessions` 查询和 connected 派生值；保留 `execution_target`、profile 查找及 `target.is_valid(profile)`。
3. 按设计契约生成 `●/○` 或 `*/o`，使用独立 Span 设置颜色。
4. 按数据库类型构造图标 Span、连接名称 Span、位置 Span，分别设色。
5. 完成 schema 缺失、未绑定、失效目标的显示分支。
6. 保持辅助逻辑为 UI 私有函数；仅在多处确实复用时扩展 IconSet，避免为一个圆点引入通用状态框架。

**验收:** 圆点完全由 open 决定，数据库图标匹配所属连接，异常目标信息明确。

### Task 3：实现共享列宽与稳定右对齐

**Files:** 修改 `src/ui/mod.rs`，辅助函数放在 Console renderer 附近。

1. 用一次列表级遍历计算连接列、位置列自然宽度，按设计契约收敛为实际预算。
2. 实现基于 cell width 的名称截断、目标位置截断和左右补空格，优先复用现有宽度工具。
3. 删除固定 `status_width = 40` 以及拼接整段 detail 的实现。
4. 为每行生成独立 Span：选择前缀、名称、空格、圆点、弹性留白、连接左填充、图标、连接名、列间距、位置左填充、位置、右侧留白。
5. 每个 Span 使用同一行背景，包括空白；最终显示宽度严格等于内部行宽。
6. 完成窄窗口降级和零宽处理，禁止换行、覆盖边框或丢失可容纳的状态标志。

**验收:** 不同长度的名称不影响右侧锚点；相同列表的两列各自拥有统一右边界。

### Task 4：接入图例并验证交互模式

**Files:** 修改 `src/ui/mod.rs`，验证 `tests/ui_render.rs`。

1. Browse 模式在现有列表与快捷键之间的空白行显示短图例；空间不足时优先保留快捷键。
2. Search 模式使用同一行渲染逻辑，并核对搜索输入所在行、光标及命中区域。
3. 核对 Rename/DeleteConfirm 分支、输入位置和按钮命中区域。
4. 验证已有快捷键文案和新图例在 72 列弹窗及紧凑窗口中的可见性。

**验收:** Browse/Search 外观一致；搜索、重命名、删除和 Esc 返回流程正常。

### Task 5：补充必要的布局回归检查

**Files:** 修改 `tests/ui_render.rs`。

围绕对齐和字符宽度添加少量行为测试，避免仅镜像拼接实现：

1. 构造 Console 名称、连接名称、schema 长度不同的多行，通过 Buffer 坐标验证位置列末端一致、连接列末端一致。
2. 选中一行，验证名称之后的弹性空白、右侧元数据和右留白均为 selection 背景，图标与状态保留各自前景色。
3. 用中文名称、长 database/schema、窄窗口验证无越界、无边框覆盖，且状态可见、schema 辨识信息得到保留。
4. 覆盖 NerdFont、Unicode、ASCII 三种模式，尤其检查 ASCII 内容区没有新引入非 ASCII 的图标、圆点或省略号。
5. 在现有 fixture 或表驱动用例中覆盖无 schema、未绑定、目标失效，以及 open 状态不随 session 连接状态变化。

**验收:** 测试从最终 Buffer 观察用户可见结果，不只比较内部字符串。

### Task 6：运行检查与人工验收

**Files:** 本任务通常不需要新增文件。

按顺序运行：

```bash
cargo fmt --check
cargo test --test ui_render console_manager
cargo test --test ui_render
cargo check
git diff --check
```

预期：格式和 diff 检查退出码为 0；Console 定向及 UI 渲染测试全部通过；cargo check 无编译错误。若环境依赖导致阻塞，记录准确原因与已完成的检查，不将阻塞报告成通过。

人工验收：

1. 使用截图中的两条记录，验证打开/关闭分别显示实心/空心圆点，右侧贴齐且没有旧状态文字。
2. 在多种数据库连接间检查图标与 Explorer 一致，颜色正确。
3. 切换选中项，检查整行高亮连续。
4. 缩放终端，检查右侧稳定、长文本截断、边框和快捷键正常。
5. 搜索 Console 名、连接名、database、schema；进入重命名和删除确认后取消，再打开关闭的 Console。

完成后记录改动文件、检查结果和必要截图；如用户要求提交，可使用提交信息 `feat(ui): simplify consoles status and align target metadata`。

## 四、完成标准

- [ ] 每行只用一个符号展示是否打开。
- [ ] 所属连接带现有数据库类型图标和颜色。
- [ ] 连接列和 database/schema 列统一右对齐，并与弹窗内右边距一致。
- [ ] 选中背景覆盖整行，圆点与图标颜色保留。
- [ ] 无 schema、异常目标、长文本、窄窗口及 ASCII 模式符合设计契约。
- [ ] 既有搜索、排序、打开、重命名、删除交互通过验证。
- [ ] 格式、定向测试、UI 测试、编译和 diff 检查完成并记录结果。
