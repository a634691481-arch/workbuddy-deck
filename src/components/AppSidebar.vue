<script setup lang="ts">
/**
 * 左侧导航栏（Dusk）：
 * - 图标 + 句首大写标签，选中项靠「2px 强调条 + 卡片底」区分。
 * - 底部状态：运行中 / 就绪 + 账号数，各一行。
 */
import { ref } from "vue";
import { MorphIcon } from "morphicons/vue";

import AnimatedIcon from "@/components/ui/AnimatedIcon.vue";
import { t } from "@/lib/i18n";
import { accent, sidebar } from "@/lib/prefs";
import { NAV_ICONS, type NavKey } from "@/lib/nav";
import { useUpdate } from "@/composables/useUpdate";
import playIcon from "@/assets/lordicon/play.json?url";

defineProps<{
  current: NavKey;
  accountCount: number;
  running: boolean;
}>();

const emit = defineEmits<{ select: [key: NavKey] }>();

/** 有新版本时在「设置」项右上角点一个危险色方块：面板里不弹窗，只给这一处提示。 */
const { hasUpdate, phase } = useUpdate();

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
  <nav
    class="flex shrink-0 flex-col border-r border-border bg-panel wb-flat"
    :class="[
      sidebar === 'compact' ? 'w-[56px]' : 'w-[148px]',
      'max-[560px]:!w-[52px] max-[560px]:items-center',
    ]"
  >
    <div v-if="sidebar === 'full'" class="lbl px-3 pb-2 pt-3 text-faint">{{ t("nav.section") }}</div>
    <div v-else class="h-3" />

    <button
      v-for="item in items"
      :key="item.key"
      type="button"
      class="relative flex items-center border-l-2 py-2.5 text-left text-[12.5px] transition-colors"
      :class="[
        sidebar === 'compact' ? 'justify-center pl-0 pr-0' : 'gap-1.5 pl-3 pr-2 max-[560px]:justify-center max-[560px]:px-0',
        current === item.key
          ? 'border-l-hazard bg-card text-foreground'
          : 'border-l-transparent text-muted-foreground hover:bg-muted/60 hover:text-foreground',
      ]"
      :aria-current="current === item.key ? 'page' : undefined"
      :title="sidebar === 'compact' ? t(item.labelKey) : undefined"
      @click="select(item.key)"
    >
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
      <span v-if="sidebar === 'full'" class="truncate max-[560px]:hidden">{{ t(item.labelKey) }}</span>
      <span
        v-if="item.key === 'settings' && hasUpdate"
        class="absolute right-2 top-1.5 size-1.5 bg-hazard"
        :class="phase === 'ready' || phase === 'restarting' ? 'ring-1 ring-hazard ring-offset-1 ring-offset-panel' : ''"
        :title="phase === 'ready' || phase === 'restarting' ? t('nav.updateDotReady') : t('nav.updateDot')"
        aria-hidden="true"
      />
    </button>

    <div class="mt-auto border-t border-border max-[560px]:px-1 max-[560px]:py-2 max-[560px]:text-center" :class="sidebar === 'compact' ? 'px-1 py-2 text-center' : 'px-3 py-3'">
      <div class="flex items-center gap-1.5 max-[560px]:justify-center" :class="sidebar === 'full' ? '' : 'justify-center'">
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
        <span v-if="sidebar === 'full'" class="text-[11px] max-[560px]:hidden" :class="running ? 'text-hazard' : 'text-muted-foreground'">
          {{ running ? "运行中" : "就绪" }}
        </span>
      </div>
      <div v-if="sidebar === 'full'" class="mt-1 text-[11px] text-faint max-[560px]:hidden">
        {{ accountCount }} 个账号
      </div>
    </div>
  </nav>
</template>
