# MariaDB Catalog Compatibility Implementation Plan

> **执行者：Luna。** 按本计划逐项实现、验证、审查并完成提交合并；Astra 仅负责分析与计划。不启动子 Agent，不因普通实现取舍要求用户 resume。工作流和任务分支名称由 Luna 自动确定。

**Goal:** 修复 MariaDB 11.4 连接后表、视图目录加载失败，并使同源的序列分页与目录搜索正常工作。

**Architecture:** 保持现有 MySQL/MariaDB 共用适配器，在 MariaDB 分支统一从 `information_schema.tables` 的 `TABLE_TYPE='SEQUENCE'` 获取序列名称及计数。同步对齐能力入口、对象投影与搜索；通过公共 DatabaseConnection 接口验证真实数据库上的目录到预览闭环。

**Tech Stack:** Rust 1.94、Tokio、SQLx 0.9、MariaDB 11.4、Cargo integration tests、GitHub Actions。

---

## 0. 执行上下文与证据边界

- 原工作区 `/Users/yelog/workspace/tui/lazydb`；目标分支 `main`；起点 `a09dc6a1a37f957f6336b1516afd5a779dfb945d`。
- 分析：原工作区 `.git/opencode-tasks/ses_f52a54874ffefEUoEu58NbOzg9/analysis.md`；验证：同目录 `validation.md`。
- 已确认：本地 `11.4.13-MariaDB-ubu2404` 上完整分组 SQL 报 1109；将序列计数改为 TABLES 后返回 6 张表、1 个视图、1 个函数、1 个过程、1 个触发器、0 个序列。8 项现有 smoke/profile/connection 测试通过，未覆盖目录。
- 官方 SEQUENCES 系统表从 11.5 起存在；11.4 可以通过 TABLES 枚举序列。升级 Docker 镜像不能修复旧版本兼容性，也不能消除分页入口及投影错误。
- `checkpoint.json` 在分析与计划时均不存在。读取现有状态后继续，不自行修改 `state.json` 或 checkpoint；保留既有未跟踪 `.git-opencode-tasks/`。
- 本文件为计划，不代表修复已实施或测试已通过。后续首先从实际 diff 确认进度，不重复执行已通过且代码/环境未变的检查。

### 开始实现前

### 验收来源与门禁分级

| 类别 | 内容及依据 | 完成要求 |
| --- | --- | --- |
| 用户需求 | 用户给定连接在本地 MariaDB 上能加载表、视图等，并修复导致故障的代码 | 必须完成；通过公共适配器真实目录与预览测试提供可重复证据，无需用户人工确认才能收尾 |
| 本次修复回归 | 分析确认同源的序列分页、搜索、空/非空序列计数及 MySQL 能力边界；本计划新增目录集成测试 | 作为本次技术验收执行；属于所选修复方案的回归范围，不宣称用户额外要求了新功能 |
| 已有项目强制检查 | `.github/workflows/ci.yml:81-83` 的 fmt、clippy、全量 tests；数据库 job 中 MariaDB 11.4 与 MySQL 8.4 测试 | 遵循既有 CI；本地完成适用检查，CI 结果与本地结果分别记录；未运行/跳过不能写成通过 |
| 本计划新增自动回归入口 | 将 `mariadb_catalog` 加入现有 MariaDB job 的显式列表 | 随修复交付，避免新增测试在真实数据库 CI 中遗漏 |
| 补充建议验证 | TUI/PTY 人工冒烟、截图比对、额外 MariaDB 10.11/11.5+ 矩阵、独立本地 MySQL 重复验证 | 非新增必需门禁；环境不足时最多一次定向修复重试，由 Luna 审查证据后记录限制或补证，不要求用户 resume |

`git diff --check` 是低成本改动检查，不替代编译或测试。当前 CI 中发行脚本、Windows 安装器及平台依赖检查继续由既有工作流执行，本任务不把它们扩张为全部必须在本地重建的环境。若相关平台 CI 失败，按实际原因处理，不能无依据忽略。

### 实现准备步骤

1. 由 Luna 自动命名工作流/分支，并在工作流管理的工作树实施。参考配置的 worktree parent 为 `/Users/yelog/workspace/tui/lazydb-worktree`；避免与插件重复创建。
2. 确认基线和 diff，将本计划带到任务分支。仅隔离本任务的业务代码与计划，不覆盖已有用户工作。
3. 在执行 shell 中设置本地集成环境：

```sh
export LAZYDB_TEST_MARIADB_URL='mariadb://lazydb:lazydb_password@127.0.0.1:3307/lazydb_test'
export LAZYDB_REQUIRE_DATABASE_TESTS=1
```

4. 容器已在原工作区启动；在新工作树优先使用同一地址，不再启动一套争用 3307 的 Compose 服务。需要检查时使用原工作区的 `docker compose -f docker-compose.mariadb.yml ps`。

## 1. 单一业务闭环：目录、序列、搜索与表/视图预览

**Files:**
- Modify: `src/db/mysql.rs:1041-1052,1544-1624,1689-1721,168-171`。
- Create: `tests/mariadb_catalog.rs`。
- Reference: `tests/mysql_adapter.rs:308-344,1015-1079`，`tests/support/mod.rs`，`src/db/catalog.rs`。
- Modify: `.github/workflows/ci.yml:167-168`。

### Step 1 — 先写最小真实数据库失败用例

新文件使用 `mod support;` 和 `support::mariadb_test_url()`，通过 `import_connection_url` / `DatabaseConnection::connect` 连接，采用配置的库名（probe.database），不要求全局 CREATE DATABASE 权限。

构造本测试自己的 CatalogRequest helper，沿用已有 contract：

```rust
fn catalog_request(
    profile_id: uuid::Uuid,
    target: lazydb::db::catalog::CatalogTarget,
    scope: lazydb::profile::CatalogScope,
    page_size: usize,
    cursor: Option<lazydb::db::catalog::CatalogCursor>,
    request_id: u64,
) -> lazydb::db::catalog::CatalogRequest {
    use lazydb::db::catalog::{CatalogRequest, CatalogRequestKey};
    CatalogRequest {
        key: CatalogRequestKey {
            connection: lazydb::identity::ConnectionIdentity {
                profile_id,
                generation: 7,
            },
            catalog_epoch: 3,
            request_id,
            target,
            cursor,
        },
        scope,
        page_size,
    }
}
```

测试名 `mariadb_catalog_groups_objects_search_and_preview`。先请求 Databases、Schemas，再请求 Groups；每页调用 `validate_for(&request)`，验证配置库及其镜像 schema 可见。schema id 使用 `[database_name, database_name]`。

```sh
cargo test --locked --test mariadb_catalog mariadb_catalog_groups_objects_search_and_preview -- --nocapture --test-threads=1
```

预期当前基线 FAIL，Groups 请求返回 Unknown table 'sequences'。失败若源于连接/编译而非根因，先解决具体错误再记录，不能作为正确 red 证据。

### Step 2 — 扩展回归用例，覆盖完整契约

继续在同一测试内完成下列独立断言，或按可共享 fixture 的最小结构拆分，避免测试互相依赖：

1. Groups 基线计数；使用直接只读 SQL 的 SEQUENCE count 与分组 count 对照。默认本地 fixture 基线是 0，但测试不能假定共享库永远无序列。
2. 使用 UUID 生成唯一前缀，显式限定库名并通过 `mysql::quote_identifier` 引用。创建 parent 表（主键）、child 表（主键、外键、普通索引），各插入一行，创建 child 视图及两个序列。不要用固定名字 DROP 已存在对象。
3. 记录成功创建的对象；参考 mysql_adapter 的 `AssertUnwindSafe(...).catch_unwind()` 模式，在断言失败后仍执行清理。清理顺序为 view → child → parent → sequences，仅删除本测试创建的对象；最后恢复 panic/错误并关闭连接。
4. Groups 计数相对基线增加 Tables=2、Views=1、Sequences=2。遍历 Tables/Views 找到自己创建的对象并断言序列不混入普通表。
5. Sequences 使用 page_size=1 遍历，验证每个 page、Exact count、终页 next_cursor=None；用 HashSet 校验所有 id 唯一，并设置基于首个 Exact count 的循环上界，防止错误游标无限循环。共享库可以有其他序列，断言自己的两个名字恰好各一次，顺序符合现有二进制 keyset 排序。
6. 对序列断言 kind=Sequence、native_path=`[db,db,name]`、非 relation、不可展开、comment 可空；确保名称大小写及引用不会破坏身份。不要把 sequence 当作可预览表。
7. 对 child 的 RelationChildren 验证列、主键、外键与普通索引正确返回；通过 `DatabaseConnection::preview_relation` 或 `preview_relation_with_scope` 预览 child 与 view，验证插入的一行数据。使用实际目录返回的 CatalogId。
8. `CatalogSearchRequest` 使用相同 connection/profile，`session_id=1,generation=1,limit=100`，query 为唯一前缀，scope 限制配置库。AllObjects 返回自己的序列、表、视图；RelationsOnly 返回表/视图且不包含序列。对同一 query 使用 `CatalogSelection::Selected(vec![])` 的数据库 scope，结果必须为空。
9. 创建前先用同一唯一前缀做一次搜索，结果为空且请求成功；这在没有序列时仍能捕获旧 CTE 引用不存在系统表的问题。

优先让真实测试覆盖业务行为，不添加仅检查 SQL 字符串包含某关键词的测试，不将测试结果与实现重复建模。

### Step 3 — 实施集中修复

`src/db/mysql.rs` 的修改采用以下明确内容，保持周边绑定/事务/错误处理结构。

**A. 对象入口：仅 MariaDB 允许 Sequences。** 替换当前统一拒绝分支：

```rust
if matches!(
    request.key.target,
    CatalogTarget::Objects {
        group: ObjectGroup::MaterializedViews | ObjectGroup::Types,
        ..
    }
) || (self.kind != DatabaseKind::MariaDb
    && matches!(
        request.key.target,
        CatalogTarget::Objects {
            group: ObjectGroup::Sequences,
            ..
        }
    ))
{
    return Err(DatabaseError::unsupported_catalog_target(
        self.kind,
        &request.key.target,
    ));
}
```

**B. 分组计数：** MariaDB 分支最后一个子查询替换为：

```sql
(SELECT CAST(COUNT(*) AS CHAR) FROM information_schema.tables WHERE BINARY table_schema=BINARY ? AND table_type='SEQUENCE') AS sequences
```

保留现有六个 bind、String → u64 解析与分组排序，避免额外引入数字解码兼容变化。

**C. 序列对象来源：** 替换匹配元组：

```rust
ObjectGroup::Sequences if self.kind == DatabaseKind::MariaDb => (
    "information_schema.tables",
    "table_schema",
    "table_name",
    "table_type='SEQUENCE'",
    CatalogKind::Sequence,
    "sequence",
    "table_name",
),
```

extra_columns 增加明确的序列分支：

```rust
ObjectGroup::Sequences => "NULL AS comment, NULL AS owner_name",
```

保留三段 CatalogId、CatalogEntry::object、count SQL 和现有 keyset/limit 逻辑。fallback 报错的数据库 kind 可就近改用 self.kind 以保持准确，但不开展无关重构。

**D. 搜索 UNION：**

```rust
const MARIADB_CATALOG_SEARCH_SEQUENCE_SQL: &str = " UNION ALL \
    SELECT 'sequence', table_schema, table_name, NULL, NULL, table_name, \
           CONCAT(table_schema,'.',table_name), NULL \
    FROM information_schema.tables WHERE table_type='SEQUENCE'";
```

不改搜索匹配、scope、RelationsOnly、排序规则；不通过吞掉 1109 错误伪造空数据。

### Step 4 — 定向 green 与共享 MySQL 边界

```sh
cargo test --locked --test mariadb_catalog -- --nocapture --test-threads=1
```

预期全部通过，数据库对象已清理；记录此时代码 diff 与服务版本。

查看 `tests/mysql_adapter.rs` 现有 unsupported target 用例。若已经覆盖 MySQL Sequences，运行该用例；若没有，在现有 MySQL 集成目录用例添加请求并断言 unsupported。真实 MySQL 8.4 验证由已有 CI MySQL job 承接，本地有独立 MySQL 环境时定向运行：

```sh
cargo test --locked --test mysql_adapter -- --nocapture --test-threads=1
```

必须核实 `LAZYDB_TEST_MYSQL_URL` 指向真实 MySQL，且明确是否设置。未设置的 skip 不是 MySQL 实测通过，MariaDB URL 也不能冒充 MySQL 验证。

### Step 5 — 纳入真实数据库 CI

`.github/workflows/ci.yml` 的 MariaDB adapter tests 命令中显式增加 `--test mariadb_catalog`。保留 `LAZYDB_REQUIRE_DATABASE_TESTS=1`、MariaDB 11.4 服务及已有各测试参数。当前 CI 显式枚举文件，因此只创建测试文件不足以让 database job 执行它。

若 Step 4 添加 MySQL 断言，已在 `--test mysql_adapter` 内自动执行，无需另建 job。无需新增多版本矩阵才能完成此次修复。

## 2. 最终验收与收尾（Luna）

### Step 1 — 一次最终项目检查

当前 CI 的 Rust 核心命令如下，在功能完整的最终 diff 上统一运行：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
git diff --check
```

本地已配置 MariaDB URL 时保留配置，确保全量中的 MariaDB 测试真实执行。全量其他数据库测试可能依赖单独配置或 ignored 标记，记录实际跳过，不声明已验证全部数据库。工具链缺失先正常安装/采用项目允许方式，普通编译错误自行修复。修复后只重跑受影响检查；不得每轮反复执行完整组合。

如果业务修复需要新增其他文件，先确认其必要性，并补充对应测试与验证记录；不要为赶计划忽略后续暴露的真实目录/预览错误。

### Step 2 — 本地 fixture 冒烟

通过本地连接重新加载 `lazydb_test`，确认表/视图目录、表列与索引、表/视图预览、目录搜索可用。可以进行一次 TUI/PTY 冒烟，记录操作路径和错误输出；用户没有强制 PTY，若环境不可用，公共适配器的真实端到端测试是主要证据。

人工/PTY 或额外版本等补充检查，最多一次有针对性的环境修复重试，再由 Luna 收尾审查决定补证或记录限制。不要无限维持 progress。10.11、11.5+ 为补充兼容检查；11.4 必须真实验证。

### Step 3 — 审查、记录、提交合并

1. Luna 审查最终 diff：MySQL 分支不放行序列；所有 MariaDB 目录序列来源一致；没有 routine_comment 泄漏到序列；测试清理可靠；CI 新测试确实执行。
2. 追加原任务目录 `validation.md`：命令、退出结果、当前 SHA/未提交 diff、环境、测试跳过与限制。保留此前分析/计划记录。
3. 仅暂存本任务文件：`src/db/mysql.rs`、`tests/mariadb_catalog.rs`、`.github/workflows/ci.yml`、本计划，以及确有必要修改的 `tests/mysql_adapter.rs`。不能 `git add .` 混入用户既有目录。
4. 使用 @git-commit 技能完成常规提交；建议语义 `fix(mariadb): restore catalog discovery on supported versions`，实际名称由 Luna 决定。
5. 按自动工作流的后续阶段完成审查、纠偏、提交合并，不由 Astra 抢先执行；写入当轮插件指定的新回执，不能复用旧 token/文件名。

## 完成标准

- MariaDB 11.4 普通账号的 Groups 请求成功，表、视图与数据预览恢复。
- 空序列基线与非空序列库均可加载；序列分页、identity、计数、搜索及 relation-only scope 正确。
- MySQL 对序列目录仍 unsupported；相关共享适配器回归通过或明确由 CI 验证。
- 新真实目录测试进入 MariaDB CI 显式列表；核心项目检查有当前代码版本的结果。
- 验证记录完整，测试对象清理完毕，无业务外改动；由 Luna 完成最终提交合并。
