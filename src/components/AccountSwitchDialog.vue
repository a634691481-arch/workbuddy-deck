<script setup lang="ts">
// 完全同步切换的确认弹框：警示结构 + 切换前安全检查 + 失败现场。
// 纯展示：目标/进度/失败原因/预检结果都由 AccountsView 传进来，确认与关闭只用 emit 回抛。
import { computed } from "vue";
import { CircleAlert, Loader2, ShieldCheck } from "lucide-vue-next";

import { useShell } from "@/composables/useShell";
import type { SwitchPreflight } from "@/lib/api";
import { t, type I18nKey } from "@/lib/i18n";
import { mask } from "@/lib/task-meta";

withDefaults(
  defineProps<{
    /** 非空即开：待切换目标（name 由本组件脱敏） */
    target: { label: string } | null;
    switching?: boolean;
    /** 切换进度文案（后端 switch-progress 事件） */
    progress?: string;
    /** 失败现场：留在弹框里，别跟着 toast 一起消失 */
    error?: {
      rolledBack: boolean;
      /** 认证已还原，但目标账号会话已被改写（还不了） */
      sessionsDirty: boolean;
      result: string;
      stage: string;
      identity: string;
      message: string;
    } | null;
    preflight?: SwitchPreflight | null;
    preflightLoading?: boolean;
  }>(),
  { switching: false, progress: "", error: null, preflight: null, preflightLoading: false },
);

const emit = defineEmits<{ close: []; confirm: [] }>();

const { go } = useShell();

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

function fmtBytes(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${bytes} B`;
}

/** 失败现场里的「查看运行日志」：跳日志分区，弹框交给视图关 */
function viewLogs() {
  go("logs");
  emit("close");
}
</script>

<template>
  <Teleport to="body">
    <Transition name="wb-modal">
      <div
        v-if="target"
        class="fixed inset-0 z-50 flex items-center justify-center bg-background/80 p-4"
        role="dialog"
        aria-modal="true"
        @click.self="emit('close')"
      >
        <div class="flex max-h-[85vh] w-full max-w-[480px] flex-col border border-border-strong bg-card wb-modal-card">
          <header class="flex shrink-0 items-start justify-between gap-3 border-b border-border px-3 py-2 wb-flat">
            <div class="min-w-0">
              <div class="text-[13px] text-foreground">{{ t("ac.switchTitle") }}「{{ mask(target.label) }}」</div>
              <div class="readout mt-0.5 text-[9px] text-faint">{{ t("ac.switchSubtitle") }}</div>
            </div>
            <button
              type="button"
              class="shrink-0 px-1 text-[11px] text-faint hover:text-foreground"
              :disabled="switching"
              @click="emit('close')"
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

            <!-- 切换前安全检查：源/目标会话数 + 将删除清单 + 备份 -->
            <div v-if="preflightLoading" class="readout mt-3 flex items-center gap-2 text-[10px] text-muted-foreground">
              <Loader2 class="size-3 animate-spin" />
              {{ t("pf.loading") }}
            </div>
            <div v-else-if="preflight?.ok" class="mt-3 border border-border">
              <div class="flex items-center gap-1.5 border-b border-border bg-muted px-3 py-1.5 text-[11px] text-foreground">
                <ShieldCheck class="size-3.5 shrink-0" />
                {{ t("pf.title") }}
              </div>
              <div class="flex gap-2 border-b border-border px-3 py-1.5 text-[10px]">
                <span class="w-14 shrink-0 text-faint">{{ t("pf.source") }}</span>
                <span class="readout min-w-0 flex-1 text-foreground">
                  {{ preflight.source?.label ? mask(preflight.source.label) : t("pf.sourceUnknown") }}
                  · {{ t("pf.sessionCount", { n: String(preflight.source?.sessionCount ?? 0) }) }}
                </span>
              </div>
              <div class="flex gap-2 border-b border-border px-3 py-1.5 text-[10px]">
                <span class="w-14 shrink-0 text-faint">{{ t("pf.target") }}</span>
                <span class="readout min-w-0 flex-1 text-foreground">
                  {{ t("pf.sessionCount", { n: String(preflight.target?.sessionCount ?? 0) }) }}
                  <span class="text-hazard">{{ t("pf.willReplace") }}</span>
                </span>
              </div>
              <!-- 将被删除的会话：只露前 5 条，全量在备份清单里 -->
              <div v-if="(preflight.target?.sessions?.length ?? 0) > 0" class="border-b border-border px-3 py-1.5">
                <p class="lbl mb-1 text-[8px] text-faint">{{ t("pf.deleteList") }}</p>
                <ul class="space-y-0.5">
                  <li
                    v-for="item in preflight.target?.sessions.slice(0, 5)"
                    :key="item.id"
                    class="readout truncate text-[10px] text-muted-foreground"
                    :title="item.title ?? item.id"
                  >
                    · {{ item.isPlayground ? t("pf.playground") : t("pf.space") }} {{ item.title || item.id }}
                  </li>
                  <li v-if="(preflight.target?.sessions?.length ?? 0) > 5" class="text-[9px] text-faint">
                    {{ t("pf.more", { n: String((preflight.target?.sessions?.length ?? 0) - 5) }) }}
                  </li>
                </ul>
              </div>
              <!-- 备份：skipped 时红色警示（不拦切换，但必须让用户知道没有备份） -->
              <div class="flex gap-2 px-3 py-1.5 text-[10px]">
                <span class="w-14 shrink-0 text-faint">{{ t("pf.backup") }}</span>
                <div
                  v-if="preflight.backup && !preflight.backup.skipped"
                  class="readout min-w-0 flex-1 break-all leading-4 text-muted-foreground"
                  :title="preflight.backup.path"
                >
                  {{ t("pf.backupOk", {
                    n: String(preflight.backup.fileCount ?? 0),
                    size: fmtBytes(preflight.backup.totalBytes ?? 0),
                  }) }}
                  <span class="block text-faint">sha256 {{ (preflight.backup.manifestSha256 ?? "").slice(0, 16) }}…</span>
                </div>
                <span v-else-if="preflight.backup?.reason === 'size'" class="min-w-0 flex-1 leading-4 text-hazard">
                  {{ t("pf.backupSkipped", { size: fmtBytes(preflight.backup.totalBytes ?? 0) }) }}
                </span>
                <span v-else class="min-w-0 flex-1 leading-4 text-hazard">{{ t("pf.backupFailed") }}</span>
              </div>
            </div>
            <p v-else-if="preflight" class="readout mt-3 text-[10px] text-hazard">
              {{ t("pf.failed") }}{{ preflight.error ? ` · ${preflight.error}` : "" }}
            </p>

            <!-- 失败现场：阶段 + 是否回滚 + 原始原因，全部留在弹框里 -->
            <div v-if="error" class="mt-3 border border-hazard">
              <div class="flex items-center gap-1.5 border-b border-hazard bg-hazard-soft px-3 py-2 text-[11px] text-hazard">
                <CircleAlert class="size-3.5 shrink-0" />
                {{
                  error.rolledBack
                    ? error.sessionsDirty
                      ? t("ac.switchRolledBackDirty")
                      : t("ac.switchRolledBack")
                    : t("ac.switchBlocked")
                }}
              </div>
              <div class="flex gap-2 border-b border-border px-3 py-1.5 text-[10px]">
                <span class="w-14 shrink-0 text-faint">{{ t("ac.switchStage") }}</span>
                <span class="readout min-w-0 flex-1 text-foreground">{{ stageLabel(error.stage) }}</span>
              </div>
              <div v-if="error.identity" class="flex gap-2 border-b border-border px-3 py-1.5 text-[10px]">
                <span class="w-14 shrink-0 text-faint">{{ t("ac.switchIdentity") }}</span>
                <span class="readout min-w-0 flex-1 text-foreground">{{ error.identity }}</span>
              </div>
              <div v-if="error.sessionsDirty" class="flex gap-2 border-b border-border px-3 py-1.5 text-[10px]">
                <span class="w-14 shrink-0 text-faint">{{ t("ac.switchSessions") }}</span>
                <span class="min-w-0 flex-1 leading-4 text-hazard">{{ t("ac.switchSessionsDirty") }}</span>
              </div>
              <p class="readout whitespace-pre-wrap break-words px-3 py-2 text-[10px] leading-4 text-muted-foreground">
                {{ error.message }}
              </p>
              <button
                type="button"
                class="w-full border-t border-border px-3 py-1.5 text-left text-[10px] uppercase tracking-[0.1em] text-faint transition-colors hover:text-foreground"
                @click="viewLogs"
              >
                {{ t("ac.switchViewLog") }}
              </button>
            </div>

            <p v-if="switching" class="readout mt-3 flex items-center gap-2 text-[10px] text-foreground">
              <Loader2 class="size-3 animate-spin" />
              {{ progress || t("ac.switching") }}
            </p>
            <p v-else class="readout mt-3 text-[10px] text-muted-foreground">{{ t("ac.switchFoot") }}</p>
          </div>

          <footer class="flex shrink-0 items-center justify-end gap-2 border-t border-border px-3 py-2">
            <button
              type="button"
              class="border border-border-strong px-3 py-1 text-[10px] uppercase tracking-[0.1em] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground disabled:opacity-40"
              :disabled="switching"
              @click="emit('close')"
            >
              {{ t("ac.close") }}
            </button>
            <button
              type="button"
              class="flex items-center gap-1.5 bg-hazard px-3 py-1 text-[10px] uppercase tracking-[0.1em] text-white transition-opacity hover:opacity-85 disabled:opacity-40"
              :disabled="switching"
              @click="emit('confirm')"
            >
              <Loader2 v-if="switching" class="size-3 animate-spin" />
              {{ switching ? t("ac.switching") : t("ac.switchGo") }}
            </button>
          </footer>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
