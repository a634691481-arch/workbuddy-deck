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

- [ ] core `auth_file`：新增身份三态校验 `Effective / Mismatch / Unverified`
- [ ] core `switch`：写后校验身份，不符自动回滚到切换前认证文件
- [ ] core `switch`：替换式同步 `replaced=false` 时**不再写认证**（fail-closed，带数字报错）
- [ ] core 新模块 `switch_history`：500 条滚动，含 from/to/模式/原因/结果/耗时/前后余额
- [ ] core `mod.rs` 注册
- [ ] `commands.rs`：`switch_workbuddy_account` 返回结构化 report；新增 `switch_history_list` / `switch_history_clear`
- [ ] `lib.rs` 注册两条新命令
- [ ] `api.ts`：ROUTES + 类型 + 包装（字段与后端一一对应）
- [ ] `AccountsView.vue`：失败/回滚显式提示带数字；成功提示带耗时
- [ ] `LogsView.vue`：切换来源作为第四源 TAB，可单独清空
- [ ] i18n zh/en 全量补齐，无裸 key
- [ ] 验证：`cargo check` + `cargo clippy -D warnings` + `vue-tsc` + `vite build`
- [ ] AGENTS.md / README 能力表同步

## 设计要点
- 回滚用「切换前备份文件内容」而非备份路径，避免备份目录被清理后无法还原
- 身份校验读**写后的文件**，不读内存值：校验的就是磁盘真相
- 未定价/未验证一律不假装成功（三态里 `Unverified` 只在文件读不回来时出现，视为失败）
- 历史记录写失败不得阻断切换本身（降级为 eprintln）
