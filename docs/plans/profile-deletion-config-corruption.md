# 连接删除持久化与失败反馈 Implementation Plan

> 执行者：Luna。按下述步骤完成同一个端到端业务闭环，再由 Luna 审查、纠偏及按工作流提交合并。不得启动子 Agent。本计划取代技能模板中的 Claude/子代理交接要求。

**Goal:** 连续删除连接后配置始终合法、删除结果重启后仍成立，保存失败在确认窗口可见且可重试。

**Architecture:** 保留现有 ProfileStore 原子写入和 Runtime 事务结构，修正兼容合并对可用连接的识别，并在合并结束后重建所有表的输出位置、校验最终 TOML。复用既有 `render_message_line` 绘制确认窗口反馈，以持久化、Runtime/App、TestBackend 三层回归验证完整链路。

**Tech Stack:** Rust 2024 / 项目 CI Rust 1.94.0，serde，toml 0.9，toml_edit 0.23.10+spec-1.0.0，Tokio，Ratatui，tempfile。

---

## 基线与执行约束

- 根目录 `/Users/yelog/workspace/tui/lazydb`；起点及计划阶段 HEAD：`b7c3c0dd7c3df7ec2c6200d0eee969c6ee5002be`；目标 `main`。
- 分析与计划时工作树及 index 干净。没有需要复制到新 worktree 的未提交业务行为。后续若状态变化，先检查 diff，禁止清空工作区/stash/整体提交用户修改。
- 任务、分支、worktree 由后续工作流/Luna 命名创建，本阶段不代建。报告及本计划只写原目录 `.git/opencode-tasks/ses_f4274b917ffeqxbJYcrB0euHFM/`，不因 state.json 的默认 planDirectory 而另写 docs/plans。
- 本阶段 checkpoint.json 不存在；state.json 仅读。具体分析和复现见同目录 analysis.md；不把分析探针冒充最终实现测试。
- 这是一个可验收单元，下面 Task 是内部步骤，不在 Task 间要求用户 resume。实现、检查失败纠正及收尾由 Luna 持续完成。
- 已损坏的用户配置没有提供；修复不会自动恢复已有错绑子表。不在正常加载路径中添加猜测性去重或静默丢弃连接。保留原件后按 UUID/预期访问范围恢复属于单独的数据恢复操作。

## 修改范围清单

机器可读清单见同目录 `change-scope.json`，包含全部预计修改文件：

- `src/persistence/profiles.rs`
- `src/ui/profiles.rs`
- `src/app.rs`（条件修改：仅回归证明状态恢复缺陷时修改；提前纳入范围）
- `tests/profile_compatibility.rs`
- `tests/persistence.rs`
- `tests/profile_reducer.rs`
- `tests/profile_runtime.rs`
- `tests/profile_lifecycle.rs`

无预计删除、重命名或新增仓库文件，无未提交依赖文件。`src/runtime.rs`、`src/input/keymap.rs`、Cargo 文件及 CI 配置仅供参考，不列入预计修改。任务目录的报告、回执和验证记录是工作流产物，不属于要移入 worktree 提交的业务变更；本计划不额外创建 docs/plans 文档。

## 验证依据与门禁分级

| 类别 | 来源及要求 | 执行与判定 |
|---|---|---|
| 用户需求验收 | 连接可以连续删除；配置不再因删除损坏；失败不再表现为无反应 | 用 Task 1–4 的自动化持久化、Runtime/App、TestBackend 用例验证。具体框架和屏幕尺寸是本计划选择的验证手段，不是用户额外指定的人工门禁 |
| 修复兼容性验收 | 分析选定方案要求不丢未知配置、不错绑子表、保存失败不提交内存状态或覆盖原文件 | Task 1–4 的语义断言和回滚测试；与主要需求组成同一业务闭环 |
| 项目现有强制检查 | `.github/workflows/ci.yml` 的 Rust fmt、clippy、all-targets/all-features test，以及各 CI job 自身适用条件 | Task 5 列出确切 Rust 命令；不得以其他 toolchain 的本地通过冒充精确 CI 通过。其他平台/数据库 job 仍由原流水线执行，不修改其要求；本地无法复现时记录未执行项 |
| 补充建议检查 | 人工终端操作、PTY、额外截图、针对本修复另行启动真实数据库服务 | 非用户指定门禁，不因缺少这些环境阻止已经有自动化证据的闭环。环境问题最多一次有针对性修复重试，再由 Luna 收尾审查记录限制或补证 |

## 核心事实（避免重新调查）

1. `src/persistence/profiles.rs:375–465` 克隆旧表到新文档，携带旧 position。toml_edit Display 全局按 position 排序，可能把 access 放到错误的 profile 后。
2. `411–423` 已知类型硬编码漏 Redis，使已删 Redis 被追加；`294–305` 不可用类型分类也漏 Redis。
3. `save:342–365` 没有验证合并后的字符串便写入/rename，第一次损坏写入返回成功，下一次保存及重启才解析失败。
4. `src/app.rs:9771–9781` 已解除删除 busy 并存储错误；`src/ui/profiles.rs:238–332` 确认窗口不显示消息。`916–933` 的 `render_message_line` 已处理等级和终端文本清理，可直接复用。
5. Runtime 删除函数持 `profile_mutation` 锁，保存成功才提交 registry；保留现有凭据失败回滚。

## Task 1：建立能够抓住表错绑和 Redis 复活的回归

**Files:**
- Modify/Test: `tests/profile_compatibility.rs`
- Modify/Test: `tests/persistence.rs`

### Step 1 — 添加两个独立根因测试

建议测试名：

- `deleting_middle_redis_does_not_restore_it_or_corrupt_profiles`
- `preserved_unknown_profile_tables_keep_their_own_nested_values`

第一个可直接沿用现有 import helper：

```rust
#[test]
fn deleting_middle_redis_does_not_restore_it_or_corrupt_profiles() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("connections.toml");
    let store = ProfileStore::new(path.clone());
    let first = import_connection_url("sqlite::memory:", Some("first")).unwrap().profile;
    let middle = import_connection_url("redis://localhost:6379/0", Some("middle")).unwrap().profile;
    let last = import_connection_url("sqlite::memory:", Some("last")).unwrap().profile;
    store.save(vec![first.clone(), middle, last.clone()]).unwrap();
    let remaining = vec![first, last];
    store.save(remaining.clone()).unwrap();
    let saved: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved["profiles"].as_array().unwrap().len(), 2);
    assert_eq!(store.load().unwrap().profiles, remaining);
    store.save(remaining.clone()).unwrap();
    assert_eq!(store.load().unwrap().profiles, remaining);
}
```

第二个不能依赖 Redis 漏判才能失败：生成三条 profile，将中间条的 kind 改为 `future-db`，保留不同的 `[profiles.access]` 及多层未知表/ArrayOfTables。`load_report` 应得到两条可用、一条 unavailable；保存可用集合，按 id 比较未知 profile 的完整 `toml::Value`（允许格式变化）及其他 profile 的 access/catalog_scope 值。重复保存、反转可用条顺序后再保存，仍必须等价。这样只补 Redis 的伪修复不能通过。

### Step 2 — 定向证明红灯

```sh
cargo test --locked --test profile_compatibility deleting_middle_redis_does_not_restore_it_or_corrupt_profiles -- --exact
cargo test --locked --test profile_compatibility preserved_unknown_profile_tables_keep_their_own_nested_values -- --exact
```

预期当前基线至少在 parse/load 或准确 id/归属断言失败。记录真实失败，不把编译错误当作业务红灯。

### Step 3 — 补足边界矩阵

在相同测试文件或现有 persistence 文件补充：

- 首/中/尾删除及最后一条删除；每次保存后重新 load，精确比较顺序、UUID、access 和 catalog_scope。
- 未知类型、已知类型带未来字段、缺少必填字段的旧记录：保存其他可用记录时保留，unavailable 分类符合实际。
- Redis 带未来字段：应归类 UnsupportedConfiguration，而非 UnsupportedKind。
- 已匹配 id 的未知嵌套字段和嵌套 ArrayOfTables 合并；使用不同哨兵值检验字段没有跨 id 迁移。
- 原文件已损坏时 save 返回错误且原字节不变。
- 保留全部现有迁移、组、secret 字段测试。不要以 contains 字符串代替 parse + 每条记录语义断言。

## Task 2：修复 ProfileStore 的合并与提交边界

**Files:**
- Modify: `src/persistence/profiles.rs`，`load_current_report`、`preserve_unavailable_profiles`、`ProfileStore::save`
- Test: Task 1 文件

### Step 1 — 一致识别可加载记录

对旧 profile 没有在新集合匹配 id 的分支，判断旧表能否按当前 `ConnectionProfile` 成功反序列化：成功说明它原本属于可用集合，省略即删除；失败说明属于无法加载的旧记录，需要保留。可以由同一个辅助函数返回解析结果，供加载及保存分类复用；避免新建另一份数据库名列表。

建议转换方式：把 `old_profile.clone()` 放入临时 `DocumentMut`，递归重编号后转成字符串，再以 `toml::from_str::<ConnectionProfile>` 解析。或者使用库支持的结构化 serde 转换；必须证明表头路径、嵌套数组及读取失败语义正确。不要对一个保留了原 `profiles.*` 路径的字符串片段直接猜测去前缀。

不可用原因的 kind 判断复用枚举反序列化，例如将 kind 转为 `toml::Value::String(kind.to_owned()).try_into::<DatabaseKind>()`。保持缺失 kind 的现有 InvalidConfiguration 路径，保持未知字段错误的 UnsupportedConfiguration 区分。

已匹配 id 的 merge 继续新值优先；延续 secret_ref/password/secret 过滤。本次不扩展为 schema 迁移系统。

### Step 2 — 完整结构重编号

在完成全部插入后、`document.to_string()` 前递归设置严格递增的位置。可采用以下完整实现形状：

```rust
fn normalize_table_positions(table: &mut toml_edit::Table, next: &mut isize) {
    table.set_position(*next);
    *next += 1;
    for (_, item) in table.iter_mut() {
        match item {
            toml_edit::Item::Table(child) => normalize_table_positions(child, next),
            toml_edit::Item::ArrayOfTables(children) => {
                for child in children.iter_mut() {
                    normalize_table_positions(child, next);
                }
            }
            _ => {}
        }
    }
}
```

调用用 `let mut next = 0; normalize_table_positions(document.as_table_mut(), &mut next);`。保留 Table 的 dotted/implicit/decor 属性；inline table 属于 Value，不输出独立表头无需修改。不能只重编号 `[[profiles]]` 而遗漏 access、credential_policy、catalog_scope 或未来表。

### Step 3 — 校验最终输出再提交

在 `save` 中合并完成后、创建 temporary 之前添加：

```rust
let _: toml::Value = toml::from_str(&contents)?;
```

复用现有 Decode 错误类型。不要将最终文本严格反序列化为整个 ProfileFile；其中合法的未知类型/字段正是兼容保留的目标。保持文件权限、sync、rename 和失败清理逻辑。

确认空集合输出：没有 preserved 项时保持合法 `profiles = []`；有 preserved 项时必须转换成 ArrayOfTables，不能丢掉保留项或重复赋值。

### Step 4 — 定向运行并修正

```sh
cargo test --locked --test profile_compatibility --test persistence
```

预期全部通过，特别是未知 profile 的语义归属测试。若项目现有测试暴露兼容行为变化，以加载/保存一致及不丢不可用记录为约束修正，不通过放宽断言掩盖。

## Task 3：确认窗口显示失败并保持操作可恢复

**Files:**
- Modify/Test: `src/ui/profiles.rs`（同文件新增 `#[cfg(test)] mod tests`，可直接调用私有 renderer）
- Modify/Test: `tests/profile_reducer.rs`
- Conditional modify: `src/app.rs`，仅当回归证明现有状态恢复有缺陷

### Step 1 — Reducer 回归

复用 `uuid_targeted_new_edit_cancel_and_delete_confirmation_are_pure` 的初始化方式。选定 profile → 打开确认 → 切换 Delete → confirm，取得实际 request_id。

- 注入同 request_id 的 `ProfileDeleteFailed { message: "Unable to save connection profiles: invalid TOML" }`。
- 断言 operation 为 None，消息等级 Error、文本一致，profile 仍存在且确认页仍在。
- 再确认必须发出新 DeleteProfile 请求；取消关闭窗口。
- 过时 request_id 的失败不能终止当前新请求。

该 reducer 正常情况下已通过；其作用是证明 UI 修复不会改变事务/请求隔离语义。

### Step 2 — TestBackend 先抓住漏绘

在 `src/ui/profiles.rs` 内单元测试直接构造 ConfirmDelete 状态，设置 message 后调用 `render_profile_manager` 或 `render_confirmation`，用 TestBackend buffer 拼接可见文本。

建议测试名：`delete_confirmation_shows_error_and_actions`、`delete_confirmation_shows_warning_on_small_terminal`。覆盖 80×24 和 60×16；长多行解析错误，至少清晰显示失败摘要，并保留 Cancel/Delete 操作可见、hit_regions 在视口内。检查 Warning 同样显示，Busy 时按钮禁用语义仍正确。附极小视口无 panic 测试，但不要求任意 1 行屏幕同时容纳全部内容。

```sh
cargo test --locked --lib ui::profiles::tests::delete_confirmation -- --nocapture
```

现有 renderer 预期在错误/警告可见断言失败。

### Step 3 — 最小布局修复

保持当前确认框风格，在按钮之前分配独立反馈 Rect，调用：

```rust
render_message_line(frame, manager, feedback_area, theme);
```

从可用高度扣除按钮和快捷键行，再按是否有消息划分正文与反馈。正常尺寸允许约 3–4 行反馈；紧凑尺寸先保证错误摘要和操作，再减少固定说明正文。所有尺寸计算使用 saturating 操作，保证正文不覆盖反馈、反馈不覆盖按钮。复用 `sanitize_terminal_text` 与 `profile_message_style`，不再复制另一套颜色/转义逻辑。

### Step 4 — 定向验证

```sh
cargo test --locked --test profile_reducer
cargo test --locked --lib ui::profiles::tests
```

预期 reducer + renderer 全过。本任务不是视觉重设计，不调整其他表单布局。

## Task 4：串联 Runtime 删除、磁盘重载和失败重试

**Files:**
- Modify/Test: `tests/profile_runtime.rs`
- Modify/Test: `tests/profile_lifecycle.rs`（复用 dispatch/next_action helpers）

### Step 1 — 连续删除真实 ProfileStore

构造 TempDir 配置，包含 SQLite、Redis、Postgres 元数据，均不实际 connect，不需要外部服务。沿用 FakeSecretStore。通过 App 的 UUID 定位删除 action → Runtime command → channel result → App reducer 循环删除多条，至少包含中间 Redis。

每次接收匹配成功事件后立即重建 ProfileStore 并 load：检查磁盘和 App 的剩余 id 集合一致、错误列表空、没有被删记录。删除全部后再重建 Runtime/读取空集合，证明重启不会复活。重复操作的等待使用既有有限 timeout，禁止无限循环等待。

### Step 2 — 保存失败后能够恢复

扩展 `delete_persistence_failure_restores_the_keyring_value` 或追加测试：制造 blocked parent 或损坏现有 TOML，执行删除，核对失败 action、凭据仍在、原文件字节不变。恢复测试内的有效文件/父路径后对同一 runtime 重试，必须成功（证明失败未提前移除 registry）；不要读取真实 keyring。

必要时在 lifecycle 用 App 接收失败并验证 Task 3 同一状态，随后重试成功。不要通过直接篡改 registry 冒充正常命令流。

### Step 3 — 定向闭环验收

```sh
cargo test --locked --test profile_runtime --test profile_lifecycle
```

预期无外部数据库依赖的新测试通过；保存成功后每一次重载都成功。保持已有 active disconnect、凭据回滚和 session profile 用例通过。

## Task 5：最终验证、审查与交付

### Step 1 — 单次全量 Rust 验证

与 `.github/workflows/ci.yml:81–83` 保持一致：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

预期全部退出 0。先检查 1.94.0 是否已安装；若仅能用当前 1.94.1，记录实际命令和 toolchain 差异，不标为精确 CI 版本验证。CI 的外部数据库 service job 与 macOS 发布依赖扫描不是本次改动的定向验收前提，但完整 CI 仍由流水线按原配置执行。

定向检查已过后不重复运行多轮 clippy+全量测试；只有代码或环境变化、失败纠正或未解决疑点才重跑对应检查。FMT 失败可执行格式化后重检。

### Step 2 — 审查检查项（Luna）

- 删除可用 Redis 不再保留；真实 unavailable 项不会丢。
- 混合来源的所有表位置在一个文档中连续；ArrayOfTables 子表跟随所属 id；测试验证了实际值而非仅文本包含。
- 最终文本验证在写入/rename 前；Runtime 失败不提交 registry；secret 回滚正常。
- 错误/警告在确认框显示；重试与取消可用；旧 request 不影响新操作。
- 真实用户配置未被探针或测试读写；没有自动删重复表的恢复逻辑。
- 仅提交本任务业务和测试文件，不把 `.git` 任务证据、用户本地变更纳入提交。

### Step 3 — 环境限制与交付

TestBackend 和临时文件闭环是本次必需验证。PTY/人工真实终端检查是补充，用户未强制要求；受环境限制最多一次有针对性修复重试，然后由 Luna 收尾审查决定补充证据或明确限制，不停留无限 progress。

在原任务目录 `validation.md` 追加每条实际命令、退出码、代码版本及工作树状态、环境/跳过项。不得把分析阶段探针或旧版本检查记为修复版本通过。

本闭环完成且审查通过后，后续提交阶段按工作流执行；建议提交主题 `fix(profiles): preserve valid config when deleting connections`。按实际文件精确 git add，禁止 `git add .` 吸入未知修改。分支命名、提交和合并均由 Luna 阶段进行。

## 完成标准与交接

全部必须成立：连续删除及重载通过；未知配置语义保留；损坏文本不覆盖正式文件；失败反馈可见且可重试；相关定向与全量 Rust 检查有当前版本记录；已有损坏文件需恢复的限制如实说明。

计划阶段只完成文档，不运行尚未添加的测试，不修改业务代码。下一实施动作：Luna 按工作流命名任务/分支并准备工作区，从 Task 1 的两个根因回归开始，持续完成整个单元。
