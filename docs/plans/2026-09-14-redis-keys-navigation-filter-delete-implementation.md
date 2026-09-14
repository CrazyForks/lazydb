# Redis Keys Navigation, Filter and Delete Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 若该技能不可用，按本文依赖顺序实施、验证并记录结果。新增接口和测试名为拟定名称，执行时按现有命名风格落地。本文是实施计划，不代表功能已经完成。

**Goal:** Redis Keys 支持一致的 o/h/l 树导航、d 删除、y 复制完整 key、Explorer 风格的文件夹颜色和本地过滤，以及语义化钥匙图标。

**Architecture:** 保留 Action → App → Command → Runtime 数据流和原始字节 key 身份。普通树与过滤树共用行投影契约，输入、导航、鼠标、滚动和预览使用同一选中状态；过滤复用 Explorer 的匹配与祖先保留规则，删除由 keyspace 集中维护缓存与扫描竞争。

**Tech Stack:** Rust 1.94 / Edition 2024、Ratatui 0.30.2、Crossterm 0.29、redis 1.5.0、TextInput、IconSet、unicode-width、现有 Rust 单元测试和集成测试；无需新增生产依赖。

---

## 1. 执行约定与代码基线

- 本计划以本次用户需求为准，覆盖历史计划中“裸 h/l 切换 Keys/Preview”的约定。
- 参考历史文档：`docs/plans/2026-09-13-redis-keys-interaction-implementation.md`、`docs/plans/2026-09-14-redis-browser-ui-consistency-implementation.md`。历史计划仅用于理解背景，当前源码是实现基线。
- 开始实施时运行 `git status --short`，检查目标文件已有 diff。编写计划时存在未跟踪文件 `docs/plans/2026-09-14-sql-history-modal-implementation.md`，应保留。
- 计划写入后检查发现工作区发生并发变化：Redis dashboard 等改动已暂存，`src/ui/mod.rs` 为 `UU` 且存在合并冲突标记。实施前应由对应合并流程解决冲突，再重新核对本计划涉及的符号与测试基线；本次编写计划没有修改或处理这些代码。
- 下列任务按顺序推进；每项拆成补回归用例、复现、实现、定向验证、差异复核五类小步骤。复杂步骤可继续拆成 2–5 分钟动作。
- 重点为状态和异步行为增加有意义的回归测试。简单图标替换采用现有渲染覆盖与人工检查，不新增只复述实现的测试。
- 文中提交信息仅为建议检查点；实际 commit/push 由后续执行授权决定，不使用 `git add .` 混入其他工作。

### 已确认的代码入口

| 文件 / 符号 | 基线事实 |
| --- | --- |
| `src/input/keymap.rs::Keymap::map` | Redis 分支约在 1221、1704 两处；前一处分支提前消费 h/l，o 已映射 Primary |
| `src/app.rs::expand_redis_selection` | 进入子节点直接 tree.select，未统一加载预览 |
| `src/app.rs::collapse_redis_selection` | 有折叠及选父节点逻辑，需统一滚动和预览更新 |
| `src/model/redis_key_tree.rs` | visible_rows/visible_ids/parent_of/first_child 对额外 key_id 的处理不一致 |
| `src/model/redis_browser.rs::open_find` | 快照仅包含打开时可见节点，update_find 使用小写 contains |
| `src/ui/redis_browser.rs::render` | 搜索快照与实时可见行做交集；导航仍使用普通树 |
| `src/ui/redis_browser.rs::render_row` | 文件夹复用 Tables glyph，但图标共用文字 style；叶子使用点号 |
| `src/model/explorer.rs::filtered_search_rows` | 过滤所有目录对象，补齐祖先路径 |
| `src/db/catalog.rs` | search_text_matches 和 search_text_match_ranges 提供一致匹配、高亮 |
| `src/model/keyspace.rs` | keys、key_set、store、staged、pending 等多份扫描状态需同步维护 |
| `src/clipboard.rs::ClipboardPayload` | 剪贴板内容是 String，可复用 WriteClipboard 和现有反馈机制 |

## 2. 最终行为契约

### 2.1 按键与焦点

| 上下文 | 按键 | 行为 |
| --- | --- | --- |
| Keys 浏览 / 已确认过滤 | j/k、↓/↑ | 在当前展示行中移动，选择保持可见，预览同步 |
| Keys 浏览 / 已确认过滤 | o、Enter | 文件夹展开/折叠并停留；真实 key 显式加载预览 |
| Keys 浏览 / 已确认过滤 | l、→ | 未展开目录仅展开；已展开目录进入首个展示子节点；叶子保持当前选择 |
| Keys 浏览 / 已确认过滤 | h、← | 已展开目录折叠；否则选择父节点；根节点不变 |
| Keys 浏览 / 已确认过滤 | d | 删除当前真实 key；纯 Prefix 不执行删除 |
| Keys 浏览 / 已确认过滤 | y | 复制当前真实 key 的完整名称；纯 Prefix 不执行 key 复制 |
| Redis Keys | / | 开启过滤；已确认过滤再次按 / 时编辑当前查询，不覆盖最初恢复快照 |
| 过滤编辑 | 字符键，包括 o/h/l/d/y | 输入字符，绝不触发树操作 |
| 过滤编辑 | Enter / Esc | 确认过滤 / 取消过滤并恢复来源位置 |
| 已确认过滤 | Esc | 退出过滤，尽量保留当前 key 并展开必要祖先以使其可见 |
| 已确认过滤 | n/N | 保留为下一个/上一个匹配 key，循环跳转并展开其路径 |
| Redis 工作区 | 现有 FocusNext/FocusPrevious | Explorer → Keys → Preview → Explorer，反向相反 |
| Redis 工作区 | 现有窗口方向快捷键 | 按实际窗格位置移动焦点；裸 h/l 不切换窗格 |
| Preview | j/k、↑/↓、PageUp/PageDown | 沿用预览滚动行为；不执行 Keys 删除和复制 |

修饰键、全局快捷键、用户配置遵守现有 keymap 层级。验证实际 Tab/Shift-Tab、Ctrl-w 系列与配置映射，不在 Redis 分支再造焦点系统。

### 2.2 树身份与选择

- `KeyTreeNodeId::Key(Vec<u8>)` 是真实 key；`Prefix(Vec<u8>)` 是虚拟文件夹。
- 同时存在 `user` 与 `user:1` 时：Prefix(user:) 的展示子节点包含 Key(user) 与 Key(user:1)。收起 Prefix 时二者都隐藏。
- 额外 key_id 的 parent 是当前 Prefix；first_child、visible_rows、visible_ids 必须与此一致。
- 用户导航、点击、过滤确认、删除后选中变化，统一由 App 同步预览；模型不发起 I/O。
- 同一 key 的纯可见性调整不重复加载。显式 Enter/o 可以刷新该 key 的预览。
- 过滤编辑只更新候选选中，不逐字符加载 key；编辑期间显示“按 Enter 预览”或等效状态，避免旧预览被误认为属于候选 key。取消时恢复原选择与原预览。
- 纯滚动不隐式改变 key 或加载预览；选择操作负责 ensure_visible。

### 2.3 过滤

- `/` 使用 Explorer filter 的“匹配对象 + 祖先”模式，不是原来的可见行 find。
- 搜索当前 DB 已加载的所有 key，包括折叠目录内的 key；按完整 key 匹配。
- 采用 Explorer 的大小写不敏感及分隔符归一化规则；只含分隔符的查询遵守公共 matcher 的回退规则。
- 有效查询仅由真实 key 产生匹配数，祖先文件夹不计入匹配数量。
- 非空查询首次展示全部匹配路径；确认后允许手动折叠。再次编辑查询时重算默认展开路径。
- 过滤展开状态独立于普通树 expanded；取消不污染普通树展开集合。
- 空查询返回普通树视图；零匹配显示 `No matching loaded keys`。
- 输入字符不派发 SCAN；SCAN 新批次、删除和刷新结果被接受后，重新协调过滤结果。
- 标题或状态行明确 `loaded keys` 范围；Partial/Paused/Stale/Failed 状态与零匹配信息分开表达。
- 按 Tab 隔离。关闭 Tab、切换 DB、失效回调不能修改其他 Tab 的过滤状态。

### 2.4 删除与复制

- 删除仅作用于当前完整字节 key，不推断前缀批量删除，不拼接命令字符串。
- 使用专用单 key 删除命令，首版选择 DEL 保持简单且符合现有支持范围；未来如需大 value 异步释放，可独立引入 UNLINK 策略。
- 删除等待期间保留行，同一 key 的重复 d 合并；允许正常导航。成功不强制把用户拉回发起位置。
- 删除返回 0 或 1 均清理本地 key；失败保留本地数据并沿用错误通知。
- 复制 UTF-8 key 时保持完整原文，包括空串。二进制 key 使用每字节 `\\xHH` 的明确转义文本，通知注明 escaped key，不使用 lossy 转换。
- 展示文本统一转义终端控制字符；展示转义不改变 Redis 请求字节及 UTF-8 剪贴板原文。

## 3. 任务依赖

```text
T0 基线
  → T1 树关系
  → T2 当前行与选择入口
  → T3 按键路由
  → T4 图标与字节展示
  → T5 复制
  → T6 过滤模型
  → T7 过滤输入与 UI
  → T8 删除缓存与扫描竞争
  → T9 删除异步执行
  → T10 帮助、文档与最终验收
```

T4/T5 与过滤模型的业务依赖较少，但按此顺序执行可减少共享文件冲突。每项完成后记录修改文件、定向命令及结果。

### Task 0：建立可比较的验证基线

**Files:** 读取 `Cargo.toml`、`.github/workflows/ci.yml`、上述代码入口；验证 `tests/redis_key_tree.rs`、`tests/redis_browser_tabs.rs`、`tests/redis_scan.rs`。

1. 检查工作区状态及目标文件 diff，记录 Rust 工具链。
2. 执行以下命令，保存已有失败信息：

```bash
cargo +1.94.0 test --test redis_key_tree --test redis_browser_tabs --test redis_scan
```

3. 特别检查 `incremental_insert_preserves_state_and_matches_full_rebuild`：目前一棵树已展开，另一棵未展开却比较 visible_rows。应分别比较结构与状态，或设置相同 expanded 后比较投影，不能用错误测试约定阻止真实修复。
4. 记录 `redis_browser_find_is_tab_local_and_edits_without_reading_a_key` 将因新的过滤语义被替换；其“不逐字符加载预览”的行为仍需保留。

**完成条件：** 基线结果可追溯，未将预先存在的失败描述成本轮引入或已修复。

### Task 1：统一真实 key 与 Prefix 的父子关系

**Files:** Modify `src/model/redis_key_tree.rs`；Test `tests/redis_key_tree.rs`。

1. 为同名 Prefix/Key、折叠、父子导航增加行为回归。可先加入以下完整用例：

```rust
#[test]
fn collapsed_prefix_hides_its_own_key_and_descendants() {
    let mut tree = KeyTreeState::default();
    tree.rebuild(&[key(b"user"), key(b"user:1")]);
    let prefix = KeyTreeNodeId::Prefix(b"user:".to_vec());
    let own_key = KeyTreeNodeId::Key(b"user".to_vec());
    assert_eq!(tree.visible_ids(), vec![prefix.clone()]);
    assert_eq!(tree.parent_of(&own_key), Some(prefix.clone()));
    assert_eq!(tree.first_child(&prefix), Some(own_key.clone()));
    tree.expanded.insert(prefix.clone());
    assert_eq!(
        tree.visible_ids(),
        vec![prefix, own_key, KeyTreeNodeId::Key(b"user:1".to_vec())]
    );
}
```

2. 运行 `cargo +1.94.0 test --test redis_key_tree collapsed_prefix_hides_its_own_key_and_descendants`，确认旧实现暴露问题。
3. 建立唯一的节点遍历规则：先 Prefix，再在展开时输出附属 key_id，随后普通 children。
4. 让 visible_ids 从同一遍历规则派生；修正 parent_of、first_child 和收起后选择回退。
5. 扩展边界：空 key、`a:`、`:a`、`a::b`、非 UTF-8 key；保持原始身份不碰撞。
6. 运行 `cargo +1.94.0 test --test redis_key_tree`，预期所有树身份及增量构建用例通过。

**检查点：** `fix(redis): unify key tree parent and child relationships`。

### Task 2：统一当前展示行、选择与预览入口

**Files:** Modify `src/model/redis_browser.rs`、`src/app.rs`、`src/ui/redis_browser.rs`、`src/input/mouse.rs`；Test `tests/redis_browser_tabs.rs`、`tests/ui_render.rs`。

1. 增加测试：l 进入真实 key 派发一次预览、h 返回 Prefix 清空预览、首尾边界不重复加载、滚动不改选择。
2. 在 Tab 模型增加 `display_rows()` 与基于该投影的 move/parent/child/ensure_visible；普通模式暂时委托树投影。
3. 为后续过滤预留统一行结构，不在 UI 二次排序或做 ID 交集。
4. App 统一处理“目标 ID + 是否显式刷新预览”；expand/collapse、mouse select、scrollbar 和 viewport 更新使用一致规则。
5. 选择真实 key 时核对所属 target 的连接；不要拿与 Tab profile 不匹配的活动连接派发预览。
6. 修正嵌套行展开箭头命中区域：其 x 坐标包含 depth 缩进；文字命中与箭头命中互不抢占。
7. 执行：

```bash
cargo +1.94.0 test --test redis_browser_tabs --test ui_render
```

**完成条件：** 渲染、方向导航、鼠标、滚动使用同一行序；预览身份与选中 key 一致。

**检查点：** `refactor(redis): centralize displayed rows and selection effects`。

### Task 3：合并 Redis keymap，落地 o/h/l

**Files:** Modify `src/input/keymap.rs`、`src/app.rs`；必要时调整 `src/config.rs` 的现有上下文解析；Test `src/input/keymap.rs` 内联测试、`tests/redis_browser_tabs.rs`。

1. 增加 map → update 测试，断言 h/l 留在 Keys 并导航，o 保留 toggle，Left/Right 与 h/l 一致。
2. 删除重复 Redis 路由，收敛到一个上下文入口；保留 overlay/global/pending sequence 的既有优先级。
3. Redis 入口内部按 editing → configured navigation → Keys/Preview 默认操作分派；过滤 editing 应优先于会抢走输入字符的普通导航序列。
4. 配置导航只翻译 Redis 支持的语义，不将不适用的 GridMove/SQL 操作原样放行。
5. 验证 FocusNext/Previous 和窗口方向动作，继续使用原有焦点循环。
6. 覆盖 Shift 输入、Ctrl 修饰键、Preview 无 Keys 操作、Explorer 焦点不被 Redis 抢占。
7. 执行：

```bash
cargo +1.94.0 test --lib input::keymap
cargo +1.94.0 test --test redis_browser_tabs
```

**检查点：** `fix(redis): route vim keys to tree navigation`。

### Task 4：对齐图标颜色并统一字节展示

**Files:** Modify `src/ui/icons.rs`、`src/ui/redis_browser.rs`；Create `src/model/redis_key_text.rs`；Modify `src/model/mod.rs`；按需要复用 `src/ui/mod.rs` 的文本净化接口。

1. 在 IconSet 增加 `redis_key()`，提供 NerdFont 钥匙、Unicode 钥匙、ASCII 文本回退，使用实际存在的字形常量。
2. 文件夹仍调用 `group(ObjectGroup::Tables, expanded)`；图标 span 与 label span 分离。
3. 核对 Explorer Tables 的实际 kind/颜色映射，复用其图标前景色和选中背景规则，不硬编码颜色数值。
4. 新增小型字节展示模块，提供显示/搜索用安全文本，以及复制用文本转换；不要为此抽象全部数据库值格式。
5. 非 UTF-8 字节显示使用全字节转义；有效 UTF-8 展示净化控制字符。搜索与高亮使用同一展示字符串。
6. 在现有渲染用例中检查选中/非选中颜色、窄窗口图标宽度和嵌套点击区域；人工检查三种图标模式。

**验证：** `cargo +1.94.0 test --test ui_render`。字节转换的特殊字符行为随 Task 5 的复制测试覆盖。

**检查点：** `feat(redis): align folder styling and add key icons`。

### Task 5：复制完整 key 到现有剪贴板通路

**Files:** Modify `src/action.rs`、`src/app.rs`、`src/input/keymap.rs`、`src/model/redis_key_text.rs`；Test `tests/redis_browser_tabs.rs`。

1. 增加完整 key、空 key、二进制 key、Prefix、Preview/输入模式的复制用例。
2. 新增 `Action::RedisCopyKey`；从 tree.selected_key 获取完整字节，不读取 row.label。
3. 构造现有 `ClipboardPayload`，返回 `Command::WriteClipboard`；复用现有成功、失败和 OSC52 机制。
4. 有效 UTF-8 原样复制；无效 UTF-8 使用每字节小写 `\\xHH` 并在 description 标注 escaped key。
5. 将裸 y 映射到 Keys 浏览/确认过滤状态，后者与普通状态使用同一个 selected_key。
6. 运行 `cargo +1.94.0 test --test redis_browser_tabs`；断言 `user:profile:123` 的 payload 是完整字符串，而不是 `123`。

**检查点：** `feat(redis): copy full selected key names`。

### Task 6：实现过滤模型与独立展开状态

**Files:** Modify `src/model/redis_browser.rs`、`src/model/redis_key_tree.rs`；引用 `src/db/catalog.rs` matcher；Create `tests/redis_key_filter.rs`。

1. 新增测试：收起的 `user:profile:123` 可被完整路径搜索命中；未匹配分支消失；祖先保留且不计入 matches。
2. 将 Redis find 模型升级为 filter 模型。建议重命名 `RedisKeyFindState` 为 `RedisKeyFilterState`，同时迁移调用；内部 Action 的重命名在 T7 一次完成。
3. 状态包含 phase、TextInput、original selected/scroll、过滤行、匹配 key IDs、过滤展开状态，以及投影版本。
4. 从全部已加载树节点生成过滤结果，不使用 visible_ids 快照。一次遍历获得父关系，避免每个匹配 key 再扫描整棵树找祖先。
5. 使用公共 matcher 对完整 key 的安全显示文本匹配；匹配 key + 祖先形成 included 集合，再按原树顺序生成行。
6. 查询改变时自动展开匹配祖先；确认后 o/h/l 修改 filter 专用 expanded。n/N 使用全体 matched IDs，展开目标路径后定位。
7. 空查询显示普通树；清空查询恢复来源选择；再次 / 编辑保持第一次 open 的恢复快照。
8. 接入 T2 display_rows，让导航、父子关系和滚动在过滤模式下自然使用过滤投影。
9. 扫描批次/删除/刷新触发投影失效并协调：优先保留仍存在的选中 ID，否则下一项、上一项、祖先，最后 None。
10. 查询、key 集合或展开变化时才重建投影；render 仅读取缓存，不每帧重新匹配全部 key。
11. 执行：

```bash
cargo +1.94.0 test --test redis_key_filter --test redis_key_tree
```

**额外验收：** 输入空白、仅冒号、大小写、中文、二进制转义；新增 SCAN 批次进入结果；两个 Tab 的过滤互不影响。

**检查点：** `feat(redis): project filtered keys with ancestor paths`。

### Task 7：过滤输入、UI、鼠标与预览协调

**Files:** Modify `src/action.rs`、`src/app.rs`、`src/input/keymap.rs`、`src/input/mouse.rs`、`src/runtime.rs` 的输入事件转发点、`src/ui/redis_browser.rs`、`src/ui/mod.rs`；Test `tests/redis_key_filter.rs`、`tests/redis_browser_tabs.rs`、`tests/ui_render.rs`。

1. 新增 editing/confirmed/cancel/close 测试和输入期间无删除、无复制、无预览请求的断言。
2. 将 RedisFind 系列入口迁移为 RedisFilter；使用 `TextInputEdit` 等现有通用文本编辑动作，覆盖左右移动、Home/End、删词、清空、撤销和粘贴。
3. 输入和鼠标点击过滤栏均能恢复 editing；缩小窗口后输入光标保持在可见范围，用 TextInput.cursor 和显示宽度计算，不用 chars().count。
4. 布局对齐 Explorer：顶部 `/ query` + 匹配数，中间过滤树，底部 editing/confirmed 帮助。小高度优先保证输入行和有效可见行。
5. 复用 match_spans 或抽取最小公共高亮 helper；高亮实际 label 可见的匹配部分，完整路径命中但叶子片段无命中时不伪造高亮。
6. 使用模型返回的 filtered rows；移除旧 find.rows 与普通 rows 的 HashMap 交集逻辑。
7. 过滤输入期间暂停候选预览加载，显示明确预览状态；确认/已确认跳转走统一选择入口，取消恢复来源预览。
8. 点击 editing 状态的某行定义为确认过滤并选择该行，随后加载预览；点击输入栏只进入编辑。
9. 编辑 Esc 恢复原选择/滚动；确认状态 Esc 保留当前 key，展开普通树必要祖先；若来源 key 已不存在，执行统一回退。
10. 在 Partial/Paused 等状态保留扫描提示，零匹配单独显示，不把它解释为 Redis 空库。
11. 执行：

```bash
cargo +1.94.0 test --test redis_key_filter --test redis_browser_tabs --test ui_render
cargo +1.94.0 test --lib input::keymap
```

**检查点：** `feat(redis): integrate explorer-style key filtering`。

### Task 8：删除缓存一致性与 SCAN 竞争

**Files:** Modify `src/model/keyspace.rs`、`src/db/redis/key_store.rs`、`src/model/redis_browser.rs`；Test `tests/redis_scan.rs`；Create `tests/redis_key_delete.rs`。

1. 为 remove_key 增加状态用例：keys、key_set、key_bytes、store、staged_keys/staged_set/staged_bytes/staged_store、pending_keys 同步，重复删除幂等。
2. 给 KeyStore 增加按字节删除接口，MemoryKeyStore 精确更新 bytes/count；如执行时存在其他实现，同步实现该接口。
3. KeyspaceState 提供一个删除协调方法，外部不逐字段修改私有扫描状态。
4. 采用扫描周期内的临时 tombstone 策略：有效删除成功后屏蔽该 key 进入当前扫描周期，包括 staged/pending 与后续批次；下一次显式 refresh 开启新 generation 并清空旧 tombstone。
5. refresh 后真实重新创建的同名 key 可以重新出现。记录这是一种 loaded-cache 一致性规则，不能把 tombstone 永久保留。
6. 删除与 refresh 交错时：有效连接上的删除成功即应用到仍打开的同 target Tab 的当前缓存及当前扫描周期，不能只因 scan generation 已变就丢弃已发生的删除；失效连接回调按连接 identity 拒绝。
7. 树删除首版采用从剩余 key 重建，保留仍有效 expanded/selected，再应用邻项回退；不在本轮引入复杂树增量删除算法。
8. 在删除发起时记录邻项仅作回退候选；返回时若用户已选其他仍有效节点，保留用户当前选择。
9. 验证 Paused 队列不会重新插入已删 key；保持 COUNT/cursor 和既有缓存上限语义，避免把 Partial 误标记为 CompleteEmpty。
10. 执行：

```bash
cargo +1.94.0 test --test redis_scan --test redis_key_delete --test redis_key_filter
```

**必须覆盖的交错序列：** scan 请求 → 删除成功 → 旧 batch 含该 key；refresh → 删除成功 → 新 batch；删除成功 → refresh → 同名 key 重建；删除失败 → batch 正常保留 key。

**检查点：** `fix(redis): reconcile deleted keys across scan caches`。

### Task 9：删除 Action、Runtime 和 Redis adapter

**Files:** Modify `src/action.rs`、`src/app.rs`、`src/runtime.rs`、`src/input/keymap.rs`、`src/db/redis/types.rs`、`src/db/redis/mod.rs`；Create `src/db/redis/write.rs`；Test `tests/redis_key_delete.rs`、`tests/redis_commands.rs`。

1. 定义删除请求身份，包含有效 ConnectionIdentity、RedisTarget、owner tab ID、独立 mutation request ID 和原始 key；不要占用 SCAN request_id 计数。
2. 增加 RedisDeleteKey、RedisKeyDeleted、RedisKeyDeleteFailed Action，以及 ExecuteRedisKeyDelete Command；名称可按已有命名微调。
3. App 校验 Keys 焦点、非 editing、Key 身份、target 有效；按 `(target, key)` 记录 pending，重复 d 不重复派发。
4. Runtime 使用 target 对应连接和数据库。复用当前 adapter 的按数据库操作方式，避免对被其他并发任务共享的连接随意 SELECT。
5. adapter 使用参数化字节参数执行单 key DEL，返回删除数量；错误转换遵循现有 DatabaseError 习惯。
6. 成功回调先核对连接及请求身份，再对同 target 的仍打开 Tab 应用 T8 缓存协调；owner 已关闭时不重建 Tab、不切焦点。相同 target 若存在其他视图，应同步清理或标记失效。
7. 成功 0/1 均清理；失败解除 pending、保留数据、发送通知。新 key 预览由当前有效选中决定，不由被删除 key 决定。
8. 删除后使相关预览请求/metadata cache 失效；旧预览回调不能再次显示已删除 key。
9. 增加 fake transport/现有测试夹具断言：发送完整原始 key、目标 DB 正确、NOPERM 错误、连接重建、Tab 关闭、重复 d、当前选择已移动。
10. 执行：

```bash
cargo +1.94.0 test --test redis_key_delete --test redis_commands --test redis_browser_tabs
```

**完成条件：** d 从按键到服务端到 UI 闭环；同名 Prefix 保留其他子 key；失败没有乐观移除。

**检查点：** `feat(redis): delete selected keys with identity-safe callbacks`。

### Task 10：帮助、文档、规模回归与验收

**Files:** Modify `src/help.rs`、`docs/redis-browser.md`、`docs/keybindings.md`、`docs/testing/redis-browser-performance.md`；必要时调整 `src/config.rs` 对新增命令的帮助绑定；Test `tests/redis_scale.rs`、现有 keymap/help 测试。

1. 为 Redis Keys、Preview、Filter Editing、Filter Confirmed 提供正确上下文提示；确保帮助中的操作与实际按键一致。
2. 文档明确 o/h/l/d/y、焦点循环、已加载范围过滤、分隔符匹配、二进制复制转义、删除后的刷新语义。
3. 按现有规模测试构造接近当前 10,000 key 上限的数据，确认过滤不会按帧重新扫描、不派发额外 SCAN、不按每个匹配重复全树找祖先。
4. 性能测试使用可比较的操作次数/结构断言；时间测量记入性能文档，避免脆弱的 CI 毫秒阈值。
5. 完成一次完整手工流程：打开 DB → o/l/h → / 输入折叠路径中的 key → Enter → y → d → Esc；检查树、计数、剪贴板、预览与滚动。
6. 手工覆盖中文长 key、窄窗口、三种图标模式、无匹配、Paused、删除失败、复制失败及多 Tab。
7. 运行定向组合，然后按 CI 要求检查：

```bash
cargo +1.94.0 test --test redis_key_tree --test redis_key_filter --test redis_key_delete --test redis_browser_tabs --test redis_scan --test redis_scale --test ui_render
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
git diff --check
```

8. 每条命令记录通过、失败或环境阻塞；不把未运行的真实 Redis 验证写成已通过。全部通过后无需无目的重复运行。

**检查点：** `docs(redis): document key navigation filtering and mutations`。

## 4. 最终验收矩阵

| 场景 | 通过条件 |
| --- | --- |
| o 展开/折叠 | 停留目录；附属真实 key 随目录正确隐藏 |
| h/l 层级导航 | 按两段式展开/进入、折叠/返回工作，焦点保持 Keys |
| 面板切换 | 现有焦点循环仍覆盖 Explorer/Keys/Preview |
| 同名 key/Prefix | user 与 user:1 独立，d 删除 user 不删除 user:1 |
| y 完整复制 | 多层叶子复制完整原文；二进制明确转义 |
| 图标 | Prefix 与 Explorer Tables 一致，Key 为钥匙，各模式可读 |
| 折叠路径过滤 | 不展开普通树也能搜到已加载后代 |
| 过滤视图导航 | 键盘、鼠标、滚动条与 UI 的行 ID 完全一致 |
| 编辑/确认/取消 | 不误执行快捷键，不逐字符请求预览，状态可恢复 |
| 扫描期间过滤 | 新批次加入结果，不重置仍有效选择 |
| 删除成功/失败 | 成功同步计数、树和预览；失败保留数据 |
| 删除与旧 batch | 本周期已删 key 不被缓存回插；刷新可发现重新创建的 key |
| 异步身份 | 重连、切 DB、关 Tab 后无错误目标更新或焦点抢占 |
| 空及小窗口 | 无匹配、空库、Partial/Paused 区分明确；输入与树不互相覆盖 |

## 5. 交付记录模板

每个 Task 完成后记录：

- 状态：未开始 / 进行中 / 已验证 / 阻塞。
- 修改文件及主要行为。
- 实际验证命令及结果。
- 与计划差异及原因。
- 后续依赖是否已满足。

最终交付包括实现代码、必要回归测试、更新后的 Redis/快捷键文档，以及上述验收矩阵的实际结果。
