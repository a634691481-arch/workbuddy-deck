<script setup lang="ts">
/**
 * 外壳：标题栏 + 左侧导航 + 分区页面（KeepAlive 保活，切页不丢运行状态与日志）。
 * 页面自闭环：每个 view 自己取数、自己处理动作，外壳不掺业务。
 * 分区顺序以「账号管理」打头：先有账号，才谈得上任务与统计。
 */
import { computed } from "vue";

import AppSidebar from "@/components/AppSidebar.vue";
import TitleBar from "@/components/TitleBar.vue";
import AccountsView from "@/views/AccountsView.vue";
import CreditsView from "@/views/CreditsView.vue";
import LogsView from "@/views/LogsView.vue";
import SettingsView from "@/views/SettingsView.vue";
import TasksView from "@/views/TasksView.vue";
import TokensView from "@/views/TokensView.vue";
import { useShell } from "@/composables/useShell";
import type { NavKey } from "@/lib/nav";

const { accountCount, running, nav } = useShell();

// hash 直达分区（浏览器预览调试用）：#tasks / #credits / #tokens / #accounts / #logs / #settings
const NAV_KEYS: NavKey[] = ["accounts", "tasks", "credits", "tokens", "logs", "settings"];
{
  const fromHash = window.location.hash.replace("#", "") as NavKey;
  if (NAV_KEYS.includes(fromHash)) nav.value = fromHash;
}

const views = {
  accounts: AccountsView,
  tasks: TasksView,
  credits: CreditsView,
  tokens: TokensView,
  logs: LogsView,
  settings: SettingsView,
} as const;

const activeView = computed(() => views[nav.value]);
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

      <KeepAlive>
        <component :is="activeView" />
      </KeepAlive>
    </div>
  </div>
</template>
