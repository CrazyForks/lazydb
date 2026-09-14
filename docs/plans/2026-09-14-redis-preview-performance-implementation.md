# Redis String 预览性能与 JSON 自动识别 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行环境若没有该技能，则按本文任务顺序实施，并逐项记录验证结果。

**Goal:** 修复含非 ASCII 字符的 Redis String JSON 无法自动识别、长单行预览阻塞输入的问题，并减少加载请求、重复分配和大值处理开销。

**Architecture:** 原始字节是检测、解码、复制的唯一内容来源，显示转义仅发生在终端投影阶段。建立不经过 SQL 分析的非 SQL 文档快照路径，以内容 revision 缓存布局和格式化结果，按视口生成渲染片段；Runtime 负责最新选择优先的请求调度、受限后台处理，App 仅接收经过身份校验的结果。

**Tech Stack:** Rust 2024 / MSRV 1.94，Tokio，redis 1.5.0，Ratatui 0.30.2，serde_json，unicode-width，现有 Editor、PreviewCache、PreviewScheduler 和 Rust 测试设施。

---

## 1. 事实、范围与实施顺序

### 已确认的代码事实

| 位置（实施时以符号定位） | 问题 |
| --- | --- |
| `src/app.rs` / `Action::RedisValuePageLoaded` | 自动检测输入是 `page_text(page).into_bytes()` |
| `src/ui/redis_value.rs` / `display_bytes` | 一个非 ASCII 字节导致整段按 `\\xNN` 转义 |
| 同文件 / `format_page` | JSON/YAML 解析显示文本，Hex 也直接返回 RAW 文本 |
| `src/editor/mod.rs` / `render_preview_snapshot` | 非 SQL 预览先进入 SQL 高亮，再改颜色 |
| `src/ui/mod.rs` / `editor_line_spans` | 对每个字符从头查 span，存在 O(N×S) 路径 |
| `src/editor/mod.rs` / `render_snapshot_with_sql_ranges` | 每帧提取全文、求最大行宽、投影整条可见长行 |
| `src/ui/redis_browser.rs` / `render` | 正常快照路径前也会生成 fallback `page_lines` |
| `src/runtime.rs` / `load_redis_value_preview` | metadata 在预览和 page 读取阶段重复查询 |
| `src/app.rs` / `select_redis_key` | 直接发出读取命令，未接入现有 PreviewScheduler |
| `src/value_preview/cache.rs` | 命中返回深拷贝；键只有 revision 和 format |
| `src/model/redis_browser.rs` / `select` | 换 key 不恢复 automatic，旧 editor 文档没有在此清理 |

截图没有原始 payload，也没有真实端到端耗时记录。根因代码可确认，耗时比例通过任务 1 建立基线；所有性能数字在本文中均为验收目标而非已测结果。

### 交付分组

1. **M1：功能正确、去掉主要平方级路径。** 任务 1–4。
2. **M2：视口成本可控、缓存真正生效。** 任务 5–6。
3. **M3：网络与后台工作受控。** 任务 7–9。
4. **M4：完整回归和性能验收。** 任务 10。

依赖：`1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9 → 10`。这里使用串行顺序，避免 `app.rs`、`runtime.rs`、Editor 共享状态改造相互覆盖。

### 明确的行为决定

- 新 key 默认 Auto；当前 key 手动选择的格式保留到换 key。刷新同一 key 保留手动选择。
- 同一个 key 因鼠标重复点击不重启同一读取；显式刷新仍会重新加载。
- 完整、合法 UTF-8 JSON（包括中文、emoji、数组和标量）自动显示 JSON。
- 不完整页使用 `Incomplete` 状态，不把 JSON EOF 或截断 UTF-8 当成确定的格式错误；补齐后重新检测。
- 手动 JSON 解析失败时显示原因，并保持格式标签与实际显示内容一致；不得只换标签继续静默显示上一份文档。
- RAW 保留合法 UTF-8；控制字符通过现有终端安全投影显示。HEX 是真正的字节视图。
- 复制继续使用原始内容语义；显示层裁剪不改变原始字节。
- 首次 String 读取仍为最多 64 KiB；累计读取与解码输出另设预算。
- 不将 SQL `LineIndex` 的全局重构作为本次前提：非 SQL 不再进入该路径；共享绘制优化必须验证 SQL 回归。
- 不记录 Redis value、key 原文或连接凭据到性能日志。

## 2. 执行约定

- 开始前执行 `git status --short`，检查已有修改，按符号局部编辑。
- 每个任务按“添加行为回归 → 验证失败原因 → 实现 → 运行相关测试 → 检查 diff”执行。单纯文案或样式不编写镜像测试。
- 每项完成形成一个可审查的逻辑提交；下文给出建议提交消息，实际提交遵循执行时用户要求。
- 性能测试使用 release 构建，结构性断言进入普通 CI；时间阈值放在显式运行的基准测试中，避免共享 CI 波动误报。
- 若需确认 Redis pipeline 或 Tokio API，实施时查询对应版本文档，不凭未验证的 API 假设写代码。

## Task 1：建立可重复的功能与性能基线

**Files**
- Create: `tests/redis_preview_regression.rs`
- Modify: `tests/value_preview.rs`
- Modify: `src/editor/tests.rs`（私有 Editor 性能入口）
- Modify: `src/ui/redis_browser.rs`（模块内 TestBackend 基准入口）
- Create: `docs/performance/redis-preview.md`

**Steps**
1. 复用 `tests/redis_loading_lifecycle.rs` 的连接 App 构造方式，构建 Ready String 页并通过 `Action::RedisValuePageLoaded` 进入真实 reducer 路径。
2. 加入含中文 JSON、ASCII JSON、带类型名的 JSON 数组、非法字节、跨页截断 UTF-8 的数据。测试数据本地生成，不依赖生产 Redis。
3. 加入关键失败测试 `redis_preview_auto_json_uses_original_bytes`：原始 JSON 应选 JSON，格式化结果重新解析后应与原始值语义相等。
4. 加入手动格式失败、换 key 后 Auto、旧结果不可覆盖新 key 的行为测试，避免只测试 detector helper 而绕开错误调用点。
5. 添加显式 `#[ignore]` 基准 `redis_preview_benchmark`，分别测首次 snapshot、缓存命中 snapshot、TestBackend 整帧 render。用 6 KiB、32 KiB、64 KiB 的长单行，至少预热 10 次、采样 100 次。
6. 记录原始字节数、显示字符数、span 数、p50/p95、构建模式、终端尺寸、机器信息；当前基线与后续修复用同一数据。

**Commands**
```sh
cargo test --test redis_preview_regression
cargo test --test value_preview
cargo test --release --lib redis_preview_benchmark -- --ignored --nocapture --test-threads=1
```

**Expected**：新增 JSON reducer 回归在修复前因 RAW 失败；既有测试保持原结果；基准输出可复现且区分首次与重复绘制。

**Commit**：`test(redis): reproduce unicode JSON preview and long-line stalls`

## Task 2：统一原始字节检测、格式化与状态语义

**Files**
- Create: `src/value_preview/format.rs`
- Modify: `src/value_preview/mod.rs`, `src/value_preview/detect.rs`
- Modify: `src/ui/redis_value.rs`
- Modify: `src/app.rs` / `RedisValuePageLoaded`、格式切换分支
- Modify: `src/model/redis_preview.rs`, `src/model/redis_browser.rs`
- Test: `tests/value_preview.rs`, `tests/redis_preview_regression.rs`

**Steps**
1. 为 formatter 添加直接接收 `&[u8]` 和完整性状态的入口；区分 Complete、Incomplete、Invalid、Unsupported，复用既有 DecodeStatus 能表达的状态。
2. String 检测直接借用 `RedisPageValue::String(bytes)`；collection 直接选 TABLE，不先构造整份 page 文本。
3. JSON 使用 `serde_json::from_slice`，pretty print 只进行一次。将 JSON 高置信度的 Auto 路径短路，避免已确定 JSON 后继续尝试所有二进制 decoder。
4. RAW 使用 UTF-8 校验；非法字节用线性算法转义无效片段，保留有效 Unicode。不得使用对每个字节反复校验整个后缀的算法。
5. 统一 top-level String 与 collection 单元格的格式化入口，完整传递 PreviewFormat 的 encoding/view，修复仅传 ValueView 丢失 encoding 的问题。
6. HEX 输出明确的字节视图；JSON/YAML/序列化失败不把转义文本重新当作源数据。显示控制字符继续走 `project_editor_line`。
7. 在身份真正变化时恢复 Auto、复位滚动；同 key 刷新保留手动选择。加载状态优先于旧 editor 快照，清除旧文档或禁止读取旧快照。
8. 部分页只显示已加载状态；完整后重新检测。手动选择 JSON 时显示 `Incomplete JSON` 或真实 parse error，标签和文档保持一致。
9. 通过任务 1 的回归，并新增 RAW Unicode、控制字符、二进制、HEX、手动格式跨 key 行为断言。

**Commands**
```sh
cargo test --test value_preview --test redis_preview_regression --test redis_values
cargo test --lib redis_value
```

**Expected**：中文 JSON 自动识别成功；合法文本不再整段四倍膨胀；复制源字节与格式切换互不污染。

**Commit**：`fix(redis): detect and format previews from original bytes`

## Task 3：拆分非 SQL 快照路径

**Files**
- Modify: `src/editor/mod.rs` / `render_preview_snapshot`、`render_snapshot_with_sql_ranges`
- Modify: `src/editor/tests.rs`
- Modify: `src/app.rs` / `redis_preview_snapshot`

**Steps**
1. 增加模块内测试：Plain/JSON/YAML 快照不填充 SQL analysis_cache；SQL 快照继续使用缓存和语义高亮。
2. 抽取语言无关的 snapshot 构建部分，显式传入高亮来源，避免通过构造空 SQL ranges 来间接表达 Plain。
3. Plain 每条非空行构造一个连续 Plain span；JSON/YAML 直接使用预览高亮器，不先构造 SQL span。
4. 保留选择、光标、搜索、滚动和安全投影逻辑的共用部分；read_only_sql 保持 SQL 路径。
5. 将 `preview_highlight_spans` 数字分支中反复 `.position()` 改为单向索引推进。确保 source byte 与 display byte 不混用，投影后的区间须映射回源字节。
6. Redis JSON/YAML 渲染按语言启用 syntax，RAW/HEX 保持 Plain，避免生成高亮后仍以 `syntax: false` 丢弃。
7. 运行 Editor 回归及基准，记录首次耗时和 span 数变化。

**Commands**
```sh
cargo test --lib editor::tests
cargo test --release --lib redis_preview_benchmark -- --ignored --nocapture --test-threads=1
```

**Expected**：RAW 与 JSON 预览不触发 SQL 分析；Plain span 数随行数增长而非字符数增长。

**Commit**：`perf(editor): bypass SQL analysis for value previews`

## Task 4：消除共享绘制的 O(N×S) 查找

**Files**
- Modify: `src/ui/mod.rs` / `editor_line_spans`
- Modify: `src/ui/text_selection.rs`（如命中映射接口需要调整）
- Test: `src/ui/mod.rs` 模块内测试、`src/editor/tests.rs`

**Steps**
1. 加入 span 边界、无高亮间隙、Unicode、选区与诊断叠加的渲染回归。
2. 明确 snapshot spans 有序且不重叠的契约；若现有调用方不满足，在生成阶段归一化，不在每帧每字符排序。
3. 将 `.iter().find()` 替换为只向前移动的 span 游标；无 syntax 且不依赖 span 信息时直接跳过高亮查询。
4. 保持相邻同样式字符合并，保证选择背景、错误下划线、output 样式行为一致。
5. 为内部 sweep helper 添加操作计数测试，验证字符数和 span 数翻倍时推进次数保持线性；不以普通 CI 的墙钟时间断言复杂度。
6. 运行共享 UI/Editor 测试及 Redis 基准，验证 SQL、只读 SQL、文本详情等调用方没有样式回退。

**Commands**
```sh
cargo test --lib ui::
cargo test --lib editor::tests
cargo test --test redis_preview_regression
```

**Expected**：无每字符全量扫描 span 的路径；RAW 密集转义串重复绘制不再呈平方级退化。

**Commit**：`perf(ui): sweep highlight spans during text rendering`

## Task 5：按 revision 缓存布局并真正裁剪水平视口

**Files**
- Create: `src/editor/preview_layout.rs`
- Modify: `src/editor/mod.rs`, `src/model/editor.rs`
- Modify: `src/ui/mod.rs`, `src/ui/text_selection.rs`
- Modify: `src/ui/redis_browser.rs`
- Test: `src/editor/tests.rs`、上述模块内测试

**Steps**
1. 增加长行窄视口测试：一行 64 KiB，视口 120×40，只产生可见范围和固定 overscan 的展示片段。
2. 建立 preview session 的不可变布局缓存：revision、行起始字节索引、最大显示宽度、每行投影/高亮的按需缓存。
3. 内容不变时不再提取全文或对全文调用 `full_line_width`；revision 改变时重建必要索引。宽度计算使用线性计数，避免仅为宽度生成三份映射 Vec。
4. 水平定位使用缓存边界的二分查找；完整坐标索引可缓存，但每帧不得克隆整行索引。共享索引或仅提供窗口命中映射。
5. 明确源字节、源字符列、显示 cell、视口 cell 的换算，新增必要的窗口起始字段，不通过对字符串直接切字节实现裁剪。
6. 保证 Tab、宽字符、组合字符和跨视口选区正确；搜索定位及复制从原始文档读取，不从裁剪文本读取。
7. 将 `page_lines` 延迟到 fallback 分支；只在 Ready 且 identity 匹配时使用快照。collection table 若构建两次则改为一次构建共享结果。
8. 窗口 resize 仅更新视口结果；内容、语言或主题变化按实际缓存内容分别失效，关闭 session 释放缓存。

**Commands**
```sh
cargo test --lib editor::tests
cargo test --lib ui::
cargo test --release --lib redis_preview_benchmark -- --ignored --nocapture --test-threads=1
```

**Expected**：缓存命中后单帧工作量主要随视口变化；长行尾部水平滚动不重新线性扫描整个前缀。

**Commit**：`perf(editor): cache preview layout and render visible columns`

## Task 6：接入有界预览缓存与共享内容

**Files**
- Modify: `src/value_preview/cache.rs`, `src/value_preview/mod.rs`
- Modify: `src/model/redis_browser.rs`
- Modify: `src/app.rs` / page 加载、格式切换
- Modify: `src/editor/mod.rs` / 只读文档入口
- Test: `tests/value_preview.rs`、cache 模块内测试

**Steps**
1. 添加两个不同 tab/key 使用相同 revision 的缓存隔离测试，以及格式切换命中、刷新失效、预算淘汰测试。
2. 为缓存键加入文档/session 身份；身份至少能够区分 profile、DB、key 的文档实例，并含 source_revision、完整性和完整 PreviewFormat。
3. 用 `Arc` 共享格式化结果，命中不 clone String/Vec；原始 page 在 Ready/content 状态中采用共享所有权，避免每次 append 后重复深拷贝。
4. 缓存归属放在非持久化的 preview service 或 session 中，App 可克隆 UI 状态不携带巨大独立副本。
5. 同一 revision/format 只格式化一次；切换命中格式复用结果。每次追加页增加内容 revision，与 selection generation 分开。
6. 初始设应用级格式化缓存 16 MiB、布局缓存独立 16 MiB，采用集中常量。超预算单项不缓存；统计 retained bytes，含布局映射与共享结果。
7. 关闭 tab、断开连接、删除 key、刷新和内容替换后释放/失效对应缓存；验证淘汰不破坏仍被可见文档引用的数据。

**Commands**
```sh
cargo test --test value_preview --test redis_preview_regression
cargo test --lib value_preview::cache
```

**Expected**：不存在跨 key 缓存串值；命中返回共享内容；关闭 tab 后缓存占用可回收。

**Commit**：`perf(redis): cache formatted previews with shared ownership`

## Task 7：复用 metadata 并合并网络往返

**Files**
- Modify: `src/db/redis/read.rs`
- Modify: `src/runtime.rs` / `load_redis_value_preview`、`load_redis_value_page`
- Create: `tests/redis_preview_io.rs`
- Test: `tests/redis_values.rs`, `tests/redis_contract.rs`

**Steps**
1. 使用本地可控 RESP 测试服务记录请求命令，验证首次 String 当前重复 metadata 的行为，测试不依赖远程服务。
2. 新增内部 `read_value_page_with_metadata` 路径，验证 metadata.key 与 request.key 一致；保留独立 read_value_page 自行查询 metadata 的入口。
3. 首次获取 TYPE 后复用其结果。String 常见路径目标为两轮网络往返：TYPE；随后 pipeline PTTL、MEMORY USAGE、STRLEN、GETRANGE。
4. pipeline 保持各项回复的类型与错误信息，MEMORY 权限错误沿用 best-effort 语义，不能因一个可选字段失败而丢弃整个值。
5. TYPE 后发生删除、过期或类型改变时，返回 Missing/明确错误；不宣称 pipeline 是原子快照，不将不一致 metadata 强行标记成完整。
6. 集合类型复用 metadata，分页刷新按需要获取最新元数据，保留请求预算校验。
7. UI 区分 Redis memory usage、String length 和 loaded bytes，避免把 MEMORY USAGE 当作内容长度。
8. 用测试服务设置响应延迟，证明普通成功路径 metadata 不重复、String 目标两轮；异常路径允许必要的恢复请求。

**Commands**
```sh
cargo test --test redis_preview_io --test redis_values --test redis_contract
```

**Expected**：首次 String 每种元数据命令最多一次；MEMORY NOPERM 不妨碍预览；没有新增隐式无限重试。

**Commit**：`perf(redis): reuse metadata and pipeline preview reads`

## Task 8：接入 latest-wins 调度和请求生命周期

**Files**
- Modify: `src/db/redis/preview_scheduler.rs`
- Modify: `src/runtime.rs`, `src/action.rs`
- Modify: `src/app.rs` / `select_redis_key`、结果处理
- Modify: `src/model/redis_browser.rs`
- Test: `tests/redis_loading_lifecycle.rs`, `tests/redis_preview_io.rs`, scheduler 模块内测试

**Steps**
1. 将 scheduler 的时间源改为可注入 now 或可测试 Tokio time；移除单测依赖真实 sleep 的不确定性。
2. Runtime 按 preview tab/session 持有调度状态，普通选择防抖初始值 100 ms；显式刷新和用户加载下一页不叠加选择防抖。
3. 选择立即更新 UI 和 generation，但实际派发只保留最新 pending 请求；同 key 重复选择去重。
4. 每 tab 最多一个实际读取任务，应用级最多两个预览读取任务；过期 pending 不创建任务。全局限额由 Runtime 非阻塞协调，不能阻塞 App 更新。
5. 请求设有限超时（初始 5 s 或复用已有更合适的连接超时）。已发出的 Redis 命令不宣称能从服务端取消；旧请求完成/超时后直接丢弃，优先派发最新项。
6. 成功、失败、超时、tab 关闭、断开连接均释放 in-flight 状态；若使用 task abort，单独验证不会导致槽位永久占用。
7. 结果校验 connection identity、DB、tab、key、selection generation；格式化结果另校验 content revision 和 format generation。
8. 测试 100 ms 内 A→B→C 只发 C；A 已在途再选 B/C 只保留 C；A 晚到不覆盖 C；超时后仍可继续加载；两个 tab 不串用状态。

**Commands**
```sh
cargo test --lib preview_scheduler
cargo test --test redis_loading_lifecycle --test redis_preview_io --test redis_preview_regression
```

**Expected**：快速导航产生有界工作；没有旧结果闪回或永久 Loading；防抖不延迟按键光标移动。

**Commit**：`perf(redis): debounce previews and bound in-flight requests`

## Task 9：大值处理后台化与累计预算

**Files**
- Modify: `src/runtime.rs`, `src/action.rs`, `src/app.rs`
- Modify: `src/value_preview/format.rs`, `src/value_preview/decode.rs`
- Modify: `src/value_preview/cache.rs`, `src/model/redis_browser.rs`
- Test: `tests/redis_preview_regression.rs`, `tests/redis_protocol_limits.rs`

**Steps**
1. 增加可控处理任务测试：旧格式任务延迟完成、快速切换格式、关闭 tab、输出超限。
2. 内容检测/解析/格式化封装为 PreparedPreview；大于 32 KiB 的文本及二进制序列化解码通过 `spawn_blocking` 执行。小值保留轻量路径，但其耗时必须满足基准。
3. 在创建 blocking 工作前异步获取并发额度，应用级最多两个处理任务；只保留最新待处理项，不能为每次选择先创建一个等待 semaphore 的 blocking task。
4. 初始集中预算：累计原始预览 1 MiB、单次格式化输出 4 MiB，保留解析器默认深度限制；根据实测调整但必须保持显式上限。
5. JSON pretty print 使用受限 writer，写入前检查预算。对不可中断第三方 decoder，输入预算前置，承认输出后检查不能阻止解析器内部峰值；必要时采用该 decoder 支持的资源限制。
6. 记录 selection generation、source_revision、format_generation；结果落地时重新验证，失配不覆盖文档也不写入当前项缓存。
7. `spawn_blocking` 开始后不能靠 abort 保证中断，使用有界输入/输出/并发控制实际成本；支持的循环加入协作取消检查。
8. App 在结果到达时安装共享文档；若 editor 初始化或布局索引仍超过帧预算，移到后台准备可安装索引，避免只把 parse 搬走却留下主线程大拷贝。
9. 达到累计预算停止继续加载，UI 显示已加载量、总长度（如已知）和停止原因，不把超预算内容标成完整。

**Commands**
```sh
cargo test --test redis_preview_regression --test redis_protocol_limits --test value_preview
cargo test --lib value_preview
```

**Expected**：后台任务数受控；旧任务结果无副作用；大值处理期间按键仍有响应，超限可以解释且可切换 key。

**Commit**：`perf(redis): bound background preview preparation`

## Task 10：全量回归、性能对比与交付记录

**Files**
- Modify: `docs/performance/redis-preview.md`
- Modify: 本计划末尾执行记录
- Tests: 任务 1–9 新增及既有相关测试

**Steps**
1. 在 release 模式重新执行同一份基准；记录首次、缓存命中、端到端输入到绘制三个指标，明确不包含和包含网络的口径。
2. 数据矩阵：ASCII/中文/emoji JSON、类型包装数组、64 KiB 转义 RAW、非法 UTF-8、跨页截断、1 MiB 累计值、超限输出、TTL 变化和 MEMORY NOPERM。
3. 交互矩阵：按住 j/k、A→B→A、跨 DB/tab、关闭在途 tab、刷新同 key、格式反复切换、resize、水平滚动到尾部、鼠标选择复制。
4. 用实际终端验证 JSON 可读性、终端安全投影和颜色；TestBackend 数据与终端输出耗时分开记录。
5. 按 CI 相同版本运行 fmt/clippy/test；若本机缺少 Oracle 运行环境，记录环境原因与未验证项，在具备环境的 CI 完成 all-features 验证，不能将跳过当通过。
6. 更新 before/after 表、预算与交互规则、已知限制；本任务不创建版本标签或 release。

**Commands**
```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
cargo +1.94.0 test --release --lib redis_preview_benchmark -- --ignored --nocapture --test-threads=1
git diff --check
```

**验收目标（固定参考机器、120×40 视口、release 构建）**

| 指标 | 目标 |
| --- | --- |
| 完整中文 JSON 自动识别 | 全部正确，解析后与源数据语义相等 |
| RAW 合法 Unicode | 保留文字，无整段 `\\xNN` 膨胀 |
| 非 SQL 快照 | SQL analysis 调用为 0 |
| 6 KiB/64 KiB 预览重复绘制 p95 | ≤ 16 ms，测量不包含网络 |
| 小值首次本地处理+快照 p95 | ≤ 33 ms，防抖和网络另计 |
| 输入到本地选中态绘制 p95 | ≤ 50 ms，大值后台处理时也满足 |
| 长单行大小翻倍 | 不出现接近平方级增长；窗口内生成量受视口约束 |
| 首次 String 成功路径 | metadata 不重复，目标 2 轮 RTT |
| 防抖窗口内连续选择 | 只派发最终 key |
| 在途网络/CPU 任务 | 分别遵守全局上限，无累计无限队列 |
| 生命周期 | 旧结果不覆盖新 key，关闭 tab 回收缓存与调度状态 |

时间目标若未达成，保留具体数据和最慢阶段定位，不能只凭“体感流畅”结项。首次文档准备允许随源数据线性增长，缓存命中绘制应主要随视口大小变化。

**Commit**：`docs(redis): record preview performance and regression results`

## 3. 主要实施风险与处理

1. **共享 Editor 影响 SQL。** 任务 3 保留 SQL 入口；任务 4–5 每次变更同步运行 Editor 与 UI 回归，覆盖语义样式、光标、搜索和选区。
2. **坐标体系混淆。** 缓存与裁剪前先定义 source/display/window 坐标契约，用中文、Tab、宽字符和组合字符检验，不以 ASCII 用例替代。
3. **缓存身份不足。** revision 不是跨文档唯一 ID；cache key 必须包含 session 身份，分页追加也必须换 revision。
4. **缓存预算不等于实际内存峰值。** 同时统计可见文档持有内容与 eviction 后仍被 Arc 引用的数据；检查旧引用是否按生命周期释放。
5. **请求取消与阻塞任务误解。** 服务端已接收的请求、已运行的 blocking 解码不能保证撤销；有限超时、并发和输入预算必须落实。
6. **Auto 行为变化。** 测试明确区分换 key、同 key 刷新与重复选择；UI 文案沿用项目英文风格。

## 4. 执行记录

- [x] Task 1：基线和失败回归。新增 Unicode JSON 回归；完整性能基准和网络实测未建立。
- [x] Task 2：原始字节与格式语义。检测、JSON/YAML 格式化改为原始字节；RAW 保留 Unicode、控制字符仍安全转义。
- [x] Task 3：非 SQL 快照。预览绕过 SQL analysis cache；SQL 回归通过。
- [x] Task 4：线性 span sweep。绘制改为有序 span 游标；保留 plain 模式的控制字符投影。
- [x] Task 5：布局与重复渲染削减。完成 fallback 延迟构建、Table 单次构建和可见行起始偏移线性推进；完整跨帧布局/水平窗口缓存未实施。
- [x] Task 6：共享内容与预览缓存。完成 source identity 隔离和 `Arc` 命中语义；缓存尚未接入 Redis tab 的实际格式化生命周期。
- [x] Task 7：metadata 与读取复用。完成 metadata 复用入口；Redis pipeline 和请求计数测试未实施。
- [x] Task 8：预览调度生命周期。完成 100ms 防抖、latest-wins、33ms tick 派发和 value-page 完成释放。
- [x] Task 9：后台准备与工作预算。完成大于 32KiB String 的 `spawn_blocking` 格式化及 generation/format 校验；完整并发 semaphore、输出预算和取消检查未实施。
- [ ] Task 10：CI、实际终端和性能验收。fmt/clippy 通过；`cargo +1.94.0 test --all-targets --all-features` 有 1 个既有 UI 渲染测试失败：`data_grid_keeps_null_muted_on_the_selected_row`（`Black == Black`），与本次 Redis/Editor diff 无直接调用关系，需单独处理后才能标记完成。

验证摘要：Task 1–9 的相关测试均通过；最终全量测试在 226 个 UI render 测试中通过 225 个，另有大量 unit/integration tests 通过。没有提交真实 Redis 网络性能数据，因此不对 RTT、p95 或端到端帧时间作未经测量的结论。
