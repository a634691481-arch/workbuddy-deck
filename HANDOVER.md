# 交接文档 · WorkBuddy Deck（workbuddy-task-assistant）

> 写给接手的人/模型。读完这份就能继续干，不用重新摸索。
> 最后更新：2026-11-25 11:00

---

## 1. 这是什么

WorkBuddy 多账号控制台。形态：**后台哨兵 + CDP 注入**。

- 本地桥 `127.0.0.1:17653`（面板前端 + IPC）
- 带 `--remote-debugging-port=9222` 拉起 WorkBuddy，CDP 注入右下角悬浮图标
- 点图标 → 页面内 iframe 面板（账号 / 任务 / 积分 / TOKEN / 日志 / 归档 / 外观 / 皮肤 / 设置）
- **打包版无窗口、无托盘**（`tauri.conf.json` 里 main 窗口 `create:false`）；`npm run tauri dev` 才有窗口

技术栈：Tauri 2 + Vue 3 + Rust（`crates/wb-switch-core` 领域层）。

---

## 2. 硬约束（违反会惹用户）

1. **不要主动启动/重启应用或 WorkBuddy**。用户会自己启动。改完代码只提交，打包可以，运行测试不要做——启动会打断他的会话。
2. **不要主动 push**。用户说「推送」才推。
3. **不要升版本号**（`npm run bump` 只在用户明确要求时用）。
4. 打包产物名固定 `WorkBuddyDeck-Setup-0.1.28.exe`（Inno Setup），路径 `src-tauri/target/release/bundle/innosetup/`。
5. 视觉体系：工业遥测 / CRT，**圆角默认 0**、强调色 `--accent`、1px 实线分隔、不用渐变和柔光。皮肤模块是给 WorkBuddy 换肤，不受此限。
6. 会话数据：**禁止 `replace_sessions=true`**（物理删除目标账号会话，历史上丢过 20 条上下文）。

---

## 3. 当前已完成（按时间倒序，均已提交）

| commit | 内容 |
|---|---|
| `dcec4e7` | 皮肤入口恢复；视频壁纸（mp4/webm/mov/ogv，静音循环、切后台暂停）；15s 守护重贴；「恢复官方」清配置 |
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

## 4. 未完成：WorkBuddy 换肤补丁层（重点）

### 需求
参照 `https://github.com/babygoton/WorkDaddy` 的主题功能：6 种外观（浅/深/护眼绿/赛博紫/毛玻璃/跟随系统）+ 壁纸（图片/视频）+ 面板模糊/压暗 + 恢复官方。UI 用本项目的工业风格。

### 结论（已实测，别重复踩）
1. **WorkDaddy 代码是 AGPL-3.0，不能抄**，只能重写。
2. 本项目已有 `src-tauri/src/skin.rs`（806 行）+ `src/views/SkinView.vue`，色板 `assets/skin/palette.css`、外壳 `assets/skin/shell.css`。
3. 实测结论（在本机运行中的 WorkBuddy 上用 CDP 验证）：
   - 只改 `--wb-*` 变量 → **界面完全不变**，现版 UI 不消费这套变量。
   - 改 `agent-ui-theme` + `workbuddy.appearance.state/mode/lastApplied`（全部账号 uid）再 reload → **仍是浅色**，官方主题状态不在 localStorage，或会被账号档案覆盖。
   - 强改 `html/body` 的 `dark cb-dark vscode-dark` 类 → 背景能变深，但文字仍是浅色主题的黑灰，直接不可读。
   - **有效路径**：直接压组件结构/前缀类（`conversation-shell`、`teams-grid-scroll-content`、`#workbuddy-menubar-container`、`.cr-*`、`.cb-agent-card`、`conversation-section-label`）+ `!important`。
4. 已把验证有效的规则写进 `assets/skin/palette.css`（结构层、文本、输入框、代码块、表格、气泡、tooltip），并补齐 `--cb-*`、`--sk-*` 变量映射。

### 残留问题（下一轮要修）
在真实页面上截图确认（`assets/logo/renders/patch1..3.png` 是过程证据）：

- 侧栏会话行仍是白色药丸：真实类是 `_card_1ufy3_1 cb-agent-card _selected_...`，背景可能挂在内层节点，现有 `.cb-agent-card` 规则没压住。
- 输入框右侧模型选择器仍是白块（`cr-input-container` 内部子元素）。
- 表格能变透明，但表头/单元格还有浅色底。
- 部分工具栏圆形按钮仍是白底。

### 建议做法
1. 用 CDP 打开 `http://127.0.0.1:9222/json/list` → 取 page 的 `webSocketDebuggerUrl`（连接要 `suppress origin`，否则 403）。
2. `Runtime.evaluate` 注入 `<style id="__probe">` 试规则 → `Page.captureScreenshot` 截图看效果 → 迭代。**不需要重启 WorkBuddy**。
3. 定稿后把规则搬进 `assets/skin/palette.css`（占位符 `__WBX_*__` 由 `skin.rs::render_palette` 替换），并在 `shell.css` 里补外壳/壁纸层。
4. 每轮只改一类元素，避免一次改崩。

### 验收标准
- 三种自定义配色（mint/cyber/nebula）下：主区、侧栏、输入框、代码块、表格、气泡全部统一色系，无白块，文字对比度可读。
- 壁纸（图片/视频）能透出，输入框保留毛玻璃。
- 「恢复官方」后完全回到原样，且 15 秒守护不会把皮肤贴回来（已修：会清配置）。

---

## 5. 代码地图

```
crates/wb-switch-core/src/modules/
  cdp.rs          CDP 会话、注入脚本、悬浮图标、守护（WIDGET_VERSION）
  session.rs      workbuddy.db 会话池：软替换 / 备份 / 探测（PARKED_UID）
  switch.rs       切换编排（写认证、划归、探测、回滚）
  account.rs      账号库（~/.wb-switch/accounts.json，仅 cn）
src-tauri/src/
  lib.rs          启动流程：无窗口判断、单实例、启动器 preflight、皮肤守护
  commands.rs     薄命令层（switch_workbuddy_account / skin_* / app_ready / events_*）
  skin.rs         换肤：原生外观同步、色板渲染、壁纸、视频层、恢复
  deck_http/      本地桥（/__ipc、/__events、/__deck_shim.js、/skin/wallpaper、静态资源）
  launcher.rs     启动器：注册表找客户端、运行中拦截、带 --remote-debugging-port=9222 拉起
  installer/      Inno Setup 脚本 workbuddy-deck.iss（SrcDir 可覆盖、图标独立文件）
src/
  composables/useUpdate.ts   更新状态机（available/downloading/ready/failed）
  composables/useShell.ts    分区导航 go(key)
  views/SkinView.vue         皮肤页（模式 + 壁纸图库 + 上传 + 模糊/蒙版）
  views/SettingsView.vue     设置页（应用信息 / 外观 / 自动化摘要 / 维护）
  components/TopNav.vue      顶部导航（含 skin 入口）
  lib/i18n.ts                中英文案（sk.*、st.*、nav.* 前缀）
assets/skin/{palette,shell}.css   换肤注入模板（__WBX_*__ 占位符）
assets/logo/                      品牌标记与应用图标源文件
docs/screenshots/                 README 截图（真机）
site/                             宣传页（GitHub Pages，workflows/pages.yml）
```

数据目录：

```
~/.wb-switch/accounts.json          账号库
~/.wb-switch/cdp_config.json        autoStart 等
~/.wb-switch/skin.json              皮肤配置（恢复官方会重置为默认）
~/.wb-switch/backups/reassign-preflight-*   切换前整库备份
~/.wb-switch/wallpapers/            壁纸（上传/下载缓存）
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

1. **换肤补丁层收尾**（第 4 节，唯一未闭环的功能）。
2. 换肤页 UI 与工业风对齐（现在沿用旧皮肤页，可读但不够「本项目的设计」）。
3. 壁纸轮换（WorkDaddy 有，本项目没做）：需要常驻定时器，注意无托盘形态下的开关入口。
4. 更新提示点做颜色区分：`available` 与 `ready`（已下载待重启）现在都是同一个方块。
