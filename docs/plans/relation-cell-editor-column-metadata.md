# Relation Cell Metadata Editor Implementation Plan

**执行者：** Luna。按下述端到端单元连续实施、验证及审查；不启动子 Agent，不等待用户逐项 resume。

**Goal:** 让 Relation Data 单元格编辑弹窗直接呈现列名、类型、默认值和注释，同时保持全部输入器、状态切换及鼠标/光标行为正确。

**Architecture:** 在 UI 层根据 editor.column、当前 preview 和关系目录缓存派生只读列上下文；统一分配元信息、正文、状态、错误和快捷键矩形。数据库默认表达式与输入 presence 分开显示，不修改数据库驱动或编辑状态协议。

**Tech Stack:** Rust 1.94、Ratatui 0.30.2、现有 Theme、unicode-width、Ratatui TestBackend。

---

## 0. 基线、约束与交接

- 工作空间 `/Users/yelog/workspace/tui/lazydb`，目标 `main`，分析基线 `16b05f4e3d1d7b78d126891d2b751a0440fc7e07`。
- 本次计划阶段读取 checkpoint.json 仍返回 File not found；`git diff --stat` 退出 0、无输出。沿用本会话已读取的代码与分析，不重读历史日志。
- 设计依据为同目录 analysis.md；用户未提供可读取截图。视觉参考是分析文档中的终端示意，不要求像素复刻未知截图。
- 工作流名称和分支由 Luna 在计划完成后确定。本阶段不创建任务分支、不修改业务代码、不提交合并。
- 本轮指定回执为 `plan-737e07e0-094a-4970-ba24-902da77b5558.json`，token 为 `737e07e0-094a-4970-ba24-902da77b5558`；完整计划完成后写入 completed，不复用 analyze 回执。
- Luna 开始实施时先确认实际 HEAD、工作区 diff 及插件指定的任务分支/工作树；如已有用户修改，保留并在其基础上定位本任务范围。
- 不修改 state.json；checkpoint.json 只读。验证持续记录到同任务目录 validation.md。

## 1. 固定的产品与设计决策

### 正常布局

```text
╭─ Edit · delivery_status ────────────────────────────────────╮
│ varchar(24) · NOT NULL                                     │
│ Default  'pending'::character varying                      │
│ Comment  发货状态，由仓储系统回写                           │
│                                                           │
│ shipped▏                                                  │
│                                                           │
│ Enter apply  Esc cancel                                   │
╰───────────────────────────────────────────────────────────╯
```

- 列名主导标题，保留原始大小写；类型、可空性在第一内容行。
- Default 与 Comment 左对齐，标签使用 muted，值使用 text；列名用 accent/加粗，沿用 panel_block。
- Default 和 Comment 正常空间中各至少 1 行、最多 2 行。缺值为 None，字段不支持为 Not supported，目录未就绪/找不到列为 Unavailable。
- 普通 VALUE 不显示 VALUE / TEMPLATE。NULL / DEFAULT 在正文旁的独立状态行显示，绝不替代列定义的 Default。
- 无结果列时标题回退 `Edit · Column N`；无目录信息时仍可编辑，name/type 来自结果列。
- 最大宽度维持 72。高度从内容预算计算；普通通常 10–12 行，JSON 通常 20–22 行，均截断到实际 relation area。
- 只用现有 Theme，无新依赖、无动画、无新模式或快捷键。

### 数据关联规则

1. 从 tab.data 的 Ready 或 previous 快照取最后一个 result set，借用 columns[editor.column]；不克隆结果行。
2. 按 tab.descriptor.key.profile_id 取 completion index，以完整 object_id 调用 relation_columns，精确匹配原始 name。
3. 只接受 CatalogMetadata::Column。目录未命中时，可用属于同关系的 Ready DDL.children；不使用失效 previous DDL 作为权威元信息。
4. 有 catalog 时 native_type 优先，空字符串退回 result type_name；没有结果列身份时不能按 catalog 数组位置猜测。
5. comment 来自 CatalogEntry.comment，default_expression 来自 ColumnMetadata；保留 OptionalMetadata 的支持/缺值区分。
6. 展示文本经 sanitize_terminal_text 处理，宽度以终端单元格计算；处理显示投影，不改原始数据或草稿。

## 2. 单元一：普通文本弹窗完整接入列元信息

**修改文件：** `src/ui/relation.rs`（render 的 EditCell 分支，当前约 57–225 行，及同文件 tests）。

**参考文件：** `src/db/catalog.rs`、`src/db/query.rs`、`src/sql/completion.rs`；`src/app.rs` 的 relation_result、refresh_active_data_query_completion。参考即可，不扩展这些公共数据类型。

### 实施步骤

1. 在 UI tests 增加构造真实 RelationTab preview + catalog entry 的小型 fixture。沿用现有 App/WorkspaceTab 测试创建方式；优先复用现有测试数据构造 helper，避免复制大块 App 初始化。
2. 增加行为测试 `cell_editor_shows_selected_column_metadata`：构造至少两列，编辑第二列；断言标题为第二列、native_type/default/comment 可见，第一列的字段值不可见，VALUE / TEMPLATE 不存在。
3. 增加 `cell_editor_metadata_is_scoped_to_relation`：两个不同关系有相同列名和不同注释；断言当前关系的值正确。再覆盖目录列顺序与结果列顺序不同，防止用位置关联。
4. 运行上述筛选测试，记录新增功能尚未实现的失败；不要为预期失败进行环境修复。
5. 在本文件增加局部只读上下文 resolver（推荐命名 `cell_editor_column_context`）。使用借用或小型 owned 展示字符串，不向 CellEditorState 增加字段，不在 render 发请求。
6. 添加仅供显示的 OptionalMetadata 格式化/安全文本处理 helper。缺少 entry 与 Unsupported 必须走不同分支；SQL 表达式 `NULL`、`''` 保留原样，不能当成 None。
7. 提取 cell editor 渲染局部函数，集中组织标题、元信息区和正文区。首个闭环接通普通文本输入，并让 typed 分支继续可用，不能先提交无法编译的中间结构。
8. 将普通输入绘制、horizontal offset 和 register_input_selection_target 统一传入真实 body/input Rect。
9. 运行本单元定向测试，通过后记录命令、退出码及 diff 状态。

**建议命令：**

```sh
cargo +1.94.0 test --lib ui::relation::tests::cell_editor_
```

**验收：**普通单元格弹窗实际显示正确列的 name/type/default/comment；无缓存仍可编辑；另一个关系的同名列不会污染显示。无需等第二单元才能看到完整用户收益。

## 3. 单元二：统一 typed 编辑器布局、错误与鼠标区域

**修改文件：** `src/ui/relation.rs` 的 render_json_editor、register_json_selection_target、render_boolean_editor、日期时间分支及 tests。

**参考文件：** `src/ui/shortcut_hints.rs`，`src/model/cell_editor.rs`，`src/input/keymap.rs`。保持已有按键实际语义。

### 实施步骤

1. 定义局部布局结果，明确包含 metadata/body/presence/error/footer（及时间 calendar/hints 所需区域）；空区域用零高度 Rect。先计算，再绘制与注册命中。
2. 将元信息头接入布尔、日期时间、JSON。布尔保留 true/false 样式；时间保留字段操作与 calendar_label；JSON 保留多行正文及格式化操作。
3. JSON body/error/footer 的划分只由一个 helper 决定。可保留 render_json_editor 封装，但让其与 selection registration 消费同一个布局结果，不分别计算 `height - 2`。
4. 移除 JSON 外层通用 error 再次绘制路径。错误只在布局分配的 error Rect 渲染一次，不能覆盖正文中段。
5. 使用真实 JSON body height 计算 top，真实 body width 计算 left，光标和命中图使用相同偏移。仅为实际显示的正文行注册命中区域。
6. presence 为 Value 时无模板横幅；为 Null/Unprovided 时保留明确文字，与 Default 元信息同时可见。沿用原 presence 切换/编辑逻辑，不自动填充默认表达式。
7. 按可用宽度整理快捷键，参考 shortcut_hints 的 packing。普通确认/取消与 JSON 的 Enter newline/Ctrl-S apply 不混用。
8. 调整既有测试中的固定坐标，使断言基于真实正文/选区区域；保留对光标显示宽度、滚动、末尾空行和选区映射的语义要求。
9. 增加一个 error 情况下的 JSON 渲染测试，断言错误出现一次、正文不被覆盖、最后可见正文行有正确命中区域。
10. 增加 presence 三态测试，切换输入状态时列 Default/Comment 不变；日期、布尔、JSON 均展示同一个字段上下文。

**运行：**

```sh
cargo +1.94.0 test --lib ui::relation::tests
cargo +1.94.0 test --test relation_tabs --test keymap --test ui_render
```

**验收：**四类输入器全部获得列信息；编辑、确认/取消和 presence 行为无回归；JSON 的正文、错误、footer、光标、鼠标命中一致。

## 4. 单元三：长文本、未知信息与小尺寸收敛

**修改文件：** `src/ui/relation.rs` 的本地布局与显示 helper、tests。仅在已有工具不足且真正复用时才提取跨文件 helper。

### 实施步骤

1. 对 default/comment 做显示单元格宽度换行，最多 2 行；最后一行截断时带省略号。长连续表达式也要可截断，避免只按空格换行。
2. 应用降级顺序：删装饰空行 → 删元信息续行 → 删补充约束 → 删 Comment → 删 Default。正常可用区域优先保留输入、非 Value 状态及错误；类型行在可容纳时保留。
3. 标题截断、正文显示和元信息标签都按 Unicode 宽度，而非字节长度；终端控制字符只显示受控投影。
4. 对宽/高为零的 body，不设置光标、不注册鼠标区域；对 1×1 不 panic、不遗留旧 cursor。
5. 为以下不同语义添加组合测试，不按每个 helper 实现细节写镜像测试：
   - Supported(None)、Unsupported、完全无目录；
   - 默认表达式为 NULL、空字符串 SQL 表达式、长函数表达式；
   - 中文/宽字符列名与注释、含换行/控制字符的元数据；
   - 长元数据下仍有正文与错误；
   - 80×24、120×40、40×12、20×6、1×1 的边界；
   - Theme::plain 对应无色模式下文字仍能解释 Null/Default。
6. 运行发生变化的 UI tests；若第二单元后没有改动键位或模型，不重复全部集成验证。

**验收：**正常尺寸元信息齐全；压缩时信息按重要性退让且正文仍可操作；极小尺寸安全；无色主题语义完整。

## 5. 完整验证与 Luna 收尾审查

### 验收与门禁来源

| 类别 | 内容 | 完成要求 |
| --- | --- | --- |
| 用户需求 | 专业排版的列名、列类型、默认值、注释展示，替代固定标题与低信息量模板行 | 必须实现；以实际渲染和行为断言验收 |
| 现有行为保护 | 普通/布尔/时间/JSON 编辑、NULL/DEFAULT、光标与选区 | 必须保持；定向回归证明布局改动未破坏编辑 |
| 项目强制门禁 | CI Rust 的 fmt、clippy、全量测试 | 功能齐备后执行，记录真实退出结果；环境限制不得冒充通过 |
| 本计划的定向验证 | 元数据关联、未知状态、Unicode、宽窄尺寸 TestBackend 测试 | 为本次修改选择的自动化验收手段，不声称是用户原先指定的测试清单 |
| 补充建议 | 人工/PTY 视觉查看、截图比对 | 非必需门禁；可用时补充，环境受限按下述次数约束记录限制 |

PK/IDENTITY/AUTO INCREMENT 等辅助属性是设计增强项，仅在已知且有空间时显示；不以新增全套列约束展示作为用户需求完成条件。无需新增人工审批步骤。

### 项目必需的 Rust 检查

功能齐备后，按 `.github/workflows/ci.yml` 运行一次：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

fmt 若提示格式差异，使用项目 formatter 修复，再做对应验证。普通编译/断言错误自行纠偏。记录数据库集成测试是否因环境变量未配置而跳过，不能把跳过声称为真实数据库通过。

所有成功检查预期退出码为 0；测试输出必须有实际匹配的测试及通过结果，不能把过滤后执行 0 个测试视为覆盖。新增功能测试在实现前预期因目标行为断言失败；若失败来自 fixture 构造或编译，先修复测试准备问题再判断功能缺失。定向检查通过后继续下一单元，不在代码未变时重复运行相同检查。

### 补充视觉验证

有合适终端环境时，查看一例含 default/comment 的普通字段和一例 JSON；检查焦点、间距、截断、错误及窄屏。此任务没有用户强制 PTY 检查要求，它是补充证据。环境失败最多一次针对性修复重试，随后由 Luna 收尾审查决定补充 TestBackend 输出或如实记录限制。

### 审查清单

- 列身份取 editor.column，未误用可移动的 grid.selected_column。
- 不按 catalog children 数组位置关联列，不串 profile/relation。
- 未在渲染期间查库、克隆整张结果集或新增持久化字段。
- default_expression、comment 三态明确，SQL NULL 与“无默认值”不混淆。
- 不把静态默认表达式写入草稿；presence 与数据库字段定义相互独立。
- JSON 错误只画一次，光标、body 和 hit map 共用布局。
- 主题及终端安全处理沿用已有机制。
- 所有验证结果对应最终代码版本；没有把分析阶段静态阅读写成测试通过。

### 收尾与提交

1. 将实际命令、退出码、代码版本/工作区状态、环境和限制写入 validation.md；仅因新改动/失败/环境变化重跑对应检查。
2. 审查最终 diff，确保只有相关 UI 和必要行为测试修改。
3. Luna 按任务工作流确定名称和分支，使用 git-commit 技能执行提交；建议提交主题 `feat(ui): show column metadata in relation cell editor`。如分单元提交，最终提交前保证完整回归结果对应最新工作区。
4. 提交合并及其回执遵循当阶段插件提供的授权与新 token；不复用本任务早前 analyze 回执，不修改插件 state/checkpoint。

### 验证记录格式

每次实际执行后追加短记录：阶段/单元、完整命令、退出码及通过/失败/跳过摘要、HEAD 与相关未提交文件、操作系统/工具链和必要服务状态。修复失败后只补充新的实际结果，保留历史失败证据。当前计划中的命令均待 Luna 实施时执行，不代表本阶段已经验证功能。

## 6. 开始执行的第一个动作

Luna 确认任务工作树与实际 diff 后，直接开始单元一：在 `src/ui/relation.rs` 测试中构造第二列带默认表达式和注释的真实关系 fixture，并完成“解析列信息 → 普通文本弹窗展示 → 定向测试通过”的业务闭环，然后连续推进单元二、三。
