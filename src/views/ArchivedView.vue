<script setup lang="ts">
/**
 * 归档任务（06）：WorkBuddy 会话库 archived 会话台账 + 唯一删除入口。
 * 删除是低频危险操作：确认弹框列明条数/体积/备份后路；WB 运行中后端拒绝执行。
 */
import { computed, onMounted, ref } from "vue";
import { CircleAlert, Eraser, Loader2, RefreshCw } from "lucide-vue-next";
import { archivedTasksList, purgeArchivedTasks, type ArchivedList } from "@/lib/api";
import { formatStamp } from "@/lib/utils";
import { t } from "@/lib/i18n";

const data = ref<ArchivedList | null>(null);
const loading = ref(false);
const error = ref("");
const notice = ref("");

async function load() {
  if (loading.value) return;
  loading.value = true;
  error.value = "";
  try {
    data.value = await archivedTasksList();
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    loading.value = false;
  }
}

onMounted(() => void load());

const dbMissing = computed(() => !!data.value && !data.value.dbExists);
const readout = computed(() => {
  const stats = data.value;
  if (!stats) return "—";
  return `${t("ar.total")} ${stats.total} · ${stats.human}`;
});

// ───────────────────────── 清理（唯一入口） ─────────────────────────
const purgeOpen = ref(false);
const purging = ref(false);

const purgeBlocked = computed(() => !data.value || !data.value.ok || data.value.total === 0);

async function doPurge() {
  if (purging.value) return;
  purging.value = true;
  try {
    const result = await purgeArchivedTasks();
    if (!result.ok) throw new Error(result.error || "purge failed");
    notice.value = `${t("pz.done")} · ${result.deleted ?? 0} · ${result.human ?? ""}`;
    purgeOpen.value = false;
  } catch (cause) {
    error.value = `${t("st.fail")}: ${cause instanceof Error ? cause.message : String(cause)}`;
    purgeOpen.value = false;
  } finally {
    purging.value = false;
    await load();
  }
}
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <header class="flex shrink-0 items-center justify-between gap-3 border-b border-border bg-panel px-4 py-2">
      <div class="min-w-0">
        <h1 class="text-[13px] tracking-[0.12em] text-foreground">{{ t("ar.title") }}</h1>
        <div class="readout mt-0.5 truncate text-[9px] text-faint">
          {{ t("ar.subtitle") }}
          <span v-if="notice" class="ml-2">{{ notice }}</span>
        </div>
      </div>
      <div class="flex shrink-0 items-center gap-2">
        <span v-if="data?.running" class="readout border border-hazard bg-hazard-soft px-1.5 py-0.5 text-[8px] uppercase text-hazard">
          {{ t("ar.running") }}
        </span>
        <span class="readout text-[10px]" :class="data?.total ? 'text-hazard' : 'text-muted-foreground'">{{ readout }}</span>
        <button
          type="button"
          class="flex items-center gap-1.5 border border-border-strong px-2.5 py-1 text-[10px] uppercase tracking-[0.1em] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground disabled:opacity-40"
          :disabled="loading"
          @click="load()"
        >
          <Loader2 v-if="loading" class="size-3 animate-spin" />
          <RefreshCw v-else class="size-3" />
          {{ t("ar.refresh") }}
        </button>
      </div>
    </header>

    <div v-if="error || dbMissing" class="flex shrink-0 items-center gap-2 border-b border-hazard bg-hazard-soft px-4 py-2 text-[11px] text-hazard">
      <CircleAlert class="size-3.5 shrink-0" />
      <span class="truncate">{{ dbMissing ? t("ar.dbMissing") : error }}</span>
    </div>
    <div v-else-if="data?.running" class="flex shrink-0 items-center gap-2 border-b border-hazard/40 bg-hazard-soft px-4 py-2 text-[10px] text-hazard">
      <CircleAlert class="size-3 shrink-0" />
      <span class="truncate">{{ t("pz.running") }}</span>
    </div>

    <!-- 台账表：粘性表头 + 行滚动，排序与口径都由后端定（归档时刻倒序） -->
    <div class="min-h-0 flex-1 overflow-y-auto">
      <div class="sticky top-0 z-10 grid grid-cols-[36px_minmax(0,1fr)_150px_72px] items-center gap-2 border-b border-border bg-card px-3 py-1.5">
        <span class="lbl">#</span>
        <span class="lbl">{{ t("ar.colTask") }}</span>
        <span class="lbl text-right">{{ t("ar.colDate") }}</span>
        <span class="lbl text-right">{{ t("ar.colSize") }}</span>
      </div>

      <div
        v-for="(row, index) in data?.rows ?? []"
        :key="row.id"
        class="grid grid-cols-[36px_minmax(0,1fr)_150px_72px] items-center gap-2 border-b border-border/40 px-3 py-1.5"
      >
        <span class="readout text-[9px] text-faint">{{ String(index + 1).padStart(2, "0") }}</span>
        <span class="truncate text-[11px] text-foreground" :title="row.title">{{ row.title }}</span>
        <span class="readout text-right text-[10px] text-muted-foreground">{{ formatStamp(row.updatedAt) }}</span>
        <span class="readout text-right text-[10px]" :class="row.bytes ? 'text-muted-foreground' : 'text-faint'">
          {{ row.human }}
        </span>
      </div>

      <div v-if="!loading && !data?.total" class="grid place-items-center py-16 text-[11px] text-faint">
        {{ t("ar.empty") }}
      </div>
    </div>

    <footer class="flex shrink-0 items-center justify-between gap-2 border-t border-border bg-panel px-4 py-2">
      <span class="text-[9px] text-faint">{{ t("ar.hint") }}</span>
      <button
        type="button"
        class="flex shrink-0 items-center gap-1.5 border border-border-strong px-2.5 py-1 text-[10px] uppercase tracking-[0.1em] text-muted-foreground transition-colors hover:border-hazard hover:text-hazard disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:border-border-strong disabled:hover:text-muted-foreground"
        :disabled="purgeBlocked"
        :title="data?.running ? t('pz.running') : undefined"
        @click="purgeOpen = true"
      >
        <Eraser class="size-3" />
        {{ t("pz.clean") }}
      </button>
    </footer>

    <Teleport to="body">
      <!-- 清理确认：数字来自实扫，不可恢复提示必须过目 -->
      <div
        v-if="purgeOpen"
        class="fixed inset-0 z-50 grid place-items-center bg-black/60 p-4"
        @click.self="purgeOpen = false"
      >
        <div class="w-full max-w-sm border border-hazard bg-card">
          <div class="flex items-center justify-between border-b border-border px-3 py-2">
            <span class="text-[12px] tracking-[0.1em] text-hazard">{{ t("pz.title") }}</span>
            <button type="button" class="text-[11px] text-muted-foreground hover:text-foreground" @click="purgeOpen = false">
              ✕
            </button>
          </div>
          <div class="space-y-2 px-3 py-3 text-[11px] leading-relaxed text-muted-foreground">
            <p>{{ t("pz.body") }}</p>
            <p class="readout text-[10px] text-foreground">{{ readout }}</p>
          </div>
          <div class="flex justify-end gap-1.5 border-t border-border px-3 py-2">
            <button
              type="button"
              class="border border-border-strong px-2.5 py-1 text-[10px] uppercase tracking-[0.1em] text-muted-foreground hover:text-foreground"
              @click="purgeOpen = false"
            >
              CANCEL
            </button>
            <button
              type="button"
              class="flex items-center gap-1.5 border border-hazard bg-hazard px-2.5 py-1 text-[10px] uppercase tracking-[0.1em] text-white disabled:opacity-40"
              :disabled="purging"
              @click="doPurge()"
            >
              <Loader2 v-if="purging" class="size-3 animate-spin" />
              {{ t("pz.confirm") }}
            </button>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>
