<script setup lang="ts">
/**
 * 标题栏：品牌 + 窗口控制 + 底部加载条。
 *
 * 外观 / 脱敏 / 语言切换已搬到「系统设置」页（2026-11-20）：
 * 标题栏只留壳，任何配置项都不在这里。
 */
import { computed, onMounted } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X } from "lucide-vue-next";

import { useUpdate } from "@/composables/useUpdate";
import { setWindowBackdrop } from "@/lib/api";
import { t } from "@/lib/i18n";
import { loadingBar } from "@/lib/loading-bar";
import { backdrop } from "@/lib/prefs";
import { hasHostWindow, isEmbedded } from "@/lib/tauri-env";

// 嵌入面板里没有真实窗口：win=null 时隐藏三个窗口按钮。
const win = hasHostWindow() ? getCurrentWindow() : null;
const embedded = isEmbedded();
const { loadConfig } = useUpdate();

/** 有 IPC 在飞（或刚结束的收尾窗口）就显示滑动态。 */
const busy = computed(() => loadingBar.inflight.value > 0 || loadingBar.tail.value);

onMounted(() => {
  void loadConfig();
  // 毛玻璃是原生窗口特效，不随 localStorage 恢复，启动时重放一次。
  if (hasHostWindow() && backdrop.value === "on") void setWindowBackdrop("acrylic");
});
</script>

<template>
  <!-- 标题栏：底部 2px 即加载条（空闲满格 / 加载中滑动） -->
  <header
    data-tauri-drag-region
    class="relative flex shrink-0 select-none items-center justify-between bg-titlebar"
    :class="embedded ? 'h-8 pl-2' : 'h-9 pl-3'"
  >
    <div data-tauri-drag-region class="flex min-w-0 items-baseline gap-2.5">
      <span class="text-[13.5px] font-semibold text-foreground">
        {{ embedded ? t("deck.title") : "WorkBuddy" }}
      </span>
      <span v-if="!embedded" class="text-[11px] text-faint">{{ t("app.subtitle") }}</span>
    </div>

    <div v-if="win" class="flex items-stretch self-stretch">
      <button
        class="grid w-11 place-items-center border-l border-border text-muted-foreground hover:bg-muted hover:text-foreground"
        title="最小化"
        @click="win?.minimize()"
      >
        <Minus class="size-3.5" />
      </button>
      <button
        class="grid w-11 place-items-center border-l border-border text-muted-foreground hover:bg-muted hover:text-foreground"
        title="最大化"
        @click="win?.toggleMaximize()"
      >
        <Square class="size-3" />
      </button>
      <button
        class="grid w-11 place-items-center border-l border-border text-muted-foreground hover:bg-hazard hover:text-white"
        title="关闭（最小化到托盘）"
        @click="win?.close()"
      >
        <X class="size-4" />
      </button>
    </div>

    <!-- 加载条：占位就是那 2px 强调线。空闲 = 满格；有在飞请求 = 滑块巡游 -->
    <div class="pointer-events-none absolute inset-x-0 bottom-0 h-0.5 overflow-hidden bg-hazard/25">
      <div v-if="busy" class="bar-sweep h-full w-1/4 bg-hazard" />
      <div v-else class="h-full w-full bg-hazard" />
    </div>
  </header>
</template>

<style scoped>
/* 纯色块平移，无渐变无圆角；速度固定，避免看起来像在"假装"有进度 */
.bar-sweep {
  animation: bar-sweep 1.15s linear infinite;
}
@keyframes bar-sweep {
  from {
    transform: translateX(-100%);
  }
  to {
    transform: translateX(400%);
  }
}
@media (prefers-reduced-motion: reduce) {
  .bar-sweep {
    animation: none;
    width: 100%;
    opacity: 0.45;
  }
}
</style>
