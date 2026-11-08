<script setup lang="ts">
// 成长抽屉：徽章墙 + 盲盒额度 + 任务看板。
// 纯展示：数据与写操作都在 TasksView（AGENTS.md §0 分层），关闭/刷新/领取只用 emit 回抛。
import { computed, onBeforeUnmount, watch } from "vue";
import { CircleAlert, Inbox, Loader2, Lock } from "lucide-vue-next";

import Button from "@/components/ui/Button.vue";
import type { BuddyQuota, GrowthBadge, GrowthTask } from "@/lib/api";
import { t, type I18nKey } from "@/lib/i18n";
import { progressOf } from "@/lib/task-meta";

const props = withDefaults(
  defineProps<{
    open: boolean;
    badges: GrowthBadge[];
    quota: BuddyQuota | null;
    tasks: GrowthTask[];
    loading: boolean;
    error?: string;
    claiming?: boolean;
  }>(),
  { error: "", claiming: false },
);

const emit = defineEmits<{ close: []; refresh: []; claim: [] }>();

/** 后端可能带 locked 标记，但前端契约（GrowthTask）未声明：按可选字段安全读取。 */
const lockedOf = (task: GrowthTask) => (task as GrowthTask & { locked?: boolean }).locked === true;

const earnedCount = computed(() => props.badges.filter((badge) => badge.earned).length);
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

/** 四态看板：可领取 = 做完未领（completed），已领取 = claimed。 */
type GroupKey = "claimable" | "inProgress" | "notStarted" | "claimed";
const GROUP_DEFS: { key: GroupKey; labelKey: I18nKey }[] = [
  { key: "claimable", labelKey: "gr.claimable" },
  { key: "inProgress", labelKey: "gr.inProgress" },
  { key: "notStarted", labelKey: "gr.notStarted" },
  { key: "claimed", labelKey: "gr.claimed" },
];

function groupOf(task: GrowthTask): GroupKey {
  if (lockedOf(task)) return "notStarted";
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

/** 徽章图 404/失效时隐藏，不留破图空框。 */
function hideImage(event: Event) {
  const image = event.target as HTMLImageElement | null;
  if (image) image.style.display = "none";
}

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
            <!-- 错误：原文透出，便于排查接口 -->
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
                    :title="badge.badgeName || badge.title"
                  >
                    <div class="relative size-10 shrink-0">
                      <img
                        v-if="badge.url"
                        :src="badge.url"
                        :alt="badge.badgeName || badge.title"
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
                      <Lock
                        v-if="!badge.earned"
                        class="absolute -bottom-1 -right-1 size-3 bg-panel text-faint"
                      />
                    </div>
                    <span
                      class="w-full truncate text-center text-[10px]"
                      :class="badge.earned ? 'text-foreground' : 'text-faint'"
                    >
                      {{ badge.badgeName || badge.title || badge.code }}
                    </span>
                  </div>
                </div>
                <p v-else class="px-2 py-3 text-[10px] text-faint">{{ t("gr.empty") }}</p>
              </section>

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

                <p
                  v-if="props.quota && props.quota.ok !== true"
                  class="readout border-t border-border px-2 py-1.5 text-[10px] text-hazard"
                >
                  {{ t("gr.error") }}{{ props.quota.error ? ` · ${props.quota.error}` : "" }}
                </p>
              </section>

              <!-- c) 任务看板：四态分组，只有「可领取」组给领取按钮 -->
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

                <div
                  v-for="group in groups"
                  :key="group.key"
                  class="border-b border-border last:border-b-0"
                >
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
                      :disabled="props.claiming || props.loading"
                      @click="emit('claim')"
                    >
                      <Loader2 v-if="props.claiming" class="size-3 animate-spin" />
                      {{ t("gr.claim") }}
                    </Button>
                  </div>

                  <ul>
                    <li
                      v-for="task in group.items"
                      :key="task.task_code"
                      class="flex items-center gap-2 border-t border-border/60 px-2 py-1.5"
                      :class="lockedOf(task) ? 'opacity-50' : ''"
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
                        v-if="lockedOf(task)"
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
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
