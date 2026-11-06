import { invoke as tauriInvoke } from "@tauri-apps/api/core";

import { barEnd, barStart } from "@/lib/loading-bar";

/**
 * 所有 IPC 的唯一入口：统计在飞请求，供标题栏顶部加载条使用。
 * 命令实现零改动，48 处调用自动纳入统计。
 */
function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  barStart();
  return tauriInvoke<T>(cmd, args).finally(barEnd);
}

/** 账号摘要（不含任何令牌字段）。 */
export interface AccountSummary {
  id: string;
  label: string;
  variant: string;
  note?: string | null;
}

export interface AccountList {
  total: number;
  accounts: AccountSummary[];
}

/** 官方任务清单条目（字段名来自服务端，保持原样透传）。 */
export interface GrowthTask {
  task_code: string;
  title?: string;
  task_desc?: string;
  accept_status?: string;
  progress?: { current?: number; target?: number } | null;
  reward_credit?: number;
  reward_energy?: number;
  /** 后端标记的跳过原因：manual_required = 必须去客户端/官网手动完成 */
  skipped?: string | null;
  /** 官方入口（workbuddy:// 深链或 https 页面） */
  jumpUrl?: string | null;
}

export interface GrowthRow {
  accountId?: string;
  email?: string;
  total?: number;
  done?: number;
  claimable?: string[];
  tasks?: GrowthTask[];
}

/** 签到状态：`raw` 是服务端原始字段，放到「原始响应」页展示。 */
export interface CheckinStatus {
  ok?: boolean;
  todayCheckedIn?: boolean;
  statusUnsupported?: boolean;
  error?: string;
  raw?: Record<string, unknown>;
}

export interface AccountDetail {
  accountId: string;
  variant: string;
  checkin: CheckinStatus;
  growth: GrowthRow;
  today: string;
}

/** 积分余额（只保留交付字段）。 */
export interface Balance {
  ok: boolean;
  remaining?: number | null;
  capacity?: number | null;
  error?: string | null;
}

/** 全账号积分汇总。 */
export interface TotalBalance {
  ok: boolean;
  total: number;
  failed: number;
  accounts: {
    accountId: string;
    label: string;
    ok: boolean;
    remaining?: number | null;
    error?: string | null;
  }[];
}

/** 浏览器登录发起结果。 */
export interface LoginStart {
  ok: boolean;
  loginId?: string;
  url?: string;
  expiresIn?: number;
  browserOpened?: boolean;
  browserError?: string;
  error?: string;
}

/** 登录轮询结果。done=false 表示还没授权完；result 是入库后的账号 meta（含昵称）。 */
export interface LoginPoll {
  done: boolean;
  result?: {
    id?: string;
    uid?: string | null;
    nickname?: string | null;
    email?: string | null;
    enterpriseName?: string | null;
    variant?: string | null;
  } | null;
  error?: string | null;
}

/** 成长任务单条结果（后端 results[] 透传，skipped 表示「没做」及原因）。 */
export interface GrowthRunTask {
  code: string;
  status?: string;
  current?: number;
  target?: number;
  claimed?: boolean;
  credit?: number;
  energy?: number;
  error?: string;
  /** manual_skip（有意跳过）/ manual_required（需手动）/ tries_exhausted / no_recipe */
  skipped?: string;
  /** 官方任务标题 */
  title?: string | null;
  /** 官方一键入口：workbuddy:// 深链或 http(s) 活动页 */
  jumpUrl?: string | null;
}

export interface RunRow {
  accountId?: string | null;
  label?: string;
  result?: string;
  reason?: string;
  error?: string;
  claimed?: unknown[];
  claimedCount?: number;
  checkin?: string;
  travel?: string;
  /** growth 动作才有：逐任务结果（含没做原因） */
  tasks?: GrowthRunTask[];
}

/** 批量执行的可观察进度（界面轮询 run_status）。 */
export interface RunStatus {
  running: boolean;
  cancelled: boolean;
  action: string;
  total: number;
  done: number;
  ok: number;
  fail: number;
  skip: number;
  current: string;
}

export interface RunResult {
  /** done | cancelled | busy | unsupported */
  status?: string;
  accounts: RunRow[];
}

export const listAccounts = () => invoke<AccountList>("list_accounts");

/** 从本机 WorkBuddy 登录态导入当前账号（没装过 workbuddy-switch 的用户靠它入库）。 */
export interface ImportLocalResult {
  ok: boolean;
  imported: number;
  skipped: number;
  accounts: AccountSummary[];
  errors: Array<{ variant: string; error: string }>;
}

export const importLocalAccounts = () => invoke<ImportLocalResult>("import_local_accounts");

export const runStatus = () => invoke<RunStatus>("run_status");

export const cancelRun = () => invoke<{ cancelled: boolean }>("cancel_run");

export const launchMainApp = () =>
  invoke<{ started: boolean; path?: string; error?: string }>("launch_main_app");

export const accountDetail = (accountId: string) =>
  invoke<AccountDetail>("account_detail", { accountId });

export const totalBalance = () => invoke<TotalBalance>("total_balance");

export const startLogin = (variant?: string) =>
  invoke<LoginStart>("start_login", variant ? { variant } : {});

export const pollLogin = (loginId: string) => invoke<LoginPoll>("poll_login", { loginId });

export interface ExportResult {
  ok: boolean;
  path?: string;
  count?: number;
  error?: string;
}

export interface ImportResult {
  ok: boolean;
  imported?: number;
  skipped?: number;
  overwritten?: number;
  error?: string;
}

export const exportAccountsFile = (path: string, accountIds: string[] = []) =>
  invoke<ExportResult>("export_accounts_file", { path, accountIds });

export const previewImportFile = (path: string) =>
  invoke<{ ok: boolean; preview?: unknown; error?: string }>("preview_import_file", { path });

export const importAccountsFile = (path: string) =>
  invoke<ImportResult>("import_accounts_file", { path });

export const accountBalance = (accountId: string) =>
  invoke<Balance>("account_balance", { accountId });

export const checkinNow = (accountId: string) =>
  invoke<RunRow>("checkin_now", { accountId });

/** 空数组 = 全部账号。 */
export const runCheckin = (accountIds: string[] = []) =>
  invoke<RunResult>("run_checkin", { accountIds });

export const runGrowth = (accountIds: string[] = []) =>
  invoke<RunResult>("run_growth", { accountIds });

export const runTravel = (accountIds: string[] = []) =>
  invoke<RunResult>("run_travel", { accountIds });

/** 每日自动执行配置（存后端，跨会话生效）。 */
export interface AutoDailyConfig {
  enabled: boolean;
  time: string;
  lastRunDate: string;
}

/** 自动执行日志行（后端裁剪：500 条 / 30 天）。 */
export interface AutoDailyLog {
  ts: string;
  date: string;
  time?: string;
  status?: string;
  total?: number;
  ok?: number;
  other?: number;
}

export const autoDailyGet = () =>
  invoke<{ config: AutoDailyConfig; logs: AutoDailyLog[] }>("auto_daily_get");

export const autoDailySet = (enabled: boolean, time: string) =>
  invoke<{ ok: boolean; config?: AutoDailyConfig; error?: string }>("auto_daily_set", {
    enabled,
    time,
  });

// ─────────────────────────── 统计（积分 / Token） ───────────────────────────

export interface CreditDailyPoint {
  date: string;
  usage: number;
}

export interface CreditAccountRow {
  accountId: string;
  accountName?: string | null;
  variant?: string | null;
  isCurrent?: boolean;
  currentRemaining?: number | null;
  totalCapacity?: number | null;
  usageToday: number;
  usage7Days: number;
  usageThisMonth: number;
  checkinStatusToday?: string | null;
  lastSnapshotAt?: number | null;
  daily?: CreditDailyPoint[];
}

/** 官方用量里的模型聚合：credit 为消耗积分，requestCount 为调用次数。 */
export interface CreditOfficialModel {
  model: string;
  requestCount: number;
  credit: number;
}

export interface CreditOfficialDailyPoint extends CreditDailyPoint {
  models?: CreditOfficialModel[];
}

/** 官方请求流水的一条记录；字段来自官方只读接口，不含提示词正文。 */
export interface CreditOfficialRequest {
  accountId: string;
  accountName: string;
  requestId: string;
  credit: number;
  model: string;
  client: string;
  requestTime: string;
}

export interface CreditOfficialAccount {
  accountId: string;
  accountName: string;
  ok: boolean;
  requestCount: number;
  reportedTotal?: number | null;
  detailTruncated?: boolean;
  usageToday?: number | null;
  usage7Days?: number | null;
  usageThisMonth?: number | null;
  models?: CreditOfficialModel[];
  daily?: CreditOfficialDailyPoint[];
  error?: string | null;
}

/**
 * 官方积分用量。`status` 决定前端能否使用：
 * - complete / partial：可用（partial 表示部分账号失败，失败账号的数值为 null）
 * - unavailable：不可用，页面回退到本地快照口径
 */
export interface CreditOfficialUsage {
  status: "complete" | "partial" | "unavailable";
  rangeStart: string;
  rangeEnd: string;
  collectedAt: number;
  summary: { usageToday: number; usage7Days: number; usageThisMonth: number };
  daily: CreditOfficialDailyPoint[];
  accounts: CreditOfficialAccount[];
  requests: CreditOfficialRequest[];
  models: CreditOfficialModel[];
  detailLimitPerAccount: number;
  errors: { accountId: string; accountName: string; error: string }[];
}

export interface CreditStatistics {
  generatedAt: number;
  coverageStartAt?: number | null;
  summary: {
    currentRemaining: number;
    currentCapacity: number;
    usageToday: number;
    usage7Days: number;
    usageThisMonth: number;
    todayCheckedInAccounts: number;
    todaySuccess: number;
    todayAlready: number;
    todayFailed: number;
  };
  daily: CreditDailyPoint[];
  accounts: CreditAccountRow[];
  events?: unknown[];
  /** 官方用量不一定存在（旧后端 / 演示模式）；前端按 status 判断可用性。 */
  officialUsage?: CreditOfficialUsage;
}

export const creditStatistics = (refresh = false) =>
  invoke<CreditStatistics>("credit_statistics", { refresh });

export interface TokenTotals {
  total: number;
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
  uncachedInput: number;
  records: number;
  cacheHitRate?: number | null;
}

export interface TokenGroup extends TokenTotals {
  key: string;
}

export interface TokenSession extends TokenTotals {
  key: string;
  title?: string | null;
  project?: string | null;
  sessionId?: string | null;
}

/** 单次模型调用的明细行（仅 detail 数据源返回，已脱敏：无正文/绝对路径）。 */
export interface TokenRequest extends TokenTotals {
  timestamp: number;
  model: string;
  project: string;
  sessionId: string;
  title?: string | null;
  thinking: number;
}

export interface TokenSource {
  source: string;
  summary: TokenTotals;
  models: TokenGroup[];
  projects: TokenGroup[];
  sessions: TokenSession[];
  daily: TokenGroup[];
  dailyByModel?: Record<string, TokenGroup[]>;
  hours: TokenGroup[];
  requests?: TokenRequest[];
  filesScanned: number;
  parseErrors?: number;
  coverageStartAt?: number | null;
  coverageEndAt?: number | null;
}

export interface TokenStatistics {
  generatedAt: number;
  rangeDays?: number | null;
  sources: TokenSource[];
  /** 后端 5 分钟缓存命中时为 true（秒回）；「重新扫描」强制重算则为 false。 */
  cached?: boolean;
  error?: string;
}

/** days: 0 = 全部时间；7 / 30 / 90 为窗口。refresh=true 跳过缓存强制重扫。 */
export const tokenStatistics = (days = 0, refresh = false) =>
  invoke<TokenStatistics>("token_statistics", { days, refresh });

export const accountDelete = (accountId: string) =>
  invoke<{ ok: boolean; error?: string }>("account_delete", { accountId });

export const accountSetNote = (accountId: string, note: string) =>
  invoke<{ ok: boolean; error?: string }>("account_set_note", { accountId, note });

// ───────────────────────── 账号分组与标签 ─────────────────────────

export interface AccountGroupDef {
  id: string;
  name: string;
  excludeAutoSwitch: boolean;
  createdAt: number;
}

export interface AccountMemberDef {
  groups: string[];
  tags: string[];
}

export interface AccountGroupsData {
  ok: boolean;
  error?: string;
  groups: AccountGroupDef[];
  /** 账号 id → 归属与标签 */
  members: Record<string, AccountMemberDef>;
}

export const accountGroupsList = () => invoke<AccountGroupsData>("account_groups_list");

export const accountGroupSave = (id: string | null, name: string, excludeAutoSwitch: boolean) =>
  invoke<{ ok: boolean; group: AccountGroupDef }>("account_group_save", { id, name, excludeAutoSwitch });

export const accountGroupRemove = (id: string) =>
  invoke<{ ok: boolean }>("account_group_remove", { id });

export const accountMemberSave = (accountId: string, groups: string[], tags: string[]) =>
  invoke<{ ok: boolean }>("account_member_save", { accountId, groups, tags });

// ───────────────────────── 异常提醒中心 ─────────────────────────

export type AlertKind = "balance" | "health" | "expiring" | "switch" | "runfail";
export type AlertLevel = "critical" | "warn";

/** data 字段按 kind 不同：balance{daysLeft} health{score,reasons} expiring{packageName,remaining,expireAt} switch{result,stage,error} runfail{count} */
export interface AlertItem {
  fingerprint: string;
  kind: AlertKind;
  level: AlertLevel;
  accountId: string;
  accountLabel: string;
  ts: number;
  data: Record<string, unknown>;
}

export interface AlertsData {
  ok: boolean;
  alerts: AlertItem[];
  dismissedCount: number;
  evaluatedAt: number;
}

export const alertsList = () => invoke<AlertsData>("alerts_list");

export const alertsDismiss = (fingerprint: string) =>
  invoke<{ ok: boolean }>("alerts_dismiss", { fingerprint });

export const alertsDismissAll = () =>
  invoke<{ ok: boolean; dismissed: number }>("alerts_dismiss_all");

// ───────────────────────── 切换前安全检查 ─────────────────────────

export interface PreflightSession {
  id: string;
  title: string | null;
  updatedAt: number | null;
  isPlayground: boolean;
}

export interface SwitchPreflight {
  ok: boolean;
  error?: string;
  source?: {
    accountId: string | null;
    label: string;
    sessionCount: number;
  };
  target?: {
    accountId: string;
    label: string;
    sessionCount: number;
    sessions: PreflightSession[];
  };
  backup?: {
    skipped: boolean;
    reason?: "size" | "io";
    path?: string;
    fileCount?: number;
    totalBytes?: number;
    manifestSha256?: string;
  };
  checkedAt: number;
}

export const switchPreflight = (accountId: string) =>
  invoke<SwitchPreflight>("switch_preflight", { accountId });

/** 单个积分包（官方资源包口径，字段与 core 的 credits 模块一致）。 */
export interface AccountPackage {
  packageCode?: string | null;
  packageName?: string | null;
  total: number;
  remaining: number;
  used: number;
  expireAt: number | null;
  expired: boolean;
  expiringSoon: boolean;
}

/** 该账号当前被限额的模型（来自本机日志台账 + hook 事件，官方恢复时刻）。 */
export interface AccountRateLimit {
  model: string | null;
  resetAt: number;
  firstSeenAt?: number | null;
  hitCount?: number | null;
}

export interface AccountOverviewRow {
  accountId: string;
  label: string;
  variant: string;
  note?: string | null;
  ok: boolean;
  remaining: number | null;
  capacity: number | null;
  updatedAt: number | null;
  packages: number;
  resources: AccountPackage[];
  /** 即将到期的积分额度；> 0 时卡片显示「建议优先使用」。 */
  expiringSoonRemaining?: number | null;
  rateLimits?: AccountRateLimit[];
  /** 健康分（core `health.rs` 聚合：余额 / 限流 / 预测 / 错误） */
  health?: AccountHealth | null;
  error?: string | null;
}

/** 账号健康分：0-100，带可解释的原因码（前端翻成文案）。 */
export interface AccountHealth {
  score: number;
  grade: "good" | "warn" | "critical";
  reasons: string[];
}

/**
 * 账号卡片总览。mode=local 时余额来自本地采集快照（离线秒开）；
 * mode=official 时为官方实时口径，resources 带积分包明细（按到期升序，最多 6 个）。
 */
export interface AccountOverview {
  ok: boolean;
  mode: "local" | "official";
  accounts: AccountOverviewRow[];
  failed: number;
  collectedAt: number;
}

export const accountOverview = (refresh = false) =>
  invoke<AccountOverview>("account_overview", { refresh });

/** ─────────────── 用量预测（余额还能撑几天） ─────────────── */

export interface AccountForecastRow {
  accountId: string;
  accountName: string;
  remaining: number;
  /** 日均消耗（积分/天），中位数与 EWMA 取小 */
  perDay: number;
  /** 还能撑几天；样本不足或没有消耗时为 null */
  daysLeft: number | null;
  depleteAt: number | null;
  /** ok | insufficient | flat */
  confidence: "ok" | "insufficient" | "flat";
  /** 参与计算的天数（有快照的天数） */
  basisDays: number;
  /** 近 3 天 vs 前 3 天的变化百分比 */
  trendPct: number | null;
  used: number;
  /** 最近两条快照的余额差（正 = 消耗，负 = 领取/充值） */
  lastDelta: number | null;
  lastDeltaSecs: number | null;
}

export interface AccountForecastList {
  generatedAt: number;
  windowDays: number;
  minBasisDays: number;
  accounts: AccountForecastRow[];
}

export const accountForecast = (windowDays = 0) =>
  invoke<AccountForecastList>("account_forecast", { windowDays });

/** 一键刷新全部账号 token 的逐号结果。 */
export interface TokenRefreshResult {
  ok: boolean;
  total: number;
  refreshed: number;
  failed: number;
  results: { label: string; ok: boolean; error?: string | null }[];
  startedAt: number;
}

export const refreshAllTokens = () => invoke<TokenRefreshResult>("refresh_all_tokens");

/** WorkBuddy 运行状态 + 当前登录账号（auth 文件与账号库匹配的结果）。 */
export interface WorkbuddyStatus {
  running: boolean;
  currentAccountIds: string[];
  /** 主账号：运行中的档位优先，其次 cn；其余命中账号标「已登录」 */
  primaryAccountId?: string | null;
  currentByVariant?: Array<{ variant: string; accountId: string }>;
  runningVariant?: string | null;
  current: {
    uid: string | null;
    email: string | null;
    nickname: string | null;
    variant: string | null;
  } | null;
}

export const workbuddyStatus = () => invoke<WorkbuddyStatus>("workbuddy_status");

/** 设为 WorkBuddy 当前账号（完全同步：替换目标账号会话并重启 WorkBuddy）。 */
export interface SwitchResult {
  /**
   * 是否成功。失败时后端**不拦截**，而是把错误一起返回，
   * 因此 `ok` 必须显式判断，不能靠 try/catch。
   */
  ok: boolean;
  error?: string;
  /** ok | rolled_back | blocked */
  result?: "ok" | "rolled_back" | "blocked";
  /** done | session | auth_write | verify | launch */
  stage?: string;
  /** effective | identity-mismatch | identity-unverified */
  identity?: string;
  costMs?: number;
  /** 写后校验失败或写入失败时，是否已把认证文件还原 */
  rolledBack?: boolean;
  /** 认证已还原，但目标账号的会话已被本次切换改写（还原不回来） */
  sessionsDirty?: boolean;
  sessions?: {
    copied?: number;
    sourceTotal?: number;
    failed?: number;
    replaced?: boolean;
  } | null;
  backup?: string | null;
  sessionReplace?: {
    replaced?: boolean;
    copied?: number;
    failed?: number;
    sourceTotal?: number;
    reason?: string;
    delete?: { deleted?: string[] };
    error?: string;
  } | null;
  [key: string]: unknown;
}

export const switchWorkbuddyAccount = (accountId: string) =>
  invoke<SwitchResult>("switch_workbuddy_account", { accountId });

/** 切换历史（结构化，非日志文案）。 */
export interface SwitchHistorySessions {
  copied: number;
  sourceTotal: number;
  failed: number;
  replaced: boolean;
}

export interface SwitchHistoryEntry {
  ts: number;
  result: "ok" | "rolled_back" | "blocked";
  stage: string;
  accountId: string;
  toUid: string;
  toLabel: string;
  fromUid: string;
  fromLabel: string;
  variant: string;
  mode: "plain" | "copy" | "sync" | "replace";
  identity: string;
  costMs: number;
  sessions?: SwitchHistorySessions | null;
  error?: string | null;
}

export interface SwitchHistoryList {
  ok: boolean;
  version?: number;
  total: number;
  limit?: number;
  entries: SwitchHistoryEntry[];
  error?: string;
}

/** `limit` 为 0 返回全量（上限 500）。 */
export const switchHistoryList = (limit = 0) =>
  invoke<SwitchHistoryList>("switch_history_list", { limit });

// 清空统一走 eventsClear("switch")：同一件事只留一个入口，避免「哪个按钮真的删了」的歧义。

/** ─────────────── 运行日志（通知存档 / 自动日常 / 错误日志 / 切换历史） ─────────────── */

export type EventSource = "notify" | "auto" | "error" | "switch";
export type EventLevel = "success" | "info" | "warning" | "error";

export interface RunEvent {
  at: number;
  source: EventSource;
  level: EventLevel;
  title: string;
  detail?: string | null;
}

export interface EventStatistics {
  total: number;
  today: number;
  warning: number;
  error: number;
  notify: number;
  auto: number;
  errlog: number;
  switch: number;
}

export interface EventList {
  ok: boolean;
  items: RunEvent[];
  truncated: boolean;
  stats: EventStatistics;
  loadErrors: string[];
  generatedAt: number;
}

export const eventsList = (limit = 300) => invoke<EventList>("events_list", { limit });

export const eventsClear = (source: EventSource) =>
  invoke<{ ok: boolean; error?: string }>("events_clear", { source });

/** ─────────────── 系统信息 ─────────────── */

export interface FileStat {
  exists: boolean;
  bytes: number;
  human: string;
}

export interface SystemInfo {
  ok: boolean;
  version: string;
  os: string;
  arch: string;
  storeDir: string;
  accountCount: number;
  counts: { notify: number; autoLogs: number; errorLines: number; switchHistory: number };
  files: {
    accounts: FileStat;
    autoLogs: FileStat;
    notifications: FileStat;
    errorLog: FileStat;
    switchHistory: FileStat;
  };
  schedule: { enabled?: boolean; time?: string; lastRunDate?: string };
}

export const systemInfo = () => invoke<SystemInfo>("system_info");

export const openStoreDir = () =>
  invoke<{ ok: boolean; path: string; error?: string }>("open_store_dir");

/** 用系统浏览器打开链接（后端只放行 http/https）。 */
export const openExternal = (url: string) =>
  invoke<{ ok: boolean; error?: string }>("open_external", { url });

/** 打开 WorkBuddy 客户端深链（后端只放行 workbuddy://）。 */
export const openWorkbuddyDeeplink = (url: string) =>
  invoke<{ ok: boolean; error?: string }>("open_workbuddy_deeplink", { url });

/** ─────────────── 限流自动切换 ─────────────── */

export interface AutoSwitchConfig {
  enabled: boolean;
  /** 只提示不执行（默认 true） */
  notifyOnly: boolean;
  cooldownMinutes: number;
  excludedAccountIds: string[];
  minRemaining: number;
  requireSameVariant: boolean;
  /** 切换前先询问（默认 true） */
  askBeforeSwitch: boolean;
  /** 窗口在后台时只提醒不执行（默认 true） */
  notifyWhenBackground: boolean;
  /** 询问倒计时秒数（3–120，默认 15） */
  countdownSeconds: number;
}

export interface AutoSwitchEvent {
  action: "skip" | "notify" | "switch";
  reason?: string;
  targetId?: string;
  targetLabel?: string;
  ok?: boolean;
  error?: string;
  ts?: number;
}

export interface AutoSwitchStatus {
  config: AutoSwitchConfig;
  lastSwitchAt: number | null;
  lastDecision: AutoSwitchEvent | null;
  totalEvents: number;
}

export interface AutoSwitchCheck {
  enabled: boolean;
  notifyOnly: boolean;
  currentAccountId: string | null;
  decision: AutoSwitchEvent;
}

export const autoSwitchStatus = () => invoke<AutoSwitchStatus>("auto_switch_status");

export const autoSwitchSet = (patch: Partial<AutoSwitchConfig>) =>
  invoke<{ ok: boolean; status?: AutoSwitchStatus; error?: string }>("auto_switch_set", { patch });

/** 手动判定一次：不执行切换，只告诉你会怎么判。 */
export const autoSwitchCheck = () => invoke<AutoSwitchCheck>("auto_switch_check");

/** 取消本轮待执行的自动切换（3 秒倒计时内有效）。 */
export const autoSwitchCancel = () =>
  invoke<{ ok: boolean; error?: string }>("auto_switch_cancel");

/** 用户点「立即切换」：跳过剩余倒计时，立刻执行（仍走互斥与批量任务闸门）。 */
export const autoSwitchConfirm = () =>
  invoke<{ action: string; ok?: boolean; reason?: string; error?: string }>("auto_switch_confirm");

/** 当前是否有待确认的切换（前端重挂时补画提示条）。 */
export const autoSwitchPending = () =>
  invoke<{ targetId?: string; targetLabel?: string; reason?: string; seconds?: number; deadline?: number } | null>(
    "auto_switch_pending",
  );

export const autoSwitchLogs = (limit = 100) =>
  invoke<{ total: number; items: AutoSwitchEvent[] }>("auto_switch_logs", { limit });

export const autoSwitchClearLogs = () =>
  invoke<{ ok: boolean; removed?: number; error?: string }>("auto_switch_clear_logs");

/** ─────────────── 数据导出（CSV） ─────────────── */

export type ExportKind = "accounts" | "tokensDaily" | "tokensModels" | "credits";

export interface ExportResult {
  ok: boolean;
  kind?: string;
  path?: string;
  bytes?: number;
  error?: string;
}

export interface ExportAllFile {
  kind: string;
  path?: string;
  bytes?: number;
  error?: string;
}

export interface ExportAllResult {
  ok: boolean;
  dir: string;
  files: ExportAllFile[];
  failed: number;
}

/** 导出一份 CSV 到指定路径（后端直接写盘，不回传内容）。 */
export const exportData = (kind: ExportKind, path: string) =>
  invoke<ExportResult>("export_data", { kind, path });

/** 把四份 CSV 一起写进目录。 */
/** 导出诊断包（单个 txt，已脱敏）：排障用。 */
export const exportDiagnostics = (path: string) =>
  invoke<{ ok: boolean; path?: string; error?: string }>("export_diagnostics", { path });

export const exportAllData = (dir: string) =>
  invoke<ExportAllResult>("export_all_data", { dir });

/** ─────────────── 归档任务清理（WorkBuddy 会话库） ─────────────── */

export interface ArchivedStats {
  ok: boolean;
  /** 库文件是否存在；不存在时 count 恒为 0，按钮禁用 */
  dbExists: boolean;
  /** WorkBuddy 是否正在运行（运行中后端拒绝清理） */
  running: boolean;
  count: number;
  bytes: number;
  human: string;
  error?: string;
}

export interface PurgeResult {
  ok: boolean;
  deleted?: number;
  freedBytes?: number;
  human?: string;
  /** 实际删除的正文文件数可能小于会话数（无正文的会话跳过） */
  total?: number;
  backupDir?: string;
  blocked?: boolean;
  /** 停在哪个阶段：close / purge / done */
  stage?: string;
  /** 是否替用户关掉了正在运行的 WorkBuddy */
  closed?: boolean;
  relaunched?: boolean;
  /** 清理成功但拉起失败时的原因（数据已清干净，不算清理失败） */
  launchError?: string;
  error?: string;
}

export const archivedTasksStats = () => invoke<ArchivedStats>("archived_tasks_stats");

export interface ArchivedTask {
  id: string;
  title: string;
  /** 归档时刻（毫秒时间戳，可能为空） */
  updatedAt: number | null;
  bytes: number;
  human: string;
}

export interface ArchivedList {
  ok: boolean;
  dbExists: boolean;
  running: boolean;
  total: number;
  bytes: number;
  human: string;
  rows: ArchivedTask[];
}

export const archivedTasksList = () => invoke<ArchivedList>("archived_tasks_list");

/**
 * 一键清理归档：自动关 WorkBuddy → 清理 → 自动拉起（默认）。
 * 进度走事件 `purge-progress`，与切换账号同一套路。
 */
/** 窗口背景特效：off = 不透明；acrylic = 系统毛玻璃（Windows acrylic / macOS vibrancy）。 */
export const setWindowBackdrop = (kind: "off" | "acrylic") =>
  invoke<{ ok: boolean; error?: string }>("set_backdrop", { kind });

export const purgeArchivedTasks = (relaunch = true) =>
  invoke<PurgeResult>("purge_archived_tasks", { relaunch });

// ───────────────────────── 自动更新 ─────────────────────────

/** 一次更新检查的结果。`ok=false` 时只有 `error` / `releaseUrl` 有意义。 */
export interface UpdateStatus {
  ok: boolean;
  current?: string;
  latest?: string;
  hasUpdate?: boolean;
  notes?: string | null;
  publishedAt?: string | null;
  releaseUrl?: string;
  checkedAt?: number;
  autoCheck?: boolean;
  mirror?: string;
  error?: string;
}

export interface UpdateConfig {
  ok: boolean;
  current: string;
  autoCheck: boolean;
  mirror: string;
  configPath?: string;
  error?: string;
}

/** 下载进度：`total` 为空表示服务端没给 Content-Length。 */
export interface UpdateProgress {
  downloaded: number;
  total: number | null;
}

/** 安装结束（Windows 上成功时进程已被安装器接管，多半收不到这条）。 */
export interface UpdateDone {
  ok: boolean;
  error?: string | null;
  version?: string;
}

/** 检查更新；`force=true` 绕过 6 小时缓存。 */
export const updateCheck = (force = false) => invoke<UpdateStatus>("update_check", { force });

/** 触发下载安装：立即返回，进度与结果走 `update://progress` / `update://done` 事件。 */
export const updateInstall = () =>
  invoke<{ ok: boolean; started?: boolean; error?: string }>("update_install");

export const updateConfigGet = () => invoke<UpdateConfig>("update_config_get");

export const updateConfigSet = (autoCheck: boolean, mirror: string) =>
  invoke<UpdateConfig>("update_config_set", { autoCheck, mirror });
