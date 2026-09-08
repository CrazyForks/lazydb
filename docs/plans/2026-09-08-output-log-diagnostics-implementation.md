# OUTPUT LOG 样式与错误诊断实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. 若执行环境没有该技能，按本文顺序逐项实施与验证，不调用不存在的技能。

**Goal:** OUTPUT LOG 的时间使用低调主题色，错误正文与诊断续行使用错误主题色，并完整展示 PostgreSQL 返回的 SQLSTATE、severity、Position、Detail、Hint 和 Context。

**Architecture:** 保留 OutputEntry 文本、SQL 范围和只读编辑器，增加显式时间范围，在 UI 层叠加日志前景色，不改变 SQL 分析器。DatabaseError 保留可选结构化诊断，通过仅用于 Console 输出的格式化入口传递详细错误，不改变全局 Display 和关系编辑脱敏行为。

**Tech Stack:** Rust 2024、SQLx 0.9、ratatui 0.30、现有 EditorWorkspace/modalkit、Rust 单元与集成测试；不新增生产依赖。

---

## 范围与约定

- 时间及方括号使用 theme.muted；错误行除时间外使用 theme.error，包括诊断续行。
- SQL 保留语法高亮；执行上下文和成功统计保持现有正文色，不将成功 SQL 整段染绿。
- 复用主题字段，不硬编码 RGB，不叠加 DIM，不引入主题配置项。
- 保留搜索、光标、键盘/鼠标选择、复制、水平滚动和现有追加定位行为。
- 不实现自动折行、错误跳转、SQL 波浪线、日志持久化或通用日志框架。
- 时间继续采用当前 UTC 语义；本地时间转换列为独立后续事项，本次不隐式改变。
- 详细诊断只进入用户 SQL Console 输出。不扩大 agent/MCP 返回字段，不扩大通知内容，不破坏 relation mutation 的错误脱敏。
- 本计划不授权 commit、push、创建数据库连接、操作生产数据或覆盖其他工作区修改。
- 行号仅用于定位；实施前按符号检查当前代码，特别注意与 output-log-follow-tail 计划重叠的文件。

## 最终输出契约

```text
[2026-09-08 11:08:17:413] moss_biz.tools> SELECT * FROM sdfsdf;
[2026-09-08 11:08:17:413] [42P01] ERROR: relation "sdfsdf" does not exist
[2026-09-08 11:08:17:413] Position: 15
```

- 同一次失败追加的 SQL 上下文和所有诊断行使用同一个时间值，时间是当前完成/记录时间，不声称是服务器时间。
- 错误首行格式为 `[code] SEVERITY: message`；无 code 时省略方括号部分，无 severity 时使用 ERROR。
- 可选字段顺序固定为 Detail、Hint、Position 或 Internal Position、Internal Query、Context；不存在或为空时省略。
- 多行 Detail/Hint/Context 保留原始换行，每个物理行统一补时间，不截断原始文本。
- Position 保留服务器返回的数值；PostgreSQL 的原始位置按 1 起始字符位置解释，不转换为 UTF-8 字节偏移。
- 内部 SQL 位置明确标记 Internal Position，并在有内部 SQL 时显示 Internal Query，不映射到用户 SQL。
- 分页、COUNT 或其他生成 SQL 的 Position 属于实际发送给服务器的 SQL。本期不映射到原始编辑器；生成 SQL 失败处增加明确上下文说明，避免暗示可直接在原 SQL 中定位。
- 非数据库的本地执行错误同样标红并补时间；使用明确的 ERROR 标记，但不对任意错误文本执行猜测式解析或重复补前缀。

## 已确认的接线位置

| 位置 | 当前行为 | 本次处理 |
| --- | --- | --- |
| src/db/mod.rs:74 DatabaseError | category/code/message，Display 仅 message | 增加可选诊断；Display 不变 |
| src/db/mod.rs:108 from_sqlx | 只提取通用 message/code | 提取 PostgreSQL 原生诊断 |
| src/runtime.rs:1944 run_query | error.to_string() 进入 QueryFailed | 使用 Console 专用格式化 |
| src/runtime.rs:2138 起分页失败分支 | 多个阶段转为字符串 | 区分服务器错误与本地构造错误 |
| src/db/transaction.rs:110 | DatabaseError 转为 TransactionError 时仅保留 Display | 仅 Console execute 路径显式保留诊断 |
| src/db/transaction.rs:88 | relation diagnostic 是已脱敏的桥接格式 | 保持原样，不附带 DETAIL/值 |
| src/runtime.rs:2824/2890 | 手动执行直接转发 error.0 | 在此前的数据库执行边界完成格式化 |
| src/model/tab.rs:143 OutputEntry | kind/message/sql_range | 增加显式时间范围 |
| src/app.rs:164/172 | 拼接文本与全局 SQL 范围 | 保持 SQL 范围正确，补日志样式投影 |
| src/app.rs:16080 起 | SQL、成功、失败和事务日志构造 | 统一时间范围构造，不用正则识别 |
| src/ui/mod.rs:3311 editor_line_spans | SQL 前景色与选择背景合成 | 增加可选前景色覆盖 |
| src/ui/mod.rs:3755 render_output | 只读快照渲染 | 根据 OutputEntry 生成可见行覆盖 |

## Task 1: 固定回归用例与基线

**Files:** src/app.rs 的测试模块、tests/ui_render.rs、src/model/tab.rs 的测试模块。

1. 检查 git status 和相关 diff；保留现有未跟踪计划文件，不修改其他任务内容。
2. 用现有失败 Action fixture 构造一次失败执行，消息为 `[42P01] ERROR: relation "sdfsdf" does not exist\nPosition: 15`。
3. 新增 `output_log_` 前缀回归测试，断言 SQL 上下文、两行错误内容和时间前缀均存在，错误 entry 的 kind 为 Error。
4. 在 ratatui TestBackend 中检查字符 cell 的 fg，不只检查终端文本包含关系；分别断言时间为 muted、错误正文及 Position 为 error、SELECT 保持 syntax_keyword。
5. 执行 `cargo test --lib output_log_` 和 `cargo test --test ui_render output_log_`，记录符合预期的失败。新增 API 尚未实现导致的编译错误不当作行为回归证据。
6. 记录现有相关测试基线：`cargo test --lib editor::tests`、`cargo test --test sql_execution`、`cargo test --test transaction_reducer`。

**验收:** 测试能够独立捕获时间样式、错误样式与错误续行时间缺失；环境导致的原有失败单独记录。

## Task 2: 保留结构化数据库诊断

**Files:** src/db/mod.rs；受 DatabaseError 结构体构造影响的 src/db/postgres.rs、src/db/mysql.rs、src/db/sqlite.rs、src/db/mssql.rs 及实际编译报错位置；tests/postgres_adapter.rs。

1. 在 db 模块增加诊断类型，不保存 SQLx 借用值，不引入 ratatui 类型。推荐的数据契约如下：

```rust
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DatabaseDiagnostic {
    pub severity: Option<String>,
    pub detail: Option<String>,
    pub hint: Option<String>,
    pub position: Option<DatabaseErrorPosition>,
    pub context: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DatabaseErrorPosition {
    Original(usize),
    Internal { position: usize, query: String },
}
```

2. 为 DatabaseError 增加 `diagnostic: Option<DatabaseDiagnostic>`。原有 code/message/category 的语义不变；所有非原生诊断构造明确初始化为 None。检查外部使用与序列化转换，避免意外扩大公共输出。
3. 在 from_sqlx 的 Database 分支使用安全的具体类型下转取得 PgDatabaseError；提取 severity/detail/hint/position/context。实施时核对已锁定 SQLx 0.9 源码或文档中的具体方法名，不使用会在其他驱动上 panic 的下转。
4. 所有来自服务器的字符串进行 sanitize_terminal_text；保留合法换行。保留原有错误分类逻辑，不顺带重写 category 推断。
5. 保持其他驱动的通用 code/message 行为，不把 MySQL 数字错误码错误描述成 SQLSTATE，也不伪造 Position。
6. 增加无数据库依赖的模型与清洗测试；真实 PostgreSQL 提取测试沿用 LAZYDB_TEST_POSTGRES_URL 门控。
7. PostgreSQL 测试使用唯一不存在的 schema 下的只读 SELECT，稳定产生 42P01，并断言 severity/message/Original position。不要假定已有业务表名不存在。
8. 执行 `cargo check --all-targets`、`cargo test --lib output_diagnostic_`、`cargo test --test postgres_adapter output_diagnostic_`。

**验收:** 普通错误仍保持原 Display；PostgreSQL 转换保留诊断；未设置测试 URL 时明确记录集成测试跳过，不能宣称驱动验证通过。

## Task 3: 专用日志错误格式化与安全边界

**Files:** src/db/mod.rs；tests/agent_serialization.rs、tests/agent_security.rs、tests/postgres_relation_mutations.rs 作为回归覆盖。

1. 新增 `DatabaseError::output_message()`，只输出诊断正文，不输出时间、ANSI 或主题样式。
2. 为完整诊断、只有 code/message、无 code、无诊断、Internal position、多行内容和控制字符分别编写 `output_diagnostic_` 测试。
3. 用精确字符串断言锁定最小示例：

```rust
assert_eq!(
    error.output_message(),
    "[42P01] ERROR: relation \"sdfsdf\" does not exist\nPosition: 15"
);
assert_eq!(error.to_string(), "relation \"sdfsdf\" does not exist");
```

4. 实现固定顺序格式化，仅存在的字段追加行；保留服务端消息本身，不匹配消息正文来推断诊断。
5. 用伪造的敏感 Detail fixture 验证：详细内容允许出现在显式 Console 格式化结果，但不得自动进入 Display、agent 序列化或 relation diagnostic 桥接。
6. 执行 `cargo test --lib output_diagnostic_`、`cargo test --test agent_serialization`、`cargo test --test agent_security`。

**验收:** 格式确定、无重复 ERROR 前缀加工、可复制，无额外信息泄露到现有受限接口。终端控制字符清洗不等同于数据脱敏，两者不可混淆。

## Task 4: 接通所有 Console 查询失败路径

**Files:** src/runtime.rs、src/runtime/transaction.rs、src/db/transaction.rs；src/db/postgres.rs、src/db/mysql.rs、src/db/sqlite.rs、src/db/mssql.rs 中实际实现 TransactionBackend::execute 的位置；src/app.rs；tests/sql_execution.rs、tests/transaction_reducer.rs、tests/sqlite_transactions.rs。

1. 列出 QueryFailed、QueryPageFailed、DerivedQueryFailed、DerivedQueryPageFailed、ManualQueryFailed、ManualQueryPageFailed 的生产者与消费者；标明哪些确实写入 Console OUTPUT，哪些仅更新查询栏。
2. 普通 Console 数据库执行错误改用 output_message；本地 SQL 构造错误单独加一次 ERROR 标签。不要全局替换 error.to_string()。
3. 审核自动分页各阶段，COUNT 和生成的 page SQL 失败附加明确的实际执行阶段上下文；Position 不重算，不直接映射原 SQL。
4. 检查手动事务后端的 SQL 执行错误转换，在 DatabaseError 变成 TransactionError 前显式调用 output_message。可增加窄用途 `TransactionError::execution(error)` 构造函数供后端复用。
5. 保持 `From<DatabaseError> for TransactionError` 的现有通用行为，保持 relation diagnostic 前缀和 payload 不变。begin/commit/rollback 的通用异常不因本任务意外附带 Detail。
6. 如某后端直接把原始驱动错误转为 TransactionError，只修改该后端 Console execute 边界，先经现有 DatabaseError 转换再调用专用格式化，不更改关系编辑写入链路。
7. 保留所有 connection、query generation、transaction generation 校验和既有事务失败状态迁移。
8. 使用现有 mock transaction backend 验证含 code/Position 的字符串能穿过手动执行与分页回复，检查旧 generation 不追加日志。自动执行用 reducer fixture 验证同样的最终格式。
9. 执行 `cargo test --test sql_execution`、`cargo test --test transaction_reducer`、`cargo test --test sqlite_transactions`、`cargo test --lib runtime::transaction`。

**验收:** 普通和手动 Console 执行诊断一致；取消不被改判为服务器错误；关系表编辑仍不携带原始 Detail。

## Task 5: 统一时间构造与范围元数据

**Files:** src/model/tab.rs、src/app.rs 的日志构造函数与测试模块。

1. 为 OutputEntry 增加 `timestamp_ranges: Vec<TextRange>`，范围是 entry.message 中的 UTF-8 字节半开区间，包含时间方括号。plain/sql 构造默认空范围，避免误识别任意文本。
2. 增加窄用途的带时间正文构造函数，为每个物理行补同一时间，生成相应范围。不要对多行 SQL 使用此函数，以免每行 SQL 前插入时间。
3. format_sql_output_entry 仅记录第一行时间范围，SQL 多行布局和 sql_range 保持现有语义。
4. 成功统计、目标切换、事务状态/模式通知等当前带时间的构造入口统一使用该函数，不再手动拼时间但漏记范围。
5. append_failed_execution_output 在函数入口只取一次时间；SQL 上下文和所有错误续行共用它。即使没有匹配 last_execution，错误自身仍带时间。
6. 审核其他直接追加 OutputKind::Error 的入口。保持既有消息正文，使用适合该入口的明确标签与时间构造，不重新解析已格式化数据库正文。
7. 新增测试覆盖中英文、多行消息、空行、尾随换行、无 last_execution、已有 SQLSTATE 方括号及 SQL 中的伪时间字符串。断言实际范围切片，而非只断言范围数量。
8. 重新验证 output_sql_ranges：多条 entry 间的换行偏移、中文上下文和多行 SQL 均准确；纯错误 entry 永远不产生 SQL 范围。
9. 执行 `cargo test --lib output_log_`、`cargo test --lib output_sql`、`cargo test --lib model::tab`。

**验收:** 所有真实时间前缀有显式范围；数据库 code 的方括号不被误认为时间；复制正文保留完整诊断。

## Task 6: UI 语义着色与选择兼容

**Files:** src/ui/mod.rs、src/ui/text_detail.rs 中 editor_line_spans 的调用；tests/ui_render.rs；src/ui/mod.rs 的单元测试模块。

1. 保持 EditorHighlightKind、SQL AnalysisKey 和 EditorRenderSnapshot 不变，日志级别不伪装成 SQL token。
2. 在 OUTPUT 渲染路径依据 OutputEntry.message 的换行和 timestamp_ranges 生成可见逻辑行的前景色覆盖。扫描 entry 时累计行号，勿对每个可见行从头扫描全量日志。
3. 将 entry 字节范围裁剪到逻辑行，先确定源字符边界，再通过现有 source_to_display_cells 投影到显示单元。明确处理 tab、中文宽字符和组合字符，不能把字节偏移当列号。
4. 扩展 editor_line_spans 接受可选前景覆盖，其他编辑器调用传空。颜色优先级固定为时间 muted > 错误正文 error > SQL 高亮 > 普通正文。
5. 使用有序且不重叠的覆盖区间，错误正文覆盖应扣除时间范围。选中背景仍由现有 mouse_selection/selection 逻辑决定，不在最终 Span 上粗暴覆盖整个 Style。
6. 继续由现有 Paragraph 水平滚动，样式计算基于未滚动内容，避免重复减 horizontal_offset。
7. 补充 TestBackend 断言：默认主题、自定义主题、无颜色模式、多行 SQL、错误里的 SELECT 文本、两个连续失败、混合成功/失败、窄窗口、纵向与水平滚动。
8. 补充选中错误文本的样式断言：fg 仍为 error，bg 为既有选择色；普通 SQL 编辑器和 text detail 无行为改变。
9. 执行 `cargo test --test ui_render output_log_`、`cargo test --test ui_render`、`cargo test --lib editor::tests`、`cargo test --test mouse`。

**验收:** 时间低调且错误醒目，未影响 SQL 高亮、鼠标命中或字符宽度投影，不新增 SQL 分析缓存维度。

## Task 7: 集成验证与交付检查

**Files:** tests/postgres_adapter.rs、tests/postgres_relation_mutations.rs、tests/sql_execution.rs、tests/ui_render.rs；仅必要时更新已有相关测试 fixture。

1. 无数据库环境下执行以下检查，按实际结果记录，不把命令执行成功等同于门控集成测试实际运行：

```sh
cargo fmt --check
cargo check --all-targets
cargo test --lib
cargo test --tests
cargo clippy --all-targets -- -D warnings
git diff --check
```

2. 已配置且确认是隔离测试 PostgreSQL 时执行：`cargo test --test postgres_adapter output_diagnostic_ -- --nocapture`。不打印 LAZYDB_TEST_POSTGRES_URL 或凭据。
3. 在隔离测试事务或临时表中验证唯一约束错误的 Detail；无测试库则仅运行合成诊断单测并明确列为未验证，禁止在用户业务表制造冲突。
4. 人工 TUI 验证同一 SQL 的自动提交与手动事务失败，检查 code、Position、红色续行和时间颜色；再验证一次成功 SQL 的高亮与统计。
5. 调整终端宽高、横向滚动、选择并复制跨行错误，确认文本完整且没有 ANSI；验证后台 Console 追加和现有尾部定位不回归。
6. 检查 agent 序列化与 relation mutation 脱敏测试；原始 Detail 不得进入这些既有受限输出。
7. 检查最终 diff：只包含本任务业务改动、必要构造字段调整和测试，无全局 Display 改写、SQL tokenizer 改写、时间时区改写或无关格式化。

**最终验收:** 用户示例可完整显示；真实服务器 Position 未丢失；所有主题行为一致；现有编辑器交互和安全边界不变。交付说明应分开列出已运行测试、门控跳过测试和人工验证结果。

## 执行依赖与建议顺序

- 先完成 Task 1，再按 Task 2、3、4、5、6、7 顺序推进，每个任务先新增失败用例，再最小实现，再跑对应测试。
- 诊断链路与样式链路在概念上独立，但都修改 src/app.rs，默认串行更稳妥，不建议多个 agent 同时修改这些文件。
- 公共 DatabaseError 新增字段需要调整较多结构体字面量，这是模型变更的必要影响，不借机统一所有错误构造或改写其他驱动。
- 不引入新存储格式，因此不新增兼容迁移逻辑；若实施时发现 shipped 外部消费者依赖公共结构体字面量，先说明影响并确认接口处理方式。
- 若后续确认要改为本地时间，另建小任务复用 chrono，增加固定时区与跨日测试，不依赖测试机器 TZ，也不并入本次验收。
