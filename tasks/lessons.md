
## 2026-11-03 发版必须用户明示
- 错误：修完窗口尺寸就自行 bump 0.1.17 + 打 tag + push 触发发版 CI。
- 用户规则：**改完功能只提交推送，不发版**。只有用户明确说「发版 / 更新安装包 / 出新版本」时才执行 bump + tag + push release.yml。
- 推送 ≠ 发版：push main 是默认动作；tag 触发安装包构建是独立授权动作。
- 补救流程（已执行）：删远端/本地 tag → cancel Actions run → 版本号回退 → push。

## 2026-11-03 推送也要用户明示 + flex 收缩三件套
- 用户再纠正：**别主动推送代码**。本地 commit 可以，`git push` 必须用户明示。规则已写入用户级 MEMORY.md。
- 布局坑：右栏 `main` 缺 `min-w-0`，flex 子项默认 min-width:auto，TelemetryBar 六格 nowrap 读数把面板撑出窗口 → 右侧固定列被推出屏幕外（用户「看不到状态列」的根因）。
- 修法（三处，通用）：① 分区视图容器 `min-w-0 flex-1`（App.vue KeepAlive）② 面板 `main` 加 `min-w-0` ③ 宽内容格 `min-w-0` + 读数 `truncate`（TelemetryBar）。以后新面板先检查 flex 链路上每一层都有 min-w-0。
