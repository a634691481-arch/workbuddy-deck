<script setup lang="ts">
// 成长抽屉：只做布局（外壳 + 头部 + 盲盒额度卡），徽章墙与任务看板拆成子组件。
// 纯展示：数据与写操作在 useGrowthCenter / TasksView（AGENTS.md §0），只用 emit 回抛。
import { computed, onBeforeUnmount, watch } from "vue";
import { CircleAlert, Inbox, Loader2 } from "lucide-vue-next";

import GrowthBadgeGrid from "@/components/GrowthBadgeGrid.vue";
import GrowthTaskBoard from "@/components/GrowthTaskBoard.vue";
import Button from "@/components/ui/Button.vue";
import type { BuddyQuota, GrowthBadge, GrowthTask } from "@/lib/api";
import { t } from "@/lib/i18n";

const props = withDefaults(
  defineProps<{
    open: boolean;
    badges: GrowthBadge[];
    quota: BuddyQuota | null;
    tasks: GrowthTask[];
    loading: boolean;
    error?: string;
    claiming?: boolean;
    /** 领取完成后的结果行（可领取数前后对比） */
    note?: string;
    /** 批量任务运行中：领取按钮不可点，避免点了没反应。 */
    busy?: boolean;
  }>(),
  { error: "", claiming: false, busy: false, note: "" },
);

const emit = defineEmits<{ close: []; refresh: []; claim: [] }>();

const affordable = computed(() => props.quota?.affordable ?? 0);
const showLoading = computed(
  () =>
    props.loading &&
    props.badges.length === 0 &&
    !props.quota &&
    props.tasks.length === 0,
);
const empty = computed(
  () => !props.loading && !props.error && props.badges.length === 0 && !props.quota && props.tasks.length === 0,
);

/** Esc 关闭（与点遮罩 / 点 ✕ 等价） */
function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") emit("close");
}

watch(
  () => props.open,
  (open) => {
    if (typeof window === "undefined") return;
    if (open) window.addEventListener("keydown", onKeydown);
    else window.removeEventListener("keydown", onKeydown);
  },
);
onBeforeUnmount(() => {
  if (typeof window !== "undefined") window.removeEventListener("keydown", onKeydown);
});
</script>

<template>
  <Teleport to="body">
    <Transition name="wb-modal">
      <div
        v-if="props.open"
        class="fixed inset-0 z-50 flex justify-end bg-background/80"
        role="dialog"
        aria-modal="true"
        @click.self="emit('close')"
      >
        <div class="flex h-full w-[440px] max-w-[92vw] flex-col border-l border-border-strong bg-card wb-modal-card">
          <header class="flex shrink-0 items-center gap-1.5 border-b border-border bg-panel px-3 py-2 wb-flat">
            <span class="lbl text-hazard">[</span>
            <span class="lbl lbl-ink">{{ t("gr.title") }}</span>
            <span class="lbl text-hazard">]</span>
            <span v-if="props.loading" class="ml-1 flex items-center gap-1 text-faint">
              <Loader2 class="size-3 animate-spin" />
              <span class="lbl">{{ t("gr.loading") }}</span>
            </span>
            <div class="ml-auto flex items-center gap-2">
              <Button variant="outline" size="sm" :disabled="props.loading" @click="emit('refresh')">
                {{ t("gr.refresh") }}
              </Button>
              <button
                type="button"
                class="px-1 text-[11px] text-faint hover:text-foreground"
                @click="emit('close')"
              >
                ✕
              </button>
            </div>
          </header>

          <div class="min-h-0 flex-1 space-y-3 overflow-y-auto px-3 py-3">
            <!-- 错误：只按 error prop 判断，原文透出便于排查接口 -->
            <div v-if="props.error" class="border border-hazard bg-hazard-soft px-3 py-2">
              <div class="flex items-center gap-1.5 text-[11px] text-hazard">
                <CircleAlert class="size-3.5 shrink-0" />
                {{ t("gr.error") }}
              </div>
              <p class="readout mt-1 break-words text-[10px] leading-4 text-muted-foreground">
                {{ props.error }}
              </p>
            </div>

            <!-- 首次加载 -->
            <div
              v-if="showLoading"
              class="flex items-center gap-2 border border-border bg-panel px-3 py-8 text-[10px] text-muted-foreground"
            >
              <Loader2 class="size-3.5 animate-spin" />
              {{ t("gr.loading") }}
            </div>

            <!-- 空态 -->
            <div
              v-else-if="empty"
              class="flex flex-col items-start gap-2 border border-border bg-panel px-3 py-10"
            >
              <Inbox class="size-5 text-faint" />
              <p class="lbl lbl-ink">{{ t("gr.empty") }}</p>
            </div>

            <template v-else>
              <!-- a) 徽章墙 -->
              <GrowthBadgeGrid :badges="props.badges" />

              <!-- b) 盲盒额度 -->
              <section class="border border-border bg-panel">
                <header class="flex items-center gap-1.5 border-b border-border px-2 py-1.5">
                  <span class="lbl text-hazard">[</span>
                  <span class="lbl lbl-ink">{{ t("gr.buddy") }}</span>
                  <span class="lbl text-hazard">]</span>
                </header>

                <div class="grid grid-cols-2 gap-px bg-border">
                  <div class="bg-card px-2 py-2">
                    <div class="lbl">{{ t("gr.energy") }}</div>
                    <div class="readout mt-1 truncate text-[15px] leading-none text-foreground">
                      {{ props.quota ? props.quota.balance : "—" }}
                    </div>
                  </div>
                  <div class="bg-card px-2 py-2">
                    <div class="lbl">{{ t("gr.perOpen") }}</div>
                    <div class="readout mt-1 truncate text-[15px] leading-none text-foreground">
                      {{ props.quota ? props.quota.costPerOpen : "—" }}
                    </div>
                  </div>
                  <div class="bg-card px-2 py-2">
                    <div class="lbl">{{ t("gr.affordable") }}</div>
                    <div
                      class="readout mt-1 truncate text-[15px] leading-none"
                      :class="affordable > 0 ? 'text-hazard' : 'text-foreground'"
                    >
                      {{ props.quota ? affordable : "—" }}
                    </div>
                  </div>
                  <div class="bg-card px-2 py-2">
                    <div class="lbl">{{ t("gr.maxOpen") }}</div>
                    <div class="readout mt-1 truncate text-[15px] leading-none text-foreground">
                      {{ props.quota ? props.quota.maxOpenCount : "—" }}
                    </div>
                  </div>
                </div>
              </section>

              <!-- c) 任务看板 -->
              <GrowthTaskBoard
                :tasks="props.tasks"
                :loading="props.loading"
                :claiming="props.claiming"
                :busy="props.busy"
                :note="props.note"
                @claim="emit('claim')"
              />
            </template>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
