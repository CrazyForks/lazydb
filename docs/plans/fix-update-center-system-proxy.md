# Update Center 系统代理与错误诊断修复 Implementation Plan

> **执行者：Luna。** 按以下可验收单元连续实施；Astra 仅负责当前分析/计划。不要启动子 Agent。实现、审查、纠偏、提交合并均遵循任务插件的后续阶段协议。

**Goal:** 让只配置 macOS 系统 HTTP/HTTPS 代理的用户能够下载更新，并在请求失败时看到可定位的底层原因。

**Architecture:** 使用 reqwest 0.12.28 已有的系统代理发现功能，保留当前 HTTP 客户端、进度回调和安全校验。更新 runtime 传递完整错误链；用本地网络 fixture、安装保护测试和显式网络探针验证行为，不引入 curl 运行时依赖。

**Tech Stack:** Rust 1.94.0、reqwest 0.12.28、hyper-util 0.1.20、Tokio、anyhow、ratatui。

---

## 0. 基线、证据和范围

### 验证分级（适用于全文）

- **用户需求验收：**修复已确认的系统代理兼容缺口，使失败原因可诊断，更新失败保持原安装可用。实现者通过实际生产代码路径和定向自动测试证明这些行为。
- **项目强制门禁：**后文从 `.github/workflows/ci.yml` 引用的 fmt、clippy、全量 Rust 测试，以及 macOS 环境对应的二进制依赖检查。平台或服务专属检查按其 CI 环境执行并记录，不能声称本机已通过。
- **本计划新增的自动回归：**本地代理 fixture、错误链传播、安装保护与 TestBackend 渲染；用于验证本次变更，不要求人工操作真实用户安装。
- **补充建议验证：**真实 GitHub 资产基线/修复版探针，依赖外部网络和已有系统代理，不是用户或项目新增的硬门禁。环境不具备时，记录限制并由 Luna 收尾审查评估已有自动化证据；不得因此要求用户反复恢复任务。若可用环境下探针暴露明确产品错误，应修复该错误。无需人工/PTY 验收。

### 预计变更清单

精确文件列表另存同目录 `change-scope.json`。包含 `Cargo.toml`、`Cargo.lock`、`src/update.rs`、`src/runtime.rs`、`src/ui/update.rs`、`tests/update_http.rs`、`tests/update_reducer.rs`。其中 UI 至少预计增加渲染回归，生产布局仅在测试证明必要时调整；lock 仅提交实际必要变更。`tests/update_native.rs`、CI 配置等仅读取或运行，不列入修改范围。本阶段不额外生成仓库 docs 计划文件；无删除、重命名或未提交依赖文件。

- 原工作区 `/Users/yelog/workspace/tui/lazydb`，目标 main，起点 `bc01be88d9e0387adfe12bde63765a6284f045f8`。
- 计划阶段再次核对 HEAD 等于起点，status 为空；没有需要复制进 worktree 的未提交文件或本地新行为依赖。后续工作区由插件/Luna 管理，此阶段不创建。
- 权威分析为同目录 `analysis.md` 开头的“恢复后最终决策”；底部首轮缺少图片的阻塞已解除。checkpoint 仍可能包含首轮 next，不把它当作新的要求。
- 已确认 reqwest/system-proxy 和 hyper-util/client-proxy-system 均未启用，锁定库源码显示 macOS 系统代理发现受此开关控制。环境变量代理不受这个开关限制。
- 截图为 v0.1.7 ARM64 macOS 包请求发送失败。当前机器经环境代理完整下载成功，直连 20 秒无响应；这是代理缺口的支持证据，不是截图进程环境的直接证据。验收需要真实 Rust 下载路径。
- 必改：`Cargo.toml`、必要的 `Cargo.lock`、`src/runtime.rs`；定向增强 `src/update.rs` 及其测试。新增 `tests/update_http.rs` 承载网络 fixture 和 opt-in 探针。仅在实际长错误截断时调整 `src/ui/update.rs`。
- 不调整版本号、发布包格式、超时策略、TLS 校验、重定向 allowlist 或 checksum 规则。若后续新证据指向其他问题，先记录证据和范围变化。

## 单元 1：系统代理下载可用

### 1.1 建立同路径基线探针

**文件：**新增 `tests/update_http.rs`。直接使用公开的 `lazydb::update::{SystemUpdateHttpClient, UpdateHttpClient}`，不重新拼一个与生产配置可能不同的 reqwest client。

添加以下显式忽略的公网检查；它只下载到内存，不解压、不运行资产、不写安装目录。该测试不是默认 CI 条件：

```rust
use lazydb::update::{SystemUpdateHttpClient, UpdateHttpClient};

#[tokio::test]
#[ignore = "requires network access and an existing system proxy; run explicitly"]
async fn system_proxy_release_download() -> anyhow::Result<()> {
    let url = "https://github.com/yelog/lazydb/releases/download/v0.1.7/lazydb_0.1.7_aarch64-apple-darwin.tar.xz";
    let observed = std::sync::Mutex::new(Vec::new());
    let client = SystemUpdateHttpClient::default();
    let bytes = client
        .download_with_progress(url, &|progress| observed.lock().unwrap().push(progress))
        .await?;
    assert!(!bytes.is_empty());
    let progress = observed.lock().unwrap();
    let last = progress.last().expect("download must report progress");
    assert_eq!(last.downloaded_bytes, bytes.len() as u64);
    if let Some(total) = last.total_bytes {
        assert_eq!(total, bytes.len() as u64);
    }
    Ok(())
}
```

按项目实际公开模块编译确认 imports。不要把公网资产长度写成常量断言；该探针验证传输和进度，SHA-256 与归档验收由已有更新测试覆盖。

构建测试后对子进程清理代理环境变量，再运行：

```sh
env -u HTTP_PROXY -u HTTPS_PROXY -u ALL_PROXY -u NO_PROXY -u http_proxy -u https_proxy -u all_proxy -u no_proxy cargo +1.94.0 test --locked --test update_http system_proxy_release_download -- --ignored --exact --nocapture
```

仅在 macOS 且已经开启可用系统 HTTP/HTTPS 代理时做此对照，不修改系统代理配置。基线预期在已有请求边界内失败，记录完整 source；如果基线成功，不声称复现了截图，记录网络变化并继续已确认的功能缺口修复。

### 1.2 启用系统代理能力

**修改 `Cargo.toml:32`：**

```toml
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "system-proxy"] }
```

由 Cargo 正常解析并更新必要 lock 条目，不运行全局 `cargo update`。确认 reqwest 仍锁定 0.12.28。若本地缓存不足允许获取所需依赖，但不要升级无关依赖。

运行：

```sh
cargo tree --locked -e features -i reqwest
cargo tree --locked -e features -i hyper-util
```

预期分别出现 `system-proxy` 和 `client-proxy-system`。使用 1.1 的同一命令重跑修复版探针，预期真实生产客户端下载成功且进度终值一致。

### 1.3 默认 CI 的确定性代理回归

在 `tests/update_http.rs` 添加父/子测试进程模式：父进程用 `std::env::current_exe()` 和 `--exact` 启动一个测试 worker，在 `Command` 上设置环境变量；worker 入口无测试标记环境时立即返回。不要在 Tokio 或并行测试中 `set_var/remove_var`。

具体用例：

1. 本地假 HTTP 代理绑定 `127.0.0.1:0`，子进程设置 HTTP_PROXY（另一个用例设置小写），请求 `http://fixture/asset`。代理断言收到 absolute-form URL，然后返回固定 bytes；生产 `download_with_progress` 返回相同内容及正确进度。fixture 域名是现有校验允许的本地测试主机，勿以解析不可达域名作为超时测试。
2. HTTPS_PROXY 本地 CONNECT 代理：请求固定 HTTPS fixture 主机，代理记录 CONNECT authority 后返回确定的失败响应，断言完整错误链可见并且确实到达指定代理；不启动真实 TLS MITM。
3. NO_PROXY 子进程指定 `127.0.0.1`，设置不可用代理和可用本地目标服务，断言目标成功响应，证明 bypass 仍有效。

每个本地服务器和子进程有显式结束边界，不使用固定端口，不依赖开发者代理。环境变量优先级等第三方库全量组合不在范围内；以上测试覆盖应用实际客户端的基本路由契约。

运行 `cargo +1.94.0 test --locked --test update_http`。預期默认用例通过，公网探针明确 ignored。

**单元验收：**必要 feature 生效；默认代理 fixture 通过；记录 macOS 修复前后真实探针结果或准确环境限制。单独 curl 成功不算此单元 Rust 下载验收。

## 单元 2：请求失败 → 可诊断提示，安装失败 → 原安装保留

### 2.1 保留错误链

**文件：**`src/runtime.rs:1257,1298`，两个更新失败事件将：

```rust
message: error.to_string(),
```

改为：

```rust
message: format!("{error:#}"),
```

限定更新相关分支，不批量更改其他错误展示。若为了测试抽取事件构造函数，必须是生产分支实际调用的函数，不创建仅测试使用的格式化副本。

### 2.2 HTTP 操作上下文

**文件：**`src/update.rs` 的 `get`、`download`、`download_with_progress`。

引入 `anyhow::Context` 后，在请求发送、HTTP 状态和 body 读取边界添加简洁上下文。例如请求发送使用 `.with_context(|| format!("failed to request update asset {url}"))?`，body 使用 `.context("failed to read update asset body")?`。清单路径采用 manifest 对应表述，避免与 `fetch_manifest` 已有外层 URL 上下文重复。

保留 `validate_response_url`、`error_for_status`、source 类型和进度调用顺序，不统一映射成没有 source 的字符串，不新增显示代理认证信息。保持代码风格；如为避免三处重复提取小 helper，只封装共同边界，不重构整个 updater。

### 2.3 有效回归测试

1. `tests/update_http.rs`：本地服务提前关闭连接或 CONNECT 明确拒绝，断言生产客户端返回错误且 source 链非空；断言提示有操作上下文及底层原因，不依赖平台 errno 完整英文句子。
2. `src/runtime.rs` 测试或实际使用的错误事件构造函数测试：从带 source 的 anyhow 错误进入 `UpdateInstallFailed`，断言事件消息保留外层与底层原因。单独测试 `format!` 自身不算回归。
3. `tests/update_reducer.rs`：扩展 `install_failure_is_retryable`，输入具体错误链，断言 Failed 消息保留、无 ReadyToRestart；执行 Check again 验证重新进入检查。利用现有 request id 状态流，不直接伪造最终 App 状态。
4. `src/update.rs` 内复用 Native 安装 fixture，添加下载失败的 `UpdateHttpClient` fake。断言返回错误、旧 current 符号链接和 install.json 内容不变、没有激活新版本。既有 checksum 和 staged-version 失败用例继续保留。
5. `src/ui/update.rs` 测试用 TestBackend 在 80×24 渲染含 URL 与 source 的错误，断言关键原因和 Check again 可见；保持终端字符清理。若实际渲染截断根因，调整错误摘要/详情布局使原因可读，不为此引入全新弹窗体系。

按测试命名先定向运行新增用例；然后统一跑：

```sh
cargo +1.94.0 test --locked --lib update::
cargo +1.94.0 test --locked --test update_http --test update_native --test update_reducer
```

第一条匹配 updater 与 UI 的 update 模块测试；runtime 新增用例如不在该过滤范围，显式按其完整测试名运行。预期全通过，公网探针仍 ignored。

**单元验收：**可见错误从 HTTP source 经过 runtime 事件和 reducer 到 UI；请求失败不会误显示成功或破坏原安装；成功安装相关现有用例仍进入 ReadyToRestart。

## 单元 3：收尾检查与交付（Luna）

项目 `.github/workflows/ci.yml:81–88` 规定 Rust fmt、clippy、全量测试及 macOS 二进制依赖检查。功能齐备后在实施 worktree 对最终代码运行一遍：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 clippy --locked --all-targets --all-features -- -D warnings
cargo +1.94.0 test --locked --all-targets --all-features
cargo +1.94.0 build --locked
sh scripts/release/check-macos-dependencies.sh target/debug/lazydb
```

若设置了 CARGO_TARGET_DIR，最后一条使用该目录下的真实 binary 路径。最后两条适用于 macOS；系统代理新增系统依赖必须满足已有分发依赖规则。数据库服务矩阵/Windows 安装脚本在对应 CI 环境验收，不把本机未运行说成已通过。

验证结果追加至本任务 `validation.md`：命令、退出码、commit/工作区 diff 状态、环境以及公网探针是否清理变量。相关代码/环境未变化时不重复全量测试；普通失败由 Luna 修复。人工/PTY 非本次强制验证，可用 TestBackend；环境受限检查最多一次针对性修复重试，之后由 Luna 收尾审查记录限制或选择补充证据。

审查检查项：

- Cargo.lock 没有无关版本漂移；Linux 和 macOS feature 配置兼容。
- 无运行时 shell/curl 依赖，无证书验证豁免，无 allowlist 放宽。
- 错误上下文保留 source，用户可读，未新增代理凭据泄露。
- 下载失败不写坏原安装，已有成功安装保护行为保留。
- 从原基线到最终 diff 的变更均在任务范围内。

按插件后续阶段规则完成命名、提交、合并。建议名称“修复 Update Center 系统代理下载”，分支名供 Luna 决定；本计划不创建分支、不操作 index。提交仅纳入本任务文件，推荐单个闭环提交 `fix(update): honor system proxies and preserve request errors`；需要分提交时两单元各自保持可验证。

## 完成定义

1. 系统代理功能在实际编译图中启用。
2. 实际 HTTP 客户端的代理/NO_PROXY fixture 和错误链回归通过。
3. macOS 仅系统代理的资产探针成功，或明确记录未能取得该补充环境证据；若可用环境下仍失败，必须依据 source 继续修复而非直接宣布完成。
4. 原安装保护、更新状态流及相关 UI 测试通过，项目强制检查结果完整记录。
5. 报告不把当前代理证据推断写成截图当时进程环境的已知事实。

## 计划阶段交接

当前只写任务目录的 plan/validation 和本轮回执；没有执行上述实施命令或测试。后续无需用户选择执行方式，由插件交给 Luna 持续推进。
