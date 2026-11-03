<script setup lang="ts">
/**
 * 归档任务（06）：WorkBuddy 会话库 archived 会话台账 + 唯一删除入口。
 * 删除是低频危险操作：确认弹框列明条数/体积/备份后路；WB 运行中后端拒绝执行。
 */
import { computed, onMounted, ref } from "vue";
import { CircleAlert, Eraser, Loader2, RefreshCw } from "lucide-vue-next";
import { archivedTasksList, purgeArchivedTasks, type ArchivedList } from "@/lib/api";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { formatStamp } from "@/lib/utils";
import { t, type I18nKey } from "@/lib/i18n";

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
// 与「切换账号」同一套交互：点按钮 → 弹框 → 自动关 WorkBuddy → 清理 → 自动拉起 → 弹框自关。
// 不再挂「请先退出 WorkBuddy」的提示条：进程由后端自己关，关不掉才在弹框里报阶段。
const purgeOpen = ref(false);
const purging = ref(false);
const progress = ref("");
const purgeStage = ref("");
const purgeError = ref("");
const purgeDone = ref<{
  deleted: number;
  human: string;
  backupDir: string;
  launchError: string;
} | null>(null);

const purgeBlocked = computed(() => !data.value || !data.value.ok || data.value.total === 0);
const purgeBullets = computed(() => t("pz.bullets").split("\n"));

const STAGE_KEYS: Record<string, I18nKey> = {
  close: "pz.stageClose",
  purge: "pz.stagePurge",
  done: "pz.stageDone",
};
const stageLabel = (stage: string) => (STAGE_KEYS[stage] ? t(STAGE_KEYS[stage]) : stage || "-");

function openPurge() {
  if (purging.value) return;
  purgeError.value = "";
  purgeDone.value = null;
  progress.value = "";
  purgeStage.value = "";
  purgeOpen.value = true;
}

function closePurge() {
  if (purging.value) return;
  purgeOpen.value = false;
}

async function doPurge() {
  if (purging.value) return;
  purging.value = true;
  purgeError.value = "";
  purgeDone.value = null;
  progress.value = "";
  let unlisten: UnlistenFn | null = null;
  try {
    unlisten = await listen<{ message: string }>("purge-progress", (event) => {
      progress.value = event.payload.message;
    });
    // 后端不抛错：失败也返回 ok:false，附带 stage。
    const result = await purgeArchivedTasks(true);
    if (!result.ok) {
      purgeStage.value = result.stage ?? "";
      purgeError.value = result.error || t("st.fail");
      return;
    }
    purgeDone.value = {
      deleted: result.deleted ?? 0,
      human: result.human ?? "0B",
      backupDir: result.backupDir ?? "",
      launchError: result.launchError ?? "",
    };
    notice.value = `${t("pz.done")} · ${result.deleted ?? 0} · ${result.human ?? ""}`;
    // 成功即自动收尾（与切换账号一致），不逼用户再点一次关闭。
    window.setTimeout(() => {
      purgeOpen.value = false;
    }, 1500);
  } catch (cause) {
    purgeStage.value = "";
    purgeError.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    unlisten?.();
    purging.value = false;
    await load();
  }
}
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <header class="flex shrink-0 items-center justify-between gap-3 border-b border-border bg-panel px-4 py-2 wb-flat">
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
          class="flex shrink-0 items-center gap-1.5 border border-border-strong px-2.5 py-1 text-[10px] uppercase tracking-[0.1em] text-muted-foreground transition-colors hover:border-hazard hover:text-hazard disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:border-border-strong disabled:hover:text-muted-foreground"
          :disabled="purgeBlocked"
          :title="t('ar.hint')"
          @click="openPurge()"
        >
          <Eraser class="size-3" />
          {{ t("pz.clean") }}
        </button>
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

    <!-- 台账表：粘性表头 + 行滚动，排序与口径都由后端定（归档时刻倒序） -->
    <div class="min-h-0 flex-1 overflow-y-auto p-3">
      <div class="overflow-hidden border border-border bg-card">
      <div class="sticky top-0 z-10 grid grid-cols-[36px_minmax(0,1fr)_150px_72px] items-center gap-2 wb-cells border-b border-border bg-card px-3 py-1.5">
        <span class="lbl">#</span>
        <span class="lbl">{{ t("ar.colTask") }}</span>
        <span class="lbl text-right">{{ t("ar.colDate") }}</span>
        <span class="lbl text-right">{{ t("ar.colSize") }}</span>
      </div>

      <div
        v-for="(row, index) in data?.rows ?? []"
        :key="row.id"
        class="grid grid-cols-[36px_minmax(0,1fr)_150px_72px] items-center gap-2 wb-cells border-b border-border/40 px-3 py-1.5"
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
    </div>

    <Teleport to="body">
      <!-- 清理确认：复刻「切换账号」弹框结构；数字来自实扫，不可恢复提示必须过目 -->
      <div
        v-if="purgeOpen"
        class="fixed inset-0 z-50 flex items-center justify-center bg-background/80 p-4"
        role="dialog"
        aria-modal="true"
        @click.self="closePurge()"
      >
        <div class="flex max-h-[85vh] w-full max-w-[480px] flex-col border border-border-strong bg-card">
          <header class="flex shrink-0 items-start justify-between gap-3 border-b border-border px-3 py-2 wb-flat">
            <div class="min-w-0">
              <div class="text-[13px] text-foreground">{{ t("pz.title") }}</div>
              <div class="readout mt-0.5 text-[9px] text-faint">
                {{ t("pz.subtitle").replace("{n}", String(data?.total ?? 0)).replace("{size}", data?.human ?? "0B") }}
              </div>
            </div>
            <button
              type="button"
              class="shrink-0 px-1 text-[11px] text-faint hover:text-foreground"
              :disabled="purging"
              @click="closePurge()"
            >
              ✕
            </button>
          </header>

          <div class="min-h-0 flex-1 overflow-y-auto px-3 py-3">
            <div class="border border-hazard bg-hazard-soft px-3 py-2.5">
              <div class="flex items-center gap-1.5 text-[11px] text-hazard">
                <CircleAlert class="size-3.5 shrink-0" />
                {{ t("pz.title") }}
              </div>
              <ul class="mt-1.5 list-disc space-y-1 pl-5 text-[10px] leading-4 text-muted-foreground">
                <li v-for="(bullet, bulletIndex) in purgeBullets" :key="bulletIndex">{{ bullet }}</li>
              </ul>
            </div>

            <p v-if="purging" class="readout mt-3 flex items-center gap-2 text-[10px] text-foreground">
              <Loader2 class="size-3 animate-spin" />
              {{ progress || t("pz.going") }}
            </p>
            <p v-else-if="purgeDone" class="readout mt-3 text-[10px] text-foreground">
              {{ t("pz.deleted").replace("{n}", String(purgeDone.deleted)).replace("{size}", purgeDone.human) }}
            </p>
            <p v-else class="readout mt-3 text-[10px] text-muted-foreground">{{ t("pz.body") }}</p>

            <div
              v-if="purgeDone?.backupDir"
              class="mt-2 flex gap-2 border border-border px-3 py-1.5 text-[10px]"
            >
              <span class="w-14 shrink-0 text-faint">{{ t("pz.backup") }}</span>
              <span class="readout min-w-0 flex-1 break-all text-muted-foreground">{{ purgeDone.backupDir }}</span>
            </div>
            <div
              v-if="purgeDone?.launchError"
              class="mt-2 border border-hazard bg-hazard-soft px-3 py-1.5 text-[10px] text-hazard"
            >
              {{ t("pz.launchFail") }} · {{ purgeDone.launchError }}
            </div>

            <!-- 失败现场：阶段 + 原因，留在弹框里，别跟着 toast 一起消失 -->
            <div v-if="purgeError" class="mt-3 border border-hazard">
              <div class="flex items-center gap-1.5 border-b border-hazard bg-hazard-soft px-3 py-2 text-[11px] text-hazard">
                <CircleAlert class="size-3.5 shrink-0" />
                {{ t("pz.blocked") }}
              </div>
              <div class="flex gap-2 border-b border-border px-3 py-1.5 text-[10px]">
                <span class="w-14 shrink-0 text-faint">{{ t("pz.stage") }}</span>
                <span class="readout min-w-0 flex-1 text-foreground">{{ stageLabel(purgeStage) }}</span>
              </div>
              <p class="readout whitespace-pre-wrap break-words px-3 py-2 text-[10px] leading-4 text-muted-foreground">
                {{ purgeError }}
              </p>
            </div>
          </div>

          <footer class="flex shrink-0 items-center justify-end gap-2 border-t border-border px-3 py-2">
            <button
              type="button"
              class="border border-border-strong px-3 py-1 text-[10px] uppercase tracking-[0.1em] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground disabled:opacity-40"
              :disabled="purging"
              @click="closePurge()"
            >
              {{ t("ac.close") }}
            </button>
            <button
              type="button"
              class="flex items-center gap-1.5 bg-hazard px-3 py-1 text-[10px] uppercase tracking-[0.1em] text-white transition-opacity hover:opacity-85 disabled:opacity-40"
              :disabled="purging"
              @click="doPurge()"
            >
              <Loader2 v-if="purging" class="size-3 animate-spin" />
              <Eraser v-else class="size-3" />
              {{ purging ? t("pz.going") : t("pz.confirm") }}
            </button>
          </footer>
        </div>
      </div>
    </Teleport>
  </div>
</template>
