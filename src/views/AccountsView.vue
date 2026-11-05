<script setup lang="ts">
/**
 * 账号管理：卡片流（一账号一卡）。
 * - 卡内信息：余额 / 总额度进度 / 积分包数 / 近期到期明细（最多 6 个）/ 备注 / 更新时刻。
 * - 数据两层：进页面先读本地采集快照（离线秒开），随后后台拉一次官方口径补全积分包。
 * - 账号新增、导入、导出全部集中在本页；任务页不再提供入口。
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { MorphIcon, type IconNode } from "morphicons/vue";
import { Check, LoaderCircle, RefreshCw } from "lucide";
import AnimatedNumber from "@/components/ui/AnimatedNumber.vue";
import {
  ArrowLeftRight,
  CircleAlert,
  CircleCheck,
  Download,
  Gauge,
  Loader2,
  Pencil,
  RotateCw,
  Star,
  Trash2,
  Upload,
  UserPlus,
  Zap,
} from "lucide-vue-next";

import {
  accountDelete,
  accountForecast,
  accountOverview,
  accountSetNote,
  importLocalAccounts,
  refreshAllTokens,
  switchWorkbuddyAccount,
  workbuddyStatus,
  type AccountForecastRow,
  type AccountHealth,
  type AccountOverviewRow,
  type AccountPackage,
  type WorkbuddyStatus,
} from "@/lib/api";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useAccountIo } from "@/composables/useAccountIo";
import { useLogin } from "@/composables/useLogin";
import { useShell } from "@/composables/useShell";
import { useListAnim } from "@/composables/useListAnim";
import { t, type I18nKey } from "@/lib/i18n";
import { mask } from "@/lib/task-meta";

const { setAccountCount, go } = useShell();
const cardsEl = useListAnim();

const rows = ref<AccountOverviewRow[]>([]);
const mode = ref<"local" | "official">("local");
const loading = ref(true);
const refreshing = ref(false);
const tokenRefreshing = ref(false);
const error = ref("");
const toast = ref<{ text: string; level: "ok" | "err" } | null>(null);
const refreshDone = ref(false);

const editingId = ref("");
const noteDraft = ref("");
const savingNote = ref(false);
const pendingDelete = ref("");
/** 删除确认 8s 后自动撤销（与命令区二次确认同节奏）。 */
let confirmTimer: number | undefined;

function say(text: string, level: "ok" | "err" = "ok") {
  toast.value = { text, level };
  window.setTimeout(() => {
    if (toast.value?.text === text) toast.value = null;
  }, 2600);
}

/** 刷新按钮图标：静止 → 加载中 → 完成后短暂打勾（morphicons 形变过渡）。 */
const refreshIcon = computed<IconNode>(() => {
  if (refreshing.value) return LoaderCircle;
  if (refreshDone.value) return Check;
  return RefreshCw;
});
let refreshDoneTimer: ReturnType<typeof setTimeout> | null = null;

/** refresh=false 读本地快照；true 拉官方资源包（较慢，页面先显示旧数据）。 */
async function load(refresh = false) {
  if (refresh) refreshing.value = true;
  else loading.value = true;
  error.value = "";
  try {
    const result = await accountOverview(refresh);
    rows.value = result.accounts ?? [];
    mode.value = result.mode ?? "local";
    setAccountCount(rows.value.length);
    if (refresh && !error.value) {
      refreshDone.value = true;
      if (refreshDoneTimer) clearTimeout(refreshDoneTimer);
      refreshDoneTimer = setTimeout(() => (refreshDone.value = false), 1200);
    }
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    loading.value = false;
    refreshing.value = false;
  }
  // 预测是本地纯计算（读快照，不发请求）：主表出来后再补，失败不影响余额展示。
  void loadForecast();
}

/**
 * 用量预测：按 accountId 索引成 Map，卡片里 O(1) 取。
 *
 * 预测失败绝不能让整页报错——它只是余额旁边的一个附加读数。
 */
const forecasts = ref<Map<string, AccountForecastRow>>(new Map());
const minBasisDays = ref(3);
async function loadForecast() {
  try {
    const result = await accountForecast();
    forecasts.value = new Map((result.accounts ?? []).map((row) => [row.accountId, row]));
    if (result.minBasisDays) minBasisDays.value = result.minBasisDays;
  } catch {
    forecasts.value = new Map();
  }
}

const forecastOf = (accountId: string) => forecasts.value.get(accountId) ?? null;

/**
 * 剩余天数的紧张度。主题只有一支强调色（hazard），所以用**明度**分层而不是加新色：
 * <3 天实红、<7 天高亮正文色、其余留灰。
 */
const forecastTone = (row: AccountForecastRow | null) => {
  if (!row) return "text-faint";
  if (row.confidence !== "ok" || row.daysLeft === null) return "text-faint";
  if (row.daysLeft < 3) return "text-hazard";
  if (row.daysLeft < 7) return "text-foreground";
  return "text-muted-foreground";
};

/**
 * 卡片上的短标签。三种状态都给出**明确理由**，不留一个孤零零的 `--`：
 * `≈2.4d` / `样本 1/3d` / `无消耗`。
 */
const forecastLabel = (row: AccountForecastRow | null) => {
  if (!row) return "";
  if (row.confidence === "insufficient") {
    return t("ac.fcSample")
      .replace("{n}", String(row.basisDays))
      .replace("{min}", String(minBasisDays.value));
  }
  if (row.confidence === "flat" || row.daysLeft === null) return t("ac.fcFlat");
  const days = row.daysLeft < 1 ? "<1" : row.daysLeft.toFixed(1);
  return t("ac.fcDays").replace("{n}", days);
};

/** 悬停详情：日均、窗口、预计耗尽日、最近一次变动。 */
const forecastTitle = (row: AccountForecastRow | null) => {
  if (!row) return "";
  const parts = [
    t("ac.fcPerDay").replace("{n}", row.perDay.toFixed(1)),
    t("ac.fcWindow").replace("{n}", String(row.basisDays)),
  ];
  if (row.depleteAt) {
    parts.push(t("ac.fcDeplete").replace("{at}", fmtDate(row.depleteAt)));
  }
  if (row.lastDelta !== null && row.lastDelta !== 0) {
    const sign = row.lastDelta > 0 ? "-" : "+";
    parts.push(t("ac.fcLast").replace("{delta}", `${sign}${Math.abs(row.lastDelta).toFixed(1)}`));
  }
  return parts.join(" · ");
};

const fmtDate = (ms: number) =>
  new Date(ms).toLocaleDateString(undefined, { month: "2-digit", day: "2-digit" });

/** 健康点颜色：green / 灰白 / 红。主题只有一支强调色，所以用明度分层。 */
const healthDot = (health: AccountHealth | null | undefined) => {
  if (!health) return "bg-muted";
  if (health.grade === "critical") return "bg-hazard";
  if (health.grade === "warn") return "bg-foreground/60";
  return "bg-foreground/85";
};

/** 原因码 → 文案。后端只给码，翻译在这里，避免中英分裂。 */
const HEALTH_REASONS: Record<string, I18nKey> = {
  rateLimited: "ac.hzRateLimited",
  empty: "ac.hzEmpty",
  lowBalance: "ac.hzLowBalance",
  fetchError: "ac.hzFetchError",
  depletingSoon: "ac.hzDepletingSoon",
  expiringDominant: "ac.hzExpiring",
};

/** 悬停文案：分数 + 全部原因；没扣分时明说「正常」，不留空 title。 */
const healthTitle = (health: AccountHealth | null | undefined) => {
  if (!health) return "";
  const reasons = (health.reasons ?? []).map((code) =>
    HEALTH_REASONS[code] ? t(HEALTH_REASONS[code]) : code,
  );
  return `${health.score} · ${reasons.length > 0 ? reasons.join(" / ") : t("ac.hzOk")}`;
};

/**
 * 把「本机 WorkBuddy 已登录的账号」导入账号库。
 *
 * 账号列表读的是 `~/.wb-switch/accounts.json`；没装过上游 workbuddy-switch 的用户
 * 这个文件不存在，账号永远是空。登录态在 WorkBuddy 自己的 auth 文件里，这里补读一次。
 * 返回导入了几个（0 表示本机没有可导入的登录态，或已经在库里）。
 */
const importing = ref(false);
async function importLocal(silent = false) {
  if (importing.value) return 0;
  importing.value = true;
  try {
    const result = await importLocalAccounts();
    const count = result.imported ?? 0;
    if (silent) {
      if (count > 0) await load(false);
      return count;
    }
    if (count > 0) {
      say(t("ac.importLocalDone").replace("{n}", String(count)));
      await load(false);
    } else {
      say(t("ac.importLocalEmpty"), "err");
    }
    return count;
  } catch (cause) {
    if (!silent) say(cause instanceof Error ? cause.message : String(cause), "err");
    return 0;
  } finally {
    importing.value = false;
  }
}

onMounted(async () => {
  // 先补一次本机登录态（静默），再本地秒开；没装过 workbuddy-switch 的用户靠这一步才有账号
  await importLocal(true);
  // 先本地秒开，再后台补官方口径（失败不覆盖本地结果）；当前登录态并行查
  await load(false);
  void loadWbStatus();
  if (rows.value.length > 0) void load(true);
});

const {
  pending: loginPending,
  collected: loginCollected,
  dismissCollected,
  start: startLogin,
} = useLogin({
  log: (text, level) => say(text, level === "err" ? "err" : "ok"),
  onAdded: async () => {
    await Promise.all([load(false), loadWbStatus()]);
    // 新账号没有本地快照，紧跟一次官方拉取，卡片立刻有余额
    await load(true);
  },
});

/** 采集成功弹框展示的账号名：昵称 > 邮箱 > uid。 */
const collectedName = computed(() => {
  const meta = loginCollected.value;
  if (!meta) return "";
  return meta.nickname?.trim() || meta.email?.trim() || meta.uid?.trim() || "未知账号";
});

const { exportAccounts, importAccounts } = useAccountIo({
  log: (text, level) => say(text, level === "err" ? "err" : "ok"),
  onImported: async () => {
    await load(false);
    await load(true);
  },
});

function startNote(row: AccountOverviewRow) {
  editingId.value = row.accountId;
  noteDraft.value = row.note ?? "";
}

function cancelNote() {
  editingId.value = "";
  noteDraft.value = "";
}

async function saveNote(row: AccountOverviewRow) {
  if (savingNote.value) return;
  savingNote.value = true;
  try {
    const result = await accountSetNote(row.accountId, noteDraft.value);
    if (!result.ok) {
      say(result.error ?? "保存失败", "err");
      return;
    }
    say(t("ac.saved"));
    cancelNote();
    await load(false);
  } finally {
    savingNote.value = false;
  }
}

/** 第一次点只是进入确认态，第二次才真删。 */
async function remove(row: AccountOverviewRow) {
  if (pendingDelete.value !== row.accountId) {
    pendingDelete.value = row.accountId;
    window.clearTimeout(confirmTimer);
    confirmTimer = window.setTimeout(() => {
      pendingDelete.value = "";
    }, 8000);
    return;
  }
  pendingDelete.value = "";
  const result = await accountDelete(row.accountId);
  if (!result.ok) {
    say(result.error ?? "删除失败", "err");
    return;
  }
  say(`已删除 ${mask(row.label)}`);
  await load(false);
}

/** 卡片内「近期到期」只露 2 条，其余进弹框。 */


/** 弹框当前展示的账号；null 表示关闭。 */
const packageSheet = ref<AccountOverviewRow | null>(null);

const sheetPackages = computed<AccountPackage[]>(() => packageSheet.value?.resources ?? []);

function openPackages(row: AccountOverviewRow) {
  packageSheet.value = row;
}

function closePackages() {
  packageSheet.value = null;
}

/** Esc 关闭弹框。 */
function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") closePackages();
}

watch(packageSheet, (value) => {
  if (typeof window === "undefined") return;
  if (value) window.addEventListener("keydown", onKeydown);
  else window.removeEventListener("keydown", onKeydown);
});

onBeforeUnmount(() => {
  if (typeof window !== "undefined") window.removeEventListener("keydown", onKeydown);
  if (refreshDoneTimer) clearTimeout(refreshDoneTimer);
});

/** 一键强刷全部账号 token：逐号结果在这里汇总成一句人话。 */
async function refreshTokens() {
  if (tokenRefreshing.value) return;
  tokenRefreshing.value = true;
  say("正在刷新全部账号 TOKEN…");
  try {
    const result = await refreshAllTokens();
    if (result.failed === 0) {
      say(`${t("ac.tokenDone")} · ${result.refreshed}/${result.total}`, "ok");
    } else {
      const firstError = result.results.find((row) => !row.ok);
      const detail = firstError?.error ? ` · ${mask(firstError.label)}: ${firstError.error}` : "";
      say(`${t("ac.tokenPartial")} · ${result.refreshed}/${result.total}${detail}`, "err");
    }
    // token 换新后余额/积分包要按新凭据重取，顺手拉一次官方口径
    await load(true);
  } catch (cause) {
    say(`${t("ac.tokenRefreshFail")}: ${cause instanceof Error ? cause.message : String(cause)}`, "err");
  } finally {
    tokenRefreshing.value = false;
  }
}

const totalRemaining = computed(() =>
  rows.value.reduce((sum, row) => sum + (row.remaining ?? 0), 0),
);

const failedCount = computed(() => rows.value.filter((row) => !row.ok).length);

const variantText = (variant: string) => (variant === "ai" ? "INTL" : "CN");

const fmt = (value: number | null | undefined) => {
  if (value === null || value === undefined || !Number.isFinite(value)) return "—";
  return value.toLocaleString("zh-CN", { maximumFractionDigits: 0 });
};

const fmtTime = (ts: number | null | undefined) => {
  if (!ts) return "—";
  return new Date(ts).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
};

/** 到期时间只到分钟：卡片一行要放下「包名 + 时间」。 */
const fmtExpire = (ts: number | null | undefined) => {
  if (!ts) return t("ac.longTerm");
  return new Date(ts).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
};

const packageName = (item: AccountPackage) =>
  item.packageName?.trim() || item.packageCode?.trim() || t("ac.noPkg");

/** 余额占总额度的比例；无总额度时退回 0。 */
const balanceRatio = (row: AccountOverviewRow) => {
  if (!row.capacity || row.capacity <= 0 || row.remaining === null) return 0;
  return Math.min(100, Math.max(0, (row.remaining / row.capacity) * 100));
};

const packageRatio = (item: AccountPackage) => {
  if (!item.total || item.total <= 0) return 0;
  return Math.min(100, Math.max(0, (item.remaining / item.total) * 100));
};

// ───────────────────────── 建议优先 / 模型限流 ─────────────────────────

/** 「建议优先使用」：官方口径下该账号仍有即将到期的积分（过期就烧不出来了）。 */
const isPriority = (row: AccountOverviewRow) =>
  row.ok && (row.expiringSoonRemaining ?? 0) > 0;

/**
 * 近期到期摘要：一行说清「几个包 · 最近什么时候到期」，明细留给「查看全部」弹框。
 *
 * 之前卡片里每条包都渲染名字 + 余额 + 时间 + 独立进度条，两个包就是八行视觉噪音，
 * 用户的实际诉求是「有没有快过期的」，不是「逐条对账」。
 */
const expiringSummary = (row: AccountOverviewRow) => {
  const items = row.resources;
  if (items.length === 0) return "";
  const soonest = items
    .filter((item) => item.expireAt != null)
    .reduce((best, item) => Math.min(best, item.expireAt as number), Number.POSITIVE_INFINITY);
  // 包数右侧已经显示，这里只说「最近什么时候到期」，避免同一信息出现两遍。
  if (!Number.isFinite(soonest)) return t("ac.longTerm");
  return `${t("ac.nearest")} ${fmtExpire(soonest)}`;
};

/** 摘要的紧张度：有已过期或 24 小时内到期的包才标红。 */
const expiringTone = (row: AccountOverviewRow) =>
  row.resources.some((item) => item.expired || item.expiringSoon)
    ? "text-hazard"
    : "text-muted-foreground";

/** 官方口径下的卡片排序：即将到期 > 有过期包 > 普通 > 失败；同档比最早到期时刻。 */
const orderedRows = computed(() => {
  const list = [...rows.value];
  if (mode.value !== "official") return list;
  const rank = (row: AccountOverviewRow) => {
    if (!row.ok) return 3;
    if (isPriority(row)) return 0;
    if (row.resources.some((item) => item.expired)) return 1;
    return 2;
  };
  // 用 MAX_SAFE_INTEGER 而不是 Infinity：Infinity - Infinity = NaN，
  // 会让「都没有到期时间」的两张卡片比较结果变成未定义顺序。
  const soonest = (row: AccountOverviewRow) =>
    row.resources
      .filter((item) => item.remaining > 0 && item.expireAt != null)
      .reduce((best, item) => Math.min(best, item.expireAt as number), Number.MAX_SAFE_INTEGER);
  return list.sort(
    (left, right) =>
      rank(left) - rank(right) || soonest(left) - soonest(right),
  );
});


/** 账号当前生效的限流条目（官方恢复时刻还没到的）。 */
const activeLimits = (row: AccountOverviewRow) => {
  const now = Date.now();
  return (row.rateLimits ?? []).filter((item) => item.resetAt > now);
};

/** 「28m 后恢复」「1h05m 后恢复」——终端风格的倒计时。 */
const recoverLabel = (resetAt: number) => {
  const left = Math.max(0, resetAt - Date.now());
  const minutes = Math.round(left / 60000);
  if (minutes < 60) return `${minutes}m`;
  return `${Math.floor(minutes / 60)}h${String(minutes % 60).padStart(2, "0")}m`;
};

const limitTitle = (items: ReturnType<typeof activeLimits>) =>
  items
    .map((item) => `${item.model ?? "未知模型"} · ${recoverLabel(item.resetAt)} 后恢复`)
    .join("\n");

/** 名称行内联徽标文案：不占卡片高度（用户要求放在名称后面）。 */
const limitShort = (row: AccountOverviewRow) => {
  const items = activeLimits(row);
  if (items.length === 0) return "";
  if (items.length === 1) {
    return `${items[0].model || "未知模型"} · ${recoverLabel(items[0].resetAt)}`;
  }
  return `${items.length} ${t("ac.limited")} · ${recoverLabel(
    Math.min(...items.map((item) => item.resetAt)),
  )}`;
};

// ───────────────────────── 当前账号 / 完全同步切换 ─────────────────────────

/** WorkBuddy 当前登录账号（auth 文件与账号库匹配，后端判定）。 */
const wbStatus = ref<WorkbuddyStatus | null>(null);

async function loadWbStatus() {
  try {
    wbStatus.value = await workbuddyStatus();
  } catch {
    wbStatus.value = null;
  }
}

const isCurrent = (row: AccountOverviewRow) =>
  wbStatus.value?.currentAccountIds.includes(row.accountId) ?? false;

/** 待确认切换的目标账号；null 表示弹框关闭。 */
const switchTarget = ref<AccountOverviewRow | null>(null);
const switching = ref(false);
const switchProgress = ref("");
/**
 * 切换失败的结构化现场：留在弹框里，别跟着 toast 一起 2.6 秒后消失。
 * 用户要能看清「停在哪一步、是否已回滚、复制了几条」。
 */
const switchError = ref<null | {
  rolledBack: boolean;
  /** 认证已还原，但目标账号的会话已被改写（还不了）。 */
  sessionsDirty: boolean;
  result: string;
  stage: string;
  identity: string;
  message: string;
}>(null);

const switchBullets = computed(() => t("ac.switchBullets").split("\n"));

/** 阶段 → 文案。未知阶段原样显示（后端新增阶段时不至于空白）。 */
const STAGE_KEYS: Record<string, I18nKey> = {
  start: "ac.stageStart",
  session: "ac.stageSession",
  auth_write: "ac.stageAuthWrite",
  verify: "ac.stageVerify",
  launch: "ac.stageLaunch",
  done: "ac.stageDone",
};
const stageLabel = (stage: string) => (STAGE_KEYS[stage] ? t(STAGE_KEYS[stage]) : stage || "-");

function openSwitch(row: AccountOverviewRow) {
  if (switching.value) return;
  switchError.value = null;
  switchTarget.value = row;
  switchProgress.value = "";
}

function closeSwitch() {
  if (switching.value) return;
  switchTarget.value = null;
  switchError.value = null;
}

/** 完全同步切换：以当前登录账号为准全量覆盖目标账号，并重启 WorkBuddy。 */
async function doSwitch() {
  const target = switchTarget.value;
  if (!target || switching.value) return;
  switching.value = true;
  switchError.value = null;
  switchProgress.value = t("ac.switching");
  let unlisten: UnlistenFn | null = null;
  try {
    unlisten = await listen<{ message: string }>("switch-progress", (event) => {
      switchProgress.value = event.payload.message;
    });
    // 后端不抛错：失败也返回 ok:false，附带 stage / rolledBack / identity。
    const result = await switchWorkbuddyAccount(target.accountId);

    if (!result.ok) {
      switchError.value = {
        rolledBack: result.rolledBack === true,
        sessionsDirty: result.sessionsDirty === true,
        result: result.result ?? "blocked",
        stage: result.stage ?? "",
        identity: result.identity ?? "",
        message: result.error ?? t("ac.switchFail"),
      };
      // 回滚过：认证文件已被还原成切换前那份，余额与当前账号要按旧口径重取。
      if (result.rolledBack) await Promise.all([load(false), loadWbStatus()]);
      return;
    }

    const copied = result.sessions?.copied ?? result.sessionReplace?.copied ?? 0;
    const deleted = result.sessionReplace?.delete?.deleted?.length ?? 0;
    const seconds = ((result.costMs ?? 0) / 1000).toFixed(1);
    // 身份无法校验是警告而非失败：账号本身没有可判身份字段，如实标注即可。
    const identityNote =
      result.identity === "identity-unverified" ? ` · ${t("ac.switchIdentityUnverified")}` : "";
    say(
      `${t("ac.switchDone")} · ${mask(target.label)} · ${t("ac.switchCopied")} ${copied} · ${t("ac.switchDeleted")} ${deleted} · ${seconds}s${identityNote}`,
    );
    switchTarget.value = null;
    // 切换后 WorkBuddy 用新账号重启：余额、当前态都要重取
    await Promise.all([load(true), loadWbStatus()]);
  } catch (cause) {
    // 走到这里只可能是 IPC 本身挂了（后端已把业务失败转成 ok:false）。
    switchError.value = {
      rolledBack: false,
      sessionsDirty: false,
      result: "blocked",
      stage: "",
      identity: "",
      message: cause instanceof Error ? cause.message : String(cause),
    };
  } finally {
    unlisten?.();
    switching.value = false;
  }
}
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <header class="flex shrink-0 items-center justify-between gap-3 border-b border-border bg-panel px-4 py-2 wb-flat">
      <div class="min-w-0">
        <h1 class="text-[13px] tracking-[0.12em] text-foreground">{{ t("ac.title") }}</h1>
        <div class="readout mt-0.5 flex flex-wrap items-center gap-x-3 text-[9px] text-faint">
          <span>{{ t("ac.count") }} {{ String(rows.length).padStart(2, "0") }}</span>
          <span class="flex items-center gap-1">Σ <AnimatedNumber :value="totalRemaining" /></span>
          <span>{{ mode === "official" ? t("ac.officialMode") : t("ac.localMode") }}</span>
          <span v-if="failedCount > 0" class="text-hazard">{{ t("ac.failed") }} {{ failedCount }}</span>
        </div>
      </div>
      <div class="flex shrink-0 items-center gap-1.5">
        <button
          type="button"
          class="flex items-center gap-1.5 bg-primary px-2.5 py-1 text-[10px] uppercase tracking-[0.1em] text-primary-foreground transition-opacity hover:opacity-90 disabled:opacity-40"
          :disabled="refreshing"
          @click="load(true)"
        >
          <MorphIcon
            :icon="refreshIcon"
            :size="12"
            :stroke-width="1.75"
            spring="snappy"
            reduced-motion="user"
            color="currentColor"
            :class="refreshing ? 'animate-spin' : ''"
          />
          {{ t("ac.refresh") }}
        </button>
        <button
          type="button"
          class="flex items-center gap-1.5 border border-border-strong px-2.5 py-1 text-[10px] uppercase tracking-[0.1em] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground disabled:opacity-40"
          :disabled="tokenRefreshing"
          :title="t('ac.refreshToken')"
          @click="refreshTokens()"
        >
          <Loader2 v-if="tokenRefreshing" class="size-3 animate-spin" />
          <RotateCw v-else class="size-3" />
          {{ t("ac.refreshToken") }}
        </button>
        <button
          type="button"
          class="flex items-center gap-1.5 border border-border-strong px-2.5 py-1 text-[10px] uppercase tracking-[0.1em] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground disabled:opacity-40"
          :disabled="importing"
          :title="t('ac.importLocalHint')"
          @click="importLocal(false)"
        >
          <Loader2 v-if="importing" class="size-3 animate-spin" />
          <Download v-else class="size-3" />
          {{ t("ac.importLocal") }}
        </button>
        <button
          type="button"
          class="flex items-center gap-1.5 border border-border-strong px-2.5 py-1 text-[10px] uppercase tracking-[0.1em] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground disabled:opacity-40"
          :disabled="loginPending"
          @click="startLogin()"
        >
          <Loader2 v-if="loginPending" class="size-3 animate-spin" />
          <UserPlus v-else class="size-3" />
          {{ t("ac.add") }}
        </button>
        <button
          type="button"
          class="flex items-center gap-1.5 border border-border-strong px-2.5 py-1 text-[10px] uppercase tracking-[0.1em] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground"
          @click="importAccounts()"
        >
          <Upload class="size-3" />
          {{ t("ac.import") }}
        </button>
        <button
          type="button"
          class="flex items-center gap-1.5 border border-border-strong px-2.5 py-1 text-[10px] uppercase tracking-[0.1em] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground"
          @click="exportAccounts()"
        >
          <Download class="size-3" />
          {{ t("ac.export") }}
        </button>
      </div>
    </header>

    <div
      v-if="error"
      class="flex shrink-0 items-center gap-2 border-b border-hazard bg-hazard-soft px-4 py-2 text-[11px] text-hazard"
    >
      <CircleAlert class="size-3.5 shrink-0" />
      <span class="truncate">{{ error }}</span>
    </div>
    <div
      v-if="toast"
      class="flex shrink-0 items-center gap-2 border-b border-border bg-muted px-4 py-1.5 text-[10px]"
      :class="toast.level === 'err' ? 'text-hazard' : 'text-foreground'"
    >
      <CircleCheck v-if="toast.level === 'ok'" class="size-3.5 shrink-0" />
      <CircleAlert v-else class="size-3.5 shrink-0" />
      <span class="truncate">{{ toast.text }}</span>
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto p-3">
      <!-- 首次加载：骨架卡（呼吸块）比一个「···」更像在干活 -->
      <div
        v-if="orderedRows.length === 0 && loading"
        class="grid grid-cols-1 gap-2.5 md:grid-cols-2 xl:grid-cols-3"
      >
        <div
          v-for="n in 6"
          :key="n"
          class="flex h-[172px] flex-col gap-2 border border-border bg-card p-3"
        >
          <div class="flex items-center gap-2">
            <div class="size-1.5 bg-border-strong" />
            <div class="h-2.5 w-24 animate-pulse bg-border-strong" />
            <div class="ml-auto h-3 w-10 animate-pulse bg-border" />
          </div>
          <div class="mt-1 h-5 w-32 animate-pulse bg-border-strong" />
          <div class="h-[3px] w-full bg-border" />
          <div class="mt-auto h-2 w-4/5 animate-pulse bg-border" />
        </div>
      </div>
      <div v-else-if="orderedRows.length === 0" class="px-3 py-16 text-center text-[11px] text-faint">
        {{ t("ac.empty") }}
      </div>

      <div v-else ref="cardsEl" class="grid grid-cols-1 gap-2.5 md:grid-cols-2 xl:grid-cols-3">
        <article
          v-for="(row, index) in orderedRows"
          :key="row.accountId"
          class="flex min-w-0 flex-col border bg-card"
          :class="[
            isCurrent(row) ? 'border-hazard shadow-[0_0_0_1px_var(--color-hazard)]' : 'border-border',
            row.ok ? '' : 'border-l-2 border-l-hazard',
          ]"
        >
          <!-- 卡头：序号 + 名称 + 档位 + 徽标（限流内联，不占额外高度） -->
          <div class="flex items-start justify-between gap-2 px-3 pt-2.5">
            <div class="min-w-0">
              <div class="flex min-w-0 items-center gap-1.5">
                <!-- 健康点：一眼扫出「哪个号快废了」，不占额外高度 -->
                <span
                  class="size-1.5 shrink-0 rounded-full"
                  :class="healthDot(row.health)"
                  :title="healthTitle(row.health)"
                />
                <span class="readout shrink-0 text-[9px] text-faint">{{ String(index + 1).padStart(2, "0") }}</span>
                <span class="min-w-0 truncate text-[12px] text-foreground" :title="mask(row.label)">
                  {{ mask(row.label) }}
                </span>
                <span class="lbl shrink-0 border border-border-strong px-1 py-0.5 text-[8px] text-muted-foreground">
                  {{ variantText(row.variant) }}
                </span>
                <span
                  v-if="isCurrent(row)"
                  class="lbl-ink flex shrink-0 items-center gap-0.5 border border-hazard bg-hazard-soft px-1 py-0.5 text-[8px] text-hazard"
                  :title="t('ac.currentBadge')"
                >
                  <CircleCheck class="size-2.5" />
                  {{ t("ac.currentBadge") }}
                </span>
                <span
                  v-if="isPriority(row)"
                  class="lbl-ink flex shrink-0 items-center gap-0.5 border border-hazard px-1 py-0.5 text-[8px] text-hazard"
                  :title="t('ac.priorityHint')"
                >
                  <Star class="size-2.5" />
                  {{ t("ac.priority") }}
                </span>
                <span
                  v-if="activeLimits(row).length > 0"
                  class="lbl-ink flex min-w-0 shrink items-center gap-0.5 border border-hazard px-1 py-0.5 text-[8px] text-hazard"
                  :title="limitTitle(activeLimits(row))"
                >
                  <Gauge class="size-2.5 shrink-0" />
                  <span class="min-w-0 truncate">{{ limitShort(row) }}</span>
                </span>
              </div>
              <div class="readout mt-0.5 truncate text-[9px] text-faint" :title="row.accountId">
                {{ row.accountId }}
              </div>
            </div>
            <div class="flex shrink-0 items-center gap-1">
              <button
                v-if="!isCurrent(row)"
                type="button"
                class="flex items-center gap-1 border border-border-strong px-1.5 py-0.5 text-[9px] text-muted-foreground transition-colors hover:border-hazard hover:text-hazard"
                :title="t('ac.switchTitle')"
                :disabled="switching"
                @click="openSwitch(row)"
              >
                <ArrowLeftRight class="size-2.5" />
                {{ t("ac.setCurrent") }}
              </button>
              <button
                type="button"
                class="flex items-center gap-1 border border-border-strong px-1.5 py-0.5 text-[9px] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground"
                :title="t('ac.edit')"
                @click="editingId === row.accountId ? cancelNote() : startNote(row)"
              >
                <Pencil class="size-2.5" />
                {{ t("ac.edit") }}
              </button>
            </div>
          </div>

          <!-- 余额读数 -->
          <div class="mt-2 flex items-end justify-between gap-2 px-3">
            <div class="flex min-w-0 items-baseline gap-1.5 overflow-hidden">
              <Zap class="size-3.5 shrink-0 self-center text-hazard" />
              <!-- 骨架只做遮罩，不卸载数字组件：卸载后重挂会直接显示终值，滚动动画就丢了 -->
              <span class="relative inline-flex min-w-[6rem] items-baseline">
                <span
                  class="readout text-[24px] leading-none text-foreground"
                  :class="refreshing && row.ok ? 'opacity-0' : ''"
                >
                  <AnimatedNumber :value="row.ok ? row.remaining : null" :decimals="0" />
                </span>
                <span
                  v-if="refreshing && row.ok"
                  class="absolute inset-y-0 left-0 my-auto h-[18px] w-24 animate-pulse bg-border-strong"
                />
              </span>
              <span class="lbl whitespace-nowrap text-faint">
                / {{ fmt(row.capacity) }}
              </span>
            </div>
            <div class="flex shrink-0 flex-col items-end gap-0.5">
              <!-- 预测放右列首行：余额行只留「数字 / 容量」，窄卡片下不会互相挤压 -->
              <span
                v-if="forecastLabel(forecastOf(row.accountId))"
                class="readout whitespace-nowrap text-[11px] leading-none"
                :class="forecastTone(forecastOf(row.accountId))"
                :title="forecastTitle(forecastOf(row.accountId))"
              >
                {{ forecastLabel(forecastOf(row.accountId)) }}
              </span>
              <!-- 次级信息合成一行：三行 9px 堆叠读不出主次 -->
              <span v-if="refreshing" class="inline-block h-2 w-28 animate-pulse bg-border" />
              <span v-else class="readout whitespace-nowrap text-[9px] leading-none text-faint">
                {{ row.packages }} {{ t("ac.packages") }}
                <span class="px-0.5">·</span>
                <span :title="`${t('ac.updated')} ${fmtTime(row.updatedAt)}`">{{ fmtTime(row.updatedAt) }}</span>
              </span>
            </div>
          </div>

          <!-- 余额进度 -->
          <div class="mt-1.5 px-3" aria-hidden="true">
            <div class="h-[3px] bg-border-strong">
              <!-- 余量越低越亮：<10% 转红（唯一需要警觉的档），其余按明度分层 -->
              <div
                class="h-full transition-[width] duration-300 ease-out"
                :class="balanceRatio(row) < 10 ? 'bg-hazard' : 'bg-foreground/85'"
                :style="{ width: `${balanceRatio(row)}%` }"
              />
            </div>
          </div>

          <!-- 近期到期：一行摘要（几个包 · 最近到期），明细进弹框 -->
          <div class="mt-2 flex flex-1 items-center gap-2 border-t border-border px-3 py-1.5">
            <span class="lbl shrink-0">// {{ t("ac.expiring") }}</span>
            <span v-if="refreshing" class="inline-block h-2.5 w-full max-w-48 animate-pulse bg-border" />
            <p
              v-else-if="!row.ok"
              class="min-w-0 flex-1 truncate text-[10px] text-hazard"
              :title="row.error || t('ac.noSnapshot')"
            >
              {{ row.error || t("ac.noSnapshot") }}
            </p>
            <p
              v-else-if="row.resources.length > 0"
              class="readout min-w-0 flex-1 truncate text-[10px]"
              :class="expiringTone(row)"
              :title="expiringSummary(row)"
            >
              {{ expiringSummary(row) }}
            </p>
            <p v-else class="min-w-0 flex-1 truncate text-[9px] text-faint">
              {{ mode === "local" ? t("ac.localHint") : t("ac.noPkg") }}
            </p>
            <button
              v-if="row.resources.length > 0"
              type="button"
              class="shrink-0 text-[9px] text-muted-foreground underline decoration-border-strong underline-offset-2 transition-colors hover:text-hazard"
              @click="openPackages(row)"
            >
              {{ t("ac.viewAll") }}
            </button>
          </div>

          <!-- 备注 / 删除 -->
          <div class="border-t border-border px-3 py-1.5">
            <div v-if="editingId === row.accountId" class="flex items-center gap-1">
              <input
                v-model="noteDraft"
                class="min-w-0 flex-1 border border-border-strong bg-background px-1.5 py-0.5 text-[10px] text-foreground outline-none focus:border-hazard"
                :placeholder="t('ac.note')"
                maxlength="40"
                @keyup.enter="saveNote(row)"
                @keyup.esc="cancelNote"
              />
              <button
                type="button"
                class="border border-border-strong px-1.5 py-0.5 text-[9px] text-foreground hover:border-foreground disabled:opacity-40"
                :disabled="savingNote"
                @click="saveNote(row)"
              >
                OK
              </button>
              <button type="button" class="px-1 text-[9px] text-faint hover:text-foreground" @click="cancelNote">
                ✕
              </button>
            </div>
            <div v-else class="flex items-center justify-between gap-2">
              <button
                type="button"
                class="min-w-0 flex-1 truncate text-left text-[9px] text-muted-foreground hover:text-foreground"
                :title="row.note ? mask(row.note) : t('ac.edit')"
                @click="startNote(row)"
              >
                <span class="lbl mr-1 text-faint">{{ t("ac.note") }}</span>
                {{ row.note ? mask(row.note) : "—" }}
              </button>
              <button
                type="button"
                class="flex shrink-0 items-center gap-1 border px-1.5 py-0.5 text-[9px] transition-colors"
                :class="
                  pendingDelete === row.accountId
                    ? 'border-hazard bg-hazard-soft text-hazard'
                    : 'border-border-strong text-muted-foreground hover:border-foreground hover:text-foreground'
                "
                @click="remove(row)"
              >
                <template v-if="pendingDelete === row.accountId">{{ t("ac.confirm") }}</template>
                <template v-else><Trash2 class="size-2.5" />{{ t("ac.del") }}</template>
              </button>
            </div>
          </div>
        </article>
      </div>
    </div>

    <!-- OAuth 扫码登录：采集完成后的收尾提示（扫码在浏览器里，界面必须回话） -->
    <Teleport to="body">
    <Transition name="wb-modal">

      <div
        v-if="loginCollected"
        class="fixed inset-0 z-50 flex items-center justify-center bg-background/80 p-4"
        role="dialog"
        aria-modal="true"
        @click.self="dismissCollected"
      >
        <div class="w-full max-w-[420px] border border-border-strong bg-card wb-modal-card">
          <header class="flex shrink-0 items-start justify-between gap-3 border-b border-border px-3 py-2 wb-flat">
            <div class="min-w-0">
              <div class="text-[13px] text-foreground">{{ t("ac.oauthTitle") }}</div>
              <div class="mt-0.5 text-[10px] leading-4 text-muted-foreground">{{ t("ac.oauthDesc") }}</div>
            </div>
            <button type="button" class="shrink-0 px-1 text-[11px] text-faint hover:text-foreground" @click="dismissCollected">
              ✕
            </button>
          </header>
          <div class="px-3 py-3">
            <div class="flex items-center gap-2 border border-border-strong bg-muted px-3 py-2.5">
              <CircleCheck class="size-3.5 shrink-0 text-hazard" />
              <span class="text-[11px] text-foreground">
                {{ t("ac.oauthCollected") }}：<span class="text-foreground">{{ mask(collectedName) }}</span>
              </span>
            </div>
          </div>
          <footer class="flex justify-end border-t border-border px-3 py-2">
            <button
              type="button"
              class="bg-primary px-4 py-1 text-[10px] uppercase tracking-[0.1em] text-primary-foreground transition-opacity hover:opacity-85"
              @click="dismissCollected"
            >
              {{ t("ac.oauthDone") }}
            </button>
          </footer>
        </div>
      </div>
    </Transition>
  </Teleport>

    <!-- 查看全部积分包（弹框）：卡片只露 2 条，完整明细在这里 -->
    <Teleport to="body">
    <Transition name="wb-modal">

      <div
        v-if="packageSheet"
        class="fixed inset-0 z-50 flex items-center justify-center bg-background/80 p-4"
        role="dialog"
        aria-modal="true"
        @click.self="closePackages"
      >
        <div class="flex max-h-[80vh] w-full max-w-[560px] flex-col border border-border-strong bg-card shadow-none wb-modal-card">
          <header class="flex shrink-0 items-start justify-between gap-3 border-b border-border px-3 py-2 wb-flat">
            <div class="min-w-0">
              <div class="lbl">// {{ t("ac.allPackages") }}</div>
              <div class="mt-0.5 truncate text-[12px] text-foreground" :title="mask(packageSheet.label)">
                {{ mask(packageSheet.label) }}
              </div>
            </div>
            <div class="flex shrink-0 items-center gap-2">
              <div class="readout text-right text-[9px] text-faint">
                <div class="flex items-center gap-1">Σ <AnimatedNumber :value="packageSheet.remaining" /> / {{ fmt(packageSheet.capacity) }}</div>
                <div>{{ sheetPackages.length }} {{ t("ac.packages") }}</div>
              </div>
              <button
                type="button"
                class="border border-border-strong px-2 py-1 text-[9px] uppercase tracking-[0.1em] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground"
                @click="closePackages"
              >
                {{ t("ac.close") }}
              </button>
            </div>
          </header>

          <div class="min-h-0 flex-1 overflow-y-auto">
            <div
              class="readout grid grid-cols-[1fr_78px_78px_92px] gap-2 border-b border-border px-3 py-1.5 text-[9px] text-faint"
            >
              <span>PACKAGE</span>
              <span class="text-right">{{ t("ac.remainShort") }}</span>
              <span class="text-right">{{ t("ac.totalShort") }}</span>
              <span class="text-right">{{ t("ac.expireAt") }}</span>
            </div>
            <ul v-if="sheetPackages.length > 0" class="divide-y divide-border">
              <li
                v-for="(item, index) in sheetPackages"
                :key="`${item.packageCode}-${index}`"
                class="px-3 py-2"
              >
                <div class="grid grid-cols-[1fr_78px_78px_92px] items-baseline gap-2 text-[10px]">
                  <span class="min-w-0 truncate text-foreground" :title="packageName(item)">
                    {{ packageName(item) }}
                  </span>
                  <span class="readout text-right text-foreground">{{ fmt(item.remaining) }}</span>
                  <span class="readout text-right text-muted-foreground">{{ fmt(item.total) }}</span>
                  <span
                    class="readout text-right text-[9px]"
                    :class="item.expired || item.expiringSoon ? 'text-hazard' : 'text-faint'"
                  >
                    {{ fmtExpire(item.expireAt) }}
                  </span>
                </div>
                <div class="mt-1 h-[2px] bg-muted" aria-hidden="true">
                  <div
                    class="h-full"
                    :class="item.expiringSoon || item.expired ? 'bg-hazard' : 'bg-foreground/55'"
                    :style="{ width: `${packageRatio(item)}%` }"
                  />
                </div>
                <div class="readout mt-1 flex justify-between text-[9px] text-faint">
                  <span>{{ t("ac.usedShort") }} {{ fmt(item.used) }}</span>
                  <span v-if="item.expiringSoon" class="text-hazard">{{ t("ac.expiringSoon") }}</span>
                </div>
              </li>
            </ul>
            <p v-else class="px-3 py-10 text-center text-[11px] text-faint">{{ t("ac.noPkg") }}</p>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
    <!-- 完全同步切换确认弹框：复刻原项目「切换到目标账号」的警示结构 -->
    <Teleport to="body">
    <Transition name="wb-modal">

      <div
        v-if="switchTarget"
        class="fixed inset-0 z-50 flex items-center justify-center bg-background/80 p-4"
        role="dialog"
        aria-modal="true"
        @click.self="closeSwitch"
      >
        <div class="flex max-h-[85vh] w-full max-w-[480px] flex-col border border-border-strong bg-card wb-modal-card">
          <header class="flex shrink-0 items-start justify-between gap-3 border-b border-border px-3 py-2 wb-flat">
            <div class="min-w-0">
              <div class="text-[13px] text-foreground">
                {{ t("ac.switchTitle") }}「{{ mask(switchTarget.label) }}」
              </div>
              <div class="readout mt-0.5 text-[9px] text-faint">{{ t("ac.switchSubtitle") }}</div>
            </div>
            <button
              type="button"
              class="shrink-0 px-1 text-[11px] text-faint hover:text-foreground"
              :disabled="switching"
              @click="closeSwitch"
            >
              ✕
            </button>
          </header>

          <div class="min-h-0 flex-1 overflow-y-auto px-3 py-3">
            <div class="border border-hazard bg-hazard-soft px-3 py-2.5">
              <div class="flex items-center gap-1.5 text-[11px] text-hazard">
                <CircleAlert class="size-3.5 shrink-0" />
                {{ t("ac.switchWarn") }}
              </div>
              <ul class="mt-1.5 list-disc space-y-1 pl-5 text-[10px] leading-4 text-muted-foreground">
                <li v-for="(bullet, bulletIndex) in switchBullets" :key="bulletIndex">{{ bullet }}</li>
              </ul>
            </div>

            <!-- 失败现场：阶段 + 是否回滚 + 原始原因，全部留在弹框里 -->
            <div v-if="switchError" class="mt-3 border border-hazard">
              <div
                class="flex items-center gap-1.5 border-b border-hazard bg-hazard-soft px-3 py-2 text-[11px] text-hazard"
              >
                <CircleAlert class="size-3.5 shrink-0" />
                {{
                  switchError.rolledBack
                    ? switchError.sessionsDirty
                      ? t("ac.switchRolledBackDirty")
                      : t("ac.switchRolledBack")
                    : t("ac.switchBlocked")
                }}
              </div>
              <div class="flex gap-2 border-b border-border px-3 py-1.5 text-[10px]">
                <span class="w-14 shrink-0 text-faint">{{ t("ac.switchStage") }}</span>
                <span class="readout min-w-0 flex-1 text-foreground">{{
                  stageLabel(switchError.stage)
                }}</span>
              </div>
              <div
                v-if="switchError.identity"
                class="flex gap-2 border-b border-border px-3 py-1.5 text-[10px]"
              >
                <span class="w-14 shrink-0 text-faint">{{ t("ac.switchIdentity") }}</span>
                <span class="readout min-w-0 flex-1 text-foreground">{{
                  switchError.identity
                }}</span>
              </div>
              <div
                v-if="switchError.sessionsDirty"
                class="flex gap-2 border-b border-border px-3 py-1.5 text-[10px]"
              >
                <span class="w-14 shrink-0 text-faint">{{ t("ac.switchSessions") }}</span>
                <span class="min-w-0 flex-1 leading-4 text-hazard">{{
                  t("ac.switchSessionsDirty")
                }}</span>
              </div>
              <p
                class="readout whitespace-pre-wrap break-words px-3 py-2 text-[10px] leading-4 text-muted-foreground"
              >
                {{ switchError.message }}
              </p>
              <button
                type="button"
                class="w-full border-t border-border px-3 py-1.5 text-left text-[10px] uppercase tracking-[0.1em] text-faint transition-colors hover:text-foreground"
                @click="
                  go('logs');
                  closeSwitch();
                "
              >
                {{ t("ac.switchViewLog") }}
              </button>
            </div>

            <p v-if="switching" class="readout mt-3 flex items-center gap-2 text-[10px] text-foreground">
              <Loader2 class="size-3 animate-spin" />
              {{ switchProgress || t("ac.switching") }}
            </p>
            <p v-else class="readout mt-3 text-[10px] text-muted-foreground">{{ t("ac.switchFoot") }}</p>
          </div>

          <footer class="flex shrink-0 items-center justify-end gap-2 border-t border-border px-3 py-2">
            <button
              type="button"
              class="border border-border-strong px-3 py-1 text-[10px] uppercase tracking-[0.1em] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground disabled:opacity-40"
              :disabled="switching"
              @click="closeSwitch"
            >
              {{ t("ac.close") }}
            </button>
            <button
              type="button"
              class="flex items-center gap-1.5 bg-hazard px-3 py-1 text-[10px] uppercase tracking-[0.1em] text-white transition-opacity hover:opacity-85 disabled:opacity-40"
              :disabled="switching"
              @click="doSwitch"
            >
              <Loader2 v-if="switching" class="size-3 animate-spin" />
              {{ switching ? t("ac.switching") : t("ac.switchGo") }}
            </button>
          </footer>
        </div>
      </div>
    </Transition>
  </Teleport>
  </div>
</template>
