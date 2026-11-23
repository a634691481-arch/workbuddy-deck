<script setup lang="ts">
// 完全同步切换的确认弹框：警示结构 + 切换前安全检查 + 失败现场。
// 纯展示：目标/进度/失败原因/预检结果都由 AccountsView 传进来，确认与关闭只用 emit 回抛。
import { computed } from "vue";
import { CircleAlert, Loader2 } from "lucide-vue-next";

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
            <div class="border border-border bg-muted/40 px-3 py-2.5">
              <div class="flex items-center gap-1.5 text-[11px] text-foreground">
                <CircleAlert class="size-3.5 shrink-0" />
                {{ t("ac.switchWarn") }}
              </div>
              <ul class="mt-1.5 list-disc space-y-1 pl-5 text-[10px] leading-4 text-muted-foreground">
                <li v-for="(bullet, bulletIndex) in switchBullets" :key="bulletIndex">{{ bullet }}</li>
              </ul>
            </div>

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
                class="w-full border-t border-border px-3 py-1.5 text-left text-[10px] text-faint transition-colors hover:text-foreground"
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
              class="border border-border-strong px-3 py-1 text-[10px] text-muted-foreground transition-colors hover:border-foreground hover:text-foreground disabled:opacity-40"
              :disabled="switching"
              @click="emit('close')"
            >
              {{ t("ac.close") }}
            </button>
            <button
              type="button"
              class="flex items-center gap-1.5 bg-hazard px-3 py-1 text-[10px] text-white transition-opacity hover:opacity-85 disabled:opacity-40"
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
