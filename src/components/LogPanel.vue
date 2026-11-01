<script setup lang="ts">
// 底部控制台：默认展开（用户要求常驻可见），可折叠。
// 观感按终端：提示符 + 等宽时间戳 + 行前缀标记级别。
import { computed, ref } from "vue";
import { ChevronDown, ChevronRight, Trash2 } from "lucide-vue-next";
import { t } from "@/lib/i18n";
import { formatClock } from "@/lib/utils";

export interface LogLine {
  ts: number;
  text: string;
  level: "info" | "ok" | "err";
}

const props = defineProps<{ logs: LogLine[]; errorCount: number }>();
const emit = defineEmits<{ clear: [] }>();

/** 默认展开：日志是本工具的主要观测面，不该藏起来。 */
const collapsed = ref(false);
const VISIBLE_ROWS = 8;
const ROW_HEIGHT = 18;
const listStyle = computed(() => ({ height: `${VISIBLE_ROWS * ROW_HEIGHT}px` }));

const levelMark = (level: LogLine["level"]) =>
  level === "err" ? "✕" : level === "ok" ? "✓" : "·";
const levelClass = (level: LogLine["level"]) =>
  level === "err" ? "text-hazard" : "text-muted-foreground";
</script>

<template>
  <section class="shrink-0 border-t border-border bg-background">
    <div class="flex h-6 items-center gap-3 border-b border-border px-2">
      <button
        class="flex items-center gap-1"
        :aria-expanded="!collapsed"
        @click="collapsed = !collapsed"
      >
        <component :is="collapsed ? ChevronRight : ChevronDown" class="size-3 text-muted-foreground" />
        <span class="lbl text-hazard">›</span>
        <span class="lbl lbl-ink">{{ t("lg.log") }}</span>
        <span class="lbl readout text-faint">{{ String(props.logs.length).padStart(3, "0") }}</span>
      </button>
      <span v-if="props.errorCount > 0" class="lbl text-hazard">
        {{ t("lg.err") }} {{ String(props.errorCount).padStart(3, "0") }}
      </span>
      <span v-else class="lbl text-faint">{{ t("lg.err") }} 000</span>
      <button
        v-if="props.logs.length > 0"
        class="lbl ml-auto flex items-center gap-1 text-faint hover:text-foreground"
        @click="emit('clear')"
      >
        <Trash2 class="size-3" />{{ t("lg.clear") }}
      </button>
    </div>

    <div v-show="!collapsed" class="overflow-y-auto px-2 py-1" :style="listStyle">
      <p
        v-for="(line, index) in props.logs.slice(0, 80)"
        :key="index"
        class="flex gap-2 text-[10px] leading-[18px]"
      >
        <span class="readout shrink-0 text-faint">{{ formatClock(line.ts) }}</span>
        <span class="shrink-0" :class="levelClass(line.level)">{{ levelMark(line.level) }}</span>
        <span class="min-w-0 break-all" :class="levelClass(line.level)">{{ line.text }}</span>
      </p>
      <p v-if="props.logs.length === 0" class="py-1 text-[10px] text-faint">
        › {{ t("lg.empty") }}
      </p>
    </div>
  </section>
</template>
