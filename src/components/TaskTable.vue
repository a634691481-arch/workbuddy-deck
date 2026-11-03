<script setup lang="ts">
import { computed } from "vue";
import { Check, CircleAlert, CircleDot, Inbox, Loader, Minus } from "lucide-vue-next";
import Badge from "@/components/ui/Badge.vue";
import type { CheckinStatus, GrowthTask } from "@/lib/api";
import { t } from "@/lib/i18n";
import { useListAnim } from "@/composables/useListAnim";
import { isDone, progressOf, statusOf } from "@/lib/task-meta";

const props = defineProps<{
  tasks: GrowthTask[];
  loading: boolean;
  error: string;
  checkin: CheckinStatus | null;
  hasAccount: boolean;
  /** 批量执行中：页签栏右侧显示脉冲指示，跑的是哪个账号 */
  running?: boolean;
  runningLabel?: string;
}>();

const tab = defineModel<"tasks" | "checkin" | "raw">({ default: "tasks" });
const rowsEl = useListAnim(() => !props.running);

const tabs = [
  { key: "tasks", labelKey: "tt.growth" },
  { key: "checkin", labelKey: "tt.rawSignin" },
  { key: "raw", labelKey: "tt.rawTasks" },
] as const;

const rawText = computed(() => JSON.stringify(props.checkin?.raw ?? {}, null, 2));
const tasksText = computed(() => JSON.stringify(props.tasks, null, 2));

/** 列宽集中一处：名称自适应，数值列定宽右对齐才好纵向比较。 */
const GRID = "grid grid-cols-[minmax(0,1fr)_74px_62px_66px_30px] items-center gap-0 wb-cells";
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col border border-border bg-card">
    <!-- 页签：ASCII 风格，选中项底部压红线 -->
    <div class="flex h-8 shrink-0 items-stretch border-b border-border bg-panel" role="tablist">
      <button
        v-for="item in tabs"
        :key="item.key"
        role="tab"
        :aria-selected="tab === item.key"
        class="border-r border-border px-3 text-[10px] uppercase tracking-[0.1em]"
        :class="tab === item.key
          ? 'bg-card text-foreground shadow-[inset_0_-2px_0_0_var(--color-hazard)]'
          : 'text-faint hover:text-muted-foreground'"
        @click="tab = item.key"
      >
        {{ t(item.labelKey) }}
      </button>
      <!-- 执行中：一个脉冲方块 + 当前账号，替代整屏转圈 -->
      <span v-if="props.running" class="ml-auto flex items-center gap-1.5 pr-3">
        <i class="block size-1.5 animate-pulse bg-hazard" />
        <span class="lbl truncate text-hazard">
          {{ props.runningLabel || t("ab.running") }}
        </span>
      </span>
      <span
        v-if="tab === 'tasks' && tasks.length > 0"
        class="lbl readout ml-auto flex items-center pr-3 text-faint"
      >
        {{ t("tt.done") }} {{ tasks.filter(isDone).length }}/{{ tasks.length }}
      </span>
    </div>

    <!-- 任务表 -->
    <div v-show="tab === 'tasks'" ref="rowsEl" class="min-h-0 flex-1 overflow-y-auto">
      <!-- 表头：粘性黑底，列间竖线 -->
      <div
        :class="[GRID, 'sticky top-0 z-10 border-b border-border-strong bg-background text-[10px] uppercase tracking-[0.1em] text-muted-foreground']"
      >
        <span class="px-2 py-1.5">{{ t("tt.taskName") }}</span>
        <span class="border-l border-border px-2 py-1.5">{{ t("tt.status") }}</span>
        <span class="border-l border-border px-2 py-1.5 text-right">{{ t("tt.prog") }}</span>
        <span class="border-l border-border px-2 py-1.5 text-right">{{ t("tt.credit") }}</span>
        <span class="border-l border-border py-1.5 text-center">✓</span>
      </div>

      <!-- 加载骨架 -->
      <template v-if="loading">
        <div v-for="index in 5" :key="index" :class="[GRID, 'border-b border-border/60']">
          <div class="px-2 py-2.5"><div class="h-2.5 w-2/3 animate-pulse bg-muted" /></div>
          <div class="border-l border-border/60 px-2 py-2.5"><div class="h-2.5 w-12 animate-pulse bg-muted" /></div>
          <div class="border-l border-border/60 px-2 py-2.5"><div class="ml-auto h-2.5 w-8 animate-pulse bg-muted" /></div>
          <div class="border-l border-border/60 px-2 py-2.5"><div class="ml-auto h-2.5 w-8 animate-pulse bg-muted" /></div>
          <div class="border-l border-border/60 px-2 py-2.5"><div class="mx-auto size-2.5 animate-pulse bg-muted" /></div>
        </div>
      </template>

      <!-- 错误态 -->
      <div v-else-if="error" class="flex flex-col items-start gap-2 px-3 py-8">
        <div class="flex items-center gap-1.5 text-hazard">
          <CircleAlert class="size-3.5" /><span class="lbl text-hazard">{{ t("tt.readError") }}</span>
        </div>
        <p class="text-[11px] text-muted-foreground">{{ error }}</p>
      </div>

      <!-- 空态 -->
      <div v-else-if="tasks.length === 0" class="flex flex-col items-start gap-2 px-3 py-12">
        <Inbox class="size-5 text-faint" />
        <p class="lbl lbl-ink">{{ hasAccount ? t("tt.noTasks") : t("tt.noUnit") }}</p>
        <p class="text-[10px] text-faint">
          {{ hasAccount ? t("tt.noTasksHint") : t("tt.noUnitHint") }}
        </p>
      </div>

      <!-- 任务行：只在非加载/非错误/非空时渲染，切号加载期间不残留上个账号数据 -->
      <template v-else>
        <div
          v-for="task in tasks"
          :key="task.task_code"
          :class="[GRID, 'border-b border-border/60 text-[11px] hover:bg-muted/60']"
        >
          <div class="min-w-0 px-2 py-1">
            <div class="truncate text-foreground" :title="task.title">
              {{ task.title ?? task.task_code }}
            </div>
            <div v-if="task.task_desc" class="truncate text-[10px] text-faint" :title="task.task_desc">
              {{ task.task_desc }}
            </div>
          </div>
          <div class="flex items-center gap-1 border-l border-border/60 px-2 py-1">
            <!-- 状态与图标一一对应：看一眼形状就知道处于哪一步 -->
            <Check v-if="task.accept_status === 'claimed'" class="size-3 shrink-0 text-muted-foreground" />
            <CircleDot v-else-if="task.accept_status === 'completed'" class="size-3 shrink-0 text-foreground" />
            <Loader v-else-if="task.accept_status === 'accepted'" class="size-3 shrink-0 text-hazard" />
            <Minus v-else class="size-3 shrink-0 text-faint" />
            <Badge :tone="statusOf(task).tone">{{ statusOf(task).text }}</Badge>
          </div>
          <div class="readout border-l border-border/60 px-2 py-1 text-right text-muted-foreground">
            {{ progressOf(task) }}
          </div>
          <div class="readout border-l border-border/60 px-2 py-1.5 text-right text-muted-foreground">
            {{ task.reward_credit ?? 0 }}
          </div>
          <div class="border-l border-border/60 py-1 text-center">
            <Check v-if="isDone(task)" class="mx-auto size-3 text-foreground" />
            <span v-else class="text-faint">·</span>
          </div>
        </div>
      </template>
    </div>

    <!-- 原始响应：终端输出观感 -->
    <pre
      v-show="tab !== 'tasks'"
      class="m-0 min-h-0 flex-1 overflow-auto whitespace-pre-wrap break-all bg-background px-3 py-2 text-[10px] leading-4 text-muted-foreground"
      >{{ tab === "checkin" ? rawText : tasksText }}</pre
    >
  </div>
</template>
