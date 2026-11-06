<script setup lang="ts">
import { computed, ref } from "vue";
import { ArrowRight, Loader2, Search } from "lucide-vue-next";
import Button from "@/components/ui/Button.vue";
import { useShell } from "@/composables/useShell";
import type { AccountSummary } from "@/lib/api";
import { t } from "@/lib/i18n";
import { mask } from "@/lib/task-meta";
import { cn } from "@/lib/utils";

/** 分组速选：由任务页根据分组数据算好传进来，这里只展示与转发。 */
export interface GroupChip {
  id: string;
  name: string;
  ids: string[];
}

const props = defineProps<{
  accounts: AccountSummary[];
  selected: Set<string>;
  activeId: string | null;
  runningLabel: string;
  width: number;
  groupChips?: GroupChip[];
}>();

const emit = defineEmits<{
  select: [id: string];
  toggle: [id: string];
  selectAll: [];
  invert: [];
  clear: [];
  resize: [width: number];
  selectIds: [ids: string[]];
}>();

/** 账号增删只在账号管理页做，这里只负责「带路」。 */
const { go } = useShell();

const query = ref("");
const listRef = ref<HTMLElement | null>(null);

const filtered = computed(() => {
  const q = query.value.trim().toLowerCase();
  if (!q) return props.accounts;
  return props.accounts.filter((item) => item.label.toLowerCase().includes(q));
});

/** 分组速选：值复位到占位项，避免误导「当前停留在哪个组」。 */
function onGroupPick(event: Event) {
  const select = event.target as HTMLSelectElement;
  const id = select.value;
  select.value = "";
  if (!id) return;
  const chip = props.groupChips?.find((item) => item.id === id);
  if (chip && chip.ids.length > 0) emit("selectIds", chip.ids);
}

/** 键盘导航用：ArrowDown/Up 移动选中账号，Enter 打开，Space 勾选。 */
function onKeydown(event: KeyboardEvent) {  const index = filtered.value.findIndex((item) => item.id === props.activeId);
  if (event.key === "ArrowDown" || event.key === "ArrowUp") {
    event.preventDefault();
    if (filtered.value.length === 0) return;
    const step = event.key === "ArrowDown" ? 1 : -1;
    const next = (index + step + filtered.value.length) % filtered.value.length;
    emit("select", filtered.value[next].id);
    return;
  }
  if (!props.activeId) return;
  if (event.key === "Enter") {
    event.preventDefault();
    emit("select", props.activeId);
  } else if (event.key === " ") {
    event.preventDefault();
    emit("toggle", props.activeId);
  }
}

/** 左栏拖拽调宽，范围 240–380，宽度由父级持久化。 */
const dragging = ref(false);

function startDrag(event: MouseEvent) {
  dragging.value = true;
  const startX = event.clientX;
  const startWidth = props.width;

  const move = (moveEvent: MouseEvent) => {
    const next = Math.min(380, Math.max(240, startWidth + moveEvent.clientX - startX));
    emit("resize", next);
  };
  const up = () => {
    dragging.value = false;
    window.removeEventListener("mousemove", move);
    window.removeEventListener("mouseup", up);
  };
  window.addEventListener("mousemove", move);
  window.addEventListener("mouseup", up);
}

/** 单位编号：01 / 02 …，工业设备标识的做法 */
const unitNo = (index: number) => String(index + 1).padStart(2, "0");
</script>

<template>
  <aside
    class="relative flex shrink-0 flex-col border border-border bg-panel"
    :style="{ width: `${width}px` }"
  >
    <!-- 检索框：方形、聚焦描红 -->
    <div class="border-b border-border p-2">
      <div
        class="flex h-7 items-center gap-1.5 border border-border-strong bg-background px-2 focus-within:border-hazard"
      >
        <span class="lbl text-hazard">›</span>
        <Search class="size-3 shrink-0 text-faint" />
        <input
          v-model="query"
          class="h-full min-w-0 flex-1 bg-transparent font-mono text-[11px] text-foreground outline-none placeholder:text-faint"
          :placeholder="t('al.filter')"
          aria-label="搜索账号"
        />
      </div>
    </div>

    <!-- 列头 -->
    <div
      v-if="accounts.length > 0"
      class="flex h-5 shrink-0 items-center gap-2 border-b border-border px-2"
    >
      <span class="lbl w-5 text-faint">{{ t("al.id") }}</span>
      <span class="lbl flex-1">{{ t("al.account") }}</span>
      <span class="lbl">{{ t("al.state") }}</span>
    </div>

    <div
      ref="listRef"
      class="min-h-0 flex-1 overflow-y-auto"
      role="listbox"
      tabindex="0"
      aria-label="账号列表"
      @keydown="onKeydown"
    >
      <div
        v-for="(item, index) in filtered"
        :key="item.id"
        role="option"
        :aria-selected="item.id === activeId"
        class="flex cursor-pointer items-center gap-2 border-b border-border/60 px-2 py-[7px]"
        :class="cn(
          item.id === activeId
            ? 'bg-muted shadow-[inset_2px_0_0_0_var(--color-hazard)]'
            : 'hover:bg-muted/50',
        )"
        @click="emit('select', item.id)"
      >
        <span
          class="readout w-4 shrink-0 text-[10px]"
          :class="item.id === activeId ? 'text-hazard' : 'text-faint'"
        >{{ unitNo(index) }}</span>

        <input
          type="checkbox"
          class="size-3.5 shrink-0 cursor-pointer appearance-none border border-[#6a6a62] bg-[#161615] checked:border-hazard checked:bg-hazard"
          :checked="selected.has(item.id)"
          :aria-label="`勾选 ${mask(item.label)}`"
          @click.stop
          @change="emit('toggle', item.id)"
        />

        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-1">
            <span class="truncate text-[11px] text-foreground" :title="mask(item.label)">{{ mask(item.label) }}</span>
            <span
              v-if="item.variant === 'intl'"
              class="shrink-0 border border-border-strong px-1 text-[9px] uppercase leading-3 text-muted-foreground"
            >INTL</span>
          </div>
          <div v-if="item.note" class="truncate text-[10px] text-faint">{{ mask(item.note) }}</div>
        </div>

        <span
          v-if="runningLabel && item.label === runningLabel"
          class="shrink-0 text-hazard"
          title="正在执行"
        >
          <Loader2 class="size-3 animate-spin" />
        </span>
        <span v-else class="flex shrink-0 items-center gap-1" title="就绪">
          <i class="block size-1.5 bg-foreground/45" />
        </span>
      </div>

      <!-- 空态：给出下一步，而不是只说「没有」 -->
      <div v-if="filtered.length === 0" class="flex flex-col gap-2 px-3 py-8">
        <template v-if="accounts.length === 0">
          <p class="lbl lbl-ink">{{ t("al.empty") }}</p>
          <p class="text-[10px] leading-4 text-faint">
            {{ t("al.emptyHint") }}
          </p>
          <Button variant="outline" size="sm" class="mt-1 self-start" @click="go('accounts')">
            <ArrowRight class="size-3" />{{ t("al.goto") }}
          </Button>
        </template>
        <p v-else class="lbl text-faint">{{ t("al.noMatch") }} » {{ query }}</p>
      </div>
    </div>

    <!-- 底部：批量选择命令 -->
    <div class="flex h-7 shrink-0 items-center gap-2 border-t border-border px-2">
      <button class="lbl text-muted-foreground hover:text-hazard" @click="emit('selectAll')">[A] {{ t("al.selectAll") }}</button>
      <button class="lbl text-muted-foreground hover:text-hazard" @click="emit('invert')">[I] {{ t("al.invert") }}</button>
      <!-- 分组速选：选完即把作用域切到「所选账号」 -->
      <select
        v-if="groupChips && groupChips.length > 0"
        class="lbl max-w-24 border border-border-strong bg-input px-0.5 text-[9px] text-muted-foreground outline-none focus:border-hazard"
        @change="onGroupPick"
      >
        <option value="">{{ t("al.pickGroup") }}</option>
        <option v-for="chip in groupChips" :key="chip.id" :value="chip.id">
          #{{ chip.name }} ({{ chip.ids.length }})
        </option>
      </select>
      <span class="lbl readout ml-auto text-faint">{{ selected.size }}/{{ accounts.length }}</span>
      <button class="lbl text-faint hover:text-foreground" @click="emit('clear')" :title="t('al.clear')">[C]</button>
    </div>

    <!-- 拖拽手柄 -->
    <div
      class="absolute inset-y-0 -right-1 w-2 cursor-col-resize"
      role="separator"
      aria-label="调整账号栏宽度"
      @mousedown.prevent="startDrag"
    >
      <div class="h-full w-px" :class="dragging ? 'bg-hazard' : 'bg-border hover:bg-hazard/60'" />
    </div>
  </aside>
</template>
