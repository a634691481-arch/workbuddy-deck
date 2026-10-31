<script setup lang="ts">
// 每日自动执行面板：开关 + 执行时间 + 最近执行 + 自动执行记录。
// 状态只在后端（auto_daily_config.json），本组件是无状态的读写视图。
import { onBeforeUnmount, onMounted, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { CalendarClock, ChevronDown } from "lucide-vue-next";

import { autoDailyGet, autoDailySet, type AutoDailyConfig, type AutoDailyLog } from "@/lib/api";
import { t } from "@/lib/i18n";
import { isTauri } from "@/lib/tauri-env";

const enabled = ref(false);
const time = ref("09:00");
const lastRun = ref("");
const logs = ref<AutoDailyLog[]>([]);
const expanded = ref(false);
const saving = ref(false);

async function reload() {
  if (!isTauri()) return;
  const res = await autoDailyGet();
  const cfg: AutoDailyConfig = res.config;
  enabled.value = cfg.enabled === true;
  time.value = cfg.time || "09:00";
  lastRun.value = cfg.lastRunDate || "";
  logs.value = res.logs ?? [];
}

async function persist(nextEnabled: boolean) {
  saving.value = true;
  try {
    const res = await autoDailySet(nextEnabled, time.value.trim() || "09:00");
    if (res.ok && res.config) {
      enabled.value = res.config.enabled === true;
      time.value = res.config.time || "09:00";
    }
  } finally {
    saving.value = false;
  }
}

function toggle() {
  if (!saving.value) void persist(!enabled.value);
}

function onTimeChange() {
  // 后端会校验 HH:MM；不合法时回读配置纠偏
  void persist(enabled.value);
}

/** 2026-10-30T17-08-00Z → 10-30 17:08:00 */
function shortTs(ts: string): string {
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2})-(\d{2})-(\d{2})Z$/.exec(ts);
  if (!match) return ts;
  return `${match[2]}-${match[3]} ${match[4]}:${match[5]}:${match[6]}`;
}

let unlisten: UnlistenFn | null = null;
onMounted(async () => {
  void reload();
  if (!isTauri()) return;
  unlisten = await listen("auto://daily-done", () => void reload());
});
onBeforeUnmount(() => unlisten?.());
</script>

<template>
  <div class="shrink-0 border-t border-border bg-panel">
    <div class="flex h-9 items-center gap-2 whitespace-nowrap px-2 text-[10px] uppercase tracking-[0.1em]">
      <CalendarClock class="size-3 shrink-0 text-faint" />
      <span class="shrink-0 text-faint">{{ t("ad.title") }}</span>

      <!-- 开关：反白=启用，磷光绿仅用于就绪语义，这里用普通反白避免歧义 -->
      <button
        class="shrink-0 px-2 py-0.5"
        :class="enabled ? 'bg-foreground text-background' : 'border border-border text-faint hover:text-foreground'"
        :disabled="saving"
        @click="toggle"
      >
        {{ enabled ? t("ad.on") : t("ad.off") }}
      </button>

      <label class="flex shrink-0 items-center gap-1 text-faint">
        {{ t("ad.time") }}
        <input
          v-model="time"
          type="time"
          class="bg-transparent px-1 py-0.5 text-foreground outline-none"
          :class="enabled ? '' : 'opacity-50'"
          :disabled="!enabled || saving"
          @change="onTimeChange"
        />
      </label>

      <span class="shrink-0 text-faint">
        {{ t("ad.lastrun") }}:
        <span class="text-foreground">{{ lastRun || t("ad.never") }}</span>
      </span>

      <span class="hidden min-w-0 flex-1 truncate text-faint/60 md:inline">— {{ t("ad.hint") }}</span>

      <!-- 记录折叠 -->
      <button
        class="ml-auto flex shrink-0 items-center gap-1 text-faint hover:text-foreground"
        @click="expanded = !expanded"
      >
        [ {{ logs.length }} ]
        <ChevronDown class="size-3 transition-transform" :class="expanded ? 'rotate-180' : ''" />
      </button>
    </div>

    <ul
      v-if="expanded"
      class="max-h-24 space-y-0.5 overflow-y-auto border-t border-border px-2 py-1 text-[10px]"
    >
      <li v-if="logs.length === 0" class="text-faint">{{ t("ad.noLogs") }}</li>
      <li v-for="(row, index) in logs" :key="index" class="flex gap-3 text-faint">
        <span class="text-foreground">{{ shortTs(row.ts) }}</span>
        <span>{{ row.status === "cancelled" ? "CANCELLED" : "DONE" }}</span>
        <span>{{ row.ok ?? 0 }}/{{ row.total ?? 0 }}</span>
      </li>
    </ul>
  </div>
</template>
