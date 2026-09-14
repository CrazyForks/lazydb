# Redis Key Search Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Redis Keys 面板使用顶部固定搜索栏，按 `/` 及输入时保留浏览状态，仅在 Enter 后搜索已加载 key 并展开匹配路径。

**Architecture:** 沿用 `RedisFindPhase::Editing / Confirmed`，把查询草稿和已执行结果的生命周期分开。原始 `KeyTreeState` 保存正常浏览树，提交后用匹配 key 构建独立结果树，复用树的可见行遍历处理展开与折叠。UI 统一分配搜索栏、树列表、底部状态三个区域，应用层统一协调选中项、滚动与预览请求。

**Tech Stack:** Rust 2024 / Rust 1.94、Ratatui 0.30.2、Crossterm、现有 TextInput 和 Redis Browser 模型、Cargo 集成测试。

---

## 0. 执行约定

- 本文为实施计划，代码尚未修改。引用行号基于编写计划时的代码，实施时按符号定位。
- 按任务顺序执行；每个编号步骤作为一次小改动，完成相应行为测试后进入下一步。
- 各任务列出建议提交边界；只有在用户要求提交时才执行 git commit。
- 先运行定向测试，完成后运行与 CI 一致的 Rust 检查。不把编译通过当作交互验收通过。
- 不引入新的搜索库、后台任务或 Redis 查询协议；复用当前匹配规则和扫描生命周期。

## 1. 已确认的根因

| 位置 | 当前行为 | 修正方向 |
| --- | --- | --- |
| `src/ui/redis_browser.rs:158–188` | 搜索区域位于底部，高度 1 又带 TOP 边框 | 顶部独立一行，无占用内容高度的边框 |
| `src/ui/redis_browser.rs:103–156` | 预留区域、树绘制区域、搜索位置不一致 | 所有树绘制使用同一 tree_area |
| `src/ui/redis_browser.rs:117–122` | 滚动条覆盖整个 keys_area | 滚动条仅覆盖树区域 |
| `src/model/redis_browser.rs::open_find` | 打开即重建并展开全量投影，随后再刷新一次 | 打开仅建立编辑状态 |
| `RedisBrowserTab::visible_rows` | 只检查 find 是否存在 | 仅 Confirmed 展示结果树 |
| `src/app.rs` 中三个 RedisFind 编辑动作 | 每次输入都调用 update_find | 只编辑 TextInput |
| `RedisBrowserTab::confirm_find` | 仅切换 phase | 执行查询并生成结果 |
| `RedisBrowserTab::rebuild_filtered_rows` | 折叠只改 expanded 标志，不隐藏后代 | 使用结果树的 visible_rows |
| `Action::RedisFindConfirm` | 用原始树计算结果滚动位置 | 统一按当前展示行定位 |

## 2. 行为契约

### 状态转换

| 当前状态 | 操作 | 结果 |
| --- | --- | --- |
| Browsing | `/` | Editing；保存原始选中和滚动，顶部显示输入框 |
| Editing | 输入/退格/Delete | 仅修改草稿，原始展开和选中不变 |
| Editing | Enter，查询非空 | Confirmed；搜索并显示匹配 key 及祖先，选中首个匹配 |
| Editing | Enter，查询为空或仅空白 | 关闭搜索，恢复浏览状态 |
| Editing | Esc | 取消并关闭，恢复浏览状态 |
| Confirmed | n/N | 按稳定顺序循环跳转；展开目标路径并保证可见 |
| Confirmed | 折叠/展开目录 | 只修改结果树；折叠后隐藏后代 |
| Confirmed | Esc | 清除结果投影；恢复搜索前选中、滚动和原始展开状态 |
| Confirmed | `/` | 重新进入 Editing，保留上次查询文本；恢复正常树视图，下一次 Enter 才重算 |

### 细节约定

- 搜索范围是当前已加载 key；匹配规则仍用 `search_text_matches(display_bytes(key), query)`。
- 搜索结果不包括无关兄弟节点；真实 key 身份、字节值和稳定排序沿用现有实现。
- 无匹配时显示 `No matching loaded keys`，不误用“数据库没有 key”的空状态，也不加载原先 key 作为搜索结果。
- 编辑时隐藏匹配数量或显示提交提示，不能显示看似已执行的 `0 matches`。
- 添加搜索栏会减少视口高度；只允许必要的滚动边界钳制，不能主动跳到首行或重排树。
- 后台正常扫描仍可增加浏览树节点；“编辑不改变树”指搜索动作不改变展开/选择，而非冻结扫描。
- 搜索前节点被删除时，退出后选择仍有效的原节点，否则选择正常树首个可见节点；空树则清空选择。滚动按实际行数钳制。
- 退出时预览必须与恢复的选择一致，目录/空选择清空预览；不要仅恢复 selected 字段而保留另一 key 的值。

## Task 1: 建立延迟提交的模型契约

**Files:**
- Modify: `src/model/redis_browser.rs` — RedisKeyFindState、open_find、visible_rows、confirm_find、close_find。
- Test: `tests/redis_key_filter.rs`。
- Review/update: `tests/redis_browser_tabs.rs` 中现有 find 调用。

1. 在现有测试夹具中加入至少两级前缀、折叠目录内 key、一个无关分支；记录 visible_ids、expanded、selected、scroll。
2. 添加行为测试：open_find 后上述浏览状态不变；编辑 query 后不出现结果、不移动选中项；Enter 后才匹配。把现有依赖 `update_find()` 即时搜索的测试改为提交后断言。
3. 运行 `cargo test --test redis_key_filter`，确认新增延迟提交测试在旧实现下失败，记录实际失败原因。
4. 重构搜索状态：保留 phase、TextInput、matches/current、original_selected/original_scroll；用 `Option<KeyTreeState>` 保存提交后的结果投影，逐步移除 rows/filtered_rows/expanded 的重复状态。原始树的 expanded 始终归正常浏览所有。
5. open_find 只保存恢复信息并创建 Editing，不遍历 key、不构建投影；重新编辑沿用同一次搜索的原始恢复点。
6. visible_rows 在 Editing 返回原始树，在 Confirmed 返回结果投影。UI 后续也只从此接口获取行，避免另一个 find.is_some 分支绕过阶段判断。
7. confirm_find 处理空白查询退出；非空查询转入执行流程。取消和退出都恢复 origin，且处理原节点失效。
8. 再次运行 `cargo test --test redis_key_filter --test redis_browser_tabs`，确保阶段转换通过。

**建议提交边界:** `refactor(redis): separate key search editing from execution`。

## Task 2: 构建可折叠、仅包含匹配路径的结果树

**Files:**
- Modify: `src/model/redis_browser.rs` — 查询执行、toggle_prefix、move_find、结果刷新。
- Reuse: `src/model/redis_key_tree.rs` — rebuild、visible_rows、parent_of、contains。
- Test: `tests/redis_key_filter.rs`、`tests/redis_key_tree.rs`。

1. 添加提交结果测试：折叠目录内的 key 可被找到，只保留匹配项及其祖先；包含 key 与目录前缀共存、连续冒号、非 ASCII key 的现有边界语义。
2. 添加结果折叠测试：关闭父目录后所有后代从 visible_ids 消失，再展开后恢复；原始树 expanded 不变。
3. 运行 `cargo test --test redis_key_filter --test redis_key_tree`，确认行为缺口。
4. 一次遍历已加载 key 做匹配，按现有确定性顺序整理命中列表，再仅用命中的 RedisKeyId 构建结果 KeyTreeState。使用树结构确定祖先路径，避免独立手写分隔规则导致特殊 key 行为分叉。
5. 初次提交默认展开结果祖先；通过结果树 visible_rows 生成列表，移除原先重复的线性查找祖先及手动设置 row.expanded 的过滤逻辑。
6. toggle_prefix 在 Confirmed 路由到结果树，在正常浏览路由到原始树。Editing 期间不要允许鼠标或树快捷键意外改变搜索前展开状态。
7. move_find 保持完整匹配列表，不因目录折叠丢失匹配；跳转前展开目标祖先，随后按当前 visible_ids 对齐视口。
8. 无结果时清除搜索选中状态并把结果滚动归零；结果为空不触发旧 key 的预览请求。
9. 再运行上述两个测试目标。

**建议提交边界:** `fix(redis): build collapsible key search result trees`。

## Task 3: 接入动作、滚动和预览生命周期

**Files:**
- Modify: `src/app.rs` — RedisFindOpen/Insert/Backspace/Delete/Confirm/Cancel、move_redis_find、select_redis_key 及相关调用。
- Modify: `src/input/keymap.rs` — 两处 Redis Editing/Confirmed 分派，按实际需要调整。
- Review: `src/input/mouse.rs` — RedisFindInput 和目录命中动作。
- Test: `tests/keymap.rs`、`tests/redis_browser_tabs.rs`。

1. 添加动作链测试：`/ → 输入 → Enter`；断言编辑动作无预览 effect，提交后选中匹配 key 并走现有预览调度。
2. 添加 Esc 恢复、空查询、无匹配和重新编辑的动作测试；确认恢复目录时预览被清空，恢复 key 时加载正确 key。
3. 运行 `cargo test --test keymap --test redis_browser_tabs`，确认旧动作链不符合新契约。
4. 从三个编辑动作移除 update_find，保留 TextInput 的插入、退格、删除及原有文本编辑行为。
5. Enter 调用模型提交逻辑后，仅在有有效匹配 key 时调用现有 select_redis_key；空查询和无结果禁止落入读取旧 selected_key 的分支。
6. 为 Enter、n/N、目录折叠和窗口大小变化统一当前可见行的滚动钳制/定位逻辑。检查 select_redis_key 内部是否仍按原始树滚动，避免正确结果在后续调用被覆盖。
7. Esc 恢复后走现有选择和预览失效/调度路径，使用既有 generation 机制拒绝已经过期的搜索预览返回。
8. 检查两处分派的一致性：Editing 的 Enter/Esc/文本处理、Confirmed 的 n/N/Esc/重新搜索。鼠标点击顶部输入区域应进入 Keys 焦点；重新编辑行为与 `/` 一致。
9. 再运行两个测试目标，确认普通 Redis 导航与 Explorer f 映射无回归。

**建议提交边界:** `fix(redis): execute key searches on enter`。

## Task 4: 异步扫描期间保持搜索稳定

**Files:**
- Modify: `src/model/redis_browser.rs` — insert_tree_keys、refresh_find_rows（或替换后的结果刷新函数）。
- Review/modify as needed: `src/app.rs` — keyspace 批次、删除和刷新处理路径。
- Test: `tests/redis_loading_lifecycle.rs`、`tests/redis_key_filter.rs`、`tests/redis_key_delete.rs`。

1. 添加 Editing 时插入扫描批次的测试：草稿保留，原有展开/选择不被搜索重写，没有结果投影。
2. 添加 Confirmed 时新增匹配与非匹配 key 的测试：匹配数量更新，当前有效选择保留，已有手动折叠状态保留。
3. 添加当前匹配或 origin 被删除的测试：选择回退确定、滚动不过界、退出不恢复无效 ID。
4. 运行 `cargo test --test redis_loading_lifecycle --test redis_key_filter --test redis_key_delete`。
5. Editing 批次只更新正常树；Confirmed 批次用已提交查询刷新结果。以 key ID 保持当前位置，不能每批次把 current 重置为 0。
6. 结果刷新保留有效 expanded 集合；新出现的祖先可以默认展开，但不重新打开用户已折叠的已有目录。若现有结果树丢失后重新出现，按新目录规则处理。
7. 当前选中消失时按原匹配位置选择相邻有效匹配；没有结果则清空；必要时走应用层预览更新，而非只改模型字段。
8. 再运行上述测试目标。

**建议提交边界:** `fix(redis): preserve key search state during scans`。

## Task 5: 顶部搜索栏及统一区域布局

**Files:**
- Modify: `src/ui/redis_browser.rs` — Keys 区域划分、输入绘制、空状态、滚动条和命中区域。
- Reference: `src/ui/mod.rs::render_explorer_find`、现有文本终端宽度/输入绘制工具。
- Test: `tests/ui_render.rs`。

1. 使用项目现有 TestBackend 夹具添加 buffer 断言：搜索前缀和输入文本出现在 Keys 内部第一行，第一条树行在其后，光标位于搜索行。
2. 加入底部扫描状态/两行错误提示和低高度窗口案例，断言搜索栏、树和状态不重叠，滚动条不进入输入行。
3. 运行 `cargo test --test ui_render redis`；新增测试名称统一包含 redis，确保过滤命中。
4. 将 Keys 内部拆为 search_area、tree_area、status_area；按实际可用高度分配，使用饱和运算。空间不足时优先保留搜索输入，状态提示缩短，树允许为 0 行。
5. 删除原 key_body.bottom 搜索定位和一行 TOP block。无搜索时搜索栏高度为 0；编辑和结果态均固定顶部一行。
6. 树只在 tree_area 绘制；viewport_rows、行命中区域和滚动条均使用同一区域。保留右侧滚动条所需列宽，输入框不占其滚动轨道。
7. 输入文本终端清理、宽度裁剪沿用已有工具；按实际 TextInput.cursor 前缀的终端单元宽度定位。长查询按需水平偏移，保证光标在框内；宽度或高度为 0 时不注册无效光标/命中区域。
8. Editing 显示 `/ 查询` 与提交提示；Confirmed 显示查询和当前位置/匹配总量，底部提示 n/N、Esc。无匹配状态优先使用搜索文案，并保留必要的扫描失败信息。
9. 移除渲染器中直接读取 find.filtered_rows 的重复分支，统一消费 tab.visible_rows，避免绕过 Editing 规则。
10. 再运行 UI 定向测试，检查中文、长查询、空树、窄面板和错误状态的 buffer 与 cursor 坐标。

**建议提交边界:** `fix(ui): place redis key search above the tree`。

## Task 6: 完整验收及用户提示

**Files:**
- Review/modify as needed: `src/help.rs` — 已有 Redis 搜索上下文的提示，避免残留实时搜索说明。
- Review: `tests/redis_scale.rs` — 复用现有规模夹具，确认编辑不构造全量搜索投影。
- Update: 本计划的执行记录。

1. 检查帮助和底部提示与状态契约一致；只更新真实存在的 Redis 搜索说明。
2. 使用现有规模数据验证 open_find/编辑不创建结果树、不遍历全量 key。通过结构/行为断言验证，不增加不稳定的耗时阈值测试。
3. 执行定向回归：

```bash
cargo test --test redis_key_filter --test redis_key_tree --test redis_browser_tabs --test redis_loading_lifecycle --test redis_key_delete --test redis_scale
cargo test --test keymap --test ui_render
```

预期：所有指定测试通过，无意外跳过新用例。

4. 执行与 `.github/workflows/ci.yml` 对齐的检查：

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

预期：格式、静态检查、全部测试通过。需要数据库环境的测试按仓库现有机制执行，记录实际跳过或环境限制，不把跳过描述为已验证。

5. 手动运行 `cargo run --`，连接可用的 Redis 测试连接并按下列矩阵验证。没有可用连接时明确记录未完成的手动项。

| 场景 | 验收标准 |
| --- | --- |
| 原树有展开/折叠混合状态并滚动至中部 | `/` 后状态保留，搜索框在最上方且文字可见 |
| 连续输入和删除 | 树不因搜索展开、不跳选、不提前显示过滤结果 |
| 提交深层 key 查询 | 仅匹配路径展开，首个结果可见且预览对应正确 key |
| 折叠结果父目录后 n/N | 目标祖先自动展开，正确切换预览 |
| 无匹配、空白提交、Esc | 文案正确、无误加载，退出恢复原浏览状态 |
| 结果态 `/` 修改查询再 Enter | 草稿阶段展示正常树，提交后更新结果，退出恢复最初 origin |
| 扫描继续返回、匹配 key 删除 | 当前选择尽量稳定，失效时合理回退，无旧值覆盖 |
| 缩小终端、底部错误、中文长查询 | 输入、光标、列表、滚动条和提示均不重叠、不越界 |
| 切换到 Explorer 使用 f | Explorer 既有行为正常 |

6. 在执行记录中填写改动文件、已通过命令和未完成项，再给出最终实现总结。

## 完成标准

- `/` 只进入编辑；搜索输入固定顶部，文字不会被边框吞掉。
- 输入和编辑阶段的扫描更新均不会触发搜索投影展开。
- Enter 才产生结果，折叠目录内 key 可被匹配，结果仅显示相关路径。
- 结果折叠、n/N、滚动、预览和 Esc 恢复一致。
- 原始树展开状态不被搜索结果污染。
- 定向回归及 CI Rust 检查完成，手动验收状态被如实记录。

## 执行记录

- 2026-09-14：根据代码分析建立计划；尚未执行实现或测试。
