# 交接文档 · WorkBuddy Deck（workbuddy-task-assistant）

> 写给接手的人/模型。读完这份就能继续干，不用重新摸索。
> 最后更新：2026-11-25 11:00

---

## 1. 这是什么

WorkBuddy 多账号控制台。形态：**后台哨兵 + CDP 注入**。

- 本地桥 `127.0.0.1:17653`（面板前端 + IPC）
- 带 `--remote-debugging-port=9222` 拉起 WorkBuddy，CDP 注入右下角悬浮图标
- **打包版无窗口、无托盘**（`tauri.conf.json` 里 main 窗口 `create:false`）；`npm run tauri dev` 才有窗口

技术栈：Tauri 2 + Vue 3 + Rust（`crates/wb-switch-core` 领域层）。

---

## 2. 硬约束（违反会惹用户）

1. **不要主动启动/重启应用或 WorkBuddy**。用户会自己启动。改完代码只提交，打包可以，运行测试不要做——启动会打断他的会话。
2. **不要主动 push**。用户说「推送」才推。
3. **不要升版本号**（`npm run bump` 只在用户明确要求时用）。
4. 打包产物名固定 `WorkBuddyDeck-Setup-0.1.28.exe`（Inno Setup），路径 `src-tauri/target/release/bundle/innosetup/`。
6. 会话数据：**禁止 `replace_sessions=true`**（物理删除目标账号会话，历史上丢过 20 条上下文）。

---

## 3. 当前已完成（按时间倒序，均已提交）

| commit | 内容 |
|---|---|
| `d4e3d57` | 进「设置」即查更新（绕 6h 缓存）；面板内也弹更新框（下载/进度/重启安装闭环） |
| `5fecaea` | 设置页删除「数据规模」「CDP 调试通道」两块；侧栏设置项加新版本提示点 |
| `6c94211` | 会话池**软替换**：当前池搬进目标账号，目标账号旧会话进隔离 uid `__wb_deck_parked__`（不删除） |
| `87c1c75` | 打包版彻底后台化：`create:false`、删托盘、`--quit` |
| `82115de` | （已被 6c94211 取代）会话池双向互换 |
| `39ad666` | 安装包快捷方式图标改指向独立 `icons/icon.ico`（绕 Windows 图标缓存） |
| `9eee684` | CDP 冷启动保护：连续失败 8 轮才允许重启客户端，避免重启风暴 |
| `588da16` | README 双语重写（qiangshou），含注入链路、热切换语义 |
| `644a1e7` | 宣传页重做（hallmark，Blueprint 主题）+ 12 张真机截图 + GIF |
| `9493342` | Dock 品牌标记：exe 图标、注入图标、favicon |

关键实现细节：

- **热切换**：`switch_workbuddy_account` 默认 `restart=false`，写认证 → 会话池软替换 → CDP reload → `probe_identity_uid` 探测；探测失败才回退重启。返回 `mode: hot / hot_restored / restart_fallback`。
- **会话可见性**：WorkBuddy 按 `sessions.user_id` 过滤。切换 = 目标账号旧会话进隔离区 + 当前池改挂目标 uid，一次事务完成。
- **注入图标**：`crates/wb-switch-core/src/modules/cdp.rs` 里 `WIDGET_VERSION`（当前 9），改图标要递增；`window.__wbDeckUrl` 暴露面板 URL（含 token）。
- **更新**：`useUpdate.ts` 模块级状态；`latest.json` + minisign；有更新时侧栏「设置」出现危险色小方块。

---

## 4. 主题 / 换肤（2026-11-25 用户最终决定：整体移除）

试过三轮方案，最后用户明确要求「主题功能都给我删除不要」，前后端与资产已全部移除：

- 后端：`src-tauri/src/themes/`（八模块）与 `src-tauri/assets/themes/`（10 主题）、
  `src-tauri/assets/skin/`（旧版 9 层组件 CSS + 3 个 schemaVersion 2 主题）全删；
  `lib.rs` 的 mod/6 命令/守护、`commands.rs` 六个薄命令、`deck_http/ipc.rs` 六条分发一并清掉。
- 前端：`SkinView.vue`、`api.ts` 主题契约、`i18n.ts` 的 `sk.*` / `navS.skin` / `cm.openSkin`、
  `nav.ts` / `TopNav.vue` / `App.vue` / `CommandPalette.vue` 的入口全删。
- 数据：`~/.wb-switch/skin.json` 不再被读取（保留文件无害）。

**不要再加回来** —— 用户已经三轮否掉。若将来要做，先确认需求再动手。

唯一**保留**的相关改动（与换肤无关、对悬浮图标有益）：
`cdp::pick_page_target` 优先主壳 target。WorkBuddy 会开 `?windowKind=global-search` 的辅助 page，
原来「命中即返回」可能连到搜索窗——那样注入的 CSS 全不匹配、悬浮图标还会长进搜索窗。
两个单测 `pick_page_target_prefers_main_shell_over_search_window` /
`pick_page_target_falls_back_to_auxiliary_window` 钉死这个行为。


## 5. 代码地图

```
crates/wb-switch-core/src/modules/
  cdp.rs          CDP 会话、注入脚本、悬浮图标、守护（WIDGET_VERSION）
  session.rs      workbuddy.db 会话池：软替换 / 备份 / 探测（PARKED_UID）
  switch.rs       切换编排（写认证、划归、探测、回滚）
  account.rs      账号库（~/.wb-switch/accounts.json，仅 cn）
src-tauri/src/
  launcher.rs     启动器：注册表找客户端、运行中拦截、带 --remote-debugging-port=9222 拉起
  installer/      Inno Setup 脚本 workbuddy-deck.iss（SrcDir 可覆盖、图标独立文件）
src/
  composables/useUpdate.ts   更新状态机（available/downloading/ready/failed）
  composables/useShell.ts    分区导航 go(key)
  views/SettingsView.vue     设置页（应用信息 / 外观 / 自动化摘要 / 维护）
  lib/i18n.ts                中英文案（sk.*、st.*、nav.* 前缀）
assets/logo/                      品牌标记与应用图标源文件
docs/screenshots/                 README 截图（真机）
site/                             宣传页（GitHub Pages，workflows/pages.yml）
```

数据目录：

```
~/.wb-switch/accounts.json          账号库
~/.wb-switch/cdp_config.json        autoStart 等
~/.wb-switch/backups/reassign-preflight-*   切换前整库备份
~/.workbuddy/workbuddy.db           WorkBuddy 会话库（只读排查用）
```

---

## 6. 常用命令

```bash
# 开发（无窗口，只跑桥 + 注入）
npm run deck

# 开发（带窗口，改前端看界面）
npm run tauri dev

# 类型检查 + clippy
npm run verify

# 只出 exe（应用没在跑才能覆盖）
npm run tauri build -- --no-bundle

# 出安装包（Inno Setup 6 需已安装）
npm run installer

# 退出后台哨兵
workbuddy-task-assistant.exe --quit
```

调试 CDP：

```bash
curl http://127.0.0.1:9222/json/list
# WebSocket 连接必须 suppress origin，否则 403
```

---

## 7. 坑与注意事项

  这两个 CSS 是 `include_str!` 进二进制的，cargo **不会**因为 CSS 的 mtime 变化就重编。
  症状极隐蔽——`cargo build` 只花 2 秒就 `Finished`，你以为编好了，跑起来还是旧样式。
  判据：真重编要 40 秒左右（整个 crate 重来）；2 秒完成 = 没编。
  ② `#root` 里的 `.teams-grid-scroll-content` / `.conversation-shell` 也被 palette 设成主题纯色，
  中间层再盖一次。两条都已修，`shell.css` 的容器透明组里各有对应规则，**别删**。
- **「面板模糊」滑块无效的两个原因**（2026-11-25 踩）：
  ① `shell.css` 原来用 `[class*="chat-input"]`，WorkBuddy 现版**没有这个类**（命中 0 个元素），
  已换成实测的 `.cr-input-container` / `.cr-input-container-wrapper` / `.cr-input-box__main`；
  ② 点外观模式会把 `blur` 打回该模式默认值（light 是 0），用户调好的值被覆盖，看起来像滑块失灵。
  已改成「只在当前 blur 为 0 时才接管模式默认值」。
- `tauri-build` 不会因为换了 `icons/` 自动重编资源：**改图标后必须 `touch src-tauri/build.rs`**，否则 exe 里还是旧图标（踩过）。
- Windows 图标缓存按快捷方式路径缓存：改图标后要么卸载重装，要么让快捷方式 `IconFilename` 指向独立 ico 文件（已在用）。
- Inno `OutputDir` 里同名文件被占用/杀软扫描会报 `EndUpdateResource failed (110)`：重跑一次即可。
- `dist/` 删除有时被 safe-delete 拦住：用 `cmd //c "rd /s /q dist"`。
- 面板 iframe 里 `isEmbedded()` 为 true：以前会吞掉更新弹框（已改为也弹）。
- `app_ready` 会 `show()` 主窗口；无窗口模式下窗口不存在，是 no-op。
- 会话池软替换：切回原账号不会恢复对方历史，被隔离的会话只能靠备份回放。
- 仪表盘里的 193 条会话是历史单向合并的产物，新逻辑不会自动拆开。

---

## 8. 下一步优先级

4. 更新提示点做颜色区分：`available` 与 `ready`（已下载待重启）现在都是同一个方块。
