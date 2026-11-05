import type { GrowthTask, RunRow } from "@/lib/api";
import { t, type I18nKey } from "@/lib/i18n";
import { ref } from "vue";

/** 批量动作的展示名与后端动作名的唯一映射（按钮表由它驱动）。
 *  三个动作 = 三件彼此独立的事；「全部日常」不在按钮里（托盘菜单仍保留该入口）。 */
export const ACTIONS = [
  { key: "checkin", labelKey: "act.checkin", variant: "default" },
  { key: "travel", labelKey: "act.travel", variant: "outline" },
  { key: "growth", labelKey: "act.growth", hintKey: "act.growthHint", variant: "outline" },
] as const;

export type ActionKey = (typeof ACTIONS)[number]["key"];

export function actionLabel(key: string): string {
  const item = ACTIONS.find((entry) => entry.key === key);
  return item ? t(item.labelKey as I18nKey) : key;
}

export type TaskTone = "success" | "info" | "muted" | "warning";

/** 任务阶段 → 文案 + 色。`completed` 与 `claimed` 必须区分（前者做完未领，后者已领）。 */
export function statusOf(task: GrowthTask): { text: string; tone: TaskTone } {
  switch (task.accept_status) {
    case "claimed":
      return { text: t("st.claimed"), tone: "success" };
    case "completed":
      return { text: t("st.completed"), tone: "info" };
    case "accepted":
      return { text: t("st.accepted"), tone: "warning" };
    case "not_accepted":
      return { text: t("st.notAccepted"), tone: "muted" };
    default:
      return { text: task.accept_status || t("st.unknown"), tone: "muted" };
  }
}

/** 进度展示：有目标用 `cur/tgt`，无目标只给当前值。 */
export function progressOf(task: GrowthTask): string {
  const cur = task.progress?.current ?? 0;
  const tgt = task.progress?.target ?? 0;
  return tgt > 0 ? `${cur}/${tgt}` : String(cur);
}

export function isDone(task: GrowthTask): boolean {
  return task.accept_status === "claimed" || task.accept_status === "completed";
}

/** 失败聚合：把一批结果行里失败的挑出来，供顶部横幅展开。 */
export function failuresOf(
  rows: RunRow[],
): { accountId: string; label: string; reason: string }[] {
  return rows
    .filter((row) => row.result === "error" || row.result === "fail")
    .map((row) => ({
      // 带上 id：失败聚合条要能「只重试这几个号」，没有 id 就只能整体重跑
      accountId: row.accountId ?? "",
      label: row.label ?? row.accountId ?? "未知账号",
      reason: row.error ?? row.reason ?? "未返回原因",
    }));
}

/** 「没做」明细：只挑有缺口却没推进的任务，按账号分组。 */
export function pendingOf(
  rows: RunRow[],
): {
  label: string;
  items: { code: string; reason: string; title: string; jumpUrl: string }[];
}[] {
  return rows
    .map((row) => ({
      label: row.label ?? row.accountId ?? "未知账号",
      items: (row.tasks ?? [])
        .filter((task) => task.skipped)
        .map((task) => ({
          code: task.code,
          reason: skipReason(task.skipped ?? ""),
          title: task.title || task.code,
          jumpUrl: task.jumpUrl || "",
        })),
    }))
    .filter((entry) => entry.items.length > 0);
}

/** 跳过原因码 → 人话（后端只给码，不返回中文）。 */
export function skipReason(code: string): string {
  switch (code) {
    case "manual_skip":
      return t("tm.skipManual");
    case "tries_exhausted":
      return t("tm.skipTries");
    case "no_recipe":
      return t("tm.skipNoRecipe");
    case "manual_required":
      return t("tm.skipManualTask");
    default:
      return code;
  }
}

/** 全局名称脱敏开关：关 = 所有显示点直接回原文，开 = 按规则打码。
 *  mask() 在模板里被调用，读这个 ref 即自动跟随切换重渲染。 */
const MASK_KEY = "wb-ta/mask-names";

export const maskAll = ref(localStorage.getItem(MASK_KEY) !== "off");

export function setMaskAll(on: boolean) {
  maskAll.value = on;
  localStorage.setItem(MASK_KEY, on ? "on" : "off");
}

/** 账号标识脱敏（日志与提示统一走这里，AGENTS.md §5）。
 *  邮箱 / 手机号按各自规则；其余昵称、中文名等一律保留首字符打码。 */
export function mask(text: string): string {
  if (!text) return text;
  if (!maskAll.value) return text;
  const phone = text.match(/^(\d{3})\d{4}(\d{4})$/);
  if (phone) return `${phone[1]}****${phone[2]}`;
  const email = text.match(/^(.{2}).*(@.*)$/);
  if (email) return `${email[1]}***${email[2]}`;
  const chars = Array.from(text);
  // 只留首字会让 4 个账号全变成「1***」——留尾巴才能区分（手机号已单独处理）。
  if (chars.length <= 4) return `${chars[0]}***`;
  return `${chars[0]}***${chars.slice(-2).join("")}`;
}
