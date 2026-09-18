# Relation Data 新增行绿色背景 Implementation Plan

> 执行者：Luna。Astra 仅完成分析与计划；后续实现、审查、纠偏及提交合并由 Luna 完成。沿用当前自动任务协议，不启动子 Agent。

**Goal:** Relation Data 编辑新增行时，其数据单元格和前方序号具有绿色背景，移动光标后仍能清楚识别新增行。

**Architecture:** 复用 RelationEditSession 的 InsertDraft/Inserted 状态和 Theme.row_inserted 配色，在共享 data_grid 初次绘制及选中行二次绘制中一致应用背景。保留语义文字色及活动单元格焦点优先级，不改变数据编辑与保存流程。

**Tech Stack:** Rust 1.94、ratatui 0.30.2、现有 TestBackend 渲染验证。

---

## 0. 基线、范围与交接

- 工作目录：`/Users/yelog/workspace/tui/lazydb`。
- 目标分支：`main`；分析起点：`16b05f4e3d1d7b78d126891d2b751a0440fc7e07`。
- 计划阶段检查 `git status --short --branch`，退出 0，仅输出 `## main...origin/main`。
- 当前任务目录没有 checkpoint.json；恢复时若出现则优先读取，并以实际 diff 确认完成项。不得修改插件维护的 state.json/checkpoint.json。
- 计划完成后由工作流自动命名任务分支。本阶段不创建分支、不提交、不实现。
- 详细根因与方案比较见同目录 `analysis.md`。
- 预计业务文件：`src/ui/data_grid.rs`、`src/ui/theme.rs`；现有测试位于这两个文件及 `src/ui/theme/external.rs`。默认不改外部主题 schema。

## 1. 唯一业务验收单元：新增行全宽背景及焦点共存

### 步骤 1：确认当前渲染结构

接手时检查基线和实际 diff；如代码仍为分析版本，直接使用以下定位：

- `src/ui/data_grid.rs:194-231`：编辑状态到 row_style，再传给 body_cells。
- `src/ui/data_grid.rs:244-274`：选中行的 buffer 二次绘制，当前无条件覆盖 selection 背景。
- `src/ui/data_grid.rs:522-582`：body_cells/data_cell_style/row_number_style。
- `src/ui/theme.rs:75-78,125-129,164-167`：状态配色字段、plain 与 deep_space。
- `src/ui/relation.rs:612-622,661-678`：实际 Relation 入口同步 edit.rows，并传入编辑会话。

所有新增判断均为 InsertDraft 或 Inserted。不要根据值是否为空、行号或原始结果长度猜测。

### 步骤 2：实现主题与初次绘制

1. 将 deep_space 的 `row_inserted` 从蓝色前景值改为低亮度绿色背景值，建议 `Color::Rgb(24, 64, 46)`；给字段补充背景用途说明。plain 保持 Reset。
2. 新增状态传入包含 `.bg(theme.row_inserted)` 的行样式，不再强制蓝色文字。
3. 保留每个值的 base 样式：普通值 text、NULL muted+ITALIC、Unsupported warning。现有 `row_style.unwrap_or(base)` 会丢失 base，所以必须调整叠加方式。
4. 优先使用现有 Style 合并机制在 base 上应用行样式，但在决定采用通用合并前检查 Deleted/Conflict 的修饰变化。如果会改变其既有语义，仅对新增行做背景叠加；不要借此重构所有行状态风格。
5. 序号沿用 row_number_style 保留背景及 muted 前景的逻辑。背景要覆盖单元格完整宽度，而非只覆盖文字 Span。
6. 分隔符继续使用 separator_style，不修改列宽、滚动、命中区域或表头。

### 步骤 3：修复选中行的背景覆盖

在选中行二次绘制前读取该绝对行号对应的编辑状态，并确定选中行背景：

```rust
let selected_row_background = if edit
    .and_then(|session| session.rows.get(grid.selected_row))
    .is_some_and(|row| {
        matches!(
            row.state,
            crate::model::relation_edit::EditableRowState::InsertDraft
                | crate::model::relation_edit::EditableRowState::Inserted
        )
    })
{
    theme.row_inserted
} else {
    theme.selection
};
```

将该循环内 `.set_bg(theme.selection)` 替换为新背景。保留现有语义前景计算，仍最后对活动列调用 `theme.grid_active_cell()`。序号不进入该循环，继续保持初次绘制的绿色背景。

活动单元格覆盖绿色是明确的焦点优先规则；其他新增数据单元格及序号必须始终能识别。不要把整个新增行重新涂成普通 selection 色。

### 步骤 4：建立最少且有效的运行证据

优先复用 `src/ui/data_grid.rs` 中 TestBackend 测试风格，检查最终 buffer，避免只断言样式 helper 返回值。此处值得补充定向回归证据，因为二次绘制覆盖问题不能由初次样式断言发现。

建议一个参数化渲染测试覆盖 InsertDraft/Inserted 和光标在行内/行外，另一个场景覆盖滚动和 plain：

- 建立有两列以上、两行以上的结果和编辑会话，包含 NULL 及短文本；对比普通邻行。
- 光标位于其他行时，新增行每列完整宽度与序号背景等于 row_inserted。
- 光标移入新增行时，非活动列仍为 row_inserted；活动列应用 grid_active_cell；序号不丢背景。
- 检查 NULL muted/ITALIC、Unsupported warning。若前述数据夹具不足以同时覆盖，使用少量参数，而非复制整份绘制框架。
- 滚动后正确匹配绝对行号，不染错邻行；plain 不引入 RGB 色。
- 复用现有更新/删除/焦点/外部主题测试检查回归，无需为所有低风险 helper 新建镜像测试。

执行：

```sh
cargo +1.94.0 test --lib ui::data_grid
cargo +1.94.0 test --lib ui::theme
```

期望：匹配到实际测试，全部通过，退出 0。若过滤结果为零个测试，修正过滤条件，不能记为验证通过。记录命令、退出结果、代码版本及当时 diff 状态到 validation.md。

### 步骤 5：核对真实 Relation 入口边界

通过现有关系测试及源码确认：

- 空表首次新增：Relation render_data 已将 edit.rows 同步到 result.rows，不触发错误的 No rows 覆盖。
- 粘贴通过 insert_row，编辑新增行保留新增状态；撤销/重做由现有会话快照恢复。
- 保存返回 Inserted 时仍显示绿色；状态回到 Clean 后不额外记忆绿色。
- 非关系结果的 edit=None 不进入新增分支。

仅当实现修改了这些路径或发现实际缺陷时扩展测试/修改范围，不把颜色需求升级为数据库编辑流程重构。

## 2. 功能完整后的验证与审查

### 需求、门禁与补充验证分级

| 类别 | 内容 | 完成判据 |
| --- | --- | --- |
| 用户需求，必须满足 | 新增行数据单元格及序号绿色系背景，能快速识别 | 内置彩色主题下 InsertDraft/Inserted 的完整单元格背景为绿色，不能仅修改前景 |
| 本次方案的兼容约束 | 选中新增行持续可识别，活动格保留焦点；语义文字、其他行状态和无色模式正常 | 下列 A1–A6 验收项成立 |
| 项目已有 Rust 门禁 | CI 中 fmt、clippy、全量 Rust tests | 命令成功；环境限制单独记载，不能将未运行/跳过写成通过 |
| 实施定向验证 | 最终 buffer 的新增行与焦点回归检查，git diff --check | 作为本次缺陷修复的运行证据，不宣称是用户额外指定的门禁 |
| 补充建议 | 人工/PTY、截图、视觉色值微调、真实数据库交互观察 | 有条件时补充；缺少此类证据不自动阻塞，也不新增数据库服务强制前置条件 |

### 可验收标准

- **A1 新增未选中：** 两种新增状态的序号、全部可见数据列及单元格填充均使用 row_inserted 背景；内置配色为绿色系。
- **A2 新增被选中：** 序号和非活动列保留绿色；活动列保留 grid_active_cell 样式。光标离开后原活动格恢复绿色。
- **A3 文字可读：** 非活动格中的 NULL 保留 muted/ITALIC，Unsupported 保留 warning，普通数据保留 text；活动格以前景焦点规则为准。
- **A4 坐标正确：** 横向或纵向滚动后新增标识跟随正确的行/列；分隔线、表头和普通邻行不误染。
- **A5 生命周期正确：** 空表首次新增、粘贴和编辑继续使用已有新增状态；撤销/重做或状态恢复遵循现有模型，不引入额外持久标记。
- **A6 兼容：** edit=None、Updated、Deleted、Conflict 不发生非预期变化；plain 无新增 RGB；外部主题原 schema 可继续解析。

最终复核以 A1–A6 的实际证据和项目检查结果为依据，不要求为了每个标准分别新建一个测试。已有测试、定向渲染检查和相关代码路径复核可以组合提供证据。

### 项目 Rust 检查

与 `.github/workflows/ci.yml:81-83` 对齐，在功能完整后各执行一次：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
git diff --check
```

期望全部退出 0；修复引发失败的实际问题后仅重跑受影响检查，避免重复无关全套。数据库服务未配置导致的跳过必须如实记录，不得称为真实数据库验证成功。本需求没有新增数据库适配逻辑。

### 补充视觉检查

可在可用终端用 SQLite Relation Data 观察新增、粘贴、移动光标与绿色对比度。人工/PTY 为补充，不是用户指定的强制门禁。环境受限时最多一次针对性修复重试，再由 Luna 审查判断补证或记录限制；不要无限维持 progress。

### Luna 审查清单

- 数据单元格与序号均有背景，空白填充同色；不是单纯绿色文字。
- 选中新增行仍可识别，活动格仍清晰。
- 两种新增状态一致，plain 与自定义主题沿用配置。
- 外部主题仍使用原 row_inserted 键，没有新增必填字段；该键的背景用途已清楚说明。
- 未意外改变 Deleted/Conflict 修饰和 Updated 色块，普通结果不受影响。
- 所有验证结论都对应本次代码状态，剩余限制明示。

## 3. 收尾交接

业务验收单元完成后，由 Luna 按工作流执行审查、必要纠偏、提交合并。建议提交主题：`fix(ui): highlight inserted relation rows with green backgrounds`。实际分支名和工作流名由后续自动命名机制/Luna 决定。

当前仅完成 plan 阶段；用户已选择自动工作流继续实施，不再询问执行方式。下一阶段第一个未完成的可验收单元为第 1 节完整业务单元，由 Luna 实施。

本阶段完成后最后写入 `plan-63e8e5a5-4e4c-41c6-89e1-9cec376b3cbc.json`，token 为 `63e8e5a5-4e4c-41c6-89e1-9cec376b3cbc`，stage 为 plan，status 为 completed。不得改写历史回执或插件状态。
