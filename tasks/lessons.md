
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
