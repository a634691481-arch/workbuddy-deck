<script setup lang="ts">
/**
 * 左侧导航栏（工业风 §0.1）：
 * - 编号 + 大写标签，选中项靠「3px 航空红竖条 + 提亮底色」区分，不用圆角不用阴影。
 * - 底部状态区：READY 磷光绿是全应用唯一允许的绿色位。
 */
import { ref } from "vue";
import { MorphIcon } from "morphicons/vue";

import AnimatedIcon from "@/components/ui/AnimatedIcon.vue";
import { t } from "@/lib/i18n";
import { accent } from "@/lib/prefs";
import { NAV_ICONS, type NavKey } from "@/lib/nav";
import playIcon from "@/assets/lordicon/play.json?url";

defineProps<{
  current: NavKey;
  accountCount: number;
  running: boolean;
}>();

const emit = defineEmits<{ select: [key: NavKey] }>();

/** 刚点过的分区：给图标一个 260ms 的弹一下，点同一项也能重放。 */
const popped = ref<NavKey | null>(null);
let popTimer: ReturnType<typeof setTimeout> | null = null;

function select(key: NavKey) {
  popped.value = null;
  if (popTimer) clearTimeout(popTimer);
  requestAnimationFrame(() => {
    popped.value = key;
    popTimer = setTimeout(() => (popped.value = null), 280);
  });
  emit("select", key);
}

type LabelKey =
  | "nav.tasks"
  | "nav.credits"
  | "nav.tokens"
  | "nav.accounts"
  | "nav.logs"
  | "nav.archived"
  | "nav.settings";

const items: { key: NavKey; idx: string; labelKey: LabelKey }[] = [
  { key: "accounts", idx: "01", labelKey: "nav.accounts" },
  { key: "tasks", idx: "02", labelKey: "nav.tasks" },
  { key: "credits", idx: "03", labelKey: "nav.credits" },
  { key: "tokens", idx: "04", labelKey: "nav.tokens" },
  { key: "logs", idx: "05", labelKey: "nav.logs" },
  { key: "archived", idx: "06", labelKey: "nav.archived" },
  { key: "settings", idx: "07", labelKey: "nav.settings" },
];
</script>

<template>
  <nav class="flex w-[148px] shrink-0 flex-col border-r border-border bg-panel wb-flat">
    <div class="lbl px-3 pb-2 pt-3 text-faint">{{ t("nav.section") }}</div>

    <button
      v-for="item in items"
      :key="item.key"
      type="button"
      class="relative flex items-center gap-1.5 border-l-[3px] py-2.5 pl-3 pr-2 text-left text-[11px] tracking-[0.08em] transition-colors"
      :class="
        current === item.key
          ? 'border-l-hazard bg-card text-foreground'
          : 'border-l-transparent text-muted-foreground hover:bg-muted/60 hover:text-foreground'
      "
      :aria-current="current === item.key ? 'page' : undefined"
      @click="select(item.key)"
    >
      <span class="readout text-[9px] text-faint">{{ item.idx }}</span>
      <!-- morphicons：未选中 → 选中 做弹簧形变；点击时再叠一层 pop -->
      <span
        class="block shrink-0"
        :class="popped === item.key ? 'wb-nav-pop' : ''"
        :style="{ color: current === item.key ? 'var(--color-hazard)' : 'currentColor' }"
      >
        <MorphIcon
          :icon="current === item.key ? NAV_ICONS[item.key].active : NAV_ICONS[item.key].idle"
          :size="14"
          :stroke-width="1.75"
          spring="snappy"
          reduced-motion="user"
          color="currentColor"
        />
      </span>
      <span class="truncate">{{ t(item.labelKey) }}</span>
    </button>

    <div class="mt-auto border-t border-border px-3 py-3">
      <div class="lbl text-faint">// STATUS</div>
      <div class="mt-1.5 flex items-center gap-1.5">
        <!-- Lordicon：跑任务时播一次循环动画，就绪时回到静态磷光绿点 -->
        <AnimatedIcon
          v-if="running"
          :src="playIcon"
          :size="12"
          trigger="loop"
          :primary="accent"
          :secondary="accent"
        />
        <span v-else class="size-1.5 shrink-0 bg-success" aria-hidden="true" />
        <span class="readout text-[10px]" :class="running ? 'text-hazard' : 'text-success'">
          {{ running ? "RUNNING" : "READY" }}
        </span>
      </div>
      <div class="readout mt-1 text-[10px] text-faint">
        ACCTS {{ String(accountCount).padStart(2, "0") }}
      </div>
    </div>
  </nav>
</template>
