// 编排层：状态 / 副作用 / 轮询 / 动作。App.vue 只渲染；这里是唯一使用 `api` 的地方。
// 抽出理由见 AGENTS.md §2：模板与逻辑混在一个文件里无法单独演进。
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

import type { LogLine } from "@/components/LogPanel.vue";
import * as api from "@/lib/api";
import { isTauri } from "@/lib/tauri-env";
import { actionLabel, failuresOf, mask, pendingOf, type ActionKey } from "@/lib/task-meta";

/** 轮询间隔：只在运行中开启，结束立刻停表（AGENTS.md §6）。 */
const POLL_INTERVAL_MS = 400;
const SIDEBAR_MIN = 240;
const SIDEBAR_MAX = 380;
const SIDEBAR_KEY = "wb-ta/sidebar-width";
const ACTIVE_KEY = "wb-ta/active-account";
const LOG_LIMIT = 200;
const FAILURE_PREVIEW = 5;

function emptyRun(): api.RunStatus {
  return {
    running: false,
    cancelled: false,
    action: "",
    total: 0,
    done: 0,
    ok: 0,
    fail: 0,
    skip: 0,
    current: "",
  };
}

export function useAssistant() {
  const accounts = ref<api.AccountSummary[]>([]);
  const selected = ref<Set<string>>(new Set());
  // 上次选中的账号：重启后直接回到那个号，不用每次从 01 号开始翻
  const activeId = ref<string | null>(localStorage.getItem(ACTIVE_KEY));
  // 批量执行计时：用来估剩余时间（只在运行中显示，结束清空）
  let runStartedAt = 0;
  const etaText = computed(() => {
    const { running, done, total } = run.value;
    if (!running || !runStartedAt || done <= 0 || total <= done) return "";
    const elapsed = Date.now() - runStartedAt;
    const left = Math.round(((elapsed / done) * (total - done)) / 1000);
    if (left <= 0) return "";
    return left >= 60 ? `≈${Math.floor(left / 60)}m${String(left % 60).padStart(2, "0")}s` : `≈${left}s`;
  });
  const detail = ref<api.AccountDetail | null>(null);
  const detailLoading = ref(false);
  const detailError = ref("");
  const logs = ref<LogLine[]>([]);
  const scope = ref<"all" | "selected">("all");
  const lastRefresh = ref("—");
  const tab = ref<"tasks" | "checkin" | "raw">("tasks");
  const sidebarWidth = ref(clampWidth(Number(localStorage.getItem(SIDEBAR_KEY)) || 300));
  const failures = ref<{ accountId: string; label: string; reason: string }[]>([]);
  const pendingRows = ref<ReturnType<typeof pendingOf>>([]);
  const showFailures = ref(false);
  const cancelling = ref(false);
  const busyAction = ref<ActionKey | "single" | "refresh" | "">("");
  const run = ref<api.RunStatus>(emptyRun());

  let pollTimer: number | undefined;

  const busy = computed(() => busyAction.value !== "");
  const activeAccount = computed(
    () => accounts.value.find((item) => item.id === activeId.value) ?? null,
  );
  const targetIds = computed(() => (scope.value === "all" ? [] : [...selected.value]));
  const runLabel = computed(() => actionLabel(run.value.action));

  function clampWidth(width: number) {
    return Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, width));
  }

  function pushLog(text: string, level: LogLine["level"] = "info") {
    logs.value.unshift({ ts: Date.now(), text: mask(text), level });
    if (logs.value.length > LOG_LIMIT) logs.value.length = LOG_LIMIT;
  }

  function stamp() {
    const now = new Date();
    const pad = (n: number) => String(n).padStart(2, "0");
    return `${pad(now.getHours())}:${pad(now.getMinutes())}`;
  }

  function stopPolling() {
    if (pollTimer !== undefined) {
      window.clearInterval(pollTimer);
      pollTimer = undefined;
    }
  }

  function startPolling() {
    stopPolling();
    pollTimer = window.setInterval(async () => {
      try {
        run.value = await api.runStatus();
        if (!run.value.running) stopPolling();
      } catch {
        // 轮询失败不打断执行本身，下一次再试。
        stopPolling();
      }
    }, POLL_INTERVAL_MS);
  }

  async function selectAccount(id: string) {
    // 切到不同账号时先清掉旧详情，加载期间不残留上个账号的数据
    if (activeId.value !== id) detail.value = null;
    activeId.value = id;
    localStorage.setItem(ACTIVE_KEY, id);
    detailLoading.value = true;
    detailError.value = "";
    try {
      detail.value = await api.accountDetail(id);
    } catch (error) {
      detail.value = null;
      detailError.value = `读取失败：${String(error)}`;
      const label = mask(accounts.value.find((item) => item.id === id)?.label ?? id);
      pushLog(`读取 ${label} 详情失败：${String(error)}`, "err");
    } finally {
      detailLoading.value = false;
    }
  }

  async function refreshAll() {
    if (busy.value) return;
    busyAction.value = "refresh";
    try {
      const res = await api.listAccounts();
      accounts.value = res.accounts;
      lastRefresh.value = stamp();
      pushLog(`已刷新账号列表（${res.total} 个）`);
      if (
        res.accounts.length > 0 &&
        (!activeId.value || !res.accounts.some((item) => item.id === activeId.value))
      ) {
        await selectAccount(res.accounts[0].id);
      } else if (activeId.value) {
        await selectAccount(activeId.value);
      }
    } catch (error) {
      pushLog(`刷新失败：${String(error)}`, "err");
    } finally {
      busyAction.value = "";
    }
  }

  /** 执行后只刷新受影响的数据，不做全量重扫。 */
  async function refreshQuietly() {
    try {
      const res = await api.listAccounts();
      accounts.value = res.accounts;
      if (activeId.value) await selectAccount(activeId.value);
      lastRefresh.value = stamp();
    } catch {
      /* 刷新失败不覆盖已有结果 */
    }
  }

  function toggleSelect(id: string) {
    const next = new Set(selected.value);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    selected.value = next;
  }

  function selectAll() {
    selected.value = new Set(accounts.value.map((item) => item.id));
  }

  function invert() {
    const next = new Set<string>();
    for (const item of accounts.value) {
      if (!selected.value.has(item.id)) next.add(item.id);
    }
    selected.value = next;
  }

  function clearSelection() {
    selected.value = new Set();
    pushLog("已清空勾选（不删除账号）");
  }
  function resizeSidebar(width: number) {
    sidebarWidth.value = clampWidth(width);
    localStorage.setItem(SIDEBAR_KEY, String(sidebarWidth.value));
  }

  async function runAction(action: ActionKey, onlyIds?: string[]) {
    if (busy.value) return;
    // 失败重试带明确 id，不受当前作用域影响
    if (!onlyIds && scope.value === "selected" && selected.value.size === 0) return;

    const label = actionLabel(action);
    busyAction.value = action;
    showFailures.value = false;
    failures.value = [];
    const ids = onlyIds ?? targetIds.value;
    runStartedAt = Date.now();
    pushLog(`开始执行「${label}」，共 ${ids.length || accounts.value.length} 个账号`);

    try {
      startPolling();
      // 动作 → 命令一一对应，不再有 else 兜底（按钮表只有三个动作）。
      const runner = {
        checkin: api.runCheckin,
        travel: api.runTravel,
        growth: api.runGrowth,
      } as const;
      const result = await runner[action](ids);

      run.value = await api.runStatus().catch(() => run.value);
      if (!run.value.running) runStartedAt = 0;
      failures.value = failuresOf(result.accounts ?? []);
      pendingRows.value = pendingOf(result.accounts ?? []);

      const verdict =
        result.status === "cancelled"
          ? "已取消"
          : result.status === "busy"
            ? "已有任务在执行"
            : "完成";
      pushLog(
        `「${label}」${verdict}：成功 ${run.value.ok}，失败 ${run.value.fail}，跳过 ${run.value.skip}`,
        run.value.fail > 0 ? "err" : "ok",
      );
      for (const failure of failures.value.slice(0, FAILURE_PREVIEW)) {
        pushLog(`${failure.label} 失败：${failure.reason}`, "err");
      }
      void notifyResult(
        `WorkBuddy Deck · ${label}`,
        `${verdict}：成功 ${run.value.ok}，失败 ${run.value.fail}，跳过 ${run.value.skip}`,
      );
      await refreshQuietly();
    } catch (error) {
      pushLog(`「${label}」执行异常：${String(error)}`, "err");
    } finally {
      stopPolling();
      busyAction.value = "";
      cancelling.value = false;
    }
  }

  /** 打开任务官方入口：workbuddy:// 走专用命令（后端只放行该协议），http(s) 走浏览器。 */
  async function openTaskJump(url: string) {
    if (!url) return;
    try {
      const result = url.startsWith("workbuddy://")
        ? await api.openWorkbuddyDeeplink(url)
        : await api.openExternal(url);
      if (!result.ok) pushLog(`打开入口失败：${result.error ?? "未知原因"}`, "err");
    } catch (error) {
      pushLog(`打开入口失败：${String(error)}`, "err");
    }
  }

  /**
   * 「去完成」：打开任务官方入口（客户端深链 / 官网），随后轮询刷新该账号详情，
   * 用户在客户端点完，这里能第一时间看到状态变 completed，再点「执行」即可自动领奖。
   *
   * 为什么不直接替用户打点：Library_read / Buddy_App 的完成判定来自客户端真实交互
   * （多账号实测伪造事件 0/1），服务端不认，硬发只是浪费额度。
   */
  async function goComplete(url: string) {
    if (!url) return;
    await openTaskJump(url);
    pushLog("已打开任务入口，完成后回到这里点「执行」即可领奖");
    if (!activeId.value) return;
    let ticks = 0;
    const timer = window.setInterval(async () => {
      ticks += 1;
      await selectAccount(activeId.value as string);
      if (ticks >= 8) window.clearInterval(timer);
    }, 15_000);
  }

  /**
   * 批量跑完给一条系统通知：窗口不在前台时也能知道结果。
   * 前台时静默（界面已经显示结果），通知失败不影响主流程。
   */
  async function notifyResult(title: string, body: string) {
    if (!isTauri() || document.hasFocus()) return;
    try {
      let granted = await isPermissionGranted();
      if (!granted) granted = (await requestPermission()) === "granted";
      if (granted) sendNotification({ title, body });
    } catch {
      /* 通知不是关键路径 */
    }
  }

  async function cancel() {
    if (!run.value.running) return;
    cancelling.value = true;
    pushLog("已请求取消：当前账号完成这一步后停止");
    try {
      await api.cancelRun();
    } catch (error) {
      cancelling.value = false;
      pushLog(`取消失败：${String(error)}`, "err");
    }
  }

  async function signinOnce() {
    if (!activeId.value || busy.value) return;
    busyAction.value = "single";
    try {
      const row = await api.checkinNow(activeId.value);
      const state = row.result ?? "error";
      pushLog(
        `${mask(activeAccount.value?.label ?? "账号")} 签到：${state}`,
        state === "error" ? "err" : "ok",
      );
      await selectAccount(activeId.value);
    } catch (error) {
      pushLog(`签到失败：${String(error)}`, "err");
    } finally {
      busyAction.value = "";
    }
  }

  async function loginAccount() {
    try {
      const res = await api.launchMainApp();
      pushLog(
        res.started
          ? "已启动主程序，请在主程序中扫码添加账号"
          : `未启动主程序：${res.error ?? "未知原因"}`,
        res.started ? "info" : "err",
      );
    } catch (error) {
      pushLog(`启动主程序失败：${String(error)}`, "err");
    }
  }

  const clearLogs = () => {
    logs.value = [];
  };

  onMounted(refreshAll);
  onBeforeUnmount(stopPolling);

  return {
    accounts,
    etaText,
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
    pendingRows,
    openTaskJump,
    goComplete,
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
    loginAccount,
    clearLogs,
    pushLog,
  };
}
