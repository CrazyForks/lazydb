# LazyDB / Kitty 智能 Pane 导航 Implementation Plan

> **执行者：Luna。** 按下列端到端验收单元连续实施、验证、审查和收尾；Astra 只负责本计划。用户指定的阶段与目录限制优先，不创建 docs/plans 副本，不启动子 Agent。

**Goal:** Cmd+Ctrl+h/j/k/l 优先切换 LazyDB 内部可见相邻 pane，在内部边界时切换对应 Kitty pane。

**Architecture:** Kitty 根据 LazyDB 生命周期 user variable 放行按键。Keymap 产生独立智能导航意图，runtime 使用当前 App 和终端尺寸计算真实可见布局，内部移动复用现有 Action，外部移动交给有界异步 Kitty 适配器。旧 Ctrl-w 及 Redis resize 语义不被智能命令借用。

**Tech Stack:** Rust 1.94 / Tokio / Crossterm 0.29 / Ratatui 0.30；Kitty 0.47.4；OSC 1337；TOML 配置。

---

## 基线及交付边界

- 原工作区 `/Users/yelog/workspace/tui/lazydb`；目标 main；起点和 plan 阶段 HEAD 均为 `7b76fe9286fd99199ce6fc99a9e013264640b885`。
- 本阶段 `git status --short` 为空。无未提交行为依赖，无需复制本地业务文件。后续新 worktree 不会自动包含用户新修改，Luna 开始前重新核对 diff。
- `checkpoint.json` 仍不存在；不得编造 checkpoint 或改写插件 state。旧 analyze 回执不得拿来当 plan 回执。
- 工作流与任务分支名称由 Luna 决定；本计划不创建工作树、分支或提交。
- 本次实现焦点导航四方向；跨层 resize、三层 Neovim terminal 内嵌 LazyDB 路由不在交付承诺内。需要保持既有 Neovim 和 resize 配置工作。
- 分析依据见同目录 `analysis.md`。以下命令均为后续实施指令，除 validation.md 明确记录者以外均未执行。

### 需求、门禁和补充验证的分级

| 类别 | 内容 | 完成判定 |
| --- | --- | --- |
| 用户核心需求 | Cmd+Ctrl+l 有内部右邻居则内部移动，否则移动 Kitty 右邻居 | 代码路径、配置方案及自动化行为测试证明内外分流；不能只加绑定 |
| 本计划实现决策 | 同一机制覆盖 h/j/k/l、可见布局、overlay 输入隔离、失败降级、旧行为兼容 | 对应定向测试通过，Luna 代码审查确认；四方向是统一实现选择，不称为用户额外指定的门禁 |
| 项目已有门禁 | `.github/workflows/ci.yml` 的 Rust fmt/clippy/tests 及 macOS binary dependency 检查 | 按下文命令执行并记录真实结果；环境缺失单独记录，不能用旧结果代替 |
| 补充建议验证 | 真实 Kitty/GUI/PTY、Neovim 人工回归、cmd+d overlay 演练、人工 panic 场景 | 有环境则补证据；无环境最多一次定向修复重试后记录限制，由 Luna 收尾审查判断，不自动设为必须人工通过的发布门禁 |

补充演练的缺失不等于功能失败，也不等于功能已实机通过。自动化测试和源码复核仍须覆盖源窗口定位、overlay 组语义、生命周期清理；发现实际缺陷须修复，不能以“补充检查”豁免。

## 固定设计契约

### 命令和意图

新增四个可配置命令，默认空绑定（opt-in）：

```toml
[keybindings.panes]
smart-focus-pane-left = []
smart-focus-pane-down = []
smart-focus-pane-up = []
smart-focus-pane-right = []
```

智能命令和旧 `PaneCommand` 分开枚举/dispatch；配置允许注册新命令，keymap 在已有 overlay 路由之后、普通文本/编辑模式处理之前识别单次修饰键。复用现有 configured sequence 机制时确保新命令能在注册、序列匹配、help 中完整穿透，不引入第二套字符串 parser。

推荐在 `src/model/pane_navigation.rs` 定义与终端无关的类型：

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneDirection { Left, Down, Up, Right }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneTarget { Explorer, Editor, Results, RedisKeys, RedisPreview }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneNavigation {
    Internal(PaneTarget),
    Boundary(PaneDirection),
    Blocked,
}
```

`Action::SmartFocusPane(PaneDirection)` 表示意图，不能在 Keymap 内执行外部进程。runtime 在持有 terminal 尺寸的位置解析意图，Internal 转成现有 `Focus` / `RedisFocusPane` 再调用 apply_action。App reducer 对该 runtime-owned 意图给出显式无副作用分支，避免纯 model 测试触发 I/O。帮助入口如能执行该意图，必须进入同一个解析入口；不能只在 Event::Key 私有分支实现而让 help 行为失效。可给 runtime 统一 action handler 注入当前尺寸，而不是在每个调用者各写一次匹配。

### 可见相邻 pane

- `src/ui/layout.rs` 提供统一 workspace layout 计算结果，渲染和导航共同使用；从 `src/ui/mod.rs:1034–1054` 提取真实 tab-kind / empty workspace 条件，不另写一份不同逻辑。
- 每个目标是 `(PaneTarget, Rect)`；只收集实际渲染且宽高非零的可聚焦区域。不可把 footer、tabs、空白 workspace 当 pane。
- 候选必须在指定方向、与源矩形在正交轴上严格重叠；按方向间距、正交轴起点、稳定 pane 顺序选最近者。SQL Explorer 右侧等距离时上方 Editor 优先；这避免以中心点误选下方 Results。
- 找不到候选才 Boundary；app overlay 存在则 Blocked；源 pane 不可见/TooSmall 则 Blocked，防止无有效源的误移动。
- 窄屏和最大化只看实际矩形；Redis 子 pane 如仍可见则继续参与。
- SQL Results/Editor 右边界、Redis Preview 右边界、只有 Explorer 的空工作区右边界均转外部。
- Press 执行；Release/Repeat 消费但不执行，首版不承诺长按连跳。修饰键不能流入文本造成插入。

### Kitty 集成

- Kitty 上下文：有效 `KITTY_WINDOW_ID`；桥接额外需要非空 `KITTY_LISTEN_ON`。使用运行时环境，不能硬编码配置中的 socket 名。
- TUI 成功初始化后设置标记，失败恢复、Drop、panic 清除自己设置的标记，并 flush：

```text
set:   ESC ]1337;SetUserVar=IS_LAZYDB=MQ== BEL
clear: ESC ]1337;SetUserVar=IS_LAZYDB BEL
```

- 无 TUI 的 CLI/MCP 路径、非 Kitty 环境不发标记；退出后 shell 不应继续匹配 LazyDB。清理幂等，SIGKILL 不属于可保证清理场景。
- 使用 `tokio::process::Command` 参数数组：`kitten @ --to <socket> action --match id:<window-id> "neighboring_window right"`。不依赖用户目录 `neighboring_window.py`。
- stdout/stderr 捕获；stdin 关闭；timeout 初值 1 秒；子进程 kill-on-drop；单个窗口最多一个在途导航，无无限队列，退出不留下子进程。环境缺失为 no-op；执行失败用 tracing 记录，必要通知去重。
- 在途请求前尽可能约束源窗口仍是焦点，避免延迟请求作用于其他窗口。通过本机 Kitty 对应版本源码/契约复核及自动化测试核对 `action` 是否按 --match 的源组导航，尤其 `toggle_lazydb.py` 的 Kitty overlay；真实 GUI 是补充证据。若原生 action 无法满足，新增 `scripts/kitty/lazydb_neighbor.py` 作为 source-aware kitten、`scripts/kitty/test_lazydb_neighbor.py` 测试，并补安装文档；不依赖用户私有脚本。该条件分支文件已纳入 change-scope，无需无故创建。

## 单元 1：右方向从按键到内外焦点的完整闭环

**文件：**
- 新增 `src/model/pane_navigation.rs`，注册到 `src/model/mod.rs`。
- 修改 `src/ui/layout.rs`、`src/ui/mod.rs`：共用实际布局与导航目标。
- 修改 `src/input/panes.rs`、`src/input/keymap.rs`、`src/config.rs`、`src/action.rs`、`src/app.rs`：智能命令注册与意图。
- 新增 `src/terminal/kitty.rs`，由 `src/terminal.rs` 注册；修改 `src/runtime.rs` 接入。
- 测试优先放对应模块 `#[cfg(test)]`，公共键盘链路补 `tests/keymap.rs`。
- 按需修改 `src/help.rs`，使新命令的展示/执行遵循统一路由；仅在实际存在硬编码注册需求时修改。

### 步骤

1. 核对基线和当前 diff，记录实际工作树/HEAD；由 Luna 按工作流创建并命名任务分支。
2. 写右方向契约测试：SQL Explorer → Editor；Editor → Boundary(Right)；空工作区 → Boundary(Right)；overlay → Blocked。数据使用已有 App/WorkspaceTab 测试构造方法。
3. 运行 `cargo +1.94.0 test --lib smart_pane`，确认新测试在缺少实现时失败；后续所有新核心测试名统一加 `smart_pane_`，避免过滤器零匹配。
4. 提取统一布局 helper，实现可见矩形与右方向 resolver；运行同一过滤测试，检查执行数量大于零且通过。
5. 注册右方向智能命令和 Action；给 `Cmd+Ctrl+l` 配置解析与 Keymap 写测试，覆盖 Explorer、Editor Normal/Insert/Replace、Release/Repeat、overlay、普通 `l`。默认空绑定不得改变普通输入。
6. 写 Kitty OSC set/clear 字节及生命周期幂等测试；用可注入 writer 验证，不污染测试进程 stdout。实现 TUI enter/restore/Drop hook。
7. 写适配器请求构造与 fake runner 测试：目标 window/socket、单次请求、超时、失败、环境缺失。实现有界异步 runner。
8. runtime 接通 SmartFocusPane：解析当前布局 → 现有内部 Action 或 Kitty 请求。使用当前终端尺寸，不能仅复用上一帧 `UiState` 缓存。为内部路径断言远控调用数 0、边界路径 1、Blocked 路径 0。
9. 用测试捕获执行链路验证右方向闭环，检查 `Action::ResizePane` 从未由智能导航产生。
10. 单元定向测试通过后复核：默认空绑定、内部移动零远控、边界单次远控、modal 零远控、当前尺寸来源、子进程退出清理。记录结果与 diff；Luna 可创建一个完整闭环提交，不能只交付枚举/parser 后停止。

**验收：**同一智能右移意图在有邻居时仅内部移动、在边界时仅发送一次准确定位的 Kitty 请求，退出标记可清理；无 Kitty 也不崩溃。

## 单元 2：四方向、真实布局、旧行为回归

**文件：**同单元 1 的 model/layout/input/runtime 模块；`tests/keymap.rs`，必要时新增 `tests/smart_pane_navigation.rs` 检查公共 App/action 行为。

### 步骤

1. 扩展左/上/下智能命令的配置注册和绑定测试，复用方向 enum 与 resolver，不复制三套分支。
2. 将相邻算法测试参数化，覆盖左右/上下反向移动、无邻居、零尺寸、正交轴不重叠、等距稳定排序。
3. SQL：Explorer↔Editor/Results，Editor↓Results、Results↑Editor，外侧方向 Boundary；矩形用非零 origin 再跑一组，防止隐含原点假设。
4. Relation、Dashboard、Principal：无 Editor；向右进入正确内容，向上不得把不存在的 Editor 当目标。
5. Redis：Explorer→Keys→Preview 及反向，查找/预览编辑模式正确归属，Preview 右移 Boundary；断言不修改 `PaneSizePreferences`。
6. 空工作区、窄屏、最大化、TooSmall、终端 resize、切 tab 后立即导航；验证当前布局而非旧帧决定目标。
7. 保留旧 Ctrl-w 注册和 dispatch；给原有 Redis Preview resize 特例写有意义的兼容断言，避免借本任务悄悄修改旧行为。
8. 检查配置序列/help 入口能执行新命令。对于配置不支持的形式应明确校验失败，不接受配置后静默无效。
9. 运行 `cargo +1.94.0 test --lib smart_pane`、`cargo +1.94.0 test --test keymap`；有新增集成文件则运行 `cargo +1.94.0 test --test smart_pane_navigation`。记录每个实际命令、数量、退出码和代码版本。
10. 复核新增条件没有绕过文本/overlay 所有权，旧 Ctrl-w/Redis resize 分支没有意外变化，渲染与导航共享同一组 tab-kind/可见性判断。上述测试预期退出 0、实际执行测试数大于 0、失败数为 0。

**验收：**四方向在全部可见布局下一致，旧键盘行为通过回归，智能导航不混入 resize，快速切 tab/resize 不使用过期目标。

## 单元 3：用户可启用、实机集成与收尾

**文件：**
- 修改 `config/default.toml`：新增默认空绑定及简短说明。
- 修改 `docs/keybindings.md`、`docs/configuration.md`：解释 opt-in 和链接。
- 新增 `docs/kitty-integration.md`：完整配置、原理、限制、排错。
- 仓库外配置参考：`/Users/yelog/.config/kitty/kitty.conf`、`/Users/yelog/lazydb/settings.toml`。本仓库任务交付文档中的完整部署增量，不把这两个外部文件列作预计修改；它们不能以 `../` 伪装仓库路径写进 change-scope。后续工作流若明确授权部署本机配置，再单独记录实际部署范围，不混入 Git 提交。Neovim 配置只读参考。

### 完整启用片段

Kitty 保留现有默认移动及 IS_NVIM 规则，增加：

```conf
map --when-focus-on var:IS_LAZYDB kitty_mod+h
map --when-focus-on var:IS_LAZYDB kitty_mod+j
map --when-focus-on var:IS_LAZYDB kitty_mod+k
map --when-focus-on var:IS_LAZYDB kitty_mod+l
```

本机 `kitty_mod` 已是 `cmd+ctrl`。文档对其他用户注明此前提以及 `allow_remote_control yes` / `listen_on` 必须形成可用环境；不覆盖用户原有 socket 配置。

LazyDB 在现有 settings 中合并：

```toml
[keybindings.panes]
smart-focus-pane-left = ["Cmd+Ctrl+h"]
smart-focus-pane-down = ["Cmd+Ctrl+j"]
smart-focus-pane-up = ["Cmd+Ctrl+k"]
smart-focus-pane-right = ["Cmd+Ctrl+l"]
```

### 步骤

1. 编写上述文档和配置；明确 resize 不属于新功能，旧 Ctrl-w 保留，恢复方法为删除新条件映射和智能绑定。
2. 【补充实机检查】真实 Kitty splits 打开 LazyDB 与右侧 shell：Explorer→内部右 pane→Kitty 右 pane；反方向切回，最外边界保持原地。使用当前构建的绝对路径启动，记录 binary/HEAD，避免误测旧安装版。
3. 【补充实机检查】现有 `cmd+d` 的 Kitty overlay 启动场景：正确使用源 overlay 所属 split 组；退出返回底层应用后不残留 IS_LAZYDB。若实际发现缺陷，按设计契约修正 source-aware adapter。
4. 【补充实机检查】Neovim IS_NVIM 规则、LazyDB modal、正常退出及可控 panic 清理标记；不对用户真实数据库制造 panic，采用隔离测试进程。生命周期自动化测试仍属于单元 1 验收。
5. 实机检查优先使用隔离 Kitty 配置和测试 settings，避免改用户真实配置。无法在本会话连接 Kitty 时最多一次有针对性的修复尝试，然后把未覆盖场景记录为环境限制，交收尾审查判断，不能冒充通过或一直 progress。
6. 功能齐备后执行下面项目标准检查一次。发现失败后只针对修复影响重跑；不额外机械重复 cargo check。
7. Luna 审查 diff、确认功能与文档一致、核对验证证据；完成提交/合并流程。不要要求用户重复 resume 或自行完成剩余实现。

## 项目标准验证与证据

本阶段已读取 `.github/workflows/ci.yml:51–88`，Rust job 明确使用：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-targets --all-features
cargo +1.94.0 build --locked
sh scripts/release/check-macos-dependencies.sh target/debug/lazydb
```

前三项是功能齐备后的标准 Rust 检查，后两项是 macOS binary dependency 检查。未涉及的 Windows 安装器、发行打包、真实数据库服务矩阵无需每单元本地重复，仍由 CI 按配置运行。不得声称没有数据库环境的本地全量 test 已覆盖实际 adapter 服务。

预期：每条命令退出 0；fmt 无差异，clippy 无 warning，tests 无失败，macOS 依赖脚本通过。若采用 source-aware kitten 条件分支，先运行 `python3 -m unittest discover -s scripts/kitty -p 'test_*.py'`（fake Kitty 对象、无 GUI），预期退出 0 且执行数大于 0。实际 Kitty Python API 兼容性另由对应版本源码/可用环境核实。

真实 Kitty/GUI 是针对本集成的补充证据；普通 fake/PTY 测试不等于真实 Kitty source/overlay 定位验证。若无法补齐，Luna 审查时应明确记为未覆盖而非代码已证明。

每次结果追加至本任务 `validation.md`：命令、退出结果、相关文件、HEAD/未提交 diff、环境、测试数量、失败是否修复。所有实现完成后更新简短进度即可，不反复通读历史日志，也不覆盖历史验证事实。

## 阶段完成定义

本 plan 阶段完成条件：实现路线、精确文件、接口契约、启用配置、端到端验收单元、分级验证及 `change-scope.json` 均已落盘。变更范围包含预计和条件分支的仓库文件，不含只读参考、仓库外配置及任务元数据；没有删除/重命名或未提交依赖文件。本轮最后写入 `plan-3f7419c9-5e45-4e61-a531-346d6a5c44b6.json` completed 回执，不覆盖历史回执。
