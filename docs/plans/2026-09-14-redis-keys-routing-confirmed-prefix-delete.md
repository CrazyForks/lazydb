# Redis Keys Routing and Confirmed Prefix Delete Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 若执行环境没有该技能，按本文顺序实施，每项完成后复核代码、行为和实际测试结果。接口名为拟定名称，可按现有代码风格调整。

**Goal:** 修复 Redis Keys 的 o/y 路由及文件夹颜色，提供单 key 和整个前缀分组的删除确认，并确保显示数量、实际删除目标和异步回调一致。

**Architecture:** Redis 输入拥有明确上下文，普通树与过滤树共享展示行契约。删除采用 Prepare → Confirm → Execute：单 key 直接准备，分组独立 SCAN、按原始字节去重并冻结清单，确认后按有界批次执行；App 管理弹窗与请求身份，Runtime 管理连接、任务和临时清单。

**Tech Stack:** Rust 1.94 / Edition 2024、Ratatui、Crossterm、redis 1.5.0、Tokio、现有 Action/App/Command/Runtime、KeyBindings、IconSet、TestBackend、Redis 临时索引设施。

---

## 1. 基线与执行规则

- 分析对象是主工作空间 `/Users/yelog/workspace/tui/lazydb`，已合并上一轮功能提交 `99d1a32` 和合并提交 `8f5c2ee`。
- 上一轮计划及“完成”报告不能代替当前代码验证。尤其不可为了让测试通过，将 `user + user:1` 用例替换成没有身份冲突的 `user:1 + user:2`。
- 若进入实施阶段创建 worktree，遵循用户规则：分支 `task/redis-keys-confirmed-prefix-delete`，目录 `../lazydb-redis-keys-confirmed-prefix-delete`。从实施时主分支 HEAD 创建，复制本计划；保留主空间其他计划文件。
- 当前请求仅创建计划，不创建执行 worktree、不改生产代码、不自动提交或合并。
- 每项依次执行：补行为回归 → 复现缺陷 → 最小实现 → 定向测试 → 复核差异。提交检查点需在用户授权提交后执行，使用明确文件列表。
- 以实际符号定位，不依赖固定行号；主空间可能有其他任务并发推进。

### 已确认的根因

| 入口 | 事实 | 修复方向 |
| --- | --- | --- |
| `src/input/keymap.rs::map_configured_navigation` | Results 且非 Relation 被当成通用数据表；o 返回 ToggleResultView，y 返回 CopyGridCell | 显式按 Tab/View 判定，Redis 不进入 SQL 操作 |
| Redis keymap 分支 | 先调用通用配置，部分 GridMove 转译，其余 other 原样放行 | 单一 Redis 上下文路由，移除不适用 Action 回退 |
| `insert_parts` / `insert_node_parts` | 先存在 Key 再增加后代时未提升为 Prefix | 构建结果与输入顺序无关 |
| `RedisBrowserTab::toggle_prefix` | 只操作普通 expanded，过滤绘制使用缓存 filtered_rows | 当前模式展开状态与投影同步 |
| `src/ui/redis_browser.rs::render_row` | 图标前景为 text/accent | 分组使用与 Explorer 相同的 muted，标签单独高亮 |
| `src/model/workspace.rs::visible_rows` | Explorer Group 的 kind 为 None | Tables/Views 分组颜色不是 Table 对象颜色 |
| `delete_redis_key` | 无确认直接派发 DEL；Prefix 被拒绝 | 引入 Redis 删除目标、准备和确认状态 |
| 删除回调 | 仅 tab_id/key，缺少独立请求身份；选择回退和过滤保留不完整 | 请求 ID、连接、target 校验及批次协调 |

## 2. 最终行为契约

### 2.1 输入与树

- Keys：o/Enter 切换文件夹展开；真实 key 显式读取预览。
- Keys：h/Left 折叠已展开目录，否则到父节点；l/Right 展开收起目录，否则进入首个展示子节点；叶子 l 不重复读取预览。
- Keys：y 复制完整 key；d 打开删除确认/准备流程，首次按 d 不产生 DEL。
- Filter Editing：o/y/d/h/l 都输入文本；修饰键和粘贴走 TextInput 逻辑。
- Preview 不接收 Keys 的删除、复制等默认操作；焦点切换沿用窗口快捷键。
- 同时存在 `user`、`user:1` 时，树为 Prefix(user:) 下的 Key(user) 和 Key(user:1)。两种输入顺序和不同 SCAN 批次边界结果一致。
- “树中父子展示关系”不等于“Redis 字节前缀”：删除 Prefix(user:) 只匹配以 `user:` 开头的 key，不包含 Key(user)。
- 普通/过滤模式的显示行、上下移动、父子导航、鼠标、viewport 和 scrollbar 都使用当前投影。

### 2.2 图标

- 文件夹 glyph 继续由 IconSet::group 提供。
- 文件夹图标颜色与真实 Explorer Tables/Views 分组一致：theme.muted；选中时保留图标前景色，只改变背景为 theme.selection。
- 标签使用 text/accent，叶子仍使用钥匙图标；NerdFont/Unicode/ASCII 都可用。

### 2.3 删除范围

- Key：保存完整 RedisKeyId，准备数量为 1；确认后执行一次单 key 删除。
- Prefix：使用 KeyTreeNodeId::Prefix 内的原始字节，包括结尾冒号，不根据 label 拼接。
- 前缀扫描范围是目标 DB 服务端，不限已加载的浏览器 key，也不受当前过滤查询限制。
- SCAN 不是一致性快照：文案说明“本次扫描发现 N 个，确认后删除这些 key”。执行冻结清单，确认后新增且不在清单里的 key 不纳入本次操作。
- Redis 没有通用 key 实例版本身份：清单中的同名 key 若被外部删除并重建，按确认的名称删除；文档不能承诺删除的是扫描时的 value 实例。
- SCAN 去重后才计数。仅当游标回到 0 且清单完整时开放确认；扫描失败/取消/客户端清单限额触发时不能把部分清单标记成“全部”。
- 前缀里的 `* ? [ ] \\` 按字面字节处理：构造转义 MATCH，并在客户端以 starts_with 再验证。

### 2.4 确认与执行界面

参考连接删除的 `ProfileManagerMode::ConfirmDelete`、`ProfileDeleteFocus` 及 `src/ui/profiles.rs` 的布局，复用视觉和按钮机制，使用独立 Redis 状态。

```text
DELETE REDIS GROUP
Connection: development    DB: 0

即将删除以 order:history: 开头的 key。
本次扫描发现 1,280 个 key，是否确认删除？
确认后仅删除本次扫描清单中的 key。

                 [取消]  [删除 1,280 个 key]
```

- 默认取消；Left/Right、h/l、Tab/BackTab 切换；Enter 执行当前按钮；Esc 取消。
- Preparing 显示累计发现数量，只有取消可用；Prepared 默认聚焦取消。
- Executing 显示已处理/总数、实际删除/已不存在；确认键和重复 d 不重复派发。
- Executing 取消表示停止后续批次，不回滚已经执行的 DEL；正在执行的一批等待结果，不能声称服务端命令已取消。
- 网络断开导致某批结果不明时显示“结果未知”，停止后续批次，不自动重新执行该批；刷新重新核实。
- 支持鼠标命中、窄窗口、长前缀和安全字节展示。

## 3. 数据流与建议状态

建议新增 `src/model/redis_delete.rs`，包含独立的 RedisDeleteTarget、RedisDeleteIdentity、RedisDeletePhase、RedisDeleteState。

请求身份字段：独立 operation_id、owner_tab_id、ConnectionIdentity、RedisTarget。单 key/Prefix 的原始字节作为不可变目标，不能确认时重新读取当前选中行。

阶段：Preparing、Prepared、Executing、Completed、Failed、Cancelled。Prepared 携带 opaque plan_id 和 total_count，App 不持有整份大清单。

Runtime 拥有 plan store，记录身份、冻结目标、去重字节 key、分页位置、取消 token 和批次序号。清单只在 Runtime 可解析；不存在/失效 plan_id 不能执行。

```text
d → RedisDeleteRequest → App 建立身份和弹窗
  Key    → Prepared(1)
  Prefix → PrepareRedisDelete → 独立 SCAN → Prepared(plan_id, count)

确认 → ExecuteRedisDelete(plan_id, identity)
     → 分批 DEL → RedisDeleteBatchCompleted(identity, batch_id, ...)
     → RedisDeleteCompleted

取消/失效 → CancelRedisDelete → 停止后续工作、释放清单
```

## 4. 任务列表

### Task 0：重新建立真实基线

**Files:** 读取 `src/input/keymap.rs`、`src/model/redis_key_tree.rs`、`src/app.rs`、`src/ui/redis_browser.rs`；Test `tests/keymap.rs`、`tests/redis_key_tree.rs`、`tests/redis_key_delete.rs`。

1. 检查 status、目标 diff、分支和工具链，记录 HEAD。
2. 执行定向基线：

```bash
cargo +1.94.0 test --test keymap --test redis_key_tree --test redis_key_delete --test redis_browser_tabs
```

3. 记录当前结果，不将现有测试通过解释为 o/y 已可用。
4. 为 o/y 新增 map→update 回归：使用默认 Keymap、真实 Redis Tab、Focus::Results 和已加载节点；断言 o 后 expanded 改变，y 返回 WriteClipboard 且文本等于完整 key。
5. 执行新增用例，预期旧实现返回 SQL Action 而失败；保留这一证据。
6. `help_overlay_is_contextual` 失败若仍存在，用合并前 HEAD 或隔离基线同条件复现后再归因，不能继续沿用之前未经独立对照的“已有失败”结论。

### Task 1：修正按键上下文及配置注册

**Files:** Modify `src/input/keymap.rs`、`src/config.rs`、`tests/keymap.rs`；必要时同步设置示例的实际文件。

1. 收紧 map_configured_navigation 中 Results 分支，只覆盖真实可浏览表格的上下文；分别检查 SQL、Relation、Dashboard 现有行为。
2. 将两个 Redis 分支合成一个入口，过滤 editing 优先，随后按 Keys/Preview 处理。
3. 注册 Redis toggle/copy/delete/expand/collapse 等语义命令，显式解析配置；不采用 other 原样返回通用 Action。
4. 检查现有全局/pending sequence 优先级，确保查询中的普通字符不启动全局导航序列。
5. 扩充测试：默认 o/y、自定义绑定、Shift/Control、Preview、Explorer、过滤 editing、无选中节点。
6. 执行：

```bash
cargo +1.94.0 test --test keymap
cargo +1.94.0 test --lib input::keymap
```

**复核：** Redis Keys 的 o/y 路径不能出现 ToggleResultView、CopyGridCell；自定义配置也走 Redis 语义。

**提交检查点：** `fix(redis): isolate keys shortcuts from result grid bindings`。

### Task 2：修复 key→Prefix 提升及父子一致性

**Files:** Modify `src/model/redis_key_tree.rs`；Test `tests/redis_key_tree.rs`。

1. 为 `[user, user:1]`、反序、分批到达分别建树，断言完整 nodes 结构一致。
2. 在 rebuild 和 incremental 插入路径中实现相同提升规则：原 Key 成为 key_id，新 id 为 Prefix，保留所有后代。
3. 索引同时保留 Key 和 Prefix；parent_of(key_id) 返回当前 Prefix，first_child 优先附属 Key。
4. 修复当前 `parent_of()` 将 node.id 和 node.key_id 共用 parent 返回值的问题。
5. 对既有选择/展开做协调：当前真实 key 保持身份，必要时展开新父路径，不让选中行不可见。
6. 验证空 key、重复分隔符、末尾分隔符、二进制字节；可见 ID 和行投影严格相同顺序。
7. 执行 `cargo +1.94.0 test --test redis_key_tree`。

**核心回归用例形状：** 使用现有 `key()` fixture，必须保留如下身份断言：

```rust
#[test]
fn key_before_descendant_is_promoted_to_a_folder() {
    let mut tree = KeyTreeState::default();
    tree.rebuild(&[key(b"user")]);
    tree.insert_keys(&[key(b"user:1")]);
    let prefix = KeyTreeNodeId::Prefix(b"user:".to_vec());
    let own_key = KeyTreeNodeId::Key(b"user".to_vec());
    assert!(tree.contains(&prefix));
    assert_eq!(tree.parent_of(&own_key), Some(prefix.clone()));
    assert_eq!(tree.visible_ids(), vec![prefix.clone()]);
    tree.expanded.insert(prefix.clone());
    assert_eq!(tree.visible_ids(), vec![prefix, own_key, KeyTreeNodeId::Key(b"user:1".to_vec())]);
}
```

**提交检查点：** `fix(redis): promote keys with descendants into prefix nodes`。

### Task 3：同步过滤展开、选择与滚动

**Files:** Modify `src/model/redis_browser.rs`、`src/app.rs`、`src/input/mouse.rs`、`src/ui/redis_browser.rs`；Test `tests/redis_key_filter.rs`、`tests/redis_browser_tabs.rs`、`tests/ui_render.rs`。

1. 为过滤后 o/h/l、n/N、鼠标 toggle 和 viewport 添加行为用例。
2. 模型统一提供当前 rows、selected index、parent、first displayed child、ensure_visible；App 不再混用 tree.visible_ids 与 filtered_rows。
3. 过滤拥有独立 expanded；非空查询初始化展开祖先，确认后用户可以折叠，修改查询后重新计算。
4. 空查询显示普通树；再次 / 编辑保留初次来源快照；editing 取消恢复来源；confirmed 退出使当前 key 在普通树中可见。
5. 增量 SCAN 重建过滤时保留仍有效选择，不能每批跳回第一个匹配；refresh 接受替换快照时重建树，不能只 insert 使已消失 key 残留。
6. 修复普通滚动偷偷移动选择的问题，选择操作负责滚动；选中变化只发一次相应预览命令。
7. 查询和节点变化时更新投影，render 不做全量重建；祖先索引用 HashMap 或一次遍历，避免每个匹配多次线性搜索全树。
8. 执行：

```bash
cargo +1.94.0 test --test redis_key_filter --test redis_browser_tabs --test redis_key_tree
cargo +1.94.0 test --test ui_render redis_browser
```

**提交检查点：** `fix(redis): synchronize filtered tree navigation and expansion`。

### Task 4：共享分组图标样式

**Files:** Modify `src/ui/mod.rs`、`src/ui/redis_browser.rs`；可将共享 helper 放入 `src/ui/icons.rs`；Test `tests/ui_render.rs`。

1. 以真实 Explorer Group 行为参照，区分 Group(None kind) 与 Table(kind Table)。
2. 提取小型 group_icon_style(theme, selected)，两端共用 muted foreground、surface/selection background。
3. Redis 以 Prefix 身份决定文件夹，而非只看 expandable；叶子保持 redis_key glyph。
4. 添加比较 Explorer Tables/Views 和 Redis Prefix 的实际 buffer cell fg/bg 的回归，不只测字符串或硬编码颜色。
5. 检查三种图标模式及选中/未选中，标签高亮独立。
6. 执行 `cargo +1.94.0 test --test ui_render`，记录任何独立失败。

**提交检查点：** `fix(redis): share explorer group icon styling`。

### Task 5：单 key 删除确认状态与弹窗

**Files:** Create `src/model/redis_delete.rs`、`src/ui/redis_delete.rs`；Modify `src/model/mod.rs`、`src/model/workspace.rs`、`src/action.rs`、`src/app.rs`、`src/ui/mod.rs`、`src/input/keymap.rs`、`src/input/mouse.rs`；Test `tests/redis_key_delete.rs`、`tests/keymap.rs`、`tests/ui_render.rs`。

1. 新增首次 d 仅打开 Overlay、不产生 DeleteRedisKey 的回归；替换旧“d 立即生成 DEL”测试。
2. 定义不可变 target 与 operation identity、默认 Cancel 的按钮 focus、phase。
3. 单 key 直接 Prepared(1)，连接/DB/key 从发起时保存；空 key 显示明确的 empty-key 标签。
4. RedisDeleteRequest/MoveFocus/Confirm/Cancel 分开，删除执行函数只允许 Confirmed Prepared 进入。
5. 参照连接删除绘制标题、目标、按钮、焦点和 hit regions；共享绘制 helper 仅在确实减少重复时提取。
6. Overlay 路由在工作区之前消费按键；重复按 d、重复 Enter、Repeat 事件不创建重复执行。
7. 直接 Action 也校验 phase、目标和有效连接，不只依赖 keymap。
8. 验证取消/Esc/默认 Enter 不执行，选 Delete 后 Enter 执行准确 key，鼠标一致。
9. 执行 `cargo +1.94.0 test --test redis_key_delete --test keymap --test ui_render`。

**提交检查点：** `feat(redis): confirm single key deletion in a dedicated dialog`。

### Task 6：独立前缀扫描与冻结清单

**Files:** Create `src/db/redis/delete_plan.rs`；Modify `src/db/redis/mod.rs`、`src/db/redis/types.rs`、`src/runtime.rs`、`src/action.rs`、`src/model/redis_delete.rs`；参考 `src/db/redis/key_index.rs`；Create `tests/redis_delete_plan.rs`。

1. 编写 fake RESP server 或现有 protocol fixture 测试：多页、重复 key、空页非零 cursor、非 UTF-8、特殊 glob 前缀、错误。
2. 实现前缀 MATCH 转义纯函数，append `*`；服务端返回结果再做 raw starts_with。
3. SCAN cursor 独立于浏览器，COUNT 使用配置作为 hint；按连接 identity、profile 和 DB 选择已绑定 adapter，不在共享连接中临时 SELECT。
4. 复用/扩展现有临时 SQLite 索引承载去重清单：key 列 BLOB UNIQUE，按二进制顺序分页；先确认已有 index 是否具备能力，不假设 KeyStore 等价于持久清单。
5. 若现有设施可支持内存到磁盘迁移则复用；否则首版直接临时索引，避免另造复杂迁移框架。
6. 每页提交后只发送节流的累计数量进度；cursor==0 才将计划设为 Prepared。0 匹配显示空状态，删除按钮禁用。
7. 临时清单受既有磁盘/内存预算约束；达到限额或异常就 Failed，不能继续确认不完整清单。
8. Prepared 保存 plan_id、总数和原始 prefix/identity；冻结清单后不可追加。
9. Prepared 取消、Tab 关闭、连接失效和 Runtime shutdown 均释放清单文件；准备期间取消只停止后续 scan，不冒充服务端取消。
10. 执行 `cargo +1.94.0 test --test redis_delete_plan --test redis_protocol_limits`。

**复核：** 客户端只加载 2 个但服务端存在 20 个匹配时，确认数为扫描得到的 20；查询过滤不缩小删除范围。

**提交检查点：** `feat(redis): prepare frozen prefix deletion plans`。

### Task 7：有界批量删除与结果反馈

**Files:** Create `src/db/redis/write.rs`；Modify `src/db/redis/mod.rs`、`src/runtime.rs`、`src/action.rs`、`src/model/redis_delete.rs`、`src/ui/redis_delete.rs`；Test `tests/redis_delete_plan.rs`、`tests/redis_key_delete.rs`。

1. 对固定清单执行器增加测试：重复 Confirm、已不存在 key、批次失败、取消、结果未知。
2. 单 key 和前缀共用计划执行通路；单 key 也具备独立 operation_id。
3. 从冻结清单顺序取页，按 key 数量与总参数字节切批，限制同时在途批次；首版一次一个批次，方便取消和精确归因。
4. 使用有界 pipeline 的逐 key DEL，保留每个回复 0/1 或 error，使部分成功可以精确映射到 key；不要只用 aggregate 数量猜测哪些 key 成功。
5. 实施时核对 redis 1.5.0 的 pipeline 错误返回行为，确保单条 error 不吞掉其他回复。若现有 API 无法保留逐条结果，采用有界逐条执行，优先结果准确。
6. 批次回调包含 identity、batch_id、成功清理 key、实际删除数、已不存在数、明确失败/未知状态；App 按 batch_id 幂等应用。
7. 遇权限或网络错误停止后续批次；网络未知结果不自动重试，刷新复核后可以发起新的准备流程。
8. 用户取消执行只阻止下一批，等待在途批次结果并同步已确认成功；终态释放清单。
9. 执行：

```bash
cargo +1.94.0 test --test redis_delete_plan --test redis_key_delete --test redis_commands
```

**提交检查点：** `feat(redis): execute bounded confirmed deletion plans`。

### Task 8：缓存、预览及请求身份协调

**Files:** Modify `src/app.rs`、`src/model/keyspace.rs`、`src/model/redis_browser.rs`、`src/db/redis/key_store.rs`、`src/db/redis/metadata_cache.rs`、`src/runtime.rs`；Test `tests/redis_scan.rs`、`tests/redis_key_delete.rs`、`tests/redis_key_filter.rs`。

1. 先补 remove_key 的真实缓存测试：通过 apply_batch 填充 store，不直接赋值 public keys 绕开计数和去重状态。
2. 修复 staged_bytes：只有 staged_set 实际移除时才扣除；loaded 不存在也必须正确维护 staged/pending，返回值明确表达变化。
3. 增加批量 remove_keys，一批成功只协调一次树和过滤，不对每个 key 全量 rebuild。
4. 成功 0/1 均清理名称并写入当前 scan generation tombstone；refresh 清空旧 tombstone；准备清单和普通 scan 独立。
5. 每个回调校验 operation_id、connection、target、batch_id；不能仅以 tab_id/key 认定有效。
6. scan generation 变化不自动丢弃同一有效连接上已经执行成功的删除：成功应用当前 cache；连接已失效则不套用到新连接，必要时标记目标视图 stale。
7. 同 target 的其他已打开视图同步更新；owner Tab 关闭不重开、不抢焦点，执行任务按取消契约停止后续批次。
8. 用户当前选择仍有效则保留；被删除时下一项→上一项→仍存在祖先→None，使用当前过滤视图顺序。
9. 保留查询和过滤模式；删除只更新结果与计数，不强制 find=None。失效的原始恢复选择使用同一回退。
10. 如果预览对应被删 key，递增 preview generation、清缓存；新选中真实 key 走统一加载入口，Prefix 清空预览。
11. 精确区分 Partial/Paused/CompleteEmpty，不能因局部 loaded 为零就声称数据库为空。
12. 执行 `cargo +1.94.0 test --test redis_scan --test redis_key_delete --test redis_key_filter --test redis_browser_tabs`。

**交错矩阵：** scan→删除→late batch；prepare→refresh→execute；execute→重连→late reply；执行中用户移到其他 key；部分成功→取消；同名 key 外部重建→显式 refresh。

**提交检查点：** `fix(redis): reconcile confirmed deletions across browser lifecycles`。

### Task 9：帮助、文档与完整验收

**Files:** Modify `src/help.rs`、`docs/redis-browser.md`、`docs/keybindings.md`、`docs/testing/redis-browser-performance.md`；Test `tests/keymap.rs`、`tests/ui_render.rs`、`tests/redis_contract.rs`。

1. 帮助区分 Redis Keys、Preview、Filter Editing、删除 Preparing/Prepared/Executing；提示从实际命令配置生成。
2. 文档说明前缀结尾冒号、统计来自独立扫描、冻结清单、执行中取消及结果未知语义。
3. 给 Redis adapter 增加显式环境配置的隔离实例集成测试：只创建和删除本测试唯一前缀，测试结束清理自己创建的数据。
4. 用实际终端执行 `cargo +1.94.0 run` 并完成流程：o 展开→y 检查完整剪贴板→d 取消→单 key 确认→分组统计/确认→过滤模式重复操作。
5. 渲染验证中文、长前缀、控制字节、三种图标模式、窄窗口；执行器验证数量/字节预算，性能测试不设脆弱毫秒阈值。
6. 运行最终检查：

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --test keymap --test redis_key_tree --test redis_key_filter --test redis_key_delete --test redis_delete_plan --test redis_browser_tabs --test redis_scan --test ui_render
cargo +1.94.0 test --all-targets --all-features
git diff --check
```

7. 全量测试使用足够超时或后台任务等待结果；不要把超时写成通过。被 ignored 的真实 Redis/规模测试需单独标注是否执行。
8. 每项记录实际变更、运行命令、结果与偏差；有验收缺口时保持“未完成”，不能仅凭编译和少量单测通过宣布整个任务完成。

**提交检查点：** `docs(redis): document confirmed key and prefix deletion`。

## 5. 最终验收清单

- [ ] 默认配置和自定义配置中 o/y 不进入 SQL Action。
- [ ] 先 key 后后代、反序及跨批次生成同一 Prefix/Key 结构。
- [ ] 普通和过滤状态的 o/h/l 都有真实可见变化。
- [ ] y 实际复制完整名称，不出现 Data view 提示。
- [ ] 文件夹实际 buffer 颜色与 Explorer Tables/Views 分组一致。
- [ ] 单 key 首次 d 只弹确认框，默认 Enter 取消。
- [ ] 分组 d 扫描目标 DB 全部前缀，数量去重，准备中不可删除。
- [ ] 删除 user: 不会删除 user 或 user_backup；特殊 glob 字符按字面处理。
- [ ] 确认仅执行冻结清单，批次受到数量和字节预算限制。
- [ ] 部分失败/取消/未知结果真实反馈，不伪造回滚或全部成功。
- [ ] 旧回调、重复 Confirm、late SCAN、refresh 和 Tab 关闭正确协调。
- [ ] 删除后仍有效选择、过滤查询、预览与数量一致。
- [ ] format/clippy/定向/全量检查有实际结果，终端交互经过验证。

## 6. 交付要求

交付代码、必要测试、更新文档，以及逐项执行记录。上一轮的简单 RedisDeleteKey/Deleted/Failed 通路应在迁移后收敛为确认计划执行通路，避免保留可绕过确认的旧入口。发布、提交、合并和 worktree 清理由后续用户授权决定。
