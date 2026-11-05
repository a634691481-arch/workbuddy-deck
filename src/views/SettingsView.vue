<script setup lang="ts">
/**
 * 系统设置：运行环境 / 数据体积 / 显示偏好 / 自动化摘要 / 维护。
 *
 * 定位是「只读体检 + 少量开关」：真正的业务配置（自动化时间、账号增删）仍在各自页面，
 * 这里只做总览与快捷入口，避免同一份配置在两处维护出分歧。
 */
import { computed, onMounted, ref } from "vue";
import { CircleAlert, Download, Eraser, ExternalLink, FolderOpen, Loader2, RefreshCw } from "lucide-vue-next";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useUpdate } from "@/composables/useUpdate";
import { formatClock } from "@/lib/utils";
import {
  archivedTasksStats,
  autoSwitchCheck,
  autoSwitchSet,
  autoSwitchStatus,
  eventsClear,
  exportAllData,
  exportDiagnostics,
  exportData,
  openExternal,
  openStoreDir,
  systemInfo,
  type ArchivedStats,
  type AutoSwitchCheck,
  type AutoSwitchConfig,
  type AutoSwitchStatus,
  type EventSource,
  type ExportKind,
  type SystemInfo,
} from "@/lib/api";
import { useShell } from "@/composables/useShell";
import { t } from "@/lib/i18n";

const { go } = useShell();

/** 开源地址：只展示本应用仓库。 */
const REPO_URL = "https://github.com/a634691481-arch/workbuddy-deck";

async function openRepo(url: string) {
  try {
    const result = await openExternal(url);
    if (!result.ok) notice.value = result.error ?? t("st.repoFail");
  } catch (cause) {
    notice.value = cause instanceof Error ? cause.message : String(cause);
  }
}

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

onMounted(() => {
  void load();
  void loadAutoSwitch();
  void loadUpdate();
  void loadArchived();
});

// ───────────────────────── 归档任务读数 ─────────────────────────
// 这里只显示用量；删除入口唯一在「归档任务」分区（06），避免两处都能删。
const archived = ref<ArchivedStats | null>(null);

async function loadArchived() {
  try {
    archived.value = await archivedTasksStats();
  } catch {
    archived.value = null;
  }
}

const purgeReadout = computed(() => {
  const stats = archived.value;
  if (!stats) return "—";
  return `${stats.count} · ${stats.human}`;
});

// ───────────────────────── 自动更新 ─────────────────────────
// 检查 / 下载 / 进度都在 composable 里，这里只管「把结果排成一行读数」。
// 解构成顶层 ref，模板才会自动解包（`up.status` 这种包一层的写法不会）。
const {
  status: upStatus,
  checking: upChecking,
  installing: upInstalling,
  downloaded: upDownloaded,
  percent: upPercent,
  message: upMessage,
  autoCheck: upAuto,
  mirror: upMirror,
  current: upCurrent,
  silent: upSilent,
  setSilent: setSilentUpdate,
  hasUpdate: upHasUpdate,
  loadConfig: loadUpdateConfig,
  check: checkUpdate,
  install: installUpdate,
  saveConfig: saveUpdateConfig,
} = useUpdate();

const mirrorDraft = ref("");
const mirrorSaved = ref(false);

async function loadUpdate() {
  await loadUpdateConfig();
  mirrorDraft.value = upMirror.value;
}

async function saveMirror() {
  await saveUpdateConfig(upAuto.value, mirrorDraft.value.trim());
  mirrorDraft.value = upMirror.value;
  mirrorSaved.value = true;
  window.setTimeout(() => (mirrorSaved.value = false), 1600);
  // 换源后旧结果作废：清掉再查一次，避免「填了镜像仍显示直连失败」
  await checkUpdate(true);
}

async function toggleAutoCheck() {
  await saveUpdateConfig(!upAuto.value, upMirror.value);
}

const checkedLabel = computed(() => {
  const at = upStatus.value?.checkedAt;
  return at ? formatClock(at * 1000) : t("up.unknown");
});

const latestLabel = computed(() => {
  const s = upStatus.value;
  if (!s?.ok) return t("up.unknown");
  return s.latest ? `v${s.latest}` : t("up.unknown");
});

const progressLabel = computed(() => {
  if (upPercent.value === null) return `${(upDownloaded.value / 1048576).toFixed(1)} MB`;
  return `${upPercent.value}%`;
});

// ───────────────────────── 限流自动切换 ─────────────────────────

const autoSwitch = ref<AutoSwitchStatus | null>(null);
const autoCheck = ref<AutoSwitchCheck | null>(null);
const autoBusy = ref(false);

async function loadAutoSwitch() {
  try {
    autoSwitch.value = await autoSwitchStatus();
  } catch {
    autoSwitch.value = null;
  }
}

/** 局部更新：只发要改的字段，后端合并后回整体状态。 */
async function patchAutoSwitch(patch: Partial<AutoSwitchConfig>) {
  if (autoBusy.value) return;
  autoBusy.value = true;
  try {
    const result = await autoSwitchSet(patch);
    if (result.ok && result.status) autoSwitch.value = result.status;
  } catch {
    /* 写入失败保持旧状态，下一次进页面会重新拉 */
  } finally {
    autoBusy.value = false;
  }
}

/** 手动判定：只回答「现在会怎么判」，绝不执行切换。 */
async function runAutoCheck() {
  if (autoBusy.value) return;
  autoBusy.value = true;
  try {
    autoCheck.value = await autoSwitchCheck();
  } catch {
    autoCheck.value = null;
  } finally {
    autoBusy.value = false;
  }
}

const autoOn = computed(() => autoSwitch.value?.config.enabled === true);
const autoNotifyOnly = computed(() => autoSwitch.value?.config.notifyOnly !== false);
const autoAsk = computed(() => autoSwitch.value?.config.askBeforeSwitch !== false);
const autoBgNotify = computed(() => autoSwitch.value?.config.notifyWhenBackground !== false);
const autoCountdown = computed(() => autoSwitch.value?.config.countdownSeconds ?? 15);

function onNumberChange(
  event: Event,
  key: "cooldownMinutes" | "minRemaining" | "countdownSeconds",
  min: number,
) {
  const raw = Number((event.target as HTMLInputElement).value);
  if (!Number.isFinite(raw)) return;
  void patchAutoSwitch({ [key]: Math.max(min, Math.round(raw)) } as Partial<AutoSwitchConfig>);
}

/** 最近一次判定的可读文案：动作 + 原因。 */
const lastDecisionText = computed(() => {
  const event = autoSwitch.value?.lastDecision;
  if (!event) return "";
  const action =
    event.action === "switch" ? t("st.autoActSwitch") : event.action === "notify" ? t("st.autoActNotify") : t("st.autoActSkip");
  return `${action} · ${event.reason ?? ""}`;
});

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
    { key: "switch", labelKey: "st.fileSwitch" as const, rows: counts.switchHistory, stat: files.switchHistory },
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

/**
 * 数据导出：四份 CSV，全部由后端直接写盘。
 *
 * 为什么不用「复制到剪贴板」：Token 明细动辄上万行，剪贴板会卡死；
 * 而且导出的意义就是能带进 Excel 做二次计算。
 */
const EXPORTS: { kind: ExportKind; labelKey: "st.exAccounts" | "st.exTokensDaily" | "st.exTokensModels" | "st.exCredits"; file: string }[] = [
  { kind: "accounts", labelKey: "st.exAccounts", file: "accounts" },
  { kind: "tokensDaily", labelKey: "st.exTokensDaily", file: "tokens-daily" },
  { kind: "tokensModels", labelKey: "st.exTokensModels", file: "tokens-models" },
  { kind: "credits", labelKey: "st.exCredits", file: "credits" },
];

const exporting = ref<"" | ExportKind | "all" | "diag">("");
const exportNote = ref("");
const exportFailed = ref(false);

const todayTag = () => new Date().toISOString().slice(0, 10);

async function runExport(kind: ExportKind, file: string) {
  if (exporting.value) return;
  try {
    const path = await save({
      title: t("st.export"),
      defaultPath: `${file}-${todayTag()}.csv`,
      filters: [{ name: "CSV", extensions: ["csv"] }],
    });
    if (!path) return;
    exporting.value = kind;
    exportNote.value = "";
    exportFailed.value = false;
    const result = await exportData(kind, path);
    exportFailed.value = !result.ok;
    exportNote.value = result.ok
      ? `${t("st.exDone")} · ${path}`
      : `${t("st.exFail")} · ${result.error ?? ""}`;
  } catch (cause) {
    exportFailed.value = true;
    exportNote.value = `${t("st.exFail")} · ${cause instanceof Error ? cause.message : String(cause)}`;
  } finally {
    exporting.value = "";
  }
}

/** 诊断包：版本 + 平台 + 数据规模 + 日志尾部，脱敏后落一个 txt。 */
async function runDiagnostics() {
  if (exporting.value) return;
  try {
    const path = await save({
      title: t("st.diagSave"),
      defaultPath: `workbuddy-deck-diagnostics-${Date.now()}.txt`,
      filters: [{ name: "Text", extensions: ["txt"] }],
    });
    if (!path) return;
    exporting.value = "diag";
    exportNote.value = "";
    exportFailed.value = false;
    const result = await exportDiagnostics(path);
    exportFailed.value = !result.ok;
    exportNote.value = result.ok ? `${t("st.diagDone")} · ${path}` : `${t("st.exFail")} · ${result.error ?? ""}`;
  } catch (cause) {
    exportFailed.value = true;
    exportNote.value = `${t("st.exFail")} · ${cause instanceof Error ? cause.message : String(cause)}`;
  } finally {
    exporting.value = "";
  }
}

/** 全部导出：选一个目录，写四份文件，逐份汇报成功/失败。 */
async function runExportAll() {
  if (exporting.value) return;
  try {
    const dir = await open({ title: t("st.exAll"), directory: true, multiple: false });
    if (!dir || typeof dir !== "string") return;
    exporting.value = "all";
    exportNote.value = "";
    exportFailed.value = false;
    const result = await exportAllData(dir);
    exportFailed.value = !result.ok;
    exportNote.value = result.ok
      ? `${t("st.exDone")} · ${result.files.length} ${t("st.exFiles")} · ${dir}`
      : `${t("st.exFail")} · ${result.failed}/${result.files.length}`;
  } catch (cause) {
    exportFailed.value = true;
    exportNote.value = `${t("st.exFail")} · ${cause instanceof Error ? cause.message : String(cause)}`;
  } finally {
    exporting.value = "";
  }
}

/** 维护区：清空某一源前先确认，避免误点把排障线索删干净。 */const clearTarget = ref<{ source: EventSource; labelKey: "st.clearNotify" | "st.clearAuto" | "st.clearError" | "st.clearSwitch" } | null>(null);
const clearing = ref(false);

const CLEARS: { source: EventSource; labelKey: "st.clearNotify" | "st.clearAuto" | "st.clearError" | "st.clearSwitch" }[] = [
  { source: "notify", labelKey: "st.clearNotify" },
  { source: "auto", labelKey: "st.clearAuto" },
  { source: "switch", labelKey: "st.clearSwitch" },
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
    <header class="flex shrink-0 items-center justify-between gap-3 border-b border-border bg-panel px-4 py-2 wb-flat">
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
      <div class="grid grid-cols-1 gap-3 lg:grid-cols-2">
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
              <span class="text-[11px] text-muted-foreground">{{ t("st.repo") }}</span>
              <button
                type="button"
                class="readout flex max-w-[240px] items-center gap-1 border border-border-strong px-1.5 py-0.5 text-[9px] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground"
                :title="REPO_URL"
                @click="openRepo(REPO_URL)"
              >
                <span class="min-w-0 truncate">a634691481-arch/workbuddy-deck</span>
                <ExternalLink class="size-2.5 shrink-0" />
              </button>
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

          <!-- 归档任务：只读读数，删除入口在「归档任务」分区 -->
          <div class="flex items-center justify-between gap-2 border-t border-border px-3 py-1.5" :title="t('pz.statHint')">
            <span class="text-[11px] text-foreground">{{ t("pz.row") }}</span>
            <span class="readout text-[10px]" :class="archived?.count ? 'text-hazard' : 'text-muted-foreground'">
              {{ purgeReadout }}
            </span>
          </div>
        </section>

        <!-- 自动更新：当前 / 最新 / 检查时刻 三格读数 + 一行动作 + 一行设置 -->
        <section class="border border-border bg-card lg:col-span-2">
          <div class="flex items-center justify-between border-b border-border px-3 py-1.5">
            <div class="flex items-center gap-2">
              <span class="lbl">// {{ t("up.title") }}</span>
              <span
                v-if="upHasUpdate && !upInstalling"
                class="readout border border-hazard bg-hazard-soft px-1.5 py-0.5 text-[8px] uppercase text-hazard"
              >
                {{ t("up.available") }}
              </span>
            </div>
            <button
              type="button"
              class="flex items-center gap-1 border border-border-strong px-2 py-0.5 text-[9px] uppercase tracking-[0.1em] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground disabled:opacity-40"
              :disabled="upChecking || upInstalling"
              @click="checkUpdate(true)"
            >
              <Loader2 v-if="upChecking" class="size-3 animate-spin" />
              <RefreshCw v-else class="size-3" />
              {{ upChecking ? t("up.checking") : t("up.check") }}
            </button>
          </div>

          <div class="mb-3 grid grid-cols-3 gap-2">
            <div class="bg-card px-3 py-2">
              <div class="lbl">{{ t("up.current") }}</div>
              <div class="readout mt-0.5 text-[13px] text-foreground">
                v{{ upCurrent || info?.version || "—" }}
              </div>
            </div>
            <div class="bg-card px-3 py-2">
              <div class="lbl">{{ t("up.latest") }}</div>
              <div class="readout mt-0.5 text-[13px]" :class="upHasUpdate ? 'text-hazard' : 'text-foreground'">
                {{ latestLabel }}
              </div>
            </div>
            <div class="bg-card px-3 py-2">
              <div class="lbl">{{ t("up.checkedAt") }}</div>
              <div class="readout mt-0.5 text-[13px] text-muted-foreground">{{ checkedLabel }}</div>
            </div>
          </div>

          <!-- 下载进度 / 结果提示：只有进行中或出错时才出现，常态不占高度 -->
          <div v-if="upInstalling || upMessage" class="border-t border-border px-3 py-2">
            <div v-if="upInstalling" class="flex items-center gap-2">
              <div class="h-1 min-w-0 flex-1 bg-border">
                <div class="h-full bg-hazard transition-[width]" :style="{ width: `${upPercent ?? 4}%` }" />
              </div>
              <span class="readout shrink-0 text-[10px] text-hazard">{{ progressLabel }}</span>
            </div>
            <div v-else class="flex items-start gap-1.5 text-[10px] text-hazard">
              <CircleAlert class="mt-px size-3 shrink-0" />
              <span class="min-w-0 break-all">{{ upMessage }}</span>
            </div>
          </div>

          <div class="flex flex-wrap items-center gap-x-3 gap-y-2 border-t border-border px-3 py-2">
            <button
              type="button"
              class="flex shrink-0 items-center gap-1 border px-2 py-1 text-[10px] uppercase tracking-[0.1em] transition-colors disabled:opacity-40"
              :class="upHasUpdate && !upInstalling ? 'border-hazard bg-hazard text-white hover:bg-hazard/90' : 'border-border-strong text-muted-foreground hover:border-foreground hover:text-foreground'"
              :disabled="upInstalling || upChecking || !upHasUpdate"
              @click="installUpdate()"
            >
              <Download class="size-3" />
              {{ upInstalling ? t("up.installing") : t("up.install") }}
            </button>

            <button
              type="button"
              class="readout shrink-0 border px-1.5 py-1 text-[9px] uppercase transition-colors"
              :class="upAuto ? 'border-hazard bg-hazard-soft text-hazard' : 'border-border-strong text-faint hover:text-foreground'"
              @click="toggleAutoCheck()"
            >
              {{ t("up.auto") }} {{ upAuto ? t("st.on") : t("st.off") }}
            </button>

            <!-- 静默更新：开后检测到新版本直接下载安装，不再弹任何提示 -->
            <button
              type="button"
              class="readout shrink-0 border px-1.5 py-1 text-[9px] uppercase transition-colors"
              :class="upSilent ? 'border-hazard bg-hazard-soft text-hazard' : 'border-border-strong text-faint hover:text-foreground'"
              :title="t('up.silentHint')"
              @click="setSilentUpdate(!upSilent)"
            >
              {{ t("up.silent") }} {{ upSilent ? t("st.on") : t("st.off") }}
            </button>

            <label class="flex min-w-[200px] flex-1 items-center gap-1 text-[9px] text-faint">
              {{ t("up.mirror") }}
              <input
                v-model="mirrorDraft"
                type="text"
                :placeholder="t('up.mirrorHint')"
                class="readout min-w-0 flex-1 border border-border-strong bg-input px-1 py-0.5 text-[10px] text-foreground"
                @keyup.enter="saveMirror()"
              />
            </label>
            <button
              type="button"
              class="shrink-0 border border-border-strong px-2 py-1 text-[10px] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground"
              @click="saveMirror()"
            >
              {{ mirrorSaved ? t("up.saved") : t("up.save") }}
            </button>
          </div>

          <div class="flex flex-wrap items-center gap-x-3 gap-y-1 border-t border-border/60 px-3 py-1.5 text-[9px] text-faint">
            <span>{{ t("up.autoHint") }}</span>
            <span class="text-border-strong">/</span>
            <span>{{ t("up.restartHint") }}</span>
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

        <!-- 限流自动切换 -->
        <section class="border border-border bg-card">
          <div class="flex items-center justify-between border-b border-border px-3 py-1.5">
            <span class="lbl">// {{ t("st.autoSwitch") }}</span>
            <button
              type="button"
              class="readout border px-1.5 py-0.5 text-[8px] uppercase transition-colors disabled:opacity-40"
              :class="autoOn ? 'border-hazard bg-hazard-soft text-hazard' : 'border-border-strong text-faint hover:text-foreground'"
              :disabled="autoBusy"
              @click="patchAutoSwitch({ enabled: !autoOn })"
            >
              {{ autoOn ? t("st.autoOn") : t("st.autoOff") }}
            </button>
          </div>
          <p class="border-b border-border/40 px-3 py-1.5 text-[9px] leading-4 text-faint">{{ t("st.autoSwitchHint") }}</p>
          <div class="flex flex-wrap items-center gap-x-3 gap-y-2 px-3 py-2">
            <div class="flex shrink-0 overflow-hidden">
              <button
                type="button"
                class="px-2 py-1 text-[10px] transition-colors disabled:opacity-40"
                :class="autoNotifyOnly ? 'bg-foreground text-background' : 'bg-card text-muted-foreground hover:text-foreground'"
                :disabled="autoBusy || !autoOn"
                @click="patchAutoSwitch({ notifyOnly: true })"
              >
                {{ t("st.autoNotify") }}
              </button>
              <button
                type="button"
                class="px-2 py-1 text-[10px] transition-colors disabled:opacity-40"
                :class="!autoNotifyOnly ? 'bg-hazard text-white' : 'bg-card text-muted-foreground hover:text-foreground'"
                :disabled="autoBusy || !autoOn"
                @click="patchAutoSwitch({ notifyOnly: false })"
              >
                {{ t("st.autoApply") }}
              </button>
            </div>
            <button
              type="button"
              class="readout shrink-0 border px-1.5 py-1 text-[9px] uppercase transition-colors disabled:opacity-40"
              :class="autoAsk ? 'border-hazard bg-hazard-soft text-hazard' : 'border-border-strong text-faint hover:text-foreground'"
              :title="t('as.askHint')"
              :disabled="autoBusy || !autoOn"
              @click="patchAutoSwitch({ askBeforeSwitch: !autoAsk })"
            >
              {{ t("as.ask") }} {{ autoAsk ? t("st.on") : t("st.off") }}
            </button>
            <button
              type="button"
              class="readout shrink-0 border px-1.5 py-1 text-[9px] uppercase transition-colors disabled:opacity-40"
              :class="autoBgNotify ? 'border-hazard bg-hazard-soft text-hazard' : 'border-border-strong text-faint hover:text-foreground'"
              :title="t('as.bgNotifyHint')"
              :disabled="autoBusy || !autoOn || !autoAsk"
              @click="patchAutoSwitch({ notifyWhenBackground: !autoBgNotify })"
            >
              {{ t("as.bgNotify") }} {{ autoBgNotify ? t("st.on") : t("st.off") }}
            </button>
            <label class="flex shrink-0 items-center gap-1 text-[9px] text-faint">
              {{ t("as.countdown") }}
              <select
                class="readout border border-border-strong bg-input px-1 py-0.5 text-[10px] text-foreground"
                :value="autoCountdown"
                :disabled="autoBusy || !autoOn || !autoAsk"
                @change="onNumberChange($event, 'countdownSeconds', 3)"
              >
                <option :value="0">{{ t("as.waitMe") }}</option>
                <option v-for="sec in [3, 10, 15, 30, 60]" :key="sec" :value="sec">{{ sec }}</option>
              </select>
            </label>
            <label class="flex shrink-0 items-center gap-1 text-[9px] text-faint">
              {{ t("st.autoCooldown") }}
              <input
                type="number"
                min="1"
                max="1440"
                class="readout w-12 border border-border-strong bg-input px-1 py-0.5 text-[10px] text-foreground"
                :value="autoSwitch?.config.cooldownMinutes ?? 30"
                :disabled="autoBusy"
                @change="onNumberChange($event, 'cooldownMinutes', 1)"
              />
            </label>
            <label class="flex shrink-0 items-center gap-1 text-[9px] text-faint">
              {{ t("st.autoMin") }}
              <input
                type="number"
                min="0"
                max="100000"
                class="readout w-14 border border-border-strong bg-input px-1 py-0.5 text-[10px] text-foreground"
                :value="autoSwitch?.config.minRemaining ?? 50"
                :disabled="autoBusy"
                @change="onNumberChange($event, 'minRemaining', 0)"
              />
            </label>
            <button
              type="button"
              class="flex shrink-0 items-center gap-1 border border-border-strong px-2 py-1 text-[10px] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground disabled:opacity-40"
              :disabled="autoBusy"
              @click="runAutoCheck()"
            >
              <Loader2 v-if="autoBusy" class="size-2.5 animate-spin" />
              <RefreshCw v-else class="size-2.5" />
              {{ t("st.autoCheckBtn") }}
            </button>
          </div>
          <p
            v-if="autoOn && !autoNotifyOnly"
            class="border-t border-hazard/40 bg-hazard-soft px-3 py-1.5 text-[9px] leading-4 text-hazard"
          >
            {{ t("st.autoWarn") }}
          </p>
          <!-- 实时性说明：让用户知道它是「事件驱动 1 秒」而不是 60 秒轮询 -->
          <p class="flex flex-wrap items-center gap-x-2 border-t border-border/40 px-3 py-1 text-[9px] text-faint">
            <span>{{ t("st.autoRealtime") }}</span>
            <span class="text-border-strong">/</span>
            <span>{{ t("st.autoEvents") }} {{ autoSwitch?.totalEvents ?? 0 }}</span>
            <span v-if="autoSwitch?.lastSwitchAt" class="text-border-strong">/</span>
            <span v-if="autoSwitch?.lastSwitchAt">
              {{ t("st.autoLastSwitch") }} {{ new Date(autoSwitch.lastSwitchAt).toLocaleTimeString() }}
            </span>
          </p>
          <p v-if="autoCheck" class="border-t border-border px-3 py-1.5 text-[9px] leading-4 text-muted-foreground">
            {{ t("st.autoCheckResult") }} · {{ autoCheck.decision.action }} · {{ autoCheck.decision.reason }}
          </p>
          <p v-if="lastDecisionText" class="truncate border-t border-border px-3 py-1.5 text-[9px] text-faint" :title="lastDecisionText">
            {{ t("st.autoLast") }} · {{ lastDecisionText }}
          </p>
        </section>

        <!-- 数据导出 -->
        <section class="border border-border bg-card lg:col-span-2">
          <div class="border-b border-border px-3 py-1.5">
            <span class="lbl">// {{ t("st.export") }}</span>
          </div>
          <div class="flex flex-wrap items-center justify-between gap-2 px-3 py-2.5">
            <span class="text-[9px] text-faint">{{ t("st.exportHint") }}</span>
            <div class="flex flex-wrap items-center gap-1.5">
              <button
                v-for="item in EXPORTS"
                :key="item.kind"
                type="button"
                class="flex items-center gap-1 border border-border-strong px-2 py-1 text-[10px] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground disabled:opacity-40"
                :disabled="exporting !== ''"
                @click="runExport(item.kind, item.file)"
              >
                <Loader2 v-if="exporting === item.kind" class="size-2.5 animate-spin" />
                <Download v-else class="size-2.5" />
                {{ t(item.labelKey) }}
              </button>
              <button
                type="button"
                class="flex items-center gap-1 border border-border-strong px-2 py-1 text-[10px] uppercase tracking-[0.1em] text-foreground transition-colors hover:border-hazard hover:text-hazard disabled:opacity-40"
                :disabled="exporting !== ''"
                @click="runExportAll()"
              >
                <Loader2 v-if="exporting === 'all'" class="size-2.5 animate-spin" />
                <FolderOpen v-else class="size-2.5" />
                {{ t("st.exAll") }}
              </button>
              <!-- 诊断包：出问题一键打包，报障不用来回问 -->
              <button
                type="button"
                class="flex items-center gap-1 border border-border-strong px-2 py-1 text-[10px] uppercase tracking-[0.1em] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground disabled:opacity-40"
                :disabled="exporting !== ''"
                @click="runDiagnostics()"
              >
                <Loader2 v-if="exporting === 'diag'" class="size-2.5 animate-spin" />
                <FileText v-else class="size-2.5" />
                {{ t("st.diag") }}
              </button>
            </div>
          </div>
          <p
            v-if="exportNote"
            class="readout break-all border-t border-border px-3 py-1.5 text-[9px]"
            :class="exportFailed ? 'text-hazard' : 'text-success'"
          >
            {{ exportNote }}
          </p>
        </section>

        <!-- 维护 -->
        <section class="border border-border bg-card lg:col-span-2">
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
    <Transition name="wb-modal">

      <div
        v-if="clearTarget"
        class="fixed inset-0 z-50 grid place-items-center bg-black/60 p-4"
        @click.self="clearTarget = null"
      >
        <div class="w-full max-w-sm border border-hazard bg-card wb-modal-card">
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
    </Transition>
  </Teleport>
  </div>
</template>
