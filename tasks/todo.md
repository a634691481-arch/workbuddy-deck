# 任务：新建「WorkBuddy 任务助手」桌面应用（Tauri 2 + Vue 3 + shadcn 风格）

## 状态：🔄 进行中（2026-10-30 16:07 起）

## 目标
按用户给的原型图，在桌面重建一个独立应用：
- 位置：`C:\Users\63469\Desktop\workbuddy-task-assistant`
- 技术栈：Tauri 2 + Vue 3 + TS + Tailwind v4 + shadcn 风格组件（`cn()` + `cva`）
- 后端**不重写网络与存储**：直接 path 依赖 `../workbuddy-switch/crates/wb-switch-core`，
  复用账号库 / 签到 / 成长任务 / 旅行 / 令牌续期

## 界面结构（对齐原型）
1. 自绘标题栏（`decorations:false` + `data-tauri-drag-region` + 最小化/最大化/关闭）
2. 工具条：标题 + 登录账号 + 刷新全部
3. 统计行：总账号 / 排队 / 运行中 / 本轮成功 / 本轮失败 / 跳过 · 最近刷新
4. 左栏：搜索、账号列表（勾选）、全选/反选/已选/清空所选
5. 右栏：每日签到卡、页签（成长任务 / 原始响应）、任务表、操作条、日志面板
6. 底部状态：在线

## 关键设计
- **任务列表是动态的**：每个账号实时 `GET /v2/activity/growth/tasks`，不写死条目；
  官方改任务后界面自动跟随。表头 名称/状态/进度/奖励/操作 全部按接口字段渲染。
- 状态映射：`claimed`→已领奖、`completed`→已完成、`accepted`→进行中、其余→未接取。
- 「全部账号 / 所选账号」作用域：全部 = 传空数组（后端语义「空=全部」）。
- 原型里的「删除所选」改为**清空所选**：本版本没有删除账号能力，不能做个假按钮。

## 文件清单
```
src-tauri/
  Cargo.toml            tauri 2 + serde + wb-switch-core(path)
  build.rs / tauri.conf.json / capabilities/default.json / icons/*
  src/{main.rs, lib.rs, commands.rs}
src/
  main.ts, App.vue, style.css
  lib/{api.ts, utils.ts}
  components/{TitleBar.vue, ui/{Button.vue, Badge.vue}}
```

## 后端命令
| 命令 | 作用 |
| --- | --- |
| `list_accounts` | 账号摘要（不含令牌） |
| `account_detail` | 签到状态 + 该账号成长任务清单 |
| `checkin_now` | 单账号签到 |
| `run_checkin` / `run_growth` / `run_travel` | 批量执行（空数组=全部） |
| `run_daily` | 签到 → 成长任务 → 旅行 |

## 验证
- [x] `npm install` 完成
- [x] `npm run build`（vue-tsc + vite）通过：1578 模块，`index-CbXWNtnc.js` 125.73 kB
- [ ] `cargo check`（src-tauri）通过
- [ ] `npm run tauri build -- --no-bundle` 出 exe
- [ ] 启动后能看到本机账号与实时任务列表

## 回顾
- 待完成。

---

# 缺陷：切换账号「数据没同步过去」+ 目标账号会话被清空（2026-10-30 20:00）

## 现象
点账号卡「设为当前」→ 提示切换完成 → 目标账号一条会话都没多，原有会话全没了。

## 根因（在 `wb-switch-core`，不在本仓库）
「完全同步」= `switch_account(replace_sessions=true)`：先复制源账号会话、后物理删除目标账号旧会话。

复制侧 `resolve_target()` 命中关联组里**目标账号已有的有效成员** → 返回 `alreadyLinked`（不复制）；
而 `copy_succeeded_count()` 把 `alreadyLinked` 也算成功 → 删除照常执行 → 目标账号清空。

复现链（本机真实数据）：10-30 13:18 切 181→xiz（建立 72 组关联）→ 19:58 切回 xiz→181，
72 条源会话全部 alreadyLinked、0 条真复制，然后删掉 181 的 72 条 → 账号空。

## 修复
- [x] core `session.rs`：`CopyContext.reuse_existing_links`；替换式复制传 `false`，强制建新副本
- [x] core `copy_succeeded_count()`：只计 `copied`，`alreadyLinked` 不再算成功（杜绝"没复制也删"）
- [x] core 新增 2 个回归测试（只报链接不删、有有效关联也建新副本）→ 120 个 session 测试全过
- [x] 前端 `AccountsView.doSwitch()`：打印 复制 N / 删除 M；copied=0 时不报成功
- [x] `tasks/restore_sessions.py`：按关联组把副本正文还原回原 id，恢复 181 的 72 条会话

## 回顾
- 数据安全铁律：「算成功」的门槛必须是**真实写入**，不能是"目标侧看起来已经有了"。
- 破坏性动作（物理删除）前面必须有一道"本次确实写入了 N 条"的硬门槛，且 N 只统计本次新增。
- 前端不能只看 `replaced===false`；`copied===0` 同样是失败信号。


---

# 第 1 批：切换事务化 + 写后身份校验 + 切换历史（2026-10-30 21:45）

## 目标（闭环验收口径）
一次切换必须做到：**要么全部生效，要么完全还原**，且事后可查。

- [x] core `auth_file`：新增身份三态校验 `Effective / Mismatch / Unverified`
- [x] core `switch`：写后校验身份，不符自动回滚到切换前认证文件
- [x] core `switch`：替换式同步 `replaced=false` 时**不再写认证**（fail-closed，带数字报错）
- [x] core 新模块 `switch_history`：500 条滚动，含 from/to/模式/原因/结果/耗时/前后余额
- [x] core `mod.rs` 注册
- [x] `commands.rs`：`switch_workbuddy_account` 返回结构化 report；新增 `switch_history_list` / `switch_history_clear`
- [x] `lib.rs` 注册两条新命令
- [x] `api.ts`：ROUTES + 类型 + 包装（字段与后端一一对应）
- [x] `AccountsView.vue`：失败/回滚显式提示带数字；成功提示带耗时
- [x] `LogsView.vue`：切换来源作为第四源 TAB，可单独清空
- [x] i18n zh/en 全量补齐，无裸 key
- [x] 验证：`cargo check` + `cargo clippy -D warnings` + `vue-tsc` + `vite build`
- [x] AGENTS.md / README 能力表同步

## 设计要点
- 回滚用「切换前备份文件内容」而非备份路径，避免备份目录被清理后无法还原
- 身份校验读**写后的文件**，不读内存值：校验的就是磁盘真相
- 未定价/未验证一律不假装成功（三态里 `Unverified` 只在文件读不回来时出现，视为失败）
- 历史记录写失败不得阻断切换本身（降级为 eprintln）

## 回顾（2026-10-30 22:20 完成）

**改了什么**
| 层 | 文件 | 内容 |
| --- | --- | --- |
| core | `auth_file.rs` | `IdentityState` 三态 + `auth_identity_at` / `verify_identity` / `snapshot_auth_file` / `restore_auth_file`；写入函数内联 uid 校验 |
| core | `switch_history.rs`（新） | 500 条滚动台账，`record` / `load` / `clear`，损坏与版本不符拒绝覆盖 |
| core | `switch.rs` | 拆 `switch_account`（外层：快照+记历史）与 `_inner`（编排）；`SwitchTrace` 记录 stage；替换不完整 fail-closed；写后校验失败自动回滚 |
| core | `mod.rs` | 注册 `switch_history` |
| 命令 | `commands.rs` | 切换命令改为返回结构化（不抛 Err）；`switch_history_list` / `switch_history_clear`；`events_list` 加第四源；`system_info` 加台账统计 |
| 注册 | `lib.rs` | 两条新命令 |
| 前端 | `api.ts` | `SwitchResult` 扩字段；`SwitchHistory*` 类型 + 两个包装；`EventSource` 加 `switch` |
| 前端 | `AccountsView.vue` | 失败现场留弹框（阶段/身份/原文 + 跳日志）；成功带耗时与身份提示 |
| 前端 | `LogsView.vue` | 切换 TAB + 台账 4 格 + 分布条 |
| 前端 | `SettingsView.vue` | 数据规模加切换历史行、维护区加清空 |
| 文案 | `i18n.ts` | zh/en 各 18 个新 key |

**验证证据**
- `cargo check -p wb-switch-core` 通过
- `cargo test -p wb-switch-core --lib`：825 passed / 4 failed，失败项全是 `process::tests::windows_registry_probe_*`（需 `reg.exe`，被本机安全策略拦截，与本次改动无关）
- 新增单测 11 条：`switch_history` 7 条 + `auth_file` 4 条，全通过
- `cargo clippy -- -D warnings`（task-assistant）通过（顺手修掉 `scheduler.rs` 一处冗余 cast）
- `vue-tsc --noEmit` 通过；`vite build` 通过（`index-*.js` 279.28 kB）

**测试抓到的真 bug**：`switch_history::record_at` 原本用 `load_at().unwrap_or_default()`，
存档损坏时会把损坏文件覆盖成「只剩一条」。已改为 `load_at(root)?` 拒绝写入。

**未做（明确留给下一批）**：P0-2 用量预测、P0-3 成本折算。

---

# 第 2 批：用量预测 + Token 环比（2026-10-30 22:5x）

## 做了什么

| 层 | 文件 | 内容 |
| --- | --- | --- |
| core | `modules/forecast.rs`（新） | `AccountForecast` + `account_forecasts`；中位数与 EWMA 取小；只算余额下降；跨天归一；样本 <3 天给 `insufficient`；`lastDelta` 兜冷启动 |
| core | `modules/mod.rs` | 注册 `forecast` |
| 命令 | `commands.rs` | `account_forecast(windowDays)` |
| 注册 | `lib.rs` | 一行 |
| 前端 | `api.ts` | `AccountForecastRow` / `AccountForecastList` / `accountForecast` |
| 前端 | `AccountsView.vue` | 余额行追加 `≈2.4 天`（不占高度）；三态文案；hover 详情（日均/窗口/耗尽日/上次变动） |
| 前端 | `TokensView.vue` | 日均格加环比 `▲12.3% 较前 7 天`，<14 天不显示 |
| 文案 | `i18n.ts` | zh/en 各 9 key |

## 关键决策（都写进 AGENTS.md 了）

1. **不做 ¥ 成本折算**：WorkBuddy 模型别名（`deepseek-v4-flash` / `glm-5.3` / `kimi-k3`…）
   没有公开单价，照抄竞品价目表只能编数据。改为「积分/天 + 样本天数」，全部可实测。
2. **样本不足不预测**：真实数据当前 `basisDays=1`，全部 `insufficient`。这正确——
   给「还能撑 0.4 天」比如实说「样本 1/3 天」更糟。冷启动靠 `lastDelta` 顶上。
3. **取小不取大**：`min(median, ewma)` 偏保守，宁可早预警。

## 验证

- `cargo test -p wb-switch-core --lib forecast::` → 5 passed
- `cargo clippy -- -D warnings`（两端）通过
- `vue-tsc --noEmit` 通过；`vite build` 通过（282.83 kB）
- 真实快照跑通：14 个账号全部 `insufficient`（basisDays=1），`lastDelta` 有值

## 未做

- 配额窗口建模（5h/7d/包/限流四窗口分开）——下一批
- 账号健康评分、导出 CSV、托盘读数

---

# 第 3 批：账号健康分（2026-10-30 23:0x）

## 做了什么

| 层 | 文件 | 内容 |
| --- | --- | --- |
| core | `modules/health.rs`（新） | `HealthInput` + `account_health`：0-100 分 + `good/warn/critical` + 原因码 |
| core | `modules/forecast.rs` | `forecast_account` → `forecast_from(snapshots, ...)`：一批账号共用一次快照读取，避免 O(N) 次磁盘 IO |
| 命令 | `commands.rs` | `account_health_of(row, rateLimits, now, forecasts)`，本地/官方两分支都挂 `health` |
| 前端 | `api.ts` | `AccountHealth` + `AccountOverviewRow.health` |
| 前端 | `AccountsView.vue` | 名称前 1.5px 健康点（绿/灰白/红），hover 显示分数与原因 |
| 文案 | `i18n.ts` | zh/en 各 7 个原因码文案 |

## 主动砍掉的东西（重要）

原计划做「配额窗口统一建模 `QuotaWindow`」。写完之后核实：
- WorkBuddy **不提供** 5h / 7d 窗口字段，只能做「包到期 + 企业周期 + 限流恢复」三类；
- 这三类在卡片上**已经都有 UI**（到期时间、企业配额当普通包、限流徽标 + `28m 后恢复`）；
- 再抽一层窗口模型 = 把同一批数据换个形状，前端还得拼回去。

结论：`quota.rs` 写完后自我删除，只保留 `health.rs`。教训写进 AGENTS.md，
避免下一轮又提「配额窗口」。

## 验证

- `cargo test -p wb-switch-core --lib` → 837 passed / 4 failed（`windows_registry_probe_*`，需 `reg.exe`，被本机黑名单拦，与本次改动无关）
- 新增 `health` 单测 4 条全通过；`forecast` 重构后 5 条仍通过
- `cargo clippy -- -D warnings`（core + 应用）通过；`vue-tsc` / `vite build` 通过
- 单测抓到：余额剩 5% 只扣 20 分 → 80 分仍判 good（等于没报警）→ 改扣 30

## 下一批

- 导出 CSV / JSON（Token + 积分 + 账号三份）
- 托盘配额读数
- 限流驱动自动切换（高风险，需 opt-in）

---

# 第 4 批：数据导出 + 托盘读数（2026-10-30 23:0x）

## 做了什么

| 层 | 文件 | 内容 |
| --- | --- | --- |
| 后端 | `src-tauri/src/export.rs`（新） | 四份 CSV：accounts / tokensDaily / tokensModels / credits；BOM、CSV 转义、后端直写 |
| 后端 | `src-tauri/src/tray.rs`（新） | tooltip「账号数 · 合计余额 · 最紧账号 ≈N 天」；`group_thousands` |
| 命令 | `commands.rs` | `export_data(kind, path)` / `export_all_data(dir)`；`account_overview` 加 `app` 参数并在末尾刷托盘 |
| 启动 | `lib.rs` | 托盘 build 后立刻刷一次读数 |
| 前端 | `api.ts` | `ExportKind` / `ExportResult` / `ExportAllResult` / `exportData` / `exportAllData` |
| 前端 | `SettingsView.vue` | 「数据导出」区：4 个单份按钮 + 「全部到目录」+ 结果行（成功绿/失败红） |
| 文案 | `i18n.ts` | zh/en 各 10 key |

## 验收（真实数据）

`cargo test --lib manual_export_smoke -- --ignored --nocapture`：

| 文件 | 行数 | 抽样 |
| --- | --- | --- |
| accounts.csv | 9（8 账号） | `...,181****6649,cn,,4671.83,6715,30.4,100,good,,208.3,22.4,2026-11-22 09:10:45` |
| tokensDaily.csv | 70 | `2026-05-18,689077,636733,3839,0,48505,12` |
| tokensModels.csv | 22 | `deepseek-v4.1-flash,4472708312,...,79.24` |
| credits.csv | 5001 | `2026-10-27 18:15:46,2026-10-27,...,6044,5022.89,` |

## 更正上一批的一个错误结论

第 2 批结尾我写「14 个账号全部 insufficient（basisDays=1）」——**这是错的**。
当时用 `tail -60` 看预测输出，只截到了最后几个新账号就下了全局结论。
本次导出证明：老账号快照跨多天，`daysLeft` 已能给出 22.4 / 254.9 天等真值。

**教训：用被截断的样本下全局判断，比不下判断更糟。** 涉及"全体"的结论必须统计全量计数。

## 验证

- 应用 `cargo test` → 5 passed（export 4 + tray 1）
- `cargo clippy -D warnings` 通过（顺手把 `% 3 == 0` 改成 `is_multiple_of(3)`）
- `vue-tsc --noEmit` 通过；`vite build` 通过（287.35 kB）

## 未做

- 限流驱动自动切换（高风险，需 opt-in + 冷却 + 会话续接，单独一批）

---

# 第 5 批：限流自动切换（2026-10-30 23:1x）

## 做了什么

| 层 | 文件 | 内容 |
| --- | --- | --- |
| core | `modules/auto_switch.rs`（新） | 配置/日志/状态三份存档；`decide` 纯函数；`run_once` 注入式执行 |
| 命令 | `commands.rs` | `auto_switch_status/set/logs/clear_logs/check` + `spawn_auto_switch`（2 分钟巡检） |
| 启动 | `lib.rs` | 注册 5 条命令 + 启动巡检 |
| 前端 | `api.ts` | 5 个类型 + 5 个包装 |
| 前端 | `SettingsView.vue` | 「限流自动切换」区块：开关 / 仅提示-自动执行 / 冷却 / 最低余额 / 立即检查 / 最近判定 |
| 文案 | `i18n.ts` | zh/en 各 15 key |

## 三条安全默认（写进 AGENTS.md）

1. `enabled = false`：装了就开等于给自己埋雷。
2. `notify_only = true`：自动切换会**关闭并重启 WorkBuddy**，正在编辑的内容可能丢；先让用户看到「想切、切到哪」。
3. 冷却 30 分钟：多个模型连续限流时防止连环切换。

另外自动执行走 **plain 切换**（`replace_sessions=false`）：自动流程绝不碰会话数据，搬会话留给用户手动确认。

## 判定顺序（不可调换）

开关 → 当前账号是否真被限流（唯一触发条件）→ 冷却 → 候选筛选（同档位/未限流/余额达标/不在排除列表）→ 模式。

拿不到当前账号一律 Skip：宁可不动，也不能把用户刚切好的号又换掉。

## 真实数据验证

临时 example 跑本机：

```
enabled=false notifyOnly=true cooldown=30m
current = Some("12cc6919-...")          // 与 auth 文件匹配上的 181****6649
8 个候选，余额 1914 ~ 5102，均无限流
decision(forced-on) = Skip("当前账号未被限流")
```

## 验证

- core `cargo test --lib` → 848 passed / 4 failed（`reg.exe` 被黑名单拦，与改动无关）
- 新增 `auto_switch` 单测 11 条全通过（禁用/未限流/选最富/仅提示/跳过受限目标/跨档位/低余额/排除/冷却/未知当前账号/配置缺字段兜底）
- `cargo clippy -D warnings` 两端通过；`vue-tsc` / `vite build` 通过（292.86 kB）


---

# 第 6 批：设置页开源地址 + 双语 README（2026-10-30 23:3x）

## 设置页开源地址

| 层 | 改动 |
| --- | --- |
| 后端 | `commands.rs::open_external(url)`：**只放行 http/https**（命令收前端任意字符串，不校验等于把 file:// 交给页面） |
| 注册 | `lib.rs` 一行 |
| 前端 | `api.ts::openExternal`；设置页「应用信息」新增「开源地址」行，显示 `a634691481-arch/workbuddy-deck`，点击用系统浏览器打开 |

按用户要求**只展示本仓库**，不展示核心库地址。

## 双语 README

| 文件 | 行数 | 说明 |
| --- | --- | --- |
| `README.md` | 158 | 中文，引用中文界面截图 |
| `README_EN.md` | 158 | 英文（重写非直译），引用英文界面截图 `-en.png` |

新增/更新内容：
- 「六个分区」表补上预测、健康点、7 日环比、切换台账、数据导出；
- 「真正不同的六点」替换原四点：切号硬门槛（含写后 uid 校验）、天数预测（样本不足不猜）、限流归因台账、健康分扣分表、CSV 导出、托盘读数；
- 架构图补 `export.rs` / `tray.rs` / 领域能力 crate；
- 已知限制补「预测需 3 天快照」；License 段维持「无 LICENSE 不承诺」口径。

## 截图

用户提供的 12 张实拍（1680x1170）转 PNG：
- 中文：`01-accounts` / `02-tasks` / `03-credits` / `04-tokens` / `05-logs` / `06-settings`
- 英文：同名 `-en` 后缀
- 旧图（1178x837 与脚本抓的）已全部覆盖

## 校验

`validate_readme_pair.py` 的语言互链、同步标记、代码围栏、关键 Token 全部通过；
唯一 FAIL 是「两侧图片 target 集合不一致」——中英各用本语言界面截图，属有意设计，已写进 AGENTS.md。

---

# 任务：自动更新闭环（GitHub Releases + Tauri Updater）

## 状态：✅ 已实现（2026-10-31 00:10）

## 目标
`发版 → CI 构建签名 → Release → 客户端检查 → 下载校验 → 覆盖安装 → 重启 → 留痕` 全自动，
失败路径可解释、不阻断启动、不静默放行。

## 闭环六段

| 段 | 触发 | 实现 | 失败兜底 |
| --- | --- | --- | --- |
| 1 发版 | 本地 `npm run bump 0.1.1` + `git tag v0.1.1 && git push --tags` | `scripts/bump.mjs` 同步 package.json / Cargo.toml / tauri.conf.json | 版本不一致时脚本报错退出 |
| 2 CI | tag `v*` / 手动 dispatch | `.github/workflows/release.yml`：windows-latest → npm ci → tauri build --bundles nsis → 签名 → latest.json → Release | 构建失败不发 Release（draft 保留） |
| 3 检查 | 启动后 8s、每 6h、设置页手动、托盘菜单 | `update.rs`：GET `releases/latest/download/latest.json`（不走 API，无限流）；支持镜像前缀 | 3 端点依次退让 → 6h 缓存 → 错误码上屏，不阻断启动 |
| 4 比对 | 拿到 manifest | semver 逐段比较，`latest > current` 才算有更新 | 本地版本口径统一 `env!("CARGO_PKG_VERSION")` |
| 5 安装 | 用户点「下载并安装」 | `tauri-plugin-updater` `download_and_install` + 进度事件 | 签名校验失败即中止；下载失败可重试 |
| 6 留痕 | 检查/安装结果 | `notifications::record` → 日志页「通知存档」 | 写入失败不影响更新结果 |

## 必须同时修的既有缺陷
`commands.rs:1399` `system_info` 返回 `wb_switch_core::...::APP_VERSION` = core 的 **0.1.62**，
而本应用 `tauri.conf.json` 是 **0.1.0**。更新比对若用这个值会永远认为自己最新（或被误判）。
改为本应用 `env!("CARGO_PKG_VERSION")`，并让 core 只提供工具函数。

## 改动清单（8 项）

1. `crates/wb-switch-core/` — vendored（2.8MB / 34 文件），`Cargo.toml` 改 `path = "crates/wb-switch-core"`。
   **CI 能编译的前提**：现在依赖 `../../workbuddy-switch/...`，GitHub runner 上不存在。
2. `src-tauri/Cargo.toml` — `+ tauri-plugin-updater = "2"`。
3. `src-tauri/tauri.conf.json` — `bundle.createUpdaterArtifacts: true`；`plugins.updater` 三件：
   `pubkey`、`endpoints: [releases/latest/download/latest.json]`、`windows.installMode: passive`。
4. `src-tauri/src/update.rs`（新，≤300 行）— `update_check(force)` / `update_install()` /
   manifest 解析 / semver 比较 / 镜像前缀 / 6h 缓存 / 进度事件 `update://progress`。
5. `src-tauri/src/lib.rs` — 注册 plugin、两个 command、启动延迟检查、托盘「检查更新」项。
6. `src-tauri/src/commands.rs` — `system_info` 版本修正；新增 `update_config_get/set`（镜像、自动检查开关）。
7. 前端 — `lib/api.ts` 类型、`composables/useUpdate.ts`、`SettingsView.vue` 更新卡
   （当前版本 / 最新版本 / 检查按钮 / 下载进度 / 重启安装）、`TitleBar.vue` 有新版本小圆点、i18n（`up.*`）。
8. `.github/workflows/release.yml`、`scripts/bump.mjs`、README 增加「自动更新」段与发版步骤。

## 验收（逐条可验）
- [x] 版本口径统一：`system_info` 改本应用版本；标题栏 `REV` 由硬编码 0.1.62 改为 `update_config_get` 返回值
- [x] `npm run verify` 全绿（vue-tsc 0 错误 / clippy 0 警告）
- [x] 单测 4 条通过：semver 多位段比较、镜像后缀归一、清单 URL 拼接顺序、**真实本地 HTTP 拉清单→解版本→比对**
- [x] 签名链验证：`tauri signer sign` 用私钥成功签出 `.exe.sig`，与 `tauri.conf.json` 公钥配对
- [x] 常驻链验证：release 版启动后 `update.log` 出现 `poller start` → 8s 后 `check ok=… current=0.1.0 mirror=…`
- [ ] 打 tag 后 Actions 绿、Release 含 `*_x64-setup.exe` / `.sig` / `latest.json`（待首次发版）
- [ ] 覆盖安装后进程自启、`REV` 变新版本（需真实 Release 才能验，本机 NSIS 工具链下载被墙）
- [ ] 篡改清单 url → 签名失败中止（同上，需真实 Release）

## 实施记录
- vendored `crates/wb-switch-core`（2.8MB / 58 文件）+ 补 `scripts/codebuddy-cli-helper.cjs`（core 用 `include_str!` 内嵌，缺了直接编译失败）
- 新增 `update.rs`（检查/manifest 解析/镜像/缓存/安装/轮询/落地日志）、4 个命令、托盘「检查更新」、设置页更新卡、标题栏更新角标
- 已知取舍：GUI 进程没有控制台，`eprintln!` 写不出去 → 更新链路写 `~/.wb-switch/update.log`（>128KB 自动重建）
- 环境限制：本机沙箱内跨进程 loopback HTTP 不通（curl 也 000），故端到端「本地清单」只能靠进程内单测证明

## 需用户操作（无法代做）
1. 仓库 Settings → Secrets and variables → Actions：新增 `TAURI_SIGNING_PRIVATE_KEY`、
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`（我生成后给出内容，私钥不入库）。
2. 首次 Release 由 CI 创建；之后每次发版只需 `bump + tag + push`。

## 已知风险
- CI 首次编译含 `rusqlite(bundled)` + `windows` crate，约 15–20 分钟；之后有缓存。
- 国内直连 GitHub Release 可能慢：内置镜像前缀开关（默认空），填 `https://ghproxy.net/` 一类前缀即可。
- 自动更新会关闭进程覆盖安装；默认**只在用户点确认后安装**，不静默后台装。

---

# 新任务：一键清理 WorkBuddy 归档任务（2026-10-31）

## 现状（已核实）
- 归档任务 = `~/.workbuddy/workbuddy.db` → `sessions` 表 `status='archived'`，当前 **309 条**
- 正文 = `~/.workbuddy/projects/<cwd编码>/<sessionId>.jsonl`，合计 **750.2 MB**
- 次要残留：file-history / changes-detail / traces 内可能有关联文件（占比小，首版不动）

## 方案（按钮放 06 系统设置 · 数据规模卡）
- [ ] 后端 `purge.rs`：命令 `archived_tasks_stats`（条数+字节，只读）与
      `purge_archived_tasks`（确认后执行：删 309 个 jsonl → 删 sessions 表 archived 行 → VACUUM）
- [ ] 安全护栏：① WorkBuddy 进程运行中 → 拒绝执行（防 DB 回写覆盖）；② 执行前把待删
      sessionId 清单与 DB 备份落盘（`tasks/restore-backup/` 同目录约定）；③ 返回 {deleted, freedBytes}
- [ ] 前端 `SettingsView` 数据规模卡新增一行：「归档任务 N 条 · X MB [清理]」危险红描边按钮
- [ ] 点击 → 确认弹框（复刻切换账号警示结构）：列出条数/大小/不可恢复提示
- [ ] i18n（`pz.*` 前缀，先 grep 占用）
- [ ] `npm run verify` 全绿

---

# 任务：启动动画（exe 启动时播放）

## 状态：✅ 已完成（2026-10-31 08:45）

- 产物：`public/splash.html`（2.8s 时间轴，纯 CSS/JS，无外部依赖）+ `src-tauri/src/splash.rs`
- 时序：窗口创建 → WebView2 冷启动（约 0.6~1.5s）→ 顶部红线展开 → 标题/读数逐行 → 进度格 24 段 → `ready` → 淡出
- 切换条件：`splash_done`（页面自报）∧ `app_ready`（主窗首帧），10s 兜底
- 实测（release，全屏连拍）：1200ms 仅头标 / 2000ms 读数 04/06 + 进度约三成 / 2800ms 三行齐 + 进度满 /
  ~3.1s 切主窗 / 3600ms 启动窗淡出中 / 4400ms 只剩主界面

## 调试中修掉的两个真实缺陷
1. **全局 `CloseRequested` 拦截**：所有窗口都被 `prevent_close` 挡住，`splash.close()` 无效 →
   主界面已显示、启动窗仍压在上面（3600ms 截图可见重叠）。改为只拦 `label=="main"`，启动窗用 `destroy()`。
2. **按窗口创建时刻定时**：WebView2 冷启动晚 0.6~1.5s，2.6s 固定倒计时会把动画切半截。
   改为启动页自报 `splash_done`，两端都到齐才切。

## 验收
- [x] release 构建后启动可见完整动画，无白屏、无重叠残留
- [x] 动画三行读数依次出现，末态 `06 / 06` + `synced` + `ready`
- [x] 主界面出现后启动窗消失（4400ms 无残留）
- [ ] 打包版（NSIS）复验（本机 NSIS 工具链下载受限，待 CI 产物验证）
