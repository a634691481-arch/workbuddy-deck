
## 2026-11-03 发版必须用户明示
- 错误：修完窗口尺寸就自行 bump 0.1.17 + 打 tag + push 触发发版 CI。
- 用户规则：**改完功能只提交推送，不发版**。只有用户明确说「发版 / 更新安装包 / 出新版本」时才执行 bump + tag + push release.yml。
- 推送 ≠ 发版：push main 是默认动作；tag 触发安装包构建是独立授权动作。
- 补救流程（已执行）：删远端/本地 tag → cancel Actions run → 版本号回退 → push。
