/**
 * 异常提醒中心：标题栏铃铛的数据与「新提醒 → 系统通知」链路。
 * - 数据源：后端每 10 分钟评估一次（账号缓存 + 切换历史 + 错误通知），失焦才弹系统通知。
 * - 忽略是 7 天静默：同一指纹 7 天内不再骚扰；面板里可单条 / 全部忽略。
 */
import { computed, onMounted, ref } from "vue";

import {
  alertsDismiss,
  alertsDismissAll,
  alertsList,
  type AlertItem,
} from "@/lib/api";
import { isTauri } from "@/lib/tauri-env";
import { t } from "@/lib/i18n";
import { mask } from "@/lib/task-meta";

const alerts = ref<AlertItem[]>([]);
let unlisten: (() => void) | null = null;

const criticalCount = computed(
  () => alerts.value.filter((item) => item.level === "critical").length,
);

async function load() {
  const result = await alertsList();
  if (!result.ok) return;
  alerts.value = result.alerts ?? [];
}

async function dismiss(fingerprint: string) {
  await alertsDismiss(fingerprint);
  await load();
}

async function dismissAll() {
  await alertsDismissAll();
  await load();
}

/** 提醒文案：按 kind 组装（原因码走 ac.hz* 翻译，与账号卡片同一套） */
function alertText(item: AlertItem): string {
  const label = mask(item.accountLabel || item.accountId);
  switch (item.kind) {
    case "balance":
      return t("alr.balance", { label, days: item.data.daysLeft.toFixed(1) });
    case "health":
      return t("alr.health", { label, score: String(item.data.score) });
    case "expiring": {
      const days = Math.max(0, Math.ceil((item.data.expireAt - Date.now()) / 86_400_000));
      return t("alr.expiring", {
        label,
        pkg: item.data.packageName ?? "",
        days: String(days),
      });
    }
    case "switch":
      return t("alr.switch", { label });
    case "runfail":
      return t("alr.runfail", { count: String(item.data.count) });
  }
}

export function useAlerts() {
  onMounted(async () => {
    if (!isTauri()) return;
    void load().catch(() => {});
    if (unlisten) return;
    const { listen } = await import("@tauri-apps/api/event");
    const off = await listen<{ alerts: AlertItem[] }>("alerts://new", async (event) => {
      await load().catch(() => {});
      // 前台不弹：面板角标已经可见；失焦才发系统通知
      if (document.hasFocus()) return;
      try {
        const { isPermissionGranted, requestPermission, sendNotification } = await import(
          "@tauri-apps/plugin-notification"
        );
        let granted = await isPermissionGranted();
        if (!granted) granted = (await requestPermission()) === "granted";
        if (!granted) return;
        for (const item of event.payload.alerts.slice(0, 3)) {
          sendNotification({
            title: item.level === "critical" ? t("alr.notifyCritical") : t("alr.notifyWarn"),
            body: alertText(item),
          });
        }
      } catch {
        /* 通知失败不影响数据刷新 */
      }
    });
    unlisten = off;
  });
  return { alerts, criticalCount, load, dismiss, dismissAll, alertText };
}
