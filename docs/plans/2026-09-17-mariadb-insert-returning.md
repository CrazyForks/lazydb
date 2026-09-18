# MariaDB Insert Returning Implementation Plan

> **执行者：Luna。** 按本计划逐项实施、验证、审查和提交；当前 Astra 仅完成计划。本任务不启动子 Agent。

**Goal:** 修复 MariaDB 表格新增行成功写入后因错误主键回查而提交失败或返回错误行的问题。

**Architecture:** MariaDB InsertRow 使用参数化 INSERT … RETURNING，直接解码数据库实际返回的新行；MySQL 保留现有方言路径。沿用现有事务 worker、错误传播、回滚和草稿恢复机制。

**Tech Stack:** Rust 2024、SQLx 0.9、Tokio、MariaDB 10.5+；本地验证实例为 MariaDB 11.4.13。

---

## 上下文、证据与边界

- 基线：`main` / `92c6bf758a3f3beaae74f3fb80a6f1985301e27a`。
- 分析：`.git/opencode-tasks/ses_f5144e1dfffeaCS3PdIToAVT0g/analysis.md`。
- 检查记录：同目录 `validation.md`，每次检查附具体代码版本及工作区状态。
- 根因位置：`src/db/mysql.rs:2924-2957` 仅取第一主键，用显式 NULL/0 或 last_insert_id 猜测新行身份。显式 NULL 在数据库触发自增后，旧代码仍查询 IS NULL，fetch_one 失败。
- 数据库行为已实测；截图不可读，尚不能确认截图报错与此缺陷完全对应。交付时保留该限定，不声称复现了完整截图操作。
- 当前 checkpoint.json 不存在；不创建它，不修改 state.json。本轮完成后仅写指定 `plan-012c55c5-1fb3-4212-a067-77079923644e.json` 回执，token 为 `012c55c5-1fb3-4212-a067-77079923644e`，不覆盖历史回执。
- 保留工作区已有未跟踪文件；Luna 在实施前按工作流命名任务分支。本计划建议名 `fix/mariadb-insert-returning`，最终由 Luna 确定。

## 验收与验证分级

1. **用户需求及工作流强制项**：修复 MariaDB 新增行提交问题；当前只做计划；后续由 Luna 自动实施、审查及提交合并；记录实际验证证据，禁止把旧结果当作当前结果；不得修改插件状态文件或要求用户反复 resume。
2. **本修复的自动化验收项**：真实 RelationMutation/SQLx 路径正确返回新增行，Commit 后持久化、Rollback 后无残留；自增 NULL/0、默认主键、复合主键回归得到覆盖；保留约束错误、值绑定和现有状态恢复语义。Task 1–4 的自动化检查用于证明这些行为，不是要求用户进行人工操作。
3. **项目强制门禁的证据边界**：当前未发现 AGENTS.md 或已确认的 CI 强制命令清单。Task 5 的 fmt/check/clippy/test/diff-check 是本计划选定的工程验证命令，不冒充用户指定或项目已证实的强制门禁。若后续读取到真实项目约定，应据其执行，并在 validation.md 标明来源。已有无关基线问题由 Luna 审查归因，不能无限重跑。
4. **补充建议验证**：PTY 人工操作、截图逐项对照、独立 MySQL 实例 live 检查、更多 MariaDB 版本矩阵。它们不自动成为完成门禁。不可获取截图或相应环境时记录限制；最多一次有针对性的环境修复重试，由 Luna 收尾审查判断是否值得补证，不保持 progress 循环。

下一阶段首个具体动作：Luna 核对实际 diff 并命名任务分支，执行 Task 1 的真实 worker 显式 NULL 失败回归，然后完成 Task 2–3 的新增提交闭环，继续 Task 4–5。用户已选择自动实施，不再询问执行方式。

## 计划修正：真实 mutation 测试放置位置

分析阶段建议扩展外部 `tests/mariadb_relation_mutations.rs`。进一步核对发现：MySqlTransactionBackend、transaction_backend、start_transaction_worker_with_forced_close 以及 worker 的请求字段均为 `pub(crate)`，外部集成测试不能直接调用。SQL Server 的公开 backend 示例不能照搬。

**采用 crate 内数据库集成测试**：在 `src/db/mysql.rs` 的现有 `#[cfg(test)]` 测试区域增加测试子模块，访问现有 backend/worker，不为测试扩大生产 API。原 `tests/mariadb_relation_mutations.rs` 的普通 SQL 冒烟测试保留。测试环境行为与 `tests/support/mod.rs` 一致：提供 LAZYDB_REQUIRE_DATABASE_TESTS 时缺 URL 必须失败。

## 单元一：新增行真实事务闭环

### Task 1：建立实际 worker 的失败回归

**Files:**
- Modify/Test: `src/db/mysql.rs` 的测试区域。
- Reference: `src/db/mutation.rs`、`src/db/transaction.rs`、`src/runtime/transaction.rs`、`tests/support/mod.rs`。

1. 增加 `mariadb_insert_returning` 测试模块，以测试 URL 导入 MariaDB profile 并连接；断言得到 MariaDb variant。使用 UUID 后缀创建 InnoDB 测试表，避免覆盖现有数据。
2. 构造三段路径 CatalogId `[database, database, table]`，MetadataFingerprint 列序与表序一致；RelationMutationRequest 的 connection/target/relation_key/scope 使用同一个 profile。
3. 从真实 adapter 创建 backend，调用 `spawn_transaction_worker`，等待 readiness；通过 TransactionRequest::RelationMutation 和 oneshot 请求返回结果。保持 cancel sender 存活至收到响应，避免意外取消。
4. 第一条回归使用 `id INT PRIMARY KEY AUTO_INCREMENT, value VARCHAR(32) DEFAULT 'server-default'`，插入 `columns=[0]`、`values=[InputValue::Null]`，断言返回非 NULL ID 及默认 value。旧代码应在 mutation 回应处失败，不能只检查原始 SQL INSERT。
5. 通过 worker Commit，等待 WorkerDisposition::Committed；独立连接查询实际行，断言与返回结果一致。用有超时的等待避免测试挂起。
6. 成功/失败路径关闭 worker 和连接并清理唯一命名对象；清理错误不得掩盖原始失败。不要依赖连接池不同连接共享临时表。

Run（本地仓库 fixture）：

```sh
LAZYDB_REQUIRE_DATABASE_TESTS=1 LAZYDB_TEST_MARIADB_URL='mariadb://lazydb:lazydb_password@127.0.0.1:3307/lazydb_test' cargo test --lib mariadb_insert_returning -- --nocapture
```

预期修复前：显式 NULL 回归因无返回行失败。记录实际错误，不预设图 2 的错误内容。

### Task 2：实现 MariaDB 原生返回路径

**Files:** Modify `src/db/mysql.rs`，InsertRow 分支约 2879-2961。

1. 保留插入列/值长度和索引检查、标识符引用、占位符生成及 DEFAULT 处理。
2. 在构造 INSERT 后、创建 query 前，MariaDB 分支追加返回列。用 metadata.columns 顺序显式引用返回列，避免 RETURNING * 对 invisible 列或返回列序的歧义；metadata.columns 为空时在执行前返回 malformed 错误。
3. 继续复用原有 Null/Value 参数绑定。绑定完成后，MariaDB fetch_one，decode_row，返回 Inserted；MySQL 继续 execute 及原有回查。

关键代码形态（放入现有变量作用域，按现有格式组织）：

```rust
let returning = self.adapter.kind == DatabaseKind::MariaDb;
if returning {
    if columns.is_empty() {
        return Err(TransactionError("MariaDB insert mutation has no relation columns".into()));
    }
    sql.push_str(" RETURNING ");
    sql.push_str(
        &columns
            .iter()
            .map(|(name, _, _)| quote_identifier(name))
            .collect::<Vec<_>>()
            .join(", "),
    );
}
```

将现有 INSERT 的 `let sql` 改为 `let mut sql`；保留现有创建 query 和绑定循环。紧接绑定循环插入：

```rust
if returning {
    let row = query
        .fetch_one(&mut *self.connection)
        .await
        .map_err(|error| TransactionError(error.to_string()))?;
    return Ok(MutationResult::Inserted {
        row: decode_row(&row),
        version: None,
    });
}
```

4. 重跑 Task 1 定向命令，预期返回行准确且 Commit 后可独立读取。
5. 不在 RETURNING 失败后重试 INSERT；不在 UI 删除 NULL 值；不更改 MySQL 方言、update/delete 分支或公共 mutation 数据结构。

### Task 3：完善行为矩阵及回滚验收

**Files:** Test `src/db/mysql.rs` 的上述测试模块。

复用连接/请求/worker 辅助函数，按业务行为分组，避免每个输入复制整套框架：

| 案例 | 核心断言 |
| --- | --- |
| 省略自增主键、显式 NULL、InputValue::Default | 返回实际生成 ID 和完整行 |
| 显式 0，session SQL mode 关闭 NO_AUTO_VALUE_ON_ZERO | 返回生成 ID，不按 0 回查 |
| 显式 0，开启 NO_AUTO_VALUE_ON_ZERO | 返回真正存入的 0，不强制改写 |
| 显式非零主键 | 返回该 ID |
| columns/values 均空 | `() VALUES () RETURNING ...` 返回默认列 |
| 默认 UUID 主键 | 返回 UUID，独立查询与之相同 |
| 复合键首列重复 | 返回本次插入的第二行，不能返回旧行 |
| 带引号需求的列名、invisible 默认列 | 引用正确，返回值与 metadata 顺序一致 |
| 显式 NULL 普通可空列 | 保持 NULL，不偷偷套用默认值 |
| 唯一约束失败 | mutation 失败，调用 Rollback，独立连接确认整批没有残留 |
| 成功 mutation 后主动 Rollback | 独立连接看不到新行 |

SQL mode 只通过 worker 的 Execute 请求在该 session 设置，不修改 GLOBAL。每个 SQL mode 场景隔离 worker/连接并恢复 session 配置，避免连接池污染。

Run Task 1 定向命令；预期全部真实数据库行为通过。对返回整数允许使用 CellValue 的实际解码类型，不能通过只比字符串掩盖列顺序错误；对 UUID 验证格式及数据库持久化值。

## 单元二：回归、审查与交付

### Task 4：状态机和共享驱动回归

**Files:** 预期只读取 `src/app.rs` 现有测试；仅在发现新覆盖缺口时增加必要测试。

```sh
cargo test --lib relation_mutation_failure
cargo test --lib failed_relation_mutation_clears_remaining_writes_and_restores_full_snapshot
cargo test --test mysql_adapter
LAZYDB_REQUIRE_DATABASE_TESTS=1 LAZYDB_TEST_MARIADB_URL='mariadb://lazydb:lazydb_password@127.0.0.1:3307/lazydb_test' cargo test --test mariadb_relation_mutations
```

预期相关状态机测试通过，错误仍请求回滚，草稿可恢复。确认各筛选命令实际运行非零测试。最后一条是已有 SQL 冒烟检查，不能替代 Task 1–3 的 mutation 验证。

审查 SQL 分支：仅 MariaDb 使用 RETURNING；所有用户值仍绑定参数；MySQL 的行为未被 MariaDB 分支改变。若有 MySQL 实例可补充 live 回归，没有则记录覆盖边界，不把 MariaDB 实例称作 MySQL 验证。

### Task 5：一次最终验证及交付

功能完成后执行：

```sh
cargo fmt --all -- --check
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
git diff --check
```

最终测试运行时继续为新增测试提供 `LAZYDB_TEST_MARIADB_URL`；不要把 LAZYDB_REQUIRE_DATABASE_TESTS 全局开启导致其他引擎缺环境误判，定向 MariaDB 检查已用强制开关证明真实执行。其他数据库测试是否跳过按实际日志记录。

将每条命令、退出码、版本及环境写入任务 validation.md。编译或功能错误自行修复；人工/PTY 环境限制最多一次有针对性重试，然后记录限制。未改源码无需重复既有全量检查。当前没有强制 PTY 截图验收要求。

由 Luna 最终审查：
- 显式 NULL 回归是否在旧代码失败、修复后经真实 worker 成功？
- Commit/rollback 后是否由独立连接确认？
- 是否准确返回复合主键和默认主键对应的行？
- 是否保留原始约束错误和草稿恢复语义？
- 是否将缺失截图证据与已确认代码缺陷分开陈述？

只暂存本任务修改并提交；提交时遵循 git-commit 技能。建议提交信息 `fix(mariadb): return inserted rows directly from mutations`。提交/合并由 Luna 依工作流完成，不交给用户手动续做。
