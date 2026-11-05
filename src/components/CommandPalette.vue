<script setup lang="ts">
/**
 * Ctrl+K 命令面板：跳分区 / 切换账号 / 刷新 / 开设置。
 *
 * 只做「入口聚合」，不复制业务：动作直接调用已有 composable 的方法，
 * 不在这里写第二套切换或刷新逻辑。
 */
import { computed, nextTick, ref, watch } from "vue";

import { t } from "@/lib/i18n";
import type { NavKey } from "@/lib/nav";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{
  close: [];
  nav: [key: NavKey];
}>();

const keyword = ref("");
const cursor = ref(0);
const inputEl = ref<HTMLInputElement | null>(null);

interface Item {
  key: string;
  label: string;
  hint?: string;
  run: () => void;
}

const staticItems = computed<Item[]>(() => [
  { key: "nav:accounts", label: t("cm.openAccounts"), hint: "01", run: () => emit("nav", "accounts") },
  { key: "nav:tasks", label: t("cm.openTasks"), hint: "02", run: () => emit("nav", "tasks") },
  { key: "nav:credits", label: t("cm.openCredits"), hint: "03", run: () => emit("nav", "credits") },
  { key: "nav:tokens", label: t("cm.openTokens"), hint: "04", run: () => emit("nav", "tokens") },
  { key: "nav:logs", label: t("cm.openLogs"), hint: "05", run: () => emit("nav", "logs") },
  { key: "nav:settings", label: t("cm.openSettings"), hint: "06", run: () => emit("nav", "settings") },
]);

const items = computed<Item[]>(() => {
  const list: Item[] = [...staticItems.value];
  const q = keyword.value.trim().toLowerCase();
  if (!q) return list;
  return list.filter((item) => item.label.toLowerCase().includes(q));
});

watch(
  () => props.open,
  async (open) => {
    if (!open) return;
    keyword.value = "";
    cursor.value = 0;
    await nextTick();
    inputEl.value?.focus();
  },
);

function move(step: number) {
  if (items.value.length === 0) return;
  cursor.value = (cursor.value + step + items.value.length) % items.value.length;
}

function run(index = cursor.value) {
  const item = items.value[index];
  if (!item) return;
  item.run();
  emit("close");
}
</script>

<template>
  <Transition name="wb-modal">
    <div
      v-if="open"
      class="fixed inset-0 z-[60] flex items-start justify-center bg-background/70 p-4 pt-[12vh]"
      @click.self="emit('close')"
    >
      <div class="wb-modal-card w-full max-w-lg border border-border-strong bg-card">
        <div class="flex items-center gap-2 border-b border-border px-3 py-2">
          <span class="lbl text-faint">CTRL K</span>
          <input
            ref="inputEl"
            v-model="keyword"
            class="min-w-0 flex-1 bg-transparent text-[12px] text-foreground outline-none"
            :placeholder="t('cm.placeholder')"
            @keydown.down.prevent="move(1)"
            @keydown.up.prevent="move(-1)"
            @keydown.enter.prevent="run()"
            @keydown.esc.prevent="emit('close')"
          />
        </div>
        <ul class="max-h-[46vh] overflow-y-auto">
          <li v-for="(item, index) in items" :key="item.key">
            <button
              type="button"
              class="flex w-full items-center justify-between gap-3 px-3 py-1.5 text-left text-[11px]"
              :class="index === cursor ? 'bg-muted text-foreground' : 'text-muted-foreground hover:bg-muted/60'"
              @mouseenter="cursor = index"
              @click="run(index)"
            >
              <span class="min-w-0 truncate">{{ item.label }}</span>
              <span v-if="item.hint" class="readout shrink-0 text-[9px] text-faint">{{ item.hint }}</span>
            </button>
          </li>
          <li v-if="items.length === 0" class="px-3 py-6 text-center text-[11px] text-faint">
            {{ t("cm.empty") }}
          </li>
        </ul>
      </div>
    </div>
  </Transition>
</template>
