# 项目目录结构

> 只描述**当前仓库**长什么样、东西该放哪。运行方式与硬约束见 `HANDOVER.md`。
> 最后更新：2026-11-25 15:10

---

## 1. 顶层

```
workbuddy-task-assistant/
├─ src/                 前端（Vue 3 + Vite）
├─ src-tauri/           Rust 宿主（Tauri 2）
├─ crates/              领域 crate（编译进 src-tauri，可单测）
├─ assets/              品牌与文档用静态资源（不进运行包）
├─ docs/                截图等文档资产
├─ site/                GitHub Pages 宣传页（self-contained 单页）
├─ scripts/             构建 / 发版 / 调试脚本
├─ tasks/               产品与体验笔记（非代码）
├─ .github/workflows/   CI：build、release、pages
├─ AGENTS.md            工程约定（改代码前必读）
├─ HANDOVER.md          交接文档（形态、约束、未完成项）
├─ README.md / _EN.md   仓库首页
└─ package.json         前端与打包脚本
```

生成物（**不入库**）：`node_modules/`、`dist/`、`src-tauri/target/`、`.hallmark/`、`*.log`。

---

## 2. 前端 `src/`

```
src/
├─ App.vue              外壳：TopNav + KeepAlive 分区视图 + 全局弹框（更新/切号确认）
├─ main.ts              入口：挂载、app_ready、嵌入面板 shim
├─ style.css            全局样式与 CSS 变量（工业遥测基线）
├─ views/               每个分区自闭环（自己取数、自己处理动作）
│  ├─ AccountsView.vue      01 账号
│  ├─ TasksView.vue         02 任务
│  ├─ CreditsView.vue       03 积分
│  ├─ TokensView.vue        04 TOKEN
│  ├─ LogsView.vue          05 日志（四源合一）
│  ├─ ArchivedView.vue      06 归档任务
│  ├─ AppearanceView.vue    本应用自身外观（不是 WorkBuddy 换肤）
│  └─ SettingsView.vue      系统设置
├─ components/
│  ├─ ui/               纯展示原子组件（无业务、无 invoke）
│  ├─ TopNav.vue        顶部导航（分区切换）
│  ├─ TitleBar.vue      窗口标题栏（仅桌面窗口态显示）
│  ├─ UpdateDialog.vue  更新弹框（下载进度 / 重启安装）
│  └─ …                 业务组件只接 props / emit
├─ composables/         编排层：状态、副作用、轮询（唯一允许调用 lib/api 的地方）
│  ├─ useAssistant.ts       任务执行状态机
│  ├─ useUpdate.ts          更新流程（模块级单例状态）
│  ├─ useShell.ts           分区导航 `go(key)`、账号数、运行态
│  ├─ useListAnim.ts        AutoAnimate 封装（列表增删）
│  └─ useStagger.ts         列表交错入场
├─ lib/
│  ├─ api.ts            **全部 IPC 契约**（类型 + invoke 封装，唯一出口）
│  ├─ i18n.ts           中英文案（前缀：nav.* st.* sk.* rl.* lg.* up.* cd.*）
│  ├─ nav.ts            NavKey 与导航图标
│  ├─ prefs.ts          本应用外观偏好（主题/字号/圆角/强调色/密度…）
│  └─ tauri-env.ts      是否 Tauri、是否嵌入面板
└─ assets/              图标、lordicon 动画等前端资源
```

规则：视图不直接 `invoke`；组件不写业务；跨分区跳转只走 `useShell().go`。

---

## 3. 后端 `src-tauri/src/`

```
src-tauri/
├─ tauri.conf.json      窗口（create:false）、bundle 图标、updater 公钥与端点
├─ build.rs             触发 Windows 资源重编（改图标后必须 touch）
├─ icons/               tauri icon 生成的各平台图标（icon.ico / icns / png）
├─ installer/           Inno Setup：workbuddy-deck.iss（向导、SrcDir 可覆盖）
└─ src/
   ├─ main.rs / lib.rs      启动：无窗口判断、单实例、启动器 preflight、调度
   ├─ commands.rs           薄命令层：参数校验 + 转调（switch_* / account_* / events_* / update_*）
   ├─ launcher.rs           启动器：注册表找客户端、运行中拦截、带 9222 拉起
   ├─ executor.rs           批量执行：互斥、进度快照、取消、结果归类
   ├─ scheduler.rs          每日定时：到点触发、写回执、错过补跑
   ├─ purge.rs              归档任务清理
   ├─ export.rs             四份 CSV 导出（UTF-8 BOM）
   ├─ preflight.rs          启动前置检查
   ├─ update.rs             更新检查/下载/安装（minisign 校验）
   ├─ alerts.rs             异常提醒轮询
   ├─ token_cache.rs        令牌缓存
   ├─ vibrancy.rs           窗口毛玻璃（原生 backdrop）
   └─ deck_http/            本地桥 127.0.0.1:17653
      ├─ mod.rs             路由：/__ipc /__events /__deck_shim.js /__deck_diag / 静态资源
      ├─ ipc.rs             命令分发（含嵌入面板的 cmd 白名单）
      ├─ events.rs          SSE：把 Tauri emit 转给面板
      ├─ asset.rs           面板资源来源解析（Embedded / dist / vite 反代）
      ├─ shim.rs            浏览器/iframe 环境下的 __TAURI_INTERNALS__ 兼容层
      ├─ boot.rs            启动期状态
      └─ proxy.rs           开发态 vite 反代
```

数据目录（运行时）：`~/.wb-switch/`（账号库、配置、备份、壁纸缓存），`~/.workbuddy/` 是官方客户端数据，只读排查用。

---

## 4. 领域层 `crates/wb-switch-core/src/modules/`

按能力分包，全部是纯 Rust、可单测：

| 组 | 模块 |
|---|---|
| 账号 | `account` `account_groups` `auth_file` `import/export` `oauth` `variant` |
| 会话 | `session` `session_backup` `session_groups` `session_link` |
| 切号 | `switch` `switch_history` `supervisor` `process` |
| 任务 | `checkin` `growth_tasks` `growth_center` `growth_center_crypto` `travel` `credit_claim` |
| 统计 | `credits` `credit_usage` `official_usage` `token_stats` `forecast` `health` `limits` `rate_limit_events` |
| 注入 | `cdp` `wb_runtime` `vscode_cn_inject` `vscode_ext` `vscode_session*` `jetbrains` `codebuddy_*` |
| 自动化 | `auto_switch` `rotate` `scheduler`（宿主） |
| 基础设施 | `config` `error_log` `notifications` `net_budget` `refresh` `update` `linux_keyring` |

约定：单账号串行执行；互斥优先于排队；令牌不出后端；预测只读本地快照。

---

## 5. 资源与文档

```
assets/
├─ logo/
│  ├─ app-icon.svg      应用图标母版（圆角底 + 白色方框 + 强调色角块）
│  ├─ app-icon.png      1024 渲染产物（`npx tauri icon` 的输入）
│  ├─ dock.svg          单色标记（注入悬浮图标用）
│  ├─ favicon.svg       站点图标
│  ├─ previews/         图标预览图（历史产物，仅供查看）
│  └─ renders/          logo 概念对照与 16px 检查图
docs/screenshots/       六分区真机截图 + feature-tour GIF（README 引用）
site/                   宣传页（index.html 自包含 + tokens/base + shots）
tasks/                  产品需求与体验笔记
```

规则：`assets/` 只放**源文件与预览**，运行期产物不要放进来；壁纸、日志、缓存一律落 `~/.wb-switch/`。

---

## 6. 脚本 `scripts/`

| 脚本 | 作用 |
|---|---|
| `bump.mjs` | 同步版本号到 package/Cargo/tauri.conf（不一致就报错） |
| `installer.mjs` | 调 ISCC 出 Inno Setup 安装包，支持 `--target=` 指定构建目录 |
| `deck.mjs` | 开发入口（headless 注入器） |
| `verify-panel.cjs` | 面板构建产物自检 |
| `gen_release_notes.py` | 生成 Release 说明 |
| `gen_cn_manifest.py` | 国内版清单 |
| `restore_deleted_sessions.py` / `restore_session_rows.py` | 从备份回放会话（事故恢复用） |
| `probe_wb_db.py` | 只读探测 workbuddy.db 结构 |
| `codebuddy-cli-helper.cjs` | CodeBuddy CLI 辅助 |

---

## 7. 常见误放（已经踩过）

| 东西 | 错放 | 正确位置 |
|---|---|---|
| 壁纸缓存（65MB） | 仓库根 `wallpapers/` | `~/.wb-switch/wallpapers/`（不入库） |
| 实验截图（probe/patch） | `assets/logo/renders/` | 用完即删，证据写进 `HANDOVER.md` |
| 构建产物 | 提交 `dist/`、`target/` | `.gitignore` 已挡 |
| 版本号 | 手改三处之一 | `npm run bump <ver>` |
| 图标 | 只换 `icons/` 不重编资源 | 换图后 `touch src-tauri/build.rs` |
