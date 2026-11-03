<script setup lang="ts">
/**
 * 外壳：标题栏 + 左侧导航 + 分区页面（KeepAlive 保活，切页不丢运行状态与日志）。
 * 页面自闭环：每个 view 自己取数、自己处理动作，外壳不掺业务。
 * 分区顺序以「账号管理」打头：先有账号，才谈得上任务与统计。
 */
import { computed, ref } from "vue";

import AppSidebar from "@/components/AppSidebar.vue";
import BootOverlay from "@/components/BootOverlay.vue";
import TitleBar from "@/components/TitleBar.vue";
import AccountsView from "@/views/AccountsView.vue";
import ArchivedView from "@/views/ArchivedView.vue";
import CreditsView from "@/views/CreditsView.vue";
import LogsView from "@/views/LogsView.vue";
import SettingsView from "@/views/SettingsView.vue";
import TasksView from "@/views/TasksView.vue";
import TokensView from "@/views/TokensView.vue";
import { useShell } from "@/composables/useShell";
import type { NavKey } from "@/lib/nav";

const { accountCount, running, nav } = useShell();

// hash 直达分区（浏览器预览与 WB_START_NAV 深链共用）：#tasks / #credits / #tokens / #accounts / #logs / #archived / #settings
const NAV_KEYS: NavKey[] = ["accounts", "tasks", "credits", "tokens", "logs", "archived", "settings"];
function navFromHash(): NavKey | null {
  const key = window.location.hash.replace("#", "") as NavKey;
  return NAV_KEYS.includes(key) ? key : null;
}
{
  const fromHash = navFromHash();
  if (fromHash) nav.value = fromHash;
  window.addEventListener("hashchange", () => {
    const key = navFromHash();
    if (key) nav.value = key;
  });
}

const views = {
  accounts: AccountsView,
  tasks: TasksView,
  credits: CreditsView,
  tokens: TokensView,
  logs: LogsView,
  archived: ArchivedView,
  settings: SettingsView,
} as const;

const activeView = computed(() => views[nav.value]);

// 启动动画覆盖在主界面之上：窗口显示时遮罩已绘好，不会先白后黑。
const booting = ref(true);
</script>

<template>
  <div class="crt flex h-full min-h-0 flex-col bg-background">
    <TitleBar />

    <div class="flex min-h-0 flex-1">
      <AppSidebar
        :current="nav"
        :account-count="accountCount"
        :running="running"
        @select="nav = $event"
      />

      <!-- 注释必须在 KeepAlive 外：里面只允许一个子组件，注释会被当成额外节点 -->
      <!-- min-w-0：分区视图可收缩，内部宽内容不允许把窗口撑出横向溢出 -->
      <KeepAlive>
        <component :is="activeView" class="wb-view min-w-0 flex-1" />
      </KeepAlive>
    </div>

    <BootOverlay v-if="booting" @done="booting = false" />
  </div>
</template>
