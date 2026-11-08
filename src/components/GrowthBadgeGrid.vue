<script setup lang="ts">
// 徽章墙：只读展示。未获得的徽章用 gr.locked 做可见角标 + title/aria 文案。
import { computed } from "vue";
import { Lock } from "lucide-vue-next";

import type { GrowthBadge } from "@/lib/api";
import { t } from "@/lib/i18n";

const props = defineProps<{ badges: GrowthBadge[] }>();

const earnedCount = computed(() => props.badges.filter((badge) => badge.earned).length);

/** 徽章图 404/失效时隐藏，不留破图空框。 */
function hideImage(event: Event) {
  const image = event.target as HTMLImageElement | null;
  if (image) image.style.display = "none";
}

function labelOf(badge: GrowthBadge) {
  return badge.badgeName || badge.title || badge.code;
}

function titleOf(badge: GrowthBadge) {
  return badge.earned ? labelOf(badge) : `${labelOf(badge)} · ${t("gr.locked")}`;
}
</script>

<template>
  <section class="border border-border bg-panel">
    <header class="flex items-center gap-1.5 border-b border-border px-2 py-1.5">
      <span class="lbl text-hazard">[</span>
      <span class="lbl lbl-ink">{{ t("gr.badges") }}</span>
      <span class="lbl text-hazard">]</span>
      <span class="readout ml-auto text-[10px] text-faint">
        {{ t("gr.earned") }} {{ earnedCount }}/{{ props.badges.length }}
      </span>
    </header>

    <div
      v-if="props.badges.length > 0"
      class="grid grid-cols-2 gap-2 p-2 md:grid-cols-3 xl:grid-cols-4"
    >
      <div
        v-for="badge in props.badges"
        :key="badge.code"
        class="flex min-w-0 flex-col items-center gap-1 border border-border px-1.5 py-2"
        :class="badge.earned ? 'bg-card' : 'grayscale opacity-50'"
        :title="titleOf(badge)"
        :aria-label="titleOf(badge)"
      >
        <div class="relative size-10 shrink-0">
          <img
            v-if="badge.url"
            :src="badge.url"
            :alt="labelOf(badge)"
            loading="lazy"
            class="size-10 object-contain"
            @error="hideImage"
          />
          <span
            v-else
            class="flex size-10 items-center justify-center border border-dashed border-border text-[12px] text-faint"
          >
            ?
          </span>
          <Lock v-if="!badge.earned" class="absolute -bottom-1 -right-1 size-3 bg-panel text-faint" />
        </div>
        <span
          class="w-full truncate text-center text-[10px]"
          :class="badge.earned ? 'text-foreground' : 'text-faint'"
        >
          {{ labelOf(badge) }}
        </span>
        <span v-if="!badge.earned" class="readout text-[8px] leading-none text-faint">
          {{ t("gr.locked") }}
        </span>
      </div>
    </div>
    <p v-else class="px-2 py-3 text-[10px] text-faint">{{ t("gr.empty") }}</p>
  </section>
</template>
