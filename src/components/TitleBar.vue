<script setup lang="ts">
// 窗口控制依赖 Tauri 宿主；浏览器预览时 win 为 null，按钮照常渲染但不生效（不白屏）。
import { computed, onMounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Eye, EyeOff, Minus, Moon, Square, Sun, X } from "lucide-vue-next";

import { locale, setLocale, t } from "@/lib/i18n";
import { loadingBar } from "@/lib/loading-bar";
import { isTauri } from "@/lib/tauri-env";
import { maskAll, setMaskAll } from "@/lib/task-meta";
import {
  ACCENTS,
  RADIUS_MAX,
  accent,
  radius,
  setAccent,
  setRadius,
  setTheme,
  theme,
} from "@/lib/prefs";
import { useShell } from "@/composables/useShell";
import { useUpdate } from "@/composables/useUpdate";

const win = isTauri() ? getCurrentWindow() : null;
const { go } = useShell();
const { current: rev, hasUpdate, loadConfig } = useUpdate();

/** 有 IPC 在飞（或刚结束的收尾窗口）就显示滑动态。 */
const busy = computed(() => loadingBar.inflight.value > 0 || loadingBar.tail.value);

// 标题栏弹出面板：圆角滑块 / 主题色格子；点空白处关闭
const panel = ref<"" | "radius" | "accent">("");
function togglePanel(name: "radius" | "accent") {
  panel.value = panel.value === name ? "" : name;
}

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
      <!-- 主题开关：暗色显示太阳（点了变白天），白天显示月亮 -->
      <button
        class="flex items-center gap-1 border-l border-border px-2 text-[10px] uppercase tracking-[0.1em]"
        :class="theme === 'light' ? 'text-hazard' : 'text-faint hover:text-foreground'"
        :title="theme === 'light' ? t('tb.toDark') : t('tb.toLight')"
        @click="setTheme(theme === 'light' ? 'dark' : 'light')"
      >
        <Sun v-if="theme === 'dark'" class="size-3" />
        <Moon v-else class="size-3" />
        {{ t("tb.theme") }}
      </button>

      <!-- 圆角：点击弹出滑块，0 = 直角，实时生效并记忆 -->
      <button
        class="flex items-center border-l border-border px-2 text-[10px] uppercase tracking-[0.1em]"
        :class="radius > 0 ? 'text-hazard' : 'text-faint hover:text-foreground'"
        :title="t('tb.roundSize')"
        @click="togglePanel('radius')"
      >
        {{ t("tb.round") }}
      </button>

      <!-- 主题色：色块 + 格子弹窗 -->
      <button
        class="flex items-center gap-1.5 border-l border-border px-2 text-[10px] uppercase tracking-[0.1em]"
        :class="accent === ACCENTS[0].value ? 'text-faint hover:text-foreground' : 'text-hazard'"
        :title="t('tb.colorTitle')"
        @click="togglePanel('accent')"
      >
        <i class="size-2.5 border border-border-strong" :style="{ background: accent }" />
        {{ t("tb.color") }}
      </button>

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

    <!-- 弹出面板：透明背板点击关闭 -->
    <div v-if="panel" class="fixed inset-0 z-40" @click="panel = ''" />
    <div
      v-if="panel === 'radius'"
      class="absolute right-24 top-full z-50 w-56 border border-border-strong bg-card p-3"
    >
      <div class="lbl mb-2 flex items-center justify-between">
        <span>{{ t("tb.roundSize") }}</span>
        <span class="readout text-[11px]" :class="radius > 0 ? 'text-hazard' : 'text-faint'">
          {{ radius > 0 ? `${radius}px` : t("tb.roundZero") }}
        </span>
      </div>
      <input
        :value="radius"
        type="range"
        min="0"
        :max="RADIUS_MAX"
        step="1"
        class="w-full accent-hazard"
        @input="setRadius(($event.target as HTMLInputElement).valueAsNumber)"
      />
      <div class="mt-1 flex justify-between text-[9px] text-faint">
        <span>{{ t("tb.roundZero") }}</span>
        <span>{{ RADIUS_MAX }}px</span>
      </div>
    </div>
    <div
      v-if="panel === 'accent'"
      class="absolute right-16 top-full z-50 w-48 border border-border-strong bg-card p-3"
    >
      <div class="lbl mb-2">{{ t("tb.colorTitle") }}</div>
      <div class="grid grid-cols-5 gap-1.5">
        <button
          v-for="c in ACCENTS"
          :key="c.value"
          type="button"
          class="relative grid size-7 place-items-center border transition-colors"
          :class="accent === c.value ? 'border-foreground' : 'border-transparent hover:border-border-strong'"
          :title="locale === 'zh' ? c.zh : c.en"
          @click="setAccent(c.value)"
        >
          <i class="size-4" :style="{ background: c.value }" />
          <i v-if="accent === c.value" class="absolute size-1.5 bg-background" />
        </button>
      </div>
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
