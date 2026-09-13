# Redis Keys Interaction Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
>
> 若该技能不可用，按本文依赖顺序执行。每项先验证行为缺陷，再实现、运行定向测试并复核差异；未验证不得标记完成。本文不授权 Git commit、push、合并或启动子代理。新增符号与测试名为拟定接口。

**Goal:** Explorer 使用与新连接表单一致的数据库产品图标与颜色；Redis DB 回车后明确聚焦 Keys，支持与 Explorer 一致的树导航、滚动及 `/` 本地搜索。

**Architecture:** 保留全局 Focus，在 RedisBrowserTab 内新增 Keys/Preview 局部焦点；Redis 面板输入优先于通用 Results 表格操作。树导航、搜索、渲染和鼠标共用按稳定节点身份生成的可见行投影；产品图标与颜色收敛到公共 UI 接口，搜索状态按 Tab 隔离。

**Tech Stack:** Rust 2024、Ratatui/Crossterm、现有 Action/App/Command/Runtime、TextInput、KeyBindings 和 TestBackend；无需新增生产依赖。

---

## 1. 执行位置与范围

- worktree：`/Users/yelog/workspace/tui/lazydb-redis-support`。
- 分支：`task/redis-support`。
- 当前有 Redis 浏览和连接分类等未提交改动。开始前检查 `git status --short --branch` 和目标文件 diff，保留用户及其他任务的已有工作。
- 本轮只完成图标一致性、焦点、树导航、本地搜索及必要生命周期修复。不要扩展 Redis 写入、Cluster、全库搜索、连接分类或重写所有 TUI 树组件。
- 已有全仓测试曾在 `tests/keymap.rs::relation_help_executes_space_tc_transaction_control` 失败。应记录基线，未经同条件对照不能宣称该失败与本次无关。

## 2. 已确认的代码原因

| 位置 | 当前事实 | 实施要求 |
| --- | --- | --- |
| `src/ui/mod.rs::explorer_list_item` | 数据库 glyph 使用 icons.database，前景色固定 theme.action | 与表单共享产品色及主题退化规则 |
| `src/ui/profiles.rs::driver_icon_color` | 产品色独立且私有 | 抽到公共 UI 接口，不把 Ratatui Color 放入 db descriptor |
| `src/model/workspace.rs::visible_rows` | Redis DB 行设置 profile_kind 与 connection_status | 产品图标与连接根状态分离，DB 不复制在线圆点 |
| `src/app.rs::open_redis_browser` | 已设置 Focus::Results | 不能只再加一次赋值；需局部焦点、视觉反馈及异步 intent 校验 |
| `src/model/redis_browser.rs` | 无局部焦点/搜索/viewport 状态 | 新增 Tab 局部状态，恢复时默认 Keys |
| `src/ui/redis_browser.rs` | 普通边框，无选中行反馈；独立递归绘制 | 使用焦点样式和共享可见行窗口 |
| `src/input/keymap.rs` | map_configured_navigation 先于 RedisBrowser 分支 | 阻止 Redis Keys 被 GridMove、SQL 分页等抢先匹配 |
| `src/input/keymap.rs::map_configured_navigation` | Results 且非 Relation 即走表格快捷键 | 收紧表格适用范围，不再按“不是 Relation”推断 |
| `src/model/redis_key_tree.rs::move_selection` | 无选择时从 0 再加 delta | 首次向下选择第一项，避免跳过第一行 |
| `RedisToggleSelection` | l/Right/Enter 都是 Toggle | Expand/Collapse/Primary 分离 |
| `KeyTreeNode.key_id` | 可存额外真实 key，visible_ids/UI 不完整投影 | 同名 Key 与 Prefix 都必须形成可导航行 |
| `ExplorerFindState` | 已有输入、匹配、原选择和滚动恢复语义 | 复用行为及 TextInput，不直接调用全局 ExplorerFind Action |

## 3. 行为契约

### 3.1 产品图标

1. New/Edit Connection、Explorer profile 根、Redis DB 行使用同一产品 glyph 和前景色。
2. Redis 图标颜色与表单保持一致；PostgreSQL/MySQL/Oracle 等同样使用统一接口，避免只对 Redis 硬编码。
3. 选中行只改变背景/标签样式，保留可辨认的产品色；busy/禁用和无颜色主题按同一规则降级。
4. Online/Offline 标记仅表达连接状态，与产品图标无关。Redis DB 行不显示“每个 DB 一个在线连接”的圆点。
5. Tab 选中时如产品色与背景冲突，可使用对比色，但必须通过共享接口表达 selected-tab 状态，不另建随机颜色表。
6. NerdFont/Unicode/ASCII 均使用已有 IconSet 映射，不强制用户安装新字体。

### 3.2 焦点

新增 `RedisBrowserFocus::{Keys, Preview}`，组合规则：

| 全局 Focus | Tab 局部 Focus | 实际输入目标 |
| --- | --- | --- |
| Explorer | 任意 | 全局连接 Explorer |
| Results | Keys | 当前 Redis Key 树 |
| Results | Preview | 当前 Key 预览 |

- DB 行 Enter 打开/激活对应 Tab，成功后全局 Results、局部 Keys。
- 若需连接，保留一个带目标/请求身份的打开意图；成功只兑现仍有效意图。取消、目标变更或后来明确聚焦其他面板后，旧回调不得抢焦点。
- 请求连接失败，焦点留在可重试位置，不进入假在线 Keys。
- 从 DB 行再次进入已有 Tab，定位 Keys但保留树选择、展开、滚动和已存在预览；新 Tab 的 Preview 为空。
- 普通 Tab 栏切换恢复该 Tab 上次局部焦点；与 DB Enter 的显式“进入 Keys”意图区分。
- Keys/Preview 空白区域也能点击聚焦；点击空白不清空树选择、不发 Redis 请求。
- 窗格循环为 Explorer→Keys→Preview→Explorer，反向相反。方向型窗口快捷键遵循实际左右位置；h/l 留给树导航。
- 初次进入空树保持无选择；首批结果可建立第一行导航高亮，但不触发预览。用户随后移动/确认真实 key 才读取。
- 焦点明确显示：活动面板边框强调；活动选中行高亮，失焦时保留弱选择。Preview 空状态遵守原目标保持无内容，不依赖提示文本代替焦点反馈。

### 3.3 树导航

| 按键/语义 | 行为 |
| --- | --- |
| j/k、Down/Up | 当前可见行移动；未选择时 Down 落第一项，Up 落最后项 |
| l/Right/Expand | 折叠 Prefix 展开；已展开则进入第一个可见子节点；Key 不折叠父级 |
| h/Left/Collapse | 展开的 Prefix 折叠；否则定位父节点；根级无父节点为 no-op |
| Enter/Primary | Prefix 切换展开；Key 选择/显式预览；无行不执行 |
| gg/G、Home/End | 首/末可见行 |
| PageUp/PageDown、半页 | 按实测 viewport 高度移动，选中项保持可见 |
| 对齐操作（沿用 Explorer 已有绑定） | 顶/中/底对齐选中项，不改变选择身份 |
| 鼠标滚轮 | 仅滚动对应面板，不发送通用 GridScrollRows |

- 点击 Prefix 标签只选择、清空预览；点击展开箭头或双击 Prefix 才 Toggle。避免把选择与展开混为一个动作。
- Key 和 Prefix 是不同节点类型。固定采用同级区分：`user [key]` 与 `user:`，两者都有稳定 ID 和独立命中行。
- a::b、:a、a:、空 key、二进制 key 均保留原始字节身份；标签转义不能影响请求 key。
- 开闭节点、继续 SCAN 导致排序变化时按 ID 恢复选择，不按旧行号猜测。
- 撤销折叠后隐藏的子选择时，选择回到折叠的 Prefix，清掉不再对应选择的 pending preview。
- Prefix 展开不产生 SCAN；本轮只导航已加载内容，不改变现有服务端扫描语义。

### 3.4 `/` 本地搜索

首版与 Explorer 本地 Find 保持一致：搜索打开时的**已加载且当前可见**行快照，不展开未加载节点，不执行 Redis 命令。

1. Keys 中 `/` 打开底部输入，预留一行；快照保存 row IDs、标签、原选择和原滚动。
2. 编辑阶段字符键全部归 TextInput：j/k/h/l/n 是文本；支持 paste、Backspace/Delete、光标、清空、undo/redo。
3. 输入同步计算匹配位置、高亮当前匹配，并按 Explorer 的居中逻辑调整 viewport。
4. 搜索匹配为现有本地标识符匹配规则；复用项目已经使用的纯匹配函数，先测试其对转义标签/分隔符的行为，不将展示匹配当 key 字节身份。
5. Enter 确认并返回 Keys 导航；确认项为真实 Key 时按正常预览机制请求，Prefix 则为空。
6. 编辑阶段 Esc 取消并恢复原选择/滚动；取消本身不发 Redis 请求。原预览可保持，因为编辑阶段不请求预览。
7. 已确认搜索 n/N 循环匹配并保证可见；真实 Key 按选择规则更新预览。无匹配无副作用，显示 0 matches。
8. 已确认状态 Esc 清除搜索及高亮，保留当前选择；再次 `/` 建立新快照。
9. 继续 SCAN 不悄悄改变正在编辑的快照；刷新、MATCH 变更、目标失效或 Tab 关闭使旧搜索失效。
10. 普通切换 Tab 保留各自搜索状态；非活动 Tab 的输入不被更新。窗口导航离开正在编辑的搜索时，先按取消规则收束输入再切焦点，避免键盘路由悬挂。
11. 搜索期间仅定位和高亮，不执行预览。不能在每个输入字符上读一个新 Key。

`/` 和服务端 MATCH 始终分开：前者本地、不产生网络请求；后者属于扫描过滤，不在本轮改为实时全库搜索。

## 4. 数据与接口设计

### 4.1 共享可见树行

建议新增 `KeyTreeRow`，至少含：

```text
id: KeyTreeNodeId
parent: Option<KeyTreeNodeId>
depth: usize
label: 转义后的展示标签（或可生成标签的数据）
kind: Prefix | Key
expanded: bool
```

- `KeyTreeState::visible_rows()` 是唯一展开投影；键盘、搜索、渲染、鼠标消费同一结果。
- 维护 selected ID、scroll、viewport height；使用 viewport change Action 同步实测容量，不在 render 内改变 App 状态。
- 树内容/展开修订改变时更新缓存，viewport 单独裁剪；普通每帧绘制不能递归重建所有 key 节点。
- 只遍历展开节点生成可见行；搜索快照复制必要 ID/标签，不复制 value。
- 选择合法性区分 contains 与 visible：存在但折叠隐藏的 key 不能被旧 hit map 直接选中。
- 限制最大展示深度/节点数，与现有 key 字节预算一起保护超多冒号等情况。

### 4.2 Tab 状态

在 RedisBrowserTab 增加：

```text
focus: RedisBrowserFocus
find: Option<RedisKeyFindState>
tree viewport / scroll / navigation selection
preview viewport / scroll（按当前文本预览实现，不预先引入 DataGrid）
```

RedisKeyFindState 保存 Editing/Confirmed、TextInput、行快照、matches、current、original selection/scroll、扫描轮次修订。

不直接持有 `ExplorerFindState`，因为它固定使用 ExplorerNodeId 且绑定全局连接树；优先复用 TextInput、匹配与 viewport 的纯算法，不在本轮泛型化整个 Explorer。

### 4.3 语义动作

拟定新增或拆分：

- RedisFocus(Keys/Preview)、RedisTreeViewportChanged、RedisPreviewScroll。
- RedisExpand、RedisCollapse、RedisPrimary、RedisSelectTarget、RedisScrollRows、RedisAlignSelection。
- RedisFindOpen、RedisFindEdit（可复用 TextInputEdit）、RedisFindPaste、RedisFindConfirm、RedisFindCancel、RedisFindNext/Previous、RedisFindClear。
- RedisSelectNode 与 RedisToggleNode 分离；所有鼠标动作带 Tab UUID，必要时带树修订，失效事件忽略。

请求结果不改变 focus，只有明确用户动作及仍有效的打开 intent 可以改变。预览请求继续使用独立 preview generation，搜索/Prefix 选择使旧预览请求失效，不能仅比较 key 字符串。

### 4.4 输入优先级

```text
应用模态窗口/退出等最高优先动作
  → 当前 Tab 的 Keys 搜索输入（Editing）
  → 窗口/Tab 全局操作
  → Redis 局部导航（Keys 或 Preview）
  → 实际 SQL/Relation/Dashboard 专属操作
```

- 审计 `map_configured_navigation`、提前处理分页/复制/只读编辑的入口，不能只移动末尾 Redis match。
- 复用 Explorer 导航按键配置，翻译为 Redis Action；不要把 explorer-new-profile、Catalog mutation、全局搜索直接带进 Keys。
- 通用 Results/Grid 路由限定到真正拥有相应 grid 的 Tab/面板。Redis Preview 目前为文本，不能当成 SQL grid。
- 搜索 confirmed 时 n/N 导航；Editing 时 n/N 是普通字符。
- Preview 中 / 不隐式修改 Keys 搜索；本轮不新增预览文本搜索，可保持 no-op 并在帮助中不宣传。

## 5. 逐项实施任务

### Task 1：基线与可复现缺陷测试

**Files:** 新增 `tests/redis_keys_navigation.rs`、`tests/redis_keys_ui.rs`；复用 `tests/redis_browser_tabs.rs`、`tests/keymap.rs` 的 setup。

1. 检查 worktree 状态和相关 diff，记录基准提交、当前失败及 Redis 连接任务是否另有改动。
2. 使用合法 Redis profile、真实形状的 DB 节点及已接受的扫描批次建立 fixture；不通过不存在 profile 的伪 Tab 绕过目标校验。
3. 在 fixture 中发送 DB Enter，断言 active Tab/全局 focus，并绘制 TestBackend 检查 Keys 焦点边框和导航高亮。
4. 使用真实 Keymap 输入 j/k/l/h，断言生成 Redis 导航 Action 而不是 GridMove；测试默认绑定和自定义绑定。
5. 对表单与 Explorer 同产品图标读取 buffer 前景色，建立当前不一致的失败断言。
6. 执行 `cargo test --test redis_keys_navigation --test redis_keys_ui`，记录预期失败。接口尚不存在时先记录编译失败。

**复核：** 测试确实覆盖 Keymap→App→UI，不只是直接调用 RedisMoveSelection；失败原因与截图问题一致。

### Task 2：统一产品图标与颜色

**Files:** 修改 `src/ui/icons.rs`、`src/ui/profiles.rs`、`src/ui/mod.rs`、`src/model/workspace.rs`；扩展 `tests/redis_keys_ui.rs`、`tests/ui_render.rs`。

1. 将 driver_icon_color 迁移为 UI 公共接口，输入产品、主题和展示状态；复用现有 IconSet glyph。
2. 新连接 Driver 列表、Explorer 正常行、Explorer Find/Search 投影都调用同一接口；避免只修 normal render。
3. Redis DB 行仅提供产品图标身份和 DB metadata，不填连接根的 connection_status/endpoint；避免因此显示重复在线圆点。
4. 渲染时保留选中背景；plain/no-color、busy、失焦主题统一处理。
5. 对 PostgreSQL、Oracle、Redis 验证表单与 Explorer glyph/颜色一致，三种 IconMode 均可渲染。
6. 执行 `cargo test --test redis_keys_ui --test ui_render`。

**复核：** 图标与颜色只有一个权威 UI 来源；DB 行不是伪连接根；不会把 Color 类型引入 db/profile 模块。

### Task 3：统一树投影与身份，修复同名节点

**Files:** 修改 `src/model/redis_key_tree.rs`、`src/model/redis_browser.rs`；扩展 `tests/redis_key_tree.rs`；新增 `tests/redis_keys_viewport.rs`。

1. 建立 Prefix/Key 独立可见行，禁止用同一个 label map entry 合并两个节点身份。修复插入顺序 user→user:1 与反序产生不同结果的问题。
2. 实现 visible_rows、parent lookup、contains/visible validation，移除 UI 独立递归定义的行序。
3. 覆盖空 key、空段、尾冒号、非法 UTF-8、ANSI、相同显示标签，以及同名 Prefix/Key 都可选。
4. 增加树修订和可见投影缓存/失效规则；选择和展开按 ID 保留，删除后沿父链回退。
5. 增加 scroll/viewport height，实现选中项 ensure_visible、分页和顶中底对齐；零容量/空树/末页边界不溢出。
6. 最大展示深度后合并 suffix，保留原始完整 key；防止递归栈和前缀节点失控。
7. 执行 `cargo test --test redis_key_tree --test redis_keys_viewport --test redis_scan`。

**复核：** 每一个可访问 key 都有唯一可见行；键盘、鼠标、搜索不再各自推断索引；不改变 SCAN 契约。

### Task 4：Redis 局部焦点与 DB Enter 打开意图

**Files:** 修改 `src/model/redis_browser.rs`、`src/app.rs`、`src/action.rs`、`src/ui/layout.rs`；扩展 `tests/redis_browser_tabs.rs`、`tests/redis_keys_navigation.rs`。

1. 新增 RedisBrowserFocus，new/restored 默认 Keys，普通 Tab 切换保留局部焦点。
2. 将 DB Enter 与普通 ActivateTab 的意图区分：前者明确 Keys，后者恢复局部焦点；统一 next/previous Tab 路径。
3. 修改 open_redis_browser：同目标已有 Tab 只激活并聚焦；初次加载第一批后建立导航选择但保持 Preview Empty。
4. pending open intent 绑定目标和 connection attempt；连接成功只兑现匹配 intent，失败/取消/另一次打开/用户后续焦点动作使旧 intent 不再抢焦点。
5. 窗格循环及方向切换处理 Explorer/Keys/Preview，normalize_focus 不把 Redis 局部状态丢掉。
6. 请求 SCAN/preview 成功或失败仅更新数据，不把 focus 重设为 Explorer 或 Results。
7. 执行 `cargo test --test redis_browser_tabs --test redis_keys_navigation --test workspace_tabs --test connection_switch`。

**复核：** DB 回车立即进入可用 Keys 或在合法建连成功后进入；没有焦点被迟到结果夺回；初始预览仍为空。

### Task 5：修正输入路由并实现 Explorer 风格导航

**Files:** 修改 `src/input/keymap.rs`、`src/action.rs`、`src/app.rs`、`src/model/redis_key_tree.rs`；扩展 `tests/redis_keys_navigation.rs`、`tests/keymap.rs`。

1. Redis Keys 路由放在通用 Results 操作之前，收紧 map_configured_navigation 的 grid/pagination 条件。
2. 提取/复用 Explorer 导航绑定解析结果，翻译为 Redis 专用 Action；排除创建连接、DDL、Catalog 服务器搜索等无关动作。
3. 实现 Expand/Collapse/Primary，不再把 l/Right/Enter 全部映射 Toggle。
4. 实现首次选择、j/k、h/l、gg/G、Home/End、整页/半页及对齐，按共享 visible_rows 移动。
5. 选择逻辑区分初始化导航、搜索预览选择和明确 Key 选择；只有后者或确认搜索触发 key preview。
6. 在树上按表格复制/SQL query bar/事务相关快捷键不会误入其他 Tab 能力。
7. 执行 `cargo test --test redis_keys_navigation`；再执行 `cargo test --test keymap`，失败需与 Task 1 基线对照。

**复核：** 按键通过真实 Keymap 生效，自定义 Explorer 导航绑定在 Keys 同样可用；没有通用 GridMove 抢占。

### Task 6：Keys/Preview 焦点渲染、鼠标与滚动

**Files:** 修改 `src/ui/redis_browser.rs`、`src/ui/mod.rs`、`src/input/mouse.rs`、`src/model/redis_browser.rs`；扩展 `tests/redis_keys_ui.rs`、`tests/redis_keys_viewport.rs`、`tests/mouse.rs`。

1. 渲染消费 visible_rows 的 viewport，移除另一套递归绘制顺序；树容量通过 viewport Action 同步。
2. 使用现有主题 panel 样式绘制活动 Keys/Preview 边框和选中行，不硬编码白框/红字。
3. 初次 SCAN 后第一行导航高亮可见；失焦选择为弱高亮；Preview Empty 保持空内容。
4. 为 Keys/Preview 整体、展开箭头、节点行分别注册命中：空白聚焦、标签选择、箭头 Toggle；命中先后优先级明确。
5. hover 不改 focus；滚轮按目标面板滚动；树滚动不发预览，选择导航才走相应选择语义。
6. 为 Preview 文本建立独立滚动 offset 和 viewport 裁剪，不改造成 SQL ResultSet；净化控制字符，保留原始复制内容边界。
7. 80×24/56×16 等窄屏在当前局部焦点展示单面板，窗口命令可切到另一个面板；极小终端沿用 fallback。
8. 执行 `cargo test --test redis_keys_ui --test redis_keys_viewport --test mouse --test ui_render`。

**复核：** 实际焦点与边框/高亮一致；远端 key 不能注入终端控制；鼠标与键盘访问相同行且不会操作旧 Tab。

### Task 7：实现 Redis Keys 本地 Find 状态机

**Files:** 新增 `src/model/redis_key_find.rs`；修改 `src/model/mod.rs`、`src/model/redis_browser.rs`；新增 `tests/redis_keys_find.rs`。

1. 先为 Editing/Confirmed、输入、匹配、取消恢复、确认保持、n/N wrap 编写纯状态测试。
2. 使用 TextInput 与已有匹配函数，定义搜索快照包含 ID/label/扫描修订，不能直接复用固定 ExplorerNodeId 的 ExplorerFindState。
3. 匹配输入仅变更临时导航/highlight，不调用 Tab::select 中会触发 preview 的完整逻辑。
4. 确认返回一个待选择节点意图，交由 App 判断是否真实 Key；取消恢复原选择/scroll。
5. 空查询/无匹配/全部行/Unicode 和转义标签行为明确；当前匹配位置按 ID 跟踪。
6. SCAN 继续不改 Editing 快照；刷新/过滤/目标变更使旧快照无效，不能恢复到不存在的节点。
7. 执行 `cargo test --test redis_keys_find --test redis_key_tree --test redis_keys_viewport`。

**复核：** 不访问 Redis、不读取 value；与 Explorer 本地 Find 的确认和取消语义一致。

### Task 8：接入 `/` 输入、n/N 与搜索栏

**Files:** 修改 `src/action.rs`、`src/app.rs`、`src/input/keymap.rs`、`src/input/mouse.rs`、`src/ui/redis_browser.rs`、`src/ui/mod.rs`；扩展 `tests/redis_keys_find.rs`、`tests/redis_keys_navigation.rs`、`tests/redis_keys_ui.rs`。

1. `/` 在 Keys 打开 Find；Preview 和全局 Explorer 各自保留对应上下文，不能误修改 Keys。
2. Editing 搜索路由置于树导航前，字符和 paste/undo/redo 使用统一 TextInput 操作；全局退出/窗口切换按约定收束搜索。
3. 底部输入栏显示 `/query`、匹配计数和阶段提示，占用一行后重新计算树 viewport；光标仅在该栏显示。
4. 高亮真实匹配字符，终端 cell width 和 UTF-8 byte offset 映射正确，不破坏 key 的转义展示。
5. Enter/取消/n/N 调用 Task 7 状态机；确认或跳转真实 Key 后复用 preview 请求代次保护，Prefix 清空 preview。
6. 使用假 Runtime/命令收集器断言输入任意搜索字符时不出现 ScanRedisKeys/LoadRedisPreview；仅确认/跳转真实 Key 可产生正常 preview Command。
7. 测试两个 Tab 各自不同查询、切换回来恢复、关闭后输入迟到、refresh 后确认旧搜索等边界。
8. 执行 `cargo test --test redis_keys_find --test redis_keys_navigation --test redis_keys_ui --test mouse`。

**复核：** /→输入→Enter→n/N→Esc 的完整物理按键链可用；搜索不被 j/k/l 或通用 Results 快捷键抢占。

### Task 9：生命周期、帮助与端到端交付

**Files:** 修改 `src/help.rs`、`src/ui/shortcut_hints.rs`、`src/app.rs`、`src/persistence/workspace.rs`（只在必要时）、`docs/keybindings.md`、`docs/redis.md`（若存在）；新增 `tests/redis_keys_flow.rs`。

1. Redis Keys/Find Editing/Find Confirmed/Preview 有准确帮助上下文和 footer，显示真正可用的操作。
2. Tab 切换、关闭、profile 断开/删除、刷新令 pending 搜索或 preview intent 正确失效；后台事件不改变焦点。
3. 搜索快照、连接身份和 transient focus intent 不持久化；恢复 Tab 默认 Keys 和空 preview，旧 workspace 向后兼容。本轮不必保存临时搜索查询。
4. 端到端测试用合法 profile、DB 发现回复、扫描批次，经 Keymap Enter→Keys→l→j→预览→/→搜索→n/N→窗口切换，断言 Action、状态和 TestBackend 输出三者一致。
5. 对异步 DB 打开增加匹配成功、失败、取消、过期成功和后续用户焦点变更案例。
6. 性能 fixture 使用 10,000 个已加载 key：折叠树只投影展开节点；搜索只处理快照；渲染只处理 viewport，不在每帧重新加载/解析全库。
7. 执行 `cargo test --test redis_keys_flow --test redis_browser_tabs --test workspace_tabs --test workspace_persistence`。
8. 最终执行 `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo check --all-targets`、`cargo test`。记录失败具体目标及对照，不把 lib 测试数当全仓总数。
9. 复核所有新增文件和目标 diff；通过后 `cargo build`，输出本 worktree binary 路径供用户验证。未明确要求时不提交。

**复核：** 用户可在 DB 回车后立即看见 Keys 焦点，按 Explorer 惯例导航并 / 搜索；图标颜色一致；不是仅模型测试通过。

## 6. 依赖与交付门槛

```text
Task 1 基线
  ├→ Task 2 图标统一
  └→ Task 3 树投影 → Task 4 局部焦点 → Task 5 路由/导航 → Task 6 视觉/鼠标
                                                        ↓
                                      Task 7 Find 模型 → Task 8 搜索接入
                                                        ↓
                                                   Task 9 交付
```

- Gate A：表单与 Explorer 图标颜色一致；DB Enter 获得可见 Keys 焦点。
- Gate B：真实键盘/鼠标可以展开、折叠、回父级、滚动，默认不自动读取首 Key。
- Gate C：/ 本地搜索确认、取消、n/N 正确，无无意的网络扫描，跨 Tab 隔离。
- Gate D：帮助、生命周期、回归和构建验证完成。

## 7. 最终验收清单

- [ ] Redis/其他数据库 glyph 和产品色在新连接与 Explorer 一致。
- [ ] DB 行没有冗余连接状态圆点。
- [ ] DB Enter 打开/复用 Tab 后焦点明确在 Keys，边框和行高亮可见。
- [ ] 迟到建连/SCAN/preview 回复不会抢焦点。
- [ ] 首批 key 建立导航落点但 Preview 仍为空。
- [ ] 默认及自定义 j/k/h/l 路由到 Redis 树而非 GridMove。
- [ ] Expand、Collapse、Primary 语义独立，已展开节点按 l 进入子节点。
- [ ] 同名 Key/Prefix、空段、二进制 key 都有稳定可访问行。
- [ ] 键盘、鼠标、搜索、渲染使用同一 visible_rows 与 viewport。
- [ ] / 输入时 j/k/n 等属于文本，Enter/Esc/n/N 行为与 Explorer 一致。
- [ ] 搜索编辑不发 Redis 请求，服务端 MATCH 没有被隐式改变。
- [ ] Explorer/Keys/Preview 窗格切换、窄终端、滚轮正确。
- [ ] 各 Tab 搜索、选择、焦点和滚动独立。
- [ ] 帮助提示与实际可执行操作一致。
- [ ] 定向及端到端测试有真实执行记录，未把未完成内容标记完成。

本文件为实施计划；编写时仅新增文档，没有修改应用实现或运行功能测试。
