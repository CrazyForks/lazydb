# Redis Preview Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> 执行环境没有该技能时，按以下任务逐项实施、验证和记录，不假定技能已安装。

**Goal:** 为 Redis Preview 提供完整 Key 信息头部、准确的大小与 TTL、与 SQL Editor 一致的 Vim 文本交互、自动格式识别、多种序列化解码以及数据库风格 Table 视图。

**Architecture:** 保留 Action → App reducer → Command → Runtime → RedisAdapter 数据流，原始字节与派生展示分离。抽取现有编辑器的语言无关视口，复用 EditorWorkspace 与 DataGrid；格式识别、解码、格式化在有预算的后台流水线中完成，以连接身份和请求代次隔离结果。

**Tech Stack:** Rust 1.94 / Edition 2024、Tokio、redis-rs 1.5.0、Ratatui 0.30.2、Crossterm 0.29、modalkit 0.0.25、serde_json；新增 YAML 解析/高亮依赖及 jaded、serde-pickle 的具体版本在依赖验证任务中确定。

---

## 0. 执行依据

- 当前项目：`/Users/yelog/workspace/tui/lazydb`。
- 参考项目：`/Users/yelog/workspace/rust/rust-redis-desktop`，仅作为解析逻辑与测试样本的参考。
- 参考文件：`src/serialization/mod.rs`、`php.rs`、`pickle.rs`、`protobuf.rs`、`java_converters.rs`、`src/formatter/preset.rs`。
- 既有相关计划：`docs/plans/2026-09-14-redis-browser-optimization-implementation.md` 与 `docs/plans/2026-09-14-redis-browser-ui-consistency-implementation.md`。
- 本计划接续上述工作，以实施时的当前代码为准；尤其 Preview 的 Vim 按键契约取代旧计划中 Preview 裸 `h/l` 切 pane 的契约。
- 创建日期：2026-09-14。本文为待执行计划，测试命令和性能目标不代表已验证结果。

### 执行规则

1. 实施前检查当前工作区状态和相关 AGENTS.md；已有未提交文件保持原样。
2. 每个任务先重新定位列出的符号及调用方，再按编号执行；大步骤按函数或行为切成小改动。
3. 为解析正确性、异步隔离、输入路由和资源边界补行为测试；纯颜色、文字和布局微调使用现有渲染测试验证，避免实现镜像测试。
4. 每个任务完成运行对应定向检查；最终只进行一轮完整 CI 等价检查，出现失败或新增修改再重跑相关部分。
5. 下方提交信息表示建议的逻辑提交边界；执行提交时只暂存该任务文件。创建计划本身不提交业务代码。
6. 新模块通过现有 `src/lib.rs`、`src/model/mod.rs`、`src/ui/mod.rs` 等实际模块入口注册，不创建重复根模块。
7. 初始预算是可调设计值，优先接入现有 Redis limits 配置；实施前定位已有配置结构，不另建平行配置体系。

## 1. 产品契约

### 1.1 布局

```text
┌ Preview ──────────────────────────────────────────────────┐
│ app:production:user:10086:profile                          │
│ [ String ]  [ Size 12.4 KB ]  [ TTL 55m30s ]                │
│─────────────────────────────────────── [ AUTO · JSON ▾ ] │
│  1  {                                                 ▲ │
│  2    "id": 10086,                                     █ │
│  3    "name": "张三"                                  ░ │
│  4  }                                                 ▼ │
│ NORMAL · Ln 3, Col 12 · Complete                          │
└──────────────────────────────────────────────────────────┘
```

- Key 使用完整原始 Key 的显示投影，强调色 + 加粗；普通长 Key 换行，超长 Key 限高并提供独立浏览和完整复制。
- 头部、格式工具栏、状态栏不随 value 滚动；窄窗口优先保留 value 空间，标签可压缩，完整信息可进入详情查看。
- Key、value、错误消息的终端控制字符统一通过现有安全投影处理；中文、emoji、Tab 使用显示 cell 坐标。
- Type 颜色：String 绿、Hash 青、List 蓝、Set 紫、ZSet 橙、Stream 品红、未知灰；通过 Theme 语义映射，不硬编码散落 RGB。

### 1.2 Size 与 TTL

- Size 统一采用 `MEMORY USAGE key` 的 Redis 内存占用；详情注明该口径及采样估算属性。
- `MEMORY USAGE` 不可用时显示 `Size —`，不影响 value；禁止用当前页 `raw_bytes` 冒充整个 Key 的大小。
- 额外获取 String 的 `STRLEN`、集合 cardinality，用于内容分页与进度，不混入 Size。
- 单位按 1024 换算，显示 B/KB/MB/GB/TB；详情明确约定。小数去掉多余尾零。
- 大小初始配色阈值：小于 1 MB 绿色，1–10 MB 黄色，大于等于 10 MB 红色，未知灰色。
- TTL：`∞`、`55m30s`、`2h15m`、`3d4h`、`850ms`、`Missing`、`—`；超过分钟最多显示两个有效单位，详情保留精确剩余毫秒。
- 使用单调时钟计算剩余时间；归零后只触发一次重查，确认前不能把本地计时归零等同于 Key 已不存在。

### 1.3 输入与编辑语义

- 默认 Preview 是 ReadOnly Vim 会话，进入 Normal 模式；复用现有移动、计数、搜索、Visual、复制、跳转和滚动行为。
- Preview 内 `h/j/k/l` 归编辑器所有；搜索 prompt 优先接收文本。
- pane 切换通过现有公共焦点动作提供，验证 `Ctrl-w h/l` 与 Tab 的上下文可用性；不占用 Vim 原有 `f`、`v`、`g` 等单键。
- 格式切换通过右上角点击和公共命令入口提供；建议默认 `Alt-f`，先检查现有绑定冲突，存在冲突则仅注册命令并复用项目配置规则。
- 菜单：方向键选择、Enter 确认、Esc 关闭并恢复原焦点。Esc 在搜索/Visual 中先按 Vim 语义退出该模式。
- 本轮按“预览”实现，修改类 Vim 操作受现有 ReadOnly capability 控制。需要写入 Redis 时另立 Edit Draft/编码回写方案，不把派生 JSON 当原始值自动提交。
- 滚动和切换已缓存格式不请求 Redis；加载下一页/完整内容由显式动作触发。

### 1.4 格式语义

- 菜单包含 Auto、RAW、JSON、YAML、Table、Hex、Java、PHP、Pickle、Protobuf。
- 内部编码与展示分离；Java/PHP/Pickle/Protobuf 菜单项是“指定解码器 + JSON 展示”的快捷预设。
- 从 Java 切到 YAML/Table 使用同一份已解码结构；RAW/Hex 始终基于原始字节。选择器显示 `JAVA → YAML` 等来源，避免误解。
- 自动默认：原生集合 → Table；String 中严格 JSON → JSON；明确结构化 YAML → YAML；成功解码的强特征编码 → 对应格式的 JSON；普通文本 → RAW；未知二进制 → Hex。
- 低置信度 Protobuf 作为候选，不自动抢占 RAW/Hex；有明确 schema 绑定时可选中 Protobuf。
- 手动选择对同一 Key 的刷新保持；更换 Key 重新 Auto；同一内容版本的不同视图保留独立光标和滚动位置。
- 截断值产生 `NeedsMoreData`，与完整输入的 `Invalid` 区分；不能把 JSON 前缀判为非法 JSON。
- 原生集合没有单一连续 value byte stream。集合 RAW 使用逐项转义投影，集合 Hex 显示带项目身份的分段字节；序列化解码作用于单元格原始 bytes，不对拼接后的集合文本解码。

## 2. 模型与模块边界

建议新增：

```text
src/value_preview/
  mod.rs              公共格式、解码结果、完整性和错误契约
  detect.rs           候选识别、置信度、默认视图选择
  format.rs           RAW、Hex、JSON、YAML 投影
  decode.rs           解码器分发
  java.rs             Java 解析及结构化归一化
  php.rs              按字节游标的 PHP 解析
  pickle.rs           Pickle 类型转换
  protobuf.rs         schema-less wire 结构解析
  table.rs            原生集合与结构化数据的表格投影
  cache.rs            带字节预算的派生结果缓存
src/model/redis_preview.rs
src/ui/redis_preview.rs
src/ui/editor_view.rs
src/editor/highlight.rs
tests/value_preview.rs
tests/redis_preview.rs
tests/redis_preview_runtime.rs
tests/fixtures/value_preview/README.md
```

接口草案（可按现有 derives 调整，不在 UI state 中引入不可比较的运行时任务句柄）：

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ValueEncoding { Text, Java, Php, Pickle, Protobuf, Unknown }

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ValueView { Raw, Json, Yaml, Table, Hex }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeStatus { Complete, NeedsMoreData, Unsupported, Invalid }

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct PreviewFormat {
    pub encoding: ValueEncoding,
    pub view: ValueView,
}
```

另建有数据载荷的结果类型，包含：解析消费字节数、问题偏移、原始字节引用、结构化结果、候选置信度、展示语言及预算消耗。上述状态枚举不能代替完整错误信息。

结构化结果应保留 bytes、非字符串 map key、类型标记和引用信息；JSON 只是展示投影。普通 JSON 可直接持有 serde_json::Value；YAML 原生节点保留 tag/非字符串 key 等必要信息；Java/PHP/Pickle 特殊值用显式标记结构映射，不能静默变成 null 或丢项。

请求身份包含：ConnectionIdentity + RedisTarget + tab_id + raw key + preview_generation + source_revision；单元格预览再加稳定的 entry identity；解码结果再加 format_revision。

## 3. 里程碑与依赖

| 里程碑 | 任务 | 交付物 |
|---|---|---|
| M0 契约 | T01–T02 | 基线、格式模型、依赖验证 |
| M1 正确读取 | T03–T05 | 元数据、分页完整性、统一生命周期 |
| M2 Vim 预览 | T06–T09 | 通用编辑器、信息头部、RAW/Hex、菜单 |
| M3 结构化显示 | T10–T12 | JSON/YAML、Table、单元格预览 |
| M4 序列化 | T13–T15 | Java/PHP/Pickle/Protobuf |
| M5 完整交付 | T16–T18 | Auto、后台预算、性能及回归文档 |

顺序：T01 → T02 → T03 → T04 → T05 → T06 → T07 → T08 → T09 → T10 → T11 → T12 → T13 → T14 → T15 → T16 → T17 → T18。

T13–T15 可按模块独立实现，但共同修改 decode 分发时应顺序合并。本文不要求自动启动多个 agent。

## 4. 详细任务

### T01：建立行为基线和真实样本

**文件**
- 阅读：`src/db/redis/read.rs`、`src/runtime.rs`、`src/model/redis_browser.rs`、`src/editor/mod.rs`、`src/ui/data_grid.rs`、`src/input/keymap.rs`。
- 新增：`tests/fixtures/value_preview/README.md`。
- 扩展现有测试：`tests/redis_values.rs`、`tests/redis_browser_tabs.rs`、`tests/mouse.rs`。

**步骤**
1. 记录 TYPE/PTTL 重复查询、64 KB String 默认页、200 项集合提示、Stream 缺失分支及旧文本预览调用者。
2. 运行 `cargo test --test redis_values --test redis_browser_tabs --test redis_loading_lifecycle`，记录基线和既有失败。
3. 定位 `EditorSessionCapability`、`active_read_only_session_id`、`mouse_session_focus`、`render_editor_scrollbars` 的复用边界。
4. 准备 JSON/YAML/Java/PHP/Pickle/Protobuf 的小型固定样本；夹具文档记录来源、编码参数、预期结构与不支持情况。
5. 样本覆盖中文、空值、无效 UTF-8、控制字符、截断、非字符串键、循环引用、Protobuf 重复字段；二进制夹具不依赖运行测试时安装 Java/Python。

**验收**：有可复现基线；样本能够用于独立解码测试。建议提交：`test(redis): establish preview fixtures and baseline`。

### T02：定义格式契约和确认依赖

**文件**
- 新增：`src/value_preview/mod.rs`、`src/value_preview/decode.rs`、`src/model/redis_preview.rs`。
- 修改：`src/lib.rs`、`src/model/mod.rs`、`Cargo.toml`、`Cargo.lock`。
- 新增：`tests/value_preview.rs`。

**步骤**
1. 定义 encoding/view/menu preset、完整性、检测候选、解码错误与源字节身份。
2. 定义 text/native collection 两种源，避免为 Hash/List 构造虚假的单一原始 byte buffer。
3. 明确 strict UTF-8 失败与二进制回退、尾随数据与完整消费规则；补契约测试。
4. 查询新增依赖的当前文档，确认 Rust 1.94、许可证、serde 支持和格式覆盖。YAML 解析/序列化及语法高亮分开评估；不直接照搬参考项目的 serde_yaml→JSON 路径。
5. 核实 jaded/serde-pickle 能否暴露引用、bytes、非字符串 key 和消费位置；记录不能保证的边界，纳入具体 decoder 适配任务。
6. 添加所需最小依赖和 feature，不引入桌面 UI 或参考项目整体依赖。
7. 运行 `cargo check --all-targets` 和 `cargo test --test value_preview`。

**验收**：类型可表达完整/不完整/不支持/非法，依赖能在项目工具链编译。建议提交：`feat(preview): define value decoding contracts`。

### T03：补齐 Key 元数据与 Size 口径

**文件**
- 修改：`src/db/redis/read.rs`、`src/db/redis/metadata_cache.rs`、`src/runtime.rs`。
- 新增/扩展测试：`tests/redis_preview_runtime.rs`、`tests/redis_values.rs`。

**步骤**
1. 扩展 RedisKeyMetadata：memory usage、String 总字节数、集合元素数、元数据采样时刻/年龄信息；未知与零必须区分。
2. 添加本地模拟 RESP server 用例，参考 `tests/redis_protocol_limits.rs`，验证 TYPE/PTTL、MEMORY USAGE 和相应长度命令。
3. 将初次预览读取收敛为 Adapter 的组合入口；类型确定后读取内容和该类型长度，不再次无条件 TYPE/PTTL。
4. 将 MEMORY USAGE 权限/不支持错误降级为 Size 未知；连接中断保持真实失败，不吞掉所有错误。
5. 独立后续页请求仍有适当类型变化检测；组合请求不承诺 Redis 快照，WRONGTYPE/消失转成明确状态。
6. 更新 metadata cache 的字段和失效行为，TTL 不直接复用缓存中的原始剩余值而忽略年龄。
7. 运行 `cargo test --test redis_preview_runtime --test redis_values`。

**验收**：整个 Key 的 Size 不受预览页大小影响；元数据缺少内存统计仍可预览；初次读取不重复 TYPE/PTTL。建议提交：`fix(redis): report key metadata independently of preview pages`。

### T04：修正分页完整性并补 Stream

**文件**
- 修改：`src/db/redis/read.rs`、`src/runtime.rs`、`src/model/redis_preview.rs`。
- 测试：`tests/redis_values.rs`、`tests/redis_preview_runtime.rs`。

**步骤**
1. 分离“服务端还有数据”“客户端保留 pending”“数据因预算被截断”；不可只用一个 truncated bool 表达全部状态。
2. String 用已读区间与总长度判断完成；实际短读/Key 变化更新状态，不机械使用请求 end + 1。
3. List/ZSet 用实际条目和已知 cardinality 判断，覆盖空值和恰好一页。
4. Hash/Set 覆盖空批次非零 cursor、COUNT 超额、cursor=0 但 pending 未消费完；重复条目按原始 field/member 身份处理。
5. pending 计入字节预算，无法保留时明确标记中断，不静默丢项后宣称完整。
6. 增加 StreamRead 请求及按 ID 的 XRANGE 分页，下一页排除上次末 ID；Missing/Module/Unknown 返回有终态的结果。
7. 为多页 String 在 UTF-8 字符中间分界添加用例：原始 bytes 原样拼接，完整后再做结构化解析。
8. 运行 `cargo test --test redis_values --test redis_preview_runtime --test redis_loading_lifecycle`。

**验收**：完整性可被 decoder 正确消费；Stream 不停在 Loading；预算限制不被描述成网络接收前的硬内存限制。建议提交：`fix(redis): track preview completeness and stream pages`。

### T05：统一 Preview 生命周期与异步身份

**文件**
- 修改：`src/model/redis_browser.rs`、`src/model/redis_preview.rs`、`src/action.rs`、`src/app.rs`、`src/runtime.rs`、`src/db/redis/preview_scheduler.rs`。
- 测试：`tests/redis_browser_tabs.rs`、`tests/redis_loading_lifecycle.rs`、`tests/redis_preview_runtime.rs`。

**步骤**
1. 将旧 `RedisPreviewState::Ready { content: String }` 和类型化 value_page 路径收敛为统一状态；先追踪所有调用者再移除旧路径。
2. 新状态持有 metadata、source revision、completeness、format selection、editor session id、table state；任务句柄留在 Runtime。
3. 元数据与 body 可分步到达，头部不能把新 Key 配上旧 value 而不标记状态。
4. 为加载、失败、取消、Key 切换、DB 切换、重连、关闭 Tab 定义明确迁移。
5. 扩展 generation 验证到数据与解码结果；取消负责节省工作，identity 校验负责拒绝迟到结果。
6. 实现显式 Load next / Load complete 动作；普通滚动不触发加载。
7. 补乱序事件用例，运行 `cargo test --test redis_browser_tabs --test redis_loading_lifecycle --test redis_preview_runtime`。

**验收**：A Key 慢响应不能覆盖 B；手动格式对同 Key 刷新保持；关闭 Tab 释放关联会话/缓存/任务。建议提交：`refactor(redis): unify preview state and request identity`。

### T06：抽取通用编辑器视口和语言入口

**文件**
- 新增：`src/ui/editor_view.rs`、`src/editor/highlight.rs`。
- 修改：`src/editor/mod.rs`、`src/model/editor.rs`、`src/ui/mod.rs`。
- 测试：`src/editor/tests.rs`、`tests/ui_render.rs`、`tests/mouse.rs`。

**步骤**
1. 增加 EditorLanguage：Plain、Sql(dialect)、Json、Yaml；既有 SQL API 作为兼容包装保留。
2. 将 SQL 高亮计算与通用快照组装拆开，cache key 包含 session、revision、language 和 SQL 特有 ranges。
3. 提取接受 session id、snapshot、area、focus、theme 的 EditorView；SQL completion、statement 标记仍由 SQL 入口提供。
4. 统一 gutter、prompt、光标、选区、横纵滚动条布局及 hit region 注册。
5. 将滚动条轨道明确传入，避免沿用整个外框高度而覆盖 Preview 头部。
6. 先让现有 SQL Editor 使用抽取后的公共视口，验证视觉和鼠标行为不回退。
7. 运行 `cargo test --lib editor::tests` 和 `cargo test --test ui_render --test mouse`。

**验收**：Plain 快照不走 SQL 解析；SQL 当前语句、补全、Unicode 定位和双向滚动仍正确。建议提交：`refactor(editor): extract language-aware reusable editor view`。

### T07：将 Preview 接入 Vim 会话与输入路由

**文件**
- 新增：`src/ui/redis_preview.rs`、`tests/redis_preview.rs`。
- 修改：`src/ui/redis_browser.rs`、`src/ui/mod.rs`、`src/app.rs`、`src/input/keymap.rs`、`src/model/redis_preview.rs`。

**步骤**
1. 在预览文本准备好后创建/更新 ReadOnly session；不得每帧 open_read_only 重置光标。
2. 扩展 active_read_only_session_id、mouse_session_focus、ensure_read_only_session、copy selection 和关闭会话路径。
3. Preview 焦点的 Vim 输入先于 Redis 裸 h/l pane 处理；Keys 搜索框输入优先级保持。
4. 接入公共焦点切换动作、点击 value 获取焦点、鼠标定位和拖动复制。
5. 用长文本覆盖 h/l、计数移动、gg/G、/ 与 n/N、Visual、半页/整页、横滚和滑块拖动。
6. 验证失焦 Preview 不绘制活跃光标；滚动不改变 Key、不发网络命令；只读修改动作不改 source revision。
7. 运行 `cargo test --test redis_preview --test keymap --test mouse --test redis_browser_tabs`。

**验收**：Preview 操作手感与现有只读编辑器一致，h/l 不再劫持光标。建议提交：`feat(redis): reuse vim editor interactions in preview`。

### T08：实现 Key 信息头部与 TTL 展示

**文件**
- 修改：`src/ui/redis_preview.rs`、`src/ui/theme.rs`、`src/model/redis_preview.rs`、`src/action.rs`、`src/app.rs`、`src/runtime.rs`。
- 测试：`tests/redis_preview.rs`、`tests/redis_preview_runtime.rs`。

**步骤**
1. 提取 size/TTL 显示函数，覆盖零、单位边界、未知值及 Persistent/Missing。
2. 布局拆分 Key、标签、格式栏、value、状态栏；Key 使用完整 bytes 的安全投影。
3. 增加长 Key 浏览/复制命中区域，确保超长 Key 不挤掉整个 value 区域。
4. 通过 Theme 映射 Type/size 样式，测试窄窗口、ASCII 模式和 CJK 标签布局。
5. TTL 计时状态使用单调时钟；利用现有 tick/重绘调度更新，不为每个 Key 启动常驻轮询任务。
6. TTL 归零只安排一次重查，连接断开时明确显示过期的采样状态。
7. 运行 `cargo test --test redis_preview --test redis_preview_runtime --test ui_render`。

**验收**：value 滚动不移动头部；55m30s 格式正确；Size 来源可解释；小窗口不越界。建议提交：`feat(redis): add key metadata header and ttl badges`。

### T09：RAW、Hex 与格式选择菜单

**文件**
- 新增：`src/value_preview/format.rs`。
- 修改：`src/ui/redis_preview.rs`、`src/ui/mod.rs`、`src/value_preview/mod.rs`、`src/model/redis_preview.rs`、`src/action.rs`、`src/commands.rs`、`src/input/keymap.rs`、`src/app.rs`。
- 测试：`tests/value_preview.rs`、`tests/redis_preview.rs`。

**步骤**
1. 实现 RAW：保留有效 UTF-8 和换行，对二进制及控制字节转义；区分原始字面反斜杠与显示转义。
2. 实现 Hex：offset + 16 bytes + ASCII 侧栏，末行补齐；集合使用分段身份，不混淆来源。
3. 实现格式菜单与 keyboard command，菜单项绑定 encoding/view preset，不使用字符串分支判断。
4. 每个 format 保存 viewport bookmark；切换不重新向 Redis 请求，同版本恢复光标，版本变化后钳制位置。
5. 菜单未实现格式明确标记不可用，后续任务逐项启用；最终验收所有本轮格式均可用。
6. 运行 `cargo test --test value_preview --test redis_preview --test keymap`。

**验收**：Hex 能定位准确原始字节；RAW 不损坏中文；菜单 Esc 正确恢复焦点。建议提交：`feat(preview): add raw hex views and format selector`。

### T10：JSON/YAML 格式化与高亮

**文件**
- 修改：`src/value_preview/format.rs`、`src/value_preview/decode.rs`、`src/editor/highlight.rs`、`src/model/editor.rs`、`src/ui/mod.rs`。
- 测试：`tests/value_preview.rs`、`tests/redis_preview.rs`、`src/editor/tests.rs`。

**步骤**
1. JSON 用 strict UTF-8/严格解析并输出统一缩进；区分 scalar JSON 与自动识别的对象/数组优先策略。
2. YAML 使用原生 YAML 节点格式化为 YAML；明确格式化会规范化布局，不能宣称保留注释原貌。
3. YAML 多文档支持整体显示；别名展开和深度/节点数有预算，特殊 tag 与非字符串 key 保留或明确报不支持。
4. JSON token 输出 key/string/number/bool/null/punctuation 的 UTF-8 ranges；YAML 使用符合语法的高亮能力，避免仅靠逐行正则。
5. 高亮范围覆盖转义引号、Unicode、跨行字符串、anchors、aliases、comments；投影继续使用现有安全映射。
6. 对不完整内容显示 NeedsMoreData 与加载入口；完整非法输入显示偏移/原因和 RAW/Hex 回退入口。
7. 运行 `cargo test --test value_preview --test redis_preview` 和 `cargo test --lib editor::tests`。

**验收**：YAML 视图不是 JSON；编辑器换格式不发生光标字节/cell 混用；控制字符保持惰性显示。建议提交：`feat(preview): format and highlight json and yaml`。

### T11：DataGrid 接入 Redis Table

**文件**
- 新增：`src/value_preview/table.rs`。
- 修改：`src/ui/data_grid.rs`、`src/ui/mod.rs`、`src/model/tab.rs`、`src/model/redis_preview.rs`、`src/ui/redis_preview.rs`、`src/app.rs`、`src/action.rs`。
- 测试：`tests/value_preview.rs`、`tests/redis_preview.rs`、`tests/mouse.rs`。

**步骤**
1. 定义原生集合投影：Hash Field/Value、List Index/Value、Set Member、ZSet Member/Score、Stream ID/Fields。
2. 定义 JSON object → Key/Value、object array → 字段并集、scalar array → Index/Value；嵌套值用摘要，不无限展开列。
3. 复用 ResultSet/CellValue 表示可显示行，同时单独保存 raw source entry identity；缺失字段与显式 null 在显示/详情中区分。
4. 为 DataGrid 交互增加明确 owner（SQL result / Relation / Redis preview）或等价上下文参数。当前 ResultCell/RelationColumnResize 等命中目标不能不加区分直接复用。
5. 将单元格选中、列宽、行列滚动、复制等动作路由到 owner；Redis Table 不进入关系表编辑/排序 SQL 路径。
6. 保留 Table 与文本视图各自状态，宽表与窄窗口显示一致的滚动条。
7. 运行 `cargo test --test value_preview --test redis_preview --test mouse --test ui_render`。

**验收**：Redis 表格点击不修改 SQL 结果状态；字段/成员原始 bytes 可追溯；SQL/Relation 表格功能正常。建议提交：`feat(redis): render collection previews through shared data grid`。

### T12：集合单元格预览与显式完整加载

**文件**
- 修改：`src/model/redis_preview.rs`、`src/ui/redis_preview.rs`、`src/app.rs`、`src/action.rs`、`src/runtime.rs`、`src/value_preview/table.rs`。
- 测试：`tests/redis_preview.rs`、`tests/redis_preview_runtime.rs`。

**步骤**
1. 添加单元格展开动作，创建带 parent key + entry identity 的子预览；返回 Table 恢复原选择和滚动。
2. Hash field/Set member 使用原始 bytes 作为身份，List 用索引并记录非快照语义，Stream 用 entry ID 与字段位置。
3. 子预览使用同一格式选择、编辑器与后续 decoder，不复制一套解析逻辑。
4. Load complete 对 String 顺序获取缺页，对集合区分“加载更多行”和“查看完整单元格”；遵守 T04 预算/pending 语义。
5. 顶层 Key 更换、source revision 更新时取消/失效旧子预览结果。
6. 运行 `cargo test --test redis_preview --test redis_preview_runtime --test redis_browser_tabs`。

**验收**：同一 Hash 中 JSON/普通文本/Java value 可分别解码，不对整个 Hash 文本误判。建议提交：`feat(redis): add value cell drilldown and explicit loading`。

### T13：Java 解码与 JSON 结构投影

**文件**
- 新增：`src/value_preview/java.rs`。
- 修改：`src/value_preview/decode.rs`、`src/value_preview/mod.rs`。
- 测试：`tests/value_preview.rs`、`tests/fixtures/value_preview/README.md` 及 Java 二进制夹具。

**步骤**
1. 迁移参考项目 jaded Parser → content → structured value 的链路，检测完整 `AC ED 00 05` 流头。
2. 普通对象保留完整 class 和 fields；string/primitive/array/enum 转为明确结构。
3. 对常见 Map/List/Set 做有依据的适配；仅凭类名不足以读取自定义 annotations，不能把 java_converters 的类型列表当完整转换器。
4. 保留 `$id/$ref` 或等价引用信息；若依赖输出只剩 Loop 标记而无法恢复 handle，显示明确未解析引用标记并保留原始 bytes，不伪造引用关系。
5. Block 数据输出合法结构化 JSON（类型、长度、bytes 编码），而非当前参考项目的自由文本。
6. 确定流中多个顶层 content 与尾随数据的行为：可支持多项结构，否则报告未消费数据，不把读到首对象视为完整成功。
7. 运行 `cargo test --test value_preview java`。

**验收**：对象、集合、Unicode、引用和 Block 均有可解释输出；截断与未知自定义序列化不静默丢失。建议提交：`feat(preview): decode java serialized values`。

### T14：PHP 和 Pickle 解码

**文件**
- 新增：`src/value_preview/php.rs`、`src/value_preview/pickle.rs`。
- 修改：`src/value_preview/decode.rs`。
- 测试：`tests/value_preview.rs` 及 PHP/Pickle 夹具。

**步骤**
1. PHP 使用 `&[u8]` 游标，所有声明长度按字节处理；替代参考项目先要求全量 UTF-8 的实现。
2. 支持 N/b/i/d/s/a/O/r/R/C 的可解释结构，保留对象类名、引用、自定义 payload 和属性原始身份。
3. 对不可表示浮点、非字符串 map key、重复 key 做显式结构投影，不过滤、不覆盖、不转 null。
4. PHP 边界采用 checked arithmetic，限制声明数量和递归深度，验证完整输入消费。
5. Pickle 先解析为能保留 bytes/tuple/set/dict key 的中间类型，再转换展示结构；不直接 deserialize 为 serde_json::Value 丢失类型表达能力。
6. 声明支持的协议/操作集合；对不支持的对象构造或 protocol 5 外部 buffer 返回明确错误，解析仅在 Rust 数据解析器内完成。
7. 运行 `cargo test --test value_preview php` 和 `cargo test --test value_preview pickle`。

**验收**：多字节中文长度、二进制字符串、非字符串键有正确结果；非法长度/截断不能 panic。建议提交：`feat(preview): decode php and pickle values`。

### T15：Protobuf schema-less 解析

**文件**
- 新增：`src/value_preview/protobuf.rs`。
- 修改：`src/value_preview/decode.rs`、`src/value_preview/mod.rs`。
- 测试：`tests/value_preview.rs` 及 Protobuf 夹具。

**步骤**
1. 用完整 varint 解 tag，验证字段号、wire type、长度、u64 overflow 和所有切片边界。
2. 实现 varint/fixed64/length-delimited/fixed32，groups 若本轮不支持则显式 Unsupported，不返回部分成功。
3. 结果保留字段出现顺序和重复项，以字段记录数组作为无损结构；友好 JSON 可聚合同字段为数组。
4. length-delimited 保留原始 bytes；UTF-8/嵌套消息仅作为可选解释，空字段合法，嵌套推断受深度预算约束。
5. 完整消费全部输入才算 Complete；删除参考项目的字段号 <=100、解析约 20 个字段就停止等限制。
6. UI 标记 Schema-less，不推断真实字段名、signed/enum/float 或 packed repeated 语义。
7. 为未来 descriptor schema 解码留接口；schema 管理/导入 UI 不作为本轮必需项，有 schema 的完整业务语义解码另行实施。
8. 运行 `cargo test --test value_preview protobuf`。

**验收**：重复字段、多字节 tag、零长度、畸形 varint、二进制嵌套都有明确结果。建议提交：`feat(preview): add strict protobuf wire decoding`。

### T16：自动检测与格式状态协调

**文件**
- 新增：`src/value_preview/detect.rs`。
- 修改：`src/value_preview/decode.rs`、`src/model/redis_preview.rs`、`src/ui/redis_preview.rs`、`src/app.rs`。
- 测试：`tests/value_preview.rs`、`tests/redis_preview.rs`。

**步骤**
1. detector 返回候选列表、置信度、原因、是否需要更多数据，而非遇到首个前缀即返回。
2. 有界强特征探测 → 严格解析验证 → JSON → 明确结构 YAML → 弱二进制候选 → RAW/Hex。
3. 普通标量 `hello`、`123`、空白文本不因 YAML 解析成功被劫持；JSON scalar 是否自动显示按产品规则固定并测试。
4. 原生集合默认 Table，内部 cell 独立检测；String 的 object array 默认 JSON，Table 为可切换视图。
5. 不完整数据保留候选，但不自动无限加载；显示 NeedsMoreData 和显式加载动作。
6. Auto 更新只在有效 source revision 变化时进行；手动 format selection 保持；强特征格式损坏时显示损坏候选和 Hex 回退，不错选低置信度格式。
7. 运行 `cargo test --test value_preview --test redis_preview`。

**验收**：普通文本、YAML、JSON、各二进制格式的默认选择稳定；用户选择不会被刷新抢走。建议提交：`feat(preview): select formats with validated auto detection`。

### T17：后台解码、缓存及大值性能

**文件**
- 新增：`src/value_preview/cache.rs`。
- 修改：`src/runtime.rs`、`src/action.rs`、`src/app.rs`、`src/editor/mod.rs`、`src/editor/highlight.rs`、`src/model/redis_preview.rs`；以及实施时定位到的既有 Redis limits 配置文件。
- 测试：`tests/redis_preview_runtime.rs`、`tests/redis_scale.rs`、`tests/value_preview.rs`、`src/editor/tests.rs`。

**步骤**
1. 将 decode/format 移到有并发限制的后台任务；render/reducer 只装配状态，不解析整份文档。
2. 默认最多一个活跃解析和一个可替换待处理项；任务身份包含 source/format revision，防止切格式后的旧结果回写。
3. 各自有界：原始页、拼接完整输入、解析节点、深度、格式化输出、缓存和编辑器文档。
4. 初始建议：自动解析完整输入最多 1 MiB，显式完整解析最多 8 MiB，派生文本最多 16 MiB，缓存总预算 32 MiB，深度 64、节点 100000。测量后调整，并与已有 limits 统一。
5. 缓存键含内容版本、格式和格式化配置；原始 bytes 尽量共享，特殊格式只按需生成。会话及其内部 buffer 复制也计入保留内存，不只计算 cache map 大小。
6. 每个 Tab 使用有限数量的编辑器文档和轻量 viewport bookmarks，避免每种格式永久保留一整份 modalkit buffer。
7. 对自写解析器增加协作取消/预算检查。`spawn_blocking` 已开始的工作无法仅靠 abort 强停；第三方解析器依靠输入/输出边界和受控队列限制，不能将超时宣称为已终止 CPU 工作。
8. 编辑器按 revision 缓存行起始 offsets、最大行宽、highlight tokens；可见行直接索引，消除每行从全文开头累计 offsets。
9. 用 64 KiB、1 MiB、8 MiB 的多行与单长行样本记录首次显示、格式切换、滚动延迟、峰值保留内存。热路径不重复全量格式化/高亮。
10. 运行 `cargo test --test redis_preview_runtime --test redis_scale --test value_preview` 和 `cargo test --lib editor::tests`。

**验收**：预算可执行且统计完整；快速切换不卡在无界任务队列；滚动没有随文档全文长度增长的重复解析。参考机热滚动 p95 小于 16 ms 为测量目标，不写成跨机器单元测试硬断言。建议提交：`perf(preview): bound decoding cache and editor projections`。

### T18：端到端验收与文档

**文件**
- 修改：`docs/redis-browser.md`、`docs/testing/redis-browser-performance.md`、`src/help.rs`。
- 测试：`tests/redis_preview.rs`、`tests/redis_preview_runtime.rs`、`tests/redis_browser_tabs.rs`、`tests/mouse.rs`、`tests/ui_render.rs`。

**步骤**
1. 完成下方验收矩阵，补齐前序任务遗留的真实行为缺口，避免重复已有断言。
2. 使用 TUI 手工验证两种主题、ASCII/Unicode 图标、窄窗口、resize、横纵滚动和鼠标菜单；记录键盘端到端路径。
3. 用专用 Redis 测试实例验证真实 TYPE/PTTL/MEMORY USAGE/分页命令；不把 mock 协议测试通过等同于真实 Redis 验证。
4. 更新用户文档：Size 口径、TTL、Vim 按键变化、格式选择、显式加载、只读语义、schema-less 边界和超预算状态。
5. 更新性能文档，记录硬件、构建模式、数据规模、测试方式和实际数值。
6. 运行 CI 等价检查：

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
```

7. 若依赖或工具链环境阻塞，记录具体命令与原因；不能把未执行的项目写成通过。
8. 用 `git diff --check` 检查格式，审查改动范围后按任务边界提交。

**验收**：所有本轮格式与交互可用，完整检查有结果，文档与默认按键一致。建议提交：`docs(redis): document rich preview and validation results`。

## 5. 最终验收矩阵

| 领域 | 必测案例 | 预期 |
|---|---|---|
| Key | 中文、超长、含控制字符、非法 UTF-8 | 完整可访问，复制语义明确，无终端注入 |
| Metadata | MEMORY 无权限、Key 消失、type 变化、Size=0 | body 可独立工作，未知不冒充零 |
| TTL | -2/-1/0/850ms/55m30s/超过一天 | 格式正确，归零只重查一次 |
| 分页 | 64 KiB 边界、UTF-8 跨页、空 SCAN 页、超 COUNT | 正确完整性和 pending，无静默跳项 |
| Stream | 空流、多个 entry、跨页 ID、删除 | 无重复边界项，无永久 Loading |
| Vim | h/l、计数、gg/G、搜索、Visual、复制 | 与现有只读编辑器一致 |
| 鼠标 | 点击、拖动、滚轮、轨道、滑块、横滚 | 坐标正确，头部固定，不发额外读取 |
| JSON | 对象、数组、scalar、转义、截断 | 格式/高亮正确，截断与非法区分 |
| YAML | map/sequence、多文档、block scalar、tag/anchor | 显示 YAML，普通文本不误识别 |
| Table | 原生集合、对象数组、嵌套值、缺失字段 | 选择/列宽/复制路由隔离 |
| Java | 普通对象、集合、enum、Block、引用、尾随内容 | 可解释 JSON，不伪造信息 |
| PHP | 中文字节长度、二进制 string、对象/引用、坏长度 | bytes 保真，无 panic |
| Pickle | bytes、tuple/set、非字符串键、不支持 opcode | 不丢项，明确支持范围 |
| Protobuf | 多字节 tag、重复字段、零长度、overflow | 完整校验，Schema-less 标识 |
| Async | A→B→A、重连、关闭 Tab、快速切格式 | 只接受当前身份/版本 |
| 性能 | 多行/单长行大值、反复切格式、缓存淘汰 | 有界内存、队列和派生工作 |
| 回归 | SQL 补全/当前语句、Relation 编辑、Output/DDL | 通用视口/表格抽取后行为保持 |

## 6. 完成记录模板

每个任务完成时在执行记录中填写：

```text
任务：Txx
改动文件：
行为结果：
定向验证命令与结果：
未通过或未执行项：
与计划的偏差及原因：
提交（如有）：
```

本轮完成定义：T01–T18 全部完成或明确记录产品接受的变更；最终格式菜单不存在占位实现；自动识别有真实验证，RAW/Hex 可回到原始字节；Preview 的 Vim、鼠标、表格操作都使用明确 owner/session 路由。
