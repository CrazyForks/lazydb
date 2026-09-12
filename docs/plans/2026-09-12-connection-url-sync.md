# 连接表单 URL 同步修复 Implementation Plan

> **执行说明：** 按任务顺序实施，每项完成相关验证后再继续；本计划不授权提交或推送。若执行环境具备 `superpowers:executing-plans`，可使用该技能逐项执行。

**Goal:** 驱动或连接字段改变后，URL 始终对应当前表单；无法生成时明确展示原因，同时保留手动 URL 编辑、解析及脱敏行为。

**Architecture:** 保留现有 `ProfileDraft` 和 `SecretTextInput`，将结构化字段生成失败与手动 URL 解析错误分开。生成失败使旧 URL 失效，提示独立渲染；提交仍由结构化字段校验确定错误焦点，不将占位文本写入连接 URL。

**Tech Stack:** Rust 2024、Ratatui 0.30、现有 profile model / formatter / integration tests。

---

## 已确认的原因和约束

- `src/model/profile_manager.rs::set_kind` 已调用 `refresh_url`；问题不在重绘或遗漏调用。
- `connection_profile_for_url` 将空 Database 转为 `None`；Oracle formatter 因缺服务名失败。
- `refresh_url` 两个失败分支直接返回，保留旧 URL 和同步状态。
- `src/profile.rs::format_connection_url` 将缺 Oracle 端口、服务名都错误地报告成 `MissingHost`。
- `tests/profile_draft.rs::structured_edits_refresh_url_and_invalid_port_keeps_last_valid_url` 明确固化了旧行为，需要更新。
- `url_error` 会被 `commit_url` / `validate` 用于阻止提交，不能直接复用为生成提示。
- 计划制定时 `src/app.rs` 已存在未提交修改；若后续确需调整调用方，先检查 diff，保留已有工作。

## 行为契约

| 事件 | 真实 URL 文本 | 生成提示 | pending / 解析错误 |
|---|---|---|---|
| 字段足够，生成成功 | 替换为当前 URL | 清除 | 清除 |
| 字段缺失或非法，生成失败 | 清空旧值和旧选择范围 | 保存字段及原因 | 清除旧 URL 编辑状态 |
| 用户开始编辑 URL | 保留原始输入 | 清除 | pending=true，清除旧解析错误 |
| 手动 URL 解析失败 | 保留输入 | 无 | 保持 pending，记录解析错误；结构化字段不变 |
| 手动 URL 解析成功 | 由解析结果更新字段，再规范化 URL | 按重新生成结果设置 | 清除 |
| 修改 Name 等非连接字段 | 不变 | 不变 | 不变 |

用户显式切换驱动或修改参与 URL 生成的结构化字段，表示以结构化字段为当前来源。若通过常规焦点切换离开正在编辑的 URL，继续遵守现有 `commit_url` 校验流程，不绕过解析失败拦截。只有手动 URL 输入可以暂时不匹配已提交字段。

## Task 1：建立失败复现和目标行为测试

**Files:** `tests/profile_draft.rs`

1. 用 `ProfileManagerState::start_new(DatabaseKind::MariaDb)` 创建无 Database 草稿，再通过 `select_driver(DatabaseKind::Oracle)` 切换。
2. 断言 kind/port 已切换、URL 不再包含 MariaDB 前缀，真实 URL 为空，生成提示关联 `ProfileField::Database`。
3. 增加填写服务名后生成 Oracle URL、再删除服务名后 URL 失效的往返测试。
4. 更新旧非法端口测试：不再断言保留最后有效 URL，改为 URL 清空、错误关联 Port、修复端口后恢复生成。为校验焦点测试先填齐 Name 等前置必填项。
5. 执行 `cargo test --test profile_draft`，确认新增断言在当前实现上失败；新访问器尚未实现时允许先出现编译失败，记录失败点。

**验收：** 回归测试能区分当前错误实现与目标实现，不能仅断言调用了刷新。

## Task 2：明确 URL 生成失败的字段和原因

**Files:** `src/model/profile_manager.rs`、`src/profile.rs`、`tests/profile_url.rs`、`tests/oracle_profile.rs`

1. 检查 `ProfileValidationError`、`ProfileError` 定义及匹配调用方，优先复用现有字段级错误和已有错误变体。
2. 将 `connection_profile_for_url` 的 `Result<ConnectionProfile, ()>` 改为字段级错误结果，至少覆盖 Host 缺失、Port 非整数/0/越界、Oracle 服务名缺失、SQLite 文件路径缺失。
3. Oracle 使用与其他网络驱动一致的端口范围校验，避免 `.parse().ok()` 隐去错误。
4. 保留当前其他驱动允许未填 Database 仍生成 URL 的行为；URL 生成不等于完整的保存校验。
5. 修正 Oracle formatter：缺 Host、Port、Service Name 返回对应错误。仅在现有枚举不能表达时增加最小错误变体，更新必要的穷举匹配。
6. 增加 formatter 测试，断言缺端口/服务名不会再报告 MissingHost，完整 Oracle URL 输出保持不变。
7. 执行 `cargo test --test profile_url --test oracle_profile`。

**验收：** 生成失败具有准确字段与安全、固定的提示信息；错误中不嵌入原始 URL 或密码。

## Task 3：修复模型同步和 URL 编辑边界

**Files:** `src/model/profile_manager.rs`、`tests/profile_draft.rs`

1. 在 `ProfileDraft` 增加私有 `url_generation_error: Option<ProfileValidationError>` 及只读访问器；更新 new/edit 初始化。
2. 把 `refresh_url` 改为完整处理 Result：成功替换 URL 并清除生成提示；失败清空旧 URL 并保存生成原因。两条分支均清除旧选择范围、pending 和旧解析错误。
3. formatter 的剩余错误转换为字段级错误；不能识别的格式化错误关联 Url，避免伪装为某个必填字段缺失。
4. `mark_url_edited` 清除生成提示，保留现有 secret input、undo/redo 和解析原子性。
5. 缩小 `connection_field_changed` 的刷新范围，只有实际参与 URL 生成的字段才刷新。审查 `cycle`、read-only / SQLite memory toggle 等独立调用路径，保留其必要刷新。
6. `validate` 保持现有结构化字段校验顺序，不把生成提示提前映射成 Url 错误；完整字段校验通过后若仍有无法生成的错误，再阻止提交并返回准确错误。
7. `commit_url` 对无手动编辑的空生成值不执行解析，不让缺服务名阻断用户从 URL 返回字段填写。
8. 增加 pending URL 下修改 Name 不丢输入，以及字段修复后错误状态清除的测试。
9. 执行 `cargo test --test profile_draft`。

**验收：** 所有模型回归通过；切换驱动失败后旧 URL、旧选择范围及旧编辑历史不可复活；手动编辑仍可撤销/重做并正确脱敏。

## Task 4：界面展示生成提示和 Oracle 字段语义

**Files:** `src/ui/profiles.rs`、`tests/ui_render.rs`

1. Oracle 的 `ProfileField::Database` 显示为 `Service Name`；保留底层字段标识以复用导航和校验焦点。
2. 调整 `field_label` 及必要调用方传入驱动上下文，检查 `field_label_width` 在窄终端的对齐。
3. 非 URL 编辑状态、生成失败时，在 URL 值区域以 muted 样式展示固定提示，例如 `Pending: service name is required`；不改 `url_display` 的真实值语义。
4. URL 获得焦点后展示真实空输入并允许粘贴，在现有 `layout.url_help` 展示生成原因；无生成错误时保留现有驱动格式帮助。
5. 生成提示不注册成 URL 文本选择内容；光标、复制、选择范围仍只基于真实 URL。保留 URL 区域聚焦点击入口。
6. 不覆盖 `layout.feedback` 的测试/保存结果；新建连接字段未填齐时使用中性提示而非持续红色报错。
7. 增加 UI 渲染测试：Oracle 标签正确、未填服务名出现提示、不出现 MariaDB URL；填齐后提示消失；窄布局无覆盖；URL 聚焦仍显示帮助并可编辑。
8. 执行 `cargo test --test ui_render profile`；新增测试名使用 profile 前缀，核对输出确实执行了新测试。

**验收：** 可见文本不会冒充可复制或可提交的 URL，焦点和帮助行为一致。

## Task 5：跨驱动和提交路径回归

**Files:** `tests/profile_draft.rs`、`tests/profile_reducer.rs`、按需 `tests/profile_lifecycle.rs`

1. 补充切换 SQLite 缺路径、设置路径、memory toggle 的 URL 往返测试。
2. 对 Host 清空、Port 空值/非数字/0/65536 建立表驱动用例，验证失效与恢复。
3. 覆盖键盘 cycle 与 select_driver 两条入口；核心断言是 URL 与当前 kind 一致或明确不可生成，不扩大到默认端口策略重构。
4. 在 reducer 层验证 Test / Save 缺服务名时返回 Database 字段错误；准备完整 Name、Host、Port，排除前置校验干扰。
5. 回归手动无效 URL 不覆盖字段、URL 密码脱敏、密码不进入生成提示、URL 编辑 undo/redo。
6. 执行 `cargo test --test profile_draft --test profile_url --test oracle_profile --test profile_reducer --test profile_lifecycle`。

**验收：** 不依赖真实数据库即可验证状态同步和提交前校验。

## Task 6：最终检查和人工验收

1. 执行 `cargo fmt --check`。
2. 执行 `cargo test --test ui_render`，覆盖完整 UI 测试目标。
3. 执行 `cargo clippy --all-targets -- -D warnings`；如被已有问题阻塞，记录具体位置并区分本次引入的问题。
4. 若默认 Oracle feature 因本机客户端依赖无法完成验证，记录原因，可补跑 `cargo test --no-default-features --test profile_draft --test profile_url --test ui_render`；不能将此作为默认构建已通过的证据。
5. 人工按截图路径 PostgreSQL → MySQL → MariaDB → Oracle 切换，确认端口变化时旧 URL 即失效；填写/清空服务名，检查 URL 往返更新。
6. 人工测试空 SQLite 路径、非法端口、URL 粘贴错误后的返回字段行为、窄终端显示及提示不可当作 URL 复制。
7. 审查 `git diff --check` 和实际 diff，确认改动范围与计划一致。

## 完成标准

- 自动生成 URL 要么对应当前表单，要么为空且显示准确原因。
- Oracle 缺服务名不会再显示上一驱动的 URL，也不会误报缺 Host。
- 手动 URL 编辑、解析错误保留、脱敏及提交校验焦点通过回归。
- 相关模型、格式化、reducer、UI 测试通过；未完成的环境验证明确记录。

## 实施顺序与交付

按 Task 1 → 2 → 3 → 4 → 5 → 6 顺序执行。Task 2 与 Task 3 共同修改模型，应串行落地。

最终交付包括模型同步修复、Oracle 标签/错误语义修复、回归测试和验证结果。计划不包含驱动默认端口策略重构、Oracle 其他连接格式扩展或连接表单整体改版。
