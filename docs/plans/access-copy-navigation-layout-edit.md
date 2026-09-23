# Access 复制、导航、布局与查看/修改分离 Implementation Plan

> 执行交接：由 Luna 按以下端到端单元持续实施、审查和交付；Astra 本轮仅计划。任务名称及任务分支由 Luna 确定。不启动子 Agent。

**Goal:** 修复 Access 当前记录复制，支持 gg/G 首尾导航，自适应 Target 列宽，并以 v 查看、e 修改提供清楚且实际可用的交互。

**Architecture:** 保留 Principal Overview 独立的 selection/offset 和一记录一行渲染。为 Access 设置专属 Action，复用现有剪贴板、TextDetail、Goto pending 与权限 SQL review/confirm/refresh 流程；只对已有结构化直接权限接通修改，不从显示字符串推断 SQL 对象。

**Tech Stack:** Rust 2024 / Rust 1.94、crossterm、ratatui 0.30、unicode-width、现有数据库 adapters、TestBackend 与 Rust 集成测试。

---

## 基线与工作约束

- 根目录 `/Users/yelog/workspace/tui/lazydb`；目标 `main`；起点及计划时 HEAD：`6841cb5ebb1fb98a591ec60cabb86f4e3998cb91`。
- plan 阶段再次检查 `git status --short && git rev-parse HEAD`：退出 0，工作区无业务改动。checkpoint.json 仍不存在。
- 本计划依赖的业务代码均已在起点提交中，无本地未提交文件需要迁移。Luna 建立任务 worktree 前再次确认；若发现新的本地行为，显式纳入范围，不能默认携带。
- 报告、计划、验证记录仅写入本任务目录。本轮回执为 `plan-f0d5cbef-2c4c-4180-b210-c684b78d04fc.json`，token 为 `f0d5cbef-2c4c-4180-b210-c684b78d04fc`；先写 change-scope.json，完成阶段后最后写回执，不覆盖历史回执，不写 state/checkpoint。
- 已有 analysis.md 为根因证据。引用行号对应起点，改动后用符号定位。当前图片只有引用、无可读实体，具体数据库/选中权限仍未知；通用交互修复不依赖图片。
- 每完成一个单元立即进行该单元验证并记录，然后继续下一项；不要等待 resume。只有外部输入、权限或用户必须决定的互斥要求才阻塞。

## 产品行为契约

| 操作 | 结果 |
|---|---|
| y / 配置化复制当前单元格键在 Access 内 | 复制当前记录完整带字段名文本，三个分区都支持 |
| v / Enter | 当前记录完整只读弹窗，不触发权限检查或 SQL mutation |
| gg / G | 当前分区第一/最后一条记录，并保持可见 |
| e，结构化 Direct 权限 | 打开当前授权的修改表单，可选择 GRANT/REVOKE 与适用 option |
| e，非直接来源、缺少目标或不支持驱动 | 显示具体只读原因，不生成计划/执行命令 |
| 宽终端 | Target 占据其他列之外所有可用列宽 |
| 内容超过物理宽度 | 表格省略；v/y 保留完整文本 |

修改语义是“修改选中授权”，不是把一行任意改成另一对象/另一 privilege。Target、Privilege 为只读上下文；Operation、Grant option 为真实可操作字段。Member of / Members 本次查看复制导航全支持，修改明确只读，避免把双向 membership 的对象错误地交给现有 principal planner。

## 验证级别与门禁来源

1. **用户需求验收（必须满足的行为）：**y 正确复制、gg/G 首尾导航、Target 利用可用宽度、v 只读详情、e 修改入口及不能修改的准确解释。上表与每个单元完成判据定义验收结果；用户没有要求必须由人工或真实 PTY 逐项操作，自动化 Keymap/App/Command/TestBackend 证据可用于行为验收。
2. **项目既有强制门禁：**`.github/workflows/ci.yml:81–83` 的 fmt、clippy、all-targets/all-features tests；涉及权限修改的 PostgreSQL 数据库契约由同文件 165–166 行的 `LAZYDB_REQUIRE_DATABASE_TESTS=1` 测试验证。这些应在对应实施/CI 环境完成，不在本计划阶段运行。当地缺数据库或工具链只能记录“未验证”，不得将缺环境或 early-return 标作项目门禁通过。
3. **本计划选择的定向自动化验证：**各单元列出的 principal_overview、principal_contract、principal_tabs、keymap、help_omni_panel、mouse 测试是实现回归策略，按修改范围执行。红灯步骤用于证明测试能捕获缺陷；若测试代码依赖新 Action 尚不能编译，先建立可编译接口或使用旧入口复现，不能制造伪业务失败。实现完成的预期结果为退出码 0、实际匹配用例执行且断言通过。
4. **补充建议验证（非新增强制门禁）：**真实终端、操作系统剪贴板粘贴、人工图片对照及手工调整窗口。可用时补充证据；环境受限最多一次针对性修复重试，之后由 Luna 收尾审查决定补证或记录限制。不得仅因缺少这些人工证据无限保持 progress、要求用户代做或阻塞自动流程。

## 预计变更清单

本轮没有业务代码变更。实施阶段预计修改下列精确文件，同目录 change-scope.json 为机器可读清单；没有预计删除、重命名、新依赖或需迁移的未提交业务文件。

- `src/action.rs`
- `src/app.rs`
- `src/input/keymap.rs`
- `src/help.rs`
- `src/model/principal.rs`
- `src/ui/principal.rs`
- `src/ui/principal_mutation_form.rs`
- `src/db/principal.rs`
- `tests/principal_overview.rs`
- `tests/principal_contract.rs`
- `tests/principal_tabs.rs`
- `tests/postgres_principal_mutations.rs`
- `tests/help_omni_panel.rs`

`tests/principal_tabs.rs` 用于来源 tab/generation 与 DDL 不受影响的回归，`tests/help_omni_panel.rs` 用于新增 Access 帮助动作与实际按键一致的回归，因此列入修改范围。`tests/mouse.rs` 仅运行现有回归，不列入；数据库 adapter、runtime、clipboard、CI 配置仅读取/复用，不预计修改。任务目录中的报告/回执是工作流元数据，不是移入工作树的业务文件。本轮不额外生成 docs/plans 文档。若实际编译证明必须修改清单外文件，Luna 先在计划和 change-scope 中补齐最小精确路径，再实施，不扩大成整个 src 目录。

## 单元 1：复制与完整查看闭环

**文件**
- Modify: `src/action.rs`（Principal Access Actions）。
- Modify: `src/app.rs`（principal_access_detail_request、OpenPrincipalAccessDetails、复制 reducer）。
- Modify: `src/input/keymap.rs`（Overview 分支、map_configured_navigation）。
- Modify: `src/help.rs`（Access 查看/复制说明与可执行动作）。
- Test: `tests/principal_overview.rs`。
- Test: `tests/help_omni_panel.rs`（新增 Access 帮助入口回归）、`tests/principal_tabs.rs`（补充 DDL 上下文回归）。

### 步骤

1. 扩展 `app_with_access` fixture：允许设置当前分区、permission 原文/来源/结构化目标，保留 sqlite 内存 fixture 作为不执行数据库的 App 状态测试。添加测试：三个分区 y 返回唯一 WriteClipboard，内容匹配选中记录全部字段；长 Target/含换行内容不取渲染省略文本；空数据不产生 clipboard command。
2. 添加 v/Enter 测试：None mutation_target、Direct/Public/Owner/Default/Inherited 均能打开 TextDetail；不得出现 PrincipalMutationForm 或 mutation Command。直接断言选中行而非第一行，关闭弹窗后 selection 不变。
3. 运行 `cargo test --locked --test principal_overview`，记录与新增用例对应的失败；不要把未建立符号时的编译失败当成业务红灯证据。
4. 提炼 `principal_access_detail_request` 的记录构造逻辑为私有共享入口，返回 title 与完整 text；详情和复制都使用它。新增 `Action::CopyPrincipalAccess`（名称可随既有命名统一）。输出现有 payload：

   ```rust
   Command::WriteClipboard(crate::clipboard::ClipboardPayload {
       text,
       description: "principal access entry".to_owned(),
       sensitive: false,
   })
   ```

   不调用 copy_grid_cell，不修改 active_record_snapshot。空记录给 Access 专属无内容反馈或既有无操作行为，不能显示复制成功。
5. 在配置化导航中，先按 Principal Overview 上下文把 results-copy-cell 转成 Access Action；v 和 Enter 转成 OpenPrincipalAccessDetails，覆盖所有分区。搜索输入/overlay 的优先路由不改变；不把 v 仅放在 Permissions guard 内。移除 v 的 revoke 旧映射。
6. 更新帮助里的复制/查看文案与可执行路由，保证帮助点击和物理按键一致。若现有统一 PrincipalDdl context 不能区分两视图，添加明确 Overview 条件，不在 DDL 视图宣称 Access 行为。
7. 运行 `cargo test --locked --test principal_overview --test principal_tabs`；若修改帮助可执行路由，再跑 `cargo test --locked --test help_omni_panel`。记录实际结果。

**完成判据：**y command 的 payload 与 v 的完整记录一致；只读来源不再影响查看；无系统剪贴板环境也可证明正确的数据传输到后端。

## 单元 2：gg/G 首尾导航闭环

**文件**
- Modify: `src/action.rs`。
- Modify: `src/input/keymap.rs`（Goto prefix dispatch、Overview direct keys）。
- Modify: `src/model/principal.rs`（首尾 selection helper）。
- Modify: `src/app.rs`（当前分区 count + reducer）。
- Modify: `src/help.rs`。
- Test: `tests/principal_overview.rs`、`src/input/keymap.rs` 现有内部测试。

### 步骤

1. 添加 0、1、100 行的三个分区首尾测试：例如 count=100、viewport=10 时，G 后 selection=99、offset=90；gg 后 selection=0、offset=0。按键必须经 Keymap→Action→App，不只直接调用 model helper。
2. 增加 `g` 后切 tab/焦点/打开关闭 overlay、超时/取消、后接非 g 的测试；确保没有首行跳转和授权误触。测试正常 Char('G') + SHIFT 与规范化的大写无修饰事件；搜索编辑状态输入 gg/G 只改变查询。
3. 运行 `cargo test --locked --test principal_overview`，确认新增首尾用例能暴露现有缺陷。
4. 添加 Access 专属首尾 Action（建议 `PrincipalAccessSelectFirst` / `PrincipalAccessSelectLast`，避免把 GridRowTarget 中 ViewMiddle 等无关取值引进来）。model helper 接收 first/last、count、visible_rows，执行：

   ```rust
   let selected = if last { count.saturating_sub(1) } else { 0 };
   self.set_access_selection(selected);
   self.normalize_access_state(count, visible_rows);
   ```

   reducer 仅在 Principal Overview 生效，count 来自当前 section 的 snapshot。按既有显式导航规则处理 access_find。
5. 在 Pending::Goto 的 `gg` 分派中增加 Access 分支，置于 generic fallback 前。沿用 PendingState 对 focus/tab/generation/timeout 的检查；不要把 Principal 加到 is_grid_navigation_focus。
6. Overview 中处理 G 时允许 SHIFT、拒绝 Ctrl/Alt 等组合。删去不可达/冲突的单 g 授权入口，将其功能统一交给后续 e 表单。gt/gT 等通用序列仍保留。
7. 更新提示；运行 `cargo test --locked --test principal_overview` 和 `cargo test --locked --lib input::keymap::`。若测试筛选输出 0 tests，修正一次筛选后按实际模块名执行，不能把零测试当作通过。

**完成判据：**三分区首尾位置及 viewport 正确；prefix 生命周期不串界面；既有其它 grid 与 tab 组合键无回归。

## 单元 3：Target 自适应及列对齐闭环

**文件**
- Modify: `src/ui/principal.rs`（shorten、access_header、access_line）。
- Test: `tests/principal_overview.rs`（TestBackend）。

### 步骤

1. 增加渲染测试：在相同记录上分别使用 80/120/180 列终端；Target 长于 28、但短于 body 可用宽度时必须完整显示。用 body 的实际起点核对 Target/Privilege/Origin 的表头和数据起点。
2. 添加窄屏、原 56 断点附近、空宽度、长中文、组合字符、刚好放满/多出一格、带控制字符数据。复用现有滚动条和搜索测试证明 body/hit region 未偏移。
3. 运行 `cargo test --locked --test principal_overview`，保存能体现旧 28 上限的失败。
4. 提炼私有布局值供表头和行共享：wide 模式预留 prefix=2、gaps=4、privilege=15、origin=9，Target=`usize::from(body_width).saturating_sub(30)`；宽度使用传入 body_width，不再次按整个 frame 计算。断点处总宽始终不超过 body_width。
5. 用按 display cells 的截断/补空格 helper 替换固定字符 padding。先 sanitize、把行内换行/tab 投影为可见单行形式，再预留省略号的一个 cell，最后补到指定终端宽度。width=0 返回空字符串；width=1 返回可容纳的一格内容或省略号。避免现有 `>=` 带来的多截一格。保留 Unicode 零宽字符，不新增依赖，优先复用项目已有满足契约的 helper。
6. wide 表头和记录共享同一列配置，保留 source style、selection background 和 find highlight。窄屏只按整个 Entry 宽度截断，移除 Target 的预先 28 限制。保持一记录一行，不引入 wrap。
7. 运行 `cargo test --locked --test principal_overview --test mouse`；记录关键宽度与 render 断言。

**完成判据：**宽屏空白被 Target 列利用，容量内原文完整；超宽内容合理省略；表头/数据对齐；滚动条、选择和搜索没有布局回归。

## 单元 4：e 修改选中授权闭环

**文件**
- Modify: `src/action.rs`（编辑入口与表单字段操作）。
- Modify: `src/app.rs`（OpenPrincipalPermissionMutation、表单 reducer、ConfirmPrincipalMutationForm）。
- Modify: `src/input/keymap.rs`（e 与 overlay 输入）。
- Modify: `src/model/principal.rs`（PrincipalMutationForm 状态/字段）。
- Modify: `src/ui/principal_mutation_form.rs`。
- Modify: `src/db/principal.rs`（draft.mutation 的 grant_option 传递）。
- Modify: `src/help.rs`。
- Test: `tests/principal_overview.rs`、`tests/principal_contract.rs`、`tests/postgres_principal_mutations.rs`。
- Test: `tests/principal_tabs.rs`（源 tab 关闭/切换及 generation 失效回归）。
- 如编译影响已有 form 构造点，在上述文件及现有 principal 测试中同步；不扩大至各驱动新对象支持。

### 步骤 A：入口和可编辑性

1. 添加 e 的行为用例：结构化 Direct 进入表单，v 仍为详情；None target、非 Direct、驱动能力不足、Membership 分区分别不产生变更命令。
2. 在 app 层集中判断原因，先说明 source 的语义限制，再说明结构化目标缺失，再检查精确 driver capability；不以笼统的 DdlOnly 或连接用户权限替代判断。文案至少区分：所有权、PUBLIC、继承、默认 ACL、不支持的结构化目标/驱动、未加载/空记录。
3. e 使用单一入口。默认 Operation=GRANT、option=当前 grantable，并明确显示当前 Target/Privilege；用户选择 REVOKE 才生成撤销。删除 g/v 的旧修改入口，但保留其它 API caller 所需兼容 Action，避免无关重构。

### 步骤 B：表单是真正可操作的状态

4. 将 overlay 表单与 source context 绑定：捕获 tab id/generation、connection identity、principal、details.database。提交前核对连接 generation 和源 tab 仍有效；不从后来激活的 tab/profile.database 推断目标。请求 ID 使用并更新来源 tab 的计数器，不只计算 next+1 后丢弃。
5. Target 与 Privilege 只读，明确显示完整值（空间不足采用既有可滚动/单独详情模式或可用宽度投影）。可聚焦字段只保留 Operation 与 GrantOption；Tab/BackTab 正反循环，Space 按选中字段切换。隐藏与 Permission 无关的 Role/Admin，不能再 Tab 到无效字段。
6. REVOKE 的 option 明确标为 `Grant option only`。修正 PrincipalMutationDraft::mutation，使 Permission Revoke 使用 draft.grant_option，而不是固定 false；单测断言 option=true 输出 Revoke{grant_option:true}。从 GRANT 切到 REVOKE 时将 option 默认重置为 false，避免之前 grantable 状态默默变成另一含义；此后用户可主动开启。切回 GRANT 也使用明确状态，UI 标签随操作改变。
7. 现有 tab.mutation_draft 与 Overlay form 为两套不同状态；新输入仅更新当前 overlay，移除本次路径中的误导分支或改为复用方法，不能新增第三套状态。

### 步骤 C：预览、执行与刷新

8. 添加 App 命令测试：e 和编辑字段时无 SQL；Enter 只发 PlanPrincipalMutation；plan ready 默认 Cancel；Esc/Cancel 无 Execute；选择 Apply 才发 ExecutePrincipalMutation；success 刷新源 principal 的 DDL/details。加入 source tab 改变/关闭、连接换代后提交的拒绝或取消测试。
9. 用捕获的 source context 构造 PrincipalMutationRequest。继续走既有 Plan→Review→Confirm→Execute→Refresh，不新增直接执行 SQL 分支。已有执行后端负责访问权限等约束；不绕过它。
10. 运行 `cargo test --locked --test principal_overview --test principal_contract --test principal_tabs`。若 planner SQL 行为受 option 修改影响，扩展 postgres_principal_mutations 的 grant-option/revoke-option round trip，检查权限本体保留而 grantable 消失，最后回收测试对象。
11. 使用隔离测试库，环境存在时运行：

    ```sh
    LAZYDB_REQUIRE_DATABASE_TESTS=1 cargo test --locked --test postgres_principal_mutations -- --nocapture --test-threads=1
    ```

    需要 `LAZYDB_TEST_POSTGRES_URL`。不得在用户真实主体上试写。没有环境则记录“未执行/受限”，不能用测试默认 early-return 冒充数据库成功。
12. 更新帮助：e 编辑当前直接授权、表单 Operation/Option 的真实按键；不再宣传 g 授权/v 撤销。

**完成判据：**支持的直接权限经 e 可修改并确认执行；v 无写副作用；只读情况原因明确；所有显示为可操作的字段确实改变 mutation；原始主体/连接/数据库绑定正确。

**实施复核重点：**单元 1 检查空选择/复制重绑定和帮助调用路径；单元 2 检查全局 g 前缀优先级及 SHIFT 事件；单元 3 检查 body_width 扣减只有一次、表头与正文共用布局；单元 4 检查入口 capability 判断不误用粗粒度 DdlOnly、overlay 与 tab draft 不串状态、request_id 真正递增，以及 REVOKE option 与 SQL 语义一致。各单元结束由 Luna 对照实际 diff 复核后继续，不切回 Astra，不新增执行方式选择。

## 单元 5：整体核验与交接

**文件**
- Review: 上述实际 diff。
- Update: 本任务目录 `validation.md`，仅记录实际执行结果。

1. 从实际 diff 逐条核对四项产品需求与上面各单元完成判据；重点检查普通数据表、DDL、搜索编辑、帮助路由未被 Access 专属按键误伤。
2. 功能齐备后执行一次项目 Rust CI 对应检查：

   ```sh
   cargo +1.94.0 fmt --all -- --check
   cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
   cargo +1.94.0 test --all-targets --all-features
   ```

   每条独立执行并记录退出码。格式失败先定向格式化修改文件；编译/测试失败自行修复，仅重跑受影响检查，不能因本轮结束要求 resume。
3. 实际 PTY/系统剪贴板是补充检查：可用时验证 y 复制、v 长文本、gg/G、宽度变化和 e 取消。无法运行时记录具体环境限制，最多一次针对性修复重试，然后由 Luna 收尾审查决定补证据或接受限制。
4. 验证记录必须含 command、exit、相关文件、commit/diff 状态、环境、数据库用例是真执行还是跳过；不得沿用 analyze 静态检查替代实现测试。
5. Luna 为任务确定名称/分支并按当前执行阶段协议审查、提交和合并。提交只 stage 本任务明确修改文件，不用 `git add .` 带入其他用户工作。各单元可作为逻辑 commit 边界；执行合并前重新检查目标分支变化，不改写历史。

## 最终范围确认

- 这份计划足以从起点提交独立实施；下一项具体动作是单元 1 的 Keymap→App→WriteClipboard/TextDetail 回归用例。
- 本轮不实现业务代码，不创建 worktree，不提交合并；plan 文档完成即结束本阶段。
- 不承诺新增序列、函数、默认 ACL、跨驱动所有权限编辑。如果后续截图或明确需求提出这类能力，必须基于原生结构化身份扩展 model+adapter+集成测试，不能删除现有检查强行生成 SQL。
