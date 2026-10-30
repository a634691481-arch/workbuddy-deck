<script setup lang="ts">
/**
 * 任务面板：原 App.vue 的六段布局整体搬入这里，成为左侧导航的第一个分区。
 * 状态与副作用仍全部在 composables（AGENTS.md §0 分层）。
 */
import { computed, onBeforeUnmount, onMounted, watch } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { CircleAlert } from "lucide-vue-next";

import AccountHeader from "@/components/AccountHeader.vue";
import AccountList from "@/components/AccountList.vue";
import ActionBar from "@/components/ActionBar.vue";
import AppToolbar from "@/components/AppToolbar.vue";
import AutoDailyPanel from "@/components/AutoDailyPanel.vue";
import LogPanel from "@/components/LogPanel.vue";
import StatusBar from "@/components/StatusBar.vue";
import TaskTable from "@/components/TaskTable.vue";
import TelemetryBar from "@/components/TelemetryBar.vue";
import { useAssistant } from "@/composables/useAssistant";
import { useBalance } from "@/composables/useBalance";
import { t } from "@/lib/i18n";
import { isDone, mask } from "@/lib/task-meta";
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
  scope,
  lastRefresh,
  tab,
  sidebarWidth,
  failures,
  showFailures,
  cancelling,
  busyAction,
  run,
  busy,
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
  signinOnce,
  clearLogs,
  pushLog,
} = useAssistant();

const { balance, loading: balanceLoading, load: loadBalance, total, totalLoading, totalFailed, loadTotal } =
  useBalance();

watch(activeId, (id) => void loadBalance(id), { immediate: true });
watch(() => accounts.value.length, () => void loadTotal(), { immediate: true });

let unlisteners: UnlistenFn[] = [];
onMounted(async () => {
  if (!isTauri()) return;
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

const errorLogCount = computed(() => logs.value.filter((line) => line.level === "err").length);
const tasks = computed(() => detail.value?.growth.tasks ?? []);
const taskDone = computed(() => tasks.value.filter(isDone).length);
const signedToday = computed(() => detail.value?.checkin.todayCheckedIn === true);
const balanceValue = computed(() =>
  balance.value?.ok && typeof balance.value.remaining === "number" ? balance.value.remaining : null,
);

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
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <AppToolbar
      :running="run.running"
      :done="run.done"
      :run-total="run.total"
      :current="run.current"
      :action-label="runLabel"
      :busy-action="busyAction"
      @refresh="refreshAll"
    />

    <div class="flex min-h-0 flex-1">
      <AccountList
        :accounts="accounts"
        :selected="selected"
        :active-id="activeId"
        :running-label="run.running ? run.current : ''"
        :width="sidebarWidth"
        @select="selectAccount"
        @toggle="toggleSelect"
        @select-all="selectAll"
        @invert="invert"
        @clear="clearSelection"
        @resize="resizeSidebar"
      />

      <main class="flex min-h-0 flex-1 flex-col bg-card">
        <AccountHeader
          :detail="detail"
          :account-label="mask(activeAccount?.label ?? '')"
          :has-account="!!activeAccount"
          :signing="busyAction === 'single'"
          :disabled="busy"
          @signin="signinOnce"
        />

        <TelemetryBar
          :accounts="accounts.length"
          :signed="signedToday"
          :task-done="taskDone"
          :task-total="tasks.length"
          :total-credit="total"
          :total-loading="totalLoading"
          :total-failed="totalFailed"
          :balance="balanceValue"
          :balance-loading="balanceLoading"
          :run-ok="run.ok"
          :run-fail="run.fail"
          :run-skip="run.skip"
          :running="run.running"
        />

        <div v-if="failures.length > 0" class="shrink-0 border-b border-hazard bg-hazard-soft px-2 py-1">
          <button
            class="flex w-full items-center gap-1.5 text-[10px] uppercase tracking-[0.1em] text-hazard"
            @click="showFailures = !showFailures"
          >
            <CircleAlert class="size-3 shrink-0" />
            [ {{ t("fail.title") }} {{ String(failures.length).padStart(2, "0") }} ]
            <span class="text-hazard/70">
              {{ showFailures ? t("fail.collapse") : t("fail.expand") }}
            </span>
          </button>
          <ul v-if="showFailures" class="mt-1 max-h-24 space-y-0.5 overflow-y-auto pl-4 text-[10px] text-hazard">
            <li
              v-for="(item, index) in failures"
              :key="index"
              class="truncate"
              :title="`${mask(item.label)}：${item.reason}`"
            >
              ✕ {{ mask(item.label) }} — {{ item.reason }}
            </li>
          </ul>
        </div>

        <TaskTable
          v-model="tab"
          :tasks="tasks"
          :loading="detailLoading"
          :error="detailError"
          :checkin="detail?.checkin ?? null"
          :has-account="!!activeAccount"
        />

        <ActionBar
          v-model="scope"
          :total="accounts.length"
          :selected-count="selected.size"
          :running="run.running"
          :cancelling="cancelling"
          :busy-action="busyAction"
          @run="runAction"
          @cancel="cancel"
        />

        <AutoDailyPanel />
      </main>
    </div>

    <LogPanel :logs="logs" :error-count="errorLogCount" @clear="clearLogs" />

    <StatusBar
      :total="accounts.length"
      :last-refresh="lastRefresh"
      :log-count="logs.length"
      :running="run.running"
    />
  </div>
</template>
