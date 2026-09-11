# SQL 编辑器诊断精确渲染实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. 若执行环境没有该技能，按本文任务顺序逐项实施和验证。

**Goal:** SQL 编辑器只对诊断实际覆盖的原文标识符显示错误前景色和下划线，跨行、Tab、Unicode、滚动及显示转义不产生错位。

**Architecture:** 统一诊断和渲染片段的源范围为全文 UTF-8 字节坐标；每行只做一次显示投影，显式记录原文字节边界、投影文本字节边界和显示单元边界。UI 基于原文字符范围一次性判定诊断，同时生成前景色和下划线，保留语法、选择区和输出日志的既有样式优先级。

**Tech Stack:** Rust、现有 sqlparser 语义分析、modalkit 编辑器快照、ratatui TestBackend、unicode-width；不新增依赖。

---

## 1. 已确认的问题与实施约束

### 1.1 可复现样例

```sql
SELECT * from ignore_table;

SELECT ignore_col from sys_user;
```

假设 `sys_user` 存在且列元数据完整，其中没有 `ignore_col`。

- 表诊断全文范围：`[14, 26)`，切片为 `ignore_table`。
- 第三行起点：`29`。
- 列诊断全文范围：`[36, 46)`，切片为 `ignore_col`。
- 当前 UI 将第一条诊断当成第三行的行内范围；该行 `[14, 26)` 恰好是 `col from sys`。
- `editor_line_spans()` 对整个语法 span 设错误色、对字符设下划线，所以错误色进一步扩散到 `ignore_col from sys_user`。

### 1.2 已检查的入口

- `src/sql/semantic.rs`：`analyze_semantics`、`validate_column`、`ident_range`。
- `src/editor/mod.rs`：快照生成、`render_span`、`statement_background_cells`。
- `src/model/editor.rs`：`EditorRenderSpan`、`EditorRenderLine`。
- `src/security.rs`：`DisplayLineProjection`、`project_editor_line`。
- `src/ui/mod.rs`：`editor_line_spans`、`line_has_diagnostic`、`render_output`。
- `src/app.rs`：`output_line_style`，返回的时间戳末尾是行内 UTF-8 字节偏移。
- 共用渲染调用方：`src/ui/relation.rs`、`src/ui/text_detail.rs`。

### 1.3 工作边界

- 实施前重新检查 `git status --short` 和相关 diff；工作区可能存在其他任务正在修改的文件，不能覆盖。
- 本计划修改诊断显示所依赖的数据契约与显示投影，不扩展 SQL 名称解析规则。
- 数据模型新增字段会影响结构体字面量，实施时查全构造点并按编译反馈迁移；这是本次修复必需的源代码兼容性变更。
- 不修改诊断调度 key、revision 校验和数据库元数据加载规则。
- 不创建真实数据库、不依赖网络执行回归测试。
- 未收到用户明确指令时，不执行 commit、push 或创建 worktree。以下任务为逻辑交付单元，不要求逐任务提交。

## 2. 数据契约和实现选择

### 2.1 源范围

`EditorRenderSpan.source_start/source_end` 统一为全文 UTF-8 字节坐标，区间左闭右开。

为 `EditorRenderLine` 增加 `source_start/source_end`，明确表示该行正文在全文中的范围，不包含逻辑换行分隔符。空行允许二者相等。按编辑器实际换行存储约定核对 CRLF、结尾空行以及经过编码的控制字符，不能把底层 buffer 编码文本长度当成解码后全文长度。

源范围不随视口、滚动、语法高亮开关或字符显示宽度变化。

### 2.2 整行显示映射

扩展现有 `DisplayLineProjection`，保留 `text` 和 `source_to_display_cells`，增加两组边界数组：

```rust
// 所有数组由相同的“原文字符边界索引”访问，均包含末尾边界。
pub source_byte_boundaries: Vec<usize>,
pub source_to_display_bytes: Vec<usize>,
```

- `source_byte_boundaries[i]`：原文第 i 个字符边界的行内 UTF-8 字节偏移。
- `source_to_display_bytes[i]`：投影文本中对应的 UTF-8 字节偏移。
- `source_to_display_cells[i]`：对应的终端显示单元偏移，沿用现有含义。
- 三个数组长度均为原文字符数加一。
- 空文本为三个 `[0]`；字节边界必须落在合法 UTF-8 边界；显示单元允许因零宽字符而重复。
- 不增加反向按每个屏幕单元复制源范围的数组，避免 Tab 和转义展开造成额外复制。

`EditorRenderLine` 保存上述两组新增映射及现有 cells 映射。span 文本仍为投影后的文本，来源改为整行投影切片，避免再次投影。这样现有依赖 span 文本的调用方可以保持其使用方式。

### 2.3 诊断样式

对于原文字符边界 `i..i+1`：

```rust
let source_start = line.source_start + line.source_byte_boundaries[i];
let source_end = line.source_start + line.source_byte_boundaries[i + 1];
let has_error = diagnostics.iter().any(|diagnostic| {
    diagnostic.range.start < source_end && diagnostic.range.end > source_start
});
```

只用这一个结果决定错误前景色与 `Modifier::UNDERLINED`。不能再对整个语法 span 的相交结果设置错误色。

一个原文字符展开出的全部显示字符共享该字符的诊断状态。选择背景仍可按显示单元处理，因此不能因 Tab 展开而扩大鼠标或 Visual 选择范围。

### 2.4 零长度诊断

- 非空诊断按严格范围相交判断。
- 零长度诊断不扩展到无关字符、不伪造原文内容；本轮通过正确行的行号错误色和诊断数量提示。
- 使用行正文起止范围确定零长度位置的归属，行尾位置属于本行，下一行起点属于下一行；显式覆盖空行及 EOF。
- 只在视口中渲染对应行标记；不要将诊断夹到当前可见的第一行或最后一行。

## 3. 任务 1：锁定跨行诊断及最终显示回归

**Files:**
- Modify: `tests/sql_semantic.rs`
- Modify: `tests/ui_render.rs`

1. 复用 `tests/sql_semantic.rs` 的 relation/column/catalog fixture，为上述完整样例建立内存目录，声明 `sys_user` 列覆盖完整且只有 `id`。
2. 增加 `diagnostic_rendering_multistatement_source_ranges`：断言诊断数量、code、精确范围和原文切片，不能仅断言 message 包含某个名称。
3. 在 UI 测试中复用 App 和 TestBackend fixture，通过真实编辑器文本生成快照；将 `analyze_semantics()` 的结果注入当前 console 的诊断状态，绘制真实 UI。
4. 增加 `diagnostic_rendering_marks_only_missing_identifiers`：逐单元断言两个标识符具有错误色和下划线；`SELECT`、`from`、`sys_user`、分号和间隔空格没有错误样式。按渲染出的行文本定位内容区，避免硬编码面板宽度及行号 gutter。
5. 增加只有第一条语句存在诊断、后续行合法的测试，排除“修好第二条诊断却仍重复绘制第一条”的情况。
6. 运行语义与 UI 定向测试，记录当前失败的单元和样式。

**Commands:**

```bash
cargo test --test sql_semantic diagnostic_rendering_
cargo test --test ui_render diagnostic_rendering_
```

**Expected:** 语义范围测试通过；当前实现的 UI 精确样式测试失败，表现为后续行误染色/下划线错位。若失败原因是 fixture 或 UI 未显示，先修测试设置，不把环境失败算作复现。

## 4. 任务 2：补齐整行投影的原文与显示边界

**Files:**
- Modify: `src/security.rs`
- Create: `tests/editor_projection.rs`

1. 为 `project_editor_line()` 增加三个边界数组对应关系的行为测试，使用真实输入与精确预期，而非重复实现投影算法。
2. 关键 fixture：`a\t中\u{1b}b`。原文字节边界为 `[0, 1, 2, 5, 6, 7]`；投影为 `a   中<ESC>b`；投影字节边界为 `[0, 1, 4, 7, 12, 13]`；显示单元边界为 `[0, 1, 4, 6, 11, 12]`。
3. 增加空文本、连续 Tab、组合附加符、CR/LF 和其他控制字符 fixture；验证现有转义文本与显示宽度策略保持一致。
4. 在投影循环开始记录原文字符起点，在每次投影后记录显示字节数及 cells；正确附加原文末尾边界。
5. 查找并迁移所有 `DisplayLineProjection` 的字面量构造；原有使用 `text` 或 cells 的调用方无需改写显示规则。

**Command:** `cargo test --test editor_projection`

**Expected:** 所有映射精确值通过，Tab 按整行列位置计算，Unicode 切片合法且不会 panic。

## 5. 任务 3：统一快照的全文坐标及一次投影

**Files:**
- Modify: `src/model/editor.rs`
- Modify: `src/editor/mod.rs`
- Modify: `src/editor/tests.rs`
- Modify: 编译器定位到的 `EditorRenderLine` 字面量构造点

1. 添加 `diagnostic_rendering_snapshot_` 前缀测试：覆盖第三行、前置中文行、空行、结尾空行及非零 first_line 视口，断言 line/span 源范围对应全文实际位置。
2. 为 line/span 字段写明坐标单位、区间约定和换行含义，加入两组映射。
3. 将 `render_span()` 保存的范围改为 `line_start + start/end`；空 span 的 fallback 必须用该行全文位置，而非固定零。
4. 每行调用一次 `project_editor_line()`，`render_span()` 根据行内原文字节边界查找投影边界并截取整行投影文本。使用边界查找，不通过字符显示宽度推算字节位置。
5. 确保 span 切分的 start/end 落在原文边界；对无效内部范围明确断言或返回错误，避免 `unwrap_or_default()` 静默吞掉可见 SQL。
6. 添加 `SELECT\tignore_col` 等跨语法片段的 Tab fixture，断言所有 span 拼接文本与整行投影完全相同，并与 cells 映射一致。
7. 行起点可使用一次计算的行起点表或从首个可见行累计推进；不要在每个可见行重复从全文首部扫描，也不增加新的持久缓存。
8. 运行编辑器定向测试和库编译检查；此时 UI 样式粒度问题仍由下一任务解决。

**Commands:**

```bash
cargo test --lib diagnostic_rendering_snapshot_
cargo check --tests
```

**Expected:** 快照测试通过，所有构造点编译通过，拼接后的显示文本与修复后的整行投影一致。

## 6. 任务 4：统一诊断覆盖与样式合成

**Files:**
- Modify: `src/ui/mod.rs`
- Create: `src/ui/editor_diagnostics_tests.rs`
- Modify: `tests/ui_render.rs`

1. 在 `src/ui/mod.rs` 注册专用测试子模块，直接测试 crate 内的 `editor_line_spans()`，不为测试扩大公共 API。
2. 构造同一行的“一个 Plain span”和“多个语法 span”两种快照，断言错误覆盖集合一致；使用 `syntax = false` 再验证一次。
3. 将 `editor_line_spans()` 改为按源字符映射推进，使用 span 游标取得语法 kind，从各 span 的投影切片中取显示内容；如需计算 span 内显示字节位置，使用整行投影字节边界减去 span 起点对应边界。
4. 每行先过滤诊断，再按原文字符进行一次范围判断；用同一个 has_error 设置错误色和下划线。
5. 删除 span 级 `has_semantic_error` 和对投影字符累计 `source_offset += character.len_utf8()` 的逻辑。
6. 保留相邻相同 Style 合并；对一个字符的多个投影字符共享源诊断状态，但仍按显示字符/单元判断背景选择。
7. `line_has_diagnostic()` 改用显式行源范围，按第 2.4 节处理零长度诊断。
8. 增加同一字符上重叠诊断、诊断仅覆盖长 Plain span 的内部片段、行边界相邻诊断测试，确保不重复输出文本。
9. 运行任务 1 中的真实 UI 回归，确认截图现象消失。

**Commands:**

```bash
cargo test --lib diagnostic_rendering_
cargo test --test ui_render diagnostic_rendering_
```

**Expected:** 只标记目标原文范围，错误前景色集合与下划线集合一致；语法 span 的拆分、合并和 syntax 开关不改变诊断覆盖。

## 7. 任务 5：保护共用渲染器和边界场景

**Files:**
- Modify: `src/ui/mod.rs`
- Modify: `src/ui/editor_diagnostics_tests.rs`
- Modify: `tests/ui_render.rs`
- Review: `src/app.rs` 的 `output_line_style`
- Review: `src/ui/relation.rs`、`src/ui/text_detail.rs`
- Review: `tests/mouse.rs`

1. 输出日志时间戳比较明确使用 `source_start - line.source_start`，和 `output_line_style()` 返回的行内字节边界保持一致。无需将日志 API 改成全文坐标。
2. 加入第二条及更后日志行测试：时间戳仍为 muted，Error 正文仍为 error，含 SQL 的日志仍保持预期语法颜色。
3. 将诊断放在 Tab 后、宽字符后、投影转义字符后，验证下划线落在最终可见的目标字符上；对于不适合 SQL 语义解析的控制字符场景，直接注入精确诊断以隔离渲染行为。
4. 对 Tab 本身注入诊断，验证其展开空格全部获得错误样式，后一个原文字符不受影响。
5. 测试 Visual 选择、鼠标选择、当前语句背景与诊断叠加，保持现有背景优先级：鼠标选择 > Visual 选择 > 当前语句 > 默认背景。
6. 通过 TestBackend 验证横向裁剪和 first_line 非零的纵向滚动；使用现有滚动入口，不只修改测试字符串。
7. 测试空行和 EOF 零长度诊断：只标记正确行号，不给相邻 SQL 字符补画下划线。
8. 检查文本详情、关系 DDL 等无诊断快照的显示文本、复制/选择行为，运行已有鼠标测试；只有出现实际不兼容时才修改调用方。

**Commands:**

```bash
cargo test --lib diagnostic_rendering_
cargo test --test ui_render diagnostic_rendering_
cargo test --test mouse
```

**Expected:** 诊断不随滚动漂移，映射不因转义展开失真，输出日志、选择区和无诊断编辑器视图无回归。

## 8. 任务 6：综合验证与交付

**Files:** 本计划涉及的实现与测试文件。

1. 检查仓库现有 CI 命令和格式约定，先执行格式检查；若需要格式化，只处理本次修改的文件并审查 diff。
2. 执行完整相关测试目标，避免只看带前缀的新测试。
3. 因投影函数和渲染模型被多个界面共用，定向测试通过后运行一次全套测试以及仓库要求的 lint；若 CI 有数据库前置条件，记录无法运行的目标及原因。
4. 复查 diff，确认不存在诊断消息解析、按字符串全局搜索错误列、固定列偏移或终端 gutter 补偿等绕过坐标问题的补丁。
5. 交付修改文件、测试结果及截图案例的修复说明；明确人工终端验收是否实际完成。

**Commands:**

```bash
cargo fmt --check
cargo test --test sql_semantic --test editor_projection --test ui_render --test mouse
cargo test --lib
cargo test
cargo clippy --all-targets --all-features -- -D warnings
git diff --check
git diff --stat
git status --short
```

仓库若规定不同 clippy feature 组合，以 CI 的实际要求为准；与本次改动无关的既有失败单独记录，不自行扩大修复范围。

## 9. 最终验收清单

- [ ] 截图样例仍有两个语义诊断，原文范围分别为 `[14, 26)`、`[36, 46)`。
- [ ] 只对 `ignore_table` 和 `ignore_col` 显示错误前景色、下划线。
- [ ] 第三行 `from sys_user` 恢复对应语法样式，没有下划线。
- [ ] 第一行诊断不会在后续行重复出现，行号错误标记归属正确。
- [ ] 多行、空行、Unicode、Tab、控制字符投影及滚动测试通过。
- [ ] Plain span、关闭语法高亮及不同语法切分不改变诊断覆盖。
- [ ] 零长度诊断归属正确，不误标相邻字符。
- [ ] 输出日志后续行时间戳、选择区和文本详情无回归。
- [ ] 测试使用内存 fixture 和 TestBackend，无真实数据库或网络依赖。
- [ ] 格式、相关测试和必要的综合检查完成，未运行或失败项有事实记录。

## 10. 执行顺序与交付方式

顺序：任务 1 → 任务 2 → 任务 3 → 任务 4 → 任务 5 → 任务 6。

这几个任务共享投影与渲染模型，默认串行实施。任务 2–4 为一个完整修复核心，中间态不能作为完成结果交付。获得执行授权后可在当前会话逐项推进；如用户明确选择委派方式，再按任务边界安排子代理并逐项审查。
