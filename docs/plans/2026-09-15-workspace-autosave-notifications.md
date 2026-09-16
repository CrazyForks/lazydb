# Workspace Autosave Notifications Implementation Plan

> 执行方式：按任务顺序实施，每一步完成后核对预期结果。

**Goal:** 工作区自动保存成功时静默完成，保存失败时继续提供可见反馈，打开连接不再因保存成功而被浮层遮挡。

**Architecture:** 在 App 处理 `WorkspaceSaveSucceeded` 的入口移除成功通知，继续完成保存状态确认、延迟 SQL 文件删除以及运行时队列确认。沿用现有失败通知和退出保存交互，通过已有保存生命周期测试与手工场景验收。

**Tech Stack:** Rust 2024、Ratatui、Tokio、现有工作区持久化与通知中心。

---

## 交互决策与验收标准

| 场景 | 预期行为 |
| --- | --- |
| 打开连接、激活工作区后保存成功 | 静默完成，不产生成功浮层 |
| 连续多次保存成功 | 不累计成功通知，也不写入通知历史 |
| 普通运行期间保存失败 | 继续显示 Workspace 错误通知和具体原因 |
| 退出期间保存失败 | 继续显示已有保存失败弹层及重试、放弃保存退出、取消退出交互 |
| 保存成功后退出 | 收到正确的保存确认及 flush 完成后退出 |
| 已有其他通知 | 正常展示，保存成功事件不清空其他通知 |
| 重启恢复工作区 | 标签页、SQL 内容等继续按现有持久化能力恢复 |

本次采用“成功静默、失败可见”的完整交互。此前建议中的状态栏“已保存”是按需增强，目前没有证据表明需要增加常驻状态，暂不纳入实施任务。

## 代码定位

- `src/app.rs:4632`：成功事件处理，目前调用 `notify_info`，是本次生产代码修改点。
- `src/app.rs:4654`：保存失败事件处理及退出失败交互。
- `src/app.rs:2467`：构造保存命令并分配 revision。
- `src/app.rs:11358`：连接就绪处理中由目标更新或工作区激活触发保存。
- `src/model/workspace_save.rs`：revision 与保存状态生命周期。
- `src/runtime.rs:149`：保存队列；保存成功后仍需接收完成命令，以继续处理队列。
- `src/model/notification.rs:88`：通知入队会同时写入历史和实时通知列表，因此应从产生通知的源头静默。
- `src/persistence/workspace.rs:265`：实际写入工作区快照和 SQL 文件。

行号是计划编写时的参考，实施时按符号定位。

### Task 1：实施前确认（约 5 分钟）

**Files:** 检查 `src/app.rs`、`src/model/workspace_save.rs`、`src/runtime.rs`。

1. 运行 `git status --short`，确认当前差异。
2. 定位 `Action::WorkspaceSaveSucceeded`、`Action::WorkspaceSaveFailed` 及 `Command::CompleteWorkspaceSave`，核对实现是否与本计划一致。
3. 运行现有基线测试：

   ```bash
   cargo test workspace_save
   ```

   预期：匹配到的保存状态、App 保存流程和运行时队列测试通过。若存在基线失败，记录具体测试和原因，再判断是否影响本任务。

### Task 2：让自动保存成功静默（约 5 分钟）

**Modify:** `src/app.rs`，`Action::WorkspaceSaveSucceeded` 分支。

1. 删除以下成功通知调用：

   ```rust
   self.notify_info("Workspace", format!("Saved workspace revision {revision}"));
   ```

2. 确认成功分支仍按原顺序执行：
   - `self.workspace_save.succeeded(revision)`；
   - 获取有效 SQL 编辑器 ID；
   - 处理该 revision 的延迟 SQL 文件删除；
   - 返回 `Command::CompleteWorkspaceSave { revision, succeeded: true }`。
3. 核对失败分支仍提供运行期间的错误通知，以及退出期间的保存失败弹层。
4. 审阅差异，确认修改位于成功通知产生处。

该修改低影响且可直接撤回，采用已有测试与手工验收，不新增只验证删除一行通知调用的测试。

### Task 3：运行现有回归检查（约 5–15 分钟，取决于编译缓存）

**Existing tests:** `src/app.rs` 内联测试、`src/model/workspace_save.rs` 内联测试、`src/runtime.rs::workspace_save_tests`、`tests/workspace_persistence.rs`。

1. 运行格式检查：

   ```bash
   cargo fmt --check
   ```

2. 运行保存相关测试：

   ```bash
   cargo test workspace_save
   ```

   重点确认成功回执、失败重试、丢弃保存退出、取消退出、关系表和 Dashboard 标签页处理，以及运行时保存队列相关测试通过。

3. 验证退出必须等待保存确认：

   ```bash
   cargo test workspace_flush_does_not_quit_before_save_is_acknowledged
   ```

4. 验证持久化与恢复：

   ```bash
   cargo test --test workspace_persistence
   ```

   预期：工作区往返保存恢复、SQL 文件保存和已有格式兼容相关测试通过。

5. 检查补丁空白错误：

   ```bash
   git diff --check
   ```

若出现工具链或依赖环境问题，记录真实阻塞，不将未执行的检查标记为通过。

### Task 4：手工验收（约 10 分钟）

**Entry:** `cargo run`，使用可连接的测试数据库。

1. 打开一个已有连接并等待工作区恢复完成：右上角不出现 `Saved workspace revision …`。
2. 连续切换标签页、打开及关闭表标签页：等待后台保存，数据区域不被保存成功浮层遮挡。
3. 打开通知历史：没有本次操作产生的工作区保存成功记录。
4. 在 SQL 编辑器输入一段可识别文本，正常退出再启动：确认 SQL 文本及已保存标签页恢复。
5. 观察连接失败等其他正常产生的错误通知：仍可见；成功保存不应清空它们。

失败注入如需手工验收，应在独立临时测试工作区进行，并先确认项目实际支持的存储路径隔离方式。普通保存失败提醒、退出重试与取消路径优先由既有自动化测试验证。

### Task 5：审阅与交付（约 5 分钟）

1. 运行 `git diff -- src/app.rs`，核对成功处理的其余流程完整。
2. 汇总实际通过的检查与手工验收结果；记录任何未能执行的场景。
3. 交付说明：工作区保存成功改为静默处理，失败仍有可见反馈。
4. 用户要求提交时，建议使用：`fix(workspace): silence successful autosave notifications`。

## 完成定义

- 截图中的成功通知不再出现，也不进入通知历史。
- 保存状态确认、保存队列推进、延迟 SQL 删除和退出保存握手继续正常执行。
- 保存失败仍可见，并保留已有退出失败处置能力。
- 指定回归检查通过，手工验收结果可追溯。

## 估算

总工作量约 30–45 分钟，首次编译时间另计；生产代码预计只涉及 `src/app.rs` 一处删除。
