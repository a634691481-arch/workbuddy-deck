<script setup lang="ts">
/**
 * Token 统计：扫描本机会话 jsonl（core `token_stats::get_statistics`）。
 * 结构：八项总量 → 构成条 → 数据源 → 每日柱 / 模型分布 → 项目分布 / 周×时热力
 *      → 效率指标 → 会话 TOP / 调用明细。
 * 全部本地只读，不发网络请求。
 */
import { computed, onMounted, ref } from "vue";
import { Loader2, RefreshCw } from "lucide-vue-next";

import { tokenStatistics, type TokenStatistics } from "@/lib/api";
import { t } from "@/lib/i18n";

const stats = ref<TokenStatistics | null>(null);
const loading = ref(true);
const error = ref("");
const days = ref(0);
const detailTab = ref<"sessions" | "calls">("sessions");

const RANGES: { key: number; label: string }[] = [
  { key: 7, label: "7D" },
  { key: 30, label: "30D" },
  { key: 90, label: "90D" },
  { key: 0, label: "ALL" },
];

const WEEKDAYS = ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"];

/** 缓存状态行的时刻（HH:mm），让「这份数据是什么时候算的」一眼可见。 */
const cacheTime = computed(() => {
  const ts = stats.value?.generatedAt;
  if (!ts) return "--:--";
  const at = new Date(ts);
  return `${String(at.getHours()).padStart(2, "0")}:${String(at.getMinutes()).padStart(2, "0")}`;
});

const cacheHint = computed(() => `${t("tk.cachedHint")} · ${t("tk.refresh")}`);


async function load(force = false) {
  loading.value = true;
  error.value = "";
  try {
    const next = await tokenStatistics(days.value, force);
    if (next?.error) error.value = next.error;
    stats.value = next;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    loading.value = false;
  }
}

onMounted(() => void load());

function pick(daysValue: number) {
  days.value = daysValue;
  void load();
}

/** 1.2M / 34.5K —— 终端风格紧凑读数。 */
function compact(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "—";
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(2)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return String(value);
}

const sources = computed(() => (stats.value?.sources ?? []).filter((source) => source.summary.total > 0));

const totals = computed(() => {
  const base = {
    total: 0,
    input: 0,
    output: 0,
    cacheRead: 0,
    cacheWrite: 0,
    uncachedInput: 0,
    records: 0,
  };
  for (const source of stats.value?.sources ?? []) {
    base.total += source.summary.total;
    base.input += source.summary.input;
    base.output += source.summary.output;
    base.cacheRead += source.summary.cacheRead;
    base.cacheWrite += source.summary.cacheWrite ?? 0;
    base.uncachedInput += source.summary.uncachedInput ?? Math.max(0, source.summary.input - source.summary.cacheRead);
    base.records += source.summary.records;
  }
  return base;
});

/** 缓存命中率 = 缓存读 / 输入（输入本身已含缓存部分）。 */
const cacheRate = computed(() => {
  const { cacheRead, input } = totals.value;
  return input > 0 ? cacheRead / input : null;
});

/** 消耗构成：四段相加即总量，用于看「钱花在缓存还是生成上」。 */
const mix = computed(() => {
  const segments = [
    { key: "cacheRead", label: "CACHE READ", value: totals.value.cacheRead, tone: "bg-foreground/75" },
    { key: "uncachedInput", label: "UNCACHED IN", value: totals.value.uncachedInput, tone: "bg-foreground/40" },
    { key: "cacheWrite", label: "CACHE WRITE", value: totals.value.cacheWrite, tone: "bg-hazard/80" },
    { key: "output", label: "OUTPUT", value: totals.value.output, tone: "bg-muted-foreground" },
  ];
  const sum = segments.reduce((total, segment) => total + segment.value, 0);
  return {
    sum,
    segments: segments.map((segment) => ({
      ...segment,
      share: sum > 0 ? segment.value / sum : 0,
      width: sum > 0 ? (segment.value / sum) * 100 : 0,
    })),
  };
});

/** 跨数据源按日期合并（同一天不同来源相加）。 */
const daily = computed(() => {
  const byDate = new Map<string, number>();
  for (const source of stats.value?.sources ?? []) {
    for (const point of source.daily ?? []) {
      byDate.set(point.key, (byDate.get(point.key) ?? 0) + point.total);
    }
  }
  const rows = [...byDate.entries()].sort((left, right) => left[0].localeCompare(right[0])).slice(-24);
  const max = rows.reduce((peak, [, total]) => Math.max(peak, total), 0);
  return rows.map(([date, total]) => ({
    date: date.slice(5).replace("-", "/"),
    fullDate: date,
    total,
    height: max > 0 ? Math.max(2, Math.round((total / max) * 100)) : 2,
  }));
});

const dailyMax = computed(() => daily.value.reduce((peak, point) => Math.max(peak, point.total), 0));

/**
 * 环比：近 7 天 vs 前 7 天。
 *
 * 不做「按今天倒推」，因为最近一次活动可能不是今天（周末不用就断档）；直接用
 * 日序列末尾的 14 天，数据说什么就是什么。不足 14 天返回 null，不拿 3 天比 3 天。
 */
const weekOverWeek = computed(() => {
  const rows = daily.value;
  if (rows.length < 14) return null;
  const recent = rows.slice(-7).reduce((sum, point) => sum + point.total, 0);
  const previous = rows.slice(-14, -7).reduce((sum, point) => sum + point.total, 0);
  if (previous <= 0) return null;
  return ((recent - previous) / previous) * 100;
});

/** 用量涨了是要注意的事（红），降了是好消息（绿）——与股票口径无关，按风险着色。 */
const wowLabel = computed(() => {
  const pct = weekOverWeek.value;
  if (pct === null) return "";
  const arrow = pct >= 0 ? "▲" : "▼";
  return `${arrow}${Math.abs(pct).toFixed(1)}%`;
});
const wowTone = computed(() => {
  const pct = weekOverWeek.value;
  if (pct === null) return "text-faint";
  return pct >= 0 ? "text-hazard" : "text-success";
});

/** 模型分布：跨源合并，取前 8。 */
const models = computed(() => {
  const merged = new Map<string, number>();
  for (const source of stats.value?.sources ?? []) {
    for (const model of source.models ?? []) {
      merged.set(model.key, (merged.get(model.key) ?? 0) + model.total);
    }
  }
  const rows = [...merged.entries()].sort((left, right) => right[1] - left[1]).slice(0, 8);
  const max = rows[0]?.[1] ?? 0;
  const sum = rows.reduce((total, [, value]) => total + value, 0);
  return rows.map(([name, total]) => ({
    name,
    total,
    share: sum > 0 ? total / sum : 0,
    width: max > 0 ? Math.max(1, Math.round((total / max) * 100)) : 0,
  }));
});

/** 项目分布：跨源合并，取前 10。 */
const projects = computed(() => {
  const merged = new Map<string, number>();
  for (const source of stats.value?.sources ?? []) {
    for (const project of source.projects ?? []) {
      merged.set(project.key, (merged.get(project.key) ?? 0) + project.total);
    }
  }
  const rows = [...merged.entries()]
    .filter(([name]) => name && name !== "—")
    .sort((left, right) => right[1] - left[1])
    .slice(0, 10);
  const max = rows[0]?.[1] ?? 0;
  return rows.map(([name, total]) => ({
    name,
    total,
    width: max > 0 ? Math.max(1, Math.round((total / max) * 100)) : 0,
  }));
});

/** 周 × 时热力：key 形如 `weekday-hour`（0=周一），7×24 = 168 格。 */
const hourGrid = computed(() => {
  const cells = new Map<string, number>();
  for (const source of stats.value?.sources ?? []) {
    for (const point of source.hours ?? []) {
      cells.set(point.key, (cells.get(point.key) ?? 0) + point.total);
    }
  }
  let max = 0;
  for (const value of cells.values()) max = Math.max(max, value);
  return {
    max,
    rows: WEEKDAYS.map((label, weekday) => ({
      label,
      weekday,
      cells: Array.from({ length: 24 }, (_, hour) => {
        const value = cells.get(`${weekday}-${hour}`) ?? 0;
        return { hour, value, level: max > 0 ? value / max : 0 };
      }),
    })),
  };
});

/** 效率指标：日均 / 峰值 / 会话 / 项目 / 每请求。 */
const efficiency = computed(() => {
  const activeDays = daily.value.length;
  const peak = daily.value.reduce(
    (best, point) => (point.total > best.total ? point : best),
    { total: 0, fullDate: "" } as { total: number; fullDate: string },
  );
  const sessionCount = (stats.value?.sources ?? []).reduce((sum, source) => sum + (source.sessions?.length ?? 0), 0);
  const projectKeys = new Set<string>();
  for (const source of stats.value?.sources ?? []) {
    for (const project of source.projects ?? []) projectKeys.add(project.key);
  }
  return {
    avgDay: activeDays > 0 ? totals.value.total / activeDays : null,
    peakTotal: peak.total,
    peakDate: peak.fullDate ? peak.fullDate.slice(5).replace("-", "/") : "—",
    sessionCount,
    projectCount: projectKeys.size,
    perRequest: totals.value.records > 0 ? totals.value.total / totals.value.records : null,
  };
});

/** 会话 TOP：跨源合并后取前 12，带项目名。 */
const sessions = computed(() => {
  const rows: { title: string; project: string; source: string; total: number }[] = [];
  for (const source of stats.value?.sources ?? []) {
    for (const session of source.sessions ?? []) {
      rows.push({
        title: session.title || session.project || session.sessionId || session.key,
        project: session.project || "—",
        source: source.source,
        total: session.total,
      });
    }
  }
  return rows.sort((left, right) => right.total - left.total).slice(0, 12);
});

/** 调用明细：所有 detail 源合并，最近 60 条。 */
const calls = computed(() => {
  const rows: {
    key: string;
    time: string;
    model: string;
    project: string;
    input: number;
    output: number;
    cacheRead: number;
    total: number;
  }[] = [];
  for (const source of stats.value?.sources ?? []) {
    for (const request of source.requests ?? []) {
      rows.push({
        key: `${source.source}-${request.sessionId}-${request.timestamp}`,
        time: new Date(request.timestamp).toLocaleString("zh-CN", {
          month: "2-digit",
          day: "2-digit",
          hour: "2-digit",
          minute: "2-digit",
        }),
        model: request.model || "—",
        project: request.project || "—",
        input: request.input,
        output: request.output,
        cacheRead: request.cacheRead,
        total: request.total,
      });
    }
  }
  return rows.sort((left, right) => right.time.localeCompare(left.time)).slice(0, 60);
});

const hasCallDetail = computed(() => calls.value.length > 0);

const sourceLabel = (name: string) => {
  switch (name) {
    case "workbuddy":
      return "WORKBUDDY CN";
    case "workbuddy-ai":
      return "WORKBUDDY INTL";
    case "codebuddy-cli":
      return "CODEBUDDY CLI";
    default:
      return name.toUpperCase();
  }
};

const dayLabel = (ts: number | null | undefined) => {
  if (!ts) return "—";
  return new Date(ts).toLocaleDateString("zh-CN", { month: "2-digit", day: "2-digit" });
};

/** 热力格着色：0 值用底色，其余按强度叠航空红。 */
const cellColor = (level: number) =>
  level <= 0 ? "var(--color-muted)" : `rgba(230, 25, 25, ${(0.12 + level * 0.88).toFixed(3)})`;
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <header class="flex shrink-0 items-center justify-between gap-3 border-b border-border bg-panel px-4 py-2 wb-flat">
      <div class="min-w-0">
        <h1 class="text-[13px] tracking-[0.12em] text-foreground">{{ t("tk.title") }}</h1>
        <div class="readout mt-0.5 text-[9px] text-faint">
          {{ stats?.rangeDays ? `RANGE ${stats.rangeDays}D` : "RANGE ALL" }} ·
          {{ t("tk.scanned") }} {{ sources.reduce((sum, item) => sum + item.filesScanned, 0) }}
          <span v-if="stats?.cached" class="ml-2 text-muted-foreground" :title="cacheHint">
            CACHED {{ cacheTime }}
          </span>
          <span v-else-if="stats" class="ml-2 text-muted-foreground">FRESH {{ cacheTime }}</span>
        </div>
      </div>
      <div class="flex shrink-0 items-center gap-1.5">
        <!-- 有旧数据时不清屏：只在角落标「扫描中」，避免整页闪一下 -->
        <span v-if="loading && stats" class="readout text-[9px] uppercase tracking-[0.14em] text-hazard">
          scanning…
        </span>
        <div class="flex gap-px bg-border overflow-hidden">
          <button
            v-for="range in RANGES"
            :key="range.key"
            type="button"
            class="px-2 py-1 text-[10px] tracking-[0.1em] transition-colors"
            :class="
              days === range.key
                ? 'bg-foreground text-background'
                : 'bg-card text-muted-foreground hover:text-foreground'
            "
            @click="pick(range.key)"
          >
            {{ range.label }}
          </button>
        </div>
        <button
          type="button"
          class="flex items-center gap-1.5 border border-border-strong px-2.5 py-1 text-[10px] uppercase tracking-[0.1em] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground disabled:opacity-40"
          :disabled="loading"
          @click="load(true)"
        >
          <Loader2 v-if="loading" class="size-3 animate-spin" />
          <RefreshCw v-else class="size-3" />
          {{ t("tk.refresh") }}
        </button>
      </div>
    </header>

    <div v-if="error" class="shrink-0 border-b border-hazard bg-hazard-soft px-4 py-2 text-[11px] text-hazard">
      {{ error }}
    </div>

    <!-- 八项总量：两行四列 -->
    <section class="mx-3 mt-3 shrink-0">
      <div class="grid grid-cols-4 gap-2">
        <div class="border border-border bg-card px-3 py-2">
          <div class="lbl">{{ t("tk.total") }}</div>
          <div class="readout mt-1 text-[19px] leading-none text-foreground">
            {{ loading && !stats ? "···" : compact(totals.total) }}
          </div>
        </div>
        <div class="border border-border bg-card px-3 py-2">
          <div class="lbl">{{ t("tk.input") }}</div>
          <div class="readout mt-1 text-[19px] leading-none text-foreground">{{ compact(totals.input) }}</div>
        </div>
        <div class="border border-border bg-card px-3 py-2">
          <div class="lbl">{{ t("tk.output") }}</div>
          <div class="readout mt-1 text-[19px] leading-none text-foreground">{{ compact(totals.output) }}</div>
        </div>
        <div class="border border-border bg-card px-3 py-2">
          <div class="lbl">{{ t("tk.records") }}</div>
          <div class="readout mt-1 text-[19px] leading-none text-foreground">{{ compact(totals.records) }}</div>
        </div>

        <div class="border border-border bg-card px-3 py-2">
          <div class="lbl">{{ t("tk.cache") }}</div>
          <div class="readout mt-1 text-[19px] leading-none text-foreground">{{ compact(totals.cacheRead) }}</div>
        </div>
        <div class="border border-border bg-card px-3 py-2">
          <div class="lbl">{{ t("tk.cacheWrite") }}</div>
          <div class="readout mt-1 text-[19px] leading-none text-foreground">{{ compact(totals.cacheWrite) }}</div>
        </div>
        <div class="border border-border bg-card px-3 py-2">
          <div class="lbl">{{ t("tk.uncached") }}</div>
          <div class="readout mt-1 text-[19px] leading-none text-foreground">{{ compact(totals.uncachedInput) }}</div>
        </div>
        <div class="border border-border bg-card px-3 py-2">
          <div class="lbl">{{ t("tk.hitRate") }}</div>
          <div class="readout mt-1 text-[19px] leading-none text-hazard">
            {{ cacheRate === null ? "—" : `${Math.round(cacheRate * 100)}%` }}
          </div>
        </div>
      </div>
    </section>

    <div class="min-h-0 flex-1 space-y-3 overflow-y-auto p-3">
      <!-- 消耗构成 -->
      <section class="border border-border bg-card px-3 py-2.5">
        <div class="flex items-baseline justify-between gap-2">
          <span class="lbl">// {{ t("tk.mix") }}</span>
          <span class="readout text-[9px] text-faint">Σ {{ compact(mix.sum) }}</span>
        </div>
        <div class="mt-2 flex h-[10px] gap-px bg-border wb-bars">
          <div
            v-for="segment in mix.segments"
            :key="segment.key"
            :class="segment.tone"
            :style="{ width: `${segment.width}%` }"
            :title="`${segment.label} · ${compact(segment.value)}`"
          />
        </div>
        <div class="readout mt-1.5 flex flex-wrap gap-x-4 gap-y-1 text-[9px] text-faint">
          <span v-for="segment in mix.segments" :key="segment.key" class="flex items-center gap-1">
            <span class="h-2 w-2" :class="segment.tone" />
            {{ segment.label }} {{ compact(segment.value) }} · {{ Math.round(segment.share * 100) }}%
          </span>
        </div>
      </section>

      <!-- 数据源 -->
      <section class="border border-border bg-card">
        <div class="lbl border-b border-border px-3 py-2">// SOURCE</div>
        <div v-if="sources.length === 0" class="px-3 py-8 text-center text-[11px] text-faint">
          {{ loading ? "···" : t("tk.empty") }}
        </div>
        <div v-else class="grid grid-cols-2 gap-px bg-border">
          <div v-for="source in sources" :key="source.source" class="bg-card px-3 py-2">
            <div class="flex items-baseline justify-between gap-2">
              <span class="text-[11px] tracking-[0.08em] text-foreground">{{ sourceLabel(source.source) }}</span>
              <span class="readout text-[13px] text-foreground">{{ compact(source.summary.total) }}</span>
            </div>
            <div class="readout mt-1 flex gap-3 text-[9px] text-faint">
              <span>IN {{ compact(source.summary.input) }}</span>
              <span>OUT {{ compact(source.summary.output) }}</span>
              <span>CACHE {{ compact(source.summary.cacheRead) }}</span>
              <span>REQ {{ compact(source.summary.records) }}</span>
            </div>
            <div class="readout mt-0.5 flex gap-3 text-[9px] text-faint">
              <span>
                {{ t("tk.coverage") }} {{ dayLabel(source.coverageStartAt) }} → {{ dayLabel(source.coverageEndAt) }}
              </span>
              <span v-if="source.parseErrors">{{ t("tk.parseErr") }} {{ source.parseErrors }}</span>
            </div>
          </div>
        </div>
      </section>

      <div class="grid grid-cols-[1fr_300px] gap-3">
        <!-- 每日 -->
        <section class="border border-border bg-card px-3 py-2.5">
          <div class="lbl">// {{ t("tk.daily") }}</div>
          <div v-if="daily.length === 0" class="py-10 text-center text-[11px] text-faint">{{ t("tk.empty") }}</div>
          <div v-else class="mt-2">
            <div class="flex h-[120px] items-end gap-[3px] border-b border-border pb-px">
              <div
                v-for="(point, index) in daily"
                :key="`${point.date}-${index}`"
                class="flex-1 bg-foreground/70 transition-colors hover:bg-hazard"
                :style="{ height: `${point.height}%` }"
                :title="`${point.date} · ${compact(point.total)}`"
              />
            </div>
            <div class="readout mt-1 flex justify-between text-[9px] text-faint">
              <span>{{ daily[0]?.date }}</span>
              <span>MAX {{ compact(dailyMax) }}</span>
              <span>{{ daily[daily.length - 1]?.date }}</span>
            </div>
          </div>
        </section>

        <!-- 模型 -->
        <section class="border border-border bg-card px-3 py-2.5">
          <div class="lbl">// {{ t("tk.models") }}</div>
          <ul v-if="models.length > 0" class="mt-2 space-y-1.5">
            <li v-for="model in models" :key="model.name" class="min-w-0">
              <div class="flex items-baseline justify-between gap-2 text-[10px]">
                <span class="min-w-0 truncate text-muted-foreground" :title="model.name">{{ model.name }}</span>
                <span class="readout shrink-0 text-foreground">
                  {{ compact(model.total) }}
                  <span class="ml-1 text-[9px] text-faint">{{ Math.round(model.share * 100) }}%</span>
                </span>
              </div>
              <div class="mt-1 h-[3px] bg-muted">
                <div class="h-full bg-hazard/80" :style="{ width: `${model.width}%` }" />
              </div>
            </li>
          </ul>
          <div v-else class="py-8 text-center text-[10px] text-faint">{{ t("tk.empty") }}</div>
        </section>
      </div>

      <div class="grid grid-cols-[1fr_300px] gap-3">
        <!-- 项目分布 -->
        <section class="border border-border bg-card px-3 py-2.5">
          <div class="lbl">// {{ t("tk.projects") }}</div>
          <ul v-if="projects.length > 0" class="mt-2 grid grid-cols-2 gap-x-4 gap-y-1.5">
            <li v-for="project in projects" :key="project.name" class="min-w-0">
              <div class="flex items-baseline justify-between gap-2 text-[10px]">
                <span class="min-w-0 truncate text-muted-foreground" :title="project.name">{{ project.name }}</span>
                <span class="readout shrink-0 text-foreground">{{ compact(project.total) }}</span>
              </div>
              <div class="mt-1 h-[3px] bg-muted">
                <div class="h-full bg-foreground/55" :style="{ width: `${project.width}%` }" />
              </div>
            </li>
          </ul>
          <div v-else class="py-8 text-center text-[10px] text-faint">{{ t("tk.empty") }}</div>
        </section>

        <!-- 周 × 时热力 -->
        <section class="border border-border bg-card px-3 py-2.5">
          <div class="flex items-baseline justify-between gap-2">
            <span class="lbl">// {{ t("tk.hours") }}</span>
            <span class="readout text-[9px] text-faint">MAX {{ compact(hourGrid.max) }}</span>
          </div>
          <div class="mt-2 space-y-px">
            <div v-for="row in hourGrid.rows" :key="row.label" class="flex items-center gap-1">
              <span class="readout w-[22px] shrink-0 text-[8px] text-faint">{{ row.label }}</span>
              <div class="grid flex-1 grid-cols-[repeat(24,minmax(0,1fr))] gap-px wb-bars">
                <div
                  v-for="cell in row.cells"
                  :key="cell.hour"
                  class="h-[7px]"
                  :style="{ backgroundColor: cellColor(cell.level) }"
                  :title="`${row.label} ${String(cell.hour).padStart(2, '0')}:00 · ${compact(cell.value)}`"
                />
              </div>
            </div>
          </div>
          <div class="readout mt-1 flex justify-between pl-[26px] text-[8px] text-faint">
            <span>00</span>
            <span>06</span>
            <span>12</span>
            <span>18</span>
            <span>23</span>
          </div>
        </section>
      </div>

      <!-- 效率指标 -->
      <section class="border border-border bg-card">
        <div class="px-3 pt-2">
          <span class="lbl">// {{ t("tk.eff") }}</span>
        </div>
        <div class="mt-1.5 grid grid-cols-5 gap-2 p-2">
          <div class="border border-border bg-card px-3 py-2">
            <div class="lbl">{{ t("tk.avgDay") }}</div>
            <div class="readout mt-1 text-[14px] leading-none text-foreground">{{ compact(efficiency.avgDay) }}</div>
            <div v-if="wowLabel" class="readout mt-0.5 text-[9px]" :class="wowTone" :title="t('tk.wowTitle')">
              {{ wowLabel }} <span class="text-faint">{{ t("tk.wow") }}</span>
            </div>
          </div>
          <div class="border border-border bg-card px-3 py-2">
            <div class="lbl">{{ t("tk.peakDay") }}</div>
            <div class="readout mt-1 text-[14px] leading-none text-foreground">{{ compact(efficiency.peakTotal) }}</div>
            <div class="readout mt-0.5 text-[9px] text-faint">{{ efficiency.peakDate }}</div>
          </div>
          <div class="border border-border bg-card px-3 py-2">
            <div class="lbl">{{ t("tk.perReq") }}</div>
            <div class="readout mt-1 text-[14px] leading-none text-foreground">{{ compact(efficiency.perRequest) }}</div>
          </div>
          <div class="border border-border bg-card px-3 py-2">
            <div class="lbl">{{ t("tk.sessionCount") }}</div>
            <div class="readout mt-1 text-[14px] leading-none text-foreground">{{ compact(efficiency.sessionCount) }}</div>
          </div>
          <div class="border border-border bg-card px-3 py-2">
            <div class="lbl">{{ t("tk.projCount") }}</div>
            <div class="readout mt-1 text-[14px] leading-none text-foreground">{{ compact(efficiency.projectCount) }}</div>
          </div>
        </div>
      </section>

      <!-- 会话 TOP / 调用明细 -->
      <section class="border border-border bg-card">
        <div class="flex items-center justify-between gap-2 border-b border-border px-3 py-2">
          <span class="lbl">// {{ t("tk.recent") }}</span>
          <div class="flex gap-px bg-border overflow-hidden">
            <button
              v-for="tab in (['sessions', 'calls'] as const)"
              :key="tab"
              type="button"
              class="px-2.5 py-1 text-[10px] tracking-[0.1em] transition-colors"
              :class="detailTab === tab ? 'bg-foreground text-background' : 'bg-card text-muted-foreground hover:text-foreground'"
              @click="detailTab = tab"
            >
              {{ tab === "sessions" ? t("tk.tabSession") : t("tk.tabCall") }}
            </button>
          </div>
        </div>

        <template v-if="detailTab === 'sessions'">
          <div class="grid grid-cols-[1fr_130px_104px_80px] gap-px bg-border text-[10px] wb-cells">
            <div class="lbl bg-card px-3 py-1.5">TITLE</div>
            <div class="lbl bg-card px-2 py-1.5">{{ t("tk.callProject") }}</div>
            <div class="lbl bg-card px-2 py-1.5">SOURCE</div>
            <div class="lbl bg-card px-2 py-1.5 text-right">TOKENS</div>
            <template v-for="(session, index) in sessions" :key="`${session.title}-${index}`">
              <div class="min-w-0 bg-card px-3 py-1.5">
                <div class="truncate text-foreground" :title="session.title">{{ session.title }}</div>
              </div>
              <div class="min-w-0 bg-card px-2 py-1.5">
                <div class="truncate text-[9px] text-faint" :title="session.project">{{ session.project }}</div>
              </div>
              <div class="readout bg-card px-2 py-1.5 text-[9px] text-faint">{{ sourceLabel(session.source) }}</div>
              <div class="readout bg-card px-2 py-1.5 text-right text-foreground">{{ compact(session.total) }}</div>
            </template>
          </div>
          <div v-if="sessions.length === 0" class="px-3 py-8 text-center text-[11px] text-faint">
            {{ loading ? "···" : t("tk.empty") }}
          </div>
        </template>

        <template v-else>
          <div
            v-if="!hasCallDetail"
            class="px-3 py-8 text-center text-[11px] text-faint"
          >
            {{ loading ? "···" : t("tk.noDetail") }}
          </div>
          <template v-else>
            <div class="grid grid-cols-[86px_1fr_1fr_62px_62px_62px_70px] gap-px bg-border text-[10px] wb-cells">
              <div class="lbl bg-card px-3 py-1.5">{{ t("tk.callTime") }}</div>
              <div class="lbl bg-card px-2 py-1.5">{{ t("tk.callModel") }}</div>
              <div class="lbl bg-card px-2 py-1.5">{{ t("tk.callProject") }}</div>
              <div class="lbl bg-card px-2 py-1.5 text-right">{{ t("tk.callIn") }}</div>
              <div class="lbl bg-card px-2 py-1.5 text-right">{{ t("tk.callCache") }}</div>
              <div class="lbl bg-card px-2 py-1.5 text-right">{{ t("tk.callOut") }}</div>
              <div class="lbl bg-card px-2 py-1.5 text-right">{{ t("tk.callTotal") }}</div>
              <template v-for="call in calls" :key="call.key">
                <div class="readout bg-card px-3 py-1.5 text-faint">{{ call.time }}</div>
                <div class="min-w-0 bg-card px-2 py-1.5">
                  <div class="truncate text-foreground" :title="call.model">{{ call.model }}</div>
                </div>
                <div class="min-w-0 bg-card px-2 py-1.5">
                  <div class="truncate text-[9px] text-faint" :title="call.project">{{ call.project }}</div>
                </div>
                <div class="readout bg-card px-2 py-1.5 text-right text-muted-foreground">{{ compact(call.input) }}</div>
                <div class="readout bg-card px-2 py-1.5 text-right text-muted-foreground">{{ compact(call.cacheRead) }}</div>
                <div class="readout bg-card px-2 py-1.5 text-right text-muted-foreground">{{ compact(call.output) }}</div>
                <div class="readout bg-card px-2 py-1.5 text-right text-foreground">{{ compact(call.total) }}</div>
              </template>
            </div>
            <div class="readout border-t border-border px-3 py-1.5 text-[9px] text-faint">
              RECENT {{ calls.length }}
            </div>
          </template>
        </template>
      </section>
    </div>
  </div>
</template>
