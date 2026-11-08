<script setup lang="ts">
// 静默更新的可见层：只在「已经开始下载安装」时出现，展示版本说明 + 进度。
// 纯展示组件：阶段、进度、说明都由 useUpdate 传入，关闭只 emit。后台下载流程不受影响。
import { computed } from "vue";
import { Loader2, X } from "lucide-vue-next";

import Button from "@/components/ui/Button.vue";
import type { UpdatePhase } from "@/composables/useUpdate";
import { t } from "@/lib/i18n";

const props = withDefaults(
  defineProps<{
    open: boolean;
    current?: string;
    latest?: string;
    notes?: string;
    percent?: number | null;
    downloaded?: number;
    total?: number | null;
    phase: UpdatePhase;
    error?: string;
  }>(),
  { current: "", latest: "", notes: "", percent: null, downloaded: 0, total: null, error: "" },
);

const emit = defineEmits<{ close: [] }>();

const failed = computed(() => props.phase === "failed");

const phaseLabel = computed(() => {
  switch (props.phase) {
    case "downloading":
      return t("up.dlgDownloading");
    case "installing":
      return t("up.dlgInstalling");
    case "restarting":
      return t("up.dlgRestarting");
    case "failed":
      return t("up.dlgFailed");
    default:
      return "";
  }
});

/** 字节数给「已下载 / 总量」用；总量未知时只显示已下载。 */
function fmt(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 MB";
  if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024).toFixed(0)} KB`;
}
</script>

<template>
  <Teleport to="body">
    <Transition name="wb-modal">
      <div
        v-if="props.open"
        class="fixed inset-0 z-[95] flex items-center justify-center bg-black/60 p-6"
        @click.self="failed && emit('close')"
      >
        <div
          class="wb-modal-card w-[420px] border border-border-strong bg-card"
          role="dialog"
          aria-modal="true"
          :aria-label="t('up.dlgTitle')"
        >
          <header class="flex items-center gap-2 border-b border-border px-3 py-2">
            <span class="lbl text-hazard">[</span>
            <span class="lbl lbl-ink">{{ t("up.dlgTitle") }}</span>
            <span class="lbl text-hazard">]</span>
            <span class="readout ml-auto text-[10px] text-faint">
              {{ props.current || "—" }} → {{ props.latest || "—" }}
            </span>
            <button
              v-if="failed"
              type="button"
              class="text-faint hover:text-foreground"
              :title="t('up.dlgClose')"
              @click="emit('close')"
            >
              <X class="size-3" />
            </button>
          </header>

          <div class="space-y-2.5 px-3 py-2.5">
            <div>
              <p class="lbl mb-1 text-faint">{{ t("up.dlgNotes") }}</p>
              <p
                class="max-h-36 overflow-y-auto whitespace-pre-wrap border border-border bg-panel px-2 py-1.5 text-[10px] leading-relaxed text-muted-foreground"
              >
                {{ props.notes || t("up.dlgNoNotes") }}
              </p>
            </div>

            <div>
              <div class="mb-1 flex items-center gap-2">
                <span class="lbl" :class="failed ? 'text-hazard' : 'text-muted-foreground'">
                  {{ phaseLabel }}
                </span>
                <span class="readout ml-auto text-[11px]" :class="failed ? 'text-hazard' : 'text-foreground'">
                  {{ props.percent === null ? "—" : `${props.percent}%` }}
                </span>
              </div>

              <div class="h-1.5 w-full overflow-hidden border border-border-strong bg-muted">
                <div
                  v-if="props.percent !== null"
                  class="h-full bg-hazard transition-[width] duration-200"
                  :style="{ width: `${props.percent}%` }"
                />
                <div v-else class="h-full w-1/3 animate-pulse bg-hazard" />
              </div>

              <p class="readout mt-1 text-[9px] text-faint">
                {{ fmt(props.downloaded) }}
                <template v-if="props.total"> / {{ fmt(props.total) }}</template>
              </p>
            </div>

            <p v-if="failed" class="break-all text-[9px] text-hazard">
              {{ props.error || t("up.dlgFailed") }}
            </p>

            <div class="flex items-center gap-2 border-t border-border pt-2">
              <Loader2 v-if="!failed && props.phase !== 'idle'" class="size-3 animate-spin text-hazard" />
              <span class="text-[9px] text-faint">
                {{ failed ? "" : t("up.dlgKeepOpen") }}
              </span>
              <Button v-if="failed" variant="outline" size="sm" class="ml-auto" @click="emit('close')">
                {{ t("up.dlgClose") }}
              </Button>
            </div>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
