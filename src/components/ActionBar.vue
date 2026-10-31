<script setup lang="ts">
// 命令区：作用域 + 三个批量动作（签到 / 旅行 / 成长任务）+ 中断。
// 二次确认内联在这里（不引入弹窗）：目标 > 1 时首次点击变红框确认态，8 秒回退。
import { computed, onBeforeUnmount, ref, watch } from "vue";

import Button from "@/components/ui/Button.vue";
import { t } from "@/lib/i18n";
import { ACTIONS, type ActionKey } from "@/lib/task-meta";

const props = defineProps<{
  total: number;
  selectedCount: number;
  running: boolean;
  cancelling: boolean;
  busyAction: ActionKey | "single" | "refresh" | "";
}>();

const emit = defineEmits<{
  run: [action: ActionKey];
  cancel: [];
}>();

const scope = defineModel<"all" | "selected">({ default: "all" });

const CONFIRM_TTL_MS = 8000;
type MaybeAction = ActionKey | "";
const pending = ref<MaybeAction>("");
let timer: number | undefined;

const targetCount = computed(() =>
  scope.value === "all" ? props.total : props.selectedCount,
);
const needsConfirm = computed(() => targetCount.value > 1);

function clearPending() {
  pending.value = "";
  if (timer !== undefined) {
    window.clearTimeout(timer);
    timer = undefined;
  }
}

function onRun(action: ActionKey) {
  if (props.busyAction !== "") return;
  if (pending.value === action) {
    clearPending();
    emit("run", action);
    return;
  }
  if (!needsConfirm.value) {
    emit("run", action);
    return;
  }
  clearPending();
  pending.value = action;
  timer = window.setTimeout(clearPending, CONFIRM_TTL_MS);
}

watch([scope, () => props.running], clearPending);
onBeforeUnmount(clearPending);
</script>

<template>
  <div class="flex h-10 shrink-0 items-stretch border-t border-border bg-panel">
    <!-- 作用域：分段按钮，选中项反白 -->
    <div class="flex items-center gap-1 border-r border-border px-2">
      <button
        class="px-2 py-1 text-[10px] uppercase tracking-[0.1em]"
        :class="scope === 'all' ? 'bg-foreground text-background' : 'text-faint hover:text-foreground'"
        @click="scope = 'all'"
      >
        {{ t("ab.all") }} {{ props.total }}
      </button>
      <button
        class="px-2 py-1 text-[10px] uppercase tracking-[0.1em]"
        :class="scope === 'selected' ? 'bg-foreground text-background' : 'text-faint hover:text-foreground'"
        @click="scope = 'selected'"
      >
        {{ t("ab.sel") }} {{ props.selectedCount }}
      </button>
    </div>

    <!-- 批量动作 -->
    <div class="flex items-center gap-1.5 px-2">
      <Button
        v-for="action in ACTIONS"
        :key="action.key"
        size="sm"
        :variant="pending === action.key ? 'danger' : action.variant"
        :title="'hintKey' in action ? t(action.hintKey) : undefined"
        :disabled="
          props.busyAction !== '' ||
          (pending !== '' && pending !== action.key) ||
          (scope === 'selected' && props.selectedCount === 0)
        "
        @click="onRun(action.key)"
      >
        <template v-if="props.busyAction === action.key">{{ t("ab.running") }}</template>
        <template v-else-if="pending === action.key">{{ t("ab.confirm") }} {{ targetCount }}</template>
        <template v-else>{{ t(action.labelKey) }}</template>
      </Button>
    </div>

    <!-- 中断 -->
    <div class="ml-auto flex items-center gap-2 border-l border-border px-2">
      <span v-if="pending !== ''" class="lbl text-hazard">{{ t("ab.confirmHint") }}</span>
      <Button
        size="sm"
        variant="outline"
        :disabled="!props.running || props.cancelling"
        @click="emit('cancel')"
      >
        {{ props.cancelling ? t("ab.stopping") : t("ab.interrupt") }}
      </Button>
    </div>
  </div>
</template>
