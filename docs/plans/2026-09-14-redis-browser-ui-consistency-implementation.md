# Redis Browser UI Consistency Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 若执行环境没有该技能，按下列任务顺序实施、验证并记录结果。

**Goal:** 修复 Redis 数据库打开后的默认选择和 tab 名称，使 Keys / Preview 的面板、滚动条、树图标和选中交互与 SQL Editor / Explorer 一致。

**Architecture:** 保留 Action → App reducer → Command → Runtime 的数据流以及 RedisTarget 绑定。选择状态在 reducer 中更新，render 只计算布局并绘制；复用公共 Theme、IconSet、panel_block 与 scrollbar geometry，提取必要的无业务依赖绘制函数。Keys 和 Preview 各自维护二维滚动状态，视口指标由 UI 回传 Runtime，再通过 Action 同步。

**Tech Stack:** Rust 1.94 / Edition 2024、Ratatui 0.30.2、Crossterm 0.29、unicode-width 0.2、现有 Rust 单元测试和集成测试。

---

## 1. 行为契约

### 1.1 打开与选择

- Explorer 对 db0（以及任意 Redis DB）回车后，激活对应 target 的 tab，并聚焦 Keys。
- 首个产生可见节点的有效扫描批次完成后，若尚无有效选择，选中树排序下的第一个顶层节点。
- 例如 `app:config`、`user:1`、`version`，默认选中 `Prefix(app:)`；不自动展开并深入叶子。
- 空批次不产生虚假选择；后续非空批次仍能初始化选择。
- 已有有效选择时，后续批次、重复打开均保留该节点，不强制跳回第一行。
- 默认节点是叶子时，沿用统一选择入口加载预览；是文件夹时清空预览。
- 异步返回可以更新所属 tab 的数据和默认选择，但不能抢占当前 tab 或全局焦点。
- 旧 identity 的扫描结果不改变树、选择和预览；预览请求必须使用所属 target 的有效连接。

### 1.2 标题与焦点

- tab 显示 `Redis @连接名`，连接名来自 `tab.target.profile_id` 对应 profile。
- profile 不存在时显示 `Redis @失效目标`；重命名后下一次绘制更新。
- DB 编号继续显示在 Keys 标题中。
- pane 激活条件：Redis 工作区拥有全局焦点，且内部 focus 指向该 pane；全局焦点回到 Explorer 后两个 pane 都显示非激活边框。
- 选中行在失焦后仍可辨认，焦点归属用 pane 边框表达。

### 1.3 按键契约

| 上下文 | 按键 | 行为 |
|---|---|---|
| Keys 导航 | `o` / Enter | 文件夹展开或折叠并停留原节点；叶子执行现有预览操作 |
| Keys 导航 | `→` | 未展开文件夹仅展开；已展开文件夹进入第一个可见子节点 |
| Keys 导航 | `←` | 已展开文件夹折叠；否则选择父节点 |
| Redis 工作区 | `h` / `l` | 保留当前 Keys / Preview pane 切换语义 |
| Keys 导航 | `j/k`、`↑/↓` | 移动树选择，保持可见并同步预览 |
| Preview 导航 | `j/k`、`↑/↓` | 纵向滚动预览 |
| Preview 导航 | `←/→` | 横向滚动预览 |
| 两个 pane | PageUp / PageDown | 按当前 pane 的有效可见高度翻页，最少 1 行 |
| 查找输入 | `o/h/j/k/l` | 输入字符，输入处理优先于导航 |

修饰键和用户配置遵守现有 keymap 优先级；只让裸 `o` 成为新增默认快捷键。Keys 横向滚动使用鼠标横向滚动和底部滚动条，避免与树方向键冲突。

### 1.4 样式与滚动

- 面板使用圆角、theme.title、theme.accent / theme.border、theme.surface。
- 选中行使用 theme.selection 背景、theme.accent 标签和加粗；普通行使用 theme.surface / theme.text。
- Prefix 图标复用 IconSet::group(ObjectGroup::Tables, expanded)，遵循 Nerd Font / Unicode / ASCII 配置。
- 纵向滚动条使用 `▲ │ ┃ ▼`；横向使用 `‹ ─ ━ ›`；颜色与 SQL Editor 相同。
- Keys / Preview 各自保持横纵偏移；切换 key 后预览滚动重置。
- 只有溢出的轴显示滚动条，窄窗口下不覆盖角点、标题、查找行或状态行。
- 纯滚动不更改选中 key，也不触发预览请求；键盘移动选择时再确保选中项可见。
- Preview 的滚动仅浏览当前已加载内容，不隐式请求 Redis 后续页。

## 2. 当前代码定位与实施注意点

行号随其他工作变化，实施以符号为准。

| 问题 | 当前入口 | 修改方向 |
|---|---|---|
| 默认未选择 | src/app.rs::RedisKeysLoaded、open_redis_browser | 树更新后协调选择，显式打开恢复 Keys 焦点 |
| 硬编码连接名 | src/ui/mod.rs::render_tabs | RedisBrowser 分支按 target.profile_id 查 profile |
| Redis 硬编码边框和颜色 | src/ui/redis_browser.rs::render、panel_style | 注入 Theme / IconSet，调用 panel_block |
| 缺少 o；左右键被提前拦截 | src/input/keymap.rs Redis 分支 | 查找处理后按 pane 分派按键 |
| expand 实际 toggle 且跳子节点 | src/app.rs::expand_redis_selection | 区分 expand 与 toggle |
| 缺少图标与一致选中样式 | src/ui/redis_browser.rs::render_row | 公共树样式、Span 分层、整行背景 |
| 深层箭头热区不跟随缩进 | src/ui/redis_browser.rs::render_row | 统一布局与 hit region 坐标 |
| 仅 Keys 纵向状态 | src/model/redis_browser.rs::RedisBrowserTab | 独立 pane 滚动状态与指标 |
| viewport 上报过早 | src/ui/redis_browser.rs::render | 在扣除状态行、查找行后上报实际 row_area |
| 指标同步仅比较高度 | src/runtime.rs::sync_redis_keys_viewport | 比较两 pane 的实际可见与内容尺寸 |
| SQL 滚动条混合绘制与业务 hit target | src/ui/mod.rs::render_editor_scrollbars | 只提取公共绘制，业务事件留在调用方 |

已有 `docs/plans/2026-09-14-redis-browser-optimization-implementation.md` 是大键空间、扫描和分页计划。本计划单独跟踪本次 UI 一致性需求，集成时确认其中是否已有更新后的键窗口和 viewport API。

## 3. 实施任务

### T01：确认基线与回归入口

**文件：** 阅读 `tests/redis_loading_lifecycle.rs`、`tests/redis_browser_tabs.rs`、`tests/redis_key_tree.rs`、`tests/ui_render.rs`、`tests/mouse.rs`、`.github/workflows/ci.yml`；更新本计划的执行记录。

1. 执行 `git status --short`，记录已有变更；按当前符号重新核实本计划涉及的入口。
2. 运行 `cargo test --test redis_loading_lifecycle --test redis_browser_tabs --test redis_key_tree`，记录基线。
3. 运行 `cargo test --test ui_render redis`，确认既有 Redis 渲染断言。
4. 阅读 mouse 手势取消、hit target 优先级及 viewport 同步的当前实现，确认后续扩展位置。

**完成标准：** 已知失败和本任务引入的失败可区分；每项行为都有明确测试落点。

### T02：补齐首次选择、重新打开与预览一致性

**修改：** `src/app.rs`、`src/model/redis_browser.rs`；必要时调整 `src/model/redis_key_tree.rs` 的选择协调方法。
**测试：** `tests/redis_loading_lifecycle.rs`、`tests/redis_browser_tabs.rs`。

1. 添加行为回归：初始树为空，输入乱序批次，检查选中排序后的第一根节点；验证 Prefix 不发预览命令、顶层 Key 发预览命令。
2. 添加空批次后非空批次、已有选择后插入排序更靠前节点、过期扫描响应、后台 tab 收包的回归。
3. 运行 `cargo test --test redis_loading_lifecycle --test redis_browser_tabs`，确认新增用例能暴露当前缺陷。
4. 在成功 apply_batch 和树更新后判断选择是否有效；只有缺失时取 `tree.nodes.first().map(|node| node.id.clone())`。释放 tab 可变借用后复用统一选择入口。
5. 将纯状态协调留在 tab/tree 方法中；Command 构造留在 App，避免 render 发请求。
6. 显式 open 时恢复内部 Keys 焦点；复用 tab 保留有效选择，必要时恢复其可见性。
7. 检查 select_redis_key 的连接归属：有效连接必须匹配 tab target；无法匹配时不向其他连接发送预览请求，也不让预览永久停在没有请求的 Loading 状态。沿用当前按 target 建连/加载能力。
8. 处理树快照替换后的选择失效与预览清理。先确认当前 apply_batch 是否替换 keys；普通增量插入不能保留已从新快照移除的旧节点。
9. 重跑本任务测试，验证选择只初始化一次、无焦点抢占、无错误连接请求。

**完成标准：** 从 db0 回车到首批可见数据出现无需再按 j，光标就在第一根节点；后续批次不抢选择。

**建议提交：** `fix(redis): initialize key selection after loading`

### T03：修正 tab 连接名称

**修改：** `src/ui/mod.rs::render_tabs`。
**验证：** 复用 `tests/ui_render.rs` 的渲染方式及最终人工检查。

1. 将 RedisBrowser 的连接名分支改为按 tab.target.profile_id 查找 profile.name，缺失时使用“失效目标”。
2. 保留现有标题终端字符清理、长度限制、tab 图标及 viewport 逻辑。
3. 用两个名称不同的 Redis profile 核对标题；切换活动连接、重命名、删除 profile 后检查显示结果。
4. 执行 `cargo test --test ui_render redis`。

**完成标准：** 标题始终反映所属连接，未使用 app.active_profile() 作为其他 tab 的标题来源。

**建议提交：** 与 T04 合并为 `fix(redis): align browser titles and panel styling`。

### T04：接入公共 pane、图标与树行样式

**修改：** `src/ui/mod.rs`、`src/ui/redis_browser.rs`。
**复用：** `src/ui/icons.rs::IconSet::group`、`src/ui/theme.rs`、`panel_block`、`explorer_list_item`。
**测试：** `tests/ui_render.rs` 中已有 Redis 焦点与 Explorer 相关用例。

1. Redis render 参数加入当前 Theme / IconSet，由 render_with_state_using_icons 的 Redis 分支传入。
2. Keys / Preview 通过公共 panel_block 绘制，删除 Redis 独立 panel_style 的硬编码颜色；状态信息使用 theme.muted / theme.error。
3. 面板焦点同时判断 app.focus 与 tab.focus，复用 title 样式。
4. 从 Explorer 的普通/选中树样式提取最小辅助函数；Explorer 特有的 unavailable、Others 分支仍在原位置处理。
5. render_row 改为缩进、箭头、图标、标签的分层 Span；Prefix 使用分组图标，叶子复用现有中性标记或已有 key 图标规范。
6. 先绘制整行背景再绘制文字，确保行尾空白也使用 selection 背景。
7. 扩展现有焦点渲染用例，验证 Explorer 拥有焦点时 Redis 边框失活，以及从 Keys 切到 Preview 后只有一个激活边框。
8. 执行 `cargo test --test ui_render`，检查 Explorer 与 SQL Editor 既有渲染回归；三种图标模式人工检查一次。

**完成标准：** 共用主题和图标配置；同屏切换 SQL Editor / Redis 时边框和行选择视觉一致。

### T05：统一树动作与 o 快捷键

**修改：** `src/input/keymap.rs`、`src/app.rs`、`src/model/redis_key_tree.rs`。
**测试：** keymap 现有单元测试、`tests/redis_key_tree.rs`、`tests/redis_browser_tabs.rs`。

1. 增加输入路径回归：裸 o 展开/折叠；查找输入 o 只更新文本；左右键在 Keys 命中树动作；h/l 仍切 pane。
2. 在 find Editing / Confirmed 的现有优先级之后，按 pane 和修饰键处理导航，消除提前 return 截断树方向键的分支。
3. 裸 o 与 Enter 共用 RedisPrimarySelection；确认配置导航不会将 o 吞掉或作为其他 pane 动作执行。
4. expand 不再调用 toggle_prefix：关闭则展开并停留，打开则选第一个可见子节点；collapse 对称处理。
5. 修正“前缀同时也是实际 key”的导航关系：visible_rows、visible_ids、first_child、parent_of 使用一致的父子语义，折叠时不留下被隐藏的选择。
6. 所有真正改变选择的动作走统一预览状态更新，并确保可见；文件夹折叠不应残留叶子预览。
7. 执行 `cargo test --lib input::keymap` 和 `cargo test --test redis_key_tree --test redis_browser_tabs`。

**完成标准：** o、方向键、Enter 行为稳定；不会出现折叠后选择隐藏子节点、移动后预览不跟随。

**建议提交：** `fix(redis): unify key tree navigation and folder toggles`

### T06：建立两 pane 的二维滚动状态与实际 viewport 同步

**修改：** `src/model/redis_browser.rs`、`src/action.rs`、`src/app.rs`、`src/ui/mod.rs`、`src/ui/redis_browser.rs`、`src/runtime.rs`。
**测试：** 新增 `tests/redis_browser_viewport.rs`。

1. 添加行为用例：两 pane 偏移互不影响；内容缩短和窗口扩大后偏移被 clamp；纯滚动不改变选中项或发送预览；切 key 重置 Preview。
2. 在 redis_browser 模型内定义可复用的小型 viewport 状态，包含纵横偏移、可见行列、内容行列。迁移旧 scroll / viewport_rows，避免保留两套 Keys 真值。
3. 相应迁移 RedisKeyFindState 的滚动快照，取消查找时恢复并 clamp 合理偏移。
4. Action 增加按 tab_id + pane 定位的 viewport 更新、相对滚动、绝对滚动；偏移运算使用 saturating 和 clamp。
5. 统一构造实际显示行：Keys 用当前 find 结果或展开树；Preview 使用当前 value_page / fallback preview 内容。宽度计算使用最终展示字符串的终端列宽。
6. 布局扣除状态、查找和必要滚动条后再上报实际内容区域；修正当前 keys_area / row_area 指标不一致以及查找行可能重复扣减的问题。
7. 扩展 UiState 和 sync_redis_keys_viewport 为两 pane 的完整指标同步，只在指标变化时派发 Action；检查所有绘制循环同步调用点。
8. viewport 同步只 clamp 偏移，不每帧 ensure_selected_visible，避免用户滚动被自动拉回；确保选择可见仅在打开/选择移动/展开折叠等操作后发生。
9. 内容宽度在视图内容变化时更新，滚动只改变偏移；避免仅滚动一行就重复格式化全部预览值。
10. 执行 `cargo test --test redis_browser_viewport --test redis_browser_tabs --test redis_key_tree`。

**完成标准：** UI、scroll 最大值、鼠标 hit region 和 runtime 上报使用同一个有效视口；空/窄窗口不溢出、不产生重绘循环。

### T07：提取公共滚动条绘制并接入 Redis

**修改：** `src/ui/scrollbar.rs`、`src/ui/mod.rs::render_editor_scrollbars`、`src/ui/redis_browser.rs`。
**测试：** scrollbar 现有单元测试、`tests/ui_render.rs`、`tests/redis_browser_viewport.rs`。

1. 在 scrollbar.rs 提取接受 track、方向、geometry、Theme 的纯绘制函数；几何计算继续复用 geometry。
2. SQL Editor 纵横滚动条调用新函数，Editor 专属 hit target 注册留在原调用方。
3. 检查现有水平滑块存在二次绘制的路径，确保提取后按 geometry.thumb_area() 只绘制一次，滑块与命中区位置一致。
4. Redis 两 pane 接入公共绘制函数，长文本按横向偏移裁剪；检查中文宽字符边界，不把字节偏移当终端列。
5. 纵向轨道对齐右边框，横向轨道位置沿用 SQL Editor 规范；明确排除角点，按最终有效尺寸计算可见性。
6. 加入几何边界检查：无溢出、仅单轴溢出、双轴溢出、滚到末尾、轨道短于最小可绘制尺寸。
7. 执行 `cargo test --lib ui::scrollbar` 和 `cargo test --test ui_render --test redis_browser_viewport`。

**完成标准：** SQL Editor / Redis 使用同一滚动条绘制源，字符、颜色、滑块位置一致。

### T08：接入滚动输入和精确鼠标热区

**修改：** `src/input/mouse.rs`、`src/input/keymap.rs`、`src/ui/mod.rs`、`src/ui/redis_browser.rs`、`src/action.rs`、`src/app.rs`。
**测试：** `tests/mouse.rs`、`tests/redis_browser_viewport.rs`、keymap 单元测试。

1. 扩展 pane body、滚动条端点、轨道、滑块 hit target，目标包含 tab_id、pane、轴及所需 geometry。
2. 绘制树行时复用同一列布局计算箭头和标签位置：缩进宽度 + marker/icon 宽度 - 水平偏移，再与 viewport 求交；被裁掉的箭头不注册可点击热区。
3. 滚动条命中优先于树行；pane 空白区域也能切换局部焦点。
4. 鼠标纵向滚轮作用于指针下方 pane；横向滚轮按终端事件能力支持，底部滚动条始终提供横向操作。
5. 端点单步、轨道翻页、滑块拖动复用 geometry 的偏移换算；保留指针在 thumb 内的抓取偏移。
6. 拖动状态跟随 tab_id / pane，松开、关闭 tab、切换工作区或 overlay 接管时清理，避免把旧拖动应用到新 tab。
7. 接入 Preview j/k、方向键和按 pane 高度的 PageUp/PageDown；删除固定 10 行且不区分 pane 的翻页路径。
8. 测试深层箭头点击、横滚后的节点点击、滚动条与行重叠处、两个 pane 独立滚动、拖动中切 tab、纯滚动不发网络命令。
9. 执行 `cargo test --test mouse --test redis_browser_viewport` 和 `cargo test --lib input::keymap`。

**完成标准：** 可见位置即点击位置；鼠标/键盘/滚动条共同操作同一偏移状态；Preview 滚动不会改变 Keys。

**建议提交：** T06–T08 编译和回归通过后提交 `feat(redis): add consistent pane scrolling and hit testing`。

### T09：文档、完整回归与终端验收

**修改：** `docs/redis-browser.md`；记录本计划执行结果。

1. 更新默认选择、o、方向键、pane 切换、滚动以及查找输入的使用说明；检查现有快捷键提示展示入口并保持一致。
2. 运行最终定向回归：

   ```bash
   cargo test --test redis_loading_lifecycle --test redis_browser_tabs --test redis_key_tree --test redis_browser_viewport --test ui_render --test mouse
   ```

3. 按 CI 执行：

   ```bash
   cargo +1.94.0 fmt --all -- --check
   cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
   cargo +1.94.0 test --all-targets --all-features
   ```

   预期全部通过；环境或既有失败单独记录，不作为本功能通过的证据。

4. 在开发 Redis 连接准备 `app:config`、`app:session:1`、`user:1`、`version` 等键，以及同名 key/前缀组合、中文长 key、多行长 value；使用空 DB 验证空状态。
5. 完成下方验收矩阵，记录终端尺寸、图标模式和关键画面。
6. 审查最终 diff，确认代码、测试和说明一致，按逻辑提交边界提交已完成工作。

**建议提交：** `docs(redis): document browser navigation and pane scrolling`

## 4. 最终验收矩阵

| 场景 | 验收结果 |
|---|---|
| Explorer db0 回车，首次非空批次完成 | Keys 聚焦，第一根节点高亮，文件夹不自动展开 |
| 首批为空，后续有 key | 后续批次初始化选择，不误判空库 |
| 后续批次插入更靠前节点 | 原选中节点保持，预览不被覆盖 |
| 空 DB / 失败 / 过期响应 | 状态文案正确，无虚假选择，无旧响应覆盖 |
| 已有 Redis tab 从 Preview 重新打开 | Keys 聚焦，保留有效选择 |
| 两个连接、同 DB、重命名 | 各自 Redis @连接名 正确，无连接串用 |
| Explorer 与 Redis 切焦点 | 激活边框唯一、未激活边框一致 |
| o / Enter / ← / → | 符合按键契约，选择与预览一致 |
| 查找输入 o | 正常输入字符，不展开文件夹 |
| 同名 key 与前缀共存 | 父子关系、展开、折叠和导航一致 |
| 深层目录和水平滚动后的点击 | 箭头与文字点击区域准确，无隐藏热区 |
| Nerd Font / Unicode / ASCII | 图标模式生效，选中背景覆盖整行 |
| Keys / Preview 双轴溢出 | 滚动条与 SQL Editor 同样式，pane 状态独立 |
| 滚动/拖动、切 tab、打开 overlay | 无错误 pane 更新，无残留手势 |
| 调整窗口大小、收起目录、切 key | 偏移合法，必要时复位，不越界 |
| SQL Editor / Explorer 既有用例 | 样式与交互无回归 |

## 5. 顺序与交付边界

推荐顺序：`T01 → T02 → T03 → T04 → T05 → T06 → T07 → T08 → T09`。

- M1：T02–T05，完成默认选择、连接名、pane 样式、图标、树选择与 o 交互。
- M2：T06–T08，完成两 pane 的二维滚动、公共滚动条和鼠标命中。
- M3：T09，完成 CI、终端验收和说明文档。

多个任务涉及 src/app.rs、src/ui/mod.rs、src/input/keymap.rs，按顺序集成可以减少冲突。每阶段通过后再进入下一阶段；本计划中的测试和命令尚未执行，实施时记录实际结果。
