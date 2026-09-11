# Oracle Client Discovery Implementation Plan

**Goal:** 在当前 Mac 安装 Oracle Instant Client 后，用户无需设置 `DYLD_LIBRARY_PATH` 即可直接运行 LazyDB；客户端加载失败时给出准确、简短、可操作的诊断。

**Architecture:** 在 Oracle 适配器边界新增应用级客户端路径解析与初始化模块，通过 `oracle::InitParams::oracle_client_lib_dir()` 显式初始化 ODPI-C。使用进程级成功状态，串行化初始化，失败可重试；所有 Oracle 连接和探针复用此入口。

**Tech Stack:** Rust、oracle 0.6.3、Tokio spawn_blocking、现有 DatabaseError/DatabaseDiagnostic、macOS ARM64 Instant Client。

## 1. 范围与事实

- 工作目录：`/Users/yelog/workspace/tui/lazydb-relational-database-expansion`。
- 分支：`task/relational-database-expansion`。
- 当前 `src/db/oracle.rs` 直接调用 `oracle::Connection::connect()`，没有配置原生库目录。
- 客户端目前位于 `$HOME/Downloads/instantclient_23_26`；该目录不属于自动库搜索目录。
- 之前真库测试依赖命令级 `DYLD_LIBRARY_PATH`，没有给用户的终端配置持久发现机制。
- `DPI-1047` 当前被连接入口统一归类为 Network；应改为本地 Configuration 错误。
- `tests/oracle_adapter.rs` 存在遇到 DPI-1047 直接返回的逻辑，不能作为客户端安装验证。
- 本计划只修复原生客户端发现和诊断，不将目录、事务、LOB 等尚未验收的 Oracle 能力标记为完成。
- 保留当前 Cargo 默认 feature 决策，本次不调整发布产品组合。
- 不保存数据库凭据，不执行远程写 SQL，不自动提交或合并 worktree。

## 2. 路径及初始化契约

### 路径优先级

1. `LAZYDB_ORACLE_CLIENT_LIB_DIR`：非空绝对目录，允许通过符号链接指向安装目录。
2. macOS 应用约定目录：`$HOME/.local/share/lazydb/oracle/current`。
3. 未设置覆盖路径且约定目录不存在：不指定 lib dir，保留 ODPI-C 自身搜索规则。

规则：
- 显式环境变量为空、相对路径、不是目录、无法访问或缺少主库入口时，返回配置错误，不回退。
- `current` 不存在时允许系统默认加载；`current` 存在但损坏（包含悬空 symlink）时返回配置错误，避免误用其他版本。
- 不扫描 Downloads，不递归扫描 HOME，不从当前工作目录自动加载动态库。
- Unix 使用 OsString/PathBuf 保留路径，不能用 lossy UTF-8 转换再加载。
- macOS 检查 `libclntsh.dylib`；其他平台的显式目录按平台主库文件名验证。上述用户默认目录只作为本次 macOS 约定，其他平台先保留显式路径和 ODPI 默认行为。
- TNS_ADMIN 是网络配置目录，与客户端动态库目录分离；不自动覆盖现有 TNS_ADMIN。

### 初始化状态

模块维护 `未初始化 / 已成功初始化（路径来源及实际目录）`，互斥锁串行化所有应用入口的初始化；不缓存失败结果。

- 初始化在 `spawn_blocking` 中调用，避免阻塞异步执行器。
- 初始化成功后相同配置复用；不重复加载或按 profile 切换版本。
- 成功后请求不同配置：返回 `oracle_client_restart_required`，要求重启。
- 上游 `InitParams::init()` 返回 false 表示已经初始化，并不证明本次指定路径生效。若本模块没有成功记录，返回初始化来源未知的诊断，不声称加载了指定路径。
- 所有进程内 Oracle 连接/池/探针必须经过统一入口。进程启动时不主动加载 Oracle Client；仅 Oracle 连接或明确诊断请求触发。
- 不在多线程运行时修改 DYLD_LIBRARY_PATH 等进程环境变量。

## 3. Task 1：稳定客户端安装位置（约 0.5 工程日）

**目标目录**：`$HOME/.local/share/lazydb/oracle/instantclient_23_26`

**操作步骤**
1. 检查源目录、目标父目录和 `current` 是否存在。已有安装或链接不得覆盖；记录冲突并决定复用或停止。
2. 对源 `libclntsh.dylib.23.1` 执行 `file`、`lipo -info` 和 `otool -L`；核实 ARM64 和依赖。
3. 将现有安装完整复制到版本化目标目录，保留权限和相对符号链接；不删除 Downloads 原件。
4. 核对关键库校验和及链接目标：libclntsh、libclntshcore、libnnz、libociei。
5. 首次创建相对链接 `current -> instantclient_23_26`；未来版本升级在新目录验证后再切换链接。
6. 不添加 sudo、不改 `/usr/lib`、不关闭 Gatekeeper、不设置全局 DYLD_LIBRARY_PATH。

**验收**：目标库架构正确，符号链接有效；卸载挂载卷或关闭原终端后目录仍可用。

**恢复方式**：保留原目录，失败时只移除本任务新建的链接和已确认新建的副本；涉及已有文件不自动处理。

## 4. Task 2：实现纯路径解析（约 0.5 工程日）

**Files**
- 新增 `src/db/oracle_client.rs`
- 修改 `src/db/mod.rs`
- 新增 `tests/oracle_client_config.rs`

**步骤**
1. 将路径解析实现为纯函数，显式传入覆盖值、HOME 和平台信息；单元测试不调用 set_var、不触发真实客户端加载。
2. 定义最小模型：路径来源、可选显式目录、用于诊断的显示目录。避免把原生句柄或密码放进该模型。
3. 实现上述路径优先级和路径验证；对环境变量保留 `var_os` 语义。
4. 添加临时目录测试：有效覆盖、默认链接、系统回退、空值、相对路径、目录不存在、普通文件、缺失主库、悬空链接、路径含空格和非 ASCII 字符。
5. 验证显式坏路径不回退到有效默认目录。

**命令**
```sh
cargo test --test oracle_client_config
```

**验收**：每种路径选择唯一、确定、无全局环境副作用；错误不包含数据库 URL 或凭据。

## 5. Task 3：进程级初始化及统一入口（约 1 工程日）

**Files**
- 修改 `src/db/oracle_client.rs`
- 修改 `src/db/oracle.rs`
- 修改 `examples/oracle_driver_probe.rs`
- 新增 `tests/oracle_client_initialization.rs`

**步骤**
1. 在路径配置之外增加同步初始化函数，通过 `InitParams` 设置显式目录后调用 init。
2. 使用小型初始化状态机与锁，成功记录来源，失败释放状态供下一次重试；不要使用 `OnceLock<Result<...>>` 永久缓存失败。
3. 在 OracleAdapter::connect 的阻塞闭包中先初始化，再连接。原有 host/service/user 等配置验证仍先执行。
4. 独立 Rust 探针改用 LazyDB 公共适配器入口，避免直接 `oracle::Connection::connect()` 绕开初始化；其他未来 Oracle 连接池入口复用同一模块。
5. 将初始化调用封装为可替换测试函数，验证并发首次调用只初始化一次、成功复用、失败后重试、配置变化要求重启，以及上游提前初始化的情况。
6. 真实 OCI 加载测试每个场景使用独立子进程，避免全局上下文相互污染；子进程环境用 Command::env/env_remove 控制。

**命令**
```sh
cargo test --test oracle_client_initialization
cargo check --all-targets
cargo check --no-default-features --all-targets
```

**验收**：TUI Test、Save & Connect、Agent/MCP 和探针使用相同初始化逻辑；没有客户端也能启动非 Oracle 功能。

## 6. Task 4：准确错误分类与可操作提示（约 0.5–1 工程日）

**Files**
- 修改 `src/db/oracle_client.rs`, `src/db/oracle.rs`
- 必要时修改 `src/db/mod.rs` 中现有 DatabaseDiagnostic 的使用方式
- 核查 `src/runtime.rs`, `src/model/profile_manager.rs`, `src/ui/profiles.rs` 的错误展示路径
- 新增 `tests/oracle_client_errors.rs`

**错误契约**
| 场景 | category | code |
| --- | --- | --- |
| 路径为空、非法、目录缺失 | Configuration | oracle_client_path_invalid |
| 动态库加载失败，包括 DPI-1047 | Configuration | oracle_client_load_failed |
| 成功初始化后配置改变 | Configuration | oracle_client_restart_required |
| 未知的外部提前初始化 | Configuration | oracle_client_initialization_conflict |
| 未编译驱动 | Unsupported | 保持 oracle_driver_not_enabled |

**步骤**
1. 初始化错误单独映射，不能再流入当前 connect 统一 Network 分支。
2. 表单 message 保持简短，例如：`无法加载 Oracle Instant Client（DPI-1047），请检查客户端目录或设置 LAZYDB_ORACLE_CLIENT_LIB_DIR。`
3. 使用现有诊断详情保留加载器原文、路径来源及平台架构；避免把完整 dlopen 搜索清单塞进表单短消息。
4. 不将 DPI-1047 一律解释为未安装；说明还可能是架构或依赖不匹配。
5. 日志和 JSON 输出复用现有终端清洗/脱敏能力；原生加载错误信息也需清洗控制字符。
6. 保留真正的认证/网络/SQL 错误类别，不因为修复客户端加载而覆盖所有 Oracle 错误。

**命令**
```sh
cargo test --test oracle_client_errors --test ui_render
```

**验收**：截图中的长错误变成可执行提示；详细技术原因仍可诊断；数据库密码不写入源代码、日志和测试报告。

## 7. Task 5：修复真实测试可信度（约 0.5 工程日）

**Files**
- 修改 `tests/oracle_adapter.rs`
- 修改 `tests/oracle_profile.rs`
- 修改 `examples/oracle_driver_probe.rs`

**步骤**
1. 删除 `Err(error) if error.message.contains("DPI-1047") => return`。
2. 真库测试采用显式 `#[ignore = "requires configured Oracle server and native client"]`；主动执行 ignored 测试时配置缺失必须失败，不能静默 return。
3. 保留无数据库的配置测试正常运行；测试数据改为虚构主机、服务名和凭据，移除已写进 fixture 的真实连接信息。
4. 真库凭据由执行环境注入，不写入文件、命令示例或报告。通过既有 SecretString 保管。
5. 探针打印成功标志和非敏感客户端来源，不输出密码和带凭据 URL。
6. 增加三种独立进程加载验证：稳定默认目录成功、显式覆盖成功、坏覆盖路径稳定失败。清除 DYLD_LIBRARY_PATH/DYLD_FALLBACK_LIBRARY_PATH 后执行。

**命令（从预先配置好真库凭据的执行环境运行）**
```sh
env -u DYLD_LIBRARY_PATH -u DYLD_FALLBACK_LIBRARY_PATH -u LAZYDB_ORACLE_CLIENT_LIB_DIR cargo test --features driver-oracle --test oracle_adapter -- --ignored --nocapture --test-threads=1
```

**验收**：真实测试的 `passed` 必须代表执行过连接和 SQL；客户端缺失不得显示成功。仅使用只读查询验证本任务。

## 8. Task 6：文档、诊断入口和 UI 验收（约 0.5 工程日）

**Files**
- 修改 `docs/configuration.md`
- 修改 `docs/database-capabilities.md`
- 修改 `docs/plans/2026-09-11-native-driver-validation.md`
- 修改 `README.md`
- 核查 `src/cli.rs` 的 doctor 兼容性

**步骤**
1. 文档明确编译 feature、客户端安装、客户端加载、远端连接是四个不同层次。
2. 记录稳定目录、环境变量优先级、初始化后修改配置需要重启、客户端架构匹配和恢复方式。
3. 修正历史验证记录中的“仍缺客户端”状态，记录当前已有客户端但此前只临时设置了路径。
4. doctor 如扩展 Oracle 检查，区分 `compiled`、`path discovered`、`load verified`，不得把文件存在当作加载成功；不主动连接远端。
5. 先保持 doctor 原有 JSON 字段与普通启动行为，新增可选诊断字段/检查前核对 tests 中机器可读契约；若改动显著，单独提交后续设计。
6. 在未设置 DYLD_* 的终端执行 cargo run，打开原有 Oracle profile，点击 Test，确认不再出现 DPI-1047。
7. 另测坏覆盖路径的 UI 提示；恢复配置重试。初始化成功后改变配置必须重启。

## 9. Task 7：最终复核（约 0.5 工程日）

1. 逐项核对预期 diff，不覆盖主 worktree 或其他任务改动。
2. 执行默认、禁用 feature 和全部 feature 三种构建检查：

```sh
cargo fmt --all -- --check
cargo check --all-targets
cargo check --no-default-features --all-targets
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
git diff --check
```

3. 运行 Task 5 的真实测试，单独记录执行数量与结果，不与 ignored 测试混为一谈。
4. 若禁用 feature 暴露原先 Oracle 模块未正确 cfg 的问题，修复与本次客户端初始化相关的编译边界；其余缺陷记录跟进。
5. 验收清单：
   - 无 DYLD_* 环境变量的 cargo run 能 Test 成功。
   - 显式环境变量可以指定另一安装目录。
   - 无效路径给出简明 Configuration 错误。
   - 未初始化失败后能够重新尝试。
   - 已成功初始化不会被其他 profile 切换客户端版本。
   - 非 Oracle 用户无客户端也能正常启动和使用。
   - 默认系统搜索方式仍然可用。
   - 原生测试没有 DPI-1047 静默跳过。
   - 没有远端写操作或凭据落盘。

## 10. 排期与交付

预计约 3–4.5 工程日，取决于当前 no-default-features 回归和进程级初始化测试的修复量。执行顺序为 Task 1 → 2 → 3 → 4 → 5 → 6 → 7，每项完成后核查改动和相关测试，再进入下一项。

交付物：稳定客户端安装目录、统一初始化模块、可操作的错误提示、可信的真库验收和安装文档。完成标准是用户直接 cargo run 点击 Test 成功，而不是仅完成编译或修改环境变量。
