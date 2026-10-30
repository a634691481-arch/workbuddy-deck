# AGENTS.md — 工程约定（WorkBuddy 任务助手）

> 本文件是**可执行约定**，不是宣传语。新增/修改代码前先读；提交前按第 8 节清单自检。
> 与 `tasks/ux-review.md`（体验）、`tasks/todo.md`（进度）配合使用。

## 0. 分层与依赖方向

```
前端  src/components/ui/*      纯展示原子组件（无业务、无 invoke）
      src/components/*         业务组件（只接 props / emit，不直接调后端）
      src/composables/*.ts     编排层：状态 / 副作用 / 轮询（唯一允许使用 lib/api 的地方）
      src/App.vue              渲染层：只做布局与事件绑定，不直接 invoke
      src/lib/api.ts           IPC 契约（类型 + invoke 封装，唯一出口）
后端  src-tauri/src/commands.rs 薄命令层：参数校验 + 转调，不含业务编排
      src-tauri/src/executor.rs 批量执行器：进度快照 / 取消 / 结果归类
      wb-switch-core            领域能力（账号、签到、成长任务、旅行、令牌）
```

## 0.1 视觉体系与布局（工业遥测风，改前先读）

**风格原型：Tactical Telemetry / CRT（深色），不与 Swiss Print 浅色混用。**

铁律（违反其一整块就不成立）：
1. **圆角恒为 0**（`style.css` 已全局 `border-radius: 0 !important`）。
2. **强调色只有航空红 `--color-hazard`**；磷光绿 `--color-success` 只允许出现在 READY 指示器。
3. 不用渐变 / 柔光投影 / 半透明玻璃；层次靠 1px 实线分隔。
4. 数据一律「等宽 + 大写 + 正字距」，中文模式用短词保持同样节奏。
5. 分隔线优先用 `grid gap-px` + 父子异色（skill §8），不逐条写 border。

```
App.vue = 外壳：TitleBar + [AppSidebar | KeepAlive 分区视图]
AppSidebar 148px  编号导航（01 账号 / 02 任务 / 03 积分 / 04 TOKEN / 05 日志 / 06 系统），
                  选中项 = 3px 航空红竖条；底部 READY（磷光绿）+ ACCTS
                  默认分区 = accounts（先有账号才谈任务与统计）
─────────────────────────────────────────────────────────────
01 AccountsView  账号管理（唯一账号入口：登录 / 导入 / 导出都只在这里）
                 卡片流 grid 1/2/3 列 + gap-2.5（真实间距，卡片带 1px 描边）
                 卡内：序号+名称(mask)+档位+徽标（当前账号/建议优先/模型限流**全部内联名称行**，
                       不占额外卡片高度）/ 余额·额度 / 积分包数·更新时刻
                       / 近期到期前 2 条 + 「查看全部 N」弹框（Teleport）/ 备注 / 删除
02 TasksView（原六段布局，状态在 useAssistant 等 composables）
TitleBar    36px  纯黑 + 底部 2px 危险红，右侧 [中|EN] 语言切换 + 窗控
AppToolbar  40px  [账号操作] + 运行状态 / 进度条 + 刷新（账号增删入口已移出本页）
├ AccountList        240–380px 可拖，bg-panel，含序号 01/02 + 方形勾选
│ └ AccountHeader    40px  单元标识 · 签到态 · 连签 · 今日可得 · 签到按钮
│   ├ TelemetryBar   约 56px  6 格读数：账号 / 签到 / 任务 / **积分总额** / 当前号 / 上轮
│   ├ 失败聚合条      仅失败时出现（红色 + 危险条纹）
│   ├ TaskTable      flex-1（页签 32px + 粘性表头 + 行 34px）
│   └ ActionBar      40px  作用域 + 4 动作 + 中断
LogPanel    24px 折叠头 / 展开 144px（默认展开）
StatusBar   24px  ● READY · 最近刷新 · 账号数 · 日志数
03 CreditsView   积分统计：4 指标 → 每日柱+账号余额 → 分类维度（模型/客户端/账号 三态切换）
                 → 积分明细（TAB：账号 / 积分流水，流水按时间·消耗排序并分页）
                 官方用量 available 时优先，unavailable 回退本地快照口径；
                 右侧副卡 = 互补维度（左列是客户端时看模型，其余看客户端）
04 TokensView    TOKEN 统计：8 指标（两行）→ 消耗构成条 → 数据源 → 每日柱+模型
                 → 项目分布+周×时热力 → 5 格效率指标 → TAB（会话 TOP / 调用明细）
05 LogsView      运行日志（i18n 前缀 rl.*，lg.* 归任务页 LogPanel）：四源合一
                 （notifications.json / auto_daily_logs.json / switch_history.json / error.log）
                 4 指标（总/今日/告警/错误）→ 来源分布条 + 24H 分布
                 → [切换源] 台账 4 格（次数/成功/已回滚/已中止 + 均耗）
                 → 来源 TAB + 关键词搜索 → 时间轴表（时间·来源·级别·标题·详情）
                 清空按源，不提供「全部清空」
06 SettingsView  系统设置：应用信息（版本/平台/数据目录+打开）/ 数据规模（行数·体积·存在性）
                 / 显示偏好（脱敏·语言）/ 自动化摘要（开关·时间·上次执行·跳任务面板）
                 / 维护（按源清空日志）
```

**约束**：
- 视图放 `src/views/`，每个视图自闭环（自己取数、自己处理动作），外壳不掺业务。
- 视图间共享的展示状态（账号数 / 运行态）走 `useShell.ts` 模块级 ref，由数据持有方回写。
- 各视图主列表区用 `flex-1 + overflow-y-auto`，其余定高；不要在视图中再造全局横条。
- **不新增全宽横条**：要加信息，先并进 `AppToolbar` 或 `TelemetryBar` 的格子。
- 同一信息只出现一次（签到态在单元条，余额在遥测带，不重复）。
- 浏览器预览可用 hash 直达分区：`#tasks` / `#credits` / `#tokens` / `#accounts` / `#logs` / `#settings`。
- **运行日志四源**：`events_list` 合并通知存档 + 自动日常日志 + **切换历史** + 错误日志（时间倒序，默认 300 条）；
  `events_clear(source)` 只删单源。任务执行结束时由 executor 写一条通知存档，
  保证「跑完 → 日志里能查到」闭环（原来只弹 toast，关掉即失忆）。
  切换源带结构化前后对照（谁→谁 / 复制几条 / 耗时 / 身份校验），是其余三源缺的那层；
  `filter === "switch"` 时另拉 `switch_history_list` 显示台账（总/成功/回滚/中止 + 均耗）。
- **i18n 前缀占用表**：`lg.*` 归任务页 LogPanel（历史），运行日志用 `rl.*`；新增页面前先 grep 前缀。
- 跨分区跳转统一走 `useShell().go(key)`（`nav` ref 由外壳持有）；组件不要自己改 hash。
- **账号的增 / 删 / 导入 / 导出只有账号管理页有入口**；其它页只做「带路」，不重复放按钮。
- 账号卡片数据走 `account_overview`：`refresh=false` 读本地快照（离线秒开），
  `refresh=true` 并发 4 路拉官方资源包；页面先出本地结果再后台补官方，失败不回滚本地。
- **当前 WorkBuddy 账号**：`workbuddy_status` 读 auth 文件（cn/ai 两档位）与账号库按
  uid/email 匹配，返回 `currentAccountIds`；命中卡片红色描边 + 「当前账号」徽标。
- **切换账号 = `switch_workbuddy_account`**：完全同步模式（replace_sessions=true），
  复用 core `switch::switch_account`（备份→关 WB→替换会话→写认证→**写后身份校验**→重启）；
  进度经 `switch-progress` 事件推送，确认弹框复刻原项目警示结构（物理删除不可恢复）。
  **该命令不抛 Err**：失败返回 `{ ok:false, result, stage, identity, rolledBack, error }`，
  前端必须显式判 `ok`。三态语义：`blocked`（未动认证）/ `rolled_back`（已还原认证）/ `ok`。
  失败现场留在弹框里（阶段 + 身份校验 + 原文），不跟 toast 一起消失。
- **切换的硬约束（勿删）**：① 会话替换 `replaced !== true` 时 fail-closed，不写认证；
  ② 写后读磁盘校验 uid，`identity-mismatch` 必须回滚；③ 历史写入失败不得影响切换结果。
- **用量预测 = `account_forecast`**（core `modules/forecast.rs`，纯本地、不发请求）：
  卡片余额后跟 `≈2.4 天`，<3 天红、<7 天高亮正文色。三条口径都别改：
  ① 只把**余额下降**算消耗——签到/领奖会让余额上升，那不是负消耗；
  ② 快照间隔 >1 天要**按天归一**，不能把 3 天的量记在最后一天；
  ③ 样本 <3 天返回 `confidence=insufficient` + `daysLeft=null`，UI 显示「样本 1/3 天」，
  **不许猜**。冷启动期用 `lastDelta`（最近两条快照差）顶上，它只要 2 条就能显示。
- **Token 页环比**：近 7 天 vs 前 7 天，基于 `daily` 末尾 14 天；不足 14 天不显示，
  不拿短的比长的。用量上涨按风险着色（hazard 红），不是按股票口径。
- **账号健康分 = core `modules/health.rs`**：`account_overview` 两个分支都会算，
  挂 `row.health = { score, grade, reasons[] }`，卡片名称前一个 1.5px 圆点
  （good 绿 / warn 灰白 / critical 红），hover 看分数与全部原因。
  扣分表：限流 −35、余额为 0 −50、余额 <10% −30、拉取失败 −20、预测 <3 天 −25、
  快到期额度占多数 −10。**原因码由前端翻（`ac.hz*`），后端不返回中文。**
  改扣分标准前先想清楚：剩 5% 只掉到 80 分会被判成 good，等于没报警。
- **打开外部链接 = `open_external(url)`**：设置页「开源地址」用。**只放行 http/https**——
  这个命令接收前端传来的任意字符串，不校验就等于把 `file://`、自定义协议都交给页面。
- **README 截图按语言分开**：`docs/screenshots/NN-xxx.png` 是中文界面，
  `NN-xxx-en.png` 是英文界面；`README.md` 默认展示英文（引后者），
  中文版在 `README_ZH.md`（引前者）。
- **数据导出 = `export_data` / `export_all_data`**（`src-tauri/src/export.rs`）：
  四份 CSV —— `accounts` / `tokensDaily` / `tokensModels` / `credits`。三条约定：
  ① **后端直接写盘**，CSV 不回传前端（几 MB 过 IPC 没意义），前端只负责 `save`/`open` 拿路径；
  ② **写 UTF-8 BOM**，否则 Excel 双击打开中文乱码；
  ③ 只导出已有数据，**导出不触发官方刷新**（导出是快照动作）。
  字段全是原始数值（不写 `1.2万`），时间统一 `YYYY-MM-DD HH:MM:SS` 本地时区。
- **托盘读数 = `src-tauri/src/tray.rs`**：tooltip 显示「账号数 · 合计余额 · 最紧账号 ≈N 天」，
  启动时刷一次 + 每次 `account_overview` 后刷一次。预测样本不足时显示「预测样本不足」，
  **不编一个天数**。
- **限流自动切换 = core `modules/auto_switch.rs`**（与 `rotate.rs` 分工不同：rotate 按
  「积分到期紧迫度」主动轮换、要拉官方接口；这里按「限流事件」被动响应、**只读本地台账**）：
  每 2 分钟巡检，当前账号被限流 → 切到同档位、未限流、余额 ≥ `minRemaining` 的最富账号。
  **三个安全默认不能改**：`enabled=false`、`notify_only=true`（自动切换会关进程打断用户）、
  冷却 30 分钟。自动执行时的切换是 **plain**（`replace_sessions=false`）——自动流程绝不碰会话数据。
  拿不到当前账号一律 Skip，宁可不动也不能瞎切。
- **自动更新 = `src-tauri/src/update.rs` + `tauri-plugin-updater`**：检查与安装是两条独立路径，
  别合并。检查用 `reqwest` 直读 release 资产 `latest.json`（**不走 GitHub API**：60 次/小时/IP，
  多人共用出口 IP 会集体 403）；安装交给插件做 minisign 校验与 NSIS 覆盖。
  三条不能动的前提：
  ① **版本基线只认本应用 `env!("CARGO_PKG_VERSION")`**，不许再借 core 的 `APP_VERSION`
  （core 是 0.1.62，一比就错，标题栏曾因此硬编码显示 0.1.62）；
  ② 签名校验失败必须中止，不做「先装后补签」；
  ③ 安装只在用户点确认后发生，后台轮询**只提示不安装**。
  发版闭环：`npm run bump X.Y.Z --tag` → CI 构建签名 → Release → 客户端 8s 首查 / 每 6h 轮询。
  `bundle.createUpdaterArtifacts=true` 意味着**本地打安装包必须带 `TAURI_SIGNING_PRIVATE_KEY`**，
  只想要 exe 时用 `npm run tauri build -- --no-bundle`（不触发签名）。
- **额度窗口没有独立模型**（2026-10-30 核实后放弃）：WorkBuddy 官方**不提供** 5h / 7d
  这类窗口字段；积分包到期（含企业配额周期）走 `resources[].expireAt`，限流恢复走
  `rateLimits[].resetAt`，卡片上已分别有到期时间与「28m 后恢复」倒计时。
  再抽一层 `QuotaWindow` 只是把同一批数据换个形状，属于重复建模，别再提。

**硬规则**：依赖只能向下。`ui/*` 不得 import `lib/api`；`components/*` 不得直接 `invoke`；
所有 IPC 只能经由 `composables/*` → `lib/api.ts`，一个关注点一个 composable（`useAssistant` / `useBalance`）。

## 1. 可扩展（Extensibility）

- **新增一种批量动作**：只改三处 —— `executor.rs` 的 `ACTION` 常量与 `classify()` 分支、`commands.rs` 一个薄命令、`App.vue` 的按钮表（数据驱动，不写 if 链）。
- **新增一个展示字段**：加在后端 `summary()`/`detail()` 的 JSON 里，前端类型同步；不新增专用命令。
- **禁止**：为一个页面需求在后端开专用接口；把业务分支写进命令行层。
- 动作/状态用**枚举 + 集中映射**（`Outcome`、`statusOf()`），不用散落的字符串比较。新增状态码时改映射表一处。

## 2. 可维护（Maintainability）

- 单文件 **≤ 300 行**、单函数 **≤ 60 行**；超了就拆（`App.vue` 410 → 160 行，逻辑移入 composable）。
- **SFC 首行必须是 `<script setup lang="ts">`**：整文件覆写时最容易丢，丢了会把 script 当模板解析，
  报 `Attribute name cannot contain U+0022`。改完 `.vue` 先 `head -1` 自查。
- 命名表达意图：`submit()`、`classify()`、`runningLabel`；不用 `data`/`tmp`/`flag`。
- 注释只写**为什么**（约束、坑、反直觉处），不复述代码。每个文件头一句话说明职责。
- 无魔法值：超时、轮询间隔、宽度上下限、日志上限全部提为具名常量。
- **浏览器可预览**：`isTauri()`（`lib/tauri-env.ts`）守卫所有窗口 API，`npx vite --port 1421`
  直接开页面看布局；依赖 IPC 的数据会为空，仅用于排版调试，**不要为此在生产代码里留 mock 数据**。
- 同一概念只有一个名字（"账号"不混用 account/user/acc 于界面文案）。

## 3. 可观测（Observability）

- 后端：关键路径打结构化前缀日志 `[run] action=growth done=3/6 ok=2 fail=1`；**不打印令牌、手机号、邮箱全文**（脱敏为 `138****1234`）。
- 每轮执行记录：动作、账号数、耗时、成功/失败/跳过、取消原因。
- 前端：日志面板就是观测面，必须包含**时间 + 主体 + 结果 + 失败原因**四要素；不写"操作成功"这类无信息量的话。
- 失败必须可追溯：错误串带上「哪个账号 + 哪个阶段 + 服务端原话」。
- 三态齐备：加载（骨架）/ 空（区分"未选账号/无任务/读取失败"）/ 错误（可重试）。

## 4. 质量（Quality）

- 提交前必须全绿：`npm run verify`（= `vue-tsc --noEmit` + `cargo clippy -- -D warnings` + `cargo check`）。
- **零警告**：`cargo clippy -D warnings` 与 `tsc` 不允许遗留 warning。
- 类型不留 `any`；后端返回值一律显式 `interface`，`invoke<T>` 必填泛型。
- 逻辑分支必须有测试或可复现验证步骤；纯函数（`classify`/`statusOf`/`progressOf`）优先单测。
- 不为通过检查而 `#[allow]`/`@ts-ignore`；确需豁免必须写明原因与移除条件。

## 5. 安全（Security）

- **令牌永不出后端**：`summary()`/`detail()` 只回显 label/variant/note，不返回 access_token / refresh_token / uid。
- 日志与异常**禁止**出现令牌、完整手机号、邮箱；统一走脱敏函数。
- 前端不直连官方 API：所有请求经 core，便于统一加超时、重试、鉴权。
- 命令参数必须校验（空 id、未知 action、越界数字），非法输入返回明确 `Err`，不 panic。
- 外部进程仅白名单：`launch_main_app` 只允许启动 `wb-switch-rust.exe`，不接任意路径。
- 破坏性能力（删除会话/清空）默认不引入；必须引入时前端二次确认 + 后端可回滚 + 单独命令。

## 6. 性能（Performance）

- 轮询只在**运行中**开启（400ms），结束立即停表；空闲零请求。
- 列表 > 200 行时启用窗口化；当前账号列表按需渲染即可，但要留好切换点。
- 搜索输入 **debounce 150ms**，不在每次按键触发过滤重算（大数据量时）。
- 避免全量刷新：操作后只刷新受影响的账号；"刷新全部"是显式动作。
- 单账号串行执行（也降低风控风险），不并发打接口。

## 7. 健壮（Robustness）

- 后端：`Result<T, String>` 出口，错误带上下文；不 `unwrap()` 在请求/IO 路径上（`Mutex` 中毒用 `ok()` 兜底）。
- 单账号失败**不中断整轮**；取消标志在每个账号开始前检查，当前账号允许做完一个原子步骤。
- 并发互斥：`RUNNING` 用 `swap(true)` 保证只允许一轮；重复触发返回 `busy` 而不是排队。
- 前端：所有 `invoke` 包 `try/catch`，失败进日志 + toast，不能只 console。
- 状态机明确：`idle → running → done|cancelled`；按钮可用性由状态推导，不靠散落布尔。

## 8. 提交前清单（DoD）

- [ ] `npm run verify` 全绿（tsc 0 错误、clippy 0 警告）
- [ ] 新增动作已在 `classify()` 注册，统计三类计数正确
- [ ] 失败路径手动验证过（断网 / 无效账号 / 取消）
- [ ] 日志四要素齐全，无敏感信息
- [ ] 空/加载/错误三态都在
- [ ] 键盘可完成主流程（选账号 → 执行 → 取消）
- [ ] 无新增 `any` / `unwrap` / 硬编码色值 / 魔法数字
