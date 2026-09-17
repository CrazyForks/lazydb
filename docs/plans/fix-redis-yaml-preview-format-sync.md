# Redis YAML Preview Implementation Plan

> 执行交接：由 Luna 按本计划实现、验证、审查及提交合并；Astra 仅负责分析与计划。遵循当前任务工作流，不启动子 Agent。

**Goal:** 让 `lazydb:test:yaml` 自动以多行 YAML 展示，且手动 Raw/YAML/Auto 切换立即生效、状态一致、不丢失草稿。

**Architecture:** 在现有字节保真预览链路增加保守 YAML 候选，继续复用 serde_yaml 格式化。将 Redis 格式切换改为“计算目标、检查草稿、准备文本、更新编辑器、提交状态”的一次完整操作，保留全局只读接口的能力约束。

**Tech Stack:** Rust 2024、serde_yaml 0.9、Redis、现有 App reducer / EditorWorkspace / ratatui 测试体系。

---

## 起点、范围与交接

- 工作区 `/Users/yelog/workspace/tui/lazydb`；目标 `main`；起点 `050b1412c052a44469ec9523cde86704985bf9f8`。
- 建议任务名：**修复 Redis YAML 自动识别与格式切换**；建议分支 `fix/redis-yaml-preview`，由工作流在计划完成后命名/创建，本阶段不创建分支。
- 分析：`.git/opencode-tasks/ses_f51ff9bc6ffedwgwvPFifpSB2n/analysis.md`；验证日志在同目录 `validation.md`。
- 当前原始数据正常，55 bytes、4 个真实 LF；无需 SET。截图附件在当前上下文不可读取。
- 已有无关未跟踪文档及 `.git-opencode-tasks/` 保留；提交使用显式路径。
- 本计划是**一个端到端验收单元**，下面任务是实施顺序。完成整个闭环才算修复完成，不能仅交付检测器。

## 已确认的调用链

1. `src/app.rs:13065-13135`：`RedisValuePageLoaded` 自动检测并将小字符串通过 `open_value` 打开为 Editable。
2. `src/value_preview/detect.rs:3-73`：没有 YAML 候选。
3. `src/ui/redis_value.rs:146-174`：Yaml 已直接对原始 bytes 解析、输出多行；Raw 的 `display_bytes` 将 LF 转义。
4. `src/app.rs:13189-13225`：Accept 调 `set_read_only_text` 并忽略错误。
5. `src/editor/mod.rs:2107-2119`：上述接口拒绝 Editable，故菜单变化而文档不变。
6. Auto reset 只设 Raw，Accept 没有立即检测；content.format 和 baseline 也未同步。

## Task 1：为报告场景建立真实文档回归测试

**Files**
- Create: `tests/redis_yaml_preview.rs`
- 参考 fixture：`tests/redis_unsaved_changes.rs:50-127`
- 参考格式菜单：`tests/redis_browser_tabs.rs:233-275`

步骤：

1. 复用上述 fixture 模式构建连接、RedisBrowserTab、key tree 与完整 String page；输入严格为 `b"name: lazydb\nversion: 1\nfeatures:\n  - redis\n  - preview"`。用真实 `RedisValuePageLoaded` action 建立会话，不手工模拟只读会话。
2. 写 `yaml_auto_load_formats_actual_editor_document`：初次 Auto 选 YAML，`app.editor_text(preview_editor_id)` 包含真实 LF 且不含 `\\x0a`；用 serde_yaml Value 比较内容语义，不绑定 emitter 的缩进风格。
3. 写 `manual_yaml_selection_replaces_editable_raw_document`：加载前将 tab.format 手动设 RAW，加载后确认单行转义；通过 `RedisPreviewCycleFormat`、`RedisPreviewFormatMove`、`RedisPreviewFormatAccept` 选择 YAML；检查实际 editor_text、selected、automatic=false、content.format、baseline。
4. 写 `auto_selection_redetects_loaded_yaml`：YAML→Raw→Auto，Auto 立即回到 YAML，无重新加载。
5. 每次切换验证 `!tab.value_is_dirty(&app.editor_text(id).unwrap())`；commands 不含 mutation。不要使用 `active_editor_text()`，它针对 SQL console；公共 `editor_text(id)` 可直接读取预览，无需新增生产测试接口。
6. 运行 `cargo test --test redis_yaml_preview`，记录红灯的具体失败：自动格式是 Raw / 手动文档仍旧，不把编译错误当成功复现。

## Task 2：增加保守 YAML 候选

**Files**
- Modify: `src/value_preview/detect.rs`
- Test: `tests/value_preview.rs`, `tests/value_preview_limits.rs`

步骤：

1. 添加检测用例：原生 YAML mapping、sequence；JSON object/array 仍 JSON；普通文本、空白、非法 YAML 不自动 YAML；二进制仍遵循原策略；collection 仍 Table。
2. 新增以下候选逻辑，置于 Raw fallback 之前，保留其他检测及优先级：

```rust
if data.len() <= super::MAX_PREVIEW_INPUT_BYTES
    && std::str::from_utf8(data).is_ok()
    && let Ok(value) = serde_yaml::from_slice::<serde_yaml::Value>(data)
    && matches!(value, serde_yaml::Value::Mapping(_) | serde_yaml::Value::Sequence(_))
{
    candidates.push(FormatCandidate {
        format: PreviewFormat::YAML,
        confidence: 90,
        reason: "valid structured YAML",
        status: DecodeStatus::Complete,
    });
}
```

3. JSON confidence 100 保持优先；强序列化头 95/100 保持优先；顶层 YAML scalar 不进入候选。不要检测 key 名，也不要全局反转义 `\\n`。
4. `tests/value_preview_limits.rs` 新增超过 `MAX_PREVIEW_INPUT_BYTES` 的 YAML mapping 输入，断言候选中无 YAML，避免扩大自动解析预算；不要要求其他既有检测器改变行为。
5. 运行 `cargo test --test value_preview --test value_preview_limits`，记录结果。此时 Task 1 中手动切换测试仍应红灯，继续 Task 3。

## Task 3：修复格式应用与编辑状态一致性

**Files**
- Modify: `src/app.rs` 的 `RedisPreviewFormatAccept`；需要复用时增加私有 Redis helper
- 使用既有：`src/editor/mod.rs::set_text` / `open_value` / `open_read_only`
- 使用既有：`src/model/redis_preview.rs`、`src/model/redis_browser.rs`
- Test: `tests/redis_yaml_preview.rs`

实施顺序与明确决策：

1. 先读当前 loaded page、旧 format、content、editor id、baseline 与 editor text。无 Ready page 时沿用空预览的菜单选择行为（已有 `preview_controls_open_picker_and_apply_only_on_enter` 依赖此行为），不伪造 content 或会话；真正加载后再应用。
2. 若当前文档 dirty，取消此次格式变更，保留文档/旧格式/基线，提示先保存或丢弃。不能先修改 tab.format 再拒绝。此最小策略不扩展现有确认 action 模型。
3. 计算候选格式状态：从现有状态拷贝，手动选项调用 select；Auto 调 reset_auto 后立刻对当前 String 原 bytes 做 default_format，对 collection 选 Table。
4. `format_page` 准备新文本。手动解析失败时保持旧状态和文档并显示解析错误；避免将 Raw 回退结果标成成功 YAML。加载路径已有 fallback 可保持原行为，防止扩大范围。
5. 对已存在会话优先用 `EditorWorkspace::set_text`，它支持现有 capability 并重置 history、cursor、viewport、mode、revision（`src/editor/mod.rs:2087-2104`），不会触发只读拒绝；失败时提示且不提交 tab 状态。不要放宽 `set_read_only_text`。
6. 会话缺失时按当前基线/加载策略用 open_value 或 open_read_only 创建；不把只读大值或 collection 升级成可编辑。
7. 编辑器成功后一次提交：tab.format、Ready content.format、preview_scroll=0；有可编辑 baseline 时替换为新文本并将 value_edit_revision 与当前会话 revision 对齐；只读会话保持 baseline=None。确保下一次真实编辑仍被判 dirty。
8. 检查 session 恢复路径 `ensure_read_only_session` 使用新 content.format；通过测试保证不会恢复旧 Raw。
9. 对大字符串沿用既有 off-thread 管线与 generation/format 校验，不新增每次切换同步解析大型数据的回归。若复用 `FormatLargeRedisValuePage`，将准备/提交逻辑复用于其回调，保留只读策略，并保证失败可见、迟到结果不能覆盖新选择或草稿。
10. 运行 `cargo test --test redis_yaml_preview --test redis_browser_tabs --test redis_unsaved_changes`。以实际文档与状态同时正确为通过条件。

备注：第 1 条细化分析中的空 page 行为：已存在的空预览菜单选择是允许的偏好设置；需要一致性约束的是已加载且实际呈现的文档。

## Task 4：补齐直接受影响的边界

**Files**
- Test: `tests/redis_yaml_preview.rs`
- Test: `tests/redis_unsaved_changes.rs`（优先复用当前 dirty fixture）
- Test: `tests/redis_preview_serialization.rs`

步骤：

1. 直接对本例 bytes 执行 format_page(YAML)，验证多行且 YAML 语义一致；Raw 保留转义。避免只测 JSON→YAML。
2. 编辑草稿后尝试切换：草稿、旧格式、baseline 不变；不发写命令。取消/保存等后续操作仍按原流程执行。
3. 干净切换后 undo 不恢复前一展示并制造脏状态；随后真实输入能产生 dirty，再换 key 仍触发现有未保存提示。
4. 无效 YAML 手动选择：保持旧文档与格式并产生提示。
5. 已加载集合只读会话执行支持的格式切换，保持只读且不出现 baseline；不把多列集合文本误当作通用 YAML 数据模型。
6. 大值测试以 >32 KiB 合法 YAML 模拟 `FormatLargeRedisValuePage` 返回，不依赖 Redis 或 PTY；覆盖正确格式返回、旧格式/旧 generation 返回被忽略、失败不静默冒充成功。若没有修改其分派流程，仍验证旧结果不会覆盖当前选择。
7. 运行一次闭环定向集：

```sh
cargo test --test redis_yaml_preview --test value_preview --test value_preview_limits --test redis_preview_serialization --test redis_browser_tabs --test redis_unsaved_changes
```

## Task 5：完整验证、审查、交付（Luna）

### 验证类别与收尾规则

- **用户需求验收（必须）**：本例自动 YAML、多行实际文档、手动切换立即生效；数据正常则不改写。用 Task 1–4 的自动化回归提供可重复证据，真实未保存草稿及直接受影响路径不得回归。
- **项目已有门禁**：下列 fmt/clippy/test 命令来自 release workflow，保持既有检查标准。该 workflow 的发布级标准不等于要求本任务新增发布、部署或外部数据库搭建；执行并如实记录当前环境结果，不能将跳过/环境失败标为通过。
- **补充建议验证（非新增门禁）**：人工 TUI、PTY、截图对比、对本地实际 key 的体验回放与只读前后对照。不能因未获得图片或 PTY 环境而将已完成的自动化闭环无限置为 progress。
- 环境受限检查最多一次有针对性的修复重试，随后由 Luna 收尾审查决定补充替代证据或记录限制；真实代码错误继续修复。相关代码或环境未变化不重跑已完成检查。

1. 实现完成后按仓库 `.github/workflows/release.yml:73-75` 的检查标准运行一次：

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

若外部数据库/Oracle 环境不具备，区分实际失败、跳过与通过；按项目 CI 对应环境补证据，不能声称全量通过。仅因相关代码或环境变化重跑。

2. 补充体验检查：本地最新构建打开 `redis://localhost:6379/0` 的目标 key，确认首次 Auto、Raw→YAML、YAML→Raw→Auto、dirty 保护；通过 STRLEN/GET/PTTL 只读检查确认未写 Redis。人工/PTY 属于补充，不是本任务新增强制门禁；受限时最多一次针对性修复重试，随后记录限制。
3. `validation.md` 逐次记录命令、退出结果、代码版本/工作区状态、环境及跳过项。首次分析已有 22 项测试通过，不得作为修复后结果。
4. Luna 审查 diff：没有全局转义替换、全局只读 API 放宽、无关格式重写或无关文档覆盖；检测、切换、baseline、异步结果属于同一业务闭环。
5. 显式 stage 本任务业务文件与计划，建议一个完整修复提交 `fix(redis): detect YAML and synchronize preview format changes`。按工作流要求合并 main；不由 Astra 提交或合并。

## 最终验收标准

- 本例自动多行 YAML、手动切换即时生效、Auto 立即重检。
- 格式标签、content、文档、baseline 一致，格式切换不产生数据写入或伪脏状态。
- 真实草稿保留；JSON、普通文本、二进制、集合与只读大值不回归。
- 定向测试覆盖实际 Editable 会话，而非只检查菜单枚举。
- 数据无需改写；用户获得的是应用修复。
