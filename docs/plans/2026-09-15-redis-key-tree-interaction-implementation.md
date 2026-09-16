# Redis Keys 树交互优化 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行环境若没有该技能，按本文任务顺序执行并记录验证结果，不依赖不可用工具。

**Goal:** 实现分组整行单击展开/收起、Explorer 一致间距、弱化的分组 Key 数量、可用的 `r` 刷新，以及仅在叶子回车或双击时打开 Value。

**Architecture:** 保留现有 Rust Action → App reducer → Command → runtime 架构、KeyTreeState 和扫描调度。将树选择与 Value 打开状态解耦，在树模型缓存子树计数，并让扫描结果明确区分追加与快照替换；鼠标和键盘共享选择、切换、打开三个动作入口。

**Tech Stack:** Rust 2024 / Rust 1.94、Ratatui 0.30、Crossterm 0.29、Tokio、redis 1.5；沿用现有集成测试与 TUI buffer/hit-region 测试方式。

---

## 0. 交互契约与范围

### 0.1 操作矩阵

| 操作 | 分组 | 叶子 Key |
| --- | --- | --- |
| 单击箭头、图标、文字、计数或行内空白 | 选中、聚焦 Keys，并切换展开状态 | 仅选中、聚焦 Keys |
| 同一节点双击 | 第二击不再次切换 | 查询并打开该 Key，一次 |
| Enter | 切换展开状态 | 查询并打开该 Key |
| 上下 / j、k | 移动选择 | 移动选择，不查询 |
| 右 / l | 展开，已展开则移到首个可见子节点 | 不查询 |
| 左 / h | 收起，已收起则移到父节点 | 移到父节点 |
| 搜索确认、n/N、取消搜索 | 只更新搜索、选择、滚动 | 不查询 |
| r | 按扫描状态刷新或继续 | 按扫描状态刷新或继续 |

- 搜索编辑态 Enter 仍为确认搜索，r 仍为输入字符；确认后再次在叶子 Enter 才打开 Value。
- 移除 Keys 面板 `o` 的叶子打开映射，满足“仅回车或双击”的约束；保留既有可配置导航。
- 展开/收起、移动、刷新均不重置已打开 Value 的格式、滚动与表格选择。
- Value 显示最后显式打开的 Key；打开后光标移走，在途响应仍可正常完成。
- 已打开 Key 被用户明确删除时，清理对应 Value 并使旧响应失效；扫描暂未发现该 Key 不等于删除，不清空 Value。
- 编辑保存是显式数据操作：只允许更新同一个已打开 Key 的预览；编辑的是另一个选中 Key 时，不自动将 Value 切换过去。
- Value 分页与重试仍可查询已打开 Key；格式切换尽量使用已加载数据，不能因树光标改变查询目标。

### 0.2 计数定义

- `total_keys` 是当前树快照中去重的实际 Key 子树总数，包括所有深层后代。
- `foo` 和 `foo:bar` 共存时，沿用现有“分组承载同名 Key”模型，foo 文件夹计数为 2；这是子树数量，不是严格 `starts_with(b"foo:")` 的数量。
- 叶子总数为 1，但 UI 不显示 `(1)`；分组显示 ` (N)`。
- 折叠和搜索过滤不改变分组总数；搜索匹配数量由现有搜索状态单独显示。
- 未完成扫描时明确标注“基于已加载 Keys”；Complete 也仅代表本轮扫描结果，不宣称实时事务快照。
- 数字使用 `theme.muted`、普通字重、当前行背景，选中时仍保持弱化。

### 0.3 r 的状态表

| 状态 | 行为 |
| --- | --- |
| NotLoaded | 发起初始扫描 |
| Complete / CompleteEmpty | generation 更新，从 Start 刷新 |
| Failed / Stale | generation 更新，从 Start 重试 |
| Partial | 使用现有 Continue cursor 继续，不重启 generation |
| Loading / 已有请求在途 | 忽略重复请求，不能重复 refresh() |
| Paused | 保持缓存上限暂停，显示无法在同一预算下继续的原因 |

## 1. 已确认代码事实

- `src/model/redis_browser.rs::select` 当前会递增 preview_generation、重置三个预览状态与滚动。
- `src/app.rs::select_redis_key` 将选择传给 100ms PreviewScheduler；移动、左右导航、初始扫描选择等调用了它。
- `src/input/mouse.rs` 当前仅分组双击 toggle，叶子单击即 SelectRedisNode。
- `src/ui/redis_browser.rs::render_row` 的 marker 自带空格后又追加空格，且箭头热区固定在 area.x。
- `src/ui/mod.rs::explorer_list_item` 已使用两格层级缩进、单字符 marker 和一个间隔；Explorer 热区考虑 depth。
- `src/input/keymap.rs` 已将 r 映射到 RedisRetryScan；`retry_redis_scan` 不处理 Complete/CompleteEmpty。
- `KeyspaceState::apply_batch` 在刷新后的首个可发布批次替换 keys，不是等待全部 SCAN 结束。
- `RedisKeysLoaded` 当前调用 `insert_tree_keys`，后者仅追加；快照替换后旧树可能残留已消失 Key。
- Value 的旧预览结果和分页结果使用不同的校验字段，拆分状态时必须逐个审计，不能仅修改查询入口。

## 2. 执行约定

- 每个任务按“小范围回归测试 → 确认预期失败 → 实现 → 定向验证”推进；不为单纯的一格空白修改单独搭测试框架。
- 使用现有 App fixture、Action、Command 和 Ratatui 测试风格，不依赖真实 Redis 验证 reducer 逻辑。
- 每一步是一个可独立完成的工作单元；文件路径和符号名为定位依据，行号可能随着编辑变化。
- 本计划只增加计划文档。执行代码改动前检查 git status，保留其他已有工作。
- 每个阶段可形成一个逻辑提交；实际 commit 仅在用户要求提交时执行。

## Task 1：分离模型中的选择与打开状态

**Files:**
- Modify: `src/model/redis_browser.rs`
- Test: `tests/redis_browser_tabs.rs`

**Step 1 — 补状态回归用例。**
构造 A/B 两个 Key，先显式打开 A 并设置 Ready、非零滚动、手动格式；选择 B 和父分组后断言 opened_key、preview_generation、preview/value_page/content、滚动和格式全部保持。首次选择叶子也不能进入 Loading。

**Step 2 — 定向运行并确认旧行为不满足契约。**

```bash
cargo test --test redis_browser_tabs
```

**Step 3 — 增加打开目标并拆分方法。**
- 增加 `opened_key: Option<RedisKeyId>`，new() 初始化 None。
- 保留 `select(node)` 作为纯选择操作，内部只选择合法节点并按当前 visible_rows 校正滚动。
- 新增 `open_key(key: RedisKeyId)`，负责 opened_key、请求 generation、Loading 状态、分页标志及 Value 滚动/网格初始化。
- open_key 只接受当前 tab.target 的 Key；App 入口还需验证真实叶子身份和节点存在性。
- 格式重置通过“上一次 opened_key 与新 opened_key”比较决定，不能比较 tree.selected。
- 打开同一个 Key 仍允许重新请求，但移动到同一个 Key 不产生请求。
- 新增集中清理预览的方法，用于明确删除和目标失效，统一递增 generation 并清空 opened_key。

**Step 4 — 更新直接用 select() 初始化预览的测试 fixture。**
所有预览 fixture 明确调用 open_key；纯树选择 fixture 继续调用 select。不能为维持旧测试保留选择副作用。

**Step 5 — 再次定向运行。**
预期模型行为测试通过；跨模块尚未迁移导致的失败记录到 Task 2，不通过放宽断言隐藏。

## Task 2：统一显式打开入口与异步请求身份

**Files:**
- Modify: `src/action.rs`
- Modify: `src/app.rs`
- Modify: `src/db/redis/preview_scheduler.rs`（仅需要调整调度接口时）
- Modify: `src/runtime.rs`（按结果身份审计结论修改）
- Modify: `src/ui/redis_browser.rs`（Value 标题与空状态）
- Test: `tests/redis_browser_tabs.rs`, `tests/redis_loading_lifecycle.rs`, `tests/redis_preview_serialization.rs`

**Step 1 — 增加 App 层请求契约测试。**
- SelectRedisNode、RedisMoveSelection、左右导航和首次加载根叶子后，推进调度 tick，断言没有 LoadRedisValuePreview。
- 显式打开后推进 tick，断言只发一个正确 target/key 请求。
- 打开 A 后只选择 B，A 结果仍接收；显式打开 B 后，A 迟到结果拒绝。
- 相同 key 字节但不同数据库、旧连接、已关闭 tab 的结果不能写入当前预览。

**Step 2 — 运行基线。**

```bash
cargo test --test redis_browser_tabs --test redis_loading_lifecycle --test redis_preview_serialization
```

**Step 3 — 拆分 App 动作。**
- `SelectRedisNode` → 纯选择处理，删除自动安排请求的行为。
- 新增携带 `tab_id` 和节点身份的 `RedisOpenKey` 动作，供鼠标显式定位使用。
- 新增/重命名 App 打开函数，验证 `KeyTreeNodeId::Key`、节点存在和连接目标，然后调用模型 open_key 并安排调度。
- `RedisPrimarySelection`：分组 toggle，叶子走同一打开函数。
- 删除或重命名含糊的 select_redis_key，并逐个迁移调用者，禁止遗留“名字是选择，实际是查询”的入口。
- scheduler debounce 改为零；保留在途限制及待处理请求合并，后续 tick 派发。

**Step 4 — 统一响应与显示目标。**
- Value 标题从 opened_key / 对应 metadata 读取；没有打开过时显示 Enter/双击提示。
- 元数据、分页、失败、序列化结果均检查连接身份、tab/target、preview_generation 与 opened_key 一致性。
- Keys 刷新的 scan generation 不能使独立 Value 请求永久停留 Loading；旧预览 Action 若错误复用 keyspace generation，应迁移为连接身份加 preview_generation。
- 过期响应不能错误释放另一个请求的 scheduler 在途状态；如现有 bool 不足，最小化保存当前请求身份并匹配完成。

**Step 5 — 再次运行定向测试。**
额外检查分页中连开 A/B、错误后重试、tab 关闭与重连；采用可控 tick/时钟，不依赖 sleep。

## Task 3：迁移搜索、删除、编辑等关联入口

**Files:**
- Modify: `src/app.rs`
- Modify: `src/model/redis_browser.rs`
- Test: `tests/redis_key_filter.rs`, `tests/redis_key_delete.rs`, `tests/redis_mutation.rs`, `tests/redis_object_editor.rs`

**Step 1 — 补关联行为用例。**
搜索确认、n/N、取消搜索恢复选择、删除非打开 Key 后回退选择，都不打开 Value；删除 opened_key 会失效旧响应；编辑 B 不把已打开 A 切换为 B。

**Step 2 — 运行对应测试，确认需要迁移的旧断言。**

```bash
cargo test --test redis_key_filter --test redis_key_delete --test redis_mutation --test redis_object_editor
```

**Step 3 — 迁移所有选择型调用。**
- 搜索及导航统一纯选择。
- 首次扫描选择首个根节点，不打开叶子。
- 删除回退只选中；明确删除 opened_key 时集中清理 Value。
- 编辑保存后更新 Keys；仅变更目标等于 opened_key 时刷新其预览。
- Value 重试、格式与分页始终以 opened_key 为目标。
- 用当前投影 `tab.visible_rows()` 校正选择和滚动，避免搜索态拿未过滤树的行索引。

**Step 4 — 再次执行定向测试。**
预期所有关联操作遵守 Task 0 契约，Value 不因树选择变化被重置。

## Task 4：鼠标单击分组、双击叶子及键盘映射

**Files:**
- Modify: `src/input/mouse.rs`, `src/input/keymap.rs`
- Modify: `src/ui/mod.rs`（点击跟踪与 HitTarget）
- Modify: `src/ui/redis_browser.rs`（行命中区域）
- Modify: `src/app.rs`, `src/model/redis_browser.rs`（toggle 选中和滚动）
- Test: `tests/mouse.rs`, `tests/keymap.rs`

**Step 1 — 补真实命中/输入回归。**
- depth=0/1/2 的分组，点击箭头、图标、文字、计数和行空白都 toggle 一次。
- 同一分组双击最终只改变一次状态；叶子单击不查询，双击恰好产生 RedisOpenKey。
- 不同 tab/不同节点/超过 400ms 不构成双击；点击其他目标应打断旧点击链。
- Value 聚焦时点击 Keys，焦点和选择正确更新。
- Enter 分组/叶子映射正确，搜索编辑态 Enter 只确认，o 不打开叶子。

**Step 2 — 运行输入测试。**

```bash
cargo test --test mouse --test keymap
```

**Step 3 — 统一分组行热区。**
推荐每行使用一个 RedisKeyNode 命中目标，按 Prefix/Key 分类；分组无需额外的 RedisKeyToggle 覆盖热区。移除仅 Redis 使用的冗余分支，保留 Explorer 行为。热区覆盖行可用宽度，并排除滚动条列。

**Step 4 — 修改点击分派。**
- Prefix 首击 → RedisToggleNode；识别到同一节点第二击 → 不再 toggle。
- Key 首击 → SelectRedisNode；第二击 → RedisOpenKey。
- toggle reducer 原子完成聚焦、选择、切换和滚动校正；折叠后不存在隐藏选择。
- 保留 400ms 现有阈值，不增加新的用户配置项。

**Step 5 — 修改键盘导航。**
左右键只导航；Enter 共享 primary；移除 o 的打开映射；保留自定义上下左右导航映射和搜索输入优先级。

**Step 6 — 运行输入测试确认通过。**
双击用显式 Instant 时间点测试，不能使用 wall-clock sleep。

## Task 5：缓存分组 Key 数量

**Files:**
- Modify: `src/model/redis_key_tree.rs`
- Modify: `src/model/redis_browser.rs`（过滤投影携带字段）
- Test: `tests/redis_key_tree.rs`, `tests/redis_key_filter.rs`, `tests/redis_scale.rs`

**Step 1 — 添加计数边界回归。**
基于现有 tests/redis_key_tree.rs 的 key() helper 添加以下完整测试：

```rust
#[test]
fn subtree_counts_include_own_key_and_ignore_duplicate_inserts() {
    let mut tree = KeyTreeState::default();
    tree.rebuild(&[key(b"foo"), key(b"foo:a"), key(b"foo:group:b")]);
    let prefix = KeyTreeNodeId::Prefix(b"foo:".to_vec());
    let rows = tree.visible_rows();
    let folder = rows.iter().find(|row| row.id == prefix).unwrap();
    assert_eq!(folder.total_keys, 3);

    tree.insert_keys(&[key(b"foo:a"), key(b"foo:group:c")]);
    let rows = tree.visible_rows();
    let folder = rows.iter().find(|row| row.id == prefix).unwrap();
    assert_eq!(folder.total_keys, 4);

    tree.rebuild(&[key(b"foo"), key(b"foo:group:c")]);
    let rows = tree.visible_rows();
    let folder = rows.iter().find(|row| row.id == prefix).unwrap();
    assert_eq!(folder.total_keys, 2);
}
```

同时扩展既有“增量插入等于全量构建”测试，覆盖反向插入顺序、重复批次、空段/连续冒号、二进制 Key 和多层分组。

**Step 2 — 运行并确认字段缺失或计数断言失败。**

```bash
cargo test --test redis_key_tree --test redis_key_filter
```

**Step 3 — 增加字段与维护规则。**
- KeyTreeNode、VisibleKeyTreeRow 增加 `total_keys: usize`。
- 全量 build：先构造子节点，总数为本节点实际承载 Key 的 0/1 加所有子节点总数；同名 key_id 与叶子身份不可双计。
- 增量：沿真正新增 Key 的插入路径递增；现有 node_index 去重先于递增。
- 叶子升格为分组时保留原来的 1，再增加新 Key；不要重置成 0。
- visible_rows 将缓存数量带出，同名实际 Key 的虚拟叶子为 1。
- 搜索投影保留原数量，过滤后不按显示行数重算。

**Step 4 — 验证计数与规模行为。**

```bash
cargo test --test redis_key_tree --test redis_key_filter --test redis_scale
```

预期增量和全量结果一致；渲染读取缓存，不在每行扫描全部 keys；新增计数只沿路径维护，不额外进行整树计数。

## Task 6：修复 r 刷新及快照替换后的树同步

**Files:**
- Modify: `src/model/keyspace.rs`
- Modify: `src/model/redis_browser.rs`
- Modify: `src/app.rs`
- Test: `tests/redis_loading_lifecycle.rs`, `tests/redis_scan.rs`, `tests/redis_key_filter.rs`

**Step 1 — 补刷新回归用例。**
- Complete/CompleteEmpty 按 r 发起 Start；Loading 再按 r 不增加 generation/请求。
- Partial 按 r 延续 cursor；Paused 不突破上限。
- 旧树为 a/b，新刷新批次为 b/c 时，发布后树只含 b/c，计数同步；空结果清空树。
- 刷新失败且尚未发布新快照时保留旧树；旧 generation 结果无效。
- 新快照还含选中节点时保留选择，否则选最近存在的可见父分组，再回退首个可见节点；展开身份与滚动合法。
- 搜索确认态刷新后匹配与计数更新，且全过程无 Value 打开请求。

**Step 2 — 运行扫描相关测试。**

```bash
cargo test --test redis_loading_lifecycle --test redis_scan --test redis_key_filter
```

**Step 3 — 明确批次发布类型。**
推荐将 apply_batch 的 bool 返回值改成清晰的结果枚举，例如 `Ignored / Appended / Replaced / Buffered`：
- Ignored：过期或非法响应。
- Appended：当前 keys 增量改变。
- Replaced：刷新旧快照被新的 keys 替换。
- Buffered：刷新数据暂存且尚未发布（例如预算暂停）。
更新所有调用者和既有测试；结果枚举只表达当前发布动作，不引入第二套扫描状态机。

**Step 4 — 按批次结果同步树。**
- Appended → insert_tree_keys。
- Replaced → rebuild_tree，随后刷新搜索投影、恢复有效展开/选择与滚动。
- Ignored/Buffered → 不把暂存数据混入当前树。
- 延续当前“首个可发布批次替换旧快照，后续继续增量”的策略；明确状态显示 Partial，不改成等待全库扫描完成。

**Step 5 — 扩展 r handler。**
按 Task 0 状态表扩展 retry_redis_scan；将面向用户的处理函数命名为 refresh_or_continue_redis_scan，内部初始自动续扫可保持独立方法，避免无意触发从头刷新。

**Step 6 — 校正打开 Value 与扫描独立性。**
刷新不清空 opened_key，也不发 Value 请求；部分扫描没有某 Key 时不当成删除。选择回退与预览失效不能共用一个方法。

**Step 7 — 再次运行扫描测试。**
预期已消失节点不会残留、数字与树来自同一快照、空库可刷新、旧结果不能覆盖新结果。

## Task 7：统一树行视觉与帮助文案

**Files:**
- Modify: `src/ui/redis_browser.rs`
- Modify: `src/help.rs`
- Modify: `README.md`（仅已有相关交互说明需要更新时）
- Test: `src/ui/redis_browser.rs` 内测试或现有 UI 测试入口
- Test: `tests/keymap.rs`

**Step 1 — 修改树行拼接。**
- marker 改为单字符 `▾` / `▸` / 空格，保留两格层级缩进和 marker 后一格。
- 图标后保留一格，与 Explorer 使用相同 IconSet。
- 分组名称之后追加独立 muted ` (N)` Span，不继承选中名称的 BOLD/accent。
- 截断按终端 cell 宽度计算，窄面板不越界、不覆盖滚动条；沿用现有文本转义和二进制 Key 显示方法。

**Step 2 — 修改状态及帮助。**
- 完成态帮助 `r refresh keys`；Partial `r continue scan`；失败态 `r retry`。
- 未完成时说明计数基于 loaded keys；旧快照显示 stale/refreshing。
- 空 Value 文案提示“Enter or double-click a key to view its value”。
- 帮助表记录单击分组、双击叶子；删除 o 的叶子打开说明，保留真正存在的导航。

**Step 3 — 使用已有 buffer 测试验证组合行为。**
复用实际 render 输出和 hit_regions 验证 depth=0/1/2 的图标位置、计数 muted 字重、选中背景、窄宽度与滚动偏移；不为字符串拼接本身编写镜像实现测试。

**Step 4 — 运行 UI 相关单元测试及键盘集成测试。**

```bash
cargo test --lib ui::redis_browser
cargo test --test keymap
```

预期新 UI 测试确实被执行（不能把 0 tests 当成验证通过），快捷键帮助与实际映射一致。

## Task 8：最终集成验证与人工验收

**Files:** 所有以上改动文件。

**Step 1 — 检查调用边界。**
检查所有 open_key/RedisOpenKey/LoadRedisValuePreview 调用点，仅保留回车/双击打开，以及已打开 Key 的显式重试、分页、保存后更新；不存在导航隐式调用。检查所有 KeyTreeNode/VisibleKeyTreeRow 构造点计数完整。

**Step 2 — 执行仓库既有检查。**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

预期全部通过。遵循 README 的默认 features；若本机 Oracle 环境阻塞，记录具体失败及覆盖缺口，不将减少 features 的结果表述为完整检查通过。

**Step 3 — 人工 TUI 验收。**
在可用 Redis 测试连接中，使用包含多层分组、无冒号叶子、同名 Key/分组、长名称的数据集：
1. 对照 Explorer 确认箭头到图标间隔一致。
2. 逐层点击箭头、图标、名称和数量，均只切换一次。
3. 展开/折叠时数量恒定；快速双击分组不闪回。
4. 快速 j/k、左右导航、单击叶子，Value 保持不动；回车和双击才打开。
5. 打开 A 后选择 B，标题、内容、分页均仍属于 A。
6. 搜索输入 r 不刷新；搜索确认 Enter 不打开，第二次 Enter 打开匹配叶子。
7. 外部新增/删除 Key 后在 Complete 状态按 r，树和数量反映新结果，无残留节点。
8. 空库 r 可用；Partial 可继续；刷新中连续按 r 无重复请求。
9. 刷新期间 Value 稳定，慢请求/快速连续打开无错位内容。
10. 使用不同图标模式、窄面板及滚动位置确认命中区域正确。

没有真实 Redis 时，记录人工验收未执行，交付已完成的自动化验证结果。

**Step 4 — 汇总交付。**
列出实际修改文件、测试结果、人工验收情况及计划偏差。推荐逻辑提交分组：状态解耦 → 输入行为 → 子树计数 → 刷新一致性 → UI/帮助；实际是否提交按用户指令执行。

## 完成标准

- 五项需求全部符合 Task 0 契约。
- 无导航触发的 Redis Value 查询，无错误 Key 标题/内容组合。
- 深层分组整行点击可靠，双击没有反向切换。
- 数量可随插入、删除、快照替换正确更新，UI 不为统计额外查询 Redis。
- Complete/CompleteEmpty 的 r 生效，刷新删除旧节点，保留合法 UI 状态。
- 异步旧结果与新打开请求隔离；Keys 扫描与 Value 生命周期解耦。
- 项目检查通过，人工验收状态如实记录。
