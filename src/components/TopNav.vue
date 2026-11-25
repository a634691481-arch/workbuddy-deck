<script setup lang="ts">
/**
 * 顶部导航：面板是 400px 宽的弹层，左侧栏会把内容压到 300px 以下，
 * 所以导航横排——图标在上、短标签在下，选中项用底部 2px 强调条。
 */
import { MorphIcon } from "morphicons/vue";

import { t } from "@/lib/i18n";
import { NAV_ICONS, type NavKey } from "@/lib/nav";

defineProps<{ current: NavKey }>();
const emit = defineEmits<{ select: [key: NavKey] }>();

/** 点击后把该项滚到可视区中间：固定宽度 + 横向滚动时，远处的项点完要回中。 */
function pick(key: NavKey, event: MouseEvent) {
  emit("select", key);
  const el = event.currentTarget as HTMLElement | null;
  el?.scrollIntoView({ inline: "center", block: "nearest", behavior: "smooth" });
}

type LabelKey =
  | "navS.tasks"
  | "navS.credits"
  | "navS.tokens"
  | "navS.accounts"
  | "navS.logs"
  | "navS.archived"
  | "navS.appearance"
  | "navS.settings";

const items: { key: NavKey; labelKey: LabelKey }[] = [
  { key: "accounts", labelKey: "navS.accounts" },
  { key: "tasks", labelKey: "navS.tasks" },
  { key: "credits", labelKey: "navS.credits" },
  { key: "tokens", labelKey: "navS.tokens" },
  { key: "logs", labelKey: "navS.logs" },
  { key: "archived", labelKey: "navS.archived" },
  { key: "appearance", labelKey: "navS.appearance" },
  { key: "settings", labelKey: "navS.settings" },
];
</script>

<template>
  <nav class="wb-nav flex shrink-0 items-stretch overflow-x-auto scroll-smooth border-b border-border bg-panel wb-flat">
    <button
      v-for="item in items"
      :key="item.key"
      type="button"
      class="relative flex w-[52px] shrink-0 flex-col items-center justify-center gap-1 border-b-2 px-0.5 pb-1.5 pt-2 transition-colors"
      :class="
        current === item.key
          ? 'border-b-hazard text-foreground'
          : 'border-b-transparent text-muted-foreground hover:text-foreground'
      "
      :aria-current="current === item.key ? 'page' : undefined"
      @click="pick(item.key, $event)"
    >
      <MorphIcon
        :icon="current === item.key ? NAV_ICONS[item.key].active : NAV_ICONS[item.key].idle"
        :size="15"
        :stroke-width="1.75"
        spring="snappy"
        reduced-motion="user"
        color="currentColor"
      />
      <span class="max-w-full truncate text-[10.5px] leading-none">{{ t(item.labelKey) }}</span>
    </button>
  </nav>
</template>
