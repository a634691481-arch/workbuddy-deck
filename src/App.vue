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
import { autoSwitchCancel, autoSwitchConfirm, importLocalAccounts } from "@/lib/api";
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
type PendingSwitch = {
  targetId?: string;
  targetLabel?: string;
  reason?: string;
  seconds?: number;
  /** 后端给的绝对截止时刻（ms）；后台待确认时为空 */
  deadline?: number;
  /** 后台待确认：没有倒计时，等用户点确认 */
  awaitingConfirm?: boolean;
};
const switchPending = ref<PendingSwitch | null>(null);
/** 倒计时显示用（按 deadline 每 250ms 重算，不靠累加） */
const switchLeft = ref(0);
/** 倒计时结束后的真实切换阶段文案（完全同步要搬会话，可能几十秒） */
const switchStage = ref("");
let pendingTimer: number | undefined;

function stopTicker() {
  if (pendingTimer) window.clearInterval(pendingTimer);
  pendingTimer = undefined;
}

/** 取消本轮：倒计时中或后台待确认都算 */
async function cancelPendingSwitch() {
  stopTicker();
  switchPending.value = null;
  switchStage.value = "";
  try {
    await autoSwitchCancel();
  } catch {
    /* 取消失败就让它继续，最多一个倒计时后自己切 */
  }
}

/** 立即切换：跳过剩余倒计时 */
async function confirmPendingSwitch() {
  stopTicker();
  switchPending.value = null;
  try {
    await autoSwitchConfirm();
  } catch (cause) {
    switchStage.value = cause instanceof Error ? cause.message : String(cause);
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
    void listen<PendingSwitch>("auto-switch-pending", (event) => {
      switchPending.value = { ...event.payload };
      switchStage.value = "";
      stopTicker();
      if (event.payload.awaitingConfirm || !event.payload.deadline) {
        switchLeft.value = 0;
        return;
      }
      const deadline = event.payload.deadline;
      const tick = () => {
        if (!switchPending.value) return;
        switchLeft.value = Math.max(0, Math.ceil((deadline - Date.now()) / 1000));
        if (switchLeft.value <= 0) {
          // 倒计时走完：后端已经开始切，提示条切成进度态
          stopTicker();
          switchPending.value = null;
        }
      };
      tick();
      pendingTimer = window.setInterval(tick, 250);
    });
    // 后台只提醒：后端不动手，这里补一条系统通知，提示条停留等用户确认
    void listen<PendingSwitch>("auto-switch-notify", async (event) => {
      switchPending.value = { ...event.payload, awaitingConfirm: true };
      switchStage.value = "";
      stopTicker();
      try {
        const { isPermissionGranted, requestPermission, sendNotification } = await import(
          "@tauri-apps/plugin-notification"
        );
        let granted = await isPermissionGranted();
        if (!granted) granted = (await requestPermission()) === "granted";
        if (granted) {
          sendNotification({
            title: t("as.pendingTitle"),
            body: `${event.payload.targetLabel || event.payload.targetId || ""} · ${event.payload.reason || ""}`,
          });
        }
      } catch {
        /* 通知失败不影响提示条 */
      }
    });
    // 倒计时结束后进入真实切换：把后端阶段原样显示，别让窗口看起来「卡死」
    void listen<{ message: string }>("switch-progress", (event) => {
      switchStage.value = event.payload.message || "…";
    });
    void listen<{ action?: string }>("auto-switch", () => {
      switchStage.value = "";
      switchPending.value = null;
    });
    // 手动切换不走 `auto-switch` 事件；命令返回时统一发终态，避免「正在切换 / 切换完成」永久残留。
    void listen("switch-finished", () => {
      stopTicker();
      switchStage.value = "";
      switchPending.value = null;
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
        v-if="switchPending || switchStage"
        class="fixed bottom-6 right-6 z-[70] w-72 border border-hazard bg-card p-3"
        role="alertdialog"
      >
        <template v-if="switchPending">
          <p class="text-[11px] text-foreground">{{ t("as.pendingTitle") }}</p>
          <p class="mt-1 break-all text-[10px] text-foreground">
            <span class="lbl mr-1 text-faint">{{ t("as.pendingTarget") }}</span>
            {{ switchPending.targetLabel || switchPending.targetId }}
          </p>
          <p class="mt-0.5 break-all text-[10px] text-muted-foreground">
            <span class="lbl mr-1 text-faint">{{ t("as.pendingReason") }}</span>{{ switchPending.reason }}
          </p>
          <p v-if="switchPending.awaitingConfirm" class="mt-1 text-[9px] text-faint">
            {{ t("as.pendingBackground") }}
          </p>
          <div class="mt-2 flex items-center justify-between gap-2">
            <span v-if="!switchPending.awaitingConfirm" class="readout text-[11px] text-hazard">
              {{ switchLeft }}{{ t("as.seconds") }}
            </span>
            <span v-else class="readout text-[10px] text-faint">—</span>
            <span class="flex items-center gap-1.5">
              <button
                type="button"
                class="border border-foreground bg-foreground px-2 py-0.5 text-[10px] uppercase tracking-[0.1em] text-background"
                @click="confirmPendingSwitch()"
              >
                {{ t("as.switchNow") }}
              </button>
              <button
                type="button"
                class="border border-border-strong px-2 py-0.5 text-[10px] uppercase tracking-[0.1em] text-muted-foreground hover:border-foreground hover:text-foreground"
                @click="cancelPendingSwitch()"
              >
                {{ t("as.cancel") }}
              </button>
            </span>
          </div>
        </template>
        <template v-else>
          <p class="text-[11px] text-foreground">{{ t("ac.switching") }}</p>
          <p class="mt-1 break-all text-[10px] text-faint">{{ switchStage }}</p>
        </template>
      </div>
    </Transition>

    <CommandPalette :open="paletteOpen" @close="paletteOpen = false" @nav="nav = $event" />

    <BootOverlay v-if="booting" @done="booting = false" />
  </div>
</template>
