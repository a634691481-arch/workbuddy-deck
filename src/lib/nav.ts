/** 左侧导航分区（与 AppSidebar 的 items 一一对应）。 */
export type NavKey = "tasks" | "credits" | "tokens" | "accounts" | "logs" | "archived" | "settings";

// ── 导航图标（morphicons：idle ↔ active 之间做形变过渡）──────────────────
// 数据取自 lucide（IconNode，非组件），morphicons 直接吃这种结构。
import type { IconNode } from "morphicons/vue";
import {
  Archive,
  Boxes,
  ChartColumn,
  Coins,
  Key,
  KeyRound,
  ListChecks,
  ListTodo,
  ScrollText,
  Settings,
  Settings2,
  SquareTerminal,
  UserCheck,
  Users,
} from "lucide";

export type IconPair = { idle: IconNode; active: IconNode };

const pair = (idle: IconNode, active: IconNode): IconPair => ({ idle, active });

export const NAV_ICONS: Record<NavKey, IconPair> = {
  accounts: pair(Users, UserCheck),
  tasks: pair(ListTodo, ListChecks),
  credits: pair(Coins, ChartColumn),
  tokens: pair(Key, KeyRound),
  logs: pair(ScrollText, SquareTerminal),
  archived: pair(Archive, Boxes),
  settings: pair(Settings, Settings2),
};
