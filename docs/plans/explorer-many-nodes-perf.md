# Explorer 大量展开节点性能优化 Implementation Plan

**Goal:** 消除约 956 个展开表节点时导航和界面重绘中的重复投影、索引分配和屏外展示数据转换，同时保持现有交互行为。
**Architecture:** 使用操作内轻量投影快照与祖先位置复用；导航只生成一次结构投影，绘制的视口与滚动条共享总行数。展示数据在裁剪后转换；优先不引入跨帧缓存及失效管理。
**Tech Stack:** Rust 2024、Ratatui、现有 Explorer 模型与集成测试；无需新增依赖。
**执行人：** Luna 负责实现、验证、审查、纠偏、提交和合并；Astra 本阶段只编写计划。

## 基线与执行约束

- 基线提交：`1b45a1eb6261cd309edb0c859fadbcbda3a0982f`，目标分支 `main`。
- 分析依据与方案比较见同目录 `analysis.md`；已执行检查见 `validation.md`。
- 原工作区的三个未跟踪其他任务计划不属于本任务，也不是实施依赖。新 worktree 从上述提交建立时不需要复制它们。
- 不清空原工作区、不 stash、不提交其他任务文件。任务分支按工作流在计划完成后自动命名，本阶段不创建分支。
- 报告、后续计划和验证日志只写当前 `.git/opencode-tasks/ses_f4796f704ffehDM3OLT7vAGY1T` 目录。checkpoint/state 由插件管理。
- plan 阶段读取 checkpoint 返回不存在。本轮按 plan 阶段指令写 `change-scope.json` 和 `plan-d712f298-9a54-4166-8f2e-d6666fc9e3cd.json`，不覆盖历史回执，也不复用 analyze 回执文件名。
- 下列命令均为未来实施检查，除 validation.md 已记录者外，不表示已执行或通过。

## 验收契约

1. 在正常树模式，一次 normalized 导航至多构建一次结构投影；workspace 移动同步不额外转换全量展示节点。
2. 滚动校正循环内没有全量节点索引重建；祖先位置按新选中项计算并在操作内复用。
3. 一次普通树/Find 绘制中，viewport 与 scrollbar 共用逻辑行数；仅为 pinned/body 行构建 label/metadata/comment。
4. 搜索模式先裁剪 frontend_rows 再转换展示数据，计数直接使用结构行数。
5. 保持稳定选择 ID、兼容 selected/scroll、祖先置顶、极小视口、Find 恢复、分页与鼠标语义；不改加载数量或快捷键。
6. 当前方案仍是必要的 O(N) 结构扫描，不宣称 O(1) 导航。956 表 release move+draw 的测量目标为 p95 ≤16.7ms；以同环境基线与修复后对照确认实际收益。

## 验证分级

本节区分必须完成的门禁与仅作补充的建议。后续不得把补充项当成未满足的强制要求，也不得省略强制项。

**用户需求（本任务目标）**

- 修复 956 个展开表节点时的 Explorer 移动/重绘卡顿，且不改变浏览与数据库加载行为。
- 单元一并覆盖移动与普通绘制闭环；这是本任务的核心验收，不是可选优化。

**项目强制门禁（来自 CONTRIBUTING.md 与 .github/workflows/ci.yml，功能完成后各执行一次）**

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

- 需要真实数据库连接的测试是条件执行：未设置对应环境变量时它们会跳过。记录实际执行/跳过情况，不把跳过写成通过，也不为纯 Explorer 优化主动申请数据库权限。
- 单元内只跑定向测试；全量 fmt/clippy/test 只在功能齐备后执行一次。

**补充建议（非强制门禁）**

- `cargo test --release ... -- --ignored` 的 956/10000 表计时用例、p95 ≤16.7ms 目标：用于证明收益并指导是否升级缓存。若基线已远低于预算，以工作量与微基准相对改善说明即可，不因未达固定倍数阻塞。
- 真实终端/PTY 连续按键与滚轮体验检查：补充证据，替代不了也非必需于 TestBackend 验证。环境受限时最多一次针对性修复重试，由 Luna 决定补证据或记录限制。
- 10000 表规模压力对照：观察增长趋势，不扩张成 60fps 新需求。

## 单元一：956 表导航与普通绘制闭环

这是第一个未完成的可验收单元。必须同时覆盖移动和重绘，不能只修滚动条后结项。

### 1. 建立可复用基线与行为保护

**文件：** `tests/explorer_performance.rs`、`tests/explorer_state.rs`；基准涉及完整渲染时参考 `tests/ui_render.rs` 已有 TestBackend fixture。

- 将现有性能 fixture 扩展为可配置数量的同一 schema/Tables 组，展开 profile/database/schema/group，表默认不展开。
- 使用 956 表、视口高 30，起点选中中段；增加 100 和 10000 表规模及多段 CatalogId 路径对照。
- 增加行为断言：连续向下/向上、移动到首尾后选择有效、scroll 在边界内、选中行能在 pinned/body 中找到、祖先不重复。
- 添加显式 ignored 的计时用例：分别测 normalized move、workspace move、viewport、TestBackend draw、App move+draw；预热后至少采集 1000 次，移动成对往返避免停在边界。
- 使用同一 fixture 分别记录优化前后结果；建树、连接和编译耗时不进入样本。完整绘制固定 120×40，并记录实际 Explorer 高度。
- 额外测编辑器焦点下的 redraw，确认非 Explorer 动作不会继续支付全量展示转换成本。

**命令：**

```sh
cargo test --locked --test explorer_performance --test explorer_state
cargo test --release --locked --test explorer_performance -- --ignored --nocapture --test-threads=1
```

**预期：** 既有行为通过；基线产生真实 p50/p95 数据而非预设失败阈值。计时测试不作抖动敏感的 CI 硬断言。

### 2. 复用导航结构投影与祖先位置

**修改：** `src/model/explorer.rs` 的导航方法、`update_scroll`、`body_height_for_scroll`、`viewport` 辅助逻辑。

- 采用私有短生命周期辅助结构或 `_with_rows` 方法；结构存 rows、已知选中 index、当前选中项祖先位置，不存 label，也不存回持久状态。
- `move_selection` 生成 rows 后确定新选中 ID，再计算该 ID 的祖先位置。
- `update_scroll` 接收已有 rows/长度和祖先位置，删除内部 `visible()`；循环只用位置和高度做计算。
- 祖先位置一次求得，可用少量借用 key 建集合后扫描 rows；避免 clone 全部 CatalogId 构造 HashMap。
- 保留原循环收敛、滚动边界和 viewport_height=0 处理，不顺带重定义小视口行为。
- 给 workspace 提供内部选择索引结果，或者等价内部 helper；保持公开 API 调用者的兼容性。
- 在模块 cfg(test) 内建立工作量回归断言，或使用已有计数机制扩展。验证一次操作的投影工作上界与循环内无全量索引重建，避免新增公开持久 telemetry API。

### 3. 去除兼容索引与滚动条的全量展示转换

**修改：** `src/model/workspace.rs` 的 `move_selection`、`sync_selected_index`、viewport 数据结构；`src/ui/mod.rs` 的 `render_explorer`、`render_explorer_scrollbar`。

- workspace 普通移动使用已知 index 同步 `selected`，从 normalized 同步 `scroll`，不调用 `visible()`。
- 通用 `sync_selected_index` 只需结构行位置，不生成展示信息；不能把 profile-local index 与全局 index 混为一谈。
- normalized viewport 用一次结构投影产出 pinned/body 及总逻辑行数；workspace 保留该数量，仅转换 pinned/body。
- scrollbar 从调用方获得数量，不自行调用 `app.explorer.visible().len()`；保持 offset、thumb geometry 与鼠标 hit region 行为。
- UI 相关测试检验总数包含 loading/empty/load-more 等合成行；展示转换数量应由视口容量限制。

**定向检查：**

```sh
cargo test --locked --test explorer_performance --test explorer_state --test ui_render
```

若计数测试在模块内，运行实际名称对应的 `cargo test --locked --lib <filter>`，把完整命令和结果记录到 validation.md。

**单元完成条件：** 956 表移动和普通绘制使用优化路径、行为回归通过、前后测量已记录。无需在这个节点跑全量 clippy/test；继续下一单元。

## 单元二：滚动、搜索与目录状态一致性闭环

**修改：** `src/model/explorer.rs`、`src/model/workspace.rs`、`src/ui/mod.rs`；仅实际契约变动需要时调整 `src/app.rs`。
**测试：** `tests/explorer_state.rs`、`tests/mouse.rs`、`tests/ui_render.rs`、`tests/catalog_reducer.rs`。

### 1. 覆盖其他导航入口

- 将 `select_target`、`scroll_nodes`、`set_scroll_offset`、`align_selected`、`ensure_selected_visible` 接入共享 rows/ancestor positions 的逻辑。
- 特别检查滚轮移动导致选择变化、整页跳转跨 schema/group、居中候选计算；不能复用旧选中项的 ancestors。
- `select_id` 只查结构位置，不生成展示行。
- 保留没有选中项、零高度及 offset 超界的既有行为。

### 2. 接通 Find 与 Search 的显示路径

- Find 使用普通树 viewport 的总逻辑行数绘制 scrollbar，保留高亮与祖先压缩。
- Search 直接取 frontend_rows.len() 作为数量；先按原规则算 start/range，再转换该范围展示节点。
- 搜索匹配、首次 Find 快照等确实需要全列表的操作继续允许遍历完整结构，不将视口优化错误套用到匹配范围。
- 不删除现有 terminal text sanitization，也不改变匹配高亮范围。

### 3. 验证变化立即生效

验证矩阵：

| 场景 | 必须保持的行为 |
| --- | --- |
| 高度 0/1/2、resize | 不越界，最近祖先/选择可见规则不变 |
| 首尾、半页、整页、居中、scrollbar 拖动 | 选择与滚动同步，鼠标命中不偏移 |
| Find 编辑、确认、n/N、取消 | 定位、居中与原选择/滚动恢复一致 |
| Search locate/cancel | 真实节点定位与恢复一致 |
| 展开/折叠、profile/group/others | 下一次绘制立即反映当前结构 |
| replace/append/drop/load-more | 节点总数正确，选择有正确回退 |
| loading/error/empty 状态变化 | 合成行立即更新且计数正确 |
| 新增/移除 profile、连接排序 | 稳定 ID 与顺序语义不变 |

**定向检查：**

```sh
cargo test --locked --test explorer_state --test mouse --test ui_render --test catalog_reducer --test explorer_performance
```

**单元完成条件：** 普通树、Find/Search 和鼠标入口没有残留的“只为数量/索引生成全量展示行”热路径；状态变化和边界回归通过。

## 单元三：性能验收与交付

### 1. 运行最终性能对照

使用单元一同样的 release fixture、features、机器、终端尺寸和样本数，记录提交/hash 或 diff、rustc 版本、p50/p95。参考目标：956 表 move+draw p95 ≤16.7ms，并有可解释的工作量下降；10000 表用于观察增长趋势，不扩张成必须达到 60fps 的新需求。

- 若基线已经远低于预算，以投影次数、分配和微基准改善作为证据，不承诺固定加速倍数。
- 若合成基准仍不达目标，继续定位其实际耗时；若剩余主要成本确为每次 O(N) 投影，升级到持久结构缓存。
- 升级缓存前必须补充实施范围：收拢 expanded/profiles/catalog/load states/groups/profile_order 的写入口或增加可靠 mutation guard。单靠 catalog_epoch 或 expand/collapse dirty 不足够。
- 缓存必须覆盖 catalog、合成行、Redis 列表、连接组织、搜索 reveal 等变化；选择、scroll、height 不使结构缓存失效；metadata 仍按视口实时读取。追加失效行为测试后再继续验收。

上述普通性能纠偏由 Luna 自行推进，不等待用户 resume，不把尚未达标当作外部阻塞。

### 2. 执行项目要求的验证

完整功能就绪后运行一次：

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

CI 使用 Rust 1.94.0，当前分析环境为 1.94.1；记录实际工具链。数据库条件测试是否真正执行必须按环境变量和输出注明，不能把跳过写成真实数据库验证通过。无需因纯 Explorer 模型优化自行引入数据库权限操作。

只有代码或环境发生相关变化、检查失败或仍有明确疑点时重跑检查；不机械重复 check+clippy+全量测试。

### 3. 补充体验与收尾审查

- 可用真实终端时检查连续按键、滚轮和编辑器输入，作为 TestBackend 之外的补充体验证据。
- PTY/人工环境受限时最多一次有针对性修复重试；由 Luna 判断需补证据还是记录限制，不能无限等待或尝试。
- Luna 审查 diff，确认没有改变数据库加载、快捷键、默认列表规模或用户文件；复核所有验收契约。
- 按当前工作流完成提交/合并及对应阶段的指定回执。仅使用该阶段新指令给出的回执路径/token。

## 下一步

由 Luna 接手执行单元一：在任务分支建立 956 表 move+draw 可重复基线与工作量回归，再完成操作内投影复用和同源滚动条计数。计划已完整，不需要用户逐单元确认。
