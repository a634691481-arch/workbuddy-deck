<script setup lang="ts">
/**
 * 外壳：标题栏 + 左侧导航 + 分区页面（KeepAlive 保活，切页不丢运行状态与日志）。
 * 页面自闭环：每个 view 自己取数、自己处理动作，外壳不掺业务。
 * 分区顺序以「账号管理」打头：先有账号，才谈得上任务与统计。
 */
import { computed, onMounted, ref } from "vue";

import AppSidebar from "@/components/AppSidebar.vue";
import BootOverlay from "@/components/BootOverlay.vue";
import CommandPalette from "@/components/CommandPalette.vue";
import TitleBar from "@/components/TitleBar.vue";
import AccountsView from "@/views/AccountsView.vue";
import ArchivedView from "@/views/ArchivedView.vue";
import CreditsView from "@/views/CreditsView.vue";
import LogsView from "@/views/LogsView.vue";
import SettingsView from "@/views/SettingsView.vue";
import TasksView from "@/views/TasksView.vue";
import TokensView from "@/views/TokensView.vue";
import { useShell } from "@/composables/useShell";
import { autoSwitchCancel, importLocalAccounts } from "@/lib/api";
import { isTauri } from "@/lib/tauri-env";
import { t } from "@/lib/i18n";
import type { NavKey } from "@/lib/nav";

const { accountCount, running, nav } = useShell();

// Ctrl/Cmd+K 命令面板：只做分区跳转，不复制业务动作
const paletteOpen = ref(false);

/**
 * 限流自动切换的 3 秒倒计时：后端决定要换号时先发事件，这 3 秒里
 * 用户可以取消（正在打字时不该被直接关掉 WorkBuddy）。
 */
const switchPending = ref<{ targetId: string; left: number } | null>(null);
let pendingTimer: number | undefined;

async function cancelPendingSwitch() {
  if (pendingTimer) window.clearInterval(pendingTimer);
  switchPending.value = null;
  try {
    await autoSwitchCancel();
  } catch {
    /* 取消失败就让它继续，最多 3 秒后自己切 */
  }
}
function onKeydown(event: KeyboardEvent) {
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
    event.preventDefault();
    paletteOpen.value = !paletteOpen.value;
  }
}

/**
 * 启动补一次「本机 WorkBuddy 登录态 → 账号库」，两种机器都要兼容：
 * - 装过 workbuddy-switch：账号库已有这些账号，core 只在 uid 缺失时才写，等于空转；
 * - 没装过：账号库为空，这里把当前登录的账号补进去，否则任务面板永远「无账号」。
 * 静默且幂等，没有登录态就什么都不发生。
 */
onMounted(async () => {
  window.addEventListener("keydown", onKeydown);
  if (isTauri()) {
    const { listen } = await import("@tauri-apps/api/event");
    void listen<{ targetId: string; seconds: number }>("auto-switch-pending", (event) => {
      const seconds = event.payload.seconds || 3;
      switchPending.value = { targetId: event.payload.targetId, left: seconds };
      if (pendingTimer) window.clearInterval(pendingTimer);
      pendingTimer = window.setInterval(() => {
        if (!switchPending.value) return;
        switchPending.value.left -= 1;
        if (switchPending.value.left <= 0) {
          window.clearInterval(pendingTimer);
          switchPending.value = null;
        }
      }, 1000);
    });
  }
  if (!isTauri()) return;
  try {
    await importLocalAccounts();
  } catch {
    /* 纯补数据动作，失败不影响启动 */
  }
});

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
  <div class="crt flex h-full min-h-0 flex-col bg-background wb-flat">
    <TitleBar />

    <div class="flex min-h-0 flex-1 wb-flat">
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

    <!-- 限流自动切换倒计时：3 秒内可取消，避免正在打字时被直接关窗 -->
    <Transition name="wb-modal">
      <div
        v-if="switchPending"
        class="fixed bottom-6 right-6 z-[70] w-72 border border-hazard bg-card p-3"
        role="alertdialog"
      >
        <p class="text-[11px] text-foreground">{{ t("as.pendingTitle") }}</p>
        <p class="mt-1 break-all text-[10px] text-faint">{{ switchPending.targetId }}</p>
        <div class="mt-2 flex items-center justify-between gap-2">
          <span class="readout text-[11px] text-hazard">{{ switchPending.left }}s</span>
          <button
            type="button"
            class="border border-foreground bg-foreground px-2 py-0.5 text-[10px] uppercase tracking-[0.1em] text-background"
            @click="cancelPendingSwitch()"
          >
            {{ t("as.cancel") }}
          </button>
        </div>
      </div>
    </Transition>

    <CommandPalette :open="paletteOpen" @close="paletteOpen = false" @nav="nav = $event" />

    <BootOverlay v-if="booting" @done="booting = false" />
  </div>
</template>
