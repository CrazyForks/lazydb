# Oracle 连接 URL 草稿预览 Implementation Plan

> **执行说明：** 按任务顺序实施并逐项验证；若执行环境提供 `superpowers:executing-plans`，可使用该技能执行。本计划不授权 commit、push 或并行子代理执行。

**Goal:** Oracle 连接字段尚未填齐时，仍显示当前连接的 URL 结构和缺失原因，同时保留真实 URL 编辑、严格提交校验和脱敏行为。

**Architecture:** 继续以 `ProfileDraft` 的结构化字段和现有真实 URL 输入作为数据来源，仅增加按需计算的 Oracle 展示预览，不增加持久化状态。严格 formatter、URL parser 和连接适配器不接受展示占位符；UI 根据生成失败与焦点状态选择模板或真实输入。为 Oracle 使用独立字段列表，并统一网络驱动默认端口的切换规则。

**Tech Stack:** Rust 2024、Ratatui 0.30.2、现有 ProfileDraft / SecretTextInput、集成测试和 TestBackend。

---

## 一、现状与范围

已确认的代码依据（行号以制定计划时为准，实施时按符号定位）：

| 位置 | 当前行为 |
|---|---|
| `src/model/profile_manager.rs::set_kind`，约 1406 行 | 切换驱动后调用 `refresh_url`；各驱动分别处理默认端口，规则不对称 |
| `src/model/profile_manager.rs::connection_profile_for_url`，约 1535 行 | Oracle 的 Host、Port、Service Name 参与生成校验 |
| `src/model/profile_manager.rs::refresh_url`，约 1505 行 | 失败时清空真实 URL，记录字段级 `url_generation_error` |
| `src/profile.rs::format_connection_url`，约 958 行 | Oracle 缺服务名时返回 `MissingOracleService` |
| `src/ui/profiles.rs`，约 185 行 | 仅 URL 聚焦时在帮助行显示生成失败原因 |
| `src/model/profile_manager.rs::visible_fields`，约 1099 行 | Oracle 复用 `POSTGRES_FIELDS`，因此显示无效的 Default schema |
| `src/model/profile_manager.rs::validate` | Oracle 的 `default_schema` 始终提交为 None；Service Name 必填 |
| `src/db/oracle.rs::OracleAdapter::connect` | 实际连接使用 Host、Port、Service Name、User、Password |
| `tests/profile_draft.rs::switching_to_oracle_does_not_keep_the_previous_driver_url` | 明确要求切换到缺服务名的 Oracle 时真实 URL 为空 |

已有 `docs/plans/2026-09-12-connection-url-sync.md` 描述的是上一阶段同步修复。本计划建立在当前源码已有的“失败清空旧 URL + 保存生成错误”行为之上，不覆盖该文件，也不重复实施其已完成部分。

本次交付：

1. Oracle 不完整草稿的展示模板及持续可见的生成提示。
2. 模板与输入、复制、选择、提交严格分离。
3. Oracle 独立字段列表，隐藏当前不生效的 Default schema。
4. 网络驱动默认端口切换的对称规则。

范围边界：其他驱动沿用现有真实 URL 生成方式，但其生成错误也可在非聚焦帮助行展示。其他驱动的占位模板、Oracle schema 切换能力、认证模式、SSL 行为和 URL 协议扩展不属于本次实现。

## 二、行为契约

### 2.1 默认值

- 新建 Oracle：Host=`localhost`，Port=`1521`。
- Service Name、User、Password 保持未填写，不自动填充示例服务名或账号。
- 用户主动清空 Host/Port 后，预览必须表示缺失，不能在渲染时恢复默认值。
- 默认值只在新建和明确切换驱动时按既有/统一规则设置。

### 2.2 展示与编辑

| 状态 | URL 值区域 | 帮助区域 | 真实 URL |
|---|---|---|---|
| Oracle 缺服务名，焦点在其他字段 | `jdbc:oracle:thin:@localhost:1521/<service-name>` | `Preview only · service name is required` | 空 |
| Oracle 缺 Host | 使用 `<host>` | 显示当前字段级生成原因 | 空 |
| Oracle Port 为空 | 使用 `<port>` | 显示端口原因 | 空 |
| Oracle Port 非数字、0 或越界 | 使用 `<invalid-port>` | 显示端口范围要求 | 空 |
| Oracle 多个字段缺失 | 缺失位置全部占位 | 使用现有首个生成错误，不新增错误聚合系统 | 空 |
| Oracle 字段足够 | 现有 formatter 生成值 | 非聚焦时无生成提示 | 有效 URL |
| 生成失败且 URL 获得焦点 | 空的真实可编辑输入 | 缺失原因优先，可附加格式帮助 | 空 |
| 手动编辑 URL 尚未提交 | 用户输入的脱敏显示 | 保持现有编辑/解析反馈 | 原始用户输入 |
| 手动 URL 解析失败 | 保留用户输入，不替换为模板 | 保持现有错误反馈 | 原始用户输入 |

示例服务名填入 `app_service` 后，实际 URL 为：

```text
jdbc:oracle:thin:@localhost:1521/app_service
```

User 填入后使用现有 `?user=...` 编码规则。展示模板不读取密码，不构造密码占位串；若包含 User，必须使用与 formatter 一致的 QUERY_VALUE 编码方式。

模板本身不保证可连接或可解析，必须配套 `Preview only` 标识。缺失原因优先于冗长的格式说明，保证窄终端首先能看到需要补填什么。

### 2.3 选择、复制和状态

- `url_display()` 继续只返回真实 URL 的脱敏结果。
- 预览模板不得传入 `self.url.set(...)`，不参与 pending、undo/redo、parser、配置保存和连接参数。
- 模板显示时不注册 `ProfileUrl` 文本选择命中映射；URL 行的点击聚焦入口仍保留。
- URL 聚焦后恢复现有输入命中映射和光标行为，不把模板长度用于光标偏移。
- 失败清空旧 URL/旧选择的行为保留，不能退回显示上一 Driver 的 URL。
- 缺 Service Name 时仍由结构化提交校验定位 `ProfileField::Database`，不能改成泛化的 Url 错误。

## 三、实施任务

### Task 1：建立 Oracle 草稿预览契约与模型实现

**Files:**
- Modify: `src/model/profile_manager.rs`
- Test: `tests/profile_draft.rs`

**步骤：**

1. 检查 `ProfileDraft`、`refresh_url`、`mark_url_edited`、`commit_url`、`url_display` 和现有测试构造方式，确认手动输入与生成失败的现有状态关系。
2. 增加针对展示预览的只读方法，建议接口为 `pub fn oracle_url_preview(&self) -> Option<String>`。只在 kind 为 Oracle 且存在 `url_generation_error`、无手动 pending/解析错误时返回模板；其余返回 None。方法按需计算，不新增缓存字段。
3. 为 Host、Port、Service Name 分别处理占位：空白 Host 为 `<host>`；空白 Port 为 `<port>`；非有效 u16 或 0 为 `<invalid-port>`；空白 Service Name 为 `<service-name>`。有效字段使用与真实生成一致的 trim 语义。UI 仍通过 `safe_line` 处理终端控制字符。
4. User 为空则省略查询参数；非空时沿用现有 percent-encoding 规则。优先复用现有编码常量；若常量不可访问，只抽取最小共享编码帮助方法，不公开严格 formatter 内部的通用容错模式。
5. 在 `tests/profile_draft.rs` 添加表驱动覆盖：缺服务名、缺 Host、空 Port、abc/0/65536、多字段缺失、非空 User 中的 `&`/`?` 等字符。
6. 增加往返测试：切换 Oracle → 出现模板且真实 URL 为空 → 填服务名 → 预览 None 且真实 URL 正确 → 删除服务名 → 真实 URL 再次失效且模板恢复。
7. 保留已有 `switching_to_oracle_does_not_keep_the_previous_driver_url` 对真实空 URL 的断言，并补充模板断言；不把该测试改成 `url_display()` 返回占位文本。
8. 增加状态测试：手动粘贴、输入、解析失败时模板不覆盖输入；Name 等非连接字段编辑不改变 URL；生成错误恢复后提示清除。

**验证：**

```bash
cargo test --test profile_draft
```

新增用例应先证明当前版本缺少模板能力，再在实现后全部通过。不得只验证模板包含 `jdbc`，必须校验当前字段及真实 URL 的分离。

### Task 2：UI 展示、帮助行与输入选择边界

**Files:**
- Modify: `src/ui/profiles.rs`
- Test: `tests/ui_render.rs`

**步骤：**

1. 在 `render_field` 中计算是否正在展示 Oracle 模板：Url 字段、未激活编辑、`oracle_url_preview()` 返回 Some。
2. 模板模式使用 muted 样式渲染整个展示值，继续使用 `safe_line`、现有宽度截断能力和 value_area；不创建第二个输入控件，也不新增持久化 UI 状态。
3. 将 `ProfileUrl` 输入选择命中映射的注册限制为真实输入显示模式。保留 URL 行现有 `HitTarget::ProfileField(ProfileField::Url)`，确保点击模板可以进入 URL 编辑。
4. 帮助行逻辑调整为：有生成错误时始终显示原因；模板模式增加 `Preview only` 前缀；无生成错误且 URL 聚焦时显示现有格式帮助。原因放前面，避免窄宽度下被格式说明挤掉。
5. URL 聚焦时显示真实空输入及光标，维持原有文本滚动、选择、粘贴、解析逻辑；非聚焦模板不使用真实 URL 光标计算滚动。
6. 不占用 `layout.feedback`，Test/Save 成功或失败反馈仍由 `render_message_line` 渲染。
7. 增加 `profile_` 前缀 UI 测试，覆盖：Password 焦点下模板与原因可见、URL 聚焦后模板消失而原因保留、字段填齐后真实 URL 可见且提示消失。
8. 检查命中区域：模板模式没有 ProfileUrl 文本选择目标，但 URL 点击聚焦入口存在；聚焦后输入目标恢复。
9. 用项目现有 TestBackend 构造覆盖常规宽度、80 列及现有最小布局宽度；断言缺失原因可辨认、无按钮/反馈覆盖、终端控制字符不直接进入输出。

**验证：**

```bash
cargo test --test ui_render profile
```

核对执行数量，确认新增测试确实被过滤器选中。

### Task 3：Oracle 专用字段列表

**Files:**
- Modify: `src/model/profile_manager.rs`
- Test: `tests/profile_draft.rs`
- Test: `tests/ui_render.rs`

**步骤：**

1. 在现有驱动字段数组附近定义 `ORACLE_FIELDS`，按当前网络表单顺序包含 Kind、Name、Host、Port、Database、VisibleObjects、User、Password、PasswordStorage、SslMode、Environment、ReadOnly、Url、Test、Save、SaveAndConnect、Cancel；不包含 Schema，也不新增 UrlFormat 可见项。
2. 将 `visible_fields()` 的 Oracle 分支切换为 `ORACLE_FIELDS`；Database 的 UI 标签继续复用已有 `Service Name`。
3. 检查 `select_driver` 和 cycle 后的焦点归一化：若旧焦点对应新驱动隐藏字段，必须回到合法可见字段，不留下隐形焦点。复用当前实现，只有缺口才修改。
4. 保留 Oracle `default_schema=None` 的提交语义；不新增 schema 持久化或会话初始化逻辑。不需要为显示问题破坏性清空其他驱动的自定义 schema 草稿。
5. 增加模型测试：Oracle 不包含 Schema；PostgreSQL/SQL Server 仍包含 Schema；Tab/Shift-Tab 的 Oracle 导航能到 Url 和操作按钮。
6. 增加 UI 测试：从 SQL Server 带 dbo 的草稿切换 Oracle，界面不显示 Default schema，但 Service Name 可见。

**验证：**

```bash
cargo test --test profile_draft
cargo test --test ui_render profile
```

### Task 4：统一网络驱动默认端口切换

**Files:**
- Modify: `src/model/profile_manager.rs`
- Test: `tests/profile_draft.rs`

**规则：**

| Driver | 默认端口 |
|---|---|
| PostgreSQL | 5432 |
| MySQL / MariaDB | 3306 |
| Oracle | 1521 |
| SQL Server | 1433 |
| SQLite | 无网络端口 |

仅当目标为网络驱动，且当前端口为空或数值等于前一网络驱动默认端口时，替换为目标默认端口。自定义端口和非空非法输入保留，由现有校验提示。新旧 kind 相同时继续直接返回。

**步骤：**

1. 先检查是否已有可复用的默认端口方法。若不存在，在 profile manager 添加返回 `Option<u16>` 的局部帮助函数；不为此重构整个 DatabaseKind 公共接口。
2. 在 `set_kind` 修改 kind 前，根据 previous、target 和当前输入计算是否替换端口。
3. 删除各网络驱动分支中被统一规则替代的端口条件，保留 Host、schema、SSL 及最终 URL 刷新逻辑。
4. 明确 SQLite 边界：切入 SQLite 不改隐藏端口；从 SQLite 切出时仅空端口自动补默认值，非空隐藏端口按保留规则处理。不新增每驱动草稿记忆。
5. 添加所有网络驱动有向切换对的表驱动测试：旧默认端口会迁移，自定义端口不变；补充空值、非法值、相同 Driver、MariaDB/MySQL 和 SQLite 边界。
6. 同时检查切换后的真实 URL 或模板使用最终端口，没有一帧/一次事件的旧端口残留。

**验证：**

```bash
cargo test --test profile_draft
```

### Task 5：提交、解析、凭据和复制回归

**Files:**
- Test: `tests/profile_reducer.rs`
- Test: `tests/profile_draft.rs`
- Test: `tests/profile_url.rs`
- Test: `tests/oracle_profile.rs`
- Reference: `tests/profile_lifecycle.rs`
- Modify only if needed: `src/app.rs` 中现有 profile 操作分支

**步骤：**

1. 在 reducer 测试中构造完整 Name/Host/Port、缺 Service Name 的 Oracle 草稿，分别触发 Test、Save、Save & Connect。
2. 断言错误焦点为 Database，且不产生相应的测试连接、持久化或连接副作用；排除 Name 必填项等前置错误干扰。
3. 填齐字段后验证提交 profile.database 为实际 Service Name、default_schema 为 None，Host/Port 不含占位符。
4. 保留严格 formatter 缺 Service Name 的错误测试；不得为了预览将 formatter 改为接受空服务名。
5. 回归手动完整 JDBC URL 导入、解析失败保留输入、密码脱敏、undo/redo、选择复制。模板状态的复制结果不得包含占位文本。
6. 测试和 UI 断言仅使用虚构凭据。预览方法不能读取 SecretTextInput 的密码值，也不能把原始 URL 放入帮助提示。
7. 若现有 lifecycle 测试已覆盖凭据保存，运行它们即可；只有本次修改影响对应路径时才补新用例。

**验证：**

```bash
cargo test --test profile_draft --test profile_reducer --test profile_url --test oracle_profile --test profile_lifecycle
```

这些测试不需要真实 Oracle 实例来证明预览、提交阻断和模型行为。实际网络连通性不作为本次预览功能的自动验收条件。

### Task 6：最终检查与人工验收

**步骤：**

1. 检查 `git diff`，确认实现集中于计划文件，不覆盖工作区已有修改。
2. 运行格式和编译检查：

```bash
cargo fmt --all -- --check
cargo check
```

3. 若默认 Oracle feature 在当前环境因依赖无法编译，记录具体错误；可补充 `cargo check --no-default-features` 作为降级检查，但不能将其报告为默认构建通过。
4. 启动 TUI 并按以下脚本人工验收：
   - 新建连接，依次切换 PostgreSQL → MySQL → MariaDB → Oracle，观察前缀与默认端口。
   - 停在 Password，确认 Oracle 模板及 Service Name 提示仍可见。
   - 填写服务名、清空服务名、清空 Host、输入非法端口，确认即时更新及恢复。
   - 点击 URL 模板，确认进入真实输入；粘贴完整 JDBC URL，确认字段回填。
   - 从 SQL Server 切换 Oracle，确认不显示 Default schema；再切回 SQL Server，确认默认端口规则。
   - 缩窄窗口，确认帮助、按钮和反馈不互相覆盖。
5. 记录实际执行的命令、通过情况和环境阻塞；不要声称未运行的测试通过。

## 四、完成标准

- [ ] Oracle 缺 Service Name 时 URL 区域显示当前驱动结构和可见缺失原因。
- [ ] 占位符从未进入真实 URL、复制内容、保存配置或连接请求。
- [ ] 用户进入 URL 编辑后，可以继续粘贴、解析、选择和撤销/重做。
- [ ] 字段补全后恢复既有 formatter 输出，字段再次非法时不会显示旧 URL。
- [ ] Oracle 不再展示当前不生效的 Default schema。
- [ ] 网络驱动默认端口切换对称，自定义端口保留。
- [ ] Test/Save/Save & Connect 缺服务名仍返回准确字段错误。
- [ ] 相关测试、格式和构建检查的结果有实际记录。

## 五、执行顺序与交付说明

按 Task 1 → 2 → 3 → 4 → 5 → 6 顺序执行。先完成预览和输入边界，再处理字段及默认端口，最后做提交回归。每项改动后运行其最小相关测试；只有后续改动影响同一范围时才重复运行。

本计划采用统一模板弱化样式和现有字段级错误，不引入分段富文本模型、每驱动草稿缓存或通用 URL 模板框架。完成实现后提供改动摘要、验证结果和必要的环境限制即可。
