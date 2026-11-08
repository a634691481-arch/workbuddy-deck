<script setup lang="ts">
// 任务看板：四态分组（可领取 / 进行中 / 未开始 / 已领取），只有「可领取」组给领取按钮。
// 纯展示 + 单次 emit；busy 为批量任务运行中，避免点了没反应。
import { computed } from "vue";
import { Loader2 } from "lucide-vue-next";

import Button from "@/components/ui/Button.vue";
import type { GrowthTask } from "@/lib/api";
import { t, type I18nKey } from "@/lib/i18n";
import { progressOf } from "@/lib/task-meta";

const props = withDefaults(
  defineProps<{
    tasks: GrowthTask[];
    loading?: boolean;
    claiming?: boolean;
    busy?: boolean;
    /** 上一轮执行结果：可领取数 from → to */
    note?: string;
  }>(),
  { loading: false, claiming: false, busy: false, note: "" },
);

const emit = defineEmits<{ claim: [] }>();

/** 四态看板：可领取 = 做完未领（completed），已领取 = claimed。 */
type GroupKey = "claimable" | "inProgress" | "notStarted" | "claimed";
const GROUP_DEFS: { key: GroupKey; labelKey: I18nKey }[] = [
  { key: "claimable", labelKey: "gr.claimable" },
  { key: "inProgress", labelKey: "gr.inProgress" },
  { key: "notStarted", labelKey: "gr.notStarted" },
  { key: "claimed", labelKey: "gr.claimed" },
];

function groupOf(task: GrowthTask): GroupKey {
  if (task.locked === true) return "notStarted";
  switch (task.accept_status) {
    case "completed":
      return "claimable";
    case "accepted":
      return "inProgress";
    case "claimed":
      return "claimed";
    default:
      return "notStarted";
  }
}

const groups = computed(() =>
  GROUP_DEFS.map((def) => ({
    ...def,
    items: props.tasks.filter((task) => groupOf(task) === def.key),
  })).filter((group) => group.items.length > 0),
);

/** 运行中/读取中出现「不可点」的明确文案，不靠 toast 解释。 */
const claimDisabled = computed(() => props.busy || props.claiming || props.loading);
const claimLabel = computed(() => (props.busy ? t("sb.busy") : t("gr.claim")));
const claimHint = computed(() => {
  if (props.busy || props.claiming) return `${t("gr.claim")} · ${t("sb.busy")}`;
  if (props.loading) return `${t("gr.claim")} · ${t("gr.loading")}`;
  return t("gr.claim");
});
</script>

<template>
  <section class="border border-border bg-panel">
    <header class="flex items-center gap-1.5 border-b border-border px-2 py-1.5">
      <span class="lbl text-hazard">[</span>
      <span class="lbl lbl-ink">{{ t("gr.tasks") }}</span>
      <span class="lbl text-hazard">]</span>
      <span class="readout ml-auto text-[10px] text-faint">
        {{ props.tasks.length }}
      </span>
    </header>

    <p v-if="groups.length === 0" class="px-2 py-3 text-[10px] text-faint">
      {{ t("gr.empty") }}
    </p>

    <p v-if="props.note" class="border-b border-border px-2 py-1 text-[9px] text-success">
      {{ props.note }}
    </p>

    <div v-for="group in groups" :key="group.key" class="border-b border-border last:border-b-0">
      <div class="flex items-center gap-1.5 bg-muted/40 px-2 py-1">
        <span
          class="lbl"
          :class="group.key === 'claimable' ? 'text-hazard' : 'text-muted-foreground'"
        >
          {{ t(group.labelKey) }}
        </span>
        <span class="readout text-[9px] text-faint">
          {{ String(group.items.length).padStart(2, "0") }}
        </span>
        <Button
          v-if="group.key === 'claimable'"
          variant="default"
          size="sm"
          class="ml-auto"
          :disabled="claimDisabled"
          :title="claimHint"
          @click="emit('claim')"
        >
          <Loader2 v-if="props.claiming" class="size-3 animate-spin" />
          {{ claimLabel }}
        </Button>
      </div>

      <ul>
        <li
          v-for="task in group.items"
          :key="task.task_code"
          class="flex items-center gap-2 border-t border-border/60 px-2 py-1.5"
          :class="task.locked === true ? 'opacity-50' : ''"
        >
          <div class="min-w-0 flex-1">
            <div
              class="truncate text-[11px]"
              :class="group.key === 'claimed' ? 'text-muted-foreground' : 'text-foreground'"
              :title="task.task_desc || task.title"
            >
              {{ task.title || task.task_code }}
            </div>
            <div class="readout text-[9px] text-faint">{{ progressOf(task) }}</div>
          </div>
          <span
            v-if="task.locked === true"
            class="shrink-0 border border-border px-1 text-[9px] text-faint"
          >
            {{ t("gr.lockedTask") }}
          </span>
          <span
            class="readout shrink-0 text-[10px]"
            :class="group.key === 'claimed' ? 'text-faint' : 'text-muted-foreground'"
          >
            +{{ task.reward_credit ?? 0 }}
          </span>
        </li>
      </ul>
    </div>
  </section>
</template>
