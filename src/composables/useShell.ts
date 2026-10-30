import { ref } from "vue";

import type { NavKey } from "@/lib/nav";

/**
 * 外壳共享状态：侧栏要显示「账号数 / 是否在跑」，但数据由各页面持有。
 * 这里只放两个只读结果，写入方是 TasksView（任务状态）与 AccountsView（账号数）。
 * `nav` 也放这里：跨页引导（如任务页空态跳去账号管理）需要切换分区。
 */
const accountCount = ref(0);
const running = ref(false);
const nav = ref<NavKey>("accounts");

export function useShell() {
  return {
    accountCount,
    running,
    nav,
    setAccountCount: (value: number) => {
      accountCount.value = value;
    },
    setRunning: (value: boolean) => {
      running.value = value;
    },
    go: (key: NavKey) => {
      nav.value = key;
    },
  };
}
