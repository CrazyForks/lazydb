# Oracle 建表类型与表单能力优化 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. 若执行环境未安装该技能，按本文件任务顺序执行并记录每项验证结果。

**Goal:** 修复 Oracle 新增列默认使用 TEXT 导致 ORA-00902 的问题，在表单和计划生成边界提供一致的类型校验，并完善字段能力与错误展示。

**Architecture:** 在数据库层新增轻量列类型策略，由打开编辑器时确定的目标数据库上下文驱动草稿初始化、校验和类型提示。保留 native_type 为用户明确选择的原生 SQL 类型，Oracle 计划生成独立使用 Oracle 策略复核输入，UI 消费共用校验结果。沿用现有 CatalogMutationCapabilities、列编辑会话和预览执行链路。

**Tech Stack:** Rust 1.94 / edition 2024、ratatui 0.30、现有 oracle 0.6.3 驱动、Cargo 集成测试。

---

## 一、已确认的依据

行号基于编写计划时的工作区，执行前按符号定位。

| 位置 | 已确认行为 |
| --- | --- |
| `src/model/catalog_editor.rs:2224`，`ColumnDraft::new_added` | 类型固定为 `text` |
| 同文件 `:1491`，`TableDraft::new` | 第一列调用相同构造函数 |
| 同文件 `:1863`，`begin_add_column_below` | 后续列也调用相同构造函数 |
| 同文件 `:2212`，`is_empty_added_column` | 用 `text` 判断空白列 |
| 同文件 `:1525`、`:1910` | 整表及列详情只验证类型非空 |
| 同文件 `:2848`、`:2990` | 编辑器尚无类型策略上下文；从 schema 创建表草稿 |
| `src/app.rs:6701`、`:6853`、`:18239` | 编辑器构造调用点 |
| `src/app.rs:7460` | 列详情确认时已有字段错误与焦点处理入口 |
| `src/db/oracle.rs:132` | 通用校验后原样拼接 native_type |
| 同文件 `:251` | 表编辑只实现重命名 |
| `tests/object_mutation_contract.rs:698` | Oracle 建表测试手动设置 NUMBER，未覆盖默认类型 |
| `src/security.rs:50` | 保留安全换行；不能认定该函数造成 `datatypeHelp` 粘连 |
| `tests/oracle_adapter.rs:14` | 现有真库测试依赖环境变量，缺少配置或部分客户端错误时会提前返回 |

## 二、设计决策

1. Oracle 新增普通字符串列默认 `VARCHAR2(255 CHAR)`；该长度是可编辑的产品默认值。
2. PostgreSQL、MySQL、SQLite、SQL Server 的现有默认行为先保留，通过共用策略显式表达。Redis 不进入关系表草稿入口。
3. 默认类型来自创建编辑器的目标连接，而不是后续切换后的活动 profile；编辑器生命周期内上下文保持稳定。
4. 第一列、后续列和从定义加载后的列编辑使用相同上下文；从定义加载时不改写已有类型。
5. `native_type` 不做 TEXT→CLOB、VARCHAR→VARCHAR2 的静默替换。建议由用户明确采纳，预览与执行使用同一最终计划。
6. 本地校验只对确定错误阻断。未收录类型返回“需数据库确认”，不伪装成“已验证合法”。
7. 未限定的 TEXT 在 Oracle 表单中作为常见跨方言误用阻断；若确有名为 TEXT 的自定义类型，允许使用正确引用或 schema 限定形式表达。后续元数据解析可支持该类未限定名称。
8. 不以固定白名单拒绝全部自定义类型。不把 NUMBER 的合法负 scale、scale 大于 precision 或合法星号形式误判为非法。
9. VARCHAR2 字符数不等于数据库字节上限；长度上限与字符集、MAX_STRING_SIZE 有关。缺少能力数据时仅做确定的语法和参数检查，不能把 4000 字符宣称为普遍合法上限。
10. 新建字段能力和编辑字段能力分别声明；现阶段 Oracle 表编辑准确体现 rename-only。

## 三、任务依赖与交付点

```text
任务 1 回归复现 → 任务 2 类型策略 → 任务 3 草稿上下文 → 任务 4 双层校验
                                                     ↓
                                           任务 5 核心真库验收（M1）

任务 2 + 4 → 任务 6 类型提示与候选项
任务 4     → 任务 7 字段能力一致性
任务 4     → 任务 8 错误展示
任务 5～8  → 任务 9 完整验收与文档（M2）
```

M1 是可独立交付的缺陷修复：默认路径合法、明显错误前置、已有行为回归通过。
M2 是完整体验优化：类型选择、能力一致性、可读错误和完整验收。

## 任务 1：补齐真实默认交互路径的回归用例

**Files**
- Modify/Test: `tests/catalog_editor_state.rs`
- Modify/Test: `tests/catalog_editor_reducer.rs`
- Modify/Test: `tests/object_mutation_contract.rs`

**步骤**
1. 阅读现有测试构造器，复用 Oracle profile、schema anchor、编辑器 action 和建表请求 fixture。
2. 增加 `oracle_new_table_default_column_type_is_native`：通过应用动作打开 Oracle 建表表单，只填写表名、列名，验证默认类型适用于 Oracle。
3. 增加 `oracle_additional_column_uses_same_type_policy`：新增第二列并确认，验证类型与第一列遵守同一策略。
4. 增加 `oracle_create_rejects_bare_text_before_execution`：直接向 Oracle planner 传入 TEXT 列草稿，断言 InvalidDraft 包含列名及建议。
5. 保留既有 NUMBER、标识符转义及目标 schema 测试。
6. 运行下述命令，记录以上新增用例因当前行为而失败；不要仅以未来接口不存在导致的编译错误作为复现依据。

```bash
cargo +1.94.0 test --test catalog_editor_reducer --test catalog_editor_state --test object_mutation_contract oracle_
```

**验收**：至少一个用例在现有公开行为下稳定复现默认 TEXT 问题。与后续修复一并提交，保持可合并提交通过测试。

## 任务 2：引入轻量数据库列类型策略

**Files**
- Create: `src/db/column_type.rs`
- Modify: `src/db/mod.rs`
- Create/Test: `tests/column_type_policy.rs`

**拟定接口契约**（名称可按仓库风格微调，语义固定）

| 接口/类型 | 职责 |
| --- | --- |
| `ColumnTypePolicy::for_database(DatabaseKind)` | 显式选择关系数据库类型策略；不支持关系表的种类返回不可用 |
| `default_native_type()` | 返回新增列默认值 |
| `suggestions()` | 返回候选原生类型、说明和可编辑示例 |
| `validate_native_type(&str)` | 返回已知合法、待数据库确认、明确错误三种结果 |
| `ColumnTypeIssue` | 保存稳定错误代码、原因、建议；不包含 UI 焦点类型 |

**步骤**
1. 为默认值和 Oracle 参数规则编写表驱动测试。
2. 实现默认值及常见候选项：VARCHAR2、NUMBER、DATE、TIMESTAMP、CLOB、BLOB、CHAR、NVARCHAR2、RAW。
3. 实现窄范围类型声明解析，正确识别大小写、空白、括号、参数、限定名和带引号标识符；不要在完整 SQL 上做字符串替换。
4. 校验 VARCHAR2/NVARCHAR2 等必需长度、正数长度、NUMBER 参数格式与已知范围、TIMESTAMP 可识别的精度格式。
5. 对普通 VARCHAR 给出非阻断的 VARCHAR2 建议；允许合法输入原样进入计划。
6. 对未知或版本相关类型返回待数据库确认；对引号/限定自定义类型保留原文。全局安全检查不以未知类型为绕过入口。
7. 执行规则测试并检查无 Oracle 驱动 feature 时模块可编译。

**必要用例**
- TEXT/text/两端空白：明确错误，提供 VARCHAR2/CLOB 建议。
- VARCHAR2(20 CHAR)、VARCHAR2(20 BYTE)、NUMBER、NUMBER(18,2)、DATE、CLOB：通过。
- VARCHAR2、VARCHAR2(0)、NUMBER(39,0)：明确错误。
- NUMBER(5,-2)、NUMBER(4,5)、NUMBER(*,2)：按 Oracle 合法语义处理。
- 带 schema、带引号、自定义类型、未知版本相关类型：不因白名单缺项被误拒绝。
- 解析不支持的合法复杂声明：返回待确认而非虚构错误。

```bash
cargo +1.94.0 test --test column_type_policy
cargo +1.94.0 check --no-default-features
```

**建议提交**：`feat(catalog): add database-aware column type policies`

## 任务 3：把类型策略贯穿编辑器与草稿生命周期

**Files**
- Modify: `src/model/catalog_editor.rs`
- Modify: `src/app.rs`
- Modify/Test: `tests/catalog_editor_state.rs`
- Modify/Test: `tests/catalog_editor_reducer.rs`
- Update callers: 编译器指出的 `TableDraft`、`CatalogEditorState` 构造和结构体字面量调用点

**步骤**
1. 在 CatalogEditorState 与 TableDraft 保存显式类型策略或小型上下文值，避免仅依赖活动连接的全局状态。
2. 更新编辑器创建入口、对象选择初始化和定义加载入口，传入目标 profile 对应策略。
3. 更新 TableDraft::new、from_definition 和 ColumnDraft::new_added，使用显式默认值；尽量用编译器强制调用方补齐上下文，避免隐式 PostgreSQL fallback 掩盖遗漏。
4. 更新 begin_add_column_below，从当前 TableDraft 策略创建列。
5. 将 is_empty_added_column 改为与同策略的空白列语义比较，排除 UUID、ordinal 等身份字段；类型以初始默认值为参照。
6. 保持已有类型、nullable/default/comment 值和用户已编辑值不被上下文初始化覆盖。
7. 增加首列、后续列、打开/取消列会话、空白变更摘要、切换 profile 后上下文稳定性的用例。
8. 核对 Oracle rename-only 编辑器仍可加载既有定义；暂未支持的新增列动作在任务 7 禁用。

```bash
cargo +1.94.0 test --test catalog_editor_state --test catalog_editor_reducer --test catalog_mutation
```

**验收**：Oracle 默认列为 VARCHAR2(255 CHAR)，空白表单无虚假修改；其他关系数据库现有行为不回归。

**建议提交**：`fix(catalog): initialize column drafts with target database types`

## 任务 4：接入列详情与 Oracle planner 双层校验

**Files**
- Modify: `src/model/catalog_editor.rs`
- Modify: `src/app.rs`
- Modify: `src/db/oracle.rs`
- Modify: `src/db/catalog_mutation.rs`（仅在现有错误结构无法传递必要上下文时）
- Modify/Test: `tests/object_mutation_contract.rs`
- Modify/Test: `tests/catalog_editor_reducer.rs`

**步骤**
1. validate_column_details 在现有必填、重名和 identity/default 校验后调用共用类型规则，映射为 TableColumnField::Type。
2. 复用 app 的现有确认失败路径：保留列编辑会话、保留输入、显示具体原因、聚焦 Type，不发送计划/执行命令。
3. 整表预览前校验所有未删除列，定位第一个错误；必要时打开对应列编辑会话。
4. OracleAdapter::plan_catalog_mutation 在生成 SQL 前独立使用 Oracle 策略验证全部有效列；不能信任草稿自称的数据库类型。
5. 将错误包装为已有 InvalidDraft，带列名、输入类型和建议。优先保持现有 runtime 错误传播协议。
6. 对待数据库确认的类型允许生成计划；可用现有提示机制明确未验证部分。
7. 添加直接 planner 调用、删除列跳过、多个错误定位第一项、错误修正后继续预览、旧计划失效的回归用例。
8. 确认 native_type 未被改写，预览及 execute_catalog_mutation 消费同一计划。

```bash
cargo +1.94.0 test --test column_type_policy --test object_mutation_contract --test catalog_editor_reducer
```

**验收**：截图中的 TEXT 在确认/计划阶段收到可操作提示；合法 VARCHAR、限定自定义类型不被静默改写。

**建议提交**：`fix(oracle): validate column types before planning table creation`

## 任务 5：核心修复的真实 Oracle 验收（M1）

**Files**
- Modify/Test: `tests/oracle_adapter.rs`
- Modify/Test: `tests/object_mutation_contract.rs`

**步骤**
1. 增加 `oracle_create_table_default_types_round_trip`，使用既有 LAZYDB_TEST_ORACLE_URL/USER/PASSWORD 连接配置与目标 schema。
2. 测试从新草稿构造两列，均保留默认类型；通过实际 mutation plan 执行 CREATE TABLE。
3. 使用唯一且兼容旧 Oracle 标识符长度限制的测试表名，通过系统字典确认两列实际类型、字符长度及 CHAR 语义。
4. 插入并读取包含中文的样例名称，验证所选默认声明可用。
5. 将操作结果保存后执行清理，再断言操作结果；失败时也尝试删除测试创建的对象，清理失败需报告表名。
6. 不依赖 DDL 事务回滚清理，不复用现有测试中固定 supportdb 的断言。
7. 新增真库用例在未配置时明确输出跳过原因；配置已提供但连接或客户端初始化失败时应失败，避免虚假通过。
8. 运行以下命令并分别记录“执行成功”与“环境未配置跳过”。

```bash
cargo +1.94.0 test --test oracle_adapter oracle_create_table_default_types_round_trip -- --nocapture
```

**验收**：有配置时完成真实建表、元数据检查、中文往返及清理。无配置时 M1 标明真库验证待完成，不能以 Cargo 退出码 0 宣称 Oracle 已验证。

**建议提交**：`test(oracle): cover default table creation against a real database`

## 任务 6：类型提示与候选选择

**Files**
- Modify: `src/ui/catalog_editor.rs`
- Modify: `src/model/catalog_editor.rs`
- Modify: `src/app.rs`
- Modify: `src/input/keymap.rs`
- Modify/Test: `tests/catalog_editor_reducer.rs`
- Modify/Test: `tests/keymap.rs`
- Modify/Test: `tests/ui_render.rs`

**步骤**
1. 在列详情 Type 区域显示当前数据库和简短类型示例。
2. 新增轻量类型候选状态，复用现有 picker 的过滤/选择习惯，内容来自任务 2 策略。
3. 先检查现有键位冲突，再绑定候选入口，并在 footer 展示实际快捷键。
4. 候选接受只更新当前列会话的 native_type；保留自由编辑、粘贴、undo/redo；接受候选后仍需确认列。
5. NUMBER、VARCHAR2 等候选给出合法可编辑示例，避免插入缺少长度的 VARCHAR2。
6. Esc 关闭候选但不关闭列详情；后续 Esc 按现有规则取消列会话。
7. 测试候选过滤、接受、取消、焦点、窄终端显示和自由输入自定义类型。

```bash
cargo +1.94.0 test --test catalog_editor_reducer --test keymap --test ui_render
```

**验收**：用户可以方便选取 Oracle 常用类型，也可输入合法原生声明。

**建议提交**：`feat(catalog): provide native column type suggestions`

## 任务 7：让 Oracle 字段能力与实际 SQL 生成一致

**Files**
- Modify: `src/db/catalog_mutation.rs`
- Modify: `src/db/oracle.rs`
- Modify: `src/model/catalog_editor.rs`
- Modify: `src/ui/catalog_editor.rs`
- Modify: `src/app.rs`
- Modify/Test: `tests/object_mutation_contract.rs`
- Modify/Test: `tests/catalog_editor_reducer.rs`
- Modify/Test: `tests/ui_render.rs`

**步骤**
1. 在现有能力模型上增加表字段/操作级能力，create 与 edit 分开，提供不可用原因。
2. Oracle create 开放已实现的列名、类型、默认值、nullable；表名可编辑，schema 明确来自选中 anchor。
3. identity、generated expression、collation、owner/comment 等尚未落实的属性标记不可用；不在此任务中顺带实现多语句注释或 identity DDL。
4. Oracle edit 仅开放表名；列新增、删除和属性修改禁用，焦点导航跳过禁用动作。
5. planner 兜底拒绝非默认的不支持创建属性；edit 对比 baseline，只拒绝实际发生的不支持修改，不能因已有表含 identity/comment 就拒绝正常重命名。
6. schema、owner 等草稿内容与目标 anchor 的关系明确校验，避免显示可编辑但执行忽略。
7. 所有其他适配器根据真实既有能力显式初始化新增字段，避免新增默认值让既有功能意外消失。
8. 回归“rename + 修改类型”必须报错、只 rename 成功、默认不可用属性不阻断普通建表。

```bash
cargo +1.94.0 test --test object_mutation_contract --test catalog_mutation --test catalog_editor_reducer --test ui_render
```

**验收**：用户可操作的字段与生成 SQL 一致，直接调用 planner 也不能静默忽略修改。

**建议提交**：`fix(oracle): enforce supported table mutation fields`

## 任务 8：改善类型错误与服务端错误展示

**Files**
- Modify: `src/ui/catalog_editor.rs`
- Modify: `src/db/oracle.rs`（仅在确认驱动消息格式处理需要调整时）
- Modify: `src/ui/mod.rs`（仅在 toast 布局确需修改时）
- Modify/Test: `tests/ui_render.rs`

**步骤**
1. 获取真实 ORA-00902 驱动消息或精确 fixture，沿 oracle_error → editor.error → preview/toast 定位 Help 粘连发生的位置。
2. UI 将本地类型错误显示为列名、类型、原因、建议；服务端消息展示错误码及原始安全文本。
3. 将原始消息按行转换为渲染行，支持 Wrap 与实际显示行数驱动的滚动，避免将多行消息塞入单个 Span。
4. toast 使用简短摘要；完整文本在预览区保留，帮助 URL 独立成行。
5. 没有字段定位信息时不从 ORA-00902 猜测具体列；避免为解决一个消息格式问题全局替换字符串 Help。
6. 在 80 列和宽终端测试，确认错误详情不覆盖 SQL 和底部快捷键。
7. 保持 sanitize_terminal_text 对控制字符的现有处理，仅对布局做必要变更。

```bash
cargo +1.94.0 test --test ui_render
```

**验收**：错误和文档链接可读，长错误可查看，正确区分本地确定信息与数据库未提供的信息。

**建议提交**：`fix(catalog): render actionable multiline mutation errors`

## 任务 9：完整验证、说明文档与交付（M2）

**Files**
- Create: `docs/oracle-table-editor.md`
- Update: 本计划的执行记录

**步骤**
1. 编写 Oracle 建表说明：VARCHAR2 与 CLOB 选择、默认 255 CHAR 的含义、VARCHAR 建议、自定义类型输入、当前 rename-only 编辑能力。
2. 核对 M1 和 M2 所有行为验收项；对新改动运行对应定向测试。
3. 按仓库 CI 执行最终格式、静态检查和全量测试，不在无新增改动或失败时反复重跑。
4. 再检查无 driver-oracle 编译，确保通用表单和策略模块不绑定客户端库。
5. 完成手工流程：新建两列 → 默认类型 → 预览 → 执行 → Explorer 刷新；输入 TEXT → 定位错误 → 修正 → 成功；取消列/候选 → 正确恢复；已有表仅重命名。
6. 在最终交付中列出修改文件、关键行为、实际测试结果及真库是否执行。

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
cargo +1.94.0 check --no-default-features
```

**建议提交**：`docs(oracle): document table types and editor capabilities`

## 四、后续能力扩展的明确触发条件

- 需要在本地精确验证扩展 VARCHAR2 上限或版本相关类型时，再接入服务端版本、字符集、MAX_STRING_SIZE；当前返回待数据库确认。
- 需要补全用户自定义类型时，再对接有权限可见的 Oracle 类型元数据，并按 connection identity/schema 缓存和失效。
- 需要支持 identity、comment 或 ALTER COLUMN 时，按各自 DDL 能力单独实现；若引入多条 Oracle DDL，必须同时处理已完成步骤与部分失败反馈。

## 五、执行记录

- 计划编写完成：2026-09-14。
- 实现状态：未开始。
- 测试状态：本次仅制定计划，未运行测试。
- 真库验证：待实施阶段配置并执行。
- 工作区已有 `src/ui/data_grid.rs` 修改；实施时保留该工作，并在全量检查遇到相关问题时单独识别来源。
