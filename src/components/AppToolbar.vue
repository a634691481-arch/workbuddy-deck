<script setup lang="ts">
// 命令条：左侧运行状态，右侧只留「刷新」。
// 账号增删/导入导出统一收在账号管理页（唯一入口），这里不再重复放按钮。
import { Loader2, RefreshCw } from "lucide-vue-next";

import { t } from "@/lib/i18n";

const props = defineProps<{
  running: boolean;
  done: number;
  runTotal: number;
  eta?: string;
  current: string;
  actionLabel: string;
  busyAction: string;
}>();

const emit = defineEmits<{ refresh: [] }>();

const btnClass =
  "flex items-center gap-1.5 border-l border-border px-2.5 text-muted-foreground hover:bg-muted hover:text-foreground disabled:opacity-40";
</script>

<template>
  <header class="flex h-10 shrink-0 items-stretch border-b border-border bg-card wb-flat">
    <!-- 左：模块标识（ASCII 括号是工业风的语法装饰） -->
    <div class="flex items-center gap-2 border-r border-border px-3">
      <span class="lbl text-hazard">[</span>
      <span class="lbl lbl-ink tracking-[0.16em]">{{ t("tb.accountOps") }}</span>
      <span class="lbl text-hazard">]</span>
    </div>

    <!-- 中：运行进度；空闲时显示待命，不留空白 -->
    <div class="flex min-w-0 flex-1 items-center gap-2 px-3">
      <template v-if="props.running">
        <Loader2 class="size-3 shrink-0 animate-spin text-hazard" />
        <span class="lbl lbl-ink">{{ props.actionLabel }}</span>
        <span class="readout text-[11px] text-foreground">{{ props.done }}/{{ props.runTotal }}</span>
        <span v-if="props.eta" class="readout text-[9px] text-faint">{{ props.eta }}</span>
        <span v-if="props.current" class="lbl truncate text-faint">» {{ props.current }}</span>
        <div class="ml-2 h-2 flex-1 border border-border-strong">
          <div
            class="h-full bg-hazard"
            :style="{ width: `${props.runTotal ? (props.done / props.runTotal) * 100 : 0}%` }"
          />
        </div>
      </template>
      <template v-else>
        <span class="lbl text-faint">» {{ t("tb.standby") }}</span>
        <span class="lbl text-faint">/// {{ t("tb.awaiting") }}</span>
      </template>
    </div>

    <!-- 右：列表刷新（账号的新增/导入/导出在账号管理页） -->
    <div class="flex items-stretch">
      <button
        :class="btnClass"
        :disabled="props.busyAction !== ''"
        @click="emit('refresh')"
      >
        <RefreshCw :class="['size-3.5', props.busyAction === 'refresh' && 'animate-spin']" />
        <span class="lbl">{{ t("tb.refresh") }}</span>
      </button>
    </div>
  </header>
</template>
