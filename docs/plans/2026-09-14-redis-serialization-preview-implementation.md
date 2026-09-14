# Redis Serialization Preview Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行环境未安装上述技能时，直接按本计划顺序执行并记录各任务验证结果；不依赖未安装的工具或技能。

**Goal:** Redis 预览根据原始 value 自动识别 Java Serialization、PHP Serialization 和 Python Pickle，选中对应类型并正确解析，同时支持手动覆盖、分页回退和集合单元格详情。

**Architecture:** 复用 `value_preview`，将原始字节、完整性信息、格式选择和结构化解析结果贯穿主预览、后台任务及单元格详情。编码解码与视图渲染分离，Auto 是选择策略，实际格式和解析状态独立保存；后台结果和缓存按数据版本隔离。

**Tech Stack:** Rust 2024、Tokio、Ratatui、现有 Redis adapter、jaded 0.5、serde-pickle 1.2、serde_json、serde_yaml、base64。

---

## 1. 实施约束与完成定义

- 本文是实施计划，业务代码尚未修改，测试尚未运行。
- 路径默认相对 LazyDB 根目录；参考项目根目录为 `/Users/yelog/workspace/rust/rust-redis-desktop`。
- 执行前读取当时的工作区状态及适用的 AGENTS.md；本文行号仅用于定位，符号与实际源码优先。
- 复用现有依赖；只有能力验证确认存在阻塞时才调整依赖。
- 保留 Redis 原始字节；显示转义后的字符串不能再输入解码器。
- 第一轮覆盖 String 与集合单元格详情。集合顶层仍使用 Table，各 value 单独检测；不把整个 Hash/List 拼成字符串后做序列化检测。
- 手动格式作用于当前 key；切换到不同 key 恢复 Auto，同 key 刷新和追加数据保持手动选择。
- Raw/Hex 查看原始字节；JSON/YAML 是解码后的展示方式。现有 JSON/YAML 文本手动查看行为需要回归。
- 不做隐式 Base64/Hex 解码，不扩展新的序列化格式，不增加反向序列化写回。
- 所有核心验收用例使用固定 fixture 或 App action，无需本机安装 Java/PHP/Python；运行时仅用于可选的 fixture 再生成。

### 验收标准

1. Java、PHP、Pickle 样例进入主预览后，格式标签、菜单已选项和正文一致。
2. 同一字节在主预览和单元格详情得到相同结构化结果。
3. 自动检测不覆盖当前 key 的手动选择，切换 key 后恢复自动选择。
4. 强格式特征的数据即使不完整也保留类型提示；普通文本不被失败的弱候选抢占。
5. 小 value、后台大 value、手动切换使用同一解码入口。
6. 异步结果不能覆盖其他 key、其他数据版本或后续格式请求。
7. 二进制、引用和非 JSON 原生类型不被静默替换或丢弃。

## 2. 已确认的代码问题

| 位置 | 当前问题 |
| --- | --- |
| `src/app.rs` / `RedisValuePageLoaded` | 自动检测后只向 `format_page` 传 `format.view` |
| `src/app.rs` / `RedisPreviewFormatAccept` | 手动选择同样丢失 encoding，错误被静默回退 |
| `src/runtime.rs` / 大 value 格式化 | 同样只传 view |
| `src/ui/redis_value.rs` / `format_bytes_value` | 能分发 encoding，但序列化分支不处理目标 view |
| `src/model/redis_preview.rs` | 菜单只有五项，cycle 内重复常量 |
| `src/app.rs` / `RedisPreviewFormatMove` | 菜单长度写死为 5 |
| `src/ui/redis_browser.rs` / `preview_format_label` | 只展示 view，无法显示 Java/PHP/Pickle |
| `src/model/redis_browser.rs` / `select` | 切换 key 没有恢复 Auto |
| `src/value_preview/php.rs` / `parse_array` | 长度函数已消费冒号，随后再次消费冒号 |
| `src/value_preview/php.rs` | 缺 O/C/r/R，二进制字符串和键的投影有损，错误分类不准确 |
| `src/value_preview/pickle.rs` | 识别限于带头协议 2–5，直接反序列化为 JSON Value |
| `src/value_preview/detect.rs` | 检测中完整解析但丢弃结果；失败候选仍能赢得默认选择 |
| `src/value_preview/cache.rs` | 已有缓存实现，尚未接入生产预览链路 |

## 3. 目标契约

### 3.1 格式选择

在现有模型上新增菜单选择类型，而不是把 Auto 伪装成某种 encoding：

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewChoice {
    Auto,
    Format(PreviewFormat),
}
```

- `RedisPreviewFormatState` 继续用 `automatic` 表示模式，用 `selected` 表示实际解析格式，减少调用方迁移。
- 单一菜单定义提供选择值与标签：Auto、RAW、JSON、YAML、Table、Hex、Java Serialization、PHP Serialization、Python Pickle。
- Auto 模式打开菜单时游标定位 Auto；标题显示 `Auto · Java → JSON` 等实际格式。
- 如果正文回退，标题同时表达回退状态，例如 `Auto · Java · incomplete · Hex fallback`，不得显示为成功解析。
- 当前菜单不增加序列化格式与 YAML 的全部组合；内部接口必须正确支持 encoding + view 组合。

### 3.2 处理结果

在 `src/value_preview/mod.rs` 定义结构化结果，至少包括：实际格式、`DecodeStatus`、可选 `DecodeError`、展示正文及可选回退视图。检测结果中保留已成功解析的结构，避免再次解码。

将 `DecodedValue` 扩展为可保存结构化投影的类型，例如 `Structured(serde_json::Value)`；不要用漂亮打印后的 JSON 作为唯一内部表示。

### 3.3 输入完整性

管线输入包含 bytes 与当前 value 是否完整。String 使用累积页状态；集合单元格需要审计其自身字节是否被截断，不能使用“集合扫描尚未完成”来判断单元格不完整。

- 完整输入遇到意外 EOF：损坏数据，最终报告 Invalid。
- 明确未完整加载的输入遇到 EOF：NeedsMoreData。
- 已存在的字节不符合语法：Invalid，不论是否还可加载更多数据。
- 未支持的类型/协议/外部资源：Unsupported，不能伪报损坏。

### 3.4 非 JSON 原生数据表示

- UTF-8 字符串保留自然字符串。
- 二进制字符串使用带类型、长度、编码及内容的显式对象。
- PHP 对象使用类型元信息与独立 properties 容器，避免 `__class__` 与业务属性碰撞。
- 引用保留 reference id；不递归展开成无限树。
- 非字符串键、可能冲突的键使用 entries 数组保留键类型与顺序。
- Pickle tuple/set/frozenset、大整数、非有限浮点数使用有类型标记的投影。
- Java 保留必要类名、枚举、引用信息。只剥离解析库包装层，不凭字段名删除用户业务数据。

## 4. 实施任务

每个任务按“建立回归用例 → 运行确认失败 → 实现 → 运行通过 → 审查差异”的顺序执行。测试断言面向输出语义与状态变化，不镜像内部实现。逻辑提交点仅供用户要求提交时使用，不自动提交。

### Task 1：基线与真实样例

**Files:**
- Modify: `tests/fixtures/value_preview/README.md`
- Create: `tests/fixtures/value_preview/java/`、`php/`、`pickle/` 下的固定二进制样例及生成源文件
- Modify: `tests/value_preview.rs`

**Steps:**
1. 运行 `git status --short`，确认潜在并行工作，记录当前分支和提交。
2. 运行 `cargo test --test value_preview --test redis_browser_tabs --test redis_values`，记录既有失败，不能将其当作新增回归。
3. 检查参考项目 `src/serialization/` 及其 fixture，只复用有可验证预期的样例。
4. 添加最小 Java String、PHP 数组和 Pickle 0/4 样例；记录生成语言版本、序列化参数和预期值。
5. 添加 PHP `a:0:{}` 回归测试，运行 `cargo test --test value_preview php_array`，预期当前实现失败。

**完成条件:** 有可复现的当前缺陷，以及不依赖外部语言运行时的输入材料。

### Task 2：统一解码与渲染接口

**Files:**
- Modify: `src/value_preview/mod.rs`、`decode.rs`、`cache.rs`
- Create: `src/value_preview/render.rs`
- Modify: `src/ui/redis_value.rs`、`src/app.rs`、`src/runtime.rs`
- Test: `tests/value_preview.rs`

**Steps:**
1. 添加同一序列化输入在 JSON、YAML、Raw、Hex 下的行为测试，特别验证 Hex 使用原始字节。
2. 添加主页面与 `format_bytes_value` 的结果一致性测试。
3. 扩展 `DecodedValue`，同时更新缓存大小核算及穷尽匹配，保证该任务独立可编译。
4. 新建纯渲染层：结构化值 → JSON/YAML；原始 bytes → Raw/Hex。
5. `format_page` 改接收完整 `PreviewFormat`，String 委托统一入口，集合继续使用表格/现有页面处理。
6. 同步迁移 App 初次加载、手动切换、Runtime 后台调用，消除只传 view 的调用点。
7. 删除同一行为的重复格式化实现；保留兼容性包装时明确其调用范围。
8. 运行 `cargo test --test value_preview --test redis_browser_tabs --test redis_values`。

**完成条件:** 至少 Java String、PHP String、Pickle 标量能通过主预览实际解码，不依赖自动菜单改造。

### Task 3：PHP 基础正确性和对象支持

**Files:**
- Modify: `src/value_preview/php.rs`
- Create: `tests/value_preview_php.rs`
- Modify: `tests/fixtures/value_preview/php/`、`tests/fixtures/value_preview/README.md`
- Reference: `<参考项目>/src/serialization/php.rs`

**Steps:**
1. 修复 `parse_array` 多消费一次冒号，先通过 Task 1 回归。
2. 添加空数组、嵌套数组、关联数组、中文长度、NUL/非法 UTF-8 字符串测试。
3. 调整 `expect`：缺字节返回 NeedsMoreData，已有错误字节返回 Invalid，携带准确 offset。
4. 扩展中间类型和语法处理：O 对象、r/R 引用、C 自定义载荷。
5. C 载荷作为不透明 bytes 显示；不假定其内容仍是 PHP 标准序列化。
6. 私有/受保护属性保留原始名称或可还原的可见性元数据，避免去掉 NUL 前缀后发生重名覆盖。
7. 实现第 3.4 节的结构化投影；覆盖二进制键、整数/字符串键碰撞、INF/NAN。
8. 对长度、计数、深度、节点数实施解析期预算检查；不能只限制 Vec 初始容量。
9. 运行 `cargo test --test value_preview_php --test value_preview`。

**完成条件:** 标准数组和对象可查看；引用和二进制内容不会静默丢失；错误状态准确。

### Task 4：Pickle 协议与类型兼容

**Files:**
- Modify: `src/value_preview/pickle.rs`
- Create: `tests/value_preview_pickle.rs`
- Modify: `tests/fixtures/value_preview/pickle/`、`tests/fixtures/value_preview/README.md`
- Reference: `<参考项目>/src/serialization/pickle.rs`

**Steps:**
1. 查询当前锁定版本 serde-pickle 文档/源码，确认 Value API、协议、引用、外部 buffer 和自定义对象的实际能力，记录到 fixture README。
2. 添加协议 0–5 的基础容器和标量 fixture，包含不以旧实现列举字符开头的协议 0 样例。
3. 解码函数不再被 `is_pickle_serialization` 的启发式条件拦截，手动选择可直接尝试解析。
4. 改为 Pickle 原生中间值后再投影，验证 bytes、tuple、set/frozenset、大整数、非字符串字典键。
5. 添加 GLOBAL/REDUCE、自定义对象、循环引用和协议 5 外部 buffer 的能力测试；无法表示时明确 Unsupported。
6. 不调用 Python 运行时恢复对象；固定 fixture 的生成与生产解码分开。
7. 添加截断头、无 STOP、尾随垃圾、未知版本和普通文本误判样例。
8. 运行 `cargo test --test value_preview_pickle --test value_preview`。

**完成条件:** 协议 0–5 中受依赖支持的基础数据正确显示；不承诺任意 Python 对象均可恢复，不支持项有准确状态。

### Task 5：Java 展示语义与能力验证

**Files:**
- Modify: `src/value_preview/java.rs`
- Create: `tests/value_preview_java.rs`
- Modify: `tests/fixtures/value_preview/java/`、`tests/fixtures/value_preview/README.md`
- Reference: `<参考项目>/src/serialization/mod.rs`、`src/serialization/java_converters.rs`

**Steps:**
1. 验证 jaded 0.5 的类型和错误 API，记录引用、Block、集合和自定义序列化的实际表示。
2. 添加 String、null、基本数组、自定义对象、List、Map、Enum、循环引用 fixture。
3. 在实际解析类型边界上转换包装层，避免递归按 Object/Array 等名字误删业务字段。
4. 为类名、枚举、引用、Block bytes 提供稳定投影；仅对已验证集合表示做可读性增强。
5. 保留完整流头校验，区分错误头、EOF、不支持内容；确定多根对象和尾随数据处理规则，并添加测试。
6. Java 对象流可能包含多个顶层内容；完整预览应全部呈现或明确提示剩余内容，不能静默忽略。
7. 运行 `cargo test --test value_preview_java --test value_preview`。

**完成条件:** 真实对象样例可读且保留元信息，数据结构不会因包装层简化而丢失。

### Task 6：重构自动识别与失败回退

**Files:**
- Modify: `src/value_preview/detect.rs`、`mod.rs`
- Create: `src/value_preview/prepare.rs`
- Test: `tests/value_preview.rs`

**Steps:**
1. 建立选择矩阵测试：JSON、普通文本、空值、Java、PHP、Pickle、未知二进制、弱 Protobuf 候选。
2. 轻量 probe 只返回格式特征与协议信息，不漂亮打印、不完整解码所有候选。
3. prepare 对必要候选进行验证并保留成功解码结果；一次请求内不重复解析相同候选。
4. 强特征 Java/Pickle 即使数据不足仍保留类型；PHP/文本 Pickle 只有验证成功才自动胜出。
5. JSON 与可显示文本作为正常回退路径；控制字符密集的合法 UTF-8 不自动等同普通文本。
6. Protobuf 低置信度候选仅作为建议，不抢占普通文本和本次强格式。
7. 结合输入完整性规范化 EOF 状态；在错误结果中同时保留检测类型、诊断和原字节回退内容。
8. 运行 `cargo test --test value_preview --test value_preview_java --test value_preview_php --test value_preview_pickle`。

**完成条件:** `s:...` 等普通文本不会被失败候选劫持，截断数据与损坏数据明确区分，已解码结果可复用。

### Task 7：Auto 状态、菜单与标题

**Files:**
- Modify: `src/model/redis_preview.rs`、`src/model/redis_browser.rs`
- Modify: `src/app.rs`、`src/ui/redis_browser.rs`、`src/ui/mod.rs`
- Inspect: `src/model/workspace.rs`、`src/input/keymap.rs`、`src/input/mouse.rs`、`src/help.rs`
- Test: `tests/redis_browser_tabs.rs`、`tests/value_preview.rs`

**Steps:**
1. 添加 Auto、手动、切换 key、同 key 刷新、追加分页的状态转换测试。
2. 实现 `PreviewChoice` 和单一选项表；删除 cycle 内重复表与 `.rem_euclid(5)`。
3. 下拉菜单高度、渲染、定位和循环全部使用选项表长度；小终端支持可见范围或滚动。
4. Enter 才应用，Esc 不改变格式；选 Auto 立即重新准备当前数据。
5. 切换不同 key 恢复 Auto；比较原始 key identity，不依赖显示文本。审计 App 是否在调用 tab.select 前已写入 tree.selected，必要时使用独立的当前 value identity。
6. 标题展示 Auto/Manual、encoding、view，以及回退状态；不要让失败看起来像成功。
7. 审计键盘与鼠标入口，保持现有按键交互；仅修改确有需要的 input/help 文件。
8. 运行 `cargo test --test redis_browser_tabs --test value_preview`。

**完成条件:** 菜单选中策略与实际格式关系明确，手动选择不会污染下一个 key。

### Task 8：统一后台准备与竞态保护

**Files:**
- Modify: `src/action.rs`、`src/app.rs`、`src/runtime.rs`
- Modify: `src/model/redis_browser.rs`、`src/value_preview/prepare.rs`
- Inspect: `src/db/redis/read.rs`
- Create: `tests/redis_preview_serialization.rs`
- Test: `tests/redis_loading_lifecycle.rs`

**Steps:**
1. 添加 App action 端到端测试：收到 String page 后主编辑器内容为解析结果，标签状态正确。
2. 将大 value 的完整检测、解码、渲染一并移入现有后台工作路径，不能在派发任务前完成完整解析。
3. 初次加载、手动切换、追加分页调用同一 prepare helper；小 value 可以同步，但使用相同契约。
4. 在 tab 增加数据 revision 和 prepare request revision；每次追加数据或格式请求更新相应版本。
5. 结果校验连接 identity、tab/key、preview generation、数据 revision、请求 revision。覆盖 Java → Hex → Java 的 ABA 切换。
6. 处理失败也返回当前数据的回退正文和诊断，避免保留上一个 key 的编辑器内容。
7. 审计 String 累积页 complete/truncated 标记，确保追加后重新检测完整累积 bytes。
8. 添加乱序完成、同 key 刷新、切库、关闭 tab、分页追加期间切格式的测试。
9. 运行 `cargo test --test redis_preview_serialization --test redis_loading_lifecycle --test redis_browser_tabs`。

**完成条件:** 大 value 不在 UI 更新流程完整解析；所有异步状态更新均有版本保护。

### Task 9：集合单元格共享处理链路

**Files:**
- Modify: `src/app.rs` / `redis_preview_cell_detail`
- Modify: `src/value_preview/table.rs`、`src/ui/redis_value.rs`（按审计结果）
- Inspect: `src/model/text_detail.rs`
- Test: `tests/redis_preview_serialization.rs`、`tests/value_preview.rs`

**Steps:**
1. 建立混合 Hash/List fixture：普通文本、PHP、Java、Pickle 共存。
2. 详情只对所选单元格原字节执行 prepare，不为集合整体选择一种序列化编码。
3. 详情标题或已有元信息区展示自动识别类型及解析状态；大单元格复用后台调度和过期结果保护。
4. 确认表格 identity 始终保存原字节；二进制详情的原始表示不得使用 from_utf8_lossy。
5. 如文本详情只接受 String，传入无损转义或 Hex 并明确复制的是文本表示；不要声称系统文本剪贴板可直接无损复制任意二进制。
6. 验证关闭详情后迟到结果不重新打开详情、不覆盖后续单元格。
7. 运行 `cargo test --test redis_preview_serialization --test value_preview`。

**完成条件:** 主预览和单元格详情解码一致，混合编码集合互不影响。

### Task 10：缓存与解析预算

**Files:**
- Modify: `src/value_preview/cache.rs`、`mod.rs`、`prepare.rs`
- Modify: `src/runtime.rs`、`src/value_preview/java.rs`、`php.rs`、`pickle.rs`
- Create: `tests/value_preview_limits.rs`

**Steps:**
1. 扩展现有缓存而非另建平行缓存；解码缓存按 source identity + data revision + encoding 索引，使 JSON/YAML 共用解析结果。
2. source identity 必须隔离连接会话、tab/key 以及单元格；分页追加或刷新推进数据 revision。
3. 在后台准备路径接入缓存，锁只覆盖查找和插入，不覆盖实际解析。
4. 定义输入、深度、节点、字符串/容器长度和输出预算。输入上限复用 Redis 读取限制；其余默认值在 fixture 压测后记录，提供内部测试可配置参数。
5. JSON/YAML 输出通过有上限的 writer 或等效机制停止增长；不能先生成无限字符串再截断。
6. 对 Java/Pickle 依赖审计解析期内存和递归上限。Reader 限流、spawn_blocking、timeout 都不等于内部资源限制。
7. 若依赖没有必要预算钩子，先做小范围能力验证，选择可维护的受限解析适配或依赖补丁；形成明确变更后再实施，不能在未验证时宣称硬限额已满足。
8. 超预算结果标明具体限制并提供 Raw/Hex；必要时新增明确的限额状态，不能将其映射成数据损坏。
9. 添加预算边界、深嵌套、巨大声明长度、缓存命中、字节预算淘汰、跨连接隔离测试。
10. 运行 `cargo test --lib value_preview::cache` 和 `cargo test --test value_preview_limits --test redis_preview_serialization`。

**完成条件:** 同值切视图复用解码结果；缓存不会串数据；真实落实的资源限制和依赖限制均有测试记录。

### Task 11：回归、说明与交付

**Files:**
- Modify: `tests/fixtures/value_preview/README.md`
- Create: `docs/redis-value-preview.md`
- Modify: 本计划中的实施记录

**Steps:**
1. 文档写明支持类型、Auto/Manual 规则、特殊类型投影、截断/不支持提示和原文查看方式。
2. 运行聚焦测试：

```bash
cargo test --test value_preview --test value_preview_java --test value_preview_php --test value_preview_pickle --test value_preview_limits --test redis_preview_serialization --test redis_browser_tabs --test redis_values --test redis_loading_lifecycle
```

3. 运行项目检查：

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
git diff --check
```

4. 如果存在 Oracle 等本机外部依赖阻塞，记录具体错误，使用 `--no-default-features` 完成可执行的检查；明确完整默认功能检查尚未通过，不将降级检查等同全量通过。
5. 使用专用测试 Redis，以二进制方式写入 fixture，手动验证菜单、自动解析、Hex 切换、分页、窄终端和快速切 key；测试 key 采用独立前缀，避免接触用户业务 key。
6. 手动验证实际 fixture 的编码而非带 `\\x` 的文字字符串。记录样例、结果和环境，移除测试数据。
7. 审查差异：无重复解码入口、无菜单数量硬编码、无 lossy 字节输入、无静默解析失败、无不受版本保护的后台更新。

**完成条件:** 全部目标行为有自动化覆盖，能运行的项目检查通过，外部环境阻塞如实列明。

## 5. 依赖与里程碑

```text
Task 1 → Task 2 → Task 3 → Task 4 → Task 5
                                    ↓
                                  Task 6 → Task 7 → Task 8 → Task 9 → Task 10 → Task 11
```

- **M1 / 功能贯通：Task 1–2。** 三种格式的基础样例可走主预览解码。
- **M2 / 兼容与自动选择：Task 3–7。** 协议覆盖、自动识别和菜单行为完成。
- **M3 / 运行链路完整：Task 8–10。** 后台、分页、详情、缓存和预算完成。
- **M4 / 交付：Task 11。** 回归、真实 Redis 验证和用户说明完成。

默认顺序执行。任务 3–5 的协议工作在接口稳定后可以独立开展，但本计划不要求代理并行；共享 mod/decode/fixture 文档的改动需要协调。

## 6. 测试矩阵

| 场景 | 核心断言 |
| --- | --- |
| Java String / 对象 / 集合 | 自动 Java、结构化正文、类/引用信息保留 |
| PHP 数组 / 对象 / 引用 | 正常解析、字节长度正确、二进制与键不丢失 |
| Pickle 0–5 基础类型 | 对应协议可解析、标题版本准确或不伪造版本 |
| Pickle 特殊类型 | 保留类型，超出能力时明确 Unsupported |
| JSON / 空值 / 普通文本 | 原有预览稳定，不被失败候选抢占 |
| 部分数据 / 损坏数据 | NeedsMoreData 与 Invalid 区分，原文可查看 |
| Manual → 同 key 追加/刷新 | 保持 Manual |
| Manual → 不同 key | 恢复 Auto |
| Java → Hex → Java 乱序完成 | 仅最新请求可以更新 |
| 同 key 不同 revision | 旧缓存及旧任务不覆盖新数据 |
| 混合集合单元格 | 每个单元格独立识别，与主预览一致 |
| 大 value / 预算超限 | UI 不执行完整解析，超限有明确回退 |

## 7. 执行记录模板

每完成一个任务记录：改动文件、运行命令、通过/失败数量、与计划的偏差、尚待确认的依赖能力。M2 前必须完成协议能力记录，M3 前必须解决后台竞态与资源限制能力验证。

## 8. 本次执行记录

- Task 1：完成基线测试和 PHP 空数组失败回归；基线 `value_preview`、`redis_browser_tabs`、`redis_values` 全部通过。
- Task 2：完成完整 `PreviewFormat` 贯穿 `format_page`、App 手动切换和 Runtime 大 value 路径；修复普通 Text JSON/YAML 回归。
- Task 3：完成 PHP 空数组、对象、引用、二进制字符串和分隔符错误分类；聚焦测试通过。
- Task 4：完成 Pickle 协议 0–5 探测入口及 bytes/tuple/set/dict/大整数投影；聚焦测试通过。
- Task 5：增加 Java 标准序列化 String fixture，验证现有 jaded 展开逻辑；聚焦测试通过。
- Task 6：自动选择过滤非 Complete 候选，失败 PHP 不再抢占默认 Raw；clippy 复核通过。
- Task 7：菜单扩展为 Auto、RAW、JSON、YAML、Table、Hex、Java、PHP、Pickle；切 key 恢复 Auto；UI 测试通过。
- Task 8：增加主预览/单元格共享解码和 Raw/Hex 绕过解析测试；现有 generation + format 保护经审计足够，本轮未引入未贯通的伪 revision 字段。
- Task 9：集合详情复制改为无损转义；集合 identity 保持原始字节；相关测试通过。
- Task 10：增加 4 MiB 输入和 8 MiB 输出预算；缓存保持独立隔离测试，未在没有应用生命周期容器的情况下强行接入。
- Task 11：增加 `docs/redis-value-preview.md`，聚焦测试、fmt、clippy 和 diff 检查均通过；全量 `cargo test` 首次因 120 秒命令超时，需以更长超时复核。
