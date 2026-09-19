# MySQL/MariaDB/Oracle 注释编辑修复 Implementation Plan

> **执行者：Luna。** 用户已选择自动工作流；按本文端到端单元连续实施、审查、纠偏与交付，不再询问执行方式，不启动子 Agent。本文由 Astra 在 plan 阶段编写。

**Goal:** 在已有表编辑器中新增、修改、清空表注释或列注释后，Review SQL 生成正确语句，执行后重新打开能看到真实注释；未改变的列属性得到保留。

**Architecture:** 沿用 `CatalogDraft → CatalogMutationPlan → execute → reload`。修复共享 MySqlAdapter 的注释元数据及差异规划，通过服务端原生列定义快照避免 CHANGE COLUMN 丢失属性；Oracle 加载注释后使用独立 COMMENT ON，不重建列。公共模型仅增加数据库专用快照，其他适配器作机械性构造点迁移。

**Tech Stack:** Rust 2024、Rust 1.94.0、sqlx MySQL、oracle feature、现有 planner/reducer/integration tests。

---

## 0. 版本、输入与阶段边界

- 输入分析：本任务目录 `analysis.md`；任务目标 main；指定起点 `45e330cedd3b8da493d1c198329ec38d5e1f38b5`。
- 本 plan 阶段实际观察到主工作区 HEAD 已变为 `c634324b6cc3a94dbb4735349fc20c4516f23282`。起点之后为 `e412716 fix(explorer): hide principals until connection is open` 及其 merge。变化涉及 explorer、其测试和一份计划文档；本文相关适配器、编辑器、reducer、测试与 CI 文件相对起点没有差异。
- 两项 unstaged 用户改动仍为 `src/persistence/workspace.rs`、`tests/workspace_persistence.rs`，29 additions / 2 deletions；index 无暂存内容。它们是 principal 序列化修复，与本任务无依赖，不列入 change-scope，不复制到新 worktree。
- 后续任务分支/worktree 必须以指定起点为准；合入 main 时由 Luna 按实际 main 状态复核。不要把当前主目录 HEAD 当作起点，不能以覆盖、stash 或清空用户工作区解决分支差异。
- 本阶段仅读取代码并写任务目录文件；不创建分支，不提交，不修改 state.json。任务目录目前没有 checkpoint.json。后续若插件生成 checkpoint，读取实际 diff 与 checkpoint 恢复，不沿用旧的等待指令。
- 任务名和分支名由后续自动工作流/Luna 确定。本文不创建 docs/plans 副本。

## 1. 需求、门禁与建议检查分级

### A. 用户需求及修复验收

1. MySQL、MariaDB、Oracle 上，已有表的表注释和列注释新增/修改都能 Review SQL，不能错误返回 NoChanges。
2. 清空注释、原值未改、恢复原值也有正确语义；这是本修复的边界回归，不是额外产品功能。
3. 修改应用后基线重新读取，注释正确回填；MySQL 列注释修改不得丢失 default、AUTO_INCREMENT、COLLATE、生成列及其他未编辑属性。
4. 同时表 rename 时先 rename，COMMENT 指向新表；MySQL 同时列 rename 或调整位置时保留正确列定义。Oracle 对不支持的列结构变化明确拒绝，不能“成功”执行注释而静默丢弃结构要求。
5. 保留真正无变化时的 NoChanges，保持数据库 target、selection、refresh、autocommit 契约。

### B. 项目已有强制门禁

来自 `.github/workflows/ci.yml`：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

CI 数据库 job 使用 MySQL 8.4 / MariaDB 11.4，以及 **`LAZYDB_REQUIRE_DATABASE_TESTS=1`**。不是分析阶段环境探针中出现的 `LAZYDB_REQUIRE_INTEGRATION`。新增集成测试必须服从真实 require 约定，不能在 CI 强制模式下静默 skip。

本文不修改 CI，也不将仓库无关的发布/Windows 安装检查加入此次本地前置门禁。提交后的现有 CI 自行按其配置执行。

### C. 本计划必要的自动化回归证据

- 离线 planner 测试、MySQL 原生定义提取/替换测试、必要的 reducer 测试必须运行。
- 新增真实数据库往返测试；MySQL/MariaDB 应由已配置的 CI 数据库 job 执行。当地缺少数据库时明确记录“未执行”，不能以测试提前返回的 exit 0 声称往返通过。
- Oracle 现有测试以 `driver-oracle` feature、URL/USER/PASSWORD 和本机客户端为条件；当前 CI 没有 Oracle service。离线 planner 必须通过，真实 Oracle 往返有条件执行；环境缺失作为证据限制交 Luna 收尾审查，不能无期限阻塞自动任务。

### D. 补充建议，不是新必需门禁

- 人工 TUI/PTY 按键复现、截图及逐个数据库手工操作。
- 超出既有支持版本的数据库版本矩阵。
- 同一环境受限检查最多一次有针对性修复重试，然后记录限制，由 Luna 决定补证；不要求用户不断 resume。

## 2. 具体实现决策

### 2.1 注释比较

在 mysql/oracle 本地小 helper 中使用以下语义，不引入全局 change-set 重构：

```rust
fn comment_changed(before: &OptionalMetadata<String>, after: &str) -> bool {
    match before {
        OptionalMetadata::Supported(value) => value.as_deref().unwrap_or("") != after,
        OptionalMetadata::Unsupported => !after.is_empty(),
    }
}
```

已核对 `src/db/catalog.rs`，OptionalMetadata 只有上述两个 variants。非空值不 trim，空字符串用于清空；单空格是合法内容。Unsupported + 空输入保持未知，不擅自生成清空；真实加载路径应返回 Supported。

### 2.2 MySQL 列定义保真

只补 COMMENT 到目前的 `mysql_column_definition` 不够，它会重建并丢失服务端原生属性。采用分析中的原生快照方案，并将字段形状明确为：

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MySqlColumnDefinitionSnapshot {
    pub sql: String,
    pub no_backslash_escapes: bool,
}
```

在公共 `ColumnDefinition` 增加 `pub mysql_definition: Option<MySqlColumnDefinitionSnapshot>`，其他适配器及已有非 MySQL fixture 赋 `None`。名称明确标出方言，避免把 MySQL 定义当通用可执行片段。快照保留服务端的一整条列定义，包含已引用的列名，不包含逗号和其他列。

新增 `src/db/mysql/catalog_definition.rs` 私有模块，集中实现以下纯函数职责：

1. 从 SHOW CREATE TABLE 的顶层括号列表提取列定义，返回列名到原始定义字符串映射；只按顶层逗号切分，区分字符串、quoted identifiers、括号、普通及版本注释；根据 SQL mode 处理反斜杠。
2. 按已加载的实际 column_name 关联定义，不把 PRIMARY/UNIQUE/CONSTRAINT/KEY 误认成列；支持列名中反引号的双写。
3. 修改定义最前面的列标识符以支持 rename；定位且只替换顶层 COMMENT 的字符串值，或插入新的 COMMENT；不触碰 default/generated expression 内的 `COMMENT` 文本。
4. 遇到不闭合引号、重复或无法识别的 COMMENT 结构、无法保真定位的版本注释时返回错误，禁止猜测后输出。普通可识别的版本注释及其他选项按原字节保留。
5. 对快照字符串可验证：以指定原列标识符开始、只有一个完整列定义、不带顶层分隔符；不要把 baseline 任意 String 当多语句 SQL 插入。

推荐接口职责（名称可随项目风格调整，但行为不可省略）：`extract_column_definitions`、`rewrite_column_comment`、`quote_comment_literal`。新增模块内 colocated `#[cfg(test)]` 测试即可，不增加依赖。

**SQL mode：** 同一获取定义的连接读取 `@@SESSION.sql_mode`，记录 NO_BACKSLASH_ESCAPES；注释 literal 在该模式下单引号加倍但不双写反斜杠，否则复用现有 mysql::quote_literal。表注释使用同一表列快照中一致的 mode（已有表至少有一列）；简单合成 fixture 无快照时按现有适配器模式假设。不要为修复改变连接池 SQL mode。若实施中发现执行连接模式不一致，要在同一业务单元解决或明确拒绝这种不一致，不能声称两种模式都验证通过却使用不同模式的执行连接。

**结构组合边界：**
- 只有 comment、name、position 变化时使用原生定义替换，保留其他属性。
- 现有简单列类型/nullability/default 修改可走已有重建路径，但必须输出已知 identity、collation 和 comment，并只在确认没有遗漏原生选项时使用。未知原生选项不能静默被删除。
- 对复杂列同时要求不能保真重建的结构变更，明确 InvalidDraft，并指出具体列与不支持的组合；只改其注释必须支持。不要全面实现通用 MySQL ALTER TABLE AST。
- draft/baseline 对应列找不到时返回 StaleState/InvalidDraft，不把它当无基线新列吞掉。
- 删除列跳过 comment；ADD COLUMN 输出新列 comment；旧列重建也必须保留未改 comment。

### 2.3 元数据加载与 SQL 规划

MySQL/MariaDB：
- `information_schema.tables` 以现有 BINARY schema/name 匹配读取 TABLE_COMMENT，参数绑定与现有 columns 查询一致。
- 加载 SHOW CREATE TABLE 和 SQL mode，生成每列快照；TABLE_COMMENT 与列快照一同进入 baseline fingerprint 输入。
- table comment diff → `ALTER TABLE <qualified> COMMENT = <literal>`；column comment diff → 带完整定义的 CHANGE COLUMN（保留现有位置计算），同一列只有一个最终定义语句。
- needs_change 至少包括已有条件、comment 与 default；已知字段比较和 renderer 要一致。避免重复发 comment-only 和结构变更两条互相覆盖的列语句。

Oracle：
- 独立绑定查询 `SELECT comments FROM all_tab_comments WHERE owner = :1 AND table_name = :2`；列注释用 `SELECT column_name, comments FROM all_col_comments WHERE owner = :1 AND table_name = :2` 映射，避免改写含 LONG data_default 的原查询。
- 存在表、注释 NULL → Supported(None)；查询失败按真实错误返回，不能降级成无注释。
- 保留 table baseline，比对已有列的 comment；COMMENT 语句以新表名、实际已有列名限定。
- SQL：`COMMENT ON TABLE "APP"."T" IS 'text'`、`COMMENT ON COLUMN "APP"."T"."C" IS 'text'`，清空 `IS ''`，单引号双写。每条单独放 statements，执行驱动沿用现有去终止符逻辑。
- 純注释 `native_identity_changed=false`；表 rename 为 true。只修表分支必要的标识语义，不扩展 view/sequence 功能。

## 3. 文件清单

### 实际功能与测试

| 文件 | 预计变更 |
|---|---|
| `src/db/mysql.rs` | 加载表注释/SHOW CREATE/mode/快照；差异判断、SQL、完整列生成 |
| `src/db/mysql/catalog_definition.rs` | 新增私有原生列定义提取/替换 helper 及 unit tests |
| `src/db/oracle.rs` | 加载表/列注释，COMMENT planner，结构变化校验、identity 标识 |
| `src/db/catalog_mutation.rs` | 新增 MySqlColumnDefinitionSnapshot 及 ColumnDefinition 字段 |
| `tests/object_mutation_contract.rs` | MySQL/MariaDB/Oracle 注释 planner 回归与结构组合边界 |
| `tests/mysql_adapter.rs` | MySQL 真实注释 load/plan/apply/reload、属性保留 |
| `tests/mariadb_catalog_mutation.rs` | MariaDB 同类往返与原生选项保留 |
| `tests/oracle_adapter.rs` | Oracle 注释往返与明确 skip 日志 |
| `tests/catalog_editor_reducer.rs` | 编辑→Preview→真实 planner→ready 链路测试 |

### 公共字段迁移的全部已发现构造点

| 文件 | 预计变更 |
|---|---|
| `src/db/postgres.rs` | ColumnDefinition 构造补 mysql_definition: None |
| `src/db/sqlite.rs` | 同上 |
| `src/db/mssql.rs` | 同上 |
| `tests/catalog_mutation.rs` | 现有 column_definition fixture 补 None |
| `tests/catalog_editor_state.rs` | 所有 ColumnDefinition literals 补 None |
| `tests/ui_render.rs` | 两处 ColumnDefinition literals 补 None |

Oracle、MySQL、object_mutation_contract 自身构造点包含在上表功能文件中。当前全仓匹配共 20 行（含类型定义及 helper 返回签名），真实构造点均在这些文件内。实施时编译器若发现额外位置，先更新任务 scope 再修改；不得把整个 src/tests 作为便利范围。

`src/app.rs`、`src/runtime.rs`、`src/input/keymap.rs`、`src/model/catalog_editor.rs`、CI、Cargo 文件目前是参考文件，不计划修改。不引入新依赖，不改 release/changelog。

## 4. 可验收单元一：MySQL/MariaDB 注释完整闭环

这是首个未完成业务单元。下列每个编号是可独立完成并核对的步骤；不要停在只修改类型定义或只让一个 INT 示例成功。

### 4.1 先建立真正会失败的 planner 回归

文件：`tests/object_mutation_contract.rs`。

1. 提取小型 MySQL edit fixture：同一 profile、database/schema/name，至少一个已有列、Supported comment、合法 edit anchor；draft 必须来自 `TableDraft::from_definition_for_database`。
2. 新增测试统一前缀 `mysql_comment_`，循环 DatabaseKind::MySql/MariaDb；只改 table.comment 时期望一条 ALTER TABLE COMMENT；只改 columns[0].comment 时期望 CHANGE COLUMN 包含 COMMENT。
3. 执行：

```sh
cargo +1.94.0 test --locked --test object_mutation_contract mysql_comment_
```

预期修复前 fail，错误落在 `NoChanges`；记录实际退出结果，不能把构造 fixture 失败当故障复现。

### 4.2 增加快照模型并完成所有构造点迁移

文件：`src/db/catalog_mutation.rs` 与第 3 节列出的迁移文件。

1. 定义快照 struct 并加入 ColumnDefinition，derive Clone/Debug/Eq/PartialEq。
2. 其他适配器和旧 fixture 赋 None；MySQL 新 fixture 提供原生定义。
3. 只做字段迁移，不调整其他数据库业务行为。此时不跑全量门禁；下一步定向测试会编译实际依赖。

### 4.3 编写原生列定义 helper 及测试

文件：新增 `src/db/mysql/catalog_definition.rs`，在 `src/db/mysql.rs` 注册私有模块。

1. 为模块测试加前缀 `mysql_comment_`，覆盖顶层分割、quoted identifiers、括号嵌套、引号双写/反斜杠模式。
2. Fixture 包括：DECIMAL(12,2)、ENUM('a,b','COMMENT')、字符串 default、generated concat 表达式、自增、COLLATE、ON UPDATE、STORED/VIRTUAL、版本注释包裹的属性。
3. 替换现有 COMMENT、无 COMMENT 时添加、清空及列 rename；用保留片段精确断言证明其他定义未变，而非只断言包含 COMMENT。
4. malformed/ambiguous 输入要断言明确错误，不能产生可执行但不完整的定义。
5. 按 2.2 实现词法状态扫描及替换；不使用简单逗号 split 或全局 regex。
6. 执行：

```sh
cargo +1.94.0 test --locked --lib mysql_comment_
```

验收：复杂定义只发生指定 comment/name 替换，default/表达式中的 COMMENT 原样保留。

### 4.4 连接加载与 planner

文件：`src/db/mysql.rs`、`tests/object_mutation_contract.rs`。

1. 同连接加载 table comment、SHOW CREATE 和 mode，映射快照到 columns，缺失实际列定义时报可定位错误。
2. 将表注释纳入 baseline，确保 table draft 能读到 Supported 值。
3. 根据 2.1/2.3 补 table diff 与 column diff；default-only 变化纳入 needs_change；重写列保留 COMMENT。
4. 在生成任何重建定义之前判断是否真的需要修改；避免表注释-only 被某个未改复杂列阻挡。
5. rename、comment、position 的组合使用原生快照；结构重建仅在可保真时执行，否则明确错误。
6. 补 matrix：None/Some("")、新增、修改、清空、原样、改后恢复原样；表/列同时改；表/列 rename；removed row；新增带注释列；默认值同时修改；复杂列不支持组合的错误。
7. 执行 4.1 同一定向命令。修复后全部 pass；真正无变化仍 assert NoChanges。

### 4.5 两种数据库往返与保真

文件：`tests/mysql_adapter.rs`、`tests/mariadb_catalog_mutation.rs`。

1. 两处添加测试前缀 `catalog_comment_`，分别用各自既有 profile/env 约定，不把 MySQL 运行冒充 MariaDB。
2. 建唯一表名，含 auto_increment 主键、非默认 collation 文本列、字符串/数值 default、ON UPDATE timestamp、生成列。按两种方言分别建立 STORED/VIRTUAL fixture，插入示例数据。
3. 初始表和列有注释；load definition 断言值回填，再改表和各类列注释、plan、execute、reload，断言注释精确值与数据、原生属性不变。
4. 新值含单引号、反斜杠、换行、中文、首尾空白；分别新增、修改、清空；reload 后再次原样 plan 为 NoChanges。
5. 覆盖表 rename+comment、列 rename+comment；清理所有实际对象（包括 rename 后对象），避免固定表名冲突。
6. mode 分支至少在 helper 测试覆盖；已配置可控数据库时追加 NO_BACKSLASH_ESCAPES 往返，不通过单个池连接 SET SESSION 后假定所有池连接已改变。
7. require 模式下缺 URL 必须失败；普通本地缺环境输出明确 SKIP。

命令（配置好的数据库，URL 值不得写入日志）：

```sh
LAZYDB_REQUIRE_DATABASE_TESTS=1 cargo +1.94.0 test --locked --test mysql_adapter catalog_comment_ -- --nocapture --test-threads=1
LAZYDB_REQUIRE_DATABASE_TESTS=1 cargo +1.94.0 test --locked --test mariadb_catalog_mutation catalog_comment_ -- --nocapture --test-threads=1
```

**单元复核：** 检查 SQL 只使用目标 schema/new_name；column comment 改动没有移除原生选项；表注释-only 不改列；局部失败仍沿用既有 autocommit 行为；没有无意改变 create/view 功能。

**单元完成条件：** 离线回归与保真测试通过，两种数据库测试均已添加并按环境执行/明确记录未执行。无需等待人工 PTY。Luna 按工作流提交策略提交此闭环（建议 `fix(mysql): preserve table and column comments in catalog edits`），继续 Oracle。

## 5. 可验收单元二：Oracle 注释完整闭环

文件：`src/db/oracle.rs`、`tests/object_mutation_contract.rs`、`tests/oracle_adapter.rs`。

1. 以真实 table/column baseline 添加 `oracle_comment_` planner 测试，覆盖只改表、只改列，不沿用空 columns 的旧 rename fixture作为唯一证据。
2. 运行并确认旧行为 fail：

```sh
cargo +1.94.0 test --locked --test object_mutation_contract oracle_comment_
```

3. 按 2.3 独立查询 all_tab_comments/all_col_comments，参数绑定并映射真实列名；table fingerprint 含表、列 comment。
4. 表编辑 match 不再丢弃 baseline；收集 mutable statements，rename 在先，COMMENT 在后，空列表最终仍 NoChanges。
5. 核验结构不变：已有列身份、集合、顺序及类型/nullability/default/identity/generated/collation 的已知值。注释变化不算结构变化；未支持的结构变更即使同时改 comment 也要明确拒绝。
6. 兼容现有 rename 测试约定：若旧 fixture 空 baseline + 空白 Added placeholder，不因新结构校验误回归；优先把该测试改为真实 loaded-table fixture，保留 rename SQL 验收目标。
7. 设置 table edit 的 native_identity_changed；保持 view/sequence 既有行为。
8. 扩展离线矩阵：新增/修改/清空/无变化/Unsupported 空值、特殊标识符与单引号/空白/Unicode、多个列、rename+comment、结构混合拒绝，断言 plan.validate 与 target/selection。
9. 执行：

```sh
cargo +1.94.0 test --locked --test object_mutation_contract oracle_comment_
cargo +1.94.0 test --locked --no-default-features --test object_mutation_contract
```

10. 在 oracle_adapter 添加 `catalog_comment_` 往返测试，使用 `LAZYDB_TEST_ORACLE_URL`、`LAZYDB_TEST_ORACLE_USER`、`LAZYDB_TEST_ORACLE_PASSWORD`，遵循既有 cfg(feature = "driver-oracle")。明确报告缺凭证/缺客户端的 skip，不把 DPI-1047 伪装成通过。
11. 使用 UUID 派生的短对象名避免旧 Oracle 标识符长度边界，创建表并设置初始注释，load→draft→plan→execute→reload；检查表/列值、数据、rename、清空、重复无变化；清理对象。
12. 环境具备时运行：

```sh
cargo +1.94.0 test --locked --test oracle_adapter catalog_comment_ -- --nocapture --test-threads=1
```

**单元复核：** 每个 COMMENT 是独立 statement，清空语义正确；Oracle 不重建列；未读到 comment 不能虚构成空；纯注释不报身份改变。

**单元完成条件：** 所有离线回归通过，driver feature 开关都能编译相关 planner 测试；往返测试已添加且当前环境执行情况记录清楚。Luna 可提交 `fix(oracle): plan and reload table and column comments`，继续最后单元。

## 6. 可验收单元三：Review SQL 链路与收尾

文件：`tests/catalog_editor_reducer.rs`，必要修正限定已列任务文件。

1. 使用现有 connected-session/editor fixture 新增 `catalog_comment_preview_` 测试，至少覆盖 MySQL 与 Oracle，可循环 MariaDB kind。
2. 按 UI action 编辑注释（复用既有输入行为，不重复实现 keymap），发 CatalogEditorPreview，断言返回的 Command 携带修改后 draft 与原 baseline。
3. 调用实际适配器 planner 得到 plan，向 reducer 注入 CatalogMutationPlanReady，断言预览 SQL 含期望注释且不处于 error。不得用手工构造的成功 plan 绕开本次缺陷。
4. 执行：

```sh
cargo +1.94.0 test --locked --test catalog_editor_reducer catalog_comment_preview_
```

5. 定向功能齐备后一次性执行第 1.B 节三项项目 Rust 门禁，不再机械重复 cargo check + clippy + 全量 test；新代码变化只重跑受影响测试，必要时再跑相应门禁。
6. 全量测试若报 principal 持久化相关失败，先对照起点及用户两项本地修改归因，记录属于基线/任务回归；不能为获取绿灯复制未提交修复或全量暂存用户文件。
7. 有数据库环境时完成 MySQL/MariaDB 定向往返或由既有 CI 提供证据；Oracle 环境限制按分级记录。未配置不反复尝试启动未知外部服务。
8. Luna 审查全部 diff：公共字段迁移只有 None；没有新建表/view/sequence 范围扩张；没有静默丢原生选项；注释以值引用，标识符以标识符引用；no-change 不误报；工作区与 scope 匹配。
9. 更新 validation.md，列本轮实际命令、exit code、提交/diff 状态、feature、数据库 kind/version/mode、真实 executed/skipped 数量。日志中不输出 URL 密码。
10. 按工作流完成提交合并；显式暂存任务路径，不使用不加选择的 git add .。主工作区未提交持久化文件不能进入此任务提交。建议最后提交 `test(catalog): cover comment edits through SQL review`。

## 7. 最终验收清单

- [ ] MySQL/MariaDB 表注释及列注释的 add/change/clear 生成有效 SQL。
- [ ] Oracle 表/列 COMMENT ON 生成、顺序及清空正确。
- [ ] 所有支持数据库加载已有注释，执行重载后显示实际值。
- [ ] unchanged、Some("")/None、恢复原值不会产生伪变化。
- [ ] MySQL comment-only 保留原生列属性；复杂表达式/字符串中的 COMMENT 不被误改。
- [ ] 组合 rename/position 正确；不能保真的结构组合明确报错，不静默丢字段。
- [ ] Oracle comment-only identity=false；rename=true。
- [ ] reducer Preview 经过真实 planner；离线测试在无 Oracle driver 时可运行。
- [ ] 项目 Rust 门禁已运行，真实失败/基线失败/环境限制有准确记录。
- [ ] MySQL/MariaDB 集成测试服从 CI require 约定；Oracle 执行或 skip 有明确证据。
- [ ] 人工 PTY 仅补充，没有升级为用户强制门禁。
- [ ] scope 内改动完整，用户未提交文件和主目录 state/index 未被误改。

## 8. 当前阶段结果与交接

plan 阶段只完成了静态核对与实施规划，未运行上述拟执行 cargo 命令，未编写业务代码。`change-scope.json` 是本计划预计业务文件的精确清单，任务目录的报告/回执不属于待提交业务范围。

下一步由 Luna 在自动工作流提供的任务 worktree 中，从单元一第 4.1 步编写失败回归开始，完成第一个业务闭环后继续单元二和单元三。任务不需要用户再选择执行方式。
