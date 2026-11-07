<script setup lang="ts">
// 分组管理弹框：新建 / 改名 / 排除自动切换 / 删除。
// 只接 props / emit：数据与写操作在 AccountsView（useAccountGroups），这里只管弹框内的临时输入。
import { onBeforeUnmount, ref, watch } from "vue";
import { Loader2, Pencil, Trash2 } from "lucide-vue-next";

import { useArmedConfirm } from "@/composables/useArmedConfirm";
import type { AccountGroupDef } from "@/lib/api";
import { t } from "@/lib/i18n";

const props = withDefaults(
  defineProps<{
    open: boolean;
    groups: AccountGroupDef[];
    /** 写操作进行中：输入与按钮全禁用，避免连点落两份改名 */
    busy?: boolean;
    /** 写失败原因由视图透传（组件不碰 api，也就没有自己的错误源） */
    error?: string;
    /** 分组 id → 成员数；缺失按 0 显示 */
    counts?: Record<string, number>;
  }>(),
  { busy: false, error: "", counts: () => ({}) },
);

const emit = defineEmits<{
  close: [];
  create: [name: string];
  rename: [id: string, name: string];
  remove: [id: string];
  toggleExclude: [id: string, value: boolean];
}>();

/** 新组名草稿 */
const newName = ref("");
/** 正在改名的分组 id 与草稿（空 = 没有在改名） */
const renamingId = ref("");
const renameDraft = ref("");
/** 删除按钮：第一次点进入确认态，8 秒内再点才真删 */
const { armedId: pendingRemove, arm, reset: resetArmed } = useArmedConfirm();

function submitCreate() {
  const name = newName.value.trim();
  if (!name || props.busy) return;
  emit("create", name);
}

function startRename(group: AccountGroupDef) {
  if (props.busy) return;
  renamingId.value = group.id;
  renameDraft.value = group.name;
}

function cancelRename() {
  renamingId.value = "";
  renameDraft.value = "";
}

function submitRename() {
  const id = renamingId.value;
  const name = renameDraft.value.trim();
  cancelRename();
  if (!id || !name) return;
  emit("rename", id, name);
}

function remove(groupId: string) {
  if (props.busy) return;
  if (arm(groupId)) return;
  emit("remove", groupId);
}

/** Esc：改名中先撤销改名，否则关闭弹框（与点遮罩 / 点 ✕ 等价） */
function onKeydown(event: KeyboardEvent) {
  if (event.key !== "Escape") return;
  if (renamingId.value) {
    cancelRename();
    return;
  }
  emit("close");
}

// 新建成功与否只能由 groups 说话：名字真出现在列表里才清输入框，
// 失败时草稿留着（重名很常见，别让用户重打一遍）
watch(
  () => props.groups,
  (list) => {
    const name = newName.value.trim();
    if (name && list.some((group) => group.name === name)) newName.value = "";
  },
);

// 打开期间才挂 Esc 监听；关闭时顺手清干净临时输入，下次打开是空白状态
watch(
  () => props.open,
  (open) => {
    if (typeof window !== "undefined") {
      if (open) window.addEventListener("keydown", onKeydown);
      else window.removeEventListener("keydown", onKeydown);
    }
    if (open) return;
    newName.value = "";
    cancelRename();
    resetArmed();
  },
  { immediate: true },
);

onBeforeUnmount(() => {
  if (typeof window !== "undefined") window.removeEventListener("keydown", onKeydown);
});
</script>

<template>
  <Teleport to="body">
    <Transition name="wb-modal">
      <div
        v-if="open"
        class="fixed inset-0 z-50 flex items-center justify-center bg-background/80 p-4"
        role="dialog"
        aria-modal="true"
        @click.self="emit('close')"
      >
        <div class="w-full max-w-[420px] border border-border-strong bg-card wb-modal-card">
          <header class="flex shrink-0 items-start justify-between gap-3 border-b border-border px-3 py-2 wb-flat">
            <div class="min-w-0">
              <div class="text-[13px] text-foreground">{{ t("ag.panelTitle") }}</div>
              <div class="mt-0.5 text-[10px] leading-4 text-muted-foreground">{{ t("ag.panelDesc") }}</div>
            </div>
            <button
              type="button"
              class="shrink-0 px-1 text-[11px] text-faint hover:text-foreground"
              @click="emit('close')"
            >
              ✕
            </button>
          </header>

          <div class="max-h-[50vh] overflow-y-auto px-3 py-2">
            <p v-if="error" class="mb-2 border border-hazard bg-hazard-soft px-2 py-1 text-[10px] text-hazard">
              {{ error }}
            </p>
            <p v-if="groups.length === 0" class="py-6 text-center text-[10px] text-faint">{{ t("ag.empty") }}</p>
            <div
              v-for="group in groups"
              :key="group.id"
              class="flex items-center gap-2 border-b border-border/60 py-1.5 last:border-b-0"
            >
              <!-- 改名：点组名就地变输入框，Enter 提交 / Esc 撤销 -->
              <template v-if="renamingId === group.id">
                <input
                  v-model="renameDraft"
                  class="readout min-w-0 flex-1 border border-border-strong bg-background px-1.5 py-0.5 text-[10px] text-foreground outline-none focus:border-hazard"
                  maxlength="16"
                  @keyup.enter="submitRename"
                />
                <button
                  type="button"
                  class="shrink-0 border border-border-strong px-1.5 py-0.5 text-[9px] text-foreground hover:border-foreground disabled:opacity-40"
                  :disabled="busy"
                  @click="submitRename"
                >
                  OK
                </button>
                <button type="button" class="shrink-0 px-1 text-[9px] text-faint hover:text-foreground" @click="cancelRename">
                  ✕
                </button>
              </template>
              <template v-else>
                <button
                  type="button"
                  class="group flex min-w-0 flex-1 items-baseline gap-1 truncate text-left text-[11px] text-foreground disabled:opacity-40"
                  :title="`#${group.name}`"
                  :disabled="busy"
                  @click="startRename(group)"
                >
                  <span class="min-w-0 truncate">#{{ group.name }}</span>
                  <span class="lbl text-[8px] text-faint">{{ counts[group.id] ?? 0 }}</span>
                  <Pencil class="size-2.5 shrink-0 text-faint opacity-0 transition-opacity group-hover:opacity-100" />
                </button>
                <label
                  class="flex shrink-0 cursor-pointer items-center gap-1 text-[9px] text-muted-foreground hover:text-foreground"
                  :title="t('ag.excludeHint')"
                >
                  <input
                    type="checkbox"
                    class="accent-hazard"
                    :checked="group.excludeAutoSwitch"
                    :disabled="busy"
                    @change="emit('toggleExclude', group.id, !group.excludeAutoSwitch)"
                  />
                  {{ t("ag.exclude") }}
                </label>
                <button
                  type="button"
                  class="shrink-0 border px-1.5 py-0.5 text-[9px] transition-colors"
                  :class="
                    pendingRemove === group.id
                      ? 'border-hazard bg-hazard-soft text-hazard'
                      : 'border-border-strong text-muted-foreground hover:border-hazard hover:text-hazard'
                  "
                  @click="remove(group.id)"
                >
                  <template v-if="pendingRemove === group.id">{{ t("ac.confirm") }}</template>
                  <template v-else><Trash2 class="size-2.5" /></template>
                </button>
              </template>
            </div>
          </div>

          <footer class="flex items-center gap-1.5 border-t border-border px-3 py-2 wb-flat">
            <input
              v-model="newName"
              class="readout min-w-0 flex-1 border border-border-strong bg-background px-1.5 py-1 text-[10px] text-foreground outline-none focus:border-hazard"
              :placeholder="t('ag.newPh')"
              maxlength="16"
              @keyup.enter="submitCreate"
            />
            <button
              type="button"
              class="flex items-center gap-1 border border-foreground bg-foreground px-2 py-1 text-[10px] uppercase tracking-[0.1em] text-background transition-opacity hover:opacity-85 disabled:opacity-40"
              :disabled="busy || !newName.trim()"
              @click="submitCreate"
            >
              <Loader2 v-if="busy" class="size-3 animate-spin" />
              {{ t("ag.add") }}
            </button>
          </footer>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
