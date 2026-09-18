# MariaDB JSON and Geometry Preview Implementation Plan

> **执行者：Luna。** 按下列可验收单元连续实施，审查、纠偏、提交合并也由 Luna 完成。当前 Astra 仅制定计划，不启动子 Agent，不修改业务代码。

**Goal:** MariaDB 表格预览正确展示字符 JSON 与七类空间值的 WKT，同时保持真实二进制数据和空间原值的绑定语义。

**Architecture:** 在 MySQL/MariaDB 共享解码层使用 SQLx 文本类型兼容判断，替代依赖类型名称的错误文本/二进制分类。GEOMETRY 单独经过有界 SRID+WKB 解析，新增携带原始 bytes 与 WKT 的 CellValue；预览和复制使用 WKT，MySQL 参数绑定保留 bytes。

**Tech Stack:** Rust 1.94、SQLx 0.9.0、MariaDB 11.4、serde、现有 ratatui 测试设施。无新增 GIS 依赖。

---

## 基线、交接与范围

- 工作空间：`/Users/yelog/workspace/tui/lazydb`；目标分支 main。
- 用户起点：`0c58cbb49cae3512cfb10b1039cfd2808efedd34`。
- 计划时 HEAD：`050b1412c052a44469ec9523cde86704985bf9f8`；与起点仅差任务文档，相关业务代码相同。
- 原因、方案比较与协议证据：`.git/opencode-tasks/ses_f520d5731ffev409g2RwSXW6I6/analysis.md`。
- 验证记录：同目录 `validation.md`。计划阶段没有修改业务代码或重跑数据库测试。
- 本轮已读取插件生成的 checkpoint：stage=plan、round=1、codeChanged=false。不得创建/改写 state.json 或 checkpoint.json。完整计划交付至同任务目录 `plan.md`；回执仅使用当前阶段消息指定的路径和 token，历史回执保留，不从计划或旧总结提取回执文件名。
- 工作流与任务分支名称交由 Luna 按流程确定。创建任务工作树时从指定起点出发，不把当前未跟踪的其他任务文档带入提交；本计划可明确带入任务工作树。
- 普通实现取舍不阻塞；先完成单元一再继续单元二，不等待反复 resume。

需求完整值：

| 类型/列 | 期望 |
| --- | --- |
| nullable_json | `{"present":true}` |
| shipping_location | `POINT(116.397 39.908)` |
| geometry_value | `GEOMETRYCOLLECTION(POINT(0 0),LINESTRING(0 0,1 1))` |
| linestring_value | `LINESTRING(0 0,1 1,2 1)` |
| polygon_value | `POLYGON((0 0,0 1,1 1,1 0,0 0))` |
| multilinestring_value | `MULTILINESTRING((0 0,1 1),(2 2,3 3))` |
| multipolygon_value | `MULTIPOLYGON(((0 0,0 1,1 1,1 0,0 0)))` |
| geometrycollection_value | `GEOMETRYCOLLECTION(POINT(1 1),LINESTRING(2 2,3 3))` |

MULTIPOINT 同步支持；NULL 仍为 Null，真实 BLOB 不做内容嗅探。现有宽度截断继续有效，复制必须获得完整值。本次不增加 WKT 输入/空间编辑功能或 JSON DDL 类型推断。

## 验证层级与门禁来源

1. **用户需求验收：** 上表八个完整值在实际表格 preview 路径正确展示。本轮是计划阶段，只交付文件、步骤、复核点、验证命令及验收标准；不执行实现阶段。
2. **项目既有强制门禁：** `.github/workflows/ci.yml:81-83` 的 Rust 1.94 格式、全目标全特性 clippy（warnings 为错误）、全目标全特性测试。数据库 CI 使用其配置环境。命令见收尾章节，不增加人工审批或人工 TUI 必过要求。
3. **本方案必要自动化回归：** JSON/真 binary 的协议区分、七类 WKB 和错误输入、原始 bytes/SRID 保留、编辑与绑定、复制、serde 和预算。这些测试验证本次实现带来的实际行为与风险，不宣称它们是用户逐条指定或项目原有门禁。真实 MariaDB 测试设置强制 URL 检查，明确区分执行与跳过。
4. **补充建议验证：** 人工 TUI/PTY 查看、更多数据库版本交叉检查、额外长时间压力测试。不把这些建议自动升格为完成门禁；环境受限最多一次有针对性修复重试，再由 Luna 收尾审查决定补证或记录限制，不无限重试，不请求用户反复 resume。

以下各单元测试命令均为后续实施计划，不是本阶段已通过结果。

## 单元一：JSON 字符值解码闭环

**修改：** `src/db/mysql.rs` 的 `decode_cell`，`tests/mariadb_values.rs`。

**步骤 1：建立能失败的真实协议回归测试。**

在 `tests/mariadb_values.rs` 使用 `support::mariadb_test_url()` 和现有连接模式，创建 UUID 后缀唯一测试表。不要使用共享 fixture 的固定订单行。测试表包含：

```sql
CREATE TABLE `unique_test_table` (
  id INT PRIMARY KEY,
  nullable_json LONGTEXT CHARACTER SET utf8mb4 COLLATE utf8mb4_bin
    DEFAULT NULL CHECK (json_valid(nullable_json)),
  binary_json LONGBLOB,
  binary_text VARCHAR(80) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin,
  ordinary_text TEXT
);
```

插入 `{"present":true}`、SQL NULL、`{}`、含中文和空白的合法 JSON，以及内容相同的 binary_json。使用绑定参数或正确 SQL quoting。分别通过 `DatabaseConnection::execute` 和公开 relation preview 接口读取。CatalogId 使用真实 profile id、数据库名和测试表名，沿现有 `tests/relation_runtime.rs` 的请求构造方式完成，不能仅用 sqlx 查询替代应用预览接口。

断言：字符 JSON 精确为 `CellValue::Text`，不重排、不格式化；NULL 为 Null；binary_json 精确为 Bytes；普通 `_bin` 字符列为 Text。在清理阶段 drop 唯一测试表，保留原始失败信息；不要先 DROP 通用固定名称表。

**步骤 2：定向运行确认失败。**

```sh
env LAZYDB_TEST_MARIADB_URL=mariadb://lazydb:lazydb_password@127.0.0.1:3307/lazydb_test LAZYDB_REQUIRE_DATABASE_TESTS=1 cargo test --test mariadb_values -- --nocapture
```

预期新测试失败于字符 JSON 的 Text/Bytes 类型或值断言；旧测试可能仍通过。记录实际失败，不能假设已失败。

**步骤 3：实现公开 API 判定。**

保留 `decode_cell` 的 NULL 优先。进入字符串/二进制名称分支前保存 raw type info，并使用：

```rust
if <String as sqlx::Type<sqlx::MySql>>::compatible(&raw.type_info()) {
    return row
        .try_get::<String, _>(index)
        .map(CellValue::Text)
        .unwrap_or_else(|error| unsupported(&type_name, &error.to_string()));
}
```

具体借用按 `raw.type_info()` 返回 Cow 的生命周期落实，不依赖 Debug 输出或私有 collation 字段。此 API 同时检查字符串协议家族和非 binary collation，不会把数字或 GEOMETRY 当文本。保留 MySQL 原生 JSON/数值/日期现有分支与 fallback 行为。

**步骤 4：收紧旧测试并验收。**

将 matrix 的 binary 断言收紧到 Bytes，JSON_OBJECT 的断言收紧到 Text；新增测试同时覆盖表字段和 prepared preview，防止只修表达式。定向重跑同一命令，应全部通过且无跳过。作为完整单元提交，建议消息 `fix(mysql): decode character values using protocol compatibility`。

**单元复核：** NULL 判定仍最先执行；字符串兼容判断没有吞掉数字/日期；可打印 binary 不会被解码为 Text；测试实际进入应用 preview。以上条件和定向测试全部满足后，单元一才完成。

## 单元二：空间展示与原始数据保留闭环

此单元包含解析、值模型和绑定接入，不能仅交付孤立解析器后声称空间预览已完成。

**创建：** `src/db/mysql/geometry.rs`。

**修改：** `src/db/mysql.rs`、`src/db/value.rs`、`src/db/postgres.rs`、`src/db/sqlite.rs`、`src/db/mssql.rs`。

**测试：** 新模块单元测试、`src/db/value.rs` 测试、`tests/mariadb_values.rs`、`tests/agent_serialization.rs`、`src/db/query.rs` 预算测试。

### 步骤 1：固定字节样本与解析契约

模块对父模块暴露一个纯函数，例如：

```rust
pub(super) fn mysql_geometry_wkt(bytes: &[u8]) -> Option<String>
```

Option 足以表达展示成功/退回 bytes，不把解析失败升级为整份查询错误。若实现需要内部错误枚举用于测试定位，可保持模块私有。

首先添加固定点样本测试：

```rust
#[test]
fn shipping_point_matches_requested_wkt() {
    let bytes = [
        0, 0, 0, 0, 1, 1, 0, 0, 0, 0xC5, 0x20, 0xB0, 0x72,
        0x68, 0x19, 0x5D, 0x40, 0x4E, 0x62, 0x10, 0x58,
        0x39, 0xF4, 0x43, 0x40,
    ];
    assert_eq!(mysql_geometry_wkt(&bytes).as_deref(), Some("POINT(116.397 39.908)"));
}
```

再添加字节构造辅助函数，仅用于测试：明确 endian、u32 和 f64 编码，不借用生产解析代码生成期望字符串。覆盖所有七类 WKB、集合递归、polygon 多环和 MULTI 多成员。期望字符串采用需求表及固定字面量。

### 步骤 2：有界解析并输出 WKT

- 输入为 4 字节 SRID + WKB；不足 9 字节失败。不尝试把裸 WKB 当内部格式猜测。
- 维护只读切片、offset、输出 String、当前深度。每个 geometry 读独立字节序（仅 0/1）和 u32 类型；仅接受 1..7，拒绝未知高位维度/EWKB 标记。
- 1 POINT：两个 f64；2 LINESTRING：计数 + 点；3 POLYGON：环计数 + 每环点计数；4/5/6 MULTI：计数 + 分别只允许 1/2/3 的子 geometry；7 collection：任意七类子 geometry。
- 对 MULTI 子项输出坐标体而非重复 POINT/LINESTRING/POLYGON 类型词。collection 子项包含类型词。MULTIPOINT 固定采用 `MULTIPOINT((x y),(x y))` 的合法 WKT。
- 坐标必须有限；使用 Rust 最短往返数值格式，不截精度。逗号无额外空格，x/y 之间一个空格。负零可规范为 `0`，但原始 bytes 不变。
- 零成员集合输出对应 `TYPE EMPTY`；先用数据库验证支持的 EMPTY 行为。POINT 的非有限坐标不当作空点猜测。零环 polygon/零点 linestring 可以按 WKB 空几何规范显示 EMPTY；不做地理拓扑合法性修复。
- 定义模块常量：最大递归 64 层、最大解析输入 4 MiB、最大 WKT 输出 8 MiB。检查每次输出追加前的长度；坐标字符串只能短暂局部生成，不先构造无界大字符串再截断。
- `checked_add`/`checked_mul` 与剩余字节验证；不得按计数直接 reserve。计数先验证下界需要的字节数；循环每一步推进 offset，深度与输出共同约束。
- 根节点完成后要求输入耗尽。截断、超限、非法类型/子类型、尾随字节统一返回 None。

运行 `cargo test --lib db::mysql::geometry`，确认所有固定样本与恶意输入测试通过。恶意输入包括每个截断位置、u32::MAX 计数、错误字节序、错误 multi 子类型、65 层集合、NaN/Infinity、附加尾随字节与超限输出。

### 步骤 3：新增 lossless 值并接入解码

在 `CellValue` 添加：

```rust
MySqlGeometry { bytes: Vec<u8>, wkt: String },
```

保持既有 serde 外部标记形式，既有变体序列化不变。`clipboard_text` 返回 wkt；`preview(max_len)` 使用 `preview_text(wkt,max_len)`，不能用 bytes 的长度计算文本截断。

`decode_cell` 中 GEOMETRY 在 fallback 前专门读取 `Vec<u8>`：成功解析构造新变体；失败保留 Bytes；SQL NULL 仍在最前方返回。实际类型以协议 `GEOMETRY` 为准，不根据任意 BLOB 内容识别空间值。

MySQL `bind_cell` 为新变体绑定 `bytes.clone()`。PostgreSQL、SQLite、SQL Server 的 exhaustive bind match 对该变体返回明确不支持错误，不把 MySQL 内部 SRID 格式跨库绑定。Oracle 及其他通配分支检查是否透传或序列化；只有实际需要时修改，不为编译通过加入语义含混的 wildcard。

### 步骤 4：验证公共投影与实际数据库

- value 单元测试覆盖 WKT 预览、完整复制、原始 bytes 与非零 SRID 的 serde round-trip。
- `src/agent/types.rs` 原样持有 `Vec<Vec<CellValue>>`，无需另建 agent 投影。`tests/agent_serialization.rs` 明确新变体为 `{"MySqlGeometry":{"bytes":[...],"wkt":"..."}}` 且旧 Text/Bytes 契约不变。
- `src/db/query.rs:119-124` 已按 serde 结果计费，无需生产预算改动。新增空间值预算测试，用实际 `serde_json::to_vec` 大小设置边界，确认 bytes + WKT 都计入。
- MariaDB 唯一测试表包含需求八列和 MULTIPOINT、NULL；`execute` 和 preview 返回同样 WKT。使用 `ST_AsText` 对照服务器输出；MULTIPOINT 如果服务器采用不同合法括号风格，采用固定 WKT 预期并通过服务器再解析验证语义，不宽松放过其他七项格式。
- 在同一测试数据加入非零 SRID，并比较显示值原始 bytes 与 SQL HEX。真 BLOB 放入完全相同空间 bytes，断言仍是 Bytes。

定向命令：

```sh
cargo test --lib db::value
cargo test --lib db::query
cargo test --test agent_serialization
env LAZYDB_TEST_MARIADB_URL=mariadb://lazydb:lazydb_password@127.0.0.1:3307/lazydb_test LAZYDB_REQUIRE_DATABASE_TESTS=1 cargo test --test mariadb_values -- --nocapture
```

该单元与下面编辑适配一起完成后提交，避免引入新变体但尚可被误写回的中间发布状态。

**单元复核：** 七类解析分支、内部 SRID wrapper、每层 endian、输出预算和原始 bytes 绑定逐项检查；解析失败必须完整回退而非返回半截 WKT。未完成单元三的编辑适配前，空间功能仍视为在实施中。

## 单元三：编辑与预览整链路验收

**修改/检查：** `src/app.rs` 的 `relation_edit_cell`、`src/model/cell_editor.rs`、`src/model/relation_edit.rs`、`src/db/mutation.rs`、`src/db/mysql.rs`。

**测试：** `tests/relation_tabs.rs`、`tests/relation_runtime.rs`、`tests/mariadb_relation_mutations.rs`、`src/clipboard.rs`、`src/ui/data_grid.rs` 已有测试模块。

### 步骤 1：封闭显示文本进入写路径的漏洞

已确认 `relation_edit_cell` 调用 `CellEditorBuffer::from_value`，无专用类型时用 `clipboard_text()` 初始化文本。因此 WKT 会被当作普通 Text，甚至原样确认也可能制造 dirty。

采用最小能力边界：MySQL/MariaDB 的空间列不进入本次尚未支持的文本编辑器，其他列维持编辑。判断必须同时覆盖：

1. 当前值是 `MySqlGeometry`；
2. 数据库为 MySQL/MariaDB 且 `ResultSet.columns[column].type_name` 是 GEOMETRY（兼顾 NULL、无效 bytes fallback、insert draft）。

在进入编辑 mode 之前返回，并沿项目现有通知方式说明该空间值可预览/复制、暂不支持编辑。不要用 `CellValue::Unsupported`，它会让整表只读。不要只按值变体拦截，否则 NULL 空间列会漏过。

增加 reducer 测试：进入空间列编辑不会打开文本框或改变 dirty；对同表普通列仍能编辑保存；取消、复制、切换行不改变 geometry 原值。空间列快捷写值入口如果绕过 `relation_edit_cell`，沿 action dispatch 一并检查，使相同能力判断复用，不建立两套互相矛盾的规则。

### 步骤 2：绑定回归必须走真正适配器入口

现有 `tests/mariadb_relation_mutations.rs` 仅执行原始 UPDATE SQL，并未验证 `bind_cell`。新增测试不能照搬该模式冒充绑定验证。

在 mysql.rs 私有测试模块使用真实连接调用 `bind_cell`，把从解码得到的 MySqlGeometry 绑定到 `SELECT HEX(?)` 并与原 bytes 比较；包括非零 SRID。这直接验证 bytes 分支且不需要发明 WKT 编辑功能。数据库 URL 使用既有环境变量规则；强制验证时缺 URL 必须失败。

另通过公开 relation mutation API 编辑同表普通字段，再读取空间列 HEX 与 WKT，确认完全不变。采用现有 PostgreSQL relation mutation 测试的事务请求模式并遵循 MariaDB 现有 API；不要通过裸 SQL 替代待验证的调用链。

### 步骤 3：完整预览行为

在 `tests/mariadb_values.rs` 扩展：

- 分页至少两页，包含 NULL 和非空空间值；过滤/排序结果正确。
- 视图投影保留正常列顺序/列名；空结果仍保留 GEOMETRY 元数据。
- WKT 长值可截断但复制完整；JSON 原始空白保持。
- TestBackend 渲染可见单元格为 WKT 和 JSON，而真实 binary 为 hex。只增加一组有效整链路用例，不复制每个解析器用例到 UI。

定向运行：

```sh
cargo test --test relation_tabs --test relation_runtime
env LAZYDB_TEST_MARIADB_URL=mariadb://lazydb:lazydb_password@127.0.0.1:3307/lazydb_test LAZYDB_REQUIRE_DATABASE_TESTS=1 cargo test --test mariadb_values --test mariadb_relation_mutations -- --nocapture
```

新增私有绑定测试按最终测试名称单独执行并记录。建议空间功能提交消息 `fix(mysql): render geometry values as lossless WKT previews`。

**单元复核：** 非空、NULL、fallback bytes、insert draft 四种空间单元格都不会被隐式转换成 WKT 文本写回；同表普通列仍可编辑。预览、复制和 agent 输出使用同一值模型，完整验收矩阵逐项有自动化证据。

## 收尾：项目验证、审查与交接

1. 对所有需求值建立通过记录；检查未引入 SQL 重写、额外元数据查询、JSON 内容嗅探、私有 SQLx API、无界解析、整表只读或 WKT 文本绑定。
2. 确认 `git diff --check`，修改仅包含本任务文件。通过 @git-commit 工作流按实际 diff 暂存，不使用 `git add .` 吸入其他任务。
3. 功能齐备后按 `.github/workflows/ci.yml:81-83` 跑一次项目 Rust 检查：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

全量通用测试与强制数据库测试分别记录：不要给全量测试强行设置所有数据库必需标志却缺其他服务。MariaDB 实库定向结果是必须证据；共享 MySQL 驱动的 MySQL 8.4 实库回归使用项目 CI 的 `LAZYDB_TEST_MYSQL_URL` 环境，若本地无该服务，记录该环境覆盖由 CI 承担，不伪称本地完成。

4. enum 的跨库 exhaustive match 由全特性编译/lint 验证。仅在新修改或失败指向相关代码时重跑对应检查，不循环执行全量三件套。
5. 可补一次真实 TUI 人工查看，但不是替代自动化或额外强制验收。环境受限最多一次有针对性的修复重试，之后记录限制并由 Luna 收尾审查决定；不无限 progress。
6. 将每个实际命令、退出结果、相关文件、commit/dirty 状态、DB 版本和跳过情况追加到任务 `validation.md`。2026-09-17 分析阶段 2/2 的旧结果不能替代新代码验证。
7. Luna 审查并按当前工作流权限完成提交/合并；本计划不要求再切换 Astra 审查。只有当阶段消息提供新的回执路径/token 时才写该阶段回执，保留历史 analyze 回执。

## 完成标准

- 用户八个展示目标在应用 preview 路径精确通过，普通 SQL 查询一致。
- 所有七类空间值可显示，原始 bytes/SRID 保留；异常输入可控退回 hex。
- 真 binary、NULL、原始 JSON 文本与已有分页契约不变。
- 复制完整、显示截断正确、新值序列化和预算正确。
- 空间文本不会被误写回；同表普通列仍可编辑；MySQL 绑定实际使用原 bytes。
- 相关定向测试及项目全量验证有当前版本证据；环境限制明确记录。
