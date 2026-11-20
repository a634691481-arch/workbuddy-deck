
## 2026-11-03 发版必须用户明示
- 错误：修完窗口尺寸就自行 bump 0.1.17 + 打 tag + push 触发发版 CI。
- 用户规则：**改完功能只提交推送，不发版**。只有用户明确说「发版 / 更新安装包 / 出新版本」时才执行 bump + tag + push release.yml。
- 推送 ≠ 发版：push main 是默认动作；tag 触发安装包构建是独立授权动作。
- 补救流程（已执行）：删远端/本地 tag → cancel Actions run → 版本号回退 → push。

## 2026-11-03 推送也要用户明示 + flex 收缩三件套
- 用户再纠正：**别主动推送代码**。本地 commit 可以，`git push` 必须用户明示。规则已写入用户级 MEMORY.md。
- 布局坑：右栏 `main` 缺 `min-w-0`，flex 子项默认 min-width:auto，TelemetryBar 六格 nowrap 读数把面板撑出窗口 → 右侧固定列被推出屏幕外（用户「看不到状态列」的根因）。
- 修法（三处，通用）：① 分区视图容器 `min-w-0 flex-1`（App.vue KeepAlive）② 面板 `main` 加 `min-w-0` ③ 宽内容格 `min-w-0` + 读数 `truncate`（TelemetryBar）。以后新面板先检查 flex 链路上每一层都有 min-w-0。

## 2026-11-06 CI 新增 job 三连坑（各挂一次发布）

1. **verify job 没有 checkout**：引用仓库内脚本（`scripts/gen_cn_manifest.py`）直接 ENOENT。
   → 任何要跑仓库文件的 job，第一步必须 `actions/checkout@v4`。
2. **`gh release upload` 需要 `permissions: contents: write`**：默认 token 只读，报
   `403 Resource not accessible by integration`。build job 有权限不代表新 job 也有。
3. **YAML 块里写 heredoc 必炸**：`run: |` 的内容必须缩进，而 heredoc 结束符要求顶格。
   → 内联多行脚本一律改成「提交到 scripts/ 的独立文件 + 一行调用」。

规则：新增 job 先自查三件事——checkout、permissions、有没有用 heredoc。
发版前用 `python -c "import yaml;yaml.safe_load(...)"` 校验，再等 CI 结果，不要连推三个 tag。

## 2026-11-07 代码审查抓到的四类病（一次全犯）

1. **凭记忆读字段名**：提醒中心读 `health.daysLeft` / `health.status`，实际 `daysLeft` 在
   `forecast`、等级字段叫 `grade`。→ 结果五类告警三类永不触发，编译期完全看不出来。
   规则：跨模块取字段前先打开对方的 struct / json! 定义，别信「应该有」。
2. **备份不 fail-closed**：中途 `Err(_) => continue`，最后照样报「已备份 N 个文件」。
   规则：备份/迁移这类「用户以为能救」的动作，任一文件失败即整份作废（临时目录 + 全成功才 rename）。
3. **单文件/单函数无上限**：`alerts.rs` 334 行、`evaluate()` 141 行、`account_groups.rs` 385 行。
   规则：文件过 300 行、函数过 60 行就当场拆，别等 review 抓。
4. **toast 单槽被后一条覆盖**：失败提示后紧跟 `say(已保存)`，用户只看得到成功。
   规则：同一流程里错误分支必须 `return`，不能靠「下一条不会覆盖」的侥幸。

另：`cargo clippy` 不编译测试代码，测试文件里的字段缺失只有 `cargo test` 才发现——加字段后必须跑一次 `cargo test --lib`。

## 2026-11-09 发版 CI：中文提交把 Windows job 打红

- 现象：release workflow 只有 windows-latest 挂在「Build release notes」，ubuntu/mac 正常。
- 原因：Python `subprocess.run(..., text=True)` 在 Windows runner 用系统默认编码（cp1252）解码
  `git log` 的中文输出 → `UnicodeDecodeError`。
- 修法：显式 `encoding="utf-8", errors="replace"`；整个说明生成包 try/except 兜底，
  发布说明失败绝不能让发版失败；解释器用 `actions/setup-python`，别指望 runner 自带 `python3`。
- 另：`npm run bump 0.1.28 --tag` 的 `--tag` 会被 npm 吞掉（要 `npm run bump 0.1.28 -- --tag`），
  否则只改文件不提交/打 tag。

## 2026-11-20 切换账号删会话事故（不可逆操作必须默认关）

- 事故：`replace_sessions=true` 的切换把目标账号 20 条会话物理删除（正文 + DB 行），
  用户侧表现是「切完上下文全没了」；只能靠 `~/.wb-switch/backups/switch-preflight-<ts>/`
  的 jsonl + manifest sha256 逐条恢复，DB 行还得手工补。
- 结论：**切换只换认证，不碰会话**。`switch_account_detailed(...,restart=true,...,replace_sessions=false)`，
  手动与自动一律如此。要复制/合并会话必须另开显式入口，不能挂在「切换账号」按钮上。
- 热 reload（restart=false）会保留旧身份的缓存，侧栏看起来空 → 切换必须 restart=true。
- 恢复顺序：先 files（copy2 + sha256 校验），再补 `sessions` 行（INSERT OR IGNORE + 先备份 db），
  最后重启 WorkBuddy 让它重读索引。

## 2026-11-20 切换语义定稿：一套上下文跟着当前账号走

- 根因：WorkBuddy 侧栏按 `sessions.user_id` / `spaces.user_id` 过滤，切号 = 换 uid = 查另一批行。
  「上下文全没了」从来不是被删（replace 事故除外），而是查到了新账号（往往是空的）名下。
- 定稿：切换 = 换认证 + 把上一账号名下会话/空间整体 `UPDATE user_id` 划归新账号。
  时机必须卡在「WB 已关 + 认证写对 + 身份校验通过」之后、启动之前。
- 不用「复制会话」实现共用：复制会造出两份 id 不同的同名会话，越切越多；
  UPDATE 归属是唯一不产生垃圾副本的方式。旧 replace 事故留下的多 uid 残留行
  永远不可见（不在当前 uid 名下），不用清理。
