# Redis Value Editing Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 自动工作流执行说明：上行为 writing-plans 的标准交接声明；实际执行服从插件安排。若该技能不可用，按本文件逐项执行并记录验证结果即可，不尝试调用不存在的技能。用户已选择继续自动实施，不再询问执行方式，不启动子 Agent；分支/worktree/state.json 由插件管理。

**Goal:** 为 Redis Value 增加表格 e/a/dd 编辑及可逆文本的完整 Vim 编辑、格式校验、确认保存和未保存离开保护。

**Architecture:** 复用 EditorWorkspace 的 modalkit 内核，以会话语言和宿主能力隔离 SQL 与 Redis。Redis 草稿持有原始数据、完整性、编辑基线和精确请求所有权；表格执行原子定点 mutation，文本经可逆 codec 形成带原值检查的写入。统一的 Redis 离开协调器串行解决草稿，再重放既有关闭/断连/退出请求。

**Tech Stack:** Rust 1.94 / edition 2024、ratatui、crossterm、modalkit 0.0.25、redis 1.5.0、Tokio、serde_json、serde_yaml；复用现有依赖，Redis Lua 脚本和 Redis 7.4 集成测试。

---

## 0. 输入、约束、复核事实

- 输入分析：同目录 `analysis.md`；本计划已经先完整读取该文件。
- 原工作空间：`/Users/yelog/workspace/tui/lazydb`。
- 基线：`714eadc269b96b440950f5fe53d5942061665d5b`，目标 main；计划阶段工作树干净。
- 本文件是完整执行计划；计划阶段不修改业务代码、不运行实现测试、不创建分支/worktree。
- 实施开始前确认插件指定的工作空间和分支，保留用户已有修改。以下路径相对实施工作空间；行号仅指基线定位，修改后按符号定位。
- 真实文件名：Action 位于 `src/action.rs`，Command 位于 `src/commands.rs`，Overlay 位于 `src/model/workspace.rs`。
- `src/runtime.rs:5559` 已有 `active_database_for_target(identity, target)`，优先复用它；不是另建连接管理层。
- `src/model/session.rs` 的 SessionRegistry 支持 target/identity 查找；不要通过当前 active tab 推断提交目标。
- `.github/workflows/ci.yml:143–174` 已配置 Redis 7.4，环境变量为 `LAZYDB_TEST_REDIS_URL`，真实 mutation 测试采用 `#[ignore]` 并由 CI 显式运行。
- 当前测试/CI 以 Rust 1.94.0 为准。验证命令均在实施工作空间执行；后续新测试文件在对应任务创建后才可运行。

### 交付边界与不可违反的条件

1. Table：String 值编辑/确认删除；Hash field/value；List value 与有界 index 插入/删除；Set member；ZSet member/score；Stream 新增/删除和字段详情。
2. 文本：String Raw/JSON/YAML/Hex 可写；完整且预算内的 Hash/List/Set/ZSet 使用可逆结构文档编辑。部分预览不允许整体替换。
3. Stream 的已有 ID/entry 不伪装成可原地修改：e 显示实际字段，并允许显式“复制为新 entry”；旧 entry 保留。Java/PHP/Pickle 解码视图保持只读并提供 Raw/Hex 写原始 bytes 入口。Module/Unknown 明示不可写原因。这些例外必须出现在 UI、帮助和交付说明中。
4. e/a 提交即执行；dd 必须确认；文本 Ctrl+S 必须确认。写入过程中冻结该草稿/表单，不重复提交。
5. 写回不改变 Redis 类型、不隐式延长 TTL、不写入终端安全投影或字段数量，不从分页重建完整值。
6. 保存、放弃、取消均有可测试结果；网络错误、冲突和过期 key 不得清 dirty 或继续离开。
7. 复用现有 Vim/CellEditorBuffer 的能力，不复制整个 SQL reducer，不引入新的 Vim 库、语言服务器或 SQL 事务语义。

## 1. 工作分解与验证约定

顺序执行任务 1–15。任务 1–5 建立数据/后端契约，6–8 建立编辑与保存通路，9–11 完成表格和集合文本，12 完成退出保护，13–15 完成用户界面及验收。中间闭环不等于整体完成。

每个编号步骤是一次独立动作；同一步中的表驱动用例按测试函数逐个添加/运行，不用一次写完所有模块。涉及数据写入、生命周期或输入路由的任务先写最小失败用例，再实现再回归；帮助文案等低风险改动以现有 UI/帮助测试和人工复核验证。

各任务的“复核”须在进入下一项前完成。完成记录至少写明修改文件、实际命令、通过/失败/跳过原因；不要把计划中的预期结果当成执行结果。提交由后续执行阶段的工作流策略决定，本阶段不执行提交；文末提供适合拆分的提交边界。

## Task 1：定义编辑草稿、请求所有权和能力矩阵

**Files**
- Create: `src/model/redis_value_edit.rs`
- Modify: `src/model/mod.rs`, `src/model/redis_browser.rs`
- Test: `src/model/redis_value_edit.rs` 内联单元测试

**步骤**
1. 为 clean→dirty→undo clean、确认取消、保存失败保留 dirty、旧 request/revision 拒绝写状态添加失败测试。
2. 运行 `cargo +1.94.0 test --lib redis_value_edit`；预期新契约未实现时失败。
3. 定义 edit owner：tab_id、editor_id、ConnectionIdentity、RedisKeyId、preview_generation；save identity 再带 request_id 与 submitted_revision。不要用当前 tab index 存储所有权。
4. 定义能力枚举 EditableText/EditableTable/ReadOnly(reason)，从 Redis type、format encoding/view、完整性、profile read_only 推导，避免输入/UI/backend 各写一份规则。
5. 草稿保存 baseline bytes/typed snapshot、baseline_editor_text、current_revision、save phase、validation error；草稿随 tab 存在，但不自动序列化到 workspace 文件。
6. 将所有文本更改事件汇入单一 dirty 更新入口，仅在文本 revision 变化时比较编辑文本；移动光标和切换高亮不比较全文。
7. 同命令重跑；增加保存成功刷新失败仍 clean 的状态测试。

**可直接采用的纯函数契约与测试**（放在同模块，调用点仍使用现有类型）

```rust
fn content_is_dirty(saved: &str, current: &str) -> bool {
    saved != current
}

#[cfg(test)]
mod dirty_tests {
    use super::content_is_dirty;

    #[test]
    fn undo_to_baseline_is_clean() {
        let baseline = "{\"name\":\"缓存😀\"}\n";
        assert!(!content_is_dirty(baseline, baseline));
        assert!(content_is_dirty(baseline, "{\"name\":\"new\"}\n"));
        assert!(!content_is_dirty(baseline, baseline));
        assert!(content_is_dirty(baseline, baseline.trim_end()));
    }
}
```

此纯函数测试只验证 dirty 的字节语义；后续必须用真实 editor undo/redo 流证明调用正确，不能以此替代行为测试。

**复核/验收**：baseline bytes 与编辑展示基线分离；初次 pretty-print 不 dirty；read-only profile 不可变成可编辑会话；所有结果可关联到确定 owner。

## Task 2：建立可逆文本 codec 与 typed 表格绑定

**Files**
- Create: `src/value_preview/edit.rs`
- Modify: `src/value_preview/mod.rs`, `src/value_preview/table.rs`
- Test: `tests/redis_value_codec.rs`（新增）

**步骤**
1. 添加 round-trip 用例：空文本、尾部换行、中文/emoji、真实 byte 0x41 与文字 `\\x41`、NUL、无效 UTF-8、带 tab/newline 的集合成员。
2. 运行 `cargo +1.94.0 test --test redis_value_codec`，确认契约缺失导致失败。
3. 实现 String 文本/Hex 编解码：UTF-8 原文不转义；Hex 允许 ASCII whitespace，拒绝非 hex 和奇数位。binary Raw 不使用有歧义的 `\\xNN` 反解析，提供切换 Hex 后编辑的入口。
4. 定义集合文档 schema，JSON 与 YAML 使用同一数据模型。bytes 使用未标记字符串或单字段 `{hex: "00ff"}`，schema 显式禁止额外字段，避免普通文本误识别。
5. Hash 使用 `[{"field":"f","value":"v"}]`；List/Set 使用 `["a",{"hex":"00ff"}]`；ZSet 使用 `[{"member":"m","score":"1.5"}]`。集合不允许重复 hash field、set member、zset member，避免隐式折叠；List 允许重复。
6. Raw 集合进入编辑时显示规范 JSON 结构及“编辑格式 JSON”提示，JSON/YAML 按用户所选结构文档；集合 Hex 是派生展示，没有单一 Redis payload，提示切换结构编辑，不将其写成 string。
7. 分开 validation warning（String JSON/YAML 语法）与 encoding error（Hex/集合 schema/数值）：前者可继续保存原文，后者无合法 Redis 操作，禁止提交。
8. 在表格 binding 中加入 type-aware cell/row 定位，Stream Fields 从 `RedisPageValue::Stream` 取真实 pairs，Index/ID 标记只读；维持现有显示列顺序。
9. 重跑 codec 测试，再运行 `cargo +1.94.0 test --test value_preview --test redis_preview_serialization --test redis_values`。

**复核/验收**：展示函数 `page_text/format_page` 不作为保存 parser；显示 `\x` 不等于字节协议；合法 String JSON/YAML 保存原文而不是序列化器重排结果；解码 Java/PHP/Pickle 不生成可写文档；自动格式保存用实际格式而非 Raw 默认值。

## Task 3：增加完整、受限且一致的编辑快照读取

**Files**
- Modify: `src/db/redis/read.rs`, `src/db/mod.rs`, `src/commands.rs`, `src/action.rs`, `src/runtime.rs`
- Test: `tests/redis_value_snapshot.rs`（新增），`tests/redis_mutation.rs`

**步骤**
1. 写测试证明 64 KiB String/200 行预览不被标记为完整可替换，HSCAN 重复/分页结束也不能凭 UI 累积值构造一致快照。
2. 运行 `cargo +1.94.0 test --test redis_value_snapshot`。
3. 增加编辑快照读取操作，独立于正常预览分页。默认预算：source bytes 4 MiB、结构编辑集合 2,000 项、生成文本 8 MiB；常量集中声明并覆盖边界测试。预算是明确的首版策略，不是 Redis 协议限制。
4. String 在服务器检查 STRLEN 后取得完整 GET；集合以有界 Lua 顺序读取并累计 bytes/项数，超限立即返回不可编辑错误。检查类型、基线与内容的读取在同一脚本内完成，不把 SCAN 当一致快照。
5. Lua 获取单个巨大 member 仍有天然成本；脚本避免全表 HGETALL/LRANGE 后才检查大小，单步/批次读取有界，超过预算不进入写入阶段。
6. Action/Command 回传 owner/generation/request_id、typed snapshot 和完整标志。runtime 使用精确 ExecutionTarget（Redis database + schema None），并复用后台任务机制。
7. 开始编辑时若现有完整 String bytes 可作为 expected，则无需为它强行重新读取；部分 String/集合或需要结构性列表操作才显式加载完整编辑快照。加载中 UI 为 loading，不创建可修改的半份文档。
8. 在 `tests/redis_mutation.rs` 添加 ignored 真 Redis 快照用例，按文末 V-Redis 命令运行。

**复核/验收**：快照加载失败保留预览；不存在半份 Replace；切 key 后旧快照不会打开错误编辑器；大值可继续 Table 定点修改完整单元格，整体文本编辑清楚提示预算限制。

## Task 4：补齐 String/Hash/Set/ZSet 原子操作与明确冲突条件

**Files**
- Modify: `src/db/redis/mutation.rs`, `src/db/mod.rs`, `src/model/redis_object_editor.rs`
- Test: `tests/redis_mutation.rs`

**步骤**
1. 添加新失败测试：Absent 不覆盖现有 field/member、Hash rename 保留其他 field、Set replace 目标重复、ZSet rename 保留 score、String 删除时原值已变则拒绝。
2. 运行 `cargo +1.94.0 test --test redis_mutation`。
3. 为新 Value 操作引入明确 expected 条件（Any/Absent/Present/Value 或类型适合的等价枚举）。保留旧对象弹窗的既有语义，显式迁移调用点，不把旧 None 偷换成 Absent。
4. 增加 Hash field rename、Set member replace、ZSet member rename、String compare-and-delete；新增 form 默认目标必须不存在；无变化 edit 不发命令。
5. 新操作在 Lua 中先校验 type、旧值、目标冲突和数值，再一次执行变更；复用参数化 binary args，不能拼接用户 key/value 到脚本文本。
6. Set/ZSet 替换旧成员且其为最后成员时，处理删旧导致 key 短暂不存在的情况；保存 Persistent/Expires TTL 语义，禁止类型变化。
7. 修订脚本 TTL 保留为服务器侧取得/恢复同一期限（优先 Redis 7.4 的绝对到期时间语义），避免长脚本把原剩余 TTL 重新开始；永久 key 仍永久，删除 key 不重建。
8. 增加真实 Redis 测试断言服务器最终数据、无关行、PTTL/到期时间、冲突后内容不变；重跑普通测试及 V-Redis。

**复核/验收**：Lua 原子执行不等于事务回滚——任何可能抛错的参数校验必须在第一条写命令之前；命令预览与脚本实际语义一致；保持 runtime profile read_only 与 adapter target 二次校验。

## Task 5：实现有界 List、Stream 和完整集合 CAS

**Files**
- Modify: `src/db/redis/mutation.rs`, `src/db/redis/read.rs`
- Test: `tests/redis_mutation.rs`, `tests/redis_value_snapshot.rs`

**步骤**
1. 写列表 `["x","x","y"]` 删除 index=1 后应为 `["x","y"]` 的服务器测试；另写首/尾插入、超界、超预算、外部变化拒绝。
2. 添加 ListAppend、ListInsertAt 与有界 DeleteListAt。尾部追加只需必要长度/类型前置条件，不要求拉取整表；中间插入/删除使用 Task 3 完整有界基线，脚本先比较全部列表，再以循环 RPUSH 重建，禁止 LREM(value) 方案与无界 LRANGE。
3. List value e 延用绝对 index+expected；说明这种定点比较不能识别重复值移位后的历史身份。结构性变更使用完整列表比较，不用此弱身份替代。
4. 增加 Stream append 可选 ID（默认 `*`），DeleteStreamEntry 按完整 ID。对新增 ID/fields 在写前校验，不通过 DEL+XADD 修改已有 entry。删除后检查同 stream 的其他 entry 和消费者组仍在。
5. 对完整 Hash/List/Set/ZSet 文本新增 CompareReplace/CompareDelete：比较 typed baseline（Hash/Set/ZSet 忽略读取顺序），然后写新值。空集合对应删除 key；String 空文本对应空 String，不能混用。
6. 控制当前服务器数据的项数/bytes，再比较；超限或基线变化直接 conflict，不获取无界数据。大参数写入逐项/有界批次处理，不依赖大 `unpack(ARGV)`。
7. 新增实测：外部客户端修改后保存不覆盖；类型变化/过期失败；空集合删除；同分数不同表示按数值等价处理，拒绝新 NaN/无穷分数并明确既有值限制。
8. 运行 `cargo +1.94.0 test --test redis_mutation --test redis_value_snapshot`，再 V-Redis。

**复核/验收**：不调用现有 `Replace(Stream)` 处理 Value；List 超预算时 UI 可定点改 Value/追加，结构操作有明确原因；CAS 写前检查完成，失败无部分逻辑修改。

## Task 6：让现有 Vim 内核支持 Redis 宿主

**Files**
- Modify: `src/editor/mod.rs`, `src/model/editor_language.rs`, `src/editor/tests.rs`
- Test: `tests/editor_language.rs`, `tests/editor_projection.rs`

**步骤**
1. 新增编辑器测试：JSON 会话 `i/Esc/v/V/Ctrl+V`、`/`、yy/p/dd/u/Ctrl+R、Unicode 与尾部换行；断言真实文档内容与模式。
2. 运行 `cargo +1.94.0 test --lib editor::tests`，确认新 Redis 会话/能力测试失败。
3. 添加中性的可编辑会话入口与配置（language、capability、host）；`open_console` 作为 SQL 配置包装，`open_read_only` 保持只读路径。
4. 文本/render 使用已有 preview snapshot 的 Plain/JSON/YAML 高亮路径；修改 revision 时失效 highlights/wrap cache，移动光标不使缓存失效。
5. 按宿主允许 effects/Ex commands：Redis Save/Close/Yank/Changed/Focus 可用；Run/Commit/Rollback/SQL format 不可用。`:w` 请求当前 Redis 保存，`:q` 请求当前编辑对象离开并经过守卫，不直接设置 should_quit。
6. 核查 workspace 级 prompt/substitute 的 owner：切 tab 不得将前一个 session 的搜索/替换作用于后一个；按现有单 prompt 机制绑定 owner 或在焦点变化时取消，不扩展成第二套引擎。
7. 重跑 editor 单测及 `cargo +1.94.0 test --test editor_language --test editor_projection --test sql_completion`。

**复核/验收**：Vim 操作只维护一份实现；SQL 默认行为不变；Redis 不运行 SQL 分析/事务 effect；readonly 预览不能被新入口误升级。

## Task 7：建立 String 文本编辑路由、高亮和非 SQL 候选

**Files**
- Modify: `src/input/keymap.rs`, `src/action.rs`, `src/app.rs`, `src/ui/redis_browser.rs`, `src/ui/mod.rs`
- Create: `src/editor/value_completion.rs`
- Modify: `src/editor/mod.rs`（注册 value completion）
- Test: `tests/redis_value_edit.rs`（新增），keymap 内联测试

**步骤**
1. 写路由失败用例：Preview 文本 i 输入，Normal f/W 是 Vim，Keys `/` 仍搜索 key、Preview `/` 搜索文本，Insert `?` 是字符；Ctrl+S 三类模式均产生 Redis 保存请求。
2. 运行 `cargo +1.94.0 test --test redis_value_edit`。
3. Ready 的可逆文本创建/恢复对应 editable session，初始 Normal；Table/只读投影不创建可编辑文档。`ensure_read_only_session` 不得覆盖该 session。
4. 在 overlay、search prompt、completion 的优先级之后，将文本按键送该 owner 的 editor；Space leader 保留格式、wrap、load-next。删除旧 f/W 等对可编辑 Normal 文本的抢占。
5. 扩展 `map_paste`（keymap.rs:2929–3010）和 App Paste 分发，支持 Redis editor/表单的 bracketed paste，SQL EditorPaste 路由保持原义。
6. Changed effect 更新 Task 1 草稿；Yank 沿用共享寄存器/clipboard。Redis effect 分发不调用 SQL 默认 completion schedule。
7. JSON 候选提供 true/false/null 和当前文档字段；YAML 提供字段和基础标量；Plain/Hex 不给 SQL 候选。仅 Insert 触发，Tab/Enter 接受、Esc 关闭，搜索输入不会触发。
8. 候选与错误位置按字符/字节接口显式转换，避免 emoji 后替换错误；候选生成只按文档 revision 缓存，限制条目数量。
9. 运行 `cargo +1.94.0 test --test redis_value_edit --test sql_completion` 与 `cargo +1.94.0 test --lib input::keymap::tests`。

**复核/验收**：所有用户指定 Vim 键和终端粘贴均有行为断言；没有 SQL 关键词/对象补全；cursor/selection/mode 在 Redis panel 正确渲染，非编辑 UI 保留导航功能。

## Task 8：接通确认保存、异步所有权与精确刷新

**Files**
- Modify: `src/action.rs`, `src/commands.rs`, `src/app.rs`, `src/runtime.rs`, `src/model/workspace.rs`
- Create: `src/ui/redis_value_edit.rs`
- Modify: `src/ui/mod.rs`, `src/model/redis_value_edit.rs`
- Test: `tests/redis_value_edit.rs`, `tests/redis_value_runtime.rs`（新增）

**步骤**
1. 添加失败测试：dirty Ctrl+S 只开确认、取消不写、继续才 Execute、clean 不写、失败保留编辑、保存期间重复 Ctrl+S 只一条请求。
2. 运行 `cargo +1.94.0 test --test redis_value_edit --test redis_value_runtime`。
3. 增加 SaveConfirm overlay：展示 `Redis <type>: <lossless-key>`、目标 DB、格式与必要变更摘要；JSON/YAML 语法错误同框提供“仍然保存/返回编辑”，不重复弹两次确认。
4. 将 draft→codec→plan→execute 连接起来；请求只用捕获的 owner/key/baseline，默认 Preserve TTL。保存时冻结此草稿；切其他 tab 保留其状态。
5. mutation plan ready/success/failure 按 owner 路由，兼容现有 RedisObjectEditor；不再仅依赖当前 overlay。保存状态可在 overlay 临时不显示时继续接收正确结果。
6. runtime 以 RedisTarget 转 ExecutionTarget，使用 `active_database_for_target` 查 connection identity；非当前 tab 也能保存，连接不在则失败并保留草稿，不偷偷重连后覆盖旧基线。
7. 成功只更新匹配请求与提交 revision 的 baseline；连接 generation 或 key/owner 不匹配则拒绝旧回执。取消已提交 mutation 不能当作服务器回滚。
8. 替换“只 select key”刷新：显式新 generation 读取对应 owner 的 key；保留用户格式/焦点/列宽/选择标识。清除派生缓存；其他打开同 key 的 clean tab 可刷新，dirty tab 保持草稿并标记远端已变。
9. 将 write success 与 refresh failure 分成状态/提示。成功后不因刷新失败恢复 dirty 或重复写；删除 key 清空 Value、移除对应树 key 并选邻行。
10. 新读取仅在 capture revision/generation 仍匹配且没有新 dirty 内容时替换 buffer。成功后用户开始下一次编辑，则迟到刷新不能覆盖它。
11. 重跑该任务测试及 `cargo +1.94.0 test --test redis_object_editor --test redis_loading_lifecycle --test redis_browser_tabs`。

**复核/验收**：String Raw/JSON/YAML/Hex 端到端保存工作；后台非当前连接保存不会落到另一 DB；写入已成功但未刷新被准确呈现；应用退出协调器尚未接入前，不把此中间状态作为可发布完成态。

## Task 9：完成 Hash Table 的 e/a/dd 闭环

**Files**
- Modify: `src/model/redis_value_edit.rs`, `src/model/workspace.rs`, `src/input/keymap.rs`, `src/action.rs`, `src/app.rs`, `src/ui/redis_value_edit.rs`, `src/ui/mod.rs`
- Reuse: `src/model/cell_editor.rs`, `src/ui/data_grid.rs` 中现有输入/选中样式（仅必要公共呈现拆分，不搬 SQL reducer）
- Test: `tests/redis_table_edit.rs`（新增），keymap 内联测试

**步骤**
1. 添加 e 预填真实 Value、a 按 Field/Value 显示、dd 首次 d 等待/第二次确认、Esc 取消的路由与 reducer 用例。
2. 运行 `cargo +1.94.0 test --test redis_table_edit`。
3. 增加 RedisCellEdit/RedisRowAdd 表单 model，复用 CellEditorBuffer 文本编辑能力，明确 form owner 与原始 row identity；不要复用 RelationEditCell/RelationInsertRow Action。
4. Hash Value e 构造 SetHashField(expected old value)；Field e 构造 RenameHashField，目标冲突不覆盖。
5. a 用同列顺序表单，Tab/Shift+Tab 切字段，Enter 提交、Esc 取消；空 field/value 按 Redis bytes 语义允许；binary 字段用明确 Text/Hex 模式。
6. dd 新增独立 Redis table pending sequence，超时/焦点/tab/mode 变化清除；确认标题使用用户给定形式，正文含 field。确认前不发 mutation。
7. 复用 Task 8 提交通道，busy/error 保留在表单；成功按 field row_key 恢复选中，新增选新 field、删除选邻行，若 row 不在当前加载窗口则重载/定位而非选错误行。
8. 测试 partial HSCAN 页面编辑仅更新该 field，不替换整 hash；删除最后 field 后 key 消失。
9. 运行该任务测试、`cargo +1.94.0 test --test redis_key_delete --test redis_key_tree --test redis_object_editor` 和 keymap 测试。

**复核/验收**：e 的弹框交互与 relation data 一致；a 显示全部列；dd 显示 type/key/field 并确认；Keys 删除未被新逻辑抢占。

## Task 10：扩展 Table 至其他类型并固定例外行为

**Files**
- Modify: `src/model/redis_value_edit.rs`, `src/value_preview/table.rs`, `src/app.rs`, `src/ui/redis_value_edit.rs`
- Test: `tests/redis_table_edit.rs`, `tests/redis_mutation.rs`

**步骤**
1. 添加表驱动 type×column×operation 测试，覆盖 String/List/Set/ZSet/Stream，逐类型运行失败用例后实现。
2. String e 使用单 Value 表单，a 提示单值语义并进入相同编辑；dd 明确“删除整个 string key”，使用 compare-delete，不沿用无 expected 的普通 key 删除。
3. List Index e 提示派生不可写，Value e 定点修改；a 显示 Index/Value，Index 默认 append，可明确选择合法插入位置；dd 走 Task 5 基线快照和预算校验。
4. Set e 原子替换成员，a 重复成员提示已存在、不产生假成功；dd 按 source bytes 删除。
5. ZSet e 根据列选择 rename member/update score；a Member/Score；分数先解析校验，不能在脚本先删旧 member 后才发现 score 无效。
6. Stream a 显示 ID/Fields（实际可重复 field/value 表单，不是数量）；e 只读 ID/真实 fields，提供显式复制为新 entry；dd 确认后 XDEL，不 DEL stream。
7. 按类型恢复刷新后的选择：hash/member stable bytes、List 相邻绝对 index、Stream 返回的新 ID；不能仅保留旧 row number。
8. 对 readonly profile、缺失 key、loading、unsupported 类型与空表添加明确不可用状态。
9. 运行 `cargo +1.94.0 test --test redis_table_edit --test redis_values --test redis_mutation` 与 V-Redis。

**复核/验收**：所有表格列有明确可写/只读语义；binary member 无损；Stream 不更换已有 ID；显示 Fields 数量绝不会写回真实 fields。

## Task 11：接入集合文本编辑、格式切换与完整性约束

**Files**
- Modify: `src/app.rs`, `src/model/redis_value_edit.rs`, `src/ui/redis_browser.rs`, `src/ui/redis_value_edit.rs`, `src/value_preview/edit.rs`
- Test: `tests/redis_value_edit.rs`, `tests/redis_value_codec.rs`, `tests/redis_value_snapshot.rs`

**步骤**
1. 添加 201 项集合预览不能直接 Replace、加载完整编辑快照后允许编辑、超预算保留只读、外部更新 CAS conflict 的失败测试。
2. 运行 `cargo +1.94.0 test --test redis_value_edit --test redis_value_snapshot`。
3. 非 Table 进入集合可编辑模式时使用 Task 3 一致快照和 Task 2 规范文档；UI 显示实际编辑格式/schema；不要在旧 tab/newline 展示上直接开放 i。
4. JSON/YAML 格式切换必须基于已保存 typed value 重建编辑文档，dirty 时交给 Task 12 守卫。Auto 固定此次文档的实际格式，编辑中不反复自动探测。
5. 集合结构错误显示“无法转换为 Redis <type>”并留在编辑；String 错误则仍允许原文保存。空集合提交确认明确 key 将被删除。
6. 保存调用 CompareReplace/CompareDelete；成功后更新 text baseline 和 snapshot，失败保留草稿。
7. decoded serialization/Stream/collection Hex 页面显示不可逆原因和可用编辑入口；切换到支持路径不改变原始 bytes，未修改退出仍 clean。
8. 重跑 codec/edit/snapshot 与 `cargo +1.94.0 test --test redis_preview_serialization --test value_preview`。

**复核/验收**：集合文本真正可写且可逆；分页与展示不参与 Replace；未经修改的 pretty-print 不写 Redis；非法格式可继续保存的范围和限制准确。

## Task 12：统一未保存离开保护与多草稿协调

**Files**
- Create: `src/model/redis_edit_leave.rs`
- Modify: `src/model/mod.rs`, `src/app.rs`, `src/action.rs`, `src/model/workspace.rs`, `src/ui/redis_value_edit.rs`, `src/input/keymap.rs`
- Test: `tests/redis_unsaved_changes.rs`（新增），`tests/global_workspace.rs`, `tests/workspace_tabs.rs`, `tests/connection_switch.rs`

**步骤**
1. 先写 OpenKey、CloseTab、Disconnect、Quit 四入口×保存/放弃/取消/失败的 reducer 用例；断言确认前 opened_key、tab 集合、connection 和 should_quit 不变。
2. 运行 `cargo +1.94.0 test --test redis_unsaved_changes`。
3. 建立 leave intent：OpenKey、CloseTab、CloseOtherTabs、DisconnectProfile、SwitchConnection（确实替换会话时）、ChangeFormat、RefreshValue、LoadNext、Quit、Restart；参数捕获稳定 ID，不存 tab index。
4. 在破坏性 reducer 入口修改状态之前运行同一守卫：定位 `open_redis_key`、`request_close_tab`、`request_profile_disconnect`、Action::Quit、格式接受/刷新/分页与重启路径；包含 mouse/help/ex 发起的入口。
5. overlay 提供 Save/Discard/Cancel，默认非破坏性选项；Save 进入 Task 8 校验/确认及请求链，保存确认取消等于取消离开；Discard 丢弃该草稿后继续；Cancel 留在原页面。
6. coordinator 收集当前 tab 优先的受影响 dirty owner 队列。Quit 检查所有已打开/缓存工作空间内实际持有的草稿；Disconnect 检查同 profile 所有 database，CloseTab 仅本 tab。
7. 保存成功才处理下一 draft，失败/冲突停止离开；任一成功之前不得发 disconnect/quit。重复请求合并或在解决中忽略，不能以新 overlay 覆盖 pending intent。
8. 解决完队列后重放 guarded 请求入口，继续 SQL/关系表事务检查及 workspace flush；不要直接调用 Command::Disconnect/Command::Quit 绕过它们。
9. 普通 tab 切换保留 session/draft；只在 Keys 移动选择不弹窗，实际打开另一 key 才提示。格式切换与 load-next 不覆盖 dirty，旧异步 preview 也不能覆盖。
10. 未提交的行表单在离开时也保留输入/提示；保存中不允许销毁所属 tab/connection，其他 tab 可浏览。正常关闭事件经统一 Quit；不承诺强杀可弹窗。
11. 添加非当前 DB 保存、两个 profile、多个 dirty drafts、保存失败再重试、SQL 事务与 Redis 草稿同时存在的回归测试。
12. 运行 `cargo +1.94.0 test --test redis_unsaved_changes --test global_workspace --test workspace_tabs --test connection_switch --test workspace_persistence`。

**复核/验收**：四个用户指定离开路径均完整；任何失败/取消不丢内容；关闭其他 tab 不递归弹多个相互覆盖的 overlay；已成功保存的草稿不会重复询问；state.json 与插件状态不在业务范围内。

## Task 13：界面状态、帮助与文档闭环

**Files**
- Modify: `src/ui/redis_browser.rs`, `src/ui/redis_value_edit.rs`, `src/ui/mod.rs`, `src/help.rs`, `docs/redis-browser.md`, `docs/redis-value-preview.md`, `docs/testing/redis-browser-performance.md`
- Test: `tests/redis_help.rs`, `tests/redis_table_edit.rs`, `tests/redis_value_edit.rs`

**步骤**
1. 在 Value 标题/状态栏显示 dirty `*`、Normal/Insert/Visual、Saving/Error，避免布局覆盖原 key/type/TTL；通过现有 ratatui TestBackend 验证窄窗口和长 key 截断。
2. 确认框 key 显示用现有 lossless/terminal-safe projection，真正提交仍使用 source bytes；错误消息也经过既有 sanitization。
3. contextual help 区分 Keys、Table、editable text、read-only decoded view、form；列出 e/a/dd/Ctrl+S、leader 格式/wrap、完整 Vim 模式及 `/`。
4. 文档说明立即写入 vs 文本草稿、格式错误继续保存、空集合删除、预算、Stream/序列化只读投影与 Raw/Hex 替代入口、冲突处理与 TTL。
5. 更新性能文档：完整编辑快照是本次新增独立路径，不再被误写为尚未实现的预览全量加载；普通预览分页仍受原限制。
6. 运行 `cargo +1.94.0 test --test redis_help --test redis_table_edit --test redis_value_edit`。

**复核/验收**：用户无需看开发文档即可知道可编辑范围、保存结果与未保存状态；帮助快捷键与实际路由一致；文案不称“所有类型均支持原地编辑”。

## Task 14：真实 Redis、异步故障与性能回归

**Files**
- Modify: `tests/redis_mutation.rs`, `tests/redis_value_runtime.rs`, `tests/redis_unsaved_changes.rs`, `src/editor/preview_perf_tests.rs`, `.github/workflows/ci.yml`（仅当新增 ignored 测试未被既有命令覆盖）
- Evidence: 后续阶段产物中的测试记录，不修改插件 state.json

**步骤**
1. 将真实 EVAL/快照集成用例放入既有 `redis_mutation` test target，从而被现有 CI Redis 命令执行；若放其他 target，明确扩展 CI，不让 ignored 新测试永不运行。
2. 每个真实测试使用唯一 key 前缀、清理自己创建的 key/消费者组，不 FLUSHDB 用户实例；沿用环境变量提供的隔离服务。
3. 覆盖 String/Hash/List/Set/ZSet/Stream 成功、TTL、类型变化、同类型外部改值、目标重名、过期、大值预算、binary data、列表重复值、stream 消费者组仍存在。
4. runtime 用确定性事件顺序测试：plan A 后切 tab B、旧 success、旧 preview、generation 变更、保存成功刷新失败、同 key 两个 dirty/clean tab；不靠 sleep 碰运气复现。
5. 运行 V-Redis 和 V-Targeted；记录每项通过/失败，不把 ignored 数量当执行数量。
6. 运行 `cargo +1.94.0 test --lib preview_perf_tests -- --list` 确认基准名称，再按该模块已有 ignored 标记执行 `cargo +1.94.0 test --release --lib preview_perf_tests -- --include-ignored --nocapture`。
7. 与基线已有性能记录比较相同数据和窗口下的导航行为，关注每次移动是否复制全文/重新解码、dirty 比较是否仅发生在文本变化；若测量波动先检查同环境可比性，不凭一次耗时断言回归。

**复核/验收**：真实脚本被运行；验证是服务器内容/到期语义，不仅生成命令；异步结果不会写错目标；只读导航缓存仍有效。无需为本任务重跑百万 key 索引压力测试，除非实际改到索引路径。

## Task 15：最终验证、人工验收与交付核对

**Files**：复核前述所有改动文件，尤其 `src/app.rs`、`src/input/keymap.rs`、`src/db/redis/mutation.rs` 的跨域影响。

**步骤**
1. 运行 V-Full，修复本次引入的失败；若发现基线已有失败，提供可复现证据并单独记录，不直接降低校验要求。
2. 用隔离 Redis 运行 `cargo +1.94.0 run --locked --` 启动 TUI，通过现有连接 UI 配置测试连接；不要臆造不存在的 CLI connection 参数。
3. 逐项执行下方人工矩阵，使用独立 Redis 客户端读取实际结果；对正常关闭、保存失败、格式错误各至少走一次真实交互。
4. 检查 `git diff --check`、`git diff --stat`、`git status --short`，没有临时 Redis 数据、凭证、生成文件和无关格式化改动。
5. 按“需求追踪表”逐项标注完成/限制/验证证据；没有全部实现和验证前不宣称整体完成。

**人工验收矩阵**

| 场景 | 操作 | 必须观察到 |
| --- | --- | --- |
| Hash table | Value e 修改、a 新增、dd 删除 | 表单正确、删除含 type/key/field 确认、成功后当前表格刷新 |
| binary member | Set/ZSet e 与 a | 实际 bytes 无损，目标重名不会静默吞并 |
| List 重复值 | 删除第二个重复值、插入首尾 | index 正确、不误删其他重复值、超预算提示 |
| Stream | Fields e、复制新增、dd | 展示实际 fields，旧 ID 未伪修改，XDEL 不 DEL stream |
| Raw/JSON/YAML | i 编辑→Esc→yy/p/dd/u/Ctrl+R、visual、搜索 | 内容/模式/高亮/候选正确，undo 回原文 clean |
| 无效格式 | 删 JSON 括号、Ctrl+S | 有错误原因/位置，返回继续编辑，确认继续则原文写入 String |
| 保存确认 | dirty Ctrl+S→取消/确认 | 取消不写，确认后写入并恢复 clean，TTL 不重置 |
| 未保存离开 | 切 key、关 tab、断连、退出 | 保存/放弃/取消均正确；失败不继续离开 |
| 多目标 | DB0/DB1 同名 key 与非当前 tab dirty | 保存各落到原目标；断连前串行解决所有草稿 |
| 故障 | 保存失败、外部修改、刷新失败 | 草稿不丢；冲突不覆盖；写成功刷新失败不重复提交 |
| SQL 回归 | SQL editor 编辑/补全/执行、relation e、只读 DDL | 原有功能与退出事务保护正常 |

## 2. 验证命令清单

### V-Targeted：业务回归（任务 12 后）

```bash
cargo +1.94.0 test --locked --test redis_value_codec --test redis_value_snapshot --test redis_value_edit --test redis_table_edit --test redis_value_runtime --test redis_unsaved_changes
cargo +1.94.0 test --locked --test redis_mutation --test redis_object_editor --test redis_browser_tabs --test redis_values --test redis_preview_serialization --test redis_loading_lifecycle --test redis_key_delete --test redis_help
cargo +1.94.0 test --locked --lib editor::tests
cargo +1.94.0 test --locked --lib input::keymap::tests
cargo +1.94.0 test --locked --test sql_completion --test editor_language --test editor_projection --test relation_tabs --test relation_runtime --test connection_switch --test global_workspace --test workspace_tabs --test workspace_persistence
```

预期：全部选中测试通过；依赖 Redis 的 ignored 用例不在此处声称通过，由 V-Redis 验证。

### V-Redis：真实 Redis 脚本与快照

优先复用 CI/环境提供的 `LAZYDB_TEST_REDIS_URL`。仅在本地没有隔离服务且 Docker 可用时，实施者可按以下单条命令启动临时容器（不在 plan 阶段执行）：

```bash
docker run --rm --detach --name lazydb-value-edit-tests --publish 127.0.0.1:16379:6379 redis:7.4
docker exec lazydb-value-edit-tests redis-cli ping
LAZYDB_TEST_REDIS_URL=redis://127.0.0.1:16379/0 cargo +1.94.0 test --locked --test redis_mutation --test redis_contract -- --ignored --nocapture --test-threads=1
docker stop lazydb-value-edit-tests
```

预期：PING 返回 PONG，真实 ignored 测试全部执行并通过；容器停止并清理。命令按顺序分别执行，测试结束即清理自己启动的容器。端口/名称已被占用则选择新名称/端口并记录，不能停止未知容器。若已有隔离 URL，直接使用环境变量运行 cargo 命令即可，不创建额外服务。

不能启动服务时明确记录环境阻塞，仍完成不依赖 Redis 的验证；不将未运行的原子脚本测试标为通过，也不写“完整验证通过”。

### V-Full：与 Rust CI 对齐的最终门禁

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
git diff --check
```

预期：全部 exit 0，clippy 无 warning；默认 ignored 的外部服务测试执行情况另列。不改变 Cargo.toml 的 Rust/依赖版本来规避失败；如需修正格式先运行 fmt，再重复受影响的门禁即可。

## 3. 需求追踪与最终验收标准

| 用户需求 | 任务 | 最终验收 |
| --- | --- | --- |
| Table e 弹框编辑 | 2/4/5/9/10 | 所有支持类型的可写列正确预填与提交，派生列和 Stream 例外明确 |
| Table dd + 确认标题 | 4/5/9/10/13 | 第二次 d 才开确认，type/key/行身份准确，取消不写，确认后刷新 |
| Table a 同列表单 | 4/5/9/10 | 列顺序一致、必填/派生字段语义准确、写入成功后刷新并定位 |
| SQL Editor 一致 Vim | 6/7 | Insert/Normal/Visual、搜索、yy/p/dd/u/Ctrl+R、粘贴、Unicode 均通过 |
| 差异化高亮和候选 | 6/7/13 | JSON/YAML/Plain 正确，没有 SQL diagnostics/候选/执行效果 |
| Ctrl+S 保存确认 | 8/11 | clean 无写入、dirty 有确认、忙碌去重、失败保留、成功匹配 owner |
| JSON/YAML 错误允许继续 | 2/8/11 | String 以原文继续保存；不可编码集合错误明确阻止且不损坏数据 |
| 切 key 未保存提示 | 12 | 变更 opened key 前守卫；保存/放弃/取消/失败全部正确 |
| 关 tab/断连/退出提示 | 12 | 所有入口和多草稿覆盖，既有 SQL 事务退出检查不被跳过 |
| 不丢未加载数据 | 3/4/5/11 | Table 定点修改；文本仅完整一致有界快照 CAS；冲突拒绝 |
| 异步刷新与连接一致性 | 8/12/14 | 原 DB/key/owner 精确匹配，旧事件无法清新草稿，刷新失败不重复写 |

必须同时满足：业务矩阵实现完整；V-Targeted/V-Full 通过；V-Redis 有实际执行证据或明确环境阻塞；文档如实列出 Stream/序列化等边界。任何功能缺项需要在执行结果中显式列出，不能用“包括但不限于”隐去例外。

## 4. 建议提交边界（仅供实施阶段）

不要求每个测试用例单独提交，也不提交无法编译的中间代码。按工作流允许的提交策略使用以下逻辑边界，每次只 stage 已复核文件：

1. `feat(redis): model value edit drafts and reversible codecs`（Task 1–2）
2. `feat(redis): add bounded edit snapshots and atomic value mutations`（Task 3–5）
3. `feat(editor): support language-aware Redis value sessions`（Task 6–7）
4. `feat(redis): save value drafts with owner-scoped refresh`（Task 8）
5. `feat(redis): edit add and delete value table rows`（Task 9–10）
6. `feat(redis): edit structured values and guard unsaved changes`（Task 11–12）
7. `test(redis): cover value editing lifecycle and document controls`（Task 13–15）

## 5. 计划阶段完成情况

- 已完整读取分析，调用 writing-plans，并补充核对真实 Action/Command/Overlay、连接目标查找及 CI Redis 测试入口。
- 已形成包含文件、逐项步骤、复核、命令和验收标准的完整计划。
- 本阶段只写本任务目录的计划与回执；未实施业务代码，未运行实现验证，未修改 state.json，未启动子 Agent。
- 后续由插件安排实施、Luna 命名任务和分支；不再询问执行方式。
