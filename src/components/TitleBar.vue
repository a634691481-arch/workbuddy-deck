<script setup lang="ts">
// 窗口控制依赖 Tauri 宿主；浏览器预览时 win 为 null，按钮照常渲染但不生效（不白屏）。
import { computed, onMounted } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Eye, EyeOff, Minus, Square, X } from "lucide-vue-next";

import { locale, setLocale, t } from "@/lib/i18n";
import { loadingBar } from "@/lib/loading-bar";
import { isTauri } from "@/lib/tauri-env";
import { maskAll, setMaskAll } from "@/lib/task-meta";
import { useShell } from "@/composables/useShell";
import { useUpdate } from "@/composables/useUpdate";

const win = isTauri() ? getCurrentWindow() : null;
const { go } = useShell();
const { current: rev, hasUpdate, loadConfig } = useUpdate();

/** 有 IPC 在飞（或刚结束的收尾窗口）就显示滑动态。 */
const busy = computed(() => loadingBar.inflight.value > 0 || loadingBar.tail.value);

// 标题栏先拿到本机版本号（设置页没打开过也要显示对），不额外发检查请求。
onMounted(() => void loadConfig());
</script>

<template>
  <!-- 标题栏：纯黑底，底部 2px 即为加载条（空闲满格 / 加载中滑动） -->
  <header
    data-tauri-drag-region
    class="relative flex h-9 shrink-0 select-none items-center justify-between bg-titlebar pl-3"
  >
    <div data-tauri-drag-region class="flex min-w-0 items-baseline gap-3">
      <span class="display text-[15px] text-foreground">WorkBuddy</span>
      <span class="lbl tracking-[0.18em] text-muted-foreground">{{ t("app.subtitle") }}</span>
      <span class="lbl text-faint">REV {{ rev || "0.1.0" }}</span>
      <!-- 有新版本：一个红方块，点进设置页（唯一能安装更新的地方） -->
      <button
        v-if="hasUpdate"
        class="flex items-center gap-1 border border-hazard bg-hazard-soft px-1 text-[9px] uppercase tracking-[0.1em] text-hazard"
        :title="t('up.newBadge')"
        @click="go('settings')"
      >
        {{ t("up.newBadge") }}
      </button>
    </div>

    <div class="flex items-stretch self-stretch">
      <!-- 名称脱敏开关：开 = 眼睛闭合（已隐藏），点一下切明文 -->
      <button
        class="flex items-center gap-1 border-l border-border px-2 text-[10px] uppercase tracking-[0.1em]"
        :class="maskAll ? 'text-hazard' : 'text-faint hover:text-foreground'"
        :title="maskAll ? t('tb.maskOn') : t('tb.maskOff')"
        @click="setMaskAll(!maskAll)"
      >
        <EyeOff v-if="maskAll" class="size-3" />
        <Eye v-else class="size-3" />
        {{ t("tb.mask") }}
      </button>

      <!-- 语言切换：两段式，当前语言反白 -->
      <div class="flex items-center gap-0 border-l border-border px-1">
        <button
          class="px-2 py-0.5 text-[10px] uppercase tracking-[0.1em]"
          :class="locale === 'zh' ? 'bg-foreground text-background' : 'text-faint hover:text-foreground'"
          @click="setLocale('zh')"
        >
          中
        </button>
        <button
          class="px-2 py-0.5 text-[10px] uppercase tracking-[0.1em]"
          :class="locale === 'en' ? 'bg-foreground text-background' : 'text-faint hover:text-foreground'"
          @click="setLocale('en')"
        >
          EN
        </button>
      </div>

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

    <!-- 加载条：占位就是原来那 2px 红线。空闲 = 满格；有在飞请求 = 滑块巡游 -->
    <div class="pointer-events-none absolute inset-x-0 bottom-0 h-0.5 overflow-hidden bg-hazard/25">
      <div
        v-if="busy"
        class="bar-sweep h-full w-1/4 bg-hazard"
      />
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
