<script setup lang="ts">
/**
 * 系统设置：运行环境 / 数据体积 / 显示偏好 / 自动化摘要 / 维护。
 *
 * 定位是「只读体检 + 少量开关」：真正的业务配置（自动化时间、账号增删）仍在各自页面，
 * 这里只做总览与快捷入口，避免同一份配置在两处维护出分歧。
 */
import { computed, onMounted, ref } from "vue";
import { CircleAlert, Eraser, FolderOpen, Loader2, RefreshCw } from "lucide-vue-next";

import { eventsClear, openStoreDir, systemInfo, type EventSource, type SystemInfo } from "@/lib/api";
import { useShell } from "@/composables/useShell";
import { locale, setLocale, t } from "@/lib/i18n";
import { maskAll, setMaskAll } from "@/lib/task-meta";

const { go } = useShell();

const info = ref<SystemInfo | null>(null);
const loading = ref(true);
const error = ref("");
const notice = ref("");

async function load() {
  loading.value = true;
  error.value = "";
  try {
    info.value = await systemInfo();
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    loading.value = false;
  }
}

onMounted(() => void load());

const schedule = computed(() => info.value?.schedule ?? {});
const scheduleOn = computed(() => schedule.value.enabled === true);

/** 文件行：只有通知与日志有「条数」，账号库与错误日志按条数口径各自取。 */
const fileRows = computed(() => {
  const files = info.value?.files;
  const counts = info.value?.counts;
  if (!files || !counts) return [];
  return [
    { key: "accounts", labelKey: "st.fileAccounts" as const, rows: info.value?.accountCount ?? 0, stat: files.accounts },
    { key: "auto", labelKey: "st.fileAutoLogs" as const, rows: counts.autoLogs, stat: files.autoLogs },
    { key: "notify", labelKey: "st.fileNotify" as const, rows: counts.notify, stat: files.notifications },
    { key: "error", labelKey: "st.fileError" as const, rows: counts.errorLines, stat: files.errorLog },
  ];
});

async function openDir() {
  try {
    const result = await openStoreDir();
    if (!result.ok) throw new Error(result.error || "open failed");
    notice.value = result.path;
  } catch (cause) {
    error.value = `${t("st.openFail")}: ${cause instanceof Error ? cause.message : String(cause)}`;
  }
}

/** 维护区：清空某一源前先确认，避免误点把排障线索删干净。 */
const clearTarget = ref<{ source: EventSource; labelKey: "st.clearNotify" | "st.clearAuto" | "st.clearError" } | null>(null);
const clearing = ref(false);

const CLEARS: { source: EventSource; labelKey: "st.clearNotify" | "st.clearAuto" | "st.clearError" }[] = [
  { source: "notify", labelKey: "st.clearNotify" },
  { source: "auto", labelKey: "st.clearAuto" },
  { source: "error", labelKey: "st.clearError" },
];

async function doClear() {
  const target = clearTarget.value;
  if (!target) return;
  clearing.value = true;
  try {
    const result = await eventsClear(target.source);
    if (!result.ok) throw new Error(result.error || "clear failed");
    notice.value = t("st.cleared");
    clearTarget.value = null;
    await load();
  } catch (cause) {
    error.value = `${t("st.fail")}: ${cause instanceof Error ? cause.message : String(cause)}`;
    clearTarget.value = null;
  } finally {
    clearing.value = false;
  }
}
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <header class="flex shrink-0 items-center justify-between gap-3 border-b border-border bg-panel px-4 py-2">
      <div class="min-w-0">
        <h1 class="text-[13px] tracking-[0.12em] text-foreground">{{ t("st.title") }}</h1>
        <div class="readout mt-0.5 truncate text-[9px] text-faint">
          {{ t("st.subtitle") }}
          <span v-if="notice" class="ml-2 text-muted-foreground">{{ notice }}</span>
        </div>
      </div>
      <button
        type="button"
        class="flex shrink-0 items-center gap-1.5 border border-border-strong px-2.5 py-1 text-[10px] uppercase tracking-[0.1em] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground disabled:opacity-40"
        :disabled="loading"
        @click="load()"
      >
        <Loader2 v-if="loading" class="size-3 animate-spin" />
        <RefreshCw v-else class="size-3" />
        {{ t("st.refresh") }}
      </button>
    </header>

    <div v-if="error" class="flex shrink-0 items-center gap-2 border-b border-hazard bg-hazard-soft px-4 py-2 text-[11px] text-hazard">
      <CircleAlert class="size-3.5 shrink-0" />
      <span class="truncate">{{ error }}</span>
    </div>

    <div class="min-h-0 flex-1 overflow-auto p-3">
      <div class="grid grid-cols-1 gap-2.5 xl:grid-cols-2">
        <!-- 应用信息 -->
        <section class="border border-border bg-card">
          <div class="border-b border-border px-3 py-1.5">
            <span class="lbl">// {{ t("st.app") }}</span>
          </div>
          <div class="divide-y divide-border/60">
            <div class="flex items-center justify-between gap-3 px-3 py-2">
              <span class="text-[11px] text-muted-foreground">{{ t("st.version") }}</span>
              <span class="readout text-[11px] text-foreground">REV {{ info?.version ?? "—" }}</span>
            </div>
            <div class="flex items-center justify-between gap-3 px-3 py-2">
              <span class="text-[11px] text-muted-foreground">{{ t("st.platform") }}</span>
              <span class="readout text-[11px] text-foreground">
                {{ info ? `${info.os} / ${info.arch}` : "—" }}
              </span>
            </div>
            <div class="flex items-center justify-between gap-3 px-3 py-2">
              <span class="shrink-0 text-[11px] text-muted-foreground">{{ t("st.accounts") }}</span>
              <span class="readout text-[11px] text-foreground">{{ info?.accountCount ?? 0 }}</span>
            </div>
            <div class="px-3 py-2">
              <div class="flex items-center justify-between gap-2">
                <span class="text-[11px] text-muted-foreground">{{ t("st.storeDir") }}</span>
                <button
                  type="button"
                  class="flex shrink-0 items-center gap-1 border border-border-strong px-1.5 py-0.5 text-[9px] uppercase tracking-[0.1em] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground"
                  @click="openDir()"
                >
                  <FolderOpen class="size-2.5" />
                  {{ t("st.open") }}
                </button>
              </div>
              <div class="readout mt-1 break-all text-[9px] text-faint" :title="info?.storeDir">
                {{ info?.storeDir ?? "—" }}
              </div>
            </div>
          </div>
        </section>

        <!-- 数据规模 -->
        <section class="border border-border bg-card">
          <div class="border-b border-border px-3 py-1.5">
            <span class="lbl">// {{ t("st.data") }}</span>
          </div>
          <div class="grid grid-cols-[minmax(90px,1fr)_56px_64px_56px] items-center gap-2 border-b border-border/60 px-3 py-1">
            <span class="lbl">FILE</span>
            <span class="lbl text-right">{{ t("st.rows") }}</span>
            <span class="lbl text-right">SIZE</span>
            <span class="lbl text-right">STATE</span>
          </div>
          <div
            v-for="file in fileRows"
            :key="file.key"
            class="grid grid-cols-[minmax(90px,1fr)_56px_64px_56px] items-center gap-2 border-b border-border/40 px-3 py-1.5 last:border-b-0"
          >
            <span class="truncate text-[11px] text-foreground">{{ t(file.labelKey) }}</span>
            <span class="readout text-right text-[11px] text-foreground">{{ file.rows }}</span>
            <span class="readout text-right text-[10px] text-muted-foreground">{{ file.stat.human }}</span>
            <span
              class="readout text-right text-[9px]"
              :class="file.stat.exists ? 'text-muted-foreground' : 'text-faint'"
            >
              {{ file.stat.exists ? "OK" : t("st.missing") }}
            </span>
          </div>
        </section>

        <!-- 显示偏好 -->
        <section class="border border-border bg-card">
          <div class="border-b border-border px-3 py-1.5">
            <span class="lbl">// {{ t("st.display") }}</span>
          </div>
          <div class="flex items-start justify-between gap-3 border-b border-border/60 px-3 py-2.5">
            <div class="min-w-0">
              <div class="text-[11px] text-foreground">{{ t("st.mask") }}</div>
              <div class="mt-0.5 text-[9px] text-faint">{{ t("st.maskDesc") }}</div>
            </div>
            <button
              type="button"
              class="flex shrink-0 items-center gap-1.5 border px-2 py-1 text-[10px] uppercase tracking-[0.1em] transition-colors"
              :class="maskAll ? 'border-hazard text-hazard' : 'border-border-strong text-muted-foreground hover:text-foreground'"
              @click="setMaskAll(!maskAll)"
            >
              <span class="size-1.5" :class="maskAll ? 'bg-hazard' : 'bg-border-strong'" />
              {{ maskAll ? t("st.on") : t("st.off") }}
            </button>
          </div>
          <div class="flex items-center justify-between gap-3 px-3 py-2.5">
            <div class="min-w-0">
              <div class="text-[11px] text-foreground">{{ t("st.lang") }}</div>
              <div class="mt-0.5 text-[9px] text-faint">{{ t("st.langDesc") }}</div>
            </div>
            <div class="flex shrink-0 items-center">
              <button
                type="button"
                class="border border-border-strong px-2 py-0.5 text-[10px] uppercase tracking-[0.1em]"
                :class="locale === 'zh' ? 'bg-foreground text-background' : 'text-faint hover:text-foreground'"
                @click="setLocale('zh')"
              >
                中
              </button>
              <button
                type="button"
                class="-ml-px border border-border-strong px-2 py-0.5 text-[10px] uppercase tracking-[0.1em]"
                :class="locale === 'en' ? 'bg-foreground text-background' : 'text-faint hover:text-foreground'"
                @click="setLocale('en')"
              >
                EN
              </button>
            </div>
          </div>
        </section>

        <!-- 自动化摘要 -->
        <section class="border border-border bg-card">
          <div class="flex items-center justify-between border-b border-border px-3 py-1.5">
            <span class="lbl">// {{ t("st.schedule") }}</span>
            <span
              class="readout border px-1 py-0.5 text-[8px] uppercase"
              :class="scheduleOn ? 'border-hazard text-hazard' : 'border-border-strong text-faint'"
            >
              {{ scheduleOn ? t("st.schedOn") : t("st.schedOff") }}
            </span>
          </div>
          <div class="divide-y divide-border/60">
            <div class="flex items-center justify-between gap-3 px-3 py-2">
              <span class="text-[11px] text-muted-foreground">{{ t("st.schedTime") }}</span>
              <span class="readout text-[13px] text-foreground">{{ schedule.time ?? "—" }}</span>
            </div>
            <div class="flex items-center justify-between gap-3 px-3 py-2">
              <span class="text-[11px] text-muted-foreground">{{ t("st.lastRun") }}</span>
              <span class="readout text-[11px] text-foreground">
                {{ schedule.lastRunDate || t("st.never") }}
              </span>
            </div>
            <div class="px-3 py-2">
              <button
                type="button"
                class="w-full border border-border-strong px-2 py-1 text-[10px] uppercase tracking-[0.1em] text-muted-foreground transition-colors hover:border-hazard hover:text-hazard"
                @click="go('tasks')"
              >
                {{ t("st.gotoTasks") }}
              </button>
            </div>
          </div>
        </section>

        <!-- 维护 -->
        <section class="border border-border bg-card xl:col-span-2">
          <div class="border-b border-border px-3 py-1.5">
            <span class="lbl">// {{ t("st.maint") }}</span>
          </div>
          <div class="flex flex-wrap items-center justify-between gap-2 px-3 py-2.5">
            <span class="text-[9px] text-faint">{{ t("st.maintHint") }}</span>
            <div class="flex flex-wrap items-center gap-1.5">
              <button
                v-for="item in CLEARS"
                :key="item.source"
                type="button"
                class="flex items-center gap-1 border border-border-strong px-2 py-1 text-[10px] uppercase tracking-[0.1em] text-muted-foreground transition-colors hover:border-hazard hover:text-hazard"
                @click="clearTarget = item"
              >
                <Eraser class="size-2.5" />
                {{ t(item.labelKey) }}
              </button>
            </div>
          </div>
        </section>
      </div>
    </div>

    <Teleport to="body">
      <div
        v-if="clearTarget"
        class="fixed inset-0 z-50 grid place-items-center bg-black/60 p-4"
        @click.self="clearTarget = null"
      >
        <div class="w-full max-w-sm border border-hazard bg-card">
          <div class="flex items-center justify-between border-b border-border px-3 py-2">
            <span class="text-[12px] tracking-[0.1em] text-hazard">{{ t(clearTarget.labelKey) }}</span>
            <button type="button" class="text-[11px] text-muted-foreground hover:text-foreground" @click="clearTarget = null">
              ✕
            </button>
          </div>
          <div class="px-3 py-3 text-[11px] leading-relaxed text-muted-foreground">
            {{ t("st.maintHint") }}
          </div>
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
              {{ t("st.maint") }}
            </button>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>
