<script setup lang="ts">
/**
 * 积分统计：四指标 → 每日柱 + 账号余额 → 按模型分类 → 积分明细（账号 / 流水）。
 * 数据来自 core 的 `credit_usage::get_statistics`（快照聚合，本地口径）；
 * 官方只读用量（officialUsage）可用时优先用于消耗类读数，不可用则回退本地观察。
 * 只有「刷新统计」会带 refresh=true 重新采集官方用量。
 */
import { computed, onMounted, ref, watch } from "vue";
import { ArrowDown, ArrowUp, CircleAlert, Loader2, RefreshCw } from "lucide-vue-next";

import { creditStatistics, type CreditOfficialUsage, type CreditStatistics } from "@/lib/api";
import { t } from "@/lib/i18n";
import { mask } from "@/lib/task-meta";

const stats = ref<CreditStatistics | null>(null);
const loading = ref(true);
const error = ref("");

async function load(refresh = false) {
  loading.value = true;
  error.value = "";
  try {
    stats.value = await creditStatistics(refresh);
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    loading.value = false;
  }
}

onMounted(() => void load(false));

const fmt = (value: number | null | undefined) => {
  if (value === null || value === undefined || !Number.isFinite(value)) return "—";
  return value.toLocaleString("zh-CN", { maximumFractionDigits: 0 });
};

const pct = (ratio: number | null | undefined) => {
  if (ratio === null || ratio === undefined || !Number.isFinite(ratio)) return "—";
  const percent = ratio * 100;
  return percent < 0.05 ? "<0.1%" : `${percent.toFixed(1)}%`;
};

/** 官方用量只有 complete / partial 可用；unavailable 时整页回退本地口径。 */
const official = computed<CreditOfficialUsage | null>(() => {
  const value = stats.value?.officialUsage;
  if (!value) return null;
  return value.status === "complete" || value.status === "partial" ? value : null;
});

const officialOffline = computed(() => Boolean(stats.value?.officialUsage) && official.value === null);

/** 消耗类读数：官方优先，缺失回退本地快照口径。 */
const summary = computed(() => {
  const local = stats.value?.summary;
  const remote = official.value?.summary;
  return {
    currentRemaining: local?.currentRemaining,
    usageToday: remote ? remote.usageToday : local?.usageToday,
    usage7Days: remote ? remote.usage7Days : local?.usage7Days,
    usageThisMonth: remote ? remote.usageThisMonth : local?.usageThisMonth,
  };
});

/** 图表取最近 20 个有数据的日子（后端按日期升序给出）。 */
const chart = computed(() => {
  const daily = stats.value?.daily ?? [];
  const tail = daily.slice(-20);
  const max = tail.reduce((peak, point) => Math.max(peak, point.usage), 0);
  return tail.map((point) => ({
    date: point.date.slice(5).replace("-", "/"),
    usage: point.usage,
    height: max > 0 ? Math.max(2, Math.round((point.usage / max) * 100)) : 2,
  }));
});

const chartMax = computed(() => {
  const tail = (stats.value?.daily ?? []).slice(-20);
  return tail.reduce((peak, point) => Math.max(peak, point.usage), 0);
});

const rows = computed(() =>
  [...(stats.value?.accounts ?? [])].sort(
    (left, right) =>
      (right.currentRemaining ?? 0) - (left.currentRemaining ?? 0) ||
      (right.usageThisMonth ?? 0) - (left.usageThisMonth ?? 0),
  ),
);

const topBalances = computed(() => rows.value.filter((row) => (row.currentRemaining ?? 0) >= 0).slice(0, 6));

const checkinText = (status: string | null | undefined) => {
  if (status === "success" || status === "already") return "YES";
  if (status === "error") return "ERR";
  return "—";
};

const updatedAt = computed(() => {
  const ts = official.value?.collectedAt ?? stats.value?.generatedAt;
  if (!ts) return "—";
  return new Date(ts).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
});

const variantText = (variant: string | null | undefined) =>
  variant === "ai" ? "INTL" : variant === "cn" ? "CN" : "—";

/** 账号行的消耗读数：官方账号口径优先，失败账号回退 null（显示 —）。 */
const officialByAccount = computed(() => {
  const map = new Map<string, CreditOfficialUsage["accounts"][number]>();
  for (const account of official.value?.accounts ?? []) map.set(account.accountId, account);
  return map;
});

const accountUsage = (
  row: { accountId: string; usageToday: number; usage7Days: number; usageThisMonth: number },
  key: "usageToday" | "usage7Days" | "usageThisMonth",
): number | null => {
  const remote = officialByAccount.value.get(row.accountId);
  if (remote) return remote[key] ?? null;
  return row[key] ?? null;
};

// ───────────────────────── 分类维度（模型 / 客户端 / 账号） ─────────────────────────

/** 维度切换：三份聚合同源（官方 30 天数据），切换只换分组口径，不重算统计。 */
type Dimension = "model" | "client" | "account";
const dimension = ref<Dimension>("model");

const dimensionOptions: { key: Dimension; labelKey: "cr.byModel" | "cr.byClientDim" | "cr.byAccount" }[] = [
  { key: "model", labelKey: "cr.byModel" },
  { key: "client", labelKey: "cr.byClientDim" },
  { key: "account", labelKey: "cr.byAccount" },
];

/** 模型聚合（官方 30 天全量，不受流水明细条数上限影响），按消耗降序。 */
const models = computed(() => {
  const raw = official.value?.models ?? [];
  const merged = new Map<string, { credit: number; requestCount: number }>();
  for (const item of raw) {
    const name = item.model?.trim() || "未知模型";
    const entry = merged.get(name) ?? { credit: 0, requestCount: 0 };
    entry.credit += item.credit;
    entry.requestCount += item.requestCount;
    merged.set(name, entry);
  }
  const list = [...merged.entries()]
    .map(([name, value]) => ({ name, ...value }))
    .filter((item) => item.credit > 0 || item.requestCount > 0)
    .sort(
      (left, right) =>
        right.credit - left.credit ||
        right.requestCount - left.requestCount ||
        left.name.localeCompare(right.name),
    );
  const totalCredit = list.reduce((sum, item) => sum + item.credit, 0);
  const peak = list[0]?.credit ?? 0;
  return list.map((item) => ({
    ...item,
    width: peak > 0 ? Math.max(1, Math.round((item.credit / peak) * 100)) : 0,
    share: totalCredit > 0 ? item.credit / totalCredit : 0,
  }));
});

/** 客户端分布按流水样本聚合（受每账号明细上限影响，仅作参考）。 */
const clients = computed(() => {
  const requests = official.value?.requests ?? [];
  const merged = new Map<string, { credit: number; requestCount: number }>();
  for (const row of requests) {
    const name = (row.client ?? "").trim() || "未知客户端";
    const entry = merged.get(name) ?? { credit: 0, requestCount: 0 };
    entry.credit += row.credit ?? 0;
    entry.requestCount += 1;
    merged.set(name, entry);
  }
  const list = [...merged.entries()]
    .map(([name, value]) => ({ name, ...value }))
    .sort((left, right) => right.credit - left.credit || right.requestCount - left.requestCount);
  const total = list.reduce((sum, item) => sum + item.credit, 0);
  const peak = list[0]?.credit ?? 0;
  const rows = list.map((item) => ({
    ...item,
    width: peak > 0 ? Math.max(1, Math.round((item.credit / peak) * 100)) : 0,
    share: total > 0 ? item.credit / total : 0,
  }));
  return { sample: requests.length, total, rows, top: rows.slice(0, 6) };
});

/** 账号维度：优先用账号自带的模型聚合求和（不受流水条数上限影响），缺失时回退流水。 */
const accountRows = computed(() => {
  const accounts = official.value?.accounts ?? [];
  const merged = new Map<string, { credit: number; requestCount: number }>();
  for (const account of accounts) {
    const name = (account.accountName || account.accountId || "").trim() || "未知账号";
    const models = account.models ?? [];
    let credit = models.reduce((sum, item) => sum + (item.credit ?? 0), 0);
    let count = models.reduce((sum, item) => sum + (item.requestCount ?? 0), 0);
    if (models.length === 0) {
      // 老后台只给流水：按 accountId 反查
      for (const row of official.value?.requests ?? []) {
        if (row.accountId !== account.accountId) continue;
        credit += row.credit ?? 0;
        count += 1;
      }
    }
    const entry = merged.get(name) ?? { credit: 0, requestCount: 0 };
    entry.credit += credit;
    entry.requestCount += count;
    merged.set(name, entry);
  }
  const list = [...merged.entries()]
    .map(([name, value]) => ({ name, ...value }))
    .filter((item) => item.credit > 0 || item.requestCount > 0)
    .sort((left, right) => right.credit - left.credit || left.name.localeCompare(right.name));
  const total = list.reduce((sum, item) => sum + item.credit, 0);
  const peak = list[0]?.credit ?? 0;
  const rows = list.map((item) => ({
    ...item,
    width: peak > 0 ? Math.max(1, Math.round((item.credit / peak) * 100)) : 0,
    share: total > 0 ? item.credit / total : 0,
  }));
  return { rows, top: rows.slice(0, 6), total, sample: accounts.length };
});

/** 当前维度下的排行（模板只认这一份，切换维度即换数据）。 */
const dimRows = computed(() => {
  // 0 消耗的行不参与排名：原来 15 项里有 4 项是 0，白占四行还稀释了占比
  const dropZero = (rows: typeof models.value) => rows.filter((row) => (row.credit ?? 0) > 0);
  if (dimension.value === "client") return dropZero(clients.value.rows);
  if (dimension.value === "account") return dropZero(accountRows.value.rows);
  return dropZero(models.value);
});

/** 维度汇总：项数 / 合计 / 单次均耗 / TOP1 占比，全部按当前维度重算。 */
const dimStats = computed(() => {
  const list = dimRows.value;
  const totalCredit = list.reduce((sum, item) => sum + item.credit, 0);
  const totalRequests = list.reduce((sum, item) => sum + item.requestCount, 0);
  return {
    count: list.length,
    totalCredit,
    totalRequests,
    avgCost: totalRequests > 0 ? totalCredit / totalRequests : null,
    topShare: list[0]?.share ?? null,
    topName: list[0]?.name ?? "—",
  };
});

/** 右侧副卡：与左列互补——左列是客户端时右列看模型，其余情况看客户端。 */
const secondary = computed(() => {
  if (dimension.value === "client") {
    const total = models.value.reduce((sum, item) => sum + item.credit, 0);
    return {
      titleKey: "cr.models" as const,
      sample: models.value.reduce((sum, item) => sum + item.requestCount, 0),
      sampleKey: "cr.reqCount" as const,
      rows: models.value.slice(0, 6),
      total,
    };
  }
  return {
    titleKey: "cr.client" as const,
    sample: clients.value.sample,
    sampleKey: "cr.reqCount" as const,
    rows: clients.value.top,
    total: clients.value.total,
  };
});

// ───────────────────────── 积分明细 ─────────────────────────

const detailTab = ref<"accounts" | "flow">("accounts");
const flowSort = ref<"time" | "credit">("time");
const flowDesc = ref(true);
const flowPage = ref(0);
const FLOW_PAGE_SIZE = 60;

/** 请求时间形如 "2026-10-30 19:01:45"，按字典序即时间序。 */
const flowSorted = computed(() => {
  const list = [...(official.value?.requests ?? [])];
  list.sort((left, right) => {
    const order =
      flowSort.value === "credit"
        ? (left.credit ?? 0) - (right.credit ?? 0)
        : String(left.requestTime ?? "").localeCompare(String(right.requestTime ?? ""));
    if (order === 0) return String(left.requestId ?? "").localeCompare(String(right.requestId ?? ""));
    return flowDesc.value ? -order : order;
  });
  return list;
});

const flowPages = computed(() => Math.max(1, Math.ceil(flowSorted.value.length / FLOW_PAGE_SIZE)));

const flowRows = computed(() => {
  const start = Math.min(flowPage.value, flowPages.value - 1) * FLOW_PAGE_SIZE;
  return flowSorted.value.slice(start, start + FLOW_PAGE_SIZE);
});

const flowRange = computed(() => {
  const total = flowSorted.value.length;
  if (total === 0) return "0 / 0";
  const start = Math.min(flowPage.value, flowPages.value - 1) * FLOW_PAGE_SIZE + 1;
  return `${start}–${Math.min(start + FLOW_PAGE_SIZE - 1, total)} / ${total}`;
});

const flowTruncated = computed(() =>
  (official.value?.accounts ?? []).some((account) => account.detailTruncated),
);

function toggleFlowSort(key: "time" | "credit") {
  if (flowSort.value === key) {
    flowDesc.value = !flowDesc.value;
  } else {
    flowSort.value = key;
    flowDesc.value = true;
  }
}

// 换排序 / 换页数据源时回到第一页，避免停在越界页
watch([flowSort, flowDesc, detailTab], () => {
  flowPage.value = 0;
});

function stepFlowPage(delta: number) {
  const next = flowPage.value + delta;
  if (next < 0 || next >= flowPages.value) return;
  flowPage.value = next;
}

/** 流水时间列只保留 MM-DD HH:mm，年份在统计区间里已有。 */
const flowTime = (value: string) => (value ? value.slice(5, 16) : "—");
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <!-- 页头 -->
    <header class="flex shrink-0 items-center justify-between gap-3 border-b border-border bg-panel px-4 py-2 wb-flat">
      <div class="min-w-0">
        <h1 class="text-[13px] tracking-[0.12em] text-foreground">{{ t("cr.title") }}</h1>
        <div class="readout mt-0.5 text-[9px] text-faint">
          {{ t("cr.updated") }} {{ updatedAt }}
          <span v-if="official" class="ml-2 text-muted-foreground">
            {{ t("cr.official") }} · {{ t("cr.range") }} {{ official.rangeStart }} → {{ official.rangeEnd }}
          </span>
          <span v-else-if="stats" class="ml-2 text-muted-foreground">{{ t("cr.local") }}</span>
        </div>
      </div>
      <button
        type="button"
        class="flex items-center gap-1.5 border border-border-strong px-2.5 py-1 text-[10px] uppercase tracking-[0.1em] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground disabled:opacity-40"
        :disabled="loading"
        @click="load(true)"
      >
        <Loader2 v-if="loading" class="size-3 animate-spin" />
        <RefreshCw v-else class="size-3" />
        {{ t("cr.refresh") }}
      </button>
    </header>

    <div v-if="error" class="flex shrink-0 items-center gap-2 border-b border-hazard bg-hazard-soft px-4 py-2 text-[11px] text-hazard">
      <CircleAlert class="size-3.5 shrink-0" />
      <span class="truncate">{{ error }}</span>
    </div>

    <!-- 四指标 -->
    <section class="mx-3 mt-3 shrink-0">
      <div class="grid grid-cols-4 gap-2">
        <div class="border border-border bg-card px-3 py-2.5">
          <div class="lbl">{{ t("cr.left") }}</div>
          <div class="readout mt-1 text-[20px] leading-none text-foreground">
            {{ loading && !stats ? "···" : fmt(summary.currentRemaining) }}
          </div>
        </div>
        <div class="border border-border bg-card px-3 py-2.5">
          <div class="lbl">{{ t("cr.today") }}</div>
          <div class="readout mt-1 text-[20px] leading-none text-foreground">
            {{ loading && !stats ? "···" : fmt(summary.usageToday) }}
          </div>
        </div>
        <div class="border border-border bg-card px-3 py-2.5">
          <div class="lbl">{{ t("cr.d7") }}</div>
          <div class="readout mt-1 text-[20px] leading-none text-foreground">
            {{ loading && !stats ? "···" : fmt(summary.usage7Days) }}
          </div>
        </div>
        <div class="border border-border bg-card px-3 py-2.5">
          <div class="lbl">{{ t("cr.month") }}</div>
          <div class="readout mt-1 text-[20px] leading-none text-foreground">
            {{ loading && !stats ? "···" : fmt(summary.usageThisMonth) }}
          </div>
        </div>
      </div>
    </section>

    <div class="min-h-0 flex-1 space-y-3 overflow-y-auto p-3">
      <div class="grid grid-cols-[1fr_260px] gap-3">
        <!-- 每日消耗 -->
        <section class="border border-border bg-card px-3 py-2.5">
          <div class="lbl">// {{ t("cr.daily") }}</div>
          <div v-if="chart.length === 0" class="py-10 text-center text-[11px] text-faint">
            {{ t("cr.empty") }}
          </div>
          <div v-else class="mt-2">
            <div class="relative mt-1 h-[132px]">
              <!-- 三条参考线：让高度可读，不用再加 Y 轴刻度 -->
              <div class="pointer-events-none absolute inset-x-0 top-0 h-px bg-border/60" />
              <div class="pointer-events-none absolute inset-x-0 top-1/3 h-px bg-border/40" />
              <div class="pointer-events-none absolute inset-x-0 top-2/3 h-px bg-border/40" />
              <div class="flex h-full items-end justify-center gap-[3px] border-b border-border pb-px wb-bars">
                <div
                  v-for="(point, index) in chart"
                  :key="`${point.date}-${index}`"
                  class="origin-bottom transition-[background-color,transform] duration-300"
                  :class="
                    point.usage > 0 && point.usage === chartMax
                      ? 'w-full max-w-[46px] bg-hazard hover:bg-hazard/85'
                      : 'w-full max-w-[46px] bg-foreground/35 hover:bg-foreground/70'
                  "
                  :style="{ height: `${point.height}%` }"
                  :title="`${point.date} · ${fmt(point.usage)}`"
                />
              </div>
            </div>
            <div class="readout mt-1 flex justify-between text-[9px] text-faint">
              <span>{{ chart[0]?.date }}</span>
              <span>MAX {{ fmt(chartMax) }}</span>
              <span>{{ chart[chart.length - 1]?.date }}</span>
            </div>
          </div>
        </section>

        <!-- 账号余额 -->
        <section class="border border-border bg-card px-3 py-2.5">
          <div class="lbl">// {{ t("cr.remain") }}</div>
          <ul v-if="topBalances.length > 0" class="mt-1.5 space-y-1">
            <li
              v-for="row in topBalances"
              :key="row.accountId"
              class="flex items-baseline justify-between gap-2 text-[10px]"
            >
              <span class="min-w-0 truncate text-muted-foreground" :title="mask(row.accountName ?? row.accountId)">
                {{ mask(row.accountName ?? row.accountId) }}
              </span>
              <span class="readout shrink-0 text-foreground">{{ fmt(row.currentRemaining) }}</span>
            </li>
          </ul>
          <div v-else class="py-8 text-center text-[10px] text-faint">{{ t("cr.empty") }}</div>
          <div class="readout mt-2 border-t border-border pt-1.5 text-[9px] text-faint">
            ACCTS {{ String(rows.length).padStart(2, "0") }}
          </div>
        </section>
      </div>

      <!-- 分类维度：模型 / 客户端 / 账号 -->
      <section class="border border-border bg-card">
        <div class="flex flex-wrap items-center justify-between gap-2 border-b border-border px-3 py-2">
          <div class="flex items-center gap-2">
            <span class="lbl">// {{ t("cr.dim") }}</span>
            <div class="flex gap-px bg-border overflow-hidden">
              <button
                v-for="option in dimensionOptions"
                :key="option.key"
                type="button"
                class="px-2 py-0.5 text-[10px] tracking-[0.1em] transition-colors"
                :class="
                  dimension === option.key
                    ? 'bg-foreground text-background'
                    : 'bg-card text-muted-foreground hover:text-foreground'
                "
                @click="dimension = option.key"
              >
                {{ t(option.labelKey) }}
              </button>
            </div>
          </div>
          <span class="readout text-[9px] text-faint">
            {{ dimStats.count }} {{ t("cr.modelCount") }} · {{ fmt(dimStats.totalRequests) }} {{ t("cr.reqCount") }}
          </span>
        </div>

        <div v-if="!official" class="px-3 py-10 text-center text-[11px] text-faint">
          {{ officialOffline ? t("cr.flowOffline") : t("cr.empty") }}
        </div>
        <div v-else-if="dimRows.length === 0" class="px-3 py-10 text-center text-[11px] text-faint">
          {{ t("cr.flowEmpty") }}
        </div>
        <div v-else class="grid grid-cols-[1fr_252px] gap-px bg-border">
          <!-- 模型排行 -->
          <div class="min-w-0 bg-card px-3 py-2">
            <div class="readout grid grid-cols-[18px_1fr_64px_52px_52px] gap-2 border-b border-border pb-1 text-[9px] text-faint">
              <span>#</span>
              <span>{{ dimension === "model" ? "MODEL" : dimension === "client" ? "CLIENT" : "ACCOUNT" }}</span>
              <span class="text-right">{{ t("cr.flowCredit") }}</span>
              <span class="text-right">{{ t("cr.reqCount") }}</span>
              <span class="text-right">{{ t("cr.share") }}</span>
            </div>
            <ul class="mt-1 space-y-1">
              <li
                v-for="(model, index) in dimRows"
                :key="`${model.name}-${index}`"
                class="grid grid-cols-[18px_1fr_64px_52px_52px] items-center gap-2 text-[10px]"
              >
                <span class="readout text-faint">{{ String(index + 1).padStart(2, "0") }}</span>
                <span class="min-w-0">
                  <span class="block truncate text-foreground" :title="model.name">{{ model.name }}</span>
                  <span class="mt-1 block h-[3px] bg-muted">
                    <span class="block h-full" :class="index === 0 ? 'bg-foreground/90' : 'bg-foreground/45'" :style="{ width: `${model.width}%` }" />
                  </span>
                </span>
                <span class="readout text-right text-foreground">{{ fmt(model.credit) }}</span>
                <span class="readout text-right text-muted-foreground">{{ fmt(model.requestCount) }}</span>
                <span class="readout text-right" :class="index === 0 ? 'text-hazard' : 'text-muted-foreground'">
                  {{ pct(model.share) }}
                </span>
              </li>
            </ul>
          </div>

          <!-- 模型汇总 + 客户端 -->
          <div class="min-w-0 bg-card">
            <div class="px-3 py-2">
              <div class="lbl">// {{ t("cr.totalCredit") }}</div>
              <div class="readout mt-1 text-[18px] leading-none text-foreground">{{ fmt(dimStats.totalCredit) }}</div>
              <dl class="readout mt-2 space-y-1 text-[9px]">
                <div class="flex justify-between gap-2">
                  <dt class="text-faint">{{ t("cr.avgCost") }}</dt>
                  <dd class="text-muted-foreground">{{ dimStats.avgCost === null ? "—" : dimStats.avgCost.toFixed(2) }}</dd>
                </div>
                <div class="flex justify-between gap-2">
                  <dt class="text-faint">TOP1 {{ t("cr.share") }}</dt>
                  <dd class="text-muted-foreground">{{ pct(dimStats.topShare) }}</dd>
                </div>
                <div class="flex justify-between gap-2">
                  <dt class="text-faint">{{ t("cr.dimItems") }}</dt>
                  <dd class="text-muted-foreground">{{ dimStats.count }}</dd>
                </div>
              </dl>
              <div class="readout mt-1.5 truncate text-[9px] text-faint" :title="dimStats.topName">
                TOP1 · {{ dimStats.topName }}
              </div>
            </div>
            <div class="border-t border-border px-3 py-2">
              <div class="lbl">// {{ t(secondary.titleKey) }}</div>
              <ul v-if="secondary.rows.length > 0" class="mt-1.5 space-y-1">
                <li v-for="client in secondary.rows" :key="client.name" class="min-w-0">
                  <div class="flex items-baseline justify-between gap-2 text-[10px]">
                    <span class="min-w-0 truncate text-muted-foreground" :title="client.name">{{ client.name }}</span>
                    <span class="readout shrink-0 text-foreground">{{ fmt(client.credit) }}</span>
                  </div>
                  <div class="mt-1 h-[3px] bg-muted">
                    <div class="h-full bg-foreground/55" :style="{ width: `${client.width}%` }" />
                  </div>
                </li>
              </ul>
              <div v-else class="py-4 text-center text-[10px] text-faint">{{ t("cr.flowEmpty") }}</div>
              <div class="readout mt-1.5 text-[9px] text-faint">
                SAMPLE {{ secondary.sample }} {{ t(secondary.sampleKey) }}
              </div>
            </div>
          </div>
        </div>
      </section>

      <!-- 积分明细 -->
      <section class="border border-border bg-card">
        <div class="flex items-center justify-between gap-2 border-b border-border px-3 py-2">
          <span class="lbl">// {{ t("cr.detail") }}</span>
          <div class="flex gap-px bg-border overflow-hidden">
            <button
              v-for="tab in (['accounts', 'flow'] as const)"
              :key="tab"
              type="button"
              class="px-2.5 py-1 text-[10px] tracking-[0.1em] transition-colors"
              :class="detailTab === tab ? 'bg-foreground text-background' : 'bg-card text-muted-foreground hover:text-foreground'"
              @click="detailTab = tab"
            >
              {{ tab === "accounts" ? t("cr.tabAcct") : t("cr.tabFlow") }}
            </button>
          </div>
        </div>

        <!-- 账号口径 -->
        <template v-if="detailTab === 'accounts'">
          <div class="grid grid-cols-[1fr_70px_78px_78px_88px_64px] gap-px bg-border text-[10px] wb-cells">
            <div class="lbl bg-card px-3 py-1.5">{{ t("cr.acct") }}</div>
            <div class="lbl bg-card px-2 py-1.5 text-right">{{ t("cr.remain") }}</div>
            <div class="lbl bg-card px-2 py-1.5 text-right">{{ t("cr.today") }}</div>
            <div class="lbl bg-card px-2 py-1.5 text-right">{{ t("cr.d7") }}</div>
            <div class="lbl bg-card px-2 py-1.5 text-right">{{ t("cr.month") }}</div>
            <div class="lbl bg-card px-2 py-1.5 text-center">{{ t("cr.checkin") }}</div>

            <template v-for="row in rows" :key="row.accountId">
              <div class="min-w-0 bg-card px-3 py-1.5">
                <div class="truncate text-foreground" :title="mask(row.accountName ?? row.accountId)">
                  {{ mask(row.accountName ?? row.accountId) }}
                </div>
                <div class="readout text-[9px] text-faint">
                  {{ variantText(row.variant) }} · {{ row.accountId.slice(0, 10) }}
                </div>
              </div>
              <div class="readout bg-card px-2 py-1.5 text-right text-foreground">{{ fmt(row.currentRemaining) }}</div>
              <div class="readout bg-card px-2 py-1.5 text-right text-muted-foreground">{{ fmt(accountUsage(row, "usageToday")) }}</div>
              <div class="readout bg-card px-2 py-1.5 text-right text-muted-foreground">{{ fmt(accountUsage(row, "usage7Days")) }}</div>
              <div class="readout bg-card px-2 py-1.5 text-right text-muted-foreground">{{ fmt(accountUsage(row, "usageThisMonth")) }}</div>
              <div
                class="readout bg-card px-2 py-1.5 text-center"
                :class="checkinText(row.checkinStatusToday) === 'YES' ? 'text-foreground' : 'text-hazard'"
              >
                {{ checkinText(row.checkinStatusToday) }}
              </div>
            </template>
          </div>
          <div v-if="rows.length === 0 && !loading" class="px-3 py-10 text-center text-[11px] text-faint">
            {{ t("cr.empty") }}
          </div>
        </template>

        <!-- 流水口径 -->
        <template v-else>
          <div v-if="!official" class="flex items-center gap-2 px-3 py-10 text-[11px] text-faint">
            <CircleAlert class="size-3.5 shrink-0" />
            {{ officialOffline ? t("cr.flowOffline") : t("cr.empty") }}
          </div>
          <template v-else>
            <div v-if="flowTruncated" class="flex items-center gap-2 border-b border-border bg-hazard-soft px-3 py-1.5 text-[10px] text-hazard">
              <CircleAlert class="size-3 shrink-0" />
              {{ t("cr.flowTrunc") }} {{ official.detailLimitPerAccount }} {{ t("cr.flowRows") }}
            </div>
            <div class="grid grid-cols-[92px_1fr_68px_150px_90px] gap-px bg-border text-[10px] wb-cells">
              <button
                type="button"
                class="lbl flex items-center gap-1 bg-card px-3 py-1.5 text-left transition-colors hover:text-foreground"
                @click="toggleFlowSort('time')"
              >
                {{ t("cr.flowTime") }}
                <ArrowDown v-if="flowSort === 'time' && flowDesc" class="size-2.5" />
                <ArrowUp v-else-if="flowSort === 'time'" class="size-2.5" />
              </button>
              <div class="lbl bg-card px-2 py-1.5">{{ t("cr.flowAcct") }}</div>
              <button
                type="button"
                class="lbl flex items-center justify-end gap-1 bg-card px-2 py-1.5 text-right transition-colors hover:text-foreground"
                @click="toggleFlowSort('credit')"
              >
                {{ t("cr.flowCredit") }}
                <ArrowDown v-if="flowSort === 'credit' && flowDesc" class="size-2.5" />
                <ArrowUp v-else-if="flowSort === 'credit'" class="size-2.5" />
              </button>
              <div class="lbl bg-card px-2 py-1.5">{{ t("cr.flowModel") }}</div>
              <div class="lbl bg-card px-2 py-1.5">{{ t("cr.flowClient") }}</div>

              <template v-for="(row, index) in flowRows" :key="`${row.requestId}-${row.requestTime}-${index}`">
                <div class="readout bg-card px-3 py-1.5 text-faint">{{ flowTime(row.requestTime) }}</div>
                <div class="min-w-0 bg-card px-2 py-1.5">
                  <span class="block truncate text-foreground" :title="mask(row.accountName)">
                    {{ mask(row.accountName) }}
                  </span>
                </div>
                <div class="readout bg-card px-2 py-1.5 text-right text-hazard">{{ fmt(row.credit) }}</div>
                <div class="min-w-0 bg-card px-2 py-1.5">
                  <span class="block truncate text-muted-foreground" :title="row.model">{{ row.model || "—" }}</span>
                </div>
                <div class="min-w-0 bg-card px-2 py-1.5">
                  <span class="block truncate text-faint" :title="row.client">{{ row.client || "—" }}</span>
                </div>
              </template>
            </div>
            <div v-if="flowRows.length === 0" class="px-3 py-10 text-center text-[11px] text-faint">
              {{ t("cr.flowEmpty") }}
            </div>
            <div
              v-else
              class="readout flex items-center justify-between gap-2 border-t border-border px-3 py-1.5 text-[9px] text-faint"
            >
              <span>{{ flowRange }}</span>
              <div class="flex items-center gap-px bg-border overflow-hidden">
                <button
                  type="button"
                  class="bg-card px-2 py-0.5 text-muted-foreground transition-colors hover:text-foreground disabled:opacity-30"
                  :disabled="flowPage <= 0"
                  @click="stepFlowPage(-1)"
                >
                  PREV
                </button>
                <span class="bg-card px-2 py-0.5 text-foreground">{{ flowPage + 1 }} / {{ flowPages }}</span>
                <button
                  type="button"
                  class="bg-card px-2 py-0.5 text-muted-foreground transition-colors hover:text-foreground disabled:opacity-30"
                  :disabled="flowPage >= flowPages - 1"
                  @click="stepFlowPage(1)"
                >
                  NEXT
                </button>
              </div>
            </div>
          </template>
        </template>
      </section>
    </div>
  </div>
</template>
