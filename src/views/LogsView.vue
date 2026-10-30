<script setup lang="ts">
/**
 * 运行日志：通知存档 + 自动日常记录 + 错误日志，三源合并成一条时间轴。
 *
 * 为什么合并：这三处原本各存各的（notifications.json / auto_daily_logs.json /
 * error.log），出问题时要在三个地方翻。这里统一按时间倒序排，配来源筛选与关键词
 * 搜索，形成「做了什么 → 结果如何 → 报了什么错」的单一视图。
 * 清空是分源操作（只删当前筛选的那一源），不会误伤账号库与积分快照。
 */
import { computed, onMounted, ref } from "vue";
import { CircleAlert, Eraser, Loader2, RefreshCw, Search } from "lucide-vue-next";

import { eventsClear, eventsList, type EventList, type EventSource, type RunEvent } from "@/lib/api";
import { t } from "@/lib/i18n";

type Filter = "all" | EventSource;

const data = ref<EventList | null>(null);
const loading = ref(true);
const error = ref("");
const filter = ref<Filter>("all");
const keyword = ref("");
const clearing = ref(false);

async function load() {
  loading.value = true;
  error.value = "";
  try {
    data.value = await eventsList(300);
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    loading.value = false;
  }
}

onMounted(() => void load());

const stats = computed(
  () =>
    data.value?.stats ?? {
      total: 0,
      today: 0,
      warning: 0,
      error: 0,
      notify: 0,
      auto: 0,
      errlog: 0,
    },
);

/** 来源计数：筛选 tab 上的数字直接读这里，避免标签与分布条口径不一致。 */
const sourceCount = (source: EventSource) =>
  source === "notify" ? stats.value.notify : source === "auto" ? stats.value.auto : stats.value.errlog;

const FILTERS: { key: Filter; labelKey: "rl.all" | "rl.srcNotify" | "rl.srcAuto" | "rl.srcError" }[] = [
  { key: "all", labelKey: "rl.all" },
  { key: "notify", labelKey: "rl.srcNotify" },
  { key: "auto", labelKey: "rl.srcAuto" },
  { key: "error", labelKey: "rl.srcError" },
];

const rows = computed<RunEvent[]>(() => {
  const items = data.value?.items ?? [];
  const needle = keyword.value.trim().toLowerCase();
  return items.filter((item) => {
    if (filter.value !== "all" && item.source !== filter.value) return false;
    if (!needle) return true;
    return `${item.title} ${item.detail ?? ""}`.toLowerCase().includes(needle);
  });
});

const levelColor = (level: RunEvent["level"]) => {
  switch (level) {
    case "success":
      return "text-success";
    case "warning":
      return "text-hazard";
    case "error":
      return "text-hazard";
    default:
      return "text-muted-foreground";
  }
};

const levelBar = (level: RunEvent["level"]) => {
  switch (level) {
    case "success":
      return "bg-success";
    case "warning":
      return "bg-hazard";
    case "error":
      return "bg-hazard";
    default:
      return "bg-border-strong";
  }
};

const sourceLabel = (source: EventSource) =>
  source === "notify" ? t("rl.srcNotify") : source === "auto" ? t("rl.srcAuto") : t("rl.srcError");

/** 来源分布条的宽度（按总数归一，全 0 时不画条）。 */
const distribution = computed(() => {
  const total = Math.max(1, stats.value.notify + stats.value.auto + stats.value.errlog);
  return [
    { key: "notify" as const, count: stats.value.notify, ratio: (stats.value.notify / total) * 100, tone: "bg-foreground/70" },
    { key: "auto" as const, count: stats.value.auto, ratio: (stats.value.auto / total) * 100, tone: "bg-hazard" },
    { key: "error" as const, count: stats.value.errlog, ratio: (stats.value.errlog / total) * 100, tone: "bg-hazard/50" },
  ];
});

/** 24 小时分布：全部事件按本地小时归堆，最忙的一小时决定满格高度。 */
const hourly = computed(() => {
  const buckets = Array.from({ length: 24 }, () => 0);
  for (const item of data.value?.items ?? []) {
    if (!item.at) continue;
    buckets[new Date(item.at).getHours()] += 1;
  }
  const peak = Math.max(1, ...buckets);
  return buckets.map((count, hour) => ({ hour, count, ratio: count / peak }));
});

const pad = (value: number) => String(value).padStart(2, "0");

const dateOf = (at: number) => {
  if (!at) return "—";
  const d = new Date(at);
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
};

const clockOf = (at: number) => {
  if (!at) return "--:--:--";
  const d = new Date(at);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
};

const relative = (at: number) => {
  if (!at) return "";
  const diff = Date.now() - at;
  if (diff < 60_000) return "刚刚";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`;
  return `${Math.floor(diff / 86_400_000)} 天前`;
};

const clearTarget = ref<EventSource | null>(null);
const confirmText = computed(() => {
  const source = clearTarget.value ?? "notify";
  return t("rl.clearConfirm").replace("{src}", sourceLabel(source));
});

/** 只有单一来源筛选时才能清空——「全部」下清空等于删三份存档，不给这个口子。 */
const canClear = computed(() => filter.value !== "all");

async function doClear() {
  const source = clearTarget.value;
  if (!source) return;
  clearing.value = true;
  try {
    const result = await eventsClear(source);
    if (!result.ok) throw new Error(result.error || "clear failed");
    clearTarget.value = null;
    await load();
  } catch (cause) {
    error.value = `${t("rl.clearFail")}: ${cause instanceof Error ? cause.message : String(cause)}`;
    clearTarget.value = null;
  } finally {
    clearing.value = false;
  }
}
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <!-- 页头 -->
    <header class="flex shrink-0 items-center justify-between gap-3 border-b border-border bg-panel px-4 py-2">
      <div class="min-w-0">
        <h1 class="text-[13px] tracking-[0.12em] text-foreground">{{ t("rl.title") }}</h1>
        <div class="readout mt-0.5 text-[9px] text-faint">
          {{ t("rl.subtitle") }}
          <span v-if="data?.truncated" class="ml-2 text-muted-foreground">{{ t("rl.truncated") }}</span>
        </div>
      </div>
      <div class="flex shrink-0 items-center gap-1.5">
        <button
          type="button"
          class="flex items-center gap-1.5 border border-border-strong px-2.5 py-1 text-[10px] uppercase tracking-[0.1em] text-muted-foreground transition-colors hover:border-hazard hover:text-hazard disabled:opacity-40"
          :disabled="!canClear"
          :title="canClear ? t('rl.clear') : t('rl.all')"
          @click="clearTarget = filter as EventSource"
        >
          <Eraser class="size-3" />
          {{ t("rl.clear") }}
        </button>
        <button
          type="button"
          class="flex items-center gap-1.5 border border-border-strong px-2.5 py-1 text-[10px] uppercase tracking-[0.1em] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground disabled:opacity-40"
          :disabled="loading"
          @click="load()"
        >
          <Loader2 v-if="loading" class="size-3 animate-spin" />
          <RefreshCw v-else class="size-3" />
          {{ t("rl.refresh") }}
        </button>
      </div>
    </header>

    <div v-if="error" class="flex shrink-0 items-center gap-2 border-b border-hazard bg-hazard-soft px-4 py-2 text-[11px] text-hazard">
      <CircleAlert class="size-3.5 shrink-0" />
      <span class="truncate">{{ error }}</span>
    </div>
    <div
      v-else-if="data && data.loadErrors.length > 0"
      class="flex shrink-0 items-center gap-2 border-b border-border bg-panel px-4 py-1.5 text-[10px] text-muted-foreground"
    >
      <CircleAlert class="size-3 shrink-0" />
      <span class="truncate">{{ t("rl.loadErr") }} · {{ data.loadErrors.join(" / ") }}</span>
    </div>

    <!-- 四指标 -->
    <section class="shrink-0 border-b border-border bg-border">
      <div class="grid grid-cols-4 gap-px">
        <div class="bg-card px-3 py-2.5">
          <div class="lbl">{{ t("rl.total") }}</div>
          <div class="readout mt-1 text-[20px] leading-none text-foreground">
            {{ loading && !data ? "···" : stats.total }}
          </div>
        </div>
        <div class="bg-card px-3 py-2.5">
          <div class="lbl">{{ t("rl.today") }}</div>
          <div class="readout mt-1 text-[20px] leading-none text-foreground">
            {{ loading && !data ? "···" : stats.today }}
          </div>
        </div>
        <div class="bg-card px-3 py-2.5">
          <div class="lbl">{{ t("rl.warn") }}</div>
          <div class="readout mt-1 text-[20px] leading-none" :class="stats.warning > 0 ? 'text-hazard' : 'text-foreground'">
            {{ loading && !data ? "···" : stats.warning }}
          </div>
        </div>
        <div class="bg-card px-3 py-2.5">
          <div class="lbl">{{ t("rl.error") }}</div>
          <div class="readout mt-1 text-[20px] leading-none" :class="stats.error > 0 ? 'text-hazard' : 'text-foreground'">
            {{ loading && !data ? "···" : stats.error }}
          </div>
        </div>
      </div>
    </section>

    <!-- 来源分布 + 24H 分布 -->
    <section class="grid shrink-0 grid-cols-1 gap-px border-b border-border bg-border lg:grid-cols-2">
      <div class="bg-card px-3 py-2.5">
        <div class="flex items-baseline justify-between">
          <span class="lbl">{{ t("rl.sourceDist") }}</span>
          <span class="readout text-[9px] text-faint">{{ stats.total }} EVENTS</span>
        </div>
        <div class="mt-2 flex h-2 w-full overflow-hidden bg-muted">
          <div
            v-for="seg in distribution"
            :key="seg.key"
            :class="seg.tone"
            :style="{ width: `${seg.ratio}%` }"
            :title="`${sourceLabel(seg.key)} · ${seg.count}`"
          />
        </div>
        <div class="mt-2 flex flex-wrap items-center gap-x-4 gap-y-1">
          <span v-for="seg in distribution" :key="`legend-${seg.key}`" class="readout flex items-center gap-1.5 text-[9px] text-muted-foreground">
            <span class="inline-block size-1.5" :class="seg.tone" />
            {{ sourceLabel(seg.key) }}
            <span class="text-foreground">{{ seg.count }}</span>
          </span>
        </div>
      </div>

      <div class="bg-card px-3 py-2.5">
        <div class="flex items-baseline justify-between">
          <span class="lbl">{{ t("rl.hourDist") }}</span>
          <span class="readout text-[9px] text-faint">00 → 23</span>
        </div>
        <div class="mt-2 grid h-8 grid-cols-[repeat(24,minmax(0,1fr))] items-end gap-px">
          <div
            v-for="slot in hourly"
            :key="`h-${slot.hour}`"
            class="h-full bg-muted"
            :title="`${pad(slot.hour)}:00 · ${slot.count}`"
          >
            <div
              class="mt-auto w-full"
              :class="slot.count > 0 ? 'bg-hazard' : 'bg-transparent'"
              :style="{ height: `${Math.max(slot.count > 0 ? 12 : 0, slot.ratio * 100)}%` }"
            />
          </div>
        </div>
        <div class="readout mt-1 flex justify-between text-[8px] text-faint">
          <span>00</span><span>06</span><span>12</span><span>18</span><span>23</span>
        </div>
      </div>
    </section>

    <!-- 筛选 + 搜索 -->
    <section class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-border bg-panel px-3 py-1.5">
      <div class="flex flex-wrap items-center gap-1">
        <button
          v-for="item in FILTERS"
          :key="item.key"
          type="button"
          class="border px-2 py-0.5 text-[10px] uppercase tracking-[0.1em] transition-colors"
          :class="
            filter === item.key
              ? 'border-hazard text-hazard'
              : 'border-border-strong text-muted-foreground hover:border-foreground hover:text-foreground'
          "
          @click="filter = item.key"
        >
          {{ t(item.labelKey) }}
          <span class="readout ml-1 text-[9px] text-faint">
            {{ item.key === "all" ? stats.total : sourceCount(item.key as EventSource) }}
          </span>
        </button>
      </div>
      <label class="flex min-w-0 items-center gap-1.5 border border-border-strong px-2 py-0.5">
        <Search class="size-3 shrink-0 text-faint" />
        <input
          v-model="keyword"
          type="text"
          class="readout w-44 min-w-0 bg-transparent text-[10px] text-foreground outline-none placeholder:text-faint"
          :placeholder="t('rl.search')"
        />
      </label>
    </section>

    <!-- 日志表 -->
    <div class="min-h-0 flex-1 overflow-auto">
      <div class="grid grid-cols-[92px_76px_72px_minmax(160px,1fr)_minmax(200px,1.4fr)] items-center gap-2 border-b border-border bg-panel px-3 py-1">
        <span class="lbl">{{ t("rl.time") }}</span>
        <span class="lbl">{{ t("rl.source") }}</span>
        <span class="lbl">LEVEL</span>
        <span class="lbl">{{ t("rl.content") }}</span>
        <span class="lbl">{{ t("rl.detail") }}</span>
      </div>

      <div v-if="rows.length === 0" class="px-3 py-16 text-center">
        <div class="text-[11px] text-faint">
          {{ loading ? "···" : data && data.items.length > 0 ? t("rl.emptyFilter") : t("rl.empty") }}
        </div>
        <div v-if="!loading && (!data || data.items.length === 0)" class="mt-1 text-[10px] text-faint">
          {{ t("rl.emptyHint") }}
        </div>
      </div>

      <div
        v-for="(row, index) in rows"
        :key="`${row.at}-${index}`"
        class="grid grid-cols-[92px_76px_72px_minmax(160px,1fr)_minmax(200px,1.4fr)] items-center gap-2 border-b border-border/60 px-3 py-1.5"
      >
        <div class="flex min-w-0 items-center gap-1.5">
          <span class="h-3 w-0.5 shrink-0" :class="levelBar(row.level)" />
          <span class="readout text-[10px] text-foreground">{{ clockOf(row.at) }}</span>
          <span class="readout text-[9px] text-faint" :title="relative(row.at)">{{ dateOf(row.at) }}</span>
        </div>
        <span class="readout truncate text-[9px] text-muted-foreground">{{ sourceLabel(row.source) }}</span>
        <span class="readout text-[9px] uppercase" :class="levelColor(row.level)">{{ row.level }}</span>
        <span class="min-w-0 truncate text-[11px] text-foreground" :title="row.title">{{ row.title }}</span>
        <span class="readout min-w-0 truncate text-[9px] text-faint" :title="row.detail ?? ''">
          {{ row.detail || "—" }}
        </span>
      </div>
    </div>

    <!-- 清空确认 -->
    <Teleport to="body">
      <div
        v-if="clearTarget"
        class="fixed inset-0 z-50 grid place-items-center bg-black/60 p-4"
        @click.self="clearTarget = null"
      >
        <div class="w-full max-w-md border border-hazard bg-card">
          <div class="flex items-center justify-between border-b border-border px-3 py-2">
            <span class="text-[12px] tracking-[0.1em] text-hazard">{{ t("rl.clear") }}</span>
            <button type="button" class="text-[11px] text-muted-foreground hover:text-foreground" @click="clearTarget = null">
              ✕
            </button>
          </div>
          <div class="px-3 py-3 text-[11px] leading-relaxed text-muted-foreground">{{ confirmText }}</div>
          <div class="flex justify-end gap-1.5 border-t border-border px-3 py-2">
            <button
              type="button"
              class="border border-border-strong px-2.5 py-1 text-[10px] uppercase tracking-[0.1em] text-muted-foreground hover:text-foreground"
              @click="clearTarget = null"
            >
              CANCEL
            </button>
            <button
              type="button"
              class="flex items-center gap-1.5 border border-hazard bg-hazard px-2.5 py-1 text-[10px] uppercase tracking-[0.1em] text-white disabled:opacity-40"
              :disabled="clearing"
              @click="doClear()"
            >
              <Loader2 v-if="clearing" class="size-3 animate-spin" />
              {{ t("rl.clear") }}
            </button>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>
