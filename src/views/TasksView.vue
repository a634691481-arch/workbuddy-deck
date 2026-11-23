<script setup lang="ts">
/**
 * 任务面板：原 App.vue 的六段布局整体搬入这里，成为左侧导航的第一个分区。
 * 状态与副作用仍全部在 composables（AGENTS.md §0 分层）。
 */
import { computed, onBeforeUnmount, onMounted, watch } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import AccountList from "@/components/AccountList.vue";
import ActionBar from "@/components/ActionBar.vue";
import AppToolbar from "@/components/AppToolbar.vue";
import StatusBar from "@/components/StatusBar.vue";
import TaskTable from "@/components/TaskTable.vue";
import { useAssistant } from "@/composables/useAssistant";
import { useBalance } from "@/composables/useBalance";
import { useAccountGroups } from "@/composables/useAccountGroups";
import { isTauri } from "@/lib/tauri-env";
import { useShell } from "@/composables/useShell";

const {
  accounts,
  selected,
  activeId,
  detail,
  detailLoading,
  detailError,
  logs,
  lastRefresh,
  tab,
  sidebarWidth,
  goComplete,
  etaText,
  cancelling,
  busyAction,
  run,
  activeAccount,
  runLabel,
  selectAccount,
  refreshAll,
  toggleSelect,
  selectAll,
  invert,
  clearSelection,
  resizeSidebar,
  runAction,
  cancel,
  pushLog,
} = useAssistant();

// 分组速选：只读分组数据，编辑入口在账号管理页
const { groups, load: loadGroups, groupMemberIds } = useAccountGroups();
const groupChips = computed(() =>
  groups.value.map((group) => ({
    id: group.id,
    name: group.name,
    ids: (groupMemberIds.value.get(group.id) ?? []).filter((id) =>
      accounts.value.some((item) => item.id === id),
    ),
  })),
);
/** 按组勾选：勾上即生效——有勾选时动作只跑勾选的账号（作用域自动判定） */
function selectGroupIds(ids: string[]) {
  selected.value = new Set(ids);
}

const { load: loadBalance, loadTotal } =
  useBalance();

watch(activeId, (id) => void loadBalance(id), { immediate: true });
watch(() => accounts.value.length, () => void loadTotal(), { immediate: true });

let unlisteners: UnlistenFn[] = [];
onMounted(async () => {
  if (!isTauri()) return;
  void loadGroups().catch(() => {});
  unlisteners.push(await listen("tray://refresh", () => void refreshAll()));
  unlisteners.push(
    await listen<string>("tray://done", (event) => pushLog(`托盘任务「${event.payload}」已完成`, "ok")),
  );
  unlisteners.push(
    await listen("auto://daily-done", () => {
      pushLog("每日自动执行「全部日常」已完成", "ok");
      void refreshAll();
    }),
  );
});
onBeforeUnmount(() => {
  for (const off of unlisteners) off();
  unlisteners = [];
});

const tasks = computed(() => detail.value?.growth.tasks ?? []);

// 外壳侧栏：账号数与运行状态由本页实时回写（useShell 是模块级共享 ref）。
const { setAccountCount, setRunning } = useShell();
watch(
  () => accounts.value.length,
  (value) => setAccountCount(value),
  { immediate: true },
);
watch(
  () => run.value.running,
  (value) => setRunning(value),
  { immediate: true },
);
// 跑完一轮（签到/成长任务会加积分）自动重查合计积分：
// 之前只在账号数量变化时刷新，签到完成后合计停在旧值。
watch(
  () => run.value.running,
  (now, was) => {
    if (was && !now) void loadTotal();
  },
);

</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <AppToolbar
      :running="run.running"
      :done="run.done"
      :run-total="run.total"
      :eta="etaText"
      :current="run.current"
      :action-label="runLabel"
      :busy-action="busyAction"
      @refresh="refreshAll"
    />

    <div class="flex min-h-0 flex-1 gap-3 p-3 max-[760px]:flex-col max-[760px]:gap-2 max-[760px]:p-2">
      <AccountList
        :accounts="accounts"
        :selected="selected"
        :active-id="activeId"
        :running-label="run.running ? run.current : ''"
        :width="sidebarWidth"
        :group-chips="groupChips"
        @select="selectAccount"
        @toggle="toggleSelect"
        @select-all="selectAll"
        @invert="invert"
        @clear="clearSelection"
        @select-ids="selectGroupIds"
        @resize="resizeSidebar"
      />

      <!-- min-w-0：允许整列收缩到窗口剩余宽度，内容不把面板撑出屏幕 -->
      <main class="flex min-h-0 min-w-0 flex-1 flex-col gap-3">
        <div class="flex min-h-0 flex-1 flex-col gap-3">

        <TaskTable
          v-model="tab"
          :tasks="tasks"
          :loading="detailLoading"
          :error="detailError"
          :checkin="detail?.checkin ?? null"
          :has-account="!!activeAccount"
          :running="run.running"
          :running-label="run.current"
          @jump="goComplete"
        />

        <ActionBar
          :total="accounts.length"
          :selected-count="selected.size"
          :running="run.running"
          :cancelling="cancelling"
          :busy-action="busyAction"
          @run="runAction"
          @cancel="cancel"
        />

        </div>
      </main>
    </div>


    <StatusBar
      :total="accounts.length"
      :last-refresh="lastRefresh"
      :log-count="logs.length"
      :running="run.running"
    />

  </div>
</template>
