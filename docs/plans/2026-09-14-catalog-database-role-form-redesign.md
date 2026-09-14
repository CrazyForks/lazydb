# Database / User / Role 表单统一设计实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 将新增及编辑 Database / User / Role 升级为新增连接同款的紧凑单列表单，统一字段焦点、键鼠输入、分组、反馈及 SQL 预览操作。

**Architecture:** 复用 Catalog 现有字段、Owner picker、Action/reducer 与 SQL 预览链路，借鉴 Profile 的行式布局和焦点视口。DatabaseDraft / RoleDraft 迁移至现有 CatalogFormFocus 类型化焦点；密码编辑抽取并复用 Profile 的 SecretTextInput。布局仅负责展示，字段能力及导航顺序由模型统一定义。

**Tech Stack:** Rust 2024、Ratatui 0.30.2、Crossterm 0.29、secrecy 0.10、现有 TestBackend 集成测试。

---

## 1. 实施基线与已确认的问题

以下行号是制定计划时的定位参考，执行时以符号为准。

| 位置 | 现状 | 对应改造 |
| --- | --- | --- |
| `src/ui/catalog_editor.rs::render`，约 31 行 | 所有页面统一采用最大 106 × 34 窗口 | 为 Database / Role Form 单独计算内容高度 |
| `render_role` / `render_database`，约 571 / 606 行 | Paragraph 拼接摘要，不使用 selected_field | 逐字段渲染、光标、高亮、鼠标区域 |
| `target_label`，约 2397 行 | Profile anchor 显示完整 UUID | 按 anchor 查连接名称及驱动 |
| `src/ui/profiles.rs::render_form` / `form_layout` | 已有逐行渲染、固定底部、焦点视口 | 作为视觉与布局参照 |
| `src/model/catalog_editor.rs::RoleDraft` | 11 个数字焦点，Password/Memberships 没有接入输入 | 类型化焦点与完整字段顺序 |
| `DatabaseDraft` | 13 个数字焦点，创建选项编辑限制在局部写入方法中 | 统一字段可用性 |
| `CatalogDraft::focus_is_toggle` | 只识别 Sequence、MaterializedView | 补齐 Database / Role |
| `CatalogDraft::paste` / `move_left` 等 | Database / Role 缺少对应分支 | 补齐编辑分发 |
| `CatalogDraft::input_for_target` | 无 Database / Role 光标映射 | 接入 FormField 路径 |
| `src/model/profile_manager.rs::SecretTextInput` | 私有密码输入实现，已有编辑历史 | 抽取为共享模型，避免复制 |
| `src/action.rs::Action` | CatalogEditorInsert(char)、CatalogEditorPaste(String) 可直接 Debug | 密码输入增加脱敏载荷路径 |

执行前运行 `git status --short`，确认当前工作区改动归属。先阅读相关符号及其调用者；本计划不依赖旧行号精确定位。

## 2. 已确定的产品与技术决策

### 2.1 表单外观

1. 单列、单外边框，沿用 theme.surface / accent / muted / selection / action，不增加嵌套卡片。
2. 窗口最大 106 列、34 行，终端四周尽量各留一格；高度按内容计算。
3. 标签参考新增连接使用 22 列，值区最大 68 列；窄屏按实际宽度收缩。
4. 每字段一行；仅活动输入值区域使用选中背景，活动标签使用强调色。
5. 分组标题使用大写，字段标签自然大小写；必填项加 `*`，占位符使用 muted。
6. 开关显示 `[x] On` / `[ ] Off`。Owner 有可用 picker 时显示 `›`。
7. Template、Encoding、Locale provider 本轮保持文本输入，不呈现未实现的选择箭头。
8. Password 未修改时显示 Not set（新增）/ Unchanged（编辑），有新密码时失焦显示 Set，聚焦显示掩码及光标。
9. 顶部显示 `<driver> · <connection name>`；名称来自 editor anchor，不能使用当前活动连接替代。
10. 主按钮 Review SQL 使用填充强调样式，Cancel 使用弱化样式，沿用现有 SQL 预览与执行流程。

### 2.2 字段顺序

数据库字段与 Tab 顺序：

| 分组 | 顺序 |
| --- | --- |
| GENERAL | Name * → Owner * → Comment |
| LOCALE & ENCODING | Template * → Encoding * → Locale provider → Locale → Collation → Ctype |
| OPTIONS | Tablespace → Connection limit → Allow connections → Is template |
| 操作区 | Review SQL → Cancel |

User / Role 共用 RoleDraft；保留 `RoleDraft::new(login)` 区分初始 Login：

| 分组 | 顺序 |
| --- | --- |
| GENERAL | Name * → Comment |
| AUTHENTICATION | Login → Password → Valid until → Connection limit |
| PRIVILEGES | Superuser → Create database → Create role → Inherit → Replication → Bypass RLS |
| MEMBERSHIP | Member of |
| 操作区 | Review SQL → Cancel |

数据库编辑态沿用当前创建选项限制，以字段语义而非数字范围表达：Template、Encoding、Locale provider、Locale、Collation、Ctype、Tablespace 显示 Read only，跳过 Tab 且不注册可编辑命中区域。本轮不扩展数据库修改能力。

### 2.3 交互契约

| 操作 | 行为 |
| --- | --- |
| Tab / Shift-Tab、Down / Up | 按上述顺序移动；跳过不可编辑字段；动作按钮可到达 |
| 普通文本输入、粘贴、删除、左右/Home/End、撤销重做 | 修改当前字段并更新光标 |
| Space / Enter，焦点为 toggle | 切换且不进入预览 |
| Enter，焦点为 Owner 且候选可用 | 打开 picker；列表中的 Enter 确认选择 |
| Enter，焦点为普通文本或 Review SQL | 验证并进入预览 |
| Enter，焦点为 Cancel | 走现有取消/未保存更改流程 |
| Esc，picker 打开 | 只关闭 picker |
| Esc，表单 | 走现有取消流程 |
| 单击普通字段 | 聚焦；值区点击定位光标 |
| 单击 toggle | 聚焦并切换一次 |
| 验证失败 | 保留输入，定位首个错误字段，在固定反馈区显示错误 |
| Planning / Applying | 遵循已有 busy-state 输入阻断，鼠标也不可绕过 |

普通文本保持现有文本选择语义。密码仅使用脱敏编辑路径，不注册能够复制明文的普通文本选择目标。

### 2.4 密码语义

- 新建未输入密码：规划载荷为 None。
- 编辑未输入密码：规划载荷为 None，表示保留服务器密码。
- 输入非空密码：规划时转换为现有 RedactedSecret。
- 将新输入删除为空：回到 None，撤销本次密码修改；本轮不增加删除服务器密码的独立操作。
- 关闭再打开表单不残留上一次密码；Debug、Action、UI、SQL 预览均不能包含明文。
- Role 关闭 Login 时保留已输入内容，不隐式擦除密码或重置其他角色属性。

## 3. 实施任务

### Task 1：锁定行为契约与测试夹具

**Files**
- Modify: `tests/catalog_editor_state.rs`
- Modify: `tests/catalog_editor_reducer.rs`
- Modify: `tests/keymap.rs`
- Modify: `tests/ui_render.rs`

**Steps**
1. 阅读现有 `role_editor_uses_catalog_editor_field_keymap`、`role_editor_renders_secret_as_status_only`、Database 创建/编辑夹具及 Owner picker 测试。
2. 复用已有 App 与 CatalogEditorState 构造方式，补齐 Database create/edit、LoginRole create、Role create/edit 测试入口；不依赖在线数据库。
3. 写入最先可观察的回归测试：数据库 Tab 后光标/高亮随 Owner 移动；Role toggle 的 Space 改变对应值；Database 粘贴进入 Name。
4. 运行对应精确测试，确认失败来自现有行为缺失，而不是夹具或连接状态错误。

后续任务各自补充测试后实现，避免一次引入大量无法定位的失败测试。

**Verify**
```bash
cargo test --test keymap role_editor_uses_catalog_editor_field_keymap
cargo test --test ui_render role_editor_renders_secret_as_status_only
```
预期：旧有基线通过；新增契约用例在实现前暴露对应缺口。若基线已失败，记录并先区分是否与本任务有关。

### Task 2：Database / Role 迁移至类型化焦点

**Files**
- Modify: `src/model/catalog_editor.rs`
- Modify: `src/app.rs`（旧数字焦点处理及 draft→request 转换调用点）
- Modify: `tests/catalog_editor_state.rs`
- Modify: `tests/catalog_editor_reducer.rs`
- Modify: `tests/catalog_mutation.rs`（受 draft API 调整影响的构造点）

**Steps**
1. 在 `CatalogFormFocus` 增加 Template、Encoding、LocaleProvider、Locale、Collation、Ctype、ConnectionLimit、AllowConnections、IsTemplate、Login、Password、ValidUntil、Superuser、CreateDb、CreateRole、Inherit、Replication、BypassRls、Memberships。
2. 将两个 Draft 的 `selected_field: usize` 替换为 `focus: CatalogFormFocus`，初始为 Name。
3. 定义 `DATABASE_FOCUS_ORDER` / `ROLE_FOCUS_ORDER`，严格按第 2.2 节顺序包含 Review 和 Cancel。
4. 实现每个 Draft 的 `focus_enabled`、`focus`、`move_field`、`validation_focus`；复用 `move_catalog_form_focus`。
5. 实现按字段访问的 `Option<&TextInput>` / `Option<&mut TextInput>`，toggle 和动作一律返回 None，防止误编辑 Comment 等后备字段。
6. 将可用性检查集中在字段访问和 focus 层，替换 `2..=8` 等数字判断。
7. 更新 `CatalogDraft` 的 focus_kind、focus_accepts_text、focus_is_toggle、focused_action、validation_focus 分发。动作识别只对 Review/Cancel 返回动作，避免旧 fallback 将所有字段当 Review。
8. 更新旧测试与 App 调用点；仍使用数字焦点的 Schema 等对象维持自己的分支。

**Tests**
- 全字段正向/反向遍历及循环；同一焦点不可出现两次。
- 编辑态跳过创建专属字段；直接 Action 聚焦也不能绕过限制。
- Name/Owner/Template/Encoding/Connection limit 错误定位正确，验证规则保持现有语义。
- User / Role 初始 login 分别为 true / false。

**Verify**
```bash
cargo check
cargo test --test catalog_editor_state
cargo test --test catalog_mutation
```
预期：编译通过，状态与 SQL 计划相关测试通过。

### Task 3：抽取共享密码输入并定义 Role 密码更新

**Files**
- Create: `src/model/secret_text_input.rs`
- Modify: `src/model/mod.rs`
- Modify: `src/model/profile_manager.rs`
- Modify: `src/model/catalog_editor.rs`
- Modify: `src/app.rs`（Role mutation 载荷生成）
- Modify: `tests/profile_draft.rs`
- Modify: `tests/catalog_editor_state.rs`
- Modify: `tests/catalog_mutation.rs`

**Steps**
1. 将 SecretTextInput、SecretTextHistory、SecretTextSnapshot、SecretEditGroup 及其专用辅助函数从 profile_manager 迁移到共享模块；公共范围限制为项目内部所需范围。
2. ProfileDraft 改为导入共享实现；保留 Profile 密码/URL 输入行为与历史分组规则。
3. 为共享 SecretTextInput 实现脱敏 Debug 和所需 PartialEq/Eq；相等比较只比较当前值和光标，不把编辑历史变成 draft 的业务差异。
4. RoleDraft 使用单一密码编辑数据源，保留 `set_password` 作为便捷入口，新增只供规划层使用的密码更新转换方法；避免 password payload 和 input 各存一份却不同步。
5. 输入空串返回 None，非空转换为现有 RedactedSecret；更新 App/规划测试中的直接字段访问。
6. 接入 password 的插入、粘贴、删除、移动、撤销重做和切换字段时结束历史组。

**Tests**
- 多字节密码编辑后光标位置正确，undo/redo 恢复值但 Debug 始终脱敏。
- None / 非空 / 删空三种状态生成正确的 mutation 载荷。
- Profile 原有密码、URL 编辑测试仍通过。
- 保留并扩展 Role SQL 预览脱敏测试，不通过断言错误消息输出明文密码。

**Verify**
```bash
cargo test --test profile_draft
cargo test --test catalog_editor_state
cargo test --test catalog_mutation
```
预期：共享输入抽取无 Profile 回归，Role 密码更新语义明确。

### Task 4：补齐键盘、粘贴、Action 与鼠标输入链路

**Files**
- Modify: `src/action.rs`
- Modify: `src/input/keymap.rs`
- Modify: `src/input/mouse.rs`
- Modify: `src/app.rs`
- Modify: `src/model/catalog_editor.rs`
- Modify: `src/ui/mod.rs`
- Modify: `src/ui/text_selection.rs`（仅实际需要的目标分发）
- Modify: `tests/keymap.rs`
- Modify: `tests/catalog_editor_reducer.rs`

**Steps**
1. 为 Database / Role 在 CatalogDraft 的 paste、move_left/right/home/end、delete_previous_word、delete_to_start、undo/redo 及编辑分组方法补齐分发。
2. 在 `input_for_target` / `input_for_target_mut` 通过现有 `CatalogEditorCursorTarget::FormField` 访问普通文本字段；不新建一套数字目标。
3. 密码字符与粘贴增加脱敏载荷 Action，复用现有 ProfileInput 封装能力或将该载荷封装随共享密码模块迁移；keymap 在密码焦点先分流，不能先生成普通 CatalogEditorPaste(String)。
4. 更新 App reducer，处理 typed focus、两个 Draft 的 toggle 和秘密输入 Action；busy 时全部拒绝修改。
5. 按第 2.3 节为 Database / Role 定义 Enter 优先级：toggle → Owner picker → action → 文本预览。不要借此修改其他对象现有 Enter 行为。
6. 复用 FormField 鼠标命中链路；开关单击通过一次 Action 激活，避免 focus 与 activate 各触发一次 toggle。
7. 普通文本选区和光标使用相同投影；密码点击只聚焦或按掩码映射光标，不建立明文复制目标。

**Tests**
- 每个 toggle 的 Space/Enter 修改且只修改自身；普通文本空格仍可输入。
- 普通文本粘贴、光标移动、删除、undo/redo 正确。
- 密码字符/粘贴 Action 的 Debug 不泄漏内容。
- Review/Cancel 与普通文本 Enter 行为正确，busy 键鼠输入不修改 draft。
- 编辑态不可编辑字段通过 reducer 直接发送输入仍不可修改。

**Verify**
```bash
cargo test --test keymap
cargo test --test catalog_editor_reducer
```
预期：以上链路通过，其他 Catalog 对象快捷键无回归。

### Task 5：Database Owner picker 与多连接上下文

**Files**
- Modify: `src/model/catalog_editor.rs`
- Modify: `src/app.rs`（catalog_owner_choices、Owner 请求、picker action 分支）
- Modify: `src/ui/catalog_editor.rs`
- Modify: `tests/catalog_editor_reducer.rs`
- Modify: `tests/keymap.rs`

**Steps**
1. 在 CatalogDraft::owner / owner_mut、CatalogEditorState::owner_field_focused 中加入 Database。
2. 检查并补齐 Profile anchor 的 Owner 请求路径；请求使用 editor 所属 profile/connection identity，不能盲目读取活动连接的 owner_context。
3. 复用已有 Owner picker 搜索、选择、关闭逻辑，沿用其请求 ID/generation/epoch 检查。
4. 加载失败或候选不可用时保留手工输入 Owner，反馈区呈现状态；没有列表能力时不显示箭头。
5. 接受候选仅更新 Owner，保持其余字段与焦点视口；Esc 先关闭列表。

**Tests**
- Database 创建态 Owner 打开、搜索、确认和 Esc 关闭。
- 候选空/加载失败可以手工输入。
- A/B 两连接名称及角色列表不同：A 表单不显示或接受 B 的候选。
- 过期响应不覆盖新表单状态。

**Verify**
```bash
cargo test --test catalog_editor_reducer owner
cargo test --test keymap owner
```
预期：新增 Database 与已有 Schema/View/Sequence 的 Owner 场景均通过。确认过滤命令实际执行了相关测试。

### Task 6：建立紧凑行式布局与固定底部

**Files**
- Modify: `src/ui/catalog_editor.rs`
- Modify: `tests/ui_render.rs`

**Steps**
1. 为 Database / Role Form 增加私有布局类型，包含 header、body、feedback、actions、hint；字段区与固定区不可重叠。
2. 用 Section / Field 的行模型生成正文，字段顺序直接来自模型顺序，动作从正文行列表排除。
3. 期望高度按边框 2 + header 1 + 正文字段/分组行 + feedback 2 + actions 1 + hint 1 计算；有空间再加入少量分组空行，总高不超过 34。
4. 实际面板在终端留边后居中；矩形运算使用饱和减法，并限制在父 Rect 内。
5. 空间不足依次去掉空行、去掉分组标题，然后启用字段视口；不可把字段静默丢弃。
6. 焦点为正文时按该字段索引调整 viewport；焦点为动作时保留最近正文视口，必要时增加仅 UI 使用的 scroll 状态。
7. footer 自底向上分配，长错误最多占 feedback 两行，不能替换快捷键或推动按钮。
8. 使用当前 anchor 查连接显示名称和驱动，名称清理控制字符并按终端显示宽度截断；查不到使用短 ID 回退。

**Tests**
- 160×40 居中、最大 106×34、内容少时收缩高度。
- 120×36 正常分组可见。
- 80×24 去掉装饰后全部字段可通过 Tab 到达，按钮和 hint 可见。
- 40×10 使用应用已有 TERMINAL TOO SMALL 回退。
- 终端 resize 后当前字段与光标仍在可见区域。
- 错误前后 actions 的 Rect 相同。

**Verify**
```bash
cargo test --test ui_render catalog
```
预期：新增布局用例通过；确保测试名包含 catalog，让此过滤包含新增测试。

### Task 7：重写 Database 字段渲染

**Files**
- Modify: `src/ui/catalog_editor.rs`
- Modify: `tests/ui_render.rs`

**Steps**
1. 将 render_database 参数接入 ui、editor/app 所需上下文，删除多字段 Line::raw 拼接。
2. 使用 render_catalog_text_field、render_catalog_toggle_field、section heading 与 Owner picker 组件逐行渲染。
3. 新表单标签宽 22、值区最大 68；通过局部参数/helper 实现，避免直接改变其他 Catalog 表单的字段几何。
4. 活动字段展示真实光标和 value 高亮，注册 FormField 的 cursor / selection hit map。
5. 添加弱化占位符：Comment 为 Optional，Tablespace 为 Default；实际值仍为空字符串。
6. 编辑态创建选项标记 Read only，使用 muted，不注册输入命中区域。
7. 焦点帮助为 Connection limit 显示 `-1 means unlimited connections`；Locale 等字段只在焦点时提供简短说明。
8. 主按钮及 Cancel 接入 typed action 焦点和鼠标；hint 由真实 focus kind 派生。

**Tests**
- 每个字段独占一行，Name/Owner 不再拼接。
- 只有当前 value 区高亮，光标在值区中。
- 点击标签聚焦，点击值区中部定位，多字节文本水平滚动正确。
- 只读字段无编辑命中目标，占位符不进入输入模型。
- 头部使用 anchor 连接名，长名称和控制字符不破坏布局。

**Verify**
```bash
cargo test --test ui_render catalog_database
```
预期：为本任务用例采用 catalog_database 前缀，所有用例通过。

### Task 8：重写 User / Role 字段渲染

**Files**
- Modify: `src/ui/catalog_editor.rs`
- Modify: `tests/ui_render.rs`
- Modify: `tests/catalog_editor_reducer.rs`

**Steps**
1. 替换 render_role 的摘要 Paragraph，按四组逐字段渲染，User/Role 共用同一渲染实现。
2. Login 和权限字段全部复用 toggle 控件；Login false 时不隐藏其他字段或改变 Tab 顺序。
3. Member of 接入 draft.memberships 文本输入，焦点帮助解释使用现有逗号分隔角色名语法；沿用现有规划解析语义。
4. Password 使用专门掩码渲染：焦点状态按字符数产生掩码并计算光标，失焦只显示状态，不调用普通明文 Paragraph/helper。
5. 创建/编辑的未输入状态按 mode 区分 Not set / Unchanged；删空恢复该状态。
6. 表单标题继续遵循现有 object type 显示约定，保证 User 与 Role 可辨识。
7. 设置 Valid until / Connection limit 的上下文帮助，底部复用 Task 6/7 实现。

**Tests**
- User/Role 分组及初始 Login 展示正确。
- 所有权限开关可聚焦与单击操作。
- Password 输入前、输入中、失焦、undo 后、进入 SQL 预览均无明文。
- Member of 编辑后进入 mutation 计划；返回表单保留输入。
- 掩码光标不越界，密码没有普通文本选择复制目标。

**Verify**
```bash
cargo test --test ui_render role_editor
cargo test --test ui_render catalog_role
cargo test --test catalog_editor_reducer role
```
预期：已有脱敏测试和新样式/交互用例全部通过。

### Task 9：验证反馈与预览闭环

**Files**
- Modify: `src/app.rs`
- Modify: `src/model/catalog_editor.rs`
- Modify: `src/ui/catalog_editor.rs`
- Modify: `tests/catalog_editor_reducer.rs`
- Modify: `tests/catalog_mutation.rs`
- Modify: `tests/ui_render.rs`

**Steps**
1. Preview 前调用现有 validate；失败时通过 validation_focus 定位字段，保留所有输入。
2. 固定反馈区按 error → operation → contextual help 的优先级显示，错误文本执行 terminal sanitize。
3. 修改相关字段后清理已失效错误，避免无关字段编辑造成业务值重置。
4. 校验 Review SQL 生成的 Database / Role 请求与改造前同语义；Password 仅通过秘密载荷进入执行层。
5. SQL Preview 返回 Form 时保留 draft、焦点和合理的视口；取消仍走已有未保存变更逻辑。
6. Planning/Apply 失败回到可编辑状态，错误不覆盖 actions/hint；重试使用最新输入。

**Tests**
- Name/Owner 缺失定位正确，非法 connection limit 定位正确。
- 预览后返回，所有字段/权限/成员关系保留。
- 创建与编辑 SQL quoting、密码 redaction、成员关系载荷正确。
- 错误和 busy 改变时按钮位置稳定，过期响应被忽略。

**Verify**
```bash
cargo test --test catalog_editor_reducer
cargo test --test catalog_mutation
cargo test --test ui_render catalog
```
预期：从字段输入到计划、返回、重试的完整链路通过。

### Task 10：综合回归与终端验收

**Files**
- Review: 上述所有修改文件
- Update: 本计划的执行记录

**Steps**
1. 运行格式和编译检查。
2. 一次性运行受影响测试集；若前一步已对最终代码运行相同完整集合，无需重复。
3. 在真实终端比较 New Connection / New Database / New User / New Role，分别检查 120×36、80×24、160×40。
4. 使用 ASCII 图标和无颜色模式检查：开关、焦点、按钮主次仍可识别；中文连接名和长字段不会错位。
5. 有 PostgreSQL 测试环境时，在专用测试连接验证 create/edit→review→apply→catalog refresh；没有环境则记录未进行在线验证，不以渲染测试替代执行成功声明。
6. 审查 diff，确认所有新增可操作外观都有 Action 支撑、所有字段可到达，更新执行记录与验证结果。

**Verify**
```bash
cargo fmt --check
cargo check
cargo test --test catalog_editor_state --test catalog_editor_reducer --test catalog_mutation --test keymap --test ui_render --test profile_draft
git diff --check
```
预期：上述检查通过。若项目 CI 另有必需检查，按仓库当时配置执行。环境失败需记录准确命令与原因。

## 4. 依赖与交付节奏

按以下顺序执行，避免 UI 看似完成而输入尚不可用：

```text
Task 1 契约
  → Task 2 类型化焦点
  → Task 3 共享秘密输入
  → Task 4 键鼠输入闭环
  → Task 5 Owner 上下文
  → Task 6 布局基础
  → Task 7 Database
  → Task 8 User / Role
  → Task 9 反馈与 SQL 预览
  → Task 10 验收
```

建议三个可检查的里程碑：

1. **M1：状态与输入完整**（Tasks 1–5）：字段、开关、密码、成员关系与 Owner 能通过测试操作。
2. **M2：视觉统一**（Tasks 6–8）：三个页面使用同一视觉规则，大/小终端可用。
3. **M3：端到端完成**（Tasks 9–10）：验证、预览、返回、错误与回归通过。

需要 Git 提交时按可独立通过检查的逻辑单元提交：`refactor(catalog): type database and role form focus`、`refactor(model): share secret text input`、`feat(catalog): complete database and role form interactions`、`feat(ui): align database and role forms with connection design`。每次只暂存本任务文件；提交前执行相应检查。

## 5. 最终验收清单

- [ ] Database / User / Role 一字段一行，分组、间距、颜色与新增连接一致。
- [ ] 弹窗按内容收缩，大终端居中，小终端焦点可见。
- [ ] 显示编辑目标连接名，多连接场景不混用名称或 Owner 候选。
- [ ] Tab 顺序与视觉顺序一致，Review SQL / Cancel 可通过键鼠到达。
- [ ] 文本的插入、粘贴、删除、光标、撤销重做均工作。
- [ ] Database / Role 每个 toggle 均可通过 Space、Enter、单击切换一次。
- [ ] Password 与 Member of 是实际可编辑字段。
- [ ] 密码的 UI、Debug、Action、SQL Preview 保持脱敏，删空回到不修改语义。
- [ ] Database 编辑态创建选项可见但不可修改。
- [ ] 固定反馈区域不会挤掉按钮/快捷键，验证失败定位到错误字段。
- [ ] 预览返回保留输入，失败可重试，busy 与过期响应保护继续生效。
- [ ] 所有受影响测试和格式/编译检查通过；人工与在线验证如实记录。

## 6. 执行记录

- 当前状态：计划已完成，功能实现尚未开始。
- 自动化验证：本次仅编写计划，未运行产品测试。
- 人工终端/在线 PostgreSQL 验证：待实现后进行。
